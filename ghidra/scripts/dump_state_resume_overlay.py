# @category Legaia
# @runtime Jython
#
# Dump the 0897 field-overlay STATE_RESUME effect-actor handler - the routine
# the field-VM op 0x49 (STATE_RESUME) spawns via func_0x80020DE0(0x8007065C,
# _DAT_8007C34C) and which, on completion, writes _DAT_8007B450 = 1 (the Done
# signal the op's Armed park waits on). The town01 opening deadlocks because this
# completion never fires under headless automation (P2[3] +0x02C6, 49 03).
#
# Handler entry = FUN_801F159C (the only writer of _DAT_8007B450 = 1; shared VA
# across overlays). Its worker callee = FUN_801F1278. We dump both + a raw
# window so the completion condition (what gates _DAT_8007B450 = 1) is legible
# regardless of Ghidra's function boundaries.
#
# Run against the field overlay:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897.bin.0 -noanalysis \
#       -postScript /scripts/dump_state_resume_overlay.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# STATE_RESUME effect-actor handler + its worker callee.
TARGETS = ["801f159c", "801f1278"]

# Raw-disassembly windows (inclusive start, exclusive end) covering the handler
# + callee so the completion path reads cleanly.
RAW_WINDOWS = [
    ("801f1278", "801f16c0"),
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


def addr(s):
    return af.getAddress(s)


def out_path_for(name):
    label = prog_name.replace(".bin", "").replace(".0", "")
    return os.path.join(OUT_DIR, "overlay_%s_%s.txt" % (label, name))


for tgt in TARGETS:
    a = addr(tgt)
    if not in_program(a):
        print("skip %s: not in %s" % (tgt, prog_name))
        continue
    fn = fm.getFunctionContaining(a)
    lines = []
    lines.append("== STATE_RESUME handler dump %s [%s] ==" % (tgt, prog_name))
    if fn is not None:
        body = fn.getBody()
        lines.append("containing fn: %s entry=%s min=%s max=%s" % (
            fn.getName(), fn.getEntryPoint(), body.getMinAddress(), body.getMaxAddress()))
        res = decomp.decompileFunction(fn, 60, monitor)
        if res is not None and res.decompileCompleted():
            lines.append("--- DECOMPILED ---")
            lines.append(res.getDecompiledFunction().getC())
        else:
            lines.append("(decompile failed: %s)" % (
                res.getErrorMessage() if res is not None else "no result"))
    else:
        lines.append("(no containing function at %s)" % tgt)
    with open(out_path_for(tgt), "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("wrote %s" % out_path_for(tgt))

# Raw disassembly windows.
for (lo, hi) in RAW_WINDOWS:
    a, b = addr(lo), addr(hi)
    lines = ["== RAW DISASM %s..%s [%s] ==" % (lo, hi, prog_name)]
    cur = a
    while cur.compareTo(b) < 0:
        if not in_program(cur):
            break
        ins = listing.getInstructionAt(cur)
        if ins is None:
            cur = cur.add(4)
            continue
        lines.append("%s  %s" % (cur, ins))
        cur = cur.add(ins.getLength())
    with open(out_path_for("raw_%s_%s" % (lo, hi)), "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("wrote %s" % out_path_for("raw_%s_%s" % (lo, hi)))

print("done")
