# VRChat battle-diorama MIDI transport

Protocol tooling for the *Legend of Legaia x VRChat live battle diorama* (a
personal design doc kept out of this repo). This is the **transport** layer
that sits on top of the shared live-probe extraction
([`scripts/pcsx-redux/lib/probe/battle_state.lua`](../pcsx-redux/lib/probe/battle_state.lua)):
it turns the typed `BattleState` into a small MIDI register stream the VRChat
world decodes. No Sony bytes live here — it is wire-protocol structure only.

```
PCSX-Redux RAM ──probe.battle_state.read()──▶ BattleState ──midi_encoder──▶ CC msgs ──▶ sink
   (extraction, transport-free)                            (this dir)                  (MIDI port / log)
```

The extraction is shared with the sibling spectator-viewport PRD; this register
map is a strict **subset** of that viewport's `BattleState` (ids + scalars, no
transforms — the diorama derives placement from baked formations). Keep
extraction and transport separate so neither delivery target forks the probe.

## Files

| File | Role |
|---|---|
| `register_schema.toml` | **Single source of truth** for the register protocol (channels, cc numbers, wide pairs, flag/status bits, phase values). |
| `codegen.py` | Emits both sides of the wire from the schema. `--check` fails on drift (wired into the pre-commit hook). |
| `generated/registers.lua` | Generated register map consumed by the encoder. **Do not edit.** |
| `world-project/Assets/LegaiaDiorama/Registers.cs` | Generated UdonSharp constants (Unity-side home). **Do not edit.** |
| `midi_encoder.lua` | `BattleState` → MIDI CC messages. Stateful per instance (tracks last-sent values for delta emission); pure, no PCSX dependency. |
| `midi_sink.lua` | Pluggable byte sinks: `rawmidi` (ALSA snd-virmidi device write, `LEGAIA_MIDI_DEVICE`), `winmm` (Windows MIDI port via LuaJIT FFI, `LEGAIA_MIDI_WINPORT`), and `null` (dry run, when neither is set). `WINPORT` wins if both are set; a sink that fails to open falls back to `null` rather than aborting the run. |
| `setup-virmidi.sh` | One-time `snd-virmidi` setup; discovers + prints the device path and `--midi=` port name (PRD M0, Linux). |
| `verify-virmidi.sh` | End-to-end Linux check (no VRChat): sink → virmidi → `aseqdump`. |
| `test_midi_encoder.lua` / `test_midi_sink.lua` / `test_roundtrip.lua` | Offline validation (run with `luajit`). `test_roundtrip.lua` proves encode→decode is lossless. |
| `_send_test.lua` | Helper used by `verify-virmidi.sh` to emit known CCs through the encoder + sink. |
| `world-project/` | Drop-in VRChat world assets (`Assets/LegaiaDiorama/`): the UdonSharp `MidiDebugMonitor.cs` (M0 raw monitor) + `BattleStateDecoder.cs` (schema-driven decoder) + generated `Registers.cs`, with `.meta` GUIDs, a VPM manifest reference, and the Windows VCC setup guide. See `world-project/README.md`. |
| `../pcsx-redux/autorun_battle_midi_stream.lua` | The live relay: probe → encoder → sink, driven per VSync. |
| `../pcsx-redux/run_probe.ps1` | Windows-native runner for the relay (defaults to the `autorun_battle_midi_stream.lua` Lua); sets `LEGAIA_MIDI_WINPORT` so the `winmm` sink drives a Windows MIDI port. |

## Protocol (summary)

A MIDI Control-Change carries `(channel 0..15, cc 0..127, value 0..127)`.

- **channel = address space**: channel 15 = meta (battle-wide); channels 0..2 =
  party slots (Vahn/Noa/Gala); channels 3..7 = enemy slots (formation order).
  These map 1:1 to the probe's `BattleState` actor slots.
- **cc = register**, **value = 7-bit payload**.
- **Wide registers** (HP, max-HP, id, region) are an `(hi, lo)` cc pair carrying
  a 14-bit value, **MSB first**.
- **commit (cc 0x7F)**: the decoder latches a channel's pending registers only
  when its commit is written, so a multi-register update applies atomically (no
  torn reads). Ascending-cc emission already orders hi < lo < commit.

The encoder emits a **full sweep** (every register + commits) on battle-enter and
periodically, and **deltas** (only changed registers + commits on touched
channels) otherwise — so a late or dropped consumer self-recovers. A worst-case
full sweep is ~100 CC events (one frame of a virtual port's throughput).

See the schema file for the per-register documentation.

## Workflow

```bash
# 1. Edit register_schema.toml, then regenerate both sides:
python3 scripts/vrc-diorama/codegen.py
#    (pre-commit fails if generated/ is stale: codegen.py --check)

# 2. Validate offline (no emulator):
luajit scripts/vrc-diorama/test_midi_encoder.lua
luajit scripts/vrc-diorama/test_midi_sink.lua

# 3. Run the live relay against a battle (interpreter mode -- the recompiler
#    diverges on interpreter-authored save states). With neither
#    LEGAIA_MIDI_DEVICE nor LEGAIA_MIDI_WINPORT set it is a dry run (null
#    sink) that still writes the CC text log:
bash scripts/pcsx-redux/run_probe.sh \
  --scenario party_basic_attack_vs_gobu_gobu \
  --lua scripts/pcsx-redux/autorun_battle_midi_stream.lua
```

## MIDI transport (PRD M0, Linux)

The transport is an ALSA `snd-virmidi` virtual port. The Lua sink writes raw
3-byte CC messages to the virmidi rawmidi device node (`/dev/snd/midiC<x>D<y>`);
`snd-virmidi` loops them onto an ALSA sequencer port that Wine/Proton exposes to
VRChat as a MIDI input. This needs no FFI/libasound binding -- it is a file
write, which works inside PCSX-Redux's LuaJIT sandbox.

```bash
# one-time (loads + persists snd-virmidi; prints the device path + port name):
scripts/vrc-diorama/setup-virmidi.sh

# prove the Linux side end-to-end, no VRChat (sink -> virmidi -> aseqdump):
scripts/vrc-diorama/verify-virmidi.sh

# then point the relay at the device and run a battle:
export LEGAIA_MIDI_DEVICE=/dev/snd/midiC<x>D<y>    # from setup output
bash scripts/pcsx-redux/run_probe.sh \
  --scenario party_basic_attack_vs_gobu_gobu \
  --lua scripts/pcsx-redux/autorun_battle_midi_stream.lua

# launch VRChat (Proton) with a partial, case-insensitive name match:
#   --midi="Virtual Raw MIDI"
```

Requirements: membership in the `audio` group (so `/dev/snd/midiC*` is writable
without root) and `alsa-utils` (`amidi`/`aconnect`/`aseqdump`). If MIDI-under-
Proton turns out flaky, the fallback is the pixel-strip transport (PRD M7),
which reuses this same encoder output.

## What is proven, and what isn't

The Linux half of the transport is verified end-to-end: `verify-virmidi.sh`
passes, with the Lua encoder + sink emitting CCs that arrive on the ALSA
sequencer port (`aseqdump` shows `Control change Ch 0, controller 4, value
100`). One trap that cost real time: the rawmidi node must be opened `"wb"`,
not `"ab"` — `O_APPEND` is `EINVAL` on the char device.

The protocol is verified lossless. `test_roundtrip.lua` encodes synthetic
`BattleState`s and decodes them through a mirror of the UdonSharp decoder,
covering 14-bit wide values and late-joiner full-sweep reconstruction.

**The unproven hop is VRChat itself** (PRD M0): building the test world from
`world-project/` and confirming VRChat under Proton receives the virmidi port
via `--midi="Virtual Raw MIDI"`. Nothing here demonstrates that MIDI survives
Proton. If it turns out flaky, the fallback is the pixel-strip transport (PRD
M7), which reuses this same encoder output unchanged.

Some registers are not sourced yet, and the encoder emits a safe default for
them: party character-id (as opposed to enemy monster-id), the per-actor
`targeted` flag + `target_slot`, a fuller status-bit decode, and `region_id`.

The `world-project/` tree is a temporary home — it moves into the standalone
Unity world project (`legaia-vrc-world`, PRD Q1) once that exists.
