#!/usr/bin/env python3
"""Ratchet the cast-sequence films' sound-cue channel.

A film's image score is a continuous number that moves a little with every
engine change, so the films stay report-only. Its cue channel is not: a cast
either plays the cue retail plays, on the same clock, or it does not, and a
cue that stops playing is a defect even when the picture is unchanged. This
gate holds the cue channel of the committed film sets to a baseline.

The film sets are batch files under `scripts/ci/film-sets/` (the format of
`cast-sequence-batch.py`: a name and the `cast-sequence.py` arguments). Run one
with the batch driver, then check the directory it wrote:

    scripts/ci/cast-sequence-batch.py scripts/ci/film-sets/seru.txt --out captures/films/seru
    scripts/ci/film-cues.py --check captures/films/seru
    scripts/ci/film-cues.py --check captures/films/seru captures/films/enemy --propose /tmp/cues.json
    scripts/ci/film-cues.py --gate scripts/ci/film-sets/gate.txt --out target/film-gate
    scripts/ci/film-cues.py --self-test

A film directory's `retail/` may be a symlink to an existing recording; the
batch driver reuses it. Each film's `report.json` (written by
`legaia-engine cast-sequence-score`) gives the cue score and the matched cues.
A baselined film fails when its score falls below the baseline or it matches
fewer cues. A film measured against another retail recording than the
baseline's (a digest of the recording's cue log tells them apart) is listed
and not scored, since the two are not the same measurement. A baselined film with no report is a
skip, not a pass, and is counted as one.

`--propose PATH` writes a candidate baseline (never the committed one): the
measured films, each at the better of its baseline and its measurement. A
lower value needs a `scripts/ci/baseline-loosenings.toml` entry, which
`check-baseline-loosening.py` enforces. Exit 0 with a `[skip]` line when no
film directory holds a report, matching the repo's disc-gated convention.

`--set FILE` names a batch file whose films are all owed: under it a film of
the set with no report, no baseline row or another recording than the
baseline's fails the check instead of being listed. `--gate FILE --out DIR` is
the full gate's step: it films the set with `cast-sequence-batch.py` (engine
side only, every line of the set names its cached recording with
`--retail-from`) and checks it under `--set`. It prints `[skip]` when none of
the set's recordings is on this machine, and fails when only some are.
See `docs/tooling/cast-sequence-oracle.md`, "The cue ratchet".
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "film-cue-baseline.json"
BATCH = REPO / "scripts" / "ci" / "cast-sequence-batch.py"
PROFILE = "release-test"


def read_set(path: Path) -> dict[str, list[str]]:
    """`{film: cast-sequence.py arguments}` of a batch file."""
    films = {}
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            name, *args = shlex.split(line)
            films[name] = args
    return films


def owed(names: list[str], base: dict[str, dict], films: dict[str, dict]) -> list[str]:
    """What a set that must be measured whole is missing: a film with no
    report, no baseline row, or a recording that is not the baseline's."""
    out = []
    for name in names:
        got, want = films.get(name), base.get(name)
        if got is None:
            out.append(f"{name}: no report (the film did not record or did not score)")
        elif want is None:
            out.append(f"{name}: no baseline row")
        elif not same_recording(want, got):
            out.append(f"{name}: scored against recording {got.get('recording', '?')}, "
                       f"the baseline holds {want.get('recording', '?')}")
    return out


def film_set(path: Path, out: Path) -> int | None:
    """Film a gate set into `out`, engine side only. `None` when no recording
    of the set is on this machine (a skip), 1 when the set cannot be filmed."""
    films = read_set(path)
    missing = []
    for name, args in films.items():
        if "--retail-from" not in args:
            sys.exit(f"{path}: {name} names no recording (--retail-from); a gate set films nothing on the emulator")
        src = Path(args[args.index("--retail-from") + 1])
        src = src if src.is_absolute() else REPO / src
        if not (src / "done.txt").is_file():
            missing.append(f"{name}: {src}")
    if len(missing) == len(films):
        print(f"[skip] none of {path.name}'s {len(films)} recordings is on this machine")
        return None
    for line in missing:
        print(f"  [missing recording] {line}")
    # Without it a film falls back to seats read off the recording's first row
    # and scores as another film, silently.
    reader = REPO / "target" / PROFILE / "pcsxr-state"
    if not reader.is_file():
        print(f"  [missing] {reader} (cargo build -p psx-pcsx-redux --profile {PROFILE} --bin pcsxr-state)")
        return 1
    if missing:
        return 1
    out.mkdir(parents=True, exist_ok=True)
    # A report left by an earlier run would answer for a film that no longer records.
    for name in films:
        (out / name / "report.json").unlink(missing_ok=True)
    env = dict(os.environ)
    env.setdefault("XMODIFIERS", "@im=none")
    rc = subprocess.run([sys.executable, str(BATCH), str(path), "--out", str(out), "--no-build"],
                        cwd=REPO, env=env).returncode
    if rc != 0:
        print(f"  [batch] cast-sequence-batch.py exited {rc}")
    # The engine frames are scored by now and are most of the directory.
    for name in films:
        for frame in (out / name / "engine").glob("e_*.png"):
            frame.unlink()
    return 0


def measure(film: Path) -> dict | None:
    """`{cue, matched, retail_cues, recording}` off one film's report, or None.
    `recording` is a short digest of the retail side's cue log, which names
    the recording the score was measured against without carrying it."""
    report = film / "report.json"
    if not report.is_file():
        return None
    sound = json.loads(report.read_text()).get("sound")
    if not sound or sound.get("score") is None:
        return None
    log = film / "retail" / "sfx.csv"
    digest = hashlib.sha1(log.read_bytes()).hexdigest()[:12] if log.is_file() else ""
    return {
        "cue": round(float(sound["score"]), 4),
        "matched": len(sound.get("matched") or []),
        "retail_cues": int(sound.get("retail_cues") or 0),
        "recording": digest,
    }


def measure_dirs(dirs: list[Path]) -> dict[str, dict]:
    films: dict[str, dict] = {}
    for root in dirs:
        for film in sorted(p for p in root.iterdir() if p.is_dir()):
            got = measure(film)
            if got is not None:
                films[film.name] = got
    return films


def compare(base: dict[str, dict], films: dict[str, dict]) -> tuple[list[str], list[str], list[str]]:
    """`(failures, re-recorded, skipped)` of the measured films against the baseline."""
    failures, rerecorded, skipped = [], [], []
    for name, want in sorted(base.items()):
        got = films.get(name)
        if got is None:
            skipped.append(name)
            continue
        if not same_recording(want, got):
            rerecorded.append(
                f"{name}: recording {want.get('recording', '?')} -> {got.get('recording', '?')}, "
                f"retail cues {want['retail_cues']} -> {got['retail_cues']}"
            )
            continue
        if got["cue"] < want["cue"] or got["matched"] < want["matched"]:
            failures.append(
                f"{name}: cue {want['cue']:.3f} -> {got['cue']:.3f}, "
                f"matched {want['matched']} -> {got['matched']} of {got['retail_cues']}"
            )
    return failures, rerecorded, skipped


def same_recording(want: dict, got: dict) -> bool:
    """Whether a measurement scored the baseline's retail recording: the same
    cue log digest when both carry one, else the same retail cue count."""
    if want.get("recording") and got.get("recording"):
        return want["recording"] == got["recording"]
    return want["retail_cues"] == got["retail_cues"]


def propose(base: dict[str, dict], films: dict[str, dict]) -> dict[str, dict]:
    out = {k: dict(v) for k, v in base.items()}
    for name, got in films.items():
        want = out.get(name)
        if want is None or not same_recording(want, got):
            out[name] = dict(got)
        else:
            want["cue"] = max(want["cue"], got["cue"])
            want["matched"] = max(want["matched"], got["matched"])
    return dict(sorted(out.items()))


def self_test() -> int:
    base = {
        "a": {"cue": 1.0, "matched": 5, "retail_cues": 5},
        "b": {"cue": 0.8, "matched": 4, "retail_cues": 5},
        "c": {"cue": 1.0, "matched": 2, "retail_cues": 2, "recording": "a"},
        "d": {"cue": 1.0, "matched": 1, "retail_cues": 1},
    }
    films = {
        "a": {"cue": 0.889, "matched": 4, "retail_cues": 5},
        "b": {"cue": 0.9, "matched": 4, "retail_cues": 5},
        "c": {"cue": 1.0, "matched": 3, "retail_cues": 2, "recording": "b"},
        "e": {"cue": 0.5, "matched": 1, "retail_cues": 2},
    }
    failures, rerecorded, skipped = compare(base, films)
    bad = 0
    if [f.split(":")[0] for f in failures] != ["a"]:
        print(f"self-test: failures {failures}")
        bad += 1
    if [r.split(":")[0] for r in rerecorded] != ["c"] or skipped != ["d"]:
        print(f"self-test: rerecorded {rerecorded} skipped {skipped}")
        bad += 1
    p = propose(base, films)
    if p["a"]["cue"] != 1.0 or p["b"]["cue"] != 0.9 or p["c"]["matched"] != 3 or "e" not in p:
        print(f"self-test: propose {p}")
        bad += 1
    unmet = owed(["a", "c", "d", "e"], base, films)
    if [u.split(":")[0] for u in unmet] != ["c", "d", "e"]:
        print(f"self-test: owed {unmet}")
        bad += 1
    print("self-test ok" if not bad else f"self-test: {bad} failures")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dirs", nargs="*", type=Path, help="directories of film directories")
    ap.add_argument("--check", action="store_true", help="fail below the baseline")
    ap.add_argument("--propose", metavar="PATH", help="write a candidate baseline (never the committed one)")
    ap.add_argument("--set", action="append", default=[], type=Path, metavar="FILE",
                    help="a batch file whose films must all be measured, baselined and on the baseline's recording")
    ap.add_argument("--gate", type=Path, metavar="FILE",
                    help="film this batch file into --out (engine side only), then --check it under --set")
    ap.add_argument("--out", type=Path, help="with --gate: where the films go")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    if a.gate:
        if not a.out:
            sys.exit("--gate needs --out")
        filmed = film_set(a.gate, a.out)
        if filmed is None:
            return 0
        if filmed:
            return filmed
        a.check, a.set, a.dirs = True, [*a.set, a.gate], [*a.dirs, a.out]
    if a.propose and Path(a.propose).resolve() == BASELINE.resolve():
        sys.exit("--propose writes a candidate; the committed baseline changes by hand, with a ledger entry if it falls")
    dirs = [d for d in a.dirs if d.is_dir()]
    films = measure_dirs(dirs)
    if not films and not a.gate:
        print("[skip] no film reports under the given directories")
        return 0
    base = json.loads(BASELINE.read_text())["films"] if BASELINE.exists() else {}
    wanted = [name for path in a.set for name in read_set(path)]
    if wanted:
        # A set names what is owed: nothing else in the baseline is a skip.
        base = {k: v for k, v in base.items() if k in wanted or k in films}
    unmet = owed(wanted, base, films)
    failures, rerecorded, skipped = compare(base, films)
    unbaselined = sorted(set(films) - set(base))
    checked = len(base) - len(skipped) - len(rerecorded)
    print(f"[film-cues] {checked} baselined films checked, {len(failures)} below baseline; "
          f"{len(skipped)} baselined films not measured (skip, not pass); "
          f"{len(rerecorded)} re-recorded; {len(unbaselined)} measured films not baselined")
    for line in rerecorded:
        print(f"  [re-recorded] {line}")
    for line in failures:
        print(f"  [below] {line}")
    for line in unmet:
        print(f"  [owed] {line}")
    if a.propose:
        Path(a.propose).write_text(json.dumps({"films": propose(base, films)}, indent=2) + "\n")
        print(f"[propose] {a.propose}")
    return 1 if (a.check and (failures or unmet)) else 0


if __name__ == "__main__":
    sys.exit(main())
