# Engine reimplementation

The clean-room Rust port of the Legend of Legaia engine. End-user model: the engine is a binary; the user supplies a disc image; the engine extracts the assets at first run and plays the game using clean-room ports of every runtime subsystem.

## Goal

A playable port of Legend of Legaia (NA SCUS-94254) on modern systems via Rust + wgpu, with optional WASM/web target. JP/EU regions land after NA is solid.

## Non-goals

- Improving the game (no HD remaster, no balance changes, no QoL beyond what the original supported).
- Modding kit (useful as a side-effect, not as a designed deliverable).
- Translation work.
- Static recompilation of `SCUS_942.54`. The engine is **clean-room from documented specs and decompile-then-rewrite logic** — not auto-translated MIPS.

## Legal posture

The "user brings their own disc" model is the same one ScummVM, OpenRCT2, OpenMW, OpenLara, OpenJK, etc. use. As long as:
- Zero Sony bytes ship in the repo or in any released binary.
- All code is clean-room Rust written from format docs + decompiled-C reference (not derived assemblies, not auto-translated MIPS).
- Disc-dependent tests skip without the user's disc.

…the legal pattern is well-established. CI enforces this for every track.

The boundary to respect: **the decompiled C in `ghidra/scripts/funcs/*.txt` is reference material, not committable engine code.** A handler implementation in `crates/engine-vm/` is a fresh Rust function written *from* the decompile, not the decompile itself.

## Crate layering

```
iso          ← (none)
prot         → iso (conceptual)
lzs          ← (none)
asset        → lzs, prot
tmd          ← (none)
tim          ← (none)
xa           ← (none)
vab          → xa  (shares SPU-ADPCM F0/F1 filter constants)
mdt          ← (none)
mes          ← (none)
anm          ← (none)
extract      → all of the above

engine-core  ← (none)
engine-render → engine-core
engine-audio → engine-core
engine-vm    → engine-core
asset-viewer → engine-*, all parser crates
```

Asset crates (`tim`, `tmd`, `vab`, etc.) stay engine-agnostic — they produce typed in-memory representations. The engine layer turns those into GPU resources / audio buffers.

A future `sound` crate (sequencer playback for `.spk` sequences and the `.dpk / .MAP / .PCH` family) would depend on `vab`. A future battle / menu module belongs inside `engine-vm` next to the actor + field VMs rather than as a separate crate.

## Architectural principles

- **Asset crates stay engine-agnostic.** `crates/tim`, `crates/tmd`, etc. don't depend on wgpu / SDL3 / cpal.
- **Mockable I/O for tests.** The disc read path is abstracted via `crates/iso::RawDisc`; the same pattern extends to file-system extraction so tests can run without a disc.
- **Deterministic gameplay.** RNG seeded from a known value; physics tick on a fixed timestep. Required for any future TAS / verification work.
- **No "fix the bug" temptation.** If the original game has quirky damage rounding or oddly-timed cutscenes, replicate them. Behavioural fidelity is in scope; QoL is not.
- **Behaviour tests against runtime traces.** Long-term, capture inputs + RNG + frame outputs from the original game, replay through the engine, diff. The asset-viewer phase landed enough infrastructure to make this possible later.

## Phase plan

### Phase 1 — asset viewer (de-risks integration)

A standalone binary that loads the disc, lets the user navigate PROT entries, and renders / plays them. Render API: **winit + wgpu** (Vulkan / Metal / DX12 / WebGPU backends). Audio: cpal-backed mixer.

Implemented:
- `crates/engine-core` — `Vfs` trait + `DirVfs` (extracted-dir backend), `AssetCache`, `FrameTime`. Engine-agnostic, no GPU deps.
- `crates/engine-render` — `Renderer` (wgpu device + surface + textured-quad pipeline + flat / textured-mesh pipelines + lines pipeline). Aspect-preserving letterbox. Software PSX VRAM emulation (1024×512 R16Uint, per-prim CBA/TSB + 4/8/15bpp + CLUT decoded in fragment shader).
- `crates/engine-audio` — `AudioOut` (cpal-backed), single-voice mixer with linear resample + mono-to-N-channel fanout, supports F32 / I16 / U16 device formats.
- `crates/asset-viewer` — winit binary with subcommands:
  - `tim <PATH> [--clut N]` — display a single TIM.
  - `tmd <PATH> [--start N]` — display a Legaia TMD as a flat-shaded auto-rotating 3D mesh. PATH may be a single file or a directory; in directory mode N/P/PgDn/PgUp cycle through every `*.tmd` recursively. With `--bundle battle` (or `--vram-extra-dir`) it switches to the textured-mesh pipeline.
  - `stage <PATH>` — render a stage-geometry PROT entry as a wireframe.
  - `vab <PATH> [--offset 0xN] [--sample N] [--rate Hz]` — play one VAG sample from a VAB bank.
  - `prot <PROT.DAT> [--cdname FILE] [--start N]` — walk every PROT entry; auto-detects via the `categorize` classifier and shows / plays the first viewable sub-asset.

The PROT browser dispatch handles `tim_passthrough`, `tim_pack`, `data_field_streaming`, `scene_tmd_stream`, `scene_vab_stream`, and a VAB byte-search fallback for any class with embedded banks.

Open Phase 1 milestones:
- XA stream playback (streaming voice in `engine-audio`).
- Multi-voice mixer (the PSX SPU runs 24 voices; current mixer plays one).
- ADSR shaping for VAB tones.
- Per-vertex normals from the TMD per-object normal table (currently the renderer derives normals via screen-space derivatives, which is flat-shading).

### Phase 2 — runtime port

Port the script VM, field-loader chain, and effect VM. Handler-by-handler translation: dump each opcode handler from Ghidra, hand-port to Rust, unit-test against captured runtime traces. Aim for behavioural fidelity per opcode, not byte-exactness of the VM internals.

In progress:
- **Actor VM** — `crates/engine-vm/src/lib.rs`. 13 opcodes, full unit-test coverage. Drives the title screen sprite cluster.
- **Field VM** — `crates/engine-vm/src/field.rs`. All 43 explicit opcodes of `FUN_801DE840` are ported with a `FieldHost` trait abstracting every SCUS callback. Cross-context dispatch (extended-bit prefix), YIELD caller-propagation, `Op49State` tristate, the `0x4C` outer-nibble dispatcher, and the `0x5x/0x6x/0x7x` default-route fourth-flag-bank dispatchers are all wired. See [script VM](script-vm.md).
- **Move VM** — `crates/engine-vm/src/move_vm.rs`. All 71 main opcodes (`0x00..0x46`) of `FUN_80023070` ported, plus the `0x2F` extension dispatcher (61 sub-opcodes via `FUN_801D362C`). Per-frame entry is `actor_tick`, mirroring the gate at `FUN_80021DF4 + 0x80022B94`: skip when `wait_timer >= 0`, otherwise step, then report `Halted` if the post-call `flags & 0x8` bit is set. See [move VM](move-vm.md).
- **Effect VM** — `crates/engine-vm/src/effect_vm.rs`. Slot pool (`Pool`), 28-byte `MasterSlot` + 32-byte `ChildSlot`, the `Pool::init` / `Pool::spawn` ports of `FUN_801DE914` / `FUN_801DFDF8`, and the per-frame `Pool::tick` skeleton with `EffectHost::advance_state` extension hook. The retail walker's inlined per-state transitions are delegated to the host since they don't form a clean opcode dispatch. See [effect VM](effect-vm.md).

Pending Phase 2:
- **MES renderer** — `FUN_801ED710` in the 0897 town overlay. The container parser is done; the bytecode-to-glyph renderer is still extractable from the same overlay we already have.
- **Sprite engine** — back the actor VM's `Host` trait with real actor state.
- **Field / cutscene / battle / menu VMs** — overlay capture pipeline ready for the ones still pending capture.

### Phase 3 — gameplay assembly

Game-mode driver (28-entry table at `0x8007078C`), field map + dialog, the [battle subsystem](battle.md) including Tactical Arts and the per-actor state machine `FUN_801E295C`, menu + save / load.

### Phase 4 — targets

Native (winit + wgpu via Vulkan / Metal / DX12), WASM browser target. Mobile / console targets are deferred.

## Provenance + memory hygiene

The decompiled C dumps under `ghidra/scripts/funcs/` are reference material. Engine code in `crates/engine-vm/` is fresh Rust written *from* the decompile — never paste, always rewrite from the documented spec.

Per-opcode tests live next to the port; they use synthetic bytecode (no Sony bytes) so the test suite stays clean-room.
