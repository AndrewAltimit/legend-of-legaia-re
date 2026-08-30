#!/usr/bin/env python3
"""Detect module-only JS syntax in a script the site loads classically.

`import.meta`, static `import ... from` and `export` declarations are legal
only inside `<script type="module">`. In a classic `<script src=...>` they
are a **parse error for the whole file**: nothing in it runs, and the page
degrades to whatever its static HTML happens to show. The world-overview
page shipped that way - one `import.meta.url` in the wasm loader (pasted
from a module-loaded sibling) killed the entire 1400-line viewer app, and
every gate stayed green because nothing but a browser ever parses the file.

The audit joins two facts no single file carries:

  1. which js files the committed page sources (`site/_content/**/*.html`,
     `site/_gen.py`'s shell template) load WITHOUT `type="module"`;
  2. which js files contain module-only syntax outside comments/strings.

A file in both sets is a finding. A js file no scanned source references
classically is skipped - a dynamically `import()`ed module is entitled to
module syntax, and dynamic `import()` itself is legal in classic scripts.

Usage:
    python3 scripts/ci/check-js-classic-module-syntax.py            # audit site/
    python3 scripts/ci/check-js-classic-module-syntax.py --selftest # controls only

A finding can be waived in place with a trailing `// classic-script-ok:
<reason>` comment on the offending line. The reason is mandatory.

Exit status: 0 = clean, 1 = findings, 2 = self-test failed.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

SITE = "site"

SCRIPT_TAG_RE = re.compile(r"<script\b([^>]*)>", re.IGNORECASE)
SRC_RE = re.compile(r"""src\s*=\s*["']([^"']+)["']""", re.IGNORECASE)
MODULE_RE = re.compile(r"""type\s*=\s*["']module["']""", re.IGNORECASE)
WAIVER_RE = re.compile(r"//\s*classic-script-ok\s*:\s*(?P<reason>.*)$")

IMPORT_META_RE = re.compile(r"\bimport\s*\.\s*meta\b")
# Static import declaration: `import x from '...'`, `import {a} from '...'`,
# `import '...'`, `import * as ns from '...'`. Dynamic `import(` is legal in
# classic scripts and must NOT match.
STATIC_IMPORT_RE = re.compile(r"""^\s*import\s+(?:[\w$*{"'][^;]*)?(?:from\s+)?["']""")
EXPORT_RE = re.compile(r"^\s*export\s+(?:default\b|const\b|let\b|var\b|function\b|class\b|async\b|\{|\*)")


@dataclass
class Finding:
    path: str
    line: int
    shape: str
    text: str

    def render(self) -> str:
        return f"{self.path}:{self.line}: {self.shape}: {self.text.strip()}"


def classic_script_basenames(html_texts: list[str]) -> set[str]:
    """Basenames of .js files any source loads without type="module"."""
    out: set[str] = set()
    for text in html_texts:
        for m in SCRIPT_TAG_RE.finditer(text):
            attrs = m.group(1)
            src = SRC_RE.search(attrs)
            if not src or MODULE_RE.search(attrs):
                continue
            name = src.group(1).split("?")[0].rsplit("/", 1)[-1]
            if name.endswith(".js"):
                out.add(name)
    return out


def strip_comments_and_strings(line: str, in_block: bool) -> tuple[str, bool]:
    """Blank out string contents, // comments and /* */ spans on one line.

    Returns the scrubbed line and whether a block comment continues past it.
    Crude by design (no template-literal nesting), pinned by the self-test.
    """
    out: list[str] = []
    i, n = 0, len(line)
    quote: str | None = None
    while i < n:
        c = line[i]
        if in_block:
            if c == "*" and i + 1 < n and line[i + 1] == "/":
                in_block = False
                i += 2
            else:
                i += 1
            continue
        if quote:
            if c == "\\":
                i += 2
                continue
            if c == quote:
                quote = None
                out.append(c)
            i += 1
            continue
        if c in "\"'`":
            quote = c
            out.append(c)
            i += 1
            continue
        if c == "/" and i + 1 < n:
            if line[i + 1] == "/":
                break  # rest of line is a comment
            if line[i + 1] == "*":
                in_block = True
                i += 2
                continue
        out.append(c)
        i += 1
    # A template literal may span lines; treat the unterminated remainder as
    # string content (blanked) but do not carry state - template bodies with
    # module keywords are not the shape this gate hunts.
    return "".join(out), in_block


def scan_js(path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    in_block = False
    for line_no, raw in enumerate(text.splitlines(), 1):
        code, in_block = strip_comments_and_strings(raw, in_block)
        if not code.strip():
            continue
        shape = None
        if IMPORT_META_RE.search(code):
            shape = "IMPORT_META_IN_CLASSIC"
        elif STATIC_IMPORT_RE.match(code):
            shape = "STATIC_IMPORT_IN_CLASSIC"
        elif EXPORT_RE.match(code):
            shape = "EXPORT_IN_CLASSIC"
        if not shape:
            continue
        w = WAIVER_RE.search(raw)
        if w:
            if w.group("reason").strip():
                continue
            shape = "BARE_WAIVER"
        findings.append(Finding(path, line_no, shape, raw))
    return findings


# Positive controls: each MUST be flagged, or a clean audit means nothing.
SELFTEST_CASES: list[tuple[str, str, str]] = [
    (
        "import-meta",
        "async function load() {\n  await init(new URL('x.wasm', import.meta.url));\n}\n",
        "IMPORT_META_IN_CLASSIC",
    ),
    (
        "static-import",
        "import { init } from './wasm/glue.js';\n",
        "STATIC_IMPORT_IN_CLASSIC",
    ),
    (
        "bare-import",
        "import './side-effect.js';\n",
        "STATIC_IMPORT_IN_CLASSIC",
    ),
    (
        "export-decl",
        "export function draw() {}\n",
        "EXPORT_IN_CLASSIC",
    ),
    (
        "bare-waiver",
        "const u = import.meta.url; // classic-script-ok:\n",
        "BARE_WAIVER",
    ),
]

# Negative controls: legal-in-classic shapes that must NOT be flagged.
SELFTEST_CLEAN: list[tuple[str, str]] = [
    ("dynamic-import", "const mod = await import('wasm/glue.js?v=' + v);\n"),
    ("dynamic-import-url", "await import(new URL('wasm/glue.js', document.baseURI).href);\n"),
    ("comment-mention", "// `import.meta` is a SyntaxError in a classic script\nconst x = 1;\n"),
    ("block-comment", "/* import.meta resolves against the module URL\n   export nothing */\nconst x = 1;\n"),
    ("string-mention", "const tip = 'never use import.meta here';\n"),
    ("identifier", "const importer = importMap.resolve(name);\n"),
    (
        "waiver-with-reason",
        "const u = import.meta.url; // classic-script-ok: page inlines this as a module\n",
    ),
]


def run_selftest() -> int:
    failures = 0
    for name, snippet, expect_shape in SELFTEST_CASES:
        shapes = {f.shape for f in scan_js(f"<selftest:{name}>", snippet)}
        if expect_shape in shapes:
            print(f"  ok    {name}: flagged {expect_shape}")
        else:
            print(f"  FAIL  {name}: expected {expect_shape}, got {sorted(shapes) or 'nothing'}")
            failures += 1
    for name, snippet in SELFTEST_CLEAN:
        got = scan_js(f"<selftest:{name}>", snippet)
        if got:
            print(f"  FAIL  {name}: expected clean, got {[f.shape for f in got]}")
            failures += 1
        else:
            print(f"  ok    {name}: clean")

    tags = classic_script_basenames(
        ['<script src="js/app.js?v=abc"></script>'
         '<script type="module" src="../js/mod-app.js"></script>']
    )
    if tags == {"app.js"}:
        print("  ok    tag-classify: classic={app.js}, module excluded")
    else:
        print(f"  FAIL  tag-classify: got {sorted(tags)}")
        failures += 1

    if failures:
        print(
            f"\nself-test: {failures} case(s) failed -- the detector is not "
            f"trustworthy, so a clean audit from it means nothing"
        )
        return 2
    total = len(SELFTEST_CASES) + len(SELFTEST_CLEAN) + 1
    print(f"\nself-test: all {total} cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--selftest", action="store_true",
                    help="run the positive/negative control suite and exit")
    ap.add_argument("--quiet", action="store_true", help="only print findings")
    args = ap.parse_args()

    if args.selftest:
        print("check-js-classic-module-syntax self-test")
        return run_selftest()

    # Run the controls on every audit: a sweep that reports "clean" from a
    # detector that never matches is the defect class this file exists for.
    for _name, snippet, expect in SELFTEST_CASES:
        if expect not in {f.shape for f in scan_js("<control>", snippet)}:
            print("ERROR: built-in positive control failed; audit result is not "
                  "trustworthy (run --selftest)")
            return 2

    site = REPO_ROOT / SITE
    html_texts = [p.read_text(encoding="utf-8", errors="replace")
                  for p in sorted((site / "_content").rglob("*.html"))]
    gen = site / "_gen.py"
    if gen.exists():
        html_texts.append(gen.read_text(encoding="utf-8", errors="replace"))
    classic = classic_script_basenames(html_texts)

    findings: list[Finding] = []
    for p in sorted((site / "js").glob("*.js")):
        if p.name not in classic:
            continue
        rel = p.relative_to(REPO_ROOT).as_posix()
        findings.extend(scan_js(rel, p.read_text(encoding="utf-8", errors="replace")))

    if not args.quiet:
        print(f"[js-classic-module-syntax] {len(classic)} classic-loaded file(s) scanned")
    for f in findings:
        print(f.render())
    if findings:
        print(
            f"\n{len(findings)} finding(s): module-only syntax in a classic "
            f"script parses to NOTHING - the whole file dies. Load the page "
            f"with type=\"module\", or resolve URLs against document.baseURI "
            f"like arts-viewer.js does."
        )
        return 1
    if not args.quiet:
        print("[js-classic-module-syntax] OK - no module-only syntax in classic scripts")
    return 0


if __name__ == "__main__":
    sys.exit(main())
