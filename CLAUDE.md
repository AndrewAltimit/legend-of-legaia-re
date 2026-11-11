# CLAUDE.md

Guidance for Claude Code when working in this repository.

This file is a **map**, not the manual. The technical content lives in `docs/` and the per-crate `README.md`s; this file points at the section index for whatever you're touching, plus the rules of engagement that apply across the whole repo.

Keep it that way when you edit it. It loads into every session and every subagent, so it holds only what every session needs: the mission, the rules, the commands and the cross-cutting traps. Page lists belong on the section indexes; subtree-only rules belong in that subtree's `CLAUDE.md`.

## Project mission

Two coordinated tracks under one repo (`-re` = reverse-engineering, in both senses):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** From-scratch Rust port - render via wgpu, audio via the XA + VAB decoders, optional WASM target. End-user model: ship the engine, user supplies the disc image, engine extracts and runs.

The public framing is **a playable port and modding hub standing on Ghidra-traced reverse engineering** - fresh Rust from format docs + decompiled-C reference (ScummVM / OpenRCT2 model), not a decompilation project and not a static recompilation of `SCUS_942.54`. Don't call it "clean-room" in committed prose (the same people read the dumps and write the Rust); the enforced boundary is in [`docs/subsystems/engine.md`](docs/subsystems/engine.md).

**"Port" does not mean 1:1.** Retail behaviour is the measured ground truth - the traced dumps and parity oracles pin it exactly, and a retail-faithful mode stays available and testable - but the port is not bound by it. New features, mechanics, rendering and audio are in scope; enhancements ship enabled by default where they are clearly the better experience, with retail one toggle away.

The enhancement layer today: enhanced lighting (moods, lamp lights, emissives, halos from `render-kernels::scene_lighting`; default-on on both hosts, `I` / `--no-dynamic-lighting` disables, `F8` cycles time of day), volumetric ground fog (default-off while it matures; `F9` turns it on), the camera-occlusion fade (see-through walls around the player, default-on on both hosts, `--no-occlusion-fade` / `F4` disables), SSAO over a depth + normal pre-pass (shading stays forward; default-on on both hosts, `--no-ssao` / `Home` disables), precise free-angle movement (`options::precise_movement`), the debug orbit camera, and VR - all toggles that leave the faithful mode bit-identical when off.
Per-knob current defaults live in [`docs/subsystems/engine.md`](docs/subsystems/engine.md#fidelity-and-enhancements); a knob still defaulting to retail marks an enhanced side that is maturing, not a policy of restraint.

The render path splits in two, and the halves point opposite ways. **Shading defaults to retail** (in the renderer; the play hosts layer enhanced lighting over it): the game's textured / colour mesh paths draw the TMD's baked colour word through the GTE depth cue with no light source - except the light-source rows (TMD group flags `0x10..=0x17`), which the field dispatcher sends to its `NCCS` / `NCCT` handlers and which shade through the GTE light ([`shading.md`](docs/subsystems/shading.md)).
**Rasterisation defaults to clean**: `Renderer::set_psx_mode` is opt-in and gates the retail vertex snap (truncated onto the 320×240 grid, with its scanline texel row), the GTE register limits on field corners ([`renderer.md`](docs/subsystems/renderer.md#the-gtes-register-limits)), each field and battle primitive's draw at its ordering-table link ([`renderer-port.md`](docs/subsystems/renderer-port.md#the-field-sorts-by-each-primitives-link)), 15-bit dither and the GPU's 5-bit semi-transparency arithmetic only;
the hosts run it on the native 320×240 target (`set_native_resolution`, magnified nearest-neighbour), which is also what the retail-compare image channel scores ([`renderer-port.md`](docs/subsystems/renderer-port.md#the-native-target)). Affine UVs are not gated - they are unconditional, and they are the faithful behaviour.

The shading half holds on **both** hosts. A synthetic Lambert survives only in two viewer aids (the asset-viewer's bare-geometry `MESH_SHADER_SRC` and the site bestiary's opt-in "lit preview" toggle; its default draws the retail colour word), neither a claim about retail. The trap: an unbound colour attribute defaults to white, and white is `texel * 255/128`, so a missing colour stream reads as "too bright", not "unlit" ([`renderer.md`](docs/subsystems/renderer.md#lighting)).

Modding (`crates/patcher`) and translation are designed, shipped tracks, not side-effects - and what the patcher proves out against retail (randomizer logic, softlock fixes, tuning sliders) is expected to graduate into engine toggles. What the project still is *not*: a static recompile, and never a build that silently loses the retail-faithful mode.

**Sony IP (executable, ROM contents, asset bytes) is NEVER committed.** `extracted/` is gitignored, disc-dependent tests skip when `LEGAIA_DISC_BIN` is unset, no decompressed Sony bytes (text strings, sample data, decompiled-C dumps with literal data) get checked in. CI runs without disc data.

## Repository map

The committed docs are organised topic-first under `docs/` - public-facing technical reference, no progress tracker / session log / status tables. Operational state lives in git log + the agent-only memory directory at `~/.claude/projects/-home-mikunpc-Documents-repos-legend-of-legaia-re/memory/`.

### Top level

- [`README.md`](README.md) - public project overview, build instructions, license.
- [`docs/overview.md`](docs/overview.md) - elevator pitch + how the layers stack from disc to sub-asset.
- [`docs/decomp-recomp-reimplementation.md`](docs/decomp-recomp-reimplementation.md) - the three routes as a case study: this engine and a static recomp of the same game, built over the same months.
- [`docs/porting-by-reimplementation.md`](docs/porting-by-reimplementation.md) - the general method: reimplementation beside decompilation and static recompilation, the seven steps with their gates, the feedback loops, the traps.
- `docs/guides/` - task-oriented user guides: [getting-started](docs/guides/getting-started.md), [extracting-assets](docs/guides/extracting-assets.md), [playing-and-viewing](docs/guides/playing-and-viewing.md), [modding-and-translation](docs/guides/modding-and-translation.md), [translating](docs/guides/translating.md), [making-expansion-packs](docs/guides/making-expansion-packs.md). The site's guides section carries these plus site-only guides (desktop apps, playing online, expansion packs, VRChat world).

### Section indexes

Each section's page list lives on its own index page; open the one for the area you're touching. A new page or crate gets its row there, not here.

| Area | Index | What's in it |
|---|---|---|
| Formats | [`docs/formats/overview.md`](docs/formats/overview.md) | Per-format byte-level specs with Ghidra-traced provenance, confidence levels, format families. Read the relevant page before writing a parser; don't guess from the data. |
| Subsystems | [`docs/subsystems/README.md`](docs/subsystems/README.md) | How the runtime engine works: boot, renderer, VMs, battle, field, menus, minigames, audio, netplay. |
| Tooling | [`docs/tooling/README.md`](docs/tooling/README.md) | Extraction CLIs, Ghidra and emulator automation, every parity instrument and CI gate. Start at [`verification-ladder.md`](docs/tooling/verification-ladder.md) for the measurement stack. |
| Reference | [`docs/reference/README.md`](docs/reference/README.md) | Function directory, memory map, curated game data, the open / settled / falsified RE registers, glitches. |
| Crates | [`crates/README.md`](crates/README.md) | One row per crate, preservation track then engine track, plus the workspace rules. |

`docs/`, `crates/` and `ghidra/` each carry a `CLAUDE.md` of their own (gates, workspace rules, the container workflow) that loads when you work in that subtree.

### Ghidra-side scripts - [`ghidra/scripts/`](ghidra/scripts/README.md)

Jython analysis scripts that run inside the `ghidra` compose service, sorted into bucket subdirectories (`dump/`, `find/`, `lib/`, ...; map in [`ghidra/scripts/README.md`](ghidra/scripts/README.md)). The script catalogue lives in [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#script-catalogue). Per-function decompiled-C dumps land in `ghidra/scripts/funcs/<addr>.txt` (gitignored - they're Sony-derived).

### Host-side scripts - [`scripts/`](scripts/README.md)

Helper scripts that run on the host (not in the Ghidra container), mapped in [`scripts/README.md`](scripts/README.md): `ci/` (the pre-commit + CI gates and build/install helpers), `ghidra-analysis/` (overlay extraction + MIPS/GTE disassembly), `asset-investigation/` (TIM/TMD/slot-4/scene RE one-offs), `recomp/` (static-recomp differential oracle), `vrc-diorama/` (VRChat battle diorama), `vrchat-world/` (Unity kit for `export-glb` output), `models/` (custom-monster mesh generators), `profile/` (CPU sampler for where `perf` is refused), `lib/` (sourced bash helpers), `git-hooks/` (the shipped hook), `engine/` + `replays/` (determinism-replay fixtures), plus `pcsx-redux/` + `mednafen/` capture automation.

`scripts/scenarios.toml` (the capture-scenario manifest) and `manage-states.py` stay at the top level as operational entry points.

## Common commands

```bash
cargo build --release                                    # all binaries → target/release/
cargo fmt --all -- --check                               # CI gate
cargo clippy --all-targets --workspace -- -D warnings    # CI gate (warnings = failure)
cargo test --workspace --profile release-test           # CI's test profile: release opt-level, no LTO
cargo test -p legaia-engine-shell --profile dev-fast --test integration foo::  # the edit loop: incremental, results unchanged; CI stays on release-test
cargo test -p psx-lzs                                 # single-crate
cargo test --workspace test_name                         # single test by name
cargo test -p legaia-engine-shell --test integration foo::  # one tests/foo.rs (a crate with several tests/*.rs builds them as one `integration` binary)
cargo test -p legaia-engine-core --lib integration::foo::   # engine-core, engine-scene(-host), engine-session, parity, web-play mount tests/*.rs into the unit-test target instead
```

Top-level pipeline (recommended for end-to-end runs):

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

`--skip-png` / `--skip-verify` skip the slow steps. See [`docs/tooling/extraction.md`](docs/tooling/extraction.md) for per-stage invocations.

### Disc-gated tests

Many integration tests touch a real disc and only run when `LEGAIA_DISC_BIN` points at a valid `.bin`:

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace
```

Without the env var, every disc-gated test **skips and passes** - that's intentional, so CI works without redistributing Sony data. Don't change that gating. Find them with `grep -rl LEGAIA_DISC_BIN crates/*/tests`; each is named for what it covers. Two recurring shapes:

- **`crates/patcher/tests/*_real.rs`** - disc-round-trip oracles: patch a feature (drops / encounters / chests / steals / arts / doors / shops / starting items / starting level / equipment / item prices / unused content / weapon specialty / monster stats / move power / element affinity / spell costs) onto a scratch copy, re-decode off the patched image, assert the multiset/invariants are preserved + every touched sector stays EDC/ECC-valid + a fixed seed is byte-deterministic.
- **`crates/parity/tests/*_randomizer_runtime_e2e.rs`** - runtime oracles: patch the feature in memory, re-decode, then drive the *engine* grant kernel (`apply_battle_loot` / `apply_steal` / `buy_from_shop` / the field VM) to assert the runtime honors the patched value. Sidesteps the savestate RAM-cache trap; each keeps a baseline pass to stay non-vacuous.

Plus non-randomizer chains: `extract/validation_suite` (full pipeline), `engine-scene-host/scene_chain_e2e` (every CDNAME scene's assets resolve), `engine-audio/real_bgm_chain` (SEQ+VAB through the mixer), `parity/audio_trace` + `mednafen/real_spu_smoke` (SPU parity), `save/real_card_roundtrip` + `engine-save-state/end_to_end_gameplay_loop` (real memory-card saves; key on `~/.mednafen/sav/`, not `LEGAIA_DISC_BIN`).

## Conventions

- **Don't redistribute or commit any Sony-owned bytes** (executables, asset data, decompressed output). `extracted/` and `ghidra/projects/` are gitignored. CI runs without disc data.
- **Disc-dependent tests behind the same `LEGAIA_DISC_BIN` skip-pattern.** Tests must pass when the env var is unset.
- **Every dependency is declared once in the root `[workspace.dependencies]`** and taken as `foo.workspace = true`; the full workspace rules are in [`crates/CLAUDE.md`](crates/CLAUDE.md).
- **Prefer adding a CLI subcommand to the existing per-crate binary** over a new binary unless the new tool spans crates. The pattern is `clap` derive + an enum of subcommands at the top of each `bin/<name>.rs`.
- **CI is strict.** `cargo clippy --all-targets --workspace -- -D warnings` and `cargo fmt --all -- --check` both before pushing. A pre-commit hook is shipped - run `scripts/ci/install-hooks.sh` once per clone and the same gates run on every `git commit`. Set `LEGAIA_SKIP_PRECOMMIT=1` to bypass in emergencies.

## Cross-cutting facts that catch people out

These bite repeatedly across subsystems. Skim before chasing a "why is X broken / missing" thread.

- **"No static caller in `SCUS_942.54`" ≠ "dead in retail".** Most game logic lives in RAM overlays loaded at `0x801C0000+` (the field/event VM, the dialog renderer, the actor / battle / menu VMs). Treat zero static callers as "needs overlay sweep". Capture pipeline: [`docs/tooling/overlay-capture.md`](docs/tooling/overlay-capture.md).
- **A save state replays the RAM of the disc that booted it.** Loading a state onto a *different* (e.g. freshly patched) disc keeps the old resident SCUS, overlays, battle models and effect state - the disc bytes under test are masked until the game re-loads them, so a fix can look absent and a fixed defect can look present. Verify disc edits from a cold boot or a memory-card load, never a mid-battle state; bridge SCUS mismatches with `legaia-patcher scus-pokes`. Details: [`docs/tooling/pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md#memory-card-capture-tier).
- **PCSX-Redux silently applies a `.ppf` sitting beside the image you hand it** (`cdrom/ppf.cc`), so a probe run against a disc directory you did not stage can measure a *patched* game while every log line names the retail path. It is the sibling of the save-state trap above - there, the RAM is not the disc's; here, the disc is not the disc's. Point `--iso` at a scratch copy in a directory you control. Details: [`docs/tooling/pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md).
- **MIPS LUI+ADDIU pairs are not auto-resolved by Ghidra's reference manager.** Direct xref queries return zero hits even when the address is heavily used. Use `ghidra/scripts/find/find_lui_writers.py` (edit `LO`/`HI` to your target range). Details: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).
- **CDNAME `#define` numbers are raw in-RAM TOC indices, so every extraction filename label is shifted +2.** The named content for `#define name N` lives at extraction entry `N − 2` (`legaia_prot::cdname::block_for_extraction_index`), which is what dissolves historical "CDNAME labels mislead" cases such as `vab_01` carrying no VAB header. A CDNAME label is a hint, not a verdict: when attributing an entry, confirm with the loader-call constant or the magic bytes, and say which index space you mean. Details: [`docs/formats/cdname.md`](docs/formats/cdname.md#numbering-space).
- **An `(PROT entry N, offset K)` coordinate measured before the entry-size correction may name the wrong entry.** Entry size is the sector gap to the next entry (`toc[p+3] - toc[p+2]`); the superseded expression over-read into neighbours, so a constant whose `K` ran past entry `N`'s real end still hits the right *bytes* while naming the wrong *owner*. When a constant of this shape fails, suspect the pairing before the reader. Details: [`prot.md`](docs/formats/prot.md).
- **LZS "decompresses without error" is not a validity signal.** The 4 KB ring buffer initialises to zeros, so most random inputs decode to plausible-looking output. Always magic-check the *decoded* bytes. Details: [`docs/formats/lzs.md`](docs/formats/lzs.md).
- **Legaia SEQ has a u32 BE version field** (not PsyQ's u16) and its meta events carry **NO MIDI variable-length `length` field** - `0xFF 0x51` + 3 tempo bytes (no `0x03`), `0xFF 0x2F` ends track (no `0x00`). Reading a phantom length byte drops the first-body tempo override, pinning playback ~3x fast against the 240 BPM placeholder header. A meta **is** a running status: retail latches `0xFF`, so a data byte after a meta is the next meta's kind (PROT 1045's closing tempo). `ppqn = 480`; engine `Sequencer` clocks in exact integer SPU samples. Details: [`docs/formats/seq.md`](docs/formats/seq.md).
- **SEQ data in `scene_vab_stream` entries lives at non-zero offsets, and so do the VAG bodies.** The stream is `[chunk0 hdr][VAB header][chunk1 hdr][VAG bodies][chunk hdr][SEQ]` and some entries put the SEQ chunk first, so never probe fixed offsets: use `SceneAssets::seq_in_stream_entries` / `bgm_seq_offset` for the SEQ and `psx_vab::vag_body_origin` for the bodies. Details: [`vab.md`](docs/formats/vab.md).
- **There is one pack format and two readers for it, and no magic-prefixed pack.** `container-formats::pack` reads the pack at offset 0; `prot::timpack` reads the same pack four bytes in, past a `(TIM_LIST << 24) | size` DATA_FIELD chunk header - which is what its `word_index*4 + 4` and its `marker == 0x01` test both encode. The former "field-pack" (`0x01059B84`) is that same chunk header on a scene's texture pack, not a third format ([`field-pack.md`](docs/formats/field-pack.md)). So is the former "effect bundle" (`0x02018B0C`): a type-`0x02` (TMD pack) chunk header on a pack of 29 TMDs ([`effect.md`](docs/formats/effect.md#on-disc-effect-bundle-magic-0x02018b0c)). Pack pages: [overview](docs/formats/overview.md#compression-and-dispatch).
- **A field-VM global's name is the primary reader's, not the most interesting reader's.** `_DAT_8007B854` read as an input lock for as long as nobody enumerated its references; it is the ambient-particle master gate, raised and cleared by two arms of op `0x4C` nibble 3, and none of its six references disc-wide is pad state. The same failure named the monster record's `+0x20` an instant-death immunity byte when its primary reader is a VRAM-rect widener. Enumerate the references before naming the word.
- **Legaia TMDs are a custom variant.** Magic `0x80000002`, custom 8-byte group header, per-mode descriptor table at `DAT_8007326c`. Details: [`docs/formats/tmd.md`](docs/formats/tmd.md).
- **Ghidra promotes intra-function labels to fake `FUN_xxxxxxxx` calls.** When you see `iVar = FUN_801xxxxx(); return iVar;` in a giant dispatcher's C decomp, cross-check `grep -n "0x<addr>" overlay_<dump>.txt` - if the address appears as a `j` target inside that same function's disassembly, it's a label, not a call. Each such "label-call" is really `addiu s8, s8, N; j epilogue` (the standard PC-delta exit idiom). Catalogued for FUN_801de840 in [`docs/subsystems/script-vm.md`](docs/subsystems/script-vm.md#intra-function-label-catalogue) - applies to the dispatcher pattern in any large MIPS function, not just the field VM.
- **Port and document from the disassembly, not the decompiled C.** The label-call idiom above is one form of a broader rule: the C is a rendering, and its artifacts (dropped register arguments, reordered stores, mis-based dumps, `0 instructions` dumps, annotations read as evidence, …) have each already put a false claim into these docs. How to spot each, and what counts as evidence: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

## Ghidra container quick reference

`docker compose up -d ghidra` once, then `docker compose exec ghidra /ghidra/support/analyzeHeadless ...` per query; don't restart the service per command. Mounts, the dump workflow (`TARGETS` lists, per-overlay scripts) and the Jython traps are in [`ghidra/CLAUDE.md`](ghidra/CLAUDE.md); invocations and the script catalogue in [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).

## Writing rules for committed docs

- Present tense. State what the format / function / subsystem **is**, not when it was figured out.
- No session numbers, dates, "ported in session N" markers, before-vs-after counts.
- No rot-prone counts of project state (tests, crates, function-coverage percentages).
- Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine.
- Provenance citations: `see ghidra/scripts/funcs/<addr>.txt` and `FUN_801XXXXXX in PROT entry NNNN_<name>`.
- Operational state (progress, dates, session logs, status tables) lives in git log + agent memory, not in committed docs. Don't cite agent-memory filenames from a committed doc either - they aren't public files.
- Keep prose out of table cells. A cell is a one-line "what this covers"; if it needs a paragraph, the paragraph goes in a section on the linked page. `scripts/ci/check-doc-density.py` enforces this (>800-char lines, >150-word cells) across `docs/`, crate READMEs, the crate index, **and every `CLAUDE.md`** - this file is in scope precisely because it decayed furthest while exempt. Passing is a floor, not a target: prose that lands at 799 chars was written for the linter, not a reader.
