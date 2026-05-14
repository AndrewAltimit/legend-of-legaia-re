#!/usr/bin/env python3
"""
Standalone POLY_FT4 / POLY_GT4 emitter hunter for raw MIPS code blobs.

Sweeps a binary that was extracted from PSX main RAM (e.g. by
`scripts/extract-mednafen-overlay.py`) for sites that build a PSX-GPU
primitive packet's `code` byte. Textured-quad codes that matter:

  0x2C  POLY_FT4 (textured, flat, opaque)
  0x2D  POLY_FT4 (textured, flat, semi-transparent)
  0x2E  POLY_FT4 (textured, Gouraud, opaque)
  0x2F  POLY_FT4 (textured, Gouraud, semi-transparent)
  0x3C  POLY_GT4 (textured, Gouraud, opaque)
  0x3D  POLY_GT4 (textured, Gouraud, semi-transparent)

Two compiler-emitted shapes dominate:

  Pattern A  (libgs setcode macro):
      ori   $r, $zero, 0xKK              ; KK in {0x2C..0x2F, 0x3C..0x3F}
      sb    $r, +off($base)

  Pattern B  (inline code word with cmd in the high byte):
      lui   $r, 0xKK00                   ; KK in {0x2C..0x2F, 0x3C..0x3F}
      ...optional or/ori with RGB color in low 24 bits...
      sw    $r, +off($base)

A per-function aggregation is impossible from raw bytes alone (we don't
have function boundaries), but a heuristic clustering by 256-byte windows
catches the typical case where one function houses several emitter sites
within ~1 KB.

This is the equivalent of `ghidra/scripts/find_addprim_emitters.py` for
captured overlay blobs that haven't been imported into a Ghidra project.
The Ghidra version stays authoritative when functions are recognised; this
script unblocks first-pass investigation of newly-captured overlay regions
where no Ghidra project yet exists.

USAGE
    scripts/find-addprim-emitters.py overlay.bin --base 0x801C0000

The output groups hits per cluster (rounded to 0x100). Combine with
`scripts/extract-mednafen-overlay.py` to widen the capture window before
running this hunter.
"""

import argparse
import struct
import sys
from pathlib import Path


# MIPS-LE opcode masks. All MIPS-I instructions in PSX code are 4 bytes,
# little-endian. The bytes we receive from the binary are already in
# memory order, so we use `struct.unpack("<I", ...)` to recover the word.

# Cmd bytes worth flagging.
POLY_FT4 = {0x2C, 0x2D, 0x2E, 0x2F}
POLY_GT4 = {0x3C, 0x3D, 0x3E, 0x3F}
TEXTURED_QUAD_CODES = POLY_FT4 | POLY_GT4

CODE_NAME = {
    0x2C: "POLY_FT4   flat       opaque",
    0x2D: "POLY_FT4   flat       semi-trans",
    0x2E: "POLY_FT4   Gouraud    opaque",
    0x2F: "POLY_FT4   Gouraud    semi-trans",
    0x3C: "POLY_GT4   Gouraud    opaque",
    0x3D: "POLY_GT4   Gouraud    semi-trans",
    0x3E: "POLY_GT4   Gouraud    opaque   (alt)",
    0x3F: "POLY_GT4   Gouraud    semi-trans (alt)",
}


def parse_addr(s: str) -> int:
    return int(s, 16) if s.lower().startswith("0x") else int(s)


def disasm_imm(word: int):
    """Recover (kind, imm) for ori/addiu/lui instructions that load an
    immediate. Returns None for anything else."""
    op = word >> 26
    if op == 0x0F:  # LUI rt, imm16
        return ("lui", (word & 0xFFFF) << 16)
    if op == 0x0D:  # ORI rt, rs, imm16  (rs == 0 makes it a "li-low")
        rs = (word >> 21) & 0x1F
        return ("ori", word & 0xFFFF) if rs == 0 else ("ori-rs", word & 0xFFFF)
    if op == 0x09:  # ADDIU rt, rs, imm16  (rs == 0 makes it a "li")
        rs = (word >> 21) & 0x1F
        imm = word & 0xFFFF
        if imm & 0x8000:  # sign-extend
            imm -= 0x10000
        return ("addiu", imm) if rs == 0 else ("addiu-rs", imm)
    return None


def is_store_byte(word: int) -> bool:
    """SB rt, off(rs)?"""
    return (word >> 26) == 0x28


def is_store_word(word: int) -> bool:
    """SW rt, off(rs)?"""
    return (word >> 26) == 0x2B


def scan(blob: bytes, base: int):
    """Sweep `blob` (loaded at virtual address `base`) for POLY_FT4/POLY_GT4
    emit sites. Yields dicts."""
    # Per-register cache of the last-loaded immediate. Tracks (kind, imm).
    last = {}

    for i in range(0, len(blob) - 3, 4):
        word = struct.unpack_from("<I", blob, i)[0]
        rt = (word >> 16) & 0x1F
        addr = base + i

        # Track immediate loads.
        di = disasm_imm(word)
        if di is not None:
            kind, imm = di
            last[rt] = (kind, imm, addr)
            continue

        # Pattern A: ORI of a small immediate followed by SB. The ORI
        # already populated `last[rt]`; if THIS is the SB and the stored
        # reg's last immediate is in TEXTURED_QUAD_CODES, fire.
        if is_store_byte(word):
            srcreg = (word >> 16) & 0x1F
            prev = last.get(srcreg)
            if prev is None:
                continue
            kind, imm, src_addr = prev
            if kind == "ori" and imm in TEXTURED_QUAD_CODES:
                base_reg = (word >> 21) & 0x1F
                off = word & 0xFFFF
                if off & 0x8000:
                    off -= 0x10000
                yield dict(
                    pattern="A",
                    pc=addr,
                    src_pc=src_addr,
                    cmd=imm,
                    base_reg=base_reg,
                    off=off,
                )

        # Pattern B: LUI of 0xKK00xxxx (KK is the cmd byte) where the
        # written word is later SW'd. We catch the LUI's residual in
        # `last` and fire when we see the SW.
        if is_store_word(word):
            srcreg = (word >> 16) & 0x1F
            prev = last.get(srcreg)
            if prev is None:
                continue
            kind, imm, src_addr = prev
            if kind == "lui":
                cmd = (imm >> 24) & 0xFF
                if cmd in TEXTURED_QUAD_CODES:
                    base_reg = (word >> 21) & 0x1F
                    off = word & 0xFFFF
                    if off & 0x8000:
                        off -= 0x10000
                    yield dict(
                        pattern="B",
                        pc=addr,
                        src_pc=src_addr,
                        cmd=cmd,
                        base_reg=base_reg,
                        off=off,
                    )

    # The "lui in low 16 bits" trick: some compilers emit `lui $r,0xKK00`
    # with $r != the SW source, so the residual gets clobbered. The
    # above does miss those - in practice the Ghidra hunter catches
    # them via its broader inter-instruction tracker. For first-pass
    # surfacing this single-register heuristic is good enough.


def cluster_hits(hits, window=0x100):
    """Group hits by `pc // window`, yielding (cluster_pc, [hits])."""
    out = {}
    for h in hits:
        c = h["pc"] & ~(window - 1)
        out.setdefault(c, []).append(h)
    for cpc, hs in sorted(out.items()):
        yield cpc, hs


REG_NAMES = [
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "fp", "ra",
]


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("blob", help="raw overlay/SCUS binary captured from RAM")
    ap.add_argument("--base", type=parse_addr, default=0x801C0000,
                    help="virtual address the blob's offset 0 maps to "
                         "(default: 0x801C0000)")
    ap.add_argument(
        "--cluster",
        type=parse_addr,
        default=0x100,
        help="cluster hits by this byte window (default: 0x100)",
    )
    ap.add_argument(
        "--codes",
        default="",
        help="restrict cmd bytes (comma-separated hex). "
             "default: all 8 of 0x2C-0x2F,0x3C-0x3F.",
    )
    args = ap.parse_args()

    if args.codes:
        wanted = {int(c, 16) for c in args.codes.split(",")}
    else:
        wanted = TEXTURED_QUAD_CODES

    data = Path(args.blob).read_bytes()
    print(f"[info] scanning {args.blob}: {len(data):,} bytes "
          f"at base 0x{args.base:08X} "
          f"(end 0x{args.base + len(data):08X})")

    hits = [h for h in scan(data, args.base) if h["cmd"] in wanted]
    print(f"[info] {len(hits)} hit(s) total")

    # Per-cluster report.
    for cpc, hs in cluster_hits(hits, args.cluster):
        unique_cmds = sorted({h["cmd"] for h in hs})
        unique_sites = sorted({h["pc"] for h in hs})
        print(f"\n  cluster 0x{cpc:08X}: {len(hs)} hit(s), "
              f"{len(unique_sites)} unique PC(s), "
              f"cmds={[hex(c) for c in unique_cmds]}")
        for h in hs:
            br = REG_NAMES[h["base_reg"]] if h["base_reg"] < 32 else "?"
            note = CODE_NAME.get(h["cmd"], "")
            print(f"    [{h['pattern']}] PC=0x{h['pc']:08X} "
                  f"src=0x{h['src_pc']:08X} "
                  f"cmd=0x{h['cmd']:02X} ({note}) "
                  f"dst=+0x{h['off']:X}({br})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
