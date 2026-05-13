#!/usr/bin/env python3
"""
walk_actor_lists.py

Walks the seven world-map actor lists in a PCSX-Redux save state's RAM
blob and dumps per-actor fields. The goal is to identify which actor
owns the bulk-continent TMD - the one rendered by FUN_8002735C via the
case-5 path of FUN_8001ADA4 (per-actor render dispatcher).

The actor list heads live at PSX virtual addresses
_DAT_8007C34C..._DAT_8007C36C (seven u32 heads). For each non-null
head we walk the singly-linked list via actor[+0x00] and report:

  +0x00  next       (head of chain; NULL terminates)
  +0x0C  tick fn    (called every frame by FUN_8002519c TICK pass)
  +0x10  flags
  +0x44  mesh chain head (puVar5: puVar5[0]=count, puVar5[1..n]=meshes)
  +0x56  render mode (the switch value in FUN_8001ADA4)
  +0x14, +0x18  X/Y coordinates (sanity-check the actor's position)

The actor with `render_mode == 5` and a mesh-chain count in the
thousands is the bulk-continent emitter source.

USAGE
    python3 scripts/pcsx-redux/walk_actor_lists.py \
        ~/Tools/pcsx-redux/SCUS94254.sstate2

The save state file is the gzipped PCSX-Redux protobuf v4 format
(SaveState message at the top level). We only need the
SaveState.memory.ram bytes (8 MiB), so we walk just those two field
tags rather than depend on a full protobuf library.
"""

import argparse
import gzip
import struct
import sys
from pathlib import Path


RAM_SIZE = 0x00800000  # 8 MiB main RAM
PSX_BASE = 0x80000000  # KSEG0 / KUSEG mirror that maps to RAM[0..]

# Seven actor-list heads consumed by the world-map render passes
# (per `docs/subsystems/world-map.md` -> "Per-frame render-pass iterator").
LIST_HEAD_ADDRS = [
    0x8007C34C,
    0x8007C350,
    0x8007C354,
    0x8007C358,
    0x8007C35C,
    0x8007C360,
    0x8007C364,  # note: this slot is the camera/active-actor pointer,
    0x8007C368,  # not necessarily a list head; we walk it the same
    0x8007C36C,  # way and report - if next pointers are bogus we bail.
]

# Cap chain walks to catch corrupted/cyclic lists and avoid runaway.
MAX_CHAIN = 256


def read_varint(buf, off):
    """Return (value, new_off). Standard protobuf base-128 varint."""
    val = 0
    shift = 0
    while True:
        b = buf[off]
        off += 1
        val |= (b & 0x7F) << shift
        if (b & 0x80) == 0:
            return val, off
        shift += 7
        if shift > 63:
            raise ValueError(f"varint too long at {off:#x}")


def find_field(buf, off, end, target_tag, target_wire):
    """Scan a protobuf message from `off..end`. Return the bytes range
    [start, end) of the value for the first occurrence of field
    `target_tag` with wire type `target_wire`. Returns None if not
    found."""
    while off < end:
        key, off = read_varint(buf, off)
        tag = key >> 3
        wire = key & 7
        if wire == 0:
            _, off = read_varint(buf, off)
        elif wire == 1:
            off += 8
        elif wire == 2:
            length, off = read_varint(buf, off)
            v_start, v_end = off, off + length
            if tag == target_tag and wire == target_wire:
                return (v_start, v_end)
            off = v_end
        elif wire == 5:
            off += 4
        else:
            raise ValueError(f"unsupported wire type {wire} at {off:#x}")
    return None


def extract_ram(state_path):
    """Gunzip the save state, walk SaveState.memory.ram, return the
    raw 8 MiB RAM blob as a bytes object."""
    raw = gzip.decompress(Path(state_path).read_bytes())

    # SaveState.memory is tag 3, wire type 2 (MessageField<Memory>).
    mem = find_field(raw, 0, len(raw), target_tag=3, target_wire=2)
    if mem is None:
        raise RuntimeError("memory message field (tag 3) not found in save state")
    mem_start, mem_end = mem

    # Memory.ram is tag 1, wire type 2 (FieldPtr<FixedBytes<0x00800000>>).
    ram = find_field(raw, mem_start, mem_end, target_tag=1, target_wire=2)
    if ram is None:
        raise RuntimeError("ram bytes field (tag 1) not found in memory message")
    ram_start, ram_end = ram

    blob = raw[ram_start:ram_end]
    if len(blob) != RAM_SIZE:
        raise RuntimeError(
            f"unexpected RAM size {len(blob):#x}, want {RAM_SIZE:#x}"
        )
    return blob


def psx(ram, addr, size):
    """Read `size` bytes from `ram` at PSX virtual address `addr`."""
    if addr < PSX_BASE or addr + size > PSX_BASE + RAM_SIZE:
        return None
    off = addr - PSX_BASE
    return ram[off : off + size]


def read_u32(ram, addr):
    raw = psx(ram, addr, 4)
    if raw is None:
        return None
    return struct.unpack("<I", raw)[0]


def read_u16(ram, addr):
    raw = psx(ram, addr, 2)
    if raw is None:
        return None
    return struct.unpack("<H", raw)[0]


def read_i16(ram, addr):
    raw = psx(ram, addr, 2)
    if raw is None:
        return None
    return struct.unpack("<h", raw)[0]


def fmt_tick(addr):
    """Annotate the tick function pointer with what we know about it."""
    KNOWN = {
        0x80021DF4: "FUN_80021DF4 SCUS per-frame actor tick (move VM)",
        0x8003BC08: "FUN_8003BC08 SCUS per-actor tick (motion/AI)",
        0x801D1344: "FUN_801D1344 world_map overlay (gate-arm wrapper)",
        0x801D84D0: "FUN_801D84D0 world_map overlay (HUD/text SM)",
        0x801E76D4: "FUN_801E76D4 world_map controller",
        0x801DA51C: "FUN_801DA51C world_map per-entity tick",
        0x801D6058: "FUN_801D6058 world_map overlay",
        0x801CFC40: "FUN_801CFC40 world_map_top sprite batcher",
    }
    return KNOWN.get(addr, "")


def read_mesh_ptrs(ram, mesh_head, max_count=64):
    """Read the mesh-chain struct: [u32 count][u32 ptrs[count]]."""
    if not mesh_head or mesh_head < PSX_BASE or mesh_head >= PSX_BASE + RAM_SIZE:
        return None
    count = read_u32(ram, mesh_head)
    if count is None or count > max_count:
        return None
    ptrs = []
    for i in range(count):
        p = read_u32(ram, mesh_head + 4 + 4 * i)
        if p is None:
            break
        ptrs.append(p)
    return ptrs


def walk_chain(ram, head, list_idx, list_addr):
    """Walk a singly-linked actor chain starting at `head`. Returns a
    list of per-actor dicts."""
    actors = []
    seen = set()
    node = head
    while node and node != 0xFFFFFFFF and len(actors) < MAX_CHAIN:
        if node in seen:
            print(f"    !! cycle at {node:#010x}, bailing")
            break
        seen.add(node)
        nxt = read_u32(ram, node + 0x00)
        if nxt is None:
            print(f"    !! out-of-range actor pointer {node:#010x}, bailing")
            break
        tick = read_u32(ram, node + 0x0C)
        flags = read_u32(ram, node + 0x10)
        mesh_head = read_u32(ram, node + 0x44)
        render_mode = read_u16(ram, node + 0x56)
        x = read_i16(ram, node + 0x14)
        y = read_i16(ram, node + 0x16)
        z = read_i16(ram, node + 0x18)

        mesh_count = None
        mesh_ptrs = None
        if mesh_head and mesh_head >= PSX_BASE and mesh_head < PSX_BASE + RAM_SIZE:
            mc = read_u32(ram, mesh_head)
            # Sanity check: a sensible chain count is < 0x10000.
            if mc is not None and mc < 0x10000:
                mesh_count = mc
                mesh_ptrs = read_mesh_ptrs(ram, mesh_head)

        actors.append(
            dict(
                node=node,
                next=nxt,
                tick=tick or 0,
                flags=flags or 0,
                mesh_head=mesh_head or 0,
                mesh_count=mesh_count,
                mesh_ptrs=mesh_ptrs,
                render_mode=render_mode or 0,
                x=x or 0,
                y=y or 0,
                z=z or 0,
            )
        )
        node = nxt
    if len(actors) >= MAX_CHAIN:
        print(f"    !! chain too long (capped at {MAX_CHAIN}), suspect corruption")
    return actors


def print_list(ram, list_idx, list_addr, head, actors, dump_meshes=False):
    print(f"\nList #{list_idx}  head_addr={list_addr:#010x}  head={head:#010x}  n={len(actors)}")
    if not actors:
        return
    print(
        f"    {'idx':3s} {'node':>10s} {'next':>10s} {'tick':>10s} "
        f"{'flags':>10s} {'mesh_head':>10s} {'count':>6s} {'mode':>4s} "
        f"{'X':>6s} {'Y':>6s} {'Z':>6s}  tick_name"
    )
    for i, a in enumerate(actors):
        mc = f"{a['mesh_count']}" if a["mesh_count"] is not None else "-"
        print(
            f"    {i:3d} {a['node']:#010x} {a['next']:#010x} "
            f"{a['tick']:#010x} {a['flags']:#010x} {a['mesh_head']:#010x} "
            f"{mc:>6s} {a['render_mode']:>4d} {a['x']:>6d} {a['y']:>6d} "
            f"{a['z']:>6d}  {fmt_tick(a['tick'])}"
        )
        if dump_meshes and a["mesh_ptrs"]:
            for j, p in enumerate(a["mesh_ptrs"]):
                magic = read_u32(ram, p)
                magic_s = f"magic={magic:#010x}" if magic is not None else "magic=?"
                tag = " <-- Legaia TMD" if magic == 0x80000002 else ""
                print(f"          mesh[{j:2d}] @ {p:#010x}  {magic_s}{tag}")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("state", help="PCSX-Redux save state path (gzipped protobuf)")
    ap.add_argument(
        "--dump-meshes",
        action="store_true",
        help="Dump each mesh pointer's TMD magic word.",
    )
    args = ap.parse_args()

    ram = extract_ram(args.state)
    print(f"loaded {len(ram):#x} bytes of RAM from {args.state}")

    # Cross-check: print game mode to confirm we're in MAPDSIP (0x0D).
    game_mode = read_u16(ram, 0x8007B82C)  # _DAT_8007B82C is a known mode reg; nice to print
    submode = read_u32(ram, 0x8007BC3C)  # _DAT_8007BC3C: world-map submode register
    print(f"  _DAT_8007B82C (game mode hint) = {game_mode}")
    print(f"  _DAT_8007BC3C (world-map submode reg) = {submode}")

    # Walk each list head.
    bulk_candidate = None
    for i, addr in enumerate(LIST_HEAD_ADDRS):
        head = read_u32(ram, addr)
        if head is None:
            print(f"\nList #{i}  head_addr={addr:#010x} -- OUT OF RANGE")
            continue
        actors = walk_chain(ram, head, i, addr)
        print_list(ram, i, addr, head, actors, dump_meshes=args.dump_meshes)
        for a in actors:
            mc = a["mesh_count"] or 0
            if a["render_mode"] == 5 and mc > 100:
                if bulk_candidate is None or mc > (bulk_candidate["mesh_count"] or 0):
                    bulk_candidate = a
                    bulk_candidate["list_idx"] = i
                    bulk_candidate["list_addr"] = addr

    print("\n=== Summary ===")
    if bulk_candidate is None:
        print("No actor found with render_mode==5 and a large mesh chain.")
        print(
            "Best non-mode-5 candidates ranked by mesh chain count "
            "may still be the continent (case-4 multi-target paths)."
        )
        # Surface the top-3 mesh counts overall, regardless of mode.
        all_actors = []
        for i, addr in enumerate(LIST_HEAD_ADDRS):
            head = read_u32(ram, addr)
            if head:
                for a in walk_chain(ram, head, i, addr):
                    a = dict(a)
                    a["list_idx"] = i
                    a["list_addr"] = addr
                    all_actors.append(a)
        all_actors.sort(key=lambda a: -(a["mesh_count"] or 0))
        print("Top 5 by mesh_count overall:")
        for a in all_actors[:5]:
            mc = a["mesh_count"] if a["mesh_count"] is not None else "-"
            print(
                f"  list#{a['list_idx']} node={a['node']:#010x} "
                f"mode={a['render_mode']} mesh_count={mc} "
                f"tick={a['tick']:#010x}"
            )
    else:
        a = bulk_candidate
        print(
            f"Bulk continent candidate: list#{a['list_idx']} "
            f"(head_addr={a['list_addr']:#010x}) "
            f"actor node {a['node']:#010x}"
        )
        print(f"  render_mode = {a['render_mode']} (FUN_8001ADA4 case 5)")
        print(f"  mesh_head   = {a['mesh_head']:#010x}")
        print(f"  mesh_count  = {a['mesh_count']}")
        print(f"  flags       = {a['flags']:#010x}")
        print(f"  tick fn     = {a['tick']:#010x}  {fmt_tick(a['tick'])}")
        print(f"  position    = ({a['x']}, {a['y']}, {a['z']})")


if __name__ == "__main__":
    sys.exit(main() or 0)
