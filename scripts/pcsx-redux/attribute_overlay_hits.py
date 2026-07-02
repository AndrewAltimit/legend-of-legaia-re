#!/usr/bin/env python3
"""Attribute VA-aliased overlay trace hits to the RESIDENT overlay's functions.

The trace-driven-coverage harness arms breakpoints by virtual address. SCUS
addresses (0x800xxxxx) are always resident, so a SCUS hit is unambiguous. But
overlay addresses (0x801Cxxxx+) are VA-ALIASED: different overlays occupy the
same address window, and the raw hit's dump `stem` in the union CSV is just
whichever overlay the static extractor happened to dump that VA from - which is
usually NOT the overlay resident during the traced segment.

The fix is CONTAINMENT, not stem: attribute each overlay hit to the function of
the *resident* overlay whose [entry, entry+size) range CONTAINS the hit address.
The resident overlay's dumps are the ground truth (e.g. at game_mode 0x15 the
resident overlay is the battle overlay 0898, dumped as
`ghidra/scripts/funcs/overlay_battle_action_*.txt`). See
docs/tooling/playthrough-coverage.md (S5 battle section) for the worked example.

Usage:
  attribute_overlay_hits.py <union.csv> [--dumps GLOB] [--min-hits N]

  <union.csv>   a trace union CSV (addr,hits,first_frame,first_mode,first_ra,stem)
  --dumps GLOB  glob of the RESIDENT overlay's Ghidra dumps to resolve against
                (default: the battle overlay 0898 = for game_mode 0x15 traces)
  --min-hits N  only print per-hit rows with >= N hits (default 15); the distinct
                -function aggregation always covers every resolved hit.

Prints: per-hit containment (addr -> enclosing fn + offset), the distinct
enclosing functions with total hits and doc-citation status (a `** NEW **`
function is an undocumented resident function that ran = a documentation target),
and the unresolved hits (above the dumped function set - the render-tail / a
re-dump target).
"""
import argparse
import csv
import glob
import os
import re
import subprocess
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEFAULT_DUMPS = os.path.join(
    REPO_ROOT, "ghidra", "scripts", "funcs", "overlay_battle_action_*.txt"
)


def load_ranges(dump_glob):
    """Parse [entry, entry+size) ranges from a set of Ghidra dumps.

    Handles both header forms (`entry=0x801e295c` and `entry=801e295c`) and skips
    "citation pointer" mid-function stubs (they carry no size).
    """
    ranges = []
    for f in glob.glob(dump_glob):
        with open(f, errors="ignore") as fh:
            head = "".join(fh.readline() for _ in range(3))
        m = re.search(r"entry=0?x?([0-9a-fA-F]+)\)", head)
        s = re.search(r"size=(\d+)\s*bytes", head)
        if not (m and s):
            continue
        entry = int(m.group(1), 16)
        ranges.append((entry, entry + int(s.group(1)), os.path.basename(f)))
    ranges.sort()
    return ranges


def containing(ranges, addr):
    for lo, hi, name in ranges:
        if lo <= addr < hi:
            return lo, hi, name
    return None


def documented(addr):
    """First docs/ file citing the 8-hex address, or '' if none."""
    hexa = "%08x" % addr
    r = subprocess.run(
        ["grep", "-rilE", hexa, "docs/"],
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
    )
    out = r.stdout.strip()
    return out.split("\n")[0] if out else ""


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("union_csv")
    ap.add_argument("--dumps", default=DEFAULT_DUMPS,
                    help="glob of the resident overlay's dumps (default: 0898)")
    ap.add_argument("--min-hits", type=int, default=15)
    args = ap.parse_args()

    ranges = load_ranges(args.dumps)
    if not ranges:
        sys.exit(f"no dump ranges parsed from {args.dumps}")

    hits = []
    with open(args.union_csv) as fh:
        for row in csv.DictReader(fh):
            a = int(row["addr"], 16)
            if 0x801C0000 <= a < 0x80200000:
                hits.append((int(row["hits"]), a, row["first_ra"], row["stem"]))
    hits.sort(reverse=True)

    print(f"resident-overlay dumped functions: {len(ranges)}")
    print(f"overlay hits: {len(hits)}\n")

    agg = {}
    unresolved = []
    print(f"{'hits':>5} {'addr':>10} {'ra':>10}  -> resident function (offset)")
    for h, a, ra, stem in hits:
        c = containing(ranges, a)
        if c:
            agg[c[0]] = agg.get(c[0], 0) + h
            if h >= args.min_hits:
                fn = c[2].replace(".txt", "")
                tag = " *ENTRY*" if a == c[0] else f" +0x{a - c[0]:x}"
                print(f"{h:>5} 0x{a:08x} {ra:>10}  {fn}{tag}")
        else:
            unresolved.append((h, a, ra, stem))

    print(f"\ndistinct resident functions hit: {len(agg)}"
          f"  (resolved {len(hits) - len(unresolved)}/{len(hits)})")
    print(f"{'fn':>12} {'totalhits':>9}  doc?")
    for e in sorted(agg, key=lambda k: -agg[k]):
        d = documented(e)
        tag = os.path.basename(d) if d else "** NEW **"
        print(f"  FUN_{e:08x} {agg[e]:>9}  {tag}")

    if unresolved:
        print(f"\nUNRESOLVED (above the dumped fn set - re-dump target): "
              f"{len(unresolved)}")
        for h, a, ra, stem in sorted(unresolved, reverse=True):
            print(f"  {h:>5} 0x{a:08x} ra={ra}  [{stem}]")


if __name__ == "__main__":
    main()
