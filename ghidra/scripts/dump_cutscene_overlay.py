# @category Legaia
# @runtime Jython
#
# Dumps functions from the cutscene / field overlay (overlay_cutscene_dialogue.bin
# and overlay_cutscene_mapview.bin). Both captures load the same field overlay
# (which also hosts FUN_801DE840 field VM, FUN_801E76D4 world map controller,
# FUN_801D362C move-VM extension, and FUN_801EAD98 dev menu renderer).
#
# Key unknowns to surface (STR cutscene routing):
#   - 801dab90 (2432 bytes, 3 callers) -- large cutscene handler (XA sync?)
#   - 801ed710 (2032 bytes, 3 callers) -- STR sector dispatch candidate
#   - 801db510 (988 bytes, 3 callers)
#   - 801d7518 (732 bytes, 7 callers)
#   - 801e9b3c (652 bytes, 8 callers)
#   - 801dbc20 (636 bytes, 3 callers)
#   - 801d629c (828 bytes, 2 callers)
#   - 801daa50 (320 bytes, 9 callers)  -- heavily called (frame counter? sector read?)
#   - 801d9e1c (1396 bytes, 2 callers)
#
# Run against BOTH programs; each produces prefixed output files:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_cutscene_dialogue.bin -noanalysis \
#       -postScript /scripts/dump_cutscene_overlay.py
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_cutscene_mapview.bin -noanalysis \
#       -postScript /scripts/dump_cutscene_overlay.py
#
# Output: /scripts/funcs/overlay_cutscene_dialogue_<addr>.txt (and _mapview_)

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # Top by incoming xref count -- primary entry points
    "801daa50",  # 9 callers (320 bytes) -- heavily called (frame timing / sector read?)
    "801e9b3c",  # 8 callers (652 bytes)
    "801db8ec",  # 7 callers (308 bytes)
    "801d7518",  # 7 callers (732 bytes)
    "801de3e0",  # 6 callers (152 bytes)
    "801de190",  # 5 callers (164 bytes)
    "801d5630",  # 5 callers (148 bytes)
    "801cfc40",  # 5 callers (524 bytes)
    "801e9dc8",  # 4 callers (412 bytes)
    "801e3894",  # 4 callers (240 bytes)
    "801e3764",  # 4 callers (304 bytes)
    "801e3658",  # 4 callers (268 bytes)
    "801d65d8",  # 4 callers (300 bytes)
    "801d58f0",  # 4 callers (124 bytes)
    "801cfe4c",  # 4 callers (868 bytes)
    # 3-caller cluster -- key STR / cutscene candidates
    "801ed710",  # 3 callers (2032 bytes) -- STR sector dispatch candidate
    "801e45bc",  # 3 callers (472 bytes)
    "801ddf48",  # 3 callers (156 bytes)
    "801dbc20",  # 3 callers (636 bytes)
    "801db510",  # 3 callers (988 bytes)
    "801dab90",  # 3 callers (2432 bytes) -- large cutscene handler
    "801cf8ac",  # 3 callers (328 bytes)
    # 2-caller helpers
    "801de2b0",
    "801de084",
    "801de004",
    "801dba20",
    "801d9e1c",  # 1396 bytes
    "801d81e0",
    "801d79e8",
    "801d629c",  # 828 bytes
    "801d5b5c",
    "801d5ae0",
    "801d5a24",
    "801cf9f4",
    # 1-caller / leaf functions
    "801ead98",  # dev menu renderer (FUN_801EAD98)
    "801ea9b0",
    "801e9f64",
    "801e75dc",
    "801e7448",
    "801e57f0",
    "801e573c",
    "801e5668",
    "801e4c58",
    "801e3e00",
    "801e3984",
    "801de7bc",
    "801de754",
    "801de698",
    "801de234",
    "801ddfe4",
    "801dde34",
    "801dd310",
    "801da390",
    "801d9d30",
    "801d84b4",
    "801d8450",
    "801d841c",
    "801d835c",
    "801d8280",
    "801d8258",
    "801d77f4",
    "801d5e20",
    "801d596c",
    "801d31b0",
    "801d2d38",
    "801d25ec",
    "801d2404",
    "801d1ec4",
    "801d1ba0",
    "801d1878",
    "801d0d38",
    "801d0b90",
    "801d095c",
    "801d01b0",
    "801cf754",
    # World-map / field overlay functions (shared with world_map captures)
    "801ef2b0",
    "801ef014",
    "801eed58",
    "801ee90c",
    "801ee5d4",
    "801ee328",
    "801ee094",
    "801edf00",
    "801ed590",
    "801ed308",
    "801ecd0c",
    "801eca08",
    "801e76d4",  # WorldMapController main (FUN_801E76D4)
    "801e733c",
    "801e71d0",
    "801e6f70",
    "801e6b34",
    "801e6984",
    "801e6778",
    "801e662c",
    "801e6400",
    "801e5b4c",
    "801e5a08",
    "801e58a8",
    "801e5834",
    "801e5338",
    "801e4d8c",
    "801e4794",
    "801e4470",
    "801de840",  # field VM (FUN_801DE840)
    "801de478",
    "801ddc20",
    "801dd9d4",
    "801dc0bc",
    "801dbe9c",
    "801da7f0",
    "801da51c",  # world-map entity SM
    "801d9c3c",
    "801d84d0",
    "801d7ea0",
    "801d7b50",
    "801d6704",
    "801d6058",
    "801d5d60",
    "801d5c08",
    "801d5a68",
    "801d5780",
    "801d4a60",
    "801d362c",  # move-VM overlay extension
    "801d2ebc",
    "801d27e0",
    "801d2298",
    "801d1344",
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
