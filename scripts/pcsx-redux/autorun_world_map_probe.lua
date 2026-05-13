-- autorun_world_map_probe.lua
--
-- Closed-loop world-map VM probe. Designed to be passed via PCSX-Redux's
-- -dofile flag at launch. Loads a save state from a known path (slot 1
-- by default), arms the probe breakpoints, captures N frames of VSync,
-- writes a CSV summary, and quits the emulator.
--
-- Run via:
--   ~/Tools/pcsx-redux/pcsx-redux \
--     -interpreter \
--     -iso <legaia.bin> \
--     -run \
--     -dofile scripts/pcsx-redux/autorun_world_map_probe.lua
--
-- Customise via environment variables read at script start:
--   LEGAIA_SSTATE   path to .sstate file to load   (default slot 1 path)
--   LEGAIA_FRAMES   how many post-load VSyncs to capture (default 600)
--   LEGAIA_OUT      output CSV path               (default world_map_probe.csv)
--
-- Output: a CSV summary at LEGAIA_OUT plus a stdout report. Then quit(0).
--
-- Caveats:
--  - REQUIRES interpreter mode (-interpreter). Breakpoints don't fire under
--    dynarec (Debug::process is only called from psxinterpreter.cc).
--  - The save state needs to already have the world map active. We don't
--    drive the controller into world-map view; we just resume from a state
--    that's already there.
--  - The Lua dofile runs before the emulator main loop. We defer all real
--    work to event listeners (GPU::Vsync) so we run in-emulator context.

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
local OUT_PATH    = getenv("LEGAIA_OUT", "world_map_probe.csv")
-- LEGAIA_HOLD_UP: when nonzero, the autorun forces D-pad UP held for the
-- specified number of vsyncs starting at state-load time. Used to drive
-- a town-state character into a world-map exit so the transition fires
-- while probes are armed. PSX pad bit 4 = UP. Set to 0 to disable.
local HOLD_UP_FRAMES = tonumber(getenv("LEGAIA_HOLD_UP", "0"))
local BTN_UP = 4

PCSX.log(string.format("[autorun] sstate=%s frames=%d out=%s",
    SSTATE_PATH, FRAMES, OUT_PATH))

------------------------------------------------------------------
-- Probe definitions (mirrors probe_world_map_callchain.lua but
-- counts only, no auto-stop).

local PROBES = {
    { addr = 0x80017EC8, name = "AddPrim_dispatch (sanity)"   },
    { addr = 0x801E76D4, name = "world_map_controller"        },
    { addr = 0x80023070, name = "move_vm_entry"               },
    { addr = 0x80023AE0, name = "move_vm_op_0x2F"             },
    { addr = 0x801D362C, name = "world_map_draw_vm"           },
    { addr = 0x801D31B0, name = "scanline_emitter"            },
    { addr = 0x801D6704, name = "world_map_main_init"         },
    { addr = 0x801D7EA0, name = "FUN_801d7ea0 POLY_FT4 emit"  },
}

-- Per-call samples captured when the world-map VM dispatcher fires.
-- Same shape as scripts/pcsx-redux/log_world_map_vm.lua so the existing
-- analyzer (tools/analyze_world_map_vm_log.py) Just Works on the output.
local SAMPLE_PROBE_ADDR = 0x801D362C  -- world_map_draw_vm
local SAMPLE_DUMP_LEN   = 64
local samples = {}      -- list of { call_idx, a0, a1, sub_op, bytes_hex }

-- Secondary capture: at FUN_801D7EA0 entry, record the current value of
-- the OT pool cursor at scratchpad 0x1F8003A0 (per docs/reference/functions.md
-- for FUN_8003F3FC). That tells us which pool FUN_801D7EA0 is emitting
-- into - same `0x800AD400` pool, or a different work-buffer slot.
local OT_PROBE_ADDR     = 0x801D7EA0
local OT_CURSOR_ADDR    = 0x1F8003A0   -- scratchpad: current OT prim ptr
local ot_samples_path   = (os.getenv("LEGAIA_OUT") or "world_map_probe.csv")
                          :gsub("%.csv$", ".ot_cursor.csv")
local ot_csv_fh         = io.open(ot_samples_path, "w")
if ot_csv_fh then
    ot_csv_fh:write("hit_idx,ot_cursor_at_entry,a0,a1\n")
    ot_csv_fh:flush()
    PCSX.log("[autorun] OT cursor log -> " .. ot_samples_path)
end
local ot_hit_idx = 0

local hits = {}
for _, p in ipairs(PROBES) do hits[p.addr] = 0 end

local bps = {}

-- Open the CSV at module-load time and stream rows as they're captured.
-- Even if PCSX-Redux exits early (window closed, crash), the rows we've
-- already captured survive on disk.
local csv_fh, csv_err = io.open(OUT_PATH, "w")
if csv_fh then
    csv_fh:write("call_idx,a0_render_ctx,a1_bytecode_pc,sub_op,bytes_hex\n")
    csv_fh:flush()
else
    PCSX.log("[autorun] FATAL: cannot open " .. OUT_PATH .. ": " ..
        tostring(csv_err))
end

local function bytes_to_hex(buf)
    -- LuaBuffer (cdata) returned by readAt has __tostring that calls
    -- ffi.string(data, size). Convert to a Lua string first, then we can
    -- use :byte(i). Plain Lua strings pass through tostring unchanged.
    local s = tostring(buf)
    local out = {}
    for i = 1, #s do out[i] = string.format("%02X", s:byte(i)) end
    return table.concat(out)
end

local mem_file
local RAM_SIZE = 2 * 1024 * 1024  -- 2 MB main RAM

-- Scratchpad reader: PCSX.getScratchPtr() returns a uint8_t* into the
-- 1 KB PSX scratchpad mirrored at 0x1F800000. The OT cursor at 0x1F8003A0
-- lives there.
local scratch_u32_ptr = ffi.cast("uint32_t*", PCSX.getScratchPtr())
local function read_scratch_u32(addr)
    -- addr is the full virtual scratchpad address (0x1F800xxx).
    -- 0x1F800000..0x1F8003FF maps to scratch[0..0x3FF].
    local off = bit.band(addr, 0x3FF)
    return tonumber(scratch_u32_ptr[off / 4]) or 0
end

local function ram_offset(addr)
    -- Convert KSEG0/KSEG1/USEG virtual address to a main-RAM offset.
    -- Returns nil if the address can't possibly hit main RAM (e.g.
    -- scratchpad 0x1F8003xx, hardware regs 0x1F8010xx, or BIOS).
    local off = bit.band(addr, 0x1FFFFFFF)
    if off < 0 or off >= RAM_SIZE then return nil end
    return off
end

-- The File wrapper from getMemoryAsFile() uses rSeek/wSeek, not seek.
-- Easier: use the position-explicit accessors readU16At(pos) and
-- readAt(len, pos) which seek+read in one call.
local function read_bytes_at(addr, len)
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    local off = ram_offset(addr)
    if off == nil then return nil end
    return mem_file:readAt(len, off)
end

local function read_u16_at(addr)
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    local off = ram_offset(addr)
    if off == nil then return 0xFFFF end
    return mem_file:readU16At(off)
end

local function arm_probes()
    for _, p in ipairs(PROBES) do
        local addr     = p.addr
        local is_main  = (addr == SAMPLE_PROBE_ADDR)
        local cb
        if addr == OT_PROBE_ADDR then
            -- FUN_801D7EA0 entry: read the OT cursor it's about to write to.
            local probe_addr = addr
            cb = function()
                hits[probe_addr] = hits[probe_addr] + 1
                ot_hit_idx = ot_hit_idx + 1
                local ok, a0, a1, cursor = pcall(function()
                    local r = PCSX.getRegisters().GPR.n
                    return r.a0, r.a1, read_scratch_u32(OT_CURSOR_ADDR)
                end)
                if not ok then return end
                if ot_csv_fh then
                    ot_csv_fh:write(string.format(
                        "%d,0x%08X,0x%08X,0x%08X\n",
                        ot_hit_idx,
                        tonumber(cursor) or 0,
                        tonumber(a0) or 0,
                        tonumber(a1) or 0))
                    ot_csv_fh:flush()
                end
                if ot_hit_idx <= 3 then
                    PCSX.log(string.format(
                        "[autorun] FUN_801D7EA0 hit %d: OT cursor=0x%08X a0=0x%08X a1=0x%08X",
                        ot_hit_idx, tonumber(cursor) or 0,
                        tonumber(a0) or 0, tonumber(a1) or 0))
                end
            end
        elseif is_main then
            cb = function()
                hits[addr] = hits[addr] + 1
                -- Capture address-only sample FIRST so even if memory reads
                -- fail we still know where the bytecode pointer (a1) was.
                local ok_regs, a0, a1 = pcall(function()
                    local r = PCSX.getRegisters().GPR.n
                    return r.a0, r.a1
                end)
                if not ok_regs then
                    if hits[addr] <= 3 then
                        PCSX.log("[autorun] reg-read failed: " .. tostring(a0))
                    end
                    return
                end
                -- LuaJIT FFI uint32_t comes back as a cdata; coerce to
                -- a plain Lua number for arithmetic + table storage.
                a0 = tonumber(a0) or 0
                a1 = tonumber(a1) or 0
                local sample = {
                    call_idx  = #samples,
                    a0        = a0,
                    a1        = a1,
                    sub_op    = 0xFFFF,
                    bytes_hex = "",
                }
                samples[#samples + 1] = sample
                -- Memory reads can fail if a1 isn't in main RAM. pcall keeps
                -- the breakpoint alive even on failure.
                local ok_mem, mem_err = pcall(function()
                    local sub_op  = read_u16_at(a1 + 2)
                    local raw     = read_bytes_at(a1, SAMPLE_DUMP_LEN)
                    sample.sub_op    = tonumber(sub_op) or 0xFFFF
                    sample.bytes_hex = raw and bytes_to_hex(raw) or ""
                end)
                if not ok_mem and hits[addr] <= 3 then
                    PCSX.log(string.format(
                        "[autorun] mem-read failed at a1=0x%08X: %s",
                        a1, tostring(mem_err)))
                end
                -- Stream the row to the CSV immediately so an early exit
                -- (user closes window, crash) still leaves data on disk.
                if csv_fh then
                    csv_fh:write(string.format("%d,0x%08X,0x%08X,0x%04X,%s\n",
                        sample.call_idx, sample.a0, sample.a1,
                        sample.sub_op, sample.bytes_hex))
                    csv_fh:flush()
                end
                if hits[addr] <= 8 then
                    PCSX.log(string.format(
                        "[autorun] draw_vm hit %d: a0=0x%08X a1=0x%08X sub_op=0x%04X",
                        hits[addr], a0, a1, sample.sub_op))
                end
            end
        else
            cb = function() hits[addr] = hits[addr] + 1 end
        end
        local bp = PCSX.addBreakpoint(
            addr, "Exec", 4, "probe:" .. p.name, cb)
        bps[#bps + 1] = bp
    end
    PCSX.log(string.format("[autorun] %d probes armed", #PROBES))
end

local function disarm_probes()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
end

------------------------------------------------------------------
-- Output

local function write_csv()
    -- Rows are streamed per-hit while running; here we just close the
    -- handle and report what landed on disk.
    if csv_fh then
        csv_fh:flush()
        csv_fh:close()
        csv_fh = nil
    end
    if ot_csv_fh then
        ot_csv_fh:flush()
        ot_csv_fh:close()
        ot_csv_fh = nil
    end
    PCSX.log(string.format("[autorun] %d sample rows in %s",
        #samples, OUT_PATH))
    PCSX.log(string.format("[autorun] %d OT-cursor rows in %s",
        ot_hit_idx, ot_samples_path))
end

local function dump_summary()
    PCSX.log("=== world-map probe hits ===")
    for _, p in ipairs(PROBES) do
        PCSX.log(string.format("  0x%08X  %8d  %s",
            p.addr, hits[p.addr], p.name))
    end
    PCSX.log("=== end ===")
end

------------------------------------------------------------------
-- State machine: WAIT_BOOT -> ARM_THEN_LOAD -> CAPTURE -> DONE
--
-- The save state may itself be a scene-transition snapshot (fade-in
-- frame). In that case the terrain BUILDER runs in the first few frames
-- after load and any post-load warmup before arming will miss it. So
-- arm the probes FIRST, then load the state, and start counting capture
-- frames immediately. The probes are hot when the loaded state begins
-- executing.

local STATE_WAIT_BOOT     = 1
local STATE_ARMED_LOADED  = 2
local STATE_DONE          = 3

local state           = STATE_WAIT_BOOT
local vsync_count     = 0
local capture_start   = nil

local BOOT_DELAY_VSYNCS    = 60     -- wait for BIOS to settle before load
local CAPTURE_VSYNCS       = FRAMES -- duration of the actual capture

local function try_load_save_state()
    -- Save-state files on disk are gzip-compressed (see GUI::saveSaveState
    -- which uses ZWriter::GZIP). The Lua loadSaveStateFromFile entry point
    -- does NOT auto-decompress, so we wrap the raw file with zReader and
    -- pass the decompressed stream.
    local fh, err = Support.File.open(SSTATE_PATH, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format(
            "[autorun] FATAL: cannot open save state %s (%s)",
            SSTATE_PATH, tostring(err)))
        PCSX.quit(2)
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    PCSX.log("[autorun] save state loaded")
    return true
end

-- Pad helpers. PCSX-Redux exposes pad override via
-- PCSX.SIO0.slots[1].pads[1].{setOverride,clearOverride}(button_idx).
-- setOverride forces the button held (override mask bit cleared);
-- clearOverride releases it (override mask bit set).
local pad_held = false
local function pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end
local function pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

-- Live hit-count snapshot. Rewritten every SNAPSHOT_EVERY vsyncs so the
-- summary survives even if pcsx-redux's stdout log buffer doesn't flush
-- before quit. Same format as dump_summary's PCSX.log lines.
local HITS_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")
local function write_live_hits(label)
    local f = io.open(HITS_PATH, "w")
    if not f then return end
    f:write(string.format("# %s  vsync=%d  capture_start=%s\n",
        label, vsync_count, tostring(capture_start)))
    for _, p in ipairs(PROBES) do
        f:write(string.format("  0x%08X  %8d  %s\n",
            p.addr, hits[p.addr], p.name))
    end
    f:close()
end
local SNAPSHOT_EVERY = 60

local function on_vsync()
    vsync_count = vsync_count + 1
    if vsync_count % SNAPSHOT_EVERY == 0 then
        write_live_hits("live")
    end

    if state == STATE_WAIT_BOOT then
        if vsync_count >= BOOT_DELAY_VSYNCS then
            arm_probes()  -- ARM FIRST so we catch the very first frames
            if try_load_save_state() then
                state         = STATE_ARMED_LOADED
                capture_start = vsync_count
                PCSX.log("[autorun] probes armed before load; capture started")
                if HOLD_UP_FRAMES > 0 then
                    pad_force(BTN_UP)
                    pad_held = true
                    PCSX.log(string.format(
                        "[autorun] forcing D-pad UP held for %d vsyncs",
                        HOLD_UP_FRAMES))
                end
            end
        end
    elseif state == STATE_ARMED_LOADED then
        -- Release UP after HOLD_UP_FRAMES so the character stops walking
        -- and the transition can settle.
        if pad_held and vsync_count - capture_start >= HOLD_UP_FRAMES then
            pad_release(BTN_UP)
            pad_held = false
            PCSX.log(string.format(
                "[autorun] released D-pad UP at vsync %d",
                vsync_count - capture_start))
        end
        if vsync_count - capture_start >= CAPTURE_VSYNCS then
            if pad_held then pad_release(BTN_UP); pad_held = false end
            disarm_probes()
            write_live_hits("final")
            dump_summary()
            write_csv()
            state = STATE_DONE
            PCSX.log("[autorun] capture done; quitting in 30 vsyncs")
        end
    elseif state == STATE_DONE then
        if vsync_count - capture_start >= CAPTURE_VSYNCS + 30 then
            PCSX.quit(0)
        end
    end
end

local vsync_listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

PCSX.log("[autorun] vsync listener installed; waiting for boot")
