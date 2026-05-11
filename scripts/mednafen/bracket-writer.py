#!/usr/bin/env python3
"""Reusable sequential-save diff harness.

Given a `before` and `after` mednafen save state plus a target
description (VRAM region, RAM region, or byte needle), report:

  1. whether the target region transitioned ZERO -> POPULATED
  2. whether any provided needles appeared anywhere in main RAM or VRAM
  3. which actor-pool slots / overlay-window regions / scene-name globals
     changed (high-level diff)

The aim is to narrow the search window for "what caused X to land in
RAM/VRAM" from "anywhere in 442 KB of SCUS + overlays" to "the loader
for actor Y" or "the entry point of overlay Z" - a sequential pair of
saves brackets the moment-of-write.

This is the generalised successor to the old `bracket-row479.py` (which
hard-coded the row-479 hue-ramp scan against a constant that no longer
lives in the codebase). The standard regions / scenarios are kept
configurable on the CLI so the same harness can drive:

- Row 479 NPC palettes (`--vram-row 479 --vram-x-end 240`).
- Inventory writer hunts (`--ram 80085600 80085800`).
- Story-flag flips (`--ram 80085000 80086000 --needle <flag-bytes>`).
- XP table loads (`--ram 80070000 80080000 --needle <table-bytes>`).

Run from the repo root with the `mednafen-state` binary built
(`cargo build --release -p legaia-mednafen`):

    python3 scripts/mednafen/bracket-writer.py \\
        --before ~/.mednafen/mcs/<game>.mcA \\
        --after  ~/.mednafen/mcs/<game>.mcB \\
        --vram-row 479

For multiple needles, pass `--needle` repeatedly with hex strings.
"""
import argparse
import hashlib
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MED = REPO_ROOT / "target" / "release" / "mednafen-state"

# Canonical RAM regions to diff. Anything not in this list can still be
# scanned manually via `--ram <start> <end>` on the CLI.
DEFAULT_REGIONS = {
    "staging_buffer":           (0x800F1800, 0x800F1B00),
    "actor_anim_pool":          (0x801C9594, 0x801C9F80),
    "town_overlay_scratch":     (0x801CE808, 0x801D3018),
    "overlay_window_full":      (0x801C0000, 0x80200000),
    "scratch_buffer_b":         (0x80108EA4, 0x80109550),
    "scene_local_pool":         (0x800F1000, 0x800F2000),
    "scene_name_block":         (0x80084540, 0x800845C0),
    "battle_actor_pool":        (0x800EC9E8, 0x800ED560),
    "story_flag_bitmap":        (0x80085600, 0x80085800),
    "asset_descriptor_result":  (0x80087AF0, 0x80087B00),
}


def run(*args):
    return subprocess.run(args, check=True, capture_output=True)


def extract_ram(save_path: Path, start: int, end: int, out_path: Path):
    run(str(MED), "extract", str(save_path),
        "--start", f"0x{start:08X}", "--end", f"0x{end:08X}",
        "--out", str(out_path))


def vram_dump(save_path: Path, out_bin: Path):
    """Dump VRAM as a 1 MiB raw BGR555 blob. The PNG version is
    discarded - this tool wants byte-level access."""
    png_path = out_bin.with_suffix(".png")
    run(str(MED), "vram-dump", str(save_path),
        "--out", str(png_path),
        "--out-bin", str(out_bin))


def hash16(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()[:16]


def vram_row_slice(v: bytes, row: int, x_start: int, x_end: int) -> bytes:
    """Slice a VRAM row range. Each VRAM row is 2048 bytes (1024 px ×
    2 bytes / px), each pixel is a BGR555 halfword."""
    row_off = row * 2048
    return v[row_off + x_start * 2 : row_off + x_end * 2]


def parse_needle(s: str) -> bytes:
    s = s.strip().lower().replace(" ", "").replace("-", "").replace("0x", "")
    if len(s) % 2 != 0:
        raise ValueError(f"needle hex length must be even, got {len(s)}")
    return bytes.fromhex(s)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--before", required=True, type=Path,
                    help="'before' save state (target region empty / pre-event)")
    ap.add_argument("--after", required=True, type=Path,
                    help="'after' save state (target region populated / post-event)")
    ap.add_argument("--workdir", default="/tmp/bracket_writer", type=Path)

    # VRAM target
    ap.add_argument("--vram-row", type=int,
                    help="VRAM row to inspect (0..511). Slices fb_x range.")
    ap.add_argument("--vram-x-start", type=int, default=0)
    ap.add_argument("--vram-x-end", type=int, default=1024)

    # Custom RAM region (in addition to DEFAULT_REGIONS)
    ap.add_argument("--ram", nargs=2, action="append", default=[],
                    metavar=("START", "END"),
                    help="Extra RAM region (hex). Can repeat.")

    # Needles
    ap.add_argument("--needle", action="append", default=[],
                    help="Hex byte string to search for in VRAM + every "
                         "extracted RAM region. Can repeat.")

    # Region selection
    ap.add_argument("--skip-default-regions", action="store_true",
                    help="Skip the curated DEFAULT_REGIONS scan and only "
                         "diff the regions you pass via --ram.")
    args = ap.parse_args()

    work = args.workdir
    (work / "before").mkdir(parents=True, exist_ok=True)
    (work / "after").mkdir(parents=True, exist_ok=True)

    needles: list[bytes] = []
    for raw in args.needle:
        try:
            needles.append(parse_needle(raw))
        except ValueError as e:
            sys.exit(f"--needle {raw!r}: {e}")
    if needles:
        print(f"[needles] {len(needles)} byte pattern(s) to search")

    # VRAM scan
    if args.vram_row is not None:
        for tag, save in [("before", args.before), ("after", args.after)]:
            vbin = work / tag / "vram.bin"
            vram_dump(save, vbin)
            v = vbin.read_bytes()
            slab = vram_row_slice(v, args.vram_row, args.vram_x_start, args.vram_x_end)
            nz = sum(1 for b in slab if b != 0)
            print(f"  {tag} VRAM row {args.vram_row} "
                  f"fb_x={args.vram_x_start}..{args.vram_x_end}: "
                  f"hash={hash16(slab)}  nonzero={nz}/{len(slab)}")
            for i, n in enumerate(needles):
                p = v.find(n)
                if p != -1:
                    row = p // 2048
                    px = (p % 2048) // 2
                    print(f"    {tag} VRAM needle{i} found at row={row} fb_x={px}")

    # RAM region diffs
    print("\n[regions]")
    regions: dict[str, tuple[int, int]] = {}
    if not args.skip_default_regions:
        regions.update(DEFAULT_REGIONS)
    for s, e in args.ram:
        s_int = int(s, 0)
        e_int = int(e, 0)
        regions[f"custom_{s_int:08X}_{e_int:08X}"] = (s_int, e_int)

    transitioned: list[tuple[str, bytes, bytes]] = []
    for name, (start, end) in regions.items():
        before_bin = work / "before" / f"{name}.bin"
        after_bin = work / "after" / f"{name}.bin"
        extract_ram(args.before, start, end, before_bin)
        extract_ram(args.after, start, end, after_bin)
        before = before_bin.read_bytes()
        after = after_bin.read_bytes()
        if before == after:
            print(f"  {name:30s} unchanged  ({len(before)}B)")
            continue
        new_nz = sum(1 for a, b in zip(before, after) if a == 0 and b != 0)
        changed = sum(1 for a, b in zip(before, after) if a != b)
        print(f"  {name:30s} {hash16(before)} -> {hash16(after)}  "
              f"changed={changed}  new_nz={new_nz}")
        transitioned.append((name, before, after))
        for i, n in enumerate(needles):
            if before.find(n) == -1 and after.find(n) != -1:
                p = after.find(n)
                addr = start + p
                print(f"    + needle{i} APPEARED at 0x{addr:08X}")

    # Actor-pool slot-level diff (60-byte stride)
    actor_before = work / "before" / "actor_anim_pool.bin"
    actor_after = work / "after" / "actor_anim_pool.bin"
    if actor_before.exists() and actor_after.exists():
        print("\n[actor_anim_pool slot diff]")
        a = actor_before.read_bytes()
        b = actor_after.read_bytes()
        stride = 0x60
        n_slots = len(a) // stride
        for i in range(n_slots):
            asl = a[i * stride : (i + 1) * stride]
            bsl = b[i * stride : (i + 1) * stride]
            if asl != bsl:
                slot_addr = 0x801C9594 + i * stride
                a_dispatch = asl[0x0F] if len(asl) > 0x10 else 0
                b_dispatch = bsl[0x0F] if len(bsl) > 0x10 else 0
                a_ptr = int.from_bytes(asl[:4], "little") if len(asl) >= 4 else 0
                b_ptr = int.from_bytes(bsl[:4], "little") if len(bsl) >= 4 else 0
                print(f"  slot{i:2d} @ 0x{slot_addr:08X}: "
                      f"dispatch {a_dispatch:#04x}->{b_dispatch:#04x}  "
                      f"ptr 0x{a_ptr:08X}->0x{b_ptr:08X}")

    print("\n[done]")


if __name__ == "__main__":
    main()
