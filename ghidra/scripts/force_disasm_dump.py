# @category Legaia
# @runtime Jython
# Force-disassemble + create function at addresses that are valid `jal`
# targets but unanalyzed. Dumps each.
#
# Strategy: use DisassembleCommand to disassemble starting at the target.
# If it produces >=8 instructions ending in `jr $ra`, create a function
# via CreateFunctionCmd and dump it. Validation happens before creation
# so the project DB doesn't pick up phantom stubs.
#
# Input: one hex address per line in `ADDRS_FILE` (place the file alongside
# this script -- gitignored, so each session populates it on demand).

import os
from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor
from ghidra.program.model.address import AddressSet

OUT_DIR = "/scripts/funcs"
ADDRS_FILE = "/scripts/force_disasm_targets.txt"

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def addr(s):
    return af.getAddress(s)


def force_disasm(a):
    cmd = DisassembleCommand(a, None, True)
    if not cmd.applyTo(prog, monitor):
        return False
    return True


def find_function_end(a, max_instrs=2000):
    """Walk instructions starting at a until jr $ra or some bound."""
    cur = a
    count = 0
    last = None
    while count < max_instrs:
        ins = listing.getInstructionAt(cur)
        if ins is None:
            return None, count
        last = ins
        # delay slot of jr $ra closes the function
        mnem = ins.getMnemonicString().lower()
        if mnem == "jr":
            ops = ins.toString()
            if "ra" in ops or "$ra" in ops:
                # also include delay slot
                cur = cur.add(4)
                ds = listing.getInstructionAt(cur)
                if ds is not None:
                    return cur.add(4), count + 2
                return cur, count + 1
        cur = cur.add(4)
        count += 1
    return None, count


def dump_func(func, out_path):
    if os.path.exists(out_path):
        print("[dump_func] skip {} (file exists)".format(out_path))
        return False
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    if len(instrs) < 2:
        # `< 2` lets through legitimate `jr ra; nop` PsyQ thunks while still
        # rejecting completely empty regions or single-instruction garbage.
        print("[dump_func] skip {} ({} instrs, too short)".format(out_path, len(instrs)))
        return False
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
            func.getName(), "%08x" % func.getEntryPoint().getOffset(),
            func.getEntryPoint()))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 30, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    print("dumped " + out_path)
    return True


with open(ADDRS_FILE) as fh:
    target_addrs = [line.strip() for line in fh if line.strip()]
print("[force_disasm] loaded {} targets from {}".format(len(target_addrs), ADDRS_FILE))

count_dumped = 0
count_failed = 0

for hs in target_addrs:
    a = addr(hs)
    if a is None:
        print("[force_disasm] bad addr: {!r}".format(hs))
        continue
    blk = mem.getBlock(a)
    if blk is None:
        print("[force_disasm] {} not in any block (skip)".format(hs))
        continue
    # Already a function?
    if fm.getFunctionAt(a) is not None:
        f = fm.getFunctionAt(a)
        if dump_func(f, os.path.join(OUT_DIR, hs + ".txt")):
            count_dumped += 1
        continue
    # Force disasm
    if listing.getInstructionAt(a) is None:
        if not force_disasm(a):
            print("skip {} (disasm failed)".format(hs))
            count_failed += 1
            continue
    # Validate: walk to jr $ra
    ins0 = listing.getInstructionAt(a)
    if ins0 is None:
        print("skip {} (no instr after disasm)".format(hs))
        count_failed += 1
        continue
    end_addr, n_instrs = find_function_end(a)
    if end_addr is None or n_instrs < 2:
        # < 2 still rejects empty / data regions but lets through legitimate
        # 2-instruction stubs (`jr ra; nop` PsyQ thunks). The earlier < 8
        # heuristic was tuned for noise avoidance on speculative force-disasm
        # batches; explicit targets pre-filtered by port-catalog don't need it.
        print("skip {} (too short or no end: {} instrs)".format(hs, n_instrs))
        count_failed += 1
        continue
    # Create function
    body = AddressSet(a, end_addr.subtract(1))
    cmd = CreateFunctionCmd(None, a, body, ghidra.program.model.symbol.SourceType.USER_DEFINED)
    if not cmd.applyTo(prog, monitor):
        print("skip {} (createfn failed)".format(hs))
        count_failed += 1
        continue
    f = fm.getFunctionAt(a)
    if f is None:
        print("skip {} (no fn after create)".format(hs))
        count_failed += 1
        continue
    if dump_func(f, os.path.join(OUT_DIR, hs + ".txt")):
        count_dumped += 1
    else:
        count_failed += 1

print("done: {} dumped, {} skip/fail".format(count_dumped, count_failed))
