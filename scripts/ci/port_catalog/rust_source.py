"""One parsed Rust file: comment and string stripping, `fn` / `impl` / `trait`
spans, and the host roots a reachability walk starts from.
"""

import re
from pathlib import Path

from port_catalog.paths import CRATES_DIR

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


NEXT_ITEM_RE = re.compile(
    r"^\s*(?:pub\s*(?:\([^)]*\)\s*)?)?"
    r"(?:default\s+|const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*"
    r"(fn|struct|enum|union|trait|impl)\b\s*(?:<[^>]*>\s*)?([A-Za-z_]\w*)?"
)
# Value / alias / macro items a doc block can document. Tried only after
# `NEXT_ITEM_RE` fails, so `const fn` (matched above via the modifier run)
# never lands here and the bare `type` keyword can only mean an alias.
NEXT_DATA_ITEM_RE = re.compile(
    r"^\s*(?:pub\s*(?:\([^)]*\)\s*)?)?"
    r"(const|static|type|macro_rules!)\s+(?:mut\s+)?([A-Za-z_]\w*)"
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
        `enum` / `union` / `trait` / `impl` for the brace-bodied items, or
        `const` / `static` / `type` / `macro_rules!` for the value / alias /
        macro items, or `None` if the next code line is not an item header at
        all (a `let`, a `use`, a match arm - a loose tag).

        The forward walk is unbounded because it already stops at the first line
        that is neither a comment nor an attribute, so only an unbroken run of
        comment lines can extend it. Bounding that run demotes the anchor to
        module scope, and the runs that grow longest are exactly the thorough
        `NOT WIRED:` disclosures - so a bound makes a careful disclosure report
        itself as wired, via whatever sibling in the module a ladder did run.
        The data-item arm exists for the same structural reason: a doc block
        documents the first item after it regardless of what kind of item that
        is, so a tag above a `const` must not fall through to module scope
        either - module scope belongs to `//!` blocks only.
        """
        lines = self.raw.splitlines()
        for offset in range(line, len(lines)):
            raw_line = lines[offset]
            if COMMENT_OR_ATTR_RE.match(raw_line):
                continue
            m = NEXT_ITEM_RE.match(raw_line)
            if not m:
                dm = NEXT_DATA_ITEM_RE.match(raw_line)
                if dm:
                    return (dm.group(1), dm.group(2), offset + 1)
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
