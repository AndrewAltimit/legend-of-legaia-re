#!/usr/bin/env python3
"""Checked arms: which covered branch directions a differential's comparison sees.

`branch-coverage.py` counts a direction once the retail side takes it. That
does not show the test then compares the arm's effect with the Rust that ports
it. This measures it by mutation. The interpreter, built with the
`branch-flip` feature (`crates/retail-exec/src/branch_flip.rs`), inverts one
conditional branch of one routine image on the retail side only; the tests
that took that direction are rerun with the flip. A test that now FAILS sees
the arm: the direction is **checked**. One that still passes does not: the
direction is **unchecked**, because the arm's writes fall outside what the
test compares or because both directions compute the same thing (an
equivalent mutant). A flipped run that stops other than by returning (the step
budget, a bad access) or times out is **abnormal**: detected, but by the run
aborting, not by the comparison.

    checked-arms.py --record                 # build the flip binary, record its branch table (disc)
    checked-arms.py --sample 400 --seed 58   # mutate a seeded sample of ported, covered directions
    checked-arms.py --addr 801D0748          # every ported, covered direction of one routine
    checked-arms.py --all                    # the whole population (see --cost)
    checked-arms.py --cost                   # population size and the --all estimate
    checked-arms.py                          # summary + per-routine worklist of the results

Each direction first flips its first execution; when every test survives that,
it flips every execution. Up to `--tests` of the cheapest tests that took the
direction run per mutant. The population is the ported arms of
`branch-coverage.py`: directions taken in a PORT-tagged routine, outside every
`branch-coverage-scope/*.toml` row. Results land in
`target/checked-arms/results.tsv`, addresses and hashes only. It is a
measurement, not a gate.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import os
import random
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
OUT = REPO / "target" / "checked-arms"
TABLE = OUT / "branches.tsv"
PASSED = OUT / "baseline-passed.txt"
BINARY = OUT / "binary"
RESULTS = OUT / "results.tsv"
FEATURES = "branch-cov,branch-flip"

sys.path.insert(0, str(HERE))
_spec = importlib.util.spec_from_file_location("branch_coverage", HERE / "branch-coverage.py")
bc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bc)


# --- building and recording -----------------------------------------------------

def build() -> Path:
    cmd = ["cargo", "test", "-p", "psx-retail-exec", "--profile", "release-test", "--features", FEATURES,
           "--test", "integration", "--no-run", "--message-format=json"]
    print("+", " ".join(cmd))
    p = subprocess.run(cmd, cwd=REPO, stdout=subprocess.PIPE, text=True)
    exe = None
    for line in p.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "integration":
            exe = m["executable"]
    if p.returncode or not exe:
        sys.exit("checked-arms: the build failed")
    OUT.mkdir(parents=True, exist_ok=True)
    BINARY.write_text(exe + "\n")
    return Path(exe)


def binary() -> Path:
    if BINARY.exists():
        exe = Path(BINARY.read_text().strip())
        if exe.exists():
            return exe
    return build()


def parse_results(stdout: str) -> dict[str, str]:
    """`test name ... ok|FAILED|ignored` lines, by test."""
    out = {}
    for line in stdout.splitlines():
        if line.startswith("test ") and " ... " in line:
            name, _, verdict = line[5:].partition(" ... ")
            out[name.strip()] = verdict.strip().split()[0] if verdict.strip() else "?"
    return out


def record(threads: int) -> int:
    if not os.environ.get("LEGAIA_DISC_BIN"):
        print("checked-arms --record: LEGAIA_DISC_BIN is unset; the suite would skip every test")
        return 2
    exe = build()
    TABLE.unlink(missing_ok=True)
    env = dict(os.environ, PSX_RETAIL_EXEC_BRANCHES=str(TABLE))
    print(f"+ {exe} --test-threads={threads} (PSX_RETAIL_EXEC_BRANCHES={TABLE})")
    p = subprocess.run([str(exe), f"--test-threads={threads}"], cwd=REPO / "crates" / "retail-exec", env=env,
                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    res = parse_results(p.stdout)
    passed = sorted(t for t, v in res.items() if v == "ok")
    PASSED.write_text("\n".join(passed) + "\n")
    failed = sorted(t for t, v in res.items() if v == "FAILED")
    print(f"checked-arms --record: {len(passed)} passed, {len(failed)} failed (failing tests are never mutated)")
    for t in failed:
        print(f"  FAILED {t}")
    return 0


# --- the population ---------------------------------------------------------------

class Direction:
    __slots__ = ("entry", "image", "pc", "taken", "tests")

    def __init__(self, entry: str, image: str, pc: str, taken: bool):
        self.entry, self.image, self.pc, self.taken = entry, image, pc, taken
        self.tests: set[str] = set()

    @property
    def key(self) -> tuple[str, str, str, str]:
        return (self.entry, self.image, self.pc, "taken" if self.taken else "fell")


def population(only_entry: str | None = None) -> tuple[list[Direction], dict[str, int]]:
    """Ported, covered directions with the passing tests that took each, and each test's run time (ms)."""
    if not TABLE.exists():
        sys.exit(f"checked-arms: no table at {TABLE}; run --record first")
    passed = set(PASSED.read_text().split()) if PASSED.exists() else None
    table = bc.read(TABLE)
    bc.apply_scope(table, bc.load_scope())
    ported = {a.lower() for a in bc.collect_ports()}
    ms: dict[str, int] = {}
    per: dict[tuple, Direction] = {}
    for line in TABLE.read_text().splitlines():
        f = line.split("\t")
        if f[0] == "T":
            ms[f[1]] = max(ms.get(f[1], 0), int(f[2]))
            continue
        if f[0] != "B":
            continue
        test, entry, image, pc, mask = f[1], f[2], f[3], f[4], int(f[5])
        if entry not in ported or (only_entry and entry != only_entry):
            continue
        if pc in table[(entry, image)].scoped or (passed is not None and test not in passed):
            continue
        for bit, taken in ((1, True), (2, False)):
            if mask & bit:
                k = (entry, image, pc, taken)
                d = per.get(k) or per.setdefault(k, Direction(entry, image, pc, taken))
                d.tests.add(test)
    dirs = sorted(per.values(), key=lambda d: d.key)
    return dirs, ms


# --- running a mutant -------------------------------------------------------------

def run_mutant(exe: Path, d: Direction, which: str, tests: list[str], timeout: float) -> tuple[dict[str, str], float]:
    """Each test's outcome: detected / abnormal / survived / unreached / timeout."""
    with tempfile.NamedTemporaryFile("w+", dir=OUT, prefix="flip-", suffix=".log", delete=False) as lf:
        log = Path(lf.name)
    env = dict(os.environ, PSX_RETAIL_EXEC_FLIP=f"{d.entry}:{d.image}:{d.pc}:{d.key[3]}:{which}",
               PSX_RETAIL_EXEC_FLIP_LOG=str(log))
    env.pop("PSX_RETAIL_EXEC_BRANCHES", None)
    env.pop("PSX_RETAIL_EXEC_LEDGER", None)
    cmd = [str(exe), "--exact", f"--test-threads={max(2, len(tests))}", *tests]
    t0 = time.monotonic()
    try:
        p = subprocess.run(cmd, cwd=REPO / "crates" / "retail-exec", env=env, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, text=True, timeout=timeout)
        res = parse_results(p.stdout)
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode(errors="ignore") if isinstance(e.stdout, bytes) else (e.stdout or "")
        res = parse_results(out)
    dt = time.monotonic() - t0
    fired: dict[str, tuple[int, int, int]] = {}
    for line in log.read_text().splitlines():
        f = line.split("\t")
        if f[0] == "F":
            fired[f[1]] = (int(f[2]), int(f[3]), int(f[4]))
    log.unlink(missing_ok=True)
    out: dict[str, str] = {}
    for t in tests:
        v = res.get(t)
        seen, flipped, abnormal = fired.get(t, (0, 0, 0))
        if v is None:
            out[t] = "timeout"
        elif v == "FAILED":
            out[t] = "abnormal" if abnormal else ("detected" if flipped else "unreached-failed")
        elif flipped:
            # A pass after a runaway call: the test does not check the stop.
            out[t] = "survived-abnormal" if abnormal else "survived"
        else:
            out[t] = "unreached"
    return out, dt


def verdict(outcomes: dict[str, str]) -> str:
    vs = set(outcomes.values())
    if "detected" in vs:
        return "checked"
    if vs & {"abnormal", "timeout"}:
        return "abnormal"
    if vs & {"survived", "survived-abnormal"}:
        return "unchecked"
    if "unreached-failed" in vs:
        return "flaky"
    return "unreached"


def mutate(exe: Path, d: Direction, ms: dict[str, int], k: int, timeout: float) -> list[str]:
    tests = sorted(d.tests, key=lambda t: (ms.get(t, 10**9), t))[:k]
    # A test's own run time can pass the floor: give the mutant room for it.
    timeout = max(timeout, 4 * max(ms.get(t, 0) for t in tests) / 1000 + 10)
    total = 0.0
    stage = "first"
    outcomes, dt = run_mutant(exe, d, "1", tests, timeout)
    total += dt
    v = verdict(outcomes)
    if v in ("unchecked", "unreached"):
        stage = "all"
        outcomes, dt = run_mutant(exe, d, "all", tests, timeout)
        total += dt
        v = verdict(outcomes)
    detail = ",".join(f"{t}={o}" for t, o in outcomes.items())
    return [*d.key, v, stage, str(len(d.tests)), f"{total:.2f}", detail]


def done_keys() -> set[tuple]:
    if not RESULTS.exists():
        return set()
    return {tuple(line.split("\t")[:4]) for line in RESULTS.read_text().splitlines() if line}


def run(dirs: list[Direction], ms: dict[str, int], k: int, jobs: int, timeout: float) -> None:
    exe = binary()
    have = done_keys()
    todo = [d for d in dirs if d.key not in have]
    print(f"checked-arms: {len(todo)} mutant(s) to run ({len(dirs) - len(todo)} already in {RESULTS.name}), "
          f"{jobs} at a time, up to {k} test(s) each")
    OUT.mkdir(parents=True, exist_ok=True)
    t0 = time.monotonic()
    with RESULTS.open("a") as out, ThreadPoolExecutor(jobs) as pool:
        futures = [pool.submit(mutate, exe, d, ms, k, timeout) for d in todo]
        for i, fut in enumerate(as_completed(futures), 1):
            row = fut.result()
            out.write("\t".join(row) + "\n")
            out.flush()
            if i % 25 == 0 or i == len(todo):
                el = time.monotonic() - t0
                print(f"  {i}/{len(todo)}  {el:.0f}s elapsed, {el / i:.1f}s per mutant wall")


def recheck(k: int, jobs: int, timeout: float) -> None:
    """Rerun every unchecked direction with up to `k` of the tests that took it."""
    rows = [r.split("\t") for r in RESULTS.read_text().splitlines() if r]
    dirs, ms = population()
    by = {d.key: d for d in dirs}
    todo = [by[tuple(r[:4])] for r in rows
            if r[4] == "unchecked" and tuple(r[:4]) in by]
    print(f"checked-arms --recheck: {len(todo)} unchecked direction(s), up to {k} test(s) each")
    exe = binary()
    with ThreadPoolExecutor(jobs) as pool:
        new = {tuple(row[:4]): row for row in pool.map(lambda d: mutate(exe, d, ms, k, timeout), todo)}
    for r in new.values():
        print(f"  {r[0]} {r[2]}/{r[3]}: {r[4]}")
    RESULTS.write_text("".join("\t".join(new.get(tuple(r[:4]), r)) + "\n" for r in rows))


# --- the report -------------------------------------------------------------------

def wilson(x: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if not n:
        return 0.0, 0.0
    p = x / n
    den = 1 + z * z / n
    mid = (p + z * z / (2 * n)) / den
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / den
    return mid - half, mid + half


def report(rows: list[list[str]], worklist: int) -> None:
    by = defaultdict(int)
    for r in rows:
        by[r[4]] += 1
    n = len(rows)
    decided = by["checked"] + by["abnormal"] + by["unchecked"]
    print(f"{n} mutant(s): checked {by['checked']}, abnormal {by['abnormal']}, unchecked {by['unchecked']}, "
          f"unreached {by['unreached']}, flaky {by['flaky']}")
    if decided:
        lo, hi = wilson(by["checked"], decided)
        print(f"checked arms: {by['checked'] / decided:.1%} of ported directions taken "
              f"(95% CI {lo:.1%}..{hi:.1%}, n = {decided}); "
              f"detected incl. abnormal {(by['checked'] + by['abnormal']) / decided:.1%}")
    secs = [float(r[7]) for r in rows]
    if secs:
        print(f"cost: {sum(secs) / len(secs):.1f}s per mutant (process time, both stages)")
    per = defaultdict(list)
    for r in rows:
        if r[4] == "unchecked":
            per[(r[0], r[1])].append(r)
    if per:
        print(f"\nunchecked directions by routine (top {worklist}):")
        for (e, h), rs in sorted(per.items(), key=lambda kv: -len(kv[1]))[:worklist]:
            arms = " ".join(f"{r[2]}/{r[3][0]}" for r in sorted(rs))
            tests = sorted({t.split("=")[0] for r in rs for t in r[8].split(",")})
            print(f"  {e} {h}: {len(rs)}  {arms}")
            print(f"      tests: {', '.join(tests[:4])}{' ...' if len(tests) > 4 else ''}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--record", action="store_true", help="build the flip binary and record its branch table (disc)")
    ap.add_argument("--sample", type=int, help="mutate this many directions, drawn with --seed")
    ap.add_argument("--seed", type=int, default=58)
    ap.add_argument("--addr", help="mutate every ported, covered direction of one routine")
    ap.add_argument("--all", action="store_true", help="mutate the whole population")
    ap.add_argument("--cost", action="store_true", help="population size and the --all estimate")
    ap.add_argument("--recheck", type=int, metavar="K",
                    help="rerun every unchecked direction with up to K of the tests that took it")
    ap.add_argument("--tests", type=int, default=3, help="tests run per mutant, cheapest first")
    ap.add_argument("--jobs", type=int, default=3, help="mutant processes at once")
    ap.add_argument("--threads", type=int, default=6, help="--record's test threads")
    ap.add_argument("--timeout", type=float, default=60,
                    help="seconds per mutant process at least; four times its tests' recorded run time when longer")
    ap.add_argument("--only", help="report only the result rows of this routine entry")
    ap.add_argument("--worklist", type=int, default=40)
    args = ap.parse_args()
    if args.record:
        return record(args.threads)
    if args.recheck:
        recheck(args.recheck, args.jobs, args.timeout)
        return 0
    if args.sample or args.addr or args.all or args.cost:
        entry = args.addr.lower().removeprefix("fun_").removeprefix("0x") if args.addr else None
        dirs, ms = population(entry)
        if args.cost:
            rows = [r.split("\t") for r in RESULTS.read_text().splitlines()] if RESULTS.exists() else []
            per = sum(float(r[7]) for r in rows) / len(rows) if rows else float("nan")
            print(f"population: {len(dirs)} ported, covered directions in "
                  f"{len({(d.entry, d.image) for d in dirs})} routine image(s)")
            print(f"--all: {len(dirs)} mutants x {per:.1f}s / {args.jobs} jobs = "
                  f"{len(dirs) * per / args.jobs / 3600:.1f} h")
            return 0
        if args.sample:
            dirs = random.Random(args.seed).sample(dirs, min(args.sample, len(dirs)))
        if not os.environ.get("LEGAIA_DISC_BIN"):
            print("checked-arms: LEGAIA_DISC_BIN is unset; every test would skip")
            return 2
        run(dirs, ms, args.tests, args.jobs, args.timeout)
        keys = {d.key for d in dirs}
        rows = [r.split("\t") for r in RESULTS.read_text().splitlines()]
        report([r for r in rows if tuple(r[:4]) in keys], args.worklist)
        return 0
    if not RESULTS.exists():
        print(f"checked-arms: no results at {RESULTS}; run --sample, --addr or --all")
        return 2
    rows = [r.split("\t") for r in RESULTS.read_text().splitlines() if r]
    if args.only:
        want = args.only.lower().removeprefix("fun_").removeprefix("0x")
        rows = [r for r in rows if r[0] == want]
    report(rows, args.worklist)
    return 0


if __name__ == "__main__":
    sys.exit(main())
