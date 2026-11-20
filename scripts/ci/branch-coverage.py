#!/usr/bin/env python3
"""Branch coverage of the retail routines the kernel differential runs.

`differential-coverage.py` counts which `// PORT:` routines a differential
enters; this counts how much of each one it exercises. The interpreter
(`crates/retail-exec/src/branch_cov.rs`) walks each entered routine's control
flow statically to enumerate its conditional branches, and notes which way
each went while the suite ran. Here the per-test tables are folded together
per routine image.

    branch-coverage.py --record     # run the suite with recording on (disc)
    branch-coverage.py              # summary over the PORT-tagged routines
    branch-coverage.py --worst 30   # the 30 lowest-covered routines
    branch-coverage.py --addr 801D0748  # one routine's branches

A branch is **both** (taken and fell through), **one** (only one way) or
**unreached**. Direction coverage is (2*both + one) / (2*branches). The file
holds addresses and hashes, no instruction bytes; it stays under `target/`.

A routine's figure is the whole routine's. A dispatcher several ports tag for
the one arm each implements scores low on arms nobody ported, so read a low
dispatcher with `--addr` before calling it a gap.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from port_catalog.citations import collect_ports  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
TABLE = REPO / "target" / "branch-coverage" / "branches.tsv"


def record(path: Path) -> int:
    if not os.environ.get("LEGAIA_DISC_BIN"):
        print("branch-coverage --record: LEGAIA_DISC_BIN is unset; the suite would skip every test")
        return 2
    path.parent.mkdir(parents=True, exist_ok=True)
    path.unlink(missing_ok=True)
    env = dict(os.environ, PSX_RETAIL_EXEC_BRANCHES=str(path))
    cmd = ["cargo", "test", "-p", "psx-retail-exec", "--profile", "release-test", "--features", "branch-cov",
           "--test", "integration"]
    print("+", " ".join(cmd), f"(PSX_RETAIL_EXEC_BRANCHES={path})")
    rc = subprocess.run(cmd, cwd=REPO, env=env).returncode
    if rc != 0:
        print("branch-coverage --record: the suite failed; the table covers the tests that ran")
    return rc


class Routine:
    def __init__(self, entry: str, branches: int, jump_tables: int, truncated: bool):
        self.entry = entry
        self.branches = branches
        self.jump_tables = jump_tables
        self.truncated = truncated
        self.masks: dict[str, int] = defaultdict(int)
        self.tests: set[str] = set()

    def counts(self) -> tuple[int, int, int]:
        both = sum(1 for m in self.masks.values() if m == 3)
        one = sum(1 for m in self.masks.values() if m in (1, 2))
        return both, one, self.branches - both - one

    def coverage(self) -> float:
        both, one, _ = self.counts()
        return (2 * both + one) / (2 * self.branches) if self.branches else 1.0


def read(path: Path) -> dict[tuple[str, str], Routine]:
    out: dict[tuple[str, str], Routine] = {}
    for line in path.read_text().splitlines():
        f = line.split("\t")
        key = (f[2], f[3])
        if f[0] == "R":
            r = out.setdefault(key, Routine(f[2], int(f[4]), int(f[5]), f[6] == "1"))
            r.tests.add(f[1])
        elif f[0] == "B":
            out[key].masks[f[4]] |= int(f[5])
    return out


def summary(label: str, rs: list[Routine]) -> None:
    n = sum(r.branches for r in rs)
    both = one = none = 0
    for r in rs:
        b, o, z = r.counts()
        both, one, none = both + b, one + o, none + z
    print(f"[{label}] {len(rs)} routine image(s): {sum(1 for r in rs if r.branches == 0)} without a "
          f"conditional branch, {sum(1 for r in rs if r.jump_tables)} with a jump table, "
          f"{sum(1 for r in rs if r.truncated)} walk(s) truncated")
    if not n:
        return
    print(f"  {n} conditional branches: both ways {both} ({both / n:.1%}), one way {one} ({one / n:.1%}), "
          f"unreached {none} ({none / n:.1%})")
    print(f"  direction coverage {(2 * both + one) / (2 * n):.1%}")


def bucket(c: float) -> str:
    if c == 1:
        return "100%"
    if c >= 0.9:
        return "90-99%"
    if c >= 0.75:
        return "75-89%"
    if c >= 0.5:
        return "50-74%"
    return "<50%"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--record", action="store_true", help="run the suite with recording on (needs the disc)")
    ap.add_argument("--table", type=Path, default=TABLE, help="the recorded table")
    ap.add_argument("--worst", type=int, default=15, help="list this many lowest-covered routines")
    ap.add_argument("--addr", help="one routine's branches")
    args = ap.parse_args()
    if args.record:
        return record(args.table)
    if not args.table.exists():
        print(f"branch-coverage: no table at {args.table}; run --record first")
        return 2
    table = read(args.table)
    ported = {a.lower() for a in collect_ports()}
    tagged = [r for r in table.values() if r.entry in ported]

    if args.addr:
        want = args.addr.lower().removeprefix("fun_").removeprefix("0x")
        for (entry, h), r in table.items():
            if entry != want:
                continue
            b, o, z = r.counts()
            print(f"{entry} image {h}: {r.branches} branches, both {b}, one {o}, unreached {z}, "
                  f"coverage {r.coverage():.1%}, jump tables {r.jump_tables}; tests: {', '.join(sorted(r.tests))}")
            for pc, m in sorted(r.masks.items()):
                print(f"  {pc}  {('unreached', 'taken only', 'fell through only', 'both')[m]}")
        return 0

    print(f"{len({r.entry for r in tagged})} of {len(ported)} PORT-tagged addresses recorded")
    summary("PORT-tagged", tagged)
    summary("everything entered", list(table.values()))
    with_branches = [r for r in tagged if r.branches]
    print("  PORT-tagged routines by direction coverage:",
          ", ".join(f"{k} {v}" for k, v in sorted(Counter(bucket(r.coverage()) for r in with_branches).items())))
    print(f"\nlowest {args.worst} (entry, branches, both, one, unreached, coverage):")
    for r in sorted(with_branches, key=lambda r: (r.coverage(), -r.branches))[: args.worst]:
        b, o, z = r.counts()
        print(f"  {r.entry}  {r.branches:4d} {b:4d} {o:4d} {z:4d}  {r.coverage():.0%}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
