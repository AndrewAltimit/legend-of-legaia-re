-- autorun_scene_load_trace.lua
--
-- D-SES support probe: trace EVERY file/entry a scene transition loads, to answer
-- decisively whether the scene event-scripts prescript entry (v12 / scene_event_
-- scripts) is ever loaded at all on this build's actual loader path.
--
-- This build takes the retail path branch in the field loader FUN_8001F7C0
-- (`_DAT_8007b8c2 == 0`): it loads via `path_opener` = FUN_8003E6BC (resolves a
-- dev path string through the CDNAME name map to a PROT index, then the LBA
-- resolver FUN_8003E8A8). So FUN_8003E6BC's path STRING names each file loaded.
--
-- Hooks (all low-frequency = per-file, no interpreter crawl), armed POST-load:
--   FUN_8003E6BC(path_ptr, dest)   -> log the path string (the file name)
--   FUN_8003E8A8(prot_index, flag) -> log the raw PROT index (extraction = a0-2)
--   FUN_8001F7C0(dest, name, rec)  -> log the scene name + field record
-- Plus the load-path flags `_DAT_8007b8c2` / `_DAT_8007b868`.
--
-- Drive: boot-mash START to skip the cold-boot intro (single-listener pattern, as
-- this PCSX build fires only one GPU::Vsync listener), load the drake_castle save
-- at vsync ~180, hold UP to warp into the Drake kingdom world map, log the loads.
-- Cross-reference the logged PROT indices / path names vs categorize.json: if no
-- v12 / scene_event_scripts entry is ever requested, the prescript is never loaded.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local f = assert(io.open(probe.out_path("scene_load_trace.txt"), "w"))
local function w(s) f:write(s .. "\n"); f:flush() end

local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 180)
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 60)
local SSTATE = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local WARP_BTN = probe.BTN[probe.getenv("LEGAIA_WARP_BTN", "UP")] or probe.BTN.UP
local WARP_ON, WARP_OFF = 6, 70

local PATH_OPENER = 0x8003E6BC
local IDX_RESOLVER = 0x8003E8A8
local FIELD_LOADER = 0x8001F7C0

local seen = {}
local n_path, n_idx, n_field = 0, 0, 0
local MAXLOG = 60

-- Read a NUL-terminated ASCII string from emulated RAM (cap 64 bytes).
local function read_cstr(addr)
  if addr == nil or addr == 0 then return "<null>" end
  local out = {}
  for i = 0, 63 do
    local b = probe.read_u8(addr + i)
    if b == nil or b == 0 then break end
    out[#out + 1] = (b >= 0x20 and b < 0x7f) and string.char(b) or string.format("\\x%02X", b)
  end
  return table.concat(out)
end

local function reg(name) local r = PCSX.getRegisters(); return (tonumber(r[name]) or 0) % 0x100000000 end

local function arm()
  probe.arm_breakpoint(PATH_OPENER, "Exec", 4, "path_opener", function()
    if n_path >= MAXLOG then return end
    local a0 = reg("a0")
    w(string.format("  [path] FUN_8003E6BC a0=%08X path=\"%s\"", a0, read_cstr(a0)))
    n_path = n_path + 1
  end)
  probe.arm_breakpoint(IDX_RESOLVER, "Exec", 4, "idx_resolver", function()
    if n_idx >= MAXLOG then return end
    local a0 = reg("a0")
    local k = "idx" .. a0
    if seen[k] then return end
    seen[k] = true
    w(string.format("  [idx]  FUN_8003E8A8 a0=raw %d (extraction %d)", a0, (a0 - 2) % 0x100000000))
    n_idx = n_idx + 1
  end)
  probe.arm_breakpoint(FIELD_LOADER, "Exec", 4, "field_loader", function()
    if n_field >= MAXLOG then return end
    local a1 = reg("a1")
    w(string.format("  [field] FUN_8001F7C0 name=\"%s\" rec=%d", read_cstr(a1), reg("a2")))
    n_field = n_field + 1
  end)
  w("  [hooks armed: path_opener / idx_resolver / field_loader]")
end

local vsync, loaded, capture_start, armed = 0, false, 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
  vsync = vsync + 1
  if not loaded then
    if vsync < BOOT_DELAY - 2 then
      if vsync % 4 < 2 then probe.pad_force(probe.BTN.START) else probe.pad_release(probe.BTN.START) end
      return
    end
    if vsync < BOOT_DELAY then probe.pad_release(probe.BTN.START); return end
    probe.pad_release(probe.BTN.START)
    if not probe.load_save_state(SSTATE) then w("  ERROR: load failed"); f:close(); PCSX.quit(2); return end
    loaded, capture_start = true, vsync
    w(string.format("scene-load trace: loaded; flags _DAT_8007b8c2=%s _DAT_8007b868=%s",
      tostring(probe.read_u8(0x8007b8c2)), tostring(probe.read_u32(0x8007b868))))
    return
  end
  local el = vsync - capture_start
  if el == WARP_ON then probe.pad_force(WARP_BTN) end
  if el == WARP_OFF then probe.pad_release(WARP_BTN) end
  if el == 2 and not armed then armed = true; arm() end
  if el % 10 == 0 then
    local base = probe.read_u32(0x8007b85c)
    w(string.format("  [hb] el=%d *0x8007b85c=%s *base=%s", el,
      base and string.format("%08X", base) or "nil",
      base and tostring(probe.read_u16(base)) or "nil"))
  end
  if el >= FRAMES then
    probe.disarm_all()
    w(string.format("-- done: %d path opens, %d index loads, %d field loads --", n_path, n_idx, n_field))
    f:close()
    PCSX.quit(0)
  end
end)
w(string.format("scene-load trace armed: boot_delay=%d, %d capture frames", BOOT_DELAY, FRAMES))
