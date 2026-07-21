-- autorun_gameover_mode_writer.lua
--
-- Pin the retail GAME OVER transition: which store writes game_mode
-- (_DAT_8007B83C) = 0x12 (mode 18, the PROT 0902 gameover overlay)?
-- Static sweeps find NO `0x12` store anywhere on the disc
-- (docs/subsystems/battle.md "Party wipe + the game-over overlay");
-- the remaining space is the register-indirect game_mode stores, which
-- only a runtime write-watch can attribute.
--
-- WATCHES:
--   1. Write-watch (probe.step.find_writer, width-correct) over the
--      game_mode u16 at 0x8007B83C. Every store logs pc/ra/pre/now
--      plus battle-end context (signal byte DAT_8007BD71, wipe cause
--      _DAT_8007BD2C). The BP fires MID-store, so `pre` is the
--      pre-store value and `now` is read at the vsync drain.
--   2. Per-vsync polls of game_mode / DAT_8007BD71 / _DAT_8007BD2C /
--      party HP (`actor[+0x14C]` via the 0x801C9370 pointer table,
--      slots 0..2) - the mode/wipe timeline around each store.
--   3. On the first committed mode 0x12, banks a snapshot sstate and
--      full call context, then quits after LEGAIA_QUIT_AFTER frames.
--
-- FORCING THE WIPE (LEGAIA_KILL=1): while the battle main loop is live
-- (mode 0x15), party actors with HP > 1 are clamped DOWN to 1 (never
-- raised, never overwritten once 0) so the next enemy hit kills without
-- fighting the damage path. CROSS is edge-mashed (LEGAIA_MASH=1,
-- default) to advance menus so turns resolve headlessly.
--
-- REACHING A NON-SCRIPTED BATTLE (LEGAIA_WALK=1): from a walkable
-- field/worldmap state the probe rotates a held D-pad direction
-- (LEGAIA_WALK_SPIN vsyncs per leg, optional LEGAIA_WALK_PREFIX_BTN /
-- _FRAMES to fire a scene warp first) until the mode word enters the
-- battle chain, then switches to the in-battle mash+clamp behaviour.
-- The scripted-loss control run (queen bee) shows a wipe whose cause
-- byte IS 5 still exiting via the FUN_80046A20 mode-2 store - a
-- random-encounter wipe is the case that can differ.
--
-- Or skip the wander (LEGAIA_FORCE_ENCOUNTER=<monster_id>): at tick
-- LEGAIA_FORCE_AT the probe replays what FUN_801DA51C states 1/2 do on
-- a rolled encounter (docs/subsystems/world-map.md "Encounter-record
-- installation"): install the formation cell 0x8007BD0C..0F with the
-- monster id and store game_mode = 8. From a worldmap/field state this
-- enters a plain, non-scripted battle against that formation.
--
-- Lua BPs are DEAD under --fast; run the default -interpreter
-- -debugger tier, and ALWAYS wrap in `timeout` (PCSX-Redux probes do
-- not auto-quit on failure paths).
--
-- Launch (queen-bee auto-loss state; scripted-loss control run):
--   LEGAIA_SSTATE=saves/library/pcsx-redux/3d22fa5f...d3.sstate \
--   LEGAIA_FRAMES=7200 LEGAIA_KILL=0 \
--   timeout --kill-after=30s 1200s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gameover_mode_writer.lua
--
-- Output:
--   gameover_mode_writer.csv   tick,kind,pc,ra,pre,now,mode,bd71,bd2c,scene,count,note
--     kind = write  (mode-word store)
--          | mode | bd71 | bd2c | hp  (per-vsync change timeline)
--   gameover_mode_writer.detail.txt  register context for 0x12 stores
--   gameover_hit.sstate              snapshot at the first mode-0x12 commit

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe    = require("probe")
local mem      = require("probe.mem")
local pad      = require("probe.pad")
local sstate   = require("probe.sstate")
local snapshot = require("probe.snapshot")
local step     = require("probe.step")
local bit      = require("bit")

local GAME_MODE   = 0x8007B83C
local SCENE_NAME  = 0x8007050C
local BD71        = 0x8007BD71 -- battle-end signal (0xFE on wipe)
local BD2C        = 0x8007BD2C -- wipe cause (5 = party wipe, 0 = monsters)
local ACTOR_TABLE = 0x801C9370 -- 8 x u32 actor pointers; 0..2 party
local A_HP        = 0x14C
-- The FUN_8003AEB0 game-over gate's inputs (MAIN INIT, _DAT_8007B8B8 == 2
-- back-from-battle arm): survivors flag byte, story-flag bank byte 0
-- (bit 0x80 = flag idx 0, the scripted-loss latch), and the card-screen
-- entry-context word the gate sets alongside mode 0x16.
local BD60      = 0x8007BD60 -- bit 0x80 gates the wipe -> card handoff
local FLAG0     = 0x80085758 -- story-flag bank byte 0
local B8B8      = 0x8007B8B8 -- battle-return marker (FUN_80046A20 sets 2)
local BB00      = 0x8007BB00 -- card-screen entry context (1 = wipe path)

local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    "saves/library/pcsx-redux/3d22fa5fd53d47cd22999a7b377ec8ece057fdb5ca164357be0f96a65147ddf3.sstate")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 7200)
local QUIT_AFTER = probe.getenv_num("LEGAIA_QUIT_AFTER", 900)
local KILL       = probe.getenv("LEGAIA_KILL", "0") ~= "0"
local KILL_DELAY = probe.getenv_num("LEGAIA_KILL_DELAY", 120)
local MASH       = probe.getenv("LEGAIA_MASH", "1") ~= "0"
local MASH_PRESS, MASH_STEP = 2, 12
local FORCE_ENC  = probe.getenv_num("LEGAIA_FORCE_ENCOUNTER", 0)
local FORCE_AT   = probe.getenv_num("LEGAIA_FORCE_AT", 240)
local FORMATION_CELL = 0x8007BD0C -- 4-slot formation array (world-map.md)
local WALK       = probe.getenv("LEGAIA_WALK", "0") ~= "0"
local WALK_SPIN  = probe.getenv_num("LEGAIA_WALK_SPIN", 90)
local WALK_PREFIX_BTN    = probe.getenv_num("LEGAIA_WALK_PREFIX_BTN", -1)
local WALK_PREFIX_FRAMES = probe.getenv_num("LEGAIA_WALK_PREFIX_FRAMES", 0)
local BATTLE_MODE = 0x15 -- battle main loop (mode chain 8 -> 9 -> 0x14 -> 0x15)

local CSV = probe.csv_open(probe.out_path("gameover_mode_writer.csv"),
    "tick,kind,pc,ra,pre,now,mode,bd71,bd2c,scene,count,note")
local DETAIL = probe.out_path("gameover_mode_writer.detail.txt")

local function u8(a)  return mem.read_u8(a)  or 0 end
local function u16(a) return mem.read_u16(a) or 0 end
local function scene_name()
    local s = ""
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s = s .. string.char(b)
    end
    return (s == "") and "?" or s
end

-- Screenshot the wipe destination screen (mode LEGAIA_SHOT_MODE, default
-- the CARD per-frame mode 0x17) LEGAIA_SHOT_DELAY vsyncs after it commits
-- with the wipe cause still 5. Decode via decode_pcsx_screen.py. The
-- captured destination is the title screen with the cursor on CONTINUE
-- (the CARD surface under entry context _DAT_8007BB00 = 1).
local SHOT_MODE  = probe.getenv_num("LEGAIA_SHOT_MODE", 0x17)
local SHOT_DELAY = probe.getenv_num("LEGAIA_SHOT_DELAY", 150)

local vsync = 0
local counts = {}
local pending = {}
local quit_countdown = nil
local shot_countdown, shot_done = nil, false
local snap_banked = false
local last = { mode = nil, bd71 = nil, bd2c = nil, hp = nil, gate = nil }
local mash_on = false
local battle_seen_at = nil -- first vsync the mode word read BATTLE_MODE
local walk_dir = nil       -- currently-held D-pad button while wandering
local WALK_DIRS = { 4, 5, 6, 7 } -- UP, RIGHT, DOWN, LEFT bit indices

local function log(s) PCSX.log("[gomode] " .. s) end

local function row(kind, pc, ra, pre, now, note)
    local key = string.format("%s|%08X|%04X|%s", kind, pc or 0, now or 0,
        note or "")
    local n = (counts[key] or 0) + 1
    counts[key] = n
    if kind == "write" or n <= 4 then
        CSV:row("%d,%s,0x%08X,0x%08X,0x%04X,0x%04X,0x%02X,0x%02X,0x%02X,%s,%d,%s",
            vsync, kind, pc or 0, ra or 0, pre or 0, now or 0,
            u16(GAME_MODE), u8(BD71), u8(BD2C), scene_name(), n, note or "")
    end
end

local function party_hp_sig()
    local parts = {}
    for slot = 0, 2 do
        local ptr = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
        if ptr ~= 0 and mem.in_ram(ptr) then
            parts[#parts + 1] = string.format("%d:%d", slot, u16(ptr + A_HP))
        end
    end
    return table.concat(parts, " ")
end

local function take_screenshot(tag)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil or ss.data == nil then
        log("takeScreenShot failed: " .. tostring(ss))
        return
    end
    local path = probe.out_path(tag .. ".screen")
    local fh = io.open(path, "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mfh = io.open(path .. ".meta", "w")
    if mfh ~= nil then
        mfh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height),
            (ss.bpp == 0) and 16 or 24))
        mfh:close()
    end
    log(string.format("screenshot %s (%dx%d)", path,
        tonumber(ss.width), tonumber(ss.height)))
end

local function clamp_party_hp()
    for slot = 0, 2 do
        local ptr = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
        if ptr ~= 0 and mem.in_ram(ptr) then
            local hp = u16(ptr + A_HP)
            if hp > 1 then probe.write_u16(ptr + A_HP, 1) end
        end
    end
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = probe.getenv_num("LEGAIA_BOOT_DELAY", 60),

    on_arm = function()
        step.find_writer(GAME_MODE, 2, {
            unit = 2, read_len = 2, label = "gomode", max = 16384,
            on_write = function(rg)
                -- Fires mid-store: GAME_MODE still reads the PRE value.
                local ev = {
                    pc = rg.pc, ra = rg.ra, pre = u16(GAME_MODE),
                }
                pending[#pending + 1] = ev
                -- Capture register context NOW (post-drain regs are stale).
                ev.ctx = snapshot.capture_call_context(string.format(
                    "game_mode store pc=0x%08X ra=0x%08X pre=0x%04X tick=%d scene=%s",
                    rg.pc, rg.ra, ev.pre, vsync, scene_name()))
            end,
        })
        probe.write_manifest("autorun_gameover_mode_writer.lua", {
            sstate = SSTATE, frames = tostring(FRAMES),
            kill = tostring(KILL), mash = tostring(MASH),
        })
        log(string.format("armed on 0x%08X; kill=%s mash=%s",
            GAME_MODE, tostring(KILL), tostring(MASH)))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        vsync = elapsed

        -- Drain write hits (now = committed value).
        for i = 1, #pending do
            local ev = pending[i]
            local now = u16(GAME_MODE)
            row("write", ev.pc, ev.ra, ev.pre, now, "")
            if now == 0x12 or ev.pre == 0x12 then
                probe.append_call_context(DETAIL, ev.ctx)
                log(string.format("MODE-0x12 STORE pc=0x%08X ra=0x%08X pre=0x%04X now=0x%04X",
                    ev.pc, ev.ra, ev.pre, now))
            end
        end
        pending = {}

        -- Timeline polls.
        local m = u16(GAME_MODE)
        if m ~= last.mode then
            row("mode", 0, 0, last.mode or 0, m, "")
            last.mode = m
            if m == 0x12 and not snap_banked then
                snap_banked = true
                sstate.save(probe.out_path("gameover_hit.sstate"))
                log("mode 0x12 committed; snapshot banked")
                quit_countdown = quit_countdown or QUIT_AFTER
            end
        end
        local b71 = u8(BD71)
        if b71 ~= last.bd71 then
            row("bd71", 0, 0, last.bd71 or 0, b71, "")
            last.bd71 = b71
        end
        local b2c = u8(BD2C)
        if b2c ~= last.bd2c then
            row("bd2c", 0, 0, last.bd2c or 0, b2c, "")
            last.bd2c = b2c
        end
        local hp = party_hp_sig()
        if hp ~= last.hp then
            row("hp", 0, 0, 0, 0, (hp == "") and "none" or hp)
            last.hp = hp
        end
        local gate = string.format("bd60=%02X flag0=%02X b8b8=%02X bb00=%02X",
            u8(BD60), u8(FLAG0), u8(B8B8), u8(BB00))
        if gate ~= last.gate then
            row("gate", 0, 0, 0, 0, gate)
            last.gate = gate
        end

        -- Forced-encounter entry: replay FUN_801DA51C state 1/2's work.
        if FORCE_ENC > 0 and battle_seen_at == nil and elapsed == FORCE_AT
            and m == 3 then
            probe.write_u8(FORMATION_CELL + 0, FORCE_ENC)
            probe.write_u8(FORMATION_CELL + 1, 0)
            probe.write_u8(FORMATION_CELL + 2, 0)
            probe.write_u8(FORMATION_CELL + 3, 0)
            probe.write_u16(GAME_MODE, 8)
            row("force", 0, 0, m, 8, string.format("monster_id=%d", FORCE_ENC))
            log(string.format("forced encounter: monster id %d, mode 3 -> 8",
                FORCE_ENC))
        end

        local in_battle = (m == BATTLE_MODE)
        if in_battle and battle_seen_at == nil then
            battle_seen_at = elapsed
            log(string.format("battle main loop reached at tick %d", elapsed))
        end

        if KILL and in_battle and battle_seen_at
            and elapsed >= battle_seen_at + KILL_DELAY then
            clamp_party_hp()
        end

        -- Wander toward a random encounter until the battle chain starts.
        if WALK and battle_seen_at == nil and m < 8 then
            if elapsed <= WALK_PREFIX_FRAMES and WALK_PREFIX_BTN >= 0 then
                if walk_dir ~= WALK_PREFIX_BTN then
                    if walk_dir then pad.release(walk_dir) end
                    walk_dir = WALK_PREFIX_BTN
                    pad.force(walk_dir)
                end
            else
                local want = WALK_DIRS[(math.floor(elapsed / WALK_SPIN) % 4) + 1]
                if walk_dir ~= want then
                    if walk_dir then pad.release(walk_dir) end
                    walk_dir = want
                    pad.force(walk_dir)
                end
            end
        elseif walk_dir then
            pad.release(walk_dir)
            walk_dir = nil
        end

        -- Mash CROSS only once the battle chain is live (mashing during a
        -- WALK wander would talk to NPCs / open menus). Hold off while a
        -- wipe-destination screenshot is pending so the screen sits still;
        -- resume after it banks (the post-confirm transition then logs too).
        local mash_ok = MASH and (not WALK or battle_seen_at ~= nil)
            and shot_countdown == nil
        if mash_ok then
            local phase = elapsed % MASH_STEP
            if phase < MASH_PRESS then
                if not mash_on then pad.force(pad.BTN.CROSS); mash_on = true end
            else
                if mash_on then pad.release(pad.BTN.CROSS); mash_on = false end
            end
        elseif mash_on then
            pad.release(pad.BTN.CROSS)
            mash_on = false
        end

        if not shot_done and shot_countdown == nil and m == SHOT_MODE
            and u8(BD2C) == 5 then
            shot_countdown = SHOT_DELAY
        end
        if shot_countdown then
            shot_countdown = shot_countdown - 1
            if shot_countdown <= 0 then
                shot_countdown = nil
                shot_done = true
                take_screenshot("wipe_destination")
                sstate.save(probe.out_path("wipe_destination.sstate"))
                quit_countdown = quit_countdown or QUIT_AFTER
            end
        end

        if quit_countdown then
            quit_countdown = quit_countdown - 1
            if quit_countdown <= 0 then ctx.request_quit = true end
        end
    end,

    on_done = function()
        if mash_on then pad.release(pad.BTN.CROSS) end
        if walk_dir then pad.release(walk_dir) end
        CSV:close()
        log(string.format("done; snap_banked=%s", tostring(snap_banked)))
    end,
})
