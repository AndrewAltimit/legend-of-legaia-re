# @category Legaia
# @runtime Jython
#
# Dumps the battle-overlay backdrop draw func_0x801d02c0 (called by the static
# FUN_80026f50 for game mode 0x15) plus the functions it calls one level deep,
# so the dome-instancing / back-fill submission can be traced.
#
# Run against the battle_action named program:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_battle_action.bin -noanalysis \
#       -postScript /scripts/dump_battle_backdrop_draw.py -scriptPath /scripts

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SEED = "801d02c0"
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
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def label_for(addr_str):
    if prog_name.startswith("SCUS"):
        return addr_str
    return prog_name.replace(".bin", "").replace(".", "_") + "_" + addr_str


def get_or_make_func(addr):
    f = fm.getFunctionContaining(addr)
    if f is not None and f.getEntryPoint().getOffset() == addr.getOffset() \
            and f.getBody().getNumAddresses() > 4:
        return f
    # force-disassemble at the address (overlay funcs are often left as a 1-byte
    # stub), drop any stub function, then recreate so the body follows the flow.
    try:
        from ghidra.app.cmd.disassemble import DisassembleCommand
        DisassembleCommand(addr, None, True).applyTo(prog, monitor)
    except Exception as e:
        print("  disasm failed at %s: %s" % (addr, e))
    if f is not None and f.getEntryPoint().getOffset() == addr.getOffset():
        try:
            fm.removeFunction(addr)
        except Exception as e:
            print("  removeFunction failed at %s: %s" % (addr, e))
    try:
        from ghidra.app.cmd.function import CreateFunctionCmd
        CreateFunctionCmd(addr).applyTo(prog, monitor)
    except Exception as e:
        print("  create func failed at %s: %s" % (addr, e))
    return fm.getFunctionContaining(addr)


def dump_func(addr_str, also_callees):
    addr = af.getAddress("0x" + addr_str)
    func = get_or_make_func(addr)
    if func is None:
        print("NO FUNCTION at %s" % addr_str)
        return []
    entry = func.getEntryPoint()
    body = func.getBody()
    size = body.getNumAddresses()
    out = os.path.join(OUT_DIR, label_for("%08x" % entry.getOffset()) + ".txt")

    lines = []
    lines.append("== %s %08x (entry=%08x) [%s] ==" % (
        func.getName(), entry.getOffset(), entry.getOffset(), prog_name))
    lines.append("size=%d bytes" % size)
    lines.append("")
    lines.append("--- DISASSEMBLY ---")
    inst = listing.getInstructions(body, True)
    callees = []
    while inst.hasNext():
        i = inst.next()
        lines.append("%08x  %s" % (i.getAddress().getOffset(), i.toString()))
        # collect call targets
        mn = i.getMnemonicString()
        if mn in ("jal", "jalr"):
            for ref in i.getReferencesFrom():
                t = ref.getToAddress()
                if t is not None and t.isMemoryAddress():
                    callees.append("%08x" % t.getOffset())
    lines.append("")
    lines.append("--- DECOMPILED ---")
    res = decomp.decompileFunction(func, 60, monitor)
    if res is not None and res.getDecompiledFunction() is not None:
        lines.append(res.getDecompiledFunction().getC())
    else:
        lines.append("(decompile failed)")

    f = open(out, "w")
    f.write("\n".join(lines))
    f.close()
    print("wrote %s (size=%d, %d call sites)" % (out, size, len(callees)))

    uniq = []
    for c in callees:
        if c not in uniq:
            uniq.append(c)
    return uniq


callees = dump_func(SEED, True)
print("SEED callees: %s" % callees)
# one level deep: dump callees that live in the overlay region (0x801c0000+)
for c in callees:
    try:
        off = int(c, 16)
    except ValueError:
        continue
    if 0x801c0000 <= off < 0x80200000:
        dump_func(c, False)
print("DONE")
