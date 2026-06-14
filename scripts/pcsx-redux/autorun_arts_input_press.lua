-- autorun_arts_input_press.lua
--
-- On the pre-build arts-input state (Gala + off-class Nail Glove, button just
-- pressed to begin inputs), let the gauge draw, then INJECT directional presses
-- to actually input arts commands. Read-watch all three party weapon bytes: the
-- arm-width consult fires when an off-class ARM direction is committed to the
-- gauge. $ra of any reader names the arm-width builder + the per-weapon class data.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 360)
local GMODE = 0x8007B83C
local WEAPON = {
    { name = "Vahn", addr = 0x800848A0 },
    { name = "Noa",  addr = 0x80084CB4 },
    { name = "Gala", addr = 0x800850C8 },
}
local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or 0xff end
local readers, total = {}, 0
probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== arts INPUT-press weapon watch == Gala.wpn=0x%02X gmode=0x%02X",
            u8(WEAPON[3].addr), u8(GMODE)))
        for _, w in ipairs(WEAPON) do
            probe.arm_breakpoint(w.addr, "Read", 1, "rd_" .. w.name, function()
                local r = PCSX.getRegisters()
                local pc = tou32(r.pc); local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
                total = total + 1
                if readers[pc] == nil then
                    readers[pc] = { ra = ra, who = w.name, val = u8(w.addr), gm = u8(GMODE) }
                    PCSX.log(string.format("[READER new] pc=0x%08X ra=0x%08X %s val=0x%02X gmode=0x%02X",
                        pc, ra, w.name, readers[pc].val, readers[pc].gm))
                end
            end)
        end
        return {}
    end,
    on_capture = function(c, elapsed)
        -- After the gauge draws (~frame 20), inject a rotating direction sequence.
        -- Each direction held ~10 frames then released, cycling U/D/L/R/X repeatedly
        -- so we commit several arm + non-arm commands into the gauge.
        if elapsed > 20 then
            local phase = math.floor((elapsed - 20) / 12) % 5
            local btn = ({ probe.BTN.UP, probe.BTN.DOWN, probe.BTN.LEFT, probe.BTN.RIGHT, probe.BTN.CROSS })[phase + 1]
            local sub = (elapsed - 20) % 12
            for _, b in ipairs({ probe.BTN.UP, probe.BTN.DOWN, probe.BTN.LEFT, probe.BTN.RIGHT, probe.BTN.CROSS }) do
                probe.pad_release(b)
            end
            if sub < 4 then probe.pad_force(btn) end   -- press for 4 frames, release 8 (clean edges)
        end
        if elapsed % 60 == 0 then
            PCSX.log(string.format("[diag t%d] gmode=0x%02X total_reads=%d", elapsed, u8(GMODE), total))
        end
    end,
})
