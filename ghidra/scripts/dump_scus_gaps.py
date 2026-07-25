# @category Legaia
# @runtime Jython
#
# Dumper for the disc-denominated *code gap* worklist that
# `scripts/ci/disc-coverage.py` emits: the largest byte runs inside an
# image's text extent that no dumped function covers.
#
# A gap is bounded by dumped functions, so it holds one or more whole
# functions that were simply never dumped. This script takes address
# RANGES instead of entry points: for each range it walks the listing,
# collects every function entry that falls inside, and dumps it. Bytes
# with no function are reported so a force-disassemble pass
# (`force_disasm_dump.py`) can pick them up.
#
# Set LIST_ONLY to skip the dump step and only report what each range
# holds - useful when sizing a new range before committing DB writes.
#
# `in_program()` guards each range so the script is safe to run against
# any program in the project - ranges outside the loaded image are
# skipped silently, matching the `dump_pending_helpers.py` pattern.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/dump_scus_gaps.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.address import AddressSet
from ghidra.util.task import ConsoleTaskMonitor

# (start, end) inclusive-exclusive, as printed by disc-coverage.py's
# "largest un-dumped code runs" table for SCUS_942.54.
RANGES = [
    # First pass - the eight largest runs the report opened with.
    ("80021df4", "800243f0"),
    ("8004ad80", "8004ccd4"),
    ("80062f94", "800641ec"),
    ("8005ff20", "800608e0"),
    ("8001b964", "8001c204"),
    ("8003d764", "8003dda0"),
    ("80057358", "80057914"),
    ("8002149c", "80021934"),
    # Second pass - the runs the report promoted once the first pass closed.
    # All but one sit in the statically-linked PsyQ band above 0x80057000.
    ("80069a6c", "80069e98"),
    ("80059e10", "8005a1c0"),
    ("80059878", "80059bd4"),
    ("800648f0", "80064bd0"),
    ("800508dc", "80050bb8"),
    ("80057974", "80057c44"),
    ("8006a158", "8006a420"),
    ("8006d4f4", "8006d768"),
]

# Sub-runs the walk above reports as un-attributed: bytes inside a gap that
# Ghidra never disassembled, so no function exists to dump. These are
# force-disassembled and split into functions at each `jr ra` + delay-slot
# boundary. Only ranges already confirmed to hold MIPS (word-wise opcode check
# against `extracted/SCUS_942.54`) belong here - forcing data produces
# convincing garbage.
FORCE_RANGES = [
    ("8002149c", "8002174c"),
    ("80057550", "80057588"),
    ("800575c4", "80057600"),
    ("80057624", "80057860"),
    ("800607f4", "8006089c"),
]

LIST_ONLY = False

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def dump_function(func):
    addr_str = "%08x" % func.getEntryPoint().getOffset()
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    return out_path


def walk_range(start_str, end_str):
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if start is None or end is None:
        print("[skip] bad range {}..{}".format(start_str, end_str))
        return
    if not in_program(start):
        return

    print("=== range {}..{} ===".format(start_str, end_str))
    seen = []
    holes = []
    addr = start
    while addr.compareTo(end) < 0:
        func = fm.getFunctionAt(addr)
        if func is not None:
            entry = func.getEntryPoint()
            body = func.getBody()
            seen.append(func)
            print("  func {} {} size={} instrs={}".format(
                entry, func.getName(), body.getNumAddresses(),
                len(list(listing.getInstructions(body, True)))))
            nxt = body.getMaxAddress().add(1)
            if nxt.compareTo(addr) <= 0:
                nxt = addr.add(4)
            addr = nxt
            continue
        cu = listing.getCodeUnitAt(addr)
        if cu is None:
            holes.append(addr)
            addr = addr.add(4)
            continue
        containing = fm.getFunctionContaining(addr)
        if containing is None:
            holes.append(addr)
        addr = addr.add(cu.getLength())

    if holes:
        print("  {} un-attributed word(s), first={} last={}".format(
            len(holes), holes[0], holes[-1]))

    if not LIST_ONLY:
        for func in seen:
            print("  wrote {}".format(dump_function(func)))


def force_range(start_str, end_str):
    """Disassemble an un-analyzed run and create a function per `jr ra` unit."""
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if start is None or end is None or not in_program(start):
        return

    print("=== force {}..{} ===".format(start_str, end_str))
    span = AddressSet(start, end.subtract(1))

    cursor = start
    while cursor.compareTo(end) < 0:
        # Re-issue per sub-entry: flow-following from the range start only
        # reaches functions the start can branch to, and these runs are
        # sequences of independent leaves with no edge between them.
        if listing.getInstructionAt(cursor) is None:
            DisassembleCommand(cursor, span, True).applyTo(prog, monitor)
        entry = cursor
        probe = cursor
        jr_at = None
        while probe.compareTo(end) < 0:
            ins = listing.getInstructionAt(probe)
            if ins is None:
                break
            if ins.getMnemonicString().lower() == "jr" and "ra" in ins.toString():
                jr_at = probe
                break
            probe = probe.add(ins.getLength())
        if jr_at is None:
            print("  no `jr ra` from {} - stopping".format(entry))
            return
        stop = jr_at.add(8)

        func = fm.getFunctionAt(entry)
        if func is None:
            CreateFunctionCmd(entry).applyTo(prog, monitor)
            func = fm.getFunctionAt(entry)
        if func is None:
            print("  [skip] no function created at {}".format(entry))
        else:
            print("  func {} instrs<= {} -> {}".format(
                entry, (stop.subtract(entry)) / 4, dump_function(func)))
        cursor = stop


for rng in RANGES:
    walk_range(rng[0], rng[1])

for rng in FORCE_RANGES:
    force_range(rng[0], rng[1])

print("done [{}]".format(prog_name))
