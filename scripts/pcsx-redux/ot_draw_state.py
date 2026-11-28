#!/usr/bin/env python3
"""Census of the GPU draw state retail's ordering tables carry between packets.

The GPU keeps a draw mode across the packet stream: the texpage register (page,
ABR, colour depth, dither), the drawing area and offset, the texture window and
the mask bits. A packet that does not set a field draws with whatever the
stream left in it:

- the register starts at the DR_ENV's `E1` word (`PutDrawEnv` sends it ahead of
  the table) and every `E1` packet rewrites it;
- a textured polygon's page halfword rewrites bits 0..8 and 11 (page, ABR,
  depth), not the dither bit (PCSX-Redux `SoftRenderer::texturePage`, called
  from every textured polygon);
- an untextured polygon or line takes its semi-transparency mode (ABR) from the
  register, a sprite (`0x60..0x7F`) its page as well, and every polygon its
  dither bit.

So a walk that tracks `E1` packets alone misreads an untextured semi primitive
drawn after a textured one. This walks the drawn tables of save states (or RAM
images) and reports where the carried state differs from that reading:

- `abr`: untextured semi polygons whose carried ABR differs from the last `E1`'s;
- `page`: textured sprites whose carried page differs from the last `E1`'s;
- `dither`: shaded or texture-blended polygons drawn after an `E1` that turned
  the dither bit off, with the buckets they link in;
- `env`: `E2` (texture window), `E3`..`E5` (area, offset) and `E6` (mask)
  packets inside a table.

A table whose packets do not parse (a stale buffer the frame has not rebuilt)
is skipped. Read-only; prints addresses and counts, no Sony bytes.

Usage:
    ot_draw_state.py                       # every retail library state
    ot_draw_state.py --filter battle_      # labels containing this text
    ot_draw_state.py STATE.sstate ...      # given PCSX-Redux states
    ot_draw_state.py --ram ram_00608.bin   # a cast-sequence RAM image (4096 buckets)
    ot_draw_state.py --detail              # one line per differing primitive
"""

from __future__ import annotations

import argparse
import gzip
import struct
import sys
import tomllib
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RAM_LEN = 0x20_0000
REDUX_RAM_BLOB = 0x80_0000
SCRATCH_LEN = 0x400
DOUBLE_BUFFER = 0x7BF30
STRIDE = 0x74
DRENV = 0x1C
OT = 0x70
OT_LEN_SCRATCH = 0x3A6
SKIP_LABELS = ("delilas", "shiny_refactor", "cort_evolved_pre_battle")


def varint(b: bytes, i: int) -> tuple[int, int]:
    r = s = 0
    while True:
        c = b[i]
        i += 1
        r |= (c & 0x7F) << s
        s += 7
        if c < 0x80:
            return r, i


def fields(b: bytes, lo: int, hi: int, want: set[int], out: dict, depth: int = 0) -> None:
    """Length-delimited protobuf fields of the wanted sizes (RAM, scratchpad)."""
    i = lo
    while i < hi:
        try:
            key, i = varint(b, i)
        except IndexError:
            return
        wt = key & 7
        if wt == 0:
            _, i = varint(b, i)
        elif wt == 1:
            i += 8
        elif wt == 5:
            i += 4
        elif wt == 2:
            n, i = varint(b, i)
            if i + n > hi:
                return
            if n in want:
                out.setdefault(n, i)
            elif n > 64 and depth < 6:
                fields(b, i, i + n, want, out, depth + 1)
            i += n
        else:
            return


def state_images(path: Path) -> tuple[bytes, int] | None:
    data = path.read_bytes()
    if data[:2] == b"\x1f\x8b":
        data = gzip.decompress(data)
    out: dict[int, int] = {}
    fields(data, 0, len(data), {REDUX_RAM_BLOB, RAM_LEN, SCRATCH_LEN}, out)
    at = out.get(REDUX_RAM_BLOB, out.get(RAM_LEN))
    if at is None:
        return None
    ram = data[at : at + RAM_LEN]
    length = 4096
    if SCRATCH_LEN in out:
        length = struct.unpack_from("<H", data, out[SCRATCH_LEN] + OT_LEN_SCRATCH)[0] or 4096
    return ram, length


def s11(v: int) -> int:
    return ((v & 0x7FF) ^ 0x400) - 0x400


def walk(ram: bytes, record: int, length: int, detail: list | None) -> Counter | None:
    w = lambda o: struct.unpack_from("<I", ram, o & 0x1F_FFFC)[0]
    rec = DOUBLE_BUFFER + record * STRIDE
    head = w(rec + OT)
    if not 0x8000_0000 <= head < 0x8020_0000:
        return None
    reg = 0
    for k in range(1, 16):
        word = w(rec + DRENV + 4 * k)
        if word >> 24 == 0xE1:
            reg = word & 0x3FFF
    e1 = reg
    base = head & 0x1F_FFFC
    off, bucket = base + (length - 1) * 4, length - 1
    tally: Counter = Counter()
    for _ in range(400_000):
        if base <= off < base + length * 4:
            bucket = (off - base) // 4
        tag = w(off)
        n, nxt = tag >> 24, tag & 0xFF_FFFF
        ws = [w(off + 4 + 4 * k) for k in range(n)]
        i = 0
        while i < n:
            c = ws[i] >> 24
            if c == 0xE1:
                reg = e1 = ws[i] & 0x3FFF
                i += 1
            elif 0xE2 <= c <= 0xE6:
                tally[f"env {c:02X}"] += 1
                i += 1
            elif c == 0 or (c == 0x02 and i + 3 <= n):
                i += 3 if c == 0x02 else 1
            elif 0x20 <= c <= 0x3F:
                quad, tex, shaded, semi = c & 8, c & 4, c & 0x10, c & 2
                nv = 4 if quad else 3
                plen = 1 + nv * (1 + bool(tex) + bool(shaded)) - bool(shaded)
                if i + plen > n:
                    tally["malformed"] += 1
                    break
                p = ws[i : i + plen]
                tally["polys"] += 1
                if shaded or (tex and not c & 1):
                    tally["ditherable"] += 1
                    if not reg & 0x200:
                        tally["dither off"] += 1
                        tally[f"dither off bucket {bucket}"] += 1
                if tex:
                    page = p[4 if not shaded else 5] >> 16
                    reg = (reg & ~0x9FF) | (page & 0x9FF)
                elif semi:
                    tally["untextured semi"] += 1
                    if (reg >> 5) & 3 != (e1 >> 5) & 3:
                        tally["abr"] += 1
                        if detail is not None:
                            xy = [p[k] for k in (range(1, plen, 2) if shaded else range(1, plen))]
                            xs = [s11(v) for v in xy]
                            ys = [s11(v >> 16) for v in xy]
                            detail.append(
                                f"  abr: {c:#04x} bucket {bucket} carried ABR {(reg >> 5) & 3}, "
                                f"E1 ABR {(e1 >> 5) & 3}, x {min(xs)}..{max(xs)} y {min(ys)}..{max(ys)}"
                            )
                i += plen
            elif 0x60 <= c <= 0x7F:
                tex = c & 4
                plen = 2 + bool(tex) + ((c >> 3) & 3 == 0)
                if i + plen > n:
                    tally["malformed"] += 1
                    break
                if tex:
                    tally["sprites"] += 1
                    if (reg ^ e1) & 0x19F:
                        tally["page"] += 1
                i += plen
            elif 0x40 <= c <= 0x5F:
                # A line ends the walk of its packet (its length runs to a
                # terminator word); none of the counts reads past it.
                break
            else:
                tally["malformed"] += 1
                break
        if nxt == 0xFF_FFFF or nxt & 0x80_0000:
            break
        off = nxt & 0x1F_FFFC
    if tally["polys"] < 20 or tally["malformed"] * 50 > tally["polys"]:
        return None
    return tally


def library(filt: str | None) -> list[tuple[str, Path]]:
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())
    lib = REPO / "saves" / "library" / "pcsx-redux"
    out = []
    for s in manifest.get("scenarios", []):
        label = s.get("label", "")
        if any(x in label for x in SKIP_LABELS) or s.get("resident_patch"):
            continue
        if filt and filt not in label:
            continue
        fp = s.get("backup_fingerprint")
        if fp and (lib / f"{fp}.sstate").exists():
            out.append((label, lib / f"{fp}.sstate"))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("states", nargs="*", type=Path)
    ap.add_argument("--filter", help="library labels containing this text")
    ap.add_argument("--ram", nargs="*", type=Path, default=[], help="raw 2 MiB RAM images")
    ap.add_argument("--detail", action="store_true")
    a = ap.parse_args()
    jobs: list[tuple[str, tuple[bytes, int] | None]] = []
    for p in a.ram:
        jobs.append((p.name, (p.read_bytes()[:RAM_LEN], 4096)))
    named = [(p.name, p) for p in a.states] or ([] if a.ram else library(a.filter))
    total: Counter = Counter()
    for label, path in named + [(lab, None) for lab, _ in jobs]:
        images = dict(jobs).get(label) if path is None else state_images(path)
        if images is None:
            continue
        ram, length = images
        for record in (0, 1):
            detail: list | None = [] if a.detail else None
            t = walk(ram, record, length, detail)
            if t is None:
                continue
            total["tables"] += 1
            for k in ("polys", "untextured semi", "abr", "sprites", "page", "ditherable", "dither off"):
                total[k] += t[k]
            for k in t:
                if k.startswith("env"):
                    total[k] += t[k]
            off = sorted(int(k.split()[-1]) for k in t if k.startswith("dither off bucket"))
            notes = []
            if t["abr"]:
                notes.append(f"abr {t['abr']}/{t['untextured semi']}")
            if t["page"]:
                notes.append(f"page {t['page']}/{t['sprites']}")
            if t["dither off"]:
                notes.append(f"dither-off {t['dither off']}/{t['ditherable']} buckets {off[0]}..{off[-1]}")
            env = [f"{k[4:]} x{t[k]}" for k in sorted(t) if k.startswith("env")]
            if env:
                notes.append("env " + " ".join(env))
            if notes:
                print(f"{label} record {record}: " + "; ".join(notes))
                for line in detail or []:
                    print(line)
    print("total: " + ", ".join(f"{k} {v}" for k, v in total.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
