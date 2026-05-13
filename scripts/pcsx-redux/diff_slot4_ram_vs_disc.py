#!/usr/bin/env python3
"""
diff_slot4_ram_vs_disc.py

Byte-compare a slot-4 bin dumped from live RAM (via the autorun Lua
script `autorun_dump_slot4.lua`) against the disc-side decoded slot-4
payload. Reports:
  - total per-byte diffs across the entire decoded payload;
  - per-body diff counts (so we can see whether the runtime applies
    fixups inside one body but not another);
  - the first 32 differing offsets (or none if the bytes match).

If the two bytestreams match, that confirms slot 4 is loaded VERBATIM
into RAM and our disc-side decoder is correct. If they DON'T match,
the diff offsets point at the runtime-applied fixups - those are the
bytes the consumer reads, and the disc form would need to be patched
to reproduce them.

USAGE
    python3 scripts/pcsx-redux/diff_slot4_ram_vs_disc.py \\
        slot4_ram_drake.bin \\
        --bundle map01 \\
        --extracted extracted

The disc-side decode goes through the workspace's `asset kingdom-slot`
subcommand (so we don't reimplement the LZS pass here).
"""

import argparse
import struct
import subprocess
import sys
import tempfile
from pathlib import Path


KINGDOMS = {
    "map01": (85,  "drake"),
    "map02": (244, "sebucus"),
    "map03": (391, "karisto"),
}


def disc_slot4(extracted_dir: Path, bundle: str) -> bytes:
    """Invoke `cargo run --bin asset -- kingdom-slot ... --slot 4
    --out <tmp>` to get the disc-decoded slot 4 without
    reimplementing LZS in Python."""
    prot_index = KINGDOMS[bundle][0]
    prot_files = list(extracted_dir.joinpath("PROT").glob(f"{prot_index:04d}_*.BIN"))
    if not prot_files:
        raise RuntimeError(
            f"no PROT entry {prot_index:04d}_*.BIN under {extracted_dir}/PROT/"
        )
    prot_path = prot_files[0]

    # Prefer the release binary if it exists; fall back to debug.
    repo_root = Path(__file__).resolve().parents[2]
    for build in ("release", "debug"):
        cli = repo_root / "target" / build / "asset"
        if cli.exists():
            break
    else:
        raise RuntimeError(
            "asset CLI not built; run `cargo build -p legaia-asset --bin asset`"
        )

    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as tmp:
        out_path = Path(tmp.name)
    try:
        subprocess.run(
            [
                str(cli), "kingdom-slot",
                str(prot_path),
                "--slot", "4",
                "--out", str(out_path),
            ],
            check=True,
            capture_output=True,
        )
        return out_path.read_bytes()
    finally:
        out_path.unlink(missing_ok=True)


def parse_body_offsets(buf: bytes) -> list[tuple[int, int]]:
    """Return [(start, end)] byte ranges for every body in the slot-4
    payload. Empty if the header doesn't parse."""
    if len(buf) < 4:
        return []
    count = struct.unpack_from("<I", buf, 0)[0]
    if count == 0 or count > 256:
        return []
    if len(buf) < 4 + 4 * count:
        return []
    offsets = [struct.unpack_from("<I", buf, 4 + 4 * k)[0] for k in range(count)]
    ranges = []
    for k in range(count):
        start = offsets[k]
        end   = offsets[k + 1] if k + 1 < count else len(buf)
        if start > len(buf) or end > len(buf) or end < start:
            return []
        ranges.append((start, end))
    return ranges


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "ram_bin",
        type=Path,
        help="slot-4 bytes dumped from RAM (via autorun_dump_slot4.lua).",
    )
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=sorted(KINGDOMS.keys()),
        help="Kingdom bundle (default: map01).",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        type=Path,
        help="Extracted disc root (default: extracted/).",
    )
    args = ap.parse_args()

    if not args.ram_bin.exists():
        print(f"!! {args.ram_bin} not found", file=sys.stderr)
        return 1

    ram = args.ram_bin.read_bytes()
    print(f"RAM dump: {args.ram_bin}  ({len(ram)} bytes)")

    disc = disc_slot4(args.extracted, args.bundle)
    print(f"Disc-decoded slot 4 ({args.bundle}, PROT {KINGDOMS[args.bundle][0]:04d}): {len(disc)} bytes")

    if len(ram) != len(disc):
        print(f"!! size mismatch: ram={len(ram)} disc={len(disc)}")

    n = min(len(ram), len(disc))
    total_diffs = sum(1 for i in range(n) if ram[i] != disc[i])
    print(f"\nTotal differing bytes (over {n}): {total_diffs}")

    bodies = parse_body_offsets(disc)
    if bodies:
        print(f"\nPer-body diff counts (disc has {len(bodies)} bodies):")
        for k, (start, end) in enumerate(bodies):
            end_ram = min(end, len(ram))
            body_disc = disc[start:end]
            body_ram  = ram[start:end_ram]
            m = min(len(body_disc), len(body_ram))
            d = sum(1 for i in range(m) if body_disc[i] != body_ram[i])
            tag = "OK" if d == 0 else f"{d}/{m} diffs"
            print(f"  body {k:>2}: [{start:>6}..{end:>6})  {tag}")
    else:
        print("\n(skipping per-body diff: disc header didn't parse)")

    if total_diffs > 0:
        print(f"\nFirst 32 diffs:")
        shown = 0
        for i in range(n):
            if ram[i] != disc[i]:
                print(f"  off 0x{i:06X}  disc=0x{disc[i]:02X}  ram=0x{ram[i]:02X}")
                shown += 1
                if shown >= 32:
                    break

    if total_diffs == 0 and len(ram) == len(disc):
        print("\nMATCH: slot 4 is loaded verbatim into RAM with zero diffs.")
        return 0
    else:
        return 1


if __name__ == "__main__":
    sys.exit(main() or 0)
