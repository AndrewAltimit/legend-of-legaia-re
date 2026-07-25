# @category Legaia
# @runtime Jython
#
# Repair dumps whose function body ends before the real routine does.
#
# This is a distinct defect from a mis-based dump. A truncated dump is
# INTERNALLY CONSISTENT - its `size=` header agrees with its own
# instruction count, its printed addresses are correct, and every
# instruction it does carry is real. What is wrong is the extent: Ghidra
# computed a function body that stops short, usually because a second
# `FUN_` entry was minted inside the routine (a jump-table arm or a
# `jal` target Ghidra could not tie back), and the body was cut there.
#
# No header check and no base check can see this, so the dump reads as
# authoritative while silently withholding the rest of the body.
#
# Method: ignore Ghidra's boundary. Walk instructions from the entry to
# the first `jr ra` (force-disassembling where needed), then rebuild the
# function over that whole span - deleting any function entry strictly
# inside it first, since those are what cut the body - and dump the
# result. The dumped extent is therefore a `jr ra` walk, which is what a
# MIPS routine's extent actually is.
#
# Guards: the walk stops at MAX_INSNS, and a span that does not reach a
# `jr ra` is reported and skipped rather than dumped, so a bad target
# cannot mint a plausible-looking oversized body.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_fishing -noanalysis \
#       -postScript /scripts/repair_truncated_dumps.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.address import AddressSet
from ghidra.program.model.symbol import SourceType
from ghidra.util.task import ConsoleTaskMonitor

# Entry points whose dumps the corpus sweep flagged as defective. Each is
# tried against every program; `in_program()` skips the ones that do not
# hold the address, so one target list serves the whole project.
#
# A bare address is a plain re-dump: whatever function Ghidra already has is
# written out, and a body is rebuilt only when no function exists at all.
# That is deliberately conservative - the `jr ra` walk stops at the FIRST
# return in address order, which for a routine with an early-exit arm is
# SHORTER than the real body, so rebuilding on every target would truncate
# the very dumps this script exists to repair.
#
# Suffix an address with `!` to force the rebuild. Use it only where the
# sweep says the body is genuinely cut - i.e. NO_RETURN, where the stream
# provably does not reach a return at all.
TARGETS = [
    "801d56e4!",
]

# Optional override, one entry per line, same `addr[!]` syntax. Gitignored
# alongside the dumps, so each sweep populates it on demand.
TARGETS_FILE = "/scripts/redump_targets.txt"

MAX_INSNS = 4000

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


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def is_jr_ra(ins):
    return ins.getMnemonicString().lower() == "jr" and "ra" in ins.toString()


def walk_to_return(entry):
    """Instruction walk from `entry` to the first `jr ra`, ignoring bounds.

    Returns (stop_addr_exclusive, n_instructions) or (None, n) when no
    `jr ra` is reached inside MAX_INSNS.
    """
    cur = entry
    seen = 0
    while seen < MAX_INSNS:
        ins = listing.getInstructionAt(cur)
        if ins is None:
            DisassembleCommand(cur, None, True).applyTo(prog, monitor)
            ins = listing.getInstructionAt(cur)
        if ins is None:
            return None, seen
        seen += 1
        if is_jr_ra(ins):
            # The delay slot belongs to the routine.
            return cur.add(8), seen + 1
        cur = cur.add(ins.getLength())
    return None, seen


def dump(func, extent_note):
    addr_str = "%08x" % func.getEntryPoint().getOffset()
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    path = out_path_for(addr_str)
    fh = open(path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
        fh.write("size={} bytes, {} instructions\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("extent={}\n\n".format(extent_note))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 90, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    return path


def repair(addr_str, force_rebuild):
    entry = af.getAddress(addr_str)
    if entry is None or not in_program(entry):
        return

    before = fm.getFunctionAt(entry)
    before_bytes = before.getBody().getNumAddresses() if before else 0

    if before is not None and not force_rebuild:
        # Plain re-dump: Ghidra already has a body and the sweep did not say
        # it was cut, so its extent is the one to trust.
        print("{} {}: re-dumping ghidra body ({} B)".format(
            prog_name, addr_str, before_bytes))
        print("  wrote {}".format(dump(before, "ghidra function body")))
        return

    stop, n = walk_to_return(entry)
    if stop is None:
        print("[skip] {} {}: no `jr ra` within {} instrs".format(
            prog_name, addr_str, n))
        return
    want_bytes = stop.subtract(entry)

    print("{} {}: ghidra body {} B, `jr ra` walk {} B ({} instrs)".format(
        prog_name, addr_str, before_bytes, want_bytes, n))

    if before is not None and before_bytes >= want_bytes:
        print("  already whole - re-dumping unchanged extent")
        print("  wrote {}".format(dump(before, "ghidra function body")))
        return

    # The interior `FUN_` entries are what cut the body; drop them, and
    # the entry itself, then rebuild over the walked span.
    victims = []
    probe = entry.add(4)
    while probe.compareTo(stop) < 0:
        f = fm.getFunctionAt(probe)
        if f is not None:
            victims.append(probe)
        probe = probe.add(4)
    for v in victims:
        print("  dropping interior function entry {}".format(v))
        fm.removeFunction(v)
    if before is not None:
        fm.removeFunction(entry)

    span = AddressSet(entry, stop.subtract(1))
    CreateFunctionCmd(None, entry, span, SourceType.USER_DEFINED).applyTo(
        prog, monitor)
    func = fm.getFunctionAt(entry)
    if func is None:
        print("  [skip] rebuild produced no function")
        return
    got = func.getBody().getNumAddresses()
    print("  rebuilt body {} B (wanted {})".format(got, want_bytes))
    print("  wrote {}".format(dump(func, "jr-ra walk, body rebuilt")))


targets = TARGETS
if os.path.exists(TARGETS_FILE):
    targets = []
    fh = open(TARGETS_FILE, "r")
    try:
        for line in fh:
            line = line.split("#")[0].strip()
            if line:
                targets.append(line)
    finally:
        fh.close()
    print("[targets] {} from {}".format(len(targets), TARGETS_FILE))

for t in targets:
    force = t.endswith("!")
    repair(t.rstrip("!").strip(), force)

print("done [{}]".format(prog_name))
