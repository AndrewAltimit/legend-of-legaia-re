#!/usr/bin/env python3
"""Branch coverage of the retail routines the kernel differential runs.

`differential-coverage.py` counts which `// PORT:` routines a differential
enters; this counts how much of each one it exercises. The interpreter
(`crates/retail-exec/src/branch_cov.rs`) walks each entered routine's control
flow statically to enumerate its conditional branches, and notes which way
each went while the suite ran. Here the per-test tables are folded together
per routine image.

    branch-coverage.py --record          # run the suite with recording on (disc)
    branch-coverage.py                   # summary over the PORT-tagged routines
    branch-coverage.py --worst 30        # the 30 routines with most untested arms
    branch-coverage.py --addr 801D0748   # one routine's branches
    branch-coverage.py --worksheet 801D0748  # the same, with tag ranges and scope
    branch-coverage.py --check-scope     # every scope row names a recorded routine

A branch is **both** (taken and fell through), **one** (only one way) or
**unreached**. Direction coverage is (2*both + one) / (2*branches). The file
holds addresses and hashes, no instruction bytes; it stays under `target/`.

Not every branch is a test gap. `branch-coverage-scope/*.toml` records the arms
that are out of scope, each with a verdict and a reason: `unported` (no port
implements the arm, by decision), `unreachable` (no game state reaches it),
`cannot-drive` (the interpreter cannot run it). Branches inside a scope row
leave the denominator, and the report's **ported** figure is over the rest.
What remains below 100% there is the worklist: arms a port claims and no test
exercises.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tomllib
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from port_catalog.citations import collect_ports  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
TABLE = REPO / "target" / "branch-coverage" / "branches.tsv"
SCOPE = REPO / "scripts" / "ci" / "branch-coverage-scope"
VERDICTS = ("unported", "unreachable", "cannot-drive")


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
        self.scoped: set[str] = set()

    def counts(self, scoped: bool = False) -> tuple[int, int, int]:
        ms = [m for pc, m in self.masks.items() if not (scoped and pc in self.scoped)]
        both = sum(1 for m in ms if m == 3)
        one = sum(1 for m in ms if m in (1, 2))
        return both, one, len(ms) - both - one

    def coverage(self, scoped: bool = False) -> float:
        both, one, none = self.counts(scoped)
        n = both + one + none
        return (2 * both + one) / (2 * n) if n else 1.0

    def untested(self, scoped: bool = False) -> int:
        """Missing directions: two for an unreached branch, one for a one-way."""
        _, one, none = self.counts(scoped)
        return one + 2 * none


def read(path: Path) -> dict[tuple[str, str], Routine]:
    out: dict[tuple[str, str], Routine] = {}
    for line in path.read_text().splitlines():
        f = line.split("\t")
        if f[0] not in ("R", "B"):
            continue  # `T` (a test's run time) is checked-arms.py's
        key = (f[2], f[3])
        if f[0] == "R":
            r = out.setdefault(key, Routine(f[2], int(f[4]), int(f[5]), f[6] == "1"))
            r.tests.add(f[1])
        elif f[0] == "B":
            out[key].masks[f[4]] |= int(f[5])
    return out


def hexaddr(s: str) -> int:
    return int(s.lower().removeprefix("0x"), 16)


def load_scope(path: Path = SCOPE) -> list[dict]:
    """Every `[[arm]]` row of every `*.toml` in the scope directory, in file order.

    The rows are split across files (one per area or lane) so that parallel
    branches appending rows stop conflicting in one shared file."""
    files = sorted(path.glob("*.toml")) if path.is_dir() else ([path] if path.exists() else [])
    rows = []
    for f in files:
        for row in tomllib.loads(f.read_text()).get("arm", []):
            row["file"] = f.name
            rows.append(row)
    for row in rows:
        lo, _, hi = row["range"].partition("..")
        row["lo"], row["hi"] = hexaddr(lo), hexaddr(hi.removeprefix("=") or lo)
        row["entry"] = row["entry"].lower().removeprefix("fun_").removeprefix("0x")
        row["image"] = row.get("image", "").lower()
    return rows


def apply_scope(table: dict[tuple[str, str], Routine], rows: list[dict]) -> None:
    for (_, h), r in table.items():
        for row in rows:
            if row["entry"] != r.entry or (row["image"] and row["image"] != h):
                continue
            for pc in r.masks:
                if row["lo"] <= int(pc, 16) <= row["hi"]:
                    r.scoped.add(pc)


def check_scope(table: dict[tuple[str, str], Routine], rows: list[dict]) -> int:
    bad = 0
    entries = {r.entry for r in table.values()}
    seen: dict[tuple[str, str, int, int], str] = {}
    for row in rows:
        where = f"{row['file']}: {row['entry']} {row['range']}"
        k = (row["entry"], row["image"], row["lo"], row["hi"])
        if k in seen:
            print(f"scope: {where}: duplicates a row in {seen[k]}")
            bad += 1
        seen.setdefault(k, row["file"])
        if row.get("verdict") not in VERDICTS:
            print(f"scope: {where}: verdict must be one of {', '.join(VERDICTS)}")
            bad += 1
        if not row.get("reason", "").strip():
            print(f"scope: {where}: no reason")
            bad += 1
        if row["hi"] < row["lo"]:
            print(f"scope: {where}: range runs backwards")
            bad += 1
        if row["image"] and not any(e == row["entry"] and h == row["image"] for e, h in table):
            print(f"scope: {where}: no recorded image {row['image']} at that entry")
            bad += 1
            continue
        if row["entry"] not in entries:
            print(f"scope: {where}: no recorded routine at that entry")
            bad += 1
            continue
        hits = sum(1 for (_, h), r in table.items() if r.entry == row["entry"]
                   and (not row["image"] or row["image"] == h)
                   for pc in r.masks if row["lo"] <= int(pc, 16) <= row["hi"])
        if not hits:
            print(f"scope: {where}: holds no conditional branch of the routine")
            bad += 1
    print(f"scope: {len(rows)} row(s), {bad} problem(s)")
    return 1 if bad else 0


# --- tag ranges, for the worksheet ---------------------------------------------

HEX = r"(?:0x)?(8[0-9A-Fa-f]{7})"
RANGE = re.compile(HEX + r"`?\s*\.\.=?\s*`?" + HEX)


def tag_ranges(entry: str) -> list[tuple[str, int, int, str]]:
    """`(file:line, lo, hi, text)` for each address range a `PORT:` tag naming
    `entry` cites, on its line or the comment lines that follow it."""
    out = []
    want = entry.upper()
    for p in (REPO / "crates").rglob("*.rs"):
        try:
            lines = p.read_text(errors="ignore").splitlines()
        except OSError:
            continue
        for i, line in enumerate(lines):
            if "PORT:" not in line or want not in line.upper():
                continue
            text = line
            j = i + 1
            while j < len(lines) and j < i + 12 and lines[j].strip().startswith("//") and "PORT:" not in lines[j]:
                text += " " + lines[j].strip()
                j += 1
            loc = f"{p.relative_to(REPO)}:{i + 1}"
            ranges = RANGE.findall(text)
            for lo, hi in ranges:
                out.append((loc, int(lo, 16), int(hi, 16), line.strip()))
            if not ranges:
                out.append((loc, 0, 0, line.strip()))
    return out


def routine_report(table, entry: str, worksheet: bool, rows: list[dict]) -> None:
    want = entry.lower().removeprefix("fun_").removeprefix("0x")
    tags = tag_ranges(want) if worksheet else []
    for (e, h), r in sorted(table.items()):
        if e != want:
            continue
        b, o, z = r.counts()
        print(f"{e} image {h}: {r.branches} branches, both {b}, one {o}, unreached {z}, "
              f"coverage {r.coverage():.1%} (ported arms {r.coverage(True):.1%}), jump tables {r.jump_tables}")
        print(f"  tests: {', '.join(sorted(r.tests))}")
        if worksheet:
            print("  PORT tags naming it:")
            for loc, lo, hi, line in tags:
                span = f"{lo:08X}..{hi:08X}" if lo else "(no range)"
                print(f"    {span}  {loc}  {line[:110]}")
        for pc, m in sorted(r.masks.items()):
            state = ("unreached", "taken only", "fell through only", "both")[m]
            note = ""
            if worksheet:
                x = int(pc, 16)
                scope = [row for row in rows if row["entry"] == e and row["lo"] <= x <= row["hi"]
                         and (not row["image"] or row["image"] == h)]
                cited = [loc for loc, lo, hi, _ in tags if lo and lo <= x <= hi]
                if scope:
                    note = f"  [scope: {scope[0]['verdict']}]"
                elif cited:
                    note = f"  [cited: {cited[0]}{' +' if len(cited) > 1 else ''}]"
            print(f"    {pc}  {state}{note}")


def summary(label: str, rs: list[Routine], scoped: bool) -> None:
    both = one = none = 0
    for r in rs:
        b, o, z = r.counts(scoped)
        both, one, none = both + b, one + o, none + z
    n = both + one + none
    if not n:
        print(f"[{label}] no conditional branches")
        return
    print(f"[{label}] {n} conditional branches: both ways {both} ({both / n:.1%}), one way {one} "
          f"({one / n:.1%}), unreached {none} ({none / n:.1%}); direction coverage {(2 * both + one) / (2 * n):.1%}")


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
    ap.add_argument("--worst", type=int, default=15, help="list this many routines with the most untested arms")
    ap.add_argument("--addr", help="one routine's branches")
    ap.add_argument("--worksheet", help="one routine's branches with tag ranges and scope rows")
    ap.add_argument("--check-scope", action="store_true", help="validate the rows under branch-coverage-scope/")
    args = ap.parse_args()
    if args.record:
        return record(args.table)
    if not args.table.exists():
        print(f"branch-coverage: no table at {args.table}; run --record first")
        return 2
    table = read(args.table)
    rows = load_scope()
    apply_scope(table, rows)
    if args.check_scope:
        return check_scope(table, rows)
    if args.addr or args.worksheet:
        routine_report(table, args.addr or args.worksheet, bool(args.worksheet), rows)
        return 0

    ported = {a.lower() for a in collect_ports()}
    tagged = [r for r in table.values() if r.entry in ported]
    print(f"{len({r.entry for r in tagged})} of {len(ported)} PORT-tagged addresses recorded; "
          f"{len(tagged)} routine image(s), {sum(1 for r in tagged if r.jump_tables)} with a jump table, "
          f"{sum(1 for r in tagged if r.truncated)} walk(s) truncated")
    summary("all branches", tagged, False)
    scoped_n = sum(len(r.scoped) for r in tagged)
    print(f"  {len(rows)} scope row(s) take {scoped_n} branch(es) out (unported / unreachable / cannot-drive)")
    summary("ported arms", tagged, True)
    with_branches = [r for r in tagged if len(r.masks) - len(r.scoped) > 0]
    print("  routines by ported-arm coverage:",
          ", ".join(f"{k} {v}" for k, v in sorted(Counter(bucket(r.coverage(True)) for r in with_branches).items())))
    print(f"\nmost untested directions in ported arms, top {args.worst} "
          "(entry, branches, scoped, both, one, unreached, ported coverage):")
    for r in sorted(with_branches, key=lambda r: -r.untested(True))[: args.worst]:
        b, o, z = r.counts(True)
        print(f"  {r.entry}  {r.branches:4d} {len(r.scoped):4d} {b:4d} {o:4d} {z:4d}  {r.coverage(True):.0%}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
