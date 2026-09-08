# @category Legaia
# @runtime Jython
#
# Dumper for the SLOT-A half of the bytes-derived code-gap worklist that
# `scripts/ci/disc-coverage.py` emits (`target/disc-coverage/dump-worklist.md`).
# Sibling of `dump_scus_gaps.py`, which does the same job for SCUS's text head;
# this one covers the statically extracted per-PROT-entry overlay images.
#
# RANGES is keyed by PROGRAM label, not by address. Nineteen slot-A overlays
# load at 0x801CE818 and hold DIFFERENT bytes at the same VA, so a bare address
# list would force-disassemble one image's gap inside another image's bytes and
# print convincing garbage - the same trap `dump_static_overlay.py` documents
# for the slot-B band.
#
# Each range is walked for function entries Ghidra already found; bytes with no
# function are reported and can be picked up by adding the range to
# FORCE_RANGES, which disassembles the run and cuts a function at each
# `jr ra` + delay-slot boundary. Only ranges word-checked as MIPS against the
# extracted image belong there - forcing data produces convincing garbage.
#
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_field_0897.bin -noanalysis \
#       -postScript /scripts/dump_slot_a_gaps.py

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.program.model.address import AddressSet
from ghidra.util.task import ConsoleTaskMonitor

# {program label: [(start, end exclusive), ...]}, from the worklist's
# "un-dumped code runs" table for each image.
RANGES = {
    "overlay_field_0897": [
        ("801d820c", "801d8258"), ("801dda90", "801ddb30"),
        ("801ddb44", "801ddc20"), ("801de37c", "801de3e0"),
        ("801e59b0", "801e5a08"), ("801f1f4c", "801f1fdc"),
    ],
    "overlay_battle_action_0898": [
        ("801d32bc", "801d3444"), ("801daba4", "801db124"),
        ("801dbb2c", "801dbb8c"), ("801eeafc", "801eed1c"),
        ("801f2d54", "801f2e10"), ("801f44a0", "801f46a0"),
    ],
    "overlay_cutscene_str_0970": [
        ("801cf02c", "801cf098"), ("801cfc50", "801cfcdc"),
        ("801d0100", "801d0230"), ("801d0e94", "801d0fe4"),
        ("801d0ff4", "801d12f4"), ("801d13f4", "801d1744"),
        ("801d1758", "801d1854"), ("801d1878", "801d1978"),
    ],
    "overlay_dance_0980": [
        ("801d05b8", "801d0640"), ("801d43a4", "801d46a4"),
        ("801d4aa4", "801d4da4"), ("801d4ea4", "801d50a4"),
        ("801d60a4", "801d6300"),
    ],
    "overlay_arena_init_0977": [
        ("801ceac4", "801ceb08"), ("801cef6c", "801cf00c"),
        ("801cf014", "801cf074"), ("801cff44", "801cffdc"),
        ("801cffdc", "801d0028"), ("801d0028", "801d00f8"),
        ("801d0344", "801d042c"), ("801d0ed8", "801d0f60"),
        ("801d1ef0", "801d2018"),
    ],
    "overlay_gameover_0902": [
        ("801ced44", "801cef54"), ("801cef6c", "801cf00c"),
    ],
    "overlay_field_battle_intro_0979": [
        ("801d2784", "801d2818"),
    ],
    "overlay_other3_dev_0974": [
        ("801d1978", "801d1a90"),
    ],
}

# Runs the walk reports as un-attributed: bytes Ghidra never disassembled, so
# no function exists to dump. Same keying discipline as RANGES.
#
# Every range here was word-checked as MIPS against the extracted image before
# being forced (prologue / `jr ra` / RAM-page `lui` / in-range `jal` scan) -
# forcing data produces convincing garbage. The runs the same scan showed to be
# DATA are deliberately absent, and named in `docs/tooling/disc-coverage.md`:
# the cutscene overlay's MDEC decode tables (0x801D0E94..0x801D199C, opening
# with the `0x1F801824` / `0x1F8010F0` hardware-port words) and the dance
# overlay's step-chart records (0x801D43A4, 0x801D4AA4, 0x801D4EA4).
FORCE_RANGES = {
    # One 1272-byte frameless routine between FUN_80045988's end and
    # FUN_800460AC. It ends in `j 0x80045E54`, not `jr ra`, which is why the
    # gap classifier reported it as two runs with a hole between them.
    "SCUS_942_54": [("80045bb4", "800460ac")],
    "overlay_field_0897": [
        ("801d820c", "801d8258"), ("801dda90", "801ddb30"),
        ("801ddb44", "801ddc20"), ("801de37c", "801de3e0"),
        ("801e59b0", "801e5a08"), ("801f1f4c", "801f1fdc"),
    ],
    "overlay_battle_action_0898": [
        ("801d32bc", "801d3444"), ("801daba4", "801db124"),
        ("801dbb2c", "801dbb8c"), ("801eeafc", "801eed1c"),
        # 0x801EEAFC is a two-instruction `j 0x801EEB60` thunk, so the range
        # above closes on the thunk and the body it jumps to needs its own.
        ("801eeb60", "801eed1c"),
        ("801f2d54", "801f2e10"), ("801f44a0", "801f46a0"),
    ],
    "overlay_cutscene_str_0970": [
        ("801cf02c", "801cf098"), ("801cfc50", "801cfcdc"),
        ("801d0100", "801d0230"),
    ],
    "overlay_dance_0980": [
        ("801d05b8", "801d0640"), ("801d60a4", "801d6300"),
    ],
    "overlay_arena_init_0977": [
        ("801ceac4", "801ceb08"), ("801cef6c", "801cf074"),
        ("801cff44", "801d00f8"), ("801d0344", "801d042c"),
        ("801d0ed8", "801d0f60"), ("801d1ef0", "801d2018"),
    ],
    # PROT 0902's tail is a body the entry boundary cut short: the routine at
    # 0x801CED68 runs to the last word of the 2048-byte entry with no `jr ra`.
    # 0x801CED44..0x801CED68 ahead of it is a coordinate table.
    "overlay_gameover_0902": [
        ("801ced68", "801cef54"), ("801cef6c", "801cf018"),
    ],
    "overlay_field_battle_intro_0979": [("801d2784", "801d2818")],
    "overlay_other3_dev_0974": [("801d1978", "801d1a90")],
}

LIST_ONLY = os.environ.get("LIST_ONLY", "") == "1"

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
prog_label = prog_name.replace(".bin", "").replace(".", "_")
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
    # SCUS dumps are named by bare address, the convention the whole corpus and
    # every sweep over it already uses; overlay dumps carry the program label
    # because nineteen of them share a VA space.
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    return os.path.join(OUT_DIR, prog_label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


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
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    return out_path


def walk_range(start_str, end_str):
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if start is None or end is None:
        print("[skip] bad range {}..{}".format(start_str, end_str))
        return
    if not in_program(start):
        print("[skip] {} not in {}".format(start_str, prog_name))
        return

    print("=== range {}..{} ===".format(start_str, end_str))
    seen = []
    holes = []
    addr = start
    while addr.compareTo(end) < 0:
        func = fm.getFunctionAt(addr)
        if func is not None:
            seen.append(func)
            print("  func {} {} size={}".format(
                func.getEntryPoint(), func.getName(),
                func.getBody().getNumAddresses()))
            nxt = func.getBody().getMaxAddress().add(1)
            if nxt.compareTo(addr) <= 0:
                nxt = addr.add(4)
            addr = nxt
            continue
        cu = listing.getCodeUnitAt(addr)
        if cu is None:
            holes.append(addr)
            addr = addr.add(4)
            continue
        if fm.getFunctionContaining(addr) is None:
            holes.append(addr)
        addr = addr.add(cu.getLength())

    if holes:
        print("  {} un-attributed word(s), first={} last={}".format(
            len(holes), holes[0], holes[-1]))

    if not LIST_ONLY:
        for func in seen:
            print("  wrote {}".format(dump_function(func)))


def force_range(start_str, end_str):
    """Disassemble an un-analyzed run and create a function per `jr ra` unit."""
    start = af.getAddress(start_str)
    end = af.getAddress(end_str)
    if start is None or end is None or not in_program(start):
        return

    print("=== force {}..{} ===".format(start_str, end_str))
    span = AddressSet(start, end.subtract(1))

    cursor = start
    while cursor.compareTo(end) < 0:
        if listing.getInstructionAt(cursor) is None:
            DisassembleCommand(cursor, span, True).applyTo(prog, monitor)
        entry = cursor
        probe = cursor
        jr_at = None
        while probe.compareTo(end) < 0:
            ins = listing.getInstructionAt(probe)
            if ins is None:
                break
            if ins.getMnemonicString().lower() == "jr" and "ra" in ins.toString():
                jr_at = probe
                break
            probe = probe.add(ins.getLength())
        if jr_at is None:
            # A run with no `jr ra` inside it is either data or a body the
            # image's own extent cut short (PROT 0902 / 0977 both end that
            # way). Dump what is there as one function rather than walking
            # off the end looking for a return that is not in this image.
            stop = end
        else:
            stop = jr_at.add(8)

        func = fm.getFunctionAt(entry)
        if func is None:
            CreateFunctionCmd(entry).applyTo(prog, monitor)
            func = fm.getFunctionAt(entry)
        if func is None:
            print("  [skip] no function created at {}".format(entry))
        else:
            print("  func {} -> {}".format(entry, dump_function(func)))
        if stop.compareTo(cursor) <= 0:
            break
        cursor = stop


for rng in RANGES.get(prog_label, []):
    walk_range(rng[0], rng[1])

for rng in FORCE_RANGES.get(prog_label, []):
    force_range(rng[0], rng[1])

print("done [{}]".format(prog_name))
