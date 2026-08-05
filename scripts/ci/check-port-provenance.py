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

HEADER_RE = re.compile(
    r"^==\s+\S+\s+([0-9a-fA-F]{8})\s+\(entry=([0-9a-fA-F]{8})\)\s+\[([^\]]+)\]"
)
SIZE_RE = re.compile(r"^size=(\d+) bytes, (\d+) instructions")

LUI_RE = re.compile(r"^([0-9a-f]{8})\s+_?lui\s+(\w+),0x([0-9a-f]+)\s*$")
ADD_RE = re.compile(r"^([0-9a-f]{8})\s+_?(addiu|ori)\s+(\w+),(\w+),(-?)0x([0-9a-f]+)\s*$")
# `lui at,0x801f` / `addu at,at,index` / `lw v0,0x6aa8(at)` - the MIPS indexed
# jump-table idiom. The `addu` clobbers the register the `lui` half lives in,
# so a tracker that drops a register on any write loses every jump-table base
# in the corpus, and then reports the tags that cite one as unsupported.
ADDU_RE = re.compile(r"^([0-9a-f]{8})\s+_?add[u]?\s+(\w+),(\w+),(\w+)\s*$")
MEM_RE = re.compile(
    r"^([0-9a-f]{8})\s+_?(lw|lh|lhu|lb|lbu|sw|sh|sb|lwl|lwr|swl|swr|lwc2|swc2)\s+"
    r"\w+,(-?)0x([0-9a-f]+)\((\w+)\)\s*$"
)
JAL_RE = re.compile(r"^([0-9a-f]{8})\s+_?jal\s+0x([0-9a-f]{8})")
# Generic "this instruction writes its first operand" fallback, used to kill a
# tracked register when something other than the forms above redefines it.
WRITE_RE = re.compile(r"^[0-9a-f]{8}\s+_?([a-z0-9.]+)\s+(\w+),")

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
                 "formed", "jals", "text", "last")

    def __init__(self, path: Path):
        self.path = path
        self.stem = path.stem
        self.addr = ""
        self.entry = 0
        self.size = 0
        self.n_instr = 0
        self.image = ""
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
    in_dis = False
    for line in raw.splitlines():
        if not in_dis:
            m = HEADER_RE.match(line)
            if m:
                d.addr = m.group(1).lower()
                d.entry = int(m.group(2), 16)
                d.image = m.group(3)
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
            _, _op, sign, off, base = m.groups()
            val = int(off, 16) * (-1 if sign == "-" else 1)
            addr = _resolve(hi, full, base, val)
            if addr is not None:
                d.formed.setdefault(addr, m.group(1))
            continue

        pc = re.match(r"^([0-9a-f]{8})\s", s)
        if pc:
            d.last = max(d.last, int(pc.group(1), 16))

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
    """
    by_addr: dict[str, list[Dump]] = defaultdict(list)
    df: dict[int, int] = defaultdict(int)
    cdf: dict[int, int] = defaultdict(int)
    if not FUNCS_DIR.exists():
        return by_addr, df, cdf
    for p in sorted(FUNCS_DIR.glob("*.txt")):
        if p.name.endswith(("_index.txt", "_survey.txt")):
            continue
        d = parse_dump(p)
        if d is None or not d.addr:
            continue
        by_addr[d.addr].append(d)
    for addr, dumps in by_addr.items():
        for a in {x for d in dumps for x in d.formed}:
            df[a] += 1
        for j in {x for d in dumps for x in trusted_jals(d)}:
            cdf[j] += 1
    return by_addr, df, cdf


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


def features(
    dumps: list[Dump], df: dict[int, int], cdf: dict[int, int], df_max: int
) -> tuple[set[int], set[int]]:
    """(distinctive data addresses, distinctive callees) over an address's dumps."""
    g: set[int] = set()
    j: set[int] = set()
    for d in dumps:
        g |= {
            a for a in d.formed if df.get(a, 0) <= df_max and RAM_LO <= a <= RAM_HI
        }
        j |= {x for x in trusted_jals(d) if cdf.get(x, 0) <= CALLEE_DF_MAX}
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
            {d.image.split(".")[0].split(" ")[0] for d in dumps},
            [extent_of(d) for d in dumps],
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
    `world-map.md` and `menus.md` are two.
    """
    docs = REPO / "docs"
    if not docs.is_dir():
        return []
    head_re = re.compile(r"^#{2,4}\s+(.*)$")
    row_re = re.compile(r"^\|\s*`?(?:FUN_)?([0-9a-fA-F]{8})`?\s*\|")
    sites: dict[str, dict[str, list[str]]] = defaultdict(lambda: defaultdict(list))
    for page in sorted(docs.rglob("*.md")):
        rel = str(page.relative_to(REPO))
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
            for a in found:
                if a in by_addr:
                    sites[a][rel].append(f"{rel}:{lineno}")

    out: list[Finding] = []
    for addr, per_page in sites.items():
        pages = sorted(per_page)
        if len(pages) < 2:
            continue
        toks = {p: {t for t in re.split(r"[^a-z0-9]+", Path(p).stem.lower())
                    if len(t) >= 3} for p in pages}
        # Only unrelated page names are a contradiction; a subsystem section
        # plus its directory row is one claim filed twice by design.
        if not any(
            not (toks[a] & toks[b])
            for i, a in enumerate(pages)
            for b in pages[i + 1:]
        ):
            continue
        lines = [
            f"FUN_{addr} carries a defining description on {len(pages)} docs "
            f"pages with unrelated names; at most one can be right:"
        ]
        for pg in pages:
            lines.append("    " + ", ".join(per_page[pg]))
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
    """
    claims = elsewhere_claims(tags, by_addr, df, cdf)
    per_file: dict[str, set[str]] = defaultdict(set)
    for t in tags:
        if t.addr in by_addr:
            per_file[t.file].add(t.addr)

    out: list[Finding] = []
    for rel, addrs in sorted(per_file.items()):
        if len(addrs) < ORPHAN_MIN_SIBLINGS:
            continue
        feats = {
            a: features(by_addr[a], df, cdf, DISTINCTIVE_DF_MAX) for a in addrs
        }
        # Only meaningful if the module is cohesive to begin with: at least one
        # sibling pair must corroborate, or "shares nothing" says nothing.
        pairs = [
            (a, b)
            for i, a in enumerate(sorted(addrs))
            for b in sorted(addrs)[i + 1:]
        ]
        # Calling a sibling is the strongest corroboration there is, and the
        # feature-set intersection misses it entirely: the callee is the
        # sibling's own address, never a shared third party. `save_select.rs`
        # read as having an orphan that calls four of its own siblings.
        def linked(a: str, b: str) -> bool:
            if feats[a][0] & feats[b][0] or feats[a][1] & feats[b][1]:
                return True
            ia, ib = int(a, 16), int(b, 16)
            return any(ib in d.jals for d in by_addr[a]) or any(
                ia in d.jals for d in by_addr[b]
            )

        cohesive = {x for a, b in pairs if linked(a, b) for x in (a, b)}
        if len(cohesive) < 2:
            continue
        for a in sorted(addrs - cohesive):
            g, j = feats[a]
            if not g and not j:
                continue  # nothing to contradict - a stub or an unparsed dump
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
                f"this module ({', '.join(sorted(cohesive))}).",
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
