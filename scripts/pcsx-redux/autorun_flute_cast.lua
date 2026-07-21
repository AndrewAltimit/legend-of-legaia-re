-- autorun_flute_cast.lua -- capture a Lippian/Spikefish flute cast mid-summon.
-- Loads a mid-battle state, injects the flutes into inventory slots 0/1,
-- drives a pad script (LEGAIA_PAD_SCRIPT="frame:BTN:hold,..."), polls the
-- slot-B loader current-id 0x8007BC4C every vsync, and autosaves a state
-- + screenshot the moment it becomes 0x1D/0x1E (Lippian/Spikefish stager).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe  = require("probe")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local LOADER_B = 0x8007BC4C
local INV      = 0x80085958

local function shot(stem)
  local ok,ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
  if not ok or ss==nil then return end
  local bpp=(tonumber(ss.bpp) or 0)>16 and 24 or 16
  local h=io.open(probe.out_path(stem..".raw"),"wb"); if h then h:write(tostring(ss.data)); h:close() end
  local m=io.open(probe.out_path(stem..".meta"),"w"); if m then m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",tonumber(ss.width),tonumber(ss.height),bpp)); m:close() end
end

-- pad script: "frame:BTN:hold,frame:BTN:hold,..."
local script = {}
for tok in string.gmatch(probe.getenv("LEGAIA_PAD_SCRIPT",""), "[^,]+") do
  local f,b,h = string.match(tok, "(%d+):(%u+):(%d+)")
  if f then script[#script+1] = {f=tonumber(f), b=pad.BTN[b], h=tonumber(h), name=b} end
end
local shots = {}
for tok in string.gmatch(probe.getenv("LEGAIA_SHOT_FRAMES",""), "[^,]+") do
  shots[tonumber(tok)] = true
end

local last, saved = nil, false
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE",""),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 900),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 10 then
      -- inject flutes: slot 0 Lippian (0x98), slot 1 Spikefish (0x99)
      mem.write_u8(INV+0, 0x98); mem.write_u8(INV+1, 5)
      mem.write_u8(INV+2, 0x99); mem.write_u8(INV+3, 5)
      PCSX.log(string.format("[flute] inv injected: %02x x%d, %02x x%d",
        mem.read_u8(INV) or 0, mem.read_u8(INV+1) or 0,
        mem.read_u8(INV+2) or 0, mem.read_u8(INV+3) or 0))
    end
    for _,s in ipairs(script) do
      if el == s.f then pad.force(s.b); PCSX.log(string.format("[pad] +%s @%d", s.name, el)) end
      if el == s.f + s.h then pad.release(s.b) end
    end
    if shots[el] then shot(string.format("frame_%04d", el)) end
    local v = mem.read_u32(LOADER_B)
    if v ~= last then
      PCSX.log(string.format("[loaderB] frame %d id=0x%x", el, v or -1))
      last = v
    end
    if not saved and (v == 0x1D or v == 0x1E) then
      saved = true
      sstate.save(probe.out_path(string.format("flute_midcast_id%02x.sstate", v)))
      shot(string.format("midcast_id%02x", v))
      PCSX.log(string.format("[flute] MIDCAST id=0x%x SAVED @%d", v, el))
    end
    if el >= probe.getenv_num("LEGAIA_FRAMES", 900) - 2 then ctx.request_quit = true end
  end,
})
