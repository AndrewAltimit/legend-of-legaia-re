# legaia-engine-world-views

The play hosts' reads of a running `World` outside battle. Every function
here takes a `&World` (or a scene a scratch `World` stages) and answers what
a host draws this frame; none of them writes the world. The rules, layouts
and view structs these fill in live in the kernel crates; this crate is the
one read of live state that produces them, so the native window, the browser
play page and `engine-screens` all draw from the same answer.

## What lives here

Each module is named for the kernel module whose types it fills in.

- **`field_view_window`** - the visible-tile crop's policy over a `World`:
  whether a host crops this frame (`field_view_cells`) and the placed-actor
  and field-NPC gates (`placed_actor_visible`, `npc_actor_visible`). The
  crop kernel itself is `engine-scene::field_view_window`.
- **`scene_staging`** - the scene questions that need simulation state:
  whether a placed draw is live against the world's windowed static-object
  list (`placed_draw_live`), the story-hidden record set a scratch `World`
  staged at a scene's free-roam visit evaluates
  (`story_hidden_records_for_scene`), and the full static assembly built on
  it (`assemble_field_scene`).
- **`effect_draws`** - the live effect layer's draw lists: the effect-pool
  billboards (`effect_billboards`) and the effect models, move-VM parts,
  draw-kind-4 nodes, battle ground shadows and field stager parts with
  their placements composed (`effect_model_draws`). Each host resolves the
  meshes and adds its own outer frame.
- **`world_map_markers`** - which overworld marker segments the frame owes
  and the screen quads they become; the marker model is
  `engine-field::world_map_markers`.
- **`world_map_sky`** - the overworld sky band's sprites off the world map
  state (`render-kernels::world_map_sky`).
- **`screen_passes`** - the field and overworld screen-space passes as
  finished `screen_prim::ScreenPrim`s, one call each from `World`: drop
  shadows, move-VM strips, attached lights, world-map markers and the sky
  band. Each is the mode gate, the resolver's non-scripted frame on the
  scene box's centre (`scene_centre`) and the kernel's prim wrap - the
  three steps both hosts used to spell themselves.
- **`world_map_panel_host`** - the field party HUD's gates and rows
  (`field_hud_suppressed`, `field_hud_view_mode`, `field_party_hud_members`).
- **`shop`, `pause_screens`** - the party compare rows and the item target
  panel (`engine-menus`).
- **`tile_board`** - the tile-board minigame's per-cell actor draws
  (`engine-minigames::tile_board`).
- **`timed_fight`** - the turn-limited fight's strip gate and numbers
  (`engine-menus::timed_fight`).
- **`muscle_dome`** - the Muscle Dome magic loadout off a roster slot
  (`engine-menus::muscle_dome_session`).

## Place in the layering

```text
engine-shell / web-viewer                       hosts
engine-screens, engine-session                  host-shared top
engine-scene-host                               SceneHost
engine-world-views, engine-battle-hud           reads of World
engine-core                                     World, the tick, battle loop
```

Nothing in `engine-core` names this crate, so changing a view rebuilds the
hosts but not the simulation. The battle screen's reads are the sibling
[`engine-battle-hud`](../engine-battle-hud/README.md). The per-frame camera
resolution every host calls (`camera_view`) is the same kind of read and
still lives in `engine-core`. The `World` fields these reads take are `pub`
for this reason.

## Tests

Unit tests sit beside their module. `tests/*.rs` build as one `integration`
binary (`cargo test -p legaia-engine-world-views --test integration foo::`):
disc-gated checks on the scene assembly `assemble_field_scene` produces
(lit rows, CLUT layering, mesh pool walk, the kingdom CLUT overwrite). They
skip and pass when `LEGAIA_DISC_BIN` is unset.
