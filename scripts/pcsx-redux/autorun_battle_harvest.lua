-- autorun_battle_harvest.lua
--
-- Closed-loop retail-capture harvester: from a field (or battle) save state,
-- walk until a random encounter starts, then play every round with Spirit
-- for each member so the monsters act, and write a raw save state each time
-- a monster's action is on screen. The states become retail-compare corpus
-- candidates for the uncovered enemy attacks and monsters
-- (docs/tooling/capture-coverage.md); classify them with
-- `legaia-engine retail-compare --census` before registering any.
--
-- Pure retail play: no RAM writes, no pokes. The pad is the only input.
--
-- Field: hold a direction, cycling Up / Right / Down / Left every
-- LEGAIA_WALK_LEG vsyncs, with a Cross every so often to clear a dialogue
-- box. Battle (game mode 0x14 / 0x15, ctx = *0x8007BD24):
--   ctx[+0x06] == 0x1E  round prompt  -> Cross (Begin)
--   ctx[+0x06] == 0x28  command ring  -> Down (Spirit) for the first
--                       LEGAIA_SPIRIT_ROUNDS rounds, then Left (Attack) so
--                       the fight ends and the walk finds the next one
--   ctx[+0x06] == 0x6E  commit check  -> Cross (Begin)
--   other menu states (results, banners) -> Cross every 20 vsyncs
--   ctx[+0x06] == 0xFF  the action SM runs: no input
-- A capture is taken when the acting seat ctx[+0x13] is a monster seat (>= 3)
-- and the action-state cursor ctx[+0x07] has sat LEGAIA_HOLD vsyncs in a
-- mid-action state (strike loop 0x1E, cast 0x2B / 0x2C, module 0x6F / 0x70);
-- at most LEGAIA_PER_KEY captures per (monster id, category, queued id), the
-- queued id dropped for a physical attack (its queue bytes are swings).
--
-- Env vars:
--   LEGAIA_SSTATE    source save state (must be a retail-disc state)
--   LEGAIA_OUT_DIR   output dir: <name>.rawsstate per capture + harvest.log
--   LEGAIA_FRAMES    vsyncs to run (default 12000)
--   LEGAIA_WALK_LEG  vsyncs per walk direction (default 50)
--   LEGAIA_HOLD      vsyncs inside a mid-action state before the capture (default 8)
--   LEGAIA_PER_KEY   captures per action key (default 1)
--   LEGAIA_MAX_CAPS  stop after this many captures (default 12)
--   LEGAIA_PREFIX    capture name prefix (default "harvest")
--   LEGAIA_SPIRIT_ROUNDS  rounds of Spirit before attacking (default 3)
--   LEGAIA_SCENES    1 = also save one field state per scene the walk reaches:
--                    once the loaded scene (label 0x8007050C and define
--                    0x80084540 both stable) has run LEGAIA_SCENE_SETTLE
--                    field vsyncs (default 150) and the player has moved
--                    since the scene entered (the walk has control); named
--                    <prefix>_scene_<label>_v<vsync>. The start scene counts.
--
-- No breakpoints are armed, so --fast (dynarec) runs are fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 12000)
local WALK_LEG = probe.getenv_num("LEGAIA_WALK_LEG", 50)
local HOLD     = probe.getenv_num("LEGAIA_HOLD", 8)
local PER_KEY  = probe.getenv_num("LEGAIA_PER_KEY", 1)
local MAX_CAPS = probe.getenv_num("LEGAIA_MAX_CAPS", 12)
local PREFIX   = probe.getenv("LEGAIA_PREFIX", "harvest")
local SPIRIT_ROUNDS = probe.getenv_num("LEGAIA_SPIRIT_ROUNDS", 3)
local SCENES = probe.getenv_num("LEGAIA_SCENES", 0) == 1
local SCENE_SETTLE = probe.getenv_num("LEGAIA_SCENE_SETTLE", 150)
local LOADED_DEFINE = 0x80084540
local PLAYER_PTR = 0x8007c364

local GAME_MODE_VA = 0x8007b83c
local SCENE_VA     = 0x8007050c
local CTX_PTR      = 0x8007bd24
local ACTORS       = 0x801c9370
local MONSTER_IDS  = 0x8007bd0c

local BTN = probe.BTN
local WALK = { BTN.UP, BTN.RIGHT, BTN.DOWN, BTN.LEFT }

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[harvest] " .. s)
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end

-- The mid-action states a capture waits in: the strike loop 0x1E, the cast's
-- sustained animation / hit loop 0x2B..0x2C, the module's running phases
-- 0x6F..0x70 (docs/subsystems/battle-action.md, state table).
local function band(st)
    if st == 0x1E then return "attack" end
    if st == 0x2B or st == 0x2C then return "magic" end
    if st == 0x6F or st == 0x70 then return "module" end
    return nil
end

local held = nil        -- the button currently forced
local held_until = 0
local function press(btn, vsync, hold)
    if held ~= nil then probe.pad.release(held) end
    probe.pad.force(btn)
    held = btn
    held_until = vsync + (hold or 4)
end
local function release_if_due(vsync)
    if held ~= nil and vsync >= held_until then
        probe.pad.release(held)
        held = nil
    end
end

local function save_checkpoint(name, vsync)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    logf("CKPT %s vsync=%d ok=%s %s", name, vsync, tostring(ok), tostring(err or ""))
    return ok
end

local caps = 0
local per_key = {}
local band_key, band_since = nil, 0
local last_mode, last_flow = nil, nil
local next_menu_press = 0
local walk_i, walk_next = 1, 0
local battles = 0
local rounds = 0
local scene_seen = {}
local scene_key, scene_since, scene_pos, scene_moved = nil, 0, nil, false

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        logf("sstate=%s frames=%d hold=%d per_key=%d max=%d",
            SSTATE_PATH, FRAMES, HOLD, PER_KEY, MAX_CAPS)
        return {}
    end,

    on_capture = function(ctx, vsync)
        release_if_due(vsync)
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        if mode ~= last_mode then
            logf("vsync=%d game_mode=0x%02x scene=%q", vsync, mode, scene_name())
            if mode == 0x14 or mode == 0x15 then
                if last_mode ~= 0x14 and last_mode ~= 0x15 then
                    battles = battles + 1
                    rounds = 0
                end
            end
            last_mode = mode
        end

        if mode == 0x03 and SCENES and caps < MAX_CAPS then
            local label = scene_name()
            local key = label .. "/" .. tostring(probe.read_u16(LOADED_DEFINE) or 0)
            local pp = probe.read_u32(PLAYER_PTR)
            local pos = nil
            if ok_ptr(pp) then
                pos = string.format("%d,%d", probe.read_u16(pp + 0x14) or 0, probe.read_u16(pp + 0x18) or 0)
            end
            if key ~= scene_key then
                scene_key, scene_since, scene_pos, scene_moved = key, vsync, pos, false
            elseif pos ~= scene_pos then
                scene_moved = true
            end
            if not scene_seen[label] and scene_moved and vsync - scene_since >= SCENE_SETTLE then
                if save_checkpoint(string.format("%s_scene_%s_v%d", PREFIX, label, vsync), vsync) then
                    scene_seen[label] = true
                    caps = caps + 1
                end
            end
        elseif mode ~= 0x03 then
            scene_key = nil
        end
        if mode == 0x03 then
            -- Walk: hold one direction per leg, a Cross now and then.
            if vsync >= walk_next then
                walk_i = (walk_i % #WALK) + 1
                walk_next = vsync + WALK_LEG
                press(WALK[walk_i], vsync, WALK_LEG - 2)
            elseif vsync % 97 == 0 and held == nil then
                press(BTN.CROSS, vsync, 4)
            end
            return
        end
        if mode ~= 0x14 and mode ~= 0x15 then
            if vsync >= next_menu_press then
                press(BTN.CROSS, vsync, 4)
                next_menu_press = vsync + 30
            end
            return
        end

        local cp = probe.read_u32(CTX_PTR)
        if not ok_ptr(cp) then return end
        local flow = probe.read_u8(cp + 6) or 0
        local st = probe.read_u8(cp + 7) or 0
        local seat = probe.read_u8(cp + 0x13) or 0
        if flow ~= last_flow then
            if flow == 0x1E then rounds = rounds + 1 end
            last_flow = flow
            next_menu_press = vsync + 10
        end

        if flow == 0xFF then
            -- The action SM runs: watch for a monster's action on screen.
            local b = band(st)
            if seat >= 3 and seat < 8 and b ~= nil then
                local ap = probe.read_u32(ACTORS + seat * 4)
                if ok_ptr(ap) then
                    local mid = probe.read_u8(MONSTER_IDS + seat - 3) or 0
                    local cat = probe.read_u8(ap + 0x1DE) or 0
                    local q = probe.read_u8(ap + 0x1DF) or 0
                    -- A physical attack's queue byte is a swing, not an
                    -- attack id: key it on the monster alone.
                    if cat == 3 then q = 0 end
                    local key = string.format("m%03d_c%d_q%02x", mid, cat, q)
                    if band_key ~= key .. b then
                        band_key, band_since = key .. b, vsync
                    elseif vsync - band_since == HOLD
                        and (per_key[key] or 0) < PER_KEY and caps < MAX_CAPS then
                        local name = string.format("%s_%s_%s_st%02x_v%d",
                            PREFIX, key, scene_name(), st, vsync)
                        if save_checkpoint(name, vsync) then
                            per_key[key] = (per_key[key] or 0) + 1
                            caps = caps + 1
                            if caps >= MAX_CAPS then ctx.request_quit = true end
                        end
                    end
                end
            else
                band_key = nil
            end
            return
        end
        band_key = nil
        if vsync < next_menu_press then return end
        if flow == 0x28 then
            press(rounds > SPIRIT_ROUNDS and BTN.LEFT or BTN.DOWN, vsync, 4)
        else
            press(BTN.CROSS, vsync, 4)
        end
        next_menu_press = vsync + 20
    end,

    on_done = function(_, _)
        if held ~= nil then probe.pad.release(held) end
        logf("done: battles=%d captures=%d", battles, caps)
        local fh = io.open(probe.out_path("harvest.log"), "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
