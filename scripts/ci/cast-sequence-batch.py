#!/usr/bin/env python3
"""Run a list of casts through `cast-sequence.py`: retail recordings side by side.

Each line of the batch file is a name and the arguments for one cast:

    # name        cast-sequence.py arguments
    aluru         --spell 0x3F --cast-queue 3F,09,0E,FF --state battle_korb2_aluru_strike --caster 4 --target 0 --monster-pick
    vera          --spell 0x83 --state party_basic_attack_vs_gobu_gobu

    scripts/ci/cast-sequence-batch.py casts.txt --out captures/cast-batch
    scripts/ci/cast-sequence-batch.py casts.txt --out captures/cast-batch --jobs 6 --retail-only

A retail recording is a headless emulator that takes no display (`--jobs` of them
at once, each in `<out>/<name>/`); the engine runs and scores follow one at a time,
since the engine child opens a window. Everything `cast-sequence.py` caches stays
cached, so re-running a batch after an engine change re-does only the engine side.
`--jobs` is a throughput knob, not a score knob: a recording paces itself at 1x
whether it has the machine to itself or not. Exits 1 if any cast failed.
See `docs/tooling/cast-sequence-oracle.md`.
"""

import argparse
import concurrent.futures
import shlex
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DRIVER = REPO / "scripts" / "ci" / "cast-sequence.py"


def read_batch(path: Path) -> list[tuple[str, list[str]]]:
    casts = []
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            name, *args = shlex.split(line)
            casts.append((name, args))
    names = [n for n, _ in casts]
    dup = {n for n in names if names.count(n) > 1}
    if dup:
        sys.exit(f"duplicate names in {path}: {sorted(dup)}")
    return casts


def run(name: str, args: list[str], out: Path, stage: list[str], log_name: str) -> tuple[str, int, float]:
    t0 = time.monotonic()
    cast_dir = out / name
    cast_dir.mkdir(parents=True, exist_ok=True)
    with open(cast_dir / log_name, "w") as log:
        rc = subprocess.run([sys.executable, str(DRIVER), *args, *stage, "--no-build", "--out", str(cast_dir)],
                            cwd=REPO, stdout=log, stderr=subprocess.STDOUT).returncode
    return name, rc, time.monotonic() - t0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("batch", type=Path, help="file of `<name> <cast-sequence.py args>` lines")
    ap.add_argument("--out", type=Path, required=True, help="one directory per cast under here")
    ap.add_argument("--jobs", type=int, default=4, help="retail recordings at once")
    ap.add_argument("--retail-only", action="store_true", help="stop after the retail recordings")
    ap.add_argument("--no-build", action="store_true")
    a = ap.parse_args()
    casts = read_batch(a.batch)
    failed = set()
    t0 = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=a.jobs) as pool:
        for f in concurrent.futures.as_completed(
                [pool.submit(run, n, args, a.out, ["--retail-only"], "retail.log") for n, args in casts]):
            name, rc, secs = f.result()
            print(f"[retail] {name}: {'ok' if rc == 0 else f'FAILED (exit {rc})'}, {secs:.0f}s")
            if rc != 0:
                failed.add(name)
    print(f"[retail] {len(casts)} casts in {time.monotonic() - t0:.0f}s at --jobs {a.jobs}")
    if not a.retail_only:
        if not a.no_build:
            # pcsxr-state reads an older recording's guard stance off its state.
            subprocess.run(["cargo", "build", "-p", "legaia-engine-shell", "-p", "psx-pcsx-redux",
                            "--profile", "release-test", "--features", "legaia-engine-shell/oracles",
                            "--bin", "legaia-engine", "--bin", "pcsxr-state"], cwd=REPO, check=True)
        for name, args in casts:
            if name in failed:
                continue
            _, rc, secs = run(name, args, a.out, [], "engine.log")
            summary = (a.out / name / "engine.log").read_text().strip().splitlines()[-1:] or [""]
            print(f"[engine] {name}: {'ok' if rc == 0 else f'FAILED (exit {rc})'}, {secs:.0f}s: {summary[0][:120]}")
            if rc != 0:
                failed.add(name)
    if failed:
        print(f"failed: {' '.join(sorted(failed))}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
