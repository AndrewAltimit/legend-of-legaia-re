#!/usr/bin/env python3
"""
decode_slot4_subbodies.py

Decode kingdom slot 4 (type 0x05, ~32 KB) of a world-map bundle and
dump each sub-body's header + structural samples.

Slot 4's outer layout is the standard Legaia pack: `[u32 count]
[u32 word_offsets[count]][bodies]`. Earlier probes found count = 15
sub-bodies for Drake/map01. Each sub-body starts with a header that
contains the 16-bit marker `0x080C` at offset 4 (or possibly 6), which
matches the ANM container marker per the project's format docs - but
ANM bodies are animation streams, not geometry, so the marker may be
load-bearing for a different format here.

Goal: enumerate every sub-body, print its size + first bytes, and run
some plausibility checks for a vertex/UV/prim layout that would fit
the continent-terrain hypothesis (~3500-4300 POLY_FT4 prims visible
in the GPU pool but not from any TMD-magic-bearing pack).

USAGE
    python3 scripts/asset-investigation/decode_slot4_subbodies.py \\
        [--bundle map01|map02|map03] [--extracted PATH] [--body N]
"""

import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "pcsx-redux"))
from match_prim_groups_to_disc import find_asset_table, lzs_decompress  # noqa: E402


KINGDOM_BASE = {"map01": 85, "map02": 244, "map03": 391}


def load_slot4(extracted_dir: Path, bundle: str):
    """Return decoded slot 4 bytes for the kingdom bundle.

    Tries base then base+1, matching the runtime loader's pattern.
    Returns (bundle_path, slot_size_declared, decoded_bytes).
    """
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
        # Slot k = 4: type_size at offset 8 + 4*8 = 40, data_offset at 12 + 4*8 = 44
        ts = struct.unpack("<I", table[40:44])[0]
        do = struct.unpack("<I", table[44:48])[0]
        slot_type = ts >> 24
        slot_size = ts & 0xFFFFFF
        if slot_size == 0:
            continue
        try:
            decoded = lzs_decompress(table[do:], slot_size)
        except Exception:
            continue
        return files[0], slot_type, slot_size, decoded
    return None, None, None, None


def hexdump_line(buf: bytes, off: int, length: int = 32) -> str:
    """Format a single hexdump line."""
    chunk = buf[off : off + length]
    hexpart = " ".join(f"{b:02x}" for b in chunk)
    asciipart = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
    return f"  {off:#06x}  {hexpart:<{length*3}s}  |{asciipart}|"


def hexdump(buf: bytes, off: int, length: int) -> None:
    end = min(off + length, len(buf))
    cur = off
    while cur < end:
        print(hexdump_line(buf, cur, min(32, end - cur)))
        cur += 32


def walk_outer_pack(pack: bytes):
    """Return (count, word_offsets, byte_offsets, body_slices).

    Raises if the layout doesn't look like a pack.
    """
    count = struct.unpack("<I", pack[0:4])[0]
    if count == 0 or count > 256:
        raise ValueError(f"implausible count {count}")
    word_offsets = []
    for k in range(count):
        w = struct.unpack("<I", pack[4 + 4 * k : 8 + 4 * k])[0]
        word_offsets.append(w)
    # NOTE: empirically, slot-4's offset table holds BYTE offsets, NOT
    # word offsets (unlike the TMD pack at slot 1). first entry = 0x40
    # = 4 + 4*15 = exactly the header end, confirming byte interpretation.
    byte_offsets = list(word_offsets)
    # Slices end at the next offset (or pack end).
    bodies = []
    for k, bo in enumerate(byte_offsets):
        if k + 1 < count:
            end = byte_offsets[k + 1]
        else:
            end = len(pack)
        if bo > len(pack) or end > len(pack) or end < bo:
            raise ValueError(f"slice {k}: [{bo}..{end}] OOR (pack={len(pack)})")
        bodies.append(pack[bo:end])
    return count, word_offsets, byte_offsets, bodies


def parse_body_header(body: bytes):
    """Parse the 8-byte header confirmed by structural fit.

    Layout: `[u8 count_a, u8 flag_a, u8 count_b, u8 flag_b, u16 0x080C, u16 kind]`.
    The body holds `count_a * count_b` 8-byte records followed by an 8-byte
    trailer. (Verified empirically across all 15 sub-bodies of Drake's slot 4.)
    """
    count_a = body[0]
    flag_a = body[1]
    count_b = body[2]
    flag_b = body[3]
    marker = struct.unpack("<H", body[4:6])[0]
    kind = struct.unpack("<H", body[6:8])[0]
    return count_a, flag_a, count_b, flag_b, marker, kind


def analyze_body(body: bytes, idx: int) -> None:
    """Dump structural hypotheses for one sub-body."""
    print(f"\n--- body {idx}  size = {len(body)} bytes  ---")
    if len(body) < 16:
        print(f"  (too small)")
        return
    hexdump(body, 0, 32)

    count_a, flag_a, count_b, flag_b, marker, kind = parse_body_header(body)
    print(
        f"  header: count_a={count_a} flag_a={flag_a} count_b={count_b} "
        f"flag_b={flag_b} marker={marker:#06x} kind={kind}"
    )
    payload = len(body) - 8
    expected = count_a * count_b * 8 + 8
    fits = payload == expected
    print(
        f"  payload={payload}, expected={expected} "
        f"({'FITS' if fits else 'mismatch'})"
    )
    if not fits:
        return

    # Read records as 4 int16le -> (x, y, z, attr)
    n_records = count_a * count_b
    records = []
    for k in range(n_records):
        off = 8 + k * 8
        rx, ry, rz, ra = struct.unpack("<4h", body[off : off + 8])
        records.append((rx, ry, rz, ra))

    # Stats on first three columns: are they "vertex-like"? Range, mean.
    xs = [r[0] for r in records]
    ys = [r[1] for r in records]
    zs = [r[2] for r in records]
    attrs = [r[3] for r in records]
    print(
        f"  x: min={min(xs):>7d} max={max(xs):>7d} span={max(xs)-min(xs):>6d}  "
        f"y: min={min(ys):>7d} max={max(ys):>7d} span={max(ys)-min(ys):>6d}  "
        f"z: min={min(zs):>7d} max={max(zs):>7d} span={max(zs)-min(zs):>6d}"
    )
    print(
        f"  attr (col 4): min={min(attrs):>7d} max={max(attrs):>7d} "
        f"distinct={len(set(attrs))}"
    )

    # Trailer (last 8 bytes after the payload records)
    trailer_off = 8 + n_records * 8
    if trailer_off + 8 <= len(body):
        tr = body[trailer_off : trailer_off + 8]
        tu = struct.unpack("<4h", tr)
        print(f"  trailer = {tr.hex()}  ints = {tu}")

    # Grid hypothesis: if records form a count_a x count_b grid, then
    # walking row-major should produce monotone-ish runs. Compare X-step
    # along axis 1 (within a row of count_b) vs axis 2 (down a column).
    if count_a >= 2 and count_b >= 2:
        # Try interpretation A: records[a*count_b + b] = grid[a][b]
        # Row-major: step along b first.
        dx_row = []
        dz_row = []
        for a in range(count_a):
            for b in range(count_b - 1):
                k0 = a * count_b + b
                k1 = a * count_b + b + 1
                dx_row.append(records[k1][0] - records[k0][0])
                dz_row.append(records[k1][2] - records[k0][2])
        dx_col = []
        dz_col = []
        for a in range(count_a - 1):
            for b in range(count_b):
                k0 = a * count_b + b
                k1 = (a + 1) * count_b + b
                dx_col.append(records[k1][0] - records[k0][0])
                dz_col.append(records[k1][2] - records[k0][2])

        def avg(xs):
            return sum(xs) / len(xs) if xs else 0

        def consistency(xs):
            """Fraction within +/- 25% of the mean."""
            if not xs:
                return 0.0
            m = avg(xs)
            if abs(m) < 1:
                return 0.0
            tol = abs(m) * 0.25
            hits = sum(1 for v in xs if abs(v - m) <= tol)
            return hits / len(xs)

        print(
            f"  grid A (a*count_b + b):  dx_row mean={avg(dx_row):>+8.1f} "
            f"consistency={consistency(dx_row)*100:.0f}%   "
            f"dz_row mean={avg(dz_row):>+8.1f} cons={consistency(dz_row)*100:.0f}%"
        )
        print(
            f"                          dx_col mean={avg(dx_col):>+8.1f} "
            f"consistency={consistency(dx_col)*100:.0f}%   "
            f"dz_col mean={avg(dz_col):>+8.1f} cons={consistency(dz_col)*100:.0f}%"
        )


def export_body_obj(body: bytes, idx: int, out_path: Path):
    """Write a sub-body as a Wavefront OBJ treating each of the
    `count_b` groups as a polygon with `count_a` vertices. Each record
    is interpreted as (x, y, z, attr) - col 4 ignored.

    This is the "group = primitive" hypothesis. If it produces coherent
    shapes, the per-record format is at least dimensionally correct.
    """
    count_a, _, count_b, _, _, kind = parse_body_header(body)
    n_records = count_a * count_b
    if 8 + n_records * 8 + 8 > len(body):
        return False
    out = [
        "# slot-4 body OBJ",
        f"# body {idx}: count_a={count_a} count_b={count_b} kind={kind}",
        f"# treating each of {count_b} groups as a {count_a}-vertex polygon",
    ]
    # Emit vertices
    for g in range(count_b):
        for v in range(count_a):
            off = 8 + (g * count_a + v) * 8
            x, y, z, _a = struct.unpack("<4h", body[off : off + 8])
            out.append(f"v {x} {y} {z}")
    # Emit each group as a polygon face
    for g in range(count_b):
        base = 1 + g * count_a
        idx_list = " ".join(str(base + v) for v in range(count_a))
        out.append(f"f {idx_list}")
    out_path.write_text("\n".join(out))
    return True


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=sorted(KINGDOM_BASE.keys()),
        help="Kingdom bundle (default: map01).",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        help="Extracted disc root (default: extracted/).",
    )
    ap.add_argument(
        "--body",
        type=int,
        help="Dump only one body index (default: all).",
    )
    ap.add_argument(
        "--dump",
        type=int,
        default=64,
        help="Hexdump prefix length per body (default: 64).",
    )
    ap.add_argument(
        "--export-obj-dir",
        help="Write each body as a grid-OBJ to this directory.",
    )
    args = ap.parse_args()

    extracted = Path(args.extracted)
    bundle_path, slot_type, slot_size, pack = load_slot4(extracted, args.bundle)
    if pack is None:
        print(f"!! could not load slot 4 for {args.bundle}")
        return 1
    print(
        f"bundle: {bundle_path.name}  slot 4 type {slot_type:#04x}  "
        f"declared size {slot_size}  decoded size {len(pack)}"
    )
    print(f"\nfirst 64 bytes of decoded slot 4:")
    hexdump(pack, 0, 64)

    try:
        count, word_offsets, byte_offsets, bodies = walk_outer_pack(pack)
    except ValueError as e:
        print(f"\n!! outer pack didn't parse: {e}")
        # Even so, hex-dump the start so we can look manually.
        return 1

    print(f"\nouter pack: count = {count}")
    print(f"word_offsets = {word_offsets}")
    print(f"byte_offsets = {byte_offsets}")

    out_dir = None
    if args.export_obj_dir:
        out_dir = Path(args.export_obj_dir)
        out_dir.mkdir(parents=True, exist_ok=True)

    if args.body is not None:
        if args.body < 0 or args.body >= count:
            print(f"!! body index {args.body} out of range [0..{count})")
            return 1
        analyze_body(bodies[args.body], args.body)
        if out_dir is not None:
            obj_path = out_dir / f"body_{args.body:02d}.obj"
            ok = export_body_obj(bodies[args.body], args.body, obj_path)
            print(f"  wrote {obj_path}" if ok else "  (skipped)")
        return 0

    for k in range(count):
        analyze_body(bodies[k], k)
        if out_dir is not None:
            obj_path = out_dir / f"body_{k:02d}.obj"
            export_body_obj(bodies[k], k, obj_path)

    # Size histogram
    print(f"\n=== Sub-body size histogram ===")
    for k in range(count):
        print(f"  body {k:>2d}: {len(bodies[k]):>8d} bytes")

    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
