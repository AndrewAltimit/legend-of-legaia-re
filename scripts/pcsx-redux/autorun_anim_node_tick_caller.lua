-- autorun_anim_node_tick_caller.lua
--
-- Pin the CALLER of the battle per-frame anim-node tick FUN_80047430.
--
-- FUN_80047430 advances each battle actor's animation cursor, detects
-- end-of-clip, and commits the queued clip via FUN_8004AD80 (see
-- docs/subsystems/battle-action.md and ghidra/scripts/funcs/80047430.txt).
-- Its own caller is NOT in the dumped corpus: no `jal 0x80047430` site
-- exists, so it is reached by a function-pointer dispatch (the per-actor
-- node tick is almost certainly invoked through a node vtable / handler
-- slot, the same shape as the screen-widget and move-VM part dispatch).
-- One Exec breakpoint reading $ra at the function entry closes it: at
-- entry the prologue has not yet saved $ra, so $ra holds the dispatch
-- site's return address, and the word at ($ra - 8) is the dispatching
-- branch (a `jalr` if indirect, identifying the register).
--
-- The probe arms an Exec breakpoint at FUN_80047430 and, on each hit,
-- records:
--   * $ra (the caller's return address), deduped into a unique-caller
--     table so a per-frame, per-actor flood collapses to the handful of
--     real dispatch sites,
--   * the word at ($ra - 8) and ($ra - 4): the branch + its delay slot.
--     A `jalr ra, rX` (opcode 0, funct 0x09) confirms fn-ptr dispatch and
--     names the register; a `jal` (opcode 0x03) would mean a direct call
--     the static scan missed,
--   * a0 = the node/actor pointer the tick is driven with,
--   * a full call-context (GPRs + straddling instructions + the sp stack
--     words, which carry the saved-ra chain for walking one frame up).
--
-- Default state = party_basic_attack_vs_gobu_gobu (a mid-battle save; the anim
-- tick fires every frame for every battle actor, so any battle save
-- works). Resolve it by name through run_probe.sh --scenario so the
-- library backup is used in preference to a wiped quicksave slot.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario party_basic_attack_vs_gobu_gobu \
--       --lua scripts/pcsx-redux/autorun_anim_node_tick_caller.lua
--
--   # or with an explicit save state:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_anim_node_tick_caller.lua \
--   LEGAIA_FRAMES=180 \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- Output (under captures/anim_node_tick_caller/<ts>/ unless LEGAIA_OUT_DIR set):
--   anim_node_tick_caller.csv          one row per UNIQUE ($ra) caller
--   anim_node_tick_caller.detail.txt   first N full call contexts
--
-- Reading the result: the CSV's `ra` column is the return address just
-- past the dispatch branch; `branch_word` decodes the dispatch. Map `ra`
-- (and, if indirect, the register's loaded value from the call context)
-- back to the dispatching function with the funcs dumps. That dispatcher
-- is the missing edge; add it to docs/reference/functions.md and close
-- the F-PROBES "FUN_80047430 caller" row in open-rev-eng-threads.md.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local BP_TICK = 0x80047430 -- FUN_80047430 (battle anim-node tick)

local csv = probe.csv_open(probe.out_path("anim_node_tick_caller.csv"),
    "first_tick,ra,branch_pc,branch_word,delay_word,is_jalr,jalr_rs,a0,hits")
local detail_path = probe.out_path("anim_node_tick_caller.detail.txt")

local armed        = false
local total_hits   = 0
local detail_count = 0
local g_elapsed    = 0
local MAX_DETAIL   = 16
-- Per-unique-$ra aggregation: key = ra, value = { ra, branch_pc,
-- branch_word, delay_word, is_jalr, jalr_rs, a0, hits, first_tick }.
local callers      = {}
local caller_count = 0
local MAX_CALLERS  = 64 -- backstop; the real answer is 1-3 sites.

-- MIPS register names indexed by number, for decoding a jalr's rs field.
local REG = {
    [0] = "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

local function on_hit()
    total_hits = total_hits + 1
    local r = PCSX.getRegisters()
    local g = r.GPR.n
    local function rr(x) return bit.band(tonumber(g[x]), 0xFFFFFFFF) end

    local ra = rr("ra")
    local rec = callers[ra]
    if rec then
        rec.hits = rec.hits + 1
        return
    end
    if caller_count >= MAX_CALLERS then return end

    -- First time we see this caller: decode its dispatch branch.
    local branch_pc = bit.band(ra - 8, 0xFFFFFFFF)
    local branch = probe.read_u32(branch_pc) or 0
    local delay = probe.read_u32(bit.band(ra - 4, 0xFFFFFFFF)) or 0
    -- jalr rd, rs : opcode 0 (bits 31..26 == 0) AND funct 0x09 (bits 5..0).
    local opcode = bit.band(bit.rshift(branch, 26), 0x3F)
    local funct = bit.band(branch, 0x3F)
    local is_jalr = (opcode == 0 and funct == 0x09)
    local jalr_rs = is_jalr and REG[bit.band(bit.rshift(branch, 21), 0x1F)] or "-"

    rec = {
        ra = ra, branch_pc = branch_pc, branch_word = branch,
        delay_word = delay, is_jalr = is_jalr, jalr_rs = jalr_rs,
        a0 = rr("a0"), hits = 1, first_tick = g_elapsed,
    }
    callers[ra] = rec
    caller_count = caller_count + 1

    PCSX.log(string.format(
        "[anim-caller] NEW caller ra=0x%08X branch@0x%08X=0x%08X %s%s a0=0x%08X",
        ra, branch_pc, branch,
        is_jalr and "jalr " or "(not jalr) ",
        is_jalr and ("rs=" .. jalr_rs) or "", rec.a0))

    if detail_count < MAX_DETAIL then
        detail_count = detail_count + 1
        probe.append_call_context(detail_path,
            probe.capture_call_context(string.format(
                "FUN_80047430 caller #%d  ra=0x%08X  branch@0x%08X=0x%08X (%s%s)  a0=0x%08X",
                caller_count, ra, branch_pc, branch,
                is_jalr and "jalr rs=" or "direct ",
                is_jalr and jalr_rs or "", rec.a0)))
    end
end

local function arm()
    probe.arm_breakpoint(BP_TICK, "Exec", 4, "anim_node_tick_80047430", on_hit)
    PCSX.log(string.format(
        "[anim-caller] armed Exec BP at 0x%08X (FUN_80047430 anim-node tick)",
        BP_TICK))
    armed = true
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 180),
    snapshot_path  = probe.out_path("anim_node_tick_caller.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then
            arm()
        end
        -- Bail as soon as we have stable coverage: a few distinct callers
        -- seen, each hit many times (the per-frame flood means a real
        -- dispatch site racks up hits fast). Don't wait out the budget.
        if armed and caller_count >= 1 and total_hits >= 600 then
            ctx.request_quit = true
        end
    end,
    on_done = function()
        -- Emit one CSV row per unique caller, most-hit first.
        local rows = {}
        for _, rec in pairs(callers) do rows[#rows + 1] = rec end
        table.sort(rows, function(x, y) return x.hits > y.hits end)
        for _, rec in ipairs(rows) do
            csv:row("%d,0x%08X,0x%08X,0x%08X,0x%08X,%d,%s,0x%08X,%d",
                rec.first_tick, rec.ra, rec.branch_pc, rec.branch_word,
                rec.delay_word, rec.is_jalr and 1 or 0, rec.jalr_rs,
                rec.a0, rec.hits)
        end
        csv:close()
        PCSX.log(string.format(
            "[anim-caller] done. unique callers=%d total hits=%d. "
            .. "The `ra` column (minus 8 = branch_pc) is the dispatch site; "
            .. "map it to its function with ghidra/scripts/funcs/. "
            .. "is_jalr=1 + jalr_rs name => fn-ptr dispatch through that register.",
            caller_count, total_hits))
    end,
})
