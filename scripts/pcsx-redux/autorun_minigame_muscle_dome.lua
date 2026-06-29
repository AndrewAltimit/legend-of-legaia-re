-- autorun_minigame_muscle_dome.lua
--
-- Pin Muscle Dome's deck / card-table bytes + the per-round card commit.
--
-- The card-battle arena runs on a per-frame match controller FUN_801d0748 that
-- dispatches on the phase byte ctx+6 (ctx = _DAT_8007bd24): deal -> select ->
-- commit -> resolve -> score. A hand has four card slots; the card/presentation
-- driver FUN_801d388c builds them (case 9/0x2c) from the deck-order tables
-- &DAT_801f4b8c / &DAT_801f4b94 and a per-step sub-draw script &PTR_DAT_801f4d34,
-- and on commit (case 0xb) appends the chosen move id into the fighter actor's
-- +0x1df queue (actor = &DAT_801c9370[ctx+0x13]) while the round point budget
-- ctx+0x6dc allows. So pinning the deck bytes + watching the +0x1df queue across
-- a commit reveals the card table.
--
-- The probe arms Exec BPs at FUN_801d0748 (match SM) and FUN_801d388c (card
-- driver). Each hit dedupes by $ra, decodes the dispatch branch at $ra-8, and
-- samples the live phase byte; the per-site call-context note resolves ctx ->
-- active fighter -> actor and dumps the +0x1df queue window + commit counters.
-- It one-shot dumps the deck-order + sub-draw script tables from RAM. (The
-- commit target is a runtime actor pointer, so it is read in the note rather
-- than via a fixed-address Write watch.)
--
-- Default state = a save inside a Muscle Dome match (card select/commit phase).
-- Resolve it by name through run_probe.sh --scenario.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario minigame_muscle_dome_pcsx \
--       --lua scripts/pcsx-redux/autorun_minigame_muscle_dome.lua
--
-- Output (under captures/minigame_muscle_dome/<ts>/ unless LEGAIA_OUT_DIR set):
--   minigame_muscle_dome.csv          one row per UNIQUE (label,$ra) exec site
--   minigame_muscle_dome.detail.txt   first-N call contexts (incl. +0x1df queue)
--                                      + the deck/script-table dump

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Exec-bp targets (verified vs docs/subsystems/minigame-muscle-dome.md).
local EXEC = {
    { addr = 0x801D0748, label = "match_sm" },     -- FUN_801d0748 per-frame match SM (phase ctx+6)
    { addr = 0x801D388C, label = "card_driver" },   -- FUN_801d388c deal/commit + sub-draw script
}
local WATCH = nil -- the +0x1df commit target is a runtime actor ptr; read in the note.
-- Live globals.
local CTX_PTR     = 0x8007BD24 -- _DAT_8007bd24 context pointer (ctx)
local ACTOR_TABLE = 0x801C9370 -- &DAT_801c9370 global actor pointer table
local TABLES = {
    { addr = 0x801F4B8C, len = 0x20, name = "deck_order_a" },     -- &DAT_801f4b8c
    { addr = 0x801F4B94, len = 0x20, name = "deck_order_b" },     -- &DAT_801f4b94
    { addr = 0x801F4D34, len = 0x40, name = "substep_script_ptr" }, -- &PTR_DAT_801f4d34
}
local OUT = "minigame_muscle_dome"

local function sample_extra()
    local ctx = probe.read_u32(CTX_PTR) or 0
    if not probe.in_ram(ctx) then return 0 end
    return probe.read_u8(ctx + 6) or 0 -- phase byte
end
local function probe_note()
    local ctx = probe.read_u32(CTX_PTR) or 0
    if not probe.in_ram(ctx) then return "ctx=<unresolved>" end
    local phase     = probe.read_u8(ctx + 6) or 0
    local committed = probe.read_u8(ctx + 0x19) or 0
    local slot      = probe.read_u8(ctx + 0x1A) or 0
    local fidx      = probe.read_u8(ctx + 0x13) or 0
    local budget    = probe.read_u16(ctx + 0x6DC) or 0
    local actor     = probe.read_u32(ACTOR_TABLE + fidx * 4) or 0
    local queue = "<no actor>"
    if probe.in_ram(actor) then
        local b = probe.read_bytes(actor + 0x1DF, 8)
        queue = b and probe.bytes_to_hex(b) or "?"
    end
    return string.format(
        "phase=0x%02X committed=%d slot=%d fidx=%d budget=%d actor=0x%08X queue+1df=[%s]",
        phase, committed, slot, fidx, budget, actor, queue)
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
    PCSX.log(string.format("[minigame_muscle_dome] armed %d exec BPs + %s",
        #EXEC, WATCH and ("Write watch @0x" .. string.format("%08X", WATCH.addr)) or "no watch"))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3600),
    snapshot_path = probe.out_path(OUT .. ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then arm() end
        -- Coverage requires the card driver (a deal/commit phase), not just the SM.
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
            "[minigame_muscle_dome] done. exec sites=%d total hits=%d", site_count, total_hits))
    end,
})
