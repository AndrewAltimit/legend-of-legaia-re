#!/usr/bin/env python3
"""Find every reference to a code address that is *not* a `jal`.

A routine with "no caller" is often a routine nobody scanned for correctly.
This engine reaches a great deal of its code through **function-pointer
tables and actor templates** rather than through direct calls, and the two
non-`jal` reference forms are invisible to the tools that look for calls:

  * a **literal address word** sitting in a table or a struct template
    (`FUN_8004DA00` is the `+0x0C` tick slot of the template word at
    `0x800767FC` - found exactly this way);
  * a **LUI+ADDIU pair** materialising the address into a register, which
    Ghidra's reference manager does not auto-resolve, so a direct xref query
    returns zero hits even for a heavily used address (see
    `docs/tooling/ghidra.md`).

A third form settles the opposite question. An address with **no** caller of
any kind may not be an entry point at all - it may be an intra-function
label, reached by a PC-relative **branch** that carries no copy of its
target. That is the shape behind Ghidra's fake `FUN_xxxxxxxx` "label-calls".

That third form comes with a trap the other two do not have. A branch is
PC-relative, so it can only reach code in its **own** image - which means a
`BR` hit is evidence about the branching image's copy of the VA and nothing
else. Slot-A overlays share a load base, so a routine's VA is live in every
sibling image, and a branch in a *different* overlay reads as a reference to
a routine it cannot possibly reach. Pass `--home <image>` to mark those.

This tool sweeps `SCUS_942.54`, every statically extracted overlay image, and
(optionally) every extracted `PROT.DAT` entry for every form - literal word,
materialisation pair, `jal`, `j`, and PC-relative branch - and classifies
each literal-word hit by what the bytes around it look like.

    scripts/ghidra-analysis/find-address-word-refs.py 8005126c
    scripts/ghidra-analysis/find-address-word-refs.py 8004da00 --expect-scus 800767fc
    scripts/ghidra-analysis/find-address-word-refs.py 80025054 --prot
    scripts/ghidra-analysis/find-address-word-refs.py --file addrs.txt --words-only

## What a hit's VA means

Reporting a hit as a VA requires knowing where the containing image loads.
Three cases, kept apart because conflating them is how this repo has
previously claimed a VA the bytes could not support (see
`docs/tooling/call-target-integrity.md` and
`docs/tooling/phantom-print-index.md`):

  * **SCUS** - one fixed base from the PS-X EXE header, so a file offset maps
    to exactly one VA.
  * **Overlay images** - the committed base map
    (`crates/asset/data/static-overlays.toml`) gives each image its own base.
    Many overlays share a base, so the *image* is as much of the answer as
    the offset is: the same VA is a different word in each slot-A sibling.
  * **Other PROT entries** (`--prot`) - streamed data with no load base at
    all. Those hits are reported by **file offset only**; the tool never
    invents a VA for them.

## Classification

Only word-aligned hits can be table entries, so unaligned ones are reported
as incidental. For the aligned ones the verdict comes from three independent
counts over the surrounding bytes, all printed so the verdict can be
second-guessed:

  * `code` - `jr ra` and `addiu sp, sp, +-N` words within +-0x200. Real code
    carries several; a pointer table carries none.
  * `entry` - neighbouring words (+-4) that are addresses landing on a
    function prologue **in this same image**. Two or more is a dispatch
    table.
  * `ptr` / `const` - neighbouring words that are RAM-band addresses, or
    small constants. A pointer among constants is a struct/template field.

The verdict is a summary of those counts, not a substitute for reading the
disassembly at the hit (`docs/tooling/ghidra.md`).
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
OVERLAY_DIR = REPO / "extracted" / "overlays"
PROT_DIR = REPO / "extracted" / "PROT"
SCUS = REPO / "extracted" / "SCUS_942.54"

# PSX kernel-segment RAM band a plausible pointer lands in (2 MB, KSEG0).
RAM_LO = 0x80010000
RAM_HI = 0x80200000

# How far either side of a hit to tally code markers.
CODE_WINDOW = 0x200
# How many words either side of a hit form its "neighbourhood".
NEIGHBOURS = 4
# How many instructions after a `lui` a materialisation partner may sit.
LUI_PAIR_WINDOW = 8
# Opcodes whose immediate is a PC-relative branch displacement.
BRANCH_OPS = {0x01, 0x04, 0x05, 0x06, 0x07, 0x14, 0x15, 0x16, 0x17}


class Image:
    """One scanned blob: bytes, an identity, and possibly a load base."""

    def __init__(self, name: str, data: bytes, base: int | None, kind: str):
        self.name = name
        self.data = data
        self.base = base
        self.kind = kind
        self._branches: dict[int, list[int]] | None = None

    def va(self, off: int) -> int | None:
        return None if self.base is None else self.base + off

    def off_of(self, va: int) -> int | None:
        if self.base is None:
            return None
        off = va - self.base
        return off if 0 <= off + 4 <= len(self.data) else None

    def word(self, off: int) -> int | None:
        if off < 0 or off + 4 > len(self.data):
            return None
        return struct.unpack_from("<I", self.data, off)[0]

    def label(self, off: int) -> str:
        va = self.va(off)
        return f"{self.name} +0x{off:x}" + (f" (VA 0x{va:08x})" if va else "")

    def branch_index(self) -> dict[int, list[int]]:
        """`branch target VA -> offsets of the branches that reach it`.

        A branch is PC-relative, so it carries no copy of its target address
        and no absolute-word scan can see it. That is exactly how an
        intra-function label is reached - the shape behind Ghidra's fake
        `FUN_xxxxxxxx` "label-calls" (`docs/tooling/ghidra.md`). An address
        with branch sites but no call sites is a label inside somebody else's
        function, not an entry point nobody calls.

        Needs a load base, so it is only built for SCUS + the based overlays.
        """
        if self._branches is None:
            index: dict[int, list[int]] = {}
            if self.base is not None:
                for at in range(0, len(self.data) - 3, 4):
                    word = struct.unpack_from("<I", self.data, at)[0]
                    op = word >> 26
                    if op not in BRANCH_OPS:
                        continue
                    if op == 0x01 and ((word >> 16) & 0x1F) not in (0, 1, 16, 17):
                        continue
                    imm = word & 0xFFFF
                    delta = (imm - 0x10000 if imm >= 0x8000 else imm) << 2
                    index.setdefault(self.base + at + 4 + delta, []).append(at)
            self._branches = index
        return self._branches


def load_scus() -> Image | None:
    """`SCUS_942.54` at its PS-X EXE load base.

    The header's `t_addr` is the VA of the byte at file offset 0x800, so the
    image is presented with the 0x800-byte header trimmed and a base that
    makes offset arithmetic direct.
    """
    if not SCUS.exists():
        return None
    raw = SCUS.read_bytes()
    if raw[:8] != b"PS-X EXE":
        sys.exit(f"{SCUS} is not a PS-X EXE")
    t_addr = struct.unpack_from("<I", raw, 0x18)[0]
    t_size = struct.unpack_from("<I", raw, 0x1C)[0]
    return Image("SCUS_942.54", raw[0x800 : 0x800 + t_size], t_addr, "exe")


def load_overlays() -> list[Image]:
    """Every extracted overlay image that the committed map gives a base."""
    if not OVERLAY_MAP.exists():
        return []
    rows = re.findall(
        r'prot_index = (\d+)\s*\nlabel = "([^"]+)"\s*\nbase_va = (0x[0-9A-Fa-f]+)',
        OVERLAY_MAP.read_text(),
    )
    out = []
    for prot, label, base in rows:
        path = OVERLAY_DIR / f"overlay_{label}_{int(prot):04d}.bin"
        if path.exists():
            out.append(
                Image(f"{int(prot):04d}/{label}", path.read_bytes(), int(base, 16), "overlay")
            )
    return out


def load_prot_entries(skip: set[str]) -> list[Image]:
    """Every other extracted PROT entry, base-less.

    These are streamed data, not overlay code with a recovered link base, so
    they are scanned for the *bytes* only - a hit is reported at its file
    offset and no VA is claimed for it.
    """
    out = []
    for path in sorted(glob.glob(str(PROT_DIR / "*.BIN"))):
        stem = Path(path).name
        if stem[:4] in skip:
            continue
        out.append(Image(stem, Path(path).read_bytes(), None, "prot"))
    return out


def is_frame_prologue(word: int | None) -> bool:
    """`addiu sp, sp, -N`."""
    return word is not None and (word >> 16) == 0x27BD and (word & 0xFFFF) >= 0x8000


def is_code_marker(word: int) -> bool:
    """`jr ra`, or any `addiu sp, sp, imm` - the frame open/close idiom."""
    return word == 0x03E00008 or (word >> 16) == 0x27BD


def code_markers(image: Image, off: int) -> int:
    lo = max(0, (off - CODE_WINDOW) & ~3)
    hi = min(len(image.data) - 3, off + CODE_WINDOW)
    n = 0
    for at in range(lo, hi, 4):
        word = image.word(at)
        if word is not None and is_code_marker(word):
            n += 1
    return n


def classify(image: Image, off: int, target: int) -> tuple[str, dict]:
    """Verdict + the counts it was drawn from."""
    if off % 4:
        return "incidental-unaligned", {}

    entry = ptr = const = 0
    for k in range(-NEIGHBOURS, NEIGHBOURS + 1):
        if k == 0:
            continue
        word = image.word(off + 4 * k)
        if word is None:
            continue
        if RAM_LO <= word < RAM_HI:
            ptr += 1
            at = image.off_of(word)
            if at is not None and is_frame_prologue(image.word(at)):
                entry += 1
        elif word < 0x10000:
            const += 1

    marks = code_markers(image, off)
    counts = {"code": marks, "entry": entry, "ptr": ptr, "const": const}

    if entry >= 2:
        verdict = "dispatch-table"
    elif marks >= 3:
        verdict = "incidental-code"
    elif ptr >= 2:
        verdict = "pointer-table"
    elif const >= 4:
        verdict = "template-field"
    else:
        verdict = "unclassified"
    return verdict, counts


def word_hits(image: Image, target: int) -> list[int]:
    """Every byte offset where the literal little-endian word appears."""
    needle = struct.pack("<I", target)
    out, at = [], image.data.find(needle)
    while at != -1:
        out.append(at)
        at = image.data.find(needle, at + 1)
    return out


def jump_hits(image: Image, target: int, opcode: int) -> list[int]:
    """`jal`/`j target` sites (word-aligned; encodes the low 28 bits)."""
    needle = struct.pack("<I", (opcode << 26) | ((target >> 2) & 0x03FFFFFF))
    out, at = [], image.data.find(needle)
    while at != -1:
        if at % 4 == 0:
            out.append(at)
        at = image.data.find(needle, at + 1)
    return out


def lui_pair_hits(image: Image, target: int) -> list[tuple[int, int, str]]:
    """LUI+ADDIU / LUI+ORI pairs that materialise `target`.

    Returns `(lui_offset, partner_offset, form)`. The `addiu` form carries the
    sign correction the assembler applies when the low half is negative.

    Candidate `lui`s are found by byte pattern rather than by decoding every
    word, so the sweep stays usable over the whole extracted PROT corpus: a
    `lui rt, imm` is `imm_lo imm_hi rt 0x3c` little-endian with `rt < 32`.
    """
    lo = target & 0xFFFF
    hi_addiu = ((target >> 16) + (1 if lo >= 0x8000 else 0)) & 0xFFFF
    hi_ori = (target >> 16) & 0xFFFF

    out = []
    for imm in {hi_addiu, hi_ori}:
        pattern = re.compile(
            bytes([imm & 0xFF, imm >> 8]) + rb"[\x00-\x1f]\x3c", re.DOTALL
        )
        for m in pattern.finditer(image.data):
            at = m.start()
            if at % 4:
                continue
            reg = image.data[at + 2]
            for k in range(1, LUI_PAIR_WINDOW + 1):
                nxt = image.word(at + 4 * k)
                if nxt is None:
                    break
                op = nxt >> 26
                rs, rt, low = (nxt >> 21) & 0x1F, (nxt >> 16) & 0x1F, nxt & 0xFFFF
                if op == 0x0F and rt == reg:
                    break  # the register is reloaded; the pair cannot span this
                if rs != reg or low != lo:
                    continue
                if op == 0x09 and imm == hi_addiu:
                    out.append((at, at + 4 * k, "addiu"))
                    break
                if op == 0x0D and imm == hi_ori:
                    out.append((at, at + 4 * k, "ori"))
                    break
    return sorted(out)


def report_target(target: int, images: list[Image], args) -> dict:
    print(f"\n=== 0x{target:08x} " + "=" * 46)
    found = {"word": 0, "jal": 0, "j": 0, "branch": 0, "branch_alias": 0, "lui": 0}

    for image in images:
        rows = []
        for off in word_hits(image, target):
            verdict, counts = classify(image, off, target)
            if args.tables_only and verdict.startswith("incidental"):
                continue
            rows.append((off, verdict, counts))
        for off, verdict, counts in rows:
            found["word"] += 1
            tally = " ".join(f"{k}={v}" for k, v in counts.items())
            print(f"  WORD  {image.label(off):<44s} {verdict:<22s} {tally}")
            if args.context:
                dump_context(image, off)

        if args.words_only:
            continue
        for off in jump_hits(image, target, 0x03):
            found["jal"] += 1
            print(f"  JAL   {image.label(off)}")
        for off in jump_hits(image, target, 0x02):
            found["j"] += 1
            print(f"  J     {image.label(off)}")
        if not args.no_branches:
            # Only overlays alias: SCUS has a unique base, so a hit in it is
            # never an artifact of two images sharing an address.
            alias = (
                args.home is not None
                and image.kind == "overlay"
                and args.home.lower() not in image.name.lower()
            )
            for off in image.branch_index().get(target, []):
                if alias:
                    # A branch is PC-relative, so it cannot leave its own
                    # image. This one reaches THIS image's copy of the VA,
                    # which under a shared load base is a different routine.
                    found["branch_alias"] += 1
                    print(f"  br~   {image.label(off)}  (other image - not your routine)")
                else:
                    found["branch"] += 1
                    print(f"  BR    {image.label(off)}")
        for lui_off, partner, form in lui_pair_hits(image, target):
            found["lui"] += 1
            span = (partner - lui_off) // 4
            print(f"  LUI   {image.label(lui_off)}  +{span} insn -> {form}")

    if not any(found.values()):
        print("  (no word, no jump, no branch, no materialisation pair - in any image)")
    else:
        print("  totals: " + " ".join(f"{k}={v}" for k, v in found.items()))
        if found["branch_alias"] and not (
            found["word"] or found["jal"] or found["j"] or found["branch"] or found["lui"]
        ):
            print("  -> every hit is a branch from another image: nothing reaches this routine")
    return found


def dump_context(image: Image, off: int) -> None:
    """Neighbouring words around a hit, for reading the shape by eye."""
    for k in range(-NEIGHBOURS, NEIGHBOURS + 1):
        at = off + 4 * k
        word = image.word(at)
        if word is None:
            continue
        va = image.va(at)
        where = f"0x{va:08x}" if va else f"+0x{at:x}"
        print(f"        {'>>' if k == 0 else '  '} {where}  {word:08x}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("addrs", nargs="*", help="target VAs, e.g. 8005126c")
    ap.add_argument("--file", help="file of VAs, one per line (# comments ok)")
    ap.add_argument(
        "--range",
        metavar="LO:HI",
        help="expand to every word-aligned VA in [LO, HI) - 'who references this table?'",
    )
    ap.add_argument("--prot", action="store_true", help="also sweep every PROT entry")
    ap.add_argument("--words-only", action="store_true", help="skip jal + LUI scans")
    ap.add_argument(
        "--no-branches",
        action="store_true",
        help="skip the PC-relative branch index (slow to build over --prot)",
    )
    ap.add_argument("--tables-only", action="store_true", help="drop incidental word hits")
    ap.add_argument(
        "--home",
        metavar="IMAGE",
        help="substring of the image that HOLDS the target routine; branch hits "
        "from any other image are marked `br~` and tallied separately, because "
        "a PC-relative branch cannot leave its own image",
    )
    ap.add_argument("--context", action="store_true", help="print neighbouring words")
    ap.add_argument(
        "--expect-scus",
        metavar="VA",
        help="self-check: assert a word hit lands at this SCUS VA (control run)",
    )
    args = ap.parse_args()

    addrs = list(args.addrs)
    if args.file:
        for line in Path(args.file).read_text().splitlines():
            line = line.split("#", 1)[0].strip()
            if line:
                addrs.append(line)
    if args.range:
        lo, hi = (int(x, 16) for x in args.range.split(":"))
        addrs += [f"{va:x}" for va in range(lo & ~3, hi, 4)]
    if not addrs:
        ap.error("no addresses given")

    images: list[Image] = []
    scus = load_scus()
    if scus:
        images.append(scus)
    overlays = load_overlays()
    images.extend(overlays)
    if args.prot:
        images.extend(load_prot_entries({im.name[:4] for im in overlays}))
    if not images:
        sys.exit("no images - extract the disc first (see docs/tooling/extraction.md)")

    if args.home and not any(
        args.home.lower() in im.name.lower() for im in images if im.kind == "overlay"
    ):
        sys.exit(f"--home {args.home!r} matches no loaded overlay image")

    total = sum(len(im.data) for im in images)
    print(f"# {len(images)} images, {total / 1e6:.1f} MB scanned")
    print("# code=frame markers +-0x200  entry=neighbour words on a prologue here")

    rc = 0
    for a in addrs:
        target = int(a, 16)
        report_target(target, images, args)

    if args.expect_scus:
        want = int(args.expect_scus, 16)
        target = int(addrs[0], 16)
        hits = [scus.va(o) for o in word_hits(scus, target)] if scus else []
        ok = want in hits
        print(f"\ncontrol: 0x{target:08x} word at SCUS 0x{want:08x}: {'FOUND' if ok else 'ABSENT'}")
        rc = 0 if ok else 1
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
