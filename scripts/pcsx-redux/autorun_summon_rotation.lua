-- autorun_summon_rotation.lua
--
-- Pin the summon-part mesh ROTATION source during a player Gimard
-- "Burning Attack" cast. The per-part orientation is built by a GTE
-- view-rotation that composes the camera Euler globals
-- (_DAT_8007B790 / _DAT_8007B792 / _DAT_8007B794, gated per-axis by a
-- node-flags word) with a per-part LOCAL Euler at a render node's
-- +0x8/+0xa/+0xc, via RotMatrixX/Y/Z (0x800461A4 / 0x8004629C /
-- 0x8004638C). The documented candidate builder is FUN_801F7088, but
-- the only static dumps of that address are the WORLD-MAP top-view
-- tile renderer (overlays alias the 0x801Fxxxx band), so it is unknown
-- whether the resident battle-summon overlay even has the rotation
-- code there. This probe resolves that:
--
--   1. Dumps the full 2 MiB main RAM so the resident overlay at
--      0x801F0000+ can be disassembled offline (capstone) to see what
--      0x801F7088 actually is during the cast.
--   2. Arms Exec breakpoints at TWO addresses and records every hit's
--      key registers + the camera Euler globals:
--        - 0x801F7088  : the documented rotation-builder candidate.
--        - 0x80043390  : the cluster-A primitive renderer the builder
--                        tail-calls. Its caller RA reveals the REAL
--                        summon renderer if 0x801F7088 is not it.
--
-- No write-watchpoint is armed: the node struct + exact rotation-field
-- offset are not yet known for this overlay, and guessing the watch
-- target risks watching garbage. Phase 2 arms a precise Write watch
-- once the dumped overlay is disassembled.
--
-- Usage (state 7 = Burning Attack performing; state 6 = Gimard visible):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate7 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_summon_rotation.lua \
--   LEGAIA_FRAMES=90 \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- Output (under captures/summon_rotation/<ts>/ unless LEGAIA_OUT_DIR set):
--   summon_ram.bin             full 2 MiB main RAM image
--   summon_rotation.csv        one row per Exec hit (bp, pc, ra, key regs, cam Euler)
--   summon_rotation.detail.txt first N full call contexts (32 GPRs + windows)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Exec-breakpoint targets.
local BP_BUILDER  = 0x801F7088 -- documented rotation-builder candidate
local BP_RENDERER = 0x80043390 -- cluster-A prim renderer it feeds

-- Camera Euler globals (cutscene-camera angle triple).
local CAM_X = 0x8007B790
local CAM_Y = 0x8007B792
local CAM_Z = 0x8007B794

local csv = probe.csv_open(probe.out_path("summon_rotation.csv"),
    "tick,bp,pc,ra,s0,s1,a0,a1,a2,camX,camY,camZ")
local detail_path = probe.out_path("summon_rotation.detail.txt")

local armed        = false
local dumped       = false
local g_elapsed    = 0
local hit_count    = 0
local detail_count = 0
local MAX_DETAIL   = 24
-- Per-bp hit caps so a per-frame per-part renderer doesn't drown the log.
local cap  = { [BP_BUILDER] = 64, [BP_RENDERER] = 64 }
local seen = { [BP_BUILDER] = 0, [BP_RENDERER] = 0 }

local function dump_ram()
    local bytes = probe.read_bytes(0x80000000, 2 * 1024 * 1024)
    if not bytes then
        PCSX.log("[summon-rot] read_bytes failed; skipping RAM dump")
        return
    end
    local f = assert(io.open(probe.out_path("summon_ram.bin"), "wb"))
    f:write(tostring(bytes))
    f:close()
    PCSX.log(string.format("[summon-rot] wrote %d bytes -> summon_ram.bin",
        #tostring(bytes)))
end

local function s16(addr)
    local v = probe.read_u16(addr) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function make_cb(bp_addr, name)
    return function()
        seen[bp_addr] = seen[bp_addr] + 1
        if seen[bp_addr] > cap[bp_addr] then return end
        local r = PCSX.getRegisters()
        local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
        local g = r.GPR.n
        local function rr(x) return bit.band(tonumber(g[x]), 0xFFFFFFFF) end
        csv:row("%d,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,%d,%d,%d",
            g_elapsed, name, pc, rr("ra"),
            rr("s0"), rr("s1"), rr("a0"), rr("a1"), rr("a2"),
            s16(CAM_X), s16(CAM_Y), s16(CAM_Z))
        hit_count = hit_count + 1
        if detail_count < MAX_DETAIL then
            detail_count = detail_count + 1
            probe.append_call_context(detail_path,
                probe.capture_call_context(string.format(
                    "%s hit #%d (global #%d) elapsed=%d camEuler=(%d,%d,%d)",
                    name, seen[bp_addr], hit_count, g_elapsed,
                    s16(CAM_X), s16(CAM_Y), s16(CAM_Z))))
        end
    end
end

local function arm()
    PCSX.log(string.format(
        "[summon-rot] cam Euler at load = (%d, %d, %d)",
        s16(CAM_X), s16(CAM_Y), s16(CAM_Z)))
    probe.arm_breakpoint(BP_BUILDER, "Exec", 4, "builder_801F7088",
        make_cb(BP_BUILDER, "builder"))
    probe.arm_breakpoint(BP_RENDERER, "Exec", 4, "renderer_80043390",
        make_cb(BP_RENDERER, "renderer"))
    PCSX.log(string.format(
        "[summon-rot] armed Exec BPs at 0x%08X (builder) + 0x%08X (renderer)",
        BP_BUILDER, BP_RENDERER))
    armed = true
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 90),
    snapshot_path  = probe.out_path("summon_rotation.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not dumped and elapsed >= 2 then
            dump_ram()
            dumped = true
        end
        if not armed and elapsed >= 3 then
            arm()
        end
    end,
    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "[summon-rot] done. builder hits=%d renderer hits=%d (recorded=%d)",
            seen[BP_BUILDER], seen[BP_RENDERER], hit_count))
    end,
})
