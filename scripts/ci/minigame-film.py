#!/usr/bin/env python3
"""Minigame film: one retail minigame state, one pad schedule, both sides game frame by game frame.

Retail: `autorun_minigame_film.lua` under PCSX-Redux (the runner stages the disc
with no sibling .ppf). Engine: `legaia-engine retail-compare` with
LEGAIA_RC_MINIGAME_FILM_OUT, which takes the seeded state's warp, stands the
session where the capture's first vsync does and writes the same words a tick.

Both sides are cut into game frames (a row whose clock column moved) and paired
on that clock's value, so a vsync of entry latency on either side does not
read as a divergence. The report is the first diverging game frame per column
and each column's agreement. Output under captures/ (retail-derived, never
committed).
"""
import argparse
import csv
import os
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# Per minigame: the retail words (name, address, kind), the columns that make
# a game frame (a row is kept when one of them moved) and the pairing key.
SETS = {
    "dance": dict(
        words=[
            ("state", 0x801D5334, "u32"), ("counter", 0x801D5158, "u32"), ("go", 0x801D515C, "s32"),
            ("lead", 0x801D513C, "s32"),
            ("phase", 0x801D581C, "u32"), ("song", 0x801D5820, "u32"),
            ("score0", 0x801D53CC, "u32"), ("score1", 0x801D53D0, "u32"), ("score2", 0x801D53D4, "u32"),
            ("gauge0", 0x801D544C, "u32"), ("gauge1", 0x801D5450, "u32"), ("gauge2", 0x801D5454, "u32"),
            ("tri0", 0x801D534C, "u32"), ("tri1", 0x801D5350, "u32"), ("tri2", 0x801D5354, "u32"),
            ("chain0", 0x801D550C, "u32"), ("chain1", 0x801D5510, "u32"), ("chain2", 0x801D5514, "u32"),
            ("latch0", 0x801D548C, "u32"), ("latcht0", 0x801D54CC, "s32"), ("miss0", 0x801D568C, "u32"),
            ("newpad", 0x8007B874, "u16"),
            ("banked1", 0x801D5790, "u32"), ("banked2", 0x801D5794, "u32"),
            ("camkey", 0x801D5338, "s32"), ("camtimer", 0x801D533C, "s32"),
            ("pitch", 0x8007B790, "s16"), ("yaw", 0x8007B792, "s16"), ("roll", 0x8007B794, "s16"),
            ("eye_x", 0x800840B8, "s32"), ("eye_y", 0x800840BC, "s32"), ("eye_z", 0x800840C0, "s32"),
            ("dt", 0x1F800393, "u8"),
        ],
        # The banner counter through READY, the GO! accumulator through its
        # fade, the lead-in count, then the song's game frames.
        key=lambda r: (r["state"], r["counter"]) if r["state"] == 3
        else (r["state"], r["go"]) if r["state"] in (4, 5)
        else (r["state"], r["lead"]) if r["state"] in (6, 7)
        else None,
        # A song frame is a move of the beat phase or the state.
        song=lambda r: r["state"] >= 10,
        moved=["phase", "state"],
        skip=lambda r: r["state"] < 3 or r["state"] > 20,
    ),
    # The fishing pond (PROT 0972): the driver's state word, the cast, the
    # lure's accumulators, the band check and the fight; the rand() word is
    # the BIOS seed, which the engine's pond stream is seated on (harness).
    # A game frame is a move of any word; each state's frames pair by their
    # ordinal in the state.
    "fishing": dict(
        words=[
            ("state", 0x801D926C, "u32"), ("power", 0x801D9274, "s32"), ("record", 0x801D927C, "s32"),
            ("height", 0x801D9178, "s32"), ("band", 0x801D90E8, "u32"), ("bandcd", 0x801D90EC, "s32"),
            ("tension", 0x801D9168, "s32"), ("casts", 0x80084460, "s32"), ("rng", 0x80009010, "u32"),
            ("lx", 0x801D9174, "s32"), ("lz", 0x801D917C, "s32"), ("species", 0x801D91CC, "u32"),
            ("strength", 0x801D91B8, "s32"), ("dt", 0x1F800393, "u8"),
            ("gframe", 0x8007B798, "u32"), ("held", 0x8007B850, "u16"), ("edge", 0x8007B874, "u16"),
            ("pull", 0x801D9148, "s32"), ("redge", 0x801D90D8, "u32"), ("sway", 0x801D9140, "s32"),
            ("onhook", 0x801D91B4, "u32"), ("fmove", 0x801D910C, "u32"), ("points", 0x8008444C, "s32"),
            ("rep", 0x8007B93C, "u32"), ("repcd", 0x8007B940, "s32"),
        ],
        # The replay's pad: Cross closed-loop on the fishing globals.
        policy="fishing_catch",
        key=lambda r: None,
        song=lambda r: True,
        moved=["state", "power", "record", "height", "band", "bandcd", "tension", "rng", "lx", "lz"],
        group="state",
        skip=lambda r: r["state"] < 0xC or r["state"] > 0x28,
    ),
    # The Muscle Dome hub's first visit (PROT 0977): the arm byte and its
    # counter family, each arm's frames by their ordinal in the arm.
    "hub": dict(
        words=[
            ("arm", 0x801D1A78, "u8"),
            ("backdrop", 0x801D1A7C, "s32"), ("intro", 0x801D1A80, "s32"), ("card", 0x801D1A84, "s32"),
            ("scale", 0x801D1A88, "s32"), ("hold", 0x801D1A70, "s32"), ("card_hold", 0x801D1A8C, "s32"),
            ("dt", 0x1F800393, "u8"),
        ],
        key=lambda r: None,
        song=lambda r: True,
        moved=["arm", "backdrop", "intro", "card", "scale", "hold", "card_hold"],
        group="arm",
        skip=lambda r: r["arm"] < 0 or r["arm"] > 0x16,
    ),
}


def read(path):
    with open(path) as f:
        return [{k: int(v) for k, v in r.items()} for r in csv.DictReader(f)]


def shift_spans(pad, shift):
    out = []
    for item in filter(None, (i.strip() for i in pad.split(","))):
        rng, btn = item.split(":")
        lo, hi = (int(x) + shift for x in rng.split("-"))
        out.append(f"{lo}-{hi}:{btn}")
    return ",".join(out)


def frames(rows, spec):
    """One row per game frame, keyed for pairing: the pre-song frames on the
    set's own key, the song's frames by their ordinal from its first."""
    out, seen, n, prev = [], set(), 0, None
    for r in rows:
        if spec["skip"](r):
            continue
        if spec["song"](r):
            if prev is None or any(r[c] != prev[c] for c in spec["moved"]):
                group = r[spec["group"]] if "group" in spec else "song"
                if "group" in spec and (prev is None or prev[spec["group"]] != group):
                    n = 0
                n += 1
                out.append(((group, n), r))
            prev = r
            continue
        k = spec["key"](r)
        if k not in seen:
            seen.add(k)
            out.append((k, r))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("label")
    ap.add_argument("--set", required=True, choices=sorted(SETS))
    ap.add_argument("--pad", default="", help="A-B:BTN spans in vsyncs from the state's load (both sides)")
    ap.add_argument("--shift", type=int, default=0,
                    help="engine ticks the engine's row 1 stands ahead of retail's vsync 1: the engine's pad "
                         "spans move by this much (a no-pad run prints it as the vsync offset)")
    ap.add_argument("--frames", type=int, default=600, help="vsyncs filmed")
    ap.add_argument("--out")
    ap.add_argument("--iso", default=os.environ.get("LEGAIA_DISC_BIN"))
    ap.add_argument("--no-retail", action="store_true", help="reuse the retail film")
    ap.add_argument("--no-engine", action="store_true", help="reuse the engine film")
    ap.add_argument("--show", type=int, default=12, help="diverging frames printed per column")
    ap.add_argument("--replay", action="store_true",
                    help="rules differential: the engine ticks the session once per retail game frame on "
                         "retail's step and pad words (fishing; harness-only), paired by retail vsync")
    a = ap.parse_args()
    spec = SETS[a.set]
    out = Path(a.out or REPO / "captures" / "minigame-film" / a.label).resolve()
    out.mkdir(parents=True, exist_ok=True)
    if not a.no_retail:
        (out / ".tmp").mkdir(exist_ok=True)
        env = dict(
            os.environ,
            LEGAIA_PAD_SCRIPT=a.pad,
            LEGAIA_FILM_WORDS=",".join(f"{n}=0x{addr:08X}:{k}" for n, addr, k in spec["words"]),
            LEGAIA_FILM_POLICY=spec.get("policy", "") if a.replay else "",
            LEGAIA_PCSX_PROFILE_DIR=str(out / ".pcsx-profile"),
            TMPDIR=str(out / ".tmp"),
        )
        subprocess.run(
            ["timeout", "--kill-after=5", str(max(a.frames, 300)), "bash",
             str(REPO / "scripts/pcsx-redux/run_probe.sh"),
             # No window: a windowed probe in an agent or CI shell never quits.
             "--fast", "--headless", "--iso", a.iso, "--scenario", a.label,
             "--lua", str(REPO / "scripts/pcsx-redux/autorun_minigame_film.lua"),
             "--frames", str(a.frames), "--out-dir", str(out)],
            cwd=REPO, env=env, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
    if not a.no_engine:
        env = dict(
            os.environ,
            LEGAIA_RC_MINIGAME_FILM=shift_spans(a.pad, a.shift),
            LEGAIA_RC_MINIGAME_FILM_OUT=str(out / "engine.csv"),
            LEGAIA_RC_MINIGAME_FILM_TICKS=str(a.frames),
            RUST_LOG="error",
        )
        if a.replay:
            env["LEGAIA_RC_MINIGAME_FILM_REPLAY"] = str(out / "minigame_film.csv")
        exe = os.environ.get("LEGAIA_ENGINE_BIN") or REPO / "target" / "release-test" / "legaia-engine"
        subprocess.run(
            [str(exe), "retail-compare", "--out", str(out / "rc"), "--library", str(REPO / "saves/library"),
             "--extracted-root", str(REPO / "extracted"), "--manifest", str(REPO / "scripts/scenarios.toml"),
             "--filter", f"={a.label}"],
            cwd=REPO, env=env, check=True, stdout=subprocess.DEVNULL,
        )
    retail = read(out / "minigame_film.csv")
    engine = read(out / "engine.csv")
    if a.replay:
        by_v = {r["vsync"]: r for r in retail}
        ef = [(e["vsync"], e) for e in engine[1:]]
        rf = [(v, by_v[v]) for v, _ in ef if v in by_v]
    else:
        rf = frames(retail, spec)
        ef = frames(engine, spec)
    by_key = dict(ef)
    paired = [(r, by_key.get(k)) for k, r in rf]
    keys = {id(r): k for k, r in rf}
    both = [(r, e) for r, e in paired if e is not None]
    cols = [c for c in engine[0] if c in retail[0] and c not in ("vsync",)]
    print(f"{a.label}: retail {len(rf)} game frames, engine {len(ef)}, paired {len(both)}")
    if both:
        lag = [e["vsync"] - r["vsync"] for r, e in both]
        print(f"vsync offset engine - retail: first {lag[0]}, last {lag[-1]}, min {min(lag)}, max {max(lag)}")
    unpaired = [r for r, e in paired if e is None]
    if unpaired:
        print(f"retail game frames with no engine frame on the key: {len(unpaired)}, first at vsync "
              f"{unpaired[0]['vsync']} {keys[id(unpaired[0])]}")
    table = []
    for c in cols:
        off = [(r, e) for r, e in both if r[c] != e[c]]
        table.append((c, len(both) - len(off), len(both), off))
    print(f"{'column':>10} {'agree':>11}  first diverging game frame (retail vsync: retail / engine)")
    for c, ok, n, off in table:
        first = "" if not off else f"vsync {off[0][0]['vsync']}: {off[0][0][c]} / {off[0][1][c]}"
        print(f"{c:>10} {ok:>5}/{n:<5}  {first}")
    for c, ok, n, off in table:
        if off and a.show:
            print(f"-- {c}: " + ", ".join(f"{r['vsync']}:{r[c]}/{e[c]}" for r, e in off[: a.show]))
    with open(out / "film-score.tsv", "w") as f:
        f.write("column\tagree\tpaired\tfirst_vsync\tretail\tengine\n")
        for c, ok, n, off in table:
            fr = off[0] if off else None
            f.write(f"{c}\t{ok}\t{n}\t{fr[0]['vsync'] if fr else ''}\t{fr[0][c] if fr else ''}\t"
                    f"{fr[1][c] if fr else ''}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
