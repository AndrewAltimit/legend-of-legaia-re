#!/usr/bin/env bash
# Import a 192K overlay slice as a NAMED Ghidra program (overlay_<label>.bin).
# Unlike analyze-overlay.sh which always uses overlay.bin, this preserves
# captures across runs.
#
# Usage:
#   scripts/ghidra-analysis/import-overlay-named.sh /tmp/legaia_overlay_menu.bin menu
set -euo pipefail
DUMP="$1"
LABEL="$2"
PROG_NAME="overlay_${LABEL}.bin"
docker compose cp "$DUMP" "ghidra:/tmp/${PROG_NAME}"
docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
    /projects legaia \
    -import "/tmp/${PROG_NAME}" \
    -loader BinaryLoader \
    -loader-baseAddr 0x801C0000 \
    -processor MIPS:LE:32:default \
    -overwrite
docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process "${PROG_NAME}"
echo "imported as ${PROG_NAME}"
