# legaia-engine-world-views

The play hosts' reads of a running `World` outside battle. Every function
here takes a `&World` (or a scene a scratch `World` stages) and answers what
a host draws this frame; none of them writes the world. The rules, layouts
and view structs these fill in live in the kernel crates; this crate is the
one read of live state that produces them, so the native window, the browser
play page and `engine-screens` all draw from the same answer.

## What lives here

Each module is named for the kernel module whose types it fills in.

- **`camera_view`** - the per-frame camera resolution every host calls:
  which camera owns the frame (`resolve_field_camera` over the follow,
  cutscene, world-map, map-display and venue arms) and its retail GTE
  inputs (`field_follow_view`, `cutscene_view`, `cutscene_owns_camera`,
  `map_display_frame`). The camera globals and the frame enum are
  `engine-field::camera` / `engine-field::camera_frame`.
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
- **`screen_passes`** - the host seam. `HostView` borrows the `World`, the
  scene camera and the scene box together; its methods are the frame's
  camera (`frame`, `follow_frame`, `scripted_target`), the overworld curve
  scale, the move-VM part camera, and the field / overworld screen passes as
  finished `ScreenPrim`s (fog sheets, drop shadows, move-VM strips, attached
  lights, world-map markers, sky band). Beside it, `weapon_trail_prims` and
  `move_fx_streak_quads` are the battle screen FX over the world alone, each
  host supplying only its battle projection. A host keeps what it holds
  alone: the scripted-shot gate and glide, its debug vantages, its boot-title
  gate, its leader-mesh answer.
- **`world_map_panel_host`** - the field party HUD's gates and rows
  (`field_hud_suppressed`, `field_hud_view_mode`, `field_party_hud_members`).
- **`shop`, `pause_screens`** - the party compare rows and the item target
  panel (`engine-menus`).
- **`tile_board`** - the tile-board minigame's per-cell actor draws
  (`engine-minigames::tile_board`).
- **`timed_fight`** - the turn-limited fight's strip gate and numbers
  (`engine-battle-menus::timed_fight`).
- **`muscle_dome`** - the Muscle Dome magic loadout off a roster slot
  (`engine-battle-menus::muscle_dome_session`).
- **`passive_hud`** - the field's passive-ability badge column
  (`FUN_801d095c`) as the `WorldPassiveHud` extension trait: the three
  head-relative world points, the icon list for a projected anchor, and the
  skip test (`engine-field::field_passive_hud`). `engine-screens` does the
  projection between them for both hosts.
- **`minigame_caption`** - the line above a live minigame as the
  `WorldMinigameCaption` extension trait
  (`engine-minigame-scenes::minigame_status`).

## Place in the layering

```text
engine-shell / web-play / web-viewer            hosts
engine-screens, engine-session                  host-shared top
engine-scene-host                               SceneHost
engine-world-views, engine-battle-hud           reads of World
engine-core                                     World, the tick, battle loop
```

Nothing in `engine-core` names this crate, so changing a view rebuilds the
hosts but not the simulation. The battle screen's reads are the sibling
[`engine-battle-hud`](../engine-battle-hud/README.md). The `World` fields
these reads take are `pub` for this reason.

## Tests

Unit tests sit beside their module. `tests/*.rs` build as one `integration`
binary (`cargo test -p legaia-engine-world-views --test integration foo::`):
disc-gated checks on the scene assembly `assemble_field_scene` produces
(lit rows, CLUT layering, mesh pool walk, the kingdom CLUT overwrite), which
skip and pass when `LEGAIA_DISC_BIN` is unset, and the disc-free
`world_map_render_ladder`, which drives the world-map render pass, the map
display's camera hand-off and the panel-actor screen through `World::tick`
on pad input.
