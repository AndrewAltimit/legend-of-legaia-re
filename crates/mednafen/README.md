# `legaia-mednafen`

Mednafen save-state parser + watchpoint-equivalent automation toolkit.

## Scope

- Parse gzipped `MDFNSVST` save states (`.mc{0..9}` files in
  `~/.mednafen/mcs/`). Note `~/.mednafen/sav/*.mcr` are **memory cards**, not
  states - 128 KiB card images carrying save blocks, with no main RAM in them.
- Index PSX-module sections (`MAIN`, `GPU`, `SPU`, `CDC`, …) and resolve
  `MAIN.MainRAM.data8` as 2 MiB of main RAM.
- Typed accessors over the `GPU` section (`PsxGpu` - VRAM bytes + control
  registers) and the `SPU` section (`PsxSpu` - 512 KiB SPU RAM, per-voice
  state snapshots, key-on/-off masks, master volume, reverb mode, the 32
  reverb coefficient registers + work area, and the per-voice reverb-send
  `EON` mask). The SPU accessor backs the audio-trace parity oracle in
  `engine-shell`; the reverb-routing accessors pinned retail's global
  Studio C reverb (the C7-REVERB hunt - see `docs/subsystems/audio.md`).
- Decode the frame's GPU primitive pool and libgpu ordering table
  (`prim_pool`): every standard textured and untextured polygon plus both
  sprite sizes, pool discovery, ordering-table discovery by the `ClearOTagR`
  signature, and a cycle-guarded chain walk that recovers true draw order.
- Diff main RAM between two snapshots - coalesce per-byte changes into
  contiguous "regions" with PSX virtual addresses, suitable for handing to
  Ghidra to look up writers.
- Bisect a sequence of snapshots to find when a target address crossed a
  predicate boundary (zero → nonzero, etc.).
- A declarative scenario manifest (`scripts/scenarios.toml`) maps
  each save slot to a labelled scenario with watchpoint regions; the CLI's
  `watch` subcommand runs all configured watchpoints against sister
  scenarios in one shot. `ScenarioManifest::mednafen_save_path` resolves a
  scenario's save preferring its immutable `saves/library` backup (by
  `backup_fingerprint`) over the wipe-prone live `.mc{slot}`.

## CLI

```text
mednafen-state info SAVE              # section table + PSX register snapshot
mednafen-state extract SAVE [--start ADDR --end ADDR --out PATH]
mednafen-state diff LEFT RIGHT [--start ADDR --end ADDR --json PATH]
mednafen-state write-taxonomy LEFT RIGHT [--start ADDR --end ADDR --samples N]
mednafen-state bisect --addr ADDR SAVE...
mednafen-state trace  --addr ADDR SAVE...
mednafen-state watch LABEL [--manifest PATH]
mednafen-state vram-dump SAVE [--out PNG --out-bin BIN --regs --display-crop]
mednafen-state spu SAVE [--all]      # reverb routing: master enable, mode, EON mask, per-voice
mednafen-state clut-trace --pack PROT_ENTRY SAVE... [--json PATH --include-tmd-body]
mednafen-state prim-trace SAVE [--pool-base ADDR --pool-end ADDR]
mednafen-state prim-dispatch-table SAVE [--overlay-targets-only]
mednafen-state prim-dispatch-survey SAVE...
mednafen-state world-map-camera SAVE... [--table]
mednafen-state identify SAVE...      # scene / game mode / player position
mednafen-state display-list SAVE [--coincident --list --all-ots --ot-addr ADDR]
mednafen-state scenarios [--manifest PATH]
```

`prim-trace` walks the primitive pool and scans the RAM windows behind it for
fixed-stride source tables. `prim-dispatch-survey` runs the dispatch-table
decode across several saves and asserts the SCUS-resident table is
byte-identical in all of them (it lives in code, so a RAM write cannot legally
touch it). `world-map-camera` decodes the top-view camera globals - the X/Z
scrolls are stored negated, so the printed `cam_x` / `cam_z` are the camera
target in world units, alongside the walk-view / top-view mode flag.

`identify` prints the scene name, game mode and player position of a batch of
states, mirroring `pcsxr-state identify` so `scripts/mednafen/state-index.py`
can sweep both emulators' corpora into one scene index. Unreadable states report
as an `!` row instead of aborting the sweep. The anchors live in the shared
`game_anchors` module, which `legaia-pcsxr` delegates to - so a mednafen `.mc`
and a PCSX-Redux `.sstate` answer "which scene is this?" identically.

`display-list` reads a frame's libgpu ordering table straight out of a RAM
image: the packets retail submitted are sitting in the bytes, so "does retail
actually draw this?" becomes a read rather than an inference from an emitter's
gate condition. It reports the packet census by opcode and by `(clut, tpage)`
texture family, and the chain in draw order; `--coincident` groups packets whose
projected screen geometry is identical, which is the "does retail stack two
copies of this surface, and which wins?" measurement.

Three things decide whether a `display-list` report means anything:

- **The pool is not the frame.** Stale packets from earlier frames sit in the
  packet pool; only the chain reachable from an ordering table is live. A
  scene-transition state can show hundreds of pool packets over a live chain of
  44 fade quads.
- **Draw order is chain order, not address order.** The PSX has no depth buffer,
  so the ordering table *is* the depth policy and the later packet wins.
- **Retail double-buffers.** Ordering tables come in pairs holding frame N and
  N-1 at near-identical counts, so merging a pair makes every surface look
  stacked with itself. One table is walked by default; `--all-ots` opts into the
  merged view and `--ot-addr` selects one explicitly.

`vram-dump --display-crop` writes only the **on-screen framebuffer** (the
display-area sub-rect: `display_fb` origin sized by the resolution decoded from
the `DisplayMode` bits, e.g. 320×240) instead of the full 1024×512 VRAM - the
right crop for comparing menu / HUD pixels against the engine renderer. Without
the flag you get whole VRAM (all texpages + CLUTs + both display buffers).

`prim-dispatch-table` decodes `FUN_80043390`'s SCUS-resident per-prim
renderer table (`0x8007657C`, 4 alpha rows × 20 slots) and the overlay
variant (`0x801F8968`, 1 row - the overlay path skips the alpha offset).
The eight overlay-resident high-mode renderers at `0x801F7644..0x801F8690`
ARE the per-prim emit leaves the world-map top-view routes its TMD
prims through - the bulk-continent emit mechanism that static `addprim`
hunters missed (cmd byte loaded from a descriptor table, leaf addresses
above the old `0x801F0000` overlay-capture cap).

See `docs/tooling/mednafen-automation.md` for the full workflow.

## Why "watchpoint-equivalent"?

PCSX-Redux and mednafen both have interactive memory-watchpoint debuggers,
but neither exposes a scriptable interface. The pragmatic substitute is
to take save states at progressive points during a sequence (mc1 → mc2 →
mc3 during a scene load) and diff the RAM regions of interest. Anything
that changed was written by code that ran in the gap. The diff output
gives addresses that map directly back to Ghidra's "Find references to
this address" search.

This crate exists to make that workflow scriptable.

## Composition

- Library API for engine-side tools that want to read live RAM out of a
  save state (e.g. validating an in-engine VM trace against the retail
  result).
- CLI binary for the per-PR manual workflow.
- Disc-gated integration tests under `tests/real_saves.rs` skip cleanly
  when `LEGAIA_MEDNAFEN_DIR` is unset.
- Library-gated save oracles that pin a runtime invariant against the
  content-hashed backup corpus (`saves/library/mednafen/`, resolved via each
  scenario's `backup_fingerprint`) and skip-pass when the corpus is absent -
  e.g. `tests/training_formation.rs` (the lone-Tetsu formation cell) and
  `tests/summon_model_base.rs` (the battle effect-model-library base
  `gp[0x754] = party_count + 2`).

## Sony-IP boundaries

Save states capture the user's runtime memory, which contains Sony-owned
bytes. The crate ships with no fixtures; tests that read real saves are
behind `LEGAIA_MEDNAFEN_DIR` and skip-pass without it.
