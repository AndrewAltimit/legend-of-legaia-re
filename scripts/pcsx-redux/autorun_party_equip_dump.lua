package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local function u8(a) return probe.read_u8(a) or -1 end
local function u32(a) return (probe.read_u32 and probe.read_u32(a)) or 0 end
local done = false
local function dump(tag)
    PCSX.log("== party + equip dump ("..tag..") ==")
    PCSX.log(string.format("gmode=0x%02X  bd24[0x13]active=0x%02X", u8(0x8007B83C), u8(0x8007BD24+0x13)))
    local s = "" for i=0,5 do s = s .. string.format("%02X ", u8(0x8007BD10+i)) end
    PCSX.log("DAT_8007bd10 party slot->charidx: " .. s)
    for n=0,3 do
        local base = 0x80084708 + n*0x414
        local eq = "" for k=0,7 do eq = eq .. string.format("%02X ", u8(base+0x196+k)) end
        local nm = "" for k=0,9 do local ch=u8(base+0x1b8+k); if ch>=32 and ch<127 then nm=nm..string.char(ch) else nm=nm.."." end end
        PCSX.log(string.format("char[%d] @0x%08X eq[196..]=%s wpn(+198)=0x%02X name='%s'",
            n, base, eq, u8(base+0x198), nm))
    end
    for i=0,5 do PCSX.log(string.format("actor_ptr[%d]=0x%08X", i, u32(0x801C9370+i*4))) end
end
probe.run({
    sstate = SSTATE_PATH, capture_frames = 60,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed == 40 and not done then done = true; dump("t40 settled") end
    end,
})
