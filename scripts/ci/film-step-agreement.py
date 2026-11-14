#!/usr/bin/env python3
"""Frame-step agreement of the cast-sequence films: retail's step against the engine's, by band.

    scripts/ci/film-step-agreement.py captures/films/arts captures/films/enemy
    scripts/ci/film-step-agreement.py captures/films/enemy --films
    scripts/ci/film-step-agreement.py captures/films/strike --costs /tmp/costs.tsv

Each argument is a directory of film directories as `cast-sequence-batch.py` leaves them
(`<film>/retail/meta.csv`, `<film>/engine/meta.csv`, `<film>/engine/cam.csv`). Both sides
are read on the game clock: a retail frame counts its step however many vsyncs it ran,
and an engine tick past its frame's step (`over`) counts nothing. Per film and band the
two sides' game time is spread over the steps `1..4`; the agreement is the overlap of
the two normalised spreads, and a set's figure weights each film's band by retail's
game time in it. Only the span both sides recorded is read: the states the engine film
holds, up to the end of retail's first Done band.

`--films` lists the bands under `.8` that hold thirty game ticks or more; `--costs`
writes every unloaded retail frame's cost (ring entry under the clamp, frame inside its
step plus one) with the fight's living party and monsters, for fitting a cost term.
Report-only: it reads recordings and writes nothing but `--costs`.
See `docs/tooling/cast-sequence-oracle.md` (the step agreement table).
"""

import argparse
import collections
import csv
import sys
from pathlib import Path

DONE = (0x50, 0x51, 0x52, 0x5A, 0x65)


def band(st: int) -> str:
    if st in DONE:
        return "done 50/51"
    if st in (0x1E, 0x1F, 0x20):
        return f"strike {st:02x}"
    if st in (0x33, 0x34):
        return "summon 33/34"
    if st == 0x70:
        return "capture 70"
    if 0x28 <= st <= 0x2E:
        return f"cast {st:02x}"
    if st in (0x6E, 0x6F):
        return "pre 6e/6f"
    if 0x3C <= st <= 0x40:
        return "heal 3c..40"
    if st < 0x1E:
        return "open <1e"
    return f"other {st:02x}"


def retail_frames(path: Path):
    """`(state, step, cost, vsyncs)` per frame, or None for a recording without the columns."""
    with open(path) as f:
        rows = list(csv.DictReader(f))
    if not rows or "frame" not in rows[0] or "step" not in rows[0]:
        return None, rows
    first, prev = [], None
    for r in rows:
        if r["frame"] != prev:
            first.append(r)
            prev = r["frame"]
    out = []
    for i, r in enumerate(first):
        nxt = first[i + 1] if i + 1 < len(first) else None
        try:
            step, st = max(1, int(r["step"])), int(r["st7"])
        except ValueError:
            continue
        cost = int(nxt["cost"]) if nxt and nxt.get("cost") not in (None, "") else -1
        span = int(nxt["vsync"]) - int(r["vsync"]) if nxt else step
        out.append((st, step, cost, span))
    return out, rows


def engine_clock(meta: Path, cam: Path):
    """Game ticks per `(state, step)`."""
    with open(cam) as f:
        steps = {r["tick"]: int(r["step"]) for r in csv.DictReader(f)}
    out = collections.Counter()
    with open(meta) as f:
        for r in csv.DictReader(f):
            if r.get("over", "0") == "1" or r["tick"] not in steps:
                continue
            out[(int(r["st7"]), steps[r["tick"]])] += 1
    return out


def alive(row: dict, keys) -> int:
    try:
        return sum(1 for k in keys if int(row[k]) > 0)
    except (KeyError, ValueError):
        return -1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("sets", nargs="+", type=Path)
    ap.add_argument("--films", action="store_true")
    ap.add_argument("--costs", type=Path)
    a = ap.parse_args()
    rows, costs = [], []
    for root in a.sets:
        for film in sorted(p for p in root.iterdir() if p.is_dir()):
            rm, em, ec = film / "retail/meta.csv", film / "engine/meta.csv", film / "engine/cam.csv"
            if not (rm.is_file() and em.is_file() and ec.is_file()):
                continue
            frames, raw = retail_frames(rm)
            if frames is None:
                continue
            eng = engine_clock(em, ec)
            held = {st for st, _ in eng}
            party = alive(raw[0], ("hp0", "hp1", "hp2"))
            mons = alive(raw[0], ("hp3", "hp4", "hp5", "hp6", "hp7"))
            rb = collections.defaultdict(collections.Counter)
            eb = collections.defaultdict(collections.Counter)
            done = False
            for st, step, cost, span in frames:
                b = band(st)
                if b.startswith("done"):
                    done = True
                elif done:
                    break
                if st not in held:
                    continue
                rb[b][step] += step
                if 0 < cost < 700 and span <= step + 1:
                    costs.append((root.name, film.name, party, mons, f"{st:02x}", step, cost))
            for (st, step), n in eng.items():
                eb[band(st)][step] += n
            for b, r in rb.items():
                e = eb.get(b)
                if not e:
                    continue
                rn, en = sum(r.values()), sum(e.values())
                overlap = sum(min(r[s] / rn, e[s] / en) for s in set(r) | set(e))
                rows.append((
                    root.name, film.name, b, rn, overlap,
                    sum(s * n for s, n in r.items()) / rn,
                    sum(s * n for s, n in e.items()) / en,
                ))
    agg = collections.defaultdict(lambda: [0, 0.0, 0.0, 0.0, 0])
    for setn, _, b, rn, ov, rmean, emean in rows:
        for key in ((setn, b), (setn, "ALL"), ("ALL", b), ("ALL", "ALL")):
            t = agg[key]
            t[0] += rn
            t[1] += ov * rn
            t[2] += rmean * rn
            t[3] += emean * rn
            t[4] += 1
    print("set\tband\tfilms\tretail_game\tagreement\tretail_mean_step\tengine_mean_step")
    for key in sorted(agg):
        t = agg[key]
        print(f"{key[0]}\t{key[1]}\t{t[4]}\t{t[0]}\t{t[1] / t[0]:.3f}\t{t[2] / t[0]:.2f}\t{t[3] / t[0]:.2f}")
    if a.films:
        print("\n# bands under .8 over 30 game ticks or more, largest loss first")
        for r in sorted(rows, key=lambda r: (r[4] - 1) * r[3]):
            if r[4] < 0.8 and r[3] >= 30:
                print(f"{r[0]}\t{r[1]}\t{r[2]}\t{r[3]}\t{r[4]:.2f}\tretail {r[5]:.2f}\tengine {r[6]:.2f}")
    if a.costs:
        with open(a.costs, "w") as f:
            f.write("set\tfilm\tparty\tmonsters\tstate\tstep\tcost\n")
            for c in costs:
                f.write("\t".join(map(str, c)) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
