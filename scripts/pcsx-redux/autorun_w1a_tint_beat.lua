-- autorun_w1a_tint_beat.lua
--
-- Log the retail field screen-effect tint across ONE beat, per frame.
--
-- What the beat is made of (`docs/subsystems/cutscene.md`, "Scripted screen
-- fade (op `0x4C 0x12`) + the effect colour (op `0x34` sub-0)"):
--
--   * op `0x4C 0x12` drives the global multiply tint `DAT_8007BCB8/B9/BA`
--     (neutral `0x80`), ramped by the slot-job spawner `FUN_8003C5F0`. This
--     probe polls those three bytes every vsync.
--   * op `0x34` sub-0 spawns a COLOUR TWEEN: the field-VM arm builds a
--     13-halfword fade template on its stack and calls `FUN_801DE2B0`
--     (default arm) or `FUN_80024E80` (when `_DAT_1F800394 & 0x800000`).
--     The tween's per-frame step `FUN_801DDC20` pushes one full-screen quad
--     through `FUN_80024EE4(kind, blend, packed_rgb)`. This probe breaks on
--     all four and records the arguments, so the beat's envelope - rise
--     frames, hold, fall, final value - comes out of the push stream rather
--     than out of a model.
--
-- The actor's own phase is its clock at `+0xC8` and its selectors at `+0xD2`
-- (blend) / `+0xD6` (kind); `FUN_801DDC20`'s `a0` is that actor, so the step
-- breakpoint carries them.
--
-- Env:
--   LEGAIA_HOLD / LEGAIA_HOLD_FROM / LEGAIA_HOLD_FOR   pad ladder, as
--       autorun_w1a_view_window_cross.lua (default: hold Down at vsync 30
--       for 90, which walks an overworld state into a town portal and takes
--       the entry fade).
--   LEGAIA_FRAMES    capture window (default 900)
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario overworld_into_town_man_load \
--       --lua scripts/pcsx-redux/autorun_w1a_tint_beat.lua --frames 900
--
-- Outputs:
--   w1a_tint_beat.csv    vsync,scene,mode,tint_r,tint_g,tint_b,kind_word
--   w1a_tint_push.csv    vsync,site,a0,a1,a2,r,g,b,ra,clock,blend,kind
--   w1a_tint_beat.log    summary

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE    = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES    = probe.getenv_num("LEGAIA_FRAMES", 900)
local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "down")
local HOLD_FROM = probe.getenv_num("LEGAIA_HOLD_FROM", 30)
local HOLD_FOR  = probe.getenv_num("LEGAIA_HOLD_FOR", 90)

local OUT_CSV  = probe.out_path("w1a_tint_beat.csv")
local OUT_PUSH = probe.out_path("w1a_tint_push.csv")
local OUT_LOG  = probe.out_path("w1a_tint_beat.log")

local TINT_R = 0x8007BCB8   -- global multiply tint, neutral 0x80
local TINT_G = 0x8007BCB9
local TINT_B = 0x8007BCBA
local KINDW  = 0x8007BCCC   -- screen-effect kind the spawners pass as a1
local SCENE  = 0x8007050C
local MODE   = 0x8007B83C

local SPAWN_TWEEN = 0x801DE2B0  -- op 0x34 sub-0 default arm
local SPAWN_FADE  = 0x80024E80  -- op 0x34 sub-0 forked arm
local STEP_TWEEN  = 0x801DDC20  -- per-frame tween step (a0 = the actor)
local PUSH_QUAD   = 0x80024EE4  -- FUN_80024EE4(kind, blend, packed_rgb)

local DIRS = {
  up = probe.BTN.UP, down = probe.BTN.DOWN,
  left = probe.BTN.LEFT, right = probe.BTN.RIGHT,
}
local HOLD = DIRS[HOLD_NAME] or probe.BTN.DOWN

local lines, csv, push_csv = {}, nil, nil
local vsync, hits = 0, {}
local function logf(fmt, ...)
  local s = string.format(fmt, ...)
  lines[#lines + 1] = s
  PCSX.log("[w1a_tint] " .. s)
end
local function u8(a) return probe.read_u8(a) or 0 end
local function scene_name()
  local out = {}
  for i = 0, 7 do
    local b = probe.read_u8(SCENE + i)
    if b == nil or b < 0x20 or b >= 0x7F then break end
    out[#out + 1] = string.char(b)
  end
  return table.concat(out)
end

-- `ra` is not decoration here: `FUN_801DE2B0` is reached from the field-VM
-- op `0x34` sub-0 arm and nowhere else, so the return address is what turns
-- "a colour tween was spawned" into "op 0x34 sub-0 spawned it".
local function note(site, a0, a1, a2, ra, clock, blend, kind)
  hits[site] = (hits[site] or 0) + 1
  if push_csv then
    push_csv:row("%d,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
      vsync, site, a0, a1, a2,
      bit.band(a2, 0xFF), bit.band(bit.rshift(a2, 8), 0xFF),
      bit.band(bit.rshift(a2, 16), 0xFF),
      ra or 0, clock or -1, blend or -1, kind or -1)
  end
  if hits[site] <= 6 then
    logf("f=%d %s a0=0x%08X a1=0x%08X a2=0x%08X ra=0x%08X clock=%d blend=%d kind=%d",
      vsync, site, a0, a1, a2, ra or 0, clock or -1, blend or -1, kind or -1)
  end
end

local function regs()
  local r = PCSX.getRegisters()
  local function g(n) return bit.band(tonumber(r.GPR.n[n]) or 0, 0xFFFFFFFF) end
  return g("a0"), g("a1"), g("a2"), g("ra")
end

local held, last_key = false, nil

probe.run({
  sstate = SSTATE,
  capture_frames = FRAMES,
  on_arm = function()
    csv = probe.csv_open(OUT_CSV, "vsync,scene,mode,tint_r,tint_g,tint_b,kind_word,holding")
    push_csv = probe.csv_open(OUT_PUSH,
      "vsync,site,a0,a1,a2,r,g,b,ra,clock,blend,kind")
    probe.env.write_manifest("autorun_w1a_tint_beat.lua",
      { sstate = SSTATE, frames = FRAMES, hold = HOLD_NAME })

    probe.arm_breakpoint(SPAWN_TWEEN, "Exec", 4, "spawn_tween", function()
      local a0, a1, a2, ra = regs()
      note("spawn_tween_801DE2B0", a0, a1, a2, ra, nil, nil, nil)
      -- The template is 13 halfwords at a0: kind[0], duration[1],
      -- start RGB[3..5], end RGB[7..9], delay[10], hold[11].
      local t = {}
      for i = 0, 12 do t[#t + 1] = tostring(probe.read_u16(a0 + i * 2) or 0) end
      logf("  template = %s", table.concat(t, " "))
    end)
    probe.arm_breakpoint(SPAWN_FADE, "Exec", 4, "spawn_fade", function()
      local a0, a1, a2, ra = regs()
      note("spawn_fade_80024E80", a0, a1, a2, ra, nil, nil, nil)
    end)
    probe.arm_breakpoint(STEP_TWEEN, "Exec", 4, "step_tween", function()
      local a0, a1, a2, ra = regs()
      note("step_801DDC20", a0, a1, a2, ra,
        probe.read_u16(a0 + 0xC8), probe.read_u16(a0 + 0xD2),
        probe.read_u16(a0 + 0xD6))
    end)
    probe.arm_breakpoint(PUSH_QUAD, "Exec", 4, "push_quad", function()
      local a0, a1, a2, ra = regs()
      note("push_80024EE4", a0, a1, a2, ra, nil, nil, nil)
    end)
    logf("armed 4 breakpoints; holding %s from %d for %d", HOLD_NAME, HOLD_FROM, HOLD_FOR)
    return {}
  end,
  on_capture = function(_, el)
    vsync = el
    local r, g, b = u8(TINT_R), u8(TINT_G), u8(TINT_B)
    local kw = probe.read_u16(KINDW) or 0
    local sc, md = scene_name(), u8(MODE)
    csv:row("%d,%s,%d,%d,%d,%d,%d,%d", el, sc, md, r, g, b, kw, held and 1 or 0)
    local key = string.format("%s|%d|%d,%d,%d|%d", sc, md, r, g, b, kw)
    if key ~= last_key then
      last_key = key
      logf("f=%d scene=%s mode=%d tint=(%d,%d,%d) kind_word=%d", el, sc, md, r, g, b, kw)
    end
    if el == HOLD_FROM then probe.pad_force(HOLD); held = true
    elseif HOLD_FOR > 0 and el == HOLD_FROM + HOLD_FOR then
      probe.pad_release(HOLD); held = false
    end
  end,
  on_summary = function()
    logf("--- breakpoint hits ---")
    for k, n in pairs(hits) do logf("  %s : %d", k, n) end
    if next(hits) == nil then logf("  (none)") end
    local fh = io.open(OUT_LOG, "w")
    if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    if csv then csv:close() end
    if push_csv then push_csv:close() end
  end,
})
