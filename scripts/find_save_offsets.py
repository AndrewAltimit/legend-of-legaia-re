#!/usr/bin/env python3
"""
find_save_offsets.py

Locates story_flags and inventory data within the Legaia PSX memory-card
save block by cross-referencing live RAM (mednafen save state) against the
MCR save block.

Steps:
 1. Parse a mednafen save state (.mc0..mc9) to extract main RAM via the
    MDFNSVST MAIN.MainRAM.data8 sub-entry.
 2. Read story-flags (0x80085600..0x80085800, 512 bytes) from RAM.
 3. Read inventory (0x80085958..0x80085A48, 144 bytes) from RAM.
 4. Parse the MCR save block (block 1 = Drake, block 2 = Sebucus).
 5. Search the display header (block+0x200..block+0x86F) for the live data.
 6. Report matches and non-zero regions of the header sorted by similarity.
 7. Interpret any inventory candidates as (item_id, count) pairs.
"""

import gzip
import struct
import sys
import os

# ── Constants ─────────────────────────────────────────────────────────────────

MDFN_MAGIC = b"MDFNSVST"
MDFN_HEADER_LEN = 0x18
SECTION_NAME_LEN = 32
PSX_RAM_KSEG0 = 0x80000000
PSX_RAM_SIZE = 2 * 1024 * 1024

MCR_BLOCK_SIZE = 0x2000
MCR_DIR_FRAME_SIZE = 0x80

# Legaia save layout constants (from crates/save/src/card.rs)
RETAIL_GAME_DATA_OFFSET = 0x200      # SC block → game data
RETAIL_CHAR_RECORD_HEADER_SIZE = 0x66F  # "display header" ends here
RETAIL_CHAR_RECORD_STRIDE = 0x414

# PSX virtual addresses for the data we want
STORY_FLAGS_START = 0x80085600
STORY_FLAGS_END   = 0x80085800
INVENTORY_START   = 0x80085958
INVENTORY_END     = 0x800859E8   # 0x90 = 144 bytes (72 slots * 2 bytes)

# Item name table (index = item_id).  Source: data/cheats + gamedata.
# Item IDs are 1-based (0 = empty slot). This is a best-effort partial table;
# gaps are filled with None.
ITEM_NAMES = {
    0x01: "Healing Leaf",
    0x02: "Healing Shroom",
    0x03: "Healing Bloom",
    0x04: "Healing Flower",
    0x05: "Healing Fruit",
    0x06: "Healing Berry",
    0x07: "Soru Bread",
    0x08: "Magic Leaf",
    0x09: "Magic Fruit",
    0x0A: "Phoenix",
    0x0B: "Antidote",
    0x0C: "Medicine",
    0x0D: "Resurrect",
    0x0E: "Ivory Book",
    0x0F: "Power Potion",
    0x10: "Castor Oil",
    0x11: "Muscle Drink",
    0x12: "Spirit Potion",
    0x13: "Smart Drink",
    0x14: "Tough Armet",
    0x15: "Amethyst",
    0x16: "Life Ring",
    0x17: "Speed Ring",
    0x18: "Wing",
    0x19: "Door of Wind",
    0x1A: "Gimard's Ashes",
    0x1B: "Chronicle",
    0x1C: "Warrior's Lion?",
    0x1D: "Fishing Bait",
    0x1E: "Lure",
    0x1F: "Old Lure",
    0x20: "Ugly Lure",
    0x21: "Wonder Lure",
    0x22: "Miracle Lure",
    0x23: "Fire Book",
    0x24: "Wind Book",
    0x25: "Earth Book",
    0x26: "Water Book",
    0x27: "Thunder Book",
    0x28: "Light Book",
    0x29: "Dark Book",
    0x2A: "Evil Book",
    0x2B: "Hyper Art Scroll?",
}

# ── Mednafen save-state parser ─────────────────────────────────────────────────

def load_save_state(path: str) -> bytes:
    """Decompress a mednafen .mcX gzip save state. Returns raw payload bytes."""
    with open(path, "rb") as f:
        raw = f.read()
    return gzip.decompress(raw)


def find_section(payload: bytes, name: str):
    """
    Targeted linear scan for a top-level section header.
    Returns (body_offset, body_len) or None.

    Section format: [32-byte NUL-padded name][4-byte LE body size][body]
    """
    needle = name.encode("ascii")
    if len(needle) > SECTION_NAME_LEN:
        return None
    padded = needle + b"\x00" * (SECTION_NAME_LEN - len(needle))
    pos = MDFN_HEADER_LEN
    while pos + SECTION_NAME_LEN + 4 <= len(payload):
        idx = payload.find(padded, pos)
        if idx == -1:
            return None
        size_off = idx + SECTION_NAME_LEN
        body_len = struct.unpack_from("<I", payload, size_off)[0]
        body_off = size_off + 4
        if body_off + body_len > len(payload) or body_len > 4 * 1024 * 1024:
            pos = idx + SECTION_NAME_LEN
            continue
        return (body_off, body_len)
    return None


def walk_subentries(payload: bytes, body_off: int, body_len: int):
    """
    Walk sub-entries within a section body.
    Each entry: [1-byte name_len][name][4-byte LE value_len][value]
    Returns dict {name: (value_offset, value_len)}.
    """
    out = {}
    pos = body_off
    end = body_off + body_len
    while pos < end:
        if pos >= len(payload):
            break
        name_len = payload[pos]
        if name_len == 0 or pos + 1 + name_len + 4 > end:
            break
        name_bytes = payload[pos + 1 : pos + 1 + name_len]
        if not all(0x20 <= b <= 0x7E for b in name_bytes):
            break
        name = name_bytes.decode("ascii")
        val_size_off = pos + 1 + name_len
        val_len = struct.unpack_from("<I", payload, val_size_off)[0]
        val_off = val_size_off + 4
        if val_off + val_len > end:
            break
        out[name] = (val_off, val_len)
        pos = val_off + val_len
    return out


def extract_main_ram(payload: bytes) -> bytes:
    """
    Extract the 2 MB PSX main RAM from a decompressed MDFNSVST payload.
    Uses the structured MAIN.MainRAM.data8 path.
    """
    if payload[:8] != MDFN_MAGIC:
        raise ValueError(f"Bad magic: {payload[:8]!r}")

    sec = find_section(payload, "MAIN")
    if sec is None:
        raise ValueError("MAIN section not found in save state")
    body_off, body_len = sec
    entries = walk_subentries(payload, body_off, body_len)

    if "MainRAM.data8" not in entries:
        raise ValueError("MainRAM.data8 sub-entry not found in MAIN section")

    val_off, val_len = entries["MainRAM.data8"]
    if val_len != PSX_RAM_SIZE:
        raise ValueError(f"MainRAM.data8 size {val_len} != expected {PSX_RAM_SIZE}")

    return payload[val_off : val_off + val_len]


def ram_slice(ram: bytes, start: int, end: int) -> bytes:
    """Slice a PSX virtual address window [start, end) from main RAM."""
    lo = start - PSX_RAM_KSEG0
    hi = end   - PSX_RAM_KSEG0
    if lo < 0 or hi > PSX_RAM_SIZE or lo > hi:
        raise ValueError(f"Address range 0x{start:08X}..0x{end:08X} out of bounds")
    return ram[lo:hi]


# ── MCR save block parser ──────────────────────────────────────────────────────

def read_mcr_block(mcr_path: str, block_idx: int) -> bytes:
    """Read one 8 KB save block from an MCR image."""
    with open(mcr_path, "rb") as f:
        mcr = f.read()
    if mcr[:2] != b"MC":
        raise ValueError("Not an MCR image (missing 'MC' magic)")
    off = MCR_BLOCK_SIZE * block_idx
    return mcr[off : off + MCR_BLOCK_SIZE]


def mcr_block_description(mcr_path: str, block_idx: int) -> str:
    """Return the product code + location name string for a save block."""
    with open(mcr_path, "rb") as f:
        mcr = f.read()
    frame_off = MCR_DIR_FRAME_SIZE * block_idx
    frame = mcr[frame_off : frame_off + MCR_DIR_FRAME_SIZE]
    product_code = frame[10:30].rstrip(b"\x00").decode("ascii", "replace")
    block = mcr[MCR_BLOCK_SIZE * block_idx : MCR_BLOCK_SIZE * (block_idx + 1)]
    # Location name is at game_data + 0x000
    game_data = block[RETAIL_GAME_DATA_OFFSET : RETAIL_GAME_DATA_OFFSET + 0x40]
    loc_name = game_data[:0x30].rstrip(b"\x00").decode("ascii", "replace")
    return f"{product_code} / '{loc_name}'"


# ── Search helpers ─────────────────────────────────────────────────────────────

def search_verbatim(haystack: bytes, needle: bytes, label: str):
    """Search haystack for needle verbatim. Print result."""
    pos = haystack.find(needle)
    if pos != -1:
        print(f"  VERBATIM MATCH for {label} at haystack+0x{pos:04X}")
        return pos
    print(f"  No verbatim match for {label}")
    return None


def nonzero_regions(data: bytes, min_run: int = 4):
    """Return list of (offset, bytes) for non-zero runs >= min_run."""
    regions = []
    i = 0
    while i < len(data):
        if data[i] != 0:
            j = i
            while j < len(data) and data[j] != 0:
                j += 1
            if j - i >= min_run:
                regions.append((i, data[i:j]))
            i = j
        else:
            i += 1
    return regions


def count_matching_bytes(a: bytes, b: bytes) -> int:
    """Count identical bytes between two equal-length byte strings."""
    return sum(x == y for x, y in zip(a, b))


def sliding_similarity(haystack: bytes, needle: bytes, step: int = 1):
    """
    Return the best (offset, match_count) for needle sliding over haystack.
    Only checks positions where needle fits entirely.
    """
    n = len(needle)
    best = (0, 0)
    for i in range(0, len(haystack) - n + 1, step):
        m = count_matching_bytes(haystack[i:i+n], needle)
        if m > best[1]:
            best = (i, m)
    return best


def search_with_sliding(haystack: bytes, needle: bytes, label: str, top_n: int = 5):
    """
    If verbatim search fails, try a 16-byte sliding window to find
    the region most similar to the needle.
    """
    n = len(needle)
    results = []
    for i in range(0, len(haystack) - n + 1, 1):
        m = count_matching_bytes(haystack[i:i+n], needle)
        results.append((m, i))
    results.sort(reverse=True)
    print(f"  Top {top_n} sliding similarity matches for {label} ({n} bytes):")
    for rank, (m, off) in enumerate(results[:top_n]):
        pct = 100.0 * m / n
        window = haystack[off:off+n]
        print(f"    [{rank+1}] offset=0x{off:04X}  matching={m}/{n}  ({pct:.1f}%)  "
              f"first16={window[:16].hex()}")


def search_4byte_windows(haystack: bytes, needle: bytes, label: str, min_match: int = 3):
    """
    Look for 4-byte chunks of needle that appear in haystack.
    Useful if the data is stored in a different byte order or interleaved.
    """
    hits = {}
    for i in range(0, len(needle) - 3, 4):
        chunk = needle[i:i+4]
        if chunk == b"\x00\x00\x00\x00":
            continue
        pos = 0
        while True:
            found = haystack.find(chunk, pos)
            if found == -1:
                break
            hits.setdefault(found, []).append(i)
            pos = found + 1
    if hits:
        # Group by haystack region
        by_region = {}
        for hpos, needle_offsets in hits.items():
            region = hpos & ~0x3F
            by_region.setdefault(region, []).append((hpos, needle_offsets))
        print(f"  4-byte chunk hits in haystack for {label}:")
        for reg, matches in sorted(by_region.items())[:8]:
            print(f"    region 0x{reg:04X}: {len(matches)} hits")


# ── Inventory decoder ──────────────────────────────────────────────────────────

def decode_inventory(inv_bytes: bytes) -> list:
    """
    Decode 144 inventory bytes as 72 × (item_id, count) u8 pairs.
    Returns list of non-empty slots.
    """
    slots = []
    for i in range(72):
        item_id = inv_bytes[i * 2]
        count   = inv_bytes[i * 2 + 1]
        if item_id != 0 or count != 0:
            name = ITEM_NAMES.get(item_id, f"Unknown(0x{item_id:02X})")
            slots.append((i, item_id, count, name))
    return slots


def decode_inventory_at(data: bytes, offset: int, n_slots: int = 72) -> list:
    """
    Try to interpret `data[offset:]` as (item_id, count) pairs.
    Returns list of non-empty slots.
    """
    slots = []
    for i in range(min(n_slots, (len(data) - offset) // 2)):
        item_id = data[offset + i * 2]
        count   = data[offset + i * 2 + 1]
        if item_id != 0 or count != 0:
            name = ITEM_NAMES.get(item_id, f"Unknown(0x{item_id:02X})")
            slots.append((i, item_id, count, name))
    return slots


# ── Main ───────────────────────────────────────────────────────────────────────

def analyze_mc_file(mc_path: str, mcr_path: str, block_idx: int, label: str):
    print(f"\n{'='*70}")
    print(f"Save state: {mc_path}")
    print(f"MCR block {block_idx}: {label}")
    print(f"{'='*70}")

    # ── Step 1: extract main RAM ──────────────────────────────────────────────
    print("\n[1] Extracting main RAM from save state...")
    payload = load_save_state(mc_path)
    ram = extract_main_ram(payload)
    print(f"  Main RAM: {len(ram)} bytes (OK)")

    # ── Step 2: read story flags ──────────────────────────────────────────────
    print("\n[2] Reading story flags from RAM...")
    story_flags = ram_slice(ram, STORY_FLAGS_START, STORY_FLAGS_END)
    nz_sf = sum(1 for b in story_flags if b != 0)
    print(f"  story_flags (0x{STORY_FLAGS_START:08X}..0x{STORY_FLAGS_END:08X}): "
          f"{len(story_flags)} bytes, {nz_sf} non-zero bytes")
    print(f"  First 32 bytes: {story_flags[:32].hex()}")
    print(f"  Last  32 bytes: {story_flags[-32:].hex()}")

    # ── Step 3: read inventory ────────────────────────────────────────────────
    print("\n[3] Reading inventory from RAM...")
    inventory = ram_slice(ram, INVENTORY_START, INVENTORY_END)
    print(f"  inventory (0x{INVENTORY_START:08X}..0x{INVENTORY_END:08X}): "
          f"{len(inventory)} bytes")
    print(f"  Raw bytes: {inventory.hex()}")
    slots = decode_inventory(inventory)
    print(f"  Non-empty slots ({len(slots)}):")
    for slot_i, item_id, count, name in slots:
        print(f"    slot[{slot_i:2d}] id=0x{item_id:02X} count={count:3d}  {name}")

    # ── Step 4: load MCR block ────────────────────────────────────────────────
    print(f"\n[4] Loading MCR block {block_idx}...")
    sc_block = read_mcr_block(mcr_path, block_idx)
    game_data = sc_block[RETAIL_GAME_DATA_OFFSET:]
    header = game_data[:RETAIL_CHAR_RECORD_HEADER_SIZE]   # 0x66F bytes
    print(f"  SC block: {len(sc_block)} bytes")
    print(f"  Game data region: {len(game_data)} bytes (block+0x{RETAIL_GAME_DATA_OFFSET:04X})")
    print(f"  Display header: 0x{len(header):04X} bytes "
          f"(block+0x{RETAIL_GAME_DATA_OFFSET:04X}..block+0x{RETAIL_GAME_DATA_OFFSET + len(header):04X})")

    # Show some well-known offsets in the header
    print(f"  Location name (hdr+0x000): {header[0:0x30].rstrip(b'\\x00').decode('ascii','replace')!r}")
    print(f"  Char name    (hdr+0x054): {header[0x54:0x64].rstrip(b'\\x00').decode('ascii','replace')!r}")
    print(f"  hdr+0x200 (CDNAME label): {header[0x200:0x218].rstrip(b'\\x00').decode('ascii','replace')!r}")

    # Non-zero regions in the header
    nz_regions = nonzero_regions(header, min_run=4)
    print(f"\n  Non-zero regions in display header (>= 4 bytes):")
    for off, data in nz_regions:
        end_off = off + len(data)
        print(f"    hdr+0x{off:04X}..0x{end_off:04X} ({len(data)} bytes): "
              f"{data[:24].hex()}{' ...' if len(data) > 24 else ''}")

    # ── Step 5: search for story flags verbatim ───────────────────────────────
    print(f"\n[5] Searching display header for story_flags (verbatim)...")
    sf_pos = search_verbatim(header, story_flags, "story_flags")

    if sf_pos is None:
        # Try non-zero prefix (first 64 non-zero bytes)
        nz_prefix = bytes(b for b in story_flags if b != 0)[:32]
        if nz_prefix:
            print(f"  Searching for first 32 non-zero story-flag bytes: {nz_prefix.hex()}")
            pos2 = header.find(nz_prefix)
            if pos2 != -1:
                print(f"  Found at hdr+0x{pos2:04X}")
            else:
                # Try 16-byte chunks
                for chunk_off in range(0, len(story_flags), 16):
                    chunk = story_flags[chunk_off:chunk_off+16]
                    if chunk.count(0) < 8:  # at least half non-zero
                        pos3 = header.find(chunk)
                        if pos3 != -1:
                            print(f"  Chunk at sf+0x{chunk_off:03X} found at hdr+0x{pos3:04X}: {chunk.hex()}")

    # Sliding similarity for story flags (use first 64 bytes as representative)
    sf_sample = story_flags[:64] if nz_sf > 0 else story_flags[:64]
    search_with_sliding(header, sf_sample, "story_flags (first 64 bytes)")

    # ── Step 6: search for inventory verbatim ─────────────────────────────────
    print(f"\n[6] Searching display header for inventory (verbatim)...")
    inv_pos = search_verbatim(header, inventory, "inventory")

    if inv_pos is None:
        # Try non-zero prefix
        nz_inv = bytes(b for b in inventory if b != 0)[:16]
        if nz_inv and len(nz_inv) >= 4:
            print(f"  Searching for non-zero inventory bytes: {nz_inv.hex()}")
            pos2 = header.find(nz_inv)
            if pos2 != -1:
                print(f"  Found at hdr+0x{pos2:04X}")

        # Try verbatim search in the full block beyond the header
        full_game = game_data
        inv_pos_full = search_verbatim(full_game, inventory, "inventory (full game_data)")
        if inv_pos_full is not None:
            char_record_off = inv_pos_full
            print(f"  Inventory lives at game_data+0x{char_record_off:04X} "
                  f"(block+0x{RETAIL_GAME_DATA_OFFSET + char_record_off:04X})")

        # Sliding similarity
        if len(slots) > 0:
            # Use the non-empty portion as needle
            # Find last non-empty slot
            last_slot_i = slots[-1][0]
            inv_needle = inventory[: (last_slot_i + 1) * 2]
            if len(inv_needle) >= 4:
                print(f"  Sliding similarity for inventory ({len(inv_needle)} bytes = slots 0..{last_slot_i}):")
                search_with_sliding(header, inv_needle, "inventory_nonempty")
                # Also in full game data
                off, m = sliding_similarity(full_game, inv_needle)
                pct = 100.0 * m / len(inv_needle)
                print(f"  Best match in full game_data: offset=0x{off:04X} "
                      f"({m}/{len(inv_needle)} = {pct:.1f}%)")
                if pct > 50:
                    candidate = full_game[off : off + len(inv_needle)]
                    print(f"  Candidate bytes: {candidate.hex()}")
                    cand_slots = decode_inventory_at(full_game, off)
                    if cand_slots:
                        print(f"  Interpreted as inventory:")
                        for si, iid, cnt, nm in cand_slots:
                            print(f"    slot[{si:2d}] id=0x{iid:02X} count={cnt:3d}  {nm}")

    # ── Step 7: scan hdr+0x4C0..0x5FF as potential inventory/flag area ────────
    print(f"\n[7] Scanning hdr+0x4C0..0x5FF as 16-bit words and inventory candidates...")
    probe_start = 0x4C0
    probe_end   = min(0x600, len(header))
    probe = header[probe_start:probe_end]
    print(f"  Raw bytes: {probe.hex()}")
    print(f"  As 16-bit LE words:")
    for i in range(0, len(probe), 2):
        w = struct.unpack_from("<H", probe, i)[0]
        print(f"    +0x{probe_start+i:04X}: 0x{w:04X} ({w})")

    # Try treating this region as inventory (item_id, count) pairs
    print(f"\n  Treating hdr+0x4C0 as (item_id, count) pairs:")
    inv_cand = decode_inventory_at(header, probe_start)
    if inv_cand:
        for si, iid, cnt, nm in inv_cand:
            print(f"    slot[{si:2d}] id=0x{iid:02X} count={cnt:3d}  {nm}")
    else:
        print("    (no non-empty slots if id=0 is 'empty')")

    # Also try: odd bytes = item_id, even bytes = count (swapped)
    print(f"\n  Treating hdr+0x4C0 as (count, item_id) pairs (swapped):")
    inv_swapped = []
    for i in range(0, len(probe) - 1, 2):
        count   = probe[i]
        item_id = probe[i + 1]
        if item_id != 0 or count != 0:
            name = ITEM_NAMES.get(item_id, f"Unknown(0x{item_id:02X})")
            inv_swapped.append((i // 2, item_id, count, name))
    if inv_swapped:
        for si, iid, cnt, nm in inv_swapped:
            print(f"    slot[{si:2d}] id=0x{iid:02X} count={cnt:3d}  {nm}")
    else:
        print("    (no non-empty slots)")

    # ── Step 8: cross-check character name positions ──────────────────────────
    print(f"\n[8] Cross-checking character names in save block...")
    for name in [b"Vahn", b"Noa\x00", b"Gala"]:
        pos = sc_block.find(name)
        if pos != -1:
            game_off = pos - RETAIL_GAME_DATA_OFFSET
            char_off = game_off - RETAIL_CHAR_RECORD_HEADER_SIZE
            print(f"  '{name.rstrip(b'\\x00').decode()}' at block+0x{pos:04X} "
                  f"= game_data+0x{game_off:04X} "
                  f"= char_off 0x{char_off:04X}")

    # ── Step 9: search entire save block for inventory match ──────────────────
    print(f"\n[9] Searching ENTIRE save block for inventory verbatim...")
    inv_pos_block = sc_block.find(inventory)
    if inv_pos_block != -1:
        print(f"  FOUND at block+0x{inv_pos_block:04X}")
        print(f"  = game_data + 0x{inv_pos_block - RETAIL_GAME_DATA_OFFSET:04X}")
    else:
        print("  Not found verbatim in block.")
        # If there are non-zero slots, try just the non-empty part
        if slots:
            last_idx = slots[-1][0]
            inv_short = inventory[:(last_idx+1)*2]
            pos2 = sc_block.find(inv_short)
            if pos2 != -1:
                print(f"  Non-empty portion ({len(inv_short)} bytes) found at block+0x{pos2:04X}")
                print(f"  = game_data + 0x{pos2 - RETAIL_GAME_DATA_OFFSET:04X}")

    # ── Step 10: search entire save block for story flags ─────────────────────
    print(f"\n[10] Searching ENTIRE save block for story_flags verbatim...")
    sf_pos_block = sc_block.find(story_flags)
    if sf_pos_block != -1:
        print(f"  FOUND at block+0x{sf_pos_block:04X}")
    else:
        print("  Not found verbatim in block.")
        # Try a 64-byte prefix starting from first non-zero byte
        first_nz = next((i for i, b in enumerate(story_flags) if b != 0), None)
        if first_nz is not None:
            sf_chunk = story_flags[first_nz:first_nz+32]
            print(f"  Trying 32-byte chunk starting at sf[{first_nz}]: {sf_chunk.hex()}")
            pos2 = sc_block.find(sf_chunk)
            if pos2 != -1:
                print(f"  Found at block+0x{pos2:04X} = game_data+0x{pos2 - RETAIL_GAME_DATA_OFFSET:04X}")
            else:
                # Byte-by-byte scan of 16-byte overlapping windows
                best_m, best_pos = 0, 0
                for woff in range(0, len(sc_block) - len(story_flags) + 1):
                    m = count_matching_bytes(sc_block[woff:woff+len(story_flags)], story_flags)
                    if m > best_m:
                        best_m = m
                        best_pos = woff
                pct = 100.0 * best_m / len(story_flags)
                print(f"  Best block-wide match: block+0x{best_pos:04X}, "
                      f"{best_m}/{len(story_flags)} bytes match ({pct:.1f}%)")


def main():
    MCS_DIR = os.path.expanduser(
        "~/.mednafen/mcs/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055."
    )
    MCR_PATH = os.path.expanduser(
        "~/.mednafen/sav/Legend of Legaia (USA).788de08f9c7e652c51d8d08ee374d055.0.mcr"
    )

    # mc0 and mc5 are likely world-map states; try the first available
    # Choose mc that best matches Drake (block 1): try mc0 first.
    # We'll run both Drake (block 1) and Sebucus (block 2) against mc0.

    mc_files_to_try = [MCS_DIR + f"mc{i}" for i in range(10)]
    mc_available = [p for p in mc_files_to_try if os.path.exists(p)]

    if not mc_available:
        print("ERROR: No mednafen save state files found.")
        sys.exit(1)

    # For each mc file, try extracting RAM and print a brief location summary
    print("Scanning mc files for world-map states (looking for location name at RAM match)...")
    for mc_path in mc_available:
        try:
            payload = load_save_state(mc_path)
            ram = extract_main_ram(payload)
            # Read location name area from character struct: 0x80084708 + 0x000
            # The party gold is at 0x8008459C
            gold_bytes = ram_slice(ram, 0x8008459C, 0x800845A0)
            gold = struct.unpack_from("<I", gold_bytes)[0]
            # Map name at 0x80084708 + 0x000 (from char record 0)
            # Actually the save-block location name lives at game_data+0x000
            # In RAM the scene name is at 0x80084540 (active scene slot)
            scene_slot_bytes = ram_slice(ram, 0x80084540, 0x80084542)
            scene_slot = struct.unpack_from("<H", scene_slot_bytes)[0]
            story_flags = ram_slice(ram, STORY_FLAGS_START, STORY_FLAGS_END)
            nz = sum(1 for b in story_flags if b != 0)
            inv = ram_slice(ram, INVENTORY_START, INVENTORY_END)
            inv_slots = decode_inventory(inv)
            mc_name = os.path.basename(mc_path)
            print(f"  {mc_name}: gold={gold}  scene_slot=0x{scene_slot:04X}  "
                  f"sf_nz={nz}  inv_slots={len(inv_slots)}")
        except Exception as e:
            print(f"  {os.path.basename(mc_path)}: error: {e}")

    # Run the full analysis on mc0 vs Drake block 1
    mc_path = MCS_DIR + "mc0"
    if not os.path.exists(mc_path):
        mc_path = mc_available[0]
    analyze_mc_file(mc_path, MCR_PATH, block_idx=1, label="Drake Kingdom (block 1)")

    # Also run on mc0 vs Sebucus block 2 for cross-check
    analyze_mc_file(mc_path, MCR_PATH, block_idx=2, label="Sebucus Islands (block 2)")


if __name__ == "__main__":
    main()
