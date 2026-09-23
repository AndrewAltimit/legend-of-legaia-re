-- autorun_talk_to_npc.lua
--
-- Walk the player to one field NPC and open its conversation, checkpointing
-- the frames after the confirm press so the dialogue box can be read back
-- offline (VRAM display crop of each checkpoint). The retail side of a
-- dialogue frame-pair: the engine opens the same placement's conversation
-- with a pad script; this reaches it on retail from any field state.
--
-- The NPC is named by its SCRIPT POINTER: a field actor's +0x90 is its
-- placement record's address in the scene MAN buffer (man_section.rs
-- ActorPlacement::record_offset), so `LEGAIA_NPC_P90 = MAN base +
-- record_offset` picks one placement regardless of where its walk route has
-- taken it. The MAN base is P1[1]'s actor +0x90 minus P1[1]'s record offset.
--
-- Steering is closed-loop on the live positions (player = *0x8007C364,
-- +0x14 / +0x18 signed 16-bit). The pad is camera-relative, so each
-- direction's world step is learnt online (the autorun_s4_doornav.lua
-- model) and the best-aligned one is held. A push that stops the player
-- for 12 vsyncs while still out of range sidesteps along the other axis for
-- 24 vsyncs (alternating sides); once within LEGAIA_TALK_DIST it releases,
-- taps toward the NPC to face it and presses CROSS. Checkpoints land
-- LEGAIA_SHOTS vsyncs after the press.
--
-- A route with walls between the player and the NPC takes LEGAIA_WAYPOINTS,
-- a "col,row;col,row;..." tile list walked first (tile centre = tile * 128 +
-- 64, each reached within 40 units) - plan it offline on the scene's
-- walkability grid, the .MAP bytes at +0x4000 (1 byte per tile, row-major
-- 128 x 128, high nibble = four sub-cell wall bits; field-locomotion.md).
--
-- LEGAIA_ROUTE ("first-last:BTN+BTN,...", vsyncs counted from the resume)
-- holds pad spans before any steering - a recorded route (e.g. the human
-- S4 -> Tetsu walk of autorun_record_inputs.lua, frame numbers doubled for
-- the vsync clock) gets past obstacles a straight-line steer cannot.
--
-- LEGAIA_POKE_POS ("x,z") instead writes the player's +0x14 / +0x18 once,
-- at the first steering vsync: a position poke, for a capture whose subject
-- is the NPC's conversation rather than the walk to it. It moves nothing
-- the dialogue reads.
--
-- Recompiler-safe (vsync-driven); run with --fast.
-- Env: LEGAIA_SSTATE, LEGAIA_NPC_P90 (hex), LEGAIA_TALK_DIST (default 200),
--      LEGAIA_TRACE_VSYNCS (after the box opens, CROSS every 20 vsyncs for N
--      vsyncs and write the player's +0x14/+0x16/+0x18 per vsync to
--      <OUT_DIR>/trace.csv - a scripted arc or walk the conversation runs),
--      LEGAIA_WAYPOINTS, LEGAIA_FACE_EVERY (also checkpoint every N vsyncs
--      between the first confirm and the box opening, for the typing's start),
--      LEGAIA_SHOTS ("20,45,90,150,240,400"), LEGAIA_OUT_DIR, LEGAIA_MAX_VSYNC.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local PLAYER_PTR = 0x8007C364
local LIST_HEADS = { 0x8007C34C, 0x8007C350, 0x8007C354, 0x8007C358, 0x8007C35C, 0x8007C360 }
local OUT_DIR = env.getenv("LEGAIA_OUT_DIR", "captures/talk_to_npc")
local START   = env.getenv("LEGAIA_SSTATE", "")
local P90     = tonumber(env.getenv("LEGAIA_NPC_P90", "0")) or 0
local DIST    = tonumber(env.getenv("LEGAIA_TALK_DIST", "200")) or 200
local TRACE_V = tonumber(env.getenv("LEGAIA_TRACE_VSYNCS", "0")) or 0
local TRACE = nil
local FACE_EVERY = tonumber(env.getenv("LEGAIA_FACE_EVERY", "0")) or 0
local MAX_V   = tonumber(env.getenv("LEGAIA_MAX_VSYNC", "3000")) or 3000
local WAYPOINTS = {}
for c, r in string.gmatch(env.getenv("LEGAIA_WAYPOINTS", ""), "(%d+),(%d+)") do
    WAYPOINTS[#WAYPOINTS + 1] = { tonumber(c) * 128 + 64, tonumber(r) * 128 + 64 }
end
local wp_i = 1
local POKE = nil
do
    local px, pz = string.match(env.getenv("LEGAIA_POKE_POS", ""), "(%-?%d+),(%-?%d+)")
    if px then POKE = { tonumber(px), tonumber(pz) } end
end
local ROUTE, route_end = {}, 0
for a, b, btns in string.gmatch(env.getenv("LEGAIA_ROUTE", ""), "(%d+)%-(%d+):([%a+]+)") do
    local list = {}
    for n in string.gmatch(btns, "%a+") do list[#list + 1] = pad.BTN[n] end
    ROUTE[#ROUTE + 1] = { tonumber(a), tonumber(b), list }
    route_end = math.max(route_end, tonumber(b))
end
local SHOTS = {}
for t in string.gmatch(env.getenv("LEGAIA_SHOTS", "20,45,90,150,240,400"), "%d+") do
    SHOTS[#SHOTS + 1] = tonumber(t)
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/talk.log", "w")
local function log(s) PCSX.log("[talk] " .. s); if LOG then LOG:write(s .. "\n"); LOG:flush() end end
local function s16(a)
    local v = mem.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function pos(a) return s16(a + 0x14), s16(a + 0x18) end
local function find_npc(verbose)
    local walked = 0
    for _, h in ipairs(LIST_HEADS) do
        local a, n = mem.read_u32(h), 0
        while a and a ~= 0 and mem.in_ram(a) and n < 400 do
            if mem.read_u32(a + 0x90) == P90 then return a end
            a = mem.read_u32(a); n = n + 1; walked = walked + 1
        end
    end
    if verbose then
        local hs = {}
        for _, h in ipairs(LIST_HEADS) do hs[#hs + 1] = string.format("%08X", mem.read_u32(h) or 0) end
        log(string.format("walked %d actors, no +0x90 == %08X (heads %s) player %08X mode %d", walked, P90, table.concat(hs, " "), mem.read_u32(PLAYER_PTR) or 0, mem.read_u8(0x8007B83C) or -1))
    end
    return nil
end
local function checkpoint(tag)
    local path = string.format("%s/%s.rawsstate", OUT_DIR, tag)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(path, "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log(string.format("checkpoint %s ok=%s %s", tag, tostring(ok), tostring(err or "")))
end

-- Pad mapping: screen-up = world Z+, screen-right = world X+ (the town
-- cameras; LEGAIA_CAM_DEG rotates it). Not learnt online: a push into a
-- partial wall slides the player sideways, and a model fitted to slides
-- rotates itself away from the true mapping.
local SEED = {
    [pad.BTN.UP] = 0.5 * math.pi, [pad.BTN.DOWN] = -0.5 * math.pi,
    [pad.BTN.LEFT] = math.pi, [pad.BTN.RIGHT] = 0.0,
}
local cam = math.rad(tonumber(env.getenv("LEGAIA_CAM_DEG", "0")) or 0)
local cur_dir = nil
local function wrap(a) while a > math.pi do a = a - 2 * math.pi end; while a < -math.pi do a = a + 2 * math.pi end; return a end
local function best_dir(tx, tz)
    local want = math.atan2(tz, tx)
    local best, bd = 10, pad.BTN.UP
    for b, a in pairs(SEED) do
        local d = math.abs(wrap(want - (a + cam)))
        if d < best then best, bd = d, b end
    end
    return bd
end
local held = {}
local function hold(b) if not held[b] then pad.force(b); held[b] = true end end
local function release_all() for b in pairs(held) do pad.release(b) end; held = {} end

local v, loaded, npc, phase, last_xy, still, press_v = 0, false, nil, "WALK", nil, 0, nil
local shot_i = 1
local side_until, side_btn, side_flip = 0, nil, false
local blocked_shot = false
local function on_vsync()
    v = v + 1
    if not loaded then
        if v >= 2 then
            loaded = true
            log((sstate.load(START) and "resumed " or "FAILED to load ") .. START)
        end
        return
    end
    if v < 20 then return end
    if v <= route_end + 20 then
        local want = {}
        for _, r in ipairs(ROUTE) do
            if v - 20 >= r[1] and v - 20 <= r[2] then for _, b in ipairs(r[3]) do want[b] = true end end
        end
        for b in pairs(held) do if not want[b] then pad.release(b); held[b] = nil end end
        for b in pairs(want) do hold(b) end
        if v == route_end + 20 then
            local p0 = mem.read_u32(PLAYER_PTR)
            log(string.format("vsync %d: route done at (%d,%d)", v, pos(p0)))
        end
        return
    end
    local pl = mem.read_u32(PLAYER_PTR)
    if not pl or not mem.in_ram(pl) then return end
    if POKE then
        mem.write_u16(pl + 0x14, POKE[1] % 0x10000); mem.write_u16(pl + 0x18, POKE[2] % 0x10000)
        log(string.format("vsync %d: poked player to (%d,%d)", v, POKE[1], POKE[2]))
        POKE = nil
        return
    end
    npc = npc or find_npc(v % 60 == 0)
    if not npc then
        if v > 300 then log("npc never appeared; quitting"); PCSX.quit(1) end
        return
    end
    local px, pz = pos(pl)
    local nx, nz = pos(npc)
    local dx, dz = nx - px, nz - pz
    if phase == "WALK" and WAYPOINTS[wp_i] then
        local wx, wz = WAYPOINTS[wp_i][1] - px, WAYPOINTS[wp_i][2] - pz
        if math.abs(wx) <= 40 and math.abs(wz) <= 40 then
            log(string.format("vsync %d: waypoint %d reached at (%d,%d)", v, wp_i, px, pz))
            wp_i = wp_i + 1
            return
        end
        local moved = last_xy and (last_xy[1] ~= px or last_xy[2] ~= pz)
        if last_xy and not moved then still = still + 1 else still = 0 end
        last_xy = { px, pz }
        if v < side_until then
            if cur_dir ~= side_btn then release_all(); hold(side_btn); cur_dir = side_btn end
            return
        end
        if still >= 12 then
            side_flip = not side_flip
            side_btn = side_flip and best_dir(-wz, wx) or best_dir(wz, -wx)
            side_until = v + 16; still = 0
            log(string.format("vsync %d: blocked at (%d,%d) on the way to waypoint %d, sidestep", v, px, pz, wp_i))
            if not blocked_shot then blocked_shot = true; checkpoint("blocked") end
            return
        end
        local b = best_dir(wx, wz)
        if b ~= cur_dir then release_all(); hold(b); cur_dir = b end
        if v >= MAX_V then log("max vsync; quitting"); PCSX.quit(1) end
        if v % 30 == 0 then log(string.format("vsync %d wp %d player (%d,%d)", v, wp_i, px, pz)) end
        return
    end
    if phase == "WALK" then
        if v % 30 == 0 then log(string.format("vsync %d player (%d,%d) npc (%d,%d)", v, px, pz, nx, nz)) end
        local moved = last_xy and (last_xy[1] ~= px or last_xy[2] ~= pz)
        if last_xy and not moved then still = still + 1 else still = 0 end
        last_xy = { px, pz }
        local d = math.max(math.abs(dx), math.abs(dz))
        if v < side_until then
            if cur_dir ~= side_btn then release_all(); hold(side_btn); cur_dir = side_btn end
            return
        end
        if still >= 12 and d > DIST then
            side_flip = not side_flip
            side_btn = side_flip and best_dir(-dz, dx) or best_dir(dz, -dx)
            side_until = v + 24; still = 0
            log(string.format("vsync %d: blocked at (%d,%d), sidestep", v, px, pz))
            return
        end
        if d <= DIST then
            release_all()
            phase = "FACE"; press_v = nil
            log(string.format("vsync %d: stop at player (%d,%d) npc (%d,%d) still=%d", v, px, pz, nx, nz, still))
            return
        end
        local b = best_dir(dx, dz)
        if b ~= cur_dir then release_all(); hold(b); cur_dir = b end
    elseif phase == "FACE" then
        -- Face the NPC, then CROSS; retry every 30 vsyncs (re-facing first)
        -- until the field-control block's dialogue byte (*0x801C6EA4 +0x62)
        -- goes non-zero. Shots count from that vsync.
        local fc = mem.read_u32(0x801C6EA4)
        local dlg = fc and mem.in_ram(fc + 0x62) and mem.read_u8(fc + 0x62) or 0
        if dlg ~= 0 then
            release_all()
            log(string.format("vsync %d: conversation open, dialogue byte %d (player (%d,%d) npc (%d,%d))", v, dlg, px, pz, nx, nz))
            phase = "SHOOT"; press_v = v
            return
        end
        press_v = press_v or v
        if FACE_EVERY > 0 and (v - press_v) % FACE_EVERY == 0 and v > press_v then
            checkpoint(string.format("face_v%04d", v))
        end
        local t = (v - press_v) % 30
        if t == 0 then release_all(); cur_dir = nil; hold(best_dir(dx, dz))
        elseif t == 6 then release_all()
        elseif t == 12 then hold(pad.BTN.CROSS)
        elseif t == 18 then release_all() end
        if v - press_v > 600 then log("no conversation after 20 tries"); PCSX.quit(1) end
    elseif phase == "SHOOT" then
        local t = v - press_v
        if TRACE_V > 0 then
            if not TRACE then
                TRACE = io.open(OUT_DIR .. "/trace.csv", "w")
                TRACE:write("t,x,y,z\n")
            end
            TRACE:write(string.format("%d,%d,%d,%d\n", t, px, s16(pl + 0x16), pz))
            if t % 20 == 0 then hold(pad.BTN.CROSS) elseif t % 20 == 6 then release_all() end
            if t >= TRACE_V then TRACE:close(); log("trace done"); PCSX.quit(0) end
            return
        end
        if SHOTS[shot_i] and t == SHOTS[shot_i] then
            checkpoint(string.format("talk_t%03d", t))
            shot_i = shot_i + 1
            if not SHOTS[shot_i] then log("done"); PCSX.quit(0) end
        end
    end
    if v >= MAX_V then log("max vsync; quitting"); PCSX.quit(1) end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log(string.format("armed: npc p90=%08X dist=%d", P90, DIST))
