#!/usr/bin/env python3
"""
gpu_packets.py - shared PSX GPU primitive decode helpers.

A small, dependency-free library for the recurring task of decoding PSX GPU
primitives out of (a) a raw main-RAM image (the libgpu ordering-table packet
form, which has a 4-byte tag prefix) and (b) raw MIPS code (matching the
primitive `code` byte materialised as an immediate). It exists so the prim-pool
decoders scattered across `scripts/` (`analyze-walk-ground-tiles.py`,
`find-addprim-emitters.py`, the slot-4 viewers, ...) stop re-deriving the same
primitive tables and VRAM-coordinate math.

Everything here is the public PSX GPU spec (GP0 primitive layout + libgpu
`POLY_*` struct shapes from PSX-SPX / the PsyQ libgpu headers); no Sony Legaia
bytes are encoded. RAM/ROM images the callers pass in stay local (gitignored).

## libgpu ordering-table packet form

libgpu builds primitives as linked-list nodes: a 4-byte `tag`
(`next_ptr<<0 | len<<24`) followed by the GP0 words. The first GP0 word is
`code<<24 | rgb` (or `code<<24 | r,g,b` of vertex 0 for Gouraud). For textured
polys the CLUT id (CBA) is packed in the high half of vertex 0's UV word and the
TPAGE in the high half of vertex 1's UV word:

    POLY_FT4 (0x28 bytes):
      +0x00 tag            +0x04 code|r,g,b
      +0x08 xy0            +0x0C uv0 | clut<<16
      +0x10 xy1            +0x14 uv1 | tpage<<16
      +0x18 xy2            +0x1C uv2
      +0x20 xy3            +0x24 uv3

The `*_TInfo` offsets below give `(clut_word_off, tpage_word_off, first_uv_off)`
relative to the packet start for the textured primitives.

## VRAM coordinate math

- CBA (CLUT id) word: bits 0..=5 = `fb_x / 16`, bits 6..=14 = `fb_y`.
- TPAGE word: bits 0..=3 = `fb_x / 64`, bit 4 = `fb_y / 256`, bits 7..=8 = bpp
  (0 = 4bpp, 1 = 8bpp, 2 = 15bpp).
"""

from __future__ import annotations

import struct
from dataclasses import dataclass


# --- Primitive code table -------------------------------------------------
#
# GP0 polygon/sprite/line `code` bytes (the top byte of the first GP0 word).
# Flags: 't' textured, 'g' gouraud, 'q' quad (vs tri), 's' semi-transparent,
# 'r' raw-texture (sprite/poly texture blending off).

POLY_FT4 = {0x2C, 0x2D, 0x2E, 0x2F}
POLY_GT4 = {0x3C, 0x3D, 0x3E, 0x3F}
POLY_FT3 = {0x24, 0x25, 0x26, 0x27}
POLY_GT3 = {0x34, 0x35, 0x36, 0x37}
SPRT = {0x64, 0x65, 0x66, 0x67}  # variable-size textured sprite
TEXTURED_QUAD_CODES = POLY_FT4 | POLY_GT4
TEXTURED_TRI_CODES = POLY_FT3 | POLY_GT3
TEXTURED_CODES = TEXTURED_QUAD_CODES | TEXTURED_TRI_CODES | SPRT

# code -> human label.
CODE_LABELS = {
    0x20: "POLY_F3", 0x22: "POLY_F3 semi",
    0x24: "POLY_FT3", 0x25: "POLY_FT3 semi", 0x26: "POLY_FT3 raw", 0x27: "POLY_FT3 semi-raw",
    0x28: "POLY_F4", 0x2A: "POLY_F4 semi",
    0x2C: "POLY_FT4", 0x2D: "POLY_FT4 semi", 0x2E: "POLY_FT4 raw", 0x2F: "POLY_FT4 semi-raw",
    0x30: "POLY_G3", 0x32: "POLY_G3 semi",
    0x34: "POLY_GT3", 0x35: "POLY_GT3 semi", 0x36: "POLY_GT3 raw", 0x37: "POLY_GT3 semi-raw",
    0x38: "POLY_G4", 0x3A: "POLY_G4 semi",
    0x3C: "POLY_GT4", 0x3D: "POLY_GT4 semi", 0x3E: "POLY_GT4 raw", 0x3F: "POLY_GT4 semi-raw",
    0x64: "SPRT", 0x65: "SPRT semi", 0x66: "SPRT raw", 0x67: "SPRT semi-raw",
}

# libgpu packet byte length (including the 4-byte tag) per primitive family.
# Keyed by the family base; use prim_packet_len(code) to look up by code byte.
_PACKET_LEN = {
    "POLY_F3": 0x10, "POLY_FT3": 0x1C, "POLY_G3": 0x18, "POLY_GT3": 0x24,
    "POLY_F4": 0x18, "POLY_FT4": 0x28, "POLY_G4": 0x24, "POLY_GT4": 0x30,
    "SPRT": 0x14,
}

# (clut_word_off, tpage_word_off, first_uv_off) relative to packet start, for the
# textured primitives. UVs are the low 16 bits at each uv word.
_TINFO = {
    "POLY_FT3": (0x0C, 0x14, 0x0C),
    "POLY_GT3": (0x10, 0x18, 0x10),
    "POLY_FT4": (0x0C, 0x14, 0x0C),
    "POLY_GT4": (0x10, 0x1C, 0x10),
    "SPRT": (0x0C, None, 0x0C),
}


def family(code: int) -> str | None:
    """Primitive family name (`POLY_FT4`, ...) for a `code` byte, or None."""
    label = CODE_LABELS.get(code)
    return label.split()[0] if label else None


def prim_packet_len(code: int) -> int | None:
    """libgpu packet length in bytes (tag included) for a `code` byte."""
    fam = family(code)
    return _PACKET_LEN.get(fam) if fam else None


# --- VRAM coordinate helpers ----------------------------------------------

def cba_to_fb(clut: int) -> tuple[int, int]:
    """CLUT id (CBA word) -> VRAM framebuffer (x, y) of the palette row."""
    return (clut & 0x3F) * 16, (clut >> 6) & 0x1FF


def tpage_to_fb(tpage: int) -> tuple[int, int, int]:
    """TPAGE word -> (fb_x, fb_y, bpp_bits) where bpp_bits 0/1/2 = 4/8/15bpp."""
    fb_x = (tpage & 0x0F) * 64
    fb_y = ((tpage >> 4) & 1) * 256
    bpp = (tpage >> 7) & 0x03
    return fb_x, fb_y, bpp


BPP_NAMES = {0: "4bpp", 1: "8bpp", 2: "15bpp", 3: "15bpp(alt)"}


# --- libgpu packet decode (from a RAM image) ------------------------------

@dataclass
class Packet:
    """A decoded textured libgpu primitive."""
    off: int            # byte offset of the tag within the source buffer
    code: int           # GP0 code byte
    family: str         # family name, e.g. "POLY_FT4"
    clut: int           # raw CBA word
    tpage: int | None   # raw tpage word (None for SPRT)
    uvs: list[tuple[int, int]]   # per-vertex (u, v)
    xys: list[tuple[int, int]]   # per-vertex signed (x, y)

    def clut_fb(self) -> tuple[int, int]:
        return cba_to_fb(self.clut)

    def tpage_fb(self) -> tuple[int, int, int] | None:
        return tpage_to_fb(self.tpage) if self.tpage is not None else None


def _s16(v: int) -> int:
    return v - 0x10000 if v & 0x8000 else v


def decode_packet(buf: bytes, off: int) -> Packet | None:
    """Decode the textured libgpu primitive whose tag is at `buf[off]`.

    Returns None if `off` doesn't hold a recognised textured primitive or the
    packet would run past the buffer. The `code` byte is read from the high
    byte of the first GP0 word (at `off+0x04+3`).
    """
    if off + 8 > len(buf):
        return None
    code = buf[off + 0x07]
    fam = family(code)
    if fam not in _TINFO:
        return None
    plen = _PACKET_LEN[fam]
    if off + plen > len(buf):
        return None
    clut_off, tpage_off, first_uv = _TINFO[fam]
    clut = struct.unpack_from("<I", buf, off + clut_off)[0] >> 16
    tpage = None
    if tpage_off is not None:
        tpage = struct.unpack_from("<I", buf, off + tpage_off)[0] >> 16

    # Vertex count and per-vertex stride differ by family; rather than encode
    # every layout we read the UV/XY pairs that the textured families share.
    # For the poly families the (xy, uv) pairs are interleaved after the code
    # word; gouraud variants insert a colour word before each xy.
    uvs: list[tuple[int, int]] = []
    xys: list[tuple[int, int]] = []
    gouraud = fam.startswith("POLY_G")
    quad = fam.endswith("4")
    nverts = 4 if quad else 3
    if fam == "SPRT":
        xy = struct.unpack_from("<I", buf, off + 0x08)[0]
        uv = struct.unpack_from("<I", buf, off + 0x0C)[0]
        xys.append((_s16(xy & 0xFFFF), _s16((xy >> 16) & 0xFFFF)))
        uvs.append((uv & 0xFF, (uv >> 8) & 0xFF))
    else:
        # Walk vertices. Layout per vertex: [colour(g only)] xy uv.
        p = off + 0x08
        for _ in range(nverts):
            if gouraud and _ != 0:
                p += 4  # per-vertex colour word (vertex 0's colour is in code word)
            xy = struct.unpack_from("<I", buf, p)[0]
            uv = struct.unpack_from("<I", buf, p + 4)[0]
            xys.append((_s16(xy & 0xFFFF), _s16((xy >> 16) & 0xFFFF)))
            uvs.append((uv & 0xFF, (uv >> 8) & 0xFF))
            p += 8
        # first_uv sanity: the decoded uv0 must sit at first_uv.
        assert off + first_uv == off + 0x0C or gouraud

    return Packet(off=off, code=code, family=fam, clut=clut, tpage=tpage, uvs=uvs, xys=xys)


def iter_textured_packets(
    ram: bytes,
    clut: int | None = None,
    tpage: int | None = None,
    codes: set[int] | None = None,
    step: int = 4,
):
    """Yield every textured [`Packet`] in `ram`, optionally filtered.

    `clut` / `tpage`: keep only packets whose CBA / TPAGE word matches.
    `codes`: restrict to these GP0 code bytes (default: all textured codes).
    `step`: scan stride (4 = word-aligned, the libgpu packet alignment).
    """
    want = codes if codes is not None else TEXTURED_CODES
    for off in range(0, len(ram), step):
        code = ram[off + 0x07] if off + 8 <= len(ram) else None
        if code not in want:
            continue
        pkt = decode_packet(ram, off)
        if pkt is None:
            continue
        if clut is not None and pkt.clut != clut:
            continue
        if tpage is not None and (pkt.tpage is None or pkt.tpage != tpage):
            continue
        yield pkt


# --- MIPS-code immediate matching -----------------------------------------

def is_textured_code_immediate(word: int) -> int | None:
    """If a 32-bit MIPS instruction materialises a textured-prim code byte as
    an immediate (`ori rt, rs, 0xKK` or `lui rt, 0xKK00`), return that code,
    else None. Used to locate `addPrim` emitters in raw code blobs.
    """
    op = word >> 26
    if op == 0x0D:  # ori
        imm = word & 0xFFFF
        if imm in TEXTURED_CODES:
            return imm
    elif op == 0x0F:  # lui
        hi = (word >> 8) & 0xFF
        if (word & 0xFF) == 0 and hi in TEXTURED_CODES:
            return hi
    return None


if __name__ == "__main__":
    # Self-test on a synthetic POLY_FT4 packet.
    import sys

    pkt_bytes = bytearray(0x28)
    pkt_bytes[0x07] = 0x2C  # code
    struct.pack_into("<I", pkt_bytes, 0x08, (10 << 16) | 20)        # xy0
    struct.pack_into("<I", pkt_bytes, 0x0C, (0x7C40 << 16) | 0x0000)  # uv0|clut
    struct.pack_into("<I", pkt_bytes, 0x14, (0x001A << 16) | 0x1F00)  # uv1|tpage
    p = decode_packet(bytes(pkt_bytes), 0)
    assert p and p.family == "POLY_FT4"
    assert p.clut == 0x7C40 and p.tpage == 0x001A
    assert p.clut_fb() == (0, 497)
    assert p.tpage_fb() == (640, 256, 0)
    assert is_textured_code_immediate((0x0D << 26) | 0x2C) == 0x2C
    print("gpu_packets self-test OK", file=sys.stderr)
