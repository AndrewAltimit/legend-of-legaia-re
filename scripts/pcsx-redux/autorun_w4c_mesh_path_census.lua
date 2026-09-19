-- autorun_w4c_mesh_path_census.lua
--
-- Which mode enters the TMD renderer `FUN_8002735C`, and what decides it?
--
-- A field / world-map capture records zero entries into `FUN_8002735C` while
-- the frames draw thousands of polygons, so either the mesh path belongs to a
-- mode those captures never entered or something upstream selects a different
-- emitter. The disassembly says the second: `FUN_8002735C` has exactly three
-- `jal` sites in `SCUS_942.54` (plus one in the dev image `PROT 0973`), and
-- each sits in the far arm of a two-way test on the drawn actor's `+0x42`
-- halfword:
--
--   FUN_8001ada4  0x8001B454  bne  $s7,$zero,0x8001B570   ($s7 = lh 0x42($s0))
--   FUN_8001b964  0x8001BC64  bne  $s7,$zero,0x8001BD80   ($s7 = lhu 0x42($s0))
--   FUN_80048a08  0x80048EA4  bne  $v0,$zero,0x80048FD0   ($v0 = lh 0x42($s0))
--
-- The near arm calls `FUN_80029888` (the light-source sibling) when the actor's
-- `+0x7A` is non-zero and `FUN_80043390` (the per-prim dispatcher) when it is
-- zero. So the question "which mode enters `FUN_8002735C`" is really "which
-- mode draws an actor whose `+0x42` is non-zero", and the three renderers are
-- alternatives on one bracket rather than a renderer and its fallbacks.
--
-- This probe measures that directly: it taps the three renderer entries (count
-- only - they are hot) and the three selector branches (register read, so the
-- `+0x42` distribution over every actor draw is recorded), and buckets both by
-- the live game mode. Run it on a battle state, a cutscene state and a field
-- state and the counts say which mode, if any, ever takes the far arm.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario s5_tetsu_battle \
--       --lua scripts/pcsx-redux/autorun_w4c_mesh_path_census.lua \
--       --frames 240
--
-- Env vars:
--   LEGAIA_SSTATE     save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES     capture vsyncs (default 240)
--   LEGAIA_SEL_ROWS   selector rows written to the CSV (default 4000)
--
-- Outputs: mesh_path_census.csv (selector hits), .log (summary)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 240)
local SEL_ROWS = probe.getenv_num("LEGAIA_SEL_ROWS", 4000)

local OUT_CSV = probe.out_path("mesh_path_census.csv")
local OUT_LOG = probe.out_path("mesh_path_census.log")

local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C

-- Exec taps. `want` is the word that must sit at the address, so a mis-based
-- or paged-out tap reports MISMATCH instead of silence. All six are in
-- `SCUS_942.54`, which is resident in every mode.
local TAPS = {
    { addr = 0x8002735C, want = 0x27BDFEA8, kind = "tmd_draw",  hot = true,
      name = "FUN_8002735C entry (the TMD renderer)" },
    { addr = 0x80029888, want = 0x27BDFF60, kind = "lit_draw",  hot = true,
      name = "FUN_80029888 entry (light-source sibling)" },
    { addr = 0x80043390, want = 0x3C0A1F80, kind = "prim_disp", hot = true,
      name = "FUN_80043390 entry (per-prim dispatcher)" },
    { addr = 0x8001B454, want = 0x16E00046, kind = "sel_ada4",  reg = "s7",
      name = "FUN_8001ada4 selector bne $s7" },
    { addr = 0x8001BC64, want = 0x16E00046, kind = "sel_b964",  reg = "s7",
      name = "FUN_8001b964 selector bne $s7" },
    { addr = 0x80048EA4, want = 0x1440004A, kind = "sel_8a08",  reg = "v0",
      name = "FUN_80048a08 selector bne $v0" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[meshpath] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end
local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
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

local csv
local g_elapsed = 0
local counts = {}          -- kind -> hits
local sel_nonzero = {}     -- kind -> far-arm (TMD) hits
local sel_zero = {}        -- kind -> near-arm hits
local by_mode = {}         -- mode -> { tmd, lit, prim, sel, sel_nz }
local sel_rows = 0
local modes_seen = {}
local last_mode = nil
local f42_values = {}      -- distinct +0x42 values observed, with counts

local function mode_bucket()
    local m = probe.read_u8(GAME_MODE) or 0
    local b = by_mode[m]
    if not b then
        b = { tmd = 0, lit = 0, prim = 0, sel = 0, sel_nz = 0 }
        by_mode[m] = b
    end
    return b, m
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "seq,vsync,selector,f42,actor,ra,mode,scene")
        probe.env.write_manifest("autorun_w4c_mesh_path_census.lua", {
            sstate = SSTATE, frames = FRAMES, sel_rows = SEL_ROWS,
        })
        local descs = {}
        for _, t in ipairs(TAPS) do
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
                d.hits_ref.n = d.hits_ref.n + 1
                counts[t.kind] = (counts[t.kind] or 0) + 1
                local b = mode_bucket()
                if t.hot then
                    if t.kind == "tmd_draw" then b.tmd = b.tmd + 1
                    elseif t.kind == "lit_draw" then b.lit = b.lit + 1
                    else b.prim = b.prim + 1 end
                    return
                end
                -- Selector: the branch register IS the actor's +0x42, so read
                -- it rather than re-reading memory (the actor pointer is $s0
                -- at all three sites, recorded for cross-checking).
                local r = PCSX.getRegisters()
                local f42 = s16(t.reg == "s7" and r.GPR.n.s7 or r.GPR.n.v0)
                b.sel = b.sel + 1
                if f42 ~= 0 then
                    sel_nonzero[t.kind] = (sel_nonzero[t.kind] or 0) + 1
                    b.sel_nz = b.sel_nz + 1
                else
                    sel_zero[t.kind] = (sel_zero[t.kind] or 0) + 1
                end
                f42_values[f42] = (f42_values[f42] or 0) + 1
                sel_rows = sel_rows + 1
                if sel_rows <= SEL_ROWS then
                    local _, m = mode_bucket()
                    csv:row("%d,%d,%s,%d,0x%s,0x%s,%d,%s",
                        sel_rows, g_elapsed, t.kind, f42,
                        hex8(r.GPR.n.s0), hex8(r.GPR.n.ra), m, scene_name())
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            local bad = 0
            for _, t in ipairs(TAPS) do
                local got = n32(probe.read_u32(t.addr) or 0)
                if got ~= n32(t.want) then
                    bad = bad + 1
                    logf("TAP MISMATCH [0x%08X] = 0x%s want 0x%s (%s)",
                         t.addr, hex8(got), hex8(t.want), t.name)
                end
            end
            logf("armed %d taps, %d mismatched; mode=%d scene=%s",
                 #TAPS, bad, probe.read_u8(GAME_MODE) or 0, scene_name())
        end
        local m = probe.read_u8(GAME_MODE) or 0
        if m ~= last_mode then
            modes_seen[#modes_seen + 1] =
                string.format("%d:mode=%d:%s", elapsed, m, scene_name())
            last_mode = m
        end
        if (elapsed % 60) == 0 then
            logf("vsync %d tmd=%d lit=%d prim=%d sel=%d (nz %d) mode=%d",
                 elapsed, counts.tmd_draw or 0, counts.lit_draw or 0,
                 counts.prim_disp or 0,
                 (counts.sel_ada4 or 0) + (counts.sel_b964 or 0)
                 + (counts.sel_8a08 or 0),
                 (sel_nonzero.sel_ada4 or 0) + (sel_nonzero.sel_b964 or 0)
                 + (sel_nonzero.sel_8a08 or 0),
                 m)
        end
    end,

    on_summary = function()
        logf("--- mesh-path census over %d vsyncs ---", g_elapsed)
        logf("modes: %s", table.concat(modes_seen, " "))
        logf("FUN_8002735C (TMD renderer)      entries: %d",
             counts.tmd_draw or 0)
        logf("FUN_80029888 (light sibling)     entries: %d",
             counts.lit_draw or 0)
        logf("FUN_80043390 (per-prim dispatch) entries: %d",
             counts.prim_disp or 0)
        for _, k in ipairs({ "sel_ada4", "sel_b964", "sel_8a08" }) do
            logf("selector %s: %d hits, %d with +0x42 != 0, %d with 0",
                 k, counts[k] or 0, sel_nonzero[k] or 0, sel_zero[k] or 0)
        end
        local vals = {}
        for v, n in pairs(f42_values) do vals[#vals + 1] = { v, n } end
        table.sort(vals, function(a, b) return a[2] > b[2] end)
        for i = 1, math.min(#vals, 12) do
            logf("  +0x42 = %d : %d actor draws", vals[i][1], vals[i][2])
        end
        for m, b in pairs(by_mode) do
            logf("mode %d: tmd=%d lit=%d prim=%d sel=%d (nz %d)",
                 m, b.tmd, b.lit, b.prim, b.sel, b.sel_nz)
        end
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
