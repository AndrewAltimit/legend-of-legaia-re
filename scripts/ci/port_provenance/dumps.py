"""The dump corpus: one parsed dump, the corpus loader, which image owns an
address, and the consensus among same-address dumps.
"""

from __future__ import annotations

import csv
import hashlib
import re
from collections import defaultdict
from functools import lru_cache
from pathlib import Path

from port_provenance import paths
from port_provenance.thresholds import (
    ATTRIBUTION_CSV,
    ATTRIBUTION_TRUSTED,
    OVERLAY_BASE,
    SCUS_IMAGE,
    UNTRUSTED_JAL_BELOW,
    UNTRUSTED_JAL_IMAGE,
)

# The image tag is optional here and required later: a third of the corpus
# predates it, and those dumps are still valid witnesses for how *common* an
# address is even though they cannot say which image a body belongs to.
HEADER_RE = re.compile(
    r"^==\s+\S+\s+([0-9a-fA-F]{8})\s+\(entry=([0-9a-fA-F]{8})\)\s*(?:\[([^\]]+)\])?"
)
SIZE_RE = re.compile(r"^size=(\d+) bytes, (\d+) instructions")
PC_RE = re.compile(r"^([0-9a-f]{8})\s")

LUI_RE = re.compile(r"^([0-9a-f]{8})\s+_?lui\s+(\w+),0x([0-9a-f]+)\s*$")
ADD_RE = re.compile(r"^([0-9a-f]{8})\s+_?(addiu|ori)\s+(\w+),(\w+),(-?)0x([0-9a-f]+)\s*$")
# `lui at,0x801f` / `addu at,at,index` / `lw v0,0x6aa8(at)` - the MIPS indexed
# jump-table idiom. The `addu` clobbers the register the `lui` half lives in,
# so a tracker that drops a register on any write loses every jump-table base
# in the corpus, and then reports the tags that cite one as unsupported.
ADDU_RE = re.compile(r"^([0-9a-f]{8})\s+_?add[u]?\s+(\w+),(\w+),(\w+)\s*$")
MEM_RE = re.compile(
    r"^([0-9a-f]{8})\s+_?(lw|lh|lhu|lb|lbu|sw|sh|sb|lwl|lwr|swl|swr|lwc2|swc2)\s+"
    r"(\w+),(-?)0x([0-9a-f]+)\((\w+)\)\s*$"
)
JAL_RE = re.compile(r"^([0-9a-f]{8})\s+_?jal\s+0x([0-9a-f]{8})")
# Generic "this instruction writes its first operand" fallback, used to kill a
# tracked register when something other than the forms above redefines it.
WRITE_RE = re.compile(r"^[0-9a-f]{8}\s+_?([a-z0-9.]+)\s+(\w+),")
# The same fallback for the one-operand writers, which carry no comma and so
# slipped past `WRITE_RE` entirely: `mflo v0` redefines `v0` as surely as any
# `addu` does, and a `lui` half kept across one is a synthesised address.
WRITE1_RE = re.compile(r"^[0-9a-f]{8}\s+_?(mflo|mfhi)\s+(\w+)\s*$")

# Loads. Their destination register is redefined from memory, which ends the
# `lui` provenance the tracker was carrying in it - see `parse_dump`.
LOAD_OPS = {"lw", "lh", "lhu", "lb", "lbu", "lwl", "lwr", "lwc2"}

# MIPS instructions whose first operand is a *source*, not a destination. Getting
# this list wrong in either direction only loosens or tightens the tracker, but a
# store killing its own source register is the common way to lose a base pointer.
NON_WRITING_OPS = {
    "sw", "sh", "sb", "swl", "swr", "swc2", "sc",
    "beq", "bne", "blez", "bgtz", "bltz", "bgez", "bltzal", "bgezal",
    "beqz", "bnez", "b", "j", "jr", "jal", "jalr",
    "mtc0", "mtc2", "ctc2", "mthi", "mtlo", "nop", "break", "syscall",
}
# Registers that survive a call under the MIPS o32 ABI. Everything else is
# clobbered at a `jal`, and keeping stale halves across a call is the fastest
# way to synthesise a data address the routine never forms.
CALLEE_SAVED = {f"s{i}" for i in range(9)} | {"fp", "gp", "sp", "ra", "s8"}


# ---------------------------------------------------------------------------
# Dump parsing
# ---------------------------------------------------------------------------


class Dump:
    """One `ghidra/scripts/funcs/<stem>.txt` file, read as disassembly only."""

    __slots__ = ("path", "stem", "addr", "entry", "size", "n_instr", "image",
                 "formed", "jals", "text", "last", "printed", "body")

    def __init__(self, path: Path):
        self.path = path
        self.stem = path.stem
        self.addr = ""
        self.entry = 0
        self.size = 0
        self.n_instr = 0
        self.image = ""
        # Instructions actually printed, and a fingerprint of them. A dump that
        # printed none carries no evidence at all (`ghidra.md` catalogues the
        # "0 instructions, decompiled C only" shape), and two dumps of one VA
        # whose fingerprints differ are two different routines - the VA-aliasing
        # case, which no amount of reading either body can resolve.
        self.printed = 0
        self.body = ""
        # Last printed instruction address. The header's `size` is the
        # instruction count times four, which is NOT the extent when Ghidra
        # skipped undefined bytes inside the body: `overlay_0977_slotA_801cf870`
        # says 1748 bytes and prints across 2088. Taking `size` as the extent
        # made every citation into such a gap read as "absent from the dump".
        self.last = 0
        self.formed: dict[int, str] = {}
        self.jals: set[int] = set()
        self.text = ""


def _resolve(hi: dict[str, int], full: dict[str, int], reg: str, off: int) -> int | None:
    if reg in full:
        return (full[reg] + off) & 0xFFFFFFFF
    if reg in hi:
        return ((hi[reg] << 16) + off) & 0xFFFFFFFF
    return None


def parse_dump(path: Path) -> Dump | None:
    """Recover formed data addresses + `jal` targets from a dump's disassembly.

    The tracker is a straight-line abstract interpretation over two register
    maps: `hi` (a `lui`'s upper half) and `full` (a completed address). It is
    intentionally simple - MIPS forms almost every global reference as a
    `lui`/`addiu` pair or as a `lui`+displacement load, both within a few
    instructions - and it is conservative in the direction that matters: a
    register whose provenance is lost is dropped, never guessed.
    """
    try:
        raw = path.read_text(errors="ignore")
    except OSError:
        return None
    d = Dump(path)
    d.text = raw
    hi: dict[str, int] = {}
    full: dict[str, int] = {}
    body: list[str] = []
    in_dis = False
    for line in raw.splitlines():
        if not in_dis:
            m = HEADER_RE.match(line)
            if m:
                d.addr = m.group(1).lower()
                d.entry = int(m.group(2), 16)
                d.image = m.group(3) or ""
                continue
            m = SIZE_RE.match(line)
            if m:
                d.size = int(m.group(1))
                d.n_instr = int(m.group(2))
                continue
            if line.startswith("--- DISASSEMBLY"):
                in_dis = True
            continue
        if line.startswith("---"):
            break
        s = line.strip()
        if not s:
            continue

        # Every printed instruction, before the opcode arms consume it: the
        # earlier placement counted only the lines no arm matched, so `d.last`
        # silently omitted every `lui` / `addiu` / load in the body.
        pc = PC_RE.match(s)
        if pc:
            d.last = max(d.last, int(pc.group(1), 16))
            d.printed += 1
            body.append(s)

        m = LUI_RE.match(s)
        if m:
            reg = m.group(2)
            hi[reg] = int(m.group(3), 16)
            full.pop(reg, None)
            continue

        m = ADD_RE.match(s)
        if m:
            _, _op, rd, rs, sign, imm = m.groups()
            val = int(imm, 16) * (-1 if sign == "-" else 1)
            base = _resolve(hi, full, rs, 0)
            if base is not None and rs in hi and rs not in full:
                addr = (base + val) & 0xFFFFFFFF if _op == "addiu" else base | (
                    val & 0xFFFF
                )
                d.formed.setdefault(addr, m.group(1))
                hi.pop(rd, None)
                full[rd] = addr
            else:
                hi.pop(rd, None)
                full.pop(rd, None)
            continue

        m = ADDU_RE.match(s)
        if m:
            _, rd, rs, rt = m.groups()
            # Exactly one operand carries an upper half and neither is a
            # completed address: the sum is that half plus an unknown index, so
            # a later displacement still names the table base.
            halves = [r for r in (rs, rt) if r in hi and r not in full]
            full.pop(rd, None)
            if len(halves) == 1:
                hi[rd] = hi[halves[0]]
            else:
                hi.pop(rd, None)
            continue

        m = MEM_RE.match(s)
        if m:
            _, _op, dst, sign, off, base = m.groups()
            val = int(off, 16) * (-1 if sign == "-" else 1)
            addr = _resolve(hi, full, base, val)
            if addr is not None:
                d.formed.setdefault(addr, m.group(1))
            # A load redefines its destination from memory. Carrying the old
            # `lui` provenance through it is how the checker manufactured a
            # data address that exists in no dump: `lui/addiu` forms
            # 0x801C9370, `lw v0,0x0(a0)` reloads `v0` from a pointer table,
            # and the next `lhu v0,0x14c(v0)` was then read as forming
            # 0x801C94BC - a field offset off a runtime pointer, reported as a
            # global the routine owns. Every `base + small offset` chain behind
            # a pointer load has this shape, so this is a class, not a case.
            if _op in LOAD_OPS:
                hi.pop(dst, None)
                full.pop(dst, None)
            continue

        m = JAL_RE.match(s)
        if m:
            d.jals.add(int(m.group(2), 16))
            for r in list(hi):
                if r not in CALLEE_SAVED:
                    del hi[r]
            for r in list(full):
                if r not in CALLEE_SAVED:
                    del full[r]
            continue

        m = WRITE_RE.match(s)
        if m and m.group(1) not in NON_WRITING_OPS:
            reg = m.group(2)
            hi.pop(reg, None)
            full.pop(reg, None)
            continue

        m = WRITE1_RE.match(s)
        if m:
            reg = m.group(2)
            hi.pop(reg, None)
            full.pop(reg, None)
    d.body = hashlib.sha1("\n".join(body).encode()).hexdigest()
    return d


def load_corpus() -> tuple[dict[str, list[Dump]], dict[int, int], dict[int, int]]:
    """Parse every dump. Returns ({addr: [Dump]}, data-DF, callee-DF).

    The document-frequency maps are the whole point of parsing dumps the checker
    does not otherwise need: without a corpus denominator there is no way to
    tell a subsystem's private table from the game-state window, and every
    routine looks related to every other one.

    DF counts **distinct addresses**, not dump files. Several routines have two
    or three dumps of the same VA from different images, and counting files
    would inflate exactly the aliased addresses the checker must not over-weight.

    Two populations, deliberately different:

    - The **denominator** is every dump that printed instructions. How many
      routines form an address is a property of the corpus, and an untagged
      dump is as good a witness to it as a tagged one. Restricting it to
      image-tagged dumps measured the commonality of half the corpus and called
      the result distinctive.
    - The **evidence** map needs an image only where a VA is ambiguous, which
      is the overlay band and nowhere else (`OVERLAY_BASE` up). One SCUS image
      ships on the disc, so a dump whose entry sits below that base has exactly
      one possible owner and its untagged header withholds nothing. Requiring
      the tag everywhere dropped every plain `<addr>.txt` SCUS dump from the
      evidence map, and three of the four signals read only that map - so
      `dual-label` and `doc-citation` went silent over the whole SCUS range
      while reporting a clean run. `elsewhere_claims` already draws the line
      here (`data_addr >= OVERLAY_BASE`); this is the same line drawn one layer
      earlier.

    Both drop a dump whose header `entry` is not the address asked for: that
    file is a window opening inside a *different* routine, and reading a
    routine's identity off it is the trap this whole checker exists to catch.
    """
    by_addr: dict[str, list[Dump]] = defaultdict(list)
    df: dict[int, int] = defaultdict(int)
    cdf: dict[int, int] = defaultdict(int)
    if not paths.FUNCS_DIR.exists():
        return by_addr, df, cdf
    every: list[Dump] = []
    for p in sorted(paths.FUNCS_DIR.glob("*.txt")):
        if p.name.endswith(("_index.txt", "_survey.txt")):
            continue
        d = parse_dump(p)
        if d is None or not d.addr or not d.printed:
            continue
        if d.entry != int(d.addr, 16):
            continue
        every.append(d)
        if not d.image and d.entry < OVERLAY_BASE:
            # One SCUS image exists, so the owner is not in question.
            d.image = SCUS_IMAGE
        if d.image:
            by_addr[d.addr].append(d)
    seen_data: dict[int, set[str]] = defaultdict(set)
    seen_call: dict[int, set[str]] = defaultdict(set)
    for d in every:
        for a in d.formed:
            seen_data[a].add(d.addr)
        for j in trusted_jals(d):
            seen_call[j].add(d.addr)
    for a, who in seen_data.items():
        df[a] = len(who)
    for j, who in seen_call.items():
        cdf[j] = len(who)
    return by_addr, df, cdf


def image_key(name: str) -> str:
    """A dump image or attribution-CSV image name reduced to `label:entry`.

    `overlay_other2_dev_0973.bin` and `other2_dev(973)` are the same image
    written by two tools; both reduce to `other2_dev:973`. The PROT entry is
    the part that decides, so a name carrying one compares on it alone and a
    name carrying none (`SCUS_942.54`) compares on its own text.
    """
    stem = name.strip().rsplit("/", 1)[-1]
    stem = re.sub(r"\.(bin|BIN)$", "", stem)
    m = re.match(r"^(.*?)\((\d+)\)$", stem)
    if m:
        return f"{m.group(1).strip().lower()}:{int(m.group(2))}"
    m = re.match(r"^overlay_(.*)_(\d{3,4})$", stem)
    if m:
        return f"{m.group(1).lower()}:{int(m.group(2))}"
    return stem.lower()


@lru_cache(maxsize=1)
def owning_images() -> dict[str, set[str]]:
    """`{addr: {image_key, ...}}` from `dump-extent-attribution.csv`.

    Only the classes that actually name an owner (`ATTRIBUTION_TRUSTED`); the
    rest of the CSV records that the instrument could not decide, and a
    verdict of "undecided" must not narrow anything.
    """
    out: dict[str, set[str]] = defaultdict(set)
    if not ATTRIBUTION_CSV.is_file():
        return out
    try:
        with ATTRIBUTION_CSV.open(newline="") as fh:
            for row in csv.DictReader(fh):
                if (row.get("class") or "").strip() not in ATTRIBUTION_TRUSTED:
                    continue
                img = (row.get("image") or "").strip()
                if not img or img == "-":
                    continue
                out[(row.get("entry") or "").strip().lower()].add(image_key(img))
    except OSError:
        return out
    return out


def consensus(dumps: list[Dump]) -> list[Dump]:
    """The dumps of one address that agree on what the routine is.

    Several images link different code over one overlay VA, so an address's
    dumps are not all the same routine, and unioning their formed addresses
    attributes one routine's tables to another. Group by body fingerprint and
    keep the plurality; a tie means the corpus cannot say which body the port
    implements, and the address contributes no evidence rather than the wrong
    evidence.

    Worked case: `0x801F8E6C` has six dumps agreeing at 47 instructions with a
    real prologue, and one 48-instruction window in `overlay_0897` that opens on
    a bare `jal` mid-routine. Unioned, the odd one's callee and string-pool
    reads read as the routine's own.

    Plurality is a vote, though, and a vote is a property of the corpus rather
    than of the disc: adding one more dump of a VA taken under a *different*
    image can flip which body wins, or tie it and silence the address, without
    a byte on the disc changing. So where
    `scripts/ghidra-analysis/dump-extent-attribution.csv` has already decided
    which image's content reproduces the window at this VA, that verdict is
    applied first and the vote runs only among the dumps of the owning image.
    The CSV is byte-derived, so this keys the evidence to the **module** that
    holds the routine instead of to whichever image happened to be dumped most.
    """
    if len(dumps) < 2:
        return dumps
    owners = owning_images().get(dumps[0].addr, set())
    if owners:
        kept = [d for d in dumps if image_key(d.image) in owners]
        # Only narrow. An address whose owning image has no dump keeps the
        # unfiltered vote - taking the empty set would silence it on the
        # strength of a row that says nothing about the dumps we hold.
        if kept:
            dumps = kept
        if len(dumps) < 2:
            return dumps
    groups: dict[str, list[Dump]] = defaultdict(list)
    for d in dumps:
        groups[d.body].append(d)
    if len(groups) == 1:
        return dumps
    ranked = sorted(groups.values(), key=len, reverse=True)
    if len(ranked[0]) == len(ranked[1]):
        return []
    return ranked[0]


def extent_of(d: Dump) -> tuple[int, int]:
    """The address range a dump actually printed, header `size` notwithstanding."""
    return (d.entry, max(d.entry + max(d.size, 4), d.last + 4))


def trusted_jals(d: Dump) -> set[int]:
    if d.image.startswith(UNTRUSTED_JAL_IMAGE) and d.entry < UNTRUSTED_JAL_BELOW:
        return set()
    return d.jals


def coverage(d: Dump) -> float:
    """Printed instructions over the address span the dump printed across."""
    span = d.last - d.entry + 4
    return (d.printed * 4) / span if span > 0 else 1.0
