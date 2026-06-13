#!/usr/bin/env python3
"""Verify the party's in-battle meshes come from the battle pack (PROT 1204),
not the field pack (PROT 0874 §0).

This is the reproducible form of the empirical finding behind
`legaia_asset::battle_char_pack`: in a real battle the party mesh pointers
`DAT_8007C018[0..=2]` reference TMDs whose (pose-independent) vertex data
byte-matches PROT 1204 (`other5`) and NOT PROT 0874 (`befect_data`, §0). The
field pack is field-only.

It reads a 2 MiB main-RAM dump (e.g. from `autorun_dump_full_ram.lua`, or
`mednafen-state extract <save> --start 0x80000000 --end 0x80200000 --out
ram.bin`), walks the party slots of the global mesh-pointer table at
`DAT_8007C018`, reconstructs each object's vertex pool from the runtime TMD
(whose object descriptors hold absolute RAM pointers), and searches those
bytes in each disc pack entry.

Usage:
    python3 scripts/asset-investigation/verify_battle_char_pack.py RAM.bin \
        extracted/PROT/1204_other5.BIN extracted/PROT/0874_befect_data.BIN

Exit status is non-zero if any party slot matches the field pack better than
the battle pack (i.e. the finding would be contradicted).
"""
import struct
import sys

RAM_BASE = 0x80000000
TABLE_ADDR = 0x8007C018
PARTY_SLOTS = 3  # DAT_8007C018[0..=2] = active party in battle
TMD_MAGIC = 0x80000002


def read_u32(ram: bytes, virt: int) -> int | None:
    off = virt - RAM_BASE
    if off < 0 or off + 4 > len(ram):
        return None
    return struct.unpack_from("<I", ram, off)[0]


def runtime_vertex_bytes(ram: bytes, tmd_ptr: int) -> bytes:
    """Reconstruct the concatenated vertex pool of a runtime (pointer-resolved)
    Legaia TMD at `tmd_ptr`. Object descriptors are 0x1c bytes; field 0 =
    absolute vertex pointer, field 1 = vertex count."""
    magic = read_u32(ram, tmd_ptr)
    if magic != TMD_MAGIC:
        return b""
    nobj = read_u32(ram, tmd_ptr + 0x08) or 0
    out = bytearray()
    for i in range(nobj):
        desc = tmd_ptr + 0x0C + i * 0x1C
        vptr = read_u32(ram, desc)
        vcnt = read_u32(ram, desc + 4)
        if vptr is None or vcnt is None:
            continue
        off = vptr - RAM_BASE
        n = vcnt * 8
        if 0 <= off and off + n <= len(ram):
            out += ram[off : off + n]
    return bytes(out)


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__)
        return 2
    ram = open(argv[1], "rb").read()
    battle_pack = open(argv[2], "rb").read()  # PROT 1204
    field_pack = open(argv[3], "rb").read()  # PROT 0874

    print(f"DAT_8007C018 party slots [0..{PARTY_SLOTS - 1}]:")
    ok = True
    any_party = False
    for slot in range(PARTY_SLOTS):
        ptr = read_u32(ram, TABLE_ADDR + slot * 4)
        if ptr is None or read_u32(ram, ptr) != TMD_MAGIC:
            print(f"  slot {slot}: ptr={ptr and hex(ptr)} (not a TMD; party member absent)")
            continue
        nobj = read_u32(ram, ptr + 0x08)
        verts = runtime_vertex_bytes(ram, ptr)
        # Count how many 96-byte object windows occur in each disc pack.
        windows = [verts[i : i + 96] for i in range(0, max(0, len(verts) - 96), 96)]
        windows = [w for w in windows if len(w) == 96 and any(w)]
        in_battle = sum(1 for w in windows if w in battle_pack)
        in_field = sum(1 for w in windows if w in field_pack)
        if in_battle == 0 and in_field == 0:
            # Matches neither pack: a non-party mesh (enemy / aux) parked in
            # this slot — happens for slots 1/2 in a Vahn-only battle.
            verdict = "non-party (enemy/aux)"
        elif in_battle > in_field:
            verdict = "BATTLE pack (1204)"
            any_party = True
        else:
            verdict = "field pack (0874)"
            any_party = True
            ok = False  # a real party slot matching field would contradict
        print(
            f"  slot {slot}: ptr={hex(ptr)} nobj={nobj}  "
            f"vertex-windows={len(windows)}  in-1204={in_battle}  in-0874={in_field}  -> {verdict}"
        )

    if not any_party:
        print("  (no party meshes resident — is this a battle save?)")
        return 1
    print()
    print(
        "RESULT: party meshes come from the battle pack PROT 1204."
        if ok
        else "RESULT: UNEXPECTED — a party slot matched the field pack; finding contradicted."
    )
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
