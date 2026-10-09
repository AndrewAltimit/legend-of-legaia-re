# legaia-engine-system

The engine's PSX runtime-system layer that never touches `World`: pad input
and the retail pad pump, the streaming-chunk installer and the MDEC DMA sync,
the RAM-cell registry, the global sound state, the fade actor, the mode-entry
initialisers and the codified capture observations. Free of wgpu, winit and
cpal, so it builds for native and `wasm32` alike.

`legaia-engine-core` owns the composition - `World` drives the fade, ticks the
sound state at frame begin and seats the mode-entry results - and re-exports
every module here at its old path, so `legaia_engine_core::input` and
`legaia_engine_system::input` name the same module.

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate or `legaia-engine-vm`, and it is system-shaped rather than a menu or
field kernel (those are `legaia-engine-menus` and `legaia-engine-field`, which
sit above this crate). Doc links that point back up at `engine-core` are plain
code spans, since rustdoc cannot resolve a link into a dependent crate.

## Modules

- **Input** - `input` (the PSX-shaped pad state, `input::Mapping`'s
  host-agnostic key binding and the DOM key-code vocabulary) and `retail_pad`
  (the per-frame libpad report to packed Legaia pad words, `FUN_8001822C`).
- **Streaming + movie** - `chunk_install` (the `[type, size, data]` stream
  chunk walker that routes sound-stream chunks), `mdec_dma_sync` (the FMV
  overlay's MDEC DMA sync pair), `cutscene` (FMV index to `MV*.STR` mapping),
  `movie_audio` (what a movie does to the score, one policy both hosts
  consult).
- **Sound + music** - `sound_state` (the global sound-system state the
  frame-begin driver services) and `music_labels` (a global BGM id /
  `music_01` bank slot resolved to its curated sound-test label; see
  [`docs/reference/music-tracks.md`](../../docs/reference/music-tracks.md)).
- **Screen fades** - `fade` (the retail fade-state primitive), `fade_ramp`
  (the SCUS fade actor's per-frame ramp step) and `pause_wipe` (the field's
  fade to black around the pause menu).
- **Mode + scene seams** - `mode_entry_init` (the one-time mode-entry
  initialisers: the field / town scene init `FUN_801D6704` and the duel-arena
  overlay seeds `FUN_801CF00C`; see
  [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md))
  and `scene_name_sync` (the name-based scene-change packet).
- **RAM + capture pins** - `ram_map` (the registry of well-known PSX RAM
  cells the cheat applier targets), `capture_observations` (codified
  save-state capture findings) and `draw_census` (a per-draw census of a
  host's frame keyed the way a retail display list is).

## See also

- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - the crate
  graph and the port boundary.
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
