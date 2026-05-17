-- probe/spec.lua  -- declarative .probe.toml -> probe.sm.run() translator.
--
-- A .probe.toml file is the "arm these N breakpoints, dump these K
-- columns" / "settle then dump this RAM region" recipe for a probe.
-- This module turns the parsed-TOML table into a probe.sm.run() opts
-- table by walking the spec, dispatching to the lib helpers, and
-- providing a built-in capture-columns vocabulary.
--
-- Schema (see scripts/pcsx-redux/probes/*.probe.toml for examples):
--   scenario        str  (informational; LEGAIA_SSTATE actually wins)
--   sstate          str  (literal save-state path; LEGAIA_SSTATE wins)
--   capture_frames  int  (default 600; LEGAIA_FRAMES wins)
--   boot_delay      int  (default 60)
--   output_path     str  (default <probe-stem>.csv; LEGAIA_OUT wins)
--   snapshot_path   str  (default derived from output_path)
--
-- Address-typed fields (dump.addr, breakpoint[i].addr, breakpoint_range[i].base)
-- accept *either* an integer literal *or* a Ghidra symbol name string
-- ("FUN_801DD35C", "_DAT_8007BCD0"). Symbol names resolve via
-- ghidra/scripts/symbols.lua (regenerate with scripts/pcsx-redux/build-symbols.py).
-- An unresolved symbol raises at spec-load time -- the probe never silently
-- arms at address 0.
--
-- Optional [dump] table (RAM-region dump on settle; mutually exclusive
-- with breakpoints / breakpoint_range):
--   [dump]
--   addr        u32
--   size        int   (bytes; defaults to RAM_SIZE if `size_ram = true`)
--   size_ram    bool  (use full 2 MiB if true)
--   output_path str   (defaults to spec.output_path with .bin)
--
-- Optional [[breakpoint]] entries:
--   addr   u32
--   kind   "Exec" | "Read" | "Write"
--   width  int (1/2/4)
--   name   str (default: hex of addr)
--
-- Optional [[breakpoint_range]] entries (fan out N adjacent bps):
--   base      u32
--   length    int (bytes total)
--   stride    int (bytes per bp; 1/2/4)
--   kind      "Exec" | "Read" | "Write"
--   name_fmt  str (printf format; %X / %x / %d substituted with the
--                  byte offset from base)
--
-- capture_columns = [...]  CSV column vocabulary. Per-name builders:
--   "tick"        per-bp hit counter (1-based, post-increment)
--   "addr"        bp address (hex)
--   "offset"      bp address - bp.base (0 for non-range bps; hex)
--   "pc"          r.pc at hit (hex)
--   "ra"          r.GPR.n.ra at hit (hex)
--   "sp"          r.GPR.n.sp at hit (hex)
--   "width"       bp width (int)
--   "value_u8"    read_u8(addr) (hex 0x%02X)
--   "value_u16"   read_u16(addr) (hex 0x%04X)
--   "value_u32"   read_u32(addr) (hex 0x%08X)
-- Unknown column names raise an error at spec-load time.
--
-- Optional [detail] table: write a sidecar text file with full
-- register/code/stack snapshots for the first N hits across all bps.
--   [detail]
--   hits = 8
--   path = "<stem>.detail.txt"

local mem      = require("probe.mem")
local bp       = require("probe.bp")
local csv_lib  = require("probe.csv")
local snapshot = require("probe.snapshot")
local env      = require("probe.env")
local sm       = require("probe.sm")
local symbols_mod = require("probe.symbols")

local M = {}

------------------------------------------------------------------
-- Symbol-name resolution. Lazy-load the symbols table so probes that
-- only use literal addresses don't pay for the dofile().

local _symbols_cache  -- nil until first symbol-typed field is seen

local function symbols()
    if _symbols_cache == nil then
        _symbols_cache = symbols_mod.load()  -- raises if missing
    end
    return _symbols_cache
end

-- Accept int (taken as-is) or string (resolved via ghidra/scripts/symbols.lua).
-- Anything else is a schema error. Symbol misses raise loudly (via the
-- __index guard in probe.symbols) so a typo never silently arms at 0.
local function resolve_addr(v, ctx)
    if type(v) == "number" then return v end
    if type(v) == "string" then
        local ok, addr = pcall(function() return symbols()[v] end)
        if ok and type(addr) == "number" then return addr end
        error(string.format(
            "probe.spec: cannot resolve symbol '%s' in %s: %s",
            v, ctx, tostring(addr or "<unknown>")), 2)
    end
    error(string.format(
        "probe.spec: %s must be an integer address or a symbol-name string (got %s)",
        ctx, type(v)), 2)
end

------------------------------------------------------------------
-- Capture-column vocabulary

local function col_value_uN(width)
    if width == 1 then return mem.read_u8 end
    if width == 2 then return mem.read_u16 end
    if width == 4 then return mem.read_u32 end
    error("probe.spec: unsupported value_uN width " .. tostring(width))
end

-- Each builder returns (fmt_spec, fn(bp_ctx, regs)). The function
-- returns the string to splice into csv:row.
local COLUMN_BUILDERS = {
    tick      = function() return "%d",     function(c, _r) return c.hits end end,
    addr      = function() return "0x%08X", function(c, _r) return c.addr end end,
    offset    = function() return "0x%03X", function(c, _r) return c.addr - (c.base or c.addr) end end,
    pc        = function() return "0x%08X", function(_c, r) return tonumber(r.pc) or 0 end end,
    ra        = function() return "0x%08X", function(_c, r) return tonumber(r.GPR.n.ra) or 0 end end,
    sp        = function() return "0x%08X", function(_c, r) return tonumber(r.GPR.n.sp) or 0 end end,
    width     = function() return "%d",     function(c, _r) return c.width end end,
    value_u8  = function() return "0x%02X", function(c, _r) return mem.read_u8(c.addr)  or 0 end end,
    value_u16 = function() return "0x%04X", function(c, _r) return mem.read_u16(c.addr) or 0 end end,
    value_u32 = function() return "0x%08X", function(c, _r) return mem.read_u32(c.addr) or 0 end end,
}

local function build_capture(columns)
    if not columns or #columns == 0 then return nil end
    local fmts, fns, names = {}, {}, {}
    for _, name in ipairs(columns) do
        local builder = COLUMN_BUILDERS[name]
        if not builder then
            error("probe.spec: unknown capture column '" .. tostring(name) ..
                  "'. Known: tick/addr/offset/pc/ra/sp/width/value_u8/value_u16/value_u32")
        end
        local fmt, fn = builder()
        fmts[#fmts + 1]  = fmt
        fns[#fns + 1]   = fn
        names[#names + 1] = name
    end
    return {
        header_line = table.concat(names, ","),
        row_format  = table.concat(fmts, ","),
        producers   = fns,
    }
end

------------------------------------------------------------------
-- Descriptor builders

local function default_name_for(addr)
    return string.format("0x%08X", addr)
end

local function bp_descriptors_from_list(list)
    local out = {}
    for i, entry in ipairs(list or {}) do
        local addr = resolve_addr(entry.addr,
            "breakpoint[" .. i .. "].addr")
        -- Default name uses the symbol string when given (more readable
        -- in CSVs than a hex address).
        local default_name = type(entry.addr) == "string"
            and entry.addr or default_name_for(addr)
        out[#out + 1] = {
            addr  = addr,
            kind  = entry.kind  or "Exec",
            width = entry.width or 4,
            name  = entry.name  or default_name,
            base  = addr,  -- offset column reads as 0 for non-range bps
        }
    end
    return out
end

local function bp_descriptors_from_range(list)
    local out = {}
    for r_idx, r in ipairs(list or {}) do
        local base = resolve_addr(r.base,
            "breakpoint_range[" .. r_idx .. "].base")
        if type(r.length) ~= "number" then
            error("probe.spec: breakpoint_range[" .. r_idx .. "].length missing/non-numeric")
        end
        local stride = r.stride or 4
        local kind   = r.kind   or "Read"
        local fmt    = r.name_fmt or "0x%08X+0x%X"
        local off = 0
        while off < r.length do
            local w = math.min(stride, r.length - off)
            local addr = base + off
            -- Try printf with the offset; if the format has no specifier
            -- the result is just the literal string.
            local ok, name = pcall(string.format, fmt, off)
            if not ok then name = string.format("0x%08X", addr) end
            out[#out + 1] = {
                addr  = addr,
                kind  = kind,
                width = w,
                name  = name,
                base  = base,
            }
            off = off + stride
        end
    end
    return out
end

------------------------------------------------------------------
-- Probe shapes

-- Shape 1: RAM dump on settle (no breakpoints).
local function build_dump_run(spec, defaults)
    local d = spec.dump
    local addr = resolve_addr(d.addr, "[dump].addr")
    local size = d.size_ram and mem.RAM_SIZE or d.size
    if type(size) ~= "number" then
        error("probe.spec: [dump] requires size or size_ram=true")
    end
    local out_path = env.out_path(d.output_path or defaults.output_path or "dump.bin")

    return {
        sstate         = defaults.sstate,
        capture_frames = defaults.capture_frames,
        boot_delay     = defaults.boot_delay,
        on_arm = function(_)
            PCSX.log(string.format("[spec] dump: settling %d vsyncs before dump",
                defaults.capture_frames))
            return {}
        end,
        on_done = function(_, _)
            local buf = mem.read_bytes(addr, size)
            if buf == nil then
                PCSX.log(string.format("[spec] dump: FATAL: cannot read %d bytes at 0x%08X",
                    size, addr))
                return
            end
            local s = tostring(buf)
            local fh, err = io.open(out_path, "wb")
            if fh == nil then
                PCSX.log(string.format("[spec] dump: FATAL: cannot open %s: %s",
                    out_path, tostring(err)))
                return
            end
            fh:write(s); fh:close()
            PCSX.log(string.format("[spec] dump: wrote %d bytes to %s", #s, out_path))
        end,
    }
end

-- Shape 2: breakpoint-driven capture (list and/or range bps).
local function build_bp_run(spec, defaults)
    local descs = {}
    for _, d in ipairs(bp_descriptors_from_list(spec.breakpoint or {}))     do descs[#descs+1] = d end
    for _, d in ipairs(bp_descriptors_from_range(spec.breakpoint_range or {})) do descs[#descs+1] = d end
    if #descs == 0 then
        error("probe.spec: spec has neither [dump] nor [[breakpoint]] nor [[breakpoint_range]]")
    end

    local capture = build_capture(spec.capture_columns)
    local out_path = env.out_path(defaults.output_path or "probe.csv")
    local snapshot_path = defaults.snapshot_path or out_path:gsub("%.csv$", ".hits.txt")
    local csv_file = nil
    if capture then
        csv_file = csv_lib.open(out_path, capture.header_line)
    end

    -- Detail sidecar (first N hits get full call-context).
    local detail = spec.detail
    local detail_path
    if detail and detail.hits and detail.hits > 0 then
        detail_path = detail.path or out_path:gsub("%.csv$", ".detail.txt")
        local fh = io.open(detail_path, "w")
        if fh then
            fh:write(string.format(
                "# detail sidecar; %d hits captured; sstate=%s\n\n",
                detail.hits, defaults.sstate))
            fh:close()
        end
    end
    local detail_remaining = detail and detail.hits or 0

    local function arm_each(d)
        d.hits_ref = { n = 0 }
        d.hits     = 0  -- kept in sync with hits_ref.n for column "tick"
        bp.arm(d.addr, d.kind, d.width, d.name, function()
            d.hits_ref.n = d.hits_ref.n + 1
            d.hits       = d.hits_ref.n
            if csv_file and capture then
                local r = PCSX.getRegisters()
                local vals = {}
                for i, fn in ipairs(capture.producers) do
                    vals[i] = fn(d, r)
                end
                csv_file:row(capture.row_format, unpack(vals))
            end
            if detail_remaining > 0 then
                local label = string.format("hit: %s @ 0x%08X (width=%d)",
                    d.name, d.addr, d.width)
                snapshot.append_call_context(detail_path,
                    snapshot.capture_call_context(label))
                detail_remaining = detail_remaining - 1
            end
        end)
    end

    return {
        sstate         = defaults.sstate,
        capture_frames = defaults.capture_frames,
        boot_delay     = defaults.boot_delay,
        snapshot_path  = snapshot_path,
        on_arm = function(_)
            for _, d in ipairs(descs) do arm_each(d) end
            PCSX.log(string.format("[spec] armed %d bps; csv=%s", #descs, tostring(out_path)))
            return descs
        end,
        on_done = function(_, _)
            if csv_file then csv_file:close() end
            PCSX.log(string.format("[spec] capture done; csv=%s", tostring(out_path)))
        end,
    }
end

------------------------------------------------------------------
-- Public entry: load + run a spec.

local function resolve_sstate(spec)
    -- LEGAIA_SSTATE always wins (run_probe.sh resolves --scenario before us).
    local from_env = env.getenv("LEGAIA_SSTATE", nil)
    if from_env then return from_env end
    if type(spec.sstate) == "string" and spec.sstate ~= "" then
        return spec.sstate
    end
    error("probe.spec: no sstate path (set LEGAIA_SSTATE, --scenario, or spec.sstate)")
end

function M.run(spec)
    if type(spec) ~= "table" then error("probe.spec.run: spec must be a table") end

    local defaults = {
        sstate         = resolve_sstate(spec),
        capture_frames = env.getenv_num("LEGAIA_FRAMES", spec.capture_frames or 600),
        boot_delay     = spec.boot_delay or 60,
        output_path    = spec.output_path,
        snapshot_path  = spec.snapshot_path,
    }

    local has_dump = spec.dump ~= nil
    local has_bps  = (spec.breakpoint and #spec.breakpoint > 0)
                  or (spec.breakpoint_range and #spec.breakpoint_range > 0)
    if has_dump and has_bps then
        error("probe.spec: [dump] is mutually exclusive with [[breakpoint]]/[[breakpoint_range]]")
    end

    local args
    if has_dump then
        args = build_dump_run(spec, defaults)
    else
        args = build_bp_run(spec, defaults)
    end

    sm.run(args)
end

return M
