# @category Legaia
# @runtime Jython
# Dump additional functions from a named overlay program.
# Skips already-existing dumps. Sized to dump the next batch beyond top-20.

import os
import sys
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OUT_DIR = "/scripts/funcs"
prog = currentProgram
prog_name = prog.getName()

if prog_name.startswith("overlay_"):
    PREFIX = prog_name + "_"
else:
    PREFIX = "overlay_" + prog_name + "_"

fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)

funcs = []
for f in fm.getFunctions(True):
    body = f.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_calls = sum(1 for i in instrs if i.getMnemonicString().lower() == "jal")
    in_refs = sum(1 for _ in prog.getReferenceManager().getReferencesTo(f.getEntryPoint()))
    funcs.append((f.getEntryPoint().getOffset(), f, body.getNumAddresses(),
                  len(instrs), out_calls, in_refs))

funcs.sort(key=lambda x: -x[2])
print("Total functions: {}".format(len(funcs)))


def dump_func(func, addr_str):
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    if os.path.exists(out_path):
        return False
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    if len(instrs) < 4:
        return False
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
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
    print("dumped " + addr_str)
    return True


dumped = 0
LIMIT = 60  # next 60 by size
for entry, func, _, _, _, _ in funcs[:LIMIT]:
    addr_str = "%08x" % entry
    if dump_func(func, addr_str):
        dumped += 1
print("done. {} new dumps".format(dumped))
