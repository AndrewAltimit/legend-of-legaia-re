#!/usr/bin/env python3
"""Field walk film: one retail state, one pad schedule, both sides pass by pass.

Retail: `autorun_field_walk_film.lua` under PCSX-Redux on a scratch-staged disc.
Engine: `legaia-engine retail-compare` with LEGAIA_RC_WALK_FILM, seated on the
state. Rows are paired on the player's walk: each retail vsync whose player or
camera moved is one pass, and pass N of the walk is compared with the engine's.
Output under captures/ (retail-derived, never committed).
"""
import argparse
import csv
import os
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
CAMERA = ["pitch", "yaw", "h", "eye_x", "eye_y", "eye_z", "focus_x", "focus_z"]


def read(path):
    with open(path) as f:
        return [{k: int(v) for k, v in r.items()} for r in csv.DictReader(f)]


def passes(rows):
    """Keep a row only when the player or the camera changed since the last kept."""
    keys = ["x", "y", "z", "heading", *CAMERA]
    out = []
    for r in rows:
        if not out or any(out[-1][k] != r[k] for k in keys):
            out.append(r)
    return out


def first_moved(rows, n=0):
    for i, r in enumerate(rows):
        if (r["x"], r["z"]) != (rows[0]["x"], rows[0]["z"]):
            return max(i - 1 - n, 0)
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("label")
    ap.add_argument("--pad", required=True, help="A-B:BTN spans; retail in vsyncs, engine scaled by --ticks-per-vsync")
    ap.add_argument("--frames", type=int, default=240)
    ap.add_argument("--out")
    ap.add_argument("--iso", default=os.environ.get("LEGAIA_DISC_BIN"))
    ap.add_argument("--no-retail", action="store_true", help="reuse the retail film")
    ap.add_argument("--no-engine", action="store_true", help="reuse the engine film")
    ap.add_argument("--run", action="store_true", help="the engine's Field Move option on Run (retail reads it off the state)")
    ap.add_argument("--show", type=int, default=40, help="pass rows printed")
    a = ap.parse_args()
    out = Path(a.out or REPO / "captures" / "walk-film" / a.label)
    out.mkdir(parents=True, exist_ok=True)
    if not a.no_retail:
        env = dict(os.environ, LEGAIA_PAD_SCRIPT=a.pad)
        subprocess.run(
            ["timeout", str(max(a.frames, 300)), "bash", str(REPO / "scripts/pcsx-redux/run_probe.sh"),
             "--fast", "--iso", a.iso, "--scenario", a.label,
             "--lua", str(REPO / "scripts/pcsx-redux/autorun_field_walk_film.lua"),
             "--frames", str(a.frames), "--out-dir", str(out)],
            cwd=REPO, env=env, check=False,
        )
    if not a.no_engine:
        env = dict(
            os.environ,
            LEGAIA_RC_WALK_FILM=a.pad,
            LEGAIA_RC_WALK_OUT=str(out / "engine.csv"),
            LEGAIA_RC_WALK_TICKS=str(a.frames),
            RUST_LOG="error",
        )
        if a.run:
            env["LEGAIA_RC_WALK_RUN"] = "1"
        exe = os.environ.get("LEGAIA_ENGINE_BIN") or REPO / "target" / "release-test" / "legaia-engine"
        subprocess.run(
            [str(exe), "retail-compare", "--out", str(out / "rc"), "--library", str(REPO / "saves/library"),
             "--extracted-root", str(REPO / "extracted"), "--manifest", str(REPO / "scripts/scenarios.toml"),
             "--filter", f"={a.label}"],
            cwd=REPO, env=env, check=True, stdout=subprocess.DEVNULL,
        )
    retail = passes(read(out / "walk_film.csv"))
    engine = passes(read(out / "engine.csv"))
    print(f"{a.label}: retail {len(retail)} passes, engine {len(engine)} passes")
    fields = ["x", "y", "z", "heading", *CAMERA, "offset_ease", "cursor", "rate"]
    # Pair each retail pass with the engine's last row standing on the same spot:
    # the engine walks a vsync at a time and eases on the pass grid.
    by_spot = {}
    for r in engine:
        by_spot[(r["x"], r["z"])] = r
    paired = [(r, by_spot.get((r["x"], r["z"]))) for r in retail]
    on_path = [(r, e) for r, e in paired if e is not None]
    if on_path:
        off = {f: sum(1 for r, e in on_path if r[f] != e[f]) for f in CAMERA}
        print(f"paired {len(on_path)}/{len(retail)} passes on the engine's path; camera fields off: {off}")
    print("pass  side " + " ".join(f"{f:>9}" for f in fields))
    for i in range(min(a.show, max(len(retail), len(engine)))):
        for side, rows in (("R", retail), ("E", engine)):
            if i < len(rows):
                print(f"{i:4}  {side}   " + " ".join(f"{rows[i][f]:>9}" for f in fields))
    report_actors(out, a.frames)
    return 0


def report_actors(out, frames):
    """Per placement: heading and position against retail's at the same vsync."""
    rp, ep = out / "walk_film_actors.csv", out / "engine.actors.csv"
    if not (rp.exists() and ep.exists()):
        return
    with open(rp) as f:
        retail = list(csv.DictReader(f))
    engine = {}
    with open(ep) as f:
        for r in csv.DictReader(f):
            engine[(int(r["tick"]), int(r["flat"]))] = r
    stats = {}
    for r in retail:
        v, flat = int(r["vsync"]), int(r["flat"])
        e = engine.get((v, flat))
        if e is None:
            continue
        h = (int(r["heading"]) - int(e["heading"]) + 2048) % 4096 - 2048
        d = abs(int(r["x"]) - int(e["x"])) + abs(int(r["z"]) - int(e["z"]))
        s = stats.setdefault(flat, dict(n=0, hoff=0, poff=0, first=None))
        s["n"] += 1
        if abs(h) > 16 or d > 8:
            s["hoff"] += abs(h) > 16
            s["poff"] += d > 8
            s["first"] = s["first"] or (v, r["x"], r["z"], r["heading"], e["x"], e["z"], e["heading"])
    off = {f: s for f, s in stats.items() if s["first"]}
    print(f"placements {len(stats)}, off retail {len(off)}")
    for f, s in sorted(off.items()):
        print(f"  flat {f}: {s['n']} rows, heading off {s['hoff']}, pos off {s['poff']}, first (vsync, R x z h, E x z h) {s['first']}")


if __name__ == "__main__":
    raise SystemExit(main())
