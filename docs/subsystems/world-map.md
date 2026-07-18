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
- [Encounter-record installation](#encounter-record-installation) · [clean-room port](#clean-room-port---both-overworld-and-field) · [NPC dialogue text source](#npc-dialogue-text-source)

**Overworld player + scenes**
- [Player movement + region-keyed encounters](#overworld-player-movement--region-keyed-encounters) · [collision / walkability](#overworld-collision--walkability) · [camera-relative movement remap](#camera-relative-movement-remap) · [boot-path seeding](#boot-path-seeding)
- [Entity / actor placement table](#entity--actor-placement-table) · [classifying the entity kind](#classifying-the-entity-kind-from-its-script) · [scene destinations](#scene-destinations) · [chapter-1 Drake hub sweep](#chapter-1-drake-hub-sweep)

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

**Story-conditional destination (in-record `0x70` branch).** A second, finer
mechanism lives *inside* an entrance record: the destination scene can change
after a story beat while the trigger tile stays the same. The Drake dungeon
entrance (`map01` records `P2[1]`/`P2[2]`) selects its `0x3F` target by an
op-`0x70` `SysFlag.Test` on system flag `0x142` - clear falls through to
`3F -> dolk` (pre-boss), set jumps to a second `3F -> dolk2` (post-boss); both
arms share the trigger tile and arrival tile `(49,45)`. This is **not** a
record-level C1/C2 gate (those are empty here) - it is a bytecode branch, so
`man_field_scripts::overworld_portal_sites` decodes the conditional `0x3F` pair
(`OverworldPortalSite::conditional` / `ConditionalDest`) and the seeder resolves
primary vs alternative through `World::system_flag_test`, mirroring the field
VM's op-`0x70` semantics. (This falsifies the earlier "dolk2 is reached from a
dungeon interior" reading - no interior scene lists `dolk2`; it is the same hub
entrance as `dolk`, chosen by flag `0x142`. Where retail *sets* `0x142` - the
dolk-dungeon-clear writer - is unrecovered, like the Zeto battle-id writer.)

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

### The Drake round trip (Rim Elm <-> map01 <-> cave01)

The two directions of a hop are **different mechanisms**, so a working exit does
not imply a working return. Both legs are disc-sourced; the table below is the
decoded bridge (`man_field_scripts::overworld_portal_sites` joined to each
scene's `.MAP` gate-1 triggers), and every row is asserted end-to-end by
`engine-core/tests/scene_round_trip_disc.rs`.

| From | Trigger tile(s) | P2 record | To | Arrival tile | `dir` |
|---|---|---|---|---|---|
| `town01` / `town0b` / `town0c` | (24..26, 46) | `P2[0]` | `map01` | (96, 25) | 4 |
| `map01` | (96, 24) | `P2[0]` | `town0c` | (25, 45) | 0 |
| `map01` | (37, 110) | `P2[5]` | `cave01` | (93, 97) | 4 |
| `cave01` | (93..94, 96) | `P2[1]` | `map01` | (37, 109) | 0 |

Field -> overworld is the walk-on tile trigger -> partition-2 record -> `0x3F`
path (`SceneHost::dispatch_walk_on_trigger`); overworld -> field is the entity
SM's `OverworldPortal` walk-onto path described above. Each arrival seat is one
tile clear of the reciprocal trigger, so a return never immediately re-fires the
entrance - that spacing is authored, not engine policy.

The arrival seat mapping is live-pinned: `door_warp_town01_to_map01` parks the
retail player at world `(3264, 5824)` in `town0c`, which is exactly
`seat_player_at_tile(25, 45)` - `map01`'s town-entrance arrival tile. So
`world = tile * 128 + 0x40` is byte-exact for `0x3F` entry bytes.

Note the two pads are **not** the same space: the field walk uses world axes
(`decode_field_direction`), while the overworld walk is **camera-relative**
(`world_map_camera_relative_bits`; at azimuth 0, screen-Right = world `Z-`). The
overworld entrance at (96, 24) is therefore reached by holding Right from the
arrival seat, not Down.

**Rim Elm's south gate is a story gate, enforced in the collision grid.** The
exit trigger band is walled off on a fresh New Game - you cannot leave Rim Elm,
exactly as retail plays. The seal is not a script gate on the `0x3F` but a
**collision delta**: `town0c` `P1[0]` first clears the approach band
(three `0x4C` nibble-7 sub-0 paints), then branches on system flags `327` and
`321`. With `327` clear the script skips both arms and the base map's wall
stands; with `327` set and `321` clear it re-blocks the band (`sub-1`
`x=23..29 z=44..45` and friends); with **both set** it takes the open-gate arm -
`sub-1 x=26..27 z=45..46`, **`sub-0 x=24..25 z=45..46` (the gate opening)**, and
`sub-1 x=21..22 z=45..45` (the side wall). Only that last arm clears grid row 47
cols 24-25, the cells that actually block the walk, and the resulting grid is
**byte-identical to the retail live grid** lifted from the
`door_warp_town01_to_map01` capture's `*(_DAT_1f8003ec) + 0x4000` region. So the
exit becomes walkable precisely when the story flags latch. (These paints were
invisible to the disassembler until the nibble-7 per-sub width fix - see
[`script-vm.md`](script-vm.md), width blindness's fourth face.)

**Gate-flag setters beyond a scene's bundle MAN.** `man-scripts
--system-flag-census` walks the MAN field-VM ops `0x50/0x60/0x70` across
EVERY carrier per scene (bundle + the streaming variant MANs); the other
disc-resident bytecode writing the same bank is the second motion VM
`FUN_80038158` (op-`7` sets, op-`8` clears, flag = `operand[1] |
operand[2] << 8`; carrier = MAN tail-section 1, see
[`motion-vm.md`](motion-vm.md)), swept by `--motion-flag-census`. A flag
absent from BOTH censuses is set by a direct code path
(`FUN_8003CE08`-class call in an overlay). Where the chapter-1 spine flags
landed:

- **`549` (`0x225`)**, the `town01` opening one-shot: writer still
  unrecovered. The bank RAM-diff pin stands (byte `+0x44` bit `0x04` flips
  pre- vs post-opening), but the disc-wide motion-flag census finds **no**
  op-7 site carrying `549` - the earlier "`FUN_80038158` op-`7` from its own
  script bytecode" carrier reading is falsified (the op-7 *mechanism* exists;
  no disc stream uses it for `549`). The setter is a direct code path; the
  spine flag-writer capture harness is the closer. See
  [`functions.md`](../reference/functions.md).
- **`0x142`** (the Caruban beat / dolk-dolk2 switch): writer **pinned**.
  The SETs are plain field-VM `51 42` script bytes in the rikuroa
  streaming-carrier MAN (extraction 157) - `P1[10..12]` plus the
  post-victory record `P2[50]` (C1 = `0x142` itself, the self-latching
  one-shot) - re-asserted by dolk2's carrier `P1[0..1]` and cleared by
  dolk's bundle `P1[26]`. The firehose capture caught the write live
  (`ra 0x801E3598`, the dispatcher's own `0x5x` SET arm) and the resident
  script heap byte-matches the carrier. The old corpus-negative stood
  because no census walked the streaming variant MANs (and the earlier
  raw-byte sweep looked for setter *code*, not script operand bytes at
  op boundaries). Engine: the whole chain is organic record execution -
  approaching the rikuroa boss-stager placement `P1[3]` runs the record
  through the field VM (`World::install_boss_stagers_from_man` /
  `run_boss_stager_record`; park gate = `0x142`, read from the record's
  own head test), whose `52 89` SETs the transient staged marker `0x289`
  and whose `3E FF 11` enters the fight; the post-battle field return
  re-runs the scene-entry script `P1[0]`, whose `72 89` test arm spawns
  `P2[50]` through the C1-gated record dispatch - the record's own
  `51 42` script bytes SET `0x142` (and `62 89` clears the marker),
  flipping the dolk-dolk2 entrance organically. Disc-gated oracle:
  `engine-core/tests/organic_beat_records_disc.rs`.
- **`0x482`** (Drake mist walls): no script writer exists - the earlier
  "SETs in the `other7` block, clears in the `edbalden`/`eddoman`
  epilogue carriers" reading was desynced-walker text noise (full-width
  SJIS digits / an `ＥＸＩＴ` label table aliasing the `54 82`/`64 82`
  op bytes; falsified per-site by hand disasm, pinned by the census
  decode-coherence flag in `man_variant_carrier_census_disc.rs`). The
  writer is a direct code path - a capture target (write-watch across
  the post-Zeto Drake-revival beat), like flag 549. The reader stays
  solid: the `map01` `P2[34..36]` C1 force-walk bands. The engine leaves
  the gate as-is meanwhile.

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

While a **cutscene timeline that staged op-`0x45` camera params** owns the
overworld - the New-Game opening's `map01` leg - the walk/top-view cameras
stand down and the shell renders the same cutscene GTE camera the field
prologue scenes use (`compute_scene_camera`'s cutscene branch; see
[`cutscene.md`](cutscene.md#timeline-execution-engine-port)). Retail's Rim Elm aerial fly-in is
three camera beats in `map01`'s opening record (`P2[38]`): a snap to the high
aerial shot (pitch `735`, H `368`, eye trio `(-1268, -3756, 18784)`, focus
`(12162, ?, 3510)`), then a `45 0B .. apply 900` beat - mode 2 = quadratic
ease-out on **every** component - descending to pitch `355` / eye trio
`(412, -2336, 12384)`, all confirmed against a per-frame RAM capture of the
live camera globals. The overworld player / entity marker overlay is hidden
while the timeline runs (retail's descent shows the bare continent); a
world-map beat record **without** camera beats (the Drake mist-wall
force-walk bands) keeps the ordinary walk camera. Disc-gated pin:
`engine-core/tests/map01_flyin_camera.rs`.

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

The retail pitch/TR path *between* the two anchors is not yet pinned. The
community poll probe streams the full tuple on the overworld (`wmcam` rows:
rotation trio + H + the TR low halves + view mode, on change;
[pcsx-redux-automation.md](../tooling/pcsx-redux-automation.md) fast
whole-playthrough capture), so any captured zoom/rotate session yields the
trajectory.

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
  [`asset-loader.md` → WARP opcode flow](asset-loader.md#warp-opcode--minigame-door-warp-flow-sub_id));
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

#### Chapter-1 Drake hub sweep

The Drake overworld (`map01`) is a hub: its controller's `0x3F` table lists a
dozen destinations and its `.MAP` walk-on triggers install one `OverworldPortal`
per town/dungeon entrance. Past the covered spine (`town01 -> map01 -> keikoku`
and the boss leg `map01 -> rikuroa / dolk -> dolk2`) the remaining interior legs
are `cave01`, `vell`, `vozz`, `suimon`, `jou`. Each is decoded + driven straight
from disc bytes (no capture) by the disc-gated
[`chapter1_hub_sweep_oracle.rs`](../../crates/engine-shell/tests/chapter1_hub_sweep_oracle.rs):
driving `town01 -> map01` and stepping onto the leg's portal tile loads the scene
in `SceneMode::Field` with its MAN present.

| leg | map01 portal tile | entrance record | bundle | MAN partitions | onward `0x3F` |
|---|---|---|---|---|---|
| `cave01` | `(37,110)` | `P2[5]` | Scripted (PROT 38) | `[1,13,18]` | `map01` |
| `vell` | `(77,97)` | `P2[6]` | Scripted (PROT 45) | `[8,17,13]` | `map01` |
| `vozz` | `(107,91)` | `P2[8]` | Scripted (PROT 103) | `[4,24,20]` | `map01` |
| `suimon` | `(57,61)` | `P2[19]` | Scripted (PROT 77) | `[10,7,3]` | `map01` |
| `jou` | `(95,23)` | `P2[37]` | Scripted (PROT 630) | `[15,8,7]` | `map01`, `jouina` |

**`dolk2` onward destination.** `dolk2` (post-boss `dolk` variant; its real
MAN is the streaming carrier extraction 70, partitions `[29,73,17]`) lists a
single named `0x3F`: back to `map01`. It is a terminal interior - the boss
chain returns the player to the overworld, no deeper leg. (`jou` is the one
swept leg with an interior onward warp of its own, to its interior variant
`jouina`, alongside the `map01` exit.)

**Gate census.** Every one of the five swept entrance records installs
**unconditionally** - empty `C1`/`C2`. On `map01` the only C1-gated overworld
entrance is the Ravine (`keikoku`, `C1=[0x193]`); the only op-`0x70` branch among
the entrances is the `dolk`/`dolk2` destination switch on flag `0x142` (a
destination selector inside the ungated `dolk` record, not a spawn gate); and the
`0x482` mist-wall pattern is a separate no-`0x3F` band record. So **no C1/C2
progression gate stands between a fresh Drake arrival and any of the five legs** -
the chapter-1 story order on this hub is carried only by the `keikoku` `0x193`
gate and the `dolk`/`dolk2` `0x142` switch.

**The "`suimon` and `dolk2` share a MAN" identity - dissolved.** Both halves
were the CDNAME-shifted scene window: `dolk2`'s unshifted window bled two
entries into `suimon`'s block and picked up suimon's sidecar copy of its own
2345-byte `[10,7,3]` MAN (`rikuroa`/`geremi` aliased the same way; byte
comparison + position law in
[scene-v12-table.md](../formats/scene-v12-table.md#the-embedded-man-at-0x1000-is-an-extended-footprint-over-read)).
In the retail frame `suimon` keeps that MAN (its scripted bundle) and
`dolk2` resolves its own streaming carrier (extraction 70, `[29,73,17]`).
`Scene::load` now converts CDNAME raw-TOC block ranges to the extraction
frame (`raw - 2`), which also dissolves the historical "a scene's `.MAP` is
two entries below its block" rule - the `.MAP` is simply the retail block's
FIRST entry.

#### Chapter-1 hub depth: vozz + jou -> jouina

One level deeper on the two story-load-bearing legs, decoded + driven by the
disc-gated
[`chapter1_hub_depth_oracle.rs`](../../crates/engine-shell/tests/chapter1_hub_depth_oracle.rs):

- **`vozz` is the Ravine unlock.** Its `0x3F` set is `{map01}` only (exit
  `P2[10]`, gate-1 tiles `(60..62, 2)`), but its P1 scripts own flag `0x193`
  outright: the ONLY `0x193` SET on the disc is `vozz` `P1[7]` (`51 93` at MAN
  offset `0xDA6`, one-shot-guarded by an op-`0x72` test on `0x2AC` with a
  companion `52 AC` SET), alongside three tests and one `P1[12]` clear. `P2`
  gate census: seven records share `C1=[0x7]`; `P2[11..=13]` are
  **self-latching one-shots** (each record's C1 is the very flag its own
  script SETs: `0x2B2`/`0x2B3`/`0x2B4` - the canonical one-shot beat idiom);
  `P2[18]` `C1=[0x2AC]` `C2=[0x2B3]`. The `town01 -> map01 -> vozz -> map01`
  round trip drives clean in-engine.
- **`jou`'s castle door is chapter-gated.** The `jouina` warp is `P2[5]`
  (`0x3F` index 655, gate-1 tiles `(93..95, 97)`) behind `C2=[0x44D]`; the
  opener chain is `P2[2]` (C1=[0x3E7], sets `0x3E7`) -> `P2[3]`
  (C1=[0x44C], C2=[0x44B], sets `0x44C`) -> `P2[4]` (C1=[0x44D],
  C2=[0x44B], sets `0x44D` - the only `0x44D` setter on the disc). `0x44B`'s
  setters are `izumi` P1[15] / `noaru` P2[31] (the Noa beat), so the door
  opens only after the Noa chain. In-engine the gate behaves: a fresh walk-on
  installs nothing; with `0x44D` set the same walk-on spawns `P2[5]`.
- **`jouina` is not terminal**: destinations `{jou, jouinb}` (both ungated;
  `P2[0..=19]` all carry the `C1=[0xF]` busy-latch pattern), so the castle
  chains one more interior deep.
- **Player-channel handshake (resolved):** `jou` `P2[5]`'s door cutscene
  drives the PLAYER channel (`A2 F8 06` ExecMove + `C3 F8 ...` HaltAcquire).
  The timeline stepper models the handshake (ExecMove arms an in-flight
  countdown, HaltAcquire parks then steps past by encoded width; see
  [cutscene.md](cutscene.md)), so the record reaches its trailing `0x3F`
  and the oracle drives the `jou -> jouina` hop to `SceneEntered`.

#### Chapter-1 hub breadth: cave01 / vell / suimon + the Drake Castle chain

The remaining hub legs, one level deep, decoded + driven by the disc-gated
[`chapter1_hub_breadth_oracle.rs`](../../crates/engine-shell/tests/chapter1_hub_breadth_oracle.rs):

- **`cave01` is a two-mouth pass-through** - one named `0x3F` destination
  (`map01`) carried by TWO exit records (`P2[0]` gate-0 trigger `(8,89)`,
  `P2[1]` gate-1 band `(93..94,96)`). Nine of 18 `P2` records are gated: six
  self-latch one-shots plus the longest ordered beat chain decoded on a hub
  leg - `P2[13]` (C1 `0x15E` / C2 `0x15D`) -> `P2[14]` (C1 `0x169` / C2
  `0x15E`) -> `P2[15]` (C1 `[0x13, 0x142]` / C2 `0x169`; the final beat stops
  replaying once the Zeto flag sets). Ungated `P2[16]` SETs the `0x15D` entry
  key. The `0x15E` beat is read cross-scene by `urudre1` `P2[0]` (the Uru
  Mais dream tests the cave beat).
- **`vell`** - single exit `P2[10]` (band `(88..92,7)`). `P2[11]` self-latch
  C1=[`0x2AF`] (strictly vell-local Set/Test pair); `P2[7]` carries
  `C1=[0x63A, 0x7]` **byte-identical to vozz `P2[7]`'s gate**, and `0x63A`
  has ZERO script sites disc-wide (no writer anywhere in the MAN corpus -
  open thread). Also carries a gate-4 trigger family (record 53, five
  scattered tiles) not seen on the other legs.
- **`suimon` is a pure corridor** - all three `P2` records are ungated `0x3F`
  exits to `map01`; story variation only via the shared controller `P1[0]`
  testing `0x142`.
- **The Drake Castle interior is FOUR scenes deep**: `jou -> jouina ->
  jouinb -> jouinc -> jouind`. `jouinb` (`[19,7,13]`) is fully ungated (no
  jouina-style `C1=[0xF]` busy-latch): `P2[9]` back to `jouina`, `P2[10]`
  deeper to `jouinc` (`[43,18,60]`, lists `{jouinb, jouind}`). Once `jou`'s
  `0x44D` door is passed the deep castle is open. The oracle's part F drives
  `jou -> jouina -> jouinb -> jouinc` end-to-end in one session (the door
  cutscene completes through the player-channel model) - the deepest driven
  interior chain in the engine. `jouinc`/`jouind` decode is an open thread.
- **Decoder asymmetry (pinned):** the partition-1 destination-table scan
  under-reports doors carried only by `P2` records (`jouinb`'s `jouina`
  return door) - the reverse of the `jou` `P2[5]` blind spot; the `P2`
  walker and the strict portal-site join both see them.

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
  `.MAP` records' `+0x10` field is used by two sparse mesh layers on top of
  that ground: the placed-landmark layer (`FUN_8003A55C`, `flags & 0x4`) and
  the **decoration layer** - walk-visible cells whose record carries a nonzero
  `+0x10` plus the mesh-drawn flag bit `0x2` *without* the placed flag (flag
  families `0x0013`/`0x0813`; Drake ~295 cells over 31 records, Sebacus ~240,
  Karisto ~210). The decorations are the crossed-quad billboard trees (one
  tree mesh is stamped from dozens of cells per forest cluster), the mountain
  groups, and small props
  ([`legaia_asset::field_objects::parse_walk_decorations`]). The `0x0011`
  family (nonzero `+0x10`, no `0x2` bit) is **not** drawn: those are the
  riverbank/system cells - record 408 in every kingdom walk `.MAP` (same
  index, same `+0x10 = 4`, same flags across all three kingdoms), and
  stamping them tiles a wall/gate mesh down every river (visually falsified
  against retail). For the bulk ground cells (97% of the walk grid) `+0x10`
  is 0.

The walk-placer `FUN_8003A55C` (placed flag `0x4`) spawns only the ~51
interactive objects (distance-culled to ~14 live actors in the capture; most
live actors are script-spawned, not from the placed-flag set). It allocates via
`FUN_80024c88` → `FUN_80020de0` (free-list `FUN_80020454`, pool `_DAT_8007c354`),
stores the record index at actor `+0x60`, and leaves the mesh chain `+0x44` at
0; the mesh is resolved from the record index by the scene draw loop (resolver
not yet pinned). These are props/entities, not the bulk continent.

Each placed spawn is **gated on a MAN interaction record** for the cell: the
placer calls the overlay lookup `FUN_801d5630(1, col + rec[+6], row + rec[+7])`
and skips the spawn when it returns null. That lookup searches a partitioned
cell-keyed table at **walk-`.MAP` + `0x10000`** (header = per-partition
`(u16 rec_off, u16 count)` pairs at `+4p+2`/`+4p+4`; records are
`[u8 col][u8 row][u8 script_id][u8 aux]`, stride from the per-partition byte
table at `0x8007B318`; searcher `FUN_801d5ae0`), falling back to the same
header shape at `+0x12000` — the next file loaded behind the walk map in the
`_DAT_1f8003ec` buffer (Drake: PROT 0084). The matched record's `script_id`
indexes the live MAN's global record-offset table (`_DAT_8007b898 + 0x2b`,
3-byte entries, count = the `+0x22/+0x24/+0x26` partition totals); the actor
stores the resolved script at `+0x90` and the placer **immediately steps its
leading ops** (first opcode `0x24`/`0x25` enters the field VM until a yield),
so a placed object's resting position/visibility is script-managed - the raw
grid cell is only the spawn point. Record flags also seed actor status bits:
flag `0x800` → actor `+0x74 |= 0x10000000`, nonzero `rec[+0x1E]` →
`|= 0x40000000`, and mesh-drawn flag `0x2` selects actor render type 5 (the
mesh-chain draw) vs 0. Concrete case: Drake's two golden-bridge (mesh 6)
stamps - record 441 is a plain decoration at the road crossing
`(12224, 6336)` and draws there, while record 349 (placed, flags `0x0017`,
grid cell over the river at `(10688, 5312)`) is spawn-scripted and does not
rest at its grid cell; retail shows a single bridge, at the record-441 site.

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
- **Walk decorations** ([`Scene::walk_decoration_placements`], appended to the
  walk render in `resolve_world_map_terrain_draws`): the nonzero-`+0x10`
  unplaced walk cells - trees, mountain groups, props (see the walk layer list
  above).

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

**The gate is the object grid, in towns too.** Fitting the camera from a Rim Elm
field capture's own ground quads (the quads share corner vertices, so the lattice
rebuilds camera-independently; the fit then re-projects every cell to sub-pixel
residual) and asking, per cell, whether retail emitted a quad gives a clean split:
**every** on-screen `0x1000` cell has one, **no** on-screen `objcell == 0` cell
has one, and all recovered quads carry their record's `+0x14`/`+0x15`/`+0x16`.

So a floor cell with no object record has **no ground quad in retail either** -
its surface is an **env mesh** (the pack meshes the `+0x10` records place over it).
Widening the ground gate to the collision grid is therefore wrong twice over: it
emits quads retail never draws, and - having no record - they sample empty atlas
space, decode to `0x0000`, and are discarded. The visible symptom of a *missing*
mesh over such cells is the render-pass clear colour, not a texturing bug; see the
mesh-id rule in [`field-locomotion.md`](field-locomotion.md#environment-geometry).

**Engine.** [`build_walk_heightfield`] reads each visible cell's record and bakes
the per-cell tile UV (`+0x14` → 8×8 atlas) into `WalkHeightfield::uvs` and the
per-cell `[clut, tpage]` (`+0x15`/`+0x16`) into `WalkHeightfield::cba_tsb`, so a
single ground mesh samples grass / mountain / water / forest pages per cell;
`play-window` draws it. `GROUND_ATLAS_TPAGE` / `_CLUT` remain only as the grass
fallback for cells whose record carries no terrain run. (Distinct from the
**top-view** bulk continent, which is per-cell *meshes* via `FUN_80043390` / the
MAN `0x7F`-sentinel resolver - see
[`world-overview-viewer.md`](world-overview-viewer.md).) Disc-gated coverage:
`crates/engine-core/tests/field_ground_surface_disc.rs` (the ground layer is
exactly the `0x1000` cells and never samples empty VRAM; every open floor cell is
surfaced by a ground quad or a mesh).

**The atlas page is contested VRAM.** `0x0C` = fb `(768, 0)`, and that is also
where a scene block's **pochi-filler** slots keep their stale `256 x 256`
character page (the fill's scratch tail parses as a real TIM - see
[`pochi.md`](../formats/pochi.md)). Any VRAM pre-pass that sweeps a scene's whole
CDNAME block for TIMs uploads that leftover *after* the scene's own atlas and
erases it, and the ground quads then sample character texels - Jeremi renders a
grid of grey "tombstone" tiles, Mt. Dhini a repeating vine/crack pattern, while
Rim Elm (whose siblings are all `scene_tmd_stream` entries, already excluded)
looks fine. The field build therefore skips pochi slots outright; regression:
`crates/engine-core/tests/field_ground_texture_pages_disc.rs`.

**Ocean / water animation.** The water tile is a 4bpp texture at fb
`(768, 256)` whose CLUT row at fb `(0, 506)` (CBA `0x7E80`) the retail engine
rewrites every few game ticks - the rolling-wave shimmer - along with seven
more shoreline/terrain shimmer cells. **The operand source is the kingdom
bundle's slot 5** (the type-byte `0x06` slot of PROT 0085 / 0244 / 0391): an
LZS-compressed 516-byte **CLUT-walk animation table**, byte-identical across
the three kingdoms (the per-kingdom colours come from the parked source
strips, not the table). Format (parser [`legaia_asset::clut_walk`]):
`[u32 count = 8][u32 entry_offsets[8]]`, then per entry
`[u8 kind = 1][u8 nframes][u16 cumulative_size][u16 dest_x][u16 dest_y]`
followed by `nframes` 8-byte frames
`[u8 0][u8 hold_vsyncs][u16 0][u16 src_x][u16 src_y]`.

At scene load the asset-type dispatcher `FUN_8001f05c` case 6 installs the
decoded table at `DAT_8007B7C8`, and field init `FUN_801d6704` spawns **one
actor per entry** via `FUN_80024cfc`, each with its own accumulator (actor
`+0x68`, seeded to `100` so every entry's first copy fires on the first game
tick at scene entry - all eight share one epoch). The SCUS actor walker
`FUN_8001ada4` **case 0xB** steps each actor: `acc += dt` per game tick
(`dt` = the adaptive frame-step byte `DAT_1F800393` `FUN_80016B6C` rewrites;
overworld `3`, towns `2`), and on `acc >= hold_vsyncs` it emits a libgpu
`MoveImage` of `RECT{src_x, src_y, 16, 1}` onto `(dest_x, dest_y)`, **resets
`acc` to zero** (not subtract-remainder: live exec-BP traces on the
`MoveImage` wrapper show strictly constant intervals with zero jitter), and
advances the frame index with wrap-around. The real interval is therefore
`ceil(hold / dt) * dt` vsyncs. The eight entries: the ocean head `(0, 506)` -
18 steps hold 8 over the row-505 strip (`x = 0..208` in 16-px steps with a
128/144 ping-pong x3 mid-cycle; every 9 vsyncs at `dt = 3`, full cycle 162
vsyncs ≈ 2.7 s), `(0, 508)` 4 steps hold 6 from row 504, `(16, 508)` 4 steps
hold 8 from row 504, `(16, 506)` 7 steps hold 48-then-12 from row 503,
`(32, 506)` 7 steps hold 10 from row 502, `(32, 509)` 4 steps hold 20 from
row 501 (`x = 0, 16, 32, 16`), `(32, 508)` 4 steps hold 6 from row 498 (the
script-faded park cells), and `(48, 500)` 4 steps hold 6 from row 498
`x = 160..208`. The cycle is live-verified on **all three kingdoms**
(`crates/engine-shell/tests/world_map_ocean_clut_live.rs`); the disc-gated
`clut_walk_real` test pins the full entry set against all three bundles.

The engine consumes the table in `play-window` (`WaterAnim::Walk` /
`advance_ocean_animation`): slot 5 is parsed at scene resolve, all eight
entries run as independent accumulators with the retail semantics above
(clock in retail vsync units - a game tick every `World::frame_step`
vsyncs), and each fire is a CPU-VRAM 16x1 `move_image` + re-upload. The
legacy single-cell 13-frame ocean-head cycle (`legaia_asset::ocean`,
`WaterAnim::Ocean`) survives only as the fallback for a bundle without a
parseable slot 5 (no retail bundle). **Source-strip residency**: the walk
sources park in VRAM rows 498/499/501..505 as raw CLUT-block records in the
bundle's slot-0 TIM_LIST (`[u32, u32]` prefix + a bare TIM CLUT block; no
TIM magic, so plain TIM walkers skip them - `clut_walk::park_strips`
locates them, and the engine parks them at scene resolve). map01 ships the
full six-record set plus TIM CLUTs for rows 500/501/508; **map02 / map03
ship only rows `{501, 503, 505}`** and inherit the kingdom-invariant rest
(rows 498/499/502/504) as **VRAM residue from the Drake upload** - map01 is
always the first world map, and the resident Sebucus / Karisto captures
hold map01's record bytes on those rows byte-exact - so the engine parks
the byte-identical records from the Drake bundle for rows the scene's own
bundle doesn't carry.

The earlier ten-state map01 capture census stays as corroboration: every
censused animating column falls inside a slot-5 destination cell (row 506
cols `0..48` incl. the STP-set ocean near-copies at 32..39 + the
pure-channel tail at 40..47; row 508 cols `0..48` incl. the map01-only
mirror `[32..47] == [0..15]`; row 509's animated entries 42..43 inside the
`(32, 509)` cell; row 500 cols 62..63 inside `(48, 500)` - a `MoveImage`
rewrites all 16 entries, the stable columns merely coincide across the
strip's frames), and row 507 - which no slot-5 entry targets - is fully
static. The VRAM parity oracle excludes exactly the destination-cell fold
for world-map scenes and asserts the rest
(`vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS`). The row-508 mirror is strip
*content*, not a second writer: on Sebucus / Karisto the `(32, 508)` cell
holds strip content that differs from `(0, 508)`.

**The row-498 park-cell fades are a separate, script-driven family** -
event-triggered MAN `4C 61` ops, not part of the slot-5 table. Two
field-overlay handlers emit them:

- `FUN_801E4C58` - the field-VM `0x4C` n6 sub-`0x61` emitter: with the
  `+0xD` frame count zero, a one-shot 16×1 CLUT-cell write whose
  **coordinates are script operands** (source `(x, y)` at instruction
  `+5`/`+7`, destination at `+9`/`+0xB`, read via the misaligned-u16 helper
  `FUN_8003CE9C`). Non-zero source-y enqueues a libgpu `MoveImage` cell
  copy; zero source-y replicates the `+5` halfword as a flat BGR555 colour
  across all 16 entries and `LoadImage`s it. A non-zero `+0xD` instead
  spawns the cross-fade actor below (descriptor `DAT_801F2918`).
- `FUN_801E4794` - the multi-frame **cross-fade** state machine (installed
  via the `[0xFFFF0000][handler]` descriptor records at `0x801F291C+`):
  captures two 16-colour cells (`StoreImage` of `+1`/`+3` and `+5`/`+7`),
  precomputes per-entry per-channel step deltas `(B−A)/frames` (`+0xD`),
  accumulates `delta × dt` each game tick where `dt` is the adaptive
  frame-skip byte `0x1F800393` (vsyncs per game tick, rewritten per frame
  by `FUN_80016B6C`), and `LoadImage`s the repacked cell to `+9`/`+0xB` -
  so the `+0xD` operand is denominated in **vsyncs** and the fade's
  real-time length is frame-rate independent. On `counter >= frames` it
  `MoveImage`s cell B (or flat-fills) onto the destination, frees the
  scratch, and clears the spawning script context's halt bit
  (`*(ctx+0x94)+0x10 &= ~0x400`). Engine mirror:
  `legaia_engine_core::clut_fx` (the arithmetic kernel) +
  `World::step_clut_fx` (the VRAM driver), fed by the
  `op4c_n6_sub_61_emitter` field-VM host hook.

Both families bottom out in the statically-linked libgpu (`MoveImage
FUN_80058490`, which patches the static 5-word GP0 packet template at
`0x80078DFC`; `LoadImage FUN_800583C8`; `StoreImage FUN_8005842C`) - which
is why no `y = 506/508/509` rect constant exists in any code image: the
head-walk operands are **data** (the kingdom-bundle slot-5 table above),
and the row-498 fade operands are MAN script operands. map01's field MAN
holds exactly eight `4C 61` ops, all on the **row-498 strip park row** -
four one-shots (`frames = 0`) copying cell `(112, 499)` onto
`(0/16/32/48, 498)`, and four cross-fades (`frames = 0x80` = 128 vsyncs)
fading those same four cells back toward `(112, 499)`
(`legaia_engine_core::man_field_scripts::scene_clut_cell_fx`, disc-gated
`map01_clut_fx_disc`); the head-walk operands appear in **no** MAN
(exhaustive u16 scan) because they never were script-carried. The lockstep
phase coupling across rows comes from the walker actors sharing the
game-tick clock and spawn epoch, not from one wider rect.

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
[`boot.md`](boot.md#game-mode-state-machine) mode-12 row.

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
