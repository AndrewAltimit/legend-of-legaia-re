# legend-of-legaia-re

Reverse engineering for the PSX game **Legend of Legaia** (1998, Sony, NA SCUS-94254): Ghidra-traced format documentation, Rust extractors for every asset on the disc, and a clean-room engine reimplementation targeting wgpu with optional WASM.

Two coordinated tracks under one Cargo workspace:

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with provenance back to a Ghidra function, build round-trip parsers (`.bin` → PNG / WAV / OBJ / JSON).
2. **Engine reimplementation.** Clean-room Rust port of the engine - render via wgpu, audio via the existing XA + VAB decoders, optional WASM target. Same legal model as [ScummVM](https://www.scummvm.org/), [OpenRCT2](https://github.com/openrct2/OpenRCT2), [OpenMW](https://github.com/OpenMW/openmw), [OpenLara](https://github.com/XProger/OpenLara) - bring your own disc image; the toolkit handles the rest.

The repo name `-re` is in both senses: **r**everse-**e**ngineering and **r**e-implementation.

**Project site:** [andrewaltimit.github.io/legend-of-legaia-re](https://andrewaltimit.github.io/legend-of-legaia-re/) - interactive viewers (run client-side off your own disc image), the full technical reference, and the demo video below.

## Demo

https://github.com/AndrewAltimit/legend-of-legaia-re/raw/refs/heads/main/site/assets/legend-of-legaia-re-demo.mp4

The clean-room engine booting a real scene, plus the asset viewers. ([direct link](site/assets/legend-of-legaia-re-demo.mp4) · [on the project site](https://andrewaltimit.github.io/legend-of-legaia-re/))

**Status:** local research project. Don't expect API stability.

**License:** dual-licensed at your option under either the [Unlicense](LICENSE) (public-domain dedication) or the [MIT License](LICENSE-MIT). Apache-2.0 is intentionally not offered - this project is meant to be as close to public domain as the law in your jurisdiction allows, with no patent-retaliation strings attached: copy it, fork it, sell it, patent improvements on it, just don't stop anyone else from doing the same. These licenses apply *only* to the code and documentation in this repository. **Sony's IP - game executable, asset data, ROM contents - is not redistributed and is not covered by these licenses.** You bring your own disc image. The `extracted/` and `ghidra/projects/` directories are gitignored. CI runs without disc data.

## Documentation

The committed docs under `docs/` are organised topic-first as a technical reference:

- **[`docs/overview.md`](docs/overview.md)** - elevator pitch + how the layers stack.
- **[`docs/formats/`](docs/formats/overview.md)** - per-format byte-level specs (PROT, LZS, TIM, TMD, VAB, MES, ANM, MDT, scene bundles, effect, overlays, …).
- **[`docs/subsystems/`](docs/subsystems/)** - how the engine works: [boot](docs/subsystems/boot.md), [asset loader](docs/subsystems/asset-loader.md), [script VM](docs/subsystems/script-vm.md), [actor VM](docs/subsystems/actor-vm.md), [effect VM](docs/subsystems/effect-vm.md), [move VM](docs/subsystems/move-vm.md), [motion VM](docs/subsystems/motion-vm.md), [renderer](docs/subsystems/renderer.md), [audio](docs/subsystems/audio.md), [cutscene](docs/subsystems/cutscene.md), [battle](docs/subsystems/battle.md), [battle action SM](docs/subsystems/battle-action.md), [battle formulas](docs/subsystems/battle-formulas.md), [engine reimplementation](docs/subsystems/engine.md).
- **[`docs/tooling/`](docs/tooling/)** - how to use the repo: [extraction CLIs](docs/tooling/extraction.md), [Ghidra setup](docs/tooling/ghidra.md), [overlay capture](docs/tooling/overlay-capture.md), [mednafen automation](docs/tooling/mednafen-automation.md), [PCSX-Redux automation](docs/tooling/pcsx-redux-automation.md), [port catalog](docs/tooling/port-catalog.md) (per-function dumped × documented × ported × ignored status with BFS-from-roots feature views).
- **[`docs/reference/`](docs/reference/)** - [key Ghidra-traced functions](docs/reference/functions.md), [RAM map + globals](docs/reference/memory-map.md), [TCRF region data](docs/reference/builds.md), [open RE threads](docs/reference/open-rev-eng-threads.md) (still-open hunts + falsified hypotheses worth not re-walking).

For workspace conventions and format gotchas (especially MIPS LUI+ADDIU pairs), read [`CLAUDE.md`](CLAUDE.md) first.

## Quick start

### Prerequisites

- Rust toolchain (`cargo`, edition 2024).
- The Legend of Legaia (USA) disc image as `.bin` + `.cue` (Mode2/2352).
- (Optional) Docker + docker-compose for headless Ghidra runs.
- (Optional) mednafen + a save state at the target scene, for runtime overlay capture.

### Build

```bash
cargo build --release
```

Binaries land in `target/release/`. Run `<binary> --help` for full subcommand listings. (Note: `legaia-engine` is the binary name; the *package* is `legaia-engine-shell`, so `cargo build -p legaia-engine-shell` builds just that crate.)

If you plan to commit, run the hook installer once - it points `core.hooksPath` at `scripts/git-hooks/` so `cargo fmt --check` and `cargo clippy -D warnings` run before each commit (matching CI). The hook auto-skips when no Rust files are staged.

```bash
scripts/install-hooks.sh
```

### Run the whole pipeline

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

Verify → disc → PROT → categorize → streaming sub-asset extract → TIM → PNG. Use `--skip-png` to skip the slowest step or `--skip-verify` to skip the SHA-256 hash. Pass `-v` for per-file output.

### Per-stage CLIs

For driving each stage individually, see [`docs/tooling/extraction.md`](docs/tooling/extraction.md). Verifying the disc image:

```bash
./target/release/disc-extract verify "/path/to/Legend of Legaia (USA).bin"
```

| Disc | SHA-256 (Mode2/2352 .bin) |
|---|---|
| Legend of Legaia (USA), SCUS-94254 | `e6120a5d70716dd2f026a2da32d0171d52651971b52c4347a68541299f75258c` |

For canonical per-track verification, cross-check against [Redump](http://redump.org/disc/425/).

### Browse the assets

After running the pipeline:

```bash
# 3D mesh + textures
./target/release/asset-viewer tmd extracted/tmd_scan/0866_battle_data \
    --shape character --sort-by-size --bundle battle

# A VAB sample
./target/release/asset-viewer vab extracted/PROT/0865_battle_data.BIN --offset 0x... --sample 0

# PROT entry browser
./target/release/asset-viewer prot extracted/PROT.DAT --cdname extracted/CDNAME.TXT

# Headless engine driver - boots a CDNAME scene straight off PROT bytes
# (no `tim_scan/` or `tmd_scan/` filesystem intermediate). Prints what the
# scene-host resolved: TIMs uploaded to VRAM, TMDs parsed, MES presence,
# SEQ / VAB / event-script counts.
./target/release/legaia-engine info --scene town01
./target/release/legaia-engine list-scenes

# Run the engine for N frames against a scene - ticks the World, drives
# the camera, drains BGM events into the audio director (if available),
# logs scene transitions. Headless smoke check that the boot-loop wiring
# (engine-shell::BootSession) actually moves state forward.
./target/release/legaia-engine play --scene town01 --frames 600 --no-audio

# Open a windowed wgpu session rendering scene TMDs + HUD; accepts keyboard
# input; exits cleanly on window close. 60 Hz fixed tick, uncapped render.
./target/release/legaia-engine play-window --scene town01

# Decode a raw PSX STR file (MDEC video) and play it back in a window with
# synced XA audio.
./target/release/legaia-engine play-str /path/to/cutscene.str

# Edit input key bindings (persisted to TOML via engine-core::input::Mapping)
./target/release/legaia-engine config set --binding cross=Z

# Save / load the world's empty default party to a slot file. Engines
# drive the same flow at runtime through `engine-core::menu_runtime`.
./target/release/legaia-engine save --slot 0 --save-dir saves
./target/release/legaia-engine load --slot 0 --save-dir saves

# Field scene runner - drives the field-VM against a real CDNAME scene's
# event-script records, with dialog rendering wired into the same window
./target/release/asset-viewer field town01

# Battle scene driver - boots the battle bundle, ticks the battle-action
# state machine, shows action state + per-slot liveness in the HUD
./target/release/asset-viewer battle-scene --queued-action 3

# SEQ playback - drives the SsAPI-shape sequencer + a VAB through cpal,
# producing live audio
./target/release/asset-viewer seq path/to.seq path/to.vab

# Standalone MES dialog viewer - typewriter-paced text rendering through
# the extracted dialog font
./target/release/asset-viewer dialog path/to.mes

# ANM keyframe inspector - per-record header + per-bone keyframe table
./target/release/anm keyframes path/to.anm --record 0

# Field-pack slot clusters - group the 97 schema slots by size to surface
# semantic record kinds (5 × 0x2088 = the scene's TIM blobs, 21 × 0x218 =
# the NPC-slot array, etc.)
./target/release/asset field-pack extracted/PROT/0005_town01.BIN --groups

# PSX memory-card reader - list active save blocks, parse a character
# record, JSON-dump a five-slot party
./target/release/save-tool dir ~/.mednafen/sav/Legend*.0.mcr
./target/release/save-tool roundtrip /path/to/character.bin
```

### Static analysis (Ghidra in Docker)

```bash
docker compose build ghidra        # one-time, sets UID/GID matching the host user
docker compose up -d ghidra
docker compose exec ghidra /ghidra/support/analyzeHeadless \
    /projects legaia -process SCUS_942.54 \
    -noanalysis -postScript find_streaming_consumers.py
```

Per-function decompile + disassembly dumps land in `ghidra/scripts/funcs/<addr>.txt`. See [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md) for the full script catalogue and gotchas.

### Capture & analyze a runtime overlay

Most game logic (field/battle/menu state machines, dialog renderer, debug-flag writers) lives in RAM overlays loaded at `0x801C0000+`, **not** in `SCUS_942.54`. Save state at the target scene in mednafen and run:

```bash
scripts/analyze-overlay.sh \
    ~/.mednafen/mcs/Legend*Legaia*.mcN \
    --label level_up
```

The pipeline decompresses the gzipped save state, slices out the overlay window, re-imports it into Ghidra, and emits a CSV of every `jal` to a known SCUS asset loader with the const-tracked argument. See [`docs/tooling/overlay-capture.md`](docs/tooling/overlay-capture.md).

### Disc-gated tests

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace --release
```

Several integration tests touch a real disc / extracted directory:

- `crates/iso/tests/disc_pipeline.rs` - disc walk, file count, key file SHA-256s.
- `crates/extract/tests/validation_suite.rs` - full pipeline assertions.
- `crates/engine-core/tests/scene_chain_e2e.rs` - load every CDNAME scene, walk MES + SEQ + TMD assets, validate the BGM resolver against the per-scene `block_start + 6 + id` math.
- `crates/engine-core/tests/battle_real_data_chain.rs` - locate the retail effect bundle and drive the battle SM against it.
- `crates/engine-audio/tests/real_bgm_chain.rs` - pull a real `music_01` SEQ + VAB pair through the sequencer and SPU mixer.
- `crates/save/tests/real_card_roundtrip.rs` - walk a real PSX memory-card image (mednafen `.mcr`) and verify the save-block layout.

If `LEGAIA_DISC_BIN` is unset, every disc-gated test skips and passes - that's intentional, so CI works without redistributing Sony data.

## Repository layout

```
legend-of-legaia-re/
├── Cargo.toml                    # workspace root
├── docker-compose.yml            # ghidra service (UID/GID-matched user)
├── docker/ghidra.Dockerfile      # wraps blacktop/ghidra:latest with host-UID mapping
├── crates/
│   ├── iso/                      # PSX disc reader + ISO9660 walker
│   ├── prot/                     # PROT.DAT TOC + CDNAME + standalone TIM-pack
│   ├── lzs/                      # Legaia LZS decoder (FUN_8001a55c)
│   ├── asset/                    # Asset dispatcher, streaming, scene-bundle + format detectors, per-entry categorize classifier
│   ├── tim/                      # PSX TIM parser + PNG exporter
│   ├── tmd/                      # Legaia TMD parser + primitive walker + OBJ export
│   ├── vab/                      # VAB sound bank extractor + SPU-ADPCM decoder
│   ├── xa/                       # XA-ADPCM decoder + WAV exporter
│   ├── mdt/                      # Move table (Tactical Arts) parser
│   ├── mes/                      # MES dialog container parser
│   ├── anm/                      # ANM animation container parser
│   ├── seq/                      # PsyQ SEQ parser + CLI inspector
│   ├── save/                     # Per-character record (0x414B) parse + write
│   ├── font/                     # Dialog font extraction + atlas / layout API
│   ├── extract/                  # Top-level pipeline driver
│   ├── mdec/                     # PSX MDEC clean-room decoder (BS v2 bitstream → RGBA8); STR sector assembler
│   ├── engine-core/              # World, scene host, scene resources (VRAM pre-pass), camera, menu runtime, save round-trip
│   ├── engine-render/            # winit + wgpu, software PSX VRAM emulation, text overlay
│   ├── engine-audio/             # cpal mixer + clean-room SPU + SEQ sequencer
│   ├── engine-vm/                # Actor / field / effect / move / motion VMs + battle SM + action validator + formulas
│   ├── engine-shell/             # `legaia-engine` top-level driver + BootSession + AudioBgmDirector; play-window renders shop + inn + level-up overlays
│   ├── asset-viewer/             # Combined viewer: TIM, TMD, stage, VAB, SEQ, dialog, field, battle, PROT
│   └── web-viewer/               # WASM target - disc browser running in the browser
├── docs/                         # Topic-first technical reference (see "Documentation")
├── ghidra/
│   ├── projects/                 # Ghidra project DB (gitignored)
│   └── scripts/                  # Jython analysis scripts + per-function dumps
├── scripts/                      # Host-side helpers (function-coverage, overlay capture)
├── site/                         # Project landing site (mirrors docs/)
└── extracted/                    # Build outputs (gitignored)
```

## Acknowledgments

- [**The Cutting Room Floor**](https://tcrf.net/Legend_of_Legaia) - developer attribution (Prokion / Contrail), debug-flag addresses, the catalog of 14 known builds.
- [Sam Ste's PROT.DAT unpacker](https://github.com/SamSteProjects/LegendOfLegaia_.Dat_unpacker) - early Python proof-of-concept that pointed at the right TOC slots and the TIM-pack heuristic.
- The PSX scene generally - Sony PsyQ docs, Martin Korth's [PSX-SPX](https://problemkaputt.de/psx-spx.htm), and decades of accumulated TIM/TMD/SPU documentation.
- Reference projects whose legal pattern this repo follows: ScummVM, OpenRCT2, OpenMW, OpenLara.

This project does not redistribute Sony's IP. You bring your own disc image. Tooling co-authored with AI agents under human direction.
