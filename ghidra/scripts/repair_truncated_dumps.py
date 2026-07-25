# @category Legaia
# @runtime Jython
#
# Repair defective dumps: bodies that stop before the routine does, dumps
# whose address column was never printed, and requested addresses that turn
# out not to be function entries at all.
#
# The first defect is distinct from a mis-based dump. A truncated dump is
# INTERNALLY CONSISTENT - its `size=` header agrees with its own instruction
# count, its printed addresses are correct, and every instruction it does
# carry is real. What is wrong is the extent: Ghidra computed a function body
# that stops short, usually because a second `FUN_` entry was minted inside
# the routine (a jump-table arm, or a `jal` target it could not tie back), and
# the body was cut there. No header check and no base check can see that, so
# the dump reads as authoritative while silently withholding the rest.
#
# Method: ignore Ghidra's boundary. Walk instructions from the entry to the
# real end of the routine (force-disassembling where needed), then rebuild the
# function over that whole span - deleting any function entry strictly inside
# it first, since those are what cut the body - and dump the result.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_fishing -noanalysis \
#       -postScript /scripts/repair_truncated_dumps.py
#
# --- Two walking rules, opposite blind spots, neither safe alone ---
#
# Stopping at the first `jr ra` truncates any routine with an early-exit arm.
# Stopping at the first unconditional `j` truncates any routine that jumps
# forward into a shared epilogue - that one reported a 563-instruction body as
# 18. Both fail SILENTLY, and in opposite directions, so a body walked by
# either rule alone can be short with nothing to show for it.
#
# So a terminator ends the body only when nothing already walked branches PAST
# it. The walk tracks `frontier`, the highest forward branch or jump target
# seen so far; a `jr ra` or an outbound `j` below the frontier is an early exit
# or an inter-block hop, and the walk continues. Same reasoning either rule
# applies locally, applied over the whole body instead.
#
# A walk that ends any other way is reported and the target is SKIPPED rather
# than dumped, because an instruction count that is really a lower bound is
# indistinguishable from a whole body once it is quoted somewhere else.
#
# --- The interior guard ---
#
# Ghidra resolves an address with `getFunctionContaining()`, so asking for an
# address inside a routine yields the ENCLOSING function. A dumper that then
# names its output after the address it REQUESTED writes a file asserting an
# entry point that does not exist - and every citation built on that filename
# inherits the assertion. The signature is that the resolved entry is always
# BELOW the requested address; nothing else produces it.
#
# This script never does that. Output is named from `getEntryPoint()`, an
# interior request is reported as INTERIOR and never rebuilt into a function,
# and the requested address is preserved in the header as `requested=` so the
# citation that pointed there is traceable rather than lost.

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
# That is deliberately conservative - a rebuild deletes interior function
# entries, which is a mutation of the project database.
#
# Suffix an address with `!` to force the rebuild. Use it where the sweep says
# the body is genuinely cut - i.e. NO_RETURN, where the stream provably does
# not reach a return at all.
#
# Suffix with `?` to AUDIT only: classify the address and write a verdict, but
# never write a dump and never touch the database.
TARGETS = [
    "801d56e4!",
]

# Optional override, one entry per line, same `addr[!?]` syntax. Gitignored
# alongside the dumps, so each sweep populates it on demand.
TARGETS_FILE = "/scripts/redump_targets.txt"

# Machine-readable verdict per (program, address), appended so a sweep across
# every program aggregates into one table. Verdicts are addresses and class
# names only - no instruction text - so the file is safe to summarise.
VERDICTS_FILE = "/scripts/redump_verdicts.tsv"

MAX_INSNS = 8192

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

_verdicts = []


def verdict(addr_str, cls, detail):
    print("[{}] {} {}: {}".format(cls, prog_name, addr_str, detail))
    _verdicts.append("{}\t{}\t{}\t{}".format(prog_name, addr_str, cls, detail))


def flush_verdicts():
    if not _verdicts:
        return
    fh = open(VERDICTS_FILE, "a")
    try:
        for line in _verdicts:
            fh.write(line + "\n")
    finally:
        fh.close()


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def word_at(addr):
    try:
        return mem.getInt(addr) & 0xFFFFFFFF
    except Exception:
        return None


def forward_target(raw, cur_pc):
    """The forward branch/jump target of this word, or None.

    Only forward edges matter: a backward branch is a loop inside the body and
    says nothing about where the body ends. `jal` is deliberately excluded - a
    call returns, so it does not extend the body.
    """
    op = (raw >> 26) & 0x3F
    if op == 0x02:  # j
        return ((cur_pc + 4) & 0xF0000000) | ((raw & 0x03FFFFFF) << 2)
    if op == 0x01 or 0x04 <= op <= 0x07:  # regimm, beq/bne/blez/bgtz
        imm = raw & 0xFFFF
        if imm & 0x8000:
            imm -= 0x10000
        tgt = cur_pc + 4 + (imm << 2)
        return tgt if tgt > cur_pc else None
    return None


JR_RA = 0x03E00008


def walk_body(entry):
    """Walk from `entry` to the real end of the routine.

    Returns `(stop_addr_exclusive, n_instructions, status)`. `status` is
    `"complete"` or a string naming why the walk is a LOWER BOUND. A caller
    must never dump a non-complete walk silently.
    """
    start = entry.getOffset()
    frontier = start
    for i in range(MAX_INSNS):
        cur = entry.add(4 * i)
        raw = word_at(cur)
        if raw is None:
            return None, i, "memory ended before a function exit"

        tgt = forward_target(raw, cur.getOffset())
        if tgt is not None and start <= tgt < start + MAX_INSNS * 4:
            if tgt > frontier:
                frontier = tgt

        past_frontier = cur.getOffset() >= frontier
        if raw == JR_RA and past_frontier:
            return cur.add(8), i + 2, "complete"
        if ((raw >> 26) & 0x3F) == 0x02 and past_frontier:
            target = ((cur.getOffset() + 4) & 0xF0000000) | ((raw & 0x03FFFFFF) << 2)
            if target < start or target > cur.getOffset():
                return cur.add(8), i + 2, "complete"
    return None, MAX_INSNS, "no exit within {} instructions".format(MAX_INSNS)


def dump(func, extent_note, requested=None):
    entry_off = func.getEntryPoint().getOffset()
    addr_str = "%08x" % entry_off
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    path = out_path_for(addr_str)
    fh = open(path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, addr_str, prog_name))
        fh.write("size={} bytes, {} instructions\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("extent={}\n".format(extent_note))
        if requested is not None and requested != entry_off:
            # Preserve the citation that pointed at the interior address. The
            # file is named for the entry that exists; this line is where the
            # request that did not went.
            fh.write("requested=%08x (INTERIOR of this body - not an entry "
                     "point)\n" % requested)
        fh.write("\n--- DISASSEMBLY ---\n")
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
    return path, addr_str, len(instrs)


def repair(addr_str, force_rebuild, audit_only):
    entry = af.getAddress(addr_str)
    if entry is None or not in_program(entry):
        return

    requested = entry.getOffset()
    at = fm.getFunctionAt(entry)
    containing = fm.getFunctionContaining(entry)

    # --- interior request -------------------------------------------------
    # The requested address is inside a body but is not its entry. Re-dumping
    # "it" would produce the ENCLOSING function under the requested address's
    # name - the exact defect that puts phantom entry points into citations.
    if at is None and containing is not None:
        owner = containing.getEntryPoint().getOffset()
        verdict(addr_str, "INTERIOR",
                "inside %s at %08x (delta %+d); not an entry point"
                % (containing.getName(), owner, owner - requested))
        if audit_only:
            return
        # Re-dump the enclosing body under ITS OWN name, and record the
        # interior request inside it so the citation is traceable.
        path, got, n = dump(containing, "ghidra function body", requested)
        print("  enclosing body written as {} ({} instrs)".format(path, n))
        return

    if at is None and containing is None:
        stop, n, status = walk_body(entry)
        if status != "complete":
            verdict(addr_str, "NOFUNC_UNWALKABLE",
                    "no function, and the walk did not reach an exit: %s" % status)
            return
        verdict(addr_str, "NOFUNC_WALKABLE",
                "no function; walk reaches an exit after %d instrs" % n)
        if audit_only:
            return
    else:
        before_bytes = at.getBody().getNumAddresses()
        if not force_rebuild:
            verdict(addr_str, "ENTRY",
                    "ghidra body %d B; re-dumped unchanged" % before_bytes)
            if audit_only:
                return
            path, got, n = dump(at, "ghidra function body", requested)
            print("  wrote {} ({} instrs)".format(path, n))
            return
        stop, n, status = walk_body(entry)
        if status != "complete":
            # Loud, and no dump. A body walked to a lower bound reads exactly
            # like a whole one once it is written to a file.
            verdict(addr_str, "WALK_INCOMPLETE",
                    "ghidra body %d B; walk did NOT reach an exit: %s "
                    "(no dump written)" % (before_bytes, status))
            return
        want_bytes = stop.subtract(entry)
        if before_bytes >= want_bytes:
            verdict(addr_str, "ALREADY_WHOLE",
                    "ghidra body %d B >= walked %d B" % (before_bytes, want_bytes))
            if audit_only:
                return
            path, got, n = dump(at, "ghidra function body", requested)
            print("  wrote {} ({} instrs)".format(path, n))
            return
        verdict(addr_str, "TRUNCATED",
                "ghidra body %d B, walked %d B (%d instrs) - rebuilding"
                % (before_bytes, want_bytes, n))
        if audit_only:
            return

    # --- rebuild over the walked span ------------------------------------
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
    if fm.getFunctionAt(entry) is not None:
        fm.removeFunction(entry)

    DisassembleCommand(AddressSet(entry, stop.subtract(1)), None, True).applyTo(
        prog, monitor)
    CreateFunctionCmd(None, entry, AddressSet(entry, stop.subtract(1)),
                      SourceType.USER_DEFINED).applyTo(prog, monitor)
    func = fm.getFunctionAt(entry)
    if func is None:
        verdict(addr_str, "REBUILD_FAILED", "CreateFunctionCmd produced no function")
        return
    got_bytes = func.getBody().getNumAddresses()
    path, got, n = dump(func, "jr-ra/j walk with frontier rule, body rebuilt",
                        requested)
    print("  rebuilt {} B, wrote {} ({} instrs)".format(got_bytes, path, n))


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
    t = t.strip()
    force = t.endswith("!")
    audit = t.endswith("?")
    repair(t.rstrip("!?").strip(), force, audit)

flush_verdicts()
print("done [{}]".format(prog_name))
