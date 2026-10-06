-- Dump Gala's cmd0C command struct (DAT_801c9360[2][0x0C] -> +0x0..0xC0) from the
-- deterministic slot-1 save. This is a verbatim image of the decompressed player-file
-- equipment section; the cost byte is at +0x74. Used to RAM->file match the offset.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local done=false
probe.run({ sstate=SSTATE_PATH, capture_frames=50,
  on_arm=function() return {} end,
  on_capture=function(c,e)
    if e~=40 or done then return end
    done=true
    local base=u32(0x801C9360+2*4)
    local s=u32(base+0x0C*4)
    PCSX.log(string.format("gmode=0x%02X DAT_801c9360[2]=0x%08X cmd0C=0x%08X cost(+0x74)=0x%02X", u8(0x8007B83C), base, s, u8(s+0x74)))
    -- dump struct +0 .. +0xC0 in 16-byte rows
    for row=0,0xB do
      local a=s+row*16
      local line=string.format("+%02X: ", row*16)
      for i=0,15 do line=line..string.format("%02x ", u8(a+i)) end
      PCSX.log(line)
    end
  end })
