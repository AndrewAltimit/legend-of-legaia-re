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
# directory so it never explodes over the user's cwd. Its top level holds
# only what a player opens - the game and Legaia Explorer - plus
# README-PLAY.txt (how to start them), both licenses and
# THIRD-PARTY-LICENSES.txt (the notices of every crate the binaries link,
# see third-party-licenses.py):
#
#     Linux     "Legend of Legaia" (the legaia-launch stub), "Legaia Explorer"
#     Windows   "Legend of Legaia.exe" (the stub), "Legaia Explorer.exe"
#     macOS     "Legend of Legaia.app", "Legaia Explorer.app"
#
# packs/rebirth.lxpack beside them is the Rebirth expansion pack, bundled
# from packs/rebirth/ by `asset expansion bundle` (deterministic, and refused
# unless the pack validates - the validator's no-disc-bytes rule included,
# so it carries disc coordinates and the pack's own art and audio only). The
# game lists it in the launcher and `--expansion rebirth` resolves it
# (legaia_app_theme::release_layout::find_bundled_packs).
#
# Every command-line program - legaia-engine itself, asset-viewer and the
# format tools - sits in command-line-tools/ with its own README.txt. The
# stub and Explorer's Play button find legaia-engine there
# (legaia_app_theme::release_layout). The Windows executables carry their
# icons as resources (legaia_app_theme::win_icon; checked below) and the
# macOS bundles theirs as .icns, both from crates/app-theme/icons/.
#
# Runs on Linux (the self-hosted runner) and on macOS (a GitHub-hosted
# runner, for universal-apple-darwin). Kept to bash 3.2 + BSD userland on
# the macOS path: no GNU-only tar flags, no sha256sum assumption.
#
# Contents are exclusively our own compiled binaries, our own text files and
# the project's own expansion pack.
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
    anm art asset cheat-tool disc-extract font-extract
    gamedata-tool legaia-extract legaia-patcher lzs-decode mdec mdt
    mednafen-state mes prot-extract save-tool seq tim tmd vab xa
)
GUI_BINS=(legaia-engine asset-viewer legaia-explorer)
# The double-click entry for the game on Linux and Windows (staged as
# "Legend of Legaia"); macOS opens the .app bundle instead.
STUB_BIN=legaia-launch

# Where every command-line program goes, where the bundled expansion packs
# go, and the icon sources.
TOOLS_DIR="command-line-tools"
PACKS_DIR="packs"
BUNDLED_PACKS=(rebirth)
ICONS="crates/app-theme/icons"

# Oldest glibc the cross-built x86_64 Linux binaries must run against.
# 2.28 == Debian 10 / RHEL 8 / Ubuntu 18.10 and newer.
GLIBC_PIN="2.28"

case "$TARGET" in
    aarch64-unknown-linux-gnu)
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}" "$STUB_BIN")
        BIN_EXT=""
        ARCHIVE_KIND="tar.gz"
        BUILD_MODE="workspace"
        ;;
    x86_64-pc-windows-gnu)
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}" "$STUB_BIN")
        BIN_EXT=".exe"
        ARCHIVE_KIND="zip"
        BUILD_MODE="workspace"
        ;;
    x86_64-unknown-linux-gnu)
        BINS=("${CLI_BINS[@]}" "${GUI_BINS[@]}" "$STUB_BIN")
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
        # libudev-sys (gilrs, the play window's controller input) resolves
        # libudev through the same pkg-config tree.
        if [[ ! -f "$ALSA_LIBDIR/pkgconfig/alsa.pc" || ! -f "$ALSA_LIBDIR/pkgconfig/libudev.pc" ]]; then
            printf '[release-build] ERROR: amd64 ALSA / libudev sysroot missing at %s\n' \
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
TOOLS="${STAGE}/${TOOLS_DIR}"
BUILT="target/${TARGET}/release"

rm -rf "$STAGE"
mkdir -p "$TOOLS"

for b in "${BINS[@]}"; do
    src="${BUILT}/${b}${BIN_EXT}"
    if [[ ! -f "$src" ]]; then
        printf '[release-build] ERROR: expected binary missing: %s\n' "$src" >&2
        exit 1
    fi
done

# The top level: what a player opens. Linux and Windows get the stub under
# the game's name and Explorer under its own; macOS gets both as .app
# bundles (make_app below), so no bare GUI binary sits at its top level.
GAME_ENTRY="Legend of Legaia${BIN_EXT}"
EXPLORER_ENTRY="Legaia Explorer${BIN_EXT}"
case "$TARGET" in
    *-apple-darwin) ;;
    *)
        cp "${BUILT}/${STUB_BIN}${BIN_EXT}" "$STAGE/$GAME_ENTRY"
        cp "${BUILT}/legaia-explorer${BIN_EXT}" "$STAGE/$EXPLORER_ENTRY"
        ;;
esac

# Everything else: command-line-tools/.
for b in "${BINS[@]}"; do
    case "$b" in
        "$STUB_BIN" | legaia-explorer) continue ;;
    esac
    cp "${BUILT}/${b}${BIN_EXT}" "$TOOLS/"
done

# Windows: legaia-engine.exe is a console program (its CLI subcommands need
# the ordinary terminal contract), so a double-click on it opens a console
# window. The double-click entry is the GUI-subsystem stub legaia-launch,
# shipped under the name a player looks for; it starts
# command-line-tools\legaia-engine.exe with no console. Legaia Explorer has
# no command-line contract, so it is a GUI program itself. Assert the
# subsystems from the PE header (2 = Windows GUI, 3 = console) so a build
# change cannot quietly bring a console back, and assert each of the three
# carries an icon group resource (RT_GROUP_ICON, type 14) so a build without
# a resource compiler cannot quietly ship iconless programs.
if [[ "$TARGET" == *-windows-* ]]; then
    pe_info() {
        python3 - "$1" <<'PY'
import struct, sys
b = open(sys.argv[1], "rb").read()
pe = struct.unpack_from("<I", b, 0x3C)[0]
assert b[pe:pe + 4] == b"PE\0\0", "not a PE image"
nsec = struct.unpack_from("<H", b, pe + 6)[0]
opt_size = struct.unpack_from("<H", b, pe + 20)[0]
opt = pe + 24
# Subsystem sits at +68 in the optional header (PE32 and PE32+ alike).
subsystem = struct.unpack_from("<H", b, opt + 68)[0]
# Data directory 2 is the resource table; the directories start at +96
# (PE32) or +112 (PE32+).
dirs = opt + (112 if struct.unpack_from("<H", b, opt)[0] == 0x20B else 96)
rsrc_rva = struct.unpack_from("<I", b, dirs + 2 * 8)[0]
icon = 0
for i in range(nsec):
    s = opt + opt_size + 40 * i
    vsize, va, raw_size, raw_ptr = struct.unpack_from("<IIII", b, s + 8)
    if rsrc_rva and va <= rsrc_rva < va + max(vsize, raw_size):
        root = raw_ptr + (rsrc_rva - va)
        named, ids = struct.unpack_from("<HH", b, root + 12)
        types = [struct.unpack_from("<I", b, root + 16 + 8 * j)[0]
                 for j in range(named + ids)]
        icon = int(14 in types)
print(subsystem, icon)
PY
    }
    if command -v python3 >/dev/null 2>&1; then
        read -r gui gui_icon < <(pe_info "$STAGE/$GAME_ENTRY")
        read -r cli cli_icon < <(pe_info "$TOOLS/legaia-engine.exe")
        read -r explorer explorer_icon < <(pe_info "$STAGE/$EXPLORER_ENTRY")
        if [[ "$gui" != 2 || "$cli" != 3 || "$explorer" != 2 ]]; then
            printf '[release-build] ERROR: PE subsystems wrong: launcher=%s (want 2), engine=%s (want 3), explorer=%s (want 2)\n' \
                "$gui" "$cli" "$explorer" >&2
            exit 1
        fi
        log "PE subsystems hold: $GAME_ENTRY + $EXPLORER_ENTRY = GUI, legaia-engine.exe = console"
        if [[ "$gui_icon" != 1 || "$cli_icon" != 1 || "$explorer_icon" != 1 ]]; then
            printf '[release-build] ERROR: icon resource missing: launcher=%s engine=%s explorer=%s (1 = present)\n' \
                "$gui_icon" "$cli_icon" "$explorer_icon" >&2
            printf '[release-build] the build needs a resource compiler (x86_64-w64-mingw32-windres, from mingw-w64)\n' >&2
            exit 1
        fi
        log "icons embedded: $GAME_ENTRY, $EXPLORER_ENTRY, legaia-engine.exe"
    fi
fi

# The glibc pin is a promise to users on older distros, and the ALSA sysroot
# link is exactly the kind of change that could quietly break it. Verify the
# real symbol table rather than trusting the target suffix.
if [[ "$BUILD_MODE" == "zigbuild" ]] && command -v objdump >/dev/null 2>&1; then
    max_glibc="$(find "$STAGE" -type f -perm -u+x -exec objdump -T {} + 2>/dev/null \
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

# macOS: a minimal .app around each GUI program, so Finder users double-click
# an app rather than a terminal binary. Started without arguments, the engine
# runs its launcher (asks for the disc once, then boots the game) and
# legaia-explorer opens its browser window. Each bundle carries its icon
# (Contents/Resources/<icon>.icns, named by CFBundleIconFile).
#
#   make_app <bundle name> <executable path> <bundle id suffix> <icon name>
make_app() {
    local name="$1" src="$2" id="$3" icon="$4"
    local exe
    exe="$(basename "$src")"
    local app="$STAGE/${name}.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$src" "$app/Contents/MacOS/$exe"
    cp "$ICONS/${icon}.icns" "$app/Contents/Resources/${icon}.icns"
    # CFBundleVersion must be dot-separated integers; a prerelease tag's
    # suffix (0.2.0-rc1) stays in the display string only.
    local bundle_version="${VERSION%%-*}"
    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>${name}</string>
    <key>CFBundleDisplayName</key><string>${name}</string>
    <key>CFBundleIdentifier</key><string>io.github.andrewaltimit.${id}</string>
    <key>CFBundleExecutable</key><string>${exe}</string>
    <key>CFBundleIconFile</key><string>${icon}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${bundle_version}</string>
    <key>LSMinimumSystemVersion</key><string>${MACOSX_DEPLOYMENT_TARGET}</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    # Ad-hoc signature: no Apple identity, but a consistent bundle seal, which
    # arm64 macOS requires before it will run the app at all.
    if command -v plutil >/dev/null 2>&1; then
        plutil -lint "$app/Contents/Info.plist"
    fi
    if command -v codesign >/dev/null 2>&1; then
        codesign --force --deep --sign - "$app"
        codesign --verify --deep --strict "$app"
    fi
}

if [[ "$TARGET" == *-apple-darwin ]]; then
    make_app "Legend of Legaia" "$TOOLS/legaia-engine" legaia-engine legend-of-legaia
    make_app "Legaia Explorer" "${BUILT}/legaia-explorer" legaia-explorer legaia-explorer
    GAME_RUN="$STAGE/Legend of Legaia.app/Contents/MacOS/legaia-engine"
    EXPLORER_RUN="$STAGE/Legaia Explorer.app/Contents/MacOS/legaia-explorer"
else
    GAME_RUN="$STAGE/$GAME_ENTRY"
    EXPLORER_RUN="$STAGE/$EXPLORER_ENTRY"
fi

# Smoke-run the shipped entry points when this host can execute them (the
# native Linux row, and the universal macOS row on its runner). A binary that
# cannot start - a missing dylib, a bad lipo - fails here, not on a user. On
# Linux the game entry is the stub, so this also proves it finds
# command-line-tools/legaia-engine.
HOST_TRIPLE="$(rustc -vV | awk '/^host: /{print $2}')"
if [[ "$TARGET" == "$HOST_TRIPLE" || "$BUILD_MODE" == "universal" ]]; then
    log "smoke: $("$TOOLS/legaia-engine${BIN_EXT}" --version)"
    log "smoke: $("$GAME_RUN" --version) (via ${GAME_RUN#"$STAGE"/})"
    log "smoke: $("$EXPLORER_RUN" --version)"
fi

# The bundled expansion packs: packs/<id>.lxpack at the top level. The
# bundler must run on this host, so a cross target bundles with a host build
# of `asset` (the bytes are the same on every host: the writer is
# deterministic and target-independent).
mkdir -p "$STAGE/$PACKS_DIR"
if [[ "$TARGET" == "$HOST_TRIPLE" || "$BUILD_MODE" == "universal" ]]; then
    BUNDLER=("${BUILT}/asset${BIN_EXT}")
else
    log "building a host asset binary to bundle the expansion packs"
    cargo build --release --locked -p legaia-asset --bin asset
    BUNDLER=("target/release/asset")
fi
for pack in "${BUNDLED_PACKS[@]}"; do
    "${BUNDLER[@]}" expansion bundle --out "$STAGE/$PACKS_DIR/${pack}.lxpack" "packs/${pack}"
done

cp LICENSE "$STAGE/LICENSE"
cp LICENSE-MIT "$STAGE/LICENSE-MIT"

# The third-party notices: every crate the shipped binaries link, resolved
# for this target (both slices for the universal macOS archive), with the
# license texts the crates themselves carry. Fails the release when a linked
# crate has no license or no text can be found for one of its ids.
case "$TARGET" in
    universal-apple-darwin)
        LICENSE_TARGETS=(--target aarch64-apple-darwin --target x86_64-apple-darwin) ;;
    *)
        LICENSE_TARGETS=(--target "$TARGET") ;;
esac
python3 scripts/ci/third-party-licenses.py "${LICENSE_TARGETS[@]}" \
    --out "$STAGE/THIRD-PARTY-LICENSES.txt"

# The one file a player needs: how to start the game.
{
    printf 'Legend of Legaia - engine port\n'
    printf '==============================\n\n'
    printf 'Version: %s (%s)\n\n' "$VERSION" "$TARGET"
    printf 'WHAT TO OPEN\n\n'
    printf 'This folder holds two programs to double-click:\n\n'
    case "$TARGET" in
        *-windows-*)
            printf '    Legend of Legaia.exe    play the game\n'
            printf '    Legaia Explorer.exe     browse and extract the game'"'"'s assets\n\n' ;;
        *-apple-darwin)
            printf '    Legend of Legaia.app    play the game\n'
            printf '    Legaia Explorer.app     browse and extract the game'"'"'s assets\n\n' ;;
        *)
            printf '    Legend of Legaia        play the game\n'
            printf '    Legaia Explorer         browse and extract the game'"'"'s assets\n\n' ;;
    esac
    printf 'The %s folder holds the command-line programs: the\n' "$TOOLS_DIR"
    printf 'game engine itself, plus modding and reverse-engineering tools for use\n'
    printf 'in a terminal. You do not need to open it to play. Keep it next to the\n'
    printf 'two programs above: they start the game from it.\n\n'
    printf 'TO PLAY\n\n'
    case "$TARGET" in
        *-windows-*)
            printf '    Double-click "Legend of Legaia.exe".\n\n'
            printf '    It starts %s\\legaia-engine.exe without a console\n' "$TOOLS_DIR"
            printf '    window. legaia-engine.exe is the same game plus every command-line\n'
            printf '    option; run it from a command prompt to see its log.\n\n' ;;
        *-apple-darwin)
            printf '    Double-click "Legend of Legaia.app" (or, in a terminal, run\n'
            printf '    %s/legaia-engine).\n\n' "$TOOLS_DIR"
            printf '    The app is not notarised by Apple. The first time, right-click it\n'
            printf '    and choose Open, or allow it under System Settings > Privacy &\n'
            printf '    Security > "Open Anyway". If macOS says the app is damaged, run:\n'
            printf '        xattr -dr com.apple.quarantine "Legend of Legaia.app"\n\n' ;;
        *)
            printf '    Double-click "Legend of Legaia" in your file manager (or run\n'
            printf '    ./"Legend of Legaia" in a terminal). It starts\n'
            printf '    %s/legaia-engine, the game engine, which you can also\n' "$TOOLS_DIR"
            printf '    run directly: it is the same game plus every command-line option.\n\n' ;;
    esac
    printf 'The launcher opens. The first time, click "Choose disc..." and pick\n'
    printf 'YOUR Legend of Legaia (USA) disc image: the .bin file of a Mode2/2352\n'
    printf 'dump (or its .cue sheet). Dragging the file onto the window, or typing\n'
    printf 'its path, works too. The disc is checked and remembered; if it is the\n'
    printf 'wrong one (another region, not a disc image, moved), the launcher\n'
    printf 'says why. Then press Play (Enter) for the title screen.\n\n'
    printf 'The launcher also sets the display options - fullscreen, window size,\n'
    printf 'enhanced lighting, see-through walls, fog, bloom, the PlayStation\n'
    printf 'pixel look, camera distance, volumes, or "Original game only" - and\n'
    printf 'shows the controls.\n'
    printf 'Arrow keys move, Left / Right change an option, Esc quits. If the game\n'
    printf 'stops with an error, the launcher comes back and shows it; the full\n'
    printf 'log is legaia-engine-game.log in the saves folder.\n\n'
    printf 'EXPANSION PACK\n\n'
    printf 'The %s folder holds Legend of Legaia: Rebirth (rebirth.lxpack), an\n' "$PACKS_DIR"
    printf 'expansion pack made for the port: a new tower to climb past level 60,\n'
    printf 'quests, bosses and game modes. In the launcher, click the Expansion\n'
    printf 'pack button (or select it and press Enter) to play with it; New Game\n'
    printf 'and Continue then use it. Delete (or a right-click) on the button goes\n'
    printf 'back to the game as it shipped. From a terminal:\n'
    printf '    %s/legaia-engine play-window --disc <disc.bin> --boot-ui --expansion rebirth\n' "$TOOLS_DIR"
    printf 'Other packs (.lxpack files) can be dropped in the same folder, or in\n'
    printf 'the packs folder of your saves folder, and show up the same way.\n'
    printf 'The pack holds no game data either: it points at the disc you supply.\n\n'
    printf 'NO GAME DATA IS INCLUDED. The engine reads the disc you supply and\n'
    printf 'nothing else; dump your own copy of the game.\n\n'
    printf 'Settings, key bindings, options and saves live in your per-user\n'
    printf 'config / data folders (on Linux ~/.config/legaia-engine and\n'
    # shellcheck disable=SC2088 # a literal ~ for the reader, not an expansion
    printf '~/.local/share/legaia-engine; on Windows %%APPDATA%%\\legaia-engine;\n'
    printf 'on macOS ~/Library/Application Support/legaia-engine).\n\n'
    printf 'CONTROLS (keyboard; a gamepad works too, by button position)\n\n'
    printf '    Arrow keys     D-pad\n'
    printf '    Z / S / A / X  Cross / Circle / Triangle / Square\n'
    printf '    Q / W          L1 / R1\n'
    printf '    1 / 2          L2 / R2\n'
    printf '    Enter, Space   Start\n'
    printf '    Right Shift    Select\n'
    printf '    Esc twice      quit (one press only asks; save in the game first)\n\n'
    printf 'Key Config on the in-game Options screen rebinds the keys.\n\n'
    printf 'Display and sound toggles:\n\n'
    printf '    I    enhanced lighting        F8   lighting time of day\n'
    printf '    F4   see-through walls        F9   ground fog\n'
    printf '    T    camera distance          R    free-angle movement\n'
    printf '    F2   mute                     F12  list every toggle (also ?)\n'
    printf '    F11 or Alt+Enter   fullscreen\n\n'
    printf 'Each toggle shows its new state at the bottom left for a moment.\n\n'
    printf 'Mouse: drag to turn the camera, wheel to zoom, double-click to reset.\n'
    printf 'Every play option is on the command line:\n'
    printf '    %s/legaia-engine play-window --help\n\n' "$TOOLS_DIR"
    printf 'Guide: https://github.com/%s/blob/main/docs/guides/playing-and-viewing.md\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf '\nTO BROWSE AND EXTRACT THE GAME'"'"'S ASSETS\n\n'
    case "$TARGET" in
        *-windows-*)
            printf '    Double-click "Legaia Explorer.exe".\n\n' ;;
        *-apple-darwin)
            printf '    Double-click "Legaia Explorer.app"; the first-open steps above\n'
            printf '    apply to it too.\n\n' ;;
        *)
            printf '    Double-click "Legaia Explorer" (or run ./"Legaia Explorer").\n\n' ;;
    esac
    printf 'Legaia Explorer opens the same disc image and lets you browse every\n'
    printf 'archive entry, preview textures, 3D models, sound banks, music and\n'
    printf 'dialog text, export them (PNG, OBJ / glTF, WAV), or unpack the whole\n'
    printf 'disc into a folder of your choice. Its Play button starts the game\n'
    printf 'on the disc you have open and remembers that disc for the game, so\n'
    printf 'starting the game directly afterwards does not ask for it. Guide:\n'
    printf 'https://github.com/%s/blob/main/docs/guides/extracting-assets.md\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf '\nBoth apps, with screenshots:\n'
    printf 'https://andrewaltimit.github.io/legend-of-legaia-re/guides/desktop-apps.html\n'
    printf '\nThe command-line tools in %s are listed in its README.txt.\n' "$TOOLS_DIR"
    printf '\nLicensed MIT OR Unlicense - see LICENSE and LICENSE-MIT. The programs\n'
    printf 'link third-party crates; their licenses and notices are in\n'
    printf 'THIRD-PARTY-LICENSES.txt.\n'
    if [[ "$TARGET" == "x86_64-unknown-linux-gnu" ]]; then
        printf '\nRequires glibc %s or newer. The game and Legaia Explorer need ALSA\n' "$GLIBC_PIN"
        printf '(libasound.so.2) at runtime, which every mainstream desktop Linux\n'
        printf 'already ships; Legaia Explorer also needs OpenGL (libGL / libEGL).\n'
    fi
} > "$STAGE/README-PLAY.txt"

{
    printf 'Legend of Legaia RE - command-line tools\n'
    printf '========================================\n\n'
    printf 'Version: %s\n' "$VERSION"
    printf 'Target:  %s\n\n' "$TARGET"
    printf 'This folder holds the command-line programs: the game engine and the\n'
    printf 'modding and reverse-engineering tools. They ship NO game data. Every\n'
    printf 'tool here reads a disc image that you supply yourself; none is\n'
    printf 'included or redistributed.\n\n'
    printf 'To PLAY or to browse assets without a terminal, use the programs one\n'
    printf 'folder up (see ../README-PLAY.txt). Here, legaia-extract drives the\n'
    printf 'whole disc pipeline from a terminal opened in this folder:\n\n'
    printf '    ./legaia-extract "/path/to/your/disc.bin" --out extracted\n\n'
    printf 'The tools in this folder, one line each:\n\n'
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
    printf '    mdt             MOVE clip-bank inspector\n'
    printf '    art             Tactical Arts data inspector\n'
    printf '    font-extract    dialog font -> glyph atlas + widths\n'
    printf 'Play + view\n'
    printf '    legaia-engine   the from-scratch engine: with no arguments the game'"'"'s\n'
    printf '                    launcher; play-window, play-str, record/replay, ...\n'
    printf '    asset-viewer    windowed viewer over legaia-extract output (textures, meshes,\n'
    printf '                    audio, scenes); with no arguments it browses ./extracted\n'
    printf 'Mod + translate\n'
    printf '    legaia-patcher  disc patcher: randomizer, "translate" language packs, manual edits\n'
    printf '    save-tool       memory-card and save-file inspector\n'
    printf '    gamedata-tool   curated game-data lookups (arts, items, enemies, shops)\n'
    printf '    cheat-tool      cheat-database inspector (databases built in)\n'
    printf 'Reverse engineering\n'
    printf '    asset           asset formats; "asset field-disasm" walks field-VM bytecode\n'
    printf '    mednafen-state  emulator save-state analysis\n\n'
    printf 'Every program supports --help and --version.\n\n'
    printf 'Guides:          https://github.com/%s/tree/main/docs/guides\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf 'Docs and source: https://github.com/%s\n\n' \
        "${GITHUB_REPOSITORY:-AndrewAltimit/legend-of-legaia-re}"
    printf 'Verify the archive against the SHA256SUMS file on the release page:\n'
    printf '    sha256sum -c SHA256SUMS --ignore-missing\n\n'
    printf 'Licensed MIT OR Unlicense - see ../LICENSE and ../LICENSE-MIT.\n'
    printf 'The programs link third-party crates; their licenses and notices are\n'
    printf 'in ../THIRD-PARTY-LICENSES.txt.\n'
    if [[ "$TARGET" == "x86_64-unknown-linux-gnu" ]]; then
        printf '\nRequires glibc %s or newer.\n' "$GLIBC_PIN"
        printf 'legaia-engine and asset-viewer need ALSA (libasound.so.2) at\n'
        printf 'runtime; every mainstream desktop Linux already ships it.\n'
    fi
} > "$TOOLS/README.txt"

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
