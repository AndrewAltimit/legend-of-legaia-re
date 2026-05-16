#!/usr/bin/env python3
"""Scan a PSX main-RAM dump for TIM headers, then byte-grep the
extracted PROT corpus to pin each TIM's on-disc source entry.

Methodology validated by pinning the title-overlay save-menu sprites
(vaddr 0x801E5120 and 0x801EE120) to PROT[899] file offsets 0x16908
and 0x1F908 — see memory file `project_title_tims_in_overlay`.

Usage:
    scripts/scan_tims_and_match_prot.py <ram_dump.bin> [options]

Options:
    --base 0x80000000   Base PSX virtual address of the RAM dump
                        (default: 0x80000000, the main-RAM start).
    --prot-dir extracted/PROT
                        Directory of per-entry PROT files (numbered
                        0000_*.BIN .. NNNN_*.BIN).
    --lzs-dir <dir>     Directory of pre-LZS-decoded PROT sections
                        (one subdir per entry; populated by
                        `lzs-decode container <prot> <out_dir>`).
                        Optional - extends the corpus search.
    --filter-vaddr LO-HI
                        Restrict TIM-scan output to vaddrs in [LO, HI).
                        Useful when a particular RAM region is suspect.
    --extract-to <dir>  Also extract each found TIM as a standalone .tim
                        in <dir>/.
    --sig-mode {full,pixel}
                        Signature mode for PROT matching.
                        - `full`: first 256 bytes of the TIM (matches
                          when the runtime hasn't fixed up the CLUT
                          RECT fields, e.g. the TIM lives inside an
                          overlay binary loaded verbatim).
                        - `pixel`: 128 bytes from deep in the pixel
                          block (skips the CLUT block, so it matches
                          even when the runtime has patched fb_x/fb_y).
                        Default: `full`.

The runtime patches CLUT_x / CLUT_y at +0x0C..+0x0F of each TIM
when relocating CLUTs at upload time — see the publisher-logo case
in `crates/asset/src/init_pak.rs`. Both signature modes work on
their respective input populations; use `--sig-mode pixel` when
`full` finds no matches.
"""

from __future__ import annotations
import argparse
import os
import struct
import sys
from glob import glob
from pathlib import Path


def parse_tim(buf: bytes, off: int) -> dict | None:
    """Parse a candidate TIM at `off` in `buf`. Return None if invalid."""
    if off + 20 > len(buf):
        return None
    magic, flag = struct.unpack_from('<II', buf, off)
    if magic != 0x10:
        return None
    bpp = flag & 0x07
    has_clut = bool(flag & 0x08)
    if bpp > 3:
        return None
    pos = off + 8
    clut_start = pos if has_clut else None
    if has_clut:
        if pos + 12 > len(buf):
            return None
        clen, cx, cy, nclut, nccluts = struct.unpack_from('<IHHHH', buf, pos)
        if clen != 12 + nclut * nccluts * 2:
            return None
        if nclut == 0 or nccluts == 0 or nclut > 4096 or nccluts > 4096:
            return None
        pos += clen
    pix_start = pos
    if pos + 12 > len(buf):
        return None
    plen, px, py, pw, ph = struct.unpack_from('<IHHHH', buf, pos)
    if pw == 0 or ph == 0 or pw > 4096 or ph > 4096:
        return None
    if plen != 12 + 2 * pw * ph:
        return None
    real_w = {0: pw * 4, 1: pw * 2, 2: pw, 3: pw * 2 // 3}[bpp]
    return {
        'off': off,
        'clut_start': clut_start,
        'pix_start': pix_start,
        'size': pos + plen - off,
        'bpp': bpp,
        'has_clut': has_clut,
        'pix_rect': (px, py, pw, ph),
        'real_w': real_w,
        'real_h': ph,
    }


def scan_ram(ram: bytes) -> list[dict]:
    """Find every valid TIM in `ram`. Return list of parse-result dicts."""
    out = []
    i = 0
    while True:
        j = ram.find(b'\x10\x00\x00\x00', i)
        if j < 0:
            break
        t = parse_tim(ram, j)
        if t is not None:
            out.append(t)
        i = j + 1
    return out


def signature(ram: bytes, t: dict, mode: str) -> bytes:
    """Build a PROT-corpus search signature for TIM `t`.

    `full`: first 256 bytes of the TIM (matches in overlay-resident TIMs).
    `pixel`: 128 bytes from middle of pixel block (skips CLUT, matches
             even after CLUT-fixup).
    """
    if mode == 'full':
        return ram[t['off']:t['off'] + min(256, t['size'])]
    elif mode == 'pixel':
        pix_data = t['pix_start'] + 12
        # Pick a deep offset within the pixel block
        start = min(pix_data + 64, t['off'] + t['size'] - 128)
        return ram[start:start + 128]
    else:
        raise ValueError(f'unknown sig mode: {mode}')


def collect_corpus(prot_dir: str, lzs_dir: str | None) -> list[tuple[str, bytes]]:
    """Yield (label, bytes) for every PROT entry plus optional LZS-decoded sections."""
    out = []
    for p in sorted(glob(os.path.join(prot_dir, '0[0-9][0-9][0-9]_*.BIN'))):
        with open(p, 'rb') as fh:
            out.append(('raw:' + os.path.basename(p), fh.read()))
    if lzs_dir and os.path.isdir(lzs_dir):
        for p in sorted(glob(os.path.join(lzs_dir, '*', '*.bin'))):
            with open(p, 'rb') as fh:
                rel = os.path.relpath(p, lzs_dir)
                out.append(('lzs:' + rel, fh.read()))
    return out


def parse_range(s: str) -> tuple[int, int]:
    lo, hi = s.split('-', 1)
    return int(lo, 0), int(hi, 0)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('ram_dump', type=Path)
    ap.add_argument('--base', type=lambda s: int(s, 0), default=0x80000000)
    ap.add_argument('--prot-dir', default='extracted/PROT')
    ap.add_argument('--lzs-dir', default=None,
                    help='Optional dir of LZS-decoded PROT sections.')
    ap.add_argument('--filter-vaddr', type=parse_range, default=None,
                    help='Restrict to vaddrs in LO-HI (hex OK).')
    ap.add_argument('--extract-to', type=Path, default=None,
                    help='Write each TIM as <dir>/tim_<vaddr>.tim.')
    ap.add_argument('--sig-mode', choices=('full', 'pixel'), default='full')
    args = ap.parse_args()

    ram = args.ram_dump.read_bytes()
    print(f'Scanning {args.ram_dump} ({len(ram)} bytes, base=0x{args.base:08x})',
          file=sys.stderr)
    tims = scan_ram(ram)
    if args.filter_vaddr:
        lo, hi = args.filter_vaddr
        tims = [t for t in tims if lo <= args.base + t['off'] < hi]
    print(f'Found {len(tims)} valid TIM(s)', file=sys.stderr)
    print(f'Loading PROT corpus from {args.prot_dir}'
          + (f' + {args.lzs_dir}' if args.lzs_dir else ''),
          file=sys.stderr)
    corpus = collect_corpus(args.prot_dir, args.lzs_dir)
    print(f'  {len(corpus)} corpus entries', file=sys.stderr)

    if args.extract_to:
        args.extract_to.mkdir(parents=True, exist_ok=True)

    for t in tims:
        vaddr = args.base + t['off']
        bpp_name = ['4bpp', '8bpp', '15bpp', '24bpp'][t['bpp']]
        clut_str = ' +CLUT' if t['has_clut'] else ''
        px, py, _, _ = t['pix_rect']
        print(f'\nvaddr=0x{vaddr:08x} {bpp_name}{clut_str} '
              f"{t['real_w']}x{t['real_h']} VRAM@({px},{py}) "
              f"size={t['size']}")

        if args.extract_to:
            out_path = args.extract_to / f'tim_{vaddr:08x}.tim'
            out_path.write_bytes(ram[t['off']:t['off'] + t['size']])
            print(f'  extracted -> {out_path}')

        sig = signature(ram, t, args.sig_mode)
        hits = []
        for label, data in corpus:
            h = data.find(sig)
            if h >= 0:
                hits.append((label, h))
                if len(hits) >= 5:
                    break
        if hits:
            print(f'  PROT source ({args.sig_mode} sig, {len(sig)} B):')
            for label, off in hits:
                print(f'    {label} @ 0x{off:x}')
        else:
            print(f'  no PROT match ({args.sig_mode} sig, {len(sig)} B)')


if __name__ == '__main__':
    main()
