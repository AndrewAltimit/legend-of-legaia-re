#!/usr/bin/env bash
# Smoke check that the WASM target builds.
#
# Cheap fast-path: `cargo build --target wasm32-unknown-unknown -p legaia-web-viewer`.
# This is what `main-ci.yml` runs on every PR - running it locally before
# pushing catches WASM-only breakage (typically API uses that don't compile
# on `cdylib` targets, or wasm-bindgen feature gates).
#
# Heavier check: append `--full` to also run `wasm-pack build --target web
# --release`, then assert your local `site/wasm/` bundle was built from the
# sources in this tree. Useful before tagging a release, or before telling anyone
# a play-page fix is live - a locally served page loads the bundle you built, not
# the sources, and the two have silently diverged before.
#
# Usage:
#     scripts/ci/check-wasm.sh           # cargo build only (fast)
#     scripts/ci/check-wasm.sh --full    # wasm-pack build + bundle freshness

set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO"

if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    echo "[check-wasm] installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

echo "[check-wasm] cargo build --target wasm32-unknown-unknown -p legaia-web-viewer..."
cargo build --release --target wasm32-unknown-unknown -p legaia-web-viewer

if [[ "${1:-}" == "--full" ]]; then
    if ! command -v wasm-pack >/dev/null 2>&1; then
        echo "[check-wasm] wasm-pack not on PATH; install via 'cargo install wasm-pack'" >&2
        exit 1
    fi
    echo "[check-wasm] wasm-pack build --target web --release..."
    (cd crates/web-viewer && wasm-pack build --target web --release)

    # The build above proves the crate compiles; it says nothing about whether
    # the bundle sitting under site/wasm/ came from these sources. That is the
    # check people actually want here, so make it hard.
    echo "[check-wasm] verifying site/wasm/ freshness..."
    python3 scripts/ci/check-wasm-freshness.py --strict
    echo "[check-wasm] OK"
fi

echo "[check-wasm] OK"
