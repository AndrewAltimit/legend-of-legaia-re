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
# Every target ships every binary. GUI_BINS (wgpu + winit + cpal) are listed
# separately only because their cpal -> alsa-sys dependency is what makes the
# x86_64 Linux row need an amd64 ALSA sysroot; setup-cross-toolchain.sh builds
# one. See docs/tooling/releases.md.

CLI_BINS=(
    anm art asset cheat-tool disc-extract field-disasm font-extract
    gamedata-tool legaia-extract legaia-rando lzs-decode mdec mdt
    mednafen-state mes prot-extract save-tool seq tim tmd vab xa
)
GUI_BINS=(legaia-engine asset-viewer)

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
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}")
        BIN_EXT=""
        ARCHIVE_KIND="tar.gz"
        BUILD_MODE="zigbuild"
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
    zigbuild)
        # alsa-sys shells out to pkg-config, which refuses a cross lookup
        # unless told to allow it and pointed at the target's own .pc tree.
        ALSA_SYSROOT="$CACHE/sysroot-amd64"
        ALSA_LIBDIR="$ALSA_SYSROOT/usr/lib/x86_64-linux-gnu"
        if [[ ! -f "$ALSA_LIBDIR/pkgconfig/alsa.pc" ]]; then
            printf '[release-build] ERROR: amd64 ALSA sysroot missing at %s\n' \
                "$ALSA_SYSROOT" >&2
            printf '[release-build] run: scripts/ci/setup-cross-toolchain.sh %s\n' \
                "$TARGET" >&2
            exit 1
        fi
        export PKG_CONFIG_ALLOW_CROSS=1
        export PKG_CONFIG_SYSROOT_DIR="$ALSA_SYSROOT"
        export PKG_CONFIG_LIBDIR="$ALSA_LIBDIR/pkgconfig"
        # --allow-shlib-undefined: libasound.so is built against a newer glibc
        # than GLIBC_PIN, so its own internal references (pow@GLIBC_2.29,
        # dlclose@GLIBC_2.34, ...) are unresolvable in this link. They don't
        # need resolving here -- libasound is a *shared* dependency, satisfied
        # at runtime by the user's own copy and their glibc. This relaxes the
        # check only for symbols undefined inside shared libraries; undefined
        # references from our own objects still fail the link, and the built
        # binaries stay pinned at GLIBC_PIN (asserted below).
        export RUSTFLAGS="${RUSTFLAGS:-} -L native=$ALSA_LIBDIR -C link-arg=-Wl,--allow-shlib-undefined"
        cargo zigbuild --release --locked \
            --target "${TARGET}.${GLIBC_PIN}" --workspace --bins
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

# The glibc pin is a promise to users on older distros, and the ALSA sysroot
# link is exactly the kind of change that could quietly break it. Verify the
# real symbol table rather than trusting the target suffix.
if [[ "$BUILD_MODE" == "zigbuild" ]] && command -v objdump >/dev/null 2>&1; then
    max_glibc="$(objdump -T "$STAGE"/* 2>/dev/null \
        | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
    want="GLIBC_${GLIBC_PIN}"
    highest="$(printf '%s\n%s\n' "$max_glibc" "$want" | sort -V | tail -1)"
    if [[ "$highest" != "$want" ]]; then
        printf '[release-build] ERROR: glibc pin broken: needs %s, pinned %s\n' \
            "$max_glibc" "$want" >&2
        exit 1
    fi
    log "glibc pin holds: highest requirement is ${max_glibc:-none} (pin $want)"
fi

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
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf 'Binaries in this archive (%d):\n\n' "${#BINS[@]}"
    for b in "${BINS[@]}"; do printf '    %s%s\n' "$b" "$BIN_EXT"; done
    if [[ "$TARGET" == "x86_64-unknown-linux-gnu" ]]; then
        printf '\nRequires glibc %s or newer.\n' "$GLIBC_PIN"
        printf 'legaia-engine and asset-viewer need ALSA (libasound.so.2) at\n'
        printf 'runtime; every mainstream desktop Linux already ships it.\n'
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
