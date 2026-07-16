# legend-of-legaia-re

Reverse engineering and reimplementation of the PSX game **Legend of Legaia** (1998, Sony, NA SCUS-94254). The disc's formats are documented byte-by-byte with provenance back to Ghidra-traced functions, Rust parsers extract every asset, and a clean-room engine runs the game's own scenes, scripts and menus - natively on wgpu, and in your browser via WebAssembly. On top of that sit a disc randomizer, a translation toolchain, and a project site full of interactive viewers that run entirely off your own disc image.

The repo name `-re` is in both senses: **r**everse-**e**ngineering and **r**e-implementation. Same legal model as [ScummVM](https://www.scummvm.org/), [OpenRCT2](https://github.com/openrct2/OpenRCT2), [OpenMW](https://github.com/OpenMW/openmw), [OpenLara](https://github.com/XProger/OpenLara): this repo is code and documentation only - you bring your own disc image, and nothing Sony owns is ever committed or distributed.

Retail behaviour is the baseline: the simulation reproduces the original's arithmetic and quirks, and the parity oracles enforce it. But the port is not a museum piece - enhancements the original never had, like dynamic lighting, free-angle movement and [VR](docs/subsystems/vr-mode.md), ride on top as opt-in toggles that default off and never touch the simulation. See [`docs/subsystems/engine.md`](docs/subsystems/engine.md#fidelity-and-enhancements).

**Project site:** [andrewaltimit.github.io/legend-of-legaia-re](https://andrewaltimit.github.io/legend-of-legaia-re/)

## Demo

https://github.com/user-attachments/assets/aff19b4f-312c-44e2-bd44-3e6d99de2b03

The clean-room engine booting a real scene, plus the asset viewers. ([direct link](site/assets/legend-of-legaia-re-demo.mp4) · [on the project site](https://andrewaltimit.github.io/legend-of-legaia-re/))

## What's here

Four things, all usable today. Everything browser-side reads your disc image locally in the tab - nothing is uploaded, and the image never leaves your machine.

**Play and explore in your browser.**

- [**Play the port**](https://andrewaltimit.github.io/legend-of-legaia-re/play.html) - walk real towns and fields with retail movement and collision, talk to NPCs (the field VM plays their actual dialogue, branches and all), pass through doors, open the retail pause menu and every screen behind it, and load/save against a real memory-card image your emulator still accepts. Works flat or in VR over WebXR. Battles and the opening cutscenes are native-only for now.
- [**Minigames**](https://andrewaltimit.github.io/legend-of-legaia-re/minigames.html) - the casino slot machine, Noa's dance, and Baka Fighter, playable against the real step charts, rosters and payout tables read from your disc. The odds you're beating are the odds the cabinet shipped with.
- [**ROM patcher**](https://andrewaltimit.github.io/legend-of-legaia-re/tooling/rom-patcher.html) - the disc randomizer running client-side, with a spoiler-safe change report.
- [**Asset viewer**](https://andrewaltimit.github.io/legend-of-legaia-re/viewer.html) and [**media browser**](https://andrewaltimit.github.io/legend-of-legaia-re/media.html) - textures, 3D models, dialog, music, sound banks, and the FMVs, decoded in the tab.
- **Data tables with 3D model views** for [enemies](https://andrewaltimit.github.io/legend-of-legaia-re/monsters.html), [characters](https://andrewaltimit.github.io/legend-of-legaia-re/characters.html) and [NPCs](https://andrewaltimit.github.io/legend-of-legaia-re/npcs.html), plus [shops](https://andrewaltimit.github.io/legend-of-legaia-re/shops.html), [Tactical Arts](https://andrewaltimit.github.io/legend-of-legaia-re/arts.html), and the [whole world in 3D](https://andrewaltimit.github.io/legend-of-legaia-re/world-overview.html).

**Native tools and engine.** Prebuilt binaries for Linux and Windows on the [Releases page](https://github.com/AndrewAltimit/legend-of-legaia-re/releases), or `cargo build --release`. `legaia-extract` turns a disc into PNG / WAV / OBJ / JSON; `legaia-engine play-window` is the windowed engine (field scenes, the full menu stack, shops, level-ups, a battle harness, and MDEC cutscene playback with synced XA audio); `asset-viewer` browses every format interactively; plus `save-tool`, `legaia-rando`, and a CLI per format.

**Modding and translation.** [`legaia-rando`](docs/tooling/randomizer.md) patches your own `.bin` in place or emits a PPF: it shuffles drops, encounters, chests, steals, arts, doors, shops, casino prizes, prices, equipment, starting items and level, and battle tuning - several features are hand-assembled MIPS hooks injected into dead space. Its [`translate`](docs/tooling/translation.md) subcommands export the game's dialog and UI text to editable YAML and reimport it in place, the basis for community language packs; [`translate lift-official`](docs/tooling/pal-localizations.md) re-keys the official PAL French / German / Italian text onto the USA disc where it fits.

**The research itself.** Byte-level [format specs](docs/formats/overview.md) with confidence levels and Ghidra provenance, [subsystem documentation](docs/subsystems/) of how the engine actually works (VMs, battle formulas, audio, renderer, minigames), and the [tooling](docs/tooling/) that produced it all - reproducible from a retail disc.

## You bring the disc

**This project ships no Sony-owned bytes, ever.** There is no game executable, no asset data, and no ROM content in this repository or in any release archive. Everything here is code and documentation that operates on a disc image *you already own and supply yourself*.

Concretely, and non-negotiably:

- **You supply the disc image.** Every tool takes a path to your own `.bin`. Nothing is bundled and nothing is downloaded for you.
- **`extracted/` and `ghidra/projects/` are gitignored.** Extraction output is Sony-derived, so it stays on your machine. The same applies to per-function Ghidra dumps under `ghidra/scripts/funcs/` and to exported translation packs.
- **Disc-gated tests skip when `LEGAIA_DISC_BIN` is unset.** Tests that need real disc bytes skip *and pass* without it, so CI runs green with no disc data present. This gating is deliberate - don't remove it.
- **The licenses below cover this repository's code and docs only.** They do not, and cannot, grant you any rights to Sony's IP.

If you are adding code here, treat "no Sony bytes get committed" as the one hard constraint that outranks everything else - including decompiled C that carries literal asset data or text strings.

## Getting started

### Install a prebuilt release

Tagged releases publish prebuilt binaries on the [Releases page](https://github.com/AndrewAltimit/legend-of-legaia-re/releases). This is the fastest path if you just want to extract assets or run the viewers - no Rust toolchain required. Every archive carries every tool, the engine and the asset viewer included.

| Platform | Archive |
|---|---|
| Linux x86_64 | `legaia-tools-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `legaia-tools-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `legaia-tools-<version>-x86_64-pc-windows-gnu.zip` |

Download the archive for your platform, unpack it, and run the binaries out of the unpacked directory:

```bash
tar -xzf legaia-tools-<version>-x86_64-unknown-linux-gnu.tar.gz
cd legaia-tools-<version>-x86_64-unknown-linux-gnu
./legaia-extract --help
```

Each release also publishes a `SHA256SUMS` manifest, so you can verify what you downloaded:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

The Linux builds need glibc 2.28 or newer (Debian 10 / RHEL 8 / Ubuntu 18.10 and up). `legaia-engine` and `asset-viewer` additionally want a GPU and, on Linux, ALSA - both standard on a desktop install.

On Windows, unpack the zip and run the `.exe`s from a terminal in the unpacked directory. Every binary supports `--help`, and every subcommand supports `<binary> help <subcommand>` for its flags.

See [`docs/tooling/releases.md`](docs/tooling/releases.md) for what each archive contains and how releases are built.

### Build from source

Requires a Rust toolchain (`cargo`, edition 2024).

```bash
cargo build --release
```

Binaries land in `target/release/`. Note that `legaia-engine` is the *binary* name while the *package* is `legaia-engine-shell`, so `cargo build -p legaia-engine-shell` builds just that crate.

Optional extras, only needed for the reverse-engineering workflows:

- Docker + docker-compose, for headless Ghidra runs.
- mednafen or PCSX-Redux plus a save state at the scene you care about, for runtime overlay capture.

### Verify your disc image

```bash
./target/release/disc-extract verify "/path/to/Legend of Legaia (USA).bin"
```

| Disc | SHA-256 (Mode2/2352 .bin) |
|---|---|
| Legend of Legaia (USA), SCUS-94254 | `e6120a5d70716dd2f026a2da32d0171d52651971b52c4347a68541299f75258c` |

This hash is a sanity check against the project author's dump; different dumping tools can produce a different whole-image hash for the same disc. For canonical per-track verification, cross-check against [Redump](http://redump.org/disc/425/).

### Extract everything

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

Runs verify → disc → PROT → categorize → streaming sub-asset extract → TIM → PNG. Skip the slow stages with `--skip-png` (PNG conversion), `--skip-xa` (CD-XA demux), `--skip-catalog` (TIM-catalog TSVs), or `--skip-verify` (input SHA-256). Pass `-v` for per-file output.

Per-stage invocations - `disc-extract`, `prot-extract`, `lzs-decode`, and friends - are in [`docs/tooling/extraction.md`](docs/tooling/extraction.md).

## Using the tools

Each crate's `README.md` documents its own CLI in full; the highlights:

```bash
# What did the scene host actually resolve for a scene? TIMs uploaded to VRAM,
# TMDs parsed, MES presence, SEQ / VAB / event-script counts.
# `--disc` reads PROT.DAT + CDNAME.TXT straight off the image, so this works
# with no `extracted/` directory at all.
./target/release/legaia-engine info --disc "/path/to/game.bin" --scene town01
./target/release/legaia-engine list-scenes --disc "/path/to/game.bin"

# Tick the engine headlessly against a scene: drives the World, the camera, and
# the BGM director; logs scene transitions. Boot-loop smoke check.
./target/release/legaia-engine play --scene town01 --frames 600 --no-audio

# Windowed wgpu session: renders scene TMDs + HUD, accepts keyboard input.
# 60 Hz fixed tick, uncapped render.
./target/release/legaia-engine play-window --scene town01

# Decode a PSX STR file (MDEC video) and play it back with synced XA audio.
./target/release/legaia-engine play-str /path/to/cutscene.str

# Persist input bindings to TOML (engine-core::input::Mapping).
./target/release/legaia-engine config set --binding cross=Z
```

Asset inspection, after `legaia-extract` has populated `extracted/`:

```bash
# PROT entry browser
./target/release/asset-viewer prot extracted/PROT.DAT --cdname extracted/CDNAME.TXT

# Field scene runner - drives the field VM against a real scene's event-script
# records, with dialog rendering in the same window
./target/release/asset-viewer field town01

# Battle scene driver - boots the battle bundle, ticks the battle-action SM
./target/release/asset-viewer battle-scene --queued-action 3

# SEQ playback - the SsAPI-shape sequencer + a VAB through cpal, live audio
./target/release/asset-viewer seq path/to.seq path/to.vab

# Texture → PNG, mesh → OBJ
./target/release/tim convert extracted/tim_scan/<entry>/000.tim -o out.png
./target/release/tmd dump-obj extracted/tmd_scan/<entry>/000.tmd --out mesh.obj

# Group a field-pack's 97 schema slots by size to surface record kinds
./target/release/asset field-pack extracted/PROT/0005_town01.BIN --groups

# PSX memory-card reader
./target/release/save-tool dir ~/.mednafen/sav/Legend*.0.mcr
```

## Documentation

Start at **[`docs/overview.md`](docs/overview.md)** - the elevator pitch plus how the layers stack from disc down to sub-asset. From there the docs are organised topic-first:

- **[`docs/formats/`](docs/formats/overview.md)** - per-format byte-level specs (PROT, LZS, TIM, TMD, VAB, MES, ANM, MDT, scene bundles, effect bundles, overlays, …), each with a confidence level and Ghidra provenance. Read the relevant page before writing a parser.
- **[`docs/subsystems/`](docs/subsystems/)** - how the runtime engine works:
  - Boot + assets: [boot](docs/subsystems/boot.md), [asset loader](docs/subsystems/asset-loader.md).
  - VMs: [script](docs/subsystems/script-vm.md), [actor](docs/subsystems/actor-vm.md), [effect](docs/subsystems/effect-vm.md), [move](docs/subsystems/move-vm.md), [motion](docs/subsystems/motion-vm.md).
  - Render + audio: [renderer](docs/subsystems/renderer.md), [audio](docs/subsystems/audio.md), [cutscene](docs/subsystems/cutscene.md), [VR mode](docs/subsystems/vr-mode.md).
  - Battle: [battle](docs/subsystems/battle.md), [battle action SM](docs/subsystems/battle-action.md), [battle formulas](docs/subsystems/battle-formulas.md).
  - World + field: [world map](docs/subsystems/world-map.md), [field locomotion](docs/subsystems/field-locomotion.md), [field menu](docs/subsystems/field-menu.md).
  - Minigames: [fishing](docs/subsystems/minigame-fishing.md), [slot machine](docs/subsystems/minigame-slot-machine.md), [Baka Fighter](docs/subsystems/minigame-baka-fighter.md), [dance](docs/subsystems/minigame-dance.md), [Muscle Dome](docs/subsystems/minigame-muscle-dome.md).
  - [Engine reimplementation](docs/subsystems/engine.md) - the clean-room boundaries.
- **[`docs/tooling/`](docs/tooling/)** - how to drive the repo: [extraction CLIs](docs/tooling/extraction.md), [Ghidra setup](docs/tooling/ghidra.md), [overlay capture](docs/tooling/overlay-capture.md), [mednafen automation](docs/tooling/mednafen-automation.md), [PCSX-Redux automation](docs/tooling/pcsx-redux-automation.md), [randomizer](docs/tooling/randomizer.md), [translation](docs/tooling/translation.md), [port catalog](docs/tooling/port-catalog.md).
- **[`docs/reference/`](docs/reference/)** - [key Ghidra-traced functions](docs/reference/functions.md), [RAM map + globals](docs/reference/memory-map.md), [TCRF region data](docs/reference/builds.md), [curated game-data tables](docs/reference/gamedata.md), [open RE threads](docs/reference/open-rev-eng-threads.md) (still-open hunts + falsified hypotheses worth not re-walking).

Contributing? Read [`CONTRIBUTING.md`](CONTRIBUTING.md), then [`CLAUDE.md`](CLAUDE.md) - the latter is the full repository map and the catalogue of format gotchas that bite repeatedly (especially the MIPS LUI+ADDIU pair problem).

## Disc-gated tests

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace --release
```

Many integration tests touch a real disc or extracted directory - the full-pipeline validation suite, the per-scene asset-chain walk, the SEQ+VAB audio chain, the randomizer round-trip oracles, and the memory-card save round-trip. Find them with `grep -rl LEGAIA_DISC_BIN crates/*/tests`; each is named for what it covers.

With `LEGAIA_DISC_BIN` unset, every one of them skips and passes. That's intentional - it's what lets CI run without redistributing Sony data.

## Repository layout

```
legend-of-legaia-re/
├── Cargo.toml                    # workspace root
├── docker-compose.yml            # ghidra service (UID/GID-matched user)
├── docker/ghidra.Dockerfile      # wraps blacktop/ghidra:latest with host-UID mapping
├── crates/
│   │   # Track 1 - preservation (disc → PNG / WAV / OBJ / JSON)
│   ├── bytes/                    # Checked little-endian readers; leaf dep of every parser
│   ├── iso/                      # PSX disc reader + ISO9660 walker + sector write-back
│   ├── prot/                     # PROT.DAT TOC + CDNAME + standalone TIM-pack
│   ├── lzs/                      # Legaia LZS decoder (FUN_8001a55c) + re-packer
│   ├── asset/                    # Asset dispatcher, streaming, bundle detectors, categorize
│   ├── tim/                      # PSX TIM parser + PNG exporter + software VRAM model
│   ├── tmd/                      # Legaia TMD parser + primitive walker + OBJ export
│   ├── vab/                      # VAB sound bank extractor + SPU-ADPCM decoder
│   ├── xa/                       # XA-ADPCM decoder + CD-XA demux + WAV exporter
│   ├── seq/                      # PsyQ SEQ parser + CLI inspector
│   ├── mdt/                      # Move table (Tactical Arts) parser
│   ├── art/                      # Tactical Arts data + arts-name / arts-voice tables
│   ├── mes/                      # MES dialog container parser
│   ├── anm/                      # ANM animation container parser
│   ├── save/                     # Character record + memory-card walker + engine saves
│   ├── font/                     # Dialog font extraction + atlas / layout API
│   ├── mdec/                     # PSX MDEC clean-room decoder (Iki bitstream → RGBA8)
│   ├── extract/                  # Top-level pipeline driver
│   ├── mednafen/                 # Mednafen save-state parser + VRAM / SPU parity oracles
│   ├── pcsxr/                    # PCSX-Redux save-state main-RAM reader
│   ├── gamedata/                 # Curated walkthrough-mined tables (ground-truth labels)
│   ├── cheats/                   # GameShark / Mednafen cheat-database parser + classifier
│   ├── rando/                    # Randomizer / disc patcher for a user-supplied .bin
│   │   # Track 2 - engine reimplementation (clean-room Rust)
│   ├── engine-core/              # World, scene host, camera, menu runtime, save round-trip
│   ├── engine-ui/                # Renderer-agnostic UI draw-list builders
│   ├── engine-render/            # winit + wgpu, software PSX VRAM emulation, text overlay
│   ├── engine-audio/             # cpal mixer + clean-room SPU + SEQ sequencer
│   ├── engine-vm/                # Actor / field / effect / move / motion VMs + battle SM
│   ├── engine-shell/             # `legaia-engine` driver + BootSession + BGM director
│   ├── asset-viewer/             # Combined viewer: TIM, TMD, stage, VAB, SEQ, field, battle
│   └── web-viewer/               # WASM target - disc browser + viewers in the browser
├── data/                         # Curated non-Sony reference data (gamedata, cheats)
├── docs/                         # Topic-first technical reference (see "Documentation")
├── ghidra/
│   ├── projects/                 # Ghidra project DB (gitignored)
│   └── scripts/                  # Jython analysis scripts + per-function dumps (gitignored)
├── scripts/                      # Host-side helpers (CI gates, capture automation)
├── site/                         # Project landing site
└── extracted/                    # Your disc's assets - Sony bytes, never committed (gitignored)
```

## Status and license

**Status:** an active research project. Expect no API stability.

**License:** dual-licensed at your option under either the [Unlicense](LICENSE) (public-domain dedication) or the [MIT License](LICENSE-MIT). Apache-2.0 is intentionally not offered - this project is meant to be as close to public domain as the law in your jurisdiction allows, with no patent-retaliation strings attached: copy it, fork it, sell it, patent improvements on it, just don't stop anyone else from doing the same.

These licenses apply *only* to the code and documentation in this repository. **Sony's IP - game executable, asset data, ROM contents - is not redistributed here and is not covered by them.** See [You bring the disc](#you-bring-the-disc) above.

## Acknowledgments

- [**The Cutting Room Floor**](https://tcrf.net/Legend_of_Legaia) - developer attribution (Prokion / Contrail), debug-flag addresses, the catalog of 14 known builds.
- [**Sam Ste's PROT.DAT unpacker**](https://github.com/SamSteProjects/LegendOfLegaia_.Dat_unpacker) - early Python proof-of-concept that pointed at the right TOC slots and the TIM-pack heuristic.
- [**ZetaPhoenix's "Legaia Arts Data" spreadsheet**](https://docs.google.com/spreadsheets/d/1_U_AKdEncylFwE0lXkvPG-OhMWpNXgUdoaSGZ6vSUg0/edit?usp=drive_link) - public Google Sheets catalog of the Tactical Arts / Miracle Arts / Super Arts trigger strings and finisher replacements. The `legaia-art` `MiracleMatcher` / `SuperMatcher` tables (`crates/art/src/miracle.rs`, `super_art.rs`) cross-reference and validate against it.
- [**Meth962's "Legend of Legaia 100% Walkthrough"** (GameFAQs)](https://gamefaqs.gamespot.com/ps/197766-legend-of-legaia/faqs/53721) - the v1.10 "all Enemy stats section" + Seru-magic + magic-leveling tables ground three layers of `legaia-gamedata`:
  - `enemies.toml` carries Meth's per-enemy HP / MP / EXP / Gold / ATK / SPD / UDF / LDF / INT / AGL / element columns for every entry (extracted from in-RAM memory, so fan-recorded values rather than retail-binary-extracted constants - useful as labels for the binary monster records `crates/asset/src/monster_archive.rs` decodes from PROT 0867).
  - `bosses.toml` is rewritten around the per-fight layer (named attacks + MP cost, XP / gold / item rewards, recommended party level) for all 18 main-story B-code bosses plus the Lapis superboss.
  - `magic.toml` grows `absorb_lv1` / `absorb_lv2` / `absorb_lv3` integer-percent fields per Seru spell (Gimard 55/60/80, Gilium 1/1/1, the full 21-spell table); the per-cast XP curve and damage-scaling multipliers live in `legaia_gamedata::magic_leveling`.
- [**Henrique Stanke Scandelari (Stann0x)**](https://github.com/Stann0xus) - a music-track disambiguation that cross-references every BGM cue across its four naming spaces (the internal debug sound-test ID + working title, the in-game context it plays in, the official OST title, and a proposed relocalization title). Incorporated as [`docs/reference/music-tracks.md`](docs/reference/music-tracks.md) - the human-readable label layer for the extracted SEQ/BGM tracks, in the same curated-reference spirit as the game-data tables above.
- The PSX scene generally - Sony PsyQ docs, Martin Korth's [PSX-SPX](https://problemkaputt.de/psx-spx.htm), and decades of accumulated TIM/TMD/SPU documentation.
- Reference projects whose legal pattern this repo follows: ScummVM, OpenRCT2, OpenMW, OpenLara.

This project does not redistribute Sony's IP. You bring your own disc image. Tooling co-authored with AI agents under human direction.
