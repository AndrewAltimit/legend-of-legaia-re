# @category Legaia
# @runtime Jython
#
# Covers the last coverage-tracker miss in the shop/save overlay:
#   801e4f40 - cited as PTR_FUN_801e4f40 in overlay_shop_save_801dc6b4.txt.
#
# 0x801E4F40 is the base of a function-pointer dispatch table used by the
# save-screen state machine (FUN_801DC6B4).  DAT_801e46a4 indexes into it:
#   (*(code *)(&PTR_FUN_801e4f40)[DAT_801e46a4])(_DAT_8007b874)
#
# Known sub-state indices from the decompiled SM:
#   0x01 = init / fade-in
#   0x02 = (condition branch)
#   0x04 = entry-context 0x0D path
#   0x19 = entry-context 0x01 path
#   0x1A = normal save (entry-context NULL/0)
#   0x1E = save write path (FUN_801DBC5C; story_flags + inventory write)
#   0x20 = entry-context 0x07 path
#
# This script:
#   1. Reads the 4-byte function pointers from the table (indices 0..0x20).
#   2. Creates Ghidra Function objects at each slot that isn't already one.
#   3. Writes ghidra/scripts/funcs/overlay_shop_save_801e4f40.txt documenting
#      the table (satisfies the coverage tracker -- filename ends in _801e4f40.txt).
#   4. Dumps each discovered handler as overlay_shop_save_<addr>.txt.
#
# Run against overlay_shop_save.bin:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_shop_save.bin -noanalysis \
#       -postScript /scripts/dump_save_screen_table.py

import os
import struct

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM = "overlay_save_ui_select.bin"

TABLE_BASE = 0x801e4f40
TABLE_LEN = 0x21  # highest known index is 0x20; dump 0..0x20 inclusive


def read_u32_le(memory, addr_obj):
    buf = [0] * 4
    memory.getBytes(addr_obj, buf)
    b = [x & 0xFF for x in buf]
    return b[0] | (b[1] << 8) | (b[2] << 16) | (b[3] << 24)


def make_or_get_func(program, addr_obj, monitor):
    fm = program.getFunctionManager()
    func = fm.getFunctionAt(addr_obj)
    if func is None:
        cmd = CreateFunctionCmd(addr_obj)
        cmd.applyTo(program, monitor)
        func = fm.getFunctionAt(addr_obj)
    return func


def dump_func(program, func, hex_addr, label, decomp, monitor, out_dir):
    listing = program.getListing()
    instrs = listing.getInstructions(func.getBody(), True)
    lines = []
    lines.append("== %s 0x%s (entry=0x%s) [%s] ==" % (
        func.getName(), hex_addr, hex_addr, program.getName()))
    size = func.getBody().getNumAddresses()
    n_instr = 0
    instr_lines = []
    for instr in instrs:
        n_instr += 1
        instr_lines.append("%s  %s" % (instr.getAddress(), instr.toString()))
    lines.append("size=%d bytes, %d instructions" % (size, n_instr))
    lines.append("")
    lines.append("--- DISASSEMBLY ---")
    lines.extend(instr_lines)
    lines.append("")
    lines.append("--- DECOMPILED ---")
    result = decomp.decompileFunction(func, 60, monitor)
    if result is not None and result.getDecompiledFunction() is not None:
        lines.append(result.getDecompiledFunction().getC())
    else:
        lines.append("(decompile failed)")
    fname = label + "_" + hex_addr + ".txt"
    out_path = os.path.join(out_dir, fname)
    with open(out_path, "w") as f:
        f.write("\n".join(lines))
    print("wrote %s" % fname)


program = state.getCurrentProgram()
if program.getName() != PROGRAM:
    print("[skip] program is %s, expected %s" % (program.getName(), PROGRAM))
else:
    mem = program.getMemory()
    af = program.getAddressFactory()
    monitor = ConsoleTaskMonitor()
    decomp = DecompInterface()
    options = DecompileOptions()
    decomp.setOptions(options)
    decomp.openProgram(program)
    out_dir = "/scripts/funcs"
    if not os.path.isdir(out_dir):
        os.makedirs(out_dir)

    label = "overlay_save_ui"

    # --- Step 1: read the pointer table and build the slot map ---
    table_addr = af.getAddress("0x%08x" % TABLE_BASE)
    slots = []
    for i in range(TABLE_LEN):
        slot_addr = table_addr.add(i * 4)
        try:
            fn_val = read_u32_le(mem, slot_addr)
            fn_hex = "%08x" % fn_val
            slots.append((i, fn_hex, fn_val))
        except Exception as e:
            slots.append((i, None, None))
            print("[warn] slot %02x: read error: %s" % (i, str(e)))

    # --- Step 2: write the table documentation file (satisfies coverage) ---
    table_lines = [
        "== save-screen handler table (PTR_FUN_801e4f40) [%s] ==" % program.getName(),
        "",
        "Base: 0x%08x  Entries: 0x%02x" % (TABLE_BASE, TABLE_LEN),
        "Sub-state index -> handler address",
        "",
    ]
    for (i, fn_hex, fn_val) in slots:
        if fn_hex is None:
            table_lines.append("  [0x%02x] = (unreadable)" % i)
        else:
            table_lines.append("  [0x%02x] = 0x%s" % (i, fn_hex))
    table_path = os.path.join(out_dir, label + "_801e4f40.txt")
    with open(table_path, "w") as f:
        f.write("\n".join(table_lines))
    print("wrote %s" % os.path.basename(table_path))

    # --- Step 3: create functions and dump each handler ---
    seen = set()
    for (i, fn_hex, fn_val) in slots:
        if fn_hex is None:
            continue
        if not (0x801c0000 <= fn_val <= 0x8020ffff):
            print("[skip] slot 0x%02x: 0x%s out of overlay range" % (i, fn_hex))
            continue
        if fn_hex in seen:
            continue
        seen.add(fn_hex)
        fn_addr = af.getAddress("0x" + fn_hex)
        func = make_or_get_func(program, fn_addr, monitor)
        if func is None:
            print("[skip] slot 0x%02x: could not create function at 0x%s" % (i, fn_hex))
            continue
        dump_func(program, func, fn_hex, label, decomp, monitor, out_dir)

    print("done. %d unique handlers dumped." % len(seen))
