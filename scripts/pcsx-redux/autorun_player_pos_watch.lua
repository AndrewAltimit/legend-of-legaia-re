-- autorun_player_pos_watch.lua
--
-- Write-watchpoint on the player actor's world-position fields to pin
-- the town/field free-movement locomotion integrator: *which function
-- writes the player position in response to the held d-pad?*
--
-- The player actor pointer is the global at 0x8007C364; the live actor
-- struct stores world X at +0x14 and world Z at +0x18 (both s16; +0x16
-- between them is the facing angle). Those offsets are confirmed by the
-- field camera code (FUN_801dbec4 reads `(player[+0x14] - 0x40) >> 7`
-- as the player tile). The camera cluster only READS them; this probe
-- finds the WRITER.
--
-- The watch target is a runtime pointer deref (`*(0x8007C364) + 0x14`),
-- so it is armed lazily in on_capture after the save state has loaded
-- (probe.run's on_arm fires pre-load). Because the scene's camera
-- facing is unknown, the probe injects each d-pad direction in turn so
-- at least one produces a world-position write regardless of facing.
--
-- Usage (pick a save parked standing in a walkable town):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate6 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_player_pos_watch.lua \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- Output: player_pos_watch.csv (tick, axis, write_addr, pc, ra, new_val)
-- + player_pos_watch.detail.txt (call-context for the first writes).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local PLAYER_PTR_ADDR = 0x8007C364
local GAME_MODE_ADDR  = 0x8007B83C
local SCENE_NAME_ADDR = 0x80084548
local OFF_X = 0x14
local OFF_Z = 0x18

local csv = probe.csv_open(probe.out_path("player_pos_watch.csv"),
    "tick,axis,write_addr,pc,ra,new_val")
local detail_path = probe.out_path("player_pos_watch.detail.txt")

local armed       = false
local hit_count   = 0
local MAX_DETAIL  = 16
local player_ptr  = nil
local g_elapsed   = 0
local cur_dir     = "none"

-- Direction schedule: {start_elapsed, button, label}. Each entry
-- releases the previous direction and holds the next.
local DIRS = {
    { 5,   probe.BTN.UP,    "UP" },
    { 75,  probe.BTN.RIGHT, "RIGHT" },
    { 145, probe.BTN.DOWN,  "DOWN" },
    { 215, probe.BTN.LEFT,  "LEFT" },
}

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

local function release_all()
    for _, d in ipairs(DIRS) do probe.pad_release(d[2]) end
end

local function arm_watch(ctx)
    player_ptr = probe.read_u32(PLAYER_PTR_ADDR)
    local gm = probe.read_u8(GAME_MODE_ADDR)
    PCSX.log(string.format(
        "[pos-watch] game_mode=0x%02X scene=%s player_ptr=0x%08X",
        gm or 0xFF, read_scene_name(), player_ptr or 0))
    if not player_ptr or not probe.in_ram(player_ptr + OFF_Z, 2) then
        PCSX.log("[pos-watch] player_ptr invalid - not a field state. aborting.")
        ctx.request_quit = true
        return
    end
    PCSX.log(string.format("[pos-watch] initial X=%d Z=%d",
        probe.read_u16(player_ptr + OFF_X) or 0,
        probe.read_u16(player_ptr + OFF_Z) or 0))

    local function make_cb(axis, addr)
        return function()
            local r  = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
            csv:row("%d,%s,0x%08X,0x%08X,0x%08X,%d",
                g_elapsed, axis, addr, pc, ra, probe.read_u16(addr) or 0)
            hit_count = hit_count + 1
            if hit_count <= MAX_DETAIL then
                probe.append_call_context(detail_path,
                    probe.capture_call_context(string.format(
                        "%s write #%d addr=0x%08X dir=%s elapsed=%d",
                        axis, hit_count, addr, cur_dir, g_elapsed)))
            end
        end
    end
    probe.arm_breakpoint(player_ptr + OFF_X, "Write", 2, "playerX",
        make_cb("X", player_ptr + OFF_X))
    probe.arm_breakpoint(player_ptr + OFF_Z, "Write", 2, "playerZ",
        make_cb("Z", player_ptr + OFF_Z))
    PCSX.log(string.format(
        "[pos-watch] armed Write watch X=0x%08X Z=0x%08X",
        player_ptr + OFF_X, player_ptr + OFF_Z))
    armed = true
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate6"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 300),
    snapshot_path  = probe.out_path("player_pos_watch.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed then
            if elapsed >= 2 then arm_watch(ctx) end
            return
        end
        for _, d in ipairs(DIRS) do
            if elapsed == d[1] then
                release_all()
                probe.pad_force(d[2])
                cur_dir = d[3]
                PCSX.log(string.format(
                    "[pos-watch] elapsed=%d hold %s  X=%d Z=%d",
                    elapsed, cur_dir,
                    probe.read_u16(player_ptr + OFF_X) or 0,
                    probe.read_u16(player_ptr + OFF_Z) or 0))
                break
            end
        end
        if elapsed >= 280 then
            release_all()
            ctx.request_quit = true
        end
    end,
    on_done = function()
        release_all()
        csv:close()
        PCSX.log(string.format(
            "[pos-watch] total position writes captured: %d", hit_count))
    end,
})
