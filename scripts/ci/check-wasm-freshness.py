#!/usr/bin/env python3
"""Is your local `site/wasm/` bundle built from the sources in this tree?

`site/wasm/` is build output and is not committed (see `site/wasm/.gitignore`).
But a locally built bundle still goes stale the moment you pull, switch branches
or edit a source, and nothing about a green test run tells you: the browser loads
the binary you built earlier, not the code you are looking at now. That mistake
has been made twice on this repo, both times reporting a fix as live to someone
whose bundle predated it.

What makes it hard to notice by eye is the size of the bundle's source closure.
`legaia-web-viewer` pulls in most of the workspace transitively - the format
crates, `engine-core`, `engine-ui`, `engine-audio` - so editing a PROT reader or
an audio kernel silently changes what the play page does. `git log`-style "was
this file touched after the bundle?" reasoning also gets it wrong, because a
branch can *build* the bundle and then be rebased onto a different tree: the
last-touched timestamps look in sync while the binary compiled against sources
that are no longer here.

So the stamp is content-addressed, not time-addressed. `build-wasm.sh` records a
hash of every source input, and this script recomputes it:

    python3 scripts/ci/check-wasm-freshness.py            # warn (exit 0)
    python3 scripts/ci/check-wasm-freshness.py --strict    # fail (exit 1)
    python3 scripts/ci/check-wasm-freshness.py --write     # stamp (build-wasm.sh)

Run it before believing anything you see on a locally served play page, and
especially before telling someone a play-page fix is live. `--strict` is the form
for that; the default warns so it can be called from scripts without aborting
them.

This is not a repository gate and is not wired into any hook - there is no
committed artifact left to gate, and the stamp it reads is itself untracked
local state. It answers a question about your working copy.

Hashing only tracked files keeps the answer reproducible across clones: build
output, editor scratch and `target/` never enter the stamp.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SITE_WASM = REPO / "site" / "wasm"
STAMP = SITE_WASM / "SOURCE_STAMP.json"
ROOT_CRATE = "legaia-web-viewer"

# The artifacts a served page actually loads. Checked for existence, because the
# stamp is not the bundle: with the bundle deleted and the stamp left behind, a
# stamp-only comparison reports "fresh" about a page that cannot load at all.
# `site/wasm/` is gitignored, so ordinary `git clean -xdf` removes these while
# leaving nothing in a diff to say so.
BUNDLE_ARTIFACTS = [
    "legaia_web_viewer_bg.wasm",
    "legaia_web_viewer.js",
]

# Inputs outside the crate closure that still change the bundle's behaviour.
EXTRA_INPUTS = ["Cargo.lock", "scripts/ci/build-wasm.sh"]

# Stamp format version. Bump when the hashed set changes in a way that should
# invalidate every existing stamp rather than read as a source edit.
STAMP_VERSION = 1


def run(cmd: list[str]) -> str:
    out = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"[check-wasm-freshness] `{' '.join(cmd)}` failed:\n{out.stderr}")
    return out.stdout


def source_closure() -> list[str]:
    """Workspace crate dirs the bundle compiles, from cargo's own resolve graph.

    Derived rather than listed so a new dependency edge cannot quietly fall
    outside the stamp - the failure mode would be a gate that reports fresh
    while the bundle is stale, which is worse than no gate.
    """
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1"]))
    members = set(meta["workspace_members"])
    by_id = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

    roots = [i for i in members if by_id[i]["name"] == ROOT_CRATE]
    if not roots:
        sys.exit(f"[check-wasm-freshness] {ROOT_CRATE} is not a workspace member")

    seen: set[str] = set()

    def walk(pkg_id: str) -> None:
        if pkg_id in seen:
            return
        seen.add(pkg_id)
        for dep in nodes[pkg_id]["deps"]:
            if dep["pkg"] in members:
                walk(dep["pkg"])

    walk(roots[0])

    dirs = []
    for pkg_id in seen:
        manifest = Path(by_id[pkg_id]["manifest_path"])
        dirs.append(str(manifest.parent.relative_to(REPO)))
    return sorted(dirs)


def tracked_files(paths: list[str]) -> list[str]:
    """Tracked files under `paths`, excluding what cannot affect the build."""
    listing = run(["git", "ls-files", "-z", "--", *paths])
    out = []
    for name in listing.split("\0"):
        if not name:
            continue
        # A crate's own README/tests do not compile into the bundle. Tests are
        # excluded deliberately: retargeting a test must not read as a stale
        # bundle, or the gate cries wolf on test-only commits.
        if name.endswith(".md") or "/tests/" in name or "/benches/" in name:
            continue
        out.append(name)
    return sorted(out)


def stamp_for(files: list[str]) -> str:
    """Hash of (path, content) over every input, order-independent by sorting."""
    digest = hashlib.sha256()
    digest.update(f"v{STAMP_VERSION}\n".encode())
    for name in files:
        blob = (REPO / name).read_bytes()
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(blob).hexdigest().encode())
        digest.update(b"\n")
    return digest.hexdigest()


def compute() -> tuple[str, list[str]]:
    inputs = tracked_files(source_closure() + EXTRA_INPUTS)
    return stamp_for(inputs), inputs


def per_file_hashes(files: list[str]) -> dict[str, str]:
    return {
        n: hashlib.sha256((REPO / n).read_bytes()).hexdigest()[:16] for n in files
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--write",
        action="store_true",
        help="record the current stamp (build-wasm.sh calls this)",
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 when the bundle is stale, instead of warning",
    )
    ap.add_argument("--quiet", action="store_true", help="only print on trouble")
    args = ap.parse_args()

    current, inputs = compute()

    if args.write:
        STAMP.parent.mkdir(parents=True, exist_ok=True)
        STAMP.write_text(
            json.dumps(
                {
                    "comment": (
                        "Content hash of the sources site/wasm/ was built from. "
                        "Written by scripts/ci/build-wasm.sh; read by "
                        "scripts/ci/check-wasm-freshness.py. Untracked local "
                        "state - do not commit it and do not hand-edit it; "
                        "editing it to silence the check is how you end up "
                        "testing a bundle you did not build."
                    ),
                    "stamp_version": STAMP_VERSION,
                    "source_stamp": current,
                    "input_count": len(inputs),
                    "files": per_file_hashes(inputs),
                },
                indent=2,
                sort_keys=True,
            )
            + "\n"
        )
        print(f"[check-wasm-freshness] stamped {len(inputs)} inputs -> {current[:16]}")
        return 0

    # Existence before freshness: "your bundle is out of date" is the wrong
    # answer when there is no bundle, and "fresh" would be actively false.
    missing = [n for n in BUNDLE_ARTIFACTS if not (SITE_WASM / n).exists()]
    if missing:
        print(
            "[check-wasm-freshness] NO BUNDLE: site/wasm/ is missing "
            + ", ".join(missing)
            + ".\n    Nothing can serve the play page. Run "
            "scripts/ci/build-wasm.sh.",
            file=sys.stderr,
        )
        if STAMP.exists():
            print(
                "    (A SOURCE_STAMP.json is present without the artifacts it "
                "describes - stale leftover, not a built bundle.)",
                file=sys.stderr,
            )
        return 1 if args.strict else 0

    if not STAMP.exists():
        print(
            "[check-wasm-freshness] WARN: no site/wasm/SOURCE_STAMP.json - cannot "
            "tell whether the shipped bundle matches this tree. Run "
            "scripts/ci/build-wasm.sh to create one.",
            file=sys.stderr,
        )
        return 1 if args.strict else 0

    recorded = json.loads(STAMP.read_text())

    if recorded.get("stamp_version") != STAMP_VERSION:
        print(
            f"[check-wasm-freshness] WARN: stamp is format v"
            f"{recorded.get('stamp_version')}, this checker is v{STAMP_VERSION}. "
            "Rebuild to re-stamp.",
            file=sys.stderr,
        )
        return 1 if args.strict else 0

    if recorded.get("source_stamp") == current:
        if not args.quiet:
            print(f"[check-wasm-freshness] OK - site/wasm/ matches {len(inputs)} sources")
        return 0

    # Name the drifted files. "The bundle is stale" is not actionable on its own;
    # which source moved tells you whether it is web-visible.
    was = recorded.get("files", {})
    now = per_file_hashes(inputs)
    changed = sorted(k for k in now if was.get(k) != now[k])
    dropped = sorted(k for k in was if k not in now)

    print(
        "[check-wasm-freshness] STALE: site/wasm/ was built from different "
        "sources than this tree.",
        file=sys.stderr,
    )
    for name in changed[:20]:
        kind = "added" if name not in was else "changed"
        print(f"    {kind:8s} {name}", file=sys.stderr)
    if len(changed) > 20:
        print(f"    ... and {len(changed) - 20} more changed", file=sys.stderr)
    for name in dropped[:10]:
        print(f"    removed  {name}", file=sys.stderr)
    if len(dropped) > 10:
        print(f"    ... and {len(dropped) - 10} more removed", file=sys.stderr)
    print(
        "\n    A locally served play page loads the bundle you built, not these "
        "sources. If any change above is web-visible, run "
        "scripts/ci/build-wasm.sh before claiming it is fixed.",
        file=sys.stderr,
    )
    return 1 if args.strict else 0


if __name__ == "__main__":
    sys.exit(main())
