#!/usr/bin/env python3
"""Extract the 1 MiB VRAM blob from a PCSX-Redux save state.

PCSX-Redux save states are gzipped protobuf (schema in
`src/core/sstate.h`). The GPU.vram field is `FixedBytes<0x100000>` at
protobuf field 3 of the inner `gpu` message. The canonical wire-tag
pattern for "field 3, wire-type 2 (length-delimited), length 0x100000"
is `0x1A 0x80 0x80 0x40`; that 4-byte tag is the only signature we
need to scan for, and the 1 MiB that follows is the raw BGR555 VRAM.

This is the equivalent of `mednafen-state vram-dump` but for the
PCSX-Redux side, so we can ground-truth-pin sprite sources by looking
at actual VRAM contents at a parked save-state - the same way we use
mednafen-state's GPU section as a byte-exact oracle for the engine
port's VRAM state.

Usage:
    extract_vram_from_sstate.py <sstate_path> <out_dir>
    # writes <out_dir>/vram.bin (1 MiB raw BGR555, 1024x512x2 bytes)

Pair with `decode_vram.py` to render the VRAM as a 1024x512 PNG.
"""
import gzip
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: extract_vram_from_sstate.py <sstate_path> <out_dir>")
    sstate_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)

    raw = sstate_path.read_bytes()
    try:
        blob = gzip.decompress(raw)
        print(f"decompressed {len(raw)} -> {len(blob)} bytes")
    except Exception:
        blob = raw
        print(f"not gzipped; using raw {len(blob)} bytes")

    # Tag for protobuf "field 3, wire-type 2, length 0x100000".
    pattern = bytes([0x1A, 0x80, 0x80, 0x40])
    p = blob.find(pattern)
    if p < 0:
        raise SystemExit("VRAM section tag not found in save state")
    end = p + 4 + 0x100000
    if end > len(blob):
        raise SystemExit(
            f"VRAM tag found at 0x{p:08X} but blob is too short for 1 MiB payload"
        )
    vram = blob[p + 4 : end]
    out = out_dir / "vram.bin"
    out.write_bytes(vram)
    print(f"wrote {out} ({len(vram)} bytes) from sstate offset 0x{p:08X}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
