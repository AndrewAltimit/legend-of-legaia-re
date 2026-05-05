# @category Legaia
# @runtime Jython
#
# Like dump_funcs.py but targets the overlay.bin program. Hardcoded list of
# functions of interest discovered via list_overlay_functions.py.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # The colossal one: 12 KB, 134 outgoing calls. Most likely the script
    # VM dispatcher OR a top-level per-frame loop.
    "801dd35c",
    # Second-largest dispatcher candidate.
    "801d33d8",
    # Other dispatcher-shaped functions.
    "801d6e18",
    "801d0520",
    "801e1c1c",
    # Most-called utility (68 callers): probably an allocator / heap helper.
    "801d6628",
    # Mode-handler entry points discovered statically (from SCUS_942.54
    # mode table; see project_state_machine memory).
    "801d6704",  # MAIN INIT thunk target
    "801ce8ec",  # CONFIG INIT thunk target
    "801cf730",  # TMD TEST handler
    # Dialog overlay: functions containing accesses to MES-related globals.
    # _DAT_8007B8AC (MES size) is read at 0x801D7110 - this is the dialog
    # MES reader. Surrounding cluster 0x801D6830..0x801D7B98 has 14 hits on
    # _DAT_8007B8B8 (likely the dialog state machine).
    "801d7110",  # MES size reader candidate
    "801d6830",  # dialog state machine candidate
    "801d7518",  # called from FUN_801d6704 in tight loop (probably draw call)
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
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    # Prefix output with `overlay_` so it doesn't collide with SCUS dumps.
    out_path = os.path.join(OUT_DIR, "overlay_" + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay] ==\n".format(
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
