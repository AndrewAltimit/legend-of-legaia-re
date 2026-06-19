-- autorun_clut_upload_watch_live.lua
-- Interactive capture: hooks the VRAM uploader FUN_80059bd4 and logs every
-- upload whose dest is a character texture page (x>=512) or the character
-- CLUT band (y 488..499), deduped by (x,y,src). NO injected input - the
-- USER plays the battle in the window so the party characters render and
-- their textures/CLUTs upload. Captures whichever path the battle chars use.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local UPLOAD = 0x80059BD4
local OUT = probe.getenv("LEGAIA_OUT_DIR","/tmp/clutprobe/live")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 7200)
os.execute(string.format("mkdir -p %q", OUT))
local csv = probe.csv_open(OUT.."/uploads.csv","tick,dst_x,dst_y,w,h,src_ptr,ra")
local function rd_u16(a) local b=probe.read_bytes(a,2); if b==nil then return -1 end local s=tostring(b); return s:byte(1)+s:byte(2)*256 end
local function dump(path,addr,len) if not probe.in_ram(addr,1) then return end local fh=io.open(path,"wb") if not fh then return end local o=0 while o<len do local n=math.min(0x4000,len-o) local c=probe.read_bytes(addr+o,n) if c==nil then break end fh:write(tostring(c)) o=o+n end fh:close() end
local seen={} local tick=0
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate5"),
  capture_frames = FRAMES, hold_frames = 0,
  out_path = OUT.."/uploads.csv",
  on_arm = function()
    probe.arm_breakpoint(UPLOAD,"Exec",4,"up",function()
      local r=PCSX.getRegisters()
      local rect=bit.band(tonumber(r.GPR.n.a0) or 0,0xFFFFFFFF)
      local src =bit.band(tonumber(r.GPR.n.a1) or 0,0xFFFFFFFF)
      local ra  =bit.band(tonumber(r.GPR.n.ra) or 0,0xFFFFFFFF)
      if not probe.in_ram(rect,8) then return end
      local x=rd_u16(rect+0); local y=rd_u16(rect+2); local w=rd_u16(rect+4); local h=rd_u16(rect+6)
      local is_charpage = (x>=512 and y<256)
      local is_band = (y>=488 and y<=499)
      if not (is_charpage or is_band) then return end
      local key=string.format("%d_%d_%08X",x,y,src)
      if seen[key] then return end
      seen[key]=true
      csv:row("%d,%d,%d,%d,%d,0x%08X,0x%08X",tick,x,y,w,h,src,ra)
      PCSX.log(string.format("[live] upload dst=(%d,%d) %dx%d src=0x%08X ra=0x%08X",x,y,w,h,src,ra))
      if is_band and probe.in_ram(src,1) then
        pcall(function() dump(string.format("%s/up_y%d_x%d_%08X.bin",OUT,y,x,src),src,512) end)
      end
    end)
    return {}
  end,
  on_capture = function(_c,e) tick=e end,
  on_done = function() csv:close(); PCSX.log("=== live upload watch done ===") end,
})
