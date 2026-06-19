#!/usr/bin/env python3
"""Map the disc reads logged by autorun_battle_char_clut_source.lua to PROT
entries, and (optionally) confirm which entry's decompressed content holds the
retail battle character CLUT band.

Input: the probe CSV (tick,kind,a0,sectors,cdloc,lba,prot_dat_off,dest,ra).

Step 1 (always): for each logged read/seek, resolve prot_dat_off to the owning
PROT entry by bracketing against the extracted PROT/*.BIN locations in PROT.DAT.

Step 2 (with --vram <mc.bin>): for each distinct candidate entry, LZS-decompress
its sections and search for the retail Noa (row 492) / Gala (row 494) palette
lifted from the supplied VRAM dump. Prints the entry + offset that holds it -
that pins the disc source. No Sony bytes are committed; this reads the user's
own disc + save-state dump locally.

Usage:
    python3 scripts/pcsx-redux/map_clut_disc_reads.py <probe.csv> \
        [--extracted extracted] [--vram /path/to/mc2_vram.bin]
"""
import argparse
import csv
import os
import struct
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def build_prot_index(extracted):
    """Return sorted [(prot_dat_offset, size, name)] for every extracted entry."""
    pdat = open(os.path.join(extracted, "PROT.DAT"), "rb").read()
    prot_dir = os.path.join(extracted, "PROT")
    index = []
    for fn in sorted(os.listdir(prot_dir)):
        fp = os.path.join(prot_dir, fn)
        if not os.path.isfile(fp):
            continue
        head = open(fp, "rb").read(64)
        if len(head) < 64:
            continue
        pos = pdat.find(head)
        if pos != -1:
            index.append((pos, os.path.getsize(fp), fn))
    index.sort()
    return pdat, index


def owner_entry(index, off):
    """Bracket a PROT.DAT byte offset to its owning entry (extended footprint:
    an entry owns bytes from its start up to the next entry's start)."""
    best = None
    for i, (start, size, name) in enumerate(index):
        if start <= off:
            nxt = index[i + 1][0] if i + 1 < len(index) else (1 << 62)
            if off < nxt:
                best = name
        else:
            break
    return best


def try_search_entry(extracted, name, needles):
    """LZS-decompress an entry's player.lzs sections; search for any needle.
    Returns list of (label, descriptor_index, offset)."""
    sys.path.insert(0, REPO)
    # Use the asset CLI's decode path via subprocess would be heavy; instead do
    # a light player.lzs walk + the bundled lzs decoder if available.
    try:
        import subprocess
        fp = os.path.join(extracted, "PROT", name)
        # Reuse the asset CLI: describe to get descriptors, decode each as lzs.
        out = subprocess.run(
            ["./target/release/asset", "describe", fp, "--count", "8"],
            cwd=REPO, capture_output=True, text=True, timeout=30)
        hits = []
        for line in out.stdout.splitlines():
            parts = line.split()
            # rows look like:  0  0x01   349872  0x00000040  TIM_LIST
            if len(parts) >= 5 and parts[1].startswith("0x"):
                di = int(parts[0])
                typ = parts[1]
                try:
                    size = int(parts[2])
                    off = int(parts[3], 16)
                except ValueError:
                    continue
                type_byte = int(typ, 16)
                type_size = f"0x{(type_byte << 24) | size:08x}"
                dec = subprocess.run(
                    ["./target/release/asset", "decode", fp,
                     "--type-size", type_size, "--offset", f"0x{off:x}",
                     "--mode", "lzs", "--out", "/tmp/_clut_probe_sec.bin"],
                    cwd=REPO, capture_output=True, text=True, timeout=60)
                if dec.returncode != 0:
                    continue
                data = open("/tmp/_clut_probe_sec.bin", "rb").read()
                for label, needle in needles.items():
                    p = data.find(needle)
                    if p != -1:
                        hits.append((label, di, p))
        return hits
    except Exception as e:  # noqa: BLE001
        return [("error:" + str(e), -1, -1)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("--extracted", default="extracted")
    ap.add_argument("--vram", help="save-state VRAM .bin to lift retail palettes from")
    args = ap.parse_args()

    extracted = os.path.join(REPO, args.extracted) if not os.path.isabs(args.extracted) else args.extracted
    pdat, index = build_prot_index(extracted)
    print(f"[map] indexed {len(index)} PROT entries from PROT.DAT")

    reads = []
    with open(args.csv) as f:
        for row in csv.DictReader(f):
            off = row.get("prot_dat_off", "")
            if not off:
                continue
            try:
                off = int(off, 16)
            except ValueError:
                continue
            reads.append((row["tick"], row["kind"], off, row.get("ra", "")))

    # distinct candidate entries, in read order
    seen = []
    for tick, kind, off, ra in reads:
        name = owner_entry(index, off) or "??"
        key = name
        if key not in [s[0] for s in seen]:
            seen.append((name, tick, kind, off, ra))
    print(f"[map] {len(reads)} disc ops -> {len(seen)} distinct PROT entries:")
    for name, tick, kind, off, ra in seen:
        print(f"   tick {tick:>5} {kind:8} 0x{off:08x}  {name}   ra={ra}")

    if not args.vram:
        print("\n[map] pass --vram <mc_vram.bin> to confirm which entry holds the palette.")
        return

    vram = open(args.vram, "rb").read()
    needles = {
        "VAHN490": vram[490 * 2048 + 16: 490 * 2048 + 16 + 32],
        "NOA492":  vram[492 * 2048 + 16: 492 * 2048 + 16 + 32],
        "GALA494": vram[494 * 2048 + 16: 494 * 2048 + 16 + 32],
    }
    print("\n[map] searching candidate entries' LZS sections for the palettes...")
    for name, *_ in seen:
        if name == "??":
            continue
        hits = try_search_entry(extracted, name, needles)
        if hits:
            for label, di, p in hits:
                print(f"   *** {name} desc{di} @ 0x{p:x} holds {label} ***")


if __name__ == "__main__":
    main()
