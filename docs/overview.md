# Overview

Legend of Legaia is a 1998 PlayStation 1 RPG by Contrail / Prokion / SCEI. This repository is a clean-room reverse-engineering project covering the disc, every asset format, the runtime engine, and a Rust reimplementation that runs the game from a user-supplied disc image.

The work runs on two coordinated tracks under one repo (`-re` = both *reverse-engineering* and *reimplementation*):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** Clean-room Rust port of the engine - render via winit + wgpu, audio via the existing XA + VAB decoders, optional WASM target. End-user model: ship the engine binary, the user supplies the disc image, the engine extracts and runs.

The reimplementation is **clean-room from documented specs and decompile-then-rewrite logic** - not a static recompilation of `SCUS_942.54`. Sony IP (the executable, ROM contents, asset bytes) is **never** committed to this repo; `extracted/` is gitignored and disc-gated tests skip when `LEGAIA_DISC_BIN` is unset. See the root [`README.md`](../README.md#you-bring-the-disc) for the full legal position.

## How the layers stack

```
PSX disc (.bin Mode2/2352)
  │   crates/iso              - RawDisc::read_sector(lba) returns 2048-byte user data only
  ▼
ISO9660 files (PROT.DAT, DMY.DAT, SCUS_942.54, MOV/, XA/, CDNAME.TXT, SYSTEM.CNF)
  │   crates/prot             - TOC math, name map, standalone TIM-pack
  ▼
PROT entries (named via CDNAME.TXT - `#define name N` marks block start, names inherit forward)
  │   crates/asset            - dispatch by format
  ▼
Per-entry contents:
   • LZS-compressed                       (crates/lzs)
   • standalone TIM-packs                 (crates/prot::timpack)
   • DATA_FIELD streaming containers      (crates/asset::parse_streaming)
   • field-pack bundles (0x01059B84)
   • effect bundles (0x02018B0C)
   • scene_tmd_stream / scene_vab_stream  - per-scene asset prefixes
   • scene_v12_table / scene_asset_table  - per-scene tables
   • mips_overlay / overlay_ptr_table     - runtime code overlays
   • sound-driver outputs (.MAP / .PCH / .spk / .dpk)
   • VAB sound banks
  │   crates/asset extract → tmd / tim / vab / mes / anm / mdt
  ▼
Sub-assets: PSX TIMs, Legaia TMDs, VAB sound banks, MES dialog blobs, ANM packs
```

## Where to start reading

Choose by what you're trying to do:

| You want to… | Read |
|---|---|
| Install prebuilt binaries or build from source | Root [`README.md`](../README.md#getting-started) |
| Get assets off your disc | [`tooling/extraction.md`](tooling/extraction.md) |
| Understand a specific file format | [`formats/overview.md`](formats/overview.md) → per-format page |
| Understand how a runtime subsystem works | [`subsystems/`](subsystems/) - boot, asset loader, script VM, move VM, renderer, audio, battle, minigames |
| Understand the Rust engine port | [`subsystems/engine.md`](subsystems/engine.md) |
| Reverse a new function in Ghidra | [`tooling/ghidra.md`](tooling/ghidra.md) |
| Capture a runtime overlay | [`tooling/overlay-capture.md`](tooling/overlay-capture.md) |
| Look up a key function or RAM address | [`reference/functions.md`](reference/functions.md), [`reference/memory-map.md`](reference/memory-map.md) |
| Cross-reference against a different region's build | [`reference/builds.md`](reference/builds.md) |
| Patch your own disc (randomizer / translation) | [`tooling/randomizer.md`](tooling/randomizer.md), [`tooling/translation.md`](tooling/translation.md) |
| Find an open question to work on | [`reference/open-rev-eng-threads.md`](reference/open-rev-eng-threads.md) |

## Workspace

The repo is a Cargo workspace. Crate naming: package `legaia-foo`, lib `legaia_foo`; one library plus an optional binary per crate. Each crate's `README.md` documents its own scope and CLI.

**Track 1 - preservation.** `bytes` (shared checked readers) and the container layer `iso`, `prot`, `lzs`, `asset`; the per-format parsers `tim`, `tmd`, `vab`, `xa`, `seq`, `mes`, `anm`, `mdt`, `art`, `font`, `mdec`, `save`; the pipeline driver `extract`; the emulator-state bridges `mednafen` and `pcsxr`; the curated label sets `gamedata` and `cheats`; and the disc patcher `rando`.

**Track 2 - engine.** `engine-core` (world + scene host), `engine-vm` (the ported VMs and battle SM), `engine-render` (winit + wgpu), `engine-audio` (SPU + sequencer), `engine-ui` (renderer-agnostic draw lists), `engine-shell` (the `legaia-engine` binary), plus `asset-viewer` and the `web-viewer` WASM target.

Run `cargo build --release` for all binaries, `cargo test --workspace --release` for all tests. Disc-gated tests skip when `LEGAIA_DISC_BIN` is unset - see [`tooling/extraction.md`](tooling/extraction.md).

## Public docs vs operational state

The documents under `docs/` and the pages under `site/` are **technical reference**. They describe what the formats and subsystems *are*, not what work has happened recently - no roadmaps, no status tables, no session notes. Operational state (work-in-progress, "what to do next") lives in git log and PR descriptions.
