# legaia-engine-minigame-scenes

The minigames' **3D scene surfaces and venues**: the kernels that turn a
running minigame into posed vertex buffers, a venue scene and a camera, one
per game, which every host draws - plus `MinigameState`, the minigame seat
the `World` holds as one field. `engine-core` depends on this crate, never
the other way round. Hosts and tests name the modules directly
(`legaia_engine_minigame_scenes::baka_duel_scene`,
`legaia_engine_minigame_scenes::fishing_scene`, ...).

## What lives here

| Module | Covers |
|---|---|
| `baka_duel_scene` | The Baka Fighter duel: the arena camera (`DuelCamera`), the per-seat fighter clip (`FighterMotion`), the combined buffers for both fighters, their afterimage ghosts, the arena walls and floor, and the per-host cache `BakaDuelSurface`. See [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md). |
| `muscle_dome_scene` | The Muscle Dome arena: the arena shell, the battle ground grid, the lead's assembled battle form and the ladder's monster on the battle seats, filmed by the battle camera script; `MuscleDomeSurface` and the turn timeline. See [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md). |
| `dance_venue` | The dance hall (`DanceVenue::build`: the `other7` scene, its resources, draw list, HUD page and entry face stamps), the venue camera and its keyframe-track pose, the GPU polygon-size rule, and `DanceVenueStage`. See [`minigame-dance.md`](../../docs/subsystems/minigame-dance.md). |
| `dance_cast_scene` | `DanceCastSurface`: the run's dancers posed into one vertex buffer off the hall's TMD pool. |
| `dance_hud_art` | `stage_dance_hud_vram`: the HUD art the run's widget table names, read through a `ProtIndex` into a VRAM. |
| `fishing_venue` | The fishing venue's actors, party placements, camera and floor solver (`FishingVenue`, `tick_fishing_venue`) and the line frame. See [`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md). |
| `fishing_scene` | `FishingSurface`: the `other1` venue scene, the party bodies and the sky strip every host draws. |
| `fishing_rod` | `rod_mesh_from_scene`: the three rods and the bend lifted out of a loaded venue `Scene`. |
| `minigame_state` | `MinigameState` (every minigame session plus the casino coin / point-card wallet), `FishingCaptionText` and `pond_event_cues`. |
| `minigame_status` | The status caption both hosts show outside the game picture: per-game rows as data (`StatusRow`), `caption` over the seat, and the host's answers (`StatusHost`). The hosts call it through `WorldMinigameCaption::minigame_caption`. |

## The split line

Nothing here names a `World` or a `SceneHost`. The duel and arena surfaces
read the disc through a `read_prot` closure; the dance and fishing venues
load a `Scene` / `SceneResources` through `engine-scene`. Their kernel
dependencies all sit in the World-free crates below `engine-core`: the rules
engines in `legaia-engine-minigames` and `legaia-engine-fishing`, the dome
session and battle command menu in `legaia-engine-menus`, the battle seats
in `legaia-engine-battle`, the packet colour helpers and field camera in
`legaia-engine-field`, the cameras in `legaia-engine-battle-vm`.

What stays in `engine-core` is everything that reads the world: the `World`
entry points that seat a rung and tick a surface, the fishing venue half
that takes the scene host (`fishing_venue::tick_fishing_venue_on_host` /
`venue_floor_bytes`), and the disc-gated tests that drive a surface through
a live `World`. The dance venue's globals step
(`dance_venue::sync_dance_venue`) lives here: it takes the scene mode, the
minigame seat and the scene block base rather than the `World` holding them. A disc-gated
test that needs only a venue and a `ProtIndex` (the dance cast surface, the
dance HUD page staging) lives in this crate's own `tests/`.
