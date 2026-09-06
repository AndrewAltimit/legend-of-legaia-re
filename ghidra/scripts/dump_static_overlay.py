# @category Legaia
# @runtime Jython
#
# Dumper for the STATICALLY extracted overlay images
# (`extracted/overlays/overlay_<label>_<entry>.bin`, bases in
# `crates/asset/data/static-overlays.toml`), imported one Ghidra program per
# PROT entry so a dump's filename carries the image identity the shared load
# base cannot - see docs/tooling/static-overlay-pipeline.md.
#
# The slot-B summon stagers and capture-class cast modules are why this exists.
# They contain NO internal `jal`: every call leaves for SCUS or the resident
# battle overlay, and the module's own arms are reached through a jump table at
# the image head. So Ghidra's auto-analysis finds at most the tail function and
# leaves the module's real entry - the one the pager jumps to - undisassembled.
#
# The entries are recovered from the bytes instead. Every `addiu sp, sp, -X`
# word in one of these images is a function prologue, every `jr ra` is an exit,
# and in every module the two counts match and interleave: function `i` runs
# from prologue `i` to prologue `i+1`, and the last ends 8 bytes past the last
# `jr ra`. RANGES below records that partition, so each dump covers a whole
# function body rather than the one basic block Ghidra's flow-following reaches
# before the dispatcher's `jr $v0`.
#
# RANGES is keyed by PROGRAM label, not by address: every slot-B image loads at
# 0x801F69D8, so a bare address list would force-disassemble one image's entry
# inside another image's bytes and print convincing garbage.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_summon_ozma_0934.bin -noanalysis \
#       -postScript /scripts/dump_static_overlay.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.address import AddressSet
from ghidra.program.model.symbol import RefType, SourceType
from ghidra.util.task import ConsoleTaskMonitor

# {program label: [(entry VA, end VA exclusive), ...]}.
# Program label = program name minus ".bin". File offsets are into the PROT
# entry (VA - base); the base is 0x801F69D8 for every row here.
RANGES = {
    "overlay_summon_gimard_0903": [("801f69d8", "801f7724")],
    "overlay_summon_stager_x83_0905": [("801f69d8", "801f8078"), ("801f8078", "801f81e4")],
    "overlay_summon_nighto_0907": [("801f69e8", "801f7fa8"), ("801f7fa8", "801f81d0")],
    "overlay_stager_ultimate_rave_0924": [("801f6a18", "801f7820"), ("801f7820", "801f787c")],
    "overlay_summon_juggernaut_0927": [("801f6a84", "801f85a8"), ("801f85a8", "801f8988")],
    "overlay_summon_palma_0928": [("801f69f4", "801f8e68"), ("801f8e68", "801f9208")],
    "overlay_summon_mule_0929": [("801f69fc", "801f8c30"), ("801f8c30", "801f90a4")],
    "overlay_summon_horn_0930": [("801f6a74", "801f7ea4"), ("801f7ea4", "801f857c")],
    "overlay_summon_jedo_0931": [("801f6a58", "801f8adc"), ("801f8adc", "801f8b68")],
    "overlay_summon_meta_0932": [("801f6a34", "801f84a4"), ("801f84a4", "801f84dc")],
    "overlay_summon_terra_0933": [("801f6a30", "801f8748"), ("801f8748", "801f881c")],
    "overlay_summon_ozma_0934": [("801f6a40", "801f92ac"), ("801f92ac", "801f9c08")],
    "overlay_summon_effect_table_0957": [("801f6a14", "801f798c"), ("801f798c", "801f99f4"),
                                         ("801f99f4", "801f9ba8"), ("801f9ba8", "801f9c20")],
    # Ordinary overlays whose routine the walk below cannot bound, because the
    # coverage run it starts from is interior: the prologue is behind the run's
    # start and the `jr ra` is past its end. Bounds recovered the same way -
    # nearest preceding `addiu sp, sp, -X`, first following `jr ra` + delay.
    "overlay_dance_0980": [("801cef54", "801cf470"), ("801d32f8", "801d387c")],
    # PROT 0978 holds exactly one prologue and one `jr ra`: one function over
    # the whole code region. The pre-existing 0x801F6B24 dump prints the same
    # span but reports a 328-byte body, so the coverage credit is short.
    "overlay_field_back_read_0978": [("801f6b24", "801f7358")],
}

# {program label: [(start VA, end VA exclusive), ...]} for images that DO have
# an internal call graph, where Ghidra's analysis already found the functions
# and the gap is only that nothing dumped them. Each range is walked: every
# function entry inside it is dumped, and any run of bytes with no function is
# force-disassembled and split at each `jr ra` + delay-slot pair, the shape a
# sequence of separately-emitted leaves has. Ranges come from
# `scripts/ci/disc-coverage.py`'s un-dumped `code` runs.
WALK_RANGES = {
    "overlay_field_0897": [
        ("801da930", "801daa50"), ("801dd4c4", "801dd9d4"), ("801e5154", "801e5338"),
        ("801f0718", "801f0adc"), ("801f0efc", "801f1138"), ("801f23b4", "801f26b4"),
        ("801f2db4", "801f30c4"), ("801f30d4", "801f32d4"),
    ],
    "overlay_dance_0980": [("801d05b8", "801d0750")],
    "overlay_arena_init_0977": [("801cf20c", "801cf870"), ("801d0cd0", "801d0e78")],
}

# Runs `disc-coverage.py` ranks as `code` that the bytes say are DATA, so no
# dump belongs there. Kept as a list rather than deleted, because the next
# reader of the worklist will otherwise re-walk them. Each was read with
# `scripts/ghidra-analysis/disasm-overlay-fn.py` at the image's own base:
# they decode as `.byte` runs, `nop` fields and impossible operands
# (`j 0x80300000`, `tge`, `syscall`), never as a body reaching a `jr ra`.
#
#   field(897)            0x801F23B4, 0x801F2DB4, 0x801F30D4
#   dance(980)            0x801D43A4, 0x801D4AA4
#   arena_init(977)       0x801D1EF0                (image tail)
#   field_back_read(978)  0x801F7624                (image tail)
#   SCUS_942.54           0x80074F80, 0x80075380, 0x80076880, 0x80076E80,
#                         0x80077480, 0x80078B80, 0x8007A880  (static tables)
#
# SCUS 0x80045CB4 is the one SCUS run that IS code, and it is still not a dump
# target: nothing in any image references it (five-form scan), its preceding
# word is a `sw` in the same instruction stream, and the nearest prologue is
# 11128 bytes back - the INTERIOR class of docs/tooling/worklist-classification.md.
NOT_CODE = ()

# {program label: (dispatcher `jr` VA, jump-table VA, arm count)}. Teaching
# Ghidra the computed jump keeps the decompiler's per-arm structure; without it
# the arms decompile as unreachable code. The arm count is the module's own
# `sltiu` bound, read out of the dispatcher - not a guess about table length.
JUMPTABLES = {
    "overlay_summon_ozma_0934": ("801f6ad0", "801f69d8", 0x1a),
}

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
label = prog_name.replace(".bin", "").replace(".", "_")
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
refs = prog.getReferenceManager()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return addr is not None and mem.getBlock(addr) is not None


def dump_function(func):
    addr_str = "%08x" % func.getEntryPoint().getOffset()
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
            res = decomp.decompileFunction(func, 180, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    return out_path


def add_jumptable():
    spec = JUMPTABLES.get(label)
    if spec is None:
        return
    jr_addr = af.getAddress(spec[0])
    tbl = af.getAddress(spec[1])
    if not in_program(jr_addr) or not in_program(tbl):
        return
    added = 0
    for i in range(spec[2]):
        word = mem.getInt(tbl.add(i * 4)) & 0xFFFFFFFF
        tgt = af.getAddress("%08x" % word)
        if not in_program(tgt):
            continue
        refs.addMemoryReference(jr_addr, tgt, RefType.COMPUTED_JUMP,
                                SourceType.USER_DEFINED, 0)
        added += 1
    print("  jumptable {}: {} arms".format(spec[1], added))


def cover_range(start_str, end_str):
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if not in_program(start) or not in_program(end.subtract(1)):
        return None
    span = AddressSet(start, end.subtract(1))

    # Interior function entries would block setBody; the range partition is the
    # authority on where a body starts, so drop anything inside it but the head.
    for func in list(fm.getFunctions(span, True)):
        if func.getEntryPoint().compareTo(start) != 0:
            fm.removeFunction(func.getEntryPoint())

    listing.clearCodeUnits(start, end.subtract(1), False)
    cursor = start
    while cursor.compareTo(end) < 0:
        if listing.getInstructionAt(cursor) is None:
            DisassembleCommand(cursor, span, True).applyTo(prog, monitor)
        ins = listing.getInstructionAt(cursor)
        cursor = cursor.add(ins.getLength() if ins is not None else 4)

    func = fm.getFunctionAt(start)
    if func is None:
        CreateFunctionCmd(start).applyTo(prog, monitor)
        func = fm.getFunctionAt(start)
    if func is None:
        print("  [warn] no function created at {}".format(start_str))
        return None
    try:
        func.setBody(span)
    except Exception as e:
        print("  [warn] setBody {}..{}: {}".format(start_str, end_str, e))
    return func


def walk_range(start_str, end_str):
    """Dump every function inside a range; force the runs that hold none."""
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if not in_program(start) or not in_program(end.subtract(1)):
        return
    span = AddressSet(start, end.subtract(1))
    print("=== walk {}..{}".format(start_str, end_str))

    cursor = start
    while cursor.compareTo(end) < 0:
        func = fm.getFunctionContaining(cursor)
        if func is not None:
            print("  {} size={} -> {}".format(
                func.getEntryPoint(), func.getBody().getNumAddresses(),
                dump_function(func)))
            nxt = func.getBody().getMaxAddress().add(1)
            cursor = nxt if nxt.compareTo(cursor) > 0 else cursor.add(4)
            continue
        # No function here: force-disassemble and cut at the first `jr ra`.
        if listing.getInstructionAt(cursor) is None:
            DisassembleCommand(cursor, span, True).applyTo(prog, monitor)
        probe, jr_at = cursor, None
        while probe.compareTo(end) < 0:
            ins = listing.getInstructionAt(probe)
            if ins is None:
                break
            if ins.getMnemonicString().lower() == "jr" and "ra" in ins.toString():
                jr_at = probe
                break
            probe = probe.add(ins.getLength())
        if jr_at is None:
            print("  no `jr ra` from {} - stopping this range".format(cursor))
            return
        CreateFunctionCmd(cursor).applyTo(prog, monitor)
        made = fm.getFunctionAt(cursor)
        if made is None:
            print("  [warn] no function created at {}".format(cursor))
            cursor = jr_at.add(8)
            continue
        print("  {} size={} -> {} (forced)".format(
            cursor, made.getBody().getNumAddresses(), dump_function(made)))
        nxt = made.getBody().getMaxAddress().add(1)
        cursor = nxt if nxt.compareTo(cursor) > 0 else jr_at.add(8)


for rng in RANGES.get(label, []):
    func = cover_range(rng[0], rng[1])
    if func is None:
        continue
    add_jumptable()
    print("  {} size={} -> {}".format(
        rng[0], func.getBody().getNumAddresses(), dump_function(func)))

for rng in WALK_RANGES.get(label, []):
    walk_range(rng[0], rng[1])

print("done [{}]".format(prog_name))
