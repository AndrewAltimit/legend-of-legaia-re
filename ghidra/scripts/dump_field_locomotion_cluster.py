# @category Legaia
# @runtime Jython
#
# Re-decompile the town/field free-movement locomotion cluster in the
# 0897 field overlay. The small "functions" at 0x801db81c / 0x801dbec4
# decompile noisily because Ghidra split a larger field-update routine
# into mid-block fake entries (args arrive in saved registers s3/s4/s7/s8,
# which is impossible for a real o32 entry). This script:
#
#   1. For each target address, reports the containing-function bounds
#      per Ghidra's function manager and decompiles that function.
#   2. Dumps the RAW disassembly of the whole 0x801db800..0x801dc200
#      window so the real instruction flow can be read regardless of the
#      (wrong) function boundaries.
#
# Run against the field overlay:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897.bin.0 -noanalysis \
#       -postScript /scripts/dump_field_locomotion_cluster.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# Containing-function probes + the spliced movement block the noisy
# decompiles kept referencing.
TARGETS = ["801db81c", "801dbec4", "801dbf9c", "801d4ba8", "801d5718"]

# Raw-disassembly windows (inclusive start, exclusive end).
RAW_WINDOWS = [
    ("801db800", "801dc200"),
    ("801d4b00", "801d5900"),
]

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


def in_program(addr):
    return mem.getBlock(addr) is not None


def label():
    return prog_name.replace(".bin", "").replace(".", "_")


def decompile_containing(addr_str, fh):
    addr = af.getAddress(addr_str)
    if addr is None or not in_program(addr):
        fh.write("[skip] {} not in {}\n\n".format(addr_str, prog_name))
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        fh.write("[no containing func] {}\n\n".format(addr_str))
        return
    body = func.getBody()
    fh.write("== probe {} -> {} (entry={}, min={}, max={}, {} bytes) ==\n".format(
        addr_str, func.getName(), func.getEntryPoint(),
        body.getMinAddress(), body.getMaxAddress(), body.getNumAddresses()))
    try:
        res = decomp.decompileFunction(func, 60, monitor)
        if res.decompileCompleted():
            fh.write(res.getDecompiledFunction().getC())
        else:
            fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
    except Exception as e:
        fh.write("(decompile exception: {})\n".format(e))
    fh.write("\n")


def raw_window(lo_str, hi_str, fh):
    lo = af.getAddress(lo_str)
    hi = af.getAddress(hi_str)
    if lo is None or not in_program(lo):
        fh.write("[skip raw] {} not in {}\n\n".format(lo_str, prog_name))
        return
    fh.write("=== RAW {}..{} ===\n".format(lo_str, hi_str))
    addr = lo
    while addr.compareTo(hi) < 0:
        ins = listing.getInstructionAt(addr)
        if ins is None:
            # Show the data byte and step one.
            try:
                b = mem.getByte(addr) & 0xff
                fh.write("{}  .byte 0x{:02x}\n".format(addr, b))
            except Exception:
                fh.write("{}  (unreadable)\n".format(addr))
            addr = addr.add(1)
            continue
        # Mark function entries inline.
        f = fm.getFunctionAt(addr)
        tag = "  <== FUNC {}".format(f.getName()) if f is not None else ""
        fh.write("{}  {}{}\n".format(addr, ins.toString(), tag))
        addr = addr.add(ins.getLength())
    fh.write("\n")


if not in_program(af.getAddress("801db81c")):
    print("[skip] cluster not in {}".format(prog_name))
else:
    out_path = os.path.join(OUT_DIR, label() + "_locomotion_cluster.txt")
    fh = open(out_path, "w")
    try:
        fh.write("# Field locomotion cluster re-decompile [{}]\n\n".format(prog_name))
        for t in TARGETS:
            decompile_containing(t, fh)
        for lo, hi in RAW_WINDOWS:
            raw_window(lo, hi, fh)
    finally:
        fh.close()
    print("wrote {}".format(out_path))

print("done [{}]".format(prog_name))
