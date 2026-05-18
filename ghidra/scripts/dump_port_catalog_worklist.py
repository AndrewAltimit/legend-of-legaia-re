# @category Legaia
# @runtime Jython
#
# Dumps the "cited but not dumped" worklist surfaced by
# scripts/port-catalog.py. Each address has been referenced from at
# least one existing dump but has no dump of its own yet - filling
# them in closes the BFS frontier on the citation graph.
#
# Overlay-aware: `in_program(addr)` skips silently when the address
# is not in the currently-loaded program's memory blocks, so a single
# script can be run against each program in the project and pick up
# whichever subset of TARGETS lives there.
#
# Output: `<addr>.txt` for SCUS-resident addresses,
#         `<prog_label>_<addr>.txt` for overlay-resident addresses
#         (matches the existing dump_pending_helpers.py pattern).
#
# Invocation per program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process <prog>.bin -noanalysis \
#       -postScript /scripts/dump_port_catalog_worklist.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # SCUS-resident, first wave
    "80044798",  # 24 refs - slot-4 area, cited from FUN_800445b0
    "8005fde8",  # 4 refs
    "8003d1ec",  # 3 refs (no-fn at first pass - may need force-disasm)
    "80059bd4",  # 2 refs - cited from FUN_80059c00
    "8005aa30",  # 2 refs
    "8005aa64",  # 2 refs
    "8001cd68",  # 1 ref
    "8002b984",  # 1 ref (no-fn at first pass)
    "8002b98c",  # 1 ref (no-fn at first pass)
    "80034cc4",  # 1 ref - gp_drawable writers
    "80034fa0",  # 1 ref - gp_drawable writers
    "80055b6c",  # 1 ref
    "8005bbf8",  # 1 ref - cited from overlay_str_fmv (no-fn at first pass)
    "8005eb68",
    "8005ec7c",
    "8005ed64",
    "8005edc4",
    "8005ee4c",  # (no-fn at first pass)
    "8005ef40",  # (no-fn at first pass)
    "8005f024",

    # SCUS-resident, surfaced by first-pass BFS
    "80054a6c",
    "8005567c",
    "80055b20",
    "8005c2e4",
    "8005ebfc",
    "8005ec1c",  # surfaced by second-pass BFS (cited from 8005bbf8)
    "8005ecd4",
    "8005ef04",
    "8005f004",
    "8005f994",
    "8005f9c8",

    # Overlay-resident, first wave
    "801f33b4",  # 5 refs - overlay_baka_fighter
    "801c2520",  # 1 ref  - overlay_0897 family
    "801cf5e8",  # 1 ref  - overlay_world_map_top
    "801cf678",  # 1 ref  - overlay_world_map_top
    "801d1694",  # 1 ref  - overlay_0897 family
    "801d1744",  # 1 ref  - overlay_0897 family
    "801d1854",  # 1 ref  - overlay_0897 family
    "801e249c",  # 1 ref  - overlay_0897 family
    "801f89b8",  # 1 ref  - overlay_world_map_top (or _ext variant)
    "801f8ab0",  # 1 ref  - overlay_0897 family

    # Overlay-resident, surfaced by first-pass BFS
    "801d0e78",
    "801d0eec",
    "801d0fe4",
    "801dfef4",

    # Surfaced by second-pass PORT-tag backfill (port without dump)
    "801e30e4",  # FMV opcode handler in cutscene_dialogue overlay
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
