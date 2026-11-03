#!/usr/bin/env python3
"""A ratchet on what a frame costs in counts: times flake, counts should not.

    scripts/ci/cost-ratchet.py --check                # the gate: measured vs the baseline
    scripts/ci/cost-ratchet.py --measure --runs 10    # the spread between runs, nothing compared
    scripts/ci/cost-ratchet.py --write-baseline       # after a real change, with its ledger entry

What it reads, each over a tick-locked run, so every run walks the same frames:

    bundle    the shipped wasm's gzip size (a `--fast` bundle is not read)
    browser   GL draws and wasm bytes returned per frame (frame-hash-web.mjs --counts)
    native    allocations and bytes allocated per frame, draw counts (play-window's
              [bench] line on a `bench-alloc` binary, as frame-bench.py runs it)

The baseline is scripts/ci/cost-baseline.json. A count over baseline + its tolerance fails;
one that fell far enough says so, so the ratchet can be taken in. Raising the baseline is a
loosening: check-baseline-loosening.py wants a ledger entry naming the reason.

`tolerance` holds a share per kind of count. None of them is covering a wobble: over ten runs
of one tree the spread is nothing or a few hundredths of a percent. It is the room a rise is
not worth a ledger entry for, see docs/tooling/frame-bench.md#cost-ratchet.

Exit 0 and a `[skip]` line with no disc, no display or no bundle, as every disc gate here.
"""

from __future__ import annotations

import argparse
import gzip
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts/ci/cost-baseline.json"
PROFILE = "release-test"
NATIVE_SCENES = ("town01", "world_map", "battle", "summon_cast")
NATIVE_MODES = ("retail", "enhanced")
NATIVE_FRAMES = 420
NATIVE_SKIP = 120


def frame_bench():
    spec = importlib.util.spec_from_file_location("frame_bench", REPO / "scripts/ci/frame-bench.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def read_bundle(site: Path) -> tuple[dict[str, float], str]:
    wasm = site / "wasm" / "legaia_web_viewer_bg.wasm"
    stamp = site / "wasm" / "SOURCE_STAMP.json"
    if not wasm.exists() or not stamp.exists():
        return {}, f"no bundle under {site}/wasm"
    profile = json.loads(stamp.read_text()).get("profile")
    if profile != "release":
        return {}, f"the bundle under {site}/wasm is a {profile} build, not the shipped one"
    return {"bundle/gzip_bytes": len(gzip.compress(wasm.read_bytes(), 6))}, ""


def read_browser(site: Path) -> tuple[dict[str, float], str]:
    if not os.environ.get("LEGAIA_DISC_BIN"):
        return {}, "LEGAIA_DISC_BIN unset"
    with tempfile.TemporaryDirectory(prefix="cost-ratchet-") as work:
        out = Path(work) / "counts.json"
        # The real GPU's driver is no part of a count, and is the fast way through.
        p = subprocess.run(
            ["node", "scripts/ci/frame-hash-web.mjs", "--site", str(site), "--gl", "egl", "--counts", str(out)],
            cwd=REPO, capture_output=True, text=True, timeout=1800,
        )
        if p.returncode or not out.exists():
            raise RuntimeError(f"frame-hash-web --counts failed ({p.returncode}):\n{p.stdout[-800:]}\n{p.stderr[-800:]}")
        rows = json.loads(out.read_text())
    found: dict[str, float] = {}
    for key, r in rows.items():
        found[f"browser/{key}/gl_draws_per_frame"] = round(r["gl_draws"] / r["frames"], 2)
        found[f"browser/{key}/wasm_bytes_per_frame"] = round(r["wasm_bytes"] / r["frames"])
    return found, ""


def build_exe() -> Path:
    subprocess.run(
        ["cargo", "build", "-p", "legaia-engine-shell", "--bin", "legaia-engine", "--profile", PROFILE,
         "--features", "bench-alloc"], cwd=REPO, check=True,
    )
    return REPO / "target" / PROFILE / "legaia-engine"


def read_native(exe: Path | None) -> tuple[dict[str, float], str]:
    extracted = REPO / "extracted"
    if not (extracted / "PROT.DAT").exists():
        return {}, "no extracted/PROT.DAT"
    if not os.environ.get("DISPLAY"):
        return {}, "no display"
    bench = frame_bench()
    exe = exe or build_exe()
    found: dict[str, float] = {}
    with tempfile.TemporaryDirectory(prefix="cost-ratchet-") as work:
        for scene in NATIVE_SCENES:
            for mode in NATIVE_MODES:
                r = bench.run_one(exe, extracted.resolve(), scene, mode, NATIVE_FRAMES, NATIVE_SKIP, work)
                if not r["allocs_per_frame"]:
                    raise RuntimeError(f"{exe} counts no allocations: not a bench-alloc build")
                found[f"native/{scene}/{mode}/allocs_per_frame"] = round(r["allocs_per_frame"], 1)
                found[f"native/{scene}/{mode}/alloc_kb_per_frame"] = round(r["alloc_bytes_per_frame"] / 1024, 1)
                found[f"native/{scene}/{mode}/draws_per_frame"] = r["draws"][0] + r["draws"][1]
    return found, ""


def measure(a: argparse.Namespace, exe: Path | None) -> tuple[dict[str, float], list[str]]:
    """One run of everything: the counts, and what could not be read."""
    site = Path(a.site).resolve()
    counts: dict[str, float] = {}
    skipped: list[str] = []
    for part, reader in (("bundle", lambda: read_bundle(site)), ("browser", lambda: read_browser(site)),
                         ("native", lambda: read_native(exe))):
        if part in a.skip_part:
            skipped.append(f"{part}: left out by --skip-part")
            continue
        found, why = reader()
        counts.update(found)
        if why:
            skipped.append(f"{part}: {why}")
    return counts, skipped


def check(baseline: dict, counts: dict[str, float]) -> int:
    tol = baseline["tolerance"]
    flat = flatten(baseline["counts"])
    over, under = [], []
    print("| count | baseline | measured | room |")
    print("|---|---:|---:|---:|")
    for key in sorted(flat):
        if key not in counts:
            continue
        was, now = flat[key], counts[key]
        room = tol.get(key.rsplit("/", 1)[-1], 0.0)
        limit = was * (1 + room)
        print(f"| {key} | {was} | {now} | {room * 100:.1f}% |")
        if now > limit:
            over.append(f"{key}: {now} over baseline {was} (+{room * 100:.1f}% room)")
        elif now < was * (1 - max(room, 0.001) * 2):
            under.append(f"{key}: {now}, baseline {was}")
    new = sorted(set(counts) - set(flat))
    missing = sorted(set(flat) - set(counts))
    for k in new:
        print(f"[note] {k} is measured and has no baseline: {counts[k]}")
    if under:
        print("[note] fell past the room, so the ratchet can be taken in (--write-baseline):")
        for u in under:
            print(f"  {u}")
    if over:
        print(f"[cost-ratchet] FAILED: {len(over)} regressions over the baseline")
        for o in over:
            print(f"  regression {o}")
        print("A real rise raises scripts/ci/cost-baseline.json with --write-baseline and a "
              "scripts/ci/baseline-loosenings.toml entry saying why.")
        return 1
    print(f"[cost-ratchet] OK: {len(flat) - len(missing)} counts within the baseline")
    return 0


def flatten(node: dict, prefix: str = "") -> dict[str, float]:
    out: dict[str, float] = {}
    for k, v in node.items():
        key = f"{prefix}/{k}" if prefix else k
        if isinstance(v, dict):
            out.update(flatten(v, key))
        else:
            out[key] = v
    return out


def nest(flat: dict[str, float]) -> dict:
    out: dict = {}
    for key, v in sorted(flat.items()):
        node = out
        parts = key.split("/")
        for p in parts[:-1]:
            node = node.setdefault(p, {})
        node[parts[-1]] = v
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    what = ap.add_mutually_exclusive_group(required=True)
    what.add_argument("--check", action="store_true", help="fail on a count over the baseline")
    what.add_argument("--measure", action="store_true", help="print the counts and their spread")
    what.add_argument("--write-baseline", action="store_true", help="rewrite the counts in the baseline")
    ap.add_argument("--runs", type=int, default=1)
    ap.add_argument("--site", default=str(REPO / "site"))
    ap.add_argument("--exe", help="a bench-alloc legaia-engine, instead of building one")
    ap.add_argument("--json", help="with --measure: every run's counts")
    ap.add_argument("--skip-part", action="append", default=[], choices=["bundle", "browser", "native"])
    a = ap.parse_args()
    exe = Path(a.exe).resolve() if a.exe else None
    if not exe and "native" not in a.skip_part and (REPO / "extracted/PROT.DAT").exists() and os.environ.get("DISPLAY"):
        built = build_exe()
        # A private copy: the next cargo build with other features overwrites the target's.
        scratch = Path(tempfile.mkdtemp(prefix="cost-ratchet-bin-"))
        exe = scratch / "legaia-engine"
        shutil.copy2(built, exe)

    runs = []
    skipped: list[str] = []
    for i in range(a.runs):
        counts, skipped = measure(a, exe)
        runs.append(counts)
        print(f"run {i + 1} of {a.runs}: {len(counts)} counts", file=sys.stderr)
    if a.measure:
        print("| count | min | max | spread | spread % |")
        print("|---|---:|---:|---:|---:|")
        for key in sorted(runs[0]):
            vals = [r[key] for r in runs]
            lo, hi = min(vals), max(vals)
            print(f"| {key} | {lo} | {hi} | {round(hi - lo, 2)} | {((hi - lo) / lo * 100 if lo else 0):.2f} |")
        if a.json:
            Path(a.json).write_text(json.dumps(runs, indent=1))
    for s in skipped:
        print(f"[skip] {s}")
    if a.measure:
        return 0
    if a.write_baseline:
        if skipped:
            print("[cost-ratchet] refusing to write a baseline from a partial measurement")
            return 1
        old = json.loads(BASELINE.read_text()) if BASELINE.exists() else {"tolerance": {}}
        BASELINE.write_text(json.dumps({"tolerance": old["tolerance"], "counts": nest(runs[0])}, indent=1) + "\n")
        print(f"[cost-ratchet] wrote {BASELINE.relative_to(REPO)}; a rise needs its entry: "
              "scripts/ci/check-baseline-loosening.py --propose")
        return 0
    if not BASELINE.exists():
        print("[no baseline] scripts/ci/cost-baseline.json")
        return 1
    return check(json.loads(BASELINE.read_text()), runs[0])


if __name__ == "__main__":
    sys.exit(main())
