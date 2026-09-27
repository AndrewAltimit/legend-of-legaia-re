-- autorun_moving_class_writer.lua
--
-- Who writes a field actor's moving-class bit `+0x10 & 0x20000`?
--
-- The motion-pause kick FUN_8003C9AC (docs/subsystems/motion-vm.md) acts
-- only on an actor carrying bit 17 of its `+0x10` flag word. The static
-- candidate is the MAN placement seater FUN_8003A1E4, whose store at
-- 0x8003A3B4 (`sw v0, 0x10(s0)`) follows `lui v1, 0x2; or v0, v0, v1`.
-- This probe confirms (or refutes) that at runtime by write-watching the
-- `+0x10` word of every slot in the field actor pool and decoding each
-- store's source register, so the logged value is the value being STORED
-- (a Write breakpoint fires before the store lands).
--
-- Pool geometry: slots are 0xD8 bytes, from 0x8007E91C (the lowest slot
-- the town01 free-roam list reaches) upward; LEGAIA_POOL_SLOTS slots are
-- armed (default 91 = 0x8007E91C..0x8008350C inclusive).
--
-- Per hit it records the PC, the slot, the old and new word, and whether
-- bit 17 changed. Aggregated per (pc, bit17 transition) into the CSV; the
-- first LEGAIA_DETAIL hits that SET bit 17 are logged with $ra.
--
-- An Exec breakpoint at 0x8003A3B4 counts the seater's own stores
-- independently of the pool geometry (if the pool guess is wrong the
-- Exec count still answers the question).
--
-- Usage (a mid-load state, so the scene setup runs inside the window):
--   timeout 900 bash scripts/pcsx-redux/run_probe.sh --isolate-config \
--       --scenario field_load_first_town \
--       --lua scripts/pcsx-redux/autorun_moving_class_writer.lua --frames 700
--
-- Output: moving_class_writer.csv (one row per pc x transition) under the
-- probe's out dir. Sony-derived values never leave captures/.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local POOL_BASE  = 0x8007E91C
local SLOT       = 0xD8
local NSLOTS     = probe.getenv_num("LEGAIA_POOL_SLOTS", 91)
local SEATER_SW  = 0x8003A3B4
local MAX_DETAIL = probe.getenv_num("LEGAIA_DETAIL", 24)

local function u32(x) return bit.band(tonumber(x), 0xFFFFFFFF) end

local csv = probe.csv_open(probe.out_path("moving_class_writer.csv"),
    "pc,transition,hits,first_tick,first_slot_addr,first_old,first_new,first_ra")

local agg, order = {}, {}
local g_tick, armed, total, seater_hits, detail = 0, false, 0, 0, 0
local REGN = {
    [0] = "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

local function on_write(slot_addr)
    local r = PCSX.getRegisters()
    local g = r.GPR.n
    local pc = u32(r.pc)
    local insn = probe.read_u32(pc) or 0
    local op = bit.rshift(insn, 26)
    local rt = bit.band(bit.rshift(insn, 16), 0x1F)
    local old = probe.read_u32(slot_addr) or 0
    local new
    if op == 0x2B then -- sw
        new = rt == 0 and 0 or u32(g[REGN[rt]])
    elseif op == 0x29 then -- sh (to +0x10 or +0x12)
        local ea_lo = bit.band(u32(g[REGN[bit.band(bit.rshift(insn, 21), 0x1F)]])
            + bit.arshift(bit.lshift(bit.band(insn, 0xFFFF), 16), 16), 0xFFFFFFFF)
        local v = rt == 0 and 0 or bit.band(u32(g[REGN[rt]]), 0xFFFF)
        if ea_lo == slot_addr then
            new = bit.bor(bit.band(old, 0xFFFF0000), v)
        else
            new = bit.bor(bit.band(old, 0xFFFF), bit.lshift(v, 16))
        end
    else
        new = old -- sb or unknown: record, transition unresolved
    end
    local b_old = bit.band(old, 0x20000) ~= 0
    local b_new = bit.band(new, 0x20000) ~= 0
    local tr
    if op ~= 0x2B and op ~= 0x29 then tr = "op" .. op
    elseif b_old == b_new then tr = b_new and "keep1" or "keep0"
    elseif b_new then tr = "SET" else tr = "CLEAR" end
    total = total + 1
    local key = string.format("%08X/%s", pc, tr)
    local a = agg[key]
    if a then a.hits = a.hits + 1; return end
    a = { pc = pc, tr = tr, hits = 1, tick = g_tick, slot = slot_addr,
          old = old, new = new, ra = u32(g.ra) }
    agg[key] = a
    order[#order + 1] = key
    if tr ~= "keep0" and tr ~= "keep1" and detail < MAX_DETAIL then
        detail = detail + 1
        PCSX.log(string.format(
            "[mc-writer] %s pc=0x%08X ra=0x%08X slot=0x%08X old=0x%08X new=0x%08X tick=%d",
            tr, pc, a.ra, slot_addr - 0x10, old, new, g_tick))
    end
end

local function arm()
    for k = 0, NSLOTS - 1 do
        local addr = POOL_BASE + k * SLOT + 0x10
        probe.arm_breakpoint(addr, "Write", 4, string.format("mc_%08X", addr),
            function() on_write(addr) end)
    end
    probe.arm_breakpoint(SEATER_SW, "Exec", 4, "seater_sw_8003A3B4", function()
        seater_hits = seater_hits + 1
    end)
    armed = true
    PCSX.log(string.format("[mc-writer] armed %d Write BPs + Exec 0x%08X", NSLOTS, SEATER_SW))
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 700),
    snapshot_path  = probe.out_path("moving_class_writer.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(_, elapsed)
        g_tick = elapsed
        if not armed then arm() end
    end,
    on_done = function()
        for _, key in ipairs(order) do
            local a = agg[key]
            csv:row("0x%08X,%s,%d,%d,0x%08X,0x%08X,0x%08X,0x%08X",
                a.pc, a.tr, a.hits, a.tick, a.slot - 0x10, a.old, a.new, a.ra)
        end
        csv:close()
        PCSX.log(string.format(
            "[mc-writer] done. writes=%d distinct(pc,transition)=%d seater_sw_hits=%d",
            total, #order, seater_hits))
    end,
})
