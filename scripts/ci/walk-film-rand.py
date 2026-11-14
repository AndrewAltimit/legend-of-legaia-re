#!/usr/bin/env python3
"""Ratchet how far each field walk film's `rand()` stream agrees with retail's.

A walk film (`walk-film.py`) writes the BIOS `rand()` seed word once a vsync on
both sides. Every field consumer of the stream (a wanderer's turn, an ambient
part's op, a fog spawn) moves the word, so the count of the film's opening
draws on which the engine holds retail's word is one number for "the field
runs retail's draws on retail's clock" - and it can only be lost silently
while it lives in a lane report. This holds it to a baseline, a film at a time.

    scripts/ci/walk-film-rand.py --check                       # every cached film
    scripts/ci/walk-film-rand.py --check --jobs 6 --out target/walk-film-rand
    scripts/ci/walk-film-rand.py --only 'kor*' --keep          # a few films, engine CSVs kept
    scripts/ci/walk-film-rand.py --propose /tmp/rand.json      # a candidate baseline
    scripts/ci/walk-film-rand.py --self-test

The retail side is a cache of recordings, `captures/walk-film-retail/<state>/`
(`walk_film.csv` and the `manifest.txt` that names the film's pad and length),
made once with `walk-film.py <state> --pad .. --frames .. --out <that directory>`.
Only the engine side runs here: `walk-film.py --no-retail`, `--jobs` at once (it
is headless). Exit 0 with a `[skip]` line when the cache is absent, matching the
repo's disc-gated convention.

The count: each side's word column is reduced to its changes (one entry a
draw burst), up to three leading entries are dropped from either side to
absorb the vsync the state was saved on, and the entries are compared in
order until the first that differs. `agree` is that run's length and `draws`
the retail film's entries; a film that agrees to its end reads `agree ==
draws` (or one short, when the film's last retail vsync drew).

A baselined film fails `--check` when its `agree` falls. A film whose retail
recording has another number of entries than the baseline's was re-recorded
and is listed, not scored. A state captured on a patched disc
(`resident_patch` in `scripts/scenarios.toml`) is reported and never
baselined. A baselined film with no result is a failure: the cache holds the
whole set or none of it. A cached state the engine writes no film for and that
has no baseline row is listed (`[no film]`) and holds nothing. `--propose PATH` writes a candidate (never the
committed file), each film at the better of baseline and measurement; a lower
value needs a `scripts/ci/baseline-loosenings.toml` entry.
See `docs/tooling/walk-film.md`, "The rand ratchet".
"""

from __future__ import annotations

import argparse
import concurrent.futures
import csv
import fnmatch
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "walk-film-rand-baseline.json"
CACHE = REPO / "captures" / "walk-film-retail"
DRIVER = REPO / "scripts" / "ci" / "walk-film.py"
# Leading entries either side may drop: the save's vsync inside its frame.
LEAD = 4


def changes(path: Path, clock: str) -> list[tuple[int, str]]:
    """`(vsync, word)` on each row whose `rng` word differs from the row before."""
    out: list[tuple[int, str]] = []
    with open(path, newline="") as f:
        for r in csv.DictReader(f):
            if not out or out[-1][1] != r["rng"]:
                out.append((int(r[clock]), r["rng"]))
    return out


def agreement(retail: list[tuple[int, str]], engine: list[tuple[int, str]]) -> dict:
    """The longest run of equal words from a start within `LEAD` of each side's."""
    best = (-1, 0, 0)
    for a in range(LEAD):
        for b in range(LEAD):
            n = 0
            while a + n < len(retail) and b + n < len(engine) and retail[a + n][1] == engine[b + n][1]:
                n += 1
            if n > best[0]:
                best = (n, a, b)
    n, a, b = best
    ended = a + n >= len(retail) or b + n >= len(engine)
    return {
        "agree": n,
        "draws": len(retail),
        "parts_at": None if ended else retail[a + n][0],
    }


def read_kv(path: Path) -> dict[str, str]:
    out = {}
    for line in path.read_text().splitlines():
        k, sep, v = line.partition("=")
        if sep:
            out[k.strip()] = v.strip()
    return out


def patched_labels() -> set[str]:
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())
    return {s["label"] for s in manifest.get("scenarios", []) if s.get("resident_patch")}


def film(label: str, out: Path, exe: str, timeout: int) -> tuple[str, dict | None, str]:
    """Run one film's engine side into `out/<label>/`; its agreement or why not."""
    src, work = CACHE / label, out / label
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    for name in ("walk_film.csv", "walk_film_actors.csv", "manifest.txt"):
        if (src / name).is_file():
            os.symlink(src / name, work / name)
    kv = read_kv(src / "manifest.txt")
    cmd = [sys.executable, str(DRIVER), label, "--pad", kv.get("pad", ""), "--frames", kv.get("frames", "240"),
           "--no-retail", "--out", str(work), "--show", "0"]
    env = dict(os.environ, LEGAIA_ENGINE_BIN=exe)
    try:
        with open(out / f"{label}.log", "w") as log:
            rc = subprocess.run(cmd, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout).returncode
    except subprocess.TimeoutExpired:
        return label, None, f"timed out after {timeout}s"
    if not (work / "engine.csv").is_file():
        return label, None, f"no engine film (exit {rc}, see {out / (label + '.log')})"
    try:
        got = agreement(changes(work / "walk_film.csv", "vsync"), changes(work / "engine.csv", "tick"))
    except (KeyError, ValueError) as e:
        return label, None, f"unreadable film: {e}"
    return label, got, ""


def compare(base: dict[str, dict], films: dict[str, dict], errors: dict[str, str], patched: set[str]):
    """`(failures, re-recorded, rises)` of the measured films against the baseline."""
    failures, rerecorded, rises = [], [], []
    for name, want in sorted(base.items()):
        got = films.get(name)
        if got is None:
            failures.append(f"{name}: {errors.get(name, 'not measured')}")
        elif got["draws"] != want["draws"]:
            rerecorded.append(f"{name}: retail entries {want['draws']} -> {got['draws']}")
        elif got["agree"] < want["agree"]:
            at = f", parts at retail vsync {got['parts_at']}" if got["parts_at"] is not None else ""
            failures.append(f"{name}: agree {want['agree']} -> {got['agree']} of {got['draws']}{at}")
        elif got["agree"] > want["agree"]:
            rises.append(f"{name}: agree {want['agree']} -> {got['agree']} of {got['draws']}")
    return failures, rerecorded, rises


def propose(base: dict[str, dict], films: dict[str, dict], patched: set[str]) -> dict[str, dict]:
    out = {k: dict(v) for k, v in base.items()}
    for name, got in films.items():
        if name in patched:
            continue
        want = out.get(name)
        row = {"agree": got["agree"], "draws": got["draws"]}
        if want is None or want["draws"] != got["draws"]:
            out[name] = row
        else:
            want["agree"] = max(want["agree"], got["agree"])
    return dict(sorted(out.items()))


def self_test() -> int:
    bad = 0
    r = [(i, str(w)) for i, w in enumerate([1, 2, 3, 4, 5, 6])]
    e = [(i, str(w)) for i, w in enumerate([9, 2, 3, 4, 7])]
    got = agreement(r, e)
    if (got["agree"], got["draws"], got["parts_at"]) != (3, 6, 4):
        print(f"self-test: agreement {got}")
        bad += 1
    whole = agreement(r, r[1:])
    if whole["agree"] != 5 or whole["parts_at"] is not None:
        print(f"self-test: whole {whole}")
        bad += 1
    base = {"a": {"agree": 5, "draws": 6}, "b": {"agree": 2, "draws": 6}, "c": {"agree": 1, "draws": 3},
            "d": {"agree": 1, "draws": 1}}
    films = {"a": {"agree": 4, "draws": 6, "parts_at": 9}, "b": {"agree": 3, "draws": 6, "parts_at": None},
             "c": {"agree": 0, "draws": 4, "parts_at": 1}, "p": {"agree": 1, "draws": 9, "parts_at": 2}}
    failures, rerecorded, rises = compare(base, films, {"d": "no engine film", "q": "timed out"}, {"p"})
    if [f.split(":")[0] for f in failures] != ["a", "d"]:
        print(f"self-test: failures {failures}")
        bad += 1
    if [x.split(":")[0] for x in rerecorded] != ["c"] or [x.split(":")[0] for x in rises] != ["b"]:
        print(f"self-test: rerecorded {rerecorded} rises {rises}")
        bad += 1
    p = propose(base, films, {"p"})
    if p["a"]["agree"] != 5 or p["b"]["agree"] != 3 or p["c"] != {"agree": 0, "draws": 4} or "p" in p:
        print(f"self-test: propose {p}")
        bad += 1
    print("self-test ok" if not bad else f"self-test: {bad} failures")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true", help="fail below the baseline")
    ap.add_argument("--propose", metavar="PATH", help="write a candidate baseline (never the committed one)")
    ap.add_argument("--only", action="append", default=[], metavar="GLOB", help="film labels to run (repeatable)")
    ap.add_argument("--jobs", type=int, default=4, help="engine films at once")
    ap.add_argument("--out", type=Path, default=REPO / "target" / "walk-film-rand", help="work directory")
    ap.add_argument("--keep", action="store_true", help="keep each film's engine CSVs (default: logs and the summary only)")
    ap.add_argument("--timeout", type=int, default=300, help="one film's wall-clock cap, seconds")
    ap.add_argument("--no-build", action="store_true", help="accepted for the gate's command shape; this never builds")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    if a.propose and Path(a.propose).resolve() == BASELINE.resolve():
        sys.exit("--propose writes a candidate; the committed baseline changes by hand, with a ledger entry if it falls")
    labels = sorted(p.name for p in CACHE.iterdir() if (p / "walk_film.csv").is_file() and (p / "manifest.txt").is_file()) \
        if CACHE.is_dir() else []
    if not labels:
        print(f"[skip] no retail walk films under {CACHE}")
        return 0
    base = json.loads(BASELINE.read_text())["films"] if BASELINE.exists() else {}
    if a.only:
        labels = [x for x in labels if any(fnmatch.fnmatchcase(x, g) for g in a.only)]
        base = {k: v for k, v in base.items() if k in labels}
    exe = os.environ.get("LEGAIA_ENGINE_BIN") or str(REPO / "target" / "release-test" / "legaia-engine")
    if not Path(exe).is_file():
        sys.exit(f"no engine binary at {exe} (cargo build -p legaia-engine-shell --profile release-test --features oracles)")
    a.out.mkdir(parents=True, exist_ok=True)
    t0 = time.monotonic()
    films: dict[str, dict] = {}
    errors: dict[str, str] = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, a.jobs)) as pool:
        for f in concurrent.futures.as_completed([pool.submit(film, x, a.out, exe, a.timeout) for x in labels]):
            label, got, why = f.result()
            if got is None:
                errors[label] = why
            else:
                films[label] = got
            if not a.keep:
                shutil.rmtree(a.out / label, ignore_errors=True)
    patched = patched_labels()
    retail = {k: v for k, v in films.items() if k not in patched}
    failures, rerecorded, rises = compare(base, films, errors, patched)
    unbaselined = sorted(set(retail) - set(base))
    with open(a.out / "summary.tsv", "w") as f:
        f.write("film\tagree\tdraws\tparts_at\tbaseline\tnote\n")
        for name in sorted(films):
            g = films[name]
            note = "patched disc, not baselined" if name in patched else ""
            f.write(f"{name}\t{g['agree']}\t{g['draws']}\t{'' if g['parts_at'] is None else g['parts_at']}\t"
                    f"{base.get(name, {}).get('agree', '')}\t{note}\n")
    total = sum(g["agree"] for g in retail.values())
    want = sum(v["agree"] for k, v in base.items() if k in retail)
    print(f"[walk-film-rand] {len(retail)} retail-disc films, agree {total} of {sum(g['draws'] for g in retail.values())} "
          f"(baseline {want} over the {sum(1 for k in base if k in retail)} baselined); {len(failures)} below baseline, "
          f"{len(rises)} above; {len(rerecorded)} re-recorded; {len(unbaselined)} not baselined; "
          f"{len(films) - len(retail)} patched-disc films reported only; {time.monotonic() - t0:.0f}s at --jobs {a.jobs}")
    for tag, rows in (("re-recorded", rerecorded), ("below", failures), ("above", rises)):
        for line in rows:
            print(f"  [{tag}] {line}")
    if unbaselined:
        print(f"  [not baselined] {' '.join(unbaselined)}")
    # A state the engine writes no film for (one saved inside a scene change)
    # has nothing to hold; a baselined film that stops recording is a failure above.
    for name in sorted(set(errors) - set(base)):
        print(f"  [no film] {name}: {errors[name]}")
    if a.propose:
        Path(a.propose).write_text(json.dumps({"films": propose(base, films, patched)}, indent=2) + "\n")
        print(f"[propose] {a.propose}")
    return 1 if (a.check and failures) else 0


if __name__ == "__main__":
    sys.exit(main())
