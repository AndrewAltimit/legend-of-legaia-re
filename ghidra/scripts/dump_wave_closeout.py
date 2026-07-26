# @category Legaia
# @runtime Jython
#
# Closes the `--missing-dumps` worklist that `scripts/ci/port-catalog.py`
# reports: addresses some existing dump CITES but that have no dump of their
# own. Most of that worklist is not undumped code - it is citation artifacts -
# so this script separates the four outcomes instead of dumping blindly.
#
#   "entry"  the address really is a function entry. Dump it the normal way,
#            named from getEntryPoint(), with the decompiled C.
#
#   "label"  the address is an intra-body jump label, a shared tail, or the
#            interior of a routine. Ghidra renders such a target in the
#            enclosing function's C as `func_0x<addr>()` / `FUN_<addr>()`, and
#            THAT rendering is what the catalog counted as a citation - the
#            disassembly only ever shows `j`/`beq`, which the citation regex
#            does not match. Emit a window report that says so: the enclosing
#            entry, the in-body sites that branch here, and the disassembly
#            from the label to the end of the body. Deliberately NO decompiled
#            C - emitting it would mint the next generation of `func_0x`
#            citations out of the same artifact.
#
#   "probe"  the address is suspected not to be code at all. Report what is
#            actually mapped there and dump nothing.
#
#   "data"   a dump already exists but was manufactured over data by a repair
#            pass that walks instructions without ever asking whether the bytes
#            are code. Rewrite it as a hexdump under a header that says so.
#            Every fabricated word in such a body is a citation the catalog
#            then treats as real work, so the repair has to remove the body,
#            not annotate it.
#
# A `label` report is named after the REQUESTED address, unlike
# dump_pending_helpers.py / repair_truncated_dumps.py, which name output from
# getEntryPoint() precisely so an interior request cannot assert a false entry.
# The distinction is the header: those scripts write a file whose first line
# claims `FUN_<addr> (entry=<addr>)`. This one opens
#
#   == citation pointer 0x<addr> (NOT A FUNCTION ENTRY) [<program>] ==
#   Mid-function citation. Enclosing function dumped as <stem>.txt
#
# which is the corpus's existing form for a file that exists to resolve a
# citation, and which scripts/ghidra-analysis/classify-worklist.py already reads
# as INTERIOR. The filename exists so the citation resolves to the finding
# rather than to nothing; nothing in the file asserts an entry point.
#
# Per-target program allowlist, and it matters. Several of these VAs are
# cited from a MIS-BASED dump, where the printed address column is offset from
# the VA the bytes occupy but the `j`/`jal` targets - absolute in the
# encoding - are correct (see docs/tooling/call-target-integrity.md). Such a
# citation names a VA in the image the BYTES live in, not in the image the
# citing dump was printed under, and dumping it from the printing image yields
# unrelated code that reads as a confident answer.
#
# Invocation, once per program named in TARGETS:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process <prog> -noanalysis \
#       -postScript /scripts/dump_wave_closeout.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

# (address, kind, [program-name prefixes this address is meaningful in])
TARGETS = [
    # Cited twice by `jal` from FUN_8006d4f4 and once from its C. Sits one
    # routine past FUN_8006ED34, the Timer-2 timeout arm; expected REAL.
    ("8006ed50", "entry", ["SCUS_942.54"]),

    # Cited only as `func_0x801e28c4()` in the C of the field/event VM
    # FUN_801DE840; the disassembly reaches it by `j` from inside that body.
    ("801e28c4", "label", ["overlay_0897"]),

    # Same shape inside the tile-board walk SM FUN_801EF2B0.
    ("801efea0", "label", ["overlay_0897"]),

    # Cited as `FUN_801ed2d4()` from FUN_801ECD0C, whose Ghidra body stops
    # short of it.
    ("801ed2d4", "label", ["overlay_0897"]),

    # Cited as `func_0x801dab7c()` from the dump printed as
    # `overlay_0897_801ff930`, which is mis-based: its bytes live in the
    # battle-action image, so the target VA is meaningful THERE only.
    ("801dab7c", "label", ["overlay_battle_action"]),

    # Cited by a lone `jal 0x80040000` inside a body that a repair pass
    # manufactured over PROT 0900's slot-B jump-table head window. Round to
    # 256 KB, which no linker output lands a function on by accident.
    ("80040000", "probe", ["SCUS_942.54"]),

    # The manufactured body itself. Its span runs from PROT 0900's slot-B link
    # base to the next real routine, and its "disassembly" prints 2-byte-
    # aligned addresses and MIPS-II / MIPS-64 mnemonics the R3000A does not
    # implement - both impossible for real PSX code.
    ("801f69ec", "data", ["overlay_muscle_dome"]),
]

OUT_DIR = "/scripts/funcs"
MAX_WINDOW_INSTRS = 400

# Hexdump window for the "data" kind: PROT 0900's head, from the slot-B link
# base to the first real routine above it.
DATA_WINDOW = {"801f69ec": ("801f69d8", "801f7088")}

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


def prog_label():
    """Stable file-name label for the current program.

    Strips a Ghidra duplicate-import suffix (`.bin.0`) so a re-imported
    program keeps writing the same label the corpus already uses.
    """
    label = prog_name
    while label and label.split(".")[-1].isdigit():
        label = label.rsplit(".", 1)[0]
    return label.replace(".bin", "").replace(".", "_")


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    return os.path.join(OUT_DIR, prog_label() + "_" + addr_str + ".txt")


def wanted_here(programs):
    for p in programs:
        if prog_name.startswith(p):
            return True
    return False


def in_program(addr):
    return mem.getBlock(addr) is not None


def reachers_of(func, addr):
    """Every instruction inside `func` whose flow can reach `addr`.

    Read off the instruction's own flow targets rather than the reference
    manager, so a `j`/`b` label - which Ghidra does not record as a call -
    still shows up.
    """
    hits = []
    needle = "0x%08x" % addr.getOffset()
    for ins in listing.getInstructions(func.getBody(), True):
        found = False
        for flow in ins.getFlows():
            if flow.equals(addr):
                found = True
                break
        # Where Ghidra minted a function at the target it re-types the `j` as
        # a call, and a call destination is not a flow. The printed operand is
        # the same either way, so match it too.
        if not found and needle in ins.toString().lower():
            found = True
        if found:
            hits.append(ins)
    return hits


def dump_entry(addr_str):
    addr = af.getAddress(addr_str)
    func = fm.getFunctionAt(addr)
    if func is None:
        holder = fm.getFunctionContaining(addr)
        if holder is None:
            print("[no-fn] {} in {}".format(addr_str, prog_name))
        else:
            print("[interior] {} is inside {} - reclassify as label"
                  .format(addr_str, holder.getEntryPoint()))
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    path = out_path_for(addr_str)
    fh = open(path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, addr_str, prog_name))
        fh.write("size={} bytes, {} instructions\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("extent=ghidra function body\n")
        fh.write("\n--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        res = decomp.decompileFunction(func, 60, monitor)
        if res.decompileCompleted():
            fh.write(res.getDecompiledFunction().getC())
        else:
            fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
    finally:
        fh.close()
    print("wrote {}".format(path))


def builds_a_frame(func):
    """Does this body allocate its own stack frame in its first few words?

    The one test that separates a real entry from a tail fragment Ghidra
    minted at a jump label. A fragment restores callee-saved registers and
    returns through `jr ra`, so `jr ra` alone proves nothing - see
    docs/tooling/worklist-classification.md. Only the negative allocation is
    decisive, and it is read out of the disassembly rather than the C.
    """
    n = 0
    for ins in listing.getInstructions(func.getBody(), True):
        if n >= 6:
            break
        n += 1
        text = ins.toString().replace(" ", "").lower()
        if text.startswith("addiusp,sp,-"):
            return True
    return False


def resolve_parent(addr):
    """The routine this VA is really interior to, and how that was decided.

    `getFunctionContaining()` is not enough on its own. Where Ghidra minted an
    entry AT the label it also cut the real routine's body there, so the
    containing function is the fragment itself and the parent is the function
    immediately below - confirmed by requiring that the parent actually
    branches here.
    """
    exact = fm.getFunctionAt(addr)
    if exact is not None and not builds_a_frame(exact):
        # Walk back word by word rather than asking for "the function before".
        # A cut body leaves a gap between the parent's last word and the
        # minted entry, so the parent is not adjacent and a single step back
        # lands in unclaimed space.
        prev = None
        for step in range(1, 513):
            cand = fm.getFunctionContaining(addr.subtract(step * 4))
            if cand is not None and cand.getEntryPoint().compareTo(addr) < 0:
                prev = cand
                break
        if prev is not None and reachers_of(prev, addr):
            return prev, "ghidra minted an entry at this label and cut the " \
                         "real body here; parent confirmed by its branches"
    holder = fm.getFunctionContaining(addr)
    if holder is not None:
        return holder, "enclosing analyzed function"
    return None, "no analyzed function contains this VA"


def dump_label(addr_str):
    addr = af.getAddress(addr_str)
    exact = fm.getFunctionAt(addr)
    ins_at = listing.getInstructionAt(addr)
    parent, how = resolve_parent(addr)

    path = out_path_for(addr_str)
    fh = open(path, "w")
    try:
        # `citation pointer` is the corpus's existing header form for a file
        # that exists to resolve a citation rather than to assert an entry;
        # scripts/ghidra-analysis/classify-worklist.py reads it, plus the
        # "Enclosing function dumped as" line, and classes the VA INTERIOR.
        fh.write("== citation pointer 0x{} (NOT A FUNCTION ENTRY) [{}] ==\n"
                 .format(addr_str, prog_name))
        if parent is not None:
            parent_str = "%08x" % parent.getEntryPoint().getOffset()
            stem = parent_str if prog_name.startswith("SCUS") \
                else prog_label() + "_" + parent_str
            fh.write("Mid-function citation. Enclosing function dumped as "
                     "{}.txt\n".format(stem))
            pb = parent.getBody()
            fh.write("enclosing={} entry={} size={} bytes span={}..{}\n".format(
                parent.getName(), parent_str, pb.getNumAddresses(),
                pb.getMinAddress(), pb.getMaxAddress()))
            fh.write("parent_resolved_by={}\n".format(how))
        else:
            fh.write("enclosing=none ({})\n".format(how))
        if exact is not None:
            fh.write("ghidra_mints_an_entry_here=yes ({}) - a minted entry is "
                     "not evidence of one\n".format(exact.getName()))
            fh.write("builds_own_frame={} (no => tail fragment; the frame it "
                     "unwinds was built by the parent above)\n".format(
                         "yes" if builds_a_frame(exact) else "no"))
        fh.write("instruction_at_va={}\n".format(
            "yes" if ins_at is not None else "no"))
        fh.write("no_decompiled_c=by design; the C rendering of a jump label "
                 "as a call is what put this VA on the worklist\n")

        fh.write("\n--- IN-BODY SITES THAT BRANCH HERE ---\n")
        if parent is not None:
            hits = reachers_of(parent, addr)
            if not hits:
                fh.write("(none inside the enclosing body)\n")
            for ins in hits:
                fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        else:
            fh.write("(no enclosing body to scan)\n")

        fh.write("\n--- WINDOW FROM THIS VA ---\n")
        cur = ins_at
        count = 0
        limit = None
        if parent is not None:
            limit = parent.getBody().getMaxAddress()
        if exact is not None:
            end = exact.getBody().getMaxAddress()
            if limit is None or end.compareTo(limit) > 0:
                limit = end
        while cur is not None and count < MAX_WINDOW_INSTRS:
            fh.write("{}  {}\n".format(cur.getAddress(), cur.toString()))
            count += 1
            if limit is not None and cur.getAddress().compareTo(limit) >= 0:
                break
            cur = listing.getInstructionAfter(cur.getAddress())
        if count == 0:
            fh.write("(no instruction decoded at this VA)\n")
    finally:
        fh.close()
    print("wrote {}".format(path))


def dump_data_window(addr_str):
    lo_str, hi_str = DATA_WINDOW[addr_str]
    lo = af.getAddress(lo_str)
    hi = af.getAddress(hi_str)
    path = out_path_for(addr_str)
    fh = open(path, "w")
    try:
        fh.write("== DATA WINDOW {} (NOT CODE) [{}] ==\n".format(
            addr_str, prog_name))
        fh.write("window={}..{}\n".format(lo_str, hi_str))
        fh.write("reading=module head / link-base table. This VA previously "
                 "carried a disassembled body that a frontier-walking repair "
                 "pass manufactured over these bytes; its printed addresses "
                 "were 2-byte aligned and it decoded MIPS-II and MIPS-64 "
                 "mnemonics, neither of which the R3000A implements.\n")
        fh.write("consequence=every word of a fabricated body reads as a call "
                 "target. One word of this table decoded as a jal whose "
                 "operand printed 0x80040000, and that print is the whole "
                 "reason 0x80040000 entered the cited-but-not-dumped "
                 "worklist. Spelling the pair out again here would re-create "
                 "the citation, so it is not spelt out.\n")
        fh.write("see=docs/tooling/dump-corpus-integrity.md, "
                 "docs/tooling/worklist-classification.md\n")
        fh.write("\n--- HEXDUMP ---\n")
        cur = lo
        while cur.compareTo(hi) < 0:
            words = []
            for i in range(4):
                a = cur.add(i * 4)
                if a.compareTo(hi) >= 0:
                    break
                try:
                    words.append("%08x" % (mem.getInt(a) & 0xFFFFFFFF))
                except Exception:
                    words.append("--------")
            fh.write("{}  {}\n".format(cur, " ".join(words)))
            cur = cur.add(16)
    finally:
        fh.close()
    print("wrote {}".format(path))


def probe(addr_str):
    addr = af.getAddress(addr_str)
    block = mem.getBlock(addr)
    func_at = fm.getFunctionAt(addr)
    holder = fm.getFunctionContaining(addr)
    ins_at = listing.getInstructionAt(addr)
    print("[probe] {} in {}: block={} func_at={} containing={} instr={}"
          .format(addr_str, prog_name,
                  block.getName() if block is not None else "UNMAPPED",
                  func_at, holder, ins_at))
    if block is not None:
        try:
            word = mem.getInt(addr)
            print("[probe] {} first word = 0x{:08x}".format(
                addr_str, word & 0xFFFFFFFF))
        except Exception as e:
            print("[probe] {} unreadable: {}".format(addr_str, e))


for addr_str, kind, programs in TARGETS:
    if not wanted_here(programs):
        continue
    a = af.getAddress(addr_str)
    if a is None or not in_program(a):
        print("[skip] {} not mapped in {}".format(addr_str, prog_name))
        continue
    if kind == "entry":
        dump_entry(addr_str)
    elif kind == "label":
        dump_label(addr_str)
    elif kind == "data":
        dump_data_window(addr_str)
    else:
        probe(addr_str)

print("done [{}]".format(prog_name))
