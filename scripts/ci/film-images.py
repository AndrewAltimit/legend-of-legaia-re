#!/usr/bin/env python3
"""Report the cast-sequence films' image channel against a committed table.

A film's image score moves a little with every engine change, so it is not a
ratchet: the cue channel is (`film-cues.py`). It is still the number a wave's
film work is measured in, and it lived only in lane reports. This keeps the
last blessed per-film table of the four film sets in the repository and says
what a fresh run moved:

    scripts/ci/cast-sequence-batch.py scripts/ci/film-sets/strike.txt --out captures/films/strike
    scripts/ci/film-images.py --report captures/films/strike
    scripts/ci/film-images.py --report --tsv lane-table.tsv --min-move 0.005
    scripts/ci/film-images.py --write captures/films/strike      # take the measurement as the table
    scripts/ci/film-images.py --self-test

A measurement is a directory of film directories (each film's `report.json`,
written by `legaia-engine cast-sequence-score`) or, with `--tsv`, a table that
has a `film` and an `image` column (a lane's own summary; a long table whose
`key` column marks the film row with `FILM` is read on those rows, its image
in the column after `key`). A film belongs to the set whose batch file under
`scripts/ci/film-sets/` names it; a film no set names is listed and left out.

`--report` prints each set's mean over the films both sides hold, then every
film that moved by `--min-move` or more, falls first, and the films only one
side holds. It exits 0 whatever it finds: the table is a reference, not a
gate. `--write` replaces the measured films' rows and keeps the rest, so one
set can be re-blessed without filming the other three.

The table is `scripts/ci/film-image-table.tsv`: `set`, `film`, `image` (the
mean of the per-pair block score), `min` (the lowest pair), `pairs`, and the
lowest scored part of the film (`low_part`, `low_image`, `low_pairs`; a part
is one action state or module arm, and only parts of four pairs or more
count). A column a `--tsv` source does not carry reads `-`.
See `docs/tooling/cast-sequence-oracle.md`, "The image table".
"""

from __future__ import annotations

import argparse
import csv
import json
import shlex
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
TABLE = REPO / "scripts" / "ci" / "film-image-table.tsv"
SETS = REPO / "scripts" / "ci" / "film-sets"
# The gate set re-films members of these four; it owns no rows of its own.
SET_ORDER = ("enemy", "strike", "seru", "arts")
COLUMNS = ("set", "film", "image", "min", "pairs", "low_part", "low_image", "low_pairs")
# Fewer pairs than this and a part's mean is one frame's noise.
PART_MIN_PAIRS = 4


def set_of() -> dict[str, str]:
    """`{film: set}` from the committed batch files."""
    out: dict[str, str] = {}
    for name in SET_ORDER:
        path = SETS / f"{name}.txt"
        if not path.is_file():
            continue
        for line in path.read_text().splitlines():
            line = line.split("#", 1)[0].strip()
            if line:
                out.setdefault(shlex.split(line)[0], name)
    return out


def part_name(key) -> str:
    """A segment key as one token: `State=0x1f`, `Arm=3`."""
    if isinstance(key, dict) and len(key) == 1:
        (kind, value), = key.items()
        if isinstance(value, int):
            return f"{kind}={value:#04x}" if kind == "State" else f"{kind}={value}"
        return f"{kind}={json.dumps(value, separators=(',', ':'))}"
    return json.dumps(key, separators=(",", ":"))


def measure(film: Path) -> dict | None:
    """One film's row off its `report.json`, or None when it did not score."""
    report = film / "report.json"
    if not report.is_file():
        return None
    r = json.loads(report.read_text())
    if r.get("mean_within") is None:
        return None
    row = {
        "image": f"{float(r['mean_within']):.4f}",
        "min": f"{float(r['min_within']):.4f}" if r.get("min_within") is not None else "-",
        "pairs": str(int(r.get("valid_pairs") or 0)),
        "low_part": "-", "low_image": "-", "low_pairs": "-",
    }
    parts = [s for s in r.get("segments") or []
             if s.get("mean_within") is not None and int(s.get("pairs") or 0) >= PART_MIN_PAIRS]
    if parts:
        low = min(parts, key=lambda s: s["mean_within"])
        row.update(low_part=part_name(low.get("key")), low_image=f"{float(low['mean_within']):.4f}",
                   low_pairs=str(int(low["pairs"])))
    return row


def measure_dirs(dirs: list[Path]) -> dict[str, dict]:
    films: dict[str, dict] = {}
    for root in dirs:
        for film in sorted(p for p in root.iterdir() if p.is_dir()):
            got = measure(film)
            if got is not None:
                films[film.name] = got
    return films


def read_tsv(path: Path) -> dict[str, dict]:
    """A lane's summary table: `film` and `image` columns, or the long form."""
    films: dict[str, dict] = {}
    with open(path, newline="") as f:
        rows = list(csv.reader(f, delimiter="\t"))
    if not rows:
        return films
    head = rows[0]
    if "film" not in head:
        sys.exit(f"{path}: no `film` column")
    fi = head.index("film")
    if "image" in head:
        col, keep = head.index("image"), (lambda r: True)
    elif "key" in head:
        ki = head.index("key")
        col, keep = ki + 1, (lambda r: len(r) > ki and r[ki] == "FILM")
    else:
        sys.exit(f"{path}: neither an `image` column nor a `key` column")
    named = {c: head.index(c) for c in ("min", "pairs", "low_part", "low_image", "low_pairs") if c in head}
    for r in rows[1:]:
        if len(r) <= max(fi, col) or not keep(r):
            continue
        try:
            image = float(r[col])
        except ValueError:
            continue
        row = {"image": f"{image:.4f}", "min": "-", "pairs": "-", "low_part": "-", "low_image": "-", "low_pairs": "-"}
        for c, i in named.items():
            if i < len(r) and r[i] != "":
                row[c] = r[i]
        films[r[fi]] = row
    return films


def read_table(path: Path = TABLE) -> dict[str, dict]:
    if not path.is_file():
        return {}
    with open(path, newline="") as f:
        return {r["film"]: {c: r[c] for c in COLUMNS} for r in csv.DictReader(f, delimiter="\t")}


def write_table(rows: dict[str, dict], path: Path = TABLE) -> None:
    order = {s: i for i, s in enumerate(SET_ORDER)}
    with open(path, "w", newline="") as f:
        w = csv.writer(f, delimiter="\t", lineterminator="\n")
        w.writerow(COLUMNS)
        for film, r in sorted(rows.items(), key=lambda kv: (order.get(kv[1]["set"], 99), kv[0])):
            w.writerow([r[c] if c != "film" else film for c in COLUMNS])


def compare(table: dict[str, dict], films: dict[str, dict], sets: dict[str, str], min_move: float) -> list[str]:
    """The report's lines."""
    lines = []
    measured_sets = sorted({sets[f] for f in films if f in sets}, key=SET_ORDER.index)
    moved = []
    for s in measured_sets:
        both = [f for f in films if sets.get(f) == s and f in table]
        if both:
            was = sum(float(table[f]["image"]) for f in both) / len(both)
            now = sum(float(films[f]["image"]) for f in both) / len(both)
            lines.append(f"[film-images] {s}: mean {was:.4f} -> {now:.4f} ({now - was:+.4f}) over {len(both)} films")
        new = sorted(f for f in films if sets.get(f) == s and f not in table)
        gone = sorted(f for f, r in table.items() if r["set"] == s and f not in films)
        if new:
            lines.append(f"  [not in the table] {len(new)}: {' '.join(new)}")
        if gone:
            lines.append(f"  [not measured] {len(gone)}: {' '.join(gone)}")
        for f in both:
            d = float(films[f]["image"]) - float(table[f]["image"])
            if abs(d) >= min_move:
                moved.append((d, s, f))
    falls = sorted(m for m in moved if m[0] < 0)
    rises = sorted((m for m in moved if m[0] > 0), reverse=True)
    for tag, rows in (("fall", falls), ("rise", rises)):
        for d, s, f in rows:
            low = films[f]
            part = f", lowest part {low['low_part']} {low['low_image']}" if low["low_part"] != "-" else ""
            lines.append(f"  [{tag}] {s} {f}: {table[f]['image']} -> {films[f]['image']} ({d:+.4f}){part}")
    lines.append(f"[film-images] {len(falls)} falls, {len(rises)} rises at {min_move:g} or more")
    stray = sorted(f for f in films if f not in sets)
    if stray:
        lines.append(f"  [in no film set] {len(stray)}: {' '.join(stray)}")
    return lines


def self_test() -> int:
    sets = {"a": "enemy", "b": "enemy", "c": "strike", "d": "strike"}
    blank = {"min": "-", "pairs": "-", "low_part": "-", "low_image": "-", "low_pairs": "-"}
    table = {
        "a": {"set": "enemy", "film": "a", "image": "0.9000", **blank},
        "b": {"set": "enemy", "film": "b", "image": "0.8000", **blank},
        "c": {"set": "strike", "film": "c", "image": "0.9500", **blank},
    }
    films = {
        "a": {"image": "0.9100", **blank},
        "b": {"image": "0.7000", **blank},
        "d": {"image": "0.5000", **blank},
        "x": {"image": "0.5000", **blank},
    }
    lines = compare(table, films, sets, 0.005)
    bad = 0
    want = ["enemy: mean 0.8500 -> 0.8050", "[fall] enemy b", "[rise] enemy a", "[not in the table] 1: d",
            "[not measured] 1: c", "1 falls, 1 rises", "[in no film set] 1: x"]
    text = "\n".join(lines)
    for w in want:
        if w not in text:
            print(f"self-test: missing {w!r} in\n{text}")
            bad += 1
    if text.index("[fall]") > text.index("[rise]"):
        print("self-test: falls must come first")
        bad += 1
    if part_name({"State": 31}) != "State=0x1f" or part_name({"Arm": 3}) != "Arm=3":
        print("self-test: part_name")
        bad += 1
    print("self-test ok" if not bad else f"self-test: {bad} failures")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dirs", nargs="*", type=Path, help="directories of film directories")
    ap.add_argument("--tsv", action="append", default=[], type=Path, help="a summary table with `film` and `image` columns")
    ap.add_argument("--report", action="store_true", help="print what moved against the committed table")
    ap.add_argument("--write", action="store_true", help="take the measured films' rows into the committed table")
    ap.add_argument("--min-move", type=float, default=0.005, help="smallest move worth a line (default 0.005)")
    ap.add_argument("--table", type=Path, default=TABLE, help="the table to compare against and write")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    films: dict[str, dict] = {}
    for path in a.tsv:
        films.update(read_tsv(path))
    films.update(measure_dirs([d for d in a.dirs if d.is_dir()]))
    if not films:
        print("[skip] no film reports under the given directories and no table rows")
        return 0
    sets = set_of()
    table = read_table(a.table)
    if a.report or not a.write:
        print("\n".join(compare(table, films, sets, a.min_move)))
    if a.write:
        taken = 0
        for film, row in films.items():
            if film in sets:
                table[film] = {"set": sets[film], "film": film, **row}
                taken += 1
        # A film a set no longer names has no owner to re-measure it.
        table = {f: r for f, r in table.items() if f in sets}
        write_table(table, a.table)
        print(f"[write] {a.table}: {taken} rows taken, {len(table)} in the table")
    return 0


if __name__ == "__main__":
    sys.exit(main())
