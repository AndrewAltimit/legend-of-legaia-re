-- autorun_world_map_fog_probe.lua
--
-- Sister script to autorun_slot4_readers.lua. Probes the five
-- distance-cue fog parameters that the world-map overlay's per-prim
-- leaves consult on every vertex (see docs/subsystems/world-map.md and
-- crates/engine-vm/src/prim_dispatch.rs for the math sketch).
--
-- The overlay-resident leaves at 0x801F7644 .. 0x801F8690 each insert a
-- ~60-instruction fog block between the GTE projection and the OT
-- packet write. The block consults these five GP-relative fields on
-- every prim:
--
--   gp-0x2E0  u32   Far-plane reference Z (mixed into prim cmd word).
--   gp-0x2DC  u32   Fog color (loaded into GTE color register pre dpcs).
--   gp-0x2D1  u8    Fog-enable flags byte; bit 0x10 gates the whole path.
--   gp-0x2BC  u32   Pointer to per-Z fog-tint LUT (2-byte entries,
--                   indexed by Z >> 5).
--   gp+0x90   u8    Z shift exponent (Z_far = max(z1..) >> *(u8 *)).
--
-- This script reads the gp register after save-state load, computes the
-- absolute address of each field, snapshots the initial values, then
-- arms Read breakpoints so every per-vertex fog read is captured with
-- PC + ra. The top PCs surface which overlay leaves are firing on the
-- current frame; cross-referencing them against the leaf table in
-- docs/subsystems/world-map.md (#per-slot-delta-vs-scus-sibling) pins
-- which slot (12..19) is producing each prim.
--
-- The LUT at gp-0x2BC is dumped on every snapshot tick so the WebGL
-- port can bake an equivalent texture / uniform array. The script
-- captures 1 KiB starting at the LUT pointer (= 512 u16 entries =
-- Z >> 5 over the full 14-bit Z range PSX hardware uses).
--
-- Capture protocol:
--   1. Start PCSX-Redux with a save state that's in the world-map
--      top-view dev menu (DAT_801F2B94 != 0). The slot-1 fog parameters
--      are paged in by the world-map overlay loader, so a non-world-map
--      save will show fog_enable cleared and every probe quiet.
--   2. Run with:
--        LEGAIA_SSTATE=/path/to/topview.sstate \
--        LEGAIA_OUT=fog_probe.csv \
--        LEGAIA_FRAMES=600 \
--        ./scripts/pcsx-redux/run_world_map_probe.sh
--   3. Inspect fog_probe.csv (per-hit PC/value log) and the .snap.txt
--      sidecar (per-frame fog state + LUT contents).
--
-- Output CSV columns:
--   probe_idx, addr, pc, width, value, ra
-- Probe indices:
--   0  gp-0x2E0 (far ref)
--   1  gp-0x2DC (fog color)
--   2  gp-0x2D1 (enable byte)
--   3  gp-0x2BC (LUT pointer)
--   4  gp+0x90  (Z shift)

------------------------------------------------------------------
-- Configuration

local function getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

local SSTATE_PATH = getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = tonumber(getenv("LEGAIA_FRAMES", "600"))
local OUT_PATH    = getenv("LEGAIA_OUT",  "fog_probe.csv")
local HOLD_BUTTON = tonumber(getenv("LEGAIA_HOLD_BUTTON", "0"))
local HOLD_FRAMES = tonumber(getenv("LEGAIA_HOLD", "0"))

-- Optional gp override. By default the script reads gp from the live
-- register file after save-state load. PsyQ boot leaves gp pointing
-- into the small-data segment; on Legaia retail this lands around
-- 0x800xxxxx and is stable across the rest of the run, so the snapshot
-- value is reusable.
local GP_OVERRIDE = tonumber(getenv("LEGAIA_GP", "0"))

-- The five fog fields as (offset, width_bytes, label) triples. Order
-- matches the probe_idx column in the CSV.
local FOG_FIELDS = {
    { off = -0x2E0, width = 4, label = "far_ref"   },
    { off = -0x2DC, width = 4, label = "fog_color" },
    { off = -0x2D1, width = 1, label = "enable"    },
    { off = -0x2BC, width = 4, label = "lut_ptr"   },
    { off =  0x90,  width = 1, label = "z_shift"   },
}

-- LUT dump size in bytes. 1 KiB = 512 u16 entries, covering Z >> 5
-- over the full PSX 14-bit average-Z output range.
local LUT_DUMP_BYTES = 1024

local MAX_HITS_PER_PROBE = 1000

------------------------------------------------------------------
-- CSV setup

local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("probe_idx,addr,pc,width,value,ra\n")
    csv_fh:flush()
else
    PCSX.log("[fog] FATAL: cannot open " .. OUT_PATH .. ": " ..
        tostring(csv_err))
end

local SNAP_PATH = OUT_PATH:gsub("%.csv$", ".snap.txt")
local LUT_PATH  = OUT_PATH:gsub("%.csv$", ".lut.bin")

PCSX.log(string.format(
    "[fog] sstate=%s frames=%d out=%s snap=%s lut=%s",
    SSTATE_PATH, FRAMES, OUT_PATH, SNAP_PATH, LUT_PATH))

------------------------------------------------------------------
-- Memory helpers

local mem_file
local RAM_SIZE = 2 * 1024 * 1024

local function ensure_mem()
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
end

local function ram_offset(addr)
    return bit.band(addr, 0x1FFFFFFF)
end

local function in_ram(addr, width)
    local off = ram_offset(addr)
    return off >= 0 and off + width <= RAM_SIZE
end

local function read_u8(addr)
    ensure_mem()
    if not in_ram(addr, 1) then return nil end
    local ok, v = pcall(function() return mem_file:readU8At(ram_offset(addr)) end)
    if not ok then return nil end
    return tonumber(v)
end

local function read_u32(addr)
    ensure_mem()
    if not in_ram(addr, 4) then return nil end
    local ok, v = pcall(function() return mem_file:readU32At(ram_offset(addr)) end)
    if not ok then return nil end
    return tonumber(v)
end

local function read_field(addr, width)
    if width == 1 then return read_u8(addr) end
    return read_u32(addr)
end

local function dump_lut(lut_addr, bytes)
    ensure_mem()
    if not in_ram(lut_addr, bytes) then return nil end
    local off = ram_offset(lut_addr)
    -- PCSX-Redux File API: readAt(size, offset), not (offset, size).
    local ok, blob = pcall(function() return mem_file:readAt(bytes, off) end)
    if not ok then return nil end
    return blob
end

------------------------------------------------------------------
-- Probe state

local gp_base = 0
local hits = {}
local bps  = {}
local PROBE_ADDRS = {}

local function format_addr(addr)
    return string.format("0x%08X", bit.band(addr, 0xFFFFFFFF))
end

------------------------------------------------------------------
-- Arm + disarm

local function arm_probes()
    for i, field in ipairs(FOG_FIELDS) do
        local idx = i
        local addr = bit.band(gp_base + field.off, 0xFFFFFFFF)
        local width = field.width
        local label = field.label
        PROBE_ADDRS[i] = addr
        hits[i] = 0
        local cb = function(_, _, _)
            hits[idx] = hits[idx] + 1
            if hits[idx] > MAX_HITS_PER_PROBE then return end
            local ok, info = pcall(function()
                local r = PCSX.getRegisters()
                local pc = tonumber(r.pc) or 0
                local ra = tonumber(r.GPR.n.ra) or 0
                local val = read_field(addr, width) or 0
                return { pc = pc, ra = ra, val = val }
            end)
            if not ok then return end
            if csv_fh then
                csv_fh:write(string.format(
                    "%d,%s,%s,%d,0x%08X,%s\n",
                    idx - 1, format_addr(addr), format_addr(info.pc),
                    width, info.val, format_addr(info.ra)))
                csv_fh:flush()
            end
            if hits[idx] <= 3 then
                PCSX.log(string.format(
                    "[fog] probe %d %s (%s) hit %d: pc=%s val=0x%08X ra=%s",
                    idx - 1, label, format_addr(addr), hits[idx],
                    format_addr(info.pc), info.val, format_addr(info.ra)))
            end
            if hits[idx] == MAX_HITS_PER_PROBE then
                PCSX.log(string.format(
                    "[fog] probe %d %s cap reached at %d hits; further hits silently counted",
                    idx - 1, label, MAX_HITS_PER_PROBE))
            end
        end
        local bp = PCSX.addBreakpoint(
            addr, "Read", width, "fog:" .. label, cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format(
        "[fog] %d Read probes armed (gp=%s)",
        #PROBE_ADDRS, format_addr(gp_base)))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Snapshot writer (per-frame fog state + LUT)

local function write_snapshot(label, vsync_count, capture_start)
    local f = io.open(SNAP_PATH, "w")
    if not f then return end
    f:write(string.format(
        "# %s  vsync=%d  capture_start=%s  gp=%s\n",
        label, vsync_count, tostring(capture_start), format_addr(gp_base)))
    for i, field in ipairs(FOG_FIELDS) do
        local addr = PROBE_ADDRS[i] or 0
        local val  = read_field(addr, field.width)
        local hit  = hits[i] or 0
        local capped = hit > MAX_HITS_PER_PROBE and " (capped)" or ""
        f:write(string.format(
            "  probe %d  %-9s  %s  width=%d  val=%s  hits=%d%s\n",
            i - 1, field.label, format_addr(addr), field.width,
            val and string.format("0x%08X", val) or "<oob>",
            hit, capped))
    end
    -- Dump the LUT contents pointed to by gp-0x2BC.
    local lut_ptr = read_u32(PROBE_ADDRS[4] or 0)
    if lut_ptr and in_ram(lut_ptr, LUT_DUMP_BYTES) then
        local blob = dump_lut(lut_ptr, LUT_DUMP_BYTES)
        if blob then
            local lf = io.open(LUT_PATH, "wb")
            if lf then lf:write(blob); lf:close() end
            f:write(string.format(
                "  lut: %s (%d bytes) written to %s\n",
                format_addr(lut_ptr), LUT_DUMP_BYTES, LUT_PATH))
        end
    else
        f:write(string.format(
            "  lut: ptr=%s out-of-range; LUT not yet populated\n",
            lut_ptr and format_addr(lut_ptr) or "<nil>"))
    end
    f:close()
end

------------------------------------------------------------------
-- State machine: WAIT_BOOT -> ARMED_LOADED -> DONE

local STATE_WAIT_BOOT    = 1
local STATE_ARMED_LOADED = 2
local STATE_DONE         = 3

local state         = STATE_WAIT_BOOT
local vsync_count   = 0
local capture_start = nil

local BOOT_DELAY_VSYNCS = 60
local CAPTURE_VSYNCS    = FRAMES
local SNAPSHOT_EVERY    = 60

local function read_gp()
    local ok, gp = pcall(function()
        local r = PCSX.getRegisters()
        return tonumber(r.GPR.n.gp) or 0
    end)
    if not ok then return 0 end
    return gp
end

local function try_load_save_state()
    local fh = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log("[fog] FATAL: cannot open save state " .. SSTATE_PATH)
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[fog] save state loaded")
    return true
end

local pad_held = false
local function pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end
local function pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

local function on_vsync()
    vsync_count = vsync_count + 1

    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY_VSYNCS then
            if not try_load_save_state() then return end
            -- The save state load restored the register file, so we
            -- can read gp now. Override if requested.
            gp_base = GP_OVERRIDE ~= 0 and GP_OVERRIDE or read_gp()
            if gp_base == 0 then
                PCSX.log("[fog] WARN: gp=0 after load; using LEGAIA_GP env or aborting")
                PCSX.quit(3)
                return
            end
            arm_probes()
            -- Initial snapshot before any prims have fired.
            write_snapshot("initial", vsync_count, vsync_count)
            state         = STATE_ARMED_LOADED
            capture_start = vsync_count
            if HOLD_BUTTON ~= 0 and HOLD_FRAMES > 0 then
                pad_force(HOLD_BUTTON)
                pad_held = true
                PCSX.log(string.format(
                    "[fog] forcing button 0x%X held for %d vsyncs",
                    HOLD_BUTTON, HOLD_FRAMES))
            end
        end
    elseif state == STATE_ARMED_LOADED then
        if vsync_count % SNAPSHOT_EVERY == 0 then
            write_snapshot("live", vsync_count, capture_start)
        end
        if pad_held and vsync_count - capture_start >= HOLD_FRAMES then
            pad_release(HOLD_BUTTON)
            pad_held = false
            PCSX.log(string.format(
                "[fog] released button 0x%X at vsync %d",
                HOLD_BUTTON, vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(HOLD_BUTTON); pad_held = false end
            disarm_probes()
            write_snapshot("final", vsync_count, capture_start)
            PCSX.log("=== fog probe hit counts ===")
            for i, field in ipairs(FOG_FIELDS) do
                PCSX.log(string.format(
                    "  probe %d  %-9s  %s  hits=%d%s",
                    i - 1, field.label, format_addr(PROBE_ADDRS[i] or 0),
                    hits[i] or 0,
                    (hits[i] or 0) > MAX_HITS_PER_PROBE and " (capped)" or ""))
            end
            PCSX.log("=== end ===")
            if csv_fh then csv_fh:flush(); csv_fh:close(); csv_fh = nil end
            state = STATE_DONE
            PCSX.log("[fog] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[fog] vsync listener installed; waiting for boot")
