# @category Legaia
# @runtime Jython
#
# Lane-B (RE wave 11) load-time-grade hunt in the cutscene/STR host overlay
# PROT 0970 (overlay_cutscene_str_0970.bin, static import at base 0x801CE818,
# `asset overlay ghidra`). Goal: find (or rule out) a CPU pass that rewrites
# CLUT / palette entries or TMD colour words at scene-asset load with the
# prologue gold law
#
#     L = max(r, g, b)  ->  (L, max(L-1,0), L >> 1)   [5-bit BGR555, STP kept]
#
# so the "far geometry too bright" residual of the New-Game opening chain can
# be attributed (docs/subsystems/cutscene.md "full-scene sepia grade").
#
# Two passes, both report ADDRESSES ONLY to stdout (no Sony bytes on stdout):
#
#   1. SIGNATURE SCAN. Walk every instruction in the program and flag the
#      arithmetic fingerprint of the law:
#        * a right-shift by 10 or 11 (extracting the BGR555 blue field), AND
#        * an `andi reg, 0x1f` (masking a 5-bit channel), AND
#        * a downstream right-shift by 1 (the `L >> 1` blue reconstruct).
#      A function carrying all three in a tight window is a palette-law
#      candidate. Also flags calls to the PsyQ image/CLUT upload primitives
#      by target address if their thunks are known.
#
#   2. DUMP. Write the disassembly+decompile of each flagged function and of
#      the explicitly listed anchors (the master dispatch + play loop) to
#      /scripts/funcs/<label>_<addr>.txt (gitignored, Sony-derived) so the
#      law can be ported/refuted from the DISASSEMBLY, not the C.
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_cutscene_str_0970.bin -noanalysis \
#       -postScript /scripts/dump_0970_lane_b.py
#
# ASCII-only (Jython 2.7).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# Explicit anchors worth dumping regardless of the scan (master dispatch +
# play loop + the two overlay init/load entries the STR host runs on entry).
ANCHORS = [
    "801cea3c",  # master dispatch + return-scene hand-off
    "801cf098",  # play loop
    "801ce818",  # overlay entry (base) -- init/reloc if present
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
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def label():
    return prog_name.replace(".bin", "").replace(".", "_")


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    return os.path.join(OUT_DIR, label() + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def dump_func(func):
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    addr_str = str(func.getEntryPoint()).lower()
    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} [{}] ==\n".format(
            func.getName(), addr_str, prog_name))
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


# ---- Pass 1: signature scan --------------------------------------------
# For each function, collect the shift/mask fingerprint of the 5-bit law.
def scan():
    hits = []
    funcs = list(fm.getFunctions(True))
    print("scanning {} functions in {}".format(len(funcs), prog_name))
    for func in funcs:
        body = func.getBody()
        has_srl10 = False   # extract blue field (>>10 or >>11)
        has_and1f = False   # 5-bit channel mask
        has_srl1 = False    # L >> 1 blue reconstruct
        has_slt = False     # max() via slt/movn or branch compare
        for ins in listing.getInstructions(body, True):
            mn = ins.getMnemonicString()
            s = ins.toString()
            if mn in ("srl", "sra"):
                # last operand is the shift amount
                if s.endswith(",0x1") or s.endswith(", 0x1") or s.endswith(",1"):
                    has_srl1 = True
                if "0xa" in s.lower() or "0xb" in s.lower() or s.endswith(",10") or s.endswith(",11"):
                    has_srl10 = True
            elif mn == "andi" and ("0x1f" in s.lower()):
                has_and1f = True
            elif mn in ("slt", "sltu", "movn", "movz"):
                has_slt = True
        score = has_srl10 + has_and1f + has_srl1 + has_slt
        if has_and1f and has_srl1 and (has_srl10 or has_slt):
            hits.append((func, score, has_srl10, has_and1f, has_srl1, has_slt))
    return hits


hits = scan()
print("--- LAW-SIGNATURE CANDIDATES ({}), highest score first ---".format(len(hits)))
hits.sort(key=lambda t: -t[1])
for (func, score, s10, a1f, s1, slt) in hits:
    print("  {} {} score={} [srl10={} and1f={} srl1={} slt={}]".format(
        str(func.getEntryPoint()).lower(), func.getName(), score, s10, a1f, s1, slt))
    dump_func(func)

# ---- Pass 2: explicit anchors ------------------------------------------
for a in ANCHORS:
    addr = af.getAddress(a)
    if addr is None or not in_program(addr):
        print("[skip anchor] {}".format(a))
        continue
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip anchor] no function at {}".format(a))
        continue
    dump_func(func)

print("done [{}]".format(prog_name))
