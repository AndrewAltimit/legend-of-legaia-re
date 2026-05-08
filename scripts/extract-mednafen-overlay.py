#!/usr/bin/env python3
"""
Extract a slice of PSX main RAM from a mednafen save state (.mc0..mc9).

Mednafen save states are gzipped streams whose decompressed body has a
'MDFNSVST' magic header followed by interleaved sections (CPU regs, GTE,
GPU, main RAM, etc.). For PSX, main RAM (2 MB) sits at a section we locate
by anchoring on a known string from SCUS_942.54.

The default slice extracts the overlay code window 0x801C0000..0x801F0000
(192 KB) where Legaia loads runtime overlays (script VM, per-mode handlers,
etc.). Use --start / --end (PSX virtual addresses) to slice a different
window.

Usage:
    scripts/extract-mednafen-overlay.py SAVE.mc0 [--out OUT.bin]
                                                  [--start 0x801C0000]
                                                  [--end   0x801F0000]
                                                  [--scus  PATH]

See docs/tooling/overlay-capture.md for the full capture pipeline.
"""

import argparse
import gzip
import os
import struct
import sys

# Anchor strings we know live in SCUS_942.54's loaded region (file offset
# >= 0x800). Picked because they're unique to this game so they won't
# collide with BIOS data. The first one that's found in BOTH the SCUS
# binary and the save state determines the file->RAM offset.
ANCHORS = [
    b"---- FIELD PROGRAM -----%d",
    b"PSX TEST PROGRAM",
    b"enter main loop",
    b"main free mem%d",
    b"h:\\prot\\cdname.dat",
]

PSX_RAM_SIZE = 2 * 1024 * 1024  # 2 MB
PSX_RAM_KSEG0 = 0x80000000
SCUS_LOAD_ADDR = 0x80010000
PSX_EXE_HEADER = 0x800
DEFAULT_OVERLAY_START = 0x801C0000
DEFAULT_OVERLAY_END = 0x801F0000


def parse_addr(s: str) -> int:
    return int(s, 16) if s.lower().startswith("0x") else int(s)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("save", help="Path to mednafen save state (.mc0/.mc1/...)")
    ap.add_argument("--out", help="Output path (default: derived from --start)")
    ap.add_argument("--start", type=parse_addr, default=DEFAULT_OVERLAY_START,
                    help="Slice start as PSX virtual address (default: 0x801C0000)")
    ap.add_argument("--end", type=parse_addr, default=DEFAULT_OVERLAY_END,
                    help="Slice end (exclusive) as PSX virtual address (default: 0x801F0000)")
    ap.add_argument("--scus", default="extracted/SCUS_942.54",
                    help="Path to extracted SCUS_942.54 (default: extracted/SCUS_942.54)")
    args = ap.parse_args()

    if not os.path.isfile(args.save):
        print(f"error: save state not found: {args.save}", file=sys.stderr)
        return 1
    if not os.path.isfile(args.scus):
        print(f"error: SCUS_942.54 not found at {args.scus}", file=sys.stderr)
        print("       pass --scus PATH to override", file=sys.stderr)
        return 1

    # Decompress the save state (gzip-wrapped 'MDFNSVST' container).
    with gzip.open(args.save, "rb") as fh:
        state = fh.read()
    print(f"[info] decompressed save state: {len(state):,} bytes")

    if state[:8] != b"MDFNSVST":
        print(f"error: bad save magic: {state[:8]!r} (expected MDFNSVST)", file=sys.stderr)
        return 1

    # Find the file->RAM offset by anchoring on a known SCUS string.
    scus = open(args.scus, "rb").read()
    ram_offset_in_state = None
    used_anchor = None
    for anchor in ANCHORS:
        scus_off = scus.find(anchor)
        if scus_off < PSX_EXE_HEADER:
            continue  # in PSX-EXE header, not loaded into RAM
        state_off = state.find(anchor)
        if state_off < 0:
            continue
        ram_addr = SCUS_LOAD_ADDR + (scus_off - PSX_EXE_HEADER)
        phys = ram_addr - PSX_RAM_KSEG0
        ram_offset_in_state = state_off - phys
        used_anchor = anchor
        print(f"[info] anchor: {anchor!r}")
        print(f"       SCUS file offset 0x{scus_off:X} -> RAM 0x{ram_addr:08X}")
        print(f"       found at state offset 0x{state_off:X}")
        print(f"       => main RAM starts at state offset 0x{ram_offset_in_state:08X}")
        break

    if ram_offset_in_state is None:
        print("error: no anchor found; can't locate main RAM", file=sys.stderr)
        return 1

    # Sanity-check by verifying a second anchor (different from the first).
    for anchor in ANCHORS:
        if anchor == used_anchor:
            continue
        scus_off = scus.find(anchor)
        if scus_off < PSX_EXE_HEADER:
            continue
        state_off = state.find(anchor)
        if state_off < 0:
            continue
        ram_addr = SCUS_LOAD_ADDR + (scus_off - PSX_EXE_HEADER)
        phys = ram_addr - PSX_RAM_KSEG0
        expected = phys + ram_offset_in_state
        if state_off != expected:
            print(f"WARNING: anchor {anchor!r} at 0x{state_off:X}; expected 0x{expected:X}",
                  file=sys.stderr)
            print(f"         RAM may not be contiguous in the state file. Slice may be wrong.",
                  file=sys.stderr)
        break

    # Extract the slice.
    if args.start < PSX_RAM_KSEG0 or args.end > PSX_RAM_KSEG0 + PSX_RAM_SIZE:
        print(f"error: slice [0x{args.start:08X}..0x{args.end:08X}) outside main RAM "
              f"[0x{PSX_RAM_KSEG0:08X}..0x{PSX_RAM_KSEG0 + PSX_RAM_SIZE:08X})", file=sys.stderr)
        return 1
    slice_start = ram_offset_in_state + (args.start - PSX_RAM_KSEG0)
    slice_end = ram_offset_in_state + (args.end - PSX_RAM_KSEG0)
    sliced = state[slice_start:slice_end]
    if len(sliced) != args.end - args.start:
        print(f"error: short read ({len(sliced)} of {args.end - args.start} bytes)", file=sys.stderr)
        return 1

    # Default output path: derive from --start.
    out_path = args.out or f"/tmp/legaia_ram_{args.start:08X}_{args.end:08X}.bin"
    open(out_path, "wb").write(sliced)
    print(f"[ok]   wrote {out_path}: {len(sliced):,} bytes "
          f"(RAM 0x{args.start:08X}..0x{args.end:08X})")

    # Quick MIPS-code-shape sanity check.
    JR_RA = bytes.fromhex("0800e003")
    n_jr = sum(1 for i in range(0, len(sliced) - 3, 4) if sliced[i:i + 4] == JR_RA)
    n_sp = sum(1 for i in range(0, len(sliced) - 3, 4)
               if sliced[i + 2] == 0xBD and sliced[i + 3] == 0x27 and sliced[i + 1] == 0xFF)
    nonzero = sum(1 for b in sliced if b)
    print(f"[info] {nonzero:,} nonzero bytes ({100 * nonzero / len(sliced):.1f}%); "
          f"{n_jr} `jr $ra`; {n_sp} SP prologues")
    return 0


if __name__ == "__main__":
    sys.exit(main())
