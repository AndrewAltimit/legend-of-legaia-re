# @category Legaia
# @runtime Jython
#
# Track-C "XA channel map / STR demux SM" dumper. Two program targets:
#
# 1. overlay_cutscene_str_0970.bin (static import of PROT 0970 at base
#    0x801CE818, `asset overlay ghidra`): the STR-mode master dispatch that
#    hosts the fmv_id selector at 0x801CECA0 (`_DAT_8007BA78 << 6 +
#    0x801D0A6C` -> FUN_801CF098), plus the MDEC reset helper the captured
#    overlay_str_fmv dumps referenced but never dumped.
#
# 2. SCUS_942.54: the XA-clip CdSync-complete callbacks that the clip
#    starters arm (FUN_8003D53C / FUN_8003EAE4 / FUN_8003F128 register
#    LAB_8003D764 / LAB_8003DAA8 via FUN_8005BECC before CdlSetloc) - the
#    sequencers that issue the actual read command + drive mode for XA
#    playback. FUN_8003D764 is where the retail XA channel selector lives:
#    state 3 issues CdlSetfilter (com 0x0D) with {file=1, chan=gp+0x954}.
#
# TARGETS entries are (address, containing) pairs: containing=True means
# "dump the function containing this address" (for mid-function anchors
# like the 0x801CECA0 selector); False means the address is the entry
# point (created on the fly if analysis missed it, dump_funcs.py-style).
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_cutscene_str_0970.bin -noanalysis \
#       -postScript /scripts/dump_str0970_xa_dispatch.py
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/dump_str0970_xa_dispatch.py
#
# Output: /scripts/funcs/<addr>.txt (SCUS) or /scripts/funcs/
# overlay_cutscene_str_0970_<addr>.txt (overlay). gitignored (Sony-derived).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # --- overlay_cutscene_str_0970.bin ---
    # STR master dispatch: contains the fmv_id -> dispatch-slot selector at
    # 0x801CECA0. Entry = the only prologue between the head string table and
    # the play loop FUN_801CF098 (raw-byte prologue scan of the extracted
    # overlay blob).
    ("0x801CEA3C", False),
    ("0x801CECA0", True),
    # MDEC hardware reset / double-buffer prime (called by FUN_801CFC18).
    ("0x801CFEE0", False),
    # --- SCUS_942.54 ---
    # XA-clip CdSync callback armed by FUN_8003D53C (menu-voice / streamed
    # SFX clip start; reads the 8-byte clip table at 0x801C6ED8).
    ("0x8003D764", False),
    # Async-read CdSync callback armed by FUN_8003F128 (generic streaming
    # read kickoff; same callback idiom).
    ("0x8003DAA8", False),
    # Per-sector data-ready poller for the streaming reads.
    ("0x8003EF14", False),
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
        return os.path.join(OUT_DIR, addr_str.replace("0x", "").lower() + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str.replace("0x", "").lower() + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def dump(addr_str, containing):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) if containing else None
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None and not containing:
        if listing.getInstructionAt(addr) is None:
            disassemble(addr)
        func = createFunction(addr, "FUN_" + addr_str.replace("0x", "").lower())
        if func is not None:
            print("[new] created function at {}".format(addr_str))
    if func is None:
        print("[skip] no function for {} in {}".format(addr_str, prog_name))
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


for t, c in TARGETS:
    dump(t, c)

print("done [{}]".format(prog_name))
