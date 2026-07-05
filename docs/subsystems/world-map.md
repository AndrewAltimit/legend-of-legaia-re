# World Map Subsystem

Covers the overworld traversal mode: normal walk view and the debug top-down view.
Sources: `overlay_world_map.bin` (walk-view) and `overlay_world_map_top.bin` (top-view debug)
captures from mednafen save states; decompiled at `ghidra/scripts/funcs/overlay_dialog_801e76d4.txt`,
`overlay_dialog_801ead98.txt`, and `801cfc40.txt`.

This is a large page. The [world-overview viewer](world-overview-viewer.md) (the
static-site WebGL deliverable) is documented in a sibling file. Use the contents
below to jump within this page.

## Contents

**Overlay + key functions**
- [Overlay structure](#overlay-structure)
- [Key functions](#key-functions) - [controller `FUN_801E76D4`](#fun_801e76d4---world-map-controller-9320-bytes) · [debug-menu renderer `FUN_801EAD98`](#fun_801ead98---world-map-debug-menu-renderer-7280-bytes) · [entity tick `FUN_801DA51C`](#fun_801da51c---world-map-entity-tick-260-bytes)

**Entity / encounter SM**
- [Encounter-record installation](#encounter-record-installation) · [clean-room port](#clean-room-port--both-overworld-and-field) · [NPC dialogue text source](#npc-dialogue-text-source)

**Overworld player + scenes**
- [Player movement + region-keyed encounters](#overworld-player-movement--region-keyed-encounters) · [collision / walkability](#overworld-collision--walkability) · [camera-relative movement remap](#camera-relative-movement-remap) · [boot-path seeding](#boot-path-seeding)
- [Entity / actor placement table](#entity--actor-placement-table) · [classifying the entity kind](#classifying-the-entity-kind-from-its-script) · [scene destinations](#scene-destinations)

**Terrain + geometry**
- [Loading the kingdom geometry](#loading-the-kingdom-geometry-engine-port) · [placing the continent terrain](#placing-the-continent-terrain-engine-port) · [ground texturing](#ground-texturing) · [rendering the placed entities](#rendering-the-placed-entities) · [auto-engage on walk-over](#auto-engage-on-walk-over)

**Render pipeline**
- [Render pipeline](#render-pipeline) - [per-frame dispatch](#per-frame-dispatch-scus-resident) · [render tick `FUN_80016444`](#fun_80016444---scus-world-map-render-tick-1352-bytes) · [horizon emitter `FUN_801D7EA0`](#fun_801d7ea0---world-map-poly_ft4-batch-emitter-832-bytes)
- [Top-view bulk-terrain render path](#top-view-bulk-terrain-render-path-overlay-replaced-per-prim-renderers) - [per-slot delta vs SCUS sibling](#per-slot-delta-vs-scus-sibling)
- [Per-frame render-pass iterator `FUN_8002519c`](#per-frame-render-pass-iterator---fun_8002519c) · [per-actor render dispatcher `FUN_8001ADA4`](#per-actor-render-dispatcher---fun_8001ada4) · [gate-arm chain](#gate-arm-chain---fun_801d1344---fun_801d8258)

**Reference**
- [Globals used](#globals-used) · [World-overview viewer](#world-overview-viewer)

## Overlay structure

Two world-map overlay variants are paged into `0x801C0000..0x801EFFFF`:

| Variant | First prologue | Triggered by |
|---|---|---|
| Normal walk (`overlay_world_map`) | `0x801CFC40` | Standard world-map mode |
| Top-view debug (`overlay_world_map_top`) | `0x801CE850` | Debug toggle combo (see below) |

Both variants share the core field VM (`FUN_801DE840`), move-VM extension (`FUN_801D362C`),
and all rendering helpers. The top-view variant adds extra rendering code that starts ~0x1400
bytes earlier in the code window.

The view-mode toggle flag lives at `DAT_801F2B94`. The world-map overlay
variants extend past `0x801F0000` - capture them with a wider window
(`0x801C0000..0x801F9000`, 228 KB) to include the prim-mode dispatch
table at `0x801F8968` and its eight overlay-resident emit leaves at
`0x801F7644..0x801F8690`. The old 192 KB default
(`0x801C0000..0x801EFFFF`) clips both. `scripts/ghidra-analysis/extract-mednafen-overlay.py`
now defaults to the wider window.

## Key functions

### `FUN_801E76D4` - world map controller (9320 bytes)

Entry: `(ctx_ptr)`. Handles:

1. **Top-view debug toggle** - fires when `_DAT_8007B98C != 0` (debug flag) AND
   `_DAT_8007B850 == 0x4A` (pad mask) AND `_DAT_8007B874 == 0x40` (held mask).
   On trigger: `DAT_801F2B94 ^= 1` (flips walk/top-view), captures current actor
   camera position into `_DAT_801F35A8/AA/AC`, clears `ctx[+0x54]` and `ctx[+0x50]`,
   calls `FUN_80035C10`.

2. **Top-view camera controls** (active when `DAT_801F2B94 != 0`):
   - `_DAT_8007B850 & 0x1000` / `0x4000` → `_DAT_80089120 -= 8` / `+= 8` (X scroll)
   - `_DAT_8007B850 & 0x2000` / `0x8000` → `_DAT_80089118 -= 8` / `+= 8` (Z scroll)
   - `_DAT_8007B850 & 0x20` / `0x80` → `_DAT_8007B794 += 0x14` / `-= 0x14` (azimuth)
   - `_DAT_8007B850 & 8` / `2` → `_DAT_8007B6F4 -= 4` / `+= 4` (zoom/height)
   - Bit `DAT_801F2B95 & 1`: enables `FUN_801E75DC` (overlay animation step)
   - Bit `DAT_801F2B95 & 2`: second animation flag

3. **Normal-walk path** (`DAT_801F2B94 == 0`): standard per-frame world-map update
   (field VM tick, actor step, camera follow via motion VM).

### `FUN_801EAD98` - world map debug menu renderer (7280 bytes)

Entry: `(ctx_ptr, x, y, scroll_idx, max_visible)`. Renders a vertically scrolling
menu list for the world map developer menu. String table at `0x801CF344..`:

| Index | Label |
|---|---|
| 0 | `MAP_CHANGE` (or `CLOSED` when `_DAT_8007B868 != 0`) |
| 1 | `CARD_OPTION` (or `CLOSED`) |
| 2 | `PLAYER_STATUS` |
| 3 | `CAMERA` - shows `_DAT_80089120/_DAT_80089118` as `000 000` |
| 4 | `ENCOUNT` - shows encounter rate from `DAT_8007B5F8` |
| 5 | `OTHER_SETTINGS` |
| 6 | `BGM_CALL` - shows `_DAT_801F2E90` as `00` |
| 7 | `DEBUG` |
| … | At least 24 entries total (bounds check `local_40 > 0x17`) |

Called by `FUN_801ECA08` when the debug menu panel is active
(`ctx[+0x54]` mod-6 dispatch resolves to cases 1 or 3).

### `FUN_801CA08` - world map panel sizer / menu caller (256 bytes)

Entry: `(ctx_ptr, row_start, row_end, col_idx, ...)`. Computes panel height
`= (row_end - row_start + 1) * 8`; vertical offset `= 0xD0 - height` (centres
a 208-pixel viewport). Writes height/offset into a panel descriptor at
`0x801F2B98 + col_idx * 28`. Dispatches on `ctx[+0x54]` (6-way JT at
`0x801CF4CC`); cases 1 and 3 call `FUN_801EAD98` to draw the menu list.

### `FUN_801EE90C` - world map text-box dispatcher (128 bytes)

Entry: `(ctx_ptr)`. Dispatches on `ctx[+0x54]` via a 15-entry jump table at
`0x801CF5FC`. When `ctx[+0x54] >= 15` but `< 10`: falls through to
`FUN_80031D00` (text-actor tick - advances the MES bytecode one frame).

### `FUN_801CFC40` - world map sprite batcher (524 bytes, top-view only)

Entry: `(actor_ptr, ?, screen_x, screen_y, ?, ?)`. When `_DAT_8007B6B8 == 0x20`
delegates to `FUN_801CF9F4`; otherwise writes actor screen coordinates into
GPU registers `0x1F800020/22/24` from `actor[+0x14/+0x16/+0x18]`, then
iterates the sprite-descriptor list at `DAT_801C93C8`. Present only in the
`world_map_top` overlay variant.

### `FUN_801DA51C` - world map entity tick (260 bytes)

Entry: `(entity_ptr)`. 5-state dispatcher on `entity[+0x8A]` (jump table at
`0x801CEC28`). When `_DAT_80083808 == 0` and the entity state is 0: calls
`FUN_800243F0` (the per-frame **BGM/asset poller** - it resolves the pending
BGM id to a PROT slot, see [`asset-loader.md`](asset-loader.md#music--sfx-selection-bgm-lookup); it is
*not* a location→scene resolver) and handles pad-button checks against
`_DAT_8007BB38` for entity interaction. Called once per world-map entity per
frame by the entity pool tick loop. (The body is the encounter→battle handoff -
state 1 installs the formation cell and state 2/3 writes `_DAT_8007B83C = 8`;
**scene/town transitions are not here** - those are the field-VM `0x3F`
named-scene-change op, see [scene destinations](#scene-destinations).)

#### Encounter-record installation

The body at `0x801DA620..0x801DA678` populates the global encounter formation
cell from a per-encounter record pointed at by `entity[+0x94]`:

1. Clear the 4-slot formation array at `0x8007BD0C..0x8007BD0F` (slots 3, 2,
   1, then 0 - slot 0 is cleared in the delay slot of `JAL 0x801DE190`).
2. Read `monster_count = entity[+0x94][+0x3]`.
3. Copy `entity[+0x94][+0x4 .. +0x4 + monster_count]` into the formation
   cell, byte-for-byte.

This copy runs in **SM state 1** (`entity[+0x8A] == 1`). The same invocation
clears `entity[+0x94]`, sets `entity[+0x88] = 0`, advances `entity[+0x8A]` to
`2`, and then **falls through** the `case 2/3` arm, which writes
`_DAT_8007B83C = 8` (the game-mode handoff that launches the battle), sets
`entity[+0x8A] = 4`, and clears the `0x80000` "encounter active" flag on the
player context (`_DAT_8007C364[+0x10]`, which state 1 had raised). So the
formation install and the battle launch happen in the **same tick** the
carrier reaches state 1. State 0 reaches state 1 either via the random roll
`FUN_801D9E1C` (which sets `+0x88`/`+0x8A`/`+0x94` from the rolled formation
index) or - in a 0%-random town like `town01` - via a scripted advance from
the scene's interaction bytecode.

The carrier `entity_ptr` (`param_1`) is a **dedicated field entity**, distinct
from the player context `_DAT_8007C364`: the routine reads `param_1[+0x8A]` /
`param_1[+0x94]` but writes the `0x80000` flag onto `_DAT_8007C364` separately,
and the player-context object (corpus-stable at `0x80083794`) carries no clean
`+0x8A`/`+0x94` SM. The loop reaches the carrier through the per-entity
update-function-pointer dispatch (no direct `jal`), so the carrier is one of
the scene's MAN-placed entities, not the player.

The encounter-record format consumed here is documented in
[`formats/encounter.md`](../formats/encounter.md). The 4-byte formation cell
at `0x8007BD0C` is the input to the battle-scene loader (`FUN_800520F0`); the
adjacent byte at `0x8007BD11` is a battle-data PROT-id selector that picks
between PROT entries `0x367` and `0x36D`.

#### Clean-room port - both overworld and field

The same SM serves overworld entities **and** field-resident carriers. The
clean-room port ([`legaia_engine_vm::world_map::step`]) is host-driven, so
`legaia_engine_core::World` ticks it in two modes:

- `SceneMode::WorldMap` via `tick_world_map` (roaming-encounter zones / town
  portals).
- `SceneMode::Field` via `tick_field_carriers`, for the scene's MAN-placed
  carriers. A `FieldCarrierConfig::ScriptedEncounter { formation_id }` sits
  Idle (towns run a 0% random rate, so its `encounter_enabled` host gate is
  `false` and it never self-fires) until the field-interact dialogue-accept
  engages it: interacting with the carrier's placement (op `0x3E`, `op0 < 100`)
  arms the engage, and accepting the prompt (the `0x4C` n5 sub-4 dialog dismiss)
  calls `World::engage_field_carrier`, advancing it Idle → Activating - so the
  field-VM bytecode drives the fight rather than a manual API. The next
  `tick_field_carriers` then runs the state-1 body (`on_activating`, the
  `entity[+0x94]` formation copy) immediately followed by the `case 2/3`
  fall-through (`on_scene_transition`, the `_DAT_8007B83C = 8` battle handoff),
  resolving the carrier's MAN formation by index and flipping Field → Battle.
  The Rim Elm Tetsu fight is `formation_id` 4. The carrier identity within the
  MAN actor-placement partition, and the field-VM bytecode that advances its
  state, remain open (see [`reference/open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

The clean-room engine ports this SM as `legaia_engine_vm::world_map::step`
(host trait `WorldMapEntityHost`). `legaia_engine_core::World` drives one
`WorldMapEntityCtx` per installed overworld entity each `SceneMode::WorldMap`
tick: the Idle state's encounter (countdown reaches zero with encounters
enabled) latches the configured formation, which the world resolves into a
battle through the same `formation_table` machinery as a field encounter,
tagged via `World::battle_return_mode` to return to the overworld rather than
the field.

Each entity carries an optional per-entity role
(`World::WorldMapEntityConfig`, paired by index with the SM list and installed
via `install_world_map_entities_with_configs`):

- `EncounterZone { formation_id }` - the entity spawns its own formation when
  it fires, instead of the map-wide shared one.
- `Portal { target_map }` - engaging the entity
  (`World::engage_world_map_entity`, the clean-room stand-in for retail's
  player-position-in-zone trigger) drives the SM to its transition state and
  surfaces a `FieldEvent::WorldMapTransition { target_map, slot }` for the host
  to load the target scene. `target_map` is the 7-id door-warp `map_id` the
  placement classifier reads off a partition-1 actor's `0x3E` warp.
- `OverworldPortal { scene_name, index, entry_x, entry_z, dir }` - an overworld
  town/dungeon entrance sourced from the disc's `.MAP` walk-on tile-trigger ->
  MAN partition-2 record -> `0x3F` named-scene-change bridge
  (`man_field_scripts::overworld_portal_sites`). This is the **real** overworld
  hop: the kingdom hubs (`map01`) have **no** partition-1 `Portal` placements -
  each gate-1 kind-1 tile trigger references a partition-2 record whose script's
  `0x3F` carries the destination scene name + arrival tile straight from
  bytecode. Engaging it surfaces the same `WorldMapTransition` (the `slot` points
  back at the config); unlike `Portal`, it needs no `MapIdResolver` - the CDNAME
  destination is in the data. `SceneHost::enter_world_map_scene` seeds one per
  bridge site at its trigger-tile centre, and the auto-engage-on-walkover trigger
  fires it when the player steps onto that tile.

The producer/consumer seam: `WorldMapTransition` is emitted by the entity SM's
`on_scene_transition` and **drained by `SceneHost::tick`** (the sibling of the
named-`0x3F` drain). For an `OverworldPortal` the drain loads `scene_name`
(field or world-map) and seats the player at the entry tile - the same arrival
semantics as the named warp; for a door `Portal` it resolves `target_map`
through the `MapIdResolver`. The story-gating is **the entrance record's own
C1/C2 gate**: `SceneHost::enter_world_map_scene` runs each bridge site's
partition-2 record through `partition2_record_gates` + `World::p2_record_gates_pass`
(retail `FUN_8003BDE0` - C1 blocks the spawn if ANY listed flag is set, C2
requires ALL set) and installs an `OverworldPortal` only when the gate passes.
Most Drake entrances carry empty gates and stay unconditional; the Ravine
(`keikoku`) portals carry `C1=[0x193]`, so a fresh continent arrival (flag clear)
keeps them reachable and setting `0x193` drops them from the installed set. This
is per-visit install-time gating - the portal set is rebuilt each time the
overworld loads, so a flag that latched during a dungeon run re-gates the
entrance on the next arrival.

Overworld walk-on **beat** records are the other half. Not every gate-1 kind-1
tile trigger on a hub is a portal: the Drake mist-wall force-walk bands (`map01`
partition-2 records `P2[34..36]`, `C1=[0x482]`) carry no `0x3F` - they shove the
player back off a not-yet-unlocked path while their flag is clear.
`SceneHost::dispatch_walk_on_trigger` runs in **both** field and world-map mode:
on the overworld a gate-1 trigger whose record IS a portal (has a `0x3F`, tested
by `p2_record_is_portal`) is left to the entity SM above, and only non-portal
beat records spawn - through the same `install_gated_p2_record` cutscene-timeline
path a town walk-on beat uses, so the `C1` one-shot latch is honored (the band
force-walks while `0x482` is clear, stops once it sets). While such a timeline
runs, `step_world_map_locomotion` stands the overworld player down (the force-walk
lock) and `World::tick`'s world-map arm steps the timeline whenever one is active
(not just during the opening `map01` fly-in).

- `Npc { interact_id, text_id, inline }` - surfaces a `FieldEvent::FieldInteract`
  with that id. `inline` is the record's structural inline dialog-text block (see
  [dialog text source](#npc-dialogue-text-source)); `tick_world_map` opens it
  (sets `World::current_dialog` + emits `FieldEvent::OpenDialog`, both carrying
  the inline bytes) when the player presses confirm while standing within one
  tile of the entity, and dismisses it on the next confirm/cancel press. This is
  the overworld talk-to path - portals are walk-onto, NPCs are talk-to. (`text_id`
  is no longer sourced from the script - it was mis-read off the `0x3F` op, which
  is the named scene-change, not a dialog op; the MAN classifier now passes
  `None` and the inline block is the text source.)

#### NPC dialogue text source

Placement-NPC and event dialogue text is **inline** in the record, not in the
scene MES container - and it is found **structurally**, not by a dialog opcode.
A field-scene interaction record carries its message as a run of `0x1F`-lead /
`0x00`-terminated MES-glyph segments; `first_inline_dialog_offset` locates the
first such segment directly (the field-VM walk can't be trusted inside text - it
desyncs on glyph bytes that look like opcodes). `OwnedDialogPanel::from_inline_dialog`
skips to that `0x1F` lead and decodes the segment through the standard MES
interpreter; `SceneHost::open_pending_dialog` prefers this inline path and falls
back to a `text_id` -> scene-MES lookup for the message-table dialogue paths. The
geometry-header layout and multi-segment (full menu) rendering are not yet pinned
- the first segment renders today.

> **Not the `0x3F` op - and not any opcode.** Earlier notes attributed this
> inline text to a `0x3F` "Dialog" opcode; `0x3F` is actually the **named
> scene-change** (it copies a destination scene *name* and calls the scene-change
> packet `FUN_8001FD44`; see [`script-vm.md`](script-vm.md) and
> [scene destinations](#scene-destinations)). In fact field dialogue has **no
> dedicated opcode**: the field-interact op (`0x3E`, `op0 < 100`) arms the actor's
> interaction context, and the per-frame actor-dialog SM (`FUN_80039b7c`) + pager
> (`FUN_801D84D0`) display the actor's inline interaction-script MES text - the
> structural `0x1F` pool above. See
> [`script-vm.md` § Field dialogue](script-vm.md#field-dialogue-has-no-opcode).
> The `0x3F`-as-dialog reading only arises when the over-approximating walk
> desyncs on a literal `?` (`0x3F`) inside a message.

Entities without a config fall back to the shared formation and a generic
interaction.

The "player walking" gate that suppresses the talk-to / interaction path reads
the d-pad direction bits (Up/Right/Down/Left), the same bits the locomotion
step consumes - not the face buttons, so a confirm press is never mistaken for
movement.

### Overworld player movement + region-keyed encounters

The overworld now has a moving player and a position-routed random-encounter
roll. `tick_world_map` walks the player actor from the held d-pad
(`World::step_world_map_locomotion`, direct screen-axis mapping at
`World::WORLD_MAP_PLAYER_SPEED` units/frame) and, on each 128-unit tile the
player crosses (`World::live_world_map_tick`, mirroring the field
`live_field_tick`), rolls the scene's region-keyed encounter table
(`World::set_world_map_regions`). That table is the clean-room port of
`FUN_801D9E1C` ([`region_encounter`](../formats/encounter.md#engine-port-region-keyed-roll)):
the player's tile selects the first region whose AABB contains it, the region's
rate increment depletes a step counter, and a `<= 0` counter rolls a formation
from the region's `[base, base + count)` slice and latches
`pending_world_map_encounter` - which the same tick resolves into a
`SceneMode::WorldMap → SceneMode::Battle` transition that returns to the
overworld. A camera-only world map (no region table routed) is unchanged.

In walk mode the native `play-window` camera **follows the player**: it passes
the player's AABB-relative world position as the `pan` offset to
[`window::world_map_camera_mvp`](../../crates/engine-render/src/window.rs), so
the framing centre tracks the player as they walk; the top-view debug camera
keeps the controller's free scroll.

### Overworld collision / walkability

Overworld walkability is **not** a separate format. The world-map-walk overlay's
free-movement controller is byte-for-byte the field locomotion integrator
`FUN_801d01b0`, and it collides through the same `FUN_801cfe4c` against the same
per-scene walkability grid at `*(_DAT_1f8003ec) + 0x4000` (see
[`field-locomotion.md`](field-locomotion.md)). The three kingdom overworld
scenes carry real wall data in that grid: the `0x12000`-byte field-map block's
`+0x4000..+0x8000` region holds thousands of wall sub-cells (map01 ≈ 7968,
map02 ≈ 2283, map03 ≈ 3837 high-nibble bits). The engine loads it through the
same [`Scene::field_collision_grid`](../../crates/engine-core/src/scene.rs)
path as the field and steps the overworld player through the shared
`World::advance_with_collision`, so walls stop the player exactly as on the
field.

### Camera-relative movement remap

The held d-pad is remapped through the overworld camera azimuth so "screen up"
walks the player toward the top of the screen and "screen right" walks
screen-right, regardless of how the map is framed - the same camera-relative
remap retail's `func_0x800467e8` applies (it feeds the pad through the camera
yaw the renderer uses). `World::world_map_camera_relative_bits(azimuth, sx, sy)`
rotates the screen delta into world space against the same azimuth
[`window::world_map_camera_mvp`](../../crates/engine-render/src/window.rs) frames
the eye with (`eye = center + (d·cosθ, +0.7d, d·sinθ)`). The kingdom pack is
drawn at raw retail Y-down coordinates; the world-map cameras compose a single
world Y-negation (the same one-negation frame the field render uses), so the
top-view camera still frames the **negated** Y range and sits at *positive* Y
looking down on the terrain. Because that overhead view inverts
the on-screen vertical axis relative to the eye→centre direction, the
world→screen axes are taken from the **real camera matrix, not a
hand-derived guess**: a disc-free projection test
(`crates/engine-shell/tests/world_map_camera_remap.rs`) projects the chosen
world direction back through `world_map_camera_mvp` and asserts it moves the
right way on screen for every azimuth, keeping the remap in lock-step with the
camera. The native `play-window` feeds the same controller azimuth to both the
camera and the remap, so they cannot drift.

### Walk-view camera (retail model, RAM-pinned)

The walk-view (free-roam) camera follows the same GTE composition as the
field and battle cameras, with its constants read directly out of the
overworld resident savestates' RAM (`sebucus_overworld_resident` /
`karisto_overworld_resident`):

```
screen = H * (R * (S*(v - focus)) + TR) / Ze     R = Rx(pitch) * Ry(azimuth)
```

- `H = _DAT_8007B6F4 = 368` (GTE projection register; both kingdoms).
- `S`: the base matrix `DAT_8007BF10` holds `24576 * I` on the overworld -
  a **6.0× uniform world scale** (the battle sibling holds `16384 * I` = 4×).
- Rotation trio at `_DAT_8007B790`: pitch-only in both captures
  (`(360, 0, 0)` / `(476, 0, 0)`); the azimuth global `_DAT_8007B794` feeds
  `ry` when the player rotates the view.
- `focus`: the player's world X/Z - `_DAT_80089118/20` hold its negation
  (the same negated-focus convention the field follow-cam uses), focus Y
  (`_DAT_8008911C`) = 0.
- `TR` from the `_DAT_800840B8` trio: `(0, 536, 9139)` in the Sebucus
  capture, `(0, 406, 11041)` in the Karisto capture - two pinned zoom
  states along one axis (closer = shallower pitch).

`play-window`'s walk view implements exactly this composition
(`psx_camera_mvp` + the 6× scale + the player-focus translation), sliding
between the two pinned zoom anchors on the controller's zoom input. The
top-view debug camera keeps its synthetic framing.

### Boot-path seeding

The overworld seeds itself on the natural scene-transition path, not just the
explicit `--world-map` entry. The overworld shares game mode `0x03` with
towns/fields, so retail distinguishes it by the loaded scene; the engine
mirrors that with [`is_world_map_scene`](../../crates/engine-core/src/scene.rs)
(the three kingdom `mapNN` labels). When the field VM issues a
`scene_transition(map_id)` that resolves to an overworld scene,
[`SceneHost::tick`](../../crates/engine-core/src/scene.rs) routes it through
`SceneHost::enter_world_map_scene` instead of the plain field path:
`enter_field_scene` (resources + walkability grid + player) followed by routing
the region-keyed encounter table from the scene's MAN and switching into
`SceneMode::WorldMap`. The `--world-map` window flag and `enter_world_map_live`
now delegate to the same `enter_world_map_scene`, so both entries seed
identically.

### Entity / actor placement table

The scene's on-map entities (NPCs, landmarks, the towns/portals you enter) are
the **MAN partition-1 actor-placement records**, decoded by `FUN_8003A1E4` and
ported as
[`ManFile::actor_placements`](../../crates/asset/src/man_section.rs) /
[`Scene::field_actor_placements`](../../crates/engine-core/src/scene.rs). The
scene-init routine `FUN_8003AEB0` runs `FUN_8003A1E4` over partition-1 records
`1..N1` (record `0` is the scene-entry controller whose script is the
[scene-entry system script](field-locomotion.md)). Each record is:

```
[u8 local_count N][N × 2 bytes locals][u8 model][u8 anim_id][u8 tile_x][u8 tile_z][field-VM script…]
```

- **model** `< 0xF0` indexes the kingdom-TMD pool from `DAT_8007b6f8`; `>= 0xF0`
  selects a special model from `_DAT_8007b824` (the lead-actor / party slot, and
  sets the actor's `0x1000000` flag).
- **anim_id** (installed into actor `+0x5C`) is the actor's clip: scene-bundle
  ANM record index + 1, `0` = none - see the [placement-header resolution
  ](script-vm.md#placement-header-model--animation-resolution).
- **tile_x / tile_z**: bits 0–6 are the 128-unit tile column / row; bit 7 shifts
  the spawn a half-tile. World position is `(b & 0x7F)·128 + (bit7 ? 128 : 64)`.
- the actor's field-VM **script** starts at `record + 1 + 2N + 4` with the
  record base as its buffer; the actor's encounter record (`+0x94`) is
  initialised to `-1` and set later by that script.

Real scenes decode cleanly: `town01` places 52 actors, the kingdom overworlds
`map01`/`map02`/`map03` place 8 / 7 / 19 (several parked at tile `(127,127)` -
preloaded models the script repositions). So the placement gives **position +
model + script pointer** for every entity.

#### Classifying the entity kind from its script

Retail has no static "entity kind" field - a placed actor *is* what its script
does. [`classify_placements`](../../crates/engine-core/src/man_field_scripts.rs)
linearly disassembles each placement's per-entity interaction script (records
`1..` are the actor interaction scripts) and reads the kind off its
distinguishing opcodes:

- a **genuine warp** (the *base* `0x3E` with `op0` in `100..=106`, retail
  `scene_transition`) → a **Portal** whose target is the field-VM map id
  `op0 - 100` (`0..=6`). The map id selects a scene-*type* code overlay
  (PROT `0x4d + map_id`), **not** a unique scene - the destination scene *name*
  is set separately by the pre-WARP handler / scene-change packet, which lives in
  an uncaptured overlay, so the id is reported raw (see
  [`asset-loader.md` → WARP opcode flow](asset-loader.md#warp-opcode--scene-transition-flow-map_id));
- an inline `0x1F`-lead **dialog-text block** or a **field interact** (`0x3E`
  with `op0 < 100`) and no warp → an **NPC** (sign / talk-to / event trigger).
  (The dialog signal is the *structural* `0x1F` text scan, not an opcode - see
  [NPC dialogue text source](#npc-dialogue-text-source);
- none of those → **Plain** (a moving / animated / model-only actor, e.g. the
  lead-actor slot).

The walk is the same over-approximating linear disassembly the encounter-arm
hunt uses (it reads every opcode-shaped byte, not only one control-flow path),
so it **desyncs inside embedded message / SJIS text** and can land on a `0x3E`
whose next byte is `>= 100`. Every such phantom in the corpus rides the `0x80`
cross-context prefix and carries an out-of-range `op0` (175 / 179 / 200, i.e.
text bytes), so the genuine-warp gate (`!extended && op0 in 100..=106`, see
[`man_field_scripts::classify_placement`](../../crates/engine-core/src/man_field_scripts.rs))
rejects it - `geremi` (a talk NPC, `op0=200`) and the leftover-JP `other7`
(`op0=175/179`) used to mis-classify as portals to non-existent maps 75 / 79 / 86
/ 100. Real data: `town01` classifies 14 NPCs / 38 plain; across the whole PROT
corpus exactly **11** genuine door-warp portals survive (all map_id `0..=6`),
clustered in interior/town scenes (`koin1` exposes exits to maps 3/4/5, `koin3`
to 6, `balden` to 3) plus a single overworld fishing-spot warp each on
`map02`/`map03` (map id 0). A disc-gated regression
(`world_map_portal_classification_disc.rs`) pins the invariant.

`SceneHost::enter_world_map_scene` seeds these typed entities on the
boot/transition path via [`World::install_world_map_entities_at`]: each Portal /
NPC placement installs a matching
[`WorldMapEntityConfig`](../../crates/engine-core/src/world.rs) **with its spawn
position** (Plain placements are skipped). So overworld portals + NPCs are
**disc-sourced**, not synthetic.

#### Scene destinations

The overworld doesn't enter towns through a partition-1 warp NPC - a corpus scan
finds none on the kingdom maps (only fishing-spot `0x3E` warps). Instead the
scene's **controller script lists every destination as a `0x3F` named
scene-change op** that carries the destination scene *name* inline (`[0x3F]`
`[i16 index][u8 name_len][name][entry_x][entry_z][dir]`; the op hands the name to
the scene-change packet `FUN_8001FD44`). So the destinations are recoverable
straight from the disc bytes - the answer the "`map_id` → scene-name table lives
in an uncaptured overlay" note assumed unreachable (that note is about the
*separate* `0x3E` door-warp, whose 7-id selector still resolves its name in an
uncaptured handler).

[`man_field_scripts::scene_destinations`](../../crates/engine-core/src/man_field_scripts.rs)
walks the partition-1 records, decodes the `0x3F` ops, and keeps each whose
inline name passes a clean-CDNAME-label gate (rejecting the text-desync phantoms
a literal `?` = `0x3F` inside a message produces). On `map01` (Drake overworld)
it recovers `town01`, `town0b`, `town0c`, `dolk`, `dolk2`, `rikuroa`, `cave01`,
`vell`, `vozz`, `suimon`, `keikoku`, `jou` - all real CDNAME scenes. Disc-gated
`scene_destinations_disc.rs` pins the set + asserts every recovered name is a
known CDNAME label.

**Live wiring.** `SceneHost` decodes + caches this table on every scene load
(`load_scene` → `refresh_scene_destinations`) and exposes it as
`SceneHost::scene_destinations()` plus a `SceneDestinationResolver`
(`SceneHost::destination_resolver()`) - an `i16`-keyed `index → scene-name`
resolver rebuilt from disc per scene. The `0x3F` op's `i16 index` is a
story/entry id in its **own** id space (observed past `u8` range, e.g. `630`),
so the resolver keys on `i16` and is deliberately **not** the `u8`-keyed
[`MapIdResolver`](#) (which serves the separate `0x3E` door-warp's 7 scene-*type*
selectors `0..=6`).

**Runtime transition (live).** The field-VM executor drives `0x3F` as a live
named scene-change: it decodes the inline destination name (gated by the
clean-CDNAME-label check), calls `host.scene_transition_named`, and
[`SceneHost::tick`] drains the resulting `World::pending_named_scene_transition`
to load that scene directly (world-map vs field routed by `is_world_map_scene`),
ahead of the `0x3E` map-id path. Field **dialogue** was re-grounded off `0x3F`
onto its real trigger - the field-interact op (`0x3E` with `op0 < 100`) opens the
interacted actor's inline interaction-script text (see
[`script-vm.md` § Field dialogue](script-vm.md#field-dialogue-has-no-opcode)).

#### Loading the kingdom geometry (engine port)

The engine port loads the scene's **kingdom-bundle slot-1 landmark TMD pack**
(Drake 40, Sebacus 36, Karisto 56 TMDs), textured by the slot-0 TIM atlas. When
`SceneHost::enter_field_scene` loads a `map\d\d` scene it selects
[`SceneLoadKind::WorldMap`], and [`SceneResources::build_targeted_with_options`]
decodes slot 1 (via [`legaia_asset::kingdom_bundle`] + [`legaia_asset::pack`])
into the TMD pool and slot 0 into the VRAM upload set, **instead of** the generic
raw/LZS `tmd_scan` sweep. The generic sweep can't follow the LZS-compressed
descriptor table, so before this it picked up only a handful of stray meshes -
the historical "the world map looks like a battle map" symptom. Only the
scene's primary kingdom entry contributes; the sub-area sibling entries are
skipped so they neither leak stray meshes nor inflate `scene_aabb`.

**The retail load mechanism (pinned).** The `DAT_8007C018` global TMD pointer
table that the world-map tile dispatcher reads is filled by a single
descriptor-walk: the per-scene field initializer `FUN_801D6704` runs
`FUN_80020118` (party meshes → `[0..4]` via `FUN_8001E890`) then `FUN_80020224`,
which walks the scene's main field file (streamed into `_DAT_8007b85c`) and
dispatches every descriptor through `FUN_8001f05c`. **Only dispatcher cases
`0x02` (TMD pack) and `0x09` (bare TMD) install** into `DAT_8007C018` via
`FUN_80026B4C`; the type-`0x05` slot-4 "MOVE" case only allocates a buffer and
never installs - so slot-4 is *not* the terrain-mesh source.

**The walk-view pool (pinned).** A real `map01` walk-view capture (game mode
`0x03`, standing on the Drake overworld) settles `DAT_8007C018` to exactly **45
entries**: `[0..4]` = 5 party meshes (heap addresses ~`0x8014xxxx`), `[5..44]` =
**the 40-mesh slot-1 landmark pack** (`prefix = 5`, install count 45). So the
walk-view pool *is* the landmark pack the port already loads. (0085's and 0093's
slot-0 atlases target the *same* VRAM pages, so the walk-view and overview pools
are mutually-exclusive sets that clobber each other if co-loaded - see
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).)

Parsed live (absolute-pointer-fixed-up TMDs), the 40 scene meshes are all
**small object-local tile/prop meshes** (dx/dz ≤ ~768 = a few 128-unit tiles,
centred near origin, Y ≤ 0) - not one map-spanning stage. These 40 are the
**landmark layer** (trees, mountains, the castle); they are *not* the continent
ground, which is a procedural heightfield (see below).

**Walk view ≠ overview - different pools, different placement.** The two retail
sweeps over the `.MAP` object grid serve two different render modes:

- **Overview** (top-down, game mode `0x0D`): `FUN_801F69D8` over the
  [`CELL_VISIBLE`](../../crates/asset/src/field_objects.rs) (`0x2000`) cells.
  Mesh = record `+0x10` geometry id (Drake: 134 distinct records, `+0x10` up to
  63/105) → indexes the *larger* overview pack. This is what
  [`legaia_asset::field_objects::parse_terrain_tiles`] + `pack_mesh_index`
  models, and it is correct **for the overview pool only**.
- **Walk** (game mode `0x03`): the bulk continent ground is **not** a per-cell
  pack-mesh sweep at all - it is a procedural heightfield (corner heights from
  the `+0x4000` floor-nibble grid, confirmed by `FUN_80019278`; see below). The
  `.MAP` records' `+0x10` field is used only by the sparse placed-landmark layer
  (`FUN_8003A55C`, `flags & 0x4`); for the bulk ground cells `+0x10` is 0.

The walk-placer `FUN_8003A55C` (placed flag `0x4`) spawns only the ~51
interactive objects (distance-culled to ~14 live actors in the capture; most
live actors are script-spawned, not from the placed-flag set). It allocates via
`FUN_80024c88` → `FUN_80020de0` (free-list `FUN_80020454`, pool `_DAT_8007c354`),
stores the record index at actor `+0x60`, and leaves the mesh chain `+0x44` at
0; the mesh is resolved from the record index by the scene draw loop (resolver
not yet pinned). These are props/entities, not the bulk continent.

#### Placing the continent terrain (engine port)

The kingdom slot-1 meshes are object-local, so the continent must be assembled
by **positioning** them per tile. The **overview** layer is modelled today:

- **Overview terrain** ([`Scene::field_terrain_tiles`] →
  `resolve_world_map_terrain_draws`): the `FUN_801F69D8` visible-bit
  (`0x2000`) cells (Drake 970, Sebacus 184, Karisto 161), mesh via record
  `+0x10`. This targets the *overview* pack; against the 40-mesh walk pool the
  high indices resolve to no mesh.
- **Interactive objects** ([`Scene::field_object_placements`] →
  `resolve_field_placement_draws`): the placed-flag (`0x4`) records (Drake 51,
  Sebacus 20, Karisto 24), the `FUN_8003A55C` set.

Both resolve through `resolve_placement_draws`: each tile draws the pack mesh at
`(col*0x80 + x_off, floor_height + y_off, row*0x80 + z_off)`, Y-flipped, in the
shared player / entity-marker world frame. Positions match live actor positions
from a top-down (`game_mode 0x0D`) save state.

**Walk-view placement mechanism (traced).** A walk capture shows the continent
is drawn by the **actor system**, not a flat table. The per-actor render
dispatcher `FUN_8001ADA4` (driven by `FUN_80016444` → `FUN_8001d140`) switches on
`actor[+0x56]`; **case 5** draws `actor[+0x44]` as a *mesh chain* whose entries
point at `pool_tmd + 0xc + obj*0x1c` (the TMD object headers), so **one actor
draws one pool TMD**. The terrain/object actors are spawned by `FUN_8003A55C`
(the `.MAP` object-grid placer) into pool `_DAT_8007c354`, with `actor[+0x60]` =
the `.MAP` grid record index and `actor[+0x90]` = the object's MAN interaction
script. A direct walk of the live render list (head `*(0x8007C354) = 0x80083BCC`,
via node `+0x0`; for each actor `+0x56` = render mode, `+0x60` = record index,
`+0x44` = mesh chain whose first entry equals `DAT_8007C018[i] + 0xc`) gives the
`rec → pool` pairs: `349→11, 414→36, 430→34, 474→21, 411→19, 409→7, …` - the pool
is `actor+0x64` (see below), matched exactly 14/14.

**The per-object pool index is `record[+0x10] + prefix`** (pinned via
`ghidra/scripts/find_mesh_chain_writer.py`, confirmed 14/14 against the live
render list). The chain `actor+0x44` is built by `FUN_80024d78` from
`DAT_8007C018[ *(u16*)(actor+0x64) ]` (the `-0x7ff83fe8` constant resolves to
`0x8007C018`): `chain[0] = tmd[+8]` (object count), `chain[1+i] = tmd+0xc+i*0x1c`.
So `actor+0x64` is the `DAT_8007C018` pool index, and `FUN_80020f88` sets it as

```text
actor+0x64 = *(s16*)(_DAT_1f8003ec + (actor+0x60)*0x20 + 0x10) + DAT_8007b6f8
           = .MAP_record[obj_idx].+0x10 (model) + prefix          (prefix = 5)
```

i.e. **`pool = record[+0x10] + prefix`** - the existing
[`legaia_asset::field_objects::pack_mesh_index`] (`+0x10`) *plus* the prefix.
Confirmed at the real live `.MAP` buffer `_DAT_1f8003ec = 0x80139530`
(`474 → 16+5=21`, `349 → 6+5=11`, `414 → 31+5=36`, `430 → 29+5=34`,
`411 → 14+5=19`). `FUN_80024e08(actor, model)` is the direct set-model primitive
for script-driven (non-`.MAP`-grid) actors. The **walk continent grid gate is
cell bit `0x1000`** (15389 cells in the live grid), distinct from the overview's
`0x2000` (304 cells) - so the walk sweep is `(cell & 0x1000)`, not
`parse_terrain_tiles`'s `0x2000`. MAN partition 1 supplies the NPC/portal/party
placements (decoded via
[`legaia_asset::man_section::ManFile::actor_placements`]).

**The continent ground is a procedural heightfield, not instanced meshes.**
This is confirmed by **`FUN_80019278`** (SCUS, always-resident - unambiguous, no
overlay aliasing), the bilinear **ground-height sampler**: from an entity's XZ
(`actor+0x14/+0x18`) it reads the object-grid cell (`+0x8000`, tests the `0x1000`
walk bit and the `0x1800` mask, sets the actor's `0x800000` off-map flag), then
reads the 2×2 floor-nibble block at `+0x4000` (`grid[0],[1],[0x80],[0x81]`, each
`& 0xf`) and **bilinearly interpolates** the floor height from the four corner
LUT values (`DAT_1f80035c[nibble]`) weighted by the sub-tile position
(`pos & 0x7f`), `>>0xe` (÷128²). So the `+0x4000` grid is **terrain elevation**,
the `0x1000`-gated continent is a smooth heightfield surface, and the slot-1
**pack meshes are only the sparse placed landmarks** (the `pool = record[+0x10] +
prefix` set above, spawned by `FUN_8003A55C` gated on `flags & 0x4`).

The only per-cell terrain emitter that sweeps this grid is **`FUN_801F69D8`**
(the **top-view overview** renderer, gate `cell & 0x2000`): for each occupied
cell it reads the object record (grid base + `(cell & 0x1ff) * 0x20`), takes the
**mesh-pool index from `+0x10`** (`record[+0x10]` plus a per-scene base, into a
drawable-pointer table), computes the same bilinear corner height as
`FUN_80019278`, and submits the per-cell mesh through `FUN_80043390` - the sole
caller of that bulk-terrain emit. The overview ground is therefore **per-cell
meshes** keyed by `+0x10`, each carrying its own TMD UVs. `FUN_80019278` (the
height math) is the reliable anchor and is all the heightfield port needs for
correct geometry. (The earlier `FUN_801F5748` write-probe lead is dead: in a
genuine continent-**walk** RAM image that address disassembles as data, not
code - the `0x801F76xx` range aliases across overlays.)

**Engine status.** The continent ground now renders as a **heightfield
surface**: [`Scene::walk_heightfield`] →
[`legaia_asset::field_objects::build_walk_heightfield`] sweeps the `0x1000`
cells and emits one quad per cell, each corner's Y taken from the `+0x4000`
floor-nibble grid via the floor LUT (the `FUN_80019278` math). The baked
corner height is `-lut[nibble]` - **already the same world height the
placement / actor transforms carry in their un-flipped translation** - so
`play-window` draws the heightfield **without** the mesh Y-flip the pack
meshes get (flipping it re-negates the elevation: every raised cell sinks to
twice its elevation below its own buildings - in town scenes that hid the
whole cliff-top core, including Rim Elm's spawn plaza). `play-window`
uploads it and draws it as the ground, with the placed landmarks
([`Scene::walk_object_placements`], the `flags & 0x4` slot-1 pack meshes via
`record[+0x10]+prefix`) on top. Verified against the real disc: map01/02/03
build dense (>10k-quad) heightfields with genuine elevation variation. The old
`walk_terrain_tiles` per-cell pack-mesh sweep - which flooded ~97% of cells
with pool-5 because the bulk-terrain records carry `+0x10 == 0` - is removed.

**Slot-4 vertex-pool inspection overlay.** The kingdom bundle's slot 4 (the
per-kingdom object-mesh library - confirmed object-local GTE vertex pools, see
[`world-map-overlay.md`](../formats/world-map-overlay.md)) is decoded onto
[`SceneResources::world_map_slot4`] for every `SceneLoadKind::WorldMap` scene
(and only those). With `LEGAIA_WORLDMAP_SLOT4=1`, `play-window` builds a
colour-by-`kind` `LineList` from
[`legaia_asset::world_map_overlay::wireframe_segments_3d`] and merges it into the
world-map overlay-lines buffer, so the decoded pool is visible in the live 3D
view. It is an **inspection overlay, not faithful world geometry**: the segments
use the group-polyline topology convention and the records render at their raw
object-local coordinates, because the per-object placement transform and true
triangle topology live in the unpinned cluster-A command stream. Off by default.

### Ground texturing

The walk-view continent ground is drawn as a field of **`POLY_FT4` (cmd `0x2C`)
textured quads, one `32×32`-texel quad per visible cell** in a window around the
player, emitted in a **row-major world-cell sweep** (the quads sit in contiguous
runs in the prim pool, screen-X stepping along each swept row). Each cell's
texture is selected **per cell** from a **terrain-type-keyed multi-page atlas** -
grass, mountain, water, and forest cells each sample a different VRAM page.

The selector is the cell's object-record `+0x14..+0x18` run (the record reached
through `cell & 0x1ff` → `×0x20`), byte-verified against the retail prim pool:

| record byte | meaning |
|---|---|
| `+0x14` | `8×8` atlas **tile** index (`u = (id % 8) × 32`, `v = (id / 8) × 32`); `0..63` |
| `+0x15` | PSX **`tpage`** word - the terrain VRAM page (= terrain type) |
| `+0x16..+0x18` | PSX **`clut`** (CBA) word (`r[0x16] | r[0x17] << 8`) |

**Corner→texel orientation.** Within each cell's `32×32` rect, **U runs along
+X/col** (left edge = the tile's `u_lo`) but **V is flipped relative to +Z/row**:
the low-Z (row) corner takes the tile's *bottom* texel row, the high-Z corner the
*top* row. Measured camera-independently from the retail prim pool - recovering
each ground `POLY_FT4`'s world `(col, row)` (run-aligned + per-cell tile/page/clut
matched) and reading its per-corner UVs gives `(c,r)→(u_lo,v_hi)`,
`(c,r+1)→(u_lo,v_lo)` for **~96–100%** of cells across the mountain + coast
captures and *every* terrain page (a uniform vertical mirror, not a per-cell
rotation; the `<4%` residue is projection edge-noise, no systematic alternate).
Baking V the other way mirrors every tile in place: uniform tiles (grass) still
look right, but directional transition tiles (coastline sand, ridge faces) face
the wrong way and break continuity with their row-neighbours.

Observed `+0x15` pages: `0x1A` fb `(640,256)` **grass**, `0x0C` fb `(768,0)`
**mountain/rock** (a full `8×8` atlas), `0x1B`/`0x1C` fb `(704/768,256)`
**water**, `0x0B` fb `(704,0)` **forest/coastal**, with a family of CLUTs per
page in VRAM rows `495..509`.

**How it was pinned** (`scripts/ghidra-analysis/analyze-walk-ground-tiles.py --verify-rule`):
the ground quads emit in world-cell-sweep order, so aligning a quad run's
UV→tile-index sequence to the walk `.MAP`'s `+0x14` grid finds an exact match;
on the aligned cells the quad's tile / page / clut equal the record's
`+0x14` / `+0x15` / `+0x16..+0x18` for **100%** of cells across mountain + coast
captures.

> The earlier reading - "single `0x1A` grass page, positional `(col % 3,
> row % 3)`, `+0x14` unused metadata" - was a **misread**. Grass cells happen to
> use page `0x1A` with `+0x14` in the top-left `3×3` block of the `8×8` atlas, so
> the mod-3 cross-row sequence was coincidental. `+0x14` **is** the tile
> selector; the page/CLUT come from `+0x15`/`+0x16`.

**Engine.** [`build_walk_heightfield`] reads each visible cell's record and bakes
the per-cell tile UV (`+0x14` → 8×8 atlas) into `WalkHeightfield::uvs` and the
per-cell `[clut, tpage]` (`+0x15`/`+0x16`) into `WalkHeightfield::cba_tsb`, so a
single ground mesh samples grass / mountain / water / forest pages per cell;
`play-window` draws it. `GROUND_ATLAS_TPAGE` / `_CLUT` remain only as the grass
fallback for cells whose record carries no terrain run. (Distinct from the
**top-view** bulk continent, which is per-cell *meshes* via `FUN_80043390` / the
MAN `0x7F`-sentinel resolver - see
[`world-overview-viewer.md`](world-overview-viewer.md).)

**Ocean animation.** The water tile is a 4bpp texture at fb `(768, 256)` whose
CLUT row at fb `(0, 506)` (CBA `0x7E80`) the retail engine DMAs one of 13
precomputed BGR555 frames into each animation step - the rolling-wave shimmer.
The 13-frame table is the shared global asset across the three kingdoms; the
tile texture + base CLUT are per-kingdom ([`legaia_asset::ocean`]). In the live
engine the ocean texture + base CLUT already land in VRAM via the kingdom
slot-0 TIM pass, and the heightfield's water cells reference that CLUT (~39% of
map01's verts carry CBA `0x7E80`), so `play-window` animates the sea by writing
each frame's 16 entries into the CPU VRAM CLUT row at `(0, 506)` and
re-uploading (`OceanAnim` / `advance_ocean_animation`). The exact retail DMA
cadence isn't pinned, so the engine's frame interval is a tuned approximation.
The cycle is live-verified on **all three kingdoms**: resident Sebucus /
Karisto captures hold a mid-cycle frame of their own bundle's 13-frame strip
at the `(0, 506)` head (`crates/engine-shell/tests/world_map_ocean_clut_live.rs`).

The cycling reaches beyond the row-506 head, on a precisely censused column
set (per-column variance across the ten map01-CLUT-resident capture states):
row 506 animates cols `0..48` (head + a second block head + a rotating ring
of STP-set ocean near-copies at 32..39 + the runtime-generated pure-channel
tail at 40..47, all phase-locked to the ocean), row **508** animates head
entries `{1, 14, 15, 26, 27}` and live-maintains a mirror `[32..47] ==
[0..15]` over a disc-base palette it overwrites, and row **509** animates
exactly cols `{42, 43}`. Row 507 is fully static. The `(48, 500)` sibling
destination (next paragraph) is censused dynamic too - cols `62..63` vary
across the map01-band captures, so the whole 16-wide cell is excluded (a
`MoveImage` rewrites all 16 entries; the stable columns merely coincide
across the strip's frames). The VRAM parity oracle excludes exactly the
censused columns for world-map scenes and asserts the rest
(`vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS`).

The destination-cell set is **kingdom-universal**: on the resident Sebucus /
Karisto captures every censused destination cell - `(0/16/32, 506)`,
`(0/16/32, 508)`, `(32, 509)`, `(48, 500)` - holds a 16-px-aligned window of
that state's own strip park rows (same test file, cross-kingdom leg), i.e.
the copy family runs against per-kingdom strips with kingdom-invariant
destination operands. The row-508 mirror, by contrast, is a **map01 script
behaviour**: on Sebucus / Karisto the `(32, 508)` cell holds strip content
that differs from `(0, 508)`.

**The writer is the script-driven CLUT-cell effect family**, not a single
hardcoded DMA. A map01-resident capture's libgpu command queue holds the
smoking gun: 16×1 GP0 `0x80` VRAM→VRAM copy packets whose destinations are
exactly the censused cells - `(0/16/32, 506)`, `(0/16/32, 508)`, `(32, 509)`
plus a `(48, 500)` sibling - and whose sources walk frame strips parked in
VRAM rows 498 / 501..505 in 16-px steps (the 13-frame palette banks). Two
field-overlay handlers emit them:

- `FUN_801E4C58` - the field-VM `0x4C` n6 sub-`0x61` emitter: a one-shot
  16×1 CLUT-cell write whose **coordinates are script operands** (source
  `(x, y)` at instruction `+5`/`+7`, destination at `+9`/`+0xB`, read via the
  misaligned-u16 helper `FUN_8003CE9C`). Non-zero source-y enqueues a libgpu
  `MoveImage` cell copy; zero source-y replicates the `+5` halfword as a flat
  BGR555 colour across all 16 entries and `LoadImage`s it.
- `FUN_801E4794` - the multi-frame **cross-fade** state machine (installed
  via the `[0xFFFF0000][handler]` descriptor records at `0x801F291C+`):
  captures two 16-colour cells (`StoreImage` of `+1`/`+3` and `+5`/`+7`),
  precomputes per-entry per-channel step deltas `(B−A)/frames` (`+0xD`),
  accumulates them each tick against the scratchpad frame-delta byte
  `0x1F800393`, and `LoadImage`s the repacked cell to `+9`/`+0xB`.

Both bottom out in the statically-linked libgpu (`MoveImage FUN_80058490`,
which patches the static 5-word GP0 packet template at `0x80078DFC`;
`LoadImage FUN_800583C8`; `StoreImage FUN_8005842C`) - which is why no
`y = 506/508/509` rect constant exists in any code image: the rows live in
the scene's field-VM bytecode operands. The lockstep phase coupling across
rows comes from sibling script ops sharing the frame counter, not from one
wider rect.

The field-file loader `FUN_8001f7c0` (`ghidra/scripts/trace_field_loader.py`) is
**dual-mode**, gated on two globals:

```c
if (_DAT_8007b868 == 0 && _DAT_8007b8c2 != 0)   // RETAIL
    FUN_8003e8a8(param_3, 1);   // param_3 = PROT entry index
else                                            // DEV-HOST
    FUN_8003e6bc("DATA\FIELD\<scene>.MAP", ...) // break 0x103 fopen on the dev PC
```

On **retail** (`_DAT_8007b8c2 != 0`, `_DAT_8007b868 == 0` - both confirmed live)
the `.MAP` is resolved purely by **PROT entry index**: `param_3` is read by the
field-init caller (`FUN_801d6704` @ `0x801d6ae8`) from the global at
**`0x80084540`** (the word right before the scene-name string at `0x80084548`),
and `FUN_8003e8a8` indexes the in-RAM PROT TOC at `0x801c70f0`
(`toc[index+2]` = start_lba; the trace verifies the `0x801c70f0` constant inside
`FUN_8003e8a8`). So the entry a scene loads is pinned by `0x80084540`, and a live
Drake walk capture reads **`0x80084540 = 0x55 = 85`** → the walk `.MAP` is **PROT
CDNAME/runtime index 85**, which `FUN_8003e8a8` resolves to `toc[87] = 3243` →
**PROT.DAT offset `0x655800`** - and that region is the `.MAP` records+grid
**raw** (99.7% byte-identical to the live buffer; records & walkability 100%, no
compression). NB the per-entry *extractor* mis-slices this: its `0085_map01.BIN`
is a `[u16 count=46][46×u16 offsets]` field-object/script pack at `0x668000`,
**not** the `.MAP`; the real `.MAP` is filed under the overlapping manifest entry
83 (the extractor's entry numbering is offset ~2 from the runtime `toc[p+2]`).

The **`break 0x103`** path (`FUN_800608f0`) is the **dev-host `fopen`** - a PsyQ
host-link open of a real `DATA\FIELD\<scene>.MAP` (+ `<scene>.PCH` at `+0x12000`,
+ `\efect.dat`; extensions from `DAT_8007b3bc`/`DAT_8007b3c4`, scene name from
`0x80084548`) on the *developer's PC*. It carries **no** extension→PROT mapping,
the retail disc has no ISO9660 `DATA\FIELD\` tree, and it is never taken when
`_DAT_8007b8c2 != 0`. So the resolver to read for retail is the `0x80084540`
PROT-index dispatch, not the trap. The walk/overview split is just the scene name
→ index: `map01 = 85` (walk, entry `0085`) vs `opmap01 = 768` (overview, block
`0768..0772`). So the walk `.MAP` is the **raw** records+grid region at PROT.DAT
`0x655800` (`toc[87]`, no compression); the landmark mesh resolver is `pool =
record[+0x10] + prefix`, and the bulk ground is the `0x1000`-gated heightfield
(Engine status, above).

#### Rendering the placed entities

[`World::world_map_entity_markers`](../../crates/engine-core/src/world.rs) is the
render-agnostic seam for the installed placements: one
`WorldMapEntityMarker { world_pos, kind }` per entity that carries a position,
pairing the placement coordinate with its coarse `WorldMapEntityKind`
(Portal / Npc / EncounterZone). The marker `y` is the player actor's current
plane (the placements are 2D), so markers sit on the walking plane. The native
`play-window` draws each as a kind-coded upright marker (a vertical post plus a
small base cross, colour-keyed: portals cyan, NPCs green, encounter zones red)
through the Lines pipeline - the same overlay slot the effect outlines use, and
mutually exclusive with them since no effects spawn on the world map. The
markers share the player's coordinate frame (both come from the scene MAN), so
they read correctly relative to the player even while the kingdom terrain mesh
still renders at its own pack-local coordinates (binding each placement to its
own actor model is the still-open per-entity mesh thread). Config-only installs
(no disc placements) produce no markers, so a camera-only world map draws
nothing extra.

The player itself is drawn the same way:
[`World::world_map_player_marker`](../../crates/engine-core/src/world.rs)
returns the player actor's position plus heading (the player's own mesh is not
drawn in world-map mode), and `play-window` draws a distinct white-yellow
marker - a taller post, a base cross, and a facing tick pointing in the
heading. Because the world-map walk uses the camera-relative direction bits
rather than the field `decode_field_direction`, `step_world_map_locomotion`
records the heading into the actor's `render_26` field itself (the same field
the field path stores), so the facing tick tracks the walk direction
deterministically. The player + entity markers build into one Lines mesh.
Diagonal movement applies the same `speed -= speed >> 2` normalise as the field
controller (and the retail walk overlay): `advance_with_collision` steps both
axes equally, so a diagonal would otherwise travel ~1.41x the cardinal speed.

#### Auto-engage on walk-over

The portals fire themselves. [`World::auto_engage_world_map_portals`] runs each
`tick_world_map` (right after locomotion, before the entity-SM step): any
`Portal` entity whose placement tile (`pos >> 7`) matches the player's current
tile is driven to its transition state - exactly what a host
[`World::engage_world_map_entity`] call does - so the same tick's SM step
surfaces the [`FieldEvent::WorldMapTransition`] with the portal's target map and
the host loads the destination. Only `Idle` portals are engaged (a portal fires
once per visit; standing on the tile doesn't re-trigger). NPCs are **not**
auto-engaged - they are talk-to, driven by the SM's idle-interact path, not
walk-onto. The clean-room stand-in for retail's per-entity
player-position-in-zone check; the region table (the random-encounter driver)
is the other half of overworld gameplay, fully boot-path seeded.

The pointer at `entity[+0x94]` is set by field-VM op handlers inside the
script VM dispatcher (`FUN_801DE840`); see
[`subsystems/script-vm.md`](script-vm.md) and the op-handler family at
`0x801DEEDC` / `0x801DEF08` / `0x801DEFA0` / `0x801DF038` / `0x801DF3FC` /
`0x801E1C38` / `0x801E1F44` / `0x801E21C0`. Each handler is a different
"trigger encounter on actor X" op; they all share the clause:

```mips
sw   <record_ptr>, 0x94(<actor>)
sh   $zero,         0x54(<actor>)
ori  $tmp, $tmp, 0x400      ; raise "encounter armed" flag in actor[+0x10]
sw   $tmp,         0x10(<actor>)
```

## Render pipeline

The per-frame world-map render dispatches from the SCUS-resident game loop
into the overlay-loaded code window. Two chains converge: a per-frame
dispatch tick that reads the prim emitter's gate flag, and a one-shot
arm path that sets the gate plus a small param block.

### Per-frame dispatch (SCUS-resident)

Two SCUS-resident handlers from the 28-mode dispatch table at
`0x8007078C` ([game-mode state machine](../reference/functions.md#game-mode-state-machine))
reach the world-map render tick:

| Address | Mode-table role | Tick call |
|---|---|---|
| `FUN_80025EEC` | Default per-frame handler (used by 12 of the 14 per-frame modes - not world-map-specific; disc-confirmed by `legaia_asset::mode_table`). | `FUN_8001698C` → `FUN_80016444(1)` → `FUN_80016B6C`. |
| `FUN_80025F2C` | Mode 13 (MAPDSIP MODE) - field/world-map display per-frame handler. | `FUN_8001698C` → `func_0x801CE850` (overlay entry) → `FUN_80016444(0)`. |

The matching **Mode 12 (MAPDSIP INIT) handler `FUN_80025DA0`** is a transient
sub-overlay swap: it saves the field overlay 0897's slot-A head
(`*0x8001038C` = `0x801CE818`, `0x4000` bytes) into a scratch buffer, loads
PROT 981 over it (`FUN_8003EBE4(0x56)`), and calls the display module's init
`0x801CF4AC` (file `+0xC94`; base `0x801CE818` pinned by the call target). The
module seeds the scratchpad display-list base `0x1F800314` from world-state
globals (player pos `0x800840B8`, scroll vec `0x80092118`) and runs a 21-state
display SM over the still-resident 0897 body (it reads `0x801D5334`, beyond its
`0x4000` swap window); on mode exit `FUN_80025DA0` restores 0897's head and
re-enters it (`0x801CE8CC`). So MAPDSIP is the world-map *display* head, while
the controller proper (`FUN_801E76D4`) stays in the co-resident 0897 body. See
[`boot.md`](boot.md#per-frame-dispatch-scus-resident) mode-12 row.

The `a0` arg controls whether `FUN_80016444` skips its early
`FUN_8005FB84` block (Mode 13 skips it; the default handler runs
it). Both reach the world-map render branch deeper in the function,
so the horizon emitter can fire from any of the 14 modes that route
through `FUN_80016444` whenever the submode register holds `2`.

### `FUN_80016444` - SCUS world-map render tick (1352 bytes)

Entry: `(submode_flag)`. Iterates the world-map render passes for one
frame. The pipeline includes a gated direct call into the overlay-
resident POLY_FT4 emitter:

```mips
80016750  lui   v1, 0x8008
80016754  lw    v1, -0x43c4(v1)        ; v1 = _DAT_8007BC3C (submode register)
80016758  li    v0, 0x2
8001675c  bne   v1, v0, 0x8001676c     ; skip unless submode == 2
80016764  jal   0x801d7ea0             ; -> overlay-resident emitter
```

Same `0x8007BC3C` register has six SCUS writers (`FUN_80016230` -
the two-write set/clear path - plus `FUN_80025980`, `FUN_80025DA0`,
`FUN_8001D424`); the writer that stores `2` is the entry point for
the world-map render branch.

### `FUN_801D7EA0` - world-map POLY_FT4 batch emitter (832 bytes)

Entry: `()`. One-shot emitter gated by `_DAT_801F351C`:

```c
if (_DAT_801F351C != 0) {
    _DAT_801F351C = 0;                   // self-clear gate
    iVar11 = 4;
    local_30 = 0x2C808080;                // POLY_FT4 GP0 cmd + neutral grey
    uVar6 = _DAT_801F3518
          + DAT_1F800393 * _DAT_801F3524; // angle += per-frame-tick * step
    _DAT_801F3518 = uVar6;
    local_3c = _DAT_801F3520;
    local_34 = _DAT_801F3520 / 5;
    local_38 = _DAT_801F3520 - local_34;
    do {
        iVar10 = cos_table[(uVar6 & 0xFFF)];   // 0x8007B81C cos LUT
        // emit 2x POLY_FT4 (chain tag 0x9000000) + 1 small prim
        // (chain tag 0x3000000); vertex coords are cos-rotation-
        // projected with local_3c/local_38 as scale moduli.
        ...
        uVar6 += 0x10;
        iVar11++;
    } while (iVar11 < 0xE4);              // 224 iterations
}
```

`_DAT_8007B81C` is the cos lookup table (`docs/reference/memory-map.md`).
The function emits ~670 prims per call (2 POLY_FT4 + 1 small per iter
across 224 iters). Vertex coordinates project via the cos table, so
the rendered output rotates with the camera angle - consistent with
a horizon / sky / animated-background plane, not a fixed continent
mesh.

The case-5 path of the [per-actor render dispatcher `FUN_8001ADA4`](#per-actor-render-dispatcher---fun_8001ada4)
draws every **landmark** TMD (castle, towers, bridges, gates) - each
world-map actor's `actor[+0x44]` mesh chain points into Drake's
40-TMD landmark pack at PROT entry 0085 slot 1, which the dispatcher
walks once per frame through `FUN_8002735C` (the 60-GTE Legaia TMD
renderer). That accounts for the landmark prims in the GPU pool.

### Top-view bulk-terrain render path (overlay-replaced per-prim renderers)

This is the **top-view / overview** render path (game mode `0x0D`), distinct
from the walk-view continent (a heightfield, above). The top-view's bulk terrain
prims are not produced by a procedural emitter sibling of `FUN_801D7EA0`; they
come out of ordinary case-5 TMD rendering (of the overview-pool meshes placed
per cell) whose per-prim dispatch is **mode-switched** to overlay-resident
renderers when the top-view overlay is paged in. `FUN_80043390` (the SCUS-side per-prim TMD
renderer at the leaf of the actor-mesh-chain walk) selects one of two
function-pointer tables based on `_DAT_1F800394 & 1`:

| Flag | Table base | Rows | Where it lives |
|---|---|---|---|
| clear | `0x8007657C` | 4 (alpha 0/50/A0/F0) | SCUS_942.54 |
| set | `0x801F8968` | 1 (alpha 0 only) | world-map overlay |

The overlay path skips the alpha offset (`_DAT_1F800028` is not added
on the overlay branch), so only the first row of the overlay table is
meaningful. Slots 8..11 of row 0 share the same low-mode dispatchers
as SCUS (`0x8004409C, 0x8004423C, 0x80044434, 0x800445B0`); slots
12..19 carry the eight overlay-resident high-mode renderers:

| Slot | Address | Role (per case-5 TMD prim mode) |
|---|---|---|
| 12 | `0x801F7644` | high-mode prim renderer A |
| 13 | `0x801F7838` | high-mode prim renderer B |
| 14 | `0x801F7F78` | high-mode prim renderer C |
| 15 | `0x801F8198` | high-mode prim renderer D |
| 16 | `0x801F7AA4` | high-mode prim renderer E |
| 17 | `0x801F7CCC` | high-mode prim renderer F |
| 18 | `0x801F8454` | high-mode prim renderer G |
| 19 | `0x801F8690` | high-mode prim renderer H |

Each is a per-primitive emitter that loads vertex indices from the TMD
prim body, looks vertices up in the actor's vertex pool (passed as
`a2`), runs them through the GTE, and emits one GPU prim packet into
the chain at `_DAT_1F8003A0`. Static `addprim` hunters never surfaced
them because (a) the cmd byte is loaded from a per-mode descriptor table
rather than as a `lui`/`li` immediate, and (b) the captured overlay
window stopped at `0x801F0000` (which clipped every leaf address).

#### Per-slot delta vs SCUS sibling

Every overlay leaf is its SCUS sibling body plus a **distance-cue fog
post-process** inserted between the GTE projection and the OT packet write
(constant shape across all eight slots): per-vertex `Z_far = max(z1,z2,z3) >>
shift`, mix the far-plane reference into the prim cmd, `dpcs`/`dpct` against the
fog colour, then a per-Z RGB-LUT tint added into the OT packet. The two textured-
quad modes (slots 15, 19) use `dpct` (triple); the rest use `dpcs`. The fog
parameters sit at GP-relative offsets in the per-frame camera/render context:

| GP offset | Role |
|---|---|
| `-0x2e0` | Far-plane reference Z (mixed into prim cmd word). |
| `-0x2dc` | Fog color (loaded into GTE color register before `dpcs`). |
| `-0x2d1` | Fog-enable flags byte; bit `0x10` gates the whole fog path. |
| `-0x2bc` | Pointer to per-Z fog-tint LUT (2-byte entries, indexed by `Z >> 5`). |
| `+0x90`  | Z shift exponent (controls how aggressively far-plane Z compresses). |

The full per-slot disassembly and the fog-pass implementation are documented in
[`world-overview-viewer.md`](world-overview-viewer.md), which ports this pass to WebGL.

This is **why the top-view bulk-terrain prims don't show up under a single
emit function**: there isn't one. There are eight per-mode leaves
called once per TMD primitive across however many actor mesh chains are
active. The source mesh data is the same kingdom-bundle TMD pack
(slot 1, type `0x02`) the landmarks draw from - the world-map top-view
just routes its prim emit through the overlay-replaced per-mode
renderers, which apply whatever camera/fog/atmospheric transform the
top-view needs that the SCUS variants don't.

`mednafen-state prim-dispatch-table <save>` decodes both tables out of
a save state's main RAM and surfaces the eight overlay-resident
targets. Use `--overlay-targets-only` to pipe the eight addresses into
a Ghidra `dump_funcs.py` `TARGETS` list. See
[`legaia_mednafen::prim_dispatch`](../../crates/mednafen/src/prim_dispatch.rs).

Slot 4 of each kingdom bundle is **not** the bulk-terrain source. Its
records are something else (a runtime library of object-local 3D
meshes, see [`world-map-overlay`](../formats/world-map-overlay.md)) -
that hunt is independent of the continent terrain emit mechanism.

The horizon emitter is called by direct `jal` from SCUS - it does not
need function-pointer dispatch. Ghidra's reference manager misses the
cross-program call when sweeping the overlay alone; sweep SCUS to
surface the caller.

There is no single bulk-terrain emit function: the top-view prims come from many
small case-5 TMD renders going through `FUN_80043390`'s overlay-replaced per-mode
renderers, where the prim cmd byte is loaded from the descriptor table
`DAT_8007326C` rather than built with `lui`/`li` immediates. Static `addprim`
hunters therefore surface only the horizon emitter `FUN_801D7EA0` inside the
overlay (SCUS-side candidates are all HUD/menu sprite batchers). The dynamic
prim-pool-writers probe
([`scripts/pcsx-redux/autorun_prim_pool_writers.lua`](../../scripts/pcsx-redux/autorun_prim_pool_writers.lua))
confirms it: top-of-list PC hits land in the `0x801F7344..0x801F8DBC`
overlay-resident range - the eight high-mode renderers pinned by the dispatch
table above. The source mesh data is the kingdom slot-1 TMD pack the landmarks
come from, plus the runtime-positioned character/NPC mesh chains in
`_DAT_8007C354` and siblings.

### Per-frame render-pass iterator - `FUN_8002519c`

Five times per frame, `FUN_80016444` invokes the SCUS-resident actor-list
iterator `FUN_8002519c` (328 bytes) against five linked-list heads at
`_DAT_8007C34C..._DAT_8007C36C`. Each list is one render pass. The
iterator walks the chain and per node:

```c
for (node = *(list_head); node != NULL; node = node->next) {
    if (node->flags & 0x8) {
        if (node->flags & 0x200) {
            // already-emitted path: skip the heavy work
        } else if (node->fn == &FUN_80021df4) {
            // standard per-frame actor tick
            ...
        }
    } else {
        ((void (*)(void *))node->fn)(node);   // jalr node->fn
    }
    // mark `flags |= 0x200` to dedupe in case the list is walked again
}
```

Per-actor record layout consumed by the iterator:

| Offset | Type | Role |
|---|---|---|
| `+0x00` | `actor *` | Next pointer (singly linked list, `NULL` terminates). |
| `+0x0C` | `void (*)(actor *)` | Tick function (the entry point `jalr` calls). |
| `+0x10` | `u32` | Flags; bit `0x8` selects the early-return path, bit `0x200` is the "already-emitted this frame" guard. |
| `+0x14` | `u32` | Saved next-pc copy used by the early-return path. |
| `+0x18` | `u16` | Halfword count exposed at `+0x20` for the early-return path. |
| `+0x44` | `chain *` | Optional prim-chain head; freed via `FUN_80017b94` when bit `0x800` is set. |
| `+0x48` | `u8 *` | Move-VM bytecode base (for actors whose tick is `FUN_80021df4`). |
| `+0x70` | `u16` | Move-VM PC in halfword units; the actual byte offset is `2 * actor[+0x70]`. |

Standard tick functions observed in the world-map render passes:

| Tick function | Where | Role |
|---|---|---|
| `FUN_80021DF4` (SCUS) | per-frame actor tick | Steps the move VM via `FUN_80023070(actor)`. The eight actors in list `_DAT_8007C350` use this tick. |
| `FUN_8003BC08` (SCUS) | per-actor tick | Calls the motion VM (`FUN_8003774C`), move-buffer setup (`FUN_800204F8`), and overlay helper `FUN_801D79E8`. The fourteen actors in list `_DAT_8007C354` use this tick. |
| `FUN_801E76D4` (world_map overlay) | world-map controller | Top-view debug toggle + camera scroll/azimuth/zoom + dev-menu render. |
| `FUN_801DA51C` (world_map overlay) | per-entity tick | 5-state SM on `entity[+0x8A]` (see [actor-vm](actor-vm.md)). |
| `FUN_801D1344` (world_map overlay) | horizon gate-arm wrapper | See the gate-arm chain below. |

### Per-actor render dispatcher - `FUN_8001ADA4`

In addition to the five TICK calls into `FUN_8002519c`, the same frame
issues six RENDER calls into the stack-swap wrapper `FUN_8001D140`,
which forwards into the per-actor render dispatcher `FUN_8001ADA4`
(2456 bytes). The render dispatcher walks the same actor lists but
runs a different switch - on `actor[+0x56]` (render mode `1..0xB`):

- **case 4** (multi-target). Dispatches on `actor[+0x9e]` flags:
  - bit `0x4000` → `FUN_8002A5A4` (SCUS).
  - bit `0x2000` → `FUN_801CFA48` (overlay-resident).
  - else → `FUN_80028158` (SCUS, distinct from the 6692-byte motion
    bytecode VM `FUN_80038158`).
- **case 5** (full TMD). Iterates the mesh chain at `actor[+0x44]`
  (`puVar5[0]` = count, `puVar5[1..n]` = mesh pointers) and per
  entry calls:
  - `FUN_80043390(mesh, color, tpage)` - textured TMD (default).
  - `FUN_80029888(...)` - environment-mapped TMD when
    `actor[+0x7a] != 0`.
  - `FUN_8002735C(...)` - 60-GTE Legaia TMD renderer (the
    **landmark emit leaf** - each landmark TMD in Drake's 40-mesh
    kingdom pack passes through here; the bulk continent ground
    terrain is *not* drawn from here).
- **cases 1, 2, 3, 6, 7, 8, B** - distance-LOD / particle / sprite-billboard
  branches calling per-effect helpers (`FUN_8001B73C`, `FUN_8001B964`,
  `FUN_800480D8`, `FUN_8002B944/94C/954`, `FUN_8001C204`).

Static `addprim` hunters do not surface `FUN_8002735C` as a POLY_FT4
emitter because the cmd byte is read from the per-mode descriptor
table at `DAT_8007326C`, not built with `lui/li` immediates. That is
why the landmark TMD emitter eluded static analysis: the addprim scan
flags every direct emitter (the horizon, the HUD sprite batch
`FUN_8002C69C`, the screen-tint, etc.) but skips the TMD renderer
where the landmark prims actually originate. The bulk continent
ground terrain follows the same dispatch-table pattern - via
`FUN_80043390`'s overlay-mode jump table at `0x801F8968` and its eight
overlay-resident high-mode renderers - documented earlier in this
section.

### Gate-arm chain - `FUN_801D1344` -> `FUN_801D8258`

The one-shot gate `_DAT_801F351C` is armed by a 40-byte trigger
function called from a 1332-byte parameter-prep wrapper:

| Address | Role |
|---|---|
| `FUN_801D1344` | World-map gate-arm wrapper. 1332 bytes; function-pointer-only entry (Ghidra `incoming=0`). Reads three globals at `_DAT_8007BCD0/_D4/_D8` and forwards them to `FUN_801D8258` as the scale / step / OT-layer params at PC `0x801D1470: jal 0x801D8258`. **Same RAM address holds a different function when the dialog overlay is paged in** - that variant is the actor frame handler (see [`reference/functions.md`](../reference/functions.md#dialog-overlay-actor-frame-helpers)). |
| `FUN_801D8258` | 40-byte gate setter. Writes `_DAT_801F351C = 1`, then `_DAT_801F3520 = param_2`, `_DAT_801F3524 = param_3`, `_DAT_801F3528 = param_4` - the inputs the emitter consumes on its next run. |
| `FUN_801C2B2C` | Code-identical relocation copy of `FUN_801D1344` in the 0897 field overlay. Same body, different load address; calls `jal 0x801D8258` at PC `0x801C2C58`. Active during field-mode entry transitions. |

The gate flag `_DAT_801F351C` is in the persistent `0x801F0000+` region,
so it survives overlay swaps. The flag is shared - both the world-map
overlay's `FUN_801D7EA0` and the 0897 field overlay's
`FUN_801C9688` read + clear it.

## Globals used

| Address | Role |
|---|---|
| `DAT_801F2B94` | View-mode flag: `0` = walk, `1` = top-view debug. Outside 192 KB extraction window. |
| `DAT_801F2B95` | Top-view animation bitfield (`& 1` = anim-A enable, `& 2` = anim-B). |
| `_DAT_80089120` | Top-view camera X scroll (adjusted ±8 per D-pad frame). Stored as the **negated map-origin X** (`_DAT_80089120 = -(int)*(short *)(actor + 0x18)` in `overlay_0978_801c5c58.txt:465`; identical in `overlay_slot_machine_801db8ec.txt:115`). The world camera target is `-_DAT_80089120`. |
| `_DAT_80089118` | Top-view camera Z scroll (adjusted ±8 per D-pad frame). Stored as the **negated map-origin Z** (`_DAT_80089118 = -(int)*(short *)(actor + 0x14)` in `overlay_0978_801c5c58.txt:464` / `overlay_slot_machine_801db8ec.txt:114,118`). The world camera target is `-_DAT_80089118`. |
| `_DAT_8007B794` | Top-view azimuth (adjusted ±0x14 per frame). |
| `_DAT_8007B6F4` | Shared word: in world-map mode this is the top-view zoom/height (adjusted ±4 per D-pad frame, retail walk-view loads `0x0170`); outside world-map mode it doubles as a camera-mode flag (the "Small Maps" debug toggle in `docs/reference/builds.md` / `docs/reference/cheats.md`; retail walk-in-field saves load `0x0002`). |
| `_DAT_8007B868` | Door/portal open flag: `0` = open (MAP_CHANGE/CARD_OPTION visible), `1` = CLOSED. |
| `_DAT_8007B6B8` | Game-mode discriminator (value `0x20` = alternate sprite path). |
| `_DAT_80083808` | World-map entity activation gate. |
| `_DAT_8007BC3C` | World-map submode register. `FUN_80016444` gates its `jal 0x801D7EA0` on this being `2`. Six SCUS writers (`FUN_80016230` / `FUN_80025980` / `FUN_80025DA0` / `FUN_8001D424`). |
| `_DAT_801F351C` | One-shot gate flag for the POLY_FT4 batch emitter. `FUN_801D8258` sets it to `1`; `FUN_801D7EA0` (and the 0897 sibling `FUN_801C9688`) clear it after one emission. Lives in the persistent `0x801F0000+` region and survives overlay swaps. |
| `_DAT_801F3518` | Running camera angle. Advanced by `DAT_1F800393 * _DAT_801F3524` per `FUN_801D7EA0` call; masked to 4096 entries when indexing the cos LUT at `0x8007B81C`. |
| `_DAT_801F3520` | Render scale / range. Sourced from `_DAT_8007BCD4` via `FUN_801D8258`'s `param_2`. The emitter uses it both as `local_3c` and `local_3c / 5`. |
| `_DAT_801F3524` | Angle step per frame tick. Sourced from `_DAT_8007BCD8` via `FUN_801D8258`'s `param_3`. |
| `_DAT_801F3528` | OT layer / draw priority. Sourced from `_DAT_8007BCDC` via `FUN_801D8258`'s `param_4`. |
| `_DAT_8007BCD0..D8` | Three contiguous u32 globals that `FUN_801D1344` reads and forwards as the gate-setter's scale / step / OT-layer params. |
| `_DAT_8007C34C..0x36C` | Seven actor-list heads consumed by `FUN_8002519c`. `_DAT_8007C34C` / `_DAT_8007C350` / `_DAT_8007C354` / `_DAT_8007C358` / `_DAT_8007C35C` / `_DAT_8007C360` / `_DAT_8007C36C` correspond to the five world-map render passes that `FUN_80016444` issues per frame plus two scratch heads. |

## World-overview viewer

The `/world-overview/` page in the static site renders each kingdom's
landmark layer in real-time WebGL 3D from a disc image. Its docs live
in a sibling file because the viewer is a clean-room deliverable
distinct from the retail subsystem analysis above:

- [`world-overview-viewer.md`](world-overview-viewer.md) - layout
  engine for unplaced slot-1 TMDs, distance-cue fog pass, bulk-terrain
  placement resolver, per-kingdom fog colour, ocean tile + 13-frame
  CLUT animation, camera anchors.

## See also

**Reference** -
[World-overview viewer](world-overview-viewer.md) ·
[Motion VM](motion-vm.md) ·
[Encounter record](../formats/encounter.md) ·
[World-map overlay](../formats/world-map-overlay.md)
