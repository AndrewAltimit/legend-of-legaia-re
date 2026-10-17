# @category Legaia
# @runtime Jython
#
# Catch-all dumper for helpers / dispatcher leaves the rest of the
# pipeline is actively interested in. The TARGETS list rotates as
# different reverse-engineering threads come and go; each entry is
# expected to land in `ghidra/scripts/funcs/<addr>.txt` (SCUS) or
# `<prog_label>_<addr>.txt` (overlay).
#
# `in_program(addr)` makes the script overlay-aware - addresses outside
# the currently loaded program are skipped silently, so a single run
# against each program (SCUS_942.54 + every captured overlay) picks up
# the relevant subset without needing per-program target lists.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    # Inventory item-give caller functions surfaced by find_refs_allprogs
    # (refs_to_giveitem.txt) but not yet dumped on their own. Each is a real
    # function entry in a captured overlay that calls the add-item trio
    # (80042ee0 capacity-check / 80043048 reserve / 800421d4 add-by-id). Run
    # against each overlay program; the in_program() guard picks the owning
    # subset. Closes the cited-but-not-dumped worklist for these.
    #   overlay_0899_xxx_dat:
    "801c0f48", "801c6268", "801c6cf8",
    #   overlay_0977_other_game:
    "801c2748",
    #   overlay_0971 / overlay_0971_xxx_dat:
    "801c36b0",
    #   overlay_0897_xxx_dat (extended footprint):
    "801d7d4c", "801f138c", "8020a178", "8020e748",
    #   overlay_0896_bat_back_dat:
    "801dd690", "801e01f0", "801e0be8", "801fa38c", "801ffba4",
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
    """Dump the function containing `addr_str`, named for ITS OWN entry.

    Ghidra resolves an address with `getFunctionContaining()`, so asking for
    an interior address yields the ENCLOSING function. Naming the output after
    the address that was REQUESTED then writes a file asserting an entry point
    that does not exist, and every citation of that filename inherits the
    assertion. The corpus-wide signature is that the resolved entry always
    sits BELOW the requested address; nothing else produces it.

    So the filename, the header and the `entry=` field all come from
    `getEntryPoint()`, and a differing request is recorded as `requested=`
    inside the file - which keeps the citation traceable instead of losing it.
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
    interior = entry_str != addr_str.lower()
    if interior:
        print("[interior] {} is inside {} at {} - dumping the enclosing body"
              .format(addr_str, func.getName(), entry_str))

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(entry_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), entry_str, entry_str, prog_name))
        fh.write("size={} bytes, {} instructions\n".format(
            body.getNumAddresses(), len(instrs)))
        if interior:
            fh.write("requested={} (INTERIOR of this body - not an entry "
                     "point)\n".format(addr_str.lower()))
        fh.write("\n--- DISASSEMBLY ---\n")
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


for t in TARGETS:
    dump(t)

print("done [{}]".format(prog_name))
