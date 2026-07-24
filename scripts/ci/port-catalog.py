#!/usr/bin/env python3
"""Port-catalog tracker: per-address status across decompilation and engine port.

Generates a unified per-function table with three independent status columns:

  - dumped     : a Ghidra decompiler dump exists under `ghidra/scripts/funcs/`
  - documented : the address is cited from at least one file under `docs/`
  - ported     : the address appears in a `// PORT: FUN_<addr>` tag in a Rust
                 source file under `crates/`. The tag is the source of truth -
                 grep-by-mention is too noisy because addresses get cited as
                 "not yet ported" or "inspired by" in port-progress comments.

Reuses helpers from `function-coverage.py` where useful: same dump-file naming
convention, same address regex shape (SCUS 0x80010000-0x8006FFFF and overlays
0x801C0000-0x8020FFFF).

PORT tag format (Rust source files under crates/). The scanner accepts the
tag inside any of three comment forms:

    // PORT: FUN_801dd35c                  -- plain line comment
    /// PORT: FUN_801dd35c                  -- outer doc-comment (attaches to next item)
    //! PORT: FUN_801dd35c, FUN_801cf244    -- inner doc-comment (module-level)

The dominant convention in this codebase is the `//!` module-level form: each
crate file leads with a doc block that lists every dispatcher / top-level
function ported into that module on one (or several) `//! PORT:` line(s). The
catalog counts each distinct `FUN_<addr>` only once across all tag forms, so a
quick sanity check is:

    grep -rEi '//[/!]?\\s*PORT\\s*:' crates/ | grep -oEi 'FUN_8[0-9a-f]{7}' | sort -u | wc -l

A plain `grep -r "// PORT:"` under-counts dramatically - most tags live in
`//!` doc blocks that don't contain the literal substring `"// "`.

Trailing context after the address list is allowed:

    //! PORT: FUN_801dd35c (sub-mode jump table)

Addresses are matched strictly to the SCUS/overlay range (0x80010000-0x8006FFFF
and 0x801C0000-0x8020FFFF) so unrelated text in the tail of a tag won't
false-positive.

Usage:
    python3 scripts/ci/port-catalog.py                       # build global catalog
    python3 scripts/ci/port-catalog.py --missing-ports       # dumped+documented, not ported
    python3 scripts/ci/port-catalog.py --missing-dumps       # cited but not dumped
    python3 scripts/ci/port-catalog.py --md                  # markdown table to stdout
    python3 scripts/ci/port-catalog.py --top 30              # cap rows
    python3 scripts/ci/port-catalog.py --addr 801dd35c       # single-row drill-down
    python3 scripts/ci/port-catalog.py --dashboard           # open-work rollup (single page)

Output (default): target/port-catalog/catalog.csv + catalog.md
"""

import argparse
import csv
import re
import sys
import tomllib
from collections import Counter, defaultdict, deque
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"
DOCS_DIR = REPO / "docs"
CRATES_DIR = REPO / "crates"
OUT_DIR = REPO / "target" / "port-catalog"
FEATURES_TOML = REPO / "scripts" / "ci" / "features.toml"
IGNORE_TOML = REPO / "scripts" / "ci" / "port-catalog-ignore.toml"

# Address ranges that correspond to executable code:
#   SCUS_942.54   : 0x80010000 - 0x8006FFFF
#   Overlays      : 0x801C0000 - 0x8020FFFF
# Match the same shape function-coverage.py uses so the two tools share a worldview.
# IGNORECASE so PORT tags written `FUN_801DD35C` (uppercase) match as cleanly
# as the lowercase form Ghidra emits.
CODE_ADDR_RE = re.compile(r"80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4}", re.IGNORECASE)

# Addresses scraped from a `// PORT:` tag's tail. Only two token shapes count
# as a port claim:
#
#   FUN_<addr>                      -- the canonical Ghidra function name
#   overlay_<label>_<addr>          -- a funcs/ dump-file stem for an
#                                      overlay-resident function (some tags
#                                      cite the dump stem when the bare VA is
#                                      aliased across overlays)
#
# Bare hex in the tag's prose does NOT count: tags routinely name data globals
# (``PORT: the `_DAT_801F0204 = N` writes in FUN_801DD35C``) and interior
# address *ranges* (``PORT: FUN_801D9E1C (rate shifts, 0x801da1b8..0x801da200)``),
# and a loose match turned those into phantom "ported but not dumped"
# provenance-gap rows - range endpoints and data globals have no function dump.
# The function named on the same line (`FUN_...`) is still picked up, so the
# real port credit is unaffected.
PORT_ADDR_RE = re.compile(
    r"(?:FUN_|overlay_[0-9a-zA-Z_]+?_)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# Citations of an address by some other piece of text. Covers Ghidra's auto-named
# call forms plus raw disassembly forms. Matches `function-coverage.py` with one
# addition: a negative lookbehind on `PTR_` so we don't false-positive on
# `PTR_FUN_<addr>` symbols, which Ghidra emits for data-pointer tables that
# happen to contain function pointers - the address there is a *table base*,
# not a function entry. Same negative lookbehind on `_DAT_` for completeness.
CITATION_RE = re.compile(
    r"(?<!PTR_)(?<!_DAT_)(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
    r"(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})"
)

# Doc-side citation. More permissive: doc text uses `FUN_8003EBE4` (caps),
# bare `0x8003e4e8` forms, and backtick-wrapped bare addresses like
# `` `80056678` `` (the shape `docs/reference/functions.md` uses in table
# rows). The backtick form requires an OPENING backtick so we don't false-
# positive on every hex literal in the SCUS range that happens to occur in
# tables or paragraphs.
DOC_CITATION_RE = re.compile(
    r"(?:FUN_|0x|`)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# // PORT: FUN_801dd35c [, FUN_xxx]*  --  the only signal we trust for "ported".
# Accepts plain `//`, doc `//!`, and outer-doc `///` so the tag can live inside
# a rustdoc block (where the provenance is co-located with the human-readable
# description) or as a standalone comment.
PORT_TAG_RE = re.compile(
    r"//[/!]?\s*PORT\s*:\s*(.*)",
    re.IGNORECASE,
)


def collect_dumped() -> dict[str, str]:
    """Return {addr: dump_filename_stem} for every dump under ghidra/scripts/funcs/.

    Band-filtered to `CODE_ADDR_RE`, which is what makes this symmetric with the
    citation regexes. Without the filter the stem match admits the deliberately
    named `data_<addr>_DAT_<addr>_*.txt` data-region dumps, whose addresses sit
    outside the code band (`0x8007xxxx`) and therefore can never be matched by
    `DOC_CITATION_RE`. Those rows then read as "dumped but never documented" in
    perpetuity - unclosable by construction, because the address is a data table
    rather than a function entry and no amount of prose about it can close the
    row. `DAT_8007326c` (the TMD per-mode descriptor table) is the worked
    example: cited from `docs/formats/tmd.md` and from `CLAUDE.md`, yet it sat
    in the undocumented worklist.
    """
    out: dict[str, str] = {}
    if not FUNCS_DIR.exists():
        return out
    for p in FUNCS_DIR.glob("*.txt"):
        m = re.search(r"([0-9a-fA-F]{8})\.txt$", p.name)
        if m:
            addr = m.group(1).lower()
            if not CODE_ADDR_RE.fullmatch(addr):
                continue
            # Prefer overlay dumps over SCUS-named bare dumps if both exist.
            stem = p.stem
            if addr not in out or "overlay_" in stem:
                out[addr] = stem
    return out


def collect_citations() -> tuple[Counter, dict[str, list[str]]]:
    """Walk every dump and count citations of every code-range address.

    Returns (refs counter, sources map). `sources[addr]` is the list of dump
    filename stems that mention `addr` - useful for traceability columns.
    """
    refs: Counter = Counter()
    sources: dict[str, list[str]] = defaultdict(list)
    if not FUNCS_DIR.exists():
        return refs, sources
    for p in sorted(FUNCS_DIR.glob("*.txt")):
        if (
            p.name.endswith("_unique_index.txt")
            or p.name.endswith("_index.txt")
            or p.name.endswith("_survey.txt")
        ):
            continue
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        seen: set[str] = set()
        for m in CITATION_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen:
                continue
            seen.add(a)
            refs[a] += 1
            sources[a].append(p.stem)
    return refs, sources


def collect_doc_citations() -> dict[str, set[str]]:
    """Return {addr: set(doc_paths)} for every code-range address cited in docs/."""
    out: dict[str, set[str]] = defaultdict(set)
    if not DOCS_DIR.exists():
        return out
    for p in DOCS_DIR.rglob("*.md"):
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        seen: set[str] = set()
        for m in DOC_CITATION_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen:
                continue
            seen.add(a)
            rel = str(p.relative_to(REPO))
            out[a].add(rel)
    return out


def collect_ports() -> dict[str, set[str]]:
    """Return {addr: set(crate_names)} for every Rust `// PORT: FUN_<addr>` tag."""
    out: dict[str, set[str]] = defaultdict(set)
    if not CRATES_DIR.exists():
        return out
    for p in CRATES_DIR.rglob("*.rs"):
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        # Crate name is the first path segment under crates/.
        try:
            rel = p.relative_to(CRATES_DIR)
        except ValueError:
            continue
        crate = rel.parts[0] if rel.parts else "?"
        for line in text.splitlines():
            tag = PORT_TAG_RE.search(line)
            if not tag:
                continue
            # Inside the tail of the tag, pick up every code-range *function*
            # address - excluding `_DAT_` / `PTR_` data globals the prose names.
            for m in PORT_ADDR_RE.finditer(tag.group(1)):
                addr = m.group(1).lower()
                out[addr].add(crate)
    return out


# ---------------------------------------------------------------------------
# Reachability axis ("live")
#
# `ported` is a provenance marker: it says a Rust function claims to implement
# a Ghidra function. It says nothing about whether that Rust function ever
# runs. `live` is the second axis - is the ported symbol reachable, through
# non-test code, from a declared host entry point?
#
# The analysis is a name-resolved call graph over `crates/**/src/**.rs`. It is
# deliberately *over*-approximating: an unresolvable call links to every
# in-tree definition sharing the callee's name. Over-approximation biases the
# result toward "reachable", which keeps the not-live list conservative - the
# addresses it reports are the ones no plausible edge could reach.
# ---------------------------------------------------------------------------

# Directories under a crate that are not host code. Test/bench/example targets
# are exactly the callers a `NOT WIRED` tag means to exclude.
NON_HOST_CRATE_DIRS = {"tests", "benches", "examples"}

CFG_TEST_RE = re.compile(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]")
TEST_ATTR_RE = re.compile(r"#\[\s*(?:\w+\s*::\s*)*test\s*\]")
WASM_EXPORT_RE = re.compile(r"#\[\s*wasm_bindgen")
FN_DEF_RE = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
IMPL_KW_RE = re.compile(r"\bimpl\b")
TRAIT_DEF_RE = re.compile(r"\btrait\s+([A-Za-z_]\w*)")
# Traits whose methods are invoked by an external crate rather than by any
# in-tree call site. winit's `event_loop.run_app(&mut app)` takes the app by
# value and then calls `window_event` / `about_to_wait` / `resumed` on it, so
# no in-tree edge names those methods and the whole per-frame GUI tree below
# them - redraw, input, HUD build - reads as unreachable. Their bodies are
# entry points in the same sense `fn main` is, so they join the root set.
EXTERNAL_DISPATCH_TRAITS = frozenset({"ApplicationHandler"})
QUAL_CALL_RE = re.compile(r"\b([A-Za-z_]\w*)\s*::\s*([A-Za-z_]\w*)")
# A method *call*, not a field access: `.foo(` and not `.foo`. Without the
# trailing paren every struct field named `id` / `step` / `phase` links to
# every method of that name, which was enough to make an entire inert module
# look reachable.
METHOD_CALL_RE = re.compile(r"\.\s*([A-Za-z_]\w*)\s*\(")
BARE_CALL_RE = re.compile(r"(?<![.\w])([A-Za-z_]\w*)\s*\(")
IDENT_RE = re.compile(r"(?<![.\w])([A-Za-z_]\w*)\b")
# A `:` that is a field separator, not the `::` of a path. Used by the strict
# graph to drop struct fields from the bare-identifier (function-value) edge.
FIELD_COLON_RE = re.compile(r"\s*:(?!:)")
# The disclosure marker is written in caps by convention (`NOT WIRED:` /
# `**NOT WIRED**`). Matching case-insensitively pulls in ordinary prose - one
# module doc says "Consumers (not wired here):" while describing retail, which
# is a statement about the *game*, not a disclosure about the port.
NOT_WIRED_RE = re.compile(r"NOT\s+WIRED")
# A module-wide disclosure has to *open* a doc line to count. Some module docs
# mention the marker while describing where the per-item notes live ("Individual
# items carry a `NOT WIRED:` note"), and reading that as a blanket disclosure
# would suppress every real gap in the file.
# The leading run allows the three spellings a module disclosure is written
# in: `//! NOT WIRED:`, `//! **NOT WIRED**` and the markdown-heading form
# `//! # NOT WIRED`. Only `#`, `*` and space may precede it, so a sentence
# that merely mentions the marker still does not count as a blanket
# disclosure.
MODULE_NOT_WIRED_RE = re.compile(r"^\s*//!\s*[#*\s]*NOT\s+WIRED", re.MULTILINE)
NEXT_ITEM_RE = re.compile(
    r"^\s*(?:pub\s*(?:\([^)]*\)\s*)?)?"
    r"(?:default\s+|const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*"
    r"(fn|struct|enum|union|trait|impl)\b\s*(?:<[^>]*>\s*)?([A-Za-z_]\w*)?"
)
COMMENT_OR_ATTR_RE = re.compile(r"^\s*(?://|#\[|#!\[|\]|\s*$)")

RUST_KEYWORDS = frozenset(
    """as async await box break const continue crate do dyn else enum extern false final fn
    for if impl in let loop macro match mod move mut override priv pub ref return self Self
    static struct super trait true try type typeof union unsafe unsized use virtual where
    while yield""".split()
)


def strip_rust_noise(text: str) -> str:
    """Blank out comments and string / char literals, preserving byte offsets.

    Offsets and newlines survive so line numbers computed on the stripped text
    still index the original. Handles nested `/* */`, raw strings (`r#".."#`)
    and distinguishes char literals from lifetimes.
    """
    out = list(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if text[j] == "/" and j + 1 < n and text[j + 1] == "*":
                    depth += 1
                    j += 2
                elif text[j] == "*" and j + 1 < n and text[j + 1] == "/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif c == "r" and i + 1 < n and text[i + 1] in '"#':
            m = re.match(r'r(#*)"', text[i:])
            if not m:
                i += 1
                continue
            close = '"' + m.group(1)
            j = text.find(close, i + m.end())
            j = n if j < 0 else j + len(close)
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif c == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                for k in range(i, i + m.end()):
                    out[k] = " "
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def match_brace(s: str, start: int) -> int:
    """`start` indexes a `{`. Return the index just past its matching `}`."""
    depth, i, n = 0, start, len(s)
    while i < n:
        if s[i] == "{":
            depth += 1
        elif s[i] == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


class RustFn:
    """One `fn` definition with a body, plus where it sits."""

    __slots__ = (
        "uid",
        "crate",
        "path",
        "name",
        "impl_type",
        "def_pos",
        "body_start",
        "body_end",
        "line",
        "is_test",
        "is_trait_default",
    )

    def __init__(self, **kw):
        for k, v in kw.items():
            setattr(self, k, v)


def _impl_spans(stripped: str) -> list[tuple[int, int, str, str | None]]:
    """`(body_start, body_end, type_name, trait_name)` for every `impl` block.

    `trait_name` is `None` for an inherent `impl Ty { }` and the trait's name
    for `impl Trait for Ty { }`. It is what lets the root set pick out the
    externally-dispatched callback traits.
    """
    spans: list[tuple[int, int, str, str | None]] = []
    n = len(stripped)
    for m in IMPL_KW_RE.finditer(stripped):
        i, angle, header_end = m.end(), 0, None
        while i < n:
            ch = stripped[i]
            if ch == "<":
                angle += 1
            elif ch == ">":
                angle -= 1
            elif ch == "{" and angle <= 0:
                header_end = i
                break
            elif ch == ";" and angle <= 0:
                break
            i += 1
        if header_end is None:
            continue
        header = stripped[m.end() : header_end]
        for _ in range(3):  # peel nested generic args
            header = re.sub(r"<[^<>]*>", " ", header)
        header = header.split(" where ")[0]
        tr = None
        if " for " in header:
            tr, ty = header.split(" for ", 1)
            tr = tr.strip().split("::")[-1].strip()
            tr = (re.split(r"\W", tr)[0] or None) if tr else None
        else:
            ty = header
        ty = ty.strip().split("::")[-1].strip()
        ty = re.split(r"\W", ty)[0] if ty else ""
        spans.append((header_end, match_brace(stripped, header_end), ty or "?", tr))
    return spans


def _trait_spans(stripped: str) -> list[tuple[int, int, str]]:
    """`(body_start, body_end, trait_name)` for every `trait Name { }` block.

    A default method in a trait body is a real call target: hosts that do not
    override it run exactly this code. Without these spans such a function has
    `impl_type = None`, lands among the free functions, and is never matched
    by a `.name(...)` call site - so it shows zero in-edges no matter how many
    hosts run it.
    """
    spans: list[tuple[int, int, str]] = []
    n = len(stripped)
    for m in TRAIT_DEF_RE.finditer(stripped):
        i, angle, header_end = m.end(), 0, None
        while i < n:
            ch = stripped[i]
            if ch == "<":
                angle += 1
            elif ch == ">":
                angle -= 1
            elif ch == "{" and angle <= 0:
                header_end = i
                break
            elif ch == ";" and angle <= 0:
                break
            i += 1
        if header_end is None:
            continue
        spans.append((header_end, match_brace(stripped, header_end), m.group(1)))
    return spans


def _cfg_test_spans(stripped: str) -> list[tuple[int, int]]:
    """Byte spans of every `#[cfg(test)] mod ... { }` block."""
    spans: list[tuple[int, int]] = []
    n = len(stripped)
    for m in CFG_TEST_RE.finditer(stripped):
        i = m.end()
        while i < n and stripped[i] not in "{;":
            i += 1
        if i < n and stripped[i] == "{":
            spans.append((m.start(), match_brace(stripped, i)))
    return spans


def _innermost_impl(spans: list[tuple], pos: int) -> str | None:
    best: tuple[int, str] | None = None
    for start, end, ty, *_ in spans:
        if start <= pos < end and (best is None or start > best[0]):
            best = (start, ty)
    return best[1] if best else None


class RustSource:
    """Parsed view of one Rust file: functions, spans, and export markers."""

    def __init__(self, path: Path, crate: str, text: str):
        self.path = path
        self.crate = crate
        self.raw = text
        self.stripped = strip_rust_noise(text)
        self.is_test_file = path.stem == "tests" or "tests" in path.parts
        self.impl_spans = _impl_spans(self.stripped)
        self.trait_spans = _trait_spans(self.stripped)
        self.test_spans = _cfg_test_spans(self.stripped)
        self.line_starts = [0]
        for i, ch in enumerate(text):
            if ch == "\n":
                self.line_starts.append(i + 1)
        self.fns: list[RustFn] = []
        self._scan_fns()

    def line_of(self, pos: int) -> int:
        lo, hi = 0, len(self.line_starts) - 1
        while lo < hi:
            mid = (lo + hi + 1) // 2
            if self.line_starts[mid] <= pos:
                lo = mid
            else:
                hi = mid - 1
        return lo + 1

    def pos_of_line(self, line: int) -> int:
        idx = min(max(line - 1, 0), len(self.line_starts) - 1)
        return self.line_starts[idx]

    def _scan_fns(self) -> None:
        s, n = self.stripped, len(self.stripped)
        for m in FN_DEF_RE.finditer(s):
            i, pdepth, angle, body_start = m.end(), 0, 0, None
            while i < n:
                ch = s[i]
                if ch in "([":
                    pdepth += 1
                elif ch in ")]":
                    pdepth -= 1
                elif pdepth <= 0 and ch == "<":
                    angle += 1
                elif pdepth <= 0 and ch == ">":
                    angle -= 1
                elif pdepth <= 0 and ch == "{":
                    body_start = i
                    break
                elif pdepth <= 0 and ch == ";":
                    break
                i += 1
            if body_start is None:
                continue  # trait / extern declaration - no body to walk
            # `mod tests;` in a separate file: the `#[cfg(test)]` lives at the
            # declaration site, not in the file, so the file needs naming.
            is_test = self.is_test_file or any(
                a <= m.start() < b for a, b in self.test_spans
            )
            if not is_test and TEST_ATTR_RE.search(
                s[max(0, m.start() - 400) : m.start()]
            ):
                is_test = True
            # A trait body's default method carries the trait's name as its
            # `impl_type`, so `.name(...)` and `Trait::name` both resolve to
            # it. An `impl` block nested inside the trait body still wins.
            impl_ty = _innermost_impl(self.impl_spans, m.start())
            trait_ty = _innermost_impl(self.trait_spans, m.start())
            is_trait_default = impl_ty is None and trait_ty is not None
            self.fns.append(
                RustFn(
                    uid=-1,
                    crate=self.crate,
                    path=self.path,
                    name=m.group(1),
                    impl_type=impl_ty or trait_ty,
                    is_trait_default=is_trait_default,
                    def_pos=m.start(),
                    body_start=body_start,
                    body_end=match_brace(s, body_start),
                    line=self.line_of(m.start()),
                    is_test=is_test,
                )
            )

    def enclosing_fn(self, pos: int) -> RustFn | None:
        best: RustFn | None = None
        for f in self.fns:
            if f.body_start <= pos < f.body_end and (
                best is None or f.body_start > best.body_start
            ):
                best = f
        return best

    def next_item_after(self, line: int) -> tuple[str, str, int] | None:
        """The item a `///` / `//` tag on `line` (1-based) attaches to.

        Returns `(kind, name, item_line)` where kind is one of `fn` / `struct` /
        `enum` / `union` / `trait` / `impl`, or `None` if the next code line is
        not an item header (a `let`, a `use`, a match arm - a loose tag).
        """
        lines = self.raw.splitlines()
        for offset in range(line, min(line + 60, len(lines))):
            raw_line = lines[offset]
            if COMMENT_OR_ATTR_RE.match(raw_line):
                continue
            m = NEXT_ITEM_RE.match(raw_line)
            if not m:
                return None
            kind, name = m.group(1), m.group(2)
            if kind == "impl":
                pos = self.pos_of_line(offset + 1) + len(raw_line)
                ty = _innermost_impl(self.impl_spans, min(pos, len(self.stripped) - 1))
                return ("impl", ty or (name or "?"), offset + 1)
            return (kind, name or "?", offset + 1)
        return None

    def fn_at_line(self, line: int) -> RustFn | None:
        return next((f for f in self.fns if f.line == line), None)


def _crate_of(path: Path) -> tuple[str, bool]:
    """Return `(crate_name, is_host_code)` for a file under `crates/`."""
    rel = path.relative_to(CRATES_DIR)
    crate = rel.parts[0] if rel.parts else "?"
    sub = rel.parts[1] if len(rel.parts) > 1 else ""
    return crate, sub not in NON_HOST_CRATE_DIRS


def load_rust_sources() -> dict[Path, RustSource]:
    out: dict[Path, RustSource] = {}
    if not CRATES_DIR.exists():
        return out
    for p in sorted(CRATES_DIR.rglob("*.rs")):
        crate, is_host = _crate_of(p)
        if not is_host:
            continue
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        out[p] = RustSource(p, crate, text)
    return out


def _is_bin_target(path: Path) -> bool:
    rel = path.relative_to(CRATES_DIR)
    return "bin" in rel.parts or rel.parts[-1] == "main.rs"


def collect_roots(srcs: dict[Path, RustSource]) -> list[RustFn]:
    """The declared host entry points the reachability BFS starts from.

    Four families, and nothing else - a `pub fn` that no host reaches is
    exactly the inert-port case this axis exists to find:

    1. `fn main` in a `[[bin]]` target (`src/bin/**` or `src/main.rs`). Covers
       every CLI subcommand, the `legaia-engine` window loop and its `World`
       tick / render entry, and the `asset-viewer` GUI.
    2. `#[wasm_bindgen]` exports in the WASM crates - the browser's entry
       points into the static site's viewer / play / patcher pages.
    3. Methods of an `impl <ExternalDispatchTrait> for T` block. `fn main` does
       reach the loop *setup* - `cmd_play_window` builds the app - but the
       chain dies at `event_loop.run_app(&mut app)`, because winit calls back
       into `impl ApplicationHandler` from outside the tree. Without these the
       whole per-frame, redraw and input surface reads as inert.
    4. Anything reachable from those, transitively.
    """
    roots: list[RustFn] = []
    for path, src in srcs.items():
        if _is_bin_target(path):
            roots += [f for f in src.fns if f.name == "main" and not f.is_test]
        for a, b, _ty, tr in src.impl_spans:
            if tr in EXTERNAL_DISPATCH_TRAITS:
                roots += [
                    f for f in src.fns if a <= f.def_pos < b and not f.is_test
                ]
        for m in WASM_EXPORT_RE.finditer(src.stripped):
            impl_hit = next(
                (
                    (a, b)
                    for a, b, *_ in src.impl_spans
                    if 0 <= a - m.end() < 200 and src.raw.find("impl", m.end(), a) != -1
                ),
                None,
            )
            if impl_hit:
                a, b = impl_hit
                roots += [f for f in src.fns if a <= f.def_pos < b and not f.is_test]
                continue
            item = src.next_item_after(src.line_of(m.start()))
            if item and item[0] == "fn":
                nxt = src.fn_at_line(item[2])
                if nxt is not None and not nxt.is_test:
                    roots.append(nxt)
    return roots


def build_rust_graph(
    srcs: dict[Path, RustSource],
    strict: bool = False,
) -> tuple[list[RustFn], dict[int, set[int]]]:
    """Name-resolved forward call graph over every non-test `fn` body.

    Resolution order per call site, most to least specific:

    - `Qual::name` - matched against `impl Qual` methods, then against a module
      (file stem) named `Qual`, then falls back to the bare name.
    - `.name(...)` - matched against every in-tree method of that name. Receiver
      types are not inferred, so this is the main over-approximation.
    - `name(...)` - a free call; matched against every in-tree `fn name`.
    - bare `name` not followed by `(` - matched against **free** functions only,
      which is how a function value reaches `map` / `sort_by_key` / a struct
      field holding a callback. Methods are deliberately excluded here: a local
      binding named `new` or a field named `id` would otherwise link to every
      `Type::new` / `Type::id` in the workspace.

    ## The two graphs, and why sharpening this one in place would be wrong

    `strict=False` (the default) is the permissive graph described above, and it
    is what every `live` / `--not-live` verdict is read off. Its
    over-approximation is load-bearing: biasing every ambiguity toward
    "reachable" is what makes the not-live list a hard floor, so an address it
    calls inert is one no plausible edge could reach.

    That same bias is wrong for the opposite question. `--live-audit`'s "tagged
    `NOT WIRED` but analysed live" section asks whether a disclosure is stale,
    and there a spurious edge manufactures a false accusation: one
    `session.tick(...)` reaching all 63 in-tree `tick`, one `new(` reaching all
    192 `new`. Measured over the whole tree, 54 of 55 rows in that section were
    false edges rather than stale tags.

    So `strict=True` builds a *second* graph with the two sharpenings that
    ambiguity allows, and only the stale-tag test reads it. Nothing is traded:
    each direction consults the graph whose error mode is safe for it. (An
    earlier attempt applied the receiver gate to the single shared graph and was
    reverted for exactly the reason above - it is the sharing, not the gate,
    that was the mistake.)

    The two sharpenings, both of which only ever *remove* edges:

    1. **Struct fields are not function values.** An identifier immediately
       followed by `:` - and not `::` - is a field declaration or a struct
       literal key, so it cannot be a function value reaching `map`. Without
       this the field `stat_deltas` links to a free `stat_deltas` in another
       crate.
    2. **A receiver gate on ambiguous method edges.** A `.name(...)` or
       `name(...)` edge onto an `impl Type` method survives only if the calling
       file names `Type` at all, or defines the method itself. A file that never
       writes `EscapeTimer` cannot be calling `EscapeTimer::tick`. Free
       functions and `Qual::name` edges are untouched - they are already
       resolved.
    """
    fns: list[RustFn] = []
    for src in srcs.values():
        fns.extend(src.fns)
    for i, f in enumerate(fns):
        f.uid = i

    by_name: dict[str, list[RustFn]] = defaultdict(list)
    by_qual: dict[tuple[str, str], list[RustFn]] = defaultdict(list)
    by_mod: dict[tuple[str, str], list[RustFn]] = defaultdict(list)
    for f in fns:
        by_name[f.name].append(f)
        if f.impl_type:
            by_qual[(f.impl_type, f.name)].append(f)
        by_mod[(f.path.stem, f.name)].append(f)
    methods_by_name: dict[str, list[RustFn]] = defaultdict(list)
    free_by_name: dict[str, list[RustFn]] = defaultdict(list)
    for f in fns:
        (methods_by_name if f.impl_type else free_by_name)[f.name].append(f)
        # A trait default method is reachable both ways: as `.name(...)` on a
        # host that does not override it, and - since it was a free function
        # to every earlier pass - through the bare-identifier edge. Listing it
        # in both tables is purely additive, so it cannot introduce a new
        # false negative.
        if f.is_trait_default:
            free_by_name[f.name].append(f)

    # Note on a rule that was tried and removed: restricting `.method(` edges
    # to types the calling crate names by hand. It does not help - the
    # collisions that matter are intra-crate (`.tick()` inside `engine-core`
    # reaching `engine-core`'s own inert `SaveScreenMachine::tick`) - and it
    # trades away the graph's one useful property, that every ambiguity
    # resolves toward reachability. Keeping the graph purely
    # over-approximating is what makes `--not-live` a hard floor.

    # Receiver gate (strict only): the set of type names each file mentions
    # anywhere in its own stripped source. A method edge into `impl Type` is
    # kept only if the calling file names `Type`, or defines that method.
    named_types: dict[Path, set[str]] = {}
    if strict:
        all_impl_types = {f.impl_type for f in fns if f.impl_type}
        for p, s in srcs.items():
            words = set(IDENT_RE.findall(s.stripped))
            named_types[p] = words & all_impl_types

    def gated(caller: RustFn, cands: list[RustFn]) -> list[RustFn]:
        """Apply the receiver gate, but only where there is ambiguity to resolve.

        A method name with exactly one definition in the tree is already
        unambiguous, so the gate has nothing to decide and must not fire: the
        receiver is routinely a local binding whose type the calling file never
        spells (`ctrl.run_horizon_emitter(..)`), and gating on the spelling
        would drop a real edge. Dropping a real edge is the one failure mode
        this graph must not have - it converts a correct `NOT WIRED` disclosure
        into a silent omission from the audit.
        """
        if not strict or len(cands) < 2:
            return cands
        return [
            c
            for c in cands
            if not c.impl_type
            or c.path == caller.path
            or c.impl_type in named_types.get(caller.path, ())
        ]

    edges: dict[int, set[int]] = defaultdict(set)
    for src in srcs.values():
        for f in src.fns:
            body = src.stripped[f.body_start : f.body_end]
            targets: set[int] = set()
            consumed: set[tuple[int, int]] = set()
            for m in QUAL_CALL_RE.finditer(body):
                qual, name = m.group(1), m.group(2)
                consumed.add(m.span(2))
                if qual == "Self" and f.impl_type:
                    qual = f.impl_type
                # Fall back to *free* functions only, never to methods. An
                # unresolved qualifier is nearly always an external type
                # (`Vec::new`, `String::from`, `Duration::from_secs`), and
                # letting those reach every in-tree `Type::new` wires up whole
                # modules that nothing constructs.
                cands = (
                    by_qual.get((qual, name))
                    or by_mod.get((qual, name))
                    or free_by_name.get(name, [])
                )
                targets.update(c.uid for c in cands)
            for m in METHOD_CALL_RE.finditer(body):
                consumed.add(m.span(1))
                targets.update(
                    c.uid for c in gated(f, methods_by_name.get(m.group(1), []))
                )
            for m in BARE_CALL_RE.finditer(body):
                if m.span(1) in consumed or m.group(1) in RUST_KEYWORDS:
                    continue
                consumed.add(m.span(1))
                targets.update(c.uid for c in gated(f, by_name.get(m.group(1), [])))
            for m in IDENT_RE.finditer(body):
                if m.span(1) in consumed or m.group(1) in RUST_KEYWORDS:
                    continue
                # A field declaration (`name: Type`) or a struct-literal key
                # (`Foo { name: v }`) is not a function value. `::` is not a
                # field separator, so exclude it from the lookahead.
                if strict and FIELD_COLON_RE.match(body, m.end(1)):
                    continue
                targets.update(c.uid for c in free_by_name.get(m.group(1), []))
            targets.discard(f.uid)
            edges[f.uid] |= targets
    return fns, edges


def reachable_fns(
    fns: list[RustFn], edges: dict[int, set[int]], roots: list[RustFn]
) -> set[int]:
    seen = {r.uid for r in roots}
    queue: deque[int] = deque(seen)
    while queue:
        for nxt in edges.get(queue.popleft(), ()):
            if nxt not in seen:
                seen.add(nxt)
                queue.append(nxt)
    return seen


def _tag_comment_block(src: RustSource, line: int) -> str:
    """The contiguous comment / attribute run containing `line` (1-based)."""
    lines = src.raw.splitlines()
    idx = min(max(line - 1, 0), len(lines) - 1)
    lo = idx
    while lo > 0 and lines[lo - 1].lstrip().startswith(("//", "#[")):
        lo -= 1
    hi = idx
    while hi + 1 < len(lines) and lines[hi + 1].lstrip().startswith(("//", "#[")):
        hi += 1
    return "\n".join(lines[lo : hi + 1])


def _module_doc_block(src: RustSource) -> str:
    keep = []
    for line in src.raw.splitlines():
        s = line.lstrip()
        if s.startswith("//!") or not s:
            keep.append(line)
        elif keep:
            break
    return "\n".join(keep)


def collect_port_anchors(
    srcs: dict[Path, RustSource],
) -> dict[str, list[dict]]:
    """Map each PORT-tagged address to the Rust symbols that carry the tag.

    Anchor resolution, in order:

    1. `///` / `//` tag above a `fn` - the anchor is that **function**. The
       precise case, and the one worth trusting.
    2. `///` / `//` tag above a `struct` / `enum` / `impl` - the anchor is the
       **type**: live if any non-test method in that type's `impl` blocks is
       reachable.
    3. A tag written inside a function body - the anchor is the enclosing
       function.
    4. `//! PORT:` - a module-level claim, so the anchor is the **file**: live
       if any non-test `fn` in the file is reachable. This is the coarsest
       anchor and the main source of over-reporting.
    """
    anchors: dict[str, list[dict]] = defaultdict(list)
    for path, src in srcs.items():
        # A PORT tag written inside a unit test is commentary on what the test
        # covers, not a claim that a port site lives there. Counting them would
        # add a permanently-inert anchor per test.
        if src.is_test_file:
            continue
        rel = str(path.relative_to(REPO))
        for lineno, line in enumerate(src.raw.splitlines(), start=1):
            tag = PORT_TAG_RE.search(line)
            if not tag:
                continue
            addrs = {m.group(1).lower() for m in PORT_ADDR_RE.finditer(tag.group(1))}
            if not addrs:
                continue
            is_module_tag = line.lstrip().startswith("//!")
            kind, symbol, fn_uid, ty = "module", "(module)", None, None
            if not is_module_tag:
                item = src.next_item_after(lineno)
                if item and item[0] == "fn":
                    fn = src.fn_at_line(item[2])
                    if fn is not None and fn.is_test:
                        continue
                    if fn is not None:
                        kind, symbol, fn_uid = "fn", fn.name, fn.uid
                elif item and item[0] in ("struct", "enum", "union", "impl", "trait"):
                    kind, symbol, ty = "type", item[1], item[1]
                else:
                    enc = src.enclosing_fn(src.pos_of_line(lineno))
                    if enc is not None and enc.is_test:
                        continue
                    if enc is not None:
                        kind, symbol, fn_uid = "fn", enc.name, enc.uid
            block = (
                _module_doc_block(src)
                if is_module_tag
                else _tag_comment_block(src, lineno)
            )
            # A `//! NOT WIRED` opening the module doc covers every port site in
            # the file - `mdec::st_ring` declares the whole module inert once
            # and then tags seven addresses inside function bodies.
            disclosed = bool(NOT_WIRED_RE.search(block)) or bool(
                MODULE_NOT_WIRED_RE.search(_module_doc_block(src))
            )
            for addr in addrs:
                anchors[addr].append(
                    {
                        "kind": kind,
                        "crate": src.crate,
                        "file": rel,
                        "line": lineno,
                        "symbol": symbol,
                        "fn_uid": fn_uid,
                        "type_name": ty,
                        "path": path,
                        "not_wired_tag": disclosed,
                    }
                )
    return anchors


def compute_live(
    anchors: dict[str, list[dict]],
    srcs: dict[Path, RustSource],
    fns: list[RustFn],
    reach: set[int],
    reach_strict: set[int] | None = None,
) -> dict[str, dict]:
    """Resolve each anchor to live / not, then fold up to `{addr: ...}`.

    Address-level `live` is `any(anchor is live)`: one address can be ported
    into several crates, and it only takes one wired implementation for the
    retail behaviour to be on the frame path. The per-anchor verdicts stay on
    the row so `--live-audit` can report at the granularity a `NOT WIRED:` tag
    is actually written at.

    `reach_strict`, when supplied, is the same resolution run against the
    receiver-gated graph (`build_rust_graph(strict=True)`). It sets a second
    verdict, `live_strict`, which nothing but the stale-`NOT WIRED` test reads -
    see that function's docstring for why the two questions need two graphs.
    Absent it, `live_strict` mirrors `live`.
    """
    # A module anchor's scope is the file. But `foo.rs` next to a `foo/`
    # directory is often a pure module-declaration file with no `fn` of its
    # own - `engine-vm/src/field.rs` declares the field VM and holds none of
    # it. Reading such a file's `//! PORT:` block against an empty function
    # set reports the whole field VM inert, which is wrong. When the file
    # defines no non-test `fn`, widen the scope to its submodule subtree.
    def module_scope(reached: set[int]) -> dict[Path, bool]:
        out: dict[Path, bool] = {}
        for p, s in srcs.items():
            own = [f for f in s.fns if not f.is_test]
            if own:
                out[p] = any(f.uid in reached for f in own)
                continue
            # `lib.rs` / `main.rs` / `mod.rs` own the directory they sit in;
            # `foo.rs` owns the sibling `foo/`.
            sub = p.parent if p.stem in ("lib", "main", "mod") else p.parent / p.stem
            out[p] = any(
                f.uid in reached
                for q, s2 in srcs.items()
                if q.is_relative_to(sub)
                for f in s2.fns
                if not f.is_test
            )
        return out

    def type_scope(reached: set[int]) -> dict[tuple[Path, str], bool]:
        out: dict[tuple[Path, str], bool] = defaultdict(bool)
        for f in fns:
            if f.impl_type and not f.is_test and f.uid in reached:
                out[(f.path, f.impl_type)] = True
        return out

    module_live = module_scope(reach)
    type_live = type_scope(reach)
    strict_on = reach_strict is not None
    module_live_s = module_scope(reach_strict) if strict_on else module_live
    type_live_s = type_scope(reach_strict) if strict_on else type_live
    # Which types the file gives an `impl` block to at all. A tag on a plain
    # data struct - no `impl`, its behaviour in free functions or in an `impl`
    # of some *other* type in the same file - has no method for the rule above
    # to find, so it could never be live however wired the port is. Those fall
    # back to the file's module scope.
    typed_in_file: dict[Path, set[str]] = defaultdict(set)
    for p, s in srcs.items():
        for _a, _b, ty, _tr in s.impl_spans:
            typed_in_file[p].add(ty)
    def verdict(e: dict, reached: set[int], mods, types) -> bool:
        if e["kind"] == "fn":
            return e["fn_uid"] in reached
        if e["kind"] == "type":
            hit = types.get((e["path"], e["type_name"]), False)
            if not hit and e["type_name"] not in typed_in_file[e["path"]]:
                hit = mods.get(e["path"], False)
            return hit
        return mods.get(e["path"], False)

    out: dict[str, dict] = {}
    for addr, entries in anchors.items():
        for e in entries:
            e["live"] = verdict(e, reach, module_live, type_live)
            e["live_strict"] = (
                verdict(e, reach_strict, module_live_s, type_live_s)
                if strict_on
                else e["live"]
            )
        out[addr] = {
            "live": any(e["live"] for e in entries),
            "live_strict": any(e["live_strict"] for e in entries),
            "anchors": entries,
            "not_wired_tag": any(e["not_wired_tag"] for e in entries),
        }
    return out


def build_call_graph(sources: dict[str, list[str]]) -> dict[str, set[str]]:
    """Build forward call-graph edges: `{src_addr: set(cited_addr)}`.

    Each dump filename ends with an 8-hex-digit address that identifies the
    function the dump documents. For every (cited_addr, [src_files]) entry in
    `sources`, we derive an edge `src_file_addr -> cited_addr`.

    Note: edges only exist between addresses that are *dumped*. Undumped
    helpers have no outgoing edges - the BFS frontier widens only as more
    dumps land.
    """
    graph: dict[str, set[str]] = defaultdict(set)
    addr_re = re.compile(r"([0-9a-fA-F]{8})$", re.IGNORECASE)
    for cited_addr, src_files in sources.items():
        for src in src_files:
            m = addr_re.search(src)
            if not m:
                continue
            src_addr = m.group(1).lower()
            if src_addr == cited_addr:
                continue
            graph[src_addr].add(cited_addr)
    return graph


def bfs_reachable(
    graph: dict[str, set[str]],
    roots: list[str],
    stop_at: set[str] | None = None,
    max_depth: int | None = None,
) -> set[str]:
    """Return the set of addresses reachable from `roots` via citation edges.

    `stop_at` addresses are kept in the result but their callees aren't followed
    - useful for stopping at shared-infrastructure boundaries (e.g. the generic
    CD loader) so a feature filter doesn't pull in everything reachable.
    """
    visited: set[str] = set()
    stop_at = stop_at or set()
    queue: deque[tuple[str, int]] = deque()
    for r in roots:
        rl = r.lower()
        if rl not in visited:
            visited.add(rl)
            queue.append((rl, 0))
    while queue:
        addr, depth = queue.popleft()
        if addr in stop_at:
            continue
        if max_depth is not None and depth >= max_depth:
            continue
        for nxt in graph.get(addr, ()):
            if nxt not in visited:
                visited.add(nxt)
                queue.append((nxt, depth + 1))
    return visited


def load_features() -> dict[str, dict]:
    """Load feature definitions from scripts/ci/features.toml. Returns {} if missing."""
    if not FEATURES_TOML.exists():
        return {}
    with FEATURES_TOML.open("rb") as f:
        data = tomllib.load(f)
    out = {}
    for name, body in data.items():
        if not isinstance(body, dict):
            continue
        out[name] = {
            "description": body.get("description", ""),
            "roots": [r.lower() for r in body.get("roots", [])],
            "stop_at": {a.lower() for a in body.get("stop_at", [])},
            "max_depth": body.get("max_depth"),
        }
    return out


def load_ignore() -> dict[str, tuple[str, str]]:
    """Load `scripts/ci/port-catalog-ignore.toml`. Returns `{addr: (category, reason)}`.

    The TOML is organised as top-level tables (one per category) of
    `addr = "reason"` rows. Addresses across different categories merge into
    a single map; the category travels along for drill-down.
    """
    if not IGNORE_TOML.exists():
        return {}
    with IGNORE_TOML.open("rb") as f:
        data = tomllib.load(f)
    out: dict[str, tuple[str, str]] = {}
    for category, body in data.items():
        if not isinstance(body, dict):
            continue
        for addr, reason in body.items():
            out[addr.lower()] = (category, str(reason))
    return out


def build_rows(
    dumped: dict[str, str],
    refs: Counter,
    sources: dict[str, list[str]],
    docs: dict[str, set[str]],
    ports: dict[str, set[str]],
    ignore: dict[str, tuple[str, str]] | None = None,
    live: dict[str, dict] | None = None,
) -> list[dict]:
    """Union the four signals into a per-address row list, sorted by address."""
    addrs = set(dumped) | set(refs) | set(docs) | set(ports)
    ignore = ignore or {}
    live = live if live is not None else {}
    rows: list[dict] = []
    for addr in sorted(addrs):
        dump_stem = dumped.get(addr, "")
        is_dumped = bool(dump_stem)
        doc_hits = sorted(docs.get(addr, set()))
        port_crates = sorted(ports.get(addr, set()))
        is_documented = bool(doc_hits)
        is_ported = bool(port_crates)
        ig = ignore.get(addr)
        bucket = "scus" if int(addr, 16) < 0x801C0000 else "overlay"
        lv = live.get(addr)
        rows.append(
            {
                "addr": addr,
                "live": bool(lv and lv["live"]),
                "live_known": lv is not None,
                "not_wired_tag": bool(lv and lv["not_wired_tag"]),
                "anchors": lv["anchors"] if lv else [],
                "bucket": bucket,
                "dumped": is_dumped,
                "dump_source": dump_stem,
                "documented": is_documented,
                "doc_sources": doc_hits,
                "ported": is_ported,
                "port_crates": port_crates,
                "refs": refs.get(addr, 0),
                "first_sources": sources.get(addr, [])[:3],
                "ignored": ig is not None,
                "ignore_category": ig[0] if ig else "",
                "ignore_reason": ig[1] if ig else "",
            }
        )
    return rows


def yesno(b: bool) -> str:
    return "yes" if b else "-"


def render_csv(rows: list[dict], out_path: Path) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(
            [
                "addr",
                "bucket",
                "dumped",
                "documented",
                "ported",
                "live",
                "not_wired_tag",
                "ignored",
                "ignore_category",
                "ignore_reason",
                "port_crates",
                "doc_sources",
                "refs",
                "first_dump_sources",
            ]
        )
        for r in rows:
            w.writerow(
                [
                    r["addr"],
                    r["bucket"],
                    int(r["dumped"]),
                    int(r["documented"]),
                    int(r["ported"]),
                    int(r["live"]) if r["live_known"] else "",
                    int(r["not_wired_tag"]) if r["live_known"] else "",
                    int(r["ignored"]),
                    r["ignore_category"],
                    r["ignore_reason"],
                    "|".join(r["port_crates"]),
                    "|".join(r["doc_sources"]),
                    r["refs"],
                    "|".join(r["first_sources"]),
                ]
            )


def render_md(rows: list[dict], out_path: Path | None, title: str) -> str:
    with_live = any(r["live_known"] for r in rows)
    lines = [
        f"# {title}",
        "",
        "Generated by `scripts/ci/port-catalog.py`. Three independent status columns:",
        "",
        "- **dumped** - Ghidra decompiler output exists under `ghidra/scripts/funcs/`.",
        "- **documented** - the address is cited from at least one file under `docs/`.",
        "- **ported** - the address appears in a `// PORT: FUN_<addr>` tag in a Rust source under `crates/`.",
        "- **ignore** - address is listed in `scripts/ci/port-catalog-ignore.toml` as non-port-site (PsyQ / BIOS / libgte / ...).",
    ] + (
        [
            "- **live** - the Rust symbol carrying the `// PORT:` tag is reachable from a host entry point (`--live`).",
            "",
        ]
        if with_live
        else [
            "",
        ]
    ) + [
        "| addr | bucket | dumped | documented | ported (crates) |"
        + (" live |" if with_live else "")
        + " ignore | refs | first dump source |",
        "|---|---|---|---|---|" + ("---|" if with_live else "") + "---|---|---|",
    ]
    for r in rows:
        crates = ", ".join(r["port_crates"]) if r["port_crates"] else "-"
        first_src = r["first_sources"][0] if r["first_sources"] else "-"
        ignore_cell = r["ignore_category"] if r["ignored"] else "-"
        live_cell = (
            f" {yesno(r['live']) if r['live_known'] else '?'} |" if with_live else ""
        )
        lines.append(
            f"| `{r['addr']}` | {r['bucket']} | {yesno(r['dumped'])} | "
            f"{yesno(r['documented'])} | {crates} |{live_cell} {ignore_cell} | "
            f"{r['refs']} | `{first_src}` |"
        )
    md = "\n".join(lines) + "\n"
    if out_path:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(md)
    return md


def summarize(rows: list[dict]) -> str:
    n = len(rows)
    n_dumped = sum(1 for r in rows if r["dumped"])
    n_documented = sum(1 for r in rows if r["documented"])
    n_ported = sum(1 for r in rows if r["ported"])
    # Cross-cuts that the table is meant to surface
    dd = sum(1 for r in rows if r["dumped"] and r["documented"])
    ddp = sum(1 for r in rows if r["dumped"] and r["documented"] and r["ported"])
    dd_not_p = sum(
        1 for r in rows if r["dumped"] and r["documented"] and not r["ported"]
    )
    dd_not_p_ignored = sum(
        1
        for r in rows
        if r["dumped"] and r["documented"] and not r["ported"] and r["ignored"]
    )
    cited_not_dumped = sum(1 for r in rows if r["refs"] > 0 and not r["dumped"])
    ported_not_documented = sum(
        1 for r in rows if r["ported"] and not r["documented"]
    )
    ported_not_dumped = sum(1 for r in rows if r["ported"] and not r["dumped"])
    live_block: list[str] = []
    if any(r["live_known"] for r in rows):
        ported_rows = [r for r in rows if r["ported"]]
        n_live = sum(1 for r in ported_rows if r["live"])
        n_inert = len(ported_rows) - n_live
        pairs = [(r, a) for r in ported_rows for a in r["anchors"]]
        n_a_live = sum(1 for _, a in pairs if a["live"])
        n_a_disclosed = sum(
            1 for _, a in pairs if not a["live"] and a["not_wired_tag"]
        )
        n_a_undisclosed = sum(
            1 for _, a in pairs if not a["live"] and not a["not_wired_tag"]
        )
        # Stale-tag is the one question read off the receiver-gated graph:
        # a spurious edge here manufactures a false accusation against a
        # correct disclosure. See build_rust_graph.
        n_stale = sum(1 for _, a in pairs if a["live_strict"] and a["not_wired_tag"])
        live_block = [
            "",
            f"ported + live  (reachable from a host root)     : {n_live}",
            f"ported, NOT live (inert)                        : {n_inert}",
            "",
            f"PORT tag sites (anchors)                        : {len(pairs)}",
            f"  live                                          : {n_a_live}",
            f"  inert, `NOT WIRED:` tag present               : {n_a_disclosed}",
            f"  inert, no tag  -> disclosure gap              : {n_a_undisclosed}",
            f"  tagged NOT WIRED but analysed live (audit)    : {n_stale}",
        ]
    return "\n".join(
        [
            f"total addresses tracked       : {n}",
            f"dumped                        : {n_dumped}",
            f"documented (in docs/)         : {n_documented}",
            f"ported (// PORT: tag)         : {n_ported}",
            "",
            f"dumped + documented           : {dd}",
            f"dumped + documented + ported  : {ddp}",
            f"dumped + documented, NOT ported                 : {dd_not_p}",
            f"  of which ignored (PsyQ / BIOS / libgte / ...) : {dd_not_p_ignored}",
            f"  remaining port worklist                       : {dd_not_p - dd_not_p_ignored}",
            "",
            f"cited but NOT dumped  (dump worklist)           : {cited_not_dumped}",
            f"ported but NOT documented (provenance gap)      : {ported_not_documented}",
            f"ported but NOT dumped     (provenance gap)      : {ported_not_dumped}",
        ]
        + live_block
    )


def render_live_audit(rows: list[dict], out_path: Path | None) -> str:
    """The `--live-audit` report: every ported address that is not reachable.

    Three sections, in the order they need acting on:

    1. **Tagged `NOT WIRED` but analysed live** - either the tag is stale or the
       call graph invented an edge. Each row needs a human decision, so this
       section comes first.
    2. **Undisclosed inert ports** - not reachable, no tag. The disclosure gap.
    3. **Disclosed inert ports** - not reachable, tag present. Working as
       intended; listed so the wiring worklist is complete.
    """
    # A `NOT WIRED:` tag is written per *anchor*, so the audit has to compare
    # per anchor. Rolling up to the address first hides the case where an
    # address is ported twice, one copy wired and one not - which is the
    # normal shape for a formula shared between `engine-vm` and `engine-core`.
    pairs = [
        (r, a) for r in rows if r["ported"] and r["live_known"] for a in r["anchors"]
    ]
    stale = [(r, a) for r, a in pairs if a["live_strict"] and a["not_wired_tag"]]
    undisclosed = [(r, a) for r, a in pairs if not a["live"] and not a["not_wired_tag"]]
    disclosed = [(r, a) for r, a in pairs if not a["live"] and a["not_wired_tag"]]

    def table(title: str, subset: list[tuple[dict, dict]], note: str) -> list[str]:
        out = [f"## {title} ({len(subset)})", "", note, ""]
        if not subset:
            out += ["None.", ""]
            return out
        out += [
            "| addr | crate | anchor | symbol | site |",
            "|---|---|---|---|---|",
        ]
        for r, a in sorted(subset, key=lambda ra: (ra[0]["addr"], ra[1]["file"])):
            out.append(
                f"| `{r['addr']}` | {a['crate']} | {a['kind']} | "
                f"`{a['symbol']}` | `{a['file']}:{a['line']}` |"
            )
        out.append("")
        return out

    lines = [
        "# Live-reachability audit",
        "",
        "Generated by `scripts/ci/port-catalog.py --live-audit`. `live` means the "
        "Rust symbol carrying the `// PORT:` tag is reachable through non-test "
        "code from a declared host entry point - see "
        "[`docs/tooling/port-catalog.md`](../../docs/tooling/port-catalog.md) for "
        "the root set and the analysis's known false negatives.",
        "",
    ]
    lines += table(
        "Tagged `NOT WIRED` but analysed live",
        stale,
        "Either the tag is stale (the port got wired since) or the call graph "
        "resolved a name-collision edge that does not exist. Check by hand.",
    )
    lines += table(
        "Undisclosed inert ports",
        undisclosed,
        "Not reachable from any host root and carrying no `NOT WIRED:` tag. "
        "Each is either a wiring gap or a missing disclosure.",
    )
    lines += table(
        "Disclosed inert ports",
        disclosed,
        "Not reachable, and the source says so. The declared wiring worklist.",
    )
    md = "\n".join(lines) + "\n"
    if out_path:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(md)
    return md


def render_dashboard(
    all_rows: list[dict],
    sources: dict[str, list[str]],
    features: dict[str, dict],
    top_n: int,
    out_path: Path | None,
) -> str:
    """Generate the open-work dashboard: one regenerable page that combines
    per-feature port stats, per-feature top-N missing-ports, the ignore-list
    summary, and a pointer to the open-RE-threads index.

    The dashboard is the answer to "what's next" at a glance. It is generated
    output (target/port-catalog/open-work.md, gitignored) and re-runs cheaply.
    """
    rows_by_addr = {r["addr"]: r for r in all_rows}
    graph = build_call_graph(sources)

    lines: list[str] = [
        "# Open work - port-catalog dashboard",
        "",
        "Regenerated by `scripts/ci/port-catalog.py --dashboard`. Cross-references:",
        "",
        "- [`docs/reference/open-rev-eng-threads.md`](../../docs/reference/open-rev-eng-threads.md) - question-level open hunts (what is *unknown*; complements this page's per-function worklists).",
        "- [`docs/tooling/port-catalog.md`](../../docs/tooling/port-catalog.md) - tool usage + column semantics.",
        "- [`scripts/ci/features.toml`](../../scripts/ci/features.toml) - feature definitions (roots + stop_at boundaries).",
        "- [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml) - addresses excluded from the port worklist.",
        "",
        "## Global",
        "",
    ]
    # Global summary block - render as a markdown-friendly variant.
    n_dumped = sum(1 for r in all_rows if r["dumped"])
    n_documented = sum(1 for r in all_rows if r["documented"])
    n_ported = sum(1 for r in all_rows if r["ported"])
    n_ignored = sum(1 for r in all_rows if r["ignored"])
    dd_not_p = sum(
        1 for r in all_rows if r["dumped"] and r["documented"] and not r["ported"]
    )
    dd_not_p_ignored = sum(
        1
        for r in all_rows
        if r["dumped"] and r["documented"] and not r["ported"] and r["ignored"]
    )
    remaining = dd_not_p - dd_not_p_ignored
    lines.extend(
        [
            f"- **dumped**: {n_dumped}",
            f"- **documented**: {n_documented}",
            f"- **ported** (`// PORT:` tag): {n_ported}",
            f"- **ignored** (PsyQ / BIOS / libgte / ...): {n_ignored}",
            f"- **port worklist** (dumped + documented, not ported, not ignored): {remaining}",
            "",
        ]
    )

    if features:
        lines.extend(
            [
                "## Per-feature status",
                "",
                "Each feature is a BFS over the citation graph starting from `roots`, bounded by `stop_at`.",
                "Reachable counts widen as more dumps land - feature views start tight and grow.",
                "",
                "| Feature | Reachable | Ported | Port % | Missing | Ignored | Description |",
                "|---|---:|---:|---:|---:|---:|---|",
            ]
        )
        # Compute per-feature numbers and stash for the per-feature top-N
        # sections that follow the summary table.
        feature_stats: list[tuple[str, dict, list[dict]]] = []
        for name in sorted(features):
            body = features[name]
            reachable = bfs_reachable(
                graph,
                body["roots"],
                stop_at=body["stop_at"],
                max_depth=body["max_depth"],
            )
            f_rows = [rows_by_addr[a] for a in reachable if a in rows_by_addr]
            n_reach = len(f_rows)
            n_ported_f = sum(1 for r in f_rows if r["ported"])
            n_missing_f = sum(
                1
                for r in f_rows
                if r["dumped"]
                and r["documented"]
                and not r["ported"]
                and not r["ignored"]
            )
            n_ignored_f = sum(1 for r in f_rows if r["ignored"])
            pct = (100.0 * n_ported_f / n_reach) if n_reach else 0.0
            stats = {
                "reachable": n_reach,
                "ported": n_ported_f,
                "pct": pct,
                "missing": n_missing_f,
                "ignored": n_ignored_f,
            }
            feature_stats.append((name, stats, f_rows))
            desc = body.get("description", "") or ""
            lines.append(
                f"| `{name}` | {n_reach} | {n_ported_f} | {pct:.1f}% | "
                f"{n_missing_f} | {n_ignored_f} | {desc} |"
            )
        lines.append("")

        # Per-feature top-N missing-ports - the highest-leverage helpers first.
        # Highest-citation-count helpers tend to be shared infrastructure the
        # feature can't progress without.
        lines.extend(
            [
                f"## Per-feature missing-ports (top {top_n} by citation count)",
                "",
                "These are the dumped + documented helpers reachable from each feature's roots that don't yet carry a `// PORT:` tag. Ignore-list entries are excluded.",
                "",
            ]
        )
        for name, stats, f_rows in feature_stats:
            missing = sorted(
                (
                    r
                    for r in f_rows
                    if r["dumped"]
                    and r["documented"]
                    and not r["ported"]
                    and not r["ignored"]
                ),
                key=lambda r: (-r["refs"], r["addr"]),
            )[:top_n]
            lines.append(f"### `{name}` - {stats['missing']} missing")
            lines.append("")
            if not missing:
                lines.append("All reachable + documented helpers ported.")
                lines.append("")
                continue
            lines.extend(
                [
                    "| addr | refs | first dump source | doc sources |",
                    "|---|---:|---|---|",
                ]
            )
            for r in missing:
                first_src = r["first_sources"][0] if r["first_sources"] else "-"
                docs_cell = (
                    ", ".join(r["doc_sources"][:3]) if r["doc_sources"] else "-"
                )
                lines.append(
                    f"| `{r['addr']}` | {r['refs']} | `{first_src}` | {docs_cell} |"
                )
            lines.append("")
    else:
        lines.extend(
            [
                "## Per-feature status",
                "",
                "_No features defined in `scripts/ci/features.toml`._",
                "",
            ]
        )

    # Ignore-list breakdown - per category.
    ignored_rows = [r for r in all_rows if r["ignored"]]
    if ignored_rows:
        by_cat: dict[str, int] = defaultdict(int)
        for r in ignored_rows:
            by_cat[r["ignore_category"]] += 1
        lines.extend(
            [
                "## Ignore-list summary",
                "",
                "Addresses explicitly out of scope for engine porting - statically-linked PsyQ / BIOS / SDK code mapped to native equivalents (Rust stdlib, wgpu, cpal). Source: [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml).",
                "",
                "| Category | Count |",
                "|---|---:|",
            ]
        )
        for cat in sorted(by_cat):
            lines.append(f"| `{cat}` | {by_cat[cat]} |")
        lines.append(f"| **total** | **{len(ignored_rows)}** |")
        lines.append("")

    # Provenance gaps - small but worth surfacing on the same page.
    gaps_port_no_doc = [r for r in all_rows if r["ported"] and not r["documented"]]
    gaps_port_no_dump = [r for r in all_rows if r["ported"] and not r["dumped"]]
    if gaps_port_no_doc or gaps_port_no_dump:
        lines.extend(
            [
                "## Provenance gaps",
                "",
                "Addresses with a `// PORT:` tag but missing the supporting trail. Either backfill the missing axis or remove the tag if attribution was wrong.",
                "",
            ]
        )
        if gaps_port_no_doc:
            lines.append(f"- **ported but not documented**: {len(gaps_port_no_doc)}")
        if gaps_port_no_dump:
            lines.append(f"- **ported but not dumped**: {len(gaps_port_no_dump)}")
        lines.append("")
        if gaps_port_no_doc:
            lines.append("### Ported but not documented")
            lines.append("")
            lines.append("| addr | crates |")
            lines.append("|---|---|")
            for r in sorted(gaps_port_no_doc, key=lambda r: r["addr"]):
                lines.append(
                    f"| `{r['addr']}` | {', '.join(r['port_crates']) or '-'} |"
                )
            lines.append("")
        if gaps_port_no_dump:
            lines.append("### Ported but not dumped")
            lines.append("")
            lines.append("| addr | crates |")
            lines.append("|---|---|")
            for r in sorted(gaps_port_no_dump, key=lambda r: r["addr"]):
                lines.append(
                    f"| `{r['addr']}` | {', '.join(r['port_crates']) or '-'} |"
                )
            lines.append("")

    md = "\n".join(lines) + "\n"
    if out_path:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(md)
    return md


def filter_rows(rows: list[dict], args: argparse.Namespace) -> list[dict]:
    out = rows
    if args.addr:
        addr = args.addr.lower().removeprefix("0x")
        out = [r for r in out if r["addr"] == addr]
    if args.missing_ports:
        out = [
            r for r in out if r["dumped"] and r["documented"] and not r["ported"]
        ]
        if not args.include_ignored:
            out = [r for r in out if not r["ignored"]]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.missing_dumps:
        out = [r for r in out if r["refs"] > 0 and not r["dumped"]]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.ported_only:
        out = [r for r in out if r["ported"]]
    if args.ignored_only:
        out = [r for r in out if r["ignored"]]
        out.sort(key=lambda r: (r["ignore_category"], r["addr"]))
    if args.not_live:
        out = [r for r in out if r["ported"] and r["live_known"] and not r["live"]]
        out.sort(key=lambda r: (r["not_wired_tag"], r["addr"]))
    if args.live_only:
        out = [r for r in out if r["ported"] and r["live"]]
    if args.bucket:
        out = [r for r in out if r["bucket"] == args.bucket]
    if args.top:
        out = out[: args.top]
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--md",
        action="store_true",
        help="emit markdown to stdout instead of CSV-to-file (CSV/MD still written to target/port-catalog/)",
    )
    ap.add_argument("--top", type=int, default=0, help="cap output rows (0 = no cap)")
    ap.add_argument("--addr", type=str, default="", help="single-address drill-down")
    ap.add_argument(
        "--missing-ports",
        action="store_true",
        help="filter: dumped + documented but not ported (port worklist)",
    )
    ap.add_argument(
        "--missing-dumps",
        action="store_true",
        help="filter: cited but not yet dumped (dump worklist)",
    )
    ap.add_argument(
        "--ported-only",
        action="store_true",
        help="filter: only addresses with a // PORT: tag",
    )
    ap.add_argument(
        "--ignored-only",
        action="store_true",
        help="filter: only addresses in scripts/ci/port-catalog-ignore.toml",
    )
    ap.add_argument(
        "--live",
        action="store_true",
        help="run the Rust reachability pass and add the `live` column "
        "(slower: parses every crates/**/src/**.rs)",
    )
    ap.add_argument(
        "--not-live",
        action="store_true",
        help="filter: ported but not reachable from a host root (implies --live)",
    )
    ap.add_argument(
        "--live-only",
        action="store_true",
        help="filter: ported and reachable from a host root (implies --live)",
    )
    ap.add_argument(
        "--live-audit",
        action="store_true",
        help="emit the live-reachability audit (stale NOT WIRED tags + "
        "undisclosed inert ports) to target/port-catalog/live-audit.md",
    )
    ap.add_argument(
        "--include-ignored",
        action="store_true",
        help="don't exclude ignore-list entries from --missing-ports (default: exclude)",
    )
    ap.add_argument(
        "--bucket",
        choices=["scus", "overlay"],
        help="filter: only SCUS-resident or only overlay-resident addresses",
    )
    ap.add_argument(
        "--no-write",
        action="store_true",
        help="don't write CSV/MD artifacts to target/port-catalog/",
    )
    ap.add_argument(
        "--feature",
        type=str,
        default="",
        help="filter to addresses reachable from a feature's roots (see scripts/ci/features.toml)",
    )
    ap.add_argument(
        "--list-features",
        action="store_true",
        help="list available features in scripts/ci/features.toml and exit",
    )
    ap.add_argument(
        "--dashboard",
        action="store_true",
        help="emit open-work rollup (per-feature stats + top-N missing-ports + "
        "ignore summary) to target/port-catalog/open-work.md and exit",
    )
    ap.add_argument(
        "--dashboard-top",
        type=int,
        default=10,
        help="per-feature top-N missing-ports cap for --dashboard (default: 10)",
    )
    args = ap.parse_args()

    if args.list_features:
        features = load_features()
        if not features:
            print(f"no features found in {FEATURES_TOML}")
            return 0
        print(f"features in {FEATURES_TOML}:")
        for name, body in features.items():
            roots = ", ".join(body["roots"]) or "(none)"
            stop_at = (
                f"  stop_at: {', '.join(sorted(body['stop_at']))}\n"
                if body["stop_at"]
                else ""
            )
            depth = (
                f"  max_depth: {body['max_depth']}\n"
                if body["max_depth"] is not None
                else ""
            )
            print(f"\n[{name}]")
            print(f"  description: {body['description']}")
            print(f"  roots: {roots}")
            if stop_at:
                print(stop_at.rstrip())
            if depth:
                print(depth.rstrip())
        return 0

    dumped = collect_dumped()
    refs, sources = collect_citations()
    docs = collect_doc_citations()
    ports = collect_ports()
    ignore = load_ignore()

    want_live = args.live or args.not_live or args.live_only or args.live_audit
    live_map: dict[str, dict] | None = None
    if want_live:
        srcs = load_rust_sources()
        fns, edges = build_rust_graph(srcs)
        roots = collect_roots(srcs)
        reach = reachable_fns(fns, edges, roots)
        # Second, receiver-gated pass. Only the stale-`NOT WIRED` test reads it;
        # every `live` / `--not-live` verdict stays on the permissive graph so
        # the not-live list keeps its hard-floor property. Built only for the
        # audit, which is the sole consumer.
        reach_strict = None
        if args.live_audit:
            _, edges_s = build_rust_graph(srcs, strict=True)
            reach_strict = reachable_fns(fns, edges_s, roots)
        live_map = compute_live(
            collect_port_anchors(srcs), srcs, fns, reach, reach_strict
        )

    rows = build_rows(dumped, refs, sources, docs, ports, ignore=ignore, live=live_map)

    # Always write the global catalog so the latest state is on disk even when
    # the user is also drilling into a feature filter.
    if not args.no_write:
        render_csv(rows, OUT_DIR / "catalog.csv")
        render_md(rows, OUT_DIR / "catalog.md", "Port catalog (global)")

    if args.live_audit:
        out_path = None if args.no_write else OUT_DIR / "live-audit.md"
        md = render_live_audit(rows, out_path)
        if args.md:
            print(md)
        else:
            print(summarize(rows))
            if out_path:
                print()
                print(f"wrote {out_path}")
        return 0

    if args.dashboard:
        features = load_features()
        out_path = None if args.no_write else OUT_DIR / "open-work.md"
        md = render_dashboard(rows, sources, features, args.dashboard_top, out_path)
        if args.md:
            print(md)
        else:
            print(summarize(rows))
            if out_path:
                print()
                print(f"wrote {out_path}")
        return 0

    # Feature filter trims rows to BFS-reachable addresses + writes a
    # per-feature artifact under target/port-catalog/<feature>.{csv,md}.
    feature_meta: dict | None = None
    if args.feature:
        features = load_features()
        if args.feature not in features:
            print(
                f"unknown feature: {args.feature!r}. "
                f"Available: {', '.join(features) or '(none - populate scripts/ci/features.toml)'}"
            )
            return 1
        feature_meta = features[args.feature]
        graph = build_call_graph(sources)
        reachable = bfs_reachable(
            graph,
            feature_meta["roots"],
            stop_at=feature_meta["stop_at"],
            max_depth=feature_meta["max_depth"],
        )
        rows = [r for r in rows if r["addr"] in reachable]
        if not args.no_write:
            title = f"Port catalog - feature: {args.feature}"
            render_csv(rows, OUT_DIR / f"{args.feature}.csv")
            render_md(rows, OUT_DIR / f"{args.feature}.md", title)

    filtered = filter_rows(rows, args)

    if args.md:
        title = "Port catalog"
        if args.feature:
            title += f" - feature: {args.feature}"
        if args.missing_ports:
            title += " - port worklist (dumped + documented, not ported)"
        elif args.missing_dumps:
            title += " - dump worklist (cited, not dumped)"
        elif args.ported_only:
            title += " - ported only"
        elif args.addr:
            title += f" - {args.addr}"
        print(render_md(filtered, None, title))
        return 0

    if feature_meta is not None:
        print(f"[feature: {args.feature}]")
        print(f"  description: {feature_meta['description']}")
        print(f"  roots:       {', '.join(feature_meta['roots']) or '(none)'}")
        if feature_meta["stop_at"]:
            print(f"  stop_at:     {', '.join(sorted(feature_meta['stop_at']))}")
        print(f"  reachable:   {len(rows)} addresses")
        print()
    print(summarize(rows))
    if filtered != rows:
        print()
        print(f"filtered rows ({len(filtered)}):")
        print(f"{'addr':<10} {'bucket':<8} {'D/d/P/I':<8} {'refs':>4}  port crates / ignore / first src")
        print(f"{'-' * 10} {'-' * 8} {'-' * 8} {'-' * 4}  {'-' * 60}")
        for r in filtered:
            flags = (
                ("D" if r["dumped"] else ".")
                + ("d" if r["documented"] else ".")
                + ("P" if r["ported"] else ".")
                + ("I" if r["ignored"] else ".")
                + (("L" if r["live"] else "x") if r["live_known"] else ".")
            )
            crates = ",".join(r["port_crates"]) if r["port_crates"] else "-"
            if r["ported"]:
                tail = crates
            elif r["ignored"]:
                tail = f"[{r['ignore_category']}] {r['ignore_reason']}"
            else:
                tail = r["first_sources"][0] if r["first_sources"] else "-"
            print(f"{r['addr']:<10} {r['bucket']:<8} {flags:<8} {r['refs']:>4}  {tail}")
    if not args.no_write:
        print()
        print(f"wrote {OUT_DIR / 'catalog.csv'}")
        print(f"wrote {OUT_DIR / 'catalog.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
