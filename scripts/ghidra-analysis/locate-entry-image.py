#!/usr/bin/env python3
"""Locate which statically-based overlay image actually holds a function entry.

A worklist address is a *printed* VA, and several overlays share the slot-A
base `0x801CE818` (field / battle / menu / the minigame siblings) or the slot-B
base `0x801F69D8`. The same VA therefore decodes to different, equally
plausible code in each image, which is how a port lands on the wrong body -
see `docs/tooling/dump-corpus-integrity.md` and the VA-aliasing row in
`docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims`.

This tool answers "which image is the entry in?" from the disc bytes rather
than from a dump's filename prefix, using two independent signals:

  * **frame** - an `addiu sp, sp, -N` within the first few instructions.
  * **jal**   - how many `jal <va>` sites elsewhere in that same image target
                the address. A decoded `jal` target is a property of the
                bytes, not of the load base.

Neither signal alone is sufficient, and the tool prints both rather than
collapsing them into a verdict:

  * A **leaf** function legitimately has no stack frame. `0x801F6D48` in PROT
    0900 is a real entry that opens `lui t6, 0x1f80` and never touches `sp`,
    so a frame-only scan reports it as "not an entry".
  * A function reached only through a **jump table** or a runtime function
    pointer has zero `jal` sites. `FUN_801EC3E4` is called from `SCUS_942.54`,
    not from within its own overlay, so an in-image `jal` count of 0 is not
    evidence of absence either.

Read a `frame` hit in exactly one image as "this is the image to port from".
Read several as genuine ambiguity that needs a content check. Read none as
"scan the body before concluding anything".

Usage:
    scripts/ghidra-analysis/locate-entry-image.py 801e1d98 801f6d48
    scripts/ghidra-analysis/locate-entry-image.py --file addrs.txt

Requires the disc to have been extracted to `extracted/PROT/` and reads the
committed base map at `crates/asset/data/static-overlays.toml`.
"""

from __future__ import annotations

import argparse
import glob
import re
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OVERLAY_MAP = REPO / "crates" / "asset" / "data" / "static-overlays.toml"
PROT_DIR = REPO / "extracted" / "PROT"

# How many instructions into a body a stack-frame prologue may sit. Ghidra
# routinely places one or two setup instructions (a `lui`/`lw` of a global the
# body needs) ahead of the `addiu sp`.
PROLOGUE_WINDOW = 4


def load_images() -> list[tuple[int, str, int, bytes]]:
    """Every overlay in the committed map that has an extracted image."""
    if not OVERLAY_MAP.exists():
        sys.exit(f"missing overlay map: {OVERLAY_MAP}")
    text = OVERLAY_MAP.read_text()
    rows = re.findall(
        r'prot_index = (\d+)\s*\nlabel = "([^"]+)"\s*\nbase_va = (0x[0-9A-Fa-f]+)',
        text,
    )
    images = []
    for prot, label, base in rows:
        hits = sorted(glob.glob(str(PROT_DIR / f"{int(prot):04d}_*.BIN")))
        if hits:
            images.append((int(prot), label, int(base, 16), Path(hits[0]).read_bytes()))
    if not images:
        sys.exit(f"no extracted overlay images under {PROT_DIR}")
    return images


def is_frame_prologue(word: int) -> bool:
    """`addiu sp, sp, -N` - opcode/rs/rt fixed, immediate negative."""
    return (word >> 16) == 0x27BD and (word & 0xFFFF) >= 0x8000


def jal_sites(image: bytes, base: int, target: int) -> int:
    """Count `jal target` instructions anywhere in this image.

    `jal` encodes bits 27..2 of an address whose top 4 bits come from the
    delay-slot PC, so the comparison is against the target's low 28 bits.
    """
    want = 0x0C000000 | ((target >> 2) & 0x03FFFFFF)
    count = 0
    for off in range(0, len(image) - 3, 4):
        if struct.unpack_from("<I", image, off)[0] == want:
            count += 1
    return count


def probe(va: int, images) -> list[tuple[int, str, int | None, int]]:
    """Per-image (prot, label, frame_offset_or_None, jal_count) for `va`."""
    out = []
    for prot, label, base, data in images:
        off = va - base
        if off < 0 or off + 4 > len(data):
            continue
        frame = None
        for k in range(PROLOGUE_WINDOW):
            at = off + 4 * k
            if at + 4 > len(data):
                break
            word = struct.unpack_from("<I", data, at)[0]
            # Stop at a `jr ra`: any `addiu sp` past it belongs to the *next*
            # function, so accepting it would report an epilogue as an entry.
            # `0x801D2D2C` in PROT 0897 is that case - it sits inside a
            # register-restore epilogue whose successor opens with a frame.
            if word == 0x03E00008:
                break
            if is_frame_prologue(word):
                frame = k
                break
        out.append((prot, label, frame, jal_sites(data, base, va)))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("addrs", nargs="*", help="VAs, e.g. 801e1d98")
    ap.add_argument("--file", help="file of VAs, one per line (# comments ok)")
    args = ap.parse_args()

    addrs = list(args.addrs)
    if args.file:
        for line in Path(args.file).read_text().splitlines():
            line = line.split("#", 1)[0].strip()
            if line:
                addrs.append(line)
    if not addrs:
        ap.error("no addresses given")

    images = load_images()
    print(f"# {len(images)} based overlay images from {OVERLAY_MAP.name}")
    print("# frame = addiu sp within the first 4 insns; jal = in-image call sites")
    for a in addrs:
        va = int(a, 16)
        hits = probe(va, images)
        framed = [h for h in hits if h[2] is not None]
        called = [h for h in hits if h[3]]
        if len(framed) == 1:
            verdict = f"entry in {framed[0][0]}/{framed[0][1]}"
        elif framed:
            verdict = "AMBIGUOUS - framed in " + ",".join(f"{p}" for p, _, _, _ in framed)
            verdict += "; disambiguate by content"
        elif called:
            verdict = "no frame anywhere - leaf or non-entry; jal sites exist"
        else:
            verdict = "no frame and no in-image jal - read the body"
        print(f"\n{va:08x}  {verdict}")
        for prot, label, frame, jal in hits:
            if frame is None and jal == 0:
                continue
            fr = f"frame+{frame}" if frame is not None else "-       "
            print(f"    {prot:>4} {label:<22s} {fr}  jal_sites={jal}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
