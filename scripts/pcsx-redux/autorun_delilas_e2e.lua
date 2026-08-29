-- autorun_delilas_e2e.lua
--
-- End-to-end live verdict for a --delilas-party patched disc: load a
-- pre-encounter field save state, stage the full party (slots 0,1,2 =
-- Vahn/Noa/Gala hosts = the mapped siblings), walk until a NATURAL
-- encounter rolls (forced battles wedge Spirit - a forced-context
-- artifact, not a disc defect), then every battle round drive the
-- command ring to SPIRIT (CROSS, DOWN, CROSS). Outputs:
--
--   delilas_e2e.csv   - mode transitions, battle-load ordinal telemetry
--                       (ctx+0x240+slot, the 0xFE variant-pair ordinal
--                       whose out-of-range pin installs foreign object
--                       pointers - the Spirit-streak class), action
--                       window changes, screenshot index
--   ring_*.raw        - periodic battle screenshots (idle / move input)
--   spirit_*.raw      - screenshots while a Spirit clip (0x10/0x11) plays
--
-- The RAW screenshots convert with scripts/pcsx-redux/raw2png.py.
-- Run on BOTH the candidate disc and a known-bad disc: the harness's
-- verdict is the CONTRAST (see scripts/e2e-delilas-party.sh).
--
-- Env (via run_probe.sh):
--   LEGAIA_SSTATE / --scenario   pre-encounter field state
--   LEGAIA_FRAMES                capture vsyncs (default 20000)
--   LEGAIA_PARTY                 party slot bytes (default "0,1,2")
--   LEGAIA_SHOT_STEP             battle screenshot period (default 120)
--   LEGAIA_MAX_SHOTS             screenshot cap (default 60)
--
-- Use --fast: this probe is poll+poke only (no exec breakpoints).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local PARTY_TABLE = 0x8007BD10
local ACTOR_TABLE = 0x801C9370
-- Battle context pointer (lw via lui 0x8008 / -0x42DC in the cast
-- modules; the ordinal bytes live at ctx+0x240+slot, the per-slot
-- variant-pair snapshots at ctx+0x1030+slot*0x10).
local CTX_PTR     = 0x8007BD24
local BATTLE_MAIN = 0x15

local SSTATE    = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES    = probe.getenv_num("LEGAIA_FRAMES", 20000)
local PARTY_RAW = probe.getenv("LEGAIA_PARTY", "0,1,2")
local SHOT_STEP = probe.getenv_num("LEGAIA_SHOT_STEP", 120)
local MAX_SHOTS = probe.getenv_num("LEGAIA_MAX_SHOTS", 60)
-- SCUS re-sync pokes (`legaia-patcher scus-pokes` output). The save
-- state carries the resident SCUS of the disc it was made on; without
-- these, fresh overlay code from the disc under test jals into a STALE
-- injection arena - a per-frame dynarec fault (seen at 0x8007782C) that
-- wedges the whole battle.
local POKES_FILE = probe.getenv("LEGAIA_POKES_FILE", "")
-- Optional forced encounter for scenes with no natural rolls (towns):
-- comma monster ids poked into the formation cells + game mode 8 (the
-- encounter transition), entering battle through the same load path a
-- walk-rolled encounter takes. Empty = walk for a natural encounter.
local FORCE_IDS = {}
for tok in string.gmatch(probe.getenv("LEGAIA_FORCE_IDS", ""), "[^,%s]+") do
    FORCE_IDS[#FORCE_IDS+1] = tonumber(tok)
end
local FORMATION_CELL = 0x8007BD0C

local party = {}
for tok in string.gmatch(PARTY_RAW, "[^,%s]+") do party[#party+1] = tonumber(tok) end

local CSV = probe.csv_open(probe.out_path("delilas_e2e.csv"), "tick,mode,note")

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228, (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local staged = false
local settle = 0
local last_mode = -1
local last_win = nil
local last_ords = nil
local battle_seen = false
local spirit_frames = 0
local ring_shots = 0
local spirit_shots = 0
-- Two-phase battle drive (see below): "unwedge" completes whatever
-- command flow the battle opened inside, "spirit" runs the ring cycle.
local phase = "unwedge"
local playing_seen = false
local quiet_ticks = 0

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(ctx_unused, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == 1 and POKES_FILE ~= "" then
            local n = 0
            for line in io.lines(POKES_FILE) do
                local a, v = string.match(line, "0x(%x+):0x(%x+)")
                if a and v then
                    probe.write_u32(tonumber(a, 16), tonumber(v, 16))
                    n = n + 1
                end
            end
            CSV:row("%d,0x%X,scus-pokes applied %d words", elapsed, mode, n)
        end
        if elapsed == 120 then
            for i = 0, 2 do probe.write_u8(PARTY_TABLE + i, party[i + 1] or 0) end
            staged = true
            CSV:row("%d,0x%X,party staged=%s", elapsed, mode, PARTY_RAW)
            return
        end
        if #FORCE_IDS > 0 and elapsed == 240 and mode == 0x3 then
            for i = 0, 3 do probe.write_u8(FORMATION_CELL + i, FORCE_IDS[i+1] or 0) end
            probe.write_u16(GAME_MODE, 8)
            CSV:row("%d,0x%X,battle forced ids=%s", elapsed, mode,
                probe.getenv("LEGAIA_FORCE_IDS", ""))
            return
        end
        -- Advance through any dialog / cutscene modes toward the field.
        if staged and (mode == 0x16 or mode == 0x17 or mode == 0x1A or mode == 0x2) then
            local ph = elapsed % 40
            if ph == 0 then pad.force(pad.BTN.CROSS)
            elseif ph == 8 then pad.release(pad.BTN.CROSS) end
        end
        -- Field walk: cycle directions until an encounter rolls.
        if staged and mode == 0x3 then
            local d = math.floor(elapsed / 120) % 4
            local btns = { pad.BTN.RIGHT, pad.BTN.DOWN, pad.BTN.LEFT, pad.BTN.UP }
            if elapsed % 120 == 0 then
                pad.release(pad.BTN.UP); pad.release(pad.BTN.RIGHT)
                pad.release(pad.BTN.DOWN); pad.release(pad.BTN.LEFT)
                pad.force(btns[d + 1])
            end
            if elapsed % 120 == 110 then
                pad.release(pad.BTN.UP); pad.release(pad.BTN.RIGHT)
                pad.release(pad.BTN.DOWN); pad.release(pad.BTN.LEFT)
            end
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change", elapsed, mode)
            last_mode = mode
        end
        if staged and mode == BATTLE_MAIN then
            if not battle_seen then
                battle_seen = true
                CSV:row("%d,0x%X,battle-reached", elapsed, mode)
            end
            settle = settle + 1
            -- Ordinal telemetry: log the three slot bytes on every change
            -- (and once at battle entry). A byte that reads >= 2 names a
            -- variant-pair PAST the two-pair snapshot - the foreign-pin
            -- precondition.
            local bctx = probe.read_u32(CTX_PTR)
            if bctx ~= nil and bctx >= 0x80000000 and bctx < 0x80800000 then
                local o0 = probe.read_u8(bctx + 0x240) or 255
                local o1 = probe.read_u8(bctx + 0x241) or 255
                local o2 = probe.read_u8(bctx + 0x242) or 255
                local s = string.format("%d %d %d", o0, o1, o2)
                if s ~= last_ords then
                    CSV:row("%d,0x%X,ordinals %s", elapsed, mode, s)
                    last_ords = s
                end
            end
            -- Scan the party actors: is any Spirit clip (0x10/0x11)
            -- playing or staged (pad must stay quiet while one is), and
            -- is ANY clip playing (the phase-flip signal below)?
            local spirit_active = false
            local any_playing = false
            for slot = 0, 2 do
                local ap = probe.read_u32(ACTOR_TABLE + slot * 4)
                if ap ~= nil and ap ~= 0 then
                    local pl = probe.read_u8(ap + 0x1D9) or 0
                    local st = probe.read_u8(ap + 0x1DA) or 0
                    if pl ~= 0 then any_playing = true end
                    if pl == 0x10 or pl == 0x11 or st == 0x10 or st == 0x11 then
                        spirit_active = true
                    end
                end
            end
            -- Phase flip: a natural battle can open INSIDE the first
            -- character's Attack flow (target pill / arrow input), which
            -- CIRCLE cannot leave - the ring cycle would wedge there
            -- forever. So phase "unwedge" first COMPLETES a full attack
            -- round: CROSS confirms whatever is pending, LEFT both picks
            -- Attack on a ring and types arrows into the input bar (the
            -- bar auto-executes when full). Once a clip has played
            -- (round ran) and the field has been quiet for 180 ticks,
            -- the cursor is back on a fresh round menu - switch to the
            -- proven Spirit ring cycle.
            if any_playing then playing_seen = true; quiet_ticks = 0
            else quiet_ticks = quiet_ticks + 1 end
            if phase == "unwedge" and playing_seen and quiet_ticks > 180 then
                phase = "spirit"
                settle = 0
                CSV:row("%d,0x%X,phase spirit (first round completed)", elapsed, mode)
            end
            -- Spirit drive (sequence proven by menu-graph mapping): the
            -- round menu is "Begin | Run"; CROSS enters the current
            -- character's command RING (Item up / Attack left / Magic
            -- right / SPIRIT down); DOWN queues Spirit and advances to
            -- the next member; after three DOWNs the cursor is back on
            -- Begin and CROSS starts the round. Each cycle opens with two
            -- CIRCLE cancels to unwind any accidental submenu (never
            -- TRIANGLE - it opens the arts list). A queued Spirit is
            -- visible as actor+0x1DE == 4 before the round begins.
            if spirit_active then
                pad.release(pad.BTN.CIRCLE)
                pad.release(pad.BTN.DOWN)
                pad.release(pad.BTN.LEFT)
                pad.release(pad.BTN.CROSS)
            elseif phase == "unwedge" then
                local ph = settle % 200
                if ph == 0 or ph == 170 then pad.force(pad.BTN.CROSS)
                elseif ph == 6 or ph == 176 then pad.release(pad.BTN.CROSS)
                elseif ph >= 30 and ph < 150 and ph % 20 == 10 then pad.force(pad.BTN.LEFT)
                elseif ph >= 30 and ph < 150 and ph % 20 == 16 then pad.release(pad.BTN.LEFT)
                end
            else
                local ph = settle % 400
                if ph == 0 or ph == 40 then pad.force(pad.BTN.CIRCLE)
                elseif ph == 6 or ph == 46 then pad.release(pad.BTN.CIRCLE)
                elseif ph == 80 or ph == 280 or ph == 330 then pad.force(pad.BTN.CROSS)
                elseif ph == 86 or ph == 286 or ph == 336 then pad.release(pad.BTN.CROSS)
                elseif ph == 130 or ph == 180 or ph == 230 then pad.force(pad.BTN.DOWN)
                elseif ph == 136 or ph == 186 or ph == 236 then pad.release(pad.BTN.DOWN)
                end
                if ph == 270 then
                    local q = {}
                    for slot = 0, 2 do
                        local ap = probe.read_u32(ACTOR_TABLE + slot * 4)
                        q[#q + 1] = (ap ~= nil and ap ~= 0)
                            and string.format("%d", probe.read_u8(ap + 0x1DE) or 0) or "-"
                    end
                    CSV:row("%d,0x%X,queued cmds %s (4 = Spirit)", elapsed,
                        mode, table.concat(q, " "))
                end
            end
            -- Periodic battle shots (idle / command ring / move input).
            if settle % SHOT_STEP == 1 and ring_shots + spirit_shots < MAX_SHOTS then
                ring_shots = ring_shots + 1
                screenshot(string.format("ring_%03d_t%d", ring_shots, elapsed))
            end
            -- Spirit shots: any party actor playing/staging 0x10/0x11.
            local pp = probe.read_u32(ACTOR_TABLE)
            if pp ~= nil and pp ~= 0 then
                local win = {}
                for off = 0x1D8, 0x1F4 do
                    win[#win+1] = string.format("%02X", probe.read_u8(pp + off) or 0)
                end
                local s = table.concat(win, " ")
                if s ~= last_win then
                    CSV:row("%d,0x%X,win | %s", elapsed, mode, s)
                    last_win = s
                end
            end
            if spirit_active then
                spirit_frames = spirit_frames + 1
                if spirit_frames % 3 == 0
                    and ring_shots + spirit_shots < MAX_SHOTS then
                    spirit_shots = spirit_shots + 1
                    screenshot(string.format("spirit_%03d_t%d", spirit_shots, elapsed))
                end
            end
        end
    end,
    on_done = function()
        CSV:row("0,0,done battle=%s spirit_frames=%d ring_shots=%d spirit_shots=%d",
            tostring(battle_seen), spirit_frames, ring_shots, spirit_shots)
        CSV:close()
    end,
})
