-- probe/snapshot.lua  -- snapshot + call-context dump helpers.
--
-- Two related concerns:
--   * write_snapshot(path, label, descs)  - tab-separated hits summary
--     keyed by probe address + name. Rewritten on every snapshot so
--     the latest state always survives a crash or window-close.
--   * capture_call_context(label)         - "what's the CPU doing right
--     now" register/code/stack dump for inclusion in a probe's hits
--     log. Reads RAM via probe.mem.
--   * append_call_context(path, text)     - append a captured block to
--     a flat text file (used by probes that want one record per first
--     hit, in addition to a CSV stream).

local mem = require("probe.mem")

local M = {}

local MIPS_GPR_NAMES = {
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0",   "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0",   "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8",   "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

-- Emit a tab-separated hits summary keyed by probe address + name.
-- `descs` is a list of
--   { addr = uint, name = string, hits_ref = { n = int } }
-- (hits_ref is wrapped in a one-key table so the breakpoint
-- callback's closure can mutate it without needing array indexing.)
function M.write(path, label, descs, extra_lines)
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

-- Capture a "what's the CPU doing right now" record from inside a
-- breakpoint callback. The caller passes a `label` and we serialise:
--   * all 32 GPRs by MIPS name
--   * the 32 instruction words straddling PC (PC-0x20 .. PC+0x60)
--   * the 32 stack words at sp (sp+0 .. sp+0x80)
--
-- The MIPS calling convention saves ra into the prologue's sp-relative
-- slot for any non-leaf function, so the stack dump captures the
-- visible ra-chain without needing real DWARF unwind info. The caller
-- is expected to walk the on-disc disassembly post-hoc to locate the
-- exact saved-ra slot for each frame.
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
    local code   = mem.read_bytes(pc_lo, 0x80)
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

    -- Stack words at sp (32 u32s = 128 bytes).
    local stack = mem.read_bytes(sp, 0x80)
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

return M
