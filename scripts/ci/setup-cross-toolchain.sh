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
    exit 0
fi

log "no cross setup rule for $TARGET - assuming the default toolchain suffices"
