-- autorun_w3a_field_walk.lua
--
-- One field probe with a scripted pad ladder, three optional breakpoint
-- banks and a per-vsync poll. It exists because the three questions it
-- serves all want the same session shape - load a field anchor, walk, and
-- watch something fire - and they were otherwise three probes:
--
--   * story-flag provenance for a region nobody has walked
--     (`autorun_flag_firehose.lua` is the whole-playthrough instrument and
--     is deliberately HUMAN-navigated with no self-quit; this one drives
--     its own ladder and stops);
--   * the field-VM actor clone (`4C 14`), whose helper `FUN_801D835C` and
--     dispatcher arm `0x801E0E80` carry the operands a capture has to
--     confirm against the disc's own bytes;
--   * a door crossing, where the deliverable is the scene word changing
--     and the frames either side of it.
--
-- Pad ladder: LEGAIA_PAD is a comma-separated list of `from:BTN:for`
-- steps in vsyncs, e.g. `30:down:120,200:cross:8`. Buttons are the
-- `probe.BTN` names, case-insensitive. Steps may overlap; each one
-- forces its button at `from` and releases it at `from + for`.
--
-- Breakpoint banks (each off unless asked for, because an exec bp costs
-- emulation speed and an unused one only adds noise):
--   LEGAIA_FLAGS=1  story-flag SET `FUN_8003CE08` / CLEAR `FUN_8003CE34`,
--                   logging `a0` (the flag index) plus the writer's `ra`.
--   LEGAIA_CLONE=1  the clone helper `FUN_801D835C` (`a0` = source actor,
--                   `a1` = modulation word, `a2` = rate) and the field-VM
--                   dispatcher arm `0x801E0E80` that calls it.
--   LEGAIA_TINT=1   the screen-effect push `FUN_80024EE4(kind, blend, rgb)`
--                   and the tween spawner `FUN_801DE2B0`.
--
-- Outputs (under the run dir):
--   w3a_walk.csv    vsync,scene,mode,px,pz,tint_r,tint_g,tint_b,held
--   w3a_hits.csv    vsync,site,a0,a1,a2,ra,scene,mode
--   w3a_walk.log    the ladder, the per-site hit totals and a scene timeline
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario chitei2_field_card_boot \
--       --lua scripts/pcsx-redux/autorun_w3a_field_walk.lua --frames 900

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local PAD_S  = probe.getenv("LEGAIA_PAD", "")
local WANT_FLAGS = probe.getenv("LEGAIA_FLAGS", "") == "1"
local WANT_CLONE = probe.getenv("LEGAIA_CLONE", "") == "1"
local WANT_TINT  = probe.getenv("LEGAIA_TINT", "") == "1"

local OUT_CSV  = probe.out_path("w3a_walk.csv")
local OUT_HITS = probe.out_path("w3a_hits.csv")
local OUT_LOG  = probe.out_path("w3a_walk.log")

local SCENE      = 0x8007050C
local MODE       = 0x8007B83C
local PLAYER_PTR = 0x8007C364
local TINT_R     = 0x8007BCB8

local FLAG_SET   = 0x8003CE08
local FLAG_CLEAR = 0x8003CE34
local CLONE_HELP = 0x801D835C
local CLONE_ARM  = 0x801E0E80
local PUSH_QUAD  = 0x80024EE4
local SPAWN_TWEEN = 0x801DE2B0

local BTN = {
  up = probe.BTN.UP, down = probe.BTN.DOWN,
  left = probe.BTN.LEFT, right = probe.BTN.RIGHT,
  cross = probe.BTN.CROSS, circle = probe.BTN.CIRCLE,
  triangle = probe.BTN.TRIANGLE, square = probe.BTN.SQUARE,
  start = probe.BTN.START, select = probe.BTN.SELECT,
}

-- Parse the ladder up front so a typo is a startup error and not a silent
-- no-input run that looks like "the beat never fired".
local steps = {}
for tok in string.gmatch(PAD_S, "[^,%s]+") do
  local from, name, dur = string.match(tok, "^(%d+):(%a+):(%d+)$")
  local btn = name and BTN[string.lower(name)]
  if btn == nil then
    error("LEGAIA_PAD step '" .. tok .. "' is not <from>:<button>:<for>")
  end
  steps[#steps + 1] = { from = tonumber(from), dur = tonumber(dur), btn = btn, name = name }
end

local lines, csv, hits_csv = {}, nil, nil
local vsync, hits, held = 0, {}, {}
local last_key = nil

local function logf(fmt, ...)
  local s = string.format(fmt, ...)
  lines[#lines + 1] = s
  PCSX.log("[w3a_walk] " .. s)
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

-- LuaJIT's bit ops return SIGNED 32-bit ints, so a raw `bit.band` result
-- never compares equal to the positive literal `0x80000000`; normalise
-- first or every pointer test fails and the column reads `-1` forever.
local function u32(v)
  v = bit.band(tonumber(v) or 0, 0xFFFFFFFF)
  if v < 0 then v = v + 4294967296 end
  return v
end

local function player_xz()
  local p = probe.read_u32(PLAYER_PTR) or 0
  if u32(bit.band(p, 0xFFE00000)) ~= 0x80000000 then return -1, -1 end
  local x = probe.read_u16(p + 0x14) or 0
  local z = probe.read_u16(p + 0x18) or 0
  return x, z
end

local function regs()
  local r = PCSX.getRegisters()
  local function g(n) return u32(r.GPR.n[n]) end
  return g("a0"), g("a1"), g("a2"), g("ra")
end

local function note(site)
  local a0, a1, a2, ra = regs()
  hits[site] = (hits[site] or 0) + 1
  if hits_csv then
    hits_csv:row("%d,%s,%d,0x%08X,0x%08X,0x%08X,%s,%d",
      vsync, site, a0, a1, a2, ra, scene_name(), u8(MODE))
  end
  -- The first few of each site go to the log too, so a run can be read
  -- without the CSV; the CSV keeps every one.
  if hits[site] <= 12 then
    logf("f=%d %s a0=0x%08X a1=0x%08X a2=0x%08X ra=0x%08X scene=%s",
      vsync, site, a0, a1, a2, ra, scene_name())
  end
end

probe.run({
  sstate = SSTATE,
  capture_frames = FRAMES,
  on_arm = function()
    csv = probe.csv_open(OUT_CSV,
      "vsync,scene,mode,px,pz,tint_r,tint_g,tint_b,held")
    hits_csv = probe.csv_open(OUT_HITS, "vsync,site,a0,a1,a2,ra,scene,mode")
    probe.env.write_manifest("autorun_w3a_field_walk.lua", {
      sstate = SSTATE, frames = FRAMES, pad = PAD_S,
      flags = WANT_FLAGS, clone = WANT_CLONE, tint = WANT_TINT,
    })
    if WANT_FLAGS then
      probe.arm_breakpoint(FLAG_SET, "Exec", 4, "flag_set",
        function() note("flag_set_8003CE08") end)
      probe.arm_breakpoint(FLAG_CLEAR, "Exec", 4, "flag_clear",
        function() note("flag_clear_8003CE34") end)
    end
    if WANT_CLONE then
      probe.arm_breakpoint(CLONE_HELP, "Exec", 4, "clone_helper",
        function() note("clone_helper_801D835C") end)
      probe.arm_breakpoint(CLONE_ARM, "Exec", 4, "clone_arm",
        function() note("clone_arm_801E0E80") end)
    end
    if WANT_TINT then
      probe.arm_breakpoint(PUSH_QUAD, "Exec", 4, "push_quad",
        function() note("push_80024EE4") end)
      probe.arm_breakpoint(SPAWN_TWEEN, "Exec", 4, "spawn_tween",
        function() note("spawn_tween_801DE2B0") end)
    end
    logf("armed flags=%s clone=%s tint=%s; %d pad step(s) over %d frames",
      tostring(WANT_FLAGS), tostring(WANT_CLONE), tostring(WANT_TINT),
      #steps, FRAMES)
    for _, s in ipairs(steps) do
      logf("  pad %s from %d for %d", s.name, s.from, s.dur)
    end
    return {}
  end,
  on_capture = function(_, el)
    vsync = el
    for i, s in ipairs(steps) do
      if el == s.from then
        probe.pad_force(s.btn); held[i] = true
      elseif el == s.from + s.dur then
        probe.pad_release(s.btn); held[i] = nil
      end
    end
    local names = {}
    for i, s in ipairs(steps) do if held[i] then names[#names + 1] = s.name end end
    local sc, md = scene_name(), u8(MODE)
    local px, pz = player_xz()
    csv:row("%d,%s,%d,%d,%d,%d,%d,%d,%s", el, sc, md, px, pz,
      u8(TINT_R), u8(TINT_R + 1), u8(TINT_R + 2),
      (#names > 0) and table.concat(names, "+") or "-")
    -- One log line per (scene, mode) transition keeps the timeline
    -- readable without re-reading the CSV.
    local key = sc .. "|" .. md
    if key ~= last_key then
      last_key = key
      logf("f=%d scene=%s mode=0x%02X player=(%d,%d)", el, sc, md, px, pz)
    end
  end,
  on_summary = function()
    logf("--- breakpoint hits ---")
    local any = false
    for k, n in pairs(hits) do logf("  %s : %d", k, n); any = true end
    if not any then logf("  (none)") end
    local fh = io.open(OUT_LOG, "w")
    if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    if csv then csv:close() end
    if hits_csv then hits_csv:close() end
  end,
})
