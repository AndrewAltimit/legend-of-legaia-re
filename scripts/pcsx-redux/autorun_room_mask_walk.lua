-- autorun_room_mask_walk.lua
--
-- Does a walk rewrite the region-type mask? Load a field state, poke the
-- player to (LEGAIA_PX, LEGAIA_PZ) at vsync 120 (a poke, not a landing: no
-- re-centre runs), hold LEGAIA_BTN from vsync 180, and log every 8 vsyncs the
-- player position and tile, the mask `_DAT_8007B8F4`, the MAN-header floor
-- ladder (`*(_DAT_8007B898) + 2`), the live ladder (scratchpad `0x1F80035C`)
-- and the visible tile window (scratchpad `0x1F8003E8..EB`).
--
-- The answer it gave on `conc_field_card_boot`, poked to (7152, 12256) and
-- walked UP through conc's southern hall: the mask held `0x9` (the start
-- building's latch) on every hall tile, whose region table says `0x5`, and the
-- ladder and window never moved (docs/subsystems/field-locomotion.md, "The
-- region-type mask is a latch").
--
-- Env: LEGAIA_SSTATE, LEGAIA_PX, LEGAIA_PZ, LEGAIA_BTN (default UP),
-- LEGAIA_FRAMES (default 700). Output: hall_latch.log. --fast is fine (no
-- breakpoints).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local OUT = probe.out_path("hall_latch.log")
local PX = probe.getenv_num("LEGAIA_PX", 7152)
local PZ = probe.getenv_num("LEGAIA_PZ", 12256)
local BTN1 = probe.getenv("LEGAIA_BTN", "UP")
local lines = {}
local function logf(fmt, ...) local s = string.format(fmt, ...); lines[#lines+1] = s; PCSX.log("[hall] " .. s) end
local function s16(v) if v and v >= 0x8000 then return v - 0x10000 end return v end
local function state(v)
  local p = probe.read_u32(0x8007c364)
  local x, z = s16(probe.read_u16(p + 0x14)), s16(probe.read_u16(p + 0x18))
  local y = s16(probe.read_u16(p + 0x16))
  local mask = probe.read_u32(0x8007b8f4)
  local lp = probe.read_u32(0x8007b898)
  local lad = {}
  for i = 0, 4 do lad[#lad+1] = tostring(s16(probe.read_u16(lp + 2 + i*2))) end
  local vw = {}
  for i = 0, 3 do local b = probe.read_scratch_u8(0x1f8003e8 + i); if b >= 0x80 then b = b - 0x100 end; vw[#vw+1] = tostring(b) end
  local live = {}
  for i = 0, 4 do local a = probe.read_scratch_u8(0x1f80035c + i*2); local b2 = probe.read_scratch_u8(0x1f80035d + i*2); live[#live+1] = tostring(s16(a + b2*256)) end
  logf("v=%d mode=%02x pos=(%d,%d,%d) tile=(%d,%d) mask=0x%x manlad=[%s] live=[%s] vw=[%s]", v,
    probe.read_u8(0x8007b83c), x, y, z, math.floor((x-64)/128), math.floor((z-64)/128), mask, table.concat(lad, ","), table.concat(live, ","), table.concat(vw, ","))
end
local held = false
probe.run({
  sstate = SSTATE,
  capture_frames = probe.getenv_num("LEGAIA_FRAMES", 700),
  on_arm = function(_) return {} end,
  on_capture = function(_, v)
    if v == 100 then state(v) end
    if v == 120 then
      local p = probe.read_u32(0x8007c364)
      probe.write_u16(p + 0x14, PX); probe.write_u16(p + 0x18, PZ)
      logf("poked (%d,%d)", PX, PZ)
    end
    if v == 180 then probe.pad.force(probe.BTN[BTN1]); held = true end
    if v >= 100 and v % 8 == 0 then state(v) end
  end,
  on_done = function(_, _)
    if held then probe.pad.release(probe.BTN[BTN1]) end
    local fh = io.open(OUT, "w"); fh:write(table.concat(lines, "\n") .. "\n"); fh:close()
  end,
})
