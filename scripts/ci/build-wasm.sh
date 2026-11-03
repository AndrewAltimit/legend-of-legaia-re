#!/usr/bin/env bash
# Build the WASM viewer and sync the generated package into the static site.
#
# Outputs:
#   - crates/web-viewer/pkg/  -- raw wasm-pack output
#   - site/wasm/              -- subset consumed by site/viewer.html
#
# Requirements: rustup target add wasm32-unknown-unknown; cargo install wasm-pack
#
# Re-run this any time the web-viewer crate or anything it depends on changes.
# Neither output is committed.
#
# `--fast` builds the iteration bundle instead: the `wasm-fast` profile with no
# wasm-opt pass, so a page check after an edit does not wait for the shipped
# build. It behaves as the shipped bundle does, larger and slower; the stamp says
# which it is, and `check-wasm-freshness.py --strict` refuses it. What it costs:
# docs/tooling/build-and-test-speed.md#the-wasm-bundle.

set -euo pipefail

PROFILE_ARGS=(--release)
STAMP_PROFILE=release
if [[ "${1:-}" == "--fast" ]]; then
    PROFILE_ARGS=(--profile wasm-fast --no-opt)
    STAMP_PROFILE=wasm-fast
elif [[ $# -gt 0 ]]; then
    echo "usage: build-wasm.sh [--fast]" >&2
    exit 2
fi

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
PKG_DIR="${REPO}/crates/web-viewer/pkg"
SITE_DIR="${REPO}/site/wasm"

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "ERROR: wasm-pack not on PATH. Install via 'cargo install wasm-pack'." >&2
    exit 1
fi

echo "[build-wasm] building crates/web-viewer for the web target..."
(cd "${REPO}/crates/web-viewer" && wasm-pack build --target web "${PROFILE_ARGS[@]}")

if [[ ! -d "${PKG_DIR}" ]]; then
    echo "ERROR: wasm-pack did not produce ${PKG_DIR}" >&2
    exit 1
fi

echo "[build-wasm] syncing pkg/ into ${SITE_DIR}..."
mkdir -p "${SITE_DIR}"
# Copy only the artifacts the site loads (skip README, .gitignore noise).
cp "${PKG_DIR}/legaia_web_viewer_bg.wasm" "${SITE_DIR}/"
cp "${PKG_DIR}/legaia_web_viewer_bg.wasm.d.ts" "${SITE_DIR}/"
cp "${PKG_DIR}/legaia_web_viewer.d.ts" "${SITE_DIR}/"
cp "${PKG_DIR}/legaia_web_viewer.js" "${SITE_DIR}/"
cp "${PKG_DIR}/package.json" "${SITE_DIR}/"

# Record what these artifacts were built from. Without this the tree has no way
# to answer "is the shipped bundle current?" - and answering it from file
# mtimes or `git log` gets it wrong, because a branch can build the bundle and
# then be rebased onto different sources. See check-wasm-freshness.py.
if command -v python3 >/dev/null 2>&1; then
    python3 "${REPO}/scripts/ci/check-wasm-freshness.py" --write --profile "${STAMP_PROFILE}"
else
    echo "[build-wasm] WARN: no python3; site/wasm/SOURCE_STAMP.json not updated" >&2
fi

# Re-stamp the pages: every page carries window.LEGAIA_WASM_V (the wasm
# content version the JS loaders append as ?v= to the module + binary
# URLs). Skipping this leaves pages advertising the PREVIOUS version, and
# a browser that cached those bytes under the old key serves them stale
# on the next rebuild.
python3 "${REPO}/site/_gen.py" >/dev/null

echo "[build-wasm] done. site/wasm/ is now in sync with crates/web-viewer (pages re-stamped)."
