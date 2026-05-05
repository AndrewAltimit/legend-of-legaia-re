# @category Legaia
# @runtime Jython
#
# Round 23: cleanup of all helpers cited 2+ times that don't yet have a
# dump. After rounds 18-22 burned through most of the 1-ref tail, the
# remaining multi-ref helpers are the real high-value targets.
#
# Mixed cluster: 1 SCUS straggler (80043048) and 11 overlay helpers in
# the 0897 town overlay (one in 0896 prefix range at 801CFCE4 - runs
# fine against the 0897 program since 0896 = 0897 + 36KB prefix).
#
# Run twice: once with -process SCUS_942.54 for the SCUS one, once with
# -process overlay_0897_xxx_dat.bin for the rest. The script auto-skips
# addresses outside the active program's range.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    ("scus", "80043048"),
    ("overlay", "801e9b3c"),
    ("overlay", "801cfce4"),
    ("overlay", "801ef91c"),
    ("overlay", "801e6f30"),
    ("overlay", "801e91e8"),
    ("overlay", "801dd864"),
    ("overlay", "801ddb30"),
    ("overlay", "801ec370"),
    ("overlay", "801eccac"),
    ("overlay", "801f3894"),
    ("overlay", "80202bcc"),
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
monitor = ConsoleTaskMonitor()

# Choose dump-name prefix and addresses based on which program is active.
is_scus = "SCUS" in prog_name.upper()
PREFIX = "" if is_scus else "overlay_0897_"


def ensure_function(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is not None:
        return func
    try:
        from ghidra.app.cmd.function import CreateFunctionCmd
        cmd = CreateFunctionCmd(addr)
        if cmd.applyTo(prog, monitor):
            func = fm.getFunctionAt(addr)
            if func is not None:
                print("[create] {}: {}".format(addr_str, func.getName()))
                return func
        print("[create-fail] {}: {}".format(addr_str, cmd.getStatusMsg()))
    except Exception as e:
        print("[create-exc] {}: {}".format(addr_str, e))
    return None


decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def dump(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip-no-func] {}".format(addr_str))
        return

    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    if os.path.exists(out_path):
        print("[skip-exists] {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

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
            res = decomp.decompileFunction(func, 300, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    print("wrote {}".format(out_path))


for kind, addr in TARGETS:
    if is_scus and kind != "scus":
        continue
    if not is_scus and kind != "overlay":
        continue
    dump(addr)

print("done")
