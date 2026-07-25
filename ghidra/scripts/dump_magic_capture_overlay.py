# @category Legaia
# @runtime Jython
#
# Dumps all 78 functions from the magic_capture overlay
# (overlay_magic_capture.bin, mednafen save state mc9).
#
# The magic_capture overlay hosts the Ra-Seru capture mechanic (grabbing
# Gimard and other Ra-Serus). Key candidates:
#   - 801e295c (16396 bytes, 19 outgoing) -- capture battle state machine
#   - 801d0748 (11124 bytes, 26 outgoing) -- capture outer dispatcher
#   - 801ec3e4 (10008 bytes)              -- large helper
#   - 801e9fd4 (8456 bytes)               -- capture sub-system
#   - 801d388c (7820 bytes, 39 incoming)  -- heavily used animation/render helper
#   - 801d5854 (6500 bytes, 47 incoming)  -- central state tick helper
#   - 801d8de8 (3028 bytes, 75 incoming)  -- central dispatcher (same offset as
#                                            battle_action FUN_801D8DE8 JT dispatcher)
#
# Run:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_magic_capture.bin -noanalysis \
#       -postScript /scripts/dump_magic_capture_overlay.py
#
# Output: /scripts/funcs/overlay_magic_capture_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # Sorted by incoming xref count (most-called helpers first)
    "801d8de8",  # 75 incoming -- central dispatcher
    "801d5854",  # 47 incoming -- central state tick helper (6500 bytes)
    "801d388c",  # 39 incoming -- animation/render helper (7820 bytes)
    "801d829c",  # 24 incoming
    "801d5718",  # 18 incoming
    "801dc0a0",  # 7 incoming (3596 bytes)
    "801d5778",  # ? incoming
    "801d57e8",  # ? incoming
    # Large entry points (0 incoming = top-level callers from game mode dispatcher)
    "801e295c",  # 0 incoming, 16396 bytes -- capture battle SM
    "801d0748",  # 0 incoming, 11124 bytes -- capture outer dispatcher
    "801ec3e4",  # 0 incoming, 10008 bytes
    "801e9fd4",  # 1 incoming, 8456 bytes
    "801e805c",  # 1 incoming, 4492 bytes
    "801e09f8",  # 0 incoming, 4280 bytes
    "801d71b8",  # 1 incoming, 4324 bytes
    # Medium functions by size
    "801d8de8",  # already above
    "801eed1c",  # 2 incoming, 3272 bytes
    "801dea50",  # 0 incoming, 2848 bytes
    "801e9504",  # 0 incoming, 2768 bytes
    "801e791c",  # 1 incoming, 1856 bytes
    "801df6b8",  # 0 incoming, 1848 bytes
    "801d9d3c",  # 1 incoming, 1552 bytes
    "801daba4",  # 3 incoming, 1408 bytes
    "801e1d98",  # 3 incoming, 1328 bytes
    "801db318",  # 1 incoming, 1176 bytes
    "801da780",  # 2 incoming, 1060 bytes
    "801e6968",  # 1 incoming, 1052 bytes
    "801d84c0",  # 1 incoming, 1036 bytes
    "801dd0ac",  # 1 incoming, 1028 bytes
    "801e6d84",  # 1 incoming, 824 bytes
    "801e2650",  # 4 incoming, 780 bytes
    "801ec0dc",  # 1 incoming, 776 bytes
    "801d3444",  # 1 incoming, 772 bytes
    "801e22c8",
    "801e2524",
    "801d88cc",  # 2 incoming, 444 bytes
    "801d8a88",  # 6 incoming, 632 bytes
    "801d8d00",  # 9 incoming, 232 bytes
    "801d99bc",  # 8 incoming, 300 bytes
    "801d9ae8",  # 1 incoming, 212 bytes
    "801d9bbc",  # 1 incoming, 384 bytes
    "801ddb30",  # 3 incoming, 3556 bytes
    "801dd864",  # ? incoming
    "801dd4b0",  # 0 incoming, 516 bytes
    "801dd6b4",  # 0 incoming, 432 bytes
    "801df570",  # 2 incoming
    "801dfdf0",  # 3 incoming
    "801e1ab0",  # 3 incoming, 1328 bytes
    "801e70bc",
    "801e7250",
    "801e7320",
    "801e752c",
    "801e7824",
    "801e91e8",  # called by 801ec3e4
    "801e92dc",
    "801e93c8",
    "801ef9e4",  # called by 801eed1c
    "801efbfc",  # called by 801eed1c
    "801efe44",
    # Remaining by entry address
    "801d32bc",
    "801d3748",
    "801da34c",
    "801da59c",
    "801da6b4",
    "801db124",
    "801db7b0",  # called by 801d8de8 (only callee)
    "801db81c",
    "801db8b4",
    "801db8f4",
    "801db9c4",
    "801dba04",
    "801dba90",
    "801dbb8c",
    "801dbc30",
    "801dbd04",
    "801dbddc",
    "801dbec4",
    "801dbf9c",
    "801dceac",
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
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    block = mem.getBlock(addr)
    return block is not None


def dump(addr_str):
    """Dump the function that CONTAINS `addr_str`, named for its own entry.

    Three defects this function used to carry, each of which made a complete
    and correct dump unusable to something downstream:

    * It resolved with `getFunctionContaining()` and then named the file and
      the `entry=` header after the address it REQUESTED. Ask for an interior
      address and you get `<interior>.txt` holding the enclosing function -
      a file asserting an entry point that does not exist, which every
      citation of that filename then inherits.
    * It wrote `ins` without `ins.getAddress()`, so the disassembly carried no
      address column and every address-keyed instrument read the file as
      empty - a whole 2781-instruction body reading as no evidence at all.
    * It emitted `--- DECOMPILED C ---`, which readers expecting the standard
      `--- DECOMPILED ---` marker parse as more disassembly.
    """
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionAt(addr) or fm.getFunctionContaining(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    entry_str = "%08x" % func.getEntryPoint().getOffset()
    if entry_str != addr_str.lower():
        print("[interior] requested {} is inside {} at {}".format(
            addr_str, func.getName(), entry_str))

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    instr_count = len(instrs)

    result = decomp.decompileFunction(func, 60, monitor)
    c_code = ""
    if result and result.decompiledFunction:
        c_code = result.decompiledFunction.getC()

    out = out_path_for(entry_str)
    with open(out, "w") as f:
        f.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), entry_str, entry_str, prog_name))
        f.write("size={} bytes, {} instructions\n".format(
            body.getNumAddresses(), instr_count))
        if entry_str != addr_str.lower():
            f.write("requested=%s (INTERIOR of this body - not an entry "
                    "point)\n" % addr_str.lower())
        f.write("\n--- DISASSEMBLY ---\n")
        for ins in instrs:
            f.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        if c_code:
            f.write("\n--- DECOMPILED ---\n")
            f.write(c_code)
    print("[done] {} -> {}".format(entry_str, out))


seen = set()
for addr in TARGETS:
    if addr not in seen:
        seen.add(addr)
        dump(addr)
print("magic_capture dump complete: {} targets".format(len(seen)))
