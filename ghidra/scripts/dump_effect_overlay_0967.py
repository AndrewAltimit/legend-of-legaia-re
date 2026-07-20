# @category Legaia
# @runtime Jython
#
# Dump the battle-effect overlay PROT 0967, which the S5 trace + a live-RAM-vs-
# static-blob comparison pinned as CO-RESIDENT at base 0x801F69D8 during battle
# (the shared summon/move-FX buffer *DAT_80010390). Its 0x801F6xxx..0x801F7xxx
# code overlaps overlay 0898's *rodata* tail (0898's own bytes there are menu
# strings "@Equip/@Status/..."), which is why dumping it from the 0898 image gave
# garbage. This dumps it from 0967's own bytes at the correct base.
#
#   analyzeHeadless /projects legaia -import /data/PROT/0967_xxx_dat.BIN \
#     -loader BinaryLoader -loader-baseAddr 0x801F69D8 \
#     -processor MIPS:LE:32:default -overwrite \
#     -postScript /scripts/dump_effect_overlay_0967.py
#
# Dumps the S5 render-tail hits that land in 0967 (VA >= 0x801F69D8).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

# 0967 render-tail hits (VA >= 0x801F69D8), with S5 hit counts.
HITS = [0x801f71e0, 0x801f7624, 0x801f6c70, 0x801f6d48]

# Sparring-tutorial prompt machine (docs/subsystems/battle.md). The tick
# FUN_801F6B70 had no dump at all, so the tutorial audit had to disassemble it
# out of extracted/PROT/0967_xxx_dat.BIN by hand; these targets exist so the
# next reader gets a dump instead. The nine handler addresses are the LIVE
# slots of the 91-entry jump table based at 0x801F69D8 (= the overlay load
# base, so the table is at file offset 0). They are jr-table destinations, so
# several may resolve into the containing function -- the `done` map below
# dedupes by entry point, which is the point: what resolves to its own entry
# is a real function, what folds into a neighbour is a label.
HITS += [
    0x801f6b70,  # tick: guards + ctx[+0x06] jump-table dispatch
    0x801f6c00,  # flow state 30  - turn start / per-lesson intro
    0x801f6cb8,  # flow state 40  - category prompt
    0x801f6cac,  # flow state 50  - run selected (always rewinds)
    0x801f6dcc,  # flow state 60  - item window
    0x801f6e4c,  # flow state 80  - arts command entry
    0x801f6ee4,  # flow state 90  - target select / hyper-arts drill validate
    0x801f7060,  # flow state 100 - target confirm
    0x801f7088,  # flow state 110 - committed-category validator
    0x801f6d30,  # flow state 120 - auto/command attack-mode prompt
    0x801f718c,  # shared no-op tail (the other 82 table slots)
    0x801f7380,  # completion tail: lesson>=5 clamp + ctx[6]=0xC8 / ctx[7]=0xFF
    0x801f7628,  # "you're learning about X now" rewind
    0x801f747c,  # box emitter (style index 0..9 -> table at 0x801F6B48)
]

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
    stem = "overlay_effect_0967_%08x" % entry.getOffset()
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, stem + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== %s %08x (entry=%08x) [PROT 0967 @ base 0x801F69D8] ==\n"
                 % (func.getName(), entry.getOffset(), entry.getOffset()))
        fh.write("size=%d bytes, %d instructions\n\n"
                 % (body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
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
        print("  0x%08x NOT IN MEMORY" % a)
        continue
    f = ensure(addr)
    if f is None:
        print("  0x%08x -> unresolved" % a)
        continue
    e = f.getEntryPoint().getOffset()
    if e not in done:
        done[e] = dump(f)
    print("  0x%08x -> FUN_%08x +0x%x" % (a, e, a - e))
print("distinct 0967 functions dumped: %d" % len(done))
for e in sorted(done):
    print("  %s.txt" % done[e])
