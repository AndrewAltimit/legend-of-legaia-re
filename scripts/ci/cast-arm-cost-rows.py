#!/usr/bin/env python3
"""Read a cast film's per-frame cost off its retail recording, arm by arm.

    scripts/ci/cast-arm-cost-rows.py captures/<films>            # every sNN/retail under it
    scripts/ci/cast-arm-cost-rows.py captures/<films> --rust     # rows for CAST_ARM_COST_TAILS
    scripts/ci/cast-arm-cost-rows.py captures/<enemy films> --capture --rust
                                                     # rows for CAPTURE_ARM_COST_TAILS

The recorder's `meta.csv` (`scripts/ci/cast-sequence.py`) logs, per vsync, the
battle frame counter, the frame step and the frame-cost ring's newest entry.
On the first row of a frame those are the pass that just ran: its module
phase, the entry its own frame end stored, and the step that frame end read
for the next pass. A frame whose first row is read before its own frame end
is the exception, at any step: the counter bump and the ring store are not one
instruction apart, and a vsync read between them holds the frame before's
entry, which the frame's next row (same frame, same arm, another entry)
replaces. A step of `4` is a raw cost of `0x2D1` or more (the ring
stores such a frame as `0x2BC`, which reads `3`), so the frame's cost rung is
`4` on that step and the entry's own rung otherwise.

Per arm the rungs are run-length coded against the arm's one entry in
`ARM_STEPS` (`crates/engine-cast/src/cast_module_frame_step.rs`): a row
`(entry, arm, pass, rung)` wherever the rung changes, and at pass `0` when
the arm opens off its table entry. An arm the table leaves unread (`0`) takes
no row. See `docs/subsystems/battle-stage-camera.md`.

`--capture` reads the capture band (state `0x70`) of the enemy films the same
way, each film directory named as its line in `scripts/ci/film-sets/enemy.txt`
(the line's `--spell`, or for a `--physical` film the table's cast whose name
the film's carries), against `CAPTURE_ARM_STEPS` and the rows
`CAPTURE_ARM_COST_TAILS` already holds. A run shorter than `--min-run` frames
is folded into the run before it (a gate film's pass count drifts a frame or
two from the recording's), and an arm is printed only where the rows it would
take differ from the table's on `--min-run` passes or more. The recording's
first arm is skipped when the film opens inside it. A cast filmed in more
than one fight takes rows only where every film of it carries a cost column
and all of them read the same rows (edges within `--edge-slack` passes): an
arm that shows the stage costs what the fight draws, and a row read off one
fight moved the other's film the other way.
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
CAPTURE_BAND = ("112",)  # MagicCapturePhase2 0x70
FILM_SET = REPO / "scripts" / "ci" / "film-sets" / "enemy.txt"


def arm_steps() -> dict[int, list[int]]:
    body = re.search(r"const ARM_STEPS: &\[\(u32, &\[u8\]\)\] = &\[(.*?)\n\];", TABLE.read_text(), re.S).group(1)
    return {int(e): [int(x) for x in v.split(",") if x.strip()]
            for e, v in re.findall(r"\(\s*(\d+),\s*&\[(.*?)\],?\s*\)", body, re.S)}


def rung(cost: int, step: int) -> int:
    if step == 4:
        return 4
    return 1 if cost < 0xF1 else 2 if cost < 0x1FF else 3


def capture_steps() -> tuple[dict[int, dict[int, int]], dict[int, str], dict[tuple[int, int], list]]:
    """`CAPTURE_ARM_STEPS` as `{spell: {arm: step}}`, each spell's comment name,
    and the rows `CAPTURE_ARM_COST_TAILS` holds as `{(spell, arm): [(pass, step)]}`."""
    text = TABLE.read_text()
    body = text[text.index("const CAPTURE_ARM_STEPS"):text.index("const CONSOLE_CAPTURE_ARM_STEPS")]
    table, names, name = {}, {}, None
    for m in re.finditer(r"//\s*([a-z_0-9]+)[^\n]*\n|\(\s*(0x[0-9A-Fa-f]+),\s*&\[(.*?)\]\s*,?\s*\)", body, re.S):
        if m.group(1):
            name = m.group(1)
            continue
        spell = int(m.group(2), 16)
        table[spell] = {int(a, 0): int(b, 0) for a, b in re.findall(r"\((\w+),\s*(\w+)\)", m.group(3))}
        names[spell] = name
    tails: dict[tuple[int, int], list] = {}
    body = text[text.index("pub const CAPTURE_ARM_COST_TAILS"):]
    for s, arm, at, k in re.findall(r"\((0x[0-9A-Fa-f]+),\s*(\d+),\s*(\d+),\s*(\d+)\)", body[:body.index("];")]):
        tails.setdefault((int(s, 16), int(arm)), []).append((int(at), int(k)))
    return table, names, tails


def film_spells(names: dict[int, str]) -> dict[str, int]:
    """Each enemy film's cast id: its line's `--spell`, or the one table cast
    whose name its own name carries."""
    out = {}
    for line in FILM_SET.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        name, *args = line.split()
        if "--spell" in args:
            out[name] = int(args[args.index("--spell") + 1], 16)
            continue
        hits = [s for s, n in names.items() if n and n in name]
        if len(hits) == 1:
            out[name] = hits[0]
    return out


def capture_main(a) -> int:
    table, names, tails = capture_steps()
    spells = film_spells(names)
    total = 0
    costed: dict[int, list[bool]] = {}
    for name, spell in spells.items():
        meta = a.films / name / "retail" / "meta.csv"
        ok = meta.exists() and "cost" in meta.open().readline()
        costed.setdefault(spell, []).append(ok)
    found: dict[tuple[int, int], list] = {}
    for d in sorted(a.films.glob("*/retail/meta.csv")):
        name = d.parts[-3]
        spell = spells.get(name)
        if spell not in table:
            continue
        rows = list(csv.DictReader(open(d)))
        if not rows or "cost" not in rows[0]:
            continue
        runs_by_arm = arm_runs(d, CAPTURE_BAND)
        first = runs_by_arm[0][0] if runs_by_arm else None
        lines = []
        for arm, runs in runs_by_arm:
            base = table[spell].get(arm)
            if base is None or (arm == first and arm != 0):
                continue
            merged: list[list[int]] = []
            at = 0
            for k, n in runs:
                if merged and (n < a.min_run or merged[-1][1] == k):
                    merged[-1][2] += n
                else:
                    merged.append([at, k, n])
                at += n
            want = [(0 if i == 0 else s, k) for i, (s, k, _) in enumerate(merged)]
            have = [(0, base)] + tails.get((spell, arm), [])
            step_at = lambda rows_, p: [k for s, k in rows_ if p >= s][-1]
            passes = sum(n for _, n in runs)
            if sum(step_at(want, p) != step_at(have, p) for p in range(passes)) < a.min_run:
                continue
            found.setdefault((spell, arm), []).append(
                (name, [(s, k) for s, k in want if (s, k) != (0, base)]))
            if not a.rust:
                print(f"{name} {spell:#04x} arm {arm:3} table {base}: " + " ".join(f"{k}x{n}" for k, n in runs))
    by_film: dict[str, list] = {}
    for (spell, arm), reads in sorted(found.items()):
        if not all(costed.get(spell, [False])):
            print(f"// skip {spell:#04x} arm {arm}: a film of the cast has no cost column", file=sys.stderr)
            continue
        first = reads[0][1]
        same = len(reads) == costed[spell].count(True) and all(
            len(r) == len(first) and all(k == k2 and abs(s - s2) <= a.edge_slack
                                         for (s, k), (s2, k2) in zip(r, first))
            for _, r in reads)
        if not same:
            print(f"// skip {spell:#04x} arm {arm}: the films read different rows", file=sys.stderr)
            continue
        by_film.setdefault(reads[0][0], []).extend((spell, arm, s, k) for s, k in first)
    for name, lines in by_film.items():
        total += len(lines)
        if a.rust:
            print(f"    // {name}")
            for spell, arm, s, k in lines:
                print(f"    ({spell:#04X}, {arm}, {s}, {k}),".replace("0X", "0x"))
    print(f"// {total} rows", file=sys.stderr)
    return 0


def arm_runs(meta: Path, band=BAND) -> list[tuple[int, list[tuple[int, int]]]]:
    """`(arm, [(rung, frames), ..])` for each arm of the band, in order."""
    rows = list(csv.DictReader(open(meta)))
    frames = []
    for i in range(1, len(rows)):
        p, r = rows[i - 1], rows[i]
        if r["frame"] == p["frame"] or r["st7"] not in band:
            continue
        n = rows[i + 1] if i + 1 < len(rows) else None
        # A frame whose next row (same frame, same arm) holds another entry
        # had its first row read before its own frame end ran: the entry and
        # step there are the frame before's, and the next row holds its own.
        if n is not None and n["frame"] == r["frame"] and n["phase"] == r["phase"] \
                and n["cost"] != r["cost"]:
            r = dict(r, cost=n["cost"], step=n["step"])
        frames.append(r)
    out = []
    for arm, g in itertools.groupby(frames, key=lambda r: int(r["phase"])):
        rungs = [rung(int(r["cost"]), int(r["step"])) for r in g]
        out.append((arm, [(k, len(list(v))) for k, v in itertools.groupby(rungs)]))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("films", type=Path, help="directory of sNN/retail recordings")
    ap.add_argument("--rust", action="store_true", help="print the rows as Rust tuples")
    ap.add_argument("--capture", action="store_true", help="the enemy films' capture band")
    ap.add_argument("--min-run", type=int, default=3, help="--capture: shortest run kept, in frames")
    ap.add_argument("--edge-slack", type=int, default=2, help="--capture: passes two films' edges may differ by")
    a = ap.parse_args()
    if a.capture:
        return capture_main(a)
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
