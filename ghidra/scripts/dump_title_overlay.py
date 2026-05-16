# @category Legaia
# @runtime Jython
#
# Dumps the title-overlay tick function (and its caller) from
# overlay_title.bin.
#
# overlay_title.bin captured from PCSX-Redux sstate8 via
# autorun_countdown_trigger.lua. The watchpoint at 0x801EF16C (title-
# attract countdown) fired once per frame; the BP captured the PC of
# the title overlay's per-frame tick function exactly at the decrement
# instruction.
#
# Captured registers (captures/boot_walk/overlay_title.bin.regs):
#   pc 0x801DDCCC  - tick instruction that decrements the countdown
#   ra 0x801DD6B8  - caller (a frame outer-loop or game-mode dispatcher)
#   a0 0x801F0000  - struct base passed in (title-overlay state? or BSS?)
#   sp 0x801FFDE0  - stack near top of overlay window
#   gp 0x8007B318  - SCUS-side globals
#
# Run against the named overlay program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_title.bin -noanalysis \
#       -postScript /scripts/dump_title_overlay.py
#
# Output files land in /scripts/funcs/overlay_title_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # Pinned from the watchpoint capture.
    "801ddccc",  # tick instruction that decrements 0x801EF16C
    "801dd6b8",  # caller (RA)
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
