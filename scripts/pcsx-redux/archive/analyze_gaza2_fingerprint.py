#!/usr/bin/env python3
"""Fingerprint a PCSX-Redux save state as a battle-scene anchor.

Slot numbers get overwritten; a save is identified by what is IN it. This
prints the identifying facts for a battle save so it can be re-found later:

  * scene label (0x8007050C) + game mode (0x8007B83C; 0x15 = battle)
  * the battle context pointer (*0x8007BD24) and its action-SM cursor ctx+7
  * the acting seat ctx+0x13 and the effect-completion counters
    ctx+0x249 / ctx+0x24C / ctx+0x24D
  * every live actor in the pointer table 0x801C9370 with its monster id,
    HP, current/queued anim ids and the hit-counter bound +0x21B
  * the enemy record table 0x801C9348 (slot >= 3) monster ids + elements
  * a SHA-256 over main RAM, so the exact state is re-identifiable

Main RAM is located inside the gunzipped sstate protobuf by anchor string,
mirroring crates/mednafen extract::main_ram_via_anchor.

Usage:
    analyze_gaza2_fingerprint.py <sstate> [<sstate> ...] [--scus PATH]
"""
import argparse
import gzip
import hashlib
import struct
import sys
from pathlib import Path

KSEG0 = 0x80000000
RAM_SIZE = 2 * 1024 * 1024
SCUS_LOAD_ADDR = 0x80010000
PSX_EXE_HEADER = 0x800

SCENE_NAME_VA = 0x8007050C
GAME_MODE_VA = 0x8007B83C
CTX_PTR_VA = 0x8007BD24
ACTOR_TABLE_VA = 0x801C9370
ENEMY_REC_TABLE_VA = 0x801C9348
CAM_YAW_VA = 0x8007B792

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
    return ram[va & 0x1FFFFF]


def rd16(ram, va):
    o = va & 0x1FFFFF
    return struct.unpack_from("<H", ram, o)[0]


def rd32(ram, va):
    o = va & 0x1FFFFF
    return struct.unpack_from("<I", ram, o)[0]


def in_ram(va):
    return 0x80000000 <= va < 0x80200000


def scene_name(ram):
    out = []
    for i in range(8):
        b = rd8(ram, SCENE_NAME_VA + i)
        if not (0x20 <= b < 0x7F):
            break
        out.append(chr(b))
    return "".join(out)


def report(path, ram):
    print(f"== {path}")
    print(f"   sha256(main_ram) = {hashlib.sha256(ram).hexdigest()}")
    print(f"   scene = {scene_name(ram)!r}   game_mode = 0x{rd8(ram, GAME_MODE_VA):02X}")
    print(f"   cam_yaw _DAT_8007B792 = 0x{rd16(ram, CAM_YAW_VA):04X}")

    ctx = rd32(ram, CTX_PTR_VA)
    print(f"   battle ctx *0x8007BD24 = 0x{ctx:08X}", end="")
    if not in_ram(ctx):
        print("  (NOT a RAM pointer -> not in battle)")
        return
    print()
    print(
        f"     ctx+7  SM cursor = 0x{rd8(ram, ctx + 7):02X}"
        f"   ctx+0x13 acting seat = {rd8(ram, ctx + 0x13)}"
    )
    print(
        f"     ctx+0x249 = {rd8(ram, ctx + 0x249):3d}"
        f"   ctx+0x24C = {rd8(ram, ctx + 0x24C):3d}"
        f"   ctx+0x24D = {rd8(ram, ctx + 0x24D):3d}"
    )

    # Battle-actor fields per docs/subsystems/battle.md: HP/MP mirrors at
    # +0x14C..+0x158, per-actor SM state byte +0x07, action id +0x1DF.
    print("   actor table 0x801C9370:")
    for seat in range(8):
        a = rd32(ram, ACTOR_TABLE_VA + seat * 4)
        if not in_ram(a):
            continue
        print(
            f"     seat {seat}: actor=0x{a:08X} "
            f"st+7=0x{rd8(ram, a + 0x07):02X} "
            f"hp={rd16(ram, a + 0x14C):5d}/{rd16(ram, a + 0x14E):<5d} "
            f"mp={rd16(ram, a + 0x150):4d} "
            f"xz=({rd16(ram, a + 0x34):6d},{rd16(ram, a + 0x38):6d}) "
            f"+0x1D9=0x{rd8(ram, a + 0x1D9):02X} "
            f"+0x1DA=0x{rd8(ram, a + 0x1DA):02X} "
            f"+0x1DF=0x{rd8(ram, a + 0x1DF):02X} "
            f"+0x1FA={rd8(ram, a + 0x1FA):3d} "
            f"+0x21B={rd8(ram, a + 0x21B):3d}"
        )

    # Monster records per docs/subsystems/battle.md "Monster-record source
    # layout": +0x00 name ptr, +0x0C HP, +0x21..+0x23 global magic-attack ids.
    print("   enemy record table 0x801C9348 (battle slots 3..7):")
    for i in range(5):
        r = rd32(ram, ENEMY_REC_TABLE_VA + i * 4)
        if not in_ram(r):
            continue
        namep = rd32(ram, r)
        name = ""
        if in_ram(namep):
            out = []
            for k in range(16):
                b = rd8(ram, namep + k)
                if not (0x20 <= b < 0x7F):
                    break
                out.append(chr(b))
            name = "".join(out)
        print(
            f"     slot {i + 3}: rec=0x{r:08X} "
            f"hp={rd16(ram, r + 0x0C):6d} mp={rd16(ram, r + 0x10):5d} "
            f"atk={rd16(ram, r + 0x12):5d} int={rd16(ram, r + 0x18):5d} "
            f"size=0x{rd8(ram, r + 0x1F):02X} "
            f"magic=[{rd8(ram, r + 0x21):#04x},{rd8(ram, r + 0x22):#04x},"
            f"{rd8(ram, r + 0x23):#04x}] name={name!r}"
        )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sstates", nargs="+")
    ap.add_argument("--scus", default=None)
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

    for p in args.sstates:
        report(p, load_ram(p, scus))
    return 0


if __name__ == "__main__":
    sys.exit(main())
