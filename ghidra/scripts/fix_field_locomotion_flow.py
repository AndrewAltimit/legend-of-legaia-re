# @category Legaia
# @runtime Jython
#
# Repair + re-decompile the 0897 field locomotion cluster.
#
# Root cause of the noisy decompiles: the field-VM operand reader at
# 0x8003ce9c is treated as non-returning, so after every `jal 0x8003ce9c`
# Ghidra abandons disassembly and leaves valid MIPS as raw data bytes.
# Those holes break flow analysis, which (a) splits the real functions at
# mid-block fake `FUN_` entries and (b) makes the decompiler splice
# phantom blocks (args appearing in saved registers s3/s4/s7/s8).
#
# Repair steps (DB-modifying, local gitignored project only):
#   1. Force-disassemble every undefined byte in the cluster range.
#   2. Remove mid-block fake functions (entry whose first instruction is
#      not a `addiu sp,sp,-N` prologue) so the real functions re-merge.
#   3. Re-decompile the true enclosing functions and dump them.
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_0897.bin.0 -noanalysis \
#       -postScript /scripts/fix_field_locomotion_flow.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.address import AddressSet
from ghidra.util.task import ConsoleTaskMonitor

# Cluster extent to repair (covers FUN_801db... through FUN_801dc0bb).
RANGE_LO = "801db000"
RANGE_HI = "801dc200"

# Enclosing functions to re-decompile after repair.
ENCLOSING = ["801db7b0", "801dbc20"]

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

lo = af.getAddress(RANGE_LO)
hi = af.getAddress(RANGE_HI)

if mem.getBlock(lo) is None:
    print("[skip] cluster not in {}".format(prog_name))
else:
    # --- Step 1: force-disassemble undefined bytes in the range ---
    disassembled = 0
    addr = lo
    while addr.compareTo(hi) < 0:
        ins = listing.getInstructionAt(addr)
        if ins is not None:
            addr = addr.add(ins.getLength())
            continue
        data = listing.getDefinedDataAt(addr)
        # Only act on undefined / raw bytes.
        cmd = DisassembleCommand(addr, None, True)
        cmd.applyTo(prog, monitor)
        ins2 = listing.getInstructionAt(addr)
        if ins2 is not None:
            disassembled += 1
            addr = addr.add(ins2.getLength())
        else:
            addr = addr.add(1)
    print("disassembled {} hole sites".format(disassembled))

    # --- Step 2: remove every function in range, then recreate at real
    # prologues so bodies follow flow through the now-filled holes ---
    def is_prologue(addr):
        ins = listing.getInstructionAt(addr)
        if ins is None or ins.getMnemonicString().lower() != "addiu":
            return False
        return "sp,sp,-" in ins.toString().lower().replace(" ", "")

    existing = list(fm.getFunctions(AddressSet(lo, hi), True))
    for f in existing:
        fm.removeFunction(f.getEntryPoint())
    print("cleared {} functions in range".format(len(existing)))

    # Scan for prologues and (re)create functions.
    prologues = []
    addr = lo
    while addr.compareTo(hi) < 0:
        ins = listing.getInstructionAt(addr)
        if ins is None:
            addr = addr.add(1)
            continue
        if is_prologue(addr):
            prologues.append(addr)
        addr = addr.add(ins.getLength())
    created = 0
    for p in prologues:
        cmd = CreateFunctionCmd(p)
        if cmd.applyTo(prog, monitor):
            created += 1
    print("found {} prologues, created {} functions".format(
        len(prologues), created))

    # --- Step 3: re-decompile enclosing functions ---
    decomp = DecompInterface()
    decomp.setOptions(DecompileOptions())
    decomp.openProgram(prog)

    label = prog_name.replace(".bin", "").replace(".", "_")
    out_path = os.path.join(OUT_DIR, label + "_locomotion_repaired.txt")
    fh = open(out_path, "w")
    try:
        fh.write("# Repaired field locomotion decompiles [{}]\n\n".format(prog_name))
        for t in ENCLOSING:
            a = af.getAddress(t)
            func = fm.getFunctionContaining(a) or fm.getFunctionAt(a)
            if func is None:
                fh.write("[no function at/around {}]\n\n".format(t))
                continue
            body = func.getBody()
            fh.write("== {} (entry={}, min={}, max={}, {} bytes) ==\n".format(
                func.getName(), func.getEntryPoint(),
                body.getMinAddress(), body.getMaxAddress(),
                body.getNumAddresses()))
            try:
                res = decomp.decompileFunction(func, 90, monitor)
                if res.decompileCompleted():
                    fh.write(res.getDecompiledFunction().getC())
                else:
                    fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
            except Exception as e:
                fh.write("(decompile exception: {})\n".format(e))
            fh.write("\n")
    finally:
        fh.close()
    print("wrote {}".format(out_path))

print("done [{}]".format(prog_name))
