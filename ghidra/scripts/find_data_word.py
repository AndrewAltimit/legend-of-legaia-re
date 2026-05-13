# @category Legaia
# @runtime Jython
#
# Scan all initialized memory in the current program for u32 literals
# matching a target list. Useful for finding function pointers stored
# in dispatch tables (LE u32 = function entry address).
#
# Reports each match with surrounding context (8 dwords before/after)
# so the table structure becomes visible.

from array import array

prog = currentProgram
mem = prog.getMemory()
af = prog.getAddressFactory()
listing = prog.getListing()
fm = prog.getFunctionManager()

PROGRAM_NAME = prog.getName()
print("=== find_data_word against %s ===" % PROGRAM_NAME)

TARGETS = {
    0x801D7EA0: "FUN_801D7EA0 (terrain emitter)",
    0x801D1344: "FUN_801D1344 (terrain controller)",
    0x801D8258: "FUN_801D8258 (gate setter)",
    0x801C9688: "FUN_801C9688 (0897 field gate reader)",
}

target_set = set(TARGETS.keys())

# Walk every initialized block, read u32s aligned to 4 bytes.
hits = []
for block in mem.getBlocks():
    if not block.isInitialized():
        continue
    if block.isExecute():
        # Executable -- still scan for inline function-pointer constants
        # but mark them in context.
        pass
    start = block.getStart().getOffset()
    end = block.getEnd().getOffset()
    name = block.getName()
    # Read all bytes in chunks
    addr = block.getStart()
    length = block.getSize()
    try:
        buf = bytearray(length)
        # Ghidra's mem.getBytes() takes a (start_addr, dest_array)
        mem.getBytes(block.getStart(), buf)
    except Exception as e:
        print("[skip] block %s @ 0x%X: %s" % (name, start, e))
        continue

    # Scan aligned u32
    for off in range(0, length - 3, 4):
        # PSX is little-endian
        v = (buf[off] |
             (buf[off + 1] << 8) |
             (buf[off + 2] << 16) |
             (buf[off + 3] << 24)) & 0xFFFFFFFF
        if v in target_set:
            abs_addr = start + off
            hits.append((abs_addr, v, name, block.isExecute(), buf, off, start))

print("\n--- %d hits ---" % len(hits))
for (abs_addr, v, blk_name, is_exec, buf, off, blk_start) in hits:
    label = TARGETS[v]
    exec_tag = "[exec]" if is_exec else "[data]"
    func = fm.getFunctionContaining(af.getAddress("%x" % abs_addr))
    fn_name = func.getName() if func else "<no function>"
    print("\n0x%08X  %s  %s  in %s  (%s)" % (
        abs_addr, exec_tag, label, blk_name, fn_name))

    # Print surrounding context: 8 dwords before, 8 dwords after
    lo = max(off - 32, 0)
    hi = min(off + 36, len(buf))
    for ctx_off in range(lo, hi, 4):
        if ctx_off + 3 >= len(buf):
            break
        ctx_v = (buf[ctx_off] |
                 (buf[ctx_off + 1] << 8) |
                 (buf[ctx_off + 2] << 16) |
                 (buf[ctx_off + 3] << 24)) & 0xFFFFFFFF
        marker = "  >>>" if ctx_off == off else "     "
        ctx_label = TARGETS.get(ctx_v, "")
        if ctx_label:
            ctx_label = "  // " + ctx_label
        print("%s 0x%08X: 0x%08X%s" % (
            marker, blk_start + ctx_off, ctx_v, ctx_label))

print("\n=== end ===")
