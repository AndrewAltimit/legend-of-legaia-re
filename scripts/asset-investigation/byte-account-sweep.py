#!/usr/bin/env python3
"""Byte-account every PROT entry, and roll the residue up by entry class.

`asset account` answers the question one entry at a time
(`docs/tooling/byte-accounting.md`). One entry at a time is the right shape for
a hunt and the wrong shape for a worklist: the entries whose residue is worth
walking are not the ones anybody thought to run it on, and 1233 hand-run reports
are not a thing anyone reads. This driver runs the whole TOC and rolls the
result up two ways - by entry class and by residue shape - so the top of the
worklist is a property of the disc rather than of what somebody sampled.

## Output

A CSV of NUMBERS ONLY, plus this workspace's own class / walker / shape
vocabulary. No disc bytes, no strings lifted out of an entry, no CDNAME label -
the per-entry `head` hex that `asset account --json` prints for each residue run
is deliberately dropped here, because that field is disc bytes. The CSV lands
under `target/` (gitignored) by default; nothing about it is meant to be
committed, and the doc section it feeds names stable shapes rather than the
counts, which move with every parser that lands.

    scripts/asset-investigation/byte-account-sweep.py            # sweep + rollup
    scripts/asset-investigation/byte-account-sweep.py --top 25
    scripts/asset-investigation/byte-account-sweep.py --out /tmp/sweep.csv

## Reading the rollup

The two rollups answer different questions and the second is the one that ranks
work:

* **by class** - which kinds of entry the workspace does not fully consume.
  Weighted by bytes, so one 15 MB archive outweighs a hundred sectors of filler.
* **by class x shape** - what the unconsumed bytes *are*. A class whose residue
  is `zero_pad` is finished; the disc's own slack is not work. A class whose
  residue is `high_entropy` or `plausible_mips` is a format or a routine nobody
  has walked, and that is the worklist.

`scan_bytes` (`accounted - structural`) is carried per row for the reason the
tiering exists: bytes found by a magic sweep are evidence a sub-asset is there,
not that anything walked to it, and a class whose accounted share rests on scan
hits has an unwalked layout however high its headline number reads.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import csv
import json
import os
import subprocess
import sys
from collections import defaultdict

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# The residue classifier's whole vocabulary, in the order
# `docs/tooling/byte-accounting.md` documents the tests in. Fixed here rather
# than collected from the data so a shape that stops appearing shows as a zero
# column instead of silently leaving the table.
SHAPES = [
    "zero_pad", "alignment", "repeated_fill", "ascii_text", "pointer_dense",
    "bgr555", "plausible_mips", "low_entropy", "high_entropy", "mixed",
]

# Shapes that are the disc's own slack rather than unwalked format. Counting
# them as work is how a sweep like this produces a worklist nobody can act on:
# `pochi` filler and sector padding are most of the residue by bytes.
SLACK_SHAPES = {"zero_pad", "alignment", "repeated_fill"}


def account_one(asset_bin, path, prot_dir, funcs, depth):
    cmd = [asset_bin, "account", path, "--prot-dir", prot_dir,
           "--depth", str(depth), "--json"]
    if funcs:
        cmd += ["--funcs", funcs]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"entry": entry_of(path), "error": type(exc).__name__}
    if proc.returncode != 0:
        return {"entry": entry_of(path), "error": "exit%d" % proc.returncode}
    try:
        doc = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"entry": entry_of(path), "error": "json"}
    shapes = {s["shape"]: s["bytes"] for s in doc.get("by_shape", [])}
    residue = doc.get("residue") or []
    row = {
        "entry": entry_of(path),
        "size": doc.get("size", 0),
        "class": doc.get("class", "?"),
        "walker": doc.get("walker", "?"),
        "accounted_pct": round(doc.get("accounted_pct", 0.0), 3),
        "structural_pct": round(doc.get("structural_pct", 0.0), 3),
        "scan_bytes": doc.get("accounted", 0) - doc.get("structural", 0),
        "residue_bytes": doc.get("residue_bytes", 0),
        # The longest single uncovered run. A class whose residue is one long
        # run is an unwalked region; the same byte total spread over hundreds of
        # short runs is inter-record slack.
        "largest_residue": max((r.get("len", 0) for r in residue), default=0),
        "residue_runs": len(residue),
        "error": "",
    }
    for shape in SHAPES:
        row[shape] = shapes.get(shape, 0)
    return row


def entry_of(path):
    """The extraction index the filename carries, or -1."""
    stem = os.path.basename(path).split("_")[0]
    try:
        return int(stem, 10)
    except ValueError:
        return -1


def rollup(rows, top):
    per_class = defaultdict(lambda: defaultdict(int))
    for r in rows:
        if r.get("error"):
            continue
        c = per_class[r["class"]]
        c["entries"] += 1
        c["size"] += r["size"]
        c["residue"] += r["residue_bytes"]
        c["scan"] += r["scan_bytes"]
        c["largest"] = max(c["largest"], r["largest_residue"])
        for shape in SHAPES:
            c[shape] += r[shape]
    for c in per_class.values():
        c["work"] = sum(c[s] for s in SHAPES if s not in SLACK_SHAPES)

    print("\n== residue by entry class, largest first (bytes) ==")
    print("%-26s %7s %12s %12s %12s  %s"
          % ("class", "entries", "size", "residue", "non-slack", "dominant shapes"))
    order = sorted(per_class.items(), key=lambda kv: -kv[1]["work"])
    for name, c in order[:top]:
        shapes = sorted(((c[s], s) for s in SHAPES if c[s]), reverse=True)[:3]
        print("%-26s %7d %12d %12d %12d  %s"
              % (name, c["entries"], c["size"], c["residue"], c["work"],
                 ", ".join("%s %d" % (s, n) for n, s in shapes)))

    print("\n== non-slack residue by shape, whole disc (bytes) ==")
    totals = {s: sum(c[s] for c in per_class.values()) for s in SHAPES}
    for shape, n in sorted(totals.items(), key=lambda kv: -kv[1]):
        if n:
            print("%-16s %12d%s" % (shape, n,
                                    "   (slack)" if shape in SLACK_SHAPES else ""))

    print("\n== entries with the largest single unclaimed run ==")
    ranked = sorted((r for r in rows if not r.get("error")),
                    key=lambda r: -r["largest_residue"])
    print("%7s %-26s %-20s %12s %10s"
          % ("entry", "class", "walker", "largest run", "accounted"))
    for r in ranked[:top]:
        print("%7d %-26s %-20s %12d %9.1f%%"
              % (r["entry"], r["class"], r["walker"], r["largest_residue"],
                 r["accounted_pct"]))

    errs = [r for r in rows if r.get("error")]
    if errs:
        print("\n%d entry(ies) did not account: %s"
              % (len(errs), ", ".join("%d(%s)" % (r["entry"], r["error"])
                                      for r in errs[:20])))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--prot-dir", default=os.path.join(REPO, "extracted", "PROT"))
    ap.add_argument("--funcs", default=os.path.join(REPO, "ghidra", "scripts", "funcs"))
    ap.add_argument("--asset", default=os.path.join(REPO, "target", "release", "asset"),
                    help="the `asset` binary (cargo build --release -p legaia-asset)")
    ap.add_argument("--out", default=os.path.join(REPO, "target", "byte-account",
                                                  "sweep.csv"))
    ap.add_argument("--depth", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    ap.add_argument("--top", type=int, default=20)
    args = ap.parse_args()

    if not os.path.isdir(args.prot_dir):
        print("[byte-account-sweep] SKIPPED - no %s. Run legaia-extract first."
              % args.prot_dir)
        return 0
    if not os.path.exists(args.asset):
        print("[byte-account-sweep] no `asset` binary at %s - "
              "cargo build --release -p legaia-asset" % args.asset)
        return 2

    paths = sorted(os.path.join(args.prot_dir, f)
                   for f in os.listdir(args.prot_dir) if f.endswith(".BIN"))
    if not paths:
        print("[byte-account-sweep] SKIPPED - %s holds no .BIN entries."
              % args.prot_dir)
        return 0
    funcs = args.funcs if os.path.isdir(args.funcs) else ""
    if not funcs:
        print("[byte-account-sweep] no dump corpus at %s - overlay code images "
              "will account to 0%% and read as pure residue." % args.funcs)

    rows = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(account_one, args.asset, p, args.prot_dir,
                               funcs, args.depth) for p in paths]
        for i, fut in enumerate(concurrent.futures.as_completed(futures), 1):
            rows.append(fut.result())
            if i % 100 == 0:
                print("  ... %d/%d" % (i, len(paths)), file=sys.stderr)
    rows.sort(key=lambda r: r["entry"])

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    cols = ["entry", "size", "class", "walker", "accounted_pct", "structural_pct",
            "scan_bytes", "residue_bytes", "largest_residue", "residue_runs",
            "error"] + SHAPES
    with open(args.out, "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        for r in rows:
            w.writerow({c: r.get(c, 0) for c in cols})
    print("[byte-account-sweep] wrote %s (%d entries)" % (args.out, len(rows)))
    rollup(rows, args.top)
    return 0


if __name__ == "__main__":
    sys.exit(main())
