-- autorun_w3b_dome_still.lua
--
-- Live verification of the Muscle Dome ringside-still emitter FUN_801D00F8
-- (PROT 0977, base 0x801CE818, file +0x18E0): does the two-POLY_FT4 arm run,
-- what is on the fork global _DAT_801D1AE0, and do the emitted packets carry
-- tpage 0x106 / 0x109?
--
-- Taps (word-asserted so a paged-out / mis-based tap reports MISMATCH):
--   0x801D00F8 entry                = 0x27BDFFC8  addiu sp,sp,-0x38
--   0x801D0148 zero-fork arm        = 0x0000B021  move s6,zero  (6x FUN_801D08EC tiles)
--   0x801D01BC non-zero arm         = 0x36940314  ori s4,s4,0x314 (scratchpad base)
--   0x801D0248 quad-1 filled        = 0x8E84... lw a0,0xe0(s4)
--   0x801D02BC quad-2 filled        = lw a0,0xe0(s4)
-- At the quad taps a1 still holds that packet's base, so the 0x28-byte
-- POLY_FT4 is read straight out of the pool.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local NOPAD  = probe.getenv_num("LEGAIA_NOPAD", 0)
local POKE   = probe.getenv_num("LEGAIA_POKE_FORK", 0)

local OUT_CSV = probe.out_path("w3b_dome_still.csv")
local OUT_LOG = probe.out_path("w3b_dome_still.log")

local GAME_MODE = 0x8007B83C
local SCENE     = 0x8007050C
local FORK      = 0x801D1AE0
local FADE      = 0x801D1A7C

local TAPS = {
  { addr = 0x801D00F8, want = 0x27BDFFC8, kind = "entry" },
  { addr = 0x801D0148, want = 0x0000B021, kind = "arm_zero" },
  { addr = 0x801D01BC, want = 0x36940314, kind = "arm_quads" },
  { addr = 0x801D0248, want = 0x8E840E0 , kind = "quad1" },
  { addr = 0x801D02BC, want = 0x8E840E0 , kind = "quad2" },
}

local lines = {}
local function logf(fmt, ...)
  local s = string.format(fmt, ...)
  lines[#lines+1] = s
  PCSX.log("[domestill] " .. s)
end
local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end

local function scene_name()
  local out = {}
  for i = 0, 7 do
    local b = probe.read_u8(SCENE + i)
    if b == nil or b < 0x20 or b >= 0x7F then break end
    out[#out+1] = string.char(b)
  end
  return table.concat(out)
end

local csv, g_elapsed, rows = nil, 0, 0
local counts, tpages, fades, forks = {}, {}, {}, {}

probe.run({
  sstate = SSTATE,
  capture_frames = FRAMES,
  on_arm = function()
    csv = probe.csv_open(OUT_CSV,
      "seq,vsync,tap,a0,fork,fade,tpage,x0,y0,x3,y3,cmd,ra,mode,scene")
    probe.env.write_manifest("autorun_w3b_dome_still.lua",
      { sstate = SSTATE, frames = FRAMES, nopad = NOPAD })
    local descs = {}
    for _, t in ipairs(TAPS) do
      local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.kind }
      probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
        -- residency gate: only count a hit when 0977's own bytes are there
        if n32(probe.read_u32(0x801D00F8) or 0) ~= 0x27BDFFC8 then return end
        d.hits_ref.n = d.hits_ref.n + 1
        counts[t.kind] = (counts[t.kind] or 0) + 1
        local r = PCSX.getRegisters()
        local a0 = n32(r.GPR.n.a0)
        local a1 = n32(r.GPR.n.a1)
        local fork = probe.read_u32(FORK) or 0
        local fade = probe.read_u32(FADE) or 0
        forks[fork] = (forks[fork] or 0) + 1
        local tp, x0, y0, x3, y3, cmd = -1, 0, 0, 0, 0, 0
        if (t.kind == "quad1" or t.kind == "quad2") and probe.in_ram(a1) then
          tp  = probe.read_u16(a1 + 0x16) or -1
          x0  = probe.read_u16(a1 + 0x08) or 0
          y0  = probe.read_u16(a1 + 0x0A) or 0
          x3  = probe.read_u16(a1 + 0x20) or 0
          y3  = probe.read_u16(a1 + 0x22) or 0
          cmd = probe.read_u32(a1 + 0x04) or 0
          tpages[string.format("%s:0x%X", t.kind, tp)] =
            (tpages[string.format("%s:0x%X", t.kind, tp)] or 0) + 1
        end
        if t.kind == "entry" then
          fades[a0] = (fades[a0] or 0) + 1
          if POKE == 1 and fork == 0 then
            probe.write_u32(FORK, 1)
            logf("f=%d FORCED _DAT_801D1AE0 = 1 at entry (was 0)", g_elapsed)
          end
        end
        rows = rows + 1
        if rows <= 4000 then
          csv:row("%d,%d,%s,%d,%d,%d,0x%X,%d,%d,%d,%d,0x%s,0x%s,%d,%s",
            rows, g_elapsed, t.kind, a0, fork, fade, tp, x0, y0, x3, y3,
            hex8(cmd), hex8(r.GPR.n.ra), probe.read_u8(GAME_MODE) or 0,
            scene_name())
        end
      end)
      descs[#descs+1] = d
    end
    return descs
  end,

  on_capture = function(ctx, el)
    g_elapsed = el
    if el == 2 then
      local bad = 0
      for _, t in ipairs(TAPS) do
        local got = n32(probe.read_u32(t.addr) or 0)
        -- quad taps are `lw a0,0xe0(s4)`; compare only the opcode+imm halves
        local ok
        if t.kind == "quad1" or t.kind == "quad2" then
          ok = (bit.band(got, 0xFC00FFFF) == 0x8C0000E0)
        else
          ok = (got == n32(t.want))
        end
        if not ok then
          bad = bad + 1
          logf("TAP MISMATCH [0x%08X] = 0x%s (%s)", t.addr, hex8(got), t.kind)
        end
      end
      logf("armed %d taps, %d mismatched; mode=%d scene=%s fork=%s fade=%s",
        #TAPS, bad, probe.read_u8(GAME_MODE) or 0, scene_name(),
        tostring(probe.read_u32(FORK)), tostring(probe.read_u32(FADE)))
    end
    if NOPAD == 0 and (el % 70) == 0 and el >= 40 then
      probe.pad_force(probe.BTN.CROSS)
    elseif NOPAD == 0 and (el % 70) == 6 then
      probe.pad_release(probe.BTN.CROSS)
    end
    if (el % 100) == 0 then
      local w = n32(probe.read_u32(0x801D00F8) or 0)
      if w == 0x27BDFFC8 then logf("f=%d 0977 RESIDENT fork=%s fade=%s", el, tostring(probe.read_u32(FORK)), tostring(probe.read_u32(FADE))) end
    end
    if (el % 200) == 0 then
      logf("vsync %d entry=%d zero=%d quads=%d q1=%d q2=%d fork=%s mode=%d scene=%s",
        el, counts.entry or 0, counts.arm_zero or 0, counts.arm_quads or 0,
        counts.quad1 or 0, counts.quad2 or 0,
        tostring(probe.read_u32(FORK)), probe.read_u8(GAME_MODE) or 0,
        scene_name())
    end
  end,

  on_summary = function()
    logf("--- dome still census over %d vsyncs ---", g_elapsed)
    for _, t in ipairs(TAPS) do
      logf("%-10s 0x%08X hits: %d", t.kind, t.addr, counts[t.kind] or 0)
    end
    for k, n in pairs(tpages) do logf("  tpage %s : %d", k, n) end
    local fk = {}
    for v, n in pairs(forks) do fk[#fk+1] = { v, n } end
    table.sort(fk, function(a,b) return a[2] > b[2] end)
    for i = 1, math.min(#fk, 8) do
      logf("  fork _DAT_801D1AE0 = %d : %d hits", fk[i][1], fk[i][2])
    end
    local fd = {}
    for v, n in pairs(fades) do fd[#fd+1] = { v, n } end
    table.sort(fd, function(a,b) return a[1] < b[1] end)
    for i = 1, math.min(#fd, 16) do
      logf("  entry a0 (fade level) = %d : %d", fd[i][1], fd[i][2])
    end
    local fh = io.open(OUT_LOG, "w")
    if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    if csv then csv:close() end
  end,
})
