#!/usr/bin/env python3
"""Markdown legibility-density checker for committed docs.

Flags two write-optimized-but-not-read-optimized patterns that creep into
long-lived documentation:

  * **Over-long lines** -- any line wider than --max-line characters (default
    800). Usually a run-on sentence or an over-stuffed table row.
  * **Over-budget table cells** -- any single markdown table cell holding more
    than --max-cell-words words (default 150). The fix is to move the cell body
    into a dedicated section (same page) or a sub-page and leave a one-line
    summary + link in the cell.

Scope: every `docs/**/*.md` plus every `crates/*/README.md`. The generated
`crates/web-viewer/pkg/README.md` is skipped. Lines inside fenced code blocks
(```...```) are skipped -- CLI examples and code are allowed to be wide.

The checker **exits non-zero when it finds violations**. The pre-commit hook
runs it on the staged doc set and aborts the commit on a violation (unlike the
warn-only `check-port-tags.py`); bypass an individual commit with
`LEGAIA_SKIP_PRECOMMIT=1`.

Usage:
    scripts/ci/check-doc-density.py                 # scan the whole corpus
    scripts/ci/check-doc-density.py --staged        # only staged md files (hook)
    scripts/ci/check-doc-density.py --quiet          # suppress the success line
    scripts/ci/check-doc-density.py --max-cell-words 120 --max-line 700

Pure standard library; ASCII-only; no external dependencies.
"""

import argparse
import glob
import os
import subprocess
import sys


def in_scope(path):
    """True if path is a doc we lint: docs/**/*.md or crates/<name>/README.md."""
    p = path.replace("\\", "/")
    if not p.endswith(".md"):
        return False
    if p == "crates/web-viewer/pkg/README.md":
        return False
    if p.startswith("docs/"):
        return True
    # crates/<name>/README.md exactly (not a nested README in a subdir)
    parts = p.split("/")
    if len(parts) == 3 and parts[0] == "crates" and parts[2] == "README.md":
        return True
    return False


def corpus_files():
    files = list(glob.glob("docs/**/*.md", recursive=True))
    files += glob.glob("crates/*/README.md")
    return sorted(f for f in files if in_scope(f))


def staged_files():
    out = subprocess.run(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"],
        capture_output=True,
        text=True,
    ).stdout.split()
    return sorted(f for f in out if in_scope(f) and os.path.exists(f))


def is_separator_row(cells):
    """A markdown table header separator like | --- | :--: | (all dashes)."""
    if not cells:
        return False
    for c in cells:
        c = c.strip()
        if not c:
            continue
        if set(c) - set("-: "):
            return False
    return True


def split_cells(line):
    """Split a table row into cell strings. Drops the leading/trailing empties
    produced by the bordering pipes. Naive split on '|' -- a pipe inside an
    inline code span only ever splits a cell into smaller fragments, which can
    under-count but never false-positive -- the safe direction for a commit
    gate (it never wrongly blocks a within-budget cell)."""
    raw = line.split("|")
    # A bordered row "| a | b |" splits to ['', ' a ', ' b ', ''].
    if raw and raw[0].strip() == "":
        raw = raw[1:]
    if raw and raw[-1].strip() == "":
        raw = raw[:-1]
    return raw


def check_file(path, max_line, max_cell_words):
    violations = []
    in_fence = False
    with open(path, encoding="utf-8", errors="replace") as fh:
        for n, line in enumerate(fh, 1):
            line = line.rstrip("\n")
            stripped = line.lstrip()
            if stripped.startswith("```") or stripped.startswith("~~~"):
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            if len(line) > max_line:
                violations.append(
                    (n, "line", len(line), "line is %d chars (> %d)" % (len(line), max_line))
                )
            if stripped.startswith("|"):
                cells = split_cells(line)
                if is_separator_row(cells):
                    continue
                for cell in cells:
                    w = len(cell.split())
                    if w > max_cell_words:
                        violations.append(
                            (n, "cell", w, "table cell is %d words (> %d)" % (w, max_cell_words))
                        )
    return violations


def main():
    ap = argparse.ArgumentParser(description="Markdown legibility-density checker.")
    ap.add_argument("--staged", action="store_true", help="only check staged markdown files")
    ap.add_argument("--quiet", action="store_true", help="suppress the success summary line")
    ap.add_argument("--max-line", type=int, default=800, help="max line width in chars (default 800)")
    ap.add_argument(
        "--max-cell-words", type=int, default=150, help="max words per table cell (default 150)"
    )
    args = ap.parse_args()

    # Run from the repo root so the globs and git paths resolve.
    root = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
    ).stdout.strip()
    if root:
        os.chdir(root)

    files = staged_files() if args.staged else corpus_files()

    total = 0
    for path in files:
        for n, kind, _size, msg in check_file(path, args.max_line, args.max_cell_words):
            print("%s:%d: %s" % (path, n, msg))
            total += 1

    if total:
        print(
            "[check-doc-density] %d density violation(s) across %d file(s)"
            % (total, len(files)),
            file=sys.stderr,
        )
        return 1
    if not args.quiet:
        print("[check-doc-density] OK -- %d file(s) within budget" % len(files))
    return 0


if __name__ == "__main__":
    sys.exit(main())
