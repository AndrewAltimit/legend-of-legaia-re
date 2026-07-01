# @category Legaia
# @runtime Jython
#
# Dump the battle overlay 0898 RENDER TAIL (the 0x801F band the trace found but
# the older overlay_battle_action.bin import - windowed 0x801C0000..0x801EFFFF -
# stopped short of). Run against a FULL-LENGTH import of the 0898 blob at its
# real base 0x801CE818 (span 0x801CE818..0x801F8018), so the 0x801F addresses map:
#
#   analyzeHeadless /projects legaia \
#     -import /data/overlays/overlay_battle_action_0898.bin \
#     -loader BinaryLoader -loader-baseAddr 0x801CE818 \
#     -processor MIPS:LE:32:default -overwrite \
#     -postScript /scripts/dump_battle_rendertail_0x801f.py
#
# Resolves each S5 0x801F render-tail hit to its enclosing function (creating one
# if analysis missed it) and dumps it as overlay_battle_action_<addr>.txt.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

# 0x801F render-tail hits (union.csv, s5_tetsu_battle), with hit counts.
HITS = [0x801f71e0, 0x801f0740, 0x801f7624, 0x801f02d0, 0x801f0adc,
        0x801f1950, 0x801f1890, 0x801f6d48, 0x801f6c70, 0x801f07ac,
        0x801f04b0, 0x801f0450, 0x801f03c0, 0x801f03b0]

OUT_DIR = "/scripts/funcs"
prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()
decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def A(a):
    return af.getDefaultAddressSpace().getAddress(a)


def ensure(addr):
    f = fm.getFunctionContaining(addr)
    if f is not None:
        return f
    if listing.getInstructionAt(addr) is None:
        DisassembleCommand(addr, None, True).applyTo(prog, monitor)
    CreateFunctionCmd(addr).applyTo(prog, monitor)
    return fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)


def dump(func):
    entry = func.getEntryPoint()
    stem = "%08x" % entry.getOffset()
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, "overlay_battle_action_%s.txt" % stem)
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(func.getName(), stem, stem))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            fh.write(res.getDecompiledFunction().getC()
                     if res.decompileCompleted()
                     else "(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    return stem


print("PROGRAM %s  span 0x%08x..0x%08x" % (
    prog.getName(), mem.getMinAddress().getOffset(), mem.getMaxAddress().getOffset()))
done = {}
for a in HITS:
    addr = A(a)
    if not mem.contains(addr):
        print("  0x%08x NOT IN MEMORY (import too short)" % a)
        continue
    f = ensure(addr)
    if f is None:
        print("  0x%08x -> could not resolve a function" % a)
        continue
    e = f.getEntryPoint().getOffset()
    if e not in done:
        done[e] = dump(f)
    print("  0x%08x -> FUN_%08x +0x%x" % (a, e, a - e))
print("distinct render-tail functions dumped: %d" % len(done))
for e in sorted(done):
    print("  overlay_battle_action_%08x.txt" % e)
