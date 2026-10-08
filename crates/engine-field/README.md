# legaia-engine-field

The field runtime's kernels that never touch `World`: the field overlay's
actor programs and per-actor kernels, the follow camera's per-scene
parameters and eases, player clip playback, the scene-transition and in-field
save-screen actors, the op-`0x49` submode, the field VM's event queue, CLUT
effects, the scene MAN's field-script decoders, the overworld draw-order and
ground-cue policies, the overworld
controller and its panel screen, and the battle animation / SFX cue
routers. Free of wgpu, winit and cpal, so it builds for
native and `wasm32` alike.

`legaia-engine-core` owns the composition - `World`'s field frame, the
`FieldHost` the field VM runs against, the camera resolver and the battle
actor tick that drains the cue routers - and re-exports every module here at
its old path, so `legaia_engine_core::camera_zone` and
`legaia_engine_field::camera_zone` name the same module.

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate, `legaia-engine-system` (pad input, fades, mode-entry
initialisers), `legaia-engine-vm` or the other `World`-free crates below
`engine-core`. Tests that need a `World` stay on the engine-core side (the
world test module `field_kernels`). Doc links that point back up at
`engine-core` are plain code spans, since rustdoc cannot resolve a link into
a dependent crate.

## Modules

- **Actors** - `actor_handler` (the actor's `+0x0C` per-frame handler
  identity), `field_actor_kernels` (the scene-transition teardown sweep and
  the actor colour tween), `field_actor_clone` (the `4C 14` clone spawn),
  `field_actor_program` (the voice-over scripted-scene actor,
  `FUN_801D4A60`), `cutscene_script_elements` (the plain-template actor
  bodies: ambient emitter, save-screen hand-off, leader swap),
  `morph_weight_apply` (the morph-weight apply pass), `actor_look` (op
  `4C 45` look rotation) and `float_tween` (the `gp+0x148` screen-position
  tween, `FUN_80031AE4`).
- **Camera** - `camera_zone` (the camera-region record loader, the composer
  and the per-frame ease), `camera_ease` (the vertical-offset ease) and
  `register_ramp` (op `0x43` sub-3..6 camera-register zone ramp).
- **Player** - `field_anim` (the party locomotion clip player over PROT 0874
  §1) and `walk_regen` (`FUN_801D0B90`, the walk-regen accessory passives).
- **Scene flow** - `scene_transition_actor` (the transition streaming actor),
  `field_save_screen_actor` (the overlay swap into the card UI and back),
  `field_submode` (op `0x49` sub-screen entry: `FUN_801D9C3C`,
  `FUN_801DE478`, the list-panel layout), `field_events` (the field VM's
  event queue), `cutscene_narration` (the opening subtitle roller) and
  `field_audio_release` (the slot-6 SPU / VAB release).
- **Colour + render policy** - `clut_fx` (the `4C` n6 sub-`0x61` CLUT-cell
  write and cross-fade), `clut_cell_fx` (the move-VM-driven HSV cycler; see
  [`docs/subsystems/field-ambient-fx.md`](../../docs/subsystems/field-ambient-fx.md)),
  `vdf_pulse` (an enhancement arm of the vertex-morph substitution),
  `field_lit_mesh` (the light-source TMD rows' shading; see
  [`docs/subsystems/shading.md`](../../docs/subsystems/shading.md)),
  `packet_color` (per-vertex packet colour streams), `overworld_draw_order`
  and `overworld_ground_cue` (`FUN_801F89B8`'s continent-cell colour).
- **Cue routers** - `anim_cue` (a battle action's `(frame, cue_id)` track to
  arts-voice XA requests and SPU ring cues) and `sfx_cue` (cue id to the
  4-slot pending ring / XA clip, `FUN_8004FE5C`); the battle actor tick in
  `engine-core` drains them.
- **MAN field scripts** - `man_field_scripts`: the opcode-aware walk of a
  scene MAN's partition scripts (record spans, the scratchpad / system
  flag sites, inline encounter records, BGM starts, stager installs),
  placement classification and carrier derivation (`FieldCarrierConfig`),
  NPC motion legs and the walk-step speed ladder, scene-change triggers,
  and the system-flag / op-`0x49` window / motion-flag censuses over a set
  of MAN carriers. Engine-core's `man_field_scripts` re-exports it by glob
  and adds the carrier resolution off a loaded `Scene` and the censuses'
  scene-name entry points.
- **Overworld** - `world_map` (`WorldMapController`, which drives
  `SceneMode::WorldMap`, and the entry fade) and `world_map_panel_host` (the
  world-map band's panel screen: the `0x801F2B98` window system, the six
  `ctx[+0x54]` panel actors and the travel arts, hosted on
  `WorldMapController::panels`, plus the field party HUD's state machine).
  Engine-core's `world_map_panel_host` re-exports it by glob and adds the
  HUD queries that read the world (the suppress gate, the view mode, the
  rearm term, the present-party rows); see
  [`docs/subsystems/world-map.md`](../../docs/subsystems/world-map.md#the-panel-actor-state-machines).

## The larger modules

- `field_submode` - the field overlay's op-`0x49` **sub-screen** entry family:
  context reset + driver-actor spawn (`FUN_801D9C3C`), the smaller
  fixed-template spawn (`FUN_801DE478`), the list-panel row layout
  (`FUN_801E6984`), and the CARD mode-request leaf (`FUN_801D84B4`).
- `field_actor_kernels` - the scene-transition **teardown sweep**
  (`FUN_801D7518`, which the field initialiser runs once per actor list on a
  warp entry) and the per-actor **colour tween** (`FUN_801DDC20`) whose actors
  the sweep retires by handler address.
- `camera_zone` - the **zone-driven follow camera**: the camera parameter
  block retail keeps at `0x8007B606..` (`CameraZoneConfig`, loaded from a MAN
  section-3 camera-region record by the port of `FUN_801DBC20`), the composer
  that turns it plus the player's position into a target pose (`FUN_801DAB90`:
  position-proportional sweeps, look-at anchors, fixed shots, the floor-height
  pitch coupling) and the per-frame ease / snap that walk the ten camera
  globals toward it (`FUN_801DB510` / `FUN_801DB8EC`), with the bearing and
  square-root LUT helpers they lean on. engine-core's `Camera::zone` runs it; the pinned
  constants in `camera_view` are only the terrain-less fallback. Format +
  arithmetic: [`docs/formats/encounter.md`](../../docs/formats/encounter.md#man-section-3-the-camera-region-table).
- `camera_ease` - the field camera's smoothed **vertical-offset** step
  (`FUN_801DA390`; `player[+0x16]` is the middle slot of the `+0x14/+0x16/+0x18`
  position triple, and the eased result lands in the Y halfword of a vector):
  settled creeps by 1, unsettled takes a gap-proportional step capped at 12.
- `anim_cue` - `walk_anim_cues` / `AnimCueState`, the per-frame walker
  over a playing battle action's 8-slot `(frame, cue)` track
  (`FUN_800508DC`): swing, hit, footstep and knockdown SFX, the party
  `0xC8..=0xFF` band resolved into the arts-voice namespace, and the
  CD-busy fallback ring cue. It emits `AnimCueEmit` decisions; engine-core's battle
  actor tick (`world::actors`) drains them into the SFX ring.

## See also

- [`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md)
- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
- [`crates/engine-system`](../engine-system/README.md) - the system layer
  below this crate.
