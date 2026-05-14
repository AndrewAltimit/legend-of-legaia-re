# @category Legaia
# @runtime Jython
#
# Dump the overlay-side functions that reference the start of the
# DAT_8007C018 table. Two candidates surfaced from the Ghidra xref scan:
#
#   - FUN_801D8280 @ 0x801D82D0 reads both 0x8007C018 and 0x8007C01C
#     (dual ref on one PC -> classic lw + 4-byte-stride pattern, e.g.
#     `lw reg, 0x4(base)` against `lui+addiu base = 0x8007C018`).
#   - FUN_801D77F4 @ 0x801D7878 carries a DATA ref to 0x8007C018
#     (probably an `addiu` that materialises the table address as a
#     base pointer, then walks it).
#
# Both functions live in the world_map / world_map_top / world_map_top_ext /
# world_map_walk overlay set + cutscene_mapview + 0897. Run against any
# one of those overlays - the addresses are identical because they share
# the overlay-image layout.
#
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_world_map_top_ext.bin \
#       -noanalysis -postScript /scripts/dump_dat_8007c018_helpers.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.program.model.address import AddressSet
from ghidra.program.model.symbol import SourceType

TARGETS = [
    ("801d77f4", "dat_8007c018_dataref"),
    ("801d8280", "dat_8007c018_dual_reader"),
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


def walk_forward_for_jr_ra(start_addr, max_instrs=4000):
    cur = start_addr
    count = 0
    while count < max_instrs:
        ins = listing.getInstructionAt(cur)
        if ins is None:
            return None
        if ins.getMnemonicString().lower() == "jr" and "ra" in ins.toString():
            return cur.add(4).add(3)
        cur = cur.add(4)
        count += 1
    return None


def ensure_function(entry_addr, end_addr):
    body = AddressSet(entry_addr, end_addr)
    for func_in_range in list(fm.getFunctions(body, True)):
        ep = func_in_range.getEntryPoint()
        if body.contains(ep):
            if func_in_range.getBody().getNumAddresses() == body.getNumAddresses():
                return func_in_range
            print("[reset] removing overlapping function at %s (size=%d, want=%d)" % (
                ep, func_in_range.getBody().getNumAddresses(), body.getNumAddresses()))
            fm.removeFunction(ep)
    cmd = CreateFunctionCmd(None, entry_addr, body, SourceType.USER_DEFINED)
    if not cmd.applyTo(prog, monitor):
        print("[fail] CreateFunctionCmd failed at %s" % entry_addr)
        return None
    func = fm.getFunctionAt(entry_addr)
    if func is None:
        return None
    if func.getBody().getNumAddresses() != body.getNumAddresses():
        try:
            func.setBody(body)
        except Exception as exc:
            print("[warn] setBody failed: %s" % exc)
    return func


def dump_func(func, entry_str, label):
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(
        OUT_DIR,
        "%s_%s_%s.txt" % (
            prog_name.replace(".bin.0", "").replace(".bin", "").replace(".", "_"),
            label, entry_str),
    )
    fh = open(out_path, "w")
    try:
        fh.write("== %s %s (entry=%s, label=%s) [%s] ==\n" % (
            func.getName(), entry_str, func.getEntryPoint(), label, prog_name))
        fh.write("size=%d bytes, %d instructions\n\n" % (
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 180, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as exc:
            fh.write("(decompile exception: %s)\n" % exc)
    finally:
        fh.close()
    print("wrote %s (%d bytes, %d instr)" % (
        out_path, body.getNumAddresses(), len(instrs)))


for entry_str, label in TARGETS:
    entry = af.getAddress(entry_str)
    if listing.getInstructionAt(entry) is None:
        DisassembleCommand(entry, None, True).applyTo(prog, monitor)
    end = walk_forward_for_jr_ra(entry)
    if end is None:
        print("[skip] no jr ra after %s" % entry_str)
        continue
    func = ensure_function(entry, end)
    if func is not None:
        dump_func(func, entry_str, label)

print("done")
