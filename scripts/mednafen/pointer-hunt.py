#!/usr/bin/env python3
"""Generalised "find the RAM cell holding a pointer into <window>" tool.

Companion to `mednafen-state diff` — that command surfaces *changed bytes*, this
one surfaces *changed pointers*. The output is a list of u32-aligned RAM cells
whose value is a PSX-RAM pointer (`0x80000000..0x80200000`) into a target
window in either save or both.

The pattern shows up everywhere in retail engine RE work: a structure is
loaded into RAM by an overlay, and then the rest of the code reads it through
a base-pointer that some scene-init writer set up. To find that writer, you
first need to know where the base-pointer LIVES — that's what this tool does.

Two main modes:

  - `into-window <save> --target-lo X --target-hi Y` — list every cell in
    `<save>` that holds a pointer into `[X, Y)`. Use this to find the
    "table base" cell after you've located the table window with `diff`.

  - `flips <save_a> <save_b> --target-lo X --target-hi Y` — list cells that
    flipped from "not pointing into the window" to "pointing into the
    window" (or vice-versa) between the two saves. Use this to find a
    base-pointer cell that gets newly populated when an overlay loads.

Both modes default to scanning all of main RAM; restrict with
`--scan-lo`/`--scan-hi` if you have a known caller-side region (e.g. the
script-VM ctx range `0x801C0000..0x801E0000`).

The tool depends only on `mednafen-state extract`'s output (a 2 MiB raw RAM
dump). Run that first:

  ./target/release/mednafen-state extract <save.mc1> --start 0x80000000 \\
      --end 0x80200000 --out /tmp/ram_a.bin

Then either:

  python3 scripts/mednafen/pointer-hunt.py into-window /tmp/ram_a.bin \\
      --target-lo 0x80108EA4 --target-hi 0x80109550

  python3 scripts/mednafen/pointer-hunt.py flips /tmp/ram_a.bin /tmp/ram_b.bin \\
      --target-lo 0x801C9300 --target-hi 0x801CA000 \\
      --scan-lo 0x801C0000 --scan-hi 0x801E0000
"""

from __future__ import annotations

import argparse
import struct
import sys
from collections import Counter
from pathlib import Path
from typing import Iterable

PSX_KSEG0 = 0x80000000
PSX_RAM_END = 0x80200000


def parse_addr(s: str) -> int:
    s = s.strip()
    return int(s, 16) if s.lower().startswith("0x") else int(s)


def read_ram(path: Path) -> bytes:
    data = path.read_bytes()
    if len(data) != PSX_RAM_END - PSX_KSEG0:
        sys.exit(
            f"{path}: expected {PSX_RAM_END - PSX_KSEG0} bytes (full main RAM); "
            f"got {len(data)}. Run `mednafen-state extract` with "
            f"--start 0x80000000 --end 0x80200000."
        )
    return data


def is_ram_pointer(v: int) -> bool:
    return PSX_KSEG0 <= v < PSX_RAM_END


def cells_pointing_into(
    ram: bytes,
    target_lo: int,
    target_hi: int,
    scan_lo: int,
    scan_hi: int,
) -> list[tuple[int, int]]:
    """Return `(cell_addr, value)` for every u32-aligned cell in `[scan_lo, scan_hi)` whose value lies in `[target_lo, target_hi)`."""
    out: list[tuple[int, int]] = []
    base = scan_lo - PSX_KSEG0
    end = scan_hi - PSX_KSEG0
    for off in range(base, end, 4):
        v = struct.unpack_from("<I", ram, off)[0]
        if target_lo <= v < target_hi:
            out.append((PSX_KSEG0 + off, v))
    return out


def cmd_into_window(args: argparse.Namespace) -> int:
    ram = read_ram(Path(args.save))
    hits = cells_pointing_into(
        ram, args.target_lo, args.target_hi, args.scan_lo, args.scan_hi
    )
    print(
        f"[into-window] {args.save}: {len(hits)} cells in "
        f"0x{args.scan_lo:08X}..0x{args.scan_hi:08X} hold pointers into "
        f"0x{args.target_lo:08X}..0x{args.target_hi:08X}"
    )
    if args.exclude_self:
        before = len(hits)
        hits = [
            (a, v) for (a, v) in hits if not (args.target_lo <= a < args.target_hi)
        ]
        print(
            f"  (filtered {before - len(hits)} self-references inside the target window)"
        )
    for a, v in hits[: args.top]:
        offset_in_target = v - args.target_lo
        print(f"  0x{a:08X}  ->  0x{v:08X}  (target+0x{offset_in_target:X})")
    if len(hits) > args.top:
        print(f"  ... and {len(hits) - args.top} more (raise --top to see)")
    if args.target_freq:
        freq = Counter(v for _, v in hits)
        print(f"  most-pointed-to addresses:")
        for tgt, cnt in freq.most_common(min(args.top, 20)):
            print(f"    {cnt:>4}x  0x{tgt:08X}  (target+0x{tgt - args.target_lo:X})")
    return 0


def cmd_flips(args: argparse.Namespace) -> int:
    ram_a = read_ram(Path(args.save_a))
    ram_b = read_ram(Path(args.save_b))
    base = args.scan_lo - PSX_KSEG0
    end = args.scan_hi - PSX_KSEG0
    in_target = lambda v: args.target_lo <= v < args.target_hi
    new_into: list[tuple[int, int, int]] = []
    new_outof: list[tuple[int, int, int]] = []
    swapped: list[tuple[int, int, int]] = []
    for off in range(base, end, 4):
        l = struct.unpack_from("<I", ram_a, off)[0]
        r = struct.unpack_from("<I", ram_b, off)[0]
        if l == r:
            continue
        a = PSX_KSEG0 + off
        if in_target(l) and in_target(r):
            swapped.append((a, l, r))
        elif in_target(r) and not in_target(l):
            new_into.append((a, l, r))
        elif in_target(l) and not in_target(r):
            new_outof.append((a, l, r))
    a_label = Path(args.save_a).name
    b_label = Path(args.save_b).name
    print(
        f"[flips] {a_label} -> {b_label}: scan 0x{args.scan_lo:08X}..0x{args.scan_hi:08X}, "
        f"target 0x{args.target_lo:08X}..0x{args.target_hi:08X}"
    )
    print(
        f"  newly-pointing-in (into target after flip):     {len(new_into)}\n"
        f"  newly-pointing-out (out of target after flip):  {len(new_outof)}\n"
        f"  swapped (different pointer, both in target):    {len(swapped)}"
    )

    def show(label: str, rows: list[tuple[int, int, int]]):
        if not rows:
            return
        print(f"\n  {label}:")
        for a, l, r in rows[: args.top]:
            print(f"    0x{a:08X}: 0x{l:08X}  ->  0x{r:08X}")
        if len(rows) > args.top:
            print(f"    ... and {len(rows) - args.top} more")

    show("newly-pointing-in", new_into)
    if not args.in_only:
        show("newly-pointing-out", new_outof)
        show("swapped", swapped)
    return 0


def main(argv: Iterable[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    p_into = sub.add_parser(
        "into-window",
        help="List cells in <save> holding pointers into a target window.",
    )
    p_into.add_argument("save", help="Raw 2 MiB RAM dump (from mednafen-state extract).")
    p_into.add_argument("--target-lo", type=parse_addr, required=True)
    p_into.add_argument("--target-hi", type=parse_addr, required=True)
    p_into.add_argument(
        "--scan-lo",
        type=parse_addr,
        default=PSX_KSEG0,
        help="Restrict the scan to this PSX-virtual lower bound (default: 0x80000000).",
    )
    p_into.add_argument(
        "--scan-hi",
        type=parse_addr,
        default=PSX_RAM_END,
        help="Restrict the scan to this PSX-virtual upper bound (default: 0x80200000).",
    )
    p_into.add_argument("--top", type=int, default=64)
    p_into.add_argument(
        "--exclude-self",
        action="store_true",
        help="Filter out cells whose own address lies inside the target window.",
    )
    p_into.add_argument(
        "--target-freq",
        action="store_true",
        help="Also print a most-pointed-to histogram (clustering signal).",
    )
    p_into.set_defaults(func=cmd_into_window)

    p_flips = sub.add_parser(
        "flips",
        help="List cells that crossed the target-window boundary between two saves.",
    )
    p_flips.add_argument("save_a", help="Earlier save (raw 2 MiB RAM dump).")
    p_flips.add_argument("save_b", help="Later save.")
    p_flips.add_argument("--target-lo", type=parse_addr, required=True)
    p_flips.add_argument("--target-hi", type=parse_addr, required=True)
    p_flips.add_argument("--scan-lo", type=parse_addr, default=PSX_KSEG0)
    p_flips.add_argument("--scan-hi", type=parse_addr, default=PSX_RAM_END)
    p_flips.add_argument("--top", type=int, default=64)
    p_flips.add_argument(
        "--in-only",
        action="store_true",
        help="Only show newly-pointing-in flips (the load-time setup signal).",
    )
    p_flips.set_defaults(func=cmd_flips)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
