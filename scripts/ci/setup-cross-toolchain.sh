#!/usr/bin/env bash
#
# Provision the toolchain needed to build one release target.
#
# Usage:
#     scripts/ci/setup-cross-toolchain.sh <rust-target>
#
# Idempotent: every step is a no-op when the tool is already present, so the
# release workflow can call it on every run without paying for reinstalls.
#
# Targets and what they need:
#
#   <host triple>                  Nothing beyond the default toolchain.
#
#   x86_64-pc-windows-gnu          The mingw-w64 cross toolchain, from apt:
#                                      sudo apt install mingw-w64
#                                  Provides x86_64-w64-mingw32-gcc, which
#                                  rustc drives as the linker. Needs root, so
#                                  this script only *checks* for it and fails
#                                  with a provisioning hint if it is missing.
#
#   x86_64-unknown-linux-gnu       Only when cross-compiling (i.e. the runner
#   (when not the host)            is not x86_64). Cross-linking an ELF binary
#                                  against glibc needs an x86_64 linker plus an
#                                  x86_64 glibc to link against. Rather than
#                                  require root for gcc-x86-64-linux-gnu plus
#                                  amd64 multiarch, this uses cargo-zigbuild:
#                                  zig ships its own multi-arch linker and
#                                  bundled glibc stubs, and both it and
#                                  cargo-zigbuild install without root.
#
#                                  The GUI binaries additionally reach cpal ->
#                                  alsa-sys, which resolves libasound through
#                                  pkg-config at build time, so the link needs
#                                  an *x86_64* libasound. apt can't supply one
#                                  here: an arm64 Ubuntu serves from
#                                  ports.ubuntu.com, which publishes no amd64,
#                                  so `dpkg --add-architecture amd64` fails to
#                                  fetch. Instead the amd64 .debs are pulled
#                                  straight from archive.ubuntu.com and
#                                  unpacked into a private sysroot -- no root,
#                                  no apt sources rewritten, nothing installed
#                                  system-wide.
#
# Everything root-free lands under $LEGAIA_RELEASE_CACHE (default
# ~/.cache/legaia-release) and is exposed by prepending its bin/ to PATH --
# see release-build.sh, which sources this script's PATH additions via
# `emit_path`.

set -euo pipefail

TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
    printf '[setup-cross] usage: %s <rust-target>\n' "$0" >&2
    exit 2
fi

CACHE="${LEGAIA_RELEASE_CACHE:-$HOME/.cache/legaia-release}"
HOST_TRIPLE="$(rustc -vV | awk '/^host: /{print $2}')"

log() { printf '[setup-cross] %s\n' "$*"; }

# --- Rust standard library for the target ----------------------------------
if rustup target list --installed | grep -qx "$TARGET"; then
    log "rust std for $TARGET already installed"
else
    log "adding rust std for $TARGET"
    rustup target add "$TARGET"
fi

# --- Native target: nothing else to do -------------------------------------
if [[ "$TARGET" == "$HOST_TRIPLE" ]]; then
    log "$TARGET is the host triple - no cross toolchain needed"
    exit 0
fi

# --- Windows: mingw-w64 (apt-provided, needs root to install) --------------
if [[ "$TARGET" == "x86_64-pc-windows-gnu" ]]; then
    if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
        log "mingw-w64 present: $(command -v x86_64-w64-mingw32-gcc)"
        exit 0
    fi
    cat >&2 <<'EOF'
[setup-cross] ERROR: x86_64-w64-mingw32-gcc not found.

The Windows target links through the mingw-w64 cross toolchain, which is an
apt package and therefore needs root to install. Provision the runner once:

    sudo apt install mingw-w64

The package is arch-independent in the sense that matters here: on an arm64
host apt installs arm64-hosted compilers that emit x86_64 PE objects.
EOF
    exit 1
fi

# --- x86_64 Linux from a non-x86_64 host: zig + cargo-zigbuild -------------
if [[ "$TARGET" == "x86_64-unknown-linux-gnu" ]]; then
    mkdir -p "$CACHE/bin"

    # zig, via the ziglang pip wheel (ships prebuilt zig for the host arch).
    # A venv sidesteps PEP 668's "externally managed environment" refusal on
    # Debian/Ubuntu without --break-system-packages.
    if [[ ! -x "$CACHE/bin/zig" ]]; then
        log "installing zig into $CACHE"
        python3 -m venv "$CACHE/zigvenv"
        "$CACHE/zigvenv/bin/pip" install --quiet --upgrade pip
        "$CACHE/zigvenv/bin/pip" install --quiet ziglang
        ZIG_BIN="$("$CACHE/zigvenv/bin/python" -c \
            'import os, ziglang; print(os.path.join(os.path.dirname(ziglang.__file__), "zig"))')"
        printf '#!/bin/sh\nexec %s "$@"\n' "$ZIG_BIN" > "$CACHE/bin/zig"
        chmod +x "$CACHE/bin/zig"
    fi
    log "zig: $("$CACHE/bin/zig" version)"

    # cargo-zigbuild wires zig in as the linker and translates the
    # `<triple>.<glibc>` suffix into the right zig target.
    if [[ ! -x "$CACHE/bin/cargo-zigbuild" ]]; then
        log "installing cargo-zigbuild into $CACHE"
        cargo install cargo-zigbuild --root "$CACHE" --quiet
    fi
    log "cargo-zigbuild: present"

    # --- amd64 ALSA sysroot, for cpal/alsa-sys in the GUI binaries ---------
    #
    # Pinned to `noble` rather than tracking the host release: any recent
    # libasound exports the symbols cpal needs, and linking against an OLDER
    # one is the safe direction (a binary linked against a newer libasound can
    # reference a symbol the user's older runtime lacks; the reverse is fine).
    # `noble` stays in the archive indefinitely, so this does not rot.
    #
    # Versions are resolved from the suite index instead of hardcoded: Ubuntu
    # drops superseded .debs from the pool, so a literal filename 404s the
    # first time a security update lands.
    ALSA_SUITE="noble"
    ALSA_MIRROR="http://archive.ubuntu.com/ubuntu"
    ALSA_SYSROOT="$CACHE/sysroot-amd64"
    ALSA_PC="$ALSA_SYSROOT/usr/lib/x86_64-linux-gnu/pkgconfig/alsa.pc"

    if [[ -f "$ALSA_PC" ]]; then
        log "amd64 ALSA sysroot already present: $ALSA_SYSROOT"
        exit 0
    fi

    for tool in curl gunzip dpkg; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            printf '[setup-cross] ERROR: %s is required to build the ALSA sysroot\n' \
                "$tool" >&2
            exit 1
        fi
    done

    log "building amd64 ALSA sysroot in $ALSA_SYSROOT"
    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064  # expand $tmp now, not at trap time
    trap "rm -rf '$tmp'" EXIT

    # -updates first so its stanza wins the "first match" below; plain noble
    # is the fallback for a package that has never been updated.
    idx="$tmp/Packages"
    : > "$idx"
    for suite in "${ALSA_SUITE}-updates" "$ALSA_SUITE"; do
        curl -fsSL "$ALSA_MIRROR/dists/$suite/main/binary-amd64/Packages.gz" \
            | gunzip -c >> "$idx" || true
    done
    if [[ ! -s "$idx" ]]; then
        printf '[setup-cross] ERROR: could not fetch the %s amd64 package index\n' \
            "$ALSA_SUITE" >&2
        exit 1
    fi

    for pkg in libasound2-dev libasound2t64; do
        fn="$(awk -v p="$pkg" '
            /^Package: /   { cur = $2 }
            /^Filename: /  { if (cur == p) { print $2; exit } }
        ' "$idx")"
        if [[ -z "$fn" ]]; then
            printf '[setup-cross] ERROR: %s not found in the %s amd64 index\n' \
                "$pkg" "$ALSA_SUITE" >&2
            exit 1
        fi
        log "fetching $(basename "$fn")"
        curl -fsSL -o "$tmp/$(basename "$fn")" "$ALSA_MIRROR/$fn"
    done

    mkdir -p "$ALSA_SYSROOT"
    for d in "$tmp"/*.deb; do dpkg -x "$d" "$ALSA_SYSROOT"; done

    if [[ ! -f "$ALSA_PC" ]]; then
        printf '[setup-cross] ERROR: sysroot built but %s is missing\n' "$ALSA_PC" >&2
        exit 1
    fi
    log "amd64 ALSA sysroot ready: $ALSA_SYSROOT"
    exit 0
fi

log "no cross setup rule for $TARGET - assuming the default toolchain suffices"
