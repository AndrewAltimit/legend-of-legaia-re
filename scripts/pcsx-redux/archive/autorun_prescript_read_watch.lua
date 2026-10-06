-- autorun_prescript_read_watch.lua
--
-- D-SES decider: is the scene event-scripts PRESCRIPT ever READ at runtime?
--
-- The prescript ([u16 count][u16 offsets[count]][records...], records open with
-- the 0xFFFF 0x0000 header sentinel, terminate with 0x0008) rides into RAM inside
-- the scripted/kingdom asset bundles, but the static sweep found NO walker of its
-- shape anywhere in the dumped corpus, and the sole asset-table walker
-- FUN_80020224 reads `count = *base` at the post-prescript +0x800 table, skipping
-- the prescript. This probe settles whether anything reads the prescript bytes.
--
-- The save-resume on this PCSX-Redux build survives only a few vsyncs (documented
-- instability), and the kingdom (map01) scene load runs IMMEDIATELY on resume, so
-- detection + arming must happen in the first vsyncs. Two complementary hooks:
--
--   PRIMARY: an Exec breakpoint on the asset-table walker FUN_80020224. It fires
--     DURING the load with _DAT_8007b85c (the walker base) already set; for a
--     scripted/kingdom bundle that base is `bundle + 0x800` (the count-7 table the
--     walker reads, logged as "rc 7"), so the prescript sits at base - 0x800. We
--     confirm the signature there and arm the Read watchpoint at the exact moment
--     the bundle lands, before any consumer runs.
--   BACKSTOP: a per-vsync STRUCTURAL heap scan (content-free; no Sony bytes) for
--     the prescript signature -- c = u16[A] in 2..4096, u16[A+2] == 2 + c*2, and
--     the record at A+offsets[0] opens 0xFFFF 0x0000 -- in case the base-0x800
--     prediction misses (e.g. a v12 +0x800 prescript resident elsewhere).
--
-- On detection, arm a Read watchpoint over the prescript header + offset-table
-- span (the bytes ANY walker must traverse) and log every reader PC. Results are
-- written incrementally (per-line flush) so a crash mid-capture still leaves the
-- residency + read trail. No pad warp: the load happens on resume.
--
-- Env knobs (all optional):
--   LEGAIA_SSTATE        save state (default the drake_castle_to_worldmap slot)
--   LEGAIA_FRAMES        capture vsyncs (default 90; the save dies fast)
--   LEGAIA_SCAN_BASE     heap scan window base (default 0x80100000)
--   LEGAIA_SCAN_LEN      heap scan window length (default 0x80000)
--   LEGAIA_SCAN_EVERY    backstop scan cadence in frames (default 1)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local f = assert(io.open(probe.out_path("prescript_read_watch.txt"), "w"))
local function w(s) f:write(s .. "\n"); f:flush() end

local SCAN_BASE  = probe.getenv_num("LEGAIA_SCAN_BASE", 0x80010000)
local SCAN_LEN   = probe.getenv_num("LEGAIA_SCAN_LEN", 0x1D0000)  -- ~1.9 MiB: 0x80010000..0x801E0000
local SCAN_AT    = probe.getenv_num("LEGAIA_SCAN_AT", 26)         -- one-shot full-RAM scan frame (post kingdom load)
local CONT_BASE  = probe.getenv_num("LEGAIA_CONT_BASE", 0x80100000) -- per-frame focused-window scan: the field-asset heap
local CONT_LEN   = probe.getenv_num("LEGAIA_CONT_LEN", 0x80000)     -- where scene buffers land (asset table seen at 0x8015CBD0)
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 45)
local scanned_once = false

-- This PCSX build fires only ONE GPU::Vsync listener, so we DON'T use probe.run
-- (its listener would shadow a separate boot-skip masher). One self-contained
-- listener below does: boot-mash START to skip the cold-boot intro/FMV (which
-- otherwise stalls vsync progression headless) -> load the save once the intro
-- has settled -> drive the warp + detection + read-watch.
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 180)
local SSTATE = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FB_PTR     = 0x8007b85c  -- _DAT_8007b85c: loaded field-buffer base (= bundle+0x800 for scripted)
local WALKER     = 0x80020224  -- FUN_80020224: the asset-table walker (count = *_DAT_8007b85c)
local LOADER     = 0x8003E8A8  -- FUN_8003E8A8: PROT-index resolver (every load; ext index = a0 - 2)
local WARP_BTN   = probe.BTN[probe.getenv("LEGAIA_WARP_BTN", "UP")] or probe.BTN.UP
local WARP_ON    = probe.getenv_num("LEGAIA_WARP_ON", 6)    -- press warp button at this frame
local WARP_OFF   = probe.getenv_num("LEGAIA_WARP_OFF", 70)  -- release it here (long hold = sure transition)

local MAX_INSTANCES = 12
local MAX_READ_LOG  = 60
local WATCH_SPAN_CAP = 0x200  -- cap the watched header+offset-table span per instance

local armed = false
local read_hits = {}   -- "pc->addr" -> count
local nread = 0
local found = {}        -- list of {addr=, count=, off0=, near_fb=bool}
local watched = {}      -- addr -> true (already has a read-watch)
local walker_fired = false
local loader_seen = {}   -- raw prot index -> true (dedup the load log)

-- Validate a prescript header at addr (content-free). Returns count,off0 or nil.
local function check_prescript(addr)
  local count = probe.read_u16(addr)
  if count == nil or count < 2 or count > 4096 then return nil end
  local off0 = probe.read_u16(addr + 2)
  if off0 ~= 2 + count * 2 then return nil end
  local h0 = probe.read_u16(addr + off0)
  local h1 = probe.read_u16(addr + off0 + 2)
  if h0 == 0xFFFF and h1 == 0x0000 then return count, off0 end
  return nil
end

-- Structural prescript scan over [SCAN_BASE, SCAN_BASE+SCAN_LEN). Returns a list
-- of {addr, count, off0}. Content-free: matches the [count][offsets]+FFFF0000
-- shape, embeds no game bytes.
local function scan_prescripts(base, len)
  base = base or SCAN_BASE
  len = len or SCAN_LEN
  local out = {}
  local buf = probe.read_bytes(base, len)
  if buf == nil then return out end
  local s = tostring(buf)
  local n = #s
  local i = 1                              -- 1-based index into s; addr = base + (i-1)
  while i + 7 <= n do
    local count = s:byte(i) + s:byte(i + 1) * 256
    if count >= 2 and count <= 4096 then
      local off0 = s:byte(i + 2) + s:byte(i + 3) * 256
      if off0 == 2 + count * 2 then
        local rp = i + off0               -- record[0] position (1-based)
        if rp + 3 <= n then
          local h0 = s:byte(rp) + s:byte(rp + 1) * 256
          local h1 = s:byte(rp + 2) + s:byte(rp + 3) * 256
          if h0 == 0xFFFF and h1 == 0x0000 then
            out[#out + 1] = { addr = base + (i - 1), count = count, off0 = off0 }
            if #out >= MAX_INSTANCES then break end
          end
        end
      end
    end
    i = i + 4                             -- prescript buffers are word-aligned
  end
  return out
end

-- Arm a Read watchpoint on a confirmed prescript at addr (idempotent).
local function arm_read(addr, count, off0, origin)
  if watched[addr] then return end
  watched[addr] = true
  found[#found + 1] = { addr = addr, count = count, off0 = off0 }
  local span = math.min(2 + count * 2, WATCH_SPAN_CAP)
  local fb = probe.read_u32(FB_PTR) or 0
  local near = (fb ~= 0) and (addr == fb - 0x800)
  w(string.format("  resident prescript @ %08X count=%d off0=%04X span=%X via %s%s",
    addr, count, off0, span, origin, near and "  (== _DAT_8007b85c - 0x800)" or ""))
  local tag = string.format("%08X", addr)
  probe.arm_breakpoint(addr, "Read", span, "rd_pre_" .. tag, function()
    local r = PCSX.getRegisters()
    local pc = (tonumber(r.pc) or 0) % 0x100000000
    local key = string.format("%08X->%08X", pc, addr)
    read_hits[key] = (read_hits[key] or 0) + 1
    if nread < MAX_READ_LOG then
      w(string.format("  READ prescript @%08X by pc=%08X", addr, pc))
      nread = nread + 1
    end
  end)
  armed = true
end

-- Per-vsync capture body (el = vsyncs since the save loaded).
local function on_capture(el)
  -- Save resumes in Drake Castle (field); a held UP warps to the Drake KINGDOM
  -- world map, which loads the scripted bundle carrying the prescript.
  if el == WARP_ON then probe.pad_force(WARP_BTN) end
  if el == WARP_OFF then probe.pad_release(WARP_BTN) end

  -- Arm the walker exec-bp post-load (el==2). On fire, _DAT_8007b85c is the
  -- table base the walker reads; for a scripted bundle the prescript is base-0x800.
  if el == 2 then
    probe.arm_breakpoint(WALKER, "Exec", 4, "walker", function()
      if walker_fired then return end  -- one-shot: avoid per-call interpreter slowdown
      walker_fired = true
      local base = probe.read_u32(FB_PTR)
      if base == nil then return end
      local cand = (base - 0x800) % 0x100000000
      local c, o = check_prescript(cand)
      w(string.format("  [walker] base=%08X *base=%s -> base-0x800=%08X %s",
        base, tostring(probe.read_u16(base)), cand, c and "PRESCRIPT" or "(no prescript)"))
      if c then arm_read(cand, c, o, "walker base-0x800") end
    end)
    w("  [walker exec-bp armed]")
  end

  if el % 10 == 0 then
    local base = probe.read_u32(FB_PTR)
    w(string.format("  [hb] el=%d base=%s *base=%s", el,
      base and string.format("%08X", base) or "nil",
      base and tostring(probe.read_u16(base)) or "nil"))
  end

  -- CONTINUOUS: scan the focused field-asset heap EVERY frame, so a prescript that
  -- is staged only TRANSIENTLY during scene-init (loaded -> consumed -> freed
  -- before the one-shot scan) is still caught and immediately read-watched. This
  -- closes the "consumed during load, gone by el=26" gap the navmesh writeup warns
  -- about (a consumer reading via a hardcoded/transient buffer the post-load scan
  -- would miss).
  for _, p in ipairs(scan_prescripts(CONT_BASE, CONT_LEN)) do
    arm_read(p.addr, p.count, p.off0, string.format("cont-scan@el=%d", el))
  end

  -- DEFINITIVE: one full-RAM structural scan after the kingdom load settles.
  if (not scanned_once) and el >= SCAN_AT then
    scanned_once = true
    local hits = scan_prescripts()
    w(string.format("  [full-scan @el=%d] %d prescript-shaped region(s) in [%08X,%08X)",
      el, #hits, SCAN_BASE, SCAN_BASE + SCAN_LEN))
    for _, p in ipairs(hits) do arm_read(p.addr, p.count, p.off0, "scan") end
    if #hits == 0 then
      w("  -> NO prescript resident anywhere in the scanned RAM after the kingdom load.")
    end
  end
end

local function write_summary()
  w("-- summary --")
  if not armed then
    w("  PRESCRIPT NEVER BECAME RESIDENT (walker base-0x800 + heap scan both empty).")
    w("  -> not loaded into this region/scene; widen LEGAIA_SCAN_BASE/LEN or try another kingdom.")
  else
    w(string.format("  resident prescript instances: %d", #found))
    if nread == 0 then
      w("  ZERO reads of any resident prescript across the capture.")
      w("  -> LOADED-BUT-UNCONSUMED confirmed (no runtime consumer; D-SES closes as vestigial).")
    else
      w("  -- unique reader pc -> prescript addr (count) --")
      for k, v in pairs(read_hits) do w(string.format("  %s : %d", k, v)) end
      w("  -> CONSUMER FOUND: dump the reader PC's function and decode the opcode VM.")
    end
  end
  w("done")
end

-- Single self-contained vsync driver (one listener for this build).
w(string.format("prescript read-watch: boot_delay=%d scan [%08X,%08X) %d capture frames",
  BOOT_DELAY, SCAN_BASE, SCAN_BASE + SCAN_LEN, FRAMES))
local vsync = 0
local loaded = false
local capture_start = 0
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
  vsync = vsync + 1
  if not loaded then
    if vsync < BOOT_DELAY - 2 then
      -- Mash START to skip the cold-boot intro/FMV so vsyncs keep progressing.
      if vsync % 4 < 2 then probe.pad_force(probe.BTN.START) else probe.pad_release(probe.BTN.START) end
      return
    end
    if vsync < BOOT_DELAY then probe.pad_release(probe.BTN.START); return end
    -- vsync == BOOT_DELAY: the intro has settled; load the resume save.
    probe.pad_release(probe.BTN.START)
    if not probe.load_save_state(SSTATE) then
      w("  ERROR: save-state load failed"); f:close(); PCSX.quit(2); return
    end
    loaded = true
    capture_start = vsync
    w("  [save loaded; capture started]")
    return
  end
  local el = vsync - capture_start
  local ok, err = pcall(on_capture, el)
  if not ok then w("  on_capture error: " .. tostring(err)) end
  -- Quit at the full frame budget, OR shortly after the definitive scan if no
  -- prescript is resident (nothing to watch -> don't idle through the slow tail).
  local done = (el >= FRAMES) or (scanned_once and not armed and el >= SCAN_AT + 2)
  if done then
    probe.disarm_all()
    write_summary()
    f:close()
    PCSX.quit(0)
  end
end)
