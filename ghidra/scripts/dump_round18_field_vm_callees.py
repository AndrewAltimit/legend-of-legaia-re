# @category Legaia
# @runtime Jython
#
# Round 18: SCUS coverage hit zero after rounds 16+17, so the next leverage
# point is the field-VM callee chain in the 0897 town/field overlay. The
# field VM dispatcher `FUN_801DE840` makes 357 outgoing calls; many are
# already dumped, but the 0x801D-0x801E range still has ~133 helpers that
# are cited from existing dumps but not yet extracted.
#
# This batch picks the first 35 by address - enough to surface a coherent
# slice of the dispatcher's callee chain without exploding the dump count.
#
# Run:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process overlay_0897_xxx_dat.bin \
#     -noanalysis -scriptPath /scripts \
#     -postScript dump_round18_field_vm_callees.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d02c0", "801d03a4", "801d095c", "801d0abc", "801d0d38",
    "801d1344", "801d1878", "801d1960", "801d1af4", "801d1cf0",
    "801d1cfc", "801d1d9c", "801d1ec4", "801d1ef0", "801d231c",
    "801d2404", "801d2524", "801d25ec", "801d261c", "801d2774",
    "801d2958", "801d2968", "801d2d1c", "801d2d2c", "801d30b8",
    "801d3170", "801d3290", "801d3b0c", "801d3d78", "801d3db4",
    "801d3e28", "801d3fd0", "801d4004", "801d4040", "801d40dc",
]

PROGRAM_PREFIX = "overlay_0897"

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
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


def dump(addr_str):
    func = ensure_function(addr_str)
    if func is None:
        print("[skip] {}".format(addr_str))
        return

    out_path = os.path.join(OUT_DIR, "{}_{}.txt".format(PROGRAM_PREFIX, addr_str))
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


for t in TARGETS:
    dump(t)

print("done")
