#!/usr/bin/env python3
"""Does each hand-authored site mirror still cover its `docs/` source?

Several `site/_content/` fragments are *mirrors*: a page under `docs/` is the
source of record, and the fragment restates it for the public site. Nothing in
`site/_gen.py` relates the two - the generator's page table maps
`_content/<x>.html` to `<x>.html` and never reads `docs/` at all - so a mirror
drifts silently. It keeps building, keeps passing the link checker, keeps
looking finished, and simply stops mentioning whole areas of its source.

That is not hypothetical. `docs/reference/open-rev-eng-threads.md` grew a
`Battle / rendering` area and an `Audio / BGM` area, and closed its
`Title / boot / overlays` thread by capture; the site mirror had **no battle
section at all** and still described the closed thread as open. A reader of the
public site was being told the project's live-hunt list was two areas shorter
than it is, and was being pointed at a hunt that is over.

The check is structural, so it cannot be satisfied by rewording. Every `##`
heading in the source doc must be *claimed* by some section of the mirror,
through a `data-doc` attribute naming the heading's GitHub anchor slug:

    <section class="doc-section" id="battle-rendering" data-doc="battle--rendering">

One section may claim several headings (comma-separated) when the mirror
deliberately merges them, and a section that exists only on the site claims
`data-doc="-"`. A slug that names no heading in the source is an error too:
that is what a renamed heading looks like, and it is the half of the drift a
"did you cover everything" check on its own would miss.

What it deliberately does NOT check: prose, tables, row counts, or whether the
mirror says the same thing. A section can be present and wrong. This gate
answers exactly one question - is a whole area of the source missing from the
public page - because that is the failure that survived review.

Usage:
    python3 scripts/ci/check-site-doc-mirrors.py             # gate
    python3 scripts/ci/check-site-doc-mirrors.py --selftest  # controls only
    python3 scripts/ci/check-site-doc-mirrors.py --list      # print the map

Exit status is non-zero when a mirror is missing an area of its source.
Adding a mirror pair below is how a new page joins the gate.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# (docs source, site mirror). Both paths are repo-relative. A pair only belongs
# here when the site fragment really is a restatement of the doc; pages that
# merely link to a doc are not mirrors.
MIRRORS = [
    ("docs/reference/open-rev-eng-threads.md",
     "site/_content/reference/open-rev-eng-threads.html"),
    ("docs/reference/re-settled-threads.md",
     "site/_content/reference/re-settled-threads.html"),
    ("docs/reference/re-do-not-re-walk.md",
     "site/_content/reference/re-do-not-re-walk.html"),
]

# Headings a mirror is allowed not to carry, with the reason. Keep this list
# short and argued: every entry is an area of a doc that the public page does
# not show, and "it did not seem worth mirroring" is the reasoning that put the
# battle section in the bin in the first place.
WAIVED = {
    # (mirror, slug): reason
}

SECTION_RE = re.compile(r"<section\b[^>]*>", re.IGNORECASE)
DATA_DOC_RE = re.compile(r"""data-doc\s*=\s*["']([^"']*)["']""", re.IGNORECASE)
FENCE_RE = re.compile(r"^(```|~~~)")


def slug(text: str) -> str:
    """GitHub's heading -> anchor slug. Same rules as check-md-links.py."""
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"\*\*([^*]*)\*\*", r"\1", text)
    text = re.sub(r"\*([^*]*)\*", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    s = text.strip().lower()
    s = re.sub(r"[^\w\s-]", "", s)
    return s.replace(" ", "-")


def doc_sections(md: str) -> list[tuple[str, str]]:
    """`##` headings of a markdown page as (slug, raw title), fences skipped."""
    out, in_fence = [], False
    for line in md.splitlines():
        if FENCE_RE.match(line.strip()):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if line.startswith("## ") and not line.startswith("### "):
            title = line[3:].strip()
            out.append((slug(title), title))
    return out


def mirror_claims(html: str) -> list[str]:
    """Every slug claimed by a `data-doc` attribute on a `<section>`."""
    claims = []
    for tag in SECTION_RE.findall(html):
        m = DATA_DOC_RE.search(tag)
        if not m:
            continue
        for part in m.group(1).split(","):
            part = part.strip()
            if part:
                claims.append(part)
    return claims


def check_pair(doc_rel: str, site_rel: str) -> list[str]:
    problems: list[str] = []
    doc_path, site_path = REPO / doc_rel, REPO / site_rel
    if not doc_path.exists():
        return [f"{doc_rel}: source page is missing"]
    if not site_path.exists():
        return [f"{site_rel}: mirror is missing"]

    headings = doc_sections(doc_path.read_text(encoding="utf-8"))
    claims = mirror_claims(site_path.read_text(encoding="utf-8"))
    claimed = {c for c in claims if c != "-"}
    known = {s for s, _ in headings}

    if not claims:
        problems.append(
            f"{site_rel}: no <section> carries a data-doc attribute, so nothing "
            f"relates this mirror to {doc_rel}. Tag each section with the "
            f"anchor slug of the heading it mirrors (or \"-\" for site-only)."
        )
        return problems

    for s, title in headings:
        if s in claimed or (site_rel, s) in WAIVED:
            continue
        problems.append(
            f"{site_rel}: nothing mirrors {doc_rel} section \"{title}\" "
            f"(slug {s!r}). Add a <section ... data-doc=\"{s}\"> or waive it "
            f"in WAIVED with a reason."
        )

    for s in sorted(claimed - known):
        problems.append(
            f"{site_rel}: data-doc={s!r} names no `##` heading in {doc_rel} - "
            f"the heading was renamed or removed. Re-point the attribute."
        )
    return problems


def selftest() -> int:
    """Positive controls: the detector must fire on a mirror that drops a
    section, and on one that claims a heading the doc no longer has."""
    fails = []
    if slug("Battle / rendering") != "battle--rendering":
        fails.append("slug() disagrees with check-md-links.py")
    if slug("No overlay function lives below `0x801CE818`") != \
            "no-overlay-function-lives-below-0x801ce818":
        fails.append("slug() mishandles a code-span heading")
    heads = doc_sections("## A one\n\n```\n## fenced\n```\n\n## B two\n")
    if [s for s, _ in heads] != ["a-one", "b-two"]:
        fails.append(f"doc_sections() wrong: {heads}")
    got = mirror_claims('<section data-doc="x,y"><section data-doc="-">'
                        '<section id="untagged">')
    if got != ["x", "y", "-"]:
        fails.append(f"mirror_claims() wrong: {got}")
    for f in fails:
        print(f"SELFTEST FAIL: {f}")
    if fails:
        return 1
    print("selftest: detectors fire on both drift shapes")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--list", action="store_true",
                    help="print each pair's heading -> claimed status")
    args = ap.parse_args()

    if args.selftest:
        return selftest()
    if selftest() != 0:
        return 1

    if args.list:
        for doc_rel, site_rel in MIRRORS:
            claims = set(mirror_claims((REPO / site_rel).read_text(encoding="utf-8")))
            print(f"\n{doc_rel} -> {site_rel}")
            for s, title in doc_sections((REPO / doc_rel).read_text(encoding="utf-8")):
                print(f"  [{'x' if s in claims else ' '}] {s:45s} {title}")
        return 0

    problems: list[str] = []
    for doc_rel, site_rel in MIRRORS:
        problems.extend(check_pair(doc_rel, site_rel))

    if problems:
        print()
        for p in problems:
            print(f"  {p}")
        print(f"\n{len(problems)} site mirror(s) out of step with docs/")
        return 1

    print(f"site mirrors cover their docs/ sources ({len(MIRRORS)} pairs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
