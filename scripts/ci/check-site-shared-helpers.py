#!/usr/bin/env python3
"""Keep the site's shared helpers shared.

`site/js/site-util.js` is loaded from every page's <head> and exports the
helpers every page needs (`window.escapeHtml`, `window.LegaiaUtil.*`). Each
of them once lived as a private copy in several scripts - nine HTML
escapers, four RGBA-canvas wrappers, three download helpers, three
identical minigame pose kernels - and copies drift: an escaper that forgot
the quote, a poser that lost its floor offset. Nothing but a reader notices
a new copy, so this gate does.

A finding is a function *definition* under one of the shared names (or a
known alias of one) anywhere in the committed site sources other than
site-util.js itself. Binding the shared function to a local name
(`const rgbaCanvas = window.LegaiaUtil.rgbaCanvas;`) is the intended use
and is not a definition.

A finding can be waived with a trailing `// shared-helper-ok: <reason>`
comment on the definition line. The reason is mandatory.

Usage:
    python3 scripts/ci/check-site-shared-helpers.py            # audit site/
    python3 scripts/ci/check-site-shared-helpers.py --selftest # controls only

Exit status: 0 = clean, 1 = findings, 2 = self-test failed.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SITE = REPO_ROOT / "site"
SHARED = SITE / "js" / "site-util.js"

# Names whose definition belongs in site-util.js only: the exported names and
# the local names the copies used to carry.
NAMES = (
    "escapeHtml",
    "escapeHTML",
    "escHtml",
    "rgbaCanvas",
    "downloadFile",
    "triggerDownload",
    "poseClipInto",
)

_NAME_ALT = "|".join(NAMES)
DEF_RE = re.compile(
    r"(?:\bfunction\s+(?P<a>" + _NAME_ALT + r")\s*\("
    r"|\b(?:const|let|var)\s+(?P<b>" + _NAME_ALT + r")\s*=\s*(?:function\b|\([^)]*\)\s*=>|[A-Za-z_$][\w$]*\s*=>))"
)
WAIVER_RE = re.compile(r"//\s*shared-helper-ok:\s*\S")


def scan_text(text: str) -> list[tuple[int, str]]:
    out = []
    for n, line in enumerate(text.splitlines(), 1):
        stripped = line.lstrip()
        if stripped.startswith("//") or stripped.startswith("*"):
            continue
        m = DEF_RE.search(line)
        if m and not WAIVER_RE.search(line):
            out.append((n, m.group("a") or m.group("b")))
    return out


def sources() -> list[Path]:
    files = sorted((SITE / "js").glob("*.js"))
    files += sorted((SITE / "_content").rglob("*.html"))
    files += sorted((SITE / "world-overview").glob("*.js"))
    files.append(SITE / "_gen.py")
    return [f for f in files if f.is_file() and f.resolve() != SHARED.resolve()]


def selftest() -> bool:
    must_fire = [
        "function escapeHtml(s) { return s; }",
        "  const rgbaCanvas = (b, w, h) => null;",
        "let triggerDownload = function (b, n) {};",
        "var poseClipInto = x => x;",
    ]
    must_pass = [
        "const rgbaCanvas = window.LegaiaUtil.rgbaCanvas;",
        "const esc = window.escapeHtml;",
        "// function escapeHtml(s) - see site-util.js",
        "function escapeHtml(s) { return s; } // shared-helper-ok: standalone tool page",
        "out = escapeHtml(text);",
    ]
    ok = all(scan_text(t) for t in must_fire) and not any(scan_text(t) for t in must_pass)
    if not ok:
        for t in must_fire:
            if not scan_text(t):
                print(f"selftest: did not fire on: {t}", file=sys.stderr)
        for t in must_pass:
            if scan_text(t):
                print(f"selftest: fired on: {t}", file=sys.stderr)
    return ok


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--selftest", action="store_true", help="run the controls only")
    ap.add_argument("--quiet", action="store_true", help="print only findings")
    args = ap.parse_args()

    if not selftest():
        print("[site-shared-helpers] self-test FAILED", file=sys.stderr)
        return 2
    if args.selftest:
        print("[site-shared-helpers] self-test passed")
        return 0
    if not SHARED.is_file():
        print(f"[site-shared-helpers] SKIPPED - no {SHARED.relative_to(REPO_ROOT)}")
        return 0

    findings = []
    files = sources()
    for f in files:
        for line, name in scan_text(f.read_text(encoding="utf-8", errors="replace")):
            findings.append((f.relative_to(REPO_ROOT), line, name))
    for path, line, name in findings:
        print(
            f"{path}:{line}: defines `{name}` - use the shared helper in "
            "site/js/site-util.js (window.escapeHtml / window.LegaiaUtil)"
        )
    if findings:
        print(f"[site-shared-helpers] {len(findings)} finding(s)", file=sys.stderr)
        return 1
    if not args.quiet:
        print(f"[site-shared-helpers] OK - {len(files)} source(s), no private copy of a shared helper")
    return 0


if __name__ == "__main__":
    sys.exit(main())
