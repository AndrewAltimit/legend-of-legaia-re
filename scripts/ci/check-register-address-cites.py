#!/usr/bin/env python3
"""Worklist: settled-register rows graded `disassembly` that cite no address.

`docs/reference/re-settled-threads/*.md` grades every answered question by
what its evidence rests on, and `disassembly` is the strongest grade. A row
can carry that grade while its cell names only a `FUN_x` and a behaviour -
which reads exactly like a checked row and is not one. Re-audits of such rows
have found claims the instruction listing contradicts.

This lists them. A row counts as *cited* when the text that answers it holds
at least one instruction-shaped address (`0x80xxxxxx`): the answer cell
itself, or - for a row whose cell is a `[details ↓](#anchor)` link - the
`###` section that anchor names on the same page.

It is a worklist, not a gate: it always exits 0. Many of the rows it prints
are not code claims at all (a disc-data census, a tool's behaviour, a capture)
and carry `disassembly` beside another kind of evidence; those want a
different grade or nothing, not an address. The count is what makes the
bucket measurable.

    python3 scripts/ci/check-register-address-cites.py            # list
    python3 scripts/ci/check-register-address-cites.py --summary  # per-page counts
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PAGES = REPO / "docs" / "reference" / "re-settled-threads"

ADDR = re.compile(r"0x80[0-9A-Fa-f]{6}")
DETAILS = re.compile(r"\[details[^\]]*\]\(#([^)]+)\)")


def slug(heading: str) -> str:
    """GitHub's anchor slug for a heading line's text."""
    text = heading.strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def sections(lines: list[str]) -> dict[str, str]:
    """Map each `###` heading's slug to the text of its section."""
    out: dict[str, str] = {}
    current: str | None = None
    buf: list[str] = []
    for line in lines:
        if line.startswith("### "):
            if current is not None:
                out[current] = "\n".join(buf)
            current = slug(line[4:])
            buf = []
        elif line.startswith("## "):
            if current is not None:
                out[current] = "\n".join(buf)
            current = None
            buf = []
        elif current is not None:
            buf.append(line)
    if current is not None:
        out[current] = "\n".join(buf)
    return out


def uncited_rows(path: Path) -> tuple[int, list[tuple[int, str, str, bool]]]:
    """Return (graded-row count, [(line, grade, title, has_details)])."""
    lines = path.read_text(encoding="utf-8").split("\n")
    secs = sections(lines)
    graded = 0
    found: list[tuple[int, str, str, bool]] = []
    for number, line in enumerate(lines, 1):
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.split("|")]
        if len(cells) < 6 or "`disassembly`" not in cells[3]:
            continue
        graded += 1
        answer = " ".join(cells[4:])
        link = DETAILS.search(answer)
        text = answer
        if link:
            text += secs.get(link.group(1), "")
        if not ADDR.search(text):
            found.append((number, cells[3], cells[1], bool(link)))
    return graded, found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--summary", action="store_true", help="per-page counts only")
    args = parser.parse_args()

    total_graded = 0
    total_found = 0
    for path in sorted(PAGES.glob("*.md")):
        graded, found = uncited_rows(path)
        total_graded += graded
        total_found += len(found)
        rel = path.relative_to(REPO)
        if args.summary:
            print(f"{rel}: {len(found)} of {graded} `disassembly` rows cite no address")
            continue
        for number, grade, title, has_details in found:
            mark = " [details]" if has_details else ""
            print(f"{rel}:{number}: {grade}{mark} | {title}")
    print(
        f"[register-address-cites] {total_found} of {total_graded} `disassembly` "
        "rows cite no instruction address (worklist, not a failure)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
