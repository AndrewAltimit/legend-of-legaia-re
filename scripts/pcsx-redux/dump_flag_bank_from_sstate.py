#!/usr/bin/env python3
"""Dump the story-flag ("fourth") bank from PCSX-Redux save states.

The bank lives at VA 0x80085758 (see docs/subsystems/script-vm.md,
"Default-case extension opcodes"): the field-VM / move-VM SET / CLEAR /
TEST dispatchers FUN_8003CE08 / FUN_8003CE34 / FUN_8003CE64 address it
as

    byte = 0x80085758 + (idx >> 3)
    mask = 0x80 >> (idx & 7)        # MSB-first within the byte

Main RAM is located inside the gunzipped sstate protobuf by anchor
string, mirroring crates/mednafen extract::main_ram_via_anchor (the
same anchors, the same SCUS-file-offset math).

Usage:
    dump_flag_bank_from_sstate.py <sstate> [<sstate> ...]
        [--scus PATH]              SCUS_942.54 binary (default: extracted/)
        [--flag IDX ...]           flag indices to test (hex ok), repeatable
        [--set-range LO HI]        list every SET flag index in [LO, HI)
        [--diff]                   with 2+ states, print per-byte bank diffs

Prints scene name / game mode per state so the caller can verify the
state is the beat it claims to be (never trust a bare slot number).
"""
import argparse
import gzip
import sys
from pathlib import Path

KSEG0 = 0x80000000
RAM_SIZE = 2 * 1024 * 1024
SCUS_LOAD_ADDR = 0x80010000
PSX_EXE_HEADER = 0x800
BANK_BASE = 0x80085758
SCENE_NAME_VA = 0x8007050C
GAME_MODE_VA = 0x8007B83C

ANCHORS = [
    b"---- FIELD PROGRAM -----%d",
    b"PSX TEST PROGRAM",
    b"enter main loop",
    b"main free mem%d",
    b"h:\\prot\\cdname.dat",
]


def load_ram(sstate_path, scus):
    raw = Path(sstate_path).read_bytes()
    try:
        payload = gzip.decompress(raw)
    except Exception:
        payload = raw
    for anchor in ANCHORS:
        scus_off = scus.find(anchor)
        if scus_off < PSX_EXE_HEADER:
            continue
        state_off = payload.find(anchor)
        if state_off < 0:
            continue
        ram_addr = SCUS_LOAD_ADDR + (scus_off - PSX_EXE_HEADER)
        phys = ram_addr - KSEG0
        start = state_off - phys
        if start < 0 or start + RAM_SIZE > len(payload):
            continue
        return payload[start : start + RAM_SIZE]
    raise SystemExit(f"{sstate_path}: no anchor matched; cannot locate main RAM")


def rd8(ram, va):
    return ram[(va & 0x1FFFFF)]


def scene_name(ram):
    out = []
    for i in range(8):
        b = rd8(ram, SCENE_NAME_VA + i)
        if not (0x20 <= b < 0x7F):
            break
        out.append(chr(b))
    return "".join(out)


def flag_addr(idx):
    return BANK_BASE + (idx >> 3), 0x80 >> (idx & 7)


def flag_set(ram, idx):
    byte_va, mask = flag_addr(idx)
    return bool(rd8(ram, byte_va) & mask)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sstates", nargs="+")
    ap.add_argument("--scus", default=None)
    ap.add_argument("--flag", action="append", default=[])
    ap.add_argument("--set-range", nargs=2, default=None)
    ap.add_argument("--diff", action="store_true")
    args = ap.parse_args()

    scus_path = args.scus
    if scus_path is None:
        here = Path(__file__).resolve()
        for parent in here.parents:
            cand = parent / "extracted" / "SCUS_942.54"
            if cand.exists():
                scus_path = str(cand)
                break
    if scus_path is None:
        raise SystemExit("--scus required (no extracted/SCUS_942.54 found)")
    scus = Path(scus_path).read_bytes()

    flags = [int(f, 0) for f in args.flag]
    rams = []
    for p in args.sstates:
        ram = load_ram(p, scus)
        rams.append((p, ram))
        print(f"== {p}")
        print(f"   scene={scene_name(ram)!r} game_mode=0x{rd8(ram, GAME_MODE_VA):02X}")
        for idx in flags:
            byte_va, mask = flag_addr(idx)
            val = rd8(ram, byte_va)
            print(
                f"   flag 0x{idx:03X}: byte 0x{byte_va:08X} (=0x{val:02X}) "
                f"mask 0x{mask:02X} -> {'SET' if val & mask else 'clear'}"
            )
        if args.set_range:
            lo = int(args.set_range[0], 0)
            hi = int(args.set_range[1], 0)
            on = [i for i in range(lo, hi) if flag_set(ram, i)]
            print(f"   set flags in [0x{lo:X},0x{hi:X}): {[hex(i) for i in on]}")

    if args.diff and len(rams) >= 2:
        base_name, base = rams[0]
        for other_name, other in rams[1:]:
            print(f"== diff {base_name} -> {other_name} (bank bytes 0x000..0x200)")
            for boff in range(0x200):
                va = BANK_BASE + boff
                a, b = rd8(base, va), rd8(other, va)
                if a != b:
                    gained = [
                        hex(boff * 8 + bit)
                        for bit in range(8)
                        if (b & (0x80 >> bit)) and not (a & (0x80 >> bit))
                    ]
                    lost = [
                        hex(boff * 8 + bit)
                        for bit in range(8)
                        if (a & (0x80 >> bit)) and not (b & (0x80 >> bit))
                    ]
                    print(
                        f"   0x{va:08X}: 0x{a:02X} -> 0x{b:02X}"
                        f"  +{gained} -{lost}"
                    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
