# @category Legaia
# @runtime Jython
#
# The S5-battle trace found hot exec hits at addresses that fall INSIDE the
# battle overlay (0898, program overlay_battle_action.bin spans
# 0x801C0000..0x801EFFFF) but that Ghidra had not turned into functions - so the
# containment attribution left them unresolved. These are undumped 0898 code
# reached from the battle draw loop (ra in 0x80048130..48). Disassemble + create
# a function at each and dump it, so the render-tail hits attribute like the rest.
#
# (Hits in the 0x801F range are OUT of this program's memory - a separate
# co-resident battle-render overlay - and are NOT handled here.)
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_battle_action.bin \
#       -postScript /scripts/dump_battle_rendertail.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

# In-0898 render-tail hit addresses (union.csv, s5_tetsu_battle), each returned
# fn=None by getFunctionContaining but inmem=True.
TARGETS = ["801e0080", "801e0598", "801e0418", "801e02a4"]

OUT_DIR = "/scripts/funcs"
prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
monitor = ConsoleTaskMonitor()
decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def ensure_function(addr):
    f = fm.getFunctionContaining(addr)
    if f is not None:
        return f
    # No code unit? disassemble first.
    if listing.getInstructionAt(addr) is None:
        DisassembleCommand(addr, None, True).applyTo(prog, monitor)
    CreateFunctionCmd(addr).applyTo(prog, monitor)
    return fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)


def dump(addr_str):
    addr = af.getAddress("0x" + addr_str)
    func = ensure_function(addr)
    if func is None:
        print("[skip] could not create function at {}".format(addr_str))
        return
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
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    print("wrote {} (hit addr 0x{} -> entry 0x{})".format(out_path, addr_str, stem))


for t in TARGETS:
    dump(t)
