#!/usr/bin/env bash
#
# Build and package the release artifact for one target.
#
# Usage:
#     scripts/ci/release-build.sh <version> <target> [outdir]
#
#     version   Version string for the archive name, no leading "v"
#               (the release workflow strips it from the tag).
#     target    Rust target triple; must appear in the matrix below.
#     outdir    Where the archive lands. Default: target/dist -- inside the
#               already-gitignored target/, so a local rehearsal leaves no
#               untracked files behind.
#
# Produces, in <outdir>:
#     legaia-tools-<version>-<target>.tar.gz   (Linux targets)
#     legaia-tools-<version>-<target>.zip      (Windows targets)
#
# The archive holds a single top-level legaia-tools-<version>-<target>/
# directory so it never explodes over the user's cwd. Inside: the binaries,
# both licenses, and a generated README.txt.
#
# Contents are exclusively our own compiled binaries plus our own text files.
# No disc image is read and no game data is packaged -- this repo ships no
# Sony-owned bytes, and nothing here should ever change that.

set -euo pipefail

VERSION="${1:-}"
TARGET="${2:-}"
OUTDIR="${3:-target/dist}"

if [[ -z "$VERSION" || -z "$TARGET" ]]; then
    printf '[release-build] usage: %s <version> <target> [outdir]\n' "$0" >&2
    exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

log() { printf '[release-build] %s\n' "$*"; }

# --- The binary matrix -----------------------------------------------------
#
# CLI_BINS build for every target. GUI_BINS (wgpu + winit + cpal) are opt-in
# per target, because cpal's alsa-sys needs an x86_64 libasound to link
# against and the arm64 runner only has the arm64 one -- see
# docs/tooling/releases.md for the full reasoning and the fix.

CLI_BINS=(
    anm art asset cheat-tool disc-extract field-disasm font-extract
    gamedata-tool legaia-extract legaia-rando lzs-decode mdec mdt
    mednafen-state mes prot-extract save-tool seq tim tmd vab xa
)
GUI_BINS=(legaia-engine asset-viewer)

# Cargo packages owning CLI_BINS, for the -p selection used when a target
# cannot take the whole workspace.
CLI_PKGS=(
    legaia-anm legaia-art legaia-asset legaia-cheats legaia-engine-vm
    legaia-extract legaia-font legaia-gamedata legaia-iso legaia-lzs
    legaia-mdec legaia-mdt legaia-mednafen legaia-mes legaia-prot
    legaia-rando legaia-save legaia-seq legaia-tim legaia-tmd
    legaia-vab legaia-xa
)

# Oldest glibc the cross-built x86_64 Linux binaries must run against.
# 2.28 == Debian 10 / RHEL 8 / Ubuntu 18.10 and newer.
GLIBC_PIN="2.28"

case "$TARGET" in
    aarch64-unknown-linux-gnu)
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}")
        BIN_EXT=""
        ARCHIVE_KIND="tar.gz"
        BUILD_MODE="workspace"
        ;;
    x86_64-pc-windows-gnu)
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}")
        BIN_EXT=".exe"
        ARCHIVE_KIND="zip"
        BUILD_MODE="workspace"
        ;;
    x86_64-unknown-linux-gnu)
        # CLI only: see the alsa-sys note above.
        BINS=("${CLI_BINS[@]}")
        BIN_EXT=""
        ARCHIVE_KIND="tar.gz"
        BUILD_MODE="cli-zigbuild"
        ;;
    *)
        printf '[release-build] ERROR: %s is not in the release matrix\n' "$TARGET" >&2
        exit 2
        ;;
esac

CACHE="${LEGAIA_RELEASE_CACHE:-$HOME/.cache/legaia-release}"
export PATH="$CACHE/bin:$PATH"

# --- Build -----------------------------------------------------------------
log "building $TARGET (mode: $BUILD_MODE, ${#BINS[@]} binaries)"

case "$BUILD_MODE" in
    workspace)
        cargo build --release --locked --target "$TARGET" --workspace
        ;;
    cli-zigbuild)
        pkg_args=()
        for p in "${CLI_PKGS[@]}"; do pkg_args+=(-p "$p"); done
        cargo zigbuild --release --locked \
            --target "${TARGET}.${GLIBC_PIN}" "${pkg_args[@]}"
        ;;
esac

# --- Stage -----------------------------------------------------------------
STAGE_NAME="legaia-tools-${VERSION}-${TARGET}"
STAGE="${OUTDIR}/${STAGE_NAME}"
BUILT="target/${TARGET}/release"

rm -rf "$STAGE"
mkdir -p "$STAGE"

for b in "${BINS[@]}"; do
    src="${BUILT}/${b}${BIN_EXT}"
    if [[ ! -f "$src" ]]; then
        printf '[release-build] ERROR: expected binary missing: %s\n' "$src" >&2
        exit 1
    fi
    cp "$src" "$STAGE/"
done

cp LICENSE "$STAGE/LICENSE"
cp LICENSE-MIT "$STAGE/LICENSE-MIT"

{
    printf 'Legend of Legaia RE - command-line tools\n'
    printf '========================================\n\n'
    printf 'Version: %s\n' "$VERSION"
    printf 'Target:  %s\n\n' "$TARGET"
    printf 'These are the reverse-engineering and engine binaries only.\n'
    printf 'They ship NO game data. Every tool here reads a disc image that\n'
    printf 'you supply yourself; none is included or redistributed.\n\n'
    printf 'Start with legaia-extract, which drives the whole disc pipeline:\n\n'
    printf '    ./legaia-extract "/path/to/your/disc.bin" --out extracted\n\n'
    printf 'Licensed MIT OR Unlicense - see LICENSE and LICENSE-MIT.\n'
    printf 'Docs and source: https://github.com/%s\n\n' \
        "${GITHUB_REPOSITORY:-altimit-mii/legend-of-legaia-re}"
    printf 'Binaries in this archive (%d):\n\n' "${#BINS[@]}"
    for b in "${BINS[@]}"; do printf '    %s%s\n' "$b" "$BIN_EXT"; done
    if [[ "$TARGET" == "x86_64-unknown-linux-gnu" ]]; then
        printf '\nNote: the GUI binaries (legaia-engine, asset-viewer) are not in\n'
        printf 'this x86_64 Linux archive. See docs/tooling/releases.md.\n'
        printf 'Requires glibc %s or newer.\n' "$GLIBC_PIN"
    fi
} > "$STAGE/README.txt"

# --- Archive ---------------------------------------------------------------
cd "$OUTDIR"
case "$ARCHIVE_KIND" in
    tar.gz)
        ARCHIVE="${STAGE_NAME}.tar.gz"
        # Deterministic-ish: sorted entries, fixed owner/mtime.
        tar --sort=name --owner=0 --group=0 --numeric-owner \
            --mtime='UTC 2020-01-01' \
            -czf "$ARCHIVE" "$STAGE_NAME"
        ;;
    zip)
        ARCHIVE="${STAGE_NAME}.zip"
        rm -f "$ARCHIVE"
        zip -q -r -X "$ARCHIVE" "$STAGE_NAME"
        ;;
esac

rm -rf "$STAGE_NAME"
sha256sum "$ARCHIVE" > "${ARCHIVE}.sha256"

log "wrote ${OUTDIR}/${ARCHIVE}"
cat "${ARCHIVE}.sha256"
