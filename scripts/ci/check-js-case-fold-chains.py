#!/usr/bin/env python3
"""Detect a case fold that covers one operand of a `+` chain instead of the chain.

`.toLowerCase()` binds to the operand it is written on, so

    return `${a} ${label} ` +
      `${b} ${c}`.toLowerCase();

folds only the second literal. The first keeps its capitals, and the whole
expression still *reads* as "this string is lowercase". Written that way, the
ROM patcher's texture filter built a haystack whose label half stayed
disc-cased while the query arrived lowercased: every capitalised word in a
label became unsearchable, and typing `ra-seru` returned nothing while
"Gala - Ra-Seru Ozma $1" sat in the grid.

Nothing catches this but a checker. The page loads, the grid fills, the filter
filters - it just silently cannot see half its own vocabulary, and the failure
looks exactly like "the disc does not have that texture".

The shape flagged: a `.toLowerCase()` / `.toUpperCase()` whose receiver is a
string or template **literal** that is an operand of a `+`. A fold on an
identifier or on a parenthesised expression is not reported - `(a + b).fold()`
is the correct form, and `x.trim().toLowerCase()` folds what it says it folds.

Usage:
    python3 scripts/ci/check-js-case-fold-chains.py            # audit site/
    python3 scripts/ci/check-js-case-fold-chains.py --selftest # controls only
    python3 scripts/ci/check-js-case-fold-chains.py a.js b.js

A finding can be waived in place with a trailing `// case-fold-ok: <reason>`
comment on the line the fold is written on. The reason is mandatory: a bare
waiver says someone decided this was fine without saying why, which is how the
wrong form gets restored later.

Exit status: 0 = clean, 1 = findings, 2 = self-test failed.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# Directories the audit walks by default.
DEFAULT_ROOTS = ["site"]

FOLD_RE = re.compile(r"\.to(?:Lower|Upper)Case\s*\(\s*\)")
WAIVER_RE = re.compile(r"//\s*case-fold-ok\s*:\s*(?P<reason>.*)$")

QUOTES = "`'\""


@dataclass
class Finding:
    path: str
    line: int
    shape: str
    text: str

    def render(self) -> str:
        return f"{self.path}:{self.line}: {self.shape}: {self.text.strip()}"


def _is_escaped(text: str, idx: int) -> bool:
    """True when the character at `idx` is preceded by an odd run of backslashes."""
    n = 0
    j = idx - 1
    while j >= 0 and text[j] == "\\":
        n += 1
        j -= 1
    return n % 2 == 1


def _literal_start(text: str, close: int) -> int | None:
    """Index of the opening delimiter of the literal closing at `close`."""
    quote = text[close]
    j = close - 1
    while j >= 0:
        if text[j] == quote and not _is_escaped(text, j):
            return j
        # A plain quoted string never spans a line; a template may.
        if quote != "`" and text[j] == "\n":
            return None
        j -= 1
    return None


def _prev_significant(text: str, idx: int) -> str:
    """The last non-whitespace character at or before `idx`, '' if none."""
    j = idx
    while j >= 0 and text[j] in " \t\r\n":
        j -= 1
    return text[j] if j >= 0 else ""


def _next_significant(text: str, idx: int) -> str:
    """The first non-whitespace character at or after `idx`, '' if none."""
    j = idx
    while j < len(text) and text[j] in " \t\r\n":
        j += 1
    return text[j] if j < len(text) else ""


def scan_text(path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    lines = text.splitlines()

    for m in FOLD_RE.finditer(text):
        close = m.start() - 1
        if close < 0 or text[close] not in QUOTES:
            # Receiver is an identifier, a call, or a parenthesised
            # expression - the fold covers what it is written on.
            continue
        open_idx = _literal_start(text, close)
        if open_idx is None:
            continue

        before = _prev_significant(text, open_idx - 1)
        after = _next_significant(text, m.end())
        if before != "+" and after != "+":
            continue

        line_no = text.count("\n", 0, m.start()) + 1
        line = lines[line_no - 1] if line_no <= len(lines) else ""
        # A waiver may sit on the fold's line or on the line the literal opens.
        open_line_no = text.count("\n", 0, open_idx) + 1
        waiver_lines = {line_no, open_line_no}
        waived = False
        for n in waiver_lines:
            if n <= len(lines):
                w = WAIVER_RE.search(lines[n - 1])
                if w and w.group("reason").strip():
                    waived = True
                elif w:
                    findings.append(
                        Finding(path, n, "BARE_WAIVER", lines[n - 1])
                    )
                    waived = True
        if waived:
            continue

        shape = "FOLD_TAIL_ONLY" if before == "+" else "FOLD_HEAD_ONLY"
        findings.append(Finding(path, line_no, shape, line))

    return findings


# Snippets that MUST be flagged. Without these the audit is a probe that has
# never been shown to fire, and "clean" from such a probe means nothing.
SELFTEST_CASES: list[tuple[str, str, str]] = [
    (
        "tail-only-template",
        "function h(t) {\n  return `${t.a} ${t.label} ` +\n"
        "    `${t.b} ${t.c}`.toLowerCase();\n}\n",
        "FOLD_TAIL_ONLY",
    ),
    (
        "tail-only-quoted",
        "const s = prefix + 'Suffix Text'.toLowerCase();\n",
        "FOLD_TAIL_ONLY",
    ),
    (
        "head-only",
        "const s = `${a} Label`.toUpperCase() + rest;\n",
        "FOLD_HEAD_ONLY",
    ),
    (
        "bare-waiver",
        "const s = a + `${b}`.toLowerCase();  // case-fold-ok:\n",
        "BARE_WAIVER",
    ),
]

# Snippets that must NOT be flagged. A detector that flags every fold is as
# useless as one that flags none - the correct form is common.
SELFTEST_CLEAN: list[tuple[str, str]] = [
    (
        "parenthesised-chain",
        "function h(t) {\n  return (`${t.a} ` +\n    `${t.b}`).toLowerCase();\n}\n",
    ),
    ("identifier-receiver", "const q = (input.value || '').trim().toLowerCase();\n"),
    ("identifier-in-chain", "const s = (k === 'r' ? '' : k.toUpperCase() + ' ') + 'ARTS';\n"),
    ("chained-call", "const id = title.replace(/x/g, '').toLowerCase();\n"),
    ("standalone-literal", "const s = 'Mixed Case'.toLowerCase();\n"),
    (
        "waiver-with-reason",
        "const s = a + `${b}`.toLowerCase();  // case-fold-ok: a is a lowercase tier id\n",
    ),
]


def run_selftest() -> int:
    failures = 0
    for name, snippet, expect_shape in SELFTEST_CASES:
        shapes = {f.shape for f in scan_text(f"<selftest:{name}>", snippet)}
        if expect_shape in shapes:
            print(f"  ok    {name}: flagged {expect_shape}")
        else:
            print(f"  FAIL  {name}: expected {expect_shape}, got {sorted(shapes) or 'nothing'}")
            failures += 1

    for name, snippet in SELFTEST_CLEAN:
        got = scan_text(f"<selftest:{name}>", snippet)
        if got:
            print(f"  FAIL  {name}: expected clean, got {[f.shape for f in got]}")
            failures += 1
        else:
            print(f"  ok    {name}: clean")

    if failures:
        print(
            f"\nself-test: {failures} case(s) failed -- the detector is not "
            f"trustworthy, so a clean audit from it means nothing"
        )
        return 2
    print(f"\nself-test: all {len(SELFTEST_CASES) + len(SELFTEST_CLEAN)} cases pass")
    return 0


def iter_js_files(roots: list[Path]) -> list[Path]:
    out: list[Path] = []
    for root in roots:
        if root.is_file():
            out.append(root)
            continue
        for p in sorted(root.rglob("*.js")):
            # Build output, not source: `site/wasm/` is wasm-bindgen's glue.
            if "wasm" in p.parts:
                continue
            out.append(p)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("paths", nargs="*", help="files or dirs to scan (default: site/)")
    ap.add_argument("--selftest", action="store_true",
                    help="run the positive/negative control suite and exit")
    ap.add_argument("--quiet", action="store_true", help="only print findings")
    args = ap.parse_args()

    if args.selftest:
        print("check-js-case-fold-chains self-test")
        return run_selftest()

    roots = [Path(p) for p in args.paths] if args.paths else [REPO_ROOT / r for r in DEFAULT_ROOTS]
    files = iter_js_files(roots)

    # Run the control on every audit, not only under --selftest: a sweep that
    # reports "clean" from a detector that never matches is the same class of
    # defect this file exists to catch.
    for _name, snippet, expect in SELFTEST_CASES:
        if expect not in {f.shape for f in scan_text("<control>", snippet)}:
            print("ERROR: built-in positive control failed; audit result is not "
                  "trustworthy. Run --selftest.", file=sys.stderr)
            return 2

    findings: list[Finding] = []
    for f in files:
        try:
            rel = str(f.relative_to(REPO_ROOT))
        except ValueError:
            rel = str(f)
        findings.extend(scan_text(rel, f.read_text(errors="replace")))

    if not args.quiet:
        print(f"scanned {len(files)} JS file(s) (positive control: passed)")

    if findings:
        for fi in findings:
            print(fi.render())
        print(f"\n{len(findings)} finding(s)")
        return 1

    if not args.quiet:
        print("no split case-fold findings")
    return 0


if __name__ == "__main__":
    sys.exit(main())
