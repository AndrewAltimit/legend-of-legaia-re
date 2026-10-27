#!/usr/bin/env python3
"""Run the audio state sweep and check it against its ratchet.

Thin driver over `legaia-engine audio-sweep` (`crates/parity/src/audio_sweep.rs`):
reads what libsnd is playing in every retail library state (the sounding
track, its play flag, sequence volume and tempo, the cue voices beside it)
and scores the engine seeded into the same state, plus the battle track rule
against each battle state's own sound set. See `docs/subsystems/audio.md`,
"The audio state sweep".

    scripts/ci/audio-sweep.py                     # run, print the summary
    scripts/ci/audio-sweep.py --check             # fail on any channel below the baseline
    scripts/ci/audio-sweep.py --propose PATH      # write a candidate baseline (never the committed one)

The baseline is `scripts/ci/audio-sweep-baseline.json`: per state, per
channel, the score last accepted. A state or channel the baseline does not
name is reported, not failed. Exit 0 and a `[skip]` line when the library or
the extracted disc is missing, matching the repo's disc-gated convention.
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "audio-sweep-baseline.json"
PROFILE = "release-test"


def main_checkout() -> Path:
    """The main checkout of this repo (the worktree's own root otherwise)."""
    try:
        common = subprocess.run(
            ["git", "-C", str(REPO), "rev-parse", "--git-common-dir"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        return (REPO / common).resolve().parent
    except (subprocess.CalledProcessError, FileNotFoundError):
        return REPO


def resolve(env: str, rel: str, probe) -> Path | None:
    if os.environ.get(env):
        p = Path(os.environ[env])
        return p if probe(p) else None
    for root in (REPO, main_checkout()):
        p = root / rel
        if probe(p):
            return p
    return None


def scores(rows) -> dict:
    return {r["label"]: r["channels"] for r in rows if r["channels"]}


def check(rows, baseline: dict) -> int:
    drops, new = [], []
    have = scores(rows)
    for label, chans in have.items():
        base = baseline.get("states", {}).get(label)
        for ch, v in chans.items():
            if base is None or ch not in base:
                new.append(f"{label}.{ch}={v}")
            elif v < base[ch]:
                detail = next(r for r in rows if r["label"] == label)["detail"].get(ch, "")
                drops.append(f"{label}.{ch}: {base[ch]} -> {v}  {detail}")
    for label, chans in baseline.get("states", {}).items():
        for ch in chans:
            if label in have and ch not in have[label]:
                new.append(f"{label}.{ch} no longer scored")
    for n in new:
        print(f"[new] {n}")
    for d in drops:
        print(f"[drop] {d}")
    print(f"audio-sweep check: {len(drops)} drop(s), {len(new)} unbaselined")
    return 1 if drops else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true", help="assert the baseline")
    ap.add_argument("--propose", help="write a candidate baseline to this path")
    ap.add_argument("--filter", help="only labels containing this (comma = any of several; =label matches exactly)")
    ap.add_argument("--out", default=str(REPO / "captures" / "audio-sweep" / "report.json"))
    ap.add_argument("--no-build", action="store_true")
    a = ap.parse_args()
    if a.propose and Path(a.propose).resolve() == BASELINE.resolve():
        ap.error("--propose never writes the committed baseline")

    library = resolve("LEGAIA_SAVES_LIBRARY", "saves/library", Path.is_dir)
    extracted = resolve(
        "LEGAIA_EXTRACTED_DIR", "extracted",
        lambda p: (p / "PROT.DAT").exists() and (p / "CDNAME.TXT").exists(),
    )
    if library is None or extracted is None:
        print("[skip] save library or extracted disc not found")
        return 0

    if not a.no_build:
        subprocess.run(
            ["cargo", "build", "-p", "legaia-engine-shell", "--profile", PROFILE,
             "--features", "oracles", "--bin", "legaia-engine"],
            cwd=REPO, check=True,
        )
    exe = REPO / "target" / PROFILE / "legaia-engine"
    cmd = [str(exe), "audio-sweep", "--out", a.out,
           "--library", str(library), "--extracted-root", str(extracted),
           "--manifest", str(REPO / "scripts" / "scenarios.toml")]
    if a.filter:
        cmd += ["--filter", a.filter]
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "error")
    rc = subprocess.run(cmd, cwd=REPO, env=env).returncode
    if rc != 0:
        return rc
    rows = json.loads(Path(a.out).read_text())
    if a.propose:
        Path(a.propose).write_text(
            json.dumps({"states": scores(rows)}, indent=2, sort_keys=True) + "\n"
        )
        print(f"[propose] wrote {a.propose}")
    if a.check:
        if not BASELINE.exists():
            print(f"[no baseline] {BASELINE.relative_to(REPO)} is not committed yet")
            return 0
        return check(rows, json.loads(BASELINE.read_text()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
