#!/usr/bin/env python3
"""Run the retail comparison corpus and write the human report.

Thin driver over `legaia-engine retail-compare`
(`crates/parity/src/retail_compare*.rs`): resolves the gitignored data
(the save library and the extracted disc) - which in a git worktree live in
the main checkout, not beside the worktree - builds the engine binary unless
told not to, and runs the corpus with the report under a gitignored
directory. See `docs/tooling/retail-compare.md`.

    scripts/ci/retail-compare.py                 # state channels only
    scripts/ci/retail-compare.py --images        # + frames (needs a display)
    scripts/ci/retail-compare.py --check         # assert the committed ratchet
    scripts/ci/retail-compare.py --bless         # update the baseline (merges: unmeasured states/channels keep their values)
    scripts/ci/retail-compare.py --smoke clean   # smoke tier: clean 3x frames, broken-frame check, no ratchet
    scripts/ci/retail-compare.py --jobs 1        # one state at a time (default: several, same report)
    scripts/ci/retail-compare.py --fresh         # measure every state, ignoring the result cache
    scripts/ci/retail-compare.py --check-split   # disc-free: the baseline's retail / patched-disc lists match the manifest

The baseline keeps two lists. `states` holds the states made on a retail disc
and is the ratchet `--check` asserts; `patched_states` holds the ones the
manifest tags with a `resident_patch` (mod states: Delilas party swap,
shiny-Seru, the location rename, the seed-code starting bag), which `--check`
reports as `[patched-drift]` and never fails on. `--check-split` needs neither
the disc nor a build, so it runs as a repo gate.

Exit 0 and a `[skip]` line when the library or the extracted disc is missing,
matching the repo's disc-gated convention.
"""

import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "retail-compare-baseline.json"
PROFILE = "release-test"
# Results reused when the binary, disc, state and environment all match
# (`crates/parity/src/retail_compare_cache.rs`). Holds retail pixels: gitignored.
CACHE = REPO / "target" / "retail-compare-cache"


def default_jobs() -> int:
    """Image children mostly wait on the driver: the corpus scales to about a
    dozen on the 20-core box, and half the cores leaves room for other work."""
    return max(1, min(8, (os.cpu_count() or 2) // 2))


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


MANIFEST = REPO / "scripts" / "scenarios.toml"


def split_problems(manifest: dict, baseline: dict) -> tuple[list[str], list[str]]:
    """`(problems, patched_labels)` for the baseline's two lists against the
    manifest. A state belongs under `patched_states` exactly when its scenario
    names a `resident_patch`; every baselined label must name a scenario; and
    the scenarios sharing one backup must agree, since the corpus walks a
    backup once under its first label and the executable is the file's."""
    problems: list[str] = []
    scenarios = {s["label"]: s for s in manifest.get("scenarios", [])}
    by_backup: dict[str, set[bool]] = {}
    for s in manifest.get("scenarios", []):
        fp = s.get("backup_fingerprint")
        if fp:
            by_backup.setdefault(fp, set()).add("resident_patch" in s)
    for fp, kinds in sorted(by_backup.items()):
        if len(kinds) > 1:
            names = [s["label"] for s in manifest["scenarios"] if s.get("backup_fingerprint") == fp]
            problems.append(f"backup {fp[:12]}: scenarios disagree on resident_patch: {', '.join(names)}")
    patched: list[str] = []
    for key, want_patched in (("states", False), ("patched_states", True)):
        for label in sorted(baseline.get(key, {})):
            sc = scenarios.get(label)
            if sc is None:
                problems.append(f"{key}/{label}: no scenario of that label in {MANIFEST.name}")
                continue
            is_patched = "resident_patch" in sc
            if is_patched:
                patched.append(label)
            if is_patched != want_patched:
                where = "patched_states" if is_patched else "states"
                why = f"resident_patch = {sc['resident_patch']!r}" if is_patched else "no resident_patch"
                problems.append(f"{key}/{label}: its scenario has {why}; it belongs under `{where}`")
    return problems, patched


def check_split(verbose: bool) -> int:
    manifest = tomllib.loads(MANIFEST.read_text())
    baseline = json.loads(BASELINE.read_text())
    problems, patched = split_problems(manifest, baseline)
    for p in problems:
        print(f"[split] {p}", file=sys.stderr)
    if verbose:
        for label in patched:
            print(f"  patched-disc: {label}")
    if problems:
        print(f"[split] {len(problems)} problem(s): move each row to the list its "
              f"scenario names (the values move unchanged; nothing is re-measured)",
              file=sys.stderr)
        return 1
    print(f"[split] OK - {len(baseline.get('states', {}))} retail-disc states ratcheted, "
          f"{len(baseline.get('patched_states', {}))} patched-disc states reported apart")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--images", action="store_true", help="score frames too")
    ap.add_argument("--check", action="store_true", help="assert the baseline")
    ap.add_argument("--bless", action="store_true", help="rewrite the baseline")
    ap.add_argument("--flags-first", action="store_true", help="diagnostic seeding order")
    ap.add_argument("--filter", help="only labels containing this (comma = any of several; =label matches exactly)")
    ap.add_argument("--out", default=str(REPO / "captures" / "retail-compare"))
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument(
        "--smoke", choices=["clean", "enhanced"],
        help="smoke tier: render the frames in this mode and fail any that reads as broken "
             "against the same engine's accuracy frame (implies --images; never blesses or checks the ratchet)",
    )
    ap.add_argument("--smoke-reference",
                    help="--smoke: an accuracy run's output from this engine, instead of running one first")
    ap.add_argument("--jobs", type=int, default=default_jobs(),
                    help="states run at once; report.json is the same at any count (default %(default)s)")
    ap.add_argument("--fresh", action="store_true",
                    help="measure every state: read no cached result (--bless, --check and --smoke always do)")
    ap.add_argument("--check-split", action="store_true",
                    help="disc-free: check the baseline's retail / patched-disc lists against the manifest, run nothing")
    ap.add_argument("--verbose", action="store_true", help="--check-split: list the patched-disc states")
    a = ap.parse_args()
    if a.check_split:
        return check_split(a.verbose)
    if a.bless and a.no_build:
        ap.error("--bless always builds: a baseline must come from the tree it is committed with")
    if a.smoke and (a.bless or a.check):
        ap.error("--smoke is the smoke tier: it never blesses or checks the ratchet")
    if a.smoke and a.out == str(REPO / "captures" / "retail-compare"):
        a.out = str(REPO / "captures" / f"retail-compare-smoke-{a.smoke}")

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
    exe = Path(os.environ.get("LEGAIA_ENGINE_BIN") or REPO / "target" / PROFILE / "legaia-engine")
    cmd = [str(exe), "retail-compare", "--out", a.out,
           "--library", str(library), "--extracted-root", str(extracted),
           "--manifest", str(REPO / "scripts" / "scenarios.toml"),
           "--jobs", str(a.jobs), "--progress"]
    if not (a.fresh or a.bless or a.check or a.smoke):
        cmd += ["--cache", str(CACHE)]
    if a.images or a.smoke:
        cmd.append("--images")
    if a.smoke:
        reference = a.smoke_reference
        if reference is None:
            reference = str(Path(a.out) / "accuracy")
            first = list(cmd)
            first[first.index("--out") + 1] = reference
            first += ["--cache", str(CACHE)]
            if a.filter:
                first += ["--filter", a.filter]
            env = dict(os.environ)
            env.setdefault("RUST_LOG", "error")
            code = subprocess.run(first, cwd=REPO, env=env).returncode
            if code != 0:
                return code
        cmd += ["--render", a.smoke, "--smoke-check", str(BASELINE), "--smoke-reference", reference]
    if a.flags_first:
        cmd.append("--flags-first")
    if a.filter:
        cmd += ["--filter", a.filter]
    if a.bless:
        cmd += ["--write-baseline", str(BASELINE)]
    if a.check:
        cmd += ["--check-baseline", str(BASELINE)]
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "error")
    return subprocess.run(cmd, cwd=REPO, env=env).returncode


if __name__ == "__main__":
    sys.exit(main())
