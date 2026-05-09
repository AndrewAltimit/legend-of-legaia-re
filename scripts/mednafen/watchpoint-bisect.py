#!/usr/bin/env python3
"""
watchpoint-bisect.py — given an address that should be zero in a "before"
state and non-zero in an "after" state, walk an ordered list of save states
and report the first one in which it transitions.

This is the "memory breakpoint over discrete snapshots" workflow. It's
useful when the user has saves at progressively later points during a
sequence (area load, level-up animation, etc.) and wants to know which
neighbouring pair brackets the write to a target address.

Examples:
    # Find when the move-table base pointer gets populated:
    scripts/mednafen/watchpoint-bisect.py \\
        --addr 0x8007B888 mc0 mc1 mc2 mc3

    # Find when a battle actor pool slot gets filled (pred = nonzero, default):
    scripts/mednafen/watchpoint-bisect.py \\
        --addr 0x801C9374 mc4 mc5 mc6

    # Trace mode: print the value at every state (no bisect outcome).
    scripts/mednafen/watchpoint-bisect.py \\
        --addr 0x80084540 --trace mc1 mc2 mc3
"""

import argparse
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_BIN = REPO_ROOT / "target" / "release" / "mednafen-state"
DEFAULT_MCS = Path(os.environ.get("LEGAIA_MEDNAFEN_DIR",
                                  str(Path.home() / ".mednafen" / "mcs")))
PATTERN = "Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.{slot}"


def resolve(slot_or_path: str) -> str:
    if slot_or_path.lower().startswith("mc") and slot_or_path[2:].isdigit():
        return str(DEFAULT_MCS / PATTERN.format(slot=slot_or_path.lower()))
    return slot_or_path


def parse_addr(s: str) -> int:
    return int(s, 16) if s.lower().startswith("0x") else int(s)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--addr", type=parse_addr, required=True,
                    help="PSX virtual address to watch (e.g. 0x8007B888)")
    ap.add_argument("--predicate", choices=["nonzero", "zero"],
                    default="nonzero",
                    help="Treat values matching this predicate as the post-write state")
    ap.add_argument("--trace", action="store_true",
                    help="Print value at every state instead of bisecting")
    ap.add_argument("saves", nargs="+",
                    help="Ordered list of save states (mc<N> or path)")
    args = ap.parse_args()

    if not DEFAULT_BIN.exists():
        print("[info] building mednafen-state...", file=sys.stderr)
        subprocess.check_call(
            ["cargo", "build", "--release", "-p", "legaia-mednafen"],
            cwd=REPO_ROOT)

    resolved = [resolve(s) for s in args.saves]
    sub = "trace" if args.trace else "bisect"
    cmd = [str(DEFAULT_BIN), sub, "--addr", f"0x{args.addr:08X}"]
    if not args.trace:
        cmd += ["--predicate", args.predicate]
    cmd += resolved
    return subprocess.call(cmd)


if __name__ == "__main__":
    sys.exit(main())
