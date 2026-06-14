-- autorun_armcost_writer.lua
--
-- Catch the WRITER of the weapon-specialty arm cost DAT_801c9360[char][0x0C]+0x74
-- (off-class Gala+Nail-Glove = 0x2A vs favored 0x1E). The field is recomputed from
-- the equipped weapon when arts input initializes; this write-watches the exact byte
-- (resolved live from the pointer chain) while injecting a cancel->reselect-Arts
-- cycle to force a re-entry. The writer pc + nearby reads = the favored-class
-- comparison the randomizer needs.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 480)
local function tou32(v) v=tonumber(v) or 0 if v<0 then v=v+0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or -1 end
local function u32(a) return probe.read_u32(a) or 0 end

local GALA_IDX = 2                  -- DAT_801c9360[2] = Gala command data
local ARM_CMD  = 0x0C
local field = 0
local writes = {}
local nwrite = 0

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed == 10 and field == 0 then
            local base = u32(0x801C9360 + GALA_IDX*4)
            local cmdptr = (base ~= 0) and u32(base + ARM_CMD*4) or 0
            field = (cmdptr ~= 0) and (cmdptr + 0x74) or 0
            PCSX.log(string.format("== armcost writer == DAT_801c9360[%d]=0x%08X cmd0C=0x%08X field=0x%08X val=0x%02X gmode=0x%02X",
                GALA_IDX, base, cmdptr, field, field~=0 and u8(field) or -1, u8(0x8007B83C)))
            if field ~= 0 then
                probe.arm_breakpoint(field, "Write", 1, "armcost", function()
                    local r = PCSX.getRegisters()
                    local pc = tou32(r.pc); local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
                    nwrite = nwrite + 1
                    if writes[pc] == nil then
                        writes[pc] = {ra=ra, val=u8(field), gm=u8(0x8007B83C)}
                        PCSX.log(string.format("[WRITER] pc=0x%08X ra=0x%08X val=0x%02X gmode=0x%02X",
                            pc, ra, writes[pc].val, writes[pc].gm))
                    end
                end)
            end
        end
        -- Force arts re-entry: cycle cancel (TRIANGLE/CROSS) then confirm (CIRCLE)
        -- + directions, so the menu bounces out of and back into arts input.
        for _,b in ipairs({probe.BTN.UP,probe.BTN.DOWN,probe.BTN.LEFT,probe.BTN.RIGHT,
                           probe.BTN.CROSS,probe.BTN.CIRCLE,probe.BTN.TRIANGLE}) do probe.pad_release(b) end
        if elapsed > 15 then
            local phase = math.floor((elapsed-15)/8) % 6
            local btn = ({probe.BTN.TRIANGLE, probe.BTN.CROSS, probe.BTN.CIRCLE,
                          probe.BTN.DOWN, probe.BTN.CIRCLE, probe.BTN.CROSS})[phase+1]
            if (elapsed-15) % 8 < 3 then probe.pad_force(btn) end
        end
        if elapsed % 80 == 0 then
            PCSX.log(string.format("[diag t%d] gmode=0x%02X writes=%d field=0x%08X val=0x%02X",
                elapsed, u8(0x8007B83C), nwrite, field, field~=0 and u8(field) or -1))
        end
    end,
})
