#!/usr/bin/env bash
# Import + analyze a PCSX-Redux RAM dump into the Ghidra project as a Raw
# Binary at base 0x801C0000 (the overlay code window). Run from the repo
# root after producing a dump via ghidra/scripts/dump_overlay.lua.
#
# Usage:
#   ghidra/scripts/import_overlay.sh /path/to/legaia_overlay_TIMESTAMP.bin
#
# The dump file is copied into the running Ghidra container, imported as a
# new program named `overlay.bin`, then analyzed. Cross-program function
# references (call from SCUS_942.54 -> overlay) require a multi-program
# project view; open the Ghidra GUI and load both programs side-by-side.

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <path-to-overlay-dump.bin>" >&2
    exit 64
fi

DUMP_PATH="$1"
if [[ ! -f "$DUMP_PATH" ]]; then
    echo "no such file: $DUMP_PATH" >&2
    exit 66
fi

if ! docker compose ps ghidra | grep -q Up; then
    echo "ghidra container isn't running; bring it up first:" >&2
    echo "  docker compose up -d ghidra" >&2
    exit 1
fi

# Copy into the container's /data directory (which the volume mounts read-only
# from ./extracted, so we use docker cp directly to a writable location).
docker compose cp "$DUMP_PATH" ghidra:/tmp/overlay.bin

# Import as Raw Binary at the overlay base address. -overwrite lets you
# re-import after a fresh capture without manually deleting the program.
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia \
    -import /tmp/overlay.bin \
    -loader BinaryLoader \
    -loader-baseAddr 0x801C0000 \
    -processor MIPS:LE:32:default \
    -overwrite

# Run analysis on the imported program. May take a few minutes on a 192 KB
# blob since Ghidra disassembles + decompiles every reachable function.
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process overlay.bin

# Normalize ownership of any output files the container produced as root.
docker compose exec ghidra chmod -R a+r /scripts/funcs 2>/dev/null || true

echo
echo "Overlay imported. To dump specific overlay functions, add their entry"
echo "addresses to ghidra/scripts/dump_funcs.py TARGETS and run:"
echo "  docker compose exec ghidra /ghidra/support/analyzeHeadless \\"
echo "      /projects legaia -process overlay.bin -noanalysis \\"
echo "      -postScript /scripts/dump_funcs.py"
