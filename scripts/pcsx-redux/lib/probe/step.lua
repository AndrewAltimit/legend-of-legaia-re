-- probe/step.lua  -- instruction-level tracing + memory-write attribution.
--
-- PCSX-Redux's Lua FFI exposes no native single-step (only pause/resume and
-- non-pausing breakpoints, which fire a callback and let execution continue).
-- These primitives reconstruct "stepping" from breakpoints so RE that needs
-- per-instruction or per-write attribution is scriptable instead of requiring
-- the interactive GUI debugger:
--
--   step.trace(lo, hi, opts)
--       Arm an Exec breakpoint on every 4-byte-aligned instruction in
--       [lo, hi). Each fires in execution order with LIVE pre-execution
--       registers and is recorded -- an observational single-step over a code
--       region, free of the before/after-timing and watch-width ambiguities
--       that confound write-watchpoints. opts.gate() (optional) restricts
--       recording to a window (e.g. one frame); opts.on_insn(pc, regs) is an
--       optional live callback; opts.max caps the record count.
--
--   step.find_writer(addr, len, opts)
--       Arm a Write breakpoint covering [addr, addr+len) and record every
--       store that touches it: the faulting PC + live registers + the bytes
--       now at addr. WIDTH-CORRECT -- it catches wider / mis-aligned stores a
--       narrow exact-address watch misses (the trap that hid the Mei's-house
--       door reposition behind a 2-byte no-op re-store).
--
-- Both return a handle with :count(), :records(), and :dump(path). Use
-- probe.bp.disarm() (or probe.disarm_all()) at end of capture to clean up.

local bp = require("probe.bp")
local mem = require("probe.mem")

local M = {}

-- 32-bit-masked snapshot of the integer registers + pc. Cheap; called per hit.
local GPRS = {
    "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "gp", "sp", "ra",
}

function M.regs()
    local r = PCSX.getRegisters()
    local out = { pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF) }
    local n = r.GPR.n
    for _, name in ipairs(GPRS) do
        out[name] = bit.band(tonumber(n[name]) or 0, 0xFFFFFFFF)
    end
    return out
end

-- Format a record (pc + regs) as one line; `regs` is a M.regs() table.
local function fmt_regs(rg)
    local parts = {}
    for _, name in ipairs(GPRS) do
        parts[#parts + 1] = string.format("%s=%08X", name, rg[name])
    end
    return table.concat(parts, " ")
end

local function new_handle()
    local h = { _rec = {} }
    function h:count() return #self._rec end
    function h:records() return self._rec end
    function h:dump(path)
        local f = io.open(path, "w")
        if not f then return end
        f:write(string.format("# %d records\n", #self._rec))
        for i, rec in ipairs(self._rec) do
            f:write(string.format("%5d  pc=0x%08X  %s%s\n", i, rec.pc,
                rec.note and (rec.note .. "  ") or "", fmt_regs(rec)))
        end
        f:close()
    end
    return h
end

-- Arm an Exec breakpoint on every instruction in [lo, hi). Returns a handle.
function M.trace(lo, hi, opts)
    opts = opts or {}
    local gate = opts.gate
    local on_insn = opts.on_insn
    local max = opts.max or 20000
    local h = new_handle()
    local stopped = false
    local function record(pc)
        if stopped then return end
        if gate and not gate() then return end
        local rg = M.regs()
        rg.pc = pc
        h._rec[#h._rec + 1] = rg
        if #h._rec >= max then stopped = true end
        if on_insn then on_insn(pc, rg) end
    end
    local addr = lo
    while addr < hi do
        local a = addr
        bp.arm(a, "Exec", 4, string.format("trace_%08X", a), function() record(a) end)
        addr = addr + 4
    end
    return h
end

-- Watch every store touching [addr, addr+len). PCSX Write breakpoints only
-- support widths 1/2/4, so a range is covered by arming a width-`unit`
-- breakpoint (default 2) at each `unit`-aligned slot across the range -- the
-- robust way to catch a write of unknown width/alignment to a struct field.
-- Records each store's faulting PC + live regs + the `read_len` bytes now at
-- `addr` (default `len`). A wide store may fire two adjacent slots; records are
-- de-duplicated per (pc, post-bytes).
function M.find_writer(addr, len, opts)
    opts = opts or {}
    local unit = opts.unit or 2
    local read_len = opts.read_len or len
    local on_write = opts.on_write
    local max = opts.max or 4096
    local h = new_handle()
    local last_key = nil
    local function on_store()
        if #h._rec >= max then return end
        local rg = M.regs()
        local now = mem.read_bytes(addr, read_len)
        local hex = now and mem.bytes_to_hex(now) or "?"
        local key = rg.pc .. ":" .. hex
        if key == last_key then return end -- collapse the adjacent-slot double-fire
        last_key = key
        rg.note = "[" .. hex .. "]"
        h._rec[#h._rec + 1] = rg
        if on_write then on_write(rg) end
    end
    local off = 0
    while off < len do
        bp.arm(addr + off, "Write", unit, string.format("%s_%X", opts.label or "find_writer", off),
            on_store)
        off = off + unit
    end
    return h
end

return M
