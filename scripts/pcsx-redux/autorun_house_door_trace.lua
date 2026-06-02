-- autorun_house_door_trace.lua
--
-- Pin the INTRA-TOWN (house / interior) door mechanism. A mednafen pre/post
-- pair proved entering Mei's house in Rim Elm is an INTRA-SCENE REPOSITION, not
-- a scene change: the scene-name buffer stays the same while the player struct
-- (0x8007C364 deref) position jumps to the interior sub-area. This probe finds
-- WHO performs that jump.
--
-- It write-watchpoints the player world position (player[+0x14] X, [+0x18] Z),
-- holds Up to walk into the door, and flags the write whose delta is LARGE (the
-- reposition warp, vs the small per-frame locomotion steps) with the writer PC +
-- a full call-context snapshot. It also logs the scene name + game_mode each
-- frame to confirm the scene never reloads (no FUN_8003aeb0).
--
-- Run (door-outside PCSX state):
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario mei_house_door_pcsx \
--     --lua scripts/pcsx-redux/autorun_house_door_trace.lua --frames 240

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local PLAYER_PTR_ADDR = 0x8007C364
local GAME_MODE_ADDR  = 0x8007B83C
local SCENE_NAME_ADDR = 0x80084548
local OFF_X = 0x14
local OFF_Z = 0x18
-- A walking step is 2 units; a reposition jumps far. Flag deltas over this.
local BIG_DELTA = 0x40
-- The known interior target (town01 PCSX variant): X=12480, Z=6976. Snapshot
-- the writer the moment the position LANDS on the interior (ignores the
-- ledge-hop/look-ahead scratch writes that confused the big-delta heuristic).
-- Override via env if a different variant lands elsewhere.
local TARGET_X = probe.getenv_num("LEGAIA_TARGET_X", 12480)
local TARGET_Z = probe.getenv_num("LEGAIA_TARGET_Z", 6976)

local csv = probe.csv_open(probe.out_path("house_door_trace.csv"),
    "tick,axis,pc,ra,old,new,delta,scene")
local detail_path = probe.out_path("house_door_trace.detail.txt")

local armed = false
local player_ptr = nil
local g_elapsed = 0
local last = { [OFF_X] = nil, [OFF_Z] = nil }
local big_hits = 0
local total_hits = 0

local function read_scene_name()
    local b = probe.read_bytes(SCENE_NAME_ADDR, 16)
    if not b then return "?" end
    local s, out = tostring(b), {}
    for i = 1, #s do
        local c = s:byte(i)
        if c == 0 then break end
        if c >= 32 and c < 127 then out[#out + 1] = string.char(c) end
    end
    return table.concat(out)
end

-- s16 read (positions are signed).
local function rd(addr)
    local v = probe.read_u16(addr) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function arm_watch(ctx)
    player_ptr = probe.read_u32(PLAYER_PTR_ADDR)
    PCSX.log(string.format("[house] game_mode=0x%02X scene=%s player_ptr=0x%08X",
        probe.read_u8(GAME_MODE_ADDR) or 0xFF, read_scene_name(), player_ptr or 0))
    if not player_ptr or not probe.in_ram(player_ptr + OFF_Z, 2) then
        PCSX.log("[house] player_ptr invalid - not a field state. aborting.")
        ctx.request_quit = true
        return
    end
    last[OFF_X] = rd(player_ptr + OFF_X)
    last[OFF_Z] = rd(player_ptr + OFF_Z)
    PCSX.log(string.format("[house] initial X=%d Z=%d", last[OFF_X], last[OFF_Z]))

    local function make_cb(axis, off)
        return function()
            local addr = player_ptr + off
            local r = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
            local new = rd(addr)
            local old = last[off] or new
            local delta = new - old
            last[off] = new
            total_hits = total_hits + 1
            local scene = read_scene_name()
            csv:row("%d,%s,0x%08X,0x%08X,%d,%d,%d,%s",
                g_elapsed, axis, pc, ra, old, new, delta, scene)
            local on_target = (axis == "X" and new == TARGET_X) or (axis == "Z" and new == TARGET_Z)
            if on_target or math.abs(delta) >= BIG_DELTA then
                big_hits = big_hits + 1
                PCSX.log(string.format(
                    "[house] %s%s @elapsed=%d pc=0x%08X ra=0x%08X %d -> %d (delta %d) scene=%s",
                    on_target and "TARGET-LAND " or "BIG ", axis, g_elapsed, pc, ra, old, new, delta, scene))
                -- Prioritise target-landing snapshots (the true reposition writer).
                if on_target or big_hits <= 8 then
                    probe.append_call_context(detail_path,
                        probe.capture_call_context(string.format(
                            "%s%s reposition #%d @elapsed=%d pc=0x%08X %d->%d scene=%s",
                            on_target and "TARGET-LAND " or "BIG ", axis, big_hits, g_elapsed, pc, old, new, scene)))
                end
            end
        end
    end
    probe.arm_breakpoint(player_ptr + OFF_X, "Write", 2, "playerX", make_cb("X", OFF_X))
    probe.arm_breakpoint(player_ptr + OFF_Z, "Write", 2, "playerZ", make_cb("Z", OFF_Z))

    -- The door trigger sets the global move-delta vector _DAT_8007bde0 (X) /
    -- _DAT_8007bde4 (Z) to the FULL displacement-to-interior (normally +-8).
    -- Catch the writer of a large value -- that's the door record consumer.
    local DELTA_DIR = { 0x8007bde0, 0x8007bde4 }
    local dbg = 0
    for _, daddr in ipairs(DELTA_DIR) do
        probe.arm_breakpoint(daddr, "Write", 2, "delta", function()
            local v = rd(daddr)
            if math.abs(v) > 0x40 then
                dbg = dbg + 1
                local r = PCSX.getRegisters()
                local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
                PCSX.log(string.format("[house] BIG DELTA write @elapsed=%d addr=0x%08X pc=0x%08X val=%d",
                    g_elapsed, daddr, pc, v))
                if dbg <= 6 then
                    probe.append_call_context(detail_path,
                        probe.capture_call_context(string.format(
                            "DELTA write #%d addr=0x%08X val=%d @elapsed=%d", dbg, daddr, v, g_elapsed)))
                end
            end
        end)
    end
    PCSX.log("[house] armed Write watch on player X/Z + delta vector; holding UP")
    armed = true
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 240),
    snapshot_path  = probe.out_path("house_door_trace.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed then
            if elapsed >= 2 then arm_watch(ctx) end
            return
        end
        if elapsed == 3 then probe.pad_force(probe.BTN.UP) end
        if elapsed >= 230 then
            probe.pad_release(probe.BTN.UP)
            ctx.request_quit = true
        end
    end,
    on_done = function()
        probe.pad_release(probe.BTN.UP)
        csv:close()
        PCSX.log(string.format("[house] done: %d position writes, %d big jumps -> %s",
            total_hits, big_hits, detail_path))
    end,
})
