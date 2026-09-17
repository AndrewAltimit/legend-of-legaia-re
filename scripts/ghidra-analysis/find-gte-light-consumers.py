#!/usr/bin/env python3
"""Disc-wide census of the GTE instructions that read the **light matrix**.

The light matrix `L` lives in GTE control registers 8..12 (`L11L12`, `L13L21`,
`L22L23`, `L31L32`, `L33`). Nothing reads it back through `cfc2` in a normal
render loop - it is consumed **implicitly**, by the GTE's own normal-colour
commands, which multiply an input normal by `L` before anything else:

    NCS  NCT  NCDS  NCDT  NCCS  NCCT

and by `MVMVA` when its `mx` selector picks matrix 1 (the light matrix)
instead of matrix 0 (rotation) or 2 (colour).

So "who reads the matrix `FUN_8001ADA4` writes" is not an xref question at
all. It is a *static census of cop2 opcodes*, and this is that census: scan
`SCUS_942.54` plus every statically based overlay image for GTE command words,
tally them by function, and list every site of the light-matrix consumers.

    scripts/ghidra-analysis/find-gte-light-consumers.py
    scripts/ghidra-analysis/find-gte-light-consumers.py --all-ops
    scripts/ghidra-analysis/find-gte-light-consumers.py --sites

## Why only based images

A GTE command word is four bytes with no relocation, so a byte scan over
*data* produces false positives at the same rate any four-byte pattern does.
The images swept here are the ones with a recovered load base - SCUS and the
rows of `crates/asset/data/static-overlays.toml` - which is where the engine's
code actually is. Raw data entries are excluded on purpose, and a hit's VA is
the image's own (slot-A overlays alias a base, so read a VA with the image
name beside it).

The denominator is printed with the answer: images scanned, bytes scanned, and
the total GTE-command count, so a zero for one family is a zero measured
against a population that is demonstrably non-empty.
"""

from __future__ import annotations

import argparse
import importlib.util
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from mips_gte import GTE_FUNC  # noqa: E402


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


_refs = _load("find_address_word_refs", HERE / "find-address-word-refs.py")

# GTE function selectors that multiply an input normal by the LIGHT matrix.
LIGHT_CONSUMERS = {
    0x13: "ncds",
    0x16: "ncdt",
    0x1B: "nccs",
    0x1E: "ncs",
    0x20: "nct",
    0x3F: "ncct",
}

# GTE control registers the light matrix occupies.
LIGHT_CTRL_REGS = {8, 9, 10, 11, 12}

MVMVA = 0x12
MX_LIGHT = 1


def is_gte_command(word: int) -> bool:
    """COP2 (`op == 0x12`) with the command flag (bit 25) set."""
    return (word >> 26) == 0x12 and (word >> 25) & 1 == 1


def mvmva_mx(word: int) -> int:
    """`MVMVA`'s matrix selector: 0 rotation, 1 light, 2 colour, 3 reserved."""
    return (word >> 17) & 3


# Top-six-bit opcodes a real MIPS R3000 code window is built out of. Used only
# to separate a GTE-shaped **data** word from a GTE instruction: a four-byte
# pattern occurs in BGR555 pixels and in Shift-JIS prose at the rate any
# four-byte pattern does, and two of this sweep's raw hits are exactly that
# ("ATK " in a menu string; high-entropy bytes in PROT 0899's data segment).
CODE_OPS = {
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x14, 0x15, 0x16, 0x17,
    0x20, 0x21, 0x23, 0x24, 0x25, 0x28, 0x29, 0x2B,
    0x32, 0x3A,
}


def code_score(data: bytes, off: int, radius: int = 6) -> float:
    """Share of the +-`radius` word window whose top six bits are a MIPS
    opcode. Weak on its own - random bytes clear 0.75 often enough that two of
    this sweep's data hits score 1.00 - so it is reported, never decisive.
    """
    lo = max(0, off - radius * 4)
    hi = min(len(data) - 3, off + (radius + 1) * 4)
    words = [struct.unpack_from("<I", data, a)[0] for a in range(lo, hi, 4)]
    if not words:
        return 0.0
    return sum(1 for w in words if (w >> 26) in CODE_OPS) / len(words)


# The COP2 instruction forms: the coprocessor-2 primary opcode plus its
# load/store word forms.
COP2_OPS = {0x12, 0x32, 0x3A}


def cop2_neighbours(data: bytes, off: int, radius: int = 8) -> int:
    """How many **distinct** other COP2 encodings sit within +-`radius` words.

    This is the discriminator, and it is structural rather than statistical.
    The GTE takes no memory operands, so a real GTE command is always packed
    among the register moves that feed and drain it (`lwc2` / `mtc2` in,
    `mfc2` / `swc2` out) - retail's own light handlers sit in runs of them. A
    GTE-shaped word inside pixel data or prose has no such company: both of
    this sweep's surviving data hits have **zero** COP2 neighbours while every
    real one has several.

    Distinct rather than raw, because a **run of one repeated word** is the
    other data shape that fakes company: PROT 0895's data segment carries five
    identical `cfc2`-shaped words in a row, and each would otherwise vouch for
    the next. Real GTE code varies - a command is fed by different moves.
    """
    lo = max(0, off - radius * 4)
    hi = min(len(data) - 3, off + (radius + 1) * 4)
    own = struct.unpack_from("<I", data, off)[0]
    seen = set()
    for a in range(lo, hi, 4):
        if a == off:
            continue
        w = struct.unpack_from("<I", data, a)[0]
        if w != own and (w >> 26) in COP2_OPS:
            seen.add(w)
    return len(seen)


def cop2_move(word: int) -> tuple[str, int] | None:
    """`(form, cop2 register)` for a non-command COP2 data move, else `None`."""
    if (word >> 26) != 0x12 or (word >> 25) & 1:
        return None
    rs = (word >> 21) & 0x1F
    form = {0: "mfc2", 2: "cfc2", 4: "mtc2", 6: "ctc2"}.get(rs)
    if form is None:
        return None
    return form, (word >> 11) & 0x1F


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--sites", action="store_true", help="print every light-consumer site")
    ap.add_argument("--all-ops", action="store_true", help="print the whole GTE opcode histogram")
    ap.add_argument(
        "--code-floor",
        type=float,
        default=0.75,
        help="minimum code_score for a hit to count (default 0.75; 0 = raw)",
    )
    ap.add_argument(
        "--min-cop2-neighbours",
        type=int,
        default=2,
        help="minimum other COP2 words within +-8 words (default 2; 0 = raw)",
    )
    ap.add_argument(
        "--ctrl-moves",
        action="store_true",
        help="also list every ctc2/cfc2 touching control registers 8..12",
    )
    args = ap.parse_args()

    images = []
    scus = _refs.load_scus()
    if scus is not None:
        images.append(scus)
    images.extend(_refs.load_overlays())
    if not images:
        print("[skip] no extracted/ images - run legaia-extract first", file=sys.stderr)
        return 0

    histogram: dict[int, int] = {}
    sites: list[tuple[str, int, int, str, float, int]] = []
    mvmva_by_mx = {0: 0, 1: 0, 2: 0, 3: 0}
    mvmva_light_sites: list[tuple[str, int, int]] = []
    ctrl_moves: list[tuple[str, int, int, str, int]] = []
    total_cmds = 0
    total_bytes = 0
    skipped_data = 0

    for img in images:
        total_bytes += len(img.data)
        for off in range(0, len(img.data) - 3, 4):
            word = struct.unpack_from("<I", img.data, off)[0]
            move = cop2_move(word)
            if move is not None:
                form, reg = move
                if (
                    reg in LIGHT_CTRL_REGS
                    and form in ("ctc2", "cfc2")
                    and cop2_neighbours(img.data, off) >= args.min_cop2_neighbours
                    and code_score(img.data, off) >= args.code_floor
                ):
                    ctrl_moves.append((img.name, off, word, form, reg))
                continue
            if not is_gte_command(word):
                continue
            fn = word & 0x3F
            if fn not in GTE_FUNC:
                continue  # not a GTE function selector at all - data
            score = code_score(img.data, off)
            neighbours = cop2_neighbours(img.data, off)
            if neighbours < args.min_cop2_neighbours or score < args.code_floor:
                skipped_data += 1
                continue
            total_cmds += 1
            histogram[fn] = histogram.get(fn, 0) + 1
            if fn in LIGHT_CONSUMERS:
                va = img.va(off) or 0
                sites.append((img.name, off, va, LIGHT_CONSUMERS[fn], score, neighbours))
            elif fn == MVMVA:
                mx = mvmva_mx(word)
                mvmva_by_mx[mx] += 1
                if mx == MX_LIGHT:
                    mvmva_light_sites.append((img.name, off, img.va(off) or 0))

    print(
        f"# {len(images)} based image(s), {total_bytes} byte(s), "
        f"{total_cmds} GTE command word(s) in code "
        f"({skipped_data} GTE-shaped word(s) dropped as data: "
        f"cop2_neighbours < {args.min_cop2_neighbours} or code_score < {args.code_floor})"
    )

    if args.all_ops:
        print("\n# GTE command histogram (function selector -> count)")
        for fn, n in sorted(histogram.items(), key=lambda kv: -kv[1]):
            print(f"  0x{fn:02X} {GTE_FUNC.get(fn, '?'):<6} {n}")

    print("\n# normal-colour commands (the implicit light-matrix consumers)")
    for fn, name in sorted(LIGHT_CONSUMERS.items()):
        print(f"  0x{fn:02X} {name:<6} {histogram.get(fn, 0)}")

    print("\n# MVMVA by matrix selector (mx=1 is the light matrix)")
    for mx, label in ((0, "rotation"), (1, "LIGHT"), (2, "colour"), (3, "reserved")):
        print(f"  mx={mx} {label:<9} {mvmva_by_mx[mx]}")

    if args.sites:
        print("\n# light-consumer sites")
        for name, off, va, op, score, neighbours in sites:
            print(
                f"  {op:<6} {name} +0x{off:x} (VA 0x{va:08x})  "
                f"code_score={score:.2f} cop2_neighbours={neighbours}"
            )
        for name, off, va in mvmva_light_sites:
            print(f"  {'mvmva':<6} {name} +0x{off:x} (VA 0x{va:08x})  mx=LIGHT")

    if args.ctrl_moves:
        print("\n# ctc2 / cfc2 on control registers 8..12")
        for name, off, _word, form, reg in ctrl_moves:
            va = 0
            for img in images:
                if img.name == name:
                    va = img.va(off) or 0
                    break
            print(f"  {form} cop2c{reg:<2} {name} +0x{off:x} (VA 0x{va:08x})")

    consumers = sum(histogram.get(fn, 0) for fn in LIGHT_CONSUMERS) + mvmva_by_mx[MX_LIGHT]
    print(f"\n# light-matrix consumers disc-wide: {consumers}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
