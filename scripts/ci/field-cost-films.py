#!/usr/bin/env python3
"""The field frame-cost model's gates: retail film rungs, unanchored step, state rings.

The model (`legaia_engine_core::world::field_frame_cost`) prices each field frame off
what it draws, and the pacer law `FUN_80016B6C` turns the price into a frame step. Three
measurements score it against retail, all on PCSX-Redux's scale:

- **films**: every game frame of a retail idle film (one per state in
  `scripts/ci/film-sets/field-cost.txt`) priced off the engine's counts at the same
  vsync, run through the sixteen-entry ring seeded from the state's own, against the step
  retail stored (`fit` and `extra` sets reported apart);
- **step**: the engine's own step a vsync, unanchored (play's path), against retail's;
- **states**: the price of the engine's first frames on each state of
  `scripts/ci/film-sets/field-cost-states.txt` against the rung of the state's saved ring,
  with the rms of price against the ring's median.

The retail films are cached in `captures/film-retail/field-cost/<state>/` (`record`
writes them; never delete that folder). The engine side is re-run every time:

    scripts/ci/field-cost-films.py record                  # film what the cache lacks
    scripts/ci/field-cost-films.py score                   # engine runs + every gate
    scripts/ci/field-cost-films.py score --only 'son*' -v  # a few films, per-film rows

Report only: it gates nothing. See docs/subsystems/engine.md, "The field frame-cost
model", and docs/tooling/walk-film.md.
"""

import argparse
import concurrent.futures
import csv
import fnmatch
import math
import os
import re
import shutil
import statistics
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
CACHE = REPO / "captures" / "film-retail" / "field-cost"
FILMS = REPO / "scripts" / "ci" / "film-sets" / "field-cost.txt"
STATES = REPO / "scripts" / "ci" / "film-sets" / "field-cost-states.txt"
PAD = "900-901:L2"
FRAMES = 300
STATE_TICKS = 12


def lines(path):
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].split()
        if line:
            yield line


def film_set(only):
    """`{state: (set, vsyncs scored)}`; a line's third column shortens its film."""
    out = {x[0]: (x[1], int(x[2]) if len(x) > 2 else FRAMES) for x in lines(FILMS)}
    return {k: v for k, v in out.items() if not only or any(fnmatch.fnmatchcase(k, g) for g in only)}


def state_set(only):
    out = [x[0] for x in lines(STATES)]
    return [k for k in out if not only or any(fnmatch.fnmatchcase(k, g) for g in only)]


def step_of(c):
    return 1 if c < 0xF1 else 2 if c < 0x1FF else 3 if c < 0x2D1 else 4


def law(c, ring):
    """The pacer's step for a frame of cost `c` over `ring` (already holding `c`), at
    the field floor `2`; a frame past the `0x2D0` rung steps `4`."""
    return 4 if c >= 0x2D0 else max(2, step_of(max(ring)))


def push(ring, c):
    return (ring + [c if c < 0x2D0 else 0x2BC])[-16:]


# ---------------------------------------------------------------- retail


def stalled(rows, tail=60):
    return len(rows) >= tail and len({r["ring_cur"] for r in rows[-tail:]}) == 1


def read_rows(path):
    try:
        with open(path) as f:
            return list(csv.DictReader(f))
    except FileNotFoundError:
        return []


def complete(d):
    rows = read_rows(d / "walk_film.csv")
    return len(rows) >= FRAMES and "frame_cost" in rows[0]


def record_one(label, force, timeout):
    out = CACHE / label
    if complete(out) and not force:
        return label, "cached"
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, LEGAIA_PAD_SCRIPT=PAD)
    for fast in (True, False):
        cmd = ["timeout", "--kill-after=5", str(timeout), "bash", str(REPO / "scripts/pcsx-redux/run_probe.sh"),
               *(["--fast"] if fast else []), "--headless", "--iso", os.environ["LEGAIA_DISC_BIN"],
               "--scenario", label, "--lua", str(REPO / "scripts/pcsx-redux/autorun_field_walk_film.lua"),
               "--frames", str(FRAMES), "--out-dir", str(out)]
        with open(out / "pcsx.log", "w") as log:
            subprocess.run(cmd, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
        rows = read_rows(out / "walk_film.csv")
        if len(rows) >= FRAMES and not stalled(rows):
            return label, "recompiler" if fast else "interpreter"
    # A state saved inside a load (game mode 2, the pacer parked) films no game frame
    # on either CPU; the film is kept and scores nothing.
    return label, "stalled" if len(read_rows(out / "walk_film.csv")) >= FRAMES else "FAILED"


# ---------------------------------------------------------------- engine


def engine_run(label, out, ticks, exe, timeout):
    o = out / label
    o.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, LEGAIA_RC_NO_COST_ANCHOR="1", LEGAIA_RC_WALK_COST="1", LEGAIA_RC_WALK_FILM=PAD,
               LEGAIA_RC_WALK_OUT=str(o / "engine.csv"), LEGAIA_RC_WALK_TICKS=str(ticks), RUST_LOG="error")
    with open(o / "log", "w") as log:
        subprocess.run(["timeout", str(timeout), exe, "retail-compare", "--out", str(o / "rc"), "--library",
                        str(REPO / "saves/library"), "--extracted-root", str(REPO / "extracted"), "--manifest",
                        str(REPO / "scripts/scenarios.toml"), "--filter", f"={label}"],
                       cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
    shutil.rmtree(o / "rc", ignore_errors=True)
    for p in o.iterdir():
        if p.name not in ("engine.cost.csv", "engine.ring.txt", "log"):
            p.unlink()


def engine_side(path):
    eng = {int(r["tick"]): r for r in read_rows(path / "engine.cost.csv")}
    ring, sb = [], None
    try:
        t = (path / "engine.ring.txt").read_text()
        m = re.search(r"\[([\d, ]+)\]", t)
        ring = [int(x) for x in m.group(1).split(",")] if m else []
        m = re.search(r"step_bytes=Some\(\((\d+), (\d+)\)\)", t)
        sb = int(m.group(2)) if m else None
    except FileNotFoundError:
        pass
    return eng, ring, sb


# ---------------------------------------------------------------- scores


def film_rows(label, vsyncs):
    return [r for r in read_rows(CACHE / label / "walk_film.csv") if int(r["vsync"]) <= vsyncs]


def film_frames(label, out, vsyncs):
    """The retail film's game frames (a mode-3 vsync whose ring cursor moved), each with
    the engine's price on the last game tick at or before it, in the film's first scene."""
    eng, ring, _ = engine_side(out / label)
    if not eng:
        return None
    scene0 = eng.get(0, {}).get("scene")
    frames, prev = [], None
    for r in film_rows(label, vsyncs):
        if r["mode"] != "3":
            prev = None
            continue
        if prev is not None and r["ring_cur"] != prev["ring_cur"]:
            v = int(r["vsync"])
            e = next((eng[t] for t in range(v, max(v - 6, -1), -1) if t in eng and eng[t]["fired"] == "1"), None)
            if e is not None and e["scene"] == scene0 and int(e["cost"]) >= 0 and \
                    (int(e["dec_draws"]) or int(e["ground"]) or int(e["npcs"])):
                frames.append((int(r["frame_cost"]), int(r["step"]), int(e["cost"])))
        prev = r
    return ring, frames


def score_film(ring, frames):
    ring = [(x if x < 0x2D0 else 0x2BC) for x in ring][-16:]
    good = 0
    for _cost, step, price in frames:
        ring = push(ring, price)
        good += law(price, ring) == step
    return good, len(frames)


def score_step(label, out, vsyncs):
    eng, _, _ = engine_side(out / label)
    g = n = 0
    for r in film_rows(label, vsyncs):
        e = eng.get(int(r["vsync"]))
        if e is None or r["mode"] != "3" or not r.get("step"):
            continue
        n += 1
        g += int(e["step"]) == int(r["step"])
    return g, n


def score_state(label, out):
    eng, ring, sb = engine_side(out / label)
    fired = [r for t, r in sorted(eng.items()) if r["fired"] == "1"]
    if not ring or not fired or fired[-1]["scene"].startswith("map0"):
        return None
    price, med = int(fired[-1]["cost"]), statistics.median(ring)
    ok = law(price, [price]) == max(2, step_of(min(med, 0x2BC)))
    return ok, price, med, sb


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("cmd", choices=("record", "score"))
    ap.add_argument("--only", action="append", default=[], metavar="GLOB", help="state labels (repeatable)")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--force", action="store_true", help="record: re-film a cached state")
    ap.add_argument("--out", type=Path, default=REPO / "target" / "field-cost-films", help="score: engine work directory")
    ap.add_argument("--no-run", action="store_true", help="score: reuse the engine side under --out")
    ap.add_argument("--no-states", action="store_true", help="score: skip the state rings")
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("-v", "--verbose", action="store_true", help="score: per-film and per-state rows")
    a = ap.parse_args()
    films = film_set(a.only)
    if a.cmd == "record":
        if not os.environ.get("LEGAIA_DISC_BIN"):
            sys.exit("LEGAIA_DISC_BIN is unset")
        CACHE.mkdir(parents=True, exist_ok=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, a.jobs)) as pool:
            for f in concurrent.futures.as_completed(
                    [pool.submit(record_one, lab, a.force, a.timeout) for lab in films]):
                print("%-40s %s" % f.result(), flush=True)
        return 0
    films = {k: v for k, v in films.items() if complete(CACHE / k)}
    states = [] if a.no_states else state_set(a.only)
    if not films and not states:
        print(f"[skip] no retail films under {CACHE}")
        return 0
    if not a.no_run:
        exe = os.environ.get("LEGAIA_ENGINE_BIN") or str(REPO / "target" / "release-test" / "legaia-engine")
        if not Path(exe).is_file():
            sys.exit(f"no engine binary at {exe} (cargo build -p legaia-engine-shell --profile release-test --features oracles)")
        jobs = [(lab, a.out / "films", n) for lab, (_, n) in films.items()] + [(lab, a.out / "states", STATE_TICKS) for lab in states]
        with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, a.jobs)) as pool:
            list(pool.map(lambda j: engine_run(*j, exe, a.timeout), jobs))
    tot = {}
    for lab, (s, vsyncs) in sorted(films.items()):
        ff = film_frames(lab, a.out / "films", vsyncs)
        if ff is None:
            print(f"  [no engine film] {lab}")
            continue
        g, n = score_film(*ff)
        sg, sn = score_step(lab, a.out / "films", vsyncs)
        t = tot.setdefault(s, [0, 0, 0, 0])
        t[0] += g; t[1] += n; t[2] += sg; t[3] += sn
        if a.verbose:
            med = statistics.median([c for c, _, _ in ff[1]]) if ff[1] else 0
            pm = statistics.median([p for _, _, p in ff[1]]) if ff[1] else 0
            print(f"  {s:6} {lab:40} frames {g:4}/{n:<4} step {sg:4}/{sn:<4} retail {med:5.0f} price {pm:5.0f}")
    for s, (g, n, sg, sn) in tot.items():
        print(f"films {s}: rung {g}/{n}, unanchored step {sg}/{sn}")
    if len(tot) > 1:
        print(f"films all: rung {sum(t[0] for t in tot.values())}/{sum(t[1] for t in tot.values())}, "
              f"unanchored step {sum(t[2] for t in tot.values())}/{sum(t[3] for t in tot.values())}")
    if states:
        res = {lab: score_state(lab, a.out / "states") for lab in states}
        res = {k: v for k, v in res.items() if v is not None}
        ok = sum(v[0] for v in res.values())
        rms = math.sqrt(statistics.mean((v[1] - v[2]) ** 2 for v in res.values())) if res else float("nan")
        print(f"states: rings {ok}/{len(res)}, price rms {rms:.1f} hblanks")
        for lab, (good, price, med, sb) in sorted(res.items()):
            if a.verbose or not good:
                print(f"  {'ok ' if good else 'OFF'} {lab:40} ring median {med:5.0f} price {price:5} saved step {sb}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
