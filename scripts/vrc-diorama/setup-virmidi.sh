#!/usr/bin/env bash
#
# One-time setup of an ALSA snd-virmidi virtual MIDI port for the battle-diorama
# transport (PRD milestone M0, Linux side). Creates a rawmidi device the Lua
# MIDI sink writes to, which snd-virmidi loops onto an ALSA sequencer port that
# Wine/Proton exposes to VRChat as a MIDI input.
#
# What it does (idempotent):
#   1. modprobe snd-virmidi (needs sudo) if not already loaded.
#   2. persist it across reboots (/etc/modules-load.d + /etc/modprobe.d).
#   3. discover + print the rawmidi device path (-> LEGAIA_MIDI_DEVICE) and the
#      ALSA seq port name (-> VRChat --midi= argument).
#
# Usage:  scripts/vrc-diorama/setup-virmidi.sh
# Then:   export LEGAIA_MIDI_DEVICE=<printed path>   # for the relay
#         launch VRChat with  --midi="<printed port name>"
#         verify with         scripts/vrc-diorama/verify-virmidi.sh

set -euo pipefail

MIDI_DEVS="${LEGAIA_VIRMIDI_DEVS:-1}"

note() { printf '[setup-virmidi] %s\n' "$*"; }
warn() { printf '[setup-virmidi] WARNING: %s\n' "$*" >&2; }

# ---- 1. load the module ----
if [[ -d /sys/module/snd_virmidi ]]; then
    note "snd-virmidi already loaded"
else
    note "loading snd-virmidi (midi_devs=$MIDI_DEVS) -- sudo required"
    sudo modprobe snd-virmidi midi_devs="$MIDI_DEVS"
    note "loaded"
fi

# ---- 2. persist across reboots ----
load_conf=/etc/modules-load.d/snd-virmidi.conf
opts_conf=/etc/modprobe.d/snd-virmidi.conf
if [[ ! -f "$load_conf" ]]; then
    note "persisting auto-load -> $load_conf (sudo)"
    echo "snd-virmidi" | sudo tee "$load_conf" >/dev/null
fi
if [[ ! -f "$opts_conf" ]]; then
    note "persisting options -> $opts_conf (sudo)"
    echo "options snd-virmidi midi_devs=$MIDI_DEVS" | sudo tee "$opts_conf" >/dev/null
fi

# ---- 3. discover the rawmidi device + seq port ----
# amidi -l row for virmidi looks like:  "IO  hw:2,0  Virtual Raw MIDI (0)"
hw=$(amidi -l 2>/dev/null | awk '/Virtual Raw MIDI/{print $2; exit}')
if [[ -z "${hw:-}" ]]; then
    warn "no 'Virtual Raw MIDI' rawmidi found in 'amidi -l'; output was:"
    amidi -l >&2 || true
    exit 1
fi
# hw:C,D  ->  /dev/snd/midiC<C>D<D>
cd_pair=${hw#hw:}                 # "2,0"
card=${cd_pair%,*}               # "2"
dev=${cd_pair#*,}                # "0"
node="/dev/snd/midiC${card}D${dev}"

# ALSA seq port name (what Wine surfaces; VRChat --midi= does a partial,
# case-insensitive match). Print the client line for the exact spelling.
seq_line=$(aconnect -l 2>/dev/null | grep -i 'Virtual Raw MIDI' | head -1 || true)

note "rawmidi device : $node  (from $hw)"
[[ -n "$seq_line" ]] && note "seq client     : ${seq_line## }"

# ---- writability check ----
if [[ ! -e "$node" ]]; then
    warn "$node does not exist (unexpected after load)"
elif [[ -w "$node" ]]; then
    note "device is writable by you (good)"
else
    warn "$node is NOT writable by you."
    if id -nG | grep -qw audio; then
        warn "you are in 'audio' but the node is still not writable -- check udev/permissions on /dev/snd."
    else
        warn "add yourself to the 'audio' group:  sudo usermod -aG audio $USER  (then re-login)."
    fi
fi

cat <<EOF

------------------------------------------------------------------
  next steps
------------------------------------------------------------------
  relay (PCSX side):
    export LEGAIA_MIDI_DEVICE=$node

  VRChat (Proton): add a launch option, partial name match is fine:
    --midi="Virtual Raw MIDI"

  verify the Linux side end-to-end (no VRChat needed):
    LEGAIA_MIDI_DEVICE=$node scripts/vrc-diorama/verify-virmidi.sh
------------------------------------------------------------------
EOF
