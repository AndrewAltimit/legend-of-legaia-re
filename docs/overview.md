# Overview

Legend of Legaia is a 1998 PlayStation 1 RPG by Contrail / Prokion / SCEI. This repository is a clean-room reverse-engineering project covering the disc, every asset format, the runtime engine, and a Rust reimplementation that runs the game from a user-supplied disc image.

The work runs on two coordinated tracks under one repo (`-re` = both *reverse-engineering* and *reimplementation*):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** Clean-room Rust port of the engine - render via wgpu/SDL3, audio via the existing XA + VAB decoders, optional WASM target. End-user model: ship the engine binary, the user supplies the disc image, the engine extracts and runs.

The reimplementation is **clean-room from documented specs and decompile-then-rewrite logic** - not a static recompilation of `SCUS_942.54`. Sony IP (the executable, ROM contents, asset bytes) is **never** committed to this repo.

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
| Understand a specific file format | [`formats/overview.md`](formats/overview.md) → per-format page |
| Understand how a runtime subsystem works | [`subsystems/`](subsystems/) - boot, asset loader, script VM, move VM, renderer, audio, battle |
| Use the extraction tooling | [`tooling/extraction.md`](tooling/extraction.md) |
| Reverse a new function in Ghidra | [`tooling/ghidra.md`](tooling/ghidra.md) |
| Capture a runtime overlay | [`tooling/overlay-capture.md`](tooling/overlay-capture.md) |
| Look up a key function or RAM address | [`reference/functions.md`](reference/functions.md), [`reference/memory-map.md`](reference/memory-map.md) |
| Cross-reference against a different region's build | [`reference/builds.md`](reference/builds.md) |
| Understand the Rust engine port plan | [`subsystems/engine.md`](subsystems/engine.md) |

## Workspace

The repo is a Cargo workspace. Crate naming: package `legaia-foo`, lib `legaia_foo`. One library + binary per crate where applicable.

**Track 1 - preservation:** `iso`, `prot`, `lzs`, `asset`, `tim`, `tmd`, `vab`, `xa`, `mes`, `anm`, `mdt`, `extract`.

**Track 2 - engine:** `engine-core`, `engine-render`, `engine-audio`, `engine-vm`, `asset-viewer`.

Run `cargo build --release` for all binaries, `cargo test --workspace --release` for all tests. Disc-gated tests skip when `LEGAIA_DISC_BIN` is unset - see [`tooling/extraction.md`](tooling/extraction.md).

## Public docs vs operational state

The documents under `docs/` and the contents of `site/index.html` are **technical reference**. They describe what the formats and subsystems *are*, not what work has happened recently. Operational state (work-in-progress, session notes, "what to do next") lives in git log, PR descriptions, and the agent-only memory files under `~/.claude/projects/`.
