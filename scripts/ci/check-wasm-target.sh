#!/usr/bin/env bash
# Type-check the wasm32 target - the half of the browser host no native gate
# compiles.
#
# `#[cfg(target_arch = "wasm32")]` code is invisible to `cargo check`,
# `cargo clippy --all-targets --workspace` and `cargo test`, every one of
# which builds for the host triple. A play-page feature written inside such a
# block can therefore reference a private field, a moved method or a type that
# no longer exists, and stay green through the whole local gate ladder: the
# first thing that ever compiles it is a wasm build.
#
# That is not hypothetical. A page-only feature shipped with a private-field
# error past every host-target gate in this repo; `scripts/ci/build-wasm.sh`
# surfaced it, minutes of wasm-pack later.
#
# This is the cheap half of `check-wasm.sh`: a type-check, not a build, and no
# `wasm-pack`, so it costs about what a `cargo check` of one crate costs once
# the wasm dependency graph is warm. Use `check-wasm.sh --full` before telling
# anyone a play-page fix is live - that one also proves `site/wasm/` was built
# from these sources.
#
# Usage:
#     scripts/ci/check-wasm-target.sh          # legaia-web-viewer
#     scripts/ci/check-wasm-target.sh -p foo   # any other package
#
# Exits non-zero on a type error, as a gate should. Exits 0 with a SKIPPED
# notice when the wasm32 target is not installed and cannot be added, so a
# clone without it is not blocked from committing.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO"

PKG_ARGS=("$@")
if [[ ${#PKG_ARGS[@]} -eq 0 ]]; then
    PKG_ARGS=(-p legaia-web-viewer)
fi

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
    echo "[check-wasm-target] wasm32-unknown-unknown not installed; adding..."
    if ! rustup target add wasm32-unknown-unknown; then
        echo "[check-wasm-target] SKIPPED - could not add the wasm32 target." >&2
        echo "[check-wasm-target] This is a vacuous pass: nothing type-checked the browser host." >&2
        exit 0
    fi
fi

# `--release` rather than the dev profile on purpose: it is the profile
# `build-wasm.sh`, `check-wasm.sh` and the CI wasm step all build, so the
# dependency graph this warms is the one they reuse.
echo "[check-wasm-target] cargo check --release --target wasm32-unknown-unknown ${PKG_ARGS[*]}"
cargo check --release --target wasm32-unknown-unknown "${PKG_ARGS[@]}"
echo "[check-wasm-target] OK"
