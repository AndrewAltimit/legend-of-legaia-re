# @category Legaia
# @runtime Jython
#
# Round 9 (2026-05-04 session 26): next batch of top-cited orphans after
# round 7+8. Most are now 1-or-2-citation helpers, so the pickings are
# leaner -- but 801EC964 has 5 refs and is the new top.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

SCUS_TARGETS = [
    "80036044",  # text glyph walker (companion of FUN_8003CC98 / 8003CD00)
    "80016e4c", "80016eb8", "800178f0", "80017978", "80017aac",
    "80017fbc", "800196a4", "80019890", "8001a374", "8001a89c",
    "8001a8dc", "8001ad38", "8001c394", "8001d058", "8001d088",
    "8001d140", "8001d424", "8001f690", "80020038", "80020310",
    "800203ec", "80020b00", "80023070", "8002519c", "80025cb4",
    "80026018",
]

OVERLAY_TARGETS = [
    "801ec964",   # 5 refs - top missing
    "801d06e0", "801e1ab0", "801e1d98", "801e45bc", "801e752c",
    "801e805c", "801f03b0",
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

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


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


def dump(addr_str, prefix):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    if prefix:
        out_name = "{}_{}.txt".format(prefix, addr_str)
    else:
        out_name = "{}.txt".format(addr_str)
    out_path = os.path.join(OUT_DIR, out_name)
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
    print("wrote {}".format(out_path))


print("program: {}".format(prog_name))
if "SCUS" in prog_name:
    for t in SCUS_TARGETS:
        dump(t, prefix=None)
elif "0897" in prog_name:
    for t in OVERLAY_TARGETS:
        dump(t, prefix="overlay_0897")
else:
    print("(no targets for this program; skipping)")

print("done")
