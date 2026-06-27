# LegaiaDiorama -- drop-in VRChat world assets

The VRChat-side scaffold for the battle diorama. This is **not** a full Unity
project (no `ProjectSettings/`, no pinned Unity version) -- the VRChat Creator
Companion (VCC) creates that for you, correctly, for your installed Unity. This
folder is the bundle you drop **into** that project, plus the setup steps.

It pairs with the protocol tooling one level up (`../`): the schema, the Lua
encoder, the ALSA sink, and the round-trip test. The decode algorithm here is
the same one `../test_roundtrip.lua` proves lossless.

## Contents

| Path | What |
|---|---|
| `Assets/LegaiaDiorama/MidiDebugMonitor.cs` | PRD M0 gate: prints incoming MIDI to a TextMeshPro panel (no schema dep). |
| `Assets/LegaiaDiorama/BattleStateDecoder.cs` | Schema-driven decoder: register file + commit-latch + wide reconstruct → typed `BattleState` + summary. |
| `Assets/LegaiaDiorama/Registers.cs` | Generated constants (by `../codegen.py`). Do not edit. |
| `Assets/**/*.meta` | Deterministic Unity GUIDs (`gen-meta.py`; create-if-missing). |
| `vpm-manifest.reference.json` | Reference list of required VPM packages (VCC writes the live one). |
| `gen-meta.py` | Regenerate any missing `.meta`. |

## Why this machine can't host Unity/VRChat

The dev box this came from is **ARM64 Linux** -- no Unity Editor (Linux Editor is
x86_64 only) and no viable VRChat client (x86 + Proton + EAC). So the build +
client run happen on a **Windows PC**; this scaffold is the hand-off.

## Windows setup (VCC)

1. Install the **VRChat Creator Companion** and let it install Unity + Unity Hub.
2. VCC → **Create New World** project. It installs the pinned Unity version and
   adds the **VRChat Worlds SDK** (which bundles UdonSharp). Open it once so it
   compiles clean.
3. Copy this scaffold's **`Assets/LegaiaDiorama/`** (with its `.meta` files) into
   the new project's `Assets/`.
4. Import your world content: **Assets → Import Package → Custom Package…** →
   your world `.unitypackage` (e.g. a `PC.unitypackage`). A content-only package
   bundles no SDK, so no SDK conflict; if TextMeshPro prompts to import TMP
   Essentials, accept.
5. Open a scene from the imported world (or the SDK SampleScene).

## Wire the M0 monitor

1. Add a world-space **UI Canvas** with a **TextMeshProUGUI** element somewhere
   visible (e.g. on a cabin wall).
2. Create an empty GameObject `MidiMonitor`; add the **MidiDebugMonitor**
   component; drag the TMP text into its `Log` field.
3. (Optional, the real decoder) add **BattleStateDecoder** to another GameObject
   with a second TMP text in its `Summary` field.

## Prove M0 (does VRChat receive MIDI)

1. **Windows MIDI port:** install **loopMIDI** (Tobias Erichsen) and create a
   port, e.g. `LegaiaDiorama`.
2. **Launch VRChat with the port** (Steam launch options, or the SDK Build&Test
   additional-args): `--midi="LegaiaDiorama"` (partial, case-insensitive).
3. **Build & Test** from the VRChat SDK Control Panel — this launches the real
   VRChat client locally (ClientSim does **not** deliver MIDI; the built client
   does). The monitor panel should print CC events once the port is driven.
4. **Drive the port** (see transport below). The panel updates → M0 is closed.

## Transport: getting CCs from PCSX into the Windows VRChat

The encoder/relay run where PCSX runs; MIDI is **local to the VRChat client**.
Pick one:

- **A. PCSX on the same Windows PC.** Simplest. The relay needs a *Windows* MIDI
  sink (winmm `midiOutShortMsg` via LuaJIT FFI) writing to the loopMIDI port —
  the sibling of the Linux ALSA sink, not yet written. Ask for it when you go
  this route.
- **B. PCSX on the Linux box, VRChat on Windows (network MIDI).** Use **rtpMIDI**
  (Tobias Erichsen) on Windows + an RTP-MIDI endpoint on Linux (`rtpmidid` /
  `raveloxmidi`) that appears as an ALSA seq port. Connect the verified
  `snd-virmidi` output to it (`aconnect`); rtpMIDI exposes it to VRChat as
  `--midi=`. The Linux half (encoder → sink → virmidi) is already proven by
  `../verify-virmidi.sh`.
- **C. Pixel-strip (PRD M7).** No MIDI: encode the same CC stream into a few
  rows of the OBS stream the world's video player already shows, decode per
  client via `VRCAsyncGPUReadback`. Most robust for a stream-based diorama;
  reuses this exact encoder output.

For a first M0 smoke test, route **A** (everything on Windows) or even just
`loopMIDI` + a desktop MIDI sender is the quickest "panel lights up" proof.

## Notes

- MIDI input is **local-only** (events fire only on the device-owning client).
  Sharing decoded state with other players / late joiners is a separate
  manual-synced relay (PRD M5), not in these files.
- Both behaviours are `BehaviourSyncMode.None`. The register file is a flat
  `int[16*128]`; channels are iterated by fixed ranges (party 0..2, enemy 3..7).
- These files move into the standalone world repo (`legaia-vrc-world`, PRD Q1)
  when it exists.
