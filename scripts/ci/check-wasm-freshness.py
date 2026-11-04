#!/usr/bin/env python3
"""Are your local `site/wasm/` bundles built from the sources in this tree?

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
    python3 scripts/ci/check-wasm-freshness.py --strict --any-profile   # a --fast bundle counts
    python3 scripts/ci/check-wasm-freshness.py --strict --bundle play   # one bundle
    python3 scripts/ci/check-wasm-freshness.py --write --bundle tool    # stamp (build-wasm.sh)

There are two bundles, each with its own stamp and its own source closure: the
tool bundle in `site/wasm/` (`legaia-web-viewer`) and the play bundle in
`site/wasm/play/` (`legaia-web-play-bundle`). The tool bundle links the play host,
so an edit in `web-play` stales both; an edit in `web-viewer` stales only the tool
bundle, and the play bundle's stamp stays good. Asked about both, `--strict` also
refuses two bundles built under different profiles, even with `--any-profile`: a
site with one iteration bundle in it is not any build anyone meant to serve, and
the page you check is on one of them while the page you did not check is on the
other.

Run it before believing anything you see on a locally served play page, and
especially before telling someone a play-page fix is live. `--strict` is the form
for that; the default warns so it can be called from scripts without aborting
them.

This is not a blocking gate: it runs advisory (`|| true`) from the pre-commit
hook and from CI, because there is no committed artifact to gate and the stamp
it reads is itself untracked local state. It answers a question about your
working copy, and answering it wrong is a warning rather than a refusal.

The hashed set is every file the index knows about **plus every untracked file
`.gitignore` does not exclude**, which is narrower than "everything on disk"
and wider than "everything committed". Build output, editor scratch and
`target/` stay out because they are ignored; a brand-new module that has not
been `git add`ed stays in, because the compiler reads it either way.

That second half was missing and it is the whole defect this paragraph now
records: the enumeration was a plain `git ls-files`, so a branch that added
three new modules to `web-viewer` and built the bundle from them got a stamp
identical to the one before the modules existed. Two different bundles, one
hash, and the gate's answer was "in sync" - the same false green that
mtime-based reasoning gives, arrived at from the other direction. A stamp that
cannot see a file the build compiles is worse than no stamp, because it is
believed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


class Bundle:
    def __init__(self, key: str, root_crate: str, rel_dir: str, stem: str) -> None:
        self.key = key
        self.root_crate = root_crate
        self.rel_dir = rel_dir
        self.dir = REPO / rel_dir
        self.stamp = self.dir / "SOURCE_STAMP.json"
        # The artifacts a served page actually loads. Checked for existence,
        # because the stamp is not the bundle: with the bundle deleted and the
        # stamp left behind, a stamp-only comparison reports "fresh" about a page
        # that cannot load at all. `site/wasm/` is gitignored, so ordinary
        # `git clean -xdf` removes these while leaving nothing in a diff to say so.
        self.artifacts = [f"{stem}_bg.wasm", f"{stem}.js"]


BUNDLES = [
    Bundle("tool", "legaia-web-viewer", "site/wasm", "legaia_web_viewer"),
    Bundle("play", "legaia-web-play-bundle", "site/wasm/play", "legaia_web_play_bundle"),
]

# Inputs outside the crate closure that still change the bundle's behaviour.
# The root manifest holds the profiles the bundle is built under.
EXTRA_INPUTS = ["Cargo.lock", "Cargo.toml", "scripts/ci/build-wasm.sh"]

# Stamp format version. Bump when the hashed set changes in a way that should
# invalidate every existing stamp rather than read as a source edit. Version 2
# added un-ignored untracked files to the set: every version-1 stamp was
# computed over a set that could not see a new module, so they are all
# unreliable rather than merely old. Version 3 added the root manifest, whose
# profiles decide what the bundle is, and the `profile` field. Version 4 is the
# split into two bundles with a closure each.
STAMP_VERSION = 4


def run(cmd: list[str]) -> str:
    out = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"[check-wasm-freshness] `{' '.join(cmd)}` failed:\n{out.stderr}")
    return out.stdout


def source_closure(root_crate: str) -> list[str]:
    """Workspace crate dirs a bundle compiles, from cargo's own resolve graph.

    Derived rather than listed so a new dependency edge cannot quietly fall
    outside the stamp - the failure mode would be a gate that reports fresh
    while the bundle is stale, which is worse than no gate.
    """
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1"]))
    members = set(meta["workspace_members"])
    by_id = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

    roots = [i for i in members if by_id[i]["name"] == root_crate]
    if not roots:
        sys.exit(f"[check-wasm-freshness] {root_crate} is not a workspace member")

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
    """Build inputs under `paths`: indexed files plus un-ignored untracked ones.

    `--others --exclude-standard` is what makes a not-yet-added module count.
    It is the difference between hashing what the repository has recorded and
    hashing what the compiler will read, and only the second is a statement
    about the bundle.
    """
    listing = run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", *paths]
    )
    out = set()
    for name in listing.split("\0"):
        if not name:
            continue
        # A crate's own README/tests do not compile into the bundle. Tests are
        # excluded deliberately: retargeting a test must not read as a stale
        # bundle, or the gate cries wolf on test-only commits.
        if name.endswith(".md") or "/tests/" in name or "/benches/" in name:
            continue
        out.add(name)
    return sorted(out)


def stamp_for(files: list[str]) -> str:
    """Hash of (path, content) over every input, order-independent by sorting."""
    digest = hashlib.sha256()
    digest.update(f"v{STAMP_VERSION}\n".encode())
    for name in files:
        path = REPO / name
        # A staged-then-deleted file is still in the index, and its absence is
        # a source change like any other - hashing a sentinel keeps the stamp
        # defined instead of crashing the gate mid-comparison.
        blob = path.read_bytes() if path.is_file() else b"\0<missing>"
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(blob).hexdigest().encode())
        digest.update(b"\n")
    return digest.hexdigest()


def compute(bundle: Bundle) -> tuple[str, list[str]]:
    inputs = tracked_files(source_closure(bundle.root_crate) + EXTRA_INPUTS)
    return stamp_for(inputs), inputs


def per_file_hashes(files: list[str]) -> dict[str, str]:
    return {
        n: hashlib.sha256((REPO / n).read_bytes()).hexdigest()[:16] for n in files
    }


def write_stamp(bundle: Bundle, profile: str) -> int:
    current, inputs = compute(bundle)
    bundle.dir.mkdir(parents=True, exist_ok=True)
    bundle.stamp.write_text(
        json.dumps(
            {
                "comment": (
                    f"Content hash of the sources {bundle.rel_dir}/ was built from. "
                    "Written by scripts/ci/build-wasm.sh; read by "
                    "scripts/ci/check-wasm-freshness.py. Untracked local "
                    "state - do not commit it and do not hand-edit it; "
                    "editing it to silence the check is how you end up "
                    "testing a bundle you did not build."
                ),
                "stamp_version": STAMP_VERSION,
                "source_stamp": current,
                "profile": profile,
                "input_count": len(inputs),
                "files": per_file_hashes(inputs),
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    print(f"[check-wasm-freshness] {bundle.key}: stamped {len(inputs)} inputs -> {current[:16]}")
    return 0


def check_bundle(bundle: Bundle, args: argparse.Namespace) -> tuple[bool, str | None]:
    """`(fresh, profile)`. `fresh` is False for anything `--strict` refuses."""
    name = f"{bundle.key} bundle, {bundle.rel_dir}/"
    current, inputs = compute(bundle)

    # Existence before freshness: "your bundle is out of date" is the wrong
    # answer when there is no bundle, and "fresh" would be actively false.
    missing = [n for n in bundle.artifacts if not (bundle.dir / n).exists()]
    if missing:
        print(
            f"[check-wasm-freshness] NO BUNDLE: {bundle.rel_dir}/ is missing "
            + ", ".join(missing)
            + f".\n    Nothing can serve the pages on the {bundle.key} bundle. Run "
            "scripts/ci/build-wasm.sh.",
            file=sys.stderr,
        )
        if bundle.stamp.exists():
            print(
                "    (A SOURCE_STAMP.json is present without the artifacts it "
                "describes - stale leftover, not a built bundle.)",
                file=sys.stderr,
            )
        return False, None

    if not bundle.stamp.exists():
        print(
            f"[check-wasm-freshness] WARN: no {bundle.rel_dir}/SOURCE_STAMP.json - cannot "
            "tell whether the shipped bundle matches this tree. Run "
            "scripts/ci/build-wasm.sh to create one.",
            file=sys.stderr,
        )
        return False, None

    recorded = json.loads(bundle.stamp.read_text())

    if recorded.get("stamp_version") != STAMP_VERSION:
        print(
            f"[check-wasm-freshness] WARN: the {bundle.key} stamp is format v"
            f"{recorded.get('stamp_version')}, this checker is v{STAMP_VERSION}. "
            "Rebuild to re-stamp.",
            file=sys.stderr,
        )
        return False, None

    built = recorded.get("profile", "release")
    if recorded.get("source_stamp") == current:
        # The sources match, so a page shows this tree's behaviour; an iteration
        # build still is not the bundle that ships, and its size and speed say
        # nothing about that one.
        if built != "release":
            print(
                f"[check-wasm-freshness] {name} matches {len(inputs)} sources, "
                f"built under `{built}`, not the shipped profile. Run "
                "scripts/ci/build-wasm.sh for the shipped bundle.",
                file=sys.stderr,
            )
            return args.any_profile, built
        if not args.quiet:
            print(f"[check-wasm-freshness] OK - {name} matches {len(inputs)} sources")
        return True, built

    # Name the drifted files. "The bundle is stale" is not actionable on its own;
    # which source moved tells you whether it is web-visible.
    was = recorded.get("files", {})
    now = per_file_hashes(inputs)
    changed = sorted(k for k in now if was.get(k) != now[k])
    dropped = sorted(k for k in was if k not in now)

    print(
        f"[check-wasm-freshness] STALE: {name} was built from different "
        "sources than this tree.",
        file=sys.stderr,
    )
    for f in changed[:20]:
        kind = "added" if f not in was else "changed"
        print(f"    {kind:8s} {f}", file=sys.stderr)
    if len(changed) > 20:
        print(f"    ... and {len(changed) - 20} more changed", file=sys.stderr)
    for f in dropped[:10]:
        print(f"    removed  {f}", file=sys.stderr)
    if len(dropped) > 10:
        print(f"    ... and {len(dropped) - 10} more removed", file=sys.stderr)
    print(
        "\n    A locally served page loads the bundle you built, not these "
        "sources. If any change above is web-visible, run "
        "scripts/ci/build-wasm.sh before claiming it is fixed.",
        file=sys.stderr,
    )
    return False, built


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--write",
        action="store_true",
        help="record the current stamp of --bundle (build-wasm.sh calls this)",
    )
    ap.add_argument(
        "--bundle",
        choices=[b.key for b in BUNDLES],
        help="one bundle; the default is both (--write needs it)",
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 when a bundle is stale, instead of warning",
    )
    ap.add_argument("--quiet", action="store_true", help="only print on trouble")
    ap.add_argument(
        "--profile",
        default="release",
        help="with --write: the cargo profile the bundle was built under",
    )
    ap.add_argument(
        "--any-profile",
        action="store_true",
        help="with --strict: accept iteration bundles (build-wasm.sh --fast) too, "
        "as long as both are iteration bundles",
    )
    args = ap.parse_args()

    chosen = [b for b in BUNDLES if args.bundle in (None, b.key)]
    if args.write:
        if not args.bundle:
            ap.error("--write needs --bundle")
        return write_stamp(chosen[0], args.profile)

    results = [check_bundle(b, args) for b in chosen]
    ok = all(fresh for fresh, _ in results)

    profiles = {profile for _, profile in results if profile}
    if len(results) > 1 and len(profiles) > 1:
        print(
            f"[check-wasm-freshness] MIXED PROFILES: the bundles were built under "
            f"{' and '.join(sorted(profiles))}. Run scripts/ci/build-wasm.sh, with or "
            "without --fast, and without --only.",
            file=sys.stderr,
        )
        ok = False

    return 1 if args.strict and not ok else 0


if __name__ == "__main__":
    sys.exit(main())
