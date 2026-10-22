# legaia-engine-fishing

The fishing minigame's rules engine. `legaia-engine-minigames` is a
sibling: neither crate depends on the other. Hosts and tests name its modules
directly (`legaia_engine_fishing::fishing`).

## What lives here

| Module | Covers |
|---|---|
| `fishing` | `PondSession`: the pond state machine, cast and reel, tension gauges, species and bite rolls, catch scoring, the rod / lure menu and the prize list. |
| `fishing_actors` | The rod, lure and line actors, the rod mesh faces, the bite kernels and the celebration burst. |
| `fishing_chrome` | The venue chrome: the float actor, splash and ripple spawn geometry, the reel cadence ring. |
| `fishing_hub` | The hub screen a venue's idle shore opens. |

All of it is documented in
[`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md).

## The split line

The venue floor the pond stands on - ground height, the polar tables, the
floor sweep - is the shared minigame floor in `legaia-engine-field`
(`minigame_floor`), which the dance floor's step markers use too.

The modules here name each other, the disc-parsed fishing tables in
`legaia-minigame-formats`, the shared minigame floor and field-region queries
in `legaia-engine-field`, pad input
and the BIOS `rand` in `legaia-engine-system`, the GTE divide in
`legaia-render-kernels`, the vertex-morph stager in `legaia-scene-formats` and
the mesh walker in `legaia-tmd` - and no other minigame. That makes fishing the one
game in the overlay band that separates as a leaf: the dance, the Baka
Fighter duel, the slot machine and the Muscle Dome ladder share the band's
overlay chrome and effect-spawn types with each other. The splash, ripple and
celebration-burst types carry their own pool seat (`fx_seat`), so the shared
effect pool in `legaia-engine-minigames` spawns them without this crate
depending on it or it on this one.

What stays in `legaia-engine-minigames`: `minigame_fx`, the shared effect-part
pool. It ages the pond's splash, ripple and celebration parts on the dance
game's ramp, so it sits above both games and takes this crate's spawn
geometry as input.

What stays in `engine-core`: `World::tick_fishing`, the fishing venue and
scene assembly (built from a loaded `Scene`), and the disc-gated tests that
drive a pond through a live `World`.

## See also

- [`crates/engine-minigames`](../engine-minigames/README.md) - the other
  minigame rules engines.
