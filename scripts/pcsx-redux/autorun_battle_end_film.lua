-- autorun_battle_end_film.lua
--
-- Play a retail battle to its end and film the END sequence (the results
-- sequencer FUN_8004E568): who poses, which clip, where its cursor stands
-- each vsync through the results hold and the exit fade, plus screenshots and
-- raw save states at the sequence's landmarks.
--
-- Battle input (ctx = *0x8007BD24, ctx[+0x06] the flow state, ctx[+0x13] the
-- seat being commanded):
--   0x1E round prompt -> Cross, after the round pokes below
--   0x28 command ring -> the seat's plan letter: A = Left (Attack),
--                        S = Down (Spirit)
--   other menu states -> Cross every 20 vsyncs
--   0xFF              -> no input (the action SM runs)
-- Once the battle-end signal DAT_8007BD71 reads 0xFE the pad is released and
-- the probe only watches.
--
-- Env:
--   LEGAIA_SSTATE   battle save state (or --scenario on the runner)
--   LEGAIA_FRAMES   vsyncs to run (default 6000)
--   LEGAIA_PLAN     one letter per party seat, e.g. "SA" (seat 0 Spirit,
--                   seat 1 Attack); default "AAA"
--   LEGAIA_FINISH   1 = at every round prompt write each standing monster's
--                   HP +0x14C to 1, so the first landed hit ends the fight
--   LEGAIA_WIPE     1 = at every round prompt write each standing party
--                   seat's HP, readout +0x172 and pending delta +0x10 to 1 /
--                   1 / 0, so the monsters' hits wipe the party
--   LEGAIA_SHOT_EVERY  vsyncs between screenshots after the signal (default 2;
--                   0 = none)
--   LEGAIA_PREFIX   save-state name prefix (default "battle_end")
--
-- Output (probe.out_path): end.csv (one row a vsync from the signal),
-- shot_<vsync>.raw + .raw.meta, <prefix>_{signal,results,hold80,fade}.rawsstate.
-- A state made with LEGAIA_FINISH / LEGAIA_WIPE is a poked state: say so in
-- its scenario description.
--
-- Poll-only (no breakpoints): --fast / --turbo / --headless are fine.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --headless --fast \
--       --iso <scratch copy of the disc> --scenario noa_levelup_fight_pre \
--       --lua scripts/pcsx-redux/autorun_battle_end_film.lua --frames 6000

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local BTN = probe.BTN

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local PLAN = probe.getenv("LEGAIA_PLAN", "AAA")
local FINISH = probe.getenv("LEGAIA_FINISH", "0") == "1"
local WIPE = probe.getenv("LEGAIA_WIPE", "0") == "1"
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 2)
local PREFIX = probe.getenv("LEGAIA_PREFIX", "battle_end")

local CTX_PTR = 0x8007BD24
local SIGNAL = 0x8007BD71
local CAUSE = 0x8007BD2C
local POSE_ID = 0x8007BD64
local HOLD = 0x8007BD6C
local GAME_MODE = 0x8007B83C
local ACTORS = 0x801C9370
local FIRST_MONSTER = 0x8007BD0C
local STEP = 0x1F800393

local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end
local function logf(fmt, ...) PCSX.log("[end] " .. string.format(fmt, ...)) end
local function seat_actor(s)
    local a = probe.read_u32(ACTORS + s * 4)
    if ok_ptr(a) then return a end
    return nil
end

local held, held_until = nil, 0
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

local function save_state(name, vsync)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    logf("CKPT %s vsync=%d ok=%s %s", name, vsync, tostring(ok), tostring(err or ""))
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240, (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local function round_pokes()
    if FINISH then
        for s = 3, 7 do
            local a = seat_actor(s)
            if a ~= nil and (probe.read_u16(a + 0x14C) or 0) > 0 then
                probe.write_u16(a + 0x14C, 1)
            end
        end
    end
    if WIPE then
        for s = 0, 2 do
            local a = seat_actor(s)
            if a ~= nil and (probe.read_u16(a + 0x14C) or 0) > 0 then
                probe.write_u16(a + 0x14C, 1)
                probe.write_u16(a + 0x172, 1)
                probe.write_u32(a + 0x10, 0)
            end
        end
    end
end

local csv = probe.csv_open(probe.out_path("end.csv"),
    "vsync,step,signal,cause,mode,ctx13,phase6ce,hold,pose_id,first_monster,ctx6da,ctx0d," ..
    "seat_cur,seat_staged,seat_latched,seat_flags,seat_rate,seat_176,seat_21b,seat_1fa,node_cur,seat_hp")

local signal_at = nil
local last_flow, next_press = nil, 0
local saved = {}
local exit_at = nil

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_battle_end_film.lua",
            { sstate = SSTATE, frames = FRAMES, plan = PLAN, finish = FINISH, wipe = WIPE })
        return {}
    end,
    on_capture = function(c, vsync)
        release_if_due(vsync)
        local cp = probe.read_u32(CTX_PTR)
        if not ok_ptr(cp) then return end
        local signal = probe.read_u8(SIGNAL) or 0
        local mode = probe.read_u16(GAME_MODE) or 0
        if signal_at == nil and signal == 0xFE then
            signal_at = vsync
            if held ~= nil then probe.pad.release(held); held = nil end
            logf("battle-end signal at vsync %d", vsync)
            save_state(PREFIX .. "_signal", vsync)
        end
        if signal_at == nil then
            if mode ~= 0x15 and mode ~= 0x14 then return end
            local flow = probe.read_u8(cp + 6) or 0
            if flow ~= last_flow then
                if flow == 0x1E then round_pokes() end
                last_flow = flow
                next_press = vsync + 10
            end
            if flow == 0xFF or vsync < next_press then return end
            if flow == 0x28 then
                local seat = probe.read_u8(cp + 0x13) or 0
                local plan = string.sub(PLAN, seat + 1, seat + 1)
                press(plan == "S" and BTN.DOWN or BTN.LEFT, vsync, 4)
            else
                press(BTN.CROSS, vsync, 4)
            end
            next_press = vsync + 20
            return
        end

        local seat = probe.read_u8(cp + 0x13) or 0
        local a = seat_actor(seat) or 0
        local function au8(o) return a ~= 0 and (probe.read_u8(a + o) or 0) or 0 end
        local function au16(o) return a ~= 0 and (probe.read_u16(a + o) or 0) or 0 end
        local node = a ~= 0 and probe.read_u32(a + 0x22C) or 0
        local node_cur = ok_ptr(node) and (probe.read_u16(node + 0x68) or 0) or 0
        local phase = probe.read_u16(cp + 0x6CE) or 0
        local hold = probe.read_u32(HOLD) or 0
        csv:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            vsync, probe.read_u8(STEP) or 0, signal, probe.read_u32(CAUSE) or 0, mode, seat,
            phase, hold, probe.read_u32(POSE_ID) or 0, probe.read_u8(FIRST_MONSTER) or 0,
            probe.read_u16(cp + 0x6DA) or 0, probe.read_u8(cp + 0xD) or 0,
            au8(0x1D9), au8(0x1DA), au8(0x1DB), au8(0x1DC), au8(0x21D), au16(0x176),
            au8(0x21B), au8(0x1FA), node_cur, au16(0x14C))
        if SHOT_EVERY > 0 and (vsync - signal_at) % SHOT_EVERY == 0 and mode == 0x15 then
            screenshot(string.format("shot_%05d", vsync))
        end
        if phase == 1 and not saved.results then
            saved.results = true
            save_state(PREFIX .. "_results", vsync)
        end
        if phase == 1 and hold >= 0x80 and not saved.hold then
            saved.hold = true
            save_state(PREFIX .. "_hold80", vsync)
        end
        if phase >= 2 and phase < 0x43 and not saved.fade then
            saved.fade = true
            save_state(PREFIX .. "_fade", vsync)
        end
        if mode ~= 0x15 and exit_at == nil then
            exit_at = vsync
            logf("game mode left battle at vsync %d (mode 0x%X)", vsync, mode)
        end
        if exit_at ~= nil and vsync > exit_at + 30 then c.request_quit = true end
    end,
    on_done = function()
        if held ~= nil then probe.pad.release(held) end
        logf("done: signal %s exit %s", tostring(signal_at), tostring(exit_at))
    end,
})
