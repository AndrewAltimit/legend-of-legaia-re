#!/usr/bin/env python3
"""Frame-time and allocation benchmark over a fixed set of scenes, native host.

Runs `play-window` tick-locked (one sim tick per drawn frame, whatever the wall
clock did), so a run walks the same frames every time and only the time varies.
The window needs a display and a disc. Each scene runs `--repeat` times in each
fidelity; the table reports the fastest run, and the spread between runs says
how far to trust it on a shared box.

    scripts/ci/frame-bench.py                         # build, every scene, both fidelities
    scripts/ci/frame-bench.py --scene town01 --mode enhanced
    scripts/ci/frame-bench.py --json out.json         # raw rows, for a before / after diff
    scripts/ci/frame-bench.py --exe path/to/engine    # another build, unbuilt
    scripts/ci/frame-bench.py --entry --base-exe old  # scene + battle entry, paired

The build carries `engine-shell`'s `bench-alloc` feature: the counting allocator
is the only thing it adds. See docs/subsystems/renderer-port.md#frame-profiler.

Exit 0 and a `[skip]` line when there is no disc or no display, matching the
repo's disc-gated convention. It asserts nothing about the numbers: a frame-time
gate on a shared machine can only flake.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PROFILE = "release-test"

# name -> (play-window arguments, environment)
SCENES = {
    # A dense town, walking: the windowed tile crop re-plans as the camera moves.
    "town01": (["--scene", "town01", "--pad-script", "30-250:Down,260-480:Up,490-710:Down,720-940:Up"], {}),
    # A big interior.
    "bio_castle": (["--scene", "jouina"], {}),
    "world_map": (["--scene", "map01", "--world-map"], {}),
    # The fight's idle prompt.
    "battle": (["--scene", "map01", "--battle", "0", "--party", "0,1,2"], {}),
    # A pack map: its entry builds the map's whole mesh set from the toml.
    "pack_map": (["--expansion", str(REPO / "packs" / "sample"), "--map", "isles_beach"], {}),
    # A big summon: the cast runs from the first prompt through the whole span.
    "summon_cast": (
        ["--scene", "map01", "--battle", "0", "--party", "0,1,2"],
        {"LEGAIA_BATTLE_INFLIGHT": "0,138,3"},
    ),
}

MODES = {
    # What a player runs: every enhancement at its default, the window's own size.
    "enhanced": [],
    # The retail-faithful path the corpus scores.
    "retail": ["--retail", "--psx-render"],
}


def run_one(exe, extracted, name, mode, frames, skip, workdir):
    args, env = SCENES[name]
    shot = Path(workdir) / f"{name}.png"
    cmd = [str(exe), "play-window", *MODES[mode], *args,
           "--extracted-root", str(extracted),
           # Tick-locks the window; the run ends on the frame limit long before it.
           "--screenshot", str(shot), "--screenshot-tick", "999999"]
    # The audio director runs (cues, sequencer) with no device under it; the
    # mixer's own cost sits on the device's thread, which is not this table's.
    e = dict(os.environ, LEGAIA_PROFILE="1", LEGAIA_VSYNC="off", LEGAIA_AUDIO_DETACHED="1",
             LEGAIA_PROFILE_FRAMES=str(frames), LEGAIA_PROFILE_SKIP=str(skip),
             **env)
    (Path(workdir) / "legaia-options.toml").write_text("")
    p = subprocess.run(cmd, cwd=workdir, env=e, capture_output=True, text=True, timeout=900)
    m = re.search(r"^\[bench\] (\{.*\})$", p.stderr, re.M)
    if not m:
        tail = "\n".join(p.stderr.splitlines()[-5:])
        raise RuntimeError(f"{name}/{mode}: no [bench] line (status {p.returncode}):\n{tail}")
    row = json.loads(m.group(1))
    # Startup and scene entry by step (`profile::startup`), up to the first frame.
    row["startup"] = {}
    for step, ms in re.findall(r"^\[startup\] (.+?) ([\d.]+) ms \(at", p.stderr, re.M):
        row["startup"][step] = row["startup"].get(step, 0.0) + float(ms)
    return row


def run_entry(exe, extracted, scenes):
    """One `entry-bench` process: {(scene, step): ms}. No window, no GPU."""
    p = subprocess.run([str(exe), "entry-bench", "--extracted-root", str(extracted), "--scenes", scenes],
                       capture_output=True, text=True, timeout=600)
    rows = {}
    for scene, step, ms in re.findall(r"^\[entry\] (\S+) (.+) ([\d.]+)$", p.stdout, re.M):
        rows[(scene, step)] = float(ms)
    if not rows:
        raise RuntimeError(f"entry-bench: no rows (status {p.returncode}):\n{p.stderr[-500:]}")
    return rows


def entry_table(exe, base, extracted, scenes, repeat, out):
    """Scene and battle entry by step, fastest of `repeat` with its spread. With a
    base build the two alternate run by run, so a busy box hits both alike."""
    runs = {"new": []}
    if base:
        runs["base"] = []
    for _ in range(repeat):
        if base:
            runs["base"].append(run_entry(base, extracted, scenes))
        runs["new"].append(run_entry(exe, extracted, scenes))
    fmt = lambda v: f"{min(v):.1f} ({max(v) - min(v):.1f})"
    keys = list(runs["new"][0])
    print("Entry by step, ms: fastest (spread). `scene+` is the second entry, as a door back.")
    print("| scene | step | " + ("base | " if base else "") + "new |")
    print("|---|---|" + ("---|" if base else "") + "---|")
    total = {k: [sum(r.values()) for r in v] for k, v in runs.items()}
    for key in keys:
        cells = [fmt([r[key] for r in runs["new"] if key in r])]
        if base:
            b = [r[key] for r in runs["base"] if key in r]
            cells.insert(0, fmt(b) if b else "")
        print(f"| {key[0]} | {key[1]} | " + " | ".join(cells) + " |")
    print("| all | total | " + (fmt(total["base"]) + " | " if base else "") + fmt(total["new"]) + " |")
    if out:
        Path(out).write_text(json.dumps({k: [{" ".join(kk): vv for kk, vv in r.items()} for r in v]
                                         for k, v in runs.items()}, indent=1))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--scene", action="append", choices=list(SCENES))
    ap.add_argument("--mode", action="append", choices=list(MODES))
    ap.add_argument("--frames", type=int, default=900)
    ap.add_argument("--skip", type=int, default=120, help="frames left out of the averages")
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--exe", help="use this binary instead of building")
    ap.add_argument("--json", help="write the raw rows here")
    ap.add_argument("--entry", action="store_true", help="time scene and battle entry instead of frames")
    ap.add_argument("--entry-scenes", default="town01,jouina,map01,koin1")
    ap.add_argument("--base-exe", help="with --entry: a build to pair against, run by run")
    a = ap.parse_args()

    extracted = REPO / "extracted"
    if a.entry and (extracted / "PROT.DAT").exists():
        exe = Path(a.exe) if a.exe else REPO / "target" / PROFILE / "legaia-engine"
        if not a.exe:
            subprocess.run(["cargo", "build", "-p", "legaia-engine-shell", "--bin", "legaia-engine",
                            "--profile", PROFILE], cwd=REPO, check=True)
        entry_table(exe, a.base_exe, extracted.resolve(), a.entry_scenes, a.repeat, a.json)
        return 0
    if not (extracted / "PROT.DAT").exists() or not os.environ.get("DISPLAY"):
        print("[skip] frame-bench needs extracted/ and a display")
        return 0

    exe = Path(a.exe) if a.exe else REPO / "target" / PROFILE / "legaia-engine"
    if not a.exe:
        subprocess.run(
            ["cargo", "build", "-p", "legaia-engine-shell", "--bin", "legaia-engine",
             "--profile", PROFILE, "--features", "bench-alloc"],
            cwd=REPO, check=True)

    rows = []
    with tempfile.TemporaryDirectory(prefix="frame-bench-") as work:
        for name in a.scene or SCENES:
            for mode in a.mode or MODES:
                runs = [run_one(exe, extracted.resolve(), name, mode, a.frames, a.skip, work)
                        for _ in range(a.repeat)]
                best = min(runs, key=lambda r: r["avg_ms"])
                spread = max(r["avg_ms"] for r in runs) - best["avg_ms"]
                rows.append({"scene": name, "mode": mode, "spread_ms": spread,
                             "runs": [r["avg_ms"] for r in runs], **best})
                print(f"{name}/{mode}: {best['avg_ms']:.2f} ms (spread {spread:.2f}); "
                      f"worst frame {best['worst']}", file=sys.stderr)

    stages = []
    for r in rows:
        for s in r["stages"]:
            if s not in stages:
                stages.append(s)
    print("| scene | mode | avg ms | p99 | max | spread | allocs/frame | KB/frame | draws | "
          + " | ".join(stages) + " |")
    print("|---|---|---|---|---|---|---|---|---|" + "---|" * len(stages))
    for r in rows:
        cells = [f"{r['stages'][s]['ms']:.2f}" if s in r["stages"] else "" for s in stages]
        print(f"| {r['scene']} | {r['mode']} | {r['avg_ms']:.2f} | {r['p99_ms']:.2f} "
              f"| {r['max_ms']:.2f} | {r['spread_ms']:.2f} | {r['allocs_per_frame']:.0f} "
              f"| {r['alloc_bytes_per_frame'] / 1024:.0f} | {r['draws'][0]}+{r['draws'][1]} | "
              + " | ".join(cells) + " |")
    print()
    print("Allocations per frame by stage:")
    print("| scene | mode | " + " | ".join(stages) + " |")
    print("|---|---|" + "---|" * len(stages))
    for r in rows:
        cells = [f"{r['stages'][s]['allocs']:.0f}" if s in r["stages"] else "" for s in stages]
        print(f"| {r['scene']} | {r['mode']} | " + " | ".join(cells) + " |")
    print()
    print("Startup to first frame, by step (ms):")
    steps = []
    for r in rows:
        for s in r["startup"]:
            if s not in steps:
                steps.append(s)
    print("| scene | mode | total | " + " | ".join(steps) + " |")
    print("|---|---|---|" + "---|" * len(steps))
    for r in rows:
        cells = [f"{r['startup'][s]:.0f}" if s in r["startup"] else "" for s in steps]
        print(f"| {r['scene']} | {r['mode']} | {sum(r['startup'].values()):.0f} | "
              + " | ".join(cells) + " |")
    if a.json:
        Path(a.json).write_text(json.dumps(rows, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
