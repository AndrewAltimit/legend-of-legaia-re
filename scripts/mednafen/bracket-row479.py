#!/usr/bin/env python3
"""Bracket the row-479 hue-ramp writer.

Diff a "before" save state (no canonical ramp) against an "after" save state
(canonical ramp installed) and report:

  1. whether VRAM `(fb_x=0..240, fb_y=479)` transitioned ZERO -> POPULATED
  2. whether the canonical ramp bytes appeared anywhere in RAM/VRAM
  3. which actor-pool slots became newly populated (new actors spawned)
  4. which overlay-window regions got newly populated (new overlay loaded)
  5. which scene-name / scene-PROT-base globals changed
  6. asset-descriptor walker result delta (`0x80087AF8`)

The aim is to surface the single new RAM/VRAM upload that *caused* the
canonical ramp to be installed, so the Ghidra writer hunt can be
narrowed from "anywhere in 442 KB of SCUS + overlays" to "the loader
for actor X" or "the entry point of overlay Y".

Run from the repo root:

    python3 scripts/mednafen/bracket-row479.py \\
        --before ~/.mednafen/mcs/<game>.mc1 \\
        --after  ~/.mednafen/mcs/<game>.mc<N>

The `mednafen-state` binary must already be built (`cargo build --release
-p legaia-mednafen`).
"""
import argparse
import hashlib
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MED = REPO_ROOT / "target" / "release" / "mednafen-state"

# Canonical hue-ramp bytes (slot 0 + needles for each slot).
def load_canonical_slots():
    src = (REPO_ROOT / "crates" / "asset" / "src" / "npc_palette.rs").read_text()
    m = re.search(r"GLOBAL_HUE_RAMP_ROW_479.*?=\s*\[(.*?)\];", src, re.S)
    if not m:
        sys.exit("could not parse GLOBAL_HUE_RAMP_ROW_479")
    slots = []
    for chunk in re.findall(r"\[(.*?)\]", m.group(1), re.S):
        bs = bytes(int(x, 16) for x in re.findall(r"0x[0-9a-fA-F]+", chunk))
        if len(bs) == 32:
            slots.append(bs)
    return slots


REGIONS = {
    "staging_buffer":           (0x800F1800, 0x800F1B00),
    "actor_anim_pool":          (0x801C9594, 0x801C9F80),
    "town_overlay_scratch":     (0x801CE808, 0x801D3018),
    "overlay_window_full":      (0x801C0000, 0x80200000),
    "scratch_buffer_b":         (0x80108EA4, 0x80109550),
    "scene_local_pool":         (0x800F1000, 0x800F2000),
    "scene_name_block":         (0x80084540, 0x800845C0),
    "battle_actor_pool":        (0x800EC9E8, 0x800ED560),
    "story_flag_bitmap":        (0x80085600, 0x80085800),
    "asset_descriptor_result":  (0x80087AF0, 0x80087B00),
}


def run(*args, **kw):
    return subprocess.run(args, check=True, capture_output=True, **kw)


def extract(save_path, start, end, out_path):
    run(str(MED), "extract", str(save_path),
        "--start", f"0x{start:08X}", "--end", f"0x{end:08X}",
        "--out", str(out_path))


def vram_dump(save_path, out_bin):
    run(str(MED), "vram-dump", str(save_path),
        "--out", str(out_bin.with_suffix(".png")),
        "--out-bin", str(out_bin))


def hash16(b):
    return hashlib.sha256(b).hexdigest()[:16]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--before", required=True, type=Path)
    ap.add_argument("--after", required=True, type=Path)
    ap.add_argument("--workdir", default="/tmp/row479_bracket", type=Path)
    args = ap.parse_args()

    work = args.workdir
    (work / "before").mkdir(parents=True, exist_ok=True)
    (work / "after").mkdir(parents=True, exist_ok=True)

    slots = load_canonical_slots()
    print(f"loaded {len(slots)} canonical slots ({sum(len(s) for s in slots)} bytes)")

    # 1) Hash + scan VRAM row 479 fb_x=0..240 in both states.
    for tag, save in [("before", args.before), ("after", args.after)]:
        vbin = work / tag / "vram.bin"
        vram_dump(save, vbin)
        v = vbin.read_bytes()
        row_off = 479 * 2048  # 0xEF800
        slab = v[row_off:row_off+480]   # fb_x=0..240
        zh = hash16(slab)
        nz = sum(1 for b in slab if b != 0)
        print(f"  {tag} VRAM r479 fb_x=0..240: hash={zh}  nonzero={nz}/480")
        for i, s in enumerate(slots):
            needle = s[:8]
            p = v.find(needle)
            if p != -1:
                row = p // 2048; px = (p % 2048) // 2
                full = v[p:p+32] == s
                print(f"    {tag} VRAM slot{i} found at row={row} fb_x={px}  full_match={full}")

    # 2) RAM region diffs.
    print("\n[regions]")
    transitioned = []
    for name, (start, end) in REGIONS.items():
        before_bin = work / "before" / f"{name}.bin"
        after_bin  = work / "after" / f"{name}.bin"
        extract(args.before, start, end, before_bin)
        extract(args.after,  start, end, after_bin)
        before = before_bin.read_bytes()
        after  = after_bin.read_bytes()
        if before == after:
            print(f"  {name:30s} unchanged  ({len(before)}B)")
            continue
        # Count newly-nonzero bytes
        new_nz = sum(1 for a, b in zip(before, after) if a == 0 and b != 0)
        changed = sum(1 for a, b in zip(before, after) if a != b)
        print(f"  {name:30s} {hash16(before)} -> {hash16(after)}  changed={changed}  new_nz={new_nz}")
        transitioned.append((name, before, after))
        # If small enough, scan for canonical slot bytes.
        for i, s in enumerate(slots):
            n = s[:8]
            if before.find(n) == -1 and after.find(n) != -1:
                p = after.find(n)
                addr = start + p
                full = after[p:p+32] == s
                print(f"    + canonical slot{i} APPEARED at 0x{addr:08X}  full_match_32B={full}")

    # 3) Newly-spawned actor slots (heuristic: scan town-overlay-scratch for record-start markers)
    print("\n[actor_anim_pool slot diff]")
    if (work / "before" / "actor_anim_pool.bin").exists():
        a = (work / "before" / "actor_anim_pool.bin").read_bytes()
        b = (work / "after"  / "actor_anim_pool.bin").read_bytes()
        stride = 0x60
        n_slots = len(a) // stride
        for i in range(n_slots):
            asl = a[i*stride:(i+1)*stride]
            bsl = b[i*stride:(i+1)*stride]
            if asl != bsl:
                # tag start address
                slot_addr = 0x801C9594 + i*stride
                a_dispatch = asl[0x0F] if len(asl) > 0x10 else 0
                b_dispatch = bsl[0x0F] if len(bsl) > 0x10 else 0
                a_ptr = int.from_bytes(asl[:4], "little") if len(asl) >= 4 else 0
                b_ptr = int.from_bytes(bsl[:4], "little") if len(bsl) >= 4 else 0
                print(f"  slot{i:2d} @ 0x{slot_addr:08X}: dispatch {a_dispatch:#04x}->{b_dispatch:#04x}  ptr 0x{a_ptr:08X}->0x{b_ptr:08X}")

    print("\n[done]")


if __name__ == "__main__":
    main()
