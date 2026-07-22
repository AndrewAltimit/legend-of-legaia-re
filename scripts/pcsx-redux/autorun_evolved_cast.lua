-- autorun_evolved_cast.lua -- capture an evolved-Seru summon cast mid-summon.
-- Sibling of autorun_flute_cast.lua for the two arithmetic-predicted legs
-- (spell 0x90 Kemaro -> loader-B 0x17 -> extraction 918, spell 0x91 Spoon ->
-- loader-B 0x18 -> extraction 919). Loads a mid-battle state, injects the
-- target spell into the caster's character-record spell list (+0x13C count /
-- +0x13D ids / +0x161 levels) plus a 999 MP grant (current + effective max +
-- base max, so the per-frame stat aggregator FUN_80042558 rebuilds the max
-- from the patched base instead of clamping the grant away), drives a pad
-- script (LEGAIA_PAD_SCRIPT="frame:BTN:hold,..."), polls the slot-B loader
-- current-id 0x8007BC4C every vsync, and autosaves a state + screenshot the
-- moment it becomes the predicted spell - 0x79.
--
-- Env knobs:
--   LEGAIA_SPELL_ID   spell to inject/cast (default 0x90)
--   LEGAIA_CHAR_SLOT  character record slot 0..3 (default 0 = Vahn)
--   LEGAIA_PAD_SCRIPT / LEGAIA_SHOT_FRAMES / LEGAIA_FRAMES / LEGAIA_SSTATE
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe  = require("probe")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local LOADER_B = 0x8007BC4C
local CHAR_REC = 0x80084708 -- + slot * 0x414

local SPELL  = probe.getenv_num("LEGAIA_SPELL_ID", 0x90)
local SLOT   = probe.getenv_num("LEGAIA_CHAR_SLOT", 0)
local TARGET = SPELL - 0x79

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

-- Save DELAY: the loader-B id flips the moment the slot-B load is *queued*,
-- while the CD stream is still in flight (an at-flip save catches only the
-- first resident sector). Wait this many frames after the flip before the
-- canonical autosave so the whole stager is byte-resident.
local SAVE_DELAY = probe.getenv_num("LEGAIA_SAVE_DELAY", 90)

local last, saved, hit_frame = nil, false, nil
probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE",""),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 900),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 10 then
      -- inject the spell as the caster's only learned spell (row 0 of the
      -- battle Magic submenu) + a 999 MP grant that survives the aggregator.
      local rec = CHAR_REC + SLOT * 0x414
      mem.write_u8(rec + 0x13C, 1)       -- spell-list count
      mem.write_u8(rec + 0x13D, SPELL)   -- spell id
      mem.write_u8(rec + 0x161, 2)       -- spell level (parallel array)
      mem.write_u16(rec + 0x108, 999)    -- effective max MP
      mem.write_u16(rec + 0x10A, 999)    -- current MP
      mem.write_u16(rec + 0x11E, 999)    -- base max MP (aggregator source)
      -- The battle HUD + the magic-menu MP gate read the battle-actor runtime
      -- struct (actor +0x150 MP), seeded at battle load - patch it too.
      local actor = mem.read_u32(0x801C9370 + SLOT * 4)
      if actor and actor >= 0x80000000 and actor < 0x80200000 then
        local before = mem.read_u16(actor + 0x150)
        mem.write_u16(actor + 0x150, 999)
        PCSX.log(string.format("[evolved] actor[%d]=%08x mp %d -> %d",
          SLOT, actor, before or -1, mem.read_u16(actor + 0x150) or -1))
      end
      PCSX.log(string.format("[evolved] rec[%d] injected: spell=%02x lvl=%d mp=%d/%d",
        SLOT, mem.read_u8(rec + 0x13D) or 0, mem.read_u8(rec + 0x161) or 0,
        mem.read_u16(rec + 0x10A) or 0, mem.read_u16(rec + 0x108) or 0))
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
    if hit_frame == nil and v == TARGET then
      hit_frame = el
      PCSX.log(string.format("[evolved] loader-B hit 0x%x @%d; saving in %d frames", v, el, SAVE_DELAY))
    end
    if not saved and hit_frame ~= nil and el >= hit_frame + SAVE_DELAY then
      saved = true
      if v == TARGET then
        sstate.save(probe.out_path(string.format("evolved_midcast_id%02x.sstate", TARGET)))
        shot(string.format("midcast_id%02x", TARGET))
        PCSX.log(string.format("[evolved] MIDCAST id=0x%x SAVED @%d", TARGET, el))
      else
        PCSX.log(string.format("[evolved] NOT SAVED: loader-B moved to 0x%x before the delay elapsed", v or -1))
      end
    end
    if el >= probe.getenv_num("LEGAIA_FRAMES", 900) - 2 then ctx.request_quit = true end
  end,
})
