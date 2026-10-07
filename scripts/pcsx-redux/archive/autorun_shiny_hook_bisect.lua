-- autorun_shiny_hook_bisect.lua
--
-- Bisect which shiny-patch hook corrupts Vahn's victory-pose mouth. Load slot 7
-- (before the Gimard cast), revert ONE hook's detour back to its original two
-- words in RAM, drive the cast -> win -> victory, screenshot the pose. The hook
-- whose removal yields a clean mouth is the culprit. (Save state has the patched
-- overlays resident, so reverting in RAM is the valid test - the disc is moot.)
--   REVERT=none : leave all hooks (baseline; should reproduce corruption)
--   REVERT=K    : summon-fade hook FUN_8004a908 @0x8004AD0C
--   REVERT=J    : banner hook FUN_80031d00 @0x800321D4
--   REVERT=KJ   : both
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local REVERT = probe.getenv("LEGAIA_REVERT", "none")

-- hook VA -> {orig word0, orig word1}  (the two instructions our detour replaced)
local HOOKS = {
  K = { va = 0x8004AD0C, w0 = 0x92220226, w1 = 0x00000000 }, -- lbu v0,0x226(s1) ; nop
  J = { va = 0x800321D4, w0 = 0x8E840018, w1 = 0x24020007 }, -- lw a0,0x18(s4) ; li v0,7
}

local function wr32(va, w)
  probe.write_u16(va, w % 0x10000)
  probe.write_u16(va + 2, math.floor(w / 0x10000) % 0x10000)
end

local function revert(name)
  local h = HOOKS[name]; if not h then return end
  wr32(h.va, h.w0); wr32(h.va + 4, h.w1)
  PCSX.log(string.format("[bisect] reverted %s @0x%08X", name, h.va))
end

local function take_fb(stem)
  local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
  if not ok or ss == nil then return end
  local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
  local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
  if fh then fh:write(tostring(ss.data)); fh:close() end
  local mh = io.open(probe.out_path(stem .. ".meta"), "w")
  if mh then mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
    tonumber(ss.width), tonumber(ss.height), bpp)); mh:close() end
  PCSX.log("[bisect] wrote " .. stem)
end

local done = false
probe.run({
  -- Slot 8 (mid-cast). No input: the cast -> enemy death -> victory pose all
  -- auto-play, then the game HOLDS at the "won the battle!" screen (= slot 9).
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate8"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 900),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if not done and el >= 1 then
      if REVERT == "KJ" then revert("K"); revert("J")
      elseif REVERT ~= "none" then revert(REVERT) end
      done = true
    end
    -- Dense capture across the cast-resolve -> victory window.
    if el >= 320 and el <= 500 and (el % 12 == 0) then take_fb(REVERT .. "_f" .. el) end
    if el >= 505 then ctx.request_quit = true end
  end,
})
