#!/usr/bin/env python3
"""Lines one commit of a branch deleted that another commit of the same branch wrote.

A lane branch rebased or re-picked from a stale copy of a file reverts the other
lanes' work with a diff that looks like an edit, and every test can stay green over
it: nothing pins a fix to the commit that made it. This blames each hunk a commit
removed against that commit's parent and prints the runs of at least `--min` lines
that belong to an earlier commit on the branch. A real rewrite shows up as well, so
the list is for reading, not a verdict.

    scripts/ci/revert-scan.py                  # main..HEAD
    scripts/ci/revert-scan.py --base REV
    scripts/ci/revert-scan.py --strict         # exit 1 when anything is listed
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures
import os
import re
import subprocess
import sys


def sh(*args: str) -> str:
    return subprocess.run(args, capture_output=True, text=True, errors="replace").stdout


def scan(commit: str, branch: set[str], minimum: int) -> tuple[str, dict]:
    counts: collections.Counter = collections.Counter()
    path = None
    for line in sh("git", "diff", "-U0", "--no-color", f"{commit}~1", commit).splitlines():
        if line.startswith("--- a/"):
            path = line[6:]
        elif line.startswith("--- "):
            path = None
        elif line.startswith("@@") and path:
            m = re.match(r"@@ -(\d+)(?:,(\d+))?", line)
            start, n = int(m.group(1)), int(m.group(2) or 1)
            if n == 0:
                continue
            blame = sh("git", "blame", "-l", "-s", "-L", f"{start},{start + n - 1}", f"{commit}~1", "--", path)
            for row in blame.splitlines():
                owner = row.split()[0].lstrip("^")
                if owner in branch:
                    counts[(path, owner[:9])] += 1
    return commit, {k: v for k, v in counts.items() if v >= minimum}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--base", default="main")
    ap.add_argument("--min", type=int, default=8, help="smallest run of lines worth listing (default %(default)s)")
    ap.add_argument("--jobs", type=int, default=min(4, os.cpu_count() or 1))
    ap.add_argument("--strict", action="store_true", help="exit 1 when anything is listed")
    a = ap.parse_args()

    commits = sh("git", "rev-list", "--reverse", f"{a.base}..HEAD").split()
    branch = set(commits)
    found = 0
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        for commit, big in pool.map(lambda c: scan(c, branch, a.min), commits):
            if not big:
                continue
            found += 1
            print(commit[:9], sh("git", "log", "-1", "--format=%s", commit).strip()[:60])
            for (path, owner), lines in sorted(big.items(), key=lambda kv: -kv[1])[:8]:
                print(f"    {lines} {path} (from {owner})")
    if not found:
        print(f"revert-scan: nothing over {len(commits)} commits")
    return 1 if found and a.strict else 0


if __name__ == "__main__":
    sys.exit(main())
