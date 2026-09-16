#!/usr/bin/env python3
"""Reduce `autorun_capture_arm_gating.lua` runs to a per-arm dwell table.

Each run directory holds a `ticks.csv` with one row per capture-class module
tick (an `FUN_801F2160` entry), carrying the module phase byte `ctx[+0x279]`,
the module's own countdown word and the scratchpad frame-step product. This
script groups the rows into arm runs and reports, per arm:

* the dwell in **ticks** - the unit the choreography counts in; the battle SM
  does not advance once per VSync, so a VSync dwell carries host timing;
* the dwell in VSyncs, for reference;
* the countdown value at the arm's first and last tick, and whether the arm is
  countdown-gated - i.e. whether the drain `cd_first - cd_last` equals the sum
  of the per-tick frame steps the arm ran under, and the arm ended with less
  than one more step left. The frame step `*(0x1F80037D) * *(0x1F800393)` is
  **adaptive and changes within a single cast**, so a dwell computed from the
  arm's first step alone reports a countdown-gated arm as ungated.

Usage:
    python3 scripts/pcsx-redux/analyze_capture_arm_gating.py <run-dir> [...]
    python3 scripts/pcsx-redux/analyze_capture_arm_gating.py --csv out.csv <run-dir> [...]

A run directory that has no `ticks.csv`, or whose `ticks.csv` has only a
header, is reported as an empty run rather than skipped silently - a zero-tick
run reads exactly like "the body never ran", which is a conclusion the reader
has to be able to see.
"""

from __future__ import annotations

import argparse
import csv
import pathlib
import sys

# Action id -> (PROT entry, doc's body VA) for the fourteen trampoline-reached
# arms plus the neighbours a run may land on. Only used to label the report;
# the measured body VA is what the rows carry.
ARM_OWNERS = {
    0x3C: (940, None),
    0x50: (940, 0x801F78B8),
    0xAC: (940, 0x801F7240),
    0xAE: (940, 0x801F78B8),
    0x51: (941, 0x801F730C),
    0xB9: (941, 0x801F6A04),
    0x40: (943, 0x801F6EF4),
    0xB5: (943, 0x801F6A04),
    0x37: (944, 0x801F6A04),
    0x53: (944, 0x801F7470),
    0x5A: (950, 0x801F79F8),
    0xAB: (950, 0x801F6A24),
    0x71: (956, 0x801F7298),
    0xA2: (962, 0x801F7AE4),
    0xA3: (962, 0x801F74A0),
    0xA4: (962, 0x801F6D54),
}


def read_manifest(run: pathlib.Path) -> dict[str, str]:
    out: dict[str, str] = {}
    path = run / "manifest.txt"
    if not path.exists():
        return out
    for line in path.read_text(errors="replace").splitlines():
        if "=" in line:
            k, _, v = line.partition("=")
            out[k.strip()] = v.strip()
    return out


def arm_runs(rows: list[dict[str, str]]) -> list[dict[str, object]]:
    """Group consecutive equal phase bytes into arm runs."""
    runs: list[dict[str, object]] = []
    for row in rows:
        phase = int(row["phase"])
        cd = int(row["cd"])
        step = int(row["step"])
        if runs and runs[-1]["phase"] == phase:
            cur = runs[-1]
            cur["ticks"] = int(cur["ticks"]) + 1
            cur["last_vsync"] = int(row["vsync"])
            cur["cd_last"] = cd
            cur["steps"].append(step)
        else:
            runs.append(
                {
                    "phase": phase,
                    "ticks": 1,
                    "first_vsync": int(row["vsync"]),
                    "last_vsync": int(row["vsync"]),
                    "cd_first": cd,
                    "cd_last": cd,
                    "steps": [step],
                    "body": row["body"],
                    "ra": row["ra"],
                }
            )
    for a in runs:
        steps = a["steps"]
        # The decrement runs once per tick except on the tick that observes the
        # final value, so the expected drain is the sum over all but the last.
        a["step_sum"] = sum(steps[:-1]) if len(steps) > 1 else 0
        a["step_min"] = min(steps)
        a["step_max"] = max(steps)
        drain = int(a["cd_first"]) - int(a["cd_last"])
        a["drain"] = drain
        a["gated"] = (
            int(a["cd_first"]) > 0
            and drain > 0
            and drain == a["step_sum"]
            and int(a["cd_last"]) <= a["step_max"]
        )
    return runs


def analyse(run: pathlib.Path) -> dict[str, object]:
    man = read_manifest(run)
    spell = man.get("spell", "?")
    ticks_path = run / "ticks.csv"
    rows: list[dict[str, str]] = []
    if ticks_path.exists():
        with ticks_path.open(newline="") as fh:
            rows = list(csv.DictReader(fh))
    try:
        spell_id = int(spell, 16) if spell.startswith("0x") else int(spell)
    except ValueError:
        spell_id = -1
    prot, doc_body = ARM_OWNERS.get(spell_id, (None, None))
    return {
        "run": run.name,
        "spell": spell,
        "spell_id": spell_id,
        "prot": prot,
        "doc_body": doc_body,
        "rows": rows,
        "arms": arm_runs(rows),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("runs", nargs="+", help="capture run directories")
    ap.add_argument("--csv", help="write the per-arm table here")
    args = ap.parse_args()

    out_rows: list[list[object]] = []
    for name in args.runs:
        run = pathlib.Path(name)
        info = analyse(run)
        arms = info["arms"]
        bodies = sorted({a["body"] for a in arms})
        doc_body = info["doc_body"]
        doc_txt = f"0x{doc_body:08X}" if doc_body else "-"
        print(f"\n== {info['run']}  action {info['spell']}  PROT {info['prot']} ==")
        print(f"   ticks={len(info['rows'])}  bodies_entered={bodies}  doc_body={doc_txt}")
        if not arms:
            print("   NO TICKS - the body never ran in this capture")
            continue
        print("   arm  ticks  vsyncs  cd_first  cd_last  drain  step_sum  step_range  gated")
        for a in arms:
            vs = int(a["last_vsync"]) - int(a["first_vsync"]) + 1
            print(
                "   %4s  %5d  %6d  %8d  %7d  %5d  %8d  %4d-%-4d  %s"
                % (
                    a["phase"],
                    a["ticks"],
                    vs,
                    int(a["cd_first"]),
                    int(a["cd_last"]),
                    int(a["drain"]),
                    int(a["step_sum"]),
                    int(a["step_min"]),
                    int(a["step_max"]),
                    "yes" if a["gated"] else "no",
                )
            )
            out_rows.append(
                [
                    info["prot"],
                    info["spell"],
                    a["body"],
                    a["phase"],
                    a["ticks"],
                    vs,
                    int(a["cd_first"]),
                    int(a["cd_last"]),
                    int(a["drain"]),
                    int(a["step_sum"]),
                    int(a["step_min"]),
                    int(a["step_max"]),
                    1 if a["gated"] else 0,
                ]
            )

    if args.csv:
        with open(args.csv, "w", newline="") as fh:
            w = csv.writer(fh)
            w.writerow(
                [
                    "prot",
                    "action_id",
                    "body_va",
                    "phase",
                    "ticks",
                    "vsyncs",
                    "cd_first",
                    "cd_last",
                    "drain",
                    "step_sum",
                    "step_min",
                    "step_max",
                    "countdown_gated",
                ]
            )
            w.writerows(out_rows)
        print(f"\nwrote {len(out_rows)} arm rows to {args.csv}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
