-- probe.lua  -- shared scaffolding for PCSX-Redux Lua autorun probes.
--
-- Every script under scripts/pcsx-redux/autorun_*.lua runs the same
-- WAIT_BOOT -> ARMED_LOADED -> DONE state machine around the same
-- handful of memory / save-state / pad / CSV helpers. This module
-- factors that scaffolding out so a new probe is the breakpoint
-- bodies + a 30-line driver, not 250 lines of boilerplate.
--
-- Usage:
--   package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
--   local probe = require("probe")
--
--   probe.run({
--     sstate         = probe.getenv("LEGAIA_SSTATE", default),
--     capture_frames = probe.getenv_num("LEGAIA_FRAMES", 600),
--     out_path       = probe.getenv("LEGAIA_OUT", "/tmp/x.csv"),
--     boot_delay     = 60,
--
--     on_arm = function(ctx)
--         -- arm breakpoints; return a list of probe descriptors used
--         -- by the snapshot writer. Each descriptor is
--         -- { addr=..., name=..., hits_ref={n=0} }.
--         local descs = {}
--         for _, addr in ipairs({ 0x80017EC8, 0x801E76D4 }) do
--             local d = { addr = addr, name = string.format("0x%08X", addr),
--                         hits_ref = { n = 0 } }
--             probe.arm_breakpoint(addr, "Exec", 4, d.name, function()
--                 d.hits_ref.n = d.hits_ref.n + 1
--             end)
--             descs[#descs+1] = d
--         end
--         return descs
--     end,
--
--     on_done = function(ctx, descs)
--         probe.write_snapshot(ctx.snapshot_path, "final", descs)
--     end,
--   })
--
-- Implementation notes:
--   - All callbacks run inside PCSX-Redux's interpreter loop. Wrap any
--     work that may touch unmapped memory in pcall; a thrown error in
--     a breakpoint callback silently disables the probe.
--   - The driver requires both -interpreter and -debugger flags
--     (psxinterpreter.cc:1652 only invokes Debug::process under
--     `if constexpr (debug)`). The shipping run_world_map_probe.sh
--     wrapper already passes both.

local M = {}

------------------------------------------------------------------
-- Configuration helpers

function M.getenv(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return v
end

function M.getenv_num(name, fallback)
    local v = os.getenv(name)
    if v == nil or v == "" then return fallback end
    return tonumber(v) or fallback
end

------------------------------------------------------------------
-- Memory access (lazy main-RAM file handle)

local RAM_SIZE = 2 * 1024 * 1024  -- 2 MiB main RAM
M.RAM_SIZE = RAM_SIZE

local mem_file = nil

local function get_mem_file()
    if mem_file == nil then mem_file = PCSX.getMemoryAsFile() end
    return mem_file
end

-- Convert a KSEG0 / KSEG1 / USEG virtual address to a main-RAM byte
-- offset. Returns nil if the address can't possibly hit main RAM
-- (scratchpad 0x1F8003xx, hardware regs 0x1F801xxx, BIOS, etc.).
function M.ram_offset(addr)
    local off = bit.band(addr, 0x1FFFFFFF)
    if off < 0 or off >= RAM_SIZE then return nil end
    return off
end

function M.in_ram(addr, width)
    local off = M.ram_offset(addr)
    if off == nil then return false end
    return off + (width or 1) <= RAM_SIZE
end

function M.read_u32(addr)
    local off = M.ram_offset(addr)
    if off == nil or off + 4 > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readU32At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

function M.read_u16(addr)
    local off = M.ram_offset(addr)
    if off == nil or off + 2 > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readU16At(off) end)
    if not ok then return nil end
    return tonumber(v)
end

function M.read_bytes(addr, len)
    local off = M.ram_offset(addr)
    if off == nil or off + len > RAM_SIZE then return nil end
    local mf = get_mem_file()
    local ok, v = pcall(function() return mf:readAt(len, off) end)
    if not ok then return nil end
    return v
end

-- Convert a LuaBuffer (cdata) or Lua string into uppercase hex.
function M.bytes_to_hex(buf)
    local s = tostring(buf)
    local out = {}
    for i = 1, #s do out[i] = string.format("%02X", s:byte(i)) end
    return table.concat(out)
end

-- Scratchpad reader. The 1 KiB scratchpad sits at 0x1F800000 and is
-- accessible via PCSX.getScratchPtr() as a uint8_t*. Any virtual
-- address in 0x1F800000..0x1F8003FF maps to that buffer.
local scratch_u32_ptr = nil
function M.read_scratch_u32(addr)
    if scratch_u32_ptr == nil then
        scratch_u32_ptr = ffi.cast("uint32_t*", PCSX.getScratchPtr())
    end
    local off = bit.band(addr, 0x3FF)
    return tonumber(scratch_u32_ptr[off / 4]) or 0
end

------------------------------------------------------------------
-- Save-state load

-- Open a gzipped save state on disk and load it into the running
-- emulator. PCSX-Redux's loadSaveState entry point does NOT
-- auto-decompress; we wrap the file with zReader so the decompressed
-- stream is what hits loadSaveState.
function M.load_save_state(path)
    local fh, err = Support.File.open(path, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format("[probe] FATAL: cannot open %s (%s)",
            path, tostring(err)))
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    return true
end

------------------------------------------------------------------
-- Pad override

-- PSX pad bit indices (D-pad + buttons in the 16-bit status word).
M.BTN = {
    SELECT = 0,  L3 = 1,  R3 = 2,  START = 3,
    UP     = 4,  RIGHT = 5,  DOWN = 6,  LEFT = 7,
    L2     = 8,  R2 = 9,  L1 = 10, R1 = 11,
    TRIANGLE = 12, CIRCLE = 13, CROSS = 14, SQUARE = 15,
}

-- setOverride forces a button held; clearOverride releases it. Wrapped
-- in pcall because pads may not be installed in headless boots and we
-- don't want a missing pad to crash the probe.
function M.pad_force(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].setOverride(button) end)
end

function M.pad_release(button)
    pcall(function() PCSX.SIO0.slots[1].pads[1].clearOverride(button) end)
end

------------------------------------------------------------------
-- Breakpoint helper
--
-- Thin wrapper around PCSX.addBreakpoint that namespaces the label,
-- protects the callback in a pcall (a thrown error from inside a
-- breakpoint callback silently disables the probe), and registers the
-- bp into a module-level list so disarm_all() can clean up at the end
-- of a capture run.

local _bps = {}

function M.arm_breakpoint(addr, kind, width, label, cb)
    local bp = PCSX.addBreakpoint(addr, kind, width, "probe:" .. label,
        function(...)
            local ok, err = pcall(cb, ...)
            if not ok then
                PCSX.log(string.format(
                    "[probe] callback error in %s: %s", label, tostring(err)))
            end
        end)
    _bps[#_bps + 1] = bp
    return bp
end

function M.disarm_all()
    for _, bp in ipairs(_bps) do
        pcall(function() bp:remove() end)
    end
    _bps = {}
end

------------------------------------------------------------------
-- CSV writer (auto-flushed per row so an early exit still leaves data)

local Csv = {}
Csv.__index = Csv

function M.csv_open(path, header)
    local fh, err = io.open(path, "w")
    if not fh then
        PCSX.log(string.format("[probe] FATAL: cannot open csv %s (%s)",
            path, tostring(err)))
        return nil
    end
    fh:write(header)
    if not header:find("\n$") then fh:write("\n") end
    fh:flush()
    return setmetatable({ fh = fh, path = path }, Csv)
end

function Csv:row(fmt, ...)
    if not self.fh then return end
    self.fh:write(string.format(fmt, ...))
    if not fmt:find("\n$") then self.fh:write("\n") end
    self.fh:flush()
end

function Csv:close()
    if self.fh then
        self.fh:flush()
        self.fh:close()
        self.fh = nil
    end
end

------------------------------------------------------------------
-- Snapshot writer
--
-- Emits a tab-separated hits summary keyed by probe address + name.
-- Rewritten on every snapshot so the latest state always survives
-- a crash or window-close. `descs` is a list of
--   { addr = uint, name = string, hits_ref = { n = int } }
-- (hits_ref is wrapped in a one-key table so the breakpoint
-- callback's closure can mutate it without needing array indexing.)

function M.write_snapshot(path, label, descs, extra_lines)
    if not path then return end
    local f = io.open(path, "w")
    if not f then return end
    f:write(string.format("# %s\n", label or "snapshot"))
    if extra_lines then
        for _, line in ipairs(extra_lines) do
            f:write("# " .. line .. "\n")
        end
    end
    for _, d in ipairs(descs or {}) do
        local hits = (d.hits_ref and d.hits_ref.n) or d.hits or 0
        f:write(string.format("  0x%08X  %10d  %s\n",
            d.addr, hits, d.name or ""))
    end
    f:close()
end

------------------------------------------------------------------
-- Register / call-context snapshot
--
-- Capture a "what's the CPU doing right now" record from inside a
-- breakpoint callback. The caller passes a `ctx` blob (a Lua string
-- with a label + free-text fields) and we serialise:
--   * all 32 GPRs by MIPS name
--   * the 8 instruction words straddling PC (PC-12 .. PC+16) so the
--     reader can see what instruction tripped the bp + the surrounding
--     basic block
--   * the 32 stack words at sp (sp+0 .. sp+0x7C). The MIPS calling
--     convention saves ra into the prologue's sp-relative slot for any
--     non-leaf function, so this captures the visible ra-chain without
--     needing real DWARF unwind info.
--
-- The caller is expected to walk the on-disc disassembly post-hoc to
-- locate the exact saved-ra slot for each frame; that beats trying to
-- guess in the probe and emitting wrong addresses.

local MIPS_GPR_NAMES = {
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0",   "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0",   "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8",   "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

function M.capture_call_context(label)
    local r        = PCSX.getRegisters()
    local pc       = tonumber(r.pc) or 0
    local sp       = tonumber(r.GPR.n.sp) or 0
    local ra       = tonumber(r.GPR.n.ra) or 0
    local lines    = {}
    lines[#lines + 1] = string.format("== %s ==", label or "snapshot")
    lines[#lines + 1] = string.format("pc=0x%08X  ra=0x%08X  sp=0x%08X",
        pc, ra, sp)

    -- GPR table, four per row.
    for i = 0, 7 do
        local row = {}
        for j = 0, 3 do
            local idx  = i * 4 + j
            local name = MIPS_GPR_NAMES[idx + 1]
            local val  = tonumber(r.GPR.r[idx]) or 0
            row[#row + 1] = string.format("%-3s=0x%08X", name, val)
        end
        lines[#lines + 1] = "  " .. table.concat(row, "  ")
    end

    -- Bytes around PC (32 instructions = 128 bytes; 8 before + 24 after).
    local pc_lo  = pc - 0x20
    local code   = M.read_bytes(pc_lo, 0x80)
    if code then
        lines[#lines + 1] = string.format("code  pc-0x20..pc+0x60 (0x%08X):", pc_lo)
        local s = tostring(code)
        for row = 0, 7 do
            local words = {}
            for w = 0, 3 do
                local off = row * 16 + w * 4 + 1
                if off + 3 <= #s then
                    local b0 = s:byte(off)
                    local b1 = s:byte(off + 1)
                    local b2 = s:byte(off + 2)
                    local b3 = s:byte(off + 3)
                    -- LE -> u32
                    local word = b0 + b1 * 0x100 + b2 * 0x10000 + b3 * 0x1000000
                    words[#words + 1] = string.format("%08X", word)
                end
            end
            local addr = pc_lo + row * 16
            local mark = (addr <= pc and pc < addr + 16) and " <- pc" or ""
            lines[#lines + 1] = string.format("  0x%08X  %s%s",
                addr, table.concat(words, " "), mark)
        end
    end

    -- Stack words at sp (32 u32s = 128 bytes). Saved ra typically lives
    -- inside this window for any non-leaf MIPS function.
    local stack = M.read_bytes(sp, 0x80)
    if stack then
        lines[#lines + 1] = string.format("stack sp..sp+0x80 (0x%08X):", sp)
        local s = tostring(stack)
        for row = 0, 7 do
            local words = {}
            for w = 0, 3 do
                local off = row * 16 + w * 4 + 1
                if off + 3 <= #s then
                    local b0 = s:byte(off)
                    local b1 = s:byte(off + 1)
                    local b2 = s:byte(off + 2)
                    local b3 = s:byte(off + 3)
                    local word = b0 + b1 * 0x100 + b2 * 0x10000 + b3 * 0x1000000
                    words[#words + 1] = string.format("%08X", word)
                end
            end
            lines[#lines + 1] = string.format("  +0x%02X  %s",
                row * 16, table.concat(words, " "))
        end
    end

    return table.concat(lines, "\n")
end

-- Append a captured call-context block to a text file. Used by probes
-- that want a flat append-only log of every "first hit" event.
function M.append_call_context(path, ctx_text)
    if not path then return end
    local f = io.open(path, "a")
    if not f then return end
    f:write(ctx_text)
    f:write("\n\n")
    f:close()
end

------------------------------------------------------------------
-- The state-machine driver
--
-- Options table:
--   sstate          (string)  path to save-state file (gzipped)
--   capture_frames  (int)     vsyncs to capture after load (default 600)
--   boot_delay      (int)     vsyncs to wait before load (default 60)
--   snapshot_every  (int)     vsyncs between live snapshots (default 60)
--   snapshot_path   (string)  optional snapshot file (auto-rotated each tick)
--   quit_delay      (int)     vsyncs after disarm before quit (default 30)
--
--   hold_button     (int|nil) pad button to hold while capturing
--   hold_frames     (int)     vsyncs to keep the button held
--
--   on_arm(ctx) -> descs      arm breakpoints; return descriptor list
--   on_capture(ctx, vsync_in_capture)  optional per-vsync hook
--   on_done(ctx, descs)       optional final-output writer (before quit)
--   on_summary(ctx, descs)    optional human-readable summary writer
--                             (defaults to PCSX.log(...) per descriptor)

function M.run(opts)
    local sstate         = assert(opts.sstate, "probe.run: opts.sstate required")
    local capture_frames = opts.capture_frames or 600
    local boot_delay     = opts.boot_delay or 60
    local snapshot_every = opts.snapshot_every or 60
    local quit_delay     = opts.quit_delay or 30
    local on_arm         = assert(opts.on_arm, "probe.run: opts.on_arm required")
    local on_capture     = opts.on_capture
    local on_done        = opts.on_done
    local on_summary     = opts.on_summary

    local ctx = {
        sstate         = sstate,
        capture_frames = capture_frames,
        snapshot_path  = opts.snapshot_path,
        descs          = nil,
    }

    PCSX.log(string.format(
        "[probe] sstate=%s frames=%d snapshot=%s",
        sstate, capture_frames, tostring(opts.snapshot_path)))

    local STATE_WAIT_BOOT    = 1
    local STATE_ARMED_LOADED = 2
    local STATE_DONE         = 3

    local state         = STATE_WAIT_BOOT
    local vsync_count   = 0
    local capture_start = nil
    local pad_held      = false

    local function on_vsync()
        vsync_count = vsync_count + 1

        if state == STATE_WAIT_BOOT then
            if vsync_count >= boot_delay then
                ctx.descs = on_arm(ctx) or {}
                if not M.load_save_state(sstate) then
                    PCSX.quit(2)
                    return
                end
                PCSX.log(string.format(
                    "[probe] %d probes armed; capture started", #ctx.descs))
                capture_start = vsync_count
                state         = STATE_ARMED_LOADED

                if opts.hold_button and (opts.hold_frames or 0) > 0 then
                    M.pad_force(opts.hold_button)
                    pad_held = true
                    PCSX.log(string.format(
                        "[probe] holding pad button %d for %d vsyncs",
                        opts.hold_button, opts.hold_frames))
                end
            end
        elseif state == STATE_ARMED_LOADED then
            local elapsed = vsync_count - capture_start

            if pad_held and elapsed >= (opts.hold_frames or 0) then
                M.pad_release(opts.hold_button)
                pad_held = false
                PCSX.log(string.format("[probe] released pad button %d at vsync %d",
                    opts.hold_button, elapsed))
            end

            if on_capture then on_capture(ctx, elapsed) end

            if ctx.snapshot_path and (vsync_count % snapshot_every) == 0 then
                M.write_snapshot(ctx.snapshot_path, "live", ctx.descs,
                    { string.format("vsync=%d capture_start=%d",
                        vsync_count, capture_start) })
            end

            -- Early-quit signal. Probes set `ctx.request_quit = true`
            -- when their stop condition is met (e.g. every probe has
            -- hit at least once); the driver exits the capture loop on
            -- the next vsync rather than waiting for capture_frames.
            if ctx.request_quit then
                PCSX.log("[probe] ctx.request_quit set; ending capture")
                elapsed = capture_frames
            end

            if elapsed >= capture_frames then
                if pad_held then
                    M.pad_release(opts.hold_button)
                    pad_held = false
                end
                M.disarm_all()
                if ctx.snapshot_path then
                    M.write_snapshot(ctx.snapshot_path, "final", ctx.descs,
                        { string.format("vsync=%d capture_frames=%d",
                            vsync_count, capture_frames) })
                end
                if on_summary then
                    on_summary(ctx, ctx.descs)
                else
                    PCSX.log("=== probe hits ===")
                    for _, d in ipairs(ctx.descs or {}) do
                        local hits = (d.hits_ref and d.hits_ref.n) or d.hits or 0
                        PCSX.log(string.format("  0x%08X  %10d  %s",
                            d.addr, hits, d.name or ""))
                    end
                    PCSX.log("=== end ===")
                end
                if on_done then on_done(ctx, ctx.descs) end
                state = STATE_DONE
                PCSX.log(string.format(
                    "[probe] capture done; quitting in %d vsyncs", quit_delay))
            end
        elseif state == STATE_DONE then
            if vsync_count - capture_start >= capture_frames + quit_delay then
                PCSX.quit(0)
            end
        end
    end

    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
    PCSX.log("[probe] vsync listener installed; waiting for boot")
end

return M
