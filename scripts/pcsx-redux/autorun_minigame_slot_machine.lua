-- autorun_minigame_slot_machine.lua
--
-- Pin the casino slot machine's payout/jackpot table + RNG.
--
-- The slot overlay is a per-frame reel state machine FUN_801cf0d8 (switch on
-- DAT_801d3c84). After all three reels stop, FUN_801d13e8 evaluates the win:
-- it keeps the highest matching payline and credits payout_table[symbol], a
-- byte read from DAT_801d3598; symbols 8/9 are the bonus/jackpot symbols. Reel
-- outcomes come from the self-contained LCG FUN_801d30cc (x = x*5+1, 16-bit
-- fold) over DAT_801d3c80. The player credit balance lives in the overlay-local
-- DAT_801d4114 and is committed to the global casino coin bank _DAT_800845A4
-- only on cash-out (state 100 store) -- so a Write watch on the coin bank pins
-- that commit's PC/RA/value.
--
-- The probe arms Exec BPs at FUN_801cf0d8 (reel SM), FUN_801d13e8 (win eval),
-- and FUN_801d30cc (LCG). Each hit dedupes by $ra, decodes the dispatch branch
-- at $ra-8, and samples the live balance; the LCG site's $ra trail names every
-- consumer of the RNG. It Write-watches the coin bank _DAT_800845A4 and
-- one-shot dumps the payout-byte table + the symbol-descriptor table.
--
-- Default state = a save sitting at the casino slot machine. Resolve it by name
-- through run_probe.sh --scenario.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario minigame_slot_machine_pcsx \
--       --lua scripts/pcsx-redux/autorun_minigame_slot_machine.lua
--
-- Output (under captures/minigame_slot_machine/<ts>/ unless LEGAIA_OUT_DIR set):
--   minigame_slot_machine.csv          one row per UNIQUE (label,$ra) exec site
--   minigame_slot_machine.writes.csv   coin-bank Write hits (tick,addr,pc,ra,value)
--   minigame_slot_machine.detail.txt   first-N call contexts + the table dump

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Exec-bp targets (verified vs docs/subsystems/minigame-slot-machine.md).
local EXEC = {
    { addr = 0x801CF0D8, label = "slot_sm" },    -- FUN_801cf0d8 reel state machine
    { addr = 0x801D13E8, label = "win_eval" },    -- FUN_801d13e8 payout/jackpot eval
    { addr = 0x801D30CC, label = "lcg" },          -- FUN_801d30cc slot LCG (x*5+1)
}
-- Write-watch: the global casino coin bank (written on cash-out commit).
local WATCH = { addr = 0x800845A4, width = 4, label = "coin_bank" } -- _DAT_800845A4
-- Live globals sampled / dumped.
local BALANCE  = 0x801D4114 -- DAT_801d4114 overlay-local credit balance
local LCG_STATE = 0x801D3C80 -- DAT_801d3c80 slot LCG seed state
local FEATURE_MODE = 0x801D3CAC -- DAT_801d3cac feature mode (0 normal .. 6 bonus)
local TABLES = {
    { addr = 0x801D3598, len = 0x20,  name = "payout_byte_tbl" }, -- DAT_801d3598 per-symbol payout
    { addr = 0x801D347C, len = 0x140, name = "symbol_desc_tbl" }, -- DAT_801d347c 0x14 stride
}
local OUT = "minigame_slot_machine"

local function sample_extra()
    return probe.read_u32(BALANCE) or 0
end
local function probe_note()
    return string.format("balance=%d lcg=0x%08X feature=%d",
        probe.read_u32(BALANCE) or 0,
        probe.read_u32(LCG_STATE) or 0,
        probe.read_u32(FEATURE_MODE) or 0)
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
    PCSX.log(string.format("[minigame_slot_machine] armed %d exec BPs + %s",
        #EXEC, WATCH and ("Write watch @0x" .. string.format("%08X", WATCH.addr)) or "no watch"))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate3"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3600),
    snapshot_path = probe.out_path(OUT .. ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then arm() end
        -- Coverage requires the win-eval / cash-out events, not just idle spins.
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
            "[minigame_slot_machine] done. exec sites=%d total hits=%d coin writes=%d",
            site_count, total_hits, w and w:total() or 0))
    end,
})
