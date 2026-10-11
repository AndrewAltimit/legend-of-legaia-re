# legaia-engine-minigame-scenes

The minigames' **3D scene surfaces**: the
kernels that turn a running minigame into posed vertex buffers and a camera,
one per game, which every host draws. `engine-core` depends on this crate,
never the other way round. Hosts and tests name the two modules directly
(`legaia_engine_minigame_scenes::baka_duel_scene`, `legaia_engine_minigame_scenes::muscle_dome_scene`).

## What lives here

| Module | Covers |
|---|---|
| `baka_duel_scene` | The Baka Fighter duel: the arena camera (`DuelCamera`), the per-seat fighter clip (`FighterMotion`), the combined buffers for both fighters, their afterimage ghosts, the arena walls and floor, and the per-host cache `BakaDuelSurface`. See [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md). |
| `muscle_dome_scene` | The Muscle Dome arena: the arena shell, the battle ground grid, the lead's assembled battle form and the ladder's monster on the battle seats, filmed by the battle camera script; `MuscleDomeSurface` and the turn timeline. See [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md). |

## The split line

Both surfaces read the disc only through a `read_prot` closure and drive
their poses off the rules engines' own state, so neither needs a `World`, a
loaded `Scene` or a renderer. Their kernel dependencies all sit in the
World-free crates below `engine-core`: the rules engines in
`legaia-engine-minigames`, the dome session and battle command menu in
`legaia-engine-menus`, the battle seats in `legaia-engine-battle`, the packet
colour helpers in `legaia-engine-field`, the cameras in
`legaia-engine-battle-vm`.

What stays in `engine-core` is everything that reads the world: the
`World` entry points that seat a rung and tick a surface, the dance and
fishing venues (built out of a loaded `Scene` / `SceneResources`), and the
disc-gated tests that drive a surface through a live `World`.
