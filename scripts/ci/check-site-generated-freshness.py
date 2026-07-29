#!/usr/bin/env python3
"""Staleness check for the generated static-site pages.

`site/*.html` is build output: `site/_gen.py` renders it from the fragments in
`site/_content/*.html`. The generated pages are untracked, so nothing in git
notices when a `_content` edit is never regenerated - and a served page then
mixes a *new* asset's expectations with an *old* page's markup.

That is not hypothetical. Adding `site/js/pad-bindings.js` and pointing
`play-app.js` at the globals it installs left the generated `play.html` loading
the new `play-app.js` without the new `pad-bindings.js`. The glue was
`undefined`, the `PlayView` constructor threw on the very first statement that
used it, and boot aborted - so keyboard input, the pause menu and the scene
picker were all dead at once, with no console error that named the cause.

`check-site-links.py` cannot catch this: every link in both files resolves to a
file that exists. The defect is not a broken reference, it is a *missing* one.

The rule here is one-directional on purpose. Every asset a `_content` fragment
references must also be referenced by its generated page; the generated page is
allowed to reference more, because the site template appends its own (`layout.js`,
`main.js`, ...). So this answers "did the generator run since the fragment
changed", which is the question that bites, and stays silent about the
template's own additions.

    python3 scripts/ci/check-site-generated-freshness.py            # fail (exit 1)
    python3 scripts/ci/check-site-generated-freshness.py --warn     # report only

Fix a finding by running `python3 site/_gen.py`, never by hand-editing a
generated page - the next generator run would discard the edit.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SITE = REPO / "site"
CONTENT = SITE / "_content"

# `src=` on <script>, `href=` on <link>. Both are asset references whose absence
# from the generated page means the generator has not run since the edit.
REF_RE = re.compile(
    r"""<(?:script|link)\b[^>]*?\b(?:src|href)\s*=\s*["']([^"']+)["']""",
    re.IGNORECASE,
)


def local_refs(html: str) -> set[str]:
    """Local asset paths referenced by a page, ignoring the cache-buster query.

    Absolute URLs and in-page anchors are not generator output, so they carry no
    staleness signal.
    """
    out: set[str] = set()
    for raw in REF_RE.findall(html):
        if raw.startswith(("http://", "https://", "//", "#", "data:", "mailto:")):
            continue
        out.add(raw.split("?", 1)[0].split("#", 1)[0])
    return out


# `site/_gen.py` carries the output->fragment map as tuples whose first element
# is the generated path and whose last is the `_content` fragment. Read it from
# there rather than assuming the two share a name: `home.html` renders to
# `index.html`, so a same-name assumption invents a missing page.
PAGE_TUPLE_RE = re.compile(
    r"""\(\s*["']([^"']+\.html)["']\s*,(?:[^()]*?),\s*["']([^"']+\.html)["']\s*\)""",
    re.DOTALL,
)


def fragment_to_output() -> dict[str, str]:
    gen_py = SITE / "_gen.py"
    if not gen_py.exists():
        return {}
    mapping: dict[str, str] = {}
    for out, frag in PAGE_TUPLE_RE.findall(gen_py.read_text(errors="replace")):
        mapping.setdefault(Path(frag).name, out)
    return mapping


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--warn", action="store_true", help="report without failing")
    args = ap.parse_args()

    if not CONTENT.is_dir():
        print("[site-freshness] no site/_content - nothing to check")
        return 0

    mapping = fragment_to_output()
    findings: list[str] = []
    checked = 0
    for frag in sorted(CONTENT.glob("*.html")):
        gen = SITE / mapping.get(frag.name, frag.name)
        if not gen.exists():
            findings.append(
                f"{frag.name}: no generated page at "
                f"{gen.relative_to(REPO)} - run site/_gen.py"
            )
            continue
        checked += 1
        missing = local_refs(frag.read_text(errors="replace")) - local_refs(
            gen.read_text(errors="replace")
        )
        for ref in sorted(missing):
            findings.append(
                f"site/{frag.name}: references {ref}, but the generated page does not"
            )

    if findings:
        print(f"[site-freshness] {len(findings)} stale generated page reference(s):")
        for f in findings:
            print(f"    {f}")
        print("[site-freshness] run `python3 site/_gen.py` to regenerate")
        return 0 if args.warn else 1

    print(f"[site-freshness] OK - {checked} generated page(s) carry every _content asset")
    return 0


if __name__ == "__main__":
    sys.exit(main())
