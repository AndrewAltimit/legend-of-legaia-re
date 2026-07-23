#!/usr/bin/env python3
"""Summarise / diff `autorun_gaza2_magic_wedge.lua` timelines.

The Gaza 2 softlock hunt works by comparison: you know what a HEALTHY player
Seru cast looks like, so a wedged one is found by diffing against it. This
reduces a timeline.csv to a per-`ctx+7`-state profile - visit count, total and
max dwell, and the range each exit counter takes while the state is current -
and diffs two profiles side by side.

Dwell is measured in vsyncs from the change-triggered rows: the probe only
emits a row when its key changes, so a state's dwell is the span from its first
row to the first row of the next state.

Two things this is built to make obvious:

  * which state the wedged run parks in that the healthy one walks through, and
  * which of the two exit counters differs there - `ctx+0x249` (the actor
    animation census, gated on `actor[+0x4]`, slots 0..6) versus `ctx+0x24D`
    (non-zero bytes among the four spell-child slots `ctx[0x252..0x255]`).

Usage:
    analyze_gaza2_timeline.py <timeline.csv>                  # profile one run
    analyze_gaza2_timeline.py <healthy.csv> --diff <other.csv>
"""
from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

# ctx+7 bands, per docs/subsystems/battle-action.md.
BANDS = [
    (0x00, 0x0C, "init/re-entry"),
    (0x14, 0x20, "attack"),
    (0x28, 0x2E, "magic/item"),
    (0x32, 0x38, "summon"),
    (0x3C, 0x40, "spirit"),
    (0x46, 0x48, "spirit-arts"),
    (0x50, 0x52, "done/cleanup"),
    (0x5A, 0x5A, "end-of-action"),
    (0x64, 0x6B, "run/capture-fail"),
    (0x6E, 0x71, "capture"),
]

COUNTERS = ["c249", "c24a", "c24b", "c24c", "c24d", "c24e"]


def band_of(state: int) -> str:
    for lo, hi, name in BANDS:
        if lo <= state <= hi:
            return name
    return "-"


def load(path: Path):
    with path.open(newline="") as fh:
        rows = list(csv.DictReader(fh))
    if not rows:
        raise SystemExit(f"{path}: empty timeline")
    return rows


def profile(rows):
    """state -> dict(visits, total_dwell, max_dwell, counter ranges)."""
    prof: dict[int, dict] = {}
    prev_state = None
    prev_vsync = None
    visit_start = None
    for r in rows:
        try:
            state = int(r["ctx7"], 16)
            vsync = int(r["vsync"])
        except (KeyError, ValueError):
            continue
        if prev_state is not None:
            d = prof.setdefault(prev_state, _blank())
            d["total"] += vsync - prev_vsync
        if state != prev_state:
            # A visit ENDED here: score the whole contiguous run of the
            # previous state, not the gap between two of its rows. The probe
            # emits extra rows mid-state whenever any watched field moves, so
            # a per-row gap badly understates a long park (measured: the
            # healthy summon 0x36 reads 670 per-row against 1224 per-visit).
            if prev_state is not None and visit_start is not None:
                span = vsync - visit_start
                d = prof.setdefault(prev_state, _blank())
                if span > d["max"]:
                    d["max"] = span
            visit_start = vsync
            prof.setdefault(state, _blank())["visits"] += 1
        for c in COUNTERS:
            v = r.get(c)
            if v is None or v == "":
                continue
            try:
                iv = int(v)
            except ValueError:
                continue
            d = prof.setdefault(state, _blank())
            lo, hi = d["ctr"][c]
            d["ctr"][c] = (min(lo, iv), max(hi, iv))
        prev_state, prev_vsync = state, vsync
    # The final state never sees a transition, so close its visit by hand.
    if prev_state is not None and visit_start is not None:
        d = prof.setdefault(prev_state, _blank())
        span = prev_vsync - visit_start
        if span > d["max"]:
            d["max"] = span
    return prof


def _blank():
    return {
        "visits": 0,
        "total": 0,
        "max": 0,
        "ctr": {c: (10**9, -1) for c in COUNTERS},
    }


def fmt_ctr(d):
    out = []
    for c in COUNTERS:
        lo, hi = d["ctr"][c]
        if hi < 0:
            continue
        out.append(f"{c}={lo}" if lo == hi else f"{c}={lo}..{hi}")
    return " ".join(out)


def print_profile(name, prof):
    print(f"== {name}")
    print(f"{'state':>6}  {'band':<16} {'visits':>6} {'maxdwell':>8} "
          f"{'total':>7}  counters")
    for state in sorted(prof):
        d = prof[state]
        print(f"  0x{state:02X}  {band_of(state):<16} {d['visits']:>6} "
              f"{d['max']:>8} {d['total']:>7}  {fmt_ctr(d)}")


def print_diff(a_name, a, b_name, b):
    print(f"== diff: A={a_name}  B={b_name}")
    print(f"{'state':>6}  {'band':<16} {'A max':>7} {'B max':>7} {'delta':>8}"
          "  note")
    for state in sorted(set(a) | set(b)):
        da, db = a.get(state), b.get(state)
        am = da["max"] if da else 0
        bm = db["max"] if db else 0
        note = ""
        if da is None:
            note = "B-only state"
        elif db is None:
            note = "A-only state"
        elif bm > am * 2 and bm - am > 60:
            note = "B parks MUCH longer here <<<"
        if da and db:
            for c in ("c249", "c24d"):
                la, ha = da["ctr"][c]
                lb, hb = db["ctr"][c]
                if ha >= 0 and hb >= 0 and (la, ha) != (lb, hb):
                    note += f" {c}: A={la}..{ha} B={lb}..{hb}"
        print(f"  0x{state:02X}  {band_of(state):<16} {am:>7} {bm:>7} "
              f"{bm - am:>8}  {note}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("timeline")
    ap.add_argument("--diff", default=None,
                    help="second timeline.csv to compare against the first")
    args = ap.parse_args()

    a_path = Path(args.timeline)
    a = profile(load(a_path))
    if args.diff is None:
        print_profile(a_path.name, a)
        return 0
    b_path = Path(args.diff)
    b = profile(load(b_path))
    print_profile(f"A {a_path}", a)
    print()
    print_profile(f"B {b_path}", b)
    print()
    print_diff(a_path.name, a, b_path.name, b)
    return 0


if __name__ == "__main__":
    sys.exit(main())
