# @category Legaia
# @runtime Jython
#
# Dumps all 130 functions from the shop/menu overlay (overlay_shop_save.bin).
# The shop capture (mc0) is the menu overlay, which hosts shop, save-screen,
# status, inn, and related UI subsystems.
#
# Key unknowns to surface:
#   - 801dd35c (12KB) -- main shop / menu dispatcher
#   - 801e1c1c (4.5KB) -- buy/sell screen
#   - 801dc6b4 -- save-screen write path (context in menu overlay)
#   - 801e2ee4, 801e3ff0, 801d0f1c -- item-list / cursor helpers
#
# Run against the named overlay program:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_shop_save.bin -noanalysis \
#       -postScript /scripts/dump_shop_overlay.py
#
# Output files land in /scripts/funcs/overlay_shop_save_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # Top by incoming xref count (primary entry points and helpers)
    "801d6628",  # actor VM entry (68 callers) -- shared with other overlays
    "801e3ee0",  # 28 callers
    "801d688c",  # 21 callers
    "801e373c",  # 20 callers
    "801e36c4",  # 20 callers
    "801cf650",  # 10 callers
    "801e3ff0",  # 9 callers (item/cursor helper)
    "801e1c1c",  # 8 callers (4520 bytes -- buy/sell screen)
    "801e2ee4",  # 6 callers (944 bytes)
    "801e435c",  # 3 callers
    "801e3f74",  # 3 callers
    "801e39a8",  # 3 callers
    "801dd35c",  # 3 callers (12104 bytes -- main shop dispatcher)
    "801d0f1c",  # 3 callers (884 bytes)
    "801cf88c",  # 3 callers (1244 bytes)
    # 2-caller helpers
    "801e3ba0",
    "801e3af0",
    "801e3a98",
    "801e3a00",
    "801e3900",
    "801e38d8",
    "801dd0c0",
    "801d6a54",
    # 1-caller helpers and leaf functions
    "801e4140",
    "801e4138",
    "801e3e7c",
    "801e3d68",
    "801e3c90",
    "801e3bec",
    "801e38d0",
    "801e380c",
    "801e37cc",
    "801e3294",
    "801e2dc4",
    "801e1934",
    "801e16e0",
    "801e13b8",
    "801e1208",
    "801e1114",
    "801e0fd0",
    "801e08d8",
    "801e06c0",
    "801e0598",
    "801e0418",
    "801e02a4",
    "801dafd4",
    "801da9f8",
    "801d64a8",
    "801d2910",
    "801cf760",
    "801cf5d0",
    "801e420c",
    "801e4190",
    "801e36a0",
    # Save-screen cluster (menu overlay also hosts save-screen write path)
    "801dc6b4",  # save-screen write (FUN_801DC6B4)
    "801dd330",
    "801dd310",
    "801dd26c",
    "801dd1b8",
    "801dd12c",
    "801dd028",
    "801dcfe4",
    "801dcf84",
    "801dcf14",
    "801dcef0",
    "801dce20",
    "801dcd58",
    "801dccb4",
    "801dcc20",
    "801dcb60",
    "801dcb1c",
    "801dcad8",
    "801dca94",
    "801dca50",
    "801dca0c",
    "801dc1cc",
    "801dbd94",
    "801dbc5c",
    "801db7f4",
    "801db380",
    "801db21c",
    "801daef4",
    "801dae24",
    "801dad6c",
    # Remaining functions sorted by entry address
    "801d9c14",
    "801d99f0",
    "801d98f0",
    "801d9594",
    "801d9280",
    "801d9110",
    "801d8f10",
    "801d8d94",
    "801d8b90",
    "801d8a58",
    "801d8734",
    "801d8308",
    "801d7ff8",
    "801d7e50",
    "801d7c00",
    "801d6e18",
    "801d6d38",
    "801d6b20",
    "801d6360",
    "801d61b0",
    "801d603c",
    "801d5de0",
    "801d5ae8",
    "801d5944",
    "801d56fc",
    "801d5510",
    "801d4c28",
    "801d4a80",
    "801d4868",
    "801d33d8",
    "801d31ec",
    "801d2e74",
    "801d2c98",
    "801d2b44",
    "801d21c0",
    "801d2094",
    "801d1f10",
    "801d1dac",
    "801d1b20",
    "801d1290",
    "801d0d18",
    "801d0520",
    "801d030c",
    "801d0148",
    "801cfd68",
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
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    block = mem.getBlock(addr)
    return block is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
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
    finally:
        fh.close()
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done [{}]".format(prog_name))
