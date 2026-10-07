# @category Legaia
# @runtime Jython
#
# Companion to find_addprim_emitters.py. For each candidate function:
#   - Dumps full disassembly + decompiled C to /scripts/funcs/<addr>.txt
#   - Lists direct callers (jal sites + address-as-data references)
# Run against SCUS_942.54 since the high-hit POLY_FT4 emitter
# (FUN_8002C69C, 10 sites) lives there.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# Candidates surfaced by find_addprim_emitters.py against SCUS_942.54.
CANDIDATES = [
    "8002c69c",  # 10 sites, all cmd=0x2C at 0x4(a1) -- prime candidate
    "8006a420",  # 2 sites, cmd=0x2F
    "8001d424",  # 1 site,  cmd=0x2C at 0x84(v1) (unusual offset)
    "8001c394",  # 1 site,  cmd=0x2E
    "8002b994",  # 1 site,  cmd=0x2E
]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def dump_func(addr_str):
    addr = af.getAddress(addr_str)
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for %s" % addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== %s %s (entry=%s) ==\n" % (
            func.getName(), addr_str, func.getEntryPoint()))
        fh.write("size=%d bytes, %d instructions\n\n" % (
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 90, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote %s" % out_path)


def list_callers(addr_str):
    addr = af.getAddress(addr_str)
    refs = list(ref_mgr.getReferencesTo(addr))
    target = fm.getFunctionAt(addr)
    name = target.getName() if target else "?"
    print("\n=== callers of %s (%s) -- %d refs ===" % (addr_str, name, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        from_func = fm.getFunctionContaining(from_a)
        from_fn_name = from_func.getName() if from_func else "?"
        from_fn_entry = str(from_func.getEntryPoint()) if from_func else "?"
        ins = listing.getInstructionAt(from_a)
        print("  from %s in %s @ %s: %s" % (
            from_a, from_fn_name, from_fn_entry,
            ins.toString() if ins else "?"))


for c in CANDIDATES:
    dump_func(c)

for c in CANDIDATES:
    list_callers(c)

print("done")
