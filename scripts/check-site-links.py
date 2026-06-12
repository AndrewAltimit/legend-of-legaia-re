#!/usr/bin/env python3
"""Internal link checker for the generated static site.

Scans every generated page under site/ (skipping the _content/ fragments,
which mirror the same hrefs) and validates:

  - relative href/src targets resolve to a file that exists under site/
  - fragment links (`page.html#anchor` and bare `#anchor`) point at an
    element id that exists in the target page

External links (http/https/mailto/data/javascript) are out of scope - this
is the "no broken internal navigation" gate, not a crawler.

Usage:
    python3 scripts/check-site-links.py            # full scan, verbose
    python3 scripts/check-site-links.py --quiet    # violations only

Exit status is non-zero when any violation is found, so it can run as a
gate (site/_gen.py invokes it after regenerating; the pre-commit hook runs
it when staged changes touch site/).
"""
from __future__ import annotations

import argparse
import re
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

REPO_ROOT = Path(__file__).resolve().parent.parent
SITE = REPO_ROOT / "site"

# Directories under site/ that are inputs, not published pages.
SKIP_DIRS = {"_content"}

EXTERNAL_SCHEMES = ("http:", "https:", "mailto:", "data:", "javascript:")


class _LinkParser(HTMLParser):
    """Collect (attr, value, line) link targets + element ids of one page."""

    def __init__(self) -> None:
        super().__init__()
        self.links: list[tuple[str, str, int]] = []
        self.ids: set[str] = set()

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        line = self.getpos()[0]
        for name, value in attrs:
            if value is None:
                continue
            if name in ("href", "src"):
                self.links.append((name, value, line))
            elif name == "id":
                self.ids.add(value)
            elif name == "name" and tag == "a":
                self.ids.add(value)


def _parse(path: Path) -> _LinkParser:
    parser = _LinkParser()
    parser.feed(path.read_text(encoding="utf-8", errors="replace"))
    return parser


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--quiet", action="store_true", help="print violations only")
    args = ap.parse_args()

    pages = [
        p
        for p in sorted(SITE.rglob("*.html"))
        if not any(part in SKIP_DIRS for part in p.relative_to(SITE).parts)
    ]
    parsed: dict[Path, _LinkParser] = {p: _parse(p) for p in pages}

    violations: list[str] = []
    checked = 0
    for page, doc in parsed.items():
        rel_page = page.relative_to(REPO_ROOT)
        for _attr, raw, line in doc.links:
            target = raw.strip()
            if not target or target.startswith(EXTERNAL_SCHEMES) or target.startswith("//"):
                continue
            split = urlsplit(target)
            frag = split.fragment
            path_part = unquote(split.path)
            checked += 1

            if not path_part:
                # Bare fragment: anchor in this same page.
                if frag and frag not in doc.ids:
                    violations.append(f"{rel_page}:{line}: missing anchor '#{frag}' (in-page)")
                continue

            resolved = (page.parent / path_part).resolve()
            if resolved.is_dir():
                resolved = resolved / "index.html"
            if not resolved.exists():
                violations.append(f"{rel_page}:{line}: broken link '{raw}'")
                continue
            if frag and resolved.suffix == ".html":
                tgt_doc = parsed.get(resolved)
                if tgt_doc is None and resolved.is_relative_to(SITE):
                    tgt_doc = parsed.setdefault(resolved, _parse(resolved))
                if tgt_doc is not None and frag not in tgt_doc.ids:
                    violations.append(
                        f"{rel_page}:{line}: missing anchor '{path_part}#{frag}'"
                    )

    for v in violations:
        print(v)
    if not args.quiet:
        print(
            f"[check-site-links] {len(pages)} pages, {checked} internal links, "
            f"{len(violations)} violation(s)"
        )
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
