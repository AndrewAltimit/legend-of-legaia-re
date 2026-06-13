#!/usr/bin/env python3
"""
slot4_to_obj.py

Combine all 15 sub-bodies of a kingdom's slot 4 into one Wavefront OBJ
for visualisation. Each body becomes its own OBJ group (`g body_N`)
so a 3D viewer can isolate them.

Each group of `count_a` records (8 bytes each, interpreted as 4 int16)
becomes either:
  - a single polygon (with --as-polys)
  - a polyline strip (--as-lines), or
  - just points (--as-points, default)

Coordinates are emitted as (X, Y, Z) = (col 0, col 1, col 2) of each
8-byte record. The 4th int16 column is dropped for now (semantics
unknown).
"""

import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "pcsx-redux"))
from match_prim_groups_to_disc import find_asset_table, lzs_decompress  # noqa: E402


KINGDOM_BASE = {"map01": 85, "map02": 244, "map03": 391}


def load_slot4(extracted_dir: Path, bundle: str):
    base = KINGDOM_BASE[bundle]
    for off in (0, 1):
        idx = base + off
        files = list(extracted_dir.joinpath("PROT").glob(f"{idx:04d}_*.BIN"))
        if not files:
            continue
        raw = files[0].read_bytes()
        table_off = find_asset_table(raw)
        if table_off is None:
            continue
        table = raw[table_off:]
        ts = struct.unpack("<I", table[40:44])[0]
        do = struct.unpack("<I", table[44:48])[0]
        slot_size = ts & 0xFFFFFF
        try:
            decoded = lzs_decompress(table[do:], slot_size)
        except Exception:
            continue
        return files[0].name, decoded
    return None, None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bundle", default="map01", choices=sorted(KINGDOM_BASE.keys()))
    ap.add_argument("--extracted", default="extracted")
    ap.add_argument("--out", default="/tmp/slot4_combined.obj")
    ap.add_argument(
        "--mode",
        default="polys",
        choices=("polys", "lines", "points"),
        help="How to connect each group's vertices.",
    )
    ap.add_argument(
        "--skip-degenerate",
        action="store_true",
        help="Skip records where (x,y,z) is all zeros.",
    )
    args = ap.parse_args()

    name, decoded = load_slot4(Path(args.extracted), args.bundle)
    if decoded is None:
        print(f"!! could not load slot 4 for {args.bundle}")
        return 1
    print(f"bundle: {name}  slot 4 decoded = {len(decoded)} bytes")

    count = struct.unpack("<I", decoded[0:4])[0]
    byte_offsets = [
        struct.unpack("<I", decoded[4 + 4 * k : 8 + 4 * k])[0]
        for k in range(count)
    ]

    out_lines = [f"# slot-4 combined OBJ  bundle={args.bundle}  mode={args.mode}"]
    vertex_count = 0
    for k in range(count):
        s = byte_offsets[k]
        e = byte_offsets[k + 1] if k + 1 < count else len(decoded)
        body = decoded[s:e]
        if len(body) < 8:
            continue
        ca, fa, cb, fb = body[0], body[1], body[2], body[3]
        kind = struct.unpack("<H", body[6:8])[0]
        n_records = ca * cb
        if 8 + n_records * 8 + 8 > len(body):
            continue

        out_lines.append("")
        out_lines.append(
            f"# body {k}: count_a={ca} count_b={cb} kind={kind} "
            f"records={n_records}"
        )
        out_lines.append(f"g body_{k:02d}")

        # Emit vertices for this body, tracking the base index.
        first_vertex_in_body = vertex_count + 1  # OBJ is 1-indexed
        for r in range(n_records):
            off = 8 + r * 8
            x, y, z, _a = struct.unpack("<4h", body[off : off + 8])
            if args.skip_degenerate and x == 0 and y == 0 and z == 0:
                # Still emit a placeholder vertex to keep indexing simple,
                # but use a sentinel value so it floats away from the mesh.
                out_lines.append(f"v 0 0 0  # degen-placeholder")
            else:
                out_lines.append(f"v {x} {y} {z}")
            vertex_count += 1

        if args.mode == "polys":
            for g in range(cb):
                base = first_vertex_in_body + g * ca
                idx_list = " ".join(str(base + v) for v in range(ca))
                out_lines.append(f"f {idx_list}")
        elif args.mode == "lines":
            for g in range(cb):
                base = first_vertex_in_body + g * ca
                idx_list = " ".join(str(base + v) for v in range(ca))
                out_lines.append(f"l {idx_list}")
        # mode == "points" emits no faces/lines (just vertices)

    Path(args.out).write_text("\n".join(out_lines))
    print(
        f"wrote {args.out}  ({vertex_count} vertices, mode={args.mode})"
    )

    # Also print combined bbox
    xs, ys, zs = [], [], []
    for line in out_lines:
        if line.startswith("v ") and "degen" not in line:
            parts = line.split()
            xs.append(int(parts[1]))
            ys.append(int(parts[2]))
            zs.append(int(parts[3]))
    if xs:
        print(
            f"  bbox: x={min(xs):>7d}..{max(xs):>7d}  "
            f"y={min(ys):>7d}..{max(ys):>7d}  z={min(zs):>7d}..{max(zs):>7d}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
