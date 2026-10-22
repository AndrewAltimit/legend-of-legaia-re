# legaia-engine-system

The engine's PSX runtime-system layer that never touches `World`: pad input
and the retail pad pump, the streaming-chunk installer and the MDEC DMA sync,
the RAM-cell registry, the global sound state, the fade actor, the mode-entry
initialisers, the menu cursor primitive and the BIOS `rand` generators. It is
the bottom of the engine stack: no other engine crate is below it, and the
only workspace crates it names are the two label crates `legaia-cheats` (the
RAM-category taxonomy `ram_map` classifies cells with) and `legaia-gamedata`
(the music table `music_labels` joins to the disc). Free of wgpu, winit and
cpal, so it builds for native and `wasm32` alike.

`legaia-engine-core` owns the composition - `World` drives the fade, ticks the
sound state at frame begin and seats the mode-entry results. Consumers name
the modules here directly (`legaia_engine_system::input`).

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate, and it is system-shaped rather than a menu or field kernel (those are `legaia-engine-menus` and `legaia-engine-field`, which
sit above this crate). Doc links that point back up at `engine-core` are plain
code spans, since rustdoc cannot resolve a link into a dependent crate.

## Modules

- **Input** - `pad` (the raw and packed PSX pad-word layouts), `input` (the
  PSX-shaped pad state, `input::Mapping`'s host-agnostic key binding and the
  DOM key-code vocabulary), `gamepad` (the W3C standard-mapping controller
  layout every host folds a gamepad through), `retail_pad` (the per-frame libpad report to
  packed Legaia pad words, `FUN_8001822C`) and `menu_input` (the shared menu
  cursor navigation `FUN_801D688C`).
- **Random numbers** - `rand` (`BiosRand`, `psyq_rand_step` and
  `bios_rand_shape`: the BIOS `rand()` LCG every kernel rolls through;
  `world_lcg_step` / `world_rand`, the world stream lent to code without
  `&mut World`).
- **12-bit trig** - `trig` (`sin12` / `trig12`) and `rotation_lut`
  (`RotationLut`, the sine / cosine pair every 12-bit rotation indexes, and
  `RetailRotationLut`, the byte-exact `SCUS_942.54` table `FUN_80026BE0`
  installs), so the battle, effect, field and minigame kernels all name one
  table.
- **Streaming** - `chunk_install` (the `[type, size, data]` stream chunk
  walker that routes sound-stream chunks).
- **Movies** - `movie::cutscene` (FMV index to `MV*.STR` mapping),
  `movie::mdec_dma_sync` (the FMV overlay's MDEC DMA sync pair),
  `movie::movie_audio` (what a movie does to the score, one policy both hosts
  consult) and `movie::cutscene_trigger` (every retail FMV trigger site).
- **Sound + music** - `sound_state` (the global sound-system state the
  frame-begin driver services) and `music_labels` (a global BGM id /
  `music_01` bank slot resolved to its curated sound-test label; see
  [`docs/reference/music-tracks.md`](../../docs/reference/music-tracks.md)).
- **Screen fades** - `fade` (the retail fade-state primitive), `fade_ramp`
  (the SCUS fade actor's per-frame ramp step) and `pause_wipe` (the field's
  fade to black around the pause menu).
- **Mode + scene seams** - `mode_entry_init` (the one-time mode-entry
  initialisers: the field / town scene init `FUN_801D6704` and the duel-arena
  overlay seeds `FUN_801CF00C`, plus the boot entry word `init.pak` leaves for
  the title; see
  [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md))
  and `scene_name_sync` (the name-based scene-change packet).
- **SCUS leaf kernels** - `scus_leaf_kernels` (the small `SCUS_942.54`
  setters and seeders the overlays call: the SFX-cue delay set, the timed
  sound-source arm, the sound-stream request pair, the scene control block's
  reset values, the boot offset table, the identity index-list init and the
  dialog line count). The reset's body over a `World` is in `engine-core`.
  `incense_window` is the Incense top-up `FUN_80046870`, the item applier's
  selector-`0x82` arm the pause Items screen runs.
- **Option values** - `option_values` (the options screen's value enums,
  one per config word it edits: sound, battle camera, Select Attack, battle
  command entry, field movement default, field HP display). The options
  session is `legaia-engine-menus`; the battle input, the battle camera and
  the field HUD key on these values too, which is why they sit this low.
- **Story flags** - `story_flag_state` (the retail system / story flag
  arrays and the story-flag bit image the scripts test and set, held as one
  unit on `World`).
- **RAM + frame census** - `ram_map` (the registry of well-known PSX RAM
  cells the cheat applier targets) and `draw_census` (a per-draw census of a
  host's frame keyed the way a retail display list is). The codified
  save-state capture findings are `legaia_parity::capture_observations`.

## See also

- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - the crate
  graph and the port boundary.
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
