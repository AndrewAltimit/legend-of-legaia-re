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
#     legaia-tools-<version>-<target>.tar.gz   (Linux + macOS targets)
#     legaia-tools-<version>-<target>.zip      (Windows targets)
#
# The archive holds a single top-level legaia-tools-<version>-<target>/
# directory so it never explodes over the user's cwd. Inside: the binaries,
# both licenses, a generated README-PLAY.txt (how to start the game) and
# README.txt (every tool), and on macOS a "Legend of Legaia.app" wrapper
# around legaia-engine.
#
# Runs on Linux (the self-hosted runner) and on macOS (a GitHub-hosted
# runner, for universal-apple-darwin). Kept to bash 3.2 + BSD userland on
# the macOS path: no GNU-only tar flags, no sha256sum assumption.
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
    gamedata-tool legaia-extract legaia-patcher lzs-decode mdec mdt
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
    universal-apple-darwin)
        # Not a rustc triple: both macOS slices, fused with lipo.
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}")
        BIN_EXT=""
        ARCHIVE_KIND="tar.gz"
        BUILD_MODE="universal"
        ;;
    aarch64-apple-darwin | x86_64-apple-darwin)
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}")
        BIN_EXT=""
        ARCHIVE_KIND="tar.gz"
        BUILD_MODE="workspace"
        ;;
    *)
        printf '[release-build] ERROR: %s is not in the release matrix\n' "$TARGET" >&2
        exit 2
        ;;
esac

CACHE="${LEGAIA_RELEASE_CACHE:-$HOME/.cache/legaia-release}"
export PATH="$CACHE/bin:$PATH"

# macOS slices target Big Sur and newer (the arm64 floor; wgpu's Metal
# backend is fine there).
case "$TARGET" in
    *-apple-darwin) export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}" ;;
esac

# sha256sum is GNU coreutils; macOS ships shasum. Same output format.
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

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
    universal)
        for slice in aarch64-apple-darwin x86_64-apple-darwin; do
            cargo build --release --locked --target "$slice" --workspace --bins
        done
        # Fuse each binary's two slices into target/universal-apple-darwin/,
        # where the staging step below expects a per-target release dir.
        mkdir -p "target/${TARGET}/release"
        for b in "${BINS[@]}"; do
            lipo -create \
                "target/aarch64-apple-darwin/release/${b}" \
                "target/x86_64-apple-darwin/release/${b}" \
                -output "target/${TARGET}/release/${b}"
        done
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

# Smoke-run the game binary when this host can execute it (the native
# Linux row, and the universal macOS row on its runner). A binary that
# cannot start - a missing dylib, a bad lipo - fails here, not on a user.
HOST_TRIPLE="$(rustc -vV | awk '/^host: /{print $2}')"
if [[ "$TARGET" == "$HOST_TRIPLE" || "$BUILD_MODE" == "universal" ]]; then
    log "smoke: $("$STAGE/legaia-engine${BIN_EXT}" --version)"
fi

# macOS: a minimal .app around legaia-engine, so Finder users double-click
# an app rather than a terminal binary. Started without arguments, the
# engine runs its launcher (asks for the disc once, then boots the game).
if [[ "$TARGET" == *-apple-darwin ]]; then
    APP="$STAGE/Legend of Legaia.app"
    mkdir -p "$APP/Contents/MacOS"
    cp "$STAGE/legaia-engine" "$APP/Contents/MacOS/legaia-engine"
    cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Legend of Legaia</string>
    <key>CFBundleDisplayName</key><string>Legend of Legaia</string>
    <key>CFBundleIdentifier</key><string>io.github.andrewaltimit.legaia-engine</string>
    <key>CFBundleExecutable</key><string>legaia-engine</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET}</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    # Ad-hoc signature: no Apple identity, but a consistent bundle seal, which
    # arm64 macOS requires before it will run the app at all.
    if command -v codesign >/dev/null 2>&1; then
        codesign --force --deep --sign - "$APP"
    fi
fi

cp LICENSE "$STAGE/LICENSE"
cp LICENSE-MIT "$STAGE/LICENSE-MIT"

# The one file a player needs: how to start the game.
{
    printf 'Legend of Legaia - engine port\n'
    printf '==============================\n\n'
    printf 'Version: %s (%s)\n\n' "$VERSION" "$TARGET"
    printf 'TO PLAY\n\n'
    case "$TARGET" in
        *-windows-*)
            printf '    Double-click legaia-engine.exe.\n\n' ;;
        *-apple-darwin)
            printf '    Double-click "Legend of Legaia.app" (or run ./legaia-engine).\n\n'
            printf '    The app is not notarised by Apple. The first time, right-click it\n'
            printf '    and choose Open, or allow it under System Settings > Privacy &\n'
            printf '    Security > "Open Anyway". If macOS says the app is damaged, run:\n'
            printf '        xattr -dr com.apple.quarantine "Legend of Legaia.app"\n\n' ;;
        *)
            printf '    Run ./legaia-engine (or double-click it in your file manager).\n\n' ;;
    esac
    printf 'On first start a window asks for YOUR Legend of Legaia (USA) disc\n'
    printf 'image: the .bin file of a Mode2/2352 dump (or its .cue sheet). Press\n'
    printf 'Enter or click to browse, drag the file onto the window, or type its\n'
    printf 'path. The choice is remembered, and the game opens on the title\n'
    printf 'screen. The picker only returns if that file moves or stops\n'
    printf 'validating.\n\n'
    printf 'NO GAME DATA IS INCLUDED. The engine reads the disc you supply and\n'
    printf 'nothing else; dump your own copy of the game.\n\n'
    printf 'Settings, key bindings, options and saves live in your per-user\n'
    printf 'config / data folders (on Linux ~/.config/legaia-engine and\n'
    # shellcheck disable=SC2088 # a literal ~ for the reader, not an expansion
    printf '~/.local/share/legaia-engine; on Windows %%APPDATA%%\\legaia-engine;\n'
    printf 'on macOS ~/Library/Application Support/legaia-engine).\n\n'
    printf 'Controls: arrows = D-pad, Z = Cross, S = Circle, A = Triangle,\n'
    printf 'X = Square, Enter = Start, Esc = quit. Every play option is on the\n'
    printf 'command line: legaia-engine play-window --help\n\n'
    printf 'Guide: https://github.com/%s/blob/main/docs/guides/playing-and-viewing.md\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf '\nThe other binaries in this folder are modding and reverse-engineering\n'
    printf 'tools; README.txt lists them.\n'
} > "$STAGE/README-PLAY.txt"

{
    printf 'Legend of Legaia RE - command-line tools\n'
    printf '========================================\n\n'
    printf 'Version: %s\n' "$VERSION"
    printf 'Target:  %s\n\n' "$TARGET"
    printf 'These are the reverse-engineering and engine binaries only.\n'
    printf 'They ship NO game data. Every tool here reads a disc image that\n'
    printf 'you supply yourself; none is included or redistributed.\n\n'
    printf 'To PLAY, see README-PLAY.txt: start legaia-engine with no arguments.\n\n'
    printf 'To extract assets, start with legaia-extract, which drives the whole\n'
    printf 'disc pipeline:\n\n'
    printf '    ./legaia-extract "/path/to/your/disc.bin" --out extracted\n\n'
    printf 'The tools in this archive (%d), one line each:\n\n' "${#BINS[@]}"
    printf 'Extract + convert\n'
    printf '    legaia-extract  full disc pipeline: files, textures, audio, font\n'
    printf '    disc-extract    disc image -> ISO9660 files; "verify" checks your dump\n'
    printf '    prot-extract    PROT.DAT archive -> named entries\n'
    printf '    lzs-decode      the game'"'"'s LZS decompressor\n'
    printf '    asset           format hub: categorize, sub-asset extract, data tables\n'
    printf '    tim             textures -> PNG\n'
    printf '    tmd             3D meshes -> OBJ\n'
    printf '    vab             instrument banks -> VAG samples / WAV\n'
    printf '    xa              streamed CD-XA audio -> WAV\n'
    printf '    seq             sequenced music (SEQ) inspector\n'
    printf '    mdec            FMV movies -> image frames\n'
    printf '    mes             dialog-container inspector\n'
    printf '    anm             animation-container inspector\n'
    printf '    mdt             move-table inspector\n'
    printf '    art             Tactical Arts data inspector\n'
    printf '    font-extract    dialog font -> glyph atlas + widths\n'
    printf 'Play + view\n'
    printf '    legaia-engine   the from-scratch engine: play-window, play-str, record/replay\n'
    printf '    asset-viewer    windowed viewer: textures, meshes, audio, scenes\n'
    printf 'Mod + translate\n'
    printf '    legaia-patcher    disc patcher: randomizer, "translate" language packs, manual edits\n'
    printf '    save-tool       memory-card and save-file inspector\n'
    printf '    gamedata-tool   curated game-data lookups (arts, items, enemies, shops)\n'
    printf '    cheat-tool      cheat-database inspector (databases built in)\n'
    printf 'Reverse engineering\n'
    printf '    field-disasm    field-VM bytecode disassembler\n'
    printf '    mednafen-state  emulator save-state analysis\n\n'
    printf 'Every binary supports --help and --version.\n\n'
    printf 'Guides:          https://github.com/%s/tree/main/docs/guides\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf 'Docs and source: https://github.com/%s\n\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf 'Verify this archive against the SHA256SUMS file on the release page:\n'
    printf '    sha256sum -c SHA256SUMS --ignore-missing\n\n'
    printf 'Licensed MIT OR Unlicense - see LICENSE and LICENSE-MIT.\n'
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
        if tar --version 2>/dev/null | grep -q 'GNU tar'; then
            # Deterministic-ish: sorted entries, fixed owner/mtime.
            tar --sort=name --owner=0 --group=0 --numeric-owner \
                --mtime='UTC 2020-01-01' \
                -czf "$ARCHIVE" "$STAGE_NAME"
        else
            # BSD tar (macOS) has none of the GNU normalising flags.
            tar -czf "$ARCHIVE" "$STAGE_NAME"
        fi
        ;;
    zip)
        ARCHIVE="${STAGE_NAME}.zip"
        rm -f "$ARCHIVE"
        zip -q -r -X "$ARCHIVE" "$STAGE_NAME"
        ;;
esac

rm -rf "$STAGE_NAME"
sha256_of "$ARCHIVE" > "${ARCHIVE}.sha256"

log "wrote ${OUTDIR}/${ARCHIVE}"
cat "${ARCHIVE}.sha256"
