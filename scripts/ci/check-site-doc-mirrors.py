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

A heading-level check is not enough, and the same page proved it twice. With
every `##` area claimed, the mirror of `open-rev-eng-threads.md` still showed
**two** of the page's twelve live hunts: its `Audio / BGM`, `Measurement +
tooling` and `Title / boot / overlays` tables were present and *empty*, its
battle table carried a thread that had since closed, and its `Field /
locomotion` table carried one row where the source carries five. Every area was
"covered"; the live-hunt list a reader saw was a sixth of the real one. A
section is the wrong unit for a page whose content is rows.

So the row half runs too. A **live row** is a row of a `| Thread | Status | ...`
table whose status cell opens `open`, `partial` or `mostly resolved` - the three
statuses `open-rev-eng-threads.md` defines - and each one must be claimed by a
`data-row` attribute naming the slug of its first cell:

    <tr data-row="where-does-prot-0896-link"><td>Where does PROT 0896 link?</td>...

The rule scopes itself: the settled and falsified registers hold hundreds of
thread rows and **no** live ones, so they are checked and cost nothing, while a
live row appearing on one of them is a finding rather than an exemption. As
with `data-doc`, a `data-row` naming no live row is an error - that is exactly
what a closed thread still advertised as open looks like, and it is the half of
the drift that "did you cover everything" cannot see.

What it deliberately does NOT check: prose, wording, or whether the mirror says
the same thing about a row it claims. A row can be present and wrong. This gate
answers exactly two questions - is a whole area, or a live hunt, missing from
the public page - because those are the failures that survived review.

Usage:
    python3 scripts/ci/check-site-doc-mirrors.py             # gate
    python3 scripts/ci/check-site-doc-mirrors.py --selftest  # controls only
    python3 scripts/ci/check-site-doc-mirrors.py --list      # print the map

Exit status is non-zero when a mirror is missing an area or a live row of its
source. Adding a mirror pair below is how a new page joins the gate.
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
    ("docs/tooling/byte-accounting.md",
     "site/_content/tooling/byte-accounting.html"),
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
DATA_ROW_RE = re.compile(r"""data-row\s*=\s*["']([^"']*)["']""", re.IGNORECASE)
TR_RE = re.compile(r"<tr\b[^>]*>", re.IGNORECASE)
FENCE_RE = re.compile(r"^(```|~~~)")

# A status cell that opens with one of these is a **live hunt**. The three are
# `open-rev-eng-threads.md`'s own status vocabulary; `resolved` and `falsified`
# (the settled / do-not-re-walk registers) deliberately are not, which is what
# keeps those two pages' several hundred thread rows out of the row check
# without an exemption list.
LIVE_STATUS_RE = re.compile(r"^(open|partial|mostly\s+resolved)\b", re.IGNORECASE)


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


def strip_markup(cell: str) -> str:
    """A markdown table cell reduced to its text - links, emphasis, code."""
    cell = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", cell)
    cell = cell.replace("`", "")
    cell = re.sub(r"\*\*([^*]*)\*\*", r"\1", cell)
    cell = re.sub(r"\*([^*]*)\*", r"\1", cell)
    return cell.strip()


def live_rows(md: str) -> list[tuple[str, str, str]]:
    """Every **live** thread row of a page, as `(slug, thread, status)`.

    A thread table is one whose header's first cell is `Thread`; the three
    reference pages each use a different set of columns after it, so the
    header's own first cell is what identifies the table rather than a column
    count. Fenced blocks are skipped, and a row is live when its status cell
    (column 2 on all three pages) opens with one of [`LIVE_STATUS_RE`]'s words.
    """
    out: list[tuple[str, str, str]] = []
    header: list[str] | None = None
    in_fence = False
    for line in md.splitlines():
        s = line.strip()
        if FENCE_RE.match(s):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if not s.startswith("|"):
            header = None
            continue
        cells = [c.strip() for c in s.strip("|").split("|")]
        if header is None:
            header = cells
            continue
        if all(set(c) <= set("-: ") for c in cells):
            continue  # the `|---|---|` separator
        if not header or strip_markup(header[0]).lower() != "thread" or len(cells) < 2:
            continue
        thread, status = strip_markup(cells[0]), strip_markup(cells[1])
        if LIVE_STATUS_RE.match(status):
            out.append((slug(cells[0]), thread, status))
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


def mirror_row_claims(html: str) -> list[str]:
    """Every slug claimed by a `data-row` attribute on a `<tr>`."""
    claims = []
    for tag in TR_RE.findall(html):
        m = DATA_ROW_RE.search(tag)
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
    problems.extend(
        check_rows(
            doc_rel,
            site_rel,
            doc_path.read_text(encoding="utf-8"),
            site_path.read_text(encoding="utf-8"),
        )
    )
    return problems


def check_rows(doc_rel: str, site_rel: str, md: str, html: str) -> list[str]:
    """The row half: every live hunt of the source claimed by the mirror."""
    problems: list[str] = []
    rows = live_rows(md)
    claimed = set(mirror_row_claims(html))
    known = {s for s, _, _ in rows}
    for s, thread, status in rows:
        if s in claimed or (site_rel, s) in WAIVED:
            continue
        problems.append(
            f"{site_rel}: nothing mirrors {doc_rel} live row \"{thread}\" "
            f"({status}; slug {s!r}). Add a <tr data-row=\"{s}\"> to the "
            f"matching table, or waive it in WAIVED with a reason."
        )
    for s in sorted(claimed - known):
        problems.append(
            f"{site_rel}: data-row={s!r} names no live row in {doc_rel} - the "
            f"thread closed, or its wording changed. A closed hunt shown as "
            f"open is the drift this half exists for: drop the row (move it "
            f"into the section's closed-threads prose) or re-point it."
        )
    return problems


SELFTEST_DOC = """## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Does `edbylon` miss? | open - one state disagrees | A capture. |
| Region gate families | partial - structure settled | [details](#x) |
| Old hunt | resolved (it is the emitter) | - |

| Page | Holds | Read it when |
|---|---|---|
| This page | open, partial | Picking up work. |
"""


def selftest() -> int:
    """Positive controls. The section half must fire on a mirror that drops a
    heading and on one that claims a heading the doc no longer has; the row
    half must fire on the same two shapes for a **live** row, and must not
    admit a `resolved` row or a non-thread table into its denominator."""
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

    # Row half. Two live rows out of a page that also carries a resolved
    # thread row and a `| Page |` table - neither of which may be counted.
    rows = live_rows(SELFTEST_DOC)
    if [s for s, _, _ in rows] != ["does-edbylon-miss", "region-gate-families"]:
        fails.append(f"live_rows() wrong: {rows}")
    fenced = live_rows("```\n| Thread | Status |\n|---|---|\n| F | open |\n```\n")
    if fenced:
        fails.append(f"live_rows() reads a fenced table: {fenced}")
    got = mirror_row_claims('<tr data-row="a"><tr data-row="b, c"><tr id="plain">')
    if got != ["a", "b", "c"]:
        fails.append(f"mirror_row_claims() wrong: {got}")
    # The detector fires on a dropped live row...
    dropped = check_rows("d", "s", SELFTEST_DOC, '<tr data-row="does-edbylon-miss">')
    if not any("region-gate-families" in p for p in dropped):
        fails.append(f"check_rows() misses a dropped live row: {dropped}")
    # ...and on a claim naming a row that is no longer live.
    stale = check_rows("d", "s", SELFTEST_DOC,
                       '<tr data-row="does-edbylon-miss">'
                       '<tr data-row="region-gate-families">'
                       '<tr data-row="old-hunt">')
    if not any("old-hunt" in p for p in stale):
        fails.append(f"check_rows() admits a closed thread as live: {stale}")
    for f in fails:
        print(f"SELFTEST FAIL: {f}")
    if fails:
        return 1
    print("selftest: section and row detectors both fire on both drift shapes")
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
            html = (REPO / site_rel).read_text(encoding="utf-8")
            md = (REPO / doc_rel).read_text(encoding="utf-8")
            claims = set(mirror_claims(html))
            rows = set(mirror_row_claims(html))
            print(f"\n{doc_rel} -> {site_rel}")
            for s, title in doc_sections(md):
                print(f"  [{'x' if s in claims else ' '}] {s:45s} {title}")
            live = live_rows(md)
            if live:
                print(f"  live rows ({sum(s in rows for s, _, _ in live)}/{len(live)} claimed):")
                for s, thread, _ in live:
                    print(f"    [{'x' if s in rows else ' '}] {s:43s} {thread}")
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
