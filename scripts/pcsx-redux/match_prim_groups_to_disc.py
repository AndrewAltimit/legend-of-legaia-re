#!/usr/bin/env python3
"""
match_prim_groups_to_disc.py

Map the live in-RAM world-map prim-group region against the on-disc
kingdom bundle (map01..map03) so the disc-only renderer knows which
disc bytes become which prim-groups at runtime.

ALGORITHM
1. Walk the seven actor lists in a PCSX-Redux save state's RAM,
   collect every distinct mesh-pointer at `actor[+0x44] -> [count,
   ptrs[]]`. The mesh pointers cluster in `0x80130000..0x80160000`.
2. For each unique pointer P, snapshot a window of bytes starting at
   P from live RAM.
3. For each kingdom-bundle PROT slot (raw + LZS-decompressed +
   chunk-header peeled), search for an exact byte match of the live
   window.
4. The (slot, in-slot-offset) match tells us the load-time relocation:
   `P (psx_vaddr) == load_base + in_slot_offset`. Given enough
   distinct ptrs from one slot, the relocation base is pinned.

OUTPUT
For each unique prim-group pointer, prints:
    psx_vaddr  ->  slot=NNNN  off=0xXXXX  (run_len=N bytes)

and at the end a summary of the inferred load-base per slot.

USAGE
    python3 scripts/pcsx-redux/match_prim_groups_to_disc.py \\
        ~/Tools/pcsx-redux/SCUS94254.sstate2 \\
        --bundle map01

Default bundle PROT slot range is auto-discovered from CDNAME.TXT
under `extracted/`.
"""

import argparse
import gzip
import struct
import sys
from pathlib import Path


RAM_SIZE = 0x00800000
PSX_BASE = 0x80000000
WINDOW_SIZE = 128  # bytes of each prim-group used as a match probe

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


# Reuse the protobuf walker from walk_actor_lists.py - keep this script
# self-contained so it can be reviewed independently.
def read_varint(buf, off):
    val = 0
    shift = 0
    while True:
        b = buf[off]
        off += 1
        val |= (b & 0x7F) << shift
        if (b & 0x80) == 0:
            return val, off
        shift += 7


def find_field(buf, off, end, target_tag, target_wire):
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
    raw = gzip.decompress(Path(state_path).read_bytes())
    mem = find_field(raw, 0, len(raw), 3, 2)
    if mem is None:
        raise RuntimeError("memory message field not found")
    mem_start, mem_end = mem
    ram = find_field(raw, mem_start, mem_end, 1, 2)
    if ram is None:
        raise RuntimeError("ram bytes field not found")
    ram_start, ram_end = ram
    blob = raw[ram_start:ram_end]
    if len(blob) != RAM_SIZE:
        raise RuntimeError(f"unexpected RAM size {len(blob):#x}")
    return blob


def read_u32(ram, addr):
    if addr < PSX_BASE or addr + 4 > PSX_BASE + RAM_SIZE:
        return None
    off = addr - PSX_BASE
    return struct.unpack("<I", ram[off : off + 4])[0]


def collect_prim_group_ptrs(ram):
    """Walk every actor list head; collect every distinct mesh-pointer
    found at `actor[+0x44] -> ptrs[1..count]`."""
    seen_actors = set()
    ptrs = set()
    for list_addr in LIST_HEAD_ADDRS:
        node = read_u32(ram, list_addr)
        while node and node != 0xFFFFFFFF and node not in seen_actors:
            seen_actors.add(node)
            mesh_head = read_u32(ram, node + 0x44)
            if (
                mesh_head
                and mesh_head >= PSX_BASE
                and mesh_head + 4 < PSX_BASE + RAM_SIZE
            ):
                count = read_u32(ram, mesh_head)
                if count and count < 0x100:
                    for i in range(count):
                        p = read_u32(ram, mesh_head + 4 + 4 * i)
                        if p and PSX_BASE <= p < PSX_BASE + RAM_SIZE:
                            ptrs.add(p)
            node = read_u32(ram, node + 0x00)
    return sorted(ptrs)


def snapshot_window(ram, addr, size=WINDOW_SIZE):
    if addr < PSX_BASE or addr + size > PSX_BASE + RAM_SIZE:
        return None
    off = addr - PSX_BASE
    return bytes(ram[off : off + size])


def auto_discover_bundle_range(cdname_path, label):
    """Given a CDNAME label like 'map01', read CDNAME.TXT to find its
    starting PROT index. Return (start, end) inclusive."""
    txt = Path(cdname_path).read_text()
    starts = []
    label_lower = label.lower()
    target_start = None
    for line in txt.splitlines():
        line = line.strip()
        if not line.startswith("#define "):
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        name = parts[1]
        try:
            idx = int(parts[2])
        except ValueError:
            continue
        starts.append((idx, name))
        if name.lower() == label_lower:
            target_start = idx
    if target_start is None:
        raise RuntimeError(f"label {label!r} not found in {cdname_path}")
    starts.sort()
    end = None
    for idx, name in starts:
        if idx > target_start:
            end = idx - 1
            break
    if end is None:
        end = target_start + 7  # arbitrary fallback for the last block
    return target_start, end


def search_in_slot(slot_bytes, window):
    """Naive byte-search. Returns list of all match offsets (usually 0 or 1)."""
    if not window:
        return []
    matches = []
    start = 0
    while True:
        idx = slot_bytes.find(window, start)
        if idx == -1:
            break
        matches.append(idx)
        start = idx + 1
        if len(matches) > 4:  # cap to detect false positives
            break
    return matches


def lzs_decompress(src, expected_size):
    """Legaia LZS decoder (reverse-engineered from FUN_8001A55C).
    4 KiB ring buffer initialised to zeros; literal/back-ref control
    bits packed LSB-first in a byte; back-ref word is two bytes where
    low nibble of byte 1 is hi 4 bits of offset, byte 0 is lo 8 bits
    of offset, and length = (byte 1 >> 4) + 3.
    """
    out = bytearray()
    ring = bytearray(4096)
    rpos = 0xFEE
    src_pos = 0
    flags = 0
    flag_mask = 0
    while len(out) < expected_size:
        if flag_mask == 0:
            if src_pos >= len(src):
                raise RuntimeError(f"LZS EOF at out={len(out)}/{expected_size}")
            flags = src[src_pos]
            src_pos += 1
            flag_mask = 1
        if flags & flag_mask:
            # Literal byte
            if src_pos >= len(src):
                raise RuntimeError(f"LZS literal EOF at out={len(out)}")
            b = src[src_pos]
            src_pos += 1
            out.append(b)
            ring[rpos] = b
            rpos = (rpos + 1) & 0xFFF
        else:
            # Back-reference word
            if src_pos + 1 >= len(src):
                raise RuntimeError(f"LZS back-ref EOF at out={len(out)}")
            lo = src[src_pos]
            hi = src[src_pos + 1]
            src_pos += 2
            offset = lo | ((hi & 0xF0) << 4)
            length = (hi & 0x0F) + 3
            for _ in range(length):
                if len(out) >= expected_size:
                    break
                b = ring[offset & 0xFFF]
                out.append(b)
                ring[rpos] = b
                rpos = (rpos + 1) & 0xFFF
                offset += 1
        flag_mask = (flag_mask << 1) & 0xFF
    return bytes(out)


def find_asset_table(buf):
    """Mirror crates/web-viewer/.../find_asset_table_offset: scan
    0x800-aligned offsets for `count == 7 && descriptor[0].data_offset
    == 0x40`."""
    off = 0
    while off + 64 <= len(buf):
        count = struct.unpack("<I", buf[off : off + 4])[0]
        if count == 7:
            d0 = struct.unpack("<I", buf[off + 12 : off + 16])[0]
            if d0 == 0x40:
                return off
        off += 0x800
    return None


def load_slot_candidates(slot_path):
    """Try every plausible candidate from one PROT slot:
    - raw bytes
    - if file holds an asset descriptor at a 0x800-aligned offset, also
      LZS-decode slot 0 (TIM_LIST) and slot 1 (TMD pack)
    """
    raw = Path(slot_path).read_bytes()
    out = [("raw", raw)]

    table_off = find_asset_table(raw)
    if table_off is not None:
        table = raw[table_off:]
        try:
            slot0_ts = struct.unpack("<I", table[8:12])[0]
            slot0_off = struct.unpack("<I", table[12:16])[0]
            slot0_size = slot0_ts & 0x00FFFFFF
            slot0_type = slot0_ts >> 24
            slot1_ts = struct.unpack("<I", table[16:20])[0]
            slot1_off = struct.unpack("<I", table[20:24])[0]
            slot1_size = slot1_ts & 0x00FFFFFF
            slot1_type = slot1_ts >> 24
            if slot0_type == 0x01:
                tim_src = table[slot0_off:]
                tim_decoded = lzs_decompress(tim_src, slot0_size)
                out.append(("table+0_TIM_LIST_decoded", tim_decoded))
            if slot1_type == 0x02:
                tmd_src = table[slot1_off:]
                tmd_decoded = lzs_decompress(tmd_src, slot1_size)
                out.append(("table+1_TMD_pack_decoded", tmd_decoded))
        except Exception as e:
            print(f"  ! {slot_path.name}: asset-table decode error: {e}")
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("state", help="PCSX-Redux save state path")
    ap.add_argument(
        "--bundle",
        default="map01",
        help="CDNAME label of the kingdom bundle (default: map01).",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        help="Path to the extracted disc root (default: extracted/).",
    )
    args = ap.parse_args()

    ram = extract_ram(args.state)
    print(f"loaded {len(ram):#x} bytes of RAM from {args.state}\n")

    ptrs = collect_prim_group_ptrs(ram)
    print(f"distinct prim-group pointers in actor mesh chains: {len(ptrs)}")
    if ptrs:
        print(
            f"  range: {min(ptrs):#010x} .. {max(ptrs):#010x}  "
            f"span: {(max(ptrs) - min(ptrs)) / 1024:.1f} KiB\n"
        )

    start_idx, end_idx = auto_discover_bundle_range(
        Path(args.extracted) / "CDNAME.TXT", args.bundle
    )
    print(f"bundle {args.bundle!r}: PROT entries {start_idx}..{end_idx}\n")

    # Load all slot candidates up front.
    slot_files = sorted(
        (Path(args.extracted) / "PROT").glob(f"[0-9][0-9][0-9][0-9]_*.BIN")
    )
    slot_files = [f for f in slot_files if start_idx <= int(f.name[:4]) <= end_idx]

    slots = []  # list of (idx, label, slot_bytes_dict)
    for f in slot_files:
        idx = int(f.name[:4])
        candidates = load_slot_candidates(f)
        slots.append((idx, f.name, candidates))
        for label, b in candidates:
            print(f"  slot {idx:04d} {f.name} [{label}] = {len(b):#x} bytes")
    print()

    # Per slot, accumulate (psx_vaddr, in_slot_off) match pairs to
    # infer the relocation base.
    matches_per_slot = {idx: [] for idx, _, _ in slots}
    no_match = []
    multi_match = []

    print("Per-pointer match results:")
    for p in ptrs:
        window = snapshot_window(ram, p)
        if window is None:
            print(f"  {p:#010x}  -- out of RAM range")
            continue
        hit = None
        all_hits = []
        for idx, label, candidates in slots:
            for cand_label, slot_bytes in candidates:
                matches = search_in_slot(slot_bytes, window)
                for m in matches:
                    all_hits.append((idx, cand_label, m))
        if not all_hits:
            no_match.append(p)
            print(f"  {p:#010x}  -- NO MATCH in any bundle slot")
        elif len(all_hits) == 1:
            idx, lab, off = all_hits[0]
            print(f"  {p:#010x}  ->  slot {idx:04d} [{lab}] off={off:#x}")
            matches_per_slot[idx].append((p, off, lab))
        else:
            multi_match.append((p, all_hits))
            # Pick the first match for relocation inference but flag.
            idx, lab, off = all_hits[0]
            print(
                f"  {p:#010x}  ->  slot {idx:04d} [{lab}] off={off:#x} "
                f"(+{len(all_hits) - 1} more matches across slots)"
            )
            matches_per_slot[idx].append((p, off, lab))

    print()
    print("=== Summary ===")
    print(f"  matched:  {len(ptrs) - len(no_match)}")
    print(f"  no match: {len(no_match)}")
    print(f"  ambiguous (>1 candidate): {len(multi_match)}")

    print()
    print("Inferred load-base per slot (psx_vaddr - in_slot_off):")
    for idx, hits in matches_per_slot.items():
        if not hits:
            continue
        bases = set()
        for psx, off, lab in hits:
            bases.add((psx - off, lab))
        if len(bases) == 1:
            base, lab = bases.pop()
            print(
                f"  slot {idx:04d} [{lab}]: load_base = {base:#010x}  "
                f"(N={len(hits)} consistent ptrs)"
            )
        else:
            print(f"  slot {idx:04d}: INCONSISTENT load bases (N={len(hits)} ptrs):")
            for base, lab in sorted(bases):
                count = sum(1 for psx, off, l in hits if (psx - off) == base and l == lab)
                print(f"    [{lab}] base={base:#010x}  N={count}")


if __name__ == "__main__":
    sys.exit(main() or 0)
