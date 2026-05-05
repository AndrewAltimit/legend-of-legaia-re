# @category Legaia
# @runtime Jython
#
# Dumps the top-N most-cited SCUS helpers that don't yet have a function
# dump. Targets are pre-computed by `scripts/function-coverage.py` and
# pasted in here. Edit TARGETS between sessions.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# Round 4 (post-2026-05-04 third batch). Picks from
# `scripts/function-coverage.py --json` -- the top-45 SCUS-range cited
# helpers still uncovered after rounds 1-3. Coverage was 284/709 = 40%
# entering this batch.
#
# High-leverage picks:
#   - 8001fbcc: VDF (asset type 7) post-processor (BACKLOG epic 4.5).
#   - 8003c6a4..8003d368: more helpers adjacent to the SCUS_8003CE08+ flag
#     dispatchers and the 0x8003CE9C operand decoders -- a tight cluster
#     around the field-VM SCUS interface.
#   - 80055ac8 / 8005b6a8 / 8005b7f8: PSX BIOS / OS thunks (printf, malloc).
#     Some may resolve as A-vectors and get marked OOB -- that's fine.
#   - 80016eb8 / 800178f0..80017aac: boot/init path helpers.
TARGETS = [
    "80064890", "8001a78c", "8001b964", "8001c204", "8001cf50",
    "8001fbcc", "80020f88", "8002689c", "80028158", "8002b96c",
    "800319a8", "80034e4c", "800355f0", "80035bd0", "80036514",
    "80036db0", "8003adac", "8003c11c", "8003c1f8", "8003c9ac",
    "8003d368", "80042f4c", "800467e8", "800558fc", "800559ec",
    "80055a5c", "80055ac8", "800589d0", "80059280", "8005b648",
    "8005b6a8", "8005b7f8", "8005ba38", "8005bac8", "8005be0c",
    "8005befc", "8005ca34", "8005cf80", "8005dbb4", "8005e9a4",
    "8005ea84", "80064370", "80065034", "80067480", "8006bcb4",
]

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

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
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


for t in TARGETS:
    dump(t)

print("done")
