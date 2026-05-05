# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project mission

Two coordinated tracks under one repo (`-re` = reverse-engineering, in both senses):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers. This is the foundation and continues independently.
2. **Engine reimplementation.** Clean-room Rust port of the engine — render via wgpu/SDL3, audio via the existing XA + VAB decoders, optional WASM target. End-user model: ship the engine binary, user supplies the disc image, engine extracts and runs.

The reimplementation is **clean-room from documented specs and decompile-then-rewrite logic**. It is not a static recompilation of `SCUS_942.54` — every line is fresh Rust, written from format docs + decompiled-C reference, the same model as ScummVM / OpenRCT2 / OpenMW / OpenLara. See [`docs/subsystems/engine.md`](docs/subsystems/engine.md) for the engine architecture and clean-room boundaries.

Sony IP (the executable, ROM contents, asset bytes) is **never** committed to this repo. The `extracted/` directory is gitignored, disc-dependent tests skip when `LEGAIA_DISC_BIN` is unset, and no decompressed Sony bytes (text strings, sample data, decompiled-C dumps with literal data) get checked in. CI runs without disc data.

## Workspace layout

Cargo workspace, edition 2024. One library + binary per crate:

**Track 1 — preservation (asset → PNG / WAV / OBJ / JSON)**

| Crate | Binary | Layer |
|---|---|---|
| `crates/iso` | `disc-extract` | PSX Mode2/2352 disc reader, ISO9660 walker |
| `crates/prot` | `prot-extract` | PROT.DAT / DMY.DAT TOC + CDNAME.TXT name map + standalone TIM-pack |
| `crates/lzs` | `lzs-decode` | Legaia LZS decoder (reverse-engineered from `FUN_8001a55c`) |
| `crates/asset` | `asset` | Asset dispatcher, DATA_FIELD streaming, pack format, stage-geom + field-pack + effect-bundle detectors |
| `crates/tmd` | `tmd` | Legaia TMD parser (header / objects / verts / normals + primitive walker, OBJ-with-faces export) |
| `crates/tim` | `tim` | PSX TIM parser + PNG exporter |
| `crates/xa` | `xa` | XA-ADPCM decoder + WAV exporter (decoder spec-correct; Legaia's .XA files have non-standard interleave) |
| `crates/vab` | `vab` | VAB sound bank extractor + SPU-ADPCM decoder |
| `crates/mdt` | `mdt` | Move table (Tactical Arts) parser |
| `crates/mes` | `mes` | MES dialog container parser (Compact + Records variants) |
| `crates/anm` | `anm` | ANM animation container parser (asset type 0x06) |
| `crates/extract` | `legaia-extract` | Top-level pipeline driver: disc → PROT → categorize → streaming sub-asset extract → PNG |

**Track 2 — engine reimplementation (clean-room Rust port)**

| Crate | Binary | Layer |
|---|---|---|
| `crates/engine-core` | — | VFS, asset cache, frame timing |
| `crates/engine-render` | — | winit 0.30 + wgpu 26; software PSX VRAM emulation (1024×512 R16Uint, per-prim CBA/TSB + CLUT decode in fragment shader) |
| `crates/engine-audio` | — | cpal-backed audio mixer |
| `crates/engine-vm` | — | Clean-room actor / sprite VM port (13 opcodes, ported from `FUN_801D6628` in the title-screen overlay) |
| `crates/asset-viewer` | `asset-viewer` | Combined viewer: TIM, TMD (textured 3D), stage geometry, VAB playback, PROT browser, scene-bundle presets |

Crate naming: package `legaia-foo`, lib `legaia_foo`. Internal deps go through workspace path entries (`legaia-asset = { path = "../asset" }`); add new ones the same way.

## Common commands

```bash
cargo build --release                   # All binaries → target/release/
cargo fmt --all -- --check              # CI gate
cargo clippy --all-targets --workspace -- -D warnings   # CI gate (warnings = failure)
cargo test --workspace --release        # CI runs --release
cargo test -p legaia-asset              # Single-crate tests
cargo test --workspace test_name        # Single test by name
```

### Disc-gated tests

Two integration tests touch a real disc and only run when `LEGAIA_DISC_BIN` points at a valid `.bin`:

- `crates/iso/tests/disc_pipeline.rs` — disc walk, file count, key file SHA-256s
- `crates/extract/tests/validation_suite.rs` — full pipeline, PROT entry count, sub-asset totals, TIM round-trip

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace
```

Without the env var, both tests **skip and pass** — that's intentional, so CI works without redistributing Sony data. Don't change that gating.

### Top-level pipeline (recommended for end-to-end runs)

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

Verify → disc → PROT → categorize → streaming-format extract → TIM → PNG. `--skip-png` / `--skip-verify` for the slow steps. The README has per-stage invocations if you need to drive individual binaries.

## Architecture: how the layers stack

```
PSX disc (.bin Mode2/2352)
  │  iso crate  — RawDisc::read_sector(lba) returns 2048-byte user data only
  ▼
ISO9660 files (PROT.DAT, DMY.DAT, SCUS_942.54, MOV/, XA/, CDNAME.TXT, SYSTEM.CNF)
  │  prot crate  — PROT.DAT / DMY.DAT TOC math (`start_lba = toc[p+2]`, `size = toc[p+5] - toc[p+3] + 4`)
  ▼
PROT entries (named via CDNAME.TXT — `#define name N` marks block START, names inherit forward)
  │  asset crate  — dispatch by format
  ▼
Per-entry contents:
   - some are LZS-compressed (lzs crate)
   - some are standalone TIM-packs (prot::timpack)
   - some are DATA_FIELD streaming containers (asset::parse_streaming) → packs (asset::pack)
   - many are stage-geometry record tables (asset::stage_geom) — the largest classified slice
   - some are field-pack bundles (magic 0x01059B84) and effect bundles (efect.dat, magic 0x02018B0C)
   - sound-driver outputs (.MAP / .PCH / .spk / .dpk) live in sound_data + sound_data2 blocks
   - VAB banks live in battle_data / level_up blocks (CDNAME's vab_01 is misleading)
  │  asset extract → tmd / tim / vab / mes / anm / mdt crates
  ▼
Sub-assets: PSX TIMs, Legaia TMDs (custom variant), VAB sound banks, MES dialog blobs, ANM packs
```

`docs/formats/` is the authoritative byte-level reference (one page per format) with Ghidra-traced provenance. Read it before writing a new parser. `docs/formats/overview.md` is the index.

## Key things that will trip you up

### Format / data gotchas

- **"No static caller in SCUS" is not "dead in retail".** Many functions (the asset descriptor walker `FUN_80020224`, the debug-flag writers, the dialog renderer, the field/battle/menu VMs) have zero xrefs *in `SCUS_942.54`* but are reachable from RAM overlays loaded at `0x801C0000+`. Only the title-screen overlay has been captured so far. Treat "0 callers in SCUS" as "needs overlay sweep", not "unused".
- **LZS "decompresses without error" is not a validity signal.** The 4096-byte ring buffer initializes to zeros, so most random inputs decode to zero-padded output of plausible length. Always magic-check the *decoded output* before claiming a hit.
- **CDNAME labels can be misleading.** `vab_01` (1072-1194) doesn't contain VAB headers (real VABs live in `battle_data`/`level_up`); `move_program_no` (0972/0973) doesn't match the consumer-expected move-table layout. Don't trust the label — verify with the loader-call constant (PROT-index hex passed to `FUN_8003e8a8`) or the file's magic bytes.
- **Three distinct pack formats** (don't confuse them):
  - `asset::pack` — used inside DATA_FIELD streaming chunks. Header is `u32 count` then `u32 word_offsets[count]`.
  - `prot::timpack` — used by some standalone PROT entries. Header is `(magic_lo, magic_hi, count<16, marker=0x01)` then offsets, with `byte_offset = word_index*4 + 4`.
  - **field-pack** (magic `0x01059B84`) and **effect-bundle** (magic `0x02018B0C`, used by `efect.dat`) — Legaia-specific TIM/TMD bundles with their own offset schemes; detected by `crates/asset`.
- **Legaia TMDs are a custom PSX TMD variant.** Magic is `0x80000002`, not the standard PSX `0x00000041`. Object table pointers are byte offsets relative to header end (12 bytes); the runtime patches them to absolute addresses via `FUN_800268dc` — static parsers should NOT do that patch. `scale` is always `0x00808080` (Legaia-custom).
- **Legaia TMD primitives are not PSX SDK standard.** Primitives are grouped: each group has an **8-byte header** `[u16 count, u16 flags, u8 olen, u8 ilen, u8 flag, u8 mode]` followed by `count × ilen*4` bytes of prim data. Vertex-index byte offset within each prim is looked up from the 6-entry descriptor table at `DAT_8007326c` keyed on `((flags >> 1) - 8) >> 1`. Walker: `legaia_tmd::legaia_prims::iter_groups`. Renderer: `FUN_8002735c` (60 GTE ops).
- **DATA_FIELD streaming "10 hits" are duplicate templates.** The streaming-format scan finds 10 PROT entries that share byte-identical chunk0/chunk1 hashes — they're a duplicated template, not 10 distinct fields. Real per-field DATA_FIELD layout is still open. Trailer data past the terminator is template padding for the duplicated cluster.
- **CLUT data scatters across PROT entries.** Many character meshes reference CLUT rows that live in *different* PROT entries from their TMD source. The `--vram-extra-dir` flag on `asset-viewer tmd` is the workaround until the runtime asset chain is fully traced (battle is done; field/town/level_up live in uncaptured overlays).

### Ghidra / static analysis gotchas

- **MIPS LUI+ADDIU pairs are NOT auto-resolved by Ghidra's reference manager.** Querying xrefs to a 32-bit address like `0x801C70F0` returns zero hits even when it's heavily used. We hit this and concluded the address was "fictional" before realizing. Workaround: `ghidra/scripts/find_lui_writers.py` walks instructions, tracks per-register LUI immediates, and finds combined accesses. Use it any time a static address looks unreferenced.
- **Use `MIPS:LE:32:default`** when importing `SCUS_942.54`, not `MIPS:LE:32:R3000` (Ghidra rejects the latter as Unsupported language). PSX R3000A is a strict MIPS-I subset.
- **PSX-EXE format**: skip the 0x800-byte header, base addr `0x80010000`. Use `BinaryLoader -loader-baseAddr 0x80010000`.
- The Ghidra container runs as **root**; files it writes to mounted volumes will be root-owned on the host. After every script run, `chown -R $(id -u):$(id -g)` the output.
- Jython 2.7 (Ghidra-bundled) chokes on Unicode in source unless an encoding declaration is added — keep `ghidra/scripts/*.py` ASCII-only.

### Per-function ground truth

`ghidra/scripts/funcs/<addr>.txt` are decompiled-C + disassembly dumps for the functions we've analyzed. When in doubt about a format, read the source dump rather than guessing. Key entry points:

| Address | Role |
|---|---|
| `8001a55c` | LZS decoder (the algorithm) |
| `8001f05c` | Asset-type dispatcher (`(type_byte << 24) | size` calling convention); type 0=TIM, 2=TMD, 4=MES, 6=ANM, 7=VDF, 8=SIN, 9=TMD2, B=MOVE2 |
| `8002541c` | Streaming-asset driver (DATA_FIELD entry point) |
| `80020224` | Descriptor-pair walker — no static caller in `SCUS_942.54` (overlay sweep pending) |
| `80026b4c` | TMD validate + register; checks `id == 0x80000002` |
| `800268dc` | TMD pointer fixup (offset → absolute, runtime only) |
| `8002735c` | Legaia TMD renderer (60 GTE ops); per-mode descriptor table at `DAT_8007326c` |
| `800520f0` | Battle scene loader (case 6 loads befect_data PROT 0x369-0x36B) |
| `80024cfc` | `play_anm_by_id` — only static reader of ANM containers (asset type 6) |
| `800204f8` | Move-buffer setup (Tactical Arts). Resolves `move_id` to a record; stages it onto an actor. Doesn't run opcodes itself. |
| `80023070` | Move-table opcode VM. 71 opcodes (`0x00..0x46`); JT at `0x80010778`. Op `0x2F` escapes to overlay-resident `FUN_801D362C` (61 sub-ops, JT at `0x801CE868`). |
| `80021df4` | Per-frame actor tick. Updates physics, then calls `FUN_80023070` to step the move VM. |
| `8003e4e8` | Boot-time TOC loader; reads first 3 sectors of PROT.DAT into `0x801C70F0` |
| `8003e8a8` | LBA resolver — reads in-RAM TOC at `0x801C70F0` |
| `8003e6bc` | Path-based file opener (resolves dev paths like `data\battle\efect.dat`) |
| `801D6628` | Actor / sprite VM (in title-screen overlay); 13-opcode dispatch table at `0x801CED70` |

Add new function dumps via `ghidra/scripts/dump_funcs.py`'s `TARGETS` list; see `docs/tooling/ghidra.md` for the compose-exec invocation, and `docs/reference/functions.md` for the full notable-functions directory.

### Runtime overlay capture

When code lives in RAM overlays at `0x801C0000+`, static analysis hits a wall. Use the one-shot pipeline:

```bash
scripts/analyze-overlay.sh ~/.mednafen/mcs/Legend*Legaia*.mc0 --label level_up
# Output: /tmp/overlay_loads_level_up.csv
```

This decompresses the gzipped mednafen save state, slices out the `0x801C0000+` window, re-imports it into Ghidra as `overlay.bin`, and emits a CSV of every `jal` to a known SCUS asset loader with the const-tracked `$a0` argument. Hex args = PROT indices; cross-reference `extracted/CDNAME.TXT`. That's how the actor VM was found and how scene asset bundles get traced.

## Docker / Ghidra environment

`docker-compose.yml` defines a single `ghidra` service (`blacktop/ghidra:latest`). Volumes:

- `./extracted:/data:ro` — the disc-extracted files (read-only into Ghidra)
- `./ghidra/projects:/projects` — Ghidra project DB (gitignored; local only)
- `./ghidra/scripts:/scripts` — analysis scripts (read-write so dumps land back on host)

Typical workflow: `docker compose up -d ghidra` once, then `docker compose exec ghidra /ghidra/support/analyzeHeadless ...` for each query. Don't restart the service per command.

## Conventions worth following

- **Don't redistribute or commit any Sony-owned bytes** (executables, asset data, decompressed output). The `extracted/` and `ghidra/projects/` directories are gitignored for this reason. CI runs without disc data.
- **Add new disc-dependent tests behind the same `LEGAIA_DISC_BIN` skip-pattern** that `crates/iso/tests/disc_pipeline.rs` uses. Tests must pass when the env var is unset.
- **Prefer adding a CLI subcommand to the existing per-crate binary** over a new binary unless the new tool spans crates. The pattern is `clap` derive + an enum of subcommands at the top of each `bin/<name>.rs`.
- **CI is strict.** `cargo clippy --all-targets --workspace -- -D warnings` treats every warning as a build failure; `cargo fmt --all -- --check` enforces formatting. Run both before pushing.

## Where to read next

The committed docs are organised topic-first under `docs/` — public-facing technical reference, no progress tracker / session log / status tables. Operational state lives in git log + the agent-only memory directory at `~/.claude/projects/-home-mikunpc-Documents-repos-legend-of-legaia-re/memory/`.

- **`docs/overview.md`** — elevator pitch + how the layers stack.
- **`docs/formats/`** — per-format byte-level specs (PROT, LZS, TIM, TMD, VAB, MES, ANM, MDT, scene bundles, effect, overlays, …). `formats/overview.md` is the index page.
- **`docs/subsystems/`** — how the engine works (boot, asset loader, script VM, actor VM, effect VM, renderer, audio, battle, engine reimplementation plan).
- **`docs/tooling/`** — extraction CLIs (`tooling/extraction.md`), Ghidra setup (`tooling/ghidra.md`), overlay capture (`tooling/overlay-capture.md`).
- **`docs/reference/`** — key Ghidra-traced functions (`reference/functions.md`), RAM map + globals (`reference/memory-map.md`), TCRF region data (`reference/builds.md`).

**Writing rules for these docs** (also enforced by the auto-memory `feedback_no_rot_counts.md`):
- Present tense. State what the format / function / subsystem **is**, not when it was figured out.
- No session numbers, dates, "ported in session N" markers, before-vs-after counts.
- No rot-prone counts of project state (tests, crates, function-coverage percentages).
- Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine.
- Provenance citations: `see ghidra/scripts/funcs/<addr>.txt` and `FUN_801XXXXXX in PROT entry NNNN_<name>`.
