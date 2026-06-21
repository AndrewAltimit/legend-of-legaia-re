-- autorun_tetsu_setup_bisect.lua
-- New slot 1 = field, just accepted the Tetsu match, transitioning INTO the
-- battle (battle setup has NOT run yet). Run forward so the PATCHED setup code
-- runs fresh, and bisect: LEGAIA_REVERT selects what to neutralise at load.
--   none : run patched as-is (confirm the freeze)
--   scus : revert the SCUS hooks (charm/shiny-B/K/J) + zero the SCUS arenas
--   all  : also revert the overlay-0898 hooks + the victory widen (re-asserted
--          each frame so an overlay (re)load can't undo them)
-- Success = the command-menu HUD appears (modeCtr advances); freeze = Tetsu pose.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local REV = probe.getenv("LEGAIA_REVERT", "none")
local f = assert(io.open(probe.out_path("tetsu_setup_"..REV..".txt"), "w"))
local function w(s) f:write(s.."\n"); f:flush() end
local function wr32(va,val) probe.write_u16(va,val%0x10000); probe.write_u16(va+2,math.floor(val/0x10000)%0x10000) end
local SCUS={{0x80051990,0x3C038008,0x9063BD0C},{0x80051A20,0x3C028008,0x3C038008},
            {0x8004AD0C,0x92220226,0x00000000},{0x800321D4,0x8E840018,0x24020007}}
local OV={{0x801EE2E8,0xA0820269,0x8E630000},{0x801E93B4,0xA0430729,0xAC4005D0},
          {0x801E9320,0x90460704,0x00000000},{0x801DDB08,0x90420729,0x8CC30000},
          {0x801D1B00,0x90430729,0x92220002},{0x801E6638,0x30420004,0x10400005}}
local ARENAS={{0x80077728,0x80077828},{0x8007AE00,0x8007AF00},{0x8007AFF6,0x8007B040},
              {0x80070759,0x8007078C},{0x8007933D,0x80079378},{0x80079509,0x80079544}}
local function revert_scus() for _,e in ipairs(SCUS) do wr32(e[1],e[2]); wr32(e[1]+4,e[3]) end
  for _,a in ipairs(ARENAS) do local v=a[1]; while v<a[2] do probe.write_u16(v,0); v=v+2 end end end
local function revert_ov() for _,e in ipairs(OV) do wr32(e[1],e[2]); wr32(e[1]+4,e[3]) end end
local function ctr() local p=probe.read_u32(0x8007BD24) or 0
  if p<0x80000000 or p>=0x80200000 then return 0xFFFF end return probe.read_u16(p+0x28A) or 0xFFFF end
local function take(stem) local ss=PCSX.GPU.takeScreenShot()
  local fh=io.open(probe.out_path(stem..".raw"),"wb"); fh:write(tostring(ss.data)); fh:close()
  local mh=io.open(probe.out_path(stem..".raw.meta"),"w")
  mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),(tonumber(ss.bpp) or 16)>16 and 24 or 16)); mh:close() end
probe.run({
  sstate=probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate1"),
  capture_frames=520,
  on_arm=function() return {} end,
  scus_done=false,
  on_capture=function(ctx,el)
    if el>=1 and not ctx.scus_done and (REV=="scus" or REV=="all") then revert_scus(); ctx.scus_done=true end
    if (REV=="all") then revert_ov() end  -- every frame (overlay loads mid-transition)
    for _,fr in ipairs({150,300,450,500}) do if el==fr then
      w(string.format("frame%-4d modeCtr=%04X formation0=%02X", fr, ctr(), probe.read_u8(0x8007BD0C) or 0xFF))
      take("tetsu_setup_"..REV.."_f"..fr) end end
    if el>=505 then w("done"); f:close(); ctx.request_quit=true end
  end,
})
