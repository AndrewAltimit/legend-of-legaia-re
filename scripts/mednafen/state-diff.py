#!/usr/bin/env python3
"""
state-diff.py - pairwise diff between two mednafen save states.

Thin python wrapper over the Rust `mednafen-state diff` binary. Useful when
you want a quick CLI without remembering the address-window flag form, or
when you're scripting the watchpoint-bisect workflow against a list of
state pairs.

Examples:
    # Diff mc1 vs mc2 in the overlay window:
    scripts/mednafen/state-diff.py mc1 mc2 --start 0x801C0000 --end 0x80200000

    # Diff every consecutive pair in [mc1, mc2, mc3]:
    scripts/mednafen/state-diff.py --pairs mc1,mc2,mc3 --json /tmp/area-load.json

The arguments `mc<N>` resolve via $LEGAIA_MEDNAFEN_DIR (defaulting to
$HOME/.mednafen/mcs/) and the filename pattern in
`scripts/scenarios.toml`.
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


def resolve(slot_or_path: str) -> Path:
    """Map mc<N> to a save-state path; otherwise treat as a literal path."""
    if slot_or_path.lower().startswith("mc") and slot_or_path[2:].isdigit():
        return DEFAULT_MCS / PATTERN.format(slot=slot_or_path.lower())
    return Path(slot_or_path)


def parse_addr(s: str) -> int:
    return int(s, 16) if s.lower().startswith("0x") else int(s)


def run_diff(left: Path, right: Path,
             start: int | None, end: int | None,
             json_out: Path | None,
             min_changed: int, merge_gap: int, top: int) -> int:
    if not DEFAULT_BIN.exists():
        print(f"[info] building mednafen-state...", file=sys.stderr)
        subprocess.check_call(
            ["cargo", "build", "--release", "-p", "legaia-mednafen"],
            cwd=REPO_ROOT)
    args = [str(DEFAULT_BIN), "diff", str(left), str(right),
            "--min-changed", str(min_changed),
            "--merge-gap", str(merge_gap),
            "--top", str(top)]
    if start is not None:
        args += ["--start", f"0x{start:08X}"]
    if end is not None:
        args += ["--end", f"0x{end:08X}"]
    if json_out is not None:
        args += ["--json", str(json_out)]
    return subprocess.call(args)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("left", nargs="?", help="Left save (mc<N> or path)")
    ap.add_argument("right", nargs="?", help="Right save (mc<N> or path)")
    ap.add_argument("--pairs", help="Comma-separated list (e.g. mc1,mc2,mc3); "
                                    "diffs every consecutive pair")
    ap.add_argument("--start", type=parse_addr, default=None)
    ap.add_argument("--end", type=parse_addr, default=None)
    ap.add_argument("--min-changed", type=int, default=4)
    ap.add_argument("--merge-gap", type=int, default=16)
    ap.add_argument("--top", type=int, default=32)
    ap.add_argument("--json", type=Path, default=None,
                    help="Write JSON. With --pairs, suffix '.<i>' is appended per pair.")
    args = ap.parse_args()

    if args.pairs:
        items = [s.strip() for s in args.pairs.split(",")]
        if len(items) < 2:
            print("--pairs needs >= 2 entries", file=sys.stderr)
            return 64
        rc = 0
        for i in range(len(items) - 1):
            left = resolve(items[i])
            right = resolve(items[i + 1])
            print(f"==== pair {i}: {items[i]} -> {items[i + 1]} ====")
            json_out = None
            if args.json is not None:
                json_out = args.json.with_suffix(args.json.suffix + f".{i}")
            r = run_diff(left, right, args.start, args.end, json_out,
                         args.min_changed, args.merge_gap, args.top)
            if r != 0:
                rc = r
        return rc

    if not args.left or not args.right:
        ap.print_help()
        return 64

    return run_diff(resolve(args.left), resolve(args.right),
                    args.start, args.end, args.json,
                    args.min_changed, args.merge_gap, args.top)


if __name__ == "__main__":
    sys.exit(main())
