-- Dump per-command AP cost DAT_801c9360[char][cmd]+0x74 for the active char,
-- to compare off-class (Nail) vs favored (Club) Gala and pin the arm-width field.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local function u8(a) return probe.read_u8(a) or -1 end
local function u32(a) return (probe.read_u32 and probe.read_u32(a)) or 0 end
local done=false
local function dump()
    PCSX.log(string.format("== AP-cost dump == gmode=0x%02X active(bd24+0x13)=0x%02X", u8(0x8007B83C), u8(0x8007BD24+0x13)))
    local active = u8(0x8007BD24+0x13)
    -- DAT_8007bd10[slot] -> char idx; show party
    local s="" for i=0,3 do s=s..string.format("%02X ", u8(0x8007BD10+i)) end
    PCSX.log("party bd10: "..s)
    for _,charslot in ipairs({active, 0,1,2}) do
        local base = u32(0x801C9360 + charslot*4)
        if base ~= 0 then
            local line = string.format("DAT_801c9360[%d]=0x%08X costs+0x74: ", charslot, base)
            for cmd=0x0c,0x11 do
                local p = u32(base + cmd*4)
                local cost = (p~=0) and u8(p+0x74) or -1
                line = line .. string.format("cmd%02X=%d ", cmd, cost)
            end
            PCSX.log(line)
        else
            PCSX.log(string.format("DAT_801c9360[%d]=NULL", charslot))
        end
    end
    -- also dump Gala's weapon for sanity (char idx 2 record)
    PCSX.log(string.format("Gala wpn(0x800850C8)=0x%02X", u8(0x800850C8)))
end
probe.run({ sstate=SSTATE_PATH, capture_frames=50,
    on_arm=function() return {} end,
    on_capture=function(c,e) if e==40 and not done then done=true; dump() end end })
