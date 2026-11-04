#!/usr/bin/env bash
# Build the site's WASM bundles and sync them into the static site.
#
# Outputs:
#   - site/wasm/        -- the tool bundle (crates/web-viewer)
#   - site/wasm/play/   -- the play bundle (crates/web-play-bundle), for the pages
#                          scripts/ci/page-bundles.toml names
#   - crates/*/pkg/     -- raw wasm-pack output of each
#
# Requirements: rustup target add wasm32-unknown-unknown; cargo install wasm-pack
#
# Re-run this any time either crate or anything it depends on changes. Neither
# output is committed.
#
# `--fast` builds the iteration bundles instead: the `wasm-fast` profile with no
# wasm-opt pass, so a page check after an edit does not wait for the shipped
# build. They behave as the shipped bundles do, larger and slower; the stamp says
# which they are, and `check-wasm-freshness.py --strict` refuses them. What it
# costs: docs/tooling/build-and-test-speed.md#the-wasm-bundle.
#
# `--only play` or `--only tool` builds one bundle, for an edit loop on one page.
# `check-wasm-freshness.py --strict` refuses the result unless the other bundle is
# fresh under the same profile, so nothing shippable comes out of it by accident.
#
# The deploy job runs this script too: it copies only what a page loads, and the
# copy into site/wasm/play/ is the part a hand-written wasm-pack line would lose.

set -euo pipefail

PROFILE_ARGS=(--release)
STAMP_PROFILE=release
ONLY=""
usage() {
    echo "usage: build-wasm.sh [--fast] [--only play|tool]" >&2
    exit 2
}
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fast)
            PROFILE_ARGS=(--profile wasm-fast --no-opt)
            STAMP_PROFILE=wasm-fast
            shift
            ;;
        --only)
            [[ "${2:-}" == "play" || "${2:-}" == "tool" ]] || usage
            ONLY="$2"
            shift 2
            ;;
        *) usage ;;
    esac
done

REPO="$(cd "$(dirname "$0")/../.." && pwd)"

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "ERROR: wasm-pack not on PATH. Install via 'cargo install wasm-pack'." >&2
    exit 1
fi

# build_bundle <key> <crate dir> <stem> <site dir>
build_bundle() {
    local key="$1" crate_dir="$2" stem="$3" site_dir="$4"
    local pkg_dir="${REPO}/${crate_dir}/pkg"
    echo "[build-wasm] building ${crate_dir} for the web target..."
    (cd "${REPO}/${crate_dir}" && wasm-pack build --target web "${PROFILE_ARGS[@]}")

    if [[ ! -f "${pkg_dir}/${stem}_bg.wasm" ]]; then
        echo "ERROR: wasm-pack did not produce ${pkg_dir}/${stem}_bg.wasm" >&2
        exit 1
    fi

    echo "[build-wasm] syncing ${crate_dir}/pkg into ${site_dir}..."
    mkdir -p "${site_dir}"
    # Copy only the artifacts the site loads (skip README, .gitignore noise).
    cp "${pkg_dir}/${stem}_bg.wasm" "${site_dir}/"
    cp "${pkg_dir}/${stem}_bg.wasm.d.ts" "${site_dir}/"
    cp "${pkg_dir}/${stem}.d.ts" "${site_dir}/"
    cp "${pkg_dir}/${stem}.js" "${site_dir}/"
    cp "${pkg_dir}/package.json" "${site_dir}/"

    # Record what these artifacts were built from. Without this the tree has no
    # way to answer "is the shipped bundle current?" - and answering it from file
    # mtimes or `git log` gets it wrong, because a branch can build the bundle and
    # then be rebased onto different sources. See check-wasm-freshness.py.
    python3 "${REPO}/scripts/ci/check-wasm-freshness.py" --write --bundle "${key}" --profile "${STAMP_PROFILE}"
}

if [[ "${ONLY}" != "play" ]]; then
    build_bundle tool crates/web-viewer legaia_web_viewer "${REPO}/site/wasm"
fi
if [[ "${ONLY}" != "tool" ]]; then
    build_bundle play crates/web-play-bundle legaia_web_play_bundle "${REPO}/site/wasm/play"
fi

# Re-stamp the pages: every page carries window.LEGAIA_WASM_V (the content version
# of its bundle, which the JS loaders append as ?v= to the module + binary URLs).
# Skipping this leaves pages advertising the PREVIOUS version, and a browser that
# cached those bytes under the old key serves them stale on the next rebuild.
python3 "${REPO}/site/_gen.py" >/dev/null

echo "[build-wasm] done. site/wasm/ and site/wasm/play/ are in sync with their crates (pages re-stamped)."
