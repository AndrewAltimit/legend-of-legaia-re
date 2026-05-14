# @category Legaia
# @runtime Jython
#
# Dumps the overlay function that calls FUN_80043390 (the slot-4 / cluster-A
# display-list dispatcher) during the warp-into-world-map transition. The
# call site lives in overlay_world_map_top_ext.bin.
#
# Pin-down sequence:
#
#   - Drake slot-4 Read-bp probe captured caller-RA ~0x801F725C, so the
#     JAL itself is at 0x801F7254 (one delay slot earlier).
#   - 0x801F7088 is NOT the function entry: a first dump from that address
#     contained backward branches to 0x801F6E3C / 0x801F6E28 - both
#     before 0x801F7088. The body also lacked a prologue while the tail
#     restored s0..s8 + ra from sp+0x48..0x6c and freed a 0x70 frame.
#     The matching prologue (`addiu sp, sp, -0x70`) therefore sits earlier
#     and was simply unanalyzed by Ghidra.
#
# Strategy:
#
#   1. Force-disassemble a wide range covering the unanalyzed prologue
#      window (0x801F6000 .. 0x801F7644).
#   2. Walk backward from 0x801F7088 looking for `addiu sp, sp, -0x70`.
#      That is the function entry.
#   3. Walk forward from the entry to `jr ra` (we already know the
#      epilogue ends at 0x801F73DC).
#   4. CreateFunctionCmd with an explicit AddressSet body so
#      func.getBody() reports the true extent.
#
# Run against overlay_world_map_top_ext.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_world_map_top_ext.bin \
#       -noanalysis -postScript /scripts/dump_world_map_top_ext_caller.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.program.model.address import AddressSet
from ghidra.program.model.symbol import SourceType

# Wide-disasm window that brackets the suspected prologues.
DISASM_LO = "801f6000"
DISASM_HI = "801f8000"

# Functions to dump. Each entry is either:
#   ("walk_back", anchor_hex, frame_size, label) - find prologue by
#       walking backward from the anchor looking for `addiu sp,sp,-N`,
#       then forward for `jr ra`.
#   ("entry", entry_hex, label) - entry is known directly; find end by
#       forward-walking for `jr ra`.
TARGETS = [
    ("walk_back", "801f7088", 0x70, "wm_ext_dispatcher_caller"),
    ("entry",     "801f73e4",       "wm_ext_dispatcher_caller_helper"),
]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def force_disasm_range(lo_str, hi_str):
    lo = af.getAddress(lo_str)
    hi = af.getAddress(hi_str)
    cur = lo
    while cur.compareTo(hi) < 0:
        if listing.getInstructionAt(cur) is None:
            DisassembleCommand(cur, None, True).applyTo(prog, monitor)
        cur = cur.add(4)


def is_prologue(ins, frame_size):
    """Match `addiu sp, sp, -frame_size`."""
    if ins is None:
        return False
    if ins.getMnemonicString().lower() != "addiu":
        return False
    s = ins.toString().lower()
    return "sp,sp," in s.replace(" ", "") and ("-0x%x" % frame_size) in s.replace(" ", "")


def walk_back_for_prologue(anchor_addr, frame_size, max_back=0x800):
    cur = anchor_addr
    steps = 0
    while steps < max_back / 4:
        cur = cur.subtract(4)
        ins = listing.getInstructionAt(cur)
        if is_prologue(ins, frame_size):
            return cur
        steps += 1
    return None


def walk_forward_for_jr_ra(start_addr, max_instrs=4000):
    cur = start_addr
    count = 0
    while count < max_instrs:
        ins = listing.getInstructionAt(cur)
        if ins is None:
            return None
        mnem = ins.getMnemonicString().lower()
        if mnem == "jr" and "ra" in ins.toString():
            return cur.add(4).add(3)  # include delay slot
        cur = cur.add(4)
        count += 1
    return None


def delete_function_if_degenerate(addr):
    func = fm.getFunctionAt(addr)
    if func is None:
        return
    body = func.getBody()
    if body.getNumAddresses() < 64:
        print("[clean] removing degenerate function at %s (size=%d)" % (
            addr, body.getNumAddresses()))
        fm.removeFunction(addr)


def ensure_function(entry_addr, end_addr):
    body = AddressSet(entry_addr, end_addr)

    # Remove any function whose entry sits inside the target range
    # (left over from earlier runs that picked the wrong boundary).
    for func_in_range in list(fm.getFunctions(body, True)):
        ep = func_in_range.getEntryPoint()
        if body.contains(ep):
            print("[reset] removing overlapping function at %s (size=%d)" % (
                ep, func_in_range.getBody().getNumAddresses()))
            fm.removeFunction(ep)

    existing = fm.getFunctionAt(entry_addr)
    if existing is not None:
        actual = existing.getBody().getNumAddresses()
        target = body.getNumAddresses()
        if actual == target:
            return existing
        print("[reset] removing function at %s (size=%d, want=%d)" % (
            entry_addr, actual, target))
        fm.removeFunction(entry_addr)

    cmd = CreateFunctionCmd(None, entry_addr, body, SourceType.USER_DEFINED)
    if not cmd.applyTo(prog, monitor):
        print("[fail] CreateFunctionCmd failed at %s" % entry_addr)
        return None
    func = fm.getFunctionAt(entry_addr)
    if func is None:
        return None
    actual = func.getBody().getNumAddresses()
    target = body.getNumAddresses()
    if actual != target:
        print("[setBody] Ghidra picked %d bytes, forcing to %d bytes" % (
            actual, target))
        try:
            func.setBody(body)
        except Exception as exc:
            print("[warn] setBody failed: %s" % exc)
    return func


def dump(func, label, entry_str):
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(
        OUT_DIR,
        "overlay_world_map_top_ext_" + label + "_" + entry_str + ".txt",
    )
    fh = open(out_path, "w")
    try:
        fh.write("== %s %s (entry=%s, label=%s) [world_map_top_ext.bin] ==\n" % (
            func.getName(), entry_str, func.getEntryPoint(), label))
        fh.write("size=%d bytes, %d instructions\n\n" % (
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 180, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as exc:
            fh.write("(decompile exception: %s)\n" % exc)
    finally:
        fh.close()
    print("wrote %s (%d bytes, %d instr)" % (
        out_path, body.getNumAddresses(), len(instrs)))


force_disasm_range(DISASM_LO, DISASM_HI)


def process_walk_back(anchor_hex, frame_size, label):
    anchor = af.getAddress(anchor_hex)
    entry = walk_back_for_prologue(anchor, frame_size)
    if entry is None:
        print("[fatal] no `addiu sp, sp, -0x%x` prologue found before %s" % (
            frame_size, anchor))
        return
    print("[found] prologue at %s (frame -0x%x, label=%s)" % (
        entry, frame_size, label))
    end = walk_forward_for_jr_ra(entry)
    if end is None:
        print("[fatal] no `jr ra` after entry %s" % entry)
        return
    print("[found] epilogue ends at %s (size %d bytes)" % (
        end, end.subtract(entry) + 1))
    delete_function_if_degenerate(anchor)
    entry_str = "%08x" % entry.getOffset()
    func = ensure_function(entry, end)
    if func is not None:
        dump(func, label, entry_str)
    else:
        print("[fail] ensure_function failed at %s" % entry)


def process_entry(entry_hex, label):
    entry = af.getAddress(entry_hex)
    if listing.getInstructionAt(entry) is None:
        DisassembleCommand(entry, None, True).applyTo(prog, monitor)
    end = walk_forward_for_jr_ra(entry)
    if end is None:
        print("[fatal] no `jr ra` after entry %s (label=%s)" % (entry, label))
        return
    print("[found] entry=%s, epilogue ends at %s (size %d bytes, label=%s)" % (
        entry, end, end.subtract(entry) + 1, label))
    entry_str = "%08x" % entry.getOffset()
    func = ensure_function(entry, end)
    if func is not None:
        dump(func, label, entry_str)
    else:
        print("[fail] ensure_function failed at %s" % entry)


for t in TARGETS:
    if t[0] == "walk_back":
        process_walk_back(t[1], t[2], t[3])
    elif t[0] == "entry":
        process_entry(t[1], t[2])
    else:
        print("[skip] unknown target kind: %s" % t[0])

print("done")
