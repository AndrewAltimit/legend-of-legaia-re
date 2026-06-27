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
| `midi_sink.lua` | Pluggable byte sinks: `rawmidi` (ALSA snd-virmidi device write) and `null` (dry run). Chosen from `LEGAIA_MIDI_DEVICE`. |
| `setup-virmidi.sh` | One-time `snd-virmidi` setup; discovers + prints the device path and `--midi=` port name (PRD M0, Linux). |
| `verify-virmidi.sh` | End-to-end Linux check (no VRChat): sink → virmidi → `aseqdump`. |
| `test_midi_encoder.lua` / `test_midi_sink.lua` / `test_roundtrip.lua` | Offline validation (run with `luajit`). `test_roundtrip.lua` proves encode→decode is lossless. |
| `_send_test.lua` | Helper used by `verify-virmidi.sh` to emit known CCs through the encoder + sink. |
| `world-project/` | Drop-in VRChat world assets (`Assets/LegaiaDiorama/`): the UdonSharp `MidiDebugMonitor.cs` (M0 raw monitor) + `BattleStateDecoder.cs` (schema-driven decoder) + generated `Registers.cs`, with `.meta` GUIDs, a VPM manifest reference, and the Windows VCC setup guide. See `world-project/README.md`. |
| `../pcsx-redux/autorun_battle_midi_stream.lua` | The live relay: probe → encoder → sink, driven per VSync. |

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
#    diverges on interpreter-authored save states). Without LEGAIA_MIDI_DEVICE
#    it is a dry run (null sink) that still writes the CC text log:
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

## Status / next steps

- **Done:** schema, codegen (Lua + C#), encoder (delta + full sweep, MSB/commit
  latching), offline tests, live relay wiring, the ALSA `snd-virmidi` sink +
  setup/verify scripts, and the UdonSharp decoder + drop-in world scaffold (`world-project/`).
- **Linux transport VERIFIED:** `verify-virmidi.sh` passes end-to-end — the Lua
  encoder + sink emit CCs that arrive on the ALSA seq port (`aseqdump` shows
  `Control change Ch 0, controller 4, value 100`). The rawmidi node must be
  opened `"wb"` (not `"ab"`: `O_APPEND` is `EINVAL` on the char device).
- **Protocol VERIFIED lossless:** `test_roundtrip.lua` encodes synthetic
  BattleStates and decodes them with a mirror of the UdonSharp decoder (14-bit
  wide values + late-joiner full-sweep reconstruction included).
- **M0 remaining (the only unproven hop):** build the test world from `world-project/`
  and confirm VRChat under Proton receives the virmidi port via
  `--midi="Virtual Raw MIDI"`. If flaky, fall back to the pixel-strip transport
  (M7), which reuses this same encoder output.
- **Decoder home:** when the Unity world project exists (`legaia-vrc-world`,
  PRD Q1), the `world-project/` tree moves into it.
- **Open register TODOs** (encoder emits a safe default today): party
  character-id (vs enemy monster-id), the per-actor `targeted` flag +
  `target_slot`, a fuller status-bit decode, and `region_id` sourcing.
