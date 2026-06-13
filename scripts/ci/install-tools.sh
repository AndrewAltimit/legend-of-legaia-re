#!/usr/bin/env bash
#
# Install dynamic-analysis tooling for the project. Intended for Ubuntu /
# Debian-family systems.
#
# Run as a normal user; the script invokes `sudo` only for the apt step.
# It does NOT touch any project files; safe to re-run.
#
# Architecture handling:
#   * x86_64    -> downloads the official PCSX-Redux AppImage (Lua scripting,
#                  built-in debugger, GDB stub).
#   * aarch64   -> PCSX-Redux upstream does NOT ship arm64 AppImages, so we
#                  install Mednafen from apt (also offers a memory editor +
#                  freeze states; less ergonomic than PCSX-Redux but works).
#                  You can alternatively build PCSX-Redux from source -- see
#                  the manual instructions printed at the end.
#
# What it intentionally does NOT do:
#   - Install or download a PSX BIOS. Both PCSX-Redux and Mednafen need one
#     for boot but we don't redistribute Sony IP. Provide your own
#     (SCPH-1001/7001/etc.).
#
# Usage:
#   bash scripts/ci/install-tools.sh
#
# After this completes, see docs/tooling/overlay-capture.md for the
# overlay capture pipeline.

set -euo pipefail

# ---------- helpers ----------
log()  { printf '\033[1;36m[install]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn   ]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[ERROR  ]\033[0m %s\n' "$*" >&2; exit 1; }

if [[ $EUID -eq 0 ]]; then
    die "do NOT run as root. Run as your normal user; the script uses sudo only when needed."
fi

if ! command -v sudo >/dev/null 2>&1; then
    die "sudo is required (used for the apt-get step)."
fi

ARCH=$(uname -m)
log "Detected architecture: $ARCH"

# ---------- 1. apt deps ----------
# `apt-get update` can fail when third-party PPAs are unreachable (e.g.
# mozillateam PPA timing out). Those failures don't prevent installing
# packages from the main archive, so tolerate the non-zero exit.
log "Refreshing apt cache (transient PPA failures are tolerated)"
sudo apt-get update -o Acquire::Retries=1 || warn "apt-get update had issues; continuing with cached lists"

# Install only what we need from the main Ubuntu archive. -y to avoid
# prompts; --no-install-recommends to keep the install lean.
COMMON_PKGS=(jq curl ca-certificates)
if [[ "$ARCH" == "x86_64" ]]; then
    # libfuse2 needed to run AppImages on Ubuntu 24.04+. The package is
    # libfuse2t64 on noble and libfuse2 on older releases.
    if apt-cache show libfuse2t64 >/dev/null 2>&1; then
        APT_PKGS=("${COMMON_PKGS[@]}" libfuse2t64)
    else
        APT_PKGS=("${COMMON_PKGS[@]}" libfuse2)
    fi
else
    # arm64 / aarch64: install mednafen as the dynamic-analysis fallback.
    APT_PKGS=("${COMMON_PKGS[@]}" mednafen)
fi

log "Installing apt packages: ${APT_PKGS[*]}"
# `--fix-missing` so a single missing package (which shouldn't happen with
# the lean set above, but guards against future additions) doesn't abort.
sudo apt-get install -y --no-install-recommends --fix-missing "${APT_PKGS[@]}"

# ---------- 2. PCSX-Redux AppImage (x86_64 only) ----------
if [[ "$ARCH" == "x86_64" ]]; then
    APP_DIR="$HOME/Applications"
    APP_PATH="$APP_DIR/PCSX-Redux.AppImage"
    mkdir -p "$APP_DIR"

    log "Querying GitHub for latest PCSX-Redux AppImage"
    RELEASES_API="https://api.github.com/repos/grumpycoders/pcsx-redux/releases?per_page=10"
    RELEASE_JSON=$(curl -fsSL "$RELEASES_API" || true)

    APPIMAGE_URL=""
    if [[ -n "$RELEASE_JSON" ]]; then
        APPIMAGE_URL=$(echo "$RELEASE_JSON" | jq -r '
            .[]
            | .assets[]?
            | select(.name | test("(?i)x86_64.*\\.appimage$"))
            | .browser_download_url
        ' | head -n1)
    fi

    if [[ -z "${APPIMAGE_URL:-}" || "$APPIMAGE_URL" == "null" ]]; then
        warn "no x86_64 AppImage found via the GitHub API"
        warn "manual download: https://github.com/grumpycoders/pcsx-redux/releases"
        die  "skipping AppImage install"
    fi

    log "Downloading $(basename "$APPIMAGE_URL")"
    log "  -> $APP_PATH"
    if ! curl -fL --progress-bar "$APPIMAGE_URL" -o "$APP_PATH.tmp"; then
        die "download failed"
    fi
    mv "$APP_PATH.tmp" "$APP_PATH"
    chmod +x "$APP_PATH"

    log "Sanity check: $APP_PATH --version"
    if ! "$APP_PATH" --version 2>/dev/null | head -n1; then
        warn "couldn't run --version; the AppImage is in place but may need manual verification"
    fi

    cat <<EOF

============================================================
PCSX-Redux installed at:
  $APP_PATH

Next steps:
  1. Launch:  $APP_PATH
  2. On first run, point it at a PSX BIOS (SCPH-1001/7001/etc).
     We don't redistribute one; provide your own.
  3. File -> Open ISO -> select your Legend of Legaia .bin
  4. Press F5 to start
  5. Once booted past the title screen, open the Lua console:
       File -> Show Lua Console
     Then 'Lua -> Run Lua File' and pick:
       ghidra/scripts/dump_overlay.lua
     This dumps 0x801C0000-0x801EFFFF to /tmp/.
  6. Import the dump into Ghidra:
       ghidra/scripts/import_overlay.sh /tmp/legaia_overlay_<TS>.bin

Full protocol: docs/tooling/overlay-capture.md
============================================================
EOF

else
    cat <<EOF

============================================================
PCSX-Redux upstream does NOT ship an AppImage for $ARCH, so we
installed Mednafen instead (apt: mednafen). It boots PSX discs
and exposes a memory viewer + freeze states sufficient for the
overlay-capture goal described in docs/tooling/overlay-capture.md.

Next steps with Mednafen:
  1. Launch:  mednafen
  2. Provide a PSX BIOS at \$HOME/.mednafen/firmware/scph7001.bin
     (or whatever your region's BIOS is; see Mednafen docs).
     We don't redistribute Sony IP.
  3. From a terminal, run:
       mednafen /path/to/Legend_of_Legaia.bin
  4. Once booted past the title screen, hit F5 to save state to
     a slot. State files land in \$HOME/.mednafen/mcs/ and
     contain the full PSX RAM in a documented header layout.
  5. Extract RAM bytes 0x1C0000-0x1EFFFF from the save state to
     produce the overlay dump. (We don't have a one-shot script
     for this yet; manual hex-extract works -- the state format
     is documented in the Mednafen source.)

Alternatively, build PCSX-Redux from source for full Lua-script
support on arm64:
  sudo apt-get install -y build-essential clang cmake pkg-config \\
      libcurl4-openssl-dev libfreetype6-dev libssl-dev libuv1-dev \\
      libavformat-dev libavutil-dev libswresample-dev libavcodec-dev \\
      git python3 python3-distutils
  git clone https://github.com/grumpycoders/pcsx-redux.git
  cd pcsx-redux
  make BUILD=Release \$NPROC
  ./pcsx-redux

Full protocol: docs/tooling/overlay-capture.md
============================================================
EOF
fi
