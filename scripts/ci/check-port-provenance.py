#!/usr/bin/env python3
"""Provenance checker for `// PORT: FUN_<addr>` tags.

`port-catalog.py` answers two questions about a port tag: does it **exist**,
and is the symbol carrying it **reachable** from a host root. Neither question
is "does that address name the routine this Rust code actually implements".
Nothing gated that, so a live, tested, shipped port could wear a wrong address
with every gate green - and `disc-coverage.py` would then count the wrong
retail bytes as covered.

The worked example, and the reason this file exists:
`engine-core::shop::shop_stock_row_ink` carried `PORT: FUN_801D5DE0` while live
on both hosts. That routine is the **casino prize list's** row renderer - it
forms the prize table `0x801E4518` and gates affordability on the coin bank
`0x800845A4`. The mis-attribution came from the dump filename
`overlay_shop_save_801d5de0.txt`, whose prefix names the *image the routine was
dumped from*, not what the routine does.
[`docs/tooling/dump-corpus-integrity.md`](../../docs/tooling/dump-corpus-integrity.md)
already states that law for load bases; this checker applies it to function
identity.

## What it measures

Four signals. The two disassembly-side ones read the **disassembly** and never
the decompiled C (see
`docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims`):
for every PORT-tagged address the checker recovers the data addresses the
routine forms (`lui`+`addiu`/`ori`, load/store displacements against a register
whose provenance is a `lui`, and the `lui`/`addu`/`lw` indexed jump-table
idiom) plus the `jal` targets it calls.

`module-orphan`
    A module PORT-tags several routines, and one of them shares **no**
    distinctive data address, **no** distinctive callee and no call edge with
    any of its siblings, while the siblings do corroborate each other. Ranked,
    not accused - a module may legitimately port an unrelated kernel. What
    lifts a row is the corroboration line: the same rare table turning up under
    a module with an unrelated name. That conjunction is the shop shape, and
    `shop.rs` / `FUN_801D5DE0` reproduces it.

    "Distinctive" is measured at a cut that follows the module, because a fixed
    one is blind to a whole kind of module rather than to a random sample of
    them: a painter's only private datum is its own string pool and a host
    dispatcher's members are unrelated by design, so both report one orphan per
    member. See `DF_LADDER`.

`dual-label`
    One routine given a defining description - a `###` section, or a row in the
    function directory - on two docs pages whose names share nothing. At most
    one can be right. `FUN_801D5DE0` was simultaneously the world-map tile
    cursor SM, the shop stock list and the casino prize list.

`absent-citation`
    A `PORT:` line's own parenthetical cites a concrete retail address - an
    interior range, a `DAT_` - that appears **nowhere** in the routine's dump.
    Evidence the port offers for itself that the disassembly does not carry.

`doc-citation`
    The same test applied to `docs/reference/functions/` rows.

## What it deliberately does not do

There is no zero-false-positive oracle here and this does not try to be one.
Precision beats recall: a noisy gate gets ignored, which is worse than no gate.
Known-good shapes are excluded by construction rather than left to the reader:

- **One routine linked into two overlays** is not a defect. `FUN_801D14B0` and
  `FUN_801D6710` are the same 24 instructions at two VAs. Identical bodies at
  different VAs never produce a finding, because the signals are per-address.
- **Deliberate kernel reuse across subsystems** is not a defect either. The
  defect is the *claim*. A finding names both sides; a human decides which
  label is wrong, or waives it as reuse.
- **Sharing a subsystem's tables is the normal case.** A fifth signal that
  reported every pair of PORT-tagged routines sharing a rare table across
  unrelated module names was built, measured at 328 findings, and deleted. See
  `elsewhere_claims` for what survived of it and why the consensus variant did
  not rescue it either.
- `jal` targets decoded from the `overlay_0896` window **below** `0x801CE818`
  are untrustworthy (`docs/tooling/call-target-integrity.md`), so those dumps
  contribute no callee signal.

The report is a **worklist, not a gate**. It is warn-only and is not wired into
pre-commit; `--strict` exists for a reader who has waived what they have read.

## Usage

    python3 scripts/ci/check-port-provenance.py             # ranked report
    python3 scripts/ci/check-port-provenance.py --live-only  # live ports first
    python3 scripts/ci/check-port-provenance.py --addr 801d5de0
    python3 scripts/ci/check-port-provenance.py --strict     # exit 1 on unwaived
    python3 scripts/ci/check-port-provenance.py --emit-waivers

Reviewed false positives go in `scripts/ci/port-provenance-waivers.toml`, one
entry per finding key, each with a `reason`. `--strict` is the ratchet: it
fails only on findings that carry no waiver.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import re
import sys
from collections import defaultdict
from pathlib import Path

try:  # py311+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - py<3.11
    tomllib = None  # type: ignore[assignment]

REPO = Path(__file__).resolve().parent.parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"
CRATES_DIR = REPO / "crates"
WAIVERS = Path(__file__).resolve().parent / "port-provenance-waivers.toml"
LIVE_CSV = REPO / "target" / "port-catalog" / "catalog.csv"

# ---------------------------------------------------------------------------
# Tuning. Every threshold here is a precision/recall dial; the defaults were
# picked by triaging the report by hand, not by fitting a number.
# ---------------------------------------------------------------------------

# A data address is "distinctive" when at most this many dumps in the whole
# corpus form it. Above the cut it is a shared runtime global (the game-state
# window, the item-name table) and carries no subsystem information.
DISTINCTIVE_DF_MAX = 8

# Same cut for callees. Without it the signal is masked: the text writer
# `0x80036888` is called from 82 dumps and the string blitter `0x8002b994` from
# 39, so "these two routines share a callee" is true of almost any pair and
# corroborates nothing.
CALLEE_DF_MAX = 8

# `split-table` reports across module boundaries, where a wrong hit costs a
# reader more, so it wants the rarest addresses only.
SPLIT_DF_MAX = 3

# PSX main RAM. The abstract interpreter can synthesise an address from a `lui`
# whose low half never arrives on the path taken; anything outside the 2 MB the
# game can address is such an artifact and is dropped rather than ranked.
RAM_LO = 0x80000000
RAM_HI = 0x801FFFFF

# Overlays are linked over this window, several of them at the same base, so a
# VA at or above it identifies an object only together with the image it was
# read in. Below it the address is always-resident and means one thing.
OVERLAY_BASE = 0x801C0000

# `module-orphan` needs enough siblings for "shares nothing" to mean anything.
ORPHAN_MIN_SIBLINGS = 3

# What share of its own address span a dump has to print before "this routine
# touches nothing its siblings touch" is a statement about the routine rather
# than about the dump. A jump-table dispatcher is routinely dumped as its head
# plus its shared epilogue and nothing in between - `FUN_801EA9B0` prints 25
# instructions across a 250-instruction span, so 90% of it was never read.
#
# Two distinct shapes live under 1.0 and the floor sits between them: a stub of
# that kind prints a tenth or less, while a body with one arm skipped prints
# about half (`FUN_801DD9D4`: 69 instructions over a 147-slot span, 0.47). A
# floor at 0.5 would have taken the second kind with the first - and did, in
# testing, silence a known-real defect.
DUMP_COVERAGE_MIN = 0.25

# One fixed distinctiveness cut cannot read every module, and the failure is not
# random: it is a property of what the module ports.
#
# A window-descriptor **painter** has exactly one datum of its own - its private
# overlay string pool - and calls only the corpus-wide text writer `0x80036888`,
# number writer `0x80034B78` and marker blitter `0x8002B994`. Its siblings share
# the choice-state word and the panel-install callee, both far above the cut. So
# every painter in a painter module reads as sharing nothing with every other
# one, and the module reports as many orphans as it has members. The same shape
# one level up produced eleven more from one actor-hub module.
#
# The fix is to let the cut follow the module. A module whose members really do
# belong together links up at *some* rarity; where that happens says how private
# its shared vocabulary is, and only a member that stays unlinked there is
# saying anything. `ui_menu_window_painters.rs` links at 96 and has no orphan
# left; `world/field_movement.rs` is already cohesive at the base cut and its
# orphan stays one at every cut on the ladder.
DF_LADDER = (8, 12, 16, 24, 32, 48, 64, 96)

# ...but a per-module estimate needs pairs to estimate from. Five tags are ten
# pairs and several of those are vacuous; the cut such a module "needs" is one
# pair's accident. Below this many tagged addresses the base cut stands, and
# the cheapest way to see why is `ui_menu_window_painters_large.rs`: four tags,
# two of them corroborating, and its one genuinely wrong tag stops reading as an
# orphan the moment the cut reaches 16.
ADAPTIVE_MIN_TAGS = 6

# What "the module reads as cohesive" means: this share of its tagged addresses
# corroborating at least one other. Under it, "shares nothing" is the module's
# normal condition and says nothing about any member.
#
# This is a fitted number and it should be read as one. It was chosen by
# sweeping it against the 66 hand-reviewed verdicts behind
# `docs/tooling/port-provenance.md`, under the constraint that every one of the
# four real defects keeps firing. 0.95 breaks that constraint - it absorbs three
# of the four - so the constraint, not the sweep, is what fixes the ceiling.
ADAPTIVE_TARGET = 0.85

# Tokens too generic to make two module names "related".
STOPWORD_TOKENS = {
    "mod",
    "lib",
    "main",
    "src",
    "crates",
    "engine",
    "core",
    "render",
    "util",
    "utils",
    "common",
    "state",
    "data",
    "table",
    "tables",
}

# The one dump window whose decoded `jal` targets are a property of a wrong
# load base (docs/tooling/call-target-integrity.md).
UNTRUSTED_JAL_IMAGE = "overlay_0896"
UNTRUSTED_JAL_BELOW = 0x801CE818

# The three doc roots that say what a routine *is*. Everything else under
# `docs/` answers a different question about it: `docs/tooling/` describes the
# instruments, `docs/guides/` the tasks, and the thread ledgers
# (`re-settled-threads.md`, `re-do-not-re-walk.md`, `open-rev-eng-threads.md`)
# record which questions are answered and which readings are falsified. A
# falsification titled "X is *not* Y" names an address without claiming it, so
# it is not a second label - `dual-label` only compares defining pages.
DEFINING_DOC_ROOTS = (
    "docs/subsystems/",
    "docs/formats/",
    "docs/reference/functions/",
)

# ---------------------------------------------------------------------------
# Regexes
# ---------------------------------------------------------------------------

CODE_ADDR = r"80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4}"

PORT_TAG_RE = re.compile(r"//[/!]?\s*PORT\s*:\s*(.*)", re.IGNORECASE)
PORT_ADDR_RE = re.compile(
    rf"(?:FUN_|overlay_[0-9a-zA-Z_]+?_)({CODE_ADDR})", re.IGNORECASE
)
# Any concrete retail address a tag line cites for itself: bare `0x8...`,
# `FUN_8...`, `DAT_8...`, `_DAT_8...`, backticked. Deliberately wider than
# PORT_ADDR_RE - this one is looking for the port's own evidence, not its claim.
CITED_ADDR_RE = re.compile(r"(?:0x|FUN_|_?DAT_|PTR_)([0-9a-fA-F]{8})", re.IGNORECASE)

MD_LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)")

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
    - The **evidence** map keeps only image-tagged dumps, because attributing a
      body to a VA in the overlay band needs the image
      (`docs/tooling/dump-corpus-integrity.md`).

    Both drop a dump whose header `entry` is not the address asked for: that
    file is a window opening inside a *different* routine, and reading a
    routine's identity off it is the trap this whole checker exists to catch.
    """
    by_addr: dict[str, list[Dump]] = defaultdict(list)
    df: dict[int, int] = defaultdict(int)
    cdf: dict[int, int] = defaultdict(int)
    if not FUNCS_DIR.exists():
        return by_addr, df, cdf
    every: list[Dump] = []
    for p in sorted(FUNCS_DIR.glob("*.txt")):
        if p.name.endswith(("_index.txt", "_survey.txt")):
            continue
        d = parse_dump(p)
        if d is None or not d.addr or not d.printed:
            continue
        if d.entry != int(d.addr, 16):
            continue
        every.append(d)
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
    """
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


# ---------------------------------------------------------------------------
# Port tags
# ---------------------------------------------------------------------------


class Tag:
    """One address claimed by one `// PORT:` line.

    `seg` is the slice of the tag's tail that belongs to **this** address: a tag
    line may claim several routines, each with its own parenthetical, and
    checking every parenthetical against every address on the line manufactured
    findings out of correct prose. `PORT: FUN_801ed308 (cases 6/7), ...,
    FUN_801ee90c (the 0x801EEA50 block), ...` reported four routines as missing
    a block only the fifth ever claimed.
    """

    __slots__ = ("addr", "file", "line", "tail", "seg", "raw")

    def __init__(
        self, addr: str, file: str, line: int, tail: str, seg: str, raw: str
    ):
        self.addr = addr
        self.file = file
        self.line = line
        self.tail = tail
        self.seg = seg
        self.raw = raw


def collect_tags() -> list[Tag]:
    """Every `// PORT:` tag in `crates/`, one record per (address, tag line)."""
    out: list[Tag] = []
    if not CRATES_DIR.exists():
        return out
    for p in sorted(CRATES_DIR.rglob("*.rs")):
        rel = str(p.relative_to(REPO))
        # A tag inside a test file is commentary on coverage, not a port claim -
        # same exclusion port-catalog.py makes when resolving anchors.
        if "/tests/" in rel or "/benches/" in rel or "/examples/" in rel:
            continue
        try:
            text = p.read_text(errors="ignore")
        except OSError:
            continue
        for lineno, line in enumerate(text.splitlines(), start=1):
            m = PORT_TAG_RE.search(line)
            if not m:
                continue
            tail = m.group(1)
            hits = list(PORT_ADDR_RE.finditer(tail))
            for i, am in enumerate(hits):
                stop = hits[i + 1].start() if i + 1 < len(hits) else len(tail)
                out.append(
                    Tag(
                        am.group(1).lower(),
                        rel,
                        lineno,
                        tail,
                        tail[am.end():stop],
                        line,
                    )
                )
    return out


def module_tokens(rel: str) -> set[str]:
    """Alphabetic tokens of a Rust file's path, minus the generic ones.

    Two modules "share a name" when these overlap. Used only to decide whether a
    `split-table` finding is worth showing - `shop.rs` vs `prize_exchange.rs`
    share nothing, `ui_menu.rs` vs `ui_menu_window_painters.rs` share plenty.
    """
    stem = rel.split("crates/", 1)[-1]
    toks = {t for t in re.split(r"[^a-z0-9]+", stem.lower()) if len(t) >= 4}
    return toks - STOPWORD_TOKENS


# ---------------------------------------------------------------------------
# Findings
# ---------------------------------------------------------------------------


class Finding:
    __slots__ = ("signal", "key", "addr", "where", "rank", "lines")

    def __init__(self, signal: str, key: str, addr: str, where: str, rank: float,
                 lines: list[str]):
        self.signal = signal
        self.key = key
        self.addr = addr
        self.where = where
        self.rank = rank
        self.lines = lines


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


def features(
    dumps: list[Dump],
    df: dict[int, int],
    cdf: dict[int, int],
    df_max: int,
    callee_max: int | None = None,
) -> tuple[set[int], set[int]]:
    """(distinctive data addresses, distinctive callees) over an address's dumps."""
    cm = CALLEE_DF_MAX if callee_max is None else callee_max
    g: set[int] = set()
    j: set[int] = set()
    for d in consensus(dumps):
        g |= {
            a for a in d.formed if df.get(a, 0) <= df_max and RAM_LO <= a <= RAM_HI
        }
        j |= {x for x in trusted_jals(d) if cdf.get(x, 0) <= cm}
    return g, j


def elsewhere_claims(
    tags: list[Tag],
    by_addr: dict[str, list[Dump]],
    df: dict[int, int],
    cdf: dict[int, int],
) -> dict[tuple[str, str], list[tuple[int, str, str]]]:
    """For each (module, routine), the rare tables it shares with other modules.

    Standalone, this was a signal of its own and it did not work. Reporting every
    pair of PORT-tagged routines that share a rare table produced hundreds of
    findings, because sharing a subsystem's own tables is the normal case and a
    host dispatcher module pairs with everything it dispatches. Requiring a
    *consensus* to contradict did not rescue it either: the largest groups are
    fifteen battle modules where "the odd one out by name" is arbitrary.

    What survives is its use as corroboration. A routine that already shares
    nothing with its own module's siblings, and whose rare table is claimed by a
    module with an unrelated name, is the shop shape exactly - and that
    conjunction is rare enough to act on. So this returns evidence, and
    `find_module_orphans` decides whether there is a finding to attach it to.

    Overlay-band addresses are VA-aliased across images, so a claim only counts
    when the two routines were dumped from a common image.
    """
    prof: dict[tuple[str, str], tuple[set[int], set[str], list[tuple[int, int]]]] = {}
    for t in tags:
        dumps = by_addr.get(t.addr)
        if not dumps or (t.file, t.addr) in prof:
            continue
        prof[(t.file, t.addr)] = (
            features(dumps, df, cdf, SPLIT_DF_MAX)[0],
            {d.image.split(".")[0].split(" ")[0] for d in consensus(dumps)},
            [extent_of(d) for d in consensus(dumps)],
        )

    users: dict[int, list[tuple[str, str]]] = defaultdict(list)
    for key, (g, _img, extent) in prof.items():
        for a in g:
            if not any(lo <= a < hi for lo, hi in extent):
                users[a].append(key)

    out: dict[tuple[str, str], list[tuple[int, str, str]]] = defaultdict(list)
    for data_addr, keys in users.items():
        for rel, addr in keys:
            for orel, oaddr in keys:
                if orel == rel or oaddr == addr:
                    continue
                if module_tokens(rel) & module_tokens(orel):
                    continue
                if data_addr >= OVERLAY_BASE and not (
                    prof[(rel, addr)][1] & prof[(orel, oaddr)][1]
                ):
                    continue
                out[(rel, addr)].append((data_addr, oaddr, orel))
    return out


def find_doc_row_citations(by_addr: dict[str, list[Dump]]) -> list[Finding]:
    """A `docs/reference/functions/` row citing an address its routine lacks.

    The function directory is the canonical label for every dumped routine, and
    a wrong label there outlives the tag that copied it - the shop mis-reading
    survived in `functions/menus.md` after the port and the subsystem page were
    both corrected. Each row's leading cell names the routine; every retail
    address in the rest of the row is evidence the row offers for that routine,
    and is checked against its dump the same way a tag line's is.
    """
    docs = REPO / "docs" / "reference" / "functions"
    if not docs.is_dir():
        return []
    row_re = re.compile(r"^\|\s*`([0-9a-fA-F]{8})`\s*\|(.*)$")
    out: list[Finding] = []
    for page in sorted(docs.glob("*.md")):
        rel = str(page.relative_to(REPO))
        for lineno, line in enumerate(page.read_text().splitlines(), start=1):
            m = row_re.match(line)
            if not m:
                continue
            subject = m.group(1).lower()
            dumps = by_addr.get(subject)
            if not dumps:
                continue
            missing = _unsupported(m.group(2), {subject}, dumps, set(by_addr))
            if not missing:
                continue
            out.append(
                Finding(
                    "doc-citation",
                    f"doc-citation:{rel}:{subject}:{'+'.join(missing)}",
                    subject,
                    f"{rel}:{lineno}",
                    float(len(missing)),
                    [
                        f"row for `{subject.upper()}` cites "
                        f"{', '.join('0x' + x for x in missing)}, absent from every "
                        f"dump of it ({', '.join(d.stem for d in dumps)}).",
                    ],
                )
            )
    return out


def _link_targets(page: Path, line: str) -> set[str]:
    """Repo-relative pages a markdown line links to, `#anchors` stripped."""
    out: set[str] = set()
    for m in MD_LINK_RE.finditer(line):
        target = m.group(1).split("#")[0]
        if not target or target.startswith(("http://", "https://", "mailto:")):
            continue
        try:
            resolved = (page.parent / target).resolve().relative_to(REPO)
        except (OSError, ValueError):
            continue
        out.add(str(resolved))
    return out


def find_dual_labels(by_addr: dict[str, list[Dump]]) -> list[Finding]:
    """One routine given a defining description on two unrelated docs pages.

    A routine gets exactly one canonical description. A `###` section in a
    subsystem page and a row in the function directory are the same claim filed
    twice on purpose; **two subsystem pages under unrelated names** are two
    different claims about the same bytes, and at most one can be right.

    This is the shape that outlived the port fix for `FUN_801D5DE0`. That
    routine's 151 instructions are byte-identical in all three of its dumps, and
    it was described as the world-map tile cursor state machine, as the shop
    stock list and as the casino prize list - on three pages, none of which
    mentioned the others.

    Page relatedness is decided on filename tokens, the same rule the module
    comparison uses, so `field-menu.md` and `menus.md` are one topic and
    `world-map.md` and `menus.md` are two. That rule alone over-fires by a wide
    margin, because the function directory is named for coarse topics
    (`menus`, `script-vms`, `battle`) and the write-ups for fine ones
    (`save-screen`, `field-locomotion`, `battle-action`), so the index entry and
    the page it indexes always read as "unrelated names". Two exclusions cut
    that class without touching the shape the signal exists for:

    - **A site that links to the counterpart page is a pointer, not a rival.**
      A directory row whose own text sends the reader to the page it is said to
      contradict is filing that page's claim, not competing with it. Scoped to
      the pair: a row that points at `save-screen.md` is still an independent
      claim against `world-map.md`. `FUN_801D5DE0`'s carriers cited nobody, so
      it keeps firing.
    - **Only defining pages count** (`DEFINING_DOC_ROOTS`). A thread ledger or a
      tooling page naming an address describes a question or an instrument, not
      the routine.
    """
    docs = REPO / "docs"
    if not docs.is_dir():
        return []
    head_re = re.compile(r"^#{2,4}\s+(.*)$")
    row_re = re.compile(r"^\|\s*`?(?:FUN_)?([0-9a-fA-F]{8})`?\s*\|")
    # {addr: {page: [(site label, pages this site links to)]}}
    sites: dict[str, dict[str, list[tuple[str, set[str]]]]] = defaultdict(
        lambda: defaultdict(list)
    )
    for page in sorted(docs.rglob("*.md")):
        rel = str(page.relative_to(REPO))
        if not rel.startswith(DEFINING_DOC_ROOTS):
            continue
        is_directory_page = "reference/functions/" in rel
        for lineno, line in enumerate(page.read_text().splitlines(), start=1):
            found: set[str] = set()
            m = head_re.match(line)
            if m:
                found |= {
                    x.group(1).lower() for x in CITED_ADDR_RE.finditer(m.group(1))
                } | {
                    x.group(1).lower()
                    for x in re.finditer(rf"`({CODE_ADDR})`", m.group(1))
                }
            elif is_directory_page:
                m = row_re.match(line)
                if m:
                    found.add(m.group(1).lower())
            if not found:
                continue
            links = _link_targets(page, line)
            for a in found:
                if a in by_addr:
                    sites[a][rel].append((f"{rel}:{lineno}", links))

    out: list[Finding] = []
    for addr, per_page in sites.items():
        pages = sorted(per_page)
        if len(pages) < 2:
            continue
        toks = {p: {t for t in re.split(r"[^a-z0-9]+", Path(p).stem.lower())
                    if len(t) >= 3} for p in pages}

        def points_at(src: str, dst: str) -> bool:
            """Every one of `src`'s sites for this address links to `dst`."""
            return all(dst in links for _, links in per_page[src])

        rivals = [
            (a, b)
            for i, a in enumerate(pages)
            for b in pages[i + 1:]
            if not (toks[a] & toks[b])
            and not points_at(a, b)
            and not points_at(b, a)
        ]
        if not rivals:
            continue
        lines = [
            f"FUN_{addr} carries a defining description on {len(pages)} docs "
            f"pages with unrelated names, neither pointing at the other; at "
            f"most one can be right:"
        ]
        for pg in pages:
            lines.append("    " + ", ".join(label for label, _ in per_page[pg]))
        lines.append(
            "    rival pair(s): "
            + "; ".join(f"{a} vs {b}" for a, b in rivals)
        )
        out.append(
            Finding("dual-label", f"dual-label:{addr}", addr,
                    ", ".join(pages), 8.0 + len(pages), lines)
        )
    return out


def _unsupported(
    text: str, claimed: set[str], dumps: list[Dump], entries: set[str]
) -> list[str]:
    """Cited retail addresses in `text` that no dump of the routine carries.

    "Carries" is read generously on purpose - inside the routine's own extent, a
    formed data address, a `jal` target, or a literal occurrence anywhere in the
    dump file. The last one covers `j`/branch targets, Ghidra's data-reference
    annotations and the globals the register tracker gives up on, and it is what
    keeps the signal at a handful of findings instead of hundreds.

    Citations of **other dumped function entries** are skipped outright. Prose
    about a routine names its callers, its siblings and the dispatcher that
    reaches it, and none of those has to appear in its own bytes; flagging them
    buried the signal under cross-references that were all correct. What is left
    is the claim worth checking - a data global or an interior range the routine
    is said to own and does not touch.
    """
    missing: list[str] = []
    for m in CITED_ADDR_RE.finditer(text):
        c = m.group(1).lower()
        if c in claimed or c in missing:
            continue
        if not re.fullmatch(CODE_ADDR, c, re.IGNORECASE) or c in entries:
            continue
        ci = int(c, 16)
        ok = False
        for d in dumps:
            lo, hi = extent_of(d)
            if (
                lo <= ci < hi
                or ci in d.formed
                or ci in d.jals
                or c in d.text.lower()
            ):
                ok = True
                break
        if not ok:
            missing.append(c)
    return missing


def find_module_orphans(
    tags: list[Tag],
    by_addr: dict[str, list[Dump]],
    df: dict[int, int],
    cdf: dict[int, int],
) -> list[Finding]:
    """A tagged routine that corroborates none of its module's other tags.

    Ranked, not accused: a module may legitimately port an unrelated kernel, and
    a routine whose dump is short enough to form nothing distinctive lands here
    for no reason at all. What lifts a row to the top of the list is the second
    half - the same routine's rare table turning up under a module with an
    unrelated name (`elsewhere_claims`). That conjunction is the shop shape.

    The cut at which "distinctive" is measured follows the module (`DF_LADDER`),
    because a fixed one reads some modules and not others, and the modules it
    cannot read are a recognisable kind rather than a random sample - see the
    ladder's own comment.
    """
    claims = elsewhere_claims(tags, by_addr, df, cdf)
    per_file: dict[str, set[str]] = defaultdict(set)
    for t in tags:
        if t.addr in by_addr:
            per_file[t.file].add(t.addr)

    # Who calls what, over the whole corpus. Two routines a third one calls are
    # siblings in that caller's eyes whatever tables they touch, and this is the
    # one corroboration channel the feature intersection cannot express: it is
    # not a shared third party and not an edge between the two.
    callers: dict[int, set[str]] = defaultdict(set)
    for a, dumps in by_addr.items():
        for d in consensus(dumps):
            for t in trusted_jals(d):
                callers[t].add(a)

    out: list[Finding] = []
    for rel, addrs in sorted(per_file.items()):
        if len(addrs) < ORPHAN_MIN_SIBLINGS:
            continue
        srt = sorted(addrs)
        pairs = [(a, b) for i, a in enumerate(srt) for b in srt[i + 1:]]
        # Calling a sibling is the strongest corroboration there is, and the
        # feature-set intersection misses it entirely: the callee is the
        # sibling's own address, never a shared third party. `save_select.rs`
        # read as having an orphan that calls four of its own siblings.
        calls_sibling = {
            (a, b)
            for a, b in pairs
            if any(int(b, 16) in d.jals for d in consensus(by_addr[a]))
            or any(int(a, 16) in d.jals for d in consensus(by_addr[b]))
            or callers[int(a, 16)] & callers[int(b, 16)]
        }

        def cohesion(cut: int) -> tuple[dict[str, tuple[set[int], set[int]]], set[str]]:
            fe = {a: features(by_addr[a], df, cdf, cut, cut) for a in addrs}
            linked = {
                (a, b)
                for a, b in pairs
                if (a, b) in calls_sibling
                or fe[a][0] & fe[b][0]
                or fe[a][1] & fe[b][1]
            }
            return fe, {x for a, b in linked for x in (a, b)}

        adopted = DISTINCTIVE_DF_MAX
        feats, cohesive = cohesion(adopted)
        if len(addrs) >= ADAPTIVE_MIN_TAGS:
            # Raise the cut until the module reads as cohesive. A module that
            # never does is one the signal cannot read at all, and silence is
            # the honest output - not one finding per member.
            for adopted in DF_LADDER:
                feats, cohesive = cohesion(adopted)
                if len(cohesive) >= ADAPTIVE_TARGET * len(addrs):
                    break
            if len(cohesive) < ADAPTIVE_TARGET * len(addrs):
                continue
        # The floor under both paths: a *majority* of the module's
        # evidence-bearing tags must corroborate before a member of the minority
        # can be called foreign. A tag whose routine forms nothing distinctive
        # cannot corroborate anything, so counting it against the module's
        # cohesion measures the corpus, not the module - which is why the
        # denominator here is the informative tags and not all of them.
        informative = {a for a in addrs if feats[a][0] or feats[a][1]}
        if len(cohesive) < 2 or 2 * len(cohesive) <= len(informative):
            continue
        for a in sorted(addrs - cohesive):
            g, j = feats[a]
            # A routine with no formed data of its own has one bit of evidence -
            # a single callee it does not share - and one bit does not carry a
            # claim about which subsystem the routine belongs to.
            if not g:
                continue
            # Corroboration and orphanhood need different evidence, and the
            # asymmetry is the whole reason this test sits here and not in
            # `features`. "These two touch the same table" is an existence
            # claim a fragment can settle, so a fragmentary sibling still
            # corroborates. "This one touches nothing any of them touch" is a
            # claim about the whole body, and a dump that printed a tenth of it
            # cannot make it. Putting the floor in `features` instead silenced
            # the field VM's own wrong tag, because its module's dispatcher
            # sibling is dumped as a fragment.
            if all(
                coverage(d) < DUMP_COVERAGE_MIN for d in consensus(by_addr[a])
            ):
                continue
            elsewhere = claims.get((rel, a), [])
            # Rank by how isolated the routine is and how specific its evidence
            # is, NOT by how much of it there is: an earlier formula multiplied
            # by the size of the feature set and simply sorted the big routines
            # to the top, where being big is not evidence of anything.
            rarest = min((df.get(x, 99) for x in g), default=99)
            rank = (
                (10.0 if elsewhere else 0.0)
                + 5.0 * len(cohesive) / len(addrs)
                + 2.0 * min(len({o for _, o, _ in elsewhere}), 3)
                + 1.0 / max(rarest, 1)
            )
            tag = next(t for t in tags if t.addr == a and t.file == rel)
            lines = [
                f"FUN_{a} shares no distinctive data address and no callee with "
                f"any of the {len(cohesive)} corroborating siblings tagged in "
                f"this module ({', '.join(sorted(cohesive))}); "
                f"distinctive = formed by at most {adopted} dumped routine(s).",
                "    its distinctive data: "
                + (", ".join(f"0x{x:08x}" for x in sorted(g)) or "(none)"),
                "    its callees: "
                + (", ".join(f"0x{x:08x}" for x in sorted(j)) or "(none)"),
            ]
            for data_addr, oaddr, orel in sorted(elsewhere)[:6]:
                lines.append(
                    f"    0x{data_addr:08x} is also formed by FUN_{oaddr}, "
                    f"PORT-tagged in {orel}"
                )
            out.append(
                Finding("module-orphan", f"module-orphan:{rel}:{a}", a,
                        f"{rel}:{tag.line}", rank, lines)
            )
    return out


def find_absent_citations(
    tags: list[Tag], by_addr: dict[str, list[Dump]]
) -> list[Finding]:
    """A tag line's own cited address that appears nowhere in the dump.

    Scoped to the tag line, not the surrounding rustdoc: a doc block routinely
    and correctly names *other* routines ("the shop's own builder FUN_80030628"),
    and flagging those would drown the signal. The parenthetical on a `PORT:`
    line is different - it is the evidence the port offers for itself.
    """
    out: list[Finding] = []
    for t in tags:
        dumps = by_addr.get(t.addr)
        if not dumps:
            continue
        claimed = {t.addr}
        claimed |= {m.group(1).lower() for m in PORT_ADDR_RE.finditer(t.tail)}
        missing = _unsupported(t.seg, claimed, dumps, set(by_addr))
        if missing:
            lines = [
                f"tag cites {', '.join('0x' + x for x in missing)}, absent from "
                f"every dump of FUN_{t.addr} "
                f"({', '.join(d.stem for d in dumps)}).",
                f"    {t.raw.strip()[:200]}",
            ]
            out.append(
                Finding(
                    "absent-citation",
                    f"absent-citation:{t.file}:{t.addr}:{'+'.join(missing)}",
                    t.addr,
                    f"{t.file}:{t.line}",
                    float(len(missing)),
                    lines,
                )
            )
    return out


# ---------------------------------------------------------------------------
# Waivers + live set
# ---------------------------------------------------------------------------


def load_waivers() -> dict[str, str]:
    if tomllib is None or not WAIVERS.exists():
        return {}
    with WAIVERS.open("rb") as fh:
        doc = tomllib.load(fh)
    out: dict[str, str] = {}
    for entry in doc.get("waiver", []):
        key = entry.get("key")
        if key:
            out[key] = entry.get("reason", "")
    return out


def load_live() -> set[str]:
    """Addresses port-catalog.py last reported as reachable from a host root."""
    if not LIVE_CSV.exists():
        return set()
    out: set[str] = set()
    with LIVE_CSV.open() as fh:
        for row in csv.DictReader(fh):
            if row.get("ported") == "1" and row.get("live") == "1":
                out.add(row["addr"].lower())
    return out


# ---------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--addr", default="", help="single-address drill-down")
    ap.add_argument(
        "--signal",
        default="",
        choices=["", "module-orphan", "absent-citation", "doc-citation", "dual-label"],
        help="restrict to one signal",
    )
    ap.add_argument(
        "--live-only",
        action="store_true",
        help="only findings touching a live port (needs target/port-catalog/catalog.csv)",
    )
    ap.add_argument("--top", type=int, default=0, help="cap rows (0 = no cap)")
    ap.add_argument(
        "--show-waived", action="store_true", help="include waived findings"
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 when an unwaived finding remains (the ratchet)",
    )
    ap.add_argument(
        "--emit-waivers",
        action="store_true",
        help="print a TOML waiver stub for every current unwaived finding",
    )
    args = ap.parse_args()

    if not FUNCS_DIR.exists() or not any(FUNCS_DIR.glob("*.txt")):
        # An empty corpus makes every port look unimpeachable. Say so instead of
        # printing a clean run - a vacuous pass is the failure mode this repo
        # has hit before (check-port-tags.py's "0 warnings across 0 files").
        print(
            "check-port-provenance: no dumps under ghidra/scripts/funcs/ - "
            "nothing to check. This is a vacuous pass, not a clean one."
        )
        return 0

    by_addr, df, cdf = load_corpus()
    tags = collect_tags()

    findings = (
        find_module_orphans(tags, by_addr, df, cdf)
        + find_absent_citations(tags, by_addr)
        + find_doc_row_citations(by_addr)
        + find_dual_labels(by_addr)
    )
    if args.signal:
        findings = [f for f in findings if f.signal == args.signal]
    if args.addr:
        # The doc-side signals scan every page, not the tag set, so filtering the
        # tags alone let a drill-down print the whole corpus back.
        want = args.addr.lower().removeprefix("0x")
        findings = [
            f
            for f in findings
            if f.addr == want or want in "\n".join(f.lines).lower()
        ]

    live = load_live()
    if args.live_only:
        if not live:
            print(
                "check-port-provenance: --live-only needs "
                "target/port-catalog/catalog.csv; run "
                "`python3 scripts/ci/port-catalog.py --live-only` first."
            )
            return 2
        findings = [
            f
            for f in findings
            if (f.addr and f.addr in live)
            or any(a in live for a in re.findall(CODE_ADDR, "\n".join(f.lines)))
        ]

    waivers = load_waivers()
    unwaived = [f for f in findings if f.key not in waivers]
    shown = findings if args.show_waived else unwaived
    shown.sort(key=lambda f: (-f.rank, f.signal, f.key))
    if args.top:
        shown = shown[: args.top]

    if args.emit_waivers:
        for f in sorted(unwaived, key=lambda x: x.key):
            print(f'[[waiver]]\nkey = "{f.key}"\nreason = "TODO"\n')
        return 0

    n_tags = len({(t.addr, t.file, t.line) for t in tags})
    n_addrs = len({t.addr for t in tags})
    print(
        f"check-port-provenance: {n_tags} PORT tag site(s) over {n_addrs} "
        f"address(es); {len(by_addr)} dumped address(es) parsed; "
        f"{len(df)} distinct data addresses formed corpus-wide."
    )
    for f in shown:
        mark = " [LIVE]" if f.addr and f.addr in live else ""
        waived = " [waived]" if f.key in waivers else ""
        print(f"\n{f.signal}  {f.where}{mark}{waived}")
        for line in f.lines:
            print("  " + line)
    by_signal = defaultdict(int)
    for f in unwaived:
        by_signal[f.signal] += 1
    print(
        f"\n{len(unwaived)} unwaived finding(s) "
        + (", ".join(f"{k}={v}" for k, v in sorted(by_signal.items())) or "(none)")
        + f"; {len(findings) - len(unwaived)} waived."
    )
    if args.strict and unwaived:
        print(
            "check-port-provenance: --strict and unwaived findings remain. "
            "Fix the tag, or waive it in scripts/ci/port-provenance-waivers.toml "
            "with a reason."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
