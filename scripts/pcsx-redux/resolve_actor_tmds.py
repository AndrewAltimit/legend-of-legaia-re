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
    # Single state, single kingdom (legacy form)
    python3 scripts/pcsx-redux/resolve_actor_tmds.py \\
        ~/Tools/pcsx-redux/SCUS94254.sstate2 \\
        [--bundle map01] [--json out.json]

    # Multiple states sharing one kingdom (merges actors across them)
    python3 scripts/pcsx-redux/resolve_actor_tmds.py \\
        --bundle map01 \\
        --json site/world-overview-live.json \\
        ~/.../drake-state-a.sstate ~/.../drake-state-b.sstate

    # Multiple states across kingdoms (one bundle per state)
    python3 scripts/pcsx-redux/resolve_actor_tmds.py \\
        --bundles map01,map02,map03 \\
        --json site/world-overview-live.json \\
        drake.sstate sebucus.sstate karisto.sstate

The optional --json output writes a placements list usable by the
disc-only viewer: each entry has `pos`, `slots` (list of int), and
`actor_node` for tracing. Multi-bundle runs emit a list of bundle
dicts; `scripts/extract-world-placements.py` merges them per-kingdom
into `site/world-overview.json`.

CAVEAT
    The original task brief described an "init blob the field-VM runs at
    scene-load" carrying per-kingdom actor placements. After tracing the
    kingdom 7-asset bundle (slots 0..6) plus the prescript at file
    offset 0x800, the MAN asset (slot 2, type 0x03) turns out to be the
    ONLY static placement source on disc - slot 5 (type 0x06) and slot 6
    (type 0x07) are template payloads that are byte-identical across all
    three kingdoms. Records flagged with (x_enc, z_enc) = (0x7F, 0x7F)
    are deliberately script-positioned and resolve to coordinates inside
    an overlay (world_map / dialog), not on disc. So extending this
    script to a disc-only walk is blocked on reversing the
    overlay-resident spawn logic; see docs/subsystems/world-map.md.
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


def run_one_state(state_path, bundle, extracted_root, verbose=True):
    """Run the full resolver against a single save state. Returns the
    output dict (the same shape that `--json` writes) or None on failure.

    Stateless wrapper around the original main()-body logic so callers
    can resolve multiple save states in one invocation."""
    ram = extract_ram(state_path)
    if verbose:
        print(f"\n=== {state_path} -> {bundle} ===")
        print(f"loaded {len(ram):#x} bytes of RAM")

    pack, byte_offsets, fname = load_kingdom_pack(extracted_root, bundle)
    if pack is None:
        if verbose:
            print(f"!! could not load kingdom pack for {bundle}")
        return None
    if verbose:
        print(
            f"kingdom pack: {fname}  {len(byte_offsets)} TMDs total, "
            f"pack size = {len(pack)}"
        )

    load_base = find_landmark_load_base(ram, pack, byte_offsets)
    if load_base is None:
        if verbose:
            print("!! could not locate landmark pack in RAM")
        return None
    if verbose:
        print(f"landmark pack loaded at RAM {load_base:#010x}")

    actors = collect_actors(ram)
    placements = []
    slot_use = Counter()
    unresolved_ptrs = 0
    total_ptrs = 0
    if verbose:
        print(f"{len(actors)} actors across all lists")
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
        unique_slots = sorted(set(s for s in slots if s >= 0))
        unresolved_for_actor = sum(1 for s in slots if s < 0)
        if verbose:
            pos_str = f"({a['x']:>5d},{a['y']:>5d},{a['z']:>5d})"
            slots_str = ",".join(str(s) for s in unique_slots) or "-"
            if unresolved_for_actor:
                slots_str += f" +{unresolved_for_actor}?"
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
                    source_state=str(state_path),
                )
            )
    return dict(
        bundle=bundle,
        kingdom_pack_load_base=f"{load_base:#010x}",
        slots_in_pack=len(byte_offsets),
        actors=placements,
        slot_usage={str(k): v for k, v in sorted(slot_use.items())},
        unresolved_ptrs=unresolved_ptrs,
        total_ptrs=total_ptrs,
        source_state=str(state_path),
    )


def merge_states(results):
    """Merge per-state results that share a `bundle` field. Actors are
    deduped by (node, pos). The latest state wins on metadata."""
    by_bundle: dict[str, dict] = {}
    for r in results:
        if r is None:
            continue
        b = r["bundle"]
        cur = by_bundle.get(b)
        if cur is None:
            by_bundle[b] = {
                **r,
                "actors": list(r["actors"]),
                "source_states": [r["source_state"]],
            }
            continue
        cur["source_states"].append(r["source_state"])
        # Newer-state metadata overrides
        cur["kingdom_pack_load_base"] = r["kingdom_pack_load_base"]
        cur["slots_in_pack"] = r["slots_in_pack"]
        seen = {(a["node"], tuple(a["pos"])) for a in cur["actors"]}
        for a in r["actors"]:
            key = (a["node"], tuple(a["pos"]))
            if key in seen:
                continue
            seen.add(key)
            cur["actors"].append(a)
        # Recompute slot_usage from the merged actor set
        slot_use = Counter()
        for a in cur["actors"]:
            for s in a["slots"]:
                slot_use[s] += 1
        cur["slot_usage"] = {str(k): v for k, v in sorted(slot_use.items())}
    if len(by_bundle) == 1:
        return next(iter(by_bundle.values()))
    return list(by_bundle.values())


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "state",
        nargs="+",
        help="PCSX-Redux save state path(s). When more than one is given, "
             "use --bundles to assign a bundle to each (positionally) or "
             "let --bundle apply to all.",
    )
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=sorted(KINGDOM_BASE.keys()),
        help="Kingdom bundle for every state when --bundles isn't given. "
             "Default: map01.",
    )
    ap.add_argument(
        "--bundles",
        help="Comma-separated bundle list aligned with the positional "
             "`state` args. Lets one call cover multiple kingdoms "
             "(e.g. --bundles map01,map02,map03 sstate1 sstate2 sstate3).",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        help="Extracted disc root (default: extracted/).",
    )
    ap.add_argument(
        "--json",
        help="Optional: write merged placements as JSON to this path. "
             "When the input covers a single bundle, the output is the "
             "legacy single-bundle dict; when it covers multiple, it's a "
             "list of bundle-dicts so site/extract-world-placements.py "
             "can merge them per-kingdom.",
    )
    args = ap.parse_args()

    if args.bundles:
        bundles = [b.strip() for b in args.bundles.split(",")]
        if len(bundles) != len(args.state):
            sys.exit(
                f"--bundles has {len(bundles)} entries but {len(args.state)} "
                f"state paths were given."
            )
    else:
        bundles = [args.bundle] * len(args.state)

    if len(args.state) == 1:
        # Preserve the original single-state output shape verbatim so any
        # downstream tooling that consumed the old structure still works.
        r = run_one_state(args.state[0], bundles[0], args.extracted)
        if r is None:
            return 1
        print(f"\n=== Resolution stats ===")
        print(f"  total chain pointers:  {r['total_ptrs']}")
        print(f"  resolved to slot:      {r['total_ptrs'] - r['unresolved_ptrs']}")
        print(f"  unresolved:            {r['unresolved_ptrs']}")
        print(
            f"  unique slots referenced: {len(r['slot_usage'])} / "
            f"{r['slots_in_pack']}"
        )
        if args.json:
            out_dict = {k: v for k, v in r.items() if k != "source_state"}
            Path(args.json).write_text(json.dumps(out_dict, indent=2))
            print(f"\nwrote {args.json}")
        return 0

    # Multi-state path
    results = [
        run_one_state(s, b, args.extracted)
        for s, b in zip(args.state, bundles)
    ]
    merged = merge_states(results)
    print(f"\n=== Merged across {len(args.state)} state(s) ===")
    if isinstance(merged, list):
        for m in merged:
            print(
                f"  bundle={m['bundle']:<8s}  actors={len(m['actors']):>3d}  "
                f"slots_referenced={len(m['slot_usage'])} / "
                f"{m['slots_in_pack']}"
            )
    else:
        print(
            f"  bundle={merged['bundle']:<8s}  actors={len(merged['actors']):>3d}  "
            f"slots_referenced={len(merged['slot_usage'])} / "
            f"{merged['slots_in_pack']}"
        )
    if args.json:
        Path(args.json).write_text(json.dumps(merged, indent=2))
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
