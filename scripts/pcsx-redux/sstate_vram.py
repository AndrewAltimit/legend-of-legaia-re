#!/usr/bin/env python3
"""Pull the 1 MiB GPU VRAM image out of a PCSX-Redux save state.

The Lua build this repo drives has no `PCSX.getVRAM()`, but every save state
(`ckpt_<f>.rawsstate` from autorun_w5b_field_watch.lua's LEGAIA_CKPTS, or a
gzipped library `.sstate`) carries VRAM as one length-delimited protobuf field
of exactly 1024 * 512 halfwords. This walks the message tree the same way
`legaia-pcsxr` finds main RAM (by size, not by field number) and writes that
field out raw: 1024 x 512 little-endian halfwords, row-major.

Usage: sstate_vram.py <state> <out.bin>
"""

from __future__ import annotations

import gzip
import sys
from pathlib import Path

VRAM_LEN = 1024 * 512 * 2


def varint(b: bytes, i: int) -> tuple[int, int]:
    r = s = 0
    while True:
        c = b[i]
        i += 1
        r |= (c & 0x7F) << s
        s += 7
        if c < 0x80:
            return r, i


def find(b: bytes, lo: int, hi: int, depth: int) -> int | None:
    i = lo
    while i < hi:
        try:
            key, i = varint(b, i)
        except IndexError:
            return None
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
                return None
            if n == VRAM_LEN:
                return i
            if n > 64 and depth < 6:
                hit = find(b, i, i + n, depth + 1)
                if hit is not None:
                    return hit
            i += n
        else:
            return None
    return None


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    data = Path(sys.argv[1]).read_bytes()
    if data[:2] == b"\x1f\x8b":
        data = gzip.decompress(data)
    at = find(data, 0, len(data), 0)
    if at is None:
        print("no 1 MiB field in this state", file=sys.stderr)
        return 1
    Path(sys.argv[2]).write_bytes(data[at : at + VRAM_LEN])
    print(f"wrote VRAM ({VRAM_LEN} bytes) from state offset {at:#x}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
