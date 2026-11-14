#!/usr/bin/env python3
"""Read a summon film's per-frame cost off its retail recording, arm by arm.

    scripts/ci/cast-arm-cost-rows.py captures/<films>            # every sNN/retail under it
    scripts/ci/cast-arm-cost-rows.py captures/<films> --rust     # rows for CAST_ARM_COST_TAILS

The recorder's `meta.csv` (`scripts/ci/cast-sequence.py`) logs, per vsync, the
battle frame counter, the frame step and the frame-cost ring's newest entry.
On the first row of a frame those are the pass that just ran: its module
phase, the entry its own frame end stored, and the step that frame end read
for the next pass. A step of `4` is a raw cost of `0x2D1` or more (the ring
stores such a frame as `0x2BC`, which reads `3`), so the frame's cost rung is
`4` on that step and the entry's own rung otherwise.

Per arm the rungs are run-length coded against the arm's one entry in
`ARM_STEPS` (`crates/engine-cast/src/cast_module_frame_step.rs`): a row
`(entry, arm, pass, rung)` wherever the rung changes, and at pass `0` when
the arm opens off its table entry. An arm the table leaves unread (`0`) takes
no row. See `docs/subsystems/battle-stage-camera.md`.
"""

import argparse
import csv
import itertools
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
TABLE = REPO / "crates" / "engine-cast" / "src" / "cast_module_frame_step.rs"
# Spell id of a film directory `sNN` to its module's PROT entry.
ENTRY = {0x81 + i: 903 + i for i in range(11)} | {
    0x8C: 914, 0x8D: 915, 0x8E: 916, 0x8F: 917, 0x90: 918, 0x91: 919, 0x92: 920, 0x93: 921,
    0x94: 922, 0x95: 923, 0x99: 927, 0x9A: 928, 0x9B: 929, 0x9C: 930, 0x9D: 931, 0x9E: 932,
    0x9F: 933, 0xA0: 934,
}
BAND = ("53", "54")  # SummonSustain 0x35, SummonReturn 0x36


def arm_steps() -> dict[int, list[int]]:
    body = re.search(r"const ARM_STEPS: &\[\(u32, &\[u8\]\)\] = &\[(.*?)\n\];", TABLE.read_text(), re.S).group(1)
    return {int(e): [int(x) for x in v.split(",") if x.strip()]
            for e, v in re.findall(r"\(\s*(\d+),\s*&\[(.*?)\],?\s*\)", body, re.S)}


def rung(cost: int, step: int) -> int:
    if step == 4:
        return 4
    return 1 if cost < 0xF1 else 2 if cost < 0x1FF else 3


def arm_runs(meta: Path) -> list[tuple[int, list[tuple[int, int]]]]:
    """`(arm, [(rung, frames), ..])` for each arm of the band, in order."""
    rows = list(csv.DictReader(open(meta)))
    frames = [r for p, r in zip(rows, rows[1:]) if r["frame"] != p["frame"] and r["st7"] in BAND]
    out = []
    for arm, g in itertools.groupby(frames, key=lambda r: int(r["phase"])):
        rungs = [rung(int(r["cost"]), int(r["step"])) for r in g]
        out.append((arm, [(k, len(list(v))) for k, v in itertools.groupby(rungs)]))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("films", type=Path, help="directory of sNN/retail recordings")
    ap.add_argument("--rust", action="store_true", help="print the rows as Rust tuples")
    a = ap.parse_args()
    table = arm_steps()
    total = 0
    for d in sorted(a.films.glob("s??/retail/meta.csv")):
        name = d.parts[-3]
        entry = ENTRY.get(int(name[1:], 16))
        if entry is None:
            continue
        steps = table.get(entry, [])
        lines = []
        for arm, runs in arm_runs(d):
            base = steps[arm] if arm < len(steps) else 0
            if base == 0 or arm == 0xFF:
                continue
            at, cur = 0, base
            for k, n in runs:
                if k != cur:
                    lines.append((arm, at, k))
                    cur = k
                at += n
            if not a.rust:
                print(f"{name} {entry} arm {arm:2} table {base}: " + " ".join(f"{k}x{n}" for k, n in runs))
        total += len(lines)
        if a.rust and lines:
            print(f"    // {name}")
            for arm, at, k in lines:
                print(f"    ({entry}, {arm}, {at}, {k}),")
    print(f"// {total} rows", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
