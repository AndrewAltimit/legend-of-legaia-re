# @category Legaia
# @runtime Jython
#
# apply_known_symbols.py - re-apply this project's pinned function names to a
# fresh Ghidra import of SCUS_942.54.
#
# A raw-blob import names every function FUN_<addr>. This script reads the
# curated table in known_symbols.py and, for each SCUS-resident entry that
# lives in the current program, (1) names the function (creating one if the
# import didn't auto-detect it) and (2) sets a one-line PLATE comment with the
# role. It's the clean-room counterpart to a PsyQ FidDB pass: instead of an
# external SDK signature DB, it replays the names we reverse-engineered.
#
# Run (after importing SCUS_942.54 into a project):
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/apply_known_symbols.py
#
# Re-runnable: renaming an already-named function is a no-op-ish update.
# Overlay programs (0x801C0000+) are skipped wholesale - the table is
# SCUS-only by design (overlay addresses alias across overlays).

import sys

# Make the sibling data module importable regardless of cwd.
sys.path.insert(0, "/scripts")
sys.path.insert(0, ".")
import known_symbols  # noqa: E402

from ghidra.app.cmd.function import CreateFunctionCmd  # noqa: E402
from ghidra.program.model.listing import CodeUnit  # noqa: E402
from ghidra.program.model.symbol import SourceType  # noqa: E402

prog = currentProgram  # noqa: F821 (Ghidra-injected)
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()


def in_program(addr):
    return mem.getBlock(addr) is not None


def apply_one(addr_int, name, comment):
    addr = af.getDefaultAddressSpace().getAddress(addr_int)
    if not in_program(addr):
        return "skip-not-in-program"
    func = fm.getFunctionAt(addr)
    if func is None:
        # No function detected at this address yet; try to create one.
        cmd = CreateFunctionCmd(addr)
        if cmd.applyTo(prog):
            func = fm.getFunctionAt(addr)
    if func is not None:
        func.setName(name, SourceType.USER_DEFINED)
    else:
        # Fall back to a plain label if a function couldn't be formed.
        prog.getSymbolTable().createLabel(addr, name, SourceType.USER_DEFINED)
    listing.setComment(addr, CodeUnit.PLATE_COMMENT, comment)
    return "ok" if func is not None else "ok-label-only"


def main():
    if not prog_name.startswith("SCUS"):
        print("[apply_known_symbols] program is '{}', not SCUS - the table is "
              "SCUS-resident only; nothing applied.".format(prog_name))
        return
    txid = prog.startTransaction("apply known symbols")
    applied = 0
    skipped = 0
    try:
        for addr_int, name, comment in known_symbols.SYMBOLS:
            if not known_symbols.in_scus_range(addr_int):
                print("[skip] 0x{:08X} {} out of SCUS range".format(addr_int, name))
                skipped += 1
                continue
            status = apply_one(addr_int, name, comment)
            if status.startswith("ok"):
                print("[{}] 0x{:08X} -> {}".format(status, addr_int, name))
                applied += 1
            else:
                print("[{}] 0x{:08X} {}".format(status, addr_int, name))
                skipped += 1
    finally:
        prog.endTransaction(txid, True)
    print("[apply_known_symbols] applied {}, skipped {} (of {})".format(
        applied, skipped, len(known_symbols.SYMBOLS)))


main()
