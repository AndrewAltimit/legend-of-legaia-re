#!/usr/bin/env python3
"""
resolve_actor_tmds.py

Resolve each live world-map actor's mesh-chain to its source TMD
slot(s) in the kingdom's landmark pack. The output is a placement
table: `(actor_pos, [tmd_slot_indices])` per actor, derived entirely
from live RAM + the on-disc TMD pack.

This bypasses the (still-unknown) field-VM script that performs the
runtime actor spawning. The runtime walks an actor list, each
actor's mesh chain at `actor[+0x44]` is `[count, prim_group_ptrs[]]`,
where each prim_group_ptr points at an object-table entry inside one
of the 40 loaded landmark TMDs. We reverse the chain:

  RAM prim-group ptr  -> containing TMD (nearest preceding 0x80000002)
                      -> TMD's slot offset (= RAM_addr - landmark_load_base)
                      -> compare against on-disc TMD pack's word_offsets
                      -> slot index 0..N-1.

USAGE
    python3 scripts/pcsx-redux/resolve_actor_tmds.py \\
        ~/Tools/pcsx-redux/SCUS94254.sstate2 \\
        [--bundle map01] [--json out.json]

The optional --json output writes a placements list usable by the
disc-only viewer: each entry has `pos`, `slots` (list of int), and
`actor_node` for tracing.
"""

import argparse
import json
import struct
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from match_prim_groups_to_disc import (  # noqa: E402
    extract_ram,
    find_asset_table,
    lzs_decompress,
)


RAM_SIZE = 0x00800000
PSX_BASE = 0x80000000
TMD_MAGIC = b"\x02\x00\x00\x80"

LIST_HEAD_ADDRS = [
    0x8007C34C,
    0x8007C350,
    0x8007C354,
    0x8007C358,
    0x8007C35C,
    0x8007C360,
    0x8007C364,
    0x8007C368,
    0x8007C36C,
]

KINGDOM_BASE = {"map01": 85, "map02": 244, "map03": 391}


def read_u32(ram, addr):
    if addr < PSX_BASE or addr + 4 > PSX_BASE + RAM_SIZE:
        return None
    off = addr - PSX_BASE
    return struct.unpack("<I", ram[off : off + 4])[0]


def read_i16(ram, addr):
    if addr < PSX_BASE or addr + 2 > PSX_BASE + RAM_SIZE:
        return None
    off = addr - PSX_BASE
    return struct.unpack("<h", ram[off : off + 2])[0]


def load_kingdom_pack(extracted_dir, bundle):
    """Return (load_base_in_ram, byte_offsets_within_pack) for the
    decompressed TMD pack, after locating it in RAM."""
    base = KINGDOM_BASE[bundle]
    for off in [0, 1]:
        idx = base + off
        files = list(Path(extracted_dir, "PROT").glob(f"{idx:04d}_*.BIN"))
        if not files:
            continue
        raw = files[0].read_bytes()
        table_off = find_asset_table(raw)
        if table_off is None:
            continue
        table = raw[table_off:]
        # Slot 1 (type 0x02, TMD pack)
        ts = struct.unpack("<I", table[16:20])[0]
        do = struct.unpack("<I", table[20:24])[0]
        slot_size = ts & 0xFFFFFF
        try:
            pack = lzs_decompress(table[do:], slot_size)
        except Exception:
            continue
        # The pack is `[u32 count][u32 word_offsets[count]][TMDs]`. Each
        # word_offset is a 4-byte multiple. byte_offsets[k] = w * 4.
        count = struct.unpack("<I", pack[0:4])[0]
        if count > 200:
            continue
        byte_offsets = []
        for k in range(count):
            w = struct.unpack("<I", pack[4 + k * 4 : 8 + k * 4])[0]
            byte_offsets.append(w * 4)
        return pack, byte_offsets, files[0].name
    return None, None, None


def find_landmark_load_base(ram, pack, byte_offsets):
    """Pick a distinctive sample from the first TMD's body in `pack`,
    search RAM for it, and infer the load base. Sample at 80% into
    the body to avoid pointer-fixup regions (vert/norm/prim tops at
    fixed disc offsets but absolute pointers in RAM)."""
    first_body_start = byte_offsets[0]
    first_body_end = byte_offsets[1] if len(byte_offsets) > 1 else len(pack)
    body = pack[first_body_start:first_body_end]
    if len(body) < 200:
        return None
    sample_off = (len(body) * 4) // 5
    sample = bytes(body[sample_off : sample_off + 64])
    pos = ram.find(sample)
    if pos < 0:
        return None
    # The matched position is `first_body_start + sample_off` past
    # `load_base`. So:
    load_base = (PSX_BASE + pos) - first_body_start - sample_off
    return load_base


def collect_actors(ram):
    """Walk every actor list head; return list of dicts with
    `node`, `tick`, `flags`, `pos`, `mesh_head`."""
    out = []
    seen = set()
    for head_addr in LIST_HEAD_ADDRS:
        node = read_u32(ram, head_addr)
        while node and node != 0xFFFFFFFF and node not in seen:
            seen.add(node)
            nxt = read_u32(ram, node + 0x00)
            if nxt is None:
                break
            out.append(
                dict(
                    node=node,
                    list_head=head_addr,
                    tick=read_u32(ram, node + 0x0C) or 0,
                    flags=read_u32(ram, node + 0x10) or 0,
                    mesh_head=read_u32(ram, node + 0x44) or 0,
                    x=read_i16(ram, node + 0x14) or 0,
                    y=read_i16(ram, node + 0x16) or 0,
                    z=read_i16(ram, node + 0x18) or 0,
                    render_mode=read_u32(ram, node + 0x56) or 0,
                )
            )
            node = nxt
    return out


def mesh_chain_ptrs(ram, mesh_head):
    """Return list of prim-group pointers in the chain."""
    if not mesh_head or mesh_head < PSX_BASE:
        return []
    count = read_u32(ram, mesh_head)
    if count is None or count == 0 or count > 64:
        return []
    out = []
    for k in range(count):
        p = read_u32(ram, mesh_head + 4 + 4 * k)
        if p and PSX_BASE <= p < PSX_BASE + RAM_SIZE:
            out.append(p)
    return out


def find_containing_tmd(ram, addr):
    """Return PSX address of the nearest preceding `0x80000002` magic
    word that's within ~256 KiB. Returns None if no plausible TMD
    found in range."""
    if addr < PSX_BASE:
        return None
    off = addr - PSX_BASE
    # Walk backwards in 4-byte steps for up to 256 KiB
    limit = max(0, off - 0x40000)
    while off >= limit:
        if ram[off : off + 4] == TMD_MAGIC:
            return PSX_BASE + off
        off -= 4
    return None


def tmd_addr_to_slot(tmd_ram_addr, load_base, byte_offsets):
    """Given a TMD's RAM start address, return its slot index in the
    pack (= position in byte_offsets). Returns -1 if not in this pack."""
    if tmd_ram_addr is None or load_base is None:
        return -1
    pack_off = tmd_ram_addr - load_base
    if pack_off < 0:
        return -1
    # byte_offsets is monotonic; find exact match (TMDs always start
    # at a known offset since FUN_80026B4C registered each from the
    # pack's word_offsets[]).
    try:
        return byte_offsets.index(pack_off)
    except ValueError:
        return -1


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("state", help="PCSX-Redux save state path")
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
        "--json",
        help="Optional: write placements as JSON to this path.",
    )
    args = ap.parse_args()

    ram = extract_ram(args.state)
    print(f"loaded {len(ram):#x} bytes of RAM\n")

    pack, byte_offsets, fname = load_kingdom_pack(args.extracted, args.bundle)
    if pack is None:
        print(f"!! could not load kingdom pack for {args.bundle}")
        return 1
    print(
        f"kingdom pack: {fname}  {len(byte_offsets)} TMDs total, "
        f"pack size = {len(pack)}"
    )

    load_base = find_landmark_load_base(ram, pack, byte_offsets)
    if load_base is None:
        print("!! could not locate landmark pack in RAM")
        return 1
    print(f"landmark pack loaded at RAM {load_base:#010x}\n")

    # Pre-compute the RAM range of each slot for diagnostics
    slot_ram_starts = [load_base + bo for bo in byte_offsets]
    print(
        f"slot 0 starts at {slot_ram_starts[0]:#010x}, "
        f"last slot starts at {slot_ram_starts[-1]:#010x}"
    )

    actors = collect_actors(ram)
    print(f"\n{len(actors)} actors across all lists\n")

    placements = []
    slot_use = Counter()
    unresolved_ptrs = 0
    total_ptrs = 0

    print(
        f"  {'node':>10s} {'list':>10s} {'pos':>22s} {'mesh#':>5s} "
        f"{'slots':<24s}  tick"
    )
    print("  " + "-" * 90)
    for a in actors:
        ptrs = mesh_chain_ptrs(ram, a["mesh_head"])
        slots = []
        for p in ptrs:
            total_ptrs += 1
            tmd_addr = find_containing_tmd(ram, p)
            slot = tmd_addr_to_slot(tmd_addr, load_base, byte_offsets)
            slots.append(slot)
            if slot >= 0:
                slot_use[slot] += 1
            else:
                unresolved_ptrs += 1
        # Deduplicate slots for the per-actor output (one actor often
        # holds many prim groups from the same TMD).
        unique_slots = sorted(set(s for s in slots if s >= 0))
        unresolved_for_actor = sum(1 for s in slots if s < 0)
        pos_str = f"({a['x']:>5d},{a['y']:>5d},{a['z']:>5d})"
        slots_str = ",".join(str(s) for s in unique_slots)
        if unresolved_for_actor:
            slots_str += f" +{unresolved_for_actor}?"
        if not slots_str:
            slots_str = "-"
        print(
            f"  {a['node']:#010x} {a['list_head']:#010x} {pos_str:>22s} "
            f"{len(ptrs):>5d} {slots_str:<24s}  {a['tick']:#010x}"
        )
        if unique_slots:
            placements.append(
                dict(
                    node=f"{a['node']:#010x}",
                    pos=[a["x"], a["y"], a["z"]],
                    slots=unique_slots,
                    tick=f"{a['tick']:#010x}",
                    list_head=f"{a['list_head']:#010x}",
                    flags=a["flags"],
                    render_mode=a["render_mode"],
                    chain_size=len(ptrs),
                    unresolved=unresolved_for_actor,
                )
            )

    print(f"\n=== Resolution stats ===")
    print(f"  total chain pointers:  {total_ptrs}")
    print(f"  resolved to slot:      {total_ptrs - unresolved_ptrs}")
    print(f"  unresolved:            {unresolved_ptrs}")
    print(
        f"  unique slots referenced: {len(slot_use)} / {len(byte_offsets)}"
    )
    print(f"\n=== Slot usage ===")
    for slot in sorted(slot_use.keys()):
        print(f"  slot {slot:>2d}: used {slot_use[slot]:>3d} times")

    if args.json:
        out = dict(
            bundle=args.bundle,
            kingdom_pack_load_base=f"{load_base:#010x}",
            slots_in_pack=len(byte_offsets),
            actors=placements,
            slot_usage={str(k): v for k, v in sorted(slot_use.items())},
            unresolved_ptrs=unresolved_ptrs,
            total_ptrs=total_ptrs,
        )
        Path(args.json).write_text(json.dumps(out, indent=2))
        print(f"\nwrote {args.json}")


if __name__ == "__main__":
    sys.exit(main() or 0)
