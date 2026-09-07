-- autorun_uru_exit_probe.lua
--
-- Drive a field-run save state toward a scene's walk-on exit band and record
-- what fires. Built for the Uru Mais chain (`uru`, `urudre1..3`) and `jouine`,
-- whose exits are partition-2 records reached only through the SECOND trigger
-- table - the scene's one-sector `.PCH` sidecar, staged past the `.MAP` and
-- searched when the map's own table misses. Those rows sort after the map's, so
-- a tile sweep capped at the first N tiles never gets to them.
--
-- The probe is direction-agnostic on purpose: the pad is camera-relative, so
-- it first sweeps the four D-pad directions for LEGAIA_SWEEP vsyncs each,
-- measures the resulting tile delta, then holds whichever direction moves the
-- player toward LEGAIA_TARGET_X / LEGAIA_TARGET_Z (tile units) until the scene
-- changes or the capture window ends. Every phase's start/end tile is logged,
-- so a run that never reaches the band says where it stalled.
--
-- Instrumentation (all SCUS-resident, so no overlay addressing is involved):
--   * FUN_8003BDE0(x, z, record, gate) - the walk-on record spawner. Each hit
--     logs the tile, the partition-2 record index and the gate, which is the
--     trigger -> record join observed live rather than decoded.
--   * FUN_8001FD44(name_ptr) - the scene-change packet. Logs the destination
--     name it is handed plus the caller `ra`, which is what attributes the
--     transition to a script op rather than to a menu / world-map path.
--   * FUN_801DE840 hits with op `0x3F` are NOT traced here; the per-op VM
--     breakpoint costs more than this probe needs. Use
--     `autorun_door_dispatch_trace.lua` when the question is the VM's route
--     into the op rather than whether the door exists.
--
-- Env vars:
--   LEGAIA_SSTATE      save-state path (or use run_probe.sh --scenario)
--   LEGAIA_FRAMES      capture vsyncs (default 900)
--   LEGAIA_SWEEP       vsyncs per probing direction (default 45)
--   LEGAIA_TARGET_X    target tile X (default -1 = ignore X)
--   LEGAIA_TARGET_Z    target tile Z (default -1 = ignore Z)
--   LEGAIA_OUT_DIR     output directory (run_probe.sh sets this)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local SWEEP    = probe.getenv_num("LEGAIA_SWEEP", 45)
local TARGET_X = probe.getenv_num("LEGAIA_TARGET_X", -1)
local TARGET_Z = probe.getenv_num("LEGAIA_TARGET_Z", -1)
local OUT_PATH = probe.out_path("uru_exit_probe.log")

-- SCUS globals. Scene name + game mode are the transition witnesses; the
-- player pointer's +0x14 / +0x18 are the world coords the tile derives from
-- (tile = coord >> 7, `FUN_801DBEC4`).
local SCENE_NAME_VA = 0x8007050C
local GAME_MODE_VA  = 0x8007B83C
local PLAYER_PTR_VA = 0x8007C364
local FMV_ID_VA     = 0x8007BA78   -- _DAT_8007BA78, the s16 the 4C E2 op writes
local SPAWN_FN      = 0x8003BDE0   -- FUN_8003BDE0(x, z, record, gate)
local SCENE_CHG_FN  = 0x8001FD44   -- FUN_8001FD44(name_ptr)

local DIRS = {
    { name = "UP",    btn = probe.BTN.UP },
    { name = "RIGHT", btn = probe.BTN.RIGHT },
    { name = "DOWN",  btn = probe.BTN.DOWN },
    { name = "LEFT",  btn = probe.BTN.LEFT },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[uru_exit] " .. s)
end

local function read_cstr(addr, maxlen)
    local out = {}
    for i = 0, (maxlen or 15) do
        local b = probe.read_u8(addr + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function scene_name() return read_cstr(SCENE_NAME_VA, 15) end

-- NB the pointer sanity check is a plain range test, NOT
-- `bit.band(p, 0xFFE00000) == 0x80000000`: LuaJIT's bit.band returns a SIGNED
-- 32-bit result, so that comparison is false for every KSEG0 pointer and the
-- probe silently reads no position at all.
local function player_pos()
    local p = probe.read_u32(PLAYER_PTR_VA)
    if p == nil or p < 0x80000000 or p >= 0x80200000 then return nil end
    local x = probe.read_u16(p + 0x14)
    local z = probe.read_u16(p + 0x18)
    if x == nil or z == nil then return nil end
    if x >= 0x8000 then x = x - 0x10000 end
    if z >= 0x8000 then z = z - 0x10000 end
    return x, z
end

-- Phase plan: 4 sweep phases (one per direction), then one hold phase.
local phase, phase_start, held_btn = 0, 0, nil
local last_mode = nil
local sweep_result = {}
local start_scene, start_x, start_z = nil, nil, nil

local function hold(btn)
    if held_btn == btn then return end
    if held_btn then probe.pad.release(held_btn) end
    held_btn = btn
    if btn then probe.pad.force(btn) end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        ctx.spawns = {}
        ctx.changes = {}
        probe.arm_breakpoint(SPAWN_FN, "Exec", 4, "walkon_spawn", function()
            local r = PCSX.getRegisters()
            local x = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFF)
            local z = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFF)
            local rec = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFF)
            local gate = bit.band(tonumber(r.GPR.n.a3) or 0, 0xFF)
            local key = string.format("(%d,%d) rec=%d gate=%d", x, z, rec, gate)
            if not ctx.spawns[key] then
                ctx.spawns[key] = 0
                logf("SPAWN %s  scene=%s", key, scene_name())
            end
            ctx.spawns[key] = ctx.spawns[key] + 1
        end)
        probe.arm_breakpoint(SCENE_CHG_FN, "Exec", 4, "scene_change", function()
            local r = PCSX.getRegisters()
            local a0 = tonumber(r.GPR.n.a0) or 0
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local dest = (a0 >= 0x80000000) and read_cstr(a0, 15) or "?"
            ctx.changes[#ctx.changes + 1] = { dest = dest, ra = ra }
            logf("SCENE_CHANGE dest='%s' from=%s ra=0x%08X", dest, scene_name(), ra)
        end)
        return {
            { addr = SPAWN_FN, name = "FUN_8003BDE0 walk-on spawn" },
            { addr = SCENE_CHG_FN, name = "FUN_8001FD44 scene-change packet" },
        }
    end,

    on_capture = function(ctx, elapsed)
        local x, z = player_pos()
        if x == nil then
            -- Say so rather than looping silently: a probe that reads no
            -- player position looks identical to one whose walk did nothing.
            if (elapsed % 60) == 0 then
                logf("vsync %4d NO PLAYER POS (ptr=0x%08X scene=%s mode=0x%02X)",
                    elapsed, probe.read_u32(PLAYER_PTR_VA) or 0, scene_name(),
                    probe.read_u8(GAME_MODE_VA) or 0)
            end
            return
        end
        if start_scene == nil then
            start_scene, start_x, start_z = scene_name(), x, z
            phase, phase_start = 1, elapsed
            logf("start scene=%s pos=(%d,%d) tile=(%d,%d) target_tile=(%d,%d)",
                start_scene, x, z, bit.rshift(x, 7), bit.rshift(z, 7), TARGET_X, TARGET_Z)
            hold(DIRS[1].btn)
            logf("phase 1: hold %s", DIRS[1].name)
            return
        end

        -- The other exit shape these scenes use is the FMV hand-off
        -- (`0x4C 0xE2 <fmv_id>`): game mode goes to 0x1A (StrInit) with
        -- `_DAT_8007BA78` set, and the post-play dispatch picks the next scene.
        -- Report every mode transition so that route is visible too.
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        if mode ~= last_mode then
            logf("MODE 0x%02X -> 0x%02X at vsync %d (fmv_id=%d scene=%s)",
                last_mode or 0, mode, elapsed,
                probe.read_u16(FMV_ID_VA) or 0, scene_name())
            last_mode = mode
        end

        local now = scene_name()
        if now ~= start_scene and now ~= "" then
            logf("SCENE LEFT: %s -> %s at vsync %d (mode 0x%02X)",
                start_scene, now, elapsed, probe.read_u8(GAME_MODE_VA) or 0)
            hold(nil)
            ctx.request_quit = true
            return
        end

        if phase >= 1 and phase <= #DIRS then
            if elapsed - phase_start >= SWEEP then
                local d = DIRS[phase]
                sweep_result[phase] = { name = d.name, x = x, z = z }
                logf("phase %d (%s) end pos=(%d,%d) tile=(%d,%d)",
                    phase, d.name, x, z, bit.rshift(x, 7), bit.rshift(z, 7))
                phase, phase_start = phase + 1, elapsed
                if phase <= #DIRS then
                    hold(DIRS[phase].btn)
                    logf("phase %d: hold %s", phase, DIRS[phase].name)
                else
                    -- Choose the direction whose sweep moved the player
                    -- closest to the target tile; ties keep the earlier one.
                    local best, best_cost = nil, nil
                    local prev_x, prev_z = start_x, start_z
                    for i, s in ipairs(sweep_result) do
                        local cost = 0
                        if TARGET_X >= 0 then cost = cost + math.abs(bit.rshift(s.x, 7) - TARGET_X) end
                        if TARGET_Z >= 0 then cost = cost + math.abs(bit.rshift(s.z, 7) - TARGET_Z) end
                        logf("  sweep %s -> tile=(%d,%d) cost=%d (from (%d,%d))",
                            s.name, bit.rshift(s.x, 7), bit.rshift(s.z, 7), cost,
                            bit.rshift(prev_x, 7), bit.rshift(prev_z, 7))
                        prev_x, prev_z = s.x, s.z
                        if best_cost == nil or cost < best_cost then best, best_cost = i, cost end
                    end
                    if best then
                        hold(DIRS[best].btn)
                        logf("hold phase: %s (cost %d) for the rest of the capture",
                            DIRS[best].name, best_cost)
                    end
                end
            end
        end

        if (elapsed % 30) == 0 then
            logf("vsync %4d scene=%s tile=(%d,%d) pos=(%d,%d) mode=0x%02X",
                elapsed, now, bit.rshift(x, 7), bit.rshift(z, 7), x, z,
                probe.read_u8(GAME_MODE_VA) or 0)
        end
    end,

    on_done = function(ctx)
        hold(nil)
        logf("=== spawns seen ===")
        local keys = {}
        for k in pairs(ctx.spawns) do keys[#keys + 1] = k end
        table.sort(keys)
        for _, k in ipairs(keys) do logf("  %s  x%d", k, ctx.spawns[k]) end
        logf("=== scene changes: %d ===", #ctx.changes)
        for _, c in ipairs(ctx.changes) do
            logf("  -> '%s' (ra=0x%08X)", c.dest, c.ra)
        end
        local f = io.open(OUT_PATH, "w")
        if f then
            f:write(table.concat(lines, "\n") .. "\n")
            f:close()
            PCSX.log("[uru_exit] wrote " .. OUT_PATH)
        end
    end,
})
