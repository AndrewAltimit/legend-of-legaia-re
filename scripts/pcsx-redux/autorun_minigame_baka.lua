-- autorun_minigame_baka.lua
--
-- Pin Baka Fighter's best-of-N round target + gold-payout constants.
--
-- The duel runs on a round/match state machine FUN_801d3468 (phase global
-- DAT_801dbf78; resolution body runs while DAT_801dbf44 == 100). Each exchange
-- is decided by the rock-paper-scissors resolver FUN_801d3a14: P1 type
-- DAT_801dbfe0 vs P2 type DAT_801dc088, 1->2->3->1 beats-cycle, type 4 = special
-- auto-win; it returns 0 (P1) / 1 (P2) / 3 (draw) / -1 (undecided). Round wins
-- accumulate per fighter and the round index DAT_801dbf20 advances; the
-- end-of-match tally FUN_801d239c drains the score counters into the player's
-- gold _DAT_80084440. Both the best-of-N target and the gold rate are read-but-
-- unpinned (doc Open), so this probe captures them live.
--
-- The probe arms Exec BPs at FUN_801d3468 (round SM) and FUN_801d3a14 (RPS
-- resolver). Each hit dedupes by $ra, decodes the dispatch branch at $ra-8, and
-- samples the round index + high score + both fighters' chosen attack types. It
-- Write-watches the gold counter _DAT_80084440 (the tally's payout store) and
-- one-shot dumps the per-opponent AI move-pattern table DAT_801d76e8.
--
-- Default state = a save in an active Baka Fighter duel. Resolve it by name
-- through run_probe.sh --scenario.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario minigame_baka_pcsx \
--       --lua scripts/pcsx-redux/autorun_minigame_baka.lua
--
-- Output (under captures/minigame_baka/<ts>/ unless LEGAIA_OUT_DIR set):
--   minigame_baka.csv          one row per UNIQUE (label,$ra) exec site
--   minigame_baka.writes.csv   gold-counter Write hits (tick,addr,pc,ra,value)
--   minigame_baka.detail.txt   first-N call contexts + the AI-table dump

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Exec-bp targets (verified vs docs/subsystems/minigame-baka-fighter.md).
local EXEC = {
    { addr = 0x801D3468, label = "round_sm" },     -- FUN_801d3468 round/match SM
    { addr = 0x801D3A14, label = "rps_resolver" },  -- FUN_801d3a14 exchange win-condition
}
-- Write-watch: the player's gold (end-of-match tally payout store).
local WATCH = { addr = 0x80084440, width = 4, label = "player_gold" } -- _DAT_80084440
-- Live globals sampled / dumped.
local ROUND_INDEX = 0x801DBF20 -- DAT_801dbf20 round index
local HIGH_SCORE  = 0x801DBEE4 -- DAT_801dbee4 running high score
local P1_TYPE = 0x801DBFE0 -- DAT_801dbfe0 P1 chosen attack type (0..4)
local P2_TYPE = 0x801DC088 -- DAT_801dc088 P2 chosen attack type
local TABLES = {
    { addr = 0x801D76E8, len = 0xD8, name = "ai_move_pattern_tbl" }, -- DAT_801d76e8 stride 0x6c
}
local OUT = "minigame_baka"

local function sample_extra()
    return probe.read_u32(ROUND_INDEX) or 0
end
local function probe_note()
    return string.format("round=%d hiscore=%d p1type=%d p2type=%d",
        probe.read_u32(ROUND_INDEX) or 0,
        probe.read_u32(HIGH_SCORE) or 0,
        probe.read_u32(P1_TYPE) or 0,
        probe.read_u32(P2_TYPE) or 0)
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
    PCSX.log(string.format("[minigame_baka] armed %d exec BPs + %s",
        #EXEC, WATCH and ("Write watch @0x" .. string.format("%08X", WATCH.addr)) or "no watch"))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3600),
    snapshot_path = probe.out_path(OUT .. ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then arm() end
        -- Coverage requires a decided exchange (resolver) and the gold tally.
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
            "[minigame_baka] done. exec sites=%d total hits=%d gold writes=%d",
            site_count, total_hits, w and w:total() or 0))
    end,
})
