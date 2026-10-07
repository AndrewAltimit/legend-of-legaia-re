-- autorun_weapon_turn_init.lua
--
-- Catch the BATTLE TURN-INIT weapon-specialty lookup. The per-Arms-input width
-- (favored = single, off-class / Astral = double) is computed when a character's
-- input turn begins, from the equipped weapon id (char+0x198) -- not at equip
-- time, not while sitting at the bar. So: read-watch all three party weapon
-- bytes, then DRIVE the battle (spam combo inputs to execute turns) so the next
-- actor's turn-init fires, and flag any NOVEL reader pc (one not in the known
-- mid-frame mesh/passive/menu set) -- that pc is the width builder, and the code
-- around it indexes the per-weapon class table the randomizer needs.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_weapon_turn_init.lua \
--     --scenario arts_bar_offclass_gala_nail --frames 2000

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2000)

local CTX = 0x800EB654          -- _DAT_8007bd24 battle ctx
local ACTIVE_OFF = 0x13         -- ctx[+0x13] = active actor slot index

local WEAPON_BYTES = {
    { name = "Vahn", addr = 0x800848A0 },
    { name = "Noa",  addr = 0x80084CB4 },
    { name = "Gala", addr = 0x800850C8 },
}

-- Known mid-frame readers of char+0x198 (mesh assembly / passive aggregator /
-- TMD-pose copier / command-menu display). Any reader OUTSIDE these is novel and
-- worth reporting -- a turn-init / width-builder candidate.
local KNOWN = {
    [0x800529D0] = true,                                   -- FUN_80052770 mesh assembly
    [0x80054038] = true, [0x8005409C] = true,
    [0x80054100] = true, [0x80054250] = true,              -- FUN_800536xx mesh splice
    [0x80042620] = true, [0x80042690] = true, [0x80042718] = true, -- FUN_80042558 passive
    [0x8001EC74] = true, [0x8001ECE4] = true,              -- FUN_8001EBEC pose copier
    [0x801ECC00] = true, [0x801ECD50] = true,              -- FUN_801ECA08 menu display
}

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function active() return probe.read_u8(CTX + ACTIVE_OFF) or 0xFF end

local novel = {}      -- pc -> count
local total = 0

local function arm_reads()
    for _, w in ipairs(WEAPON_BYTES) do
        probe.arm_breakpoint(w.addr, "Read", 1, "wpn_" .. w.name, function()
            local r = PCSX.getRegisters()
            local pc = tou32(r.pc)
            local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
            total = total + 1
            if not KNOWN[pc] then
                novel[pc] = (novel[pc] or 0) + 1
                if novel[pc] <= 4 then
                    PCSX.log(string.format(
                        "[NOVEL-READ] %s wpn=0x%02X pc=0x%08X ra=0x%08X  active_slot=%d  (novel pc #%d)",
                        w.name, probe.read_u8(w.addr) or 0, pc, ra, active(), novel[pc]))
                end
            end
        end)
    end
    PCSX.log("[probe] armed Read-watch on 3 weapon bytes; driving turns")
end

local last_active = -1
-- Combo-drive timeline: spam attack directions + an execute press, cycling so
-- each turn's input fills and executes, advancing the battle through turns.
local SEQ = { probe.BTN.RIGHT, probe.BTN.UP, probe.BTN.LEFT, probe.BTN.DOWN,
              probe.BTN.RIGHT, probe.BTN.CROSS }
local HOLD, GAP = 4, 6
local CYCLE = HOLD + GAP
local held = nil

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        PCSX.log("== weapon turn-init lookup trace ==")
        arm_reads()
        return {}
    end,
    on_capture = function(c, elapsed)
        -- Log turn boundaries (active actor changes) so a novel read near one is
        -- recognisable as turn-init.
        local a = active()
        if a ~= last_active then
            PCSX.log(string.format("[turn] active_slot %d -> %d at t%d", last_active, a, elapsed))
            last_active = a
        end
        -- Drive inputs continuously.
        if elapsed > 12 then
            local idx = (math.floor((elapsed - 12) / CYCLE) % #SEQ) + 1
            local phase = (elapsed - 12) % CYCLE
            if phase == 0 then
                if held ~= nil then probe.pad_release(held) end
                probe.pad_force(SEQ[idx]); held = SEQ[idx]
            elseif phase == HOLD then
                if held ~= nil then probe.pad_release(held); held = nil end
            end
        end
        if elapsed % 150 == 0 then
            local ks = {}
            for pc, n in pairs(novel) do ks[#ks + 1] = string.format("0x%08X(%d)", pc, n) end
            PCSX.log(string.format("[diag t%d] total_reads=%d novel_pcs=%d  %s",
                elapsed, total, #ks, table.concat(ks, " ")))
        end
    end,
})
