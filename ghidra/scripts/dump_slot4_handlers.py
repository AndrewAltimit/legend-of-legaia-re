# @category Legaia
# @runtime Jython
#
# Dump the per-kind slot-4 record handlers tail-called from FUN_80043390.
# Each handler is an unanalyzed code body in SCUS; force-create the function
# at the entry, then emit disassembly + decomp.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.app.cmd.function import CreateFunctionCmd

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

# (entry, label) pairs for the slot-4 handler table at 0x8007657C
HANDLERS = [
    ("8004409c", "k8_shared"),
    ("8004423c", "k9_shared"),
    ("80044434", "k10_shared"),
    ("800445b0", "k11_shared"),
    ("80043658", "k12_bank0"),
    ("80043768", "k13_bank0"),
    ("800438b8", "k16_bank0"),
    ("800439e4", "k17_bank0"),
    ("80043b58", "k14_bank0"),
    ("80043c6c", "k15_bank0"),
    ("80043dd4", "k18_bank0"),
    ("80043f10", "k19_bank0"),
    ("800448b0", "k12_banks12"),
    ("80044a3c", "k13_banks12"),
    ("80044c14", "k16_banks12"),
    ("80044dc8", "k17_banks12"),
    ("80044fdc", "k14_banks12"),
    ("80045194", "k15_banks12"),
    ("800453bc", "k18_bank1"),
    ("80045584", "k19_bank1"),
    ("800457c4", "k18_bank2"),
    ("80045988", "k19_bank2"),
]

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def ensure_function(addr):
    func = fm.getFunctionContaining(addr)
    if func is not None:
        return func
    func = fm.getFunctionAt(addr)
    if func is not None:
        return func
    cmd = CreateFunctionCmd(addr)
    if cmd.applyTo(prog, monitor):
        return fm.getFunctionAt(addr)
    return None


def dump(addr_str, label):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] %s not an address" % addr_str)
        return
    func = ensure_function(addr)
    if func is None:
        print("[fail] could not create function at %s" % addr_str)
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, "slot4_" + label + "_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== slot-4 handler %s (entry=%s, label=%s) ==\n" % (
            func.getName(), func.getEntryPoint(), label))
        fh.write("size=%d bytes, %d instructions\n\n" % (
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote %s (%d bytes)" % (out_path, body.getNumAddresses()))


for addr_str, label in HANDLERS:
    dump(addr_str, label)

print("done")
