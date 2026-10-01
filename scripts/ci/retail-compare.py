#!/usr/bin/env python3
"""Run the retail comparison corpus and write the human report.

Thin driver over `legaia-engine retail-compare`
(`crates/engine-shell/src/retail_compare*.rs`): resolves the gitignored data
(the save library and the extracted disc) - which in a git worktree live in
the main checkout, not beside the worktree - builds the engine binary unless
told not to, and runs the corpus with the report under a gitignored
directory. See `docs/tooling/retail-compare.md`.

    scripts/ci/retail-compare.py                 # state channels only
    scripts/ci/retail-compare.py --images        # + frames (needs a display)
    scripts/ci/retail-compare.py --check         # assert the committed ratchet
    scripts/ci/retail-compare.py --bless         # update the baseline (merges: unmeasured states/channels keep their values)

Exit 0 and a `[skip]` line when the library or the extracted disc is missing,
matching the repo's disc-gated convention.
"""

import argparse
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "retail-compare-baseline.json"
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


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--images", action="store_true", help="score frames too")
    ap.add_argument("--check", action="store_true", help="assert the baseline")
    ap.add_argument("--bless", action="store_true", help="rewrite the baseline")
    ap.add_argument("--flags-first", action="store_true", help="diagnostic seeding order")
    ap.add_argument("--filter", help="only labels containing this (comma = any of several)")
    ap.add_argument("--out", default=str(REPO / "captures" / "retail-compare"))
    ap.add_argument("--no-build", action="store_true")
    a = ap.parse_args()

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
             "--bin", "legaia-engine"],
            cwd=REPO, check=True,
        )
    exe = REPO / "target" / PROFILE / "legaia-engine"
    cmd = [str(exe), "retail-compare", "--out", a.out,
           "--library", str(library), "--extracted-root", str(extracted),
           "--manifest", str(REPO / "scripts" / "scenarios.toml")]
    if a.images:
        cmd.append("--images")
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
