#!/usr/bin/env python3
"""Ratchet the disc-wide BYTE accounting, the third denominator.

[`disc-coverage.py`](disc-coverage.py) carries two figures and says in its own
text that they are different kinds of number: a byte-exact CODE figure, and a
DATA figure that is format **recognition** - "this entry is a
`scene_vab_stream`" - which is an upper bound and not an account of what any
parser consumed. The gap between those two sentences is where a whole class of
work hides: an entry can be recognised, ratcheted and reported at 100% while
most of its bytes have never been walked by anything.

`asset account` closes that gap one entry at a time and
`scripts/asset-investigation/byte-account-sweep.py` runs it over the whole TOC
([`docs/tooling/byte-accounting.md`](../../docs/tooling/byte-accounting.md)).
This script is the ratchet over that sweep's CSV, so the byte figure moves like
the code figure does: only upward, or with a commit that says why not.

Three figures, and they answer three different questions:

  structural_pct  the share of disc bytes a parser here STRUCTURALLY consumed -
                  walked to, from a header or a table, rather than found. This
                  is the headline and the one that ratchets upward.

  accounted_pct   structural plus magic-sweep hits. Always >= structural, and
                  deliberately not the headline: a magic hit is evidence a
                  sub-asset is there, not that anything walked to it. Carried
                  so a class whose accounted share rests on scan hits is
                  visible rather than flattering.

  work_bytes      the residue that is NOT the disc's own slack - every
                  unconsumed run whose shape is not `zero_pad`, `alignment` or
                  `repeated_fill`. This is the worklist in bytes, and it
                  ratchets DOWNWARD. Counting slack as work is how a sweep
                  produces a worklist nobody can act on: filler and sector
                  padding are most of the residue by bytes.

The sweep is minutes of work and needs the disc, the `asset` binary and the
dump corpus, so it is not run from here - like the categorize cache
`disc-coverage.py` reads, the CSV is an input this gate consumes. With no CSV
this SKIPS (exit 0), which is what keeps CI green without disc data.

That makes one failure mode worth naming, because it is the same one the
categorize cache has: a **stale** CSV reports the tree it was swept on, not
this one, through a passing gate. Re-run the sweep before taking a baseline,
and prefer `--max-age-days` in any automated use.

Usage:
    python3 scripts/ci/byte-account-coverage.py                  # report
    python3 scripts/ci/byte-account-coverage.py --check          # ratchet
    python3 scripts/ci/byte-account-coverage.py --update-baseline
    python3 scripts/ci/byte-account-coverage.py --csv PATH       # another sweep
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_CSV = os.path.join(REPO, "target", "byte-account", "sweep.csv")
BASELINE = os.path.join(REPO, "scripts", "ci", "byte-account-baseline.json")

# Shapes that are the disc's own slack rather than unwalked format. Kept
# identical to the sweep's own `SLACK_SHAPES`; a divergence here would produce
# a work figure that disagrees with the rollup it came from.
SLACK_SHAPES = {"zero_pad", "alignment", "repeated_fill"}
SHAPES = [
    "zero_pad", "alignment", "repeated_fill", "ascii_text", "pointer_dense",
    "bgr555", "plausible_mips", "low_entropy", "high_entropy", "mixed",
]

# Classes reported per row as well as in the total. A per-class ratchet is what
# makes a regression attributable: the whole-disc figure moves by a rounding
# error when one 4 MB class loses its walker, and "the total fell 0.3 pp" does
# not say which parser to look at.
PER_CLASS_MIN_BYTES = 1 << 20


def read_sweep(path):
    """`(rows, mtime)` from a sweep CSV, or `(None, None)` when absent."""
    if not os.path.exists(path):
        return None, None
    with open(path, newline="") as fh:
        rows = list(csv.DictReader(fh))
    return rows, os.path.getmtime(path)


def baseline_time():
    """When the baseline was taken: its last commit time, or the file's
    mtime when it has uncommitted edits (or git is unavailable)."""
    try:
        dirty = subprocess.run(
            ["git", "status", "--porcelain", "--", BASELINE],
            cwd=REPO, capture_output=True, text=True, check=True).stdout.strip()
        if not dirty:
            out = subprocess.run(
                ["git", "log", "-1", "--format=%ct", "--", BASELINE],
                cwd=REPO, capture_output=True, text=True, check=True).stdout.strip()
            if out:
                return float(out)
    except (OSError, subprocess.CalledProcessError):
        pass
    return os.path.getmtime(BASELINE)


def figures(rows):
    """Whole-disc and per-class figures from the sweep's rows.

    The per-entry percentages are re-weighted by SIZE rather than averaged: a
    mean over 1233 entries is a statement about entries, and one 15 MB archive
    then weighs the same as one 2 KB filler sector. Every number here is a
    share of bytes.
    """
    total = {"size": 0, "structural": 0, "accounted": 0, "work": 0, "residue": 0,
             "entries": 0, "errors": 0}
    per_class = {}
    for row in rows:
        if row.get("error"):
            total["errors"] += 1
            continue
        size = int(row["size"])
        # The sweep carries percentages per entry; bytes are what add up.
        structural = round(size * float(row["structural_pct"]) / 100.0)
        accounted = round(size * float(row["accounted_pct"]) / 100.0)
        work = sum(int(row[s]) for s in SHAPES if s not in SLACK_SHAPES)
        cls = row["class"]
        acc = per_class.setdefault(
            cls, {"size": 0, "structural": 0, "accounted": 0, "work": 0,
                  "residue": 0, "entries": 0})
        for bucket in (total, acc):
            bucket["size"] += size
            bucket["structural"] += structural
            bucket["accounted"] += accounted
            bucket["work"] += work
            bucket["residue"] += int(row["residue_bytes"])
            bucket["entries"] += 1
    return total, per_class


def pct(part, whole):
    return round(100.0 * part / whole, 2) if whole else 0.0


def snapshot(total, per_class):
    snap = {
        "disc": {
            "structural_pct": pct(total["structural"], total["size"]),
            "accounted_pct": pct(total["accounted"], total["size"]),
            "work_bytes": total["work"],
        },
        "class_structural_pct": {},
        "class_work_bytes": {},
    }
    for cls, acc in sorted(per_class.items()):
        if acc["size"] < PER_CLASS_MIN_BYTES:
            continue
        snap["class_structural_pct"][cls] = pct(acc["structural"], acc["size"])
        snap["class_work_bytes"][cls] = acc["work"]
    return snap


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--csv", default=DEFAULT_CSV,
                    help="sweep CSV (default: %(default)s)")
    ap.add_argument("--check", action="store_true",
                    help="fail if a figure regressed beyond the tolerance")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--tolerance", type=float, default=0.05,
                    help="percentage points a figure may fall (default 0.05)")
    ap.add_argument("--work-tolerance", type=float, default=0.005,
                    help="fraction the work-byte figure may grow (default 0.5%%)")
    ap.add_argument("--max-age-days", type=float, default=0.0,
                    help="fail when the CSV is older than this (0 = no limit)")
    ap.add_argument("--quiet", action="store_true",
                    help="drop the per-class rollup, keep every verdict line")
    args = ap.parse_args()

    rows, mtime = read_sweep(args.csv)
    if rows is None:
        print("[byte-account] SKIPPED - no sweep at %s. Run "
              "scripts/asset-investigation/byte-account-sweep.py (needs the "
              "disc, the `asset` binary and the dump corpus)."
              % os.path.relpath(args.csv, REPO))
        return 0
    age_days = (time.time() - mtime) / 86400.0
    if args.max_age_days and age_days > args.max_age_days:
        print("[byte-account] STALE - %s is %.1f day(s) old; it reports the "
              "tree it was swept on, not this one. Re-run the sweep."
              % (os.path.relpath(args.csv, REPO), age_days))
        return 1

    total, per_class = figures(rows)
    current = snapshot(total, per_class)
    if total["errors"]:
        print("[byte-account] %d entry(ies) did not account and are outside "
              "every figure below" % total["errors"])

    if not args.quiet:
        print("[byte-account] %d entries, %d B of PROT payload"
              % (total["entries"], total["size"]))
        print("[byte-account] structurally consumed: %.2f%% (%d B) - walked to, "
              "not found" % (current["disc"]["structural_pct"], total["structural"]))
        print("[byte-account] accounted incl. magic-sweep hits: %.2f%%"
              % current["disc"]["accounted_pct"])
        print("[byte-account] non-slack residue (the worklist): %d B of %d B "
              "unconsumed" % (total["work"], total["residue"]))
        for cls in sorted(current["class_structural_pct"]):
            print("[byte-account] class %-22s structural %6.2f%%  work %10d B"
                  % (cls, current["class_structural_pct"][cls],
                     current["class_work_bytes"][cls]))

    if args.update_baseline:
        with open(BASELINE, "w") as fh:
            json.dump(current, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("[byte-account] baseline updated: %s" % os.path.relpath(BASELINE, REPO))
        return 0

    if not args.check:
        return 0

    if not os.path.exists(BASELINE):
        # Not a pass: nothing was compared, and a bare OK here would be the
        # same green-line-for-an-absent-comparison this file's sibling refuses
        # to print.
        print("[byte-account] NOT RATCHETED - no baseline at %s. Nothing was "
              "compared. Run --update-baseline once."
              % os.path.relpath(BASELINE, REPO))
        return 0

    # A sweep older than the baseline measured an older tree than the one the
    # baseline was taken from, so every difference is between two trees, not
    # a parser change here - it read as "a parser stopped consuming bytes"
    # after a merge moved the baseline past a local sweep. Name it.
    base_t = baseline_time()
    if mtime < base_t:
        print("[byte-account] STALE - %s (swept %s) predates the baseline "
              "(taken %s), so it measures an older tree than the baseline "
              "did and nothing can be compared. Re-run "
              "scripts/asset-investigation/byte-account-sweep.py after "
              "rebuilding `asset` from this tree (cargo build --release -p "
              "legaia-asset --bin asset)."
              % (os.path.relpath(args.csv, REPO),
                 time.strftime("%Y-%m-%d %H:%M", time.localtime(mtime)),
                 time.strftime("%Y-%m-%d %H:%M", time.localtime(base_t))))
        return 1

    base = json.load(open(BASELINE))
    bad, absent = [], []
    for key, was in base.get("disc", {}).items():
        now = current["disc"].get(key)
        if now is None:
            absent.append("disc/%s" % key)
            continue
        if key.endswith("_pct") and now < was - args.tolerance:
            bad.append("disc/%s: %.2f%% -> %.2f%%" % (key, was, now))
        if key == "work_bytes" and now > was * (1.0 + args.work_tolerance):
            bad.append("disc/work_bytes: %d -> %d B unconsumed non-slack" % (was, now))
    for cls, was in base.get("class_structural_pct", {}).items():
        now = current["class_structural_pct"].get(cls)
        if now is None:
            # A class that stops appearing is not a regression - a re-classed
            # entry moves its bytes to another row - but it is also not a
            # comparison, and a silent drop reads exactly like a clean run.
            absent.append("class_structural_pct/%s (baselined at %.2f%%)" % (cls, was))
            continue
        if now < was - args.tolerance:
            bad.append("class %s structural: %.2f%% -> %.2f%%" % (cls, was, now))
    for cls, was in base.get("class_work_bytes", {}).items():
        now = current["class_work_bytes"].get(cls)
        if now is None:
            absent.append("class_work_bytes/%s (baselined at %d B)" % (cls, was))
            continue
        if now > was * (1.0 + args.work_tolerance) and now - was > 4096:
            bad.append("class %s work: %d -> %d B unconsumed non-slack"
                       % (cls, was, now))
    for a in absent:
        print("[byte-account] NOT MEASURED THIS RUN: %s - the class is absent "
              "from this sweep, so the ratchet skipped it" % a)
    if bad:
        print("[byte-account] REGRESSION:")
        for b in bad:
            print("   " + b)
        print("[byte-account] a parser stopped consuming bytes it used to. If "
              "a walker was deliberately narrowed (a claim it could not "
              "defend), re-run with --update-baseline and say why in the "
              "commit message.")
        return 1
    compared = (len(base.get("disc", {}))
                + len(base.get("class_structural_pct", {}))
                + len(base.get("class_work_bytes", {})) - len(absent))
    print("[byte-account] OK - %d baselined figure(s) compared, none regressed "
          "beyond %.2f pp / %.1f%%" % (compared, args.tolerance,
                                       100.0 * args.work_tolerance))
    return 0


if __name__ == "__main__":
    sys.exit(main())
