-- autorun_w4d_dome_decal_flag.lua
--
-- Is the backdrop-object-1 trim flag `_DAT_8007B64B` clear during a Muscle
-- Dome contest?
--
-- The SCUS battle-scene loader `FUN_800513F0` spawns two backdrop actors from
-- the stream's TMD and then reads this byte at `0x80051ABC`
-- (`lbu v1,-0x49b5(v1)`, v1 = 0x80080000). `0x80051ACC bne v1,zero,0x80051BB0`
-- takes the NON-zero arm past both trim loops, so a zero byte is what removes
-- object index 1 - the arena's 12-quad dust decal - from both actors' part
-- lists (docs/subsystems/minigame-muscle-dome.md). Its one writer on the disc
-- is the field battle handoff at `0x801DA0AC`
-- (`sb v0,-0x49b5(v1)`, `v0 = (s2[+8] >> 5) & 1`), which an arena contest is
-- documented as never running - an `inference` this probe upgrades or breaks.
--
-- Instrumentation:
--   * Exec BP at `0x80051ACC` - the branch that consumes the byte. `v1` is the
--     loaded value (the `lbu` two slots earlier has retired), so the row says
--     both the value and which arm the loader took.
--   * Exec BP at `0x800513F0` - loader entry, to bracket each battle load and
--     read the byte before the loader touches anything.
--   * A width-1 Write watch on `0x8007B64B` itself. The watch, not an exec BP
--     at `0x801DA0AC`, is the right instrument here: slot A is VA-aliased
--     across the field / battle / arena overlays, so an overlay-address exec BP
--     fires on whatever module happens to be resident, while a data watch
--     catches the store from any of them and hands back the writer `pc`/`ra`.
--
-- The state is a load-transition capture, so the probe presses Cross on a
-- cadence to walk the arena's dialogue into an actual contest.
--
-- Env vars:
--   LEGAIA_SSTATE   save-state (run_probe.sh --scenario minigame_muscle_dome_pcsx)
--   LEGAIA_FRAMES   capture vsyncs (default 2400)
--   LEGAIA_ADVANCE  0 = never press Cross (default 1)
--   LEGAIA_OUT_DIR  output directory
--
-- Outputs: w4d_dome_decal.csv, w4d_dome_decal.log, w4d_dome_decal.detail.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE  = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 2400)
local ADVANCE = probe.getenv_num("LEGAIA_ADVANCE", 1)

local OUT_LOG    = probe.out_path("w4d_dome_decal.log")
local OUT_CSV    = probe.out_path("w4d_dome_decal.csv")
local OUT_DETAIL = probe.out_path("w4d_dome_decal.detail.txt")

local FLAG_VA    = 0x8007B64B   -- _DAT_8007B64B, the object-1 trim gate
local LOADER_IN  = 0x800513F0   -- FUN_800513F0 battle scene loader
local LOADER_TST = 0x80051ACC   -- bne v1,zero,0x80051BB0 - v1 = the flag byte
local GAME_MODE  = 0x8007B83C
local SCENE_NAME = 0x8007050C
local CTX_PTR    = 0x8007BD24
local BACKDROP   = 0x8007B864   -- _DAT_8007B864, the backdrop TMD buffer ptr

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[w4d_dome] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end

local function stored_value()
    local r    = PCSX.getRegisters()
    local pc   = n32(r.pc)
    local insn = probe.read_u32(pc)
    if insn == nil then return nil, pc end
    local op = bit.rshift(n32(insn), 26)
    local rt = bit.band(bit.rshift(n32(insn), 16), 0x1F)
    local v  = n32(r.GPR.r[rt])
    if op == 0x28 then return bit.band(v, 0xFF), pc
    elseif op == 0x29 then return bit.band(v, 0xFFFF), pc
    elseif op == 0x2B then return v, pc end
    return nil, pc
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local csv = nil
local g_elapsed = 0
local last_flag, last_mode = nil, nil
local n_loads, n_tests, n_writes = 0, 0, 0
local cross_held = false

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV,
            "tick,event,flag,pc,ra,mode,scene,note")
        ctx.tests = {}
        probe.write_manifest("autorun_w4d_dome_decal_flag.lua", {
            sstate = SSTATE, frames = FRAMES, advance = ADVANCE,
            core = probe.getenv("LEGAIA_CORE", "?"),
        })

        probe.arm_breakpoint(LOADER_IN, "Exec", 4, "battle_loader", function()
            n_loads = n_loads + 1
            local r = PCSX.getRegisters()
            local f = probe.read_u8(FLAG_VA) or 0
            logf("LOADER ENTRY #%d vsync %d: 0x8007B64B = 0x%02X (scene=%s mode=0x%02X ra=0x%08X)",
                n_loads, g_elapsed, f, scene_name(),
                probe.read_u8(GAME_MODE) or 0, n32(r.GPR.n.ra))
            csv:row("%d,loader_entry,0x%02X,0x%08X,0x%08X,0x%02X,%s,#%d",
                g_elapsed, f, n32(r.pc), n32(r.GPR.n.ra),
                probe.read_u8(GAME_MODE) or 0, scene_name(), n_loads)
        end)

        -- v1 holds the byte the loader just loaded; the branch about to run
        -- takes the non-zero arm (skip the trim) or falls through (trim).
        probe.arm_breakpoint(LOADER_TST, "Exec", 4, "decal_test", function()
            n_tests = n_tests + 1
            local r  = PCSX.getRegisters()
            local v1 = bit.band(n32(r.GPR.n.v1), 0xFF)
            local mem_v = probe.read_u8(FLAG_VA) or 0
            local arm = (v1 ~= 0) and "keep_object1" or "trim_object1"
            logf("DECAL TEST #%d vsync %d: v1=0x%02X mem=0x%02X -> %s (backdrop_tmd=0x%08X scene=%s)",
                n_tests, g_elapsed, v1, mem_v, arm,
                probe.read_u32(BACKDROP) or 0, scene_name())
            csv:row("%d,decal_test,0x%02X,0x%08X,0x%08X,0x%02X,%s,%s",
                g_elapsed, v1, n32(r.pc), n32(r.GPR.n.ra),
                probe.read_u8(GAME_MODE) or 0, scene_name(), arm)
            ctx.tests[arm] = (ctx.tests[arm] or 0) + 1
            if n_tests <= 8 then
                probe.append_call_context(OUT_DETAIL,
                    probe.capture_call_context(string.format(
                        "decal test #%d flag=0x%02X arm=%s vsync=%d",
                        n_tests, v1, arm, g_elapsed)))
            end
        end)

        probe.arm_breakpoint(FLAG_VA, "Write", 1, "flag_write", function()
            n_writes = n_writes + 1
            local r = PCSX.getRegisters()
            local v, pc = stored_value()
            logf("FLAG WRITE #%d vsync %d: 0x%02X -> 0x%02X pc=0x%08X ra=0x%08X scene=%s mode=0x%02X",
                n_writes, g_elapsed, probe.read_u8(FLAG_VA) or 0, v or 0, pc,
                n32(r.GPR.n.ra), scene_name(), probe.read_u8(GAME_MODE) or 0)
            csv:row("%d,flag_write,0x%02X,0x%08X,0x%08X,0x%02X,%s,prev=0x%02X",
                g_elapsed, v or 0, pc, n32(r.GPR.n.ra),
                probe.read_u8(GAME_MODE) or 0, scene_name(),
                probe.read_u8(FLAG_VA) or 0)
            if n_writes <= 8 then
                probe.append_call_context(OUT_DETAIL,
                    probe.capture_call_context(string.format(
                        "flag write #%d new=0x%02X vsync=%d", n_writes, v or 0, g_elapsed)))
            end
        end)

        return {
            { addr = LOADER_IN,  name = "FUN_800513F0 battle scene loader" },
            { addr = LOADER_TST, name = "0x80051ACC object-1 trim test" },
            { addr = FLAG_VA,    name = "_DAT_8007B64B write" },
        }
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        local flag = probe.read_u8(FLAG_VA) or 0
        local mode = probe.read_u8(GAME_MODE) or 0
        if flag ~= last_flag then
            logf("flag 0x%02X -> 0x%02X at vsync %d (mode 0x%02X scene=%s)",
                last_flag or 0, flag, elapsed, mode, scene_name())
            csv:row("%d,flag_poll,0x%02X,,,0x%02X,%s,prev=0x%02X",
                elapsed, flag, mode, scene_name(), last_flag or 0)
            last_flag = flag
        end
        if mode ~= last_mode then
            logf("MODE 0x%02X -> 0x%02X at vsync %d (scene=%s flag=0x%02X ctx=0x%08X)",
                last_mode or 0, mode, elapsed, scene_name(), flag,
                probe.read_u32(CTX_PTR) or 0)
            csv:row("%d,mode,0x%02X,,,0x%02X,%s,prev=0x%02X",
                elapsed, flag, mode, scene_name(), last_mode or 0)
            last_mode = mode
        end

        -- Walk the arena dialogue forward: 6 frames held, 18 released.
        if ADVANCE ~= 0 then
            local phase = elapsed % 24
            if phase == 0 and not cross_held then
                probe.pad_force(probe.BTN.CROSS); cross_held = true
            elseif phase == 6 and cross_held then
                probe.pad_release(probe.BTN.CROSS); cross_held = false
            end
        end

        if (elapsed % 180) == 0 then
            logf("vsync %4d mode=0x%02X scene=%s flag=0x%02X loads=%d tests=%d writes=%d",
                elapsed, mode, scene_name(), flag, n_loads, n_tests, n_writes)
        end
    end,

    on_done = function(ctx)
        if cross_held then probe.pad_release(probe.BTN.CROSS) end
        logf("=== summary: loader entries=%d decal tests=%d flag writes=%d ===",
            n_loads, n_tests, n_writes)
        for arm, n in pairs(ctx.tests) do logf("  test arm %s x%d", arm, n) end
        logf("final 0x8007B64B = 0x%02X", probe.read_u8(FLAG_VA) or 0)
        if csv then csv:close() end
        local f = io.open(OUT_LOG, "w")
        if f then
            f:write(table.concat(lines, "\n") .. "\n")
            f:close()
            PCSX.log("[w4d_dome] wrote " .. OUT_LOG)
        end
    end,
})
