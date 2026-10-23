# legaia-engine-field

The field runtime's kernels that never touch `World`: the field overlay's
actor programs and per-actor kernels, the follow camera's per-scene
parameters and eases, player clip playback, the scene-transition and in-field
save-screen actors, the op-`0x49` submode, the field VM's event queue, CLUT
effects, the scene MAN's field-script decoders, the overworld draw-order and
ground-cue policies, and the overworld
controller and its panel screen. Free of wgpu, winit and cpal, so it builds for
native and `wasm32` alike.

`legaia-engine-core` owns the composition - `World`'s field frame, the
`FieldHost` the field VM runs against and the camera resolver. Consumers
name the modules here directly (`legaia_engine_field::camera_zone`).

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate, `legaia-engine-system` (pad input, fades, mode-entry
initialisers), `legaia-engine-mode` (the game-mode seat), `legaia-engine-vm`
or the other `World`-free crates below `engine-core`. The field-overlay
kernels the field VM itself does not import live here rather than in
`legaia-engine-vm`, and `legaia-engine-fishing` and `legaia-engine-minigames`
sit above this crate; `legaia-engine-menus` does not name it. The slot-A minigame overlays'
shared scene floor (`minigame_floor`: ground height, polar tables, the floor
sweep) and the dance floor's step-marker flipbook (`dance_marker`) live here
because both the fishing and the dance engines stand on them and the floor is
a field `.MAP` floor read through `field_regions`. Tests that need a `World` stay on the engine-core side (the
world test module `field_kernels`). Doc links that point back up at
`engine-core` are plain code spans, since rustdoc cannot resolve a link into
a dependent crate.

## Modules

- **Actors** - `actor_handler` (the actor's `+0x0C` per-frame handler
  identity), `scene_transition_sweep` (the scene-transition teardown sweep),
  `actor_colour_tween` (the actor colour tween), `field_actor_clone` (the `4C 14` clone spawn),
  `field_actor_program` (the voice-over scripted-scene actor,
  `FUN_801D4A60`), `ambient_emitter`, `save_screen_spawn`, `leader_swap`
  and `primitive_colour_grade` (the plain-template actor bodies: ambient
  emitter, save-screen hand-off, leader swap, geometry colour grade),
  `morph_weight_apply` (the morph-weight apply pass), `actor_look` (op
  `4C 45` look rotation) and `float_tween` (the `gp+0x148` screen-position
  tween, `FUN_80031AE4`).
- **Camera** - `camera_zone` (the camera-region record loader, the composer
  and the per-frame ease), `camera_ease` (the vertical-offset ease) and
  `register_ramp` (op `0x43` sub-3..6 camera-register zone ramp), and
  `camera_rig` (the `World`-held rig state: saved camera snapshot, scene
  offset + ease, shake amplitude, the zone-ramp register file).
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
  `vram_scroll` (its render-mode-4 sibling, the cyclic VRAM-rect scroller:
  the period integrator and the texel rotation),
  `object_tint` (the op-`4C 42` / `4C 81` word ramps, the parked object
  walk leg, the actor draw tint and its depth-cue form `tint_cue`; the
  scheduling over the field channels is `engine-core`'s),
  `vdf_pulse` (an enhancement arm of the vertex-morph substitution),
  `field_lit_mesh` (the light-source TMD rows' shading; see
  [`docs/subsystems/shading.md`](../../docs/subsystems/shading.md)),
  `packet_color` (per-vertex packet colour streams), `overworld_draw_order`
  and `overworld_ground_cue` (`FUN_801F89B8`'s continent-cell colour), and
  `fog_particles` (the scene fog pool `FogPool`: the per-scene cap, spawn,
  the free stack, and the render step that projects each particle through a
  `FogView` into `FogQuad`s).
- **Field camera** - `camera` (`Camera`, the follow / zone / scripted
  camera both hosts tick, with the camera-zone request and cull-view types;
  its eye-back preset `CameraDistance` is `legaia-engine-camera`'s). Generic over `CameraWorld`, the slice of the world it
  reads and publishes; engine-core implements it for `World`. The host
  frame-loop rules sit beside it in `frame_step` (`SimStepper`, the camera's
  place around the world tick, `CutsceneGlide`), over `FrameStepWorld`, a
  `CameraWorld` extension `World` also implements. See
  [`docs/subsystems/world-map.md`](../../docs/subsystems/world-map.md) and
  [`docs/subsystems/cutscene.md`](../../docs/subsystems/cutscene.md) for the
  retail globals it mirrors.
- **Mode seat** - not in this crate: `mode` and `minigame_entry` are
  [`legaia-engine-mode`](../engine-mode/README.md), one crate down. The
  field camera reads its `SceneMode`.
- **MAN field scripts** - `man_field_scripts`: the opcode-aware walk of a
  scene MAN's partition scripts (record spans, the scratchpad / system
  flag sites, inline encounter records, BGM starts, stager installs),
  placement classification and carrier derivation (`FieldCarrierConfig`),
  NPC motion legs and the walk-step speed ladder, the per-placement
  conversation tree (`placement_dialog_tree`: every page, choice, branch
  and next talk an interaction script reaches), scene-change triggers,
  and the system-flag / op-`0x49` window / motion-flag censuses over a set
  of MAN carriers. `engine-scene::man_census` holds the carrier
  resolution off a loaded `Scene` and the censuses' scene-name entry points.
- **Overworld** - `world_map` (`WorldMapController`, which drives
  `SceneMode::WorldMap`, and the entry fade) and `world_map_panel_host` (the
  world-map band's panel screen: the `0x801F2B98` window system, the six
  `ctx[+0x54]` panel actors and the travel arts, hosted on
  `WorldMapController::panels`, plus the field party HUD's state machine).
  Engine-core's `world_map_panel_host` holds the
  HUD queries that read the world (the suppress gate, the view mode, the
  rearm term, the present-party rows); see
  [`docs/subsystems/world-map.md`](../../docs/subsystems/world-map.md#the-panel-actor-state-machines).
  `map_display` is the L1 map display (game mode `0x0D`, PROT 0981): the
  top-down survey screen with a label over every discovered place.

- **Field-overlay kernels** -
  `field_regions` (the `.MAP` region-table scan and zone queries),
  `field_elevation` (the `.MAP` kind-2 floor-elevation override records
  the floor sampler reads for ramp and stair tiles),
  `field_player_clip`, `field_warp_tile`, `field_actor_reflect`,
  `field_actor_timers` (the shutter envelope, the eased move and the floor
  ladder; the shutter's two quads are `render-kernels::shutter_bars`),
  `field_passive_hud`, `field_state_pick`, `field_subsystem_enter`,
  `pooled_actor_seed`, `code_lock_actor`, `escape_timer`, `motion_pause`
  and `clut_fade_actor`.
- **Submode and panels** - `submode_hub_actors` (the op-`0x49`
  submode system-actor handler family), `world_map_panel` /
  `world_map_panel_actors` (the panel window system and its actor state
  machines). The developer-menu bodies the hub and panel code run
  (`dev_menu_leaves`, `dev_menu_adjust`, `dev_menu_flags`, `dev_equip_commit`,
  `dev_menu_action`) sit one crate down, in
  [`legaia-engine-dev-menu`](../engine-dev-menu/README.md), so the menu front
  end shares them without naming this crate.
- **World map** - `world_map_entity` (the entity state machine
  `FUN_801DA51C`), `world_map_dim`, `world_map_horizon`,
  `travel_art_actor` (Riremito and Rula) and `world_map_markers` (the port's
  kind-coded entity / player markers and their screen quads; not retail),
  `world_map_entity_config` (each on-map entity's role and the
  encounter-zone countdown that state machine acts on) and
  `field_carrier_state` (the same `FUN_801DA51C` machines ticked as field-scene
  carrier entities, with their battle / engage handoffs).
- **Script contexts** - `inline_dialogue` (the resumable state of an actor's
  inline interaction script) and `cutscene_timeline` (the spawned
  partition-2 record context); both hold a `legaia-engine-dialog` panel.
  `field_channels` holds the per-actor field-VM channels (each MAN
  partition-1 placement record spawned as its own script context,
  `FUN_8003A1E4`); `dialog_state` the simplified dialog panel, the inline
  dialogue runner and the interact / talk latches; `field_script_actor_state`
  the pool actors the field VM spawns onto a target (the op `0x43` arc jump,
  the op `0x34` attached light); `move_vm_globals` the move VM's per-actor
  bytecode and buffer pools, shared predicate / counter / slot-table words,
  scratchpad ramp targets and per-tick outcomes.

## The larger modules

- `field_submode` - the field overlay's op-`0x49` **sub-screen** entry family:
  context reset + driver-actor spawn (`FUN_801D9C3C`), the smaller
  fixed-template spawn (`FUN_801DE478`), the list-panel row layout
  (`FUN_801E6984`). The CARD mode-request leaf (`FUN_801D84B4`) is
  `legaia_engine_mode::card_request`.
- `field_submode_flag_window` / `field_submode_code_lock` - two of that
  dispatcher's slots on the field path: the floor-plate flag picker
  (`FUN_801EF014`, slot `0x23`) and the code lock (`FUN_801EED58`, slot
  `0x21`), each the slot's arm on the driver actor plus its window's lines.
  The `World` seams that apply their flag writes stay in `legaia-engine-core`.
- `field_overlay` - the field overlay image (PROT 0897) as data: its load
  base and `field_overlay_cstr`, the one reader for the NUL-terminated
  strings its data segment holds.
- `scene_transition_sweep` - the scene-transition **teardown sweep**
  (`FUN_801D7518`, which the field initialiser runs once per actor list on a
  warp entry); `actor_colour_tween` - the per-actor **colour tween**
  (`FUN_801DDC20`) whose actors the sweep retires by handler address.
- `camera_zone` - the **zone-driven follow camera**: the camera parameter
  block retail keeps at `0x8007B606..` (`CameraZoneConfig`, loaded from a MAN
  section-3 camera-region record by the port of `FUN_801DBC20`), the composer
  that turns it plus the player's position into a target pose (`FUN_801DAB90`:
  position-proportional sweeps, look-at anchors, fixed shots, the floor-height
  pitch coupling) and the per-frame ease / snap that walk the ten camera
  globals toward it (`FUN_801DB510` / `FUN_801DB8EC`), with the bearing and
  square-root LUT helpers they lean on. engine-core's `Camera::zone` runs it; the pinned
  constants in `camera_frame` are only the terrain-less fallback. Format +
  arithmetic: [`docs/formats/encounter.md`](../../docs/formats/encounter.md#man-section-3-the-camera-region-table).
- `camera_frame` - the World-free half of the per-frame camera:
  `FieldCameraFrame` (one arm per camera retail runs in a walkable scene),
  its matrix / eye / curvature factor (`frame_vp`, `frame_eye`,
  `frame_curve_scale`), the overworld walk poses, the per-pass NCLIP and
  near-reject words both hosts arm, and the pinned fallback poses.
  engine-world-views' `camera_view::resolve_field_camera` picks the arm from the
  live world.
- `camera_ease` - the field camera's smoothed **vertical-offset** step
  (`FUN_801DA390`; `player[+0x16]` is the middle slot of the `+0x14/+0x16/+0x18`
  position triple, and the eased result lands in the Y halfword of a vector):
  settled creeps by 1, unsettled takes a gap-proportional step capped at 12.

## `world_map_entity` - `FUN_801DA51C`

Per-entity overworld state machine (5 states on `entity[+0x8A]`:
Idle → Activating → Transitioning → Terminal). `step` drains the shared
encounter countdown in the Idle state, fires `on_encounter` /
`on_interact` / `on_scene_transition` host callbacks, and advances the
scene-transition states. `legaia_engine_core::World` drives one
`WorldMapEntityCtx` per installed overworld entity each
`SceneMode::WorldMap` tick, bridging `on_encounter` into a real
Field-machinery battle (returning to the world map on resolution) and
`on_interact` into a `FieldInteract` event.

## `escape_timer` - `FUN_801D2EBC`

The scripted countdown the field VM arms with `0x4C 0xD3`
(`SCHEDULE_TIMED_FLAGS`) - retail's collapsing-dungeon escape clock. One
retail function does three things per frame and all three live here:
`EscapeTimer::tick` subtracts the play-clock delta from the counter and
reports the below-threshold and expiry story flags the crossing fires (the
expiry also disarms), `hud_fields` decomposes what is left into MM:SS.ff, and
`timer_ink` picks the readout colour. A "busy" frame - retail's three
short-circuit conditions - leaves the counter standing.

`legaia_engine_core::World` joins the installer and the drain: the field VM's
operand triple reaches `World::schedule_timed_flags` through
`FieldFlagsHost::schedule_timed_flags`, and `World::tick_escape_timer` runs
the drain once per retail frame, raising each fired flag in the system-flag
bank and publishing the readout.

## `field_subsystem_enter` - `FUN_801F1278`

The installer of the field subsystem actor that op `0x49` and the menu button
both spawn: input suspend, the context-flag and pad-latch writes, the roster
resolve and three-cell roster seed, and the handler install. Its default handler
id `7` is the state pick `FUN_801F1F4C` (`field_state_pick`), which hands on to
the pause-menu session - it is not a party picker. A one-member party lands in
the middle roster cell and a two-member party takes the outer two.

## `motion_pause` - `FUN_8003C9AC`

`motion_pause` is the sibling kick (`FUN_8003C9AC`) a
field interaction and a partition-2 record spawn both end with: it rewrites
every moving-class actor's requested move to its standing move. It is a clip
request, not a halt - the motion VM keeps running, and a walker its ops send
on to another step asks for its walk anim again before the request plays.

## See also

- [`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md)
- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
- [`crates/engine-system`](../engine-system/README.md) - the system layer
  below this crate.
