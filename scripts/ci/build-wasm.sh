#!/usr/bin/env bash
# Build the WASM viewer and sync the generated package into the static site.
#
# Outputs:
#   - crates/web-viewer/pkg/  -- raw wasm-pack output
#   - site/wasm/              -- subset consumed by site/viewer.html
#
# Requirements: rustup target add wasm32-unknown-unknown; cargo install wasm-pack
#
# Re-run this any time the web-viewer crate or anything it depends on changes,
# then commit both pkg/* and site/wasm/* together.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
PKG_DIR="${REPO}/crates/web-viewer/pkg"
SITE_DIR="${REPO}/site/wasm"

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "ERROR: wasm-pack not on PATH. Install via 'cargo install wasm-pack'." >&2
    exit 1
fi

echo "[build-wasm] building crates/web-viewer for the web target..."
(cd "${REPO}/crates/web-viewer" && wasm-pack build --target web --release)

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

echo "[build-wasm] done. site/wasm/ is now in sync with crates/web-viewer."
