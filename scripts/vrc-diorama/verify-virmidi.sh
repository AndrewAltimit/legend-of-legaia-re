#!/usr/bin/env bash
#
# Verify the Linux MIDI transport end-to-end WITHOUT VRChat: send CC messages
# through the Lua encoder + sink to the snd-virmidi rawmidi device, and confirm
# they arrive on the matching ALSA sequencer port (read by aseqdump). Proves the
# sink + virmidi loopback independent of Wine/Proton.
#
# Prereq: run setup-virmidi.sh first (loads snd-virmidi).
# Usage:  scripts/vrc-diorama/verify-virmidi.sh
#         (or LEGAIA_MIDI_DEVICE=/dev/snd/midiCxDy scripts/vrc-diorama/verify-virmidi.sh)

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

if [[ ! -d /sys/module/snd_virmidi ]]; then
    echo "[verify] snd-virmidi not loaded -- run scripts/vrc-diorama/setup-virmidi.sh first" >&2
    exit 1
fi
for t in amidi aconnect aseqdump luajit; do
    command -v "$t" >/dev/null 2>&1 || { echo "[verify] missing tool: $t" >&2; exit 1; }
done

# Resolve the rawmidi device node.
DEV="${LEGAIA_MIDI_DEVICE:-}"
if [[ -z "$DEV" ]]; then
    hw=$(amidi -l | awk '/Virtual Raw MIDI/{print $2; exit}')
    [[ -n "$hw" ]] || { echo "[verify] no Virtual Raw MIDI in amidi -l" >&2; exit 1; }
    cd_pair=${hw#hw:}
    DEV="/dev/snd/midiC${cd_pair%,*}D${cd_pair#*,}"
fi

# Resolve the seq client number of the virmidi port (port 0).
client=$(aconnect -l | grep -i 'Virtual Raw MIDI' | grep -oP 'client \K[0-9]+' | head -1 || true)
[[ -n "$client" ]] || { echo "[verify] could not find Virtual Raw MIDI seq client" >&2; exit 1; }
PORT="${client}:0"
echo "[verify] device=$DEV  seq=$PORT"

dump=$(mktemp)
aseqdump -p "$PORT" > "$dump" 2>&1 &
DPID=$!
# give aseqdump a moment to subscribe
sleep 1

LEGAIA_MIDI_DEVICE="$DEV" luajit "$REPO_ROOT/scripts/vrc-diorama/_send_test.lua"
sleep 1

kill "$DPID" 2>/dev/null || true
wait "$DPID" 2>/dev/null || true

echo "----- aseqdump -----"
cat "$dump"
echo "--------------------"
if grep -qi 'Control change' "$dump"; then
    echo "[verify] PASS: Control-Change received on $PORT"
    rc=0
else
    echo "[verify] FAIL: no Control-Change seen on $PORT" >&2
    rc=1
fi
rm -f "$dump"
exit "$rc"
