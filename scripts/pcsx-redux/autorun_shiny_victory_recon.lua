-- autorun_shiny_victory_recon.lua
--
-- Issue: Vahn's mouth texture is corrupted during the victory pose on the shiny
-- ROM (slot 9). Suspect: the persistent shiny array at record+0x1C0 (= 0x80 for
-- char 0 after capturing a shiny Gimard) is read somewhere in the victory /
-- level-up display, OR the summon-fade override (SHINY_CAST_FLAG + actor slot 7)
-- is leaking onto Vahn. This read-only recon dumps the suspect live state across
-- the victory frames so we can tell which mechanism is active.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local REC0 = 0x80084708           -- char 0 record base
local RECSZ = 0x414
local SHINY_CAST_FLAG = 0x80078358
local ACTOR_TBL = 0x801C9370      -- slot i at +i*4
local OUT = probe.out_path("shiny_victory_recon.txt")
local f = assert(io.open(OUT, "w"))

local function dump(tag)
  f:write(string.format("== %s ==\n", tag))
  f:write(string.format("SHINY_CAST_FLAG[0x%08X] = 0x%02X\n",
    SHINY_CAST_FLAG, probe.read_u8(SHINY_CAST_FLAG) or 0xFF))
  for s = 0, 8 do
    local p = probe.read_u32(ACTOR_TBL + s * 4) or 0
    f:write(string.format("actor[%d] = 0x%08X\n", s, p))
  end
  for c = 0, 3 do
    local b = REC0 + c * RECSZ
    local cnt = probe.read_u8(b + 0x13C) or 0
    local ids, lvls, shy = {}, {}, {}
    for i = 0, 11 do
      ids[#ids+1]  = string.format("%02X", probe.read_u8(b + 0x13D + i) or 0)
      lvls[#lvls+1]= string.format("%02X", probe.read_u8(b + 0x161 + i) or 0)
      shy[#shy+1]  = string.format("%02X", probe.read_u8(b + 0x1C0 + i) or 0)
    end
    f:write(string.format("char%d cnt=%d ids=[%s]\n  lvls=[%s]\n  shy@1C0=[%s]\n",
      c, cnt, table.concat(ids," "), table.concat(lvls," "), table.concat(shy," ")))
  end
  f:flush()
end

probe.run({
  sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME").."/Tools/pcsx-redux/SCUS94254.sstate9"),
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 120),
  on_arm = function() return {} end,
  on_capture = function(ctx, el)
    if el == 1 then dump("frame1") end
    if el == 40 then dump("frame40") end
    if el == 90 then dump("frame90"); ctx.request_quit = true end
  end,
  on_summary = function() f:write("done\n"); f:close() end,
})
