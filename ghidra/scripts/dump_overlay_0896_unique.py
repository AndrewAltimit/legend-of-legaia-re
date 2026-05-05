# @category Legaia
# @runtime Jython
#
# Dump functions in 0896 that are NOT in 0897 -- i.e. those in the
# 0x801C5818 - 0x801CE818 prefix range. These are the menu/town
# subsystem code that's unique to 0896.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OUT_DIR = "/scripts/funcs"
PREFIX_LO = 0x801C5818
PREFIX_HI = 0x801CE818

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def in_prefix(addr):
    a = addr.getOffset()
    return PREFIX_LO <= a < PREFIX_HI


def dump(func):
    addr_str = "%08x" % func.getEntryPoint().getOffset()
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, "overlay_0896_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_0896 base=0x801C5818] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 300, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))


# Index: list every prefix-unique function with size + outgoing-call count.
unique = []
for f in fm.getFunctions(True):
    if in_prefix(f.getEntryPoint()):
        body = f.getBody()
        size = body.getNumAddresses()
        insns = list(listing.getInstructions(body, True))
        out_calls = sum(1 for i in insns if i.getMnemonicString() in ("jal", "JAL"))
        in_refs = sum(1 for _ in prog.getReferenceManager().getReferencesTo(f.getEntryPoint()))
        unique.append((f, size, len(insns), out_calls, in_refs))

unique.sort(key=lambda x: -x[1])

idx_path = os.path.join(OUT_DIR, "overlay_0896_unique_index.txt")
with open(idx_path, "w") as fh:
    fh.write("== 0896-only functions (RAM 0x801C5818 - 0x801CE818) ==\n")
    fh.write("These are NOT present in the 0897 overlay -- only loaded when 0896 is mapped.\n")
    fh.write("Likely menu / town-init / Shift-JIS-localized subsystem.\n\n")
    fh.write("addr        name                          size  insns  out  in\n")
    for func, size, ins, out, refs in unique:
        fh.write("{:08x}    {:<28}  {:>5}  {:>5}  {:>3}  {:>3}\n".format(
            func.getEntryPoint().getOffset(), func.getName(), size, ins, out, refs))
    fh.write("\nTotal: {} functions\n".format(len(unique)))
print("wrote " + idx_path)

# Dump the top 12 by size for further analysis.
for func, _, _, _, _ in unique[:12]:
    dump(func)
    print("dumped " + func.getName())

print("done")
