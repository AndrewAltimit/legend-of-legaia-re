# @category Legaia
# @runtime Jython
#
# Dumps all unique sub-state handlers from the save-screen dispatch table
# PTR_FUN_801e4f40.  The table was already documented in
# overlay_save_ui_801e4f40.txt; this script dumps every handler body.
#
# Run against overlay_save_ui_select.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_save_ui_select.bin -noanalysis \
#       -postScript /scripts/dump_save_ui_handlers.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.util.task import ConsoleTaskMonitor

# All unique handler addresses from PTR_FUN_801e4f40 (indices 0x00-0x20)
# Gathered from overlay_save_ui_801e4f40.txt
TARGETS = [
    "801dd12c",  # [0x00] init/null
    "801d6b20",  # [0x01] fade-in init
    "801d6e18",  # [0x02] wait (branch path)
    "801d6d38",  # [0x03]
    "801dd1b8",  # [0x04] entry-context 0x0D path
    "801d7c00",  # [0x05]
    "801d7e50",  # [0x06]
    "801d8734",  # [0x07]
    "801dd26c",  # [0x08]
    "801d7ff8",  # [0x09]
    "801d8308",  # [0x0a]
    "801d8a58",  # [0x0b]
    "801d8b90",  # [0x0c]
    "801d8d94",  # [0x0d]
    "801d8f10",  # [0x0e]
    "801d9110",  # [0x0f]
    "801d9280",  # [0x10]
    "801d9594",  # [0x11]
    "801d98f0",  # [0x12]
    "801d99f0",  # [0x13]
    "801d9c14",  # [0x14]
    "801da2a0",  # [0x15] (also targeted by dump_save_ui_handler_0x15.py)
    "801dd310",  # [0x16]
    "801dd330",  # [0x17]
    "801dae24",  # [0x18]
    "801daef4",  # [0x19] entry-context 0x01 path
    "801dafd4",  # [0x1a] normal save (entry-context NULL/0)
    "801db21c",  # [0x1b]
    "801db380",  # [0x1c]
    "801db7f4",  # [0x1d]
    "801dbc5c",  # [0x1e] save write path (story_flags + inventory)
    "801dbd94",  # [0x1f]
    "801dc1cc",  # [0x20] entry-context 0x07 path
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_save_ui_"

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


def ensure_func(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        return None
    func = fm.getFunctionAt(addr)
    if func is None:
        cmd = CreateFunctionCmd(addr)
        cmd.applyTo(prog, monitor)
        func = fm.getFunctionAt(addr)
    return func


def dump(addr_str):
    func = ensure_func(addr_str)
    if func is None:
        print("[skip] no function at " + addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_save_ui_select base=0x801C0000] ==\n".format(
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
            fh.write("(decompile exception: {})\n".format(str(e)))
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)

print("done")
