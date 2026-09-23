#!/usr/bin/env bash
# Point CARGO_TARGET_DIR at a per-toolchain subdirectory of the persistent CI
# target cache, and prune every other toolchain's subdirectory.
#
# main-ci.yml keeps one target directory outside the checkout so cargo's
# fingerprints survive `git clean -ffdx` between runs. That cache outlives the
# toolchain: when `stable` moved on the runner, a run under the new rustc was
# handed rlibs the old one had compiled, and the doctest pass died on
#
#     error[E0514]: found crate `anyhow` compiled by an incompatible version
#     of rustc
#
# with every rlib in `release-test/deps/` stale. Keying the directory on the
# exact compiler (`rustc -vV` commit hash) makes a toolchain bump start from a
# cold directory instead of a poisoned warm one, and pruning the other keys
# keeps the cache from growing by a whole workspace build per rustc release.
#
# The rustc and rustdoc versions are also compared up front: a doctest is the
# one step that runs rustdoc against rustc's rlibs, so a split toolchain on
# PATH fails here with both versions named, not later as the same E0514.
#
# Usage (a workflow step, with CARGO_TARGET_DIR set to the cache root):
#     scripts/ci/select-ci-target-dir.sh
#
# Appends the chosen CARGO_TARGET_DIR to $GITHUB_ENV so every later step of
# the job builds into it. Outside Actions it only prints the directory.

set -euo pipefail

BASE="${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must name the persistent cache root}"
BASE="${BASE%/}"

# Guard the prune below: it deletes siblings under BASE, so BASE must be the
# dedicated cache and never a checkout's own target/ or a home directory.
case "$(basename "$BASE")" in
    legaia-ci-target) ;;
    *)
        echo "select-ci-target-dir: refusing to manage '$BASE'" \
             "(expected a directory named legaia-ci-target)" >&2
        exit 1
        ;;
esac

RUSTC_VV="$(rustc -vV)"
RUSTC_V="$(rustc -V)"
RUSTDOC_V="$(rustdoc -V)"
echo "$RUSTC_VV"
echo "$RUSTDOC_V"

# `rustc 1.98.1 (48a229cea 2026-09-01)` vs `rustdoc 1.98.1 (48a229cea ...)`.
if [[ "${RUSTC_V#rustc }" != "${RUSTDOC_V#rustdoc }" ]]; then
    echo "select-ci-target-dir: rustc and rustdoc disagree -" \
         "doctests would load rlibs from the wrong compiler" >&2
    exit 1
fi

HASH="$(awk '/^commit-hash:/ {print $2}' <<<"$RUSTC_VV")"
HOST="$(awk '/^host:/ {print $2}' <<<"$RUSTC_VV")"
if [[ -z "$HASH" || "$HASH" == "unknown" ]]; then
    # A locally built compiler reports no hash; the release line still
    # distinguishes toolchains.
    HASH="$(awk '/^release:/ {print $2}' <<<"$RUSTC_VV")"
fi
KEY="rustc-${HASH:0:12}-${HOST}"
DIR="$BASE/$KEY"

mkdir -p "$DIR"
shopt -s dotglob nullglob
for entry in "$BASE"/*; do
    if [[ "$entry" != "$DIR" ]]; then
        echo "select-ci-target-dir: pruning stale $(basename "$entry")"
        rm -rf -- "$entry"
    fi
done

echo "select-ci-target-dir: CARGO_TARGET_DIR=$DIR"
if [[ -n "${GITHUB_ENV:-}" ]]; then
    echo "CARGO_TARGET_DIR=$DIR" >>"$GITHUB_ENV"
fi
