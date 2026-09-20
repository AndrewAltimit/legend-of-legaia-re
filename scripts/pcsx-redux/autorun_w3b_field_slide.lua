-- autorun_w3b_field_slide.lua
--
-- Retail pin for the wall-slide direction resolver `FUN_80046494`
-- (`SCUS_942.54`), the routine `World::resolve_field_slide` ports.
--
-- The field locomotion controller `FUN_801D01B0` (PROT 0897, base
-- 0x801CE818) calls it at `0x801D03EC` right after the camera remap
-- `func_0x800467E8`, and keeps the result in `s0` at `0x801D0404`. This
-- probe taps the RETURN site `0x801D03F4` and records, per call:
--
--   v0                     the resolved direction mask (bits & 0xF000)
--   *(0x8007B850)          the camera-remapped held pad mask the resolver read
--   player +0x14 / +0x18   the position it resolved at
--
-- A call whose resolved nibble carries a perpendicular bit the held mask
-- does not is a SKID: retail walked the player along a wall in a direction
-- the pad never asked for. No pokes - the pad is driven through
-- `probe.pad_force`, so every row is a real retail step.
--
-- Taps are word-asserted (a paged-out overlay reports MISMATCH, not a zero
-- census):
--   0x801D03EC = 0x0C011925  jal 0x80046494
--   0x801D03F4 = 0x3C038008  lui v1,0x8008   (the return landing)
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario s3_rimelm_freeroam \
--       --lua scripts/pcsx-redux/autorun_w3b_field_slide.lua --frames 1600
--
-- Env: LEGAIA_HOLD_FRAMES  vsyncs to hold each cardinal (default 240)
--
-- Outputs: w3b_field_slide.csv (one row per resolver call), .log (summary)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1600)
local HOLD   = probe.getenv_num("LEGAIA_HOLD_FRAMES", 240)

local OUT_CSV = probe.out_path("w3b_field_slide.csv")
local OUT_LOG = probe.out_path("w3b_field_slide.log")

local PLAYER_PTR = 0x8007C364
local PAD_MASK   = 0x8007B850
local SCENE      = 0x8007050C
local GAME_MODE  = 0x8007B83C
local RET_SITE   = 0x801D03F4
local RET_WORD   = 0x3C038008
local CALL_SITE  = 0x801D03EC
local CALL_WORD  = 0x0C011925

local lines = {}
local function logf(fmt, ...)
  local s = string.format(fmt, ...)
  lines[#lines+1] = s
  PCSX.log("[slide] " .. s)
end
local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function s16(v)
  v = bit.band(v or 0, 0xFFFF)
  if v >= 0x8000 then return v - 0x10000 end
  return v
end
local function scene_name()
  local out = {}
  for i = 0, 7 do
    local b = probe.read_u8(SCENE + i)
    if b == nil or b < 0x20 or b >= 0x7F then break end
    out[#out+1] = string.char(b)
  end
  return table.concat(out)
end

-- The four d-pad buttons, held one at a time.
local HOLDS = { probe.BTN.UP, probe.BTN.DOWN, probe.BTN.LEFT, probe.BTN.RIGHT }
local hold_i, held_btn = 0, nil

local csv, g_elapsed, rows = nil, 0, 0
local calls, skids = 0, 0
local pairs_seen = {}

probe.run({
  sstate = SSTATE,
  capture_frames = FRAMES,
  on_arm = function()
    csv = probe.csv_open(OUT_CSV, "seq,vsync,btn,held,resolved,skid,px,pz,mode,scene")
    probe.env.write_manifest("autorun_w3b_field_slide.lua",
      { sstate = SSTATE, frames = FRAMES, hold = HOLD })
    local d = { addr = RET_SITE, hits_ref = { n = 0 }, name = "resolver return" }
    probe.arm_breakpoint(RET_SITE, "Exec", 4, "ret", function()
      if n32(probe.read_u32(RET_SITE) or 0) ~= RET_WORD then return end
      d.hits_ref.n = d.hits_ref.n + 1
      calls = calls + 1
      local r = PCSX.getRegisters()
      local resolved = bit.band(n32(r.GPR.n.v0), 0xF000)
      local held = bit.band(probe.read_u32(PAD_MASK) or 0, 0xF000)
      local skid = (resolved ~= held) and 1 or 0
      if skid == 1 then skids = skids + 1 end
      local pp = probe.read_u32(PLAYER_PTR) or 0
      local px, pz = 0, 0
      if probe.in_ram(pp) then
        px = s16(probe.read_u16(pp + 0x14))
        pz = s16(probe.read_u16(pp + 0x18))
      end
      local key = string.format("%04X->%04X", held, resolved)
      pairs_seen[key] = (pairs_seen[key] or 0) + 1
      rows = rows + 1
      if rows <= 20000 then
        csv:row("%d,%d,%s,0x%04X,0x%04X,%d,%d,%d,%d,%s", rows, g_elapsed,
          tostring(held_btn), held, resolved, skid, px, pz,
          probe.read_u8(GAME_MODE) or 0, scene_name())
      end
    end)
    return { d }
  end,

  on_capture = function(ctx, el)
    g_elapsed = el
    if el == 2 then
      local bad = 0
      for _, t in ipairs({ { CALL_SITE, CALL_WORD }, { RET_SITE, RET_WORD } }) do
        local got = n32(probe.read_u32(t[1]) or 0)
        if got ~= n32(t[2]) then
          bad = bad + 1
          logf("TAP MISMATCH [0x%08X] = 0x%08X want 0x%08X", t[1], got, t[2])
        end
      end
      logf("armed, %d mismatched; mode=%d scene=%s", bad,
        probe.read_u8(GAME_MODE) or 0, scene_name())
    end
    -- Hold one cardinal per HOLD-vsync block, cycling.
    if el >= 30 then
      local want = math.floor((el - 30) / HOLD) % #HOLDS + 1
      if want ~= hold_i then
        if held_btn then probe.pad_release(held_btn) end
        hold_i = want
        held_btn = HOLDS[hold_i]
        probe.pad_force(held_btn)
        logf("f=%d hold btn %d (block %d)", el, held_btn, hold_i)
      end
    end
    if (el % 200) == 0 then
      logf("vsync %d calls=%d skids=%d mode=%d scene=%s", el, calls, skids,
        probe.read_u8(GAME_MODE) or 0, scene_name())
    end
  end,

  on_summary = function()
    logf("--- FUN_80046494 census over %d vsyncs ---", g_elapsed)
    logf("calls=%d skids=%d (%.2f%%)", calls, skids,
      calls > 0 and (100.0 * skids / calls) or 0.0)
    local ks = {}
    for k, n in pairs(pairs_seen) do ks[#ks+1] = { k, n } end
    table.sort(ks, function(a, b) return a[2] > b[2] end)
    for i = 1, math.min(#ks, 24) do
      logf("  held->resolved %s : %d", ks[i][1], ks[i][2])
    end
    local fh = io.open(OUT_LOG, "w")
    if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    if csv then csv:close() end
  end,
})
