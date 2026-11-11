"""Which retail routines the retail kernel differential runs, read from the
declarations in `crates/retail-exec/tests/`.

Every `#[test]` in that directory opens with one of two macros (defined in
`tests/common.rs`):

    differential!(scus(0x8004_CE2C), prot(898, ACTION_SM));
    not_a_differential!("compares a table, runs no routine");

`differential!` plants a guard that, when the test passes with the disc
present, fails it unless each declared routine was entered by a `Cpu::call`
on the test's thread and the bytes run at its entry are the named image's
(`SCUS_942.54`, or PROT entry N). So a declaration is checked by the run it
describes, and this module can read the declarations statically - no disc, no
build - which is what lets the coverage figure gate on a runner.

The parse is deliberately narrow. An argument is an integer literal or a
`const` of the same file (or of `common.rs`); anything else is an error rather
than a guess, because a coverage number built on a guessed address is the
defect this exists to remove.

Used by `differential-coverage.py` (the report and ratchet), `port-catalog.py`
(the `differential` column) and `check-port-provenance.py` (a differential is
provenance evidence).
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
TESTS_DIR = REPO / "crates" / "retail-exec" / "tests"
INTEGRATION = TESTS_DIR / "integration.rs"
COMMON = TESTS_DIR / "common.rs"

_CONST_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z_][A-Z0-9_]*)\s*:\s*(?:u32|usize|u16|i32|u64)\s*=\s*([0-9][0-9a-fA-Fx_]*)\s*;",
    re.MULTILINE,
)
_TEST_RE = re.compile(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn\s+([a-z_][a-z0-9_]*)\s*\(")
_MOD_RE = re.compile(r"^\s*mod\s+([a-z_][a-z0-9_]*)\s*;", re.MULTILINE)
_ROUTINE_RE = re.compile(r"\b(scus|prot)\s*\(([^()]*)\)")


@dataclass
class TestDecl:
    module: str
    name: str
    line: int
    # (prot entry or None for SCUS, va)
    routines: list[tuple[int | None, int]] = field(default_factory=list)
    none_reason: str | None = None
    declared: bool = False

    @property
    def path(self) -> str:
        return f"{self.module}::{self.name}"


@dataclass
class Scan:
    tests: list[TestDecl]
    errors: list[str]

    def by_va(self) -> dict[int, list[tuple[TestDecl, int | None]]]:
        out: dict[int, list[tuple[TestDecl, int | None]]] = {}
        for t in self.tests:
            for img, va in t.routines:
                out.setdefault(va, []).append((t, img))
        return out

    def addrs(self) -> set[str]:
        """Declared entries as the lower-case 8-hex keys the catalog uses."""
        return {f"{va:08x}" for t in self.tests for _, va in t.routines}


def _int(text: str) -> int | None:
    t = text.strip().replace("_", "")
    try:
        return int(t, 16) if t.lower().startswith("0x") else int(t, 10)
    except ValueError:
        return None


def _consts(text: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for name, value in _CONST_RE.findall(text):
        v = _int(value)
        if v is not None:
            out[name] = v
    return out


def _ambiguous(text: str) -> set[str]:
    """Names a file defines as `const` with more than one value. A test that
    names one without defining it in its own body could mean either, and the
    file-wide table would silently credit whichever came last."""
    seen: dict[str, set[int]] = {}
    for name, value in _CONST_RE.findall(text):
        v = _int(value)
        if v is not None:
            seen.setdefault(name, set()).add(v)
    return {n for n, vs in seen.items() if len(vs) > 1}


def _balanced(text: str, open_at: int, pair: str = "{}") -> int:
    """Index one past the bracket closing the one at `open_at`. Skips string
    literals, char literals of one character, and line comments."""
    o, c = pair
    depth = 0
    i = open_at
    n = len(text)
    while i < n:
        ch = text[i]
        if ch == "/" and text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
            continue
        if ch == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif ch == "'" and i + 2 < n and text[i + 2] == "'":
            i += 2
        elif ch == o:
            depth += 1
        elif ch == c:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def _resolve(expr: str, consts: dict[str, int]) -> int | None:
    e = expr.strip()
    if e.startswith("crate::common::") or e.startswith("common::"):
        e = e.rsplit("::", 1)[1]
    v = _int(e)
    if v is not None:
        return v
    return consts.get(e)


def mounted_modules() -> list[str]:
    if not INTEGRATION.is_file():
        return []
    return [m for m in _MOD_RE.findall(INTEGRATION.read_text()) if m != "common"]


def scan(tests_dir: Path = TESTS_DIR) -> Scan:
    """Every `#[test]` under `tests_dir` with its declaration."""
    errors: list[str] = []
    tests: list[TestDecl] = []
    common = _consts(COMMON.read_text()) if COMMON.is_file() else {}
    mounted = set(mounted_modules())
    for path in sorted(tests_dir.glob("*.rs")):
        module = path.stem
        if module in ("integration", "common"):
            continue
        text = path.read_text()
        has_tests = bool(_TEST_RE.search(text))
        if has_tests and module not in mounted:
            errors.append(
                f"{path.relative_to(REPO)}: has tests but integration.rs mounts no "
                f"`mod {module};`, so they never run"
            )
        file_consts = {**common, **_consts(text)}
        ambiguous = _ambiguous(text)
        spans: list[tuple[int, int]] = []
        for m in _TEST_RE.finditer(text):
            brace = text.find("{", m.end())
            end = _balanced(text, brace)
            spans.append((brace, end))
            body = text[brace:end]
            # A test's own `const`s shadow the file's, as Rust scopes them.
            local = _consts(body)
            consts = {**file_consts, **local}
            line = text.count("\n", 0, m.start()) + 1
            t = TestDecl(module, m.group(1), line)
            where = f"{path.relative_to(REPO)}:{line} {t.path}"
            for mac in re.finditer(r"\b(differential|not_a_differential)!\s*\(", body):
                open_at = mac.end() - 1
                args = body[open_at + 1 : _balanced(body, open_at, "()") - 1]
                if t.declared:
                    errors.append(f"{where}: more than one declaration")
                t.declared = True
                if mac.group(1) == "not_a_differential":
                    reason = args.strip().strip('"').strip()
                    if not reason:
                        errors.append(f"{where}: not_a_differential! needs a reason")
                    t.none_reason = reason
                    continue
                found = _ROUTINE_RE.findall(args)
                if not found:
                    errors.append(f"{where}: differential! names no scus(..) / prot(..)")
                for kind, inner in found:
                    parts = [p for p in inner.split(",") if p.strip()]
                    want = 1 if kind == "scus" else 2
                    if len(parts) != want:
                        errors.append(f"{where}: {kind}({inner}) takes {want} argument(s)")
                        continue
                    clash = [
                        p.strip()
                        for p in parts
                        if p.strip() in ambiguous and p.strip() not in local
                    ]
                    if clash:
                        errors.append(
                            f"{where}: {kind}({inner}) - {', '.join(clash)} is defined "
                            "with more than one value in this file; define it in the "
                            "test or rename one"
                        )
                        continue
                    vals = [_resolve(p, consts) for p in parts]
                    if any(v is None for v in vals):
                        errors.append(
                            f"{where}: {kind}({inner}) - each argument must be a "
                            "literal or a const of this file or common.rs"
                        )
                        continue
                    img = None if kind == "scus" else vals[0]
                    t.routines.append((img, vals[-1]))
            if not t.declared:
                errors.append(
                    f"{where}: declares nothing - open it with differential!(..) "
                    "or not_a_differential!(\"why\")"
                )
            tests.append(t)
        for mac in re.finditer(r"\b(differential|not_a_differential)!\s*\(", text):
            if not any(a <= mac.start() < b for a, b in spans):
                line = text.count("\n", 0, mac.start()) + 1
                errors.append(
                    f"{path.relative_to(REPO)}:{line}: {mac.group(1)}! outside a "
                    "#[test] body is not counted - move it into the test"
                )
    return Scan(tests, errors)
