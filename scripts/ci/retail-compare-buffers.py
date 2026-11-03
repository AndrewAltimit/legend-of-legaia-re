#!/usr/bin/env python3
"""Score a retail-compare report's engine frames against both framebuffers.

The image channel scores the engine against the buffer the state displays. A
retail state holds a second finished picture: the buffer the GPU drew last.
Every lag consumer stands the engine on the displayed pass where the RAM can
say what that pass was, and on the pass after it where it cannot, and this
report tells those apart without running anything: an engine frame that
matches the undisplayed buffer far better than the displayed one drew retail's
next pass correctly, so what the image channel measures on that state is the
instant it was saved at.

    scripts/ci/retail-compare-buffers.py --report captures/retail-compare
    scripts/ci/retail-compare-buffers.py --report DIR --filter koru_pre

Needs the report's side-by-side PNGs (`retail-compare --images`), numpy and
Pillow. It prints retail-derived scores only and writes no pixels. See
docs/tooling/retail-compare.md#the-other-framebuffer.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import numpy as np
from PIL import Image

REPO = Path(__file__).resolve().parents[2]
PROFILE = "release-test"
FRAME_W, FRAME_H, BLOCK, TOLERANCE = 320, 224, 8, 24.0
# Legaia's two draw areas stand this far apart in VRAM (the records at
# `0x8007BF30`: `y = 4` and `y = 244`).
BUFFER_GAP = 240
# How far the other buffer must beat the displayed one to call the frame a
# pass off rather than noise.
PASS_OFF_MARGIN = 0.05


def main_checkout() -> Path:
    out = subprocess.run(["git", "worktree", "list", "--porcelain"], cwd=REPO,
                         capture_output=True, text=True, check=True).stdout
    return Path(out.splitlines()[0].split(" ", 1)[1])


def tool(name: str) -> Path:
    for root in (REPO, main_checkout()):
        exe = root / "target" / PROFILE / name
        if exe.exists():
            return exe
    sys.exit(f"[error] {name} not built under target/{PROFILE}")


def library() -> Path:
    for root in (REPO, main_checkout()):
        lib = root / "saves" / "library"
        if lib.is_dir():
            return lib
    sys.exit("[error] saves/library not found")


def vram_of(emulator: str, state: Path, tmp: Path) -> tuple[np.ndarray, int, int]:
    """The state's VRAM and its display origin."""
    blob = tmp / "vram.bin"
    if emulator == "pcsx-redux":
        exe = str(tool("pcsxr-state"))
        subprocess.run([exe, "vram", state, "--out", blob], check=True, capture_output=True)
        out = subprocess.run([exe, "vram", state, "--out", tmp / "crop.bin", "--display-crop"],
                             check=True, capture_output=True, text=True).stdout
        x, y = (int(v) for v in out.split()[-4:-2])
    else:
        out = subprocess.run([str(tool("mednafen-state")), "vram-dump", "--out", tmp / "v.png",
                              "--out-bin", blob, "--display-crop", state],
                             check=True, capture_output=True, text=True).stdout
        m = re.search(r"at VRAM \((\d+),(\d+)\)", out)
        if not m:
            sys.exit(f"[error] no display origin in: {out}")
        x, y = int(m.group(1)), int(m.group(2))
    return np.frombuffer(blob.read_bytes(), dtype="<u2").reshape(512, 1024), x, y


def crop(vram: np.ndarray, x: int, y: int) -> np.ndarray:
    a = vram[y:y + FRAME_H, x:x + FRAME_W].astype(np.int32)
    five = lambda c: (c << 3) | (c >> 2)
    return np.stack([five(a & 31), five((a >> 5) & 31), five((a >> 10) & 31)], -1)


def within(a: np.ndarray, b: np.ndarray) -> float:
    """`retail_compare_image::score`'s block test."""
    bh, bw = FRAME_H // BLOCK, FRAME_W // BLOCK
    shape = (bh, BLOCK, bw, BLOCK, 3)
    ma = a.reshape(shape).mean((1, 3))
    mb = b.reshape(shape).mean((1, 3))
    return float((np.abs(ma - mb) <= TOLERANCE).all(-1).mean())


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--report", required=True, type=Path, help="retail-compare --out directory")
    ap.add_argument("--filter", help="only labels containing this (comma = any of several)")
    ap.add_argument("--json", type=Path, help="also write the rows here (keep it out of the repo)")
    a = ap.parse_args()

    report = json.loads((a.report / "report.json").read_text())
    scenarios = {s["label"]: s for s in
                 tomllib.loads((REPO / "scripts/scenarios.toml").read_text())["scenarios"]}
    wanted = a.filter.split(",") if a.filter else None
    lib = library()

    rows = []
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        for st in report["states"]:
            label = st["label"]
            png = a.report / f"{label}.png"
            if (wanted and not any(w in label for w in wanted)) or st.get("resident_patch") \
                    or not st.get("image") or not png.exists() or label not in scenarios:
                continue
            found = list((lib / st["emulator"]).rglob(scenarios[label]["backup_fingerprint"] + "*"))
            if not found:
                continue
            vram, x, y = vram_of(st["emulator"], found[0], tmp)
            side = np.asarray(Image.open(png).convert("RGB")).astype(np.int32)
            engine = side[:FRAME_H, FRAME_W:FRAME_W * 2]
            shown = crop(vram, x, y)
            # A hi-res or moved display has no second buffer at this stride,
            # and neither has a frame the side-by-side does not reproduce.
            if within(side[:FRAME_H, :FRAME_W], shown) < 0.99:
                continue
            other = crop(vram, x, y + BUFFER_GAP if y < BUFFER_GAP else y - BUFFER_GAP)
            rows.append({
                "label": label,
                "displayed": within(shown, engine),
                "other": within(other, engine),
                "between": within(shown, other),
            })
    rows.sort(key=lambda r: r["displayed"])
    print("| displayed | other | buffers apart | |  state |\n|---|---|---|---|---|")
    for r in rows:
        off = "pass off" if r["other"] >= r["displayed"] + PASS_OFF_MARGIN else ""
        print(f"| {r['displayed']:.3f} | {r['other']:.3f} | {r['between']:.3f} | {off} | {r['label']} |")
    if a.json:
        a.json.write_text(json.dumps(rows, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
