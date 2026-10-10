# legaia-engine-fishing

The fishing minigame's rules engine, split out of `legaia-engine-minigames`.
`engine-minigames` depends on this crate, never the other way round, and
re-exports every module at its old path (`legaia_engine_minigames::fishing`,
and through `engine-core`, `legaia_engine_core::fishing`), so hosts and tests
name the same paths they always did.

## What lives here

| Module | Covers |
|---|---|
| `fishing` | `PondSession`: the pond state machine, cast and reel, tension gauges, species and bite rolls, catch scoring, the rod / lure menu and the prize list. |
| `fishing_actors` | The rod, lure and line actors, the rod mesh faces, the bite kernels and the celebration burst. |
| `fishing_chrome` | The venue chrome: the float actor, splash and ripple spawn geometry, the reel cadence ring. |
| `fishing_hub` | The hub screen a venue's idle shore opens. |
| `minigame_floor` | The venue floor grid: ground height, the polar tables, the water tile class. |

All of it is documented in
[`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md).

## The split line

The five modules name each other, the disc-parsed fishing tables in
`legaia-asset`, the field-region, pad and projection kernels in
`legaia-engine-vm` and the mesh walker in `legaia-tmd` - and no other minigame. That makes fishing the one
game in the overlay band that separates as a leaf: the dance, the Baka
Fighter duel, the slot machine and the Muscle Dome ladder share the band's
overlay chrome and effect-spawn types with each other.

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
