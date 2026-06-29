-- autorun_minigame_dance.lua
--
-- Pin the Noa dance (rhythm) minigame's per-tier rating multipliers.
--
-- The dance overlay ticks a beat-clock state machine FUN_801cf470 (switch on
-- DAT_801d5334) whose play-loop tail advances the beat phase. A press is graded
-- by FUN_801d1960(player, lane, variant), which returns three tiers: 0 = miss
-- (outside the dead-zone window or wrong direction), 1 = hit, 2 = sequence /
-- bonus complete. The continuous accuracy weight (0..0x1000, peaks on-beat) and
-- the chart row selected by the groove gauge DAT_801d544c (/1000 = lane) are
-- what scale the award -- so capturing the judge's args + the live gauge per
-- hit, plus the step-chart bytes at DAT_801d509c (row*0x20 + beat) and the
-- per-lane bonus table DAT_801d41a4, pins the per-tier multipliers.
--
-- The probe arms Exec BPs at FUN_801cf470 (beat-clock SM) and FUN_801d1960
-- (hit judge). Each hit dedupes by $ra, decodes the dispatch branch at $ra-8,
-- and records a0/a1/a2 (the judge's player/lane/variant) + the live groove
-- gauge. It also one-shot dumps the step chart + bonus-value table from RAM.
--
-- Default state = a save mid-song in the dance minigame (the judge fires on a
-- press, so play a few beats). Resolve it by name through run_probe.sh.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario minigame_dance_pcsx \
--       --lua scripts/pcsx-redux/autorun_minigame_dance.lua
--
-- Output (under captures/minigame_dance/<ts>/ unless LEGAIA_OUT_DIR set):
--   minigame_dance.csv          one row per UNIQUE (label,$ra) exec site
--   minigame_dance.detail.txt   first-N call contexts + the step-chart dump

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Exec-bp targets (verified vs docs/subsystems/minigame-dance.md).
local EXEC = {
    { addr = 0x801CF470, label = "dance_sm" },     -- FUN_801cf470 beat-clock SM
    { addr = 0x801D1960, label = "hit_judge" },     -- FUN_801d1960 hit judge (0/1/2 tiers)
}
local WATCH = nil -- reads only; no scoring Write watch for this minigame.
-- Live globals sampled / dumped.
local GROOVE_GAUGE = 0x801D544C -- DAT_801d544c[player] (player*4), /1000 = chart row
local PLAYER_SCORE = 0x801D53CC -- DAT_801d53cc[player] score, clamped 999
local TABLES = {
    { addr = 0x801D509C, len = 0x100, name = "step_chart" },     -- DAT_801d509c row*0x20+beat
    { addr = 0x801D41A4, len = 0x40,  name = "bonus_value_tbl" }, -- DAT_801d41a4 per-lane bonus
}
local OUT = "minigame_dance"

local function sample_extra()
    return probe.read_u32(GROOVE_GAUGE) or 0 -- player 0 groove gauge
end
local function probe_note()
    return string.format("gauge[0..2]=%d/%d/%d score[0..2]=%d/%d/%d",
        probe.read_u32(GROOVE_GAUGE) or 0,
        probe.read_u32(GROOVE_GAUGE + 4) or 0,
        probe.read_u32(GROOVE_GAUGE + 8) or 0,
        probe.read_u32(PLAYER_SCORE) or 0,
        probe.read_u32(PLAYER_SCORE + 4) or 0,
        probe.read_u32(PLAYER_SCORE + 8) or 0)
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
    PCSX.log(string.format("[minigame_dance] armed %d exec BPs + %s",
        #EXEC, WATCH and ("Write watch @0x" .. string.format("%08X", WATCH.addr)) or "no watch"))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3600),
    snapshot_path = probe.out_path(OUT .. ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then arm() end
        -- Stable coverage requires both the SM and the (press-gated) judge seen.
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
            "[minigame_dance] done. exec sites=%d total hits=%d", site_count, total_hits))
    end,
})
