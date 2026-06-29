-- autorun_minigame_fishing.lua
--
-- Pin the fishing minigame's tension tug-of-war + scoring writers.
--
-- The fishing overlay (the minigame-hub binary, lower band) ticks a numeric
-- state machine FUN_801cf3bc that walks rod-select -> cast -> reel -> catch.
-- The hooked-fight is a tug-of-war on the tension gauge DAT_801d9168 (0..0x1000)
-- whose whole integration lives in FUN_801d4004 (fish-AI + tension tick). NB the
-- doc clarifies FUN_801d4004 is driven from the ACTOR side (via FUN_801d26cc),
-- not the mode switch, so its $ra at entry names that actor-handler dispatch.
-- A landed catch is resolved in FUN_801d5298, which credits the persistent
-- fishing-point counter _DAT_8008444c (capped 999999) -- so a Write watch on
-- that counter pins the scoring writer's PC/RA/value directly.
--
-- The probe arms:
--   * Exec BPs at FUN_801cf3bc (mode SM) and FUN_801d4004 (tension tick).
--     Each hit dedupes by $ra, decodes the dispatch branch at $ra-8 (a jalr
--     confirms fn-ptr dispatch + names the register, the same shape as the
--     other minigame/field mode handlers), and samples the live tension gauge.
--   * a Write watch (probe.watch) on the score counter _DAT_8008444c: the
--     faulting PC/RA is the scoring writer (FUN_801d5298's credit store).
--   * a one-shot dump of the per-species parameter table base &DAT_801d81a8.
--
-- Default state = a save parked at the fishing pond, mid-cast or reeling so the
-- tension tick fires. Resolve it by name through run_probe.sh --scenario.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario minigame_fishing_pcsx \
--       --lua scripts/pcsx-redux/autorun_minigame_fishing.lua
--
-- Output (under captures/minigame_fishing/<ts>/ unless LEGAIA_OUT_DIR set):
--   minigame_fishing.csv          one row per UNIQUE (label,$ra) exec site
--   minigame_fishing.writes.csv   score-counter Write hits (tick,addr,pc,ra,value)
--   minigame_fishing.detail.txt   first-N call contexts + the data-table dump

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Exec-bp targets (verified vs docs/subsystems/minigame-fishing.md).
local EXEC = {
    { addr = 0x801CF3BC, label = "fishing_sm" },   -- FUN_801cf3bc per-frame mode SM
    { addr = 0x801D4004, label = "tension_tick" },  -- FUN_801d4004 fish-AI + tension gauge
}
-- Write-watch: the persistent fishing-point score (scoring writer lands here).
local WATCH = { addr = 0x8008444C, width = 4, label = "fishing_points" } -- _DAT_8008444c
-- Live globals sampled / dumped.
local TENSION_GAUGE = 0x801D9168 -- DAT_801d9168 tension 0..0x1000
local TABLES = {
    { addr = 0x801D81A8, len = 0x50, name = "species_param_table" }, -- &DAT_801d81a8 stride 0x28
}
local OUT = "minigame_fishing"

local function sample_extra()
    return probe.read_u32(TENSION_GAUGE) or 0
end
local function probe_note()
    return string.format("tension=0x%X points=%d",
        probe.read_u32(TENSION_GAUGE) or 0, probe.read_u32(WATCH.addr) or 0)
end

----------------------------------------------------------------------
-- Shared scaffold (mirrors autorun_anim_node_tick_caller.lua).

local REG = {
    [0] = "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

local csv = probe.csv_open(probe.out_path(OUT .. ".csv"),
    "first_tick,label,ra,branch_pc,branch_word,is_jalr,jalr_rs,a0,a1,a2,a3,extra,hits")
local detail_path = probe.out_path(OUT .. ".detail.txt")

local g_elapsed = 0
local total_hits = 0
local sites = {}
local site_count = 0
local MAX_SITES = 64
local detail_count = 0
local MAX_DETAIL = 16
local armed = false

local wcsv, w
if WATCH then
    wcsv = probe.csv_open(probe.out_path(OUT .. ".writes.csv"),
        "tick,label,addr,pc,ra,value")
    w = probe.watch.new{
        csv = wcsv,
        detail_path = probe.out_path(OUT .. ".writes.detail.txt"),
        elapsed = function() return g_elapsed end,
    }
end

local function on_exec(label, bp_addr)
    return function()
        total_hits = total_hits + 1
        local r = PCSX.getRegisters()
        local g = r.GPR.n
        local function rr(x) return bit.band(tonumber(g[x]), 0xFFFFFFFF) end
        local ra = rr("ra")
        local key = label .. "@" .. string.format("0x%08X", ra)
        local rec = sites[key]
        if rec then rec.hits = rec.hits + 1; return end
        if site_count >= MAX_SITES then return end
        local branch_pc = bit.band(ra - 8, 0xFFFFFFFF)
        local branch = probe.read_u32(branch_pc) or 0
        local opcode = bit.band(bit.rshift(branch, 26), 0x3F)
        local funct = bit.band(branch, 0x3F)
        local is_jalr = (opcode == 0 and funct == 0x09)
        local jalr_rs = is_jalr and REG[bit.band(bit.rshift(branch, 21), 0x1F)] or "-"
        rec = {
            label = label, ra = ra, branch_pc = branch_pc, branch_word = branch,
            is_jalr = is_jalr, jalr_rs = jalr_rs,
            a0 = rr("a0"), a1 = rr("a1"), a2 = rr("a2"), a3 = rr("a3"),
            extra = sample_extra(), hits = 1, first_tick = g_elapsed,
        }
        sites[key] = rec
        site_count = site_count + 1
        if detail_count < MAX_DETAIL then
            detail_count = detail_count + 1
            probe.append_call_context(detail_path,
                probe.capture_call_context(string.format(
                    "%s site #%d ra=0x%08X branch@0x%08X=0x%08X (%s%s) a0=0x%08X | %s",
                    label, site_count, ra, branch_pc, branch,
                    is_jalr and "jalr rs=" or "direct ", is_jalr and jalr_rs or "",
                    rec.a0, probe_note())))
        end
    end
end

local function dump_tables()
    if #TABLES == 0 then return end
    local f = io.open(detail_path, "a")
    if not f then return end
    f:write("\n==== data tables (live RAM snapshot at arm) ====\n")
    for _, t in ipairs(TABLES) do
        local b = probe.read_bytes(t.addr, t.len)
        f:write(string.format("%s @0x%08X (%d bytes): %s\n",
            t.name, t.addr, t.len, b and probe.bytes_to_hex(b) or "<unreadable>"))
    end
    f:close()
end

local function arm()
    for _, e in ipairs(EXEC) do
        probe.arm_breakpoint(e.addr, "Exec", 4, e.label, on_exec(e.label, e.addr))
    end
    if WATCH then w:arm(WATCH.addr, WATCH.width, WATCH.label) end
    dump_tables()
    armed = true
    PCSX.log(string.format("[minigame_fishing] armed %d exec BPs + %s",
        #EXEC, WATCH and ("Write watch @0x" .. string.format("%08X", WATCH.addr)) or "no watch"))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3600),
    snapshot_path = probe.out_path(OUT .. ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then arm() end
        -- Stable coverage: every exec site seen, and (if watched) the scoring
        -- write fired at least once. Don't wait out the whole frame budget.
        if armed and site_count >= #EXEC
            and (not WATCH or w:total() >= 1) and total_hits >= 1200 then
            ctx.request_quit = true
        end
    end,
    on_done = function()
        local rows = {}
        for _, rec in pairs(sites) do rows[#rows + 1] = rec end
        table.sort(rows, function(x, y) return x.hits > y.hits end)
        for _, rec in ipairs(rows) do
            csv:row("%d,%s,0x%08X,0x%08X,0x%08X,%d,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,%d",
                rec.first_tick, rec.label, rec.ra, rec.branch_pc, rec.branch_word,
                rec.is_jalr and 1 or 0, rec.jalr_rs, rec.a0, rec.a1, rec.a2, rec.a3,
                rec.extra, rec.hits)
        end
        csv:close()
        if wcsv then wcsv:close() end
        PCSX.log(string.format(
            "[minigame_fishing] done. exec sites=%d total hits=%d score writes=%d",
            site_count, total_hits, w and w:total() or 0))
    end,
})
