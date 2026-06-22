-- autorun_tetsu_idle_probe.lua
-- Slot 2: Tetsu tutorial, "spinning around the battle map but not taking input"
-- after a Healing Leaf (shiny+charm ROM, post-alignment-fix). Characterize the
-- stuck state: is the battle SM advancing (modeCtr) or frozen? what's the active
-- actor + its queued action? is the command menu expected? party HP (did the leaf
-- apply?). Also dump SHINY_CAST_FLAG + actor charm flags.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local f = assert(io.open(probe.out_path("tetsu_idle.txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local u8=function(a) return probe.read_u8(a) or 0xFF end
local u16=function(a) return probe.read_u16(a) or 0xFFFF end
local u32=function(a) return probe.read_u32(a) or 0 end
local function dump(tag)
  local c=u32(0x8007BD24)
  local mode = (c>=0x80000000 and c<0x80200000) and u16(c+0x28A) or 0xFFFF
  local act  = (c>=0x80000000 and c<0x80200000) and u8(c+0x13) or 0xFF
  w(string.format("%s ctx=0x%08X active=%02X modeCtr=%04X  SHINY_CAST_FLAG=%02X",
    tag, c, act, mode, u8(0x80078358)))
end
local function take(stem)
  local ss=PCSX.GPU.takeScreenShot()
  local fh=io.open(probe.out_path(stem..".raw"),"wb"); fh:write(tostring(ss.data)); fh:close()
  local mh=io.open(probe.out_path(stem..".raw.meta"),"w")
  mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),(tonumber(ss.bpp) or 16)>16 and 24 or 16)); mh:close()
end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate2"),
  capture_frames=140,
  on_arm=function() return {} end,
  on_capture=function(ctx,el)
    if el==5 then dump("f5") end
    if el==70 then dump("f70") end
    if el==130 then
      dump("f130")
      -- battle actors: party 0..2, enemy 3. action byte +0x1df, flags +0x16E, HP +0x14C
      for i=0,3 do
        local a=0x800EC9E8 + i*0x2D4
        w(string.format("  actor[%d] +0x1df(act)=%02X +0x16E=%04X HP+0x14C=%04X +0x9a8(queued)=%02X",
          i, u8(a+0x1df), u16(a+0x16E), u16(a+0x14C), u8(a+0x9a8)))
      end
      take("tetsu_idle")
      w("done"); f:close(); ctx.request_quit=true
    end
  end,
})
