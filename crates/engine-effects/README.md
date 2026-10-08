# legaia-engine-effects

The engine's effect kernels that never touch `World`: the battle action's
effect-script walk, the effect ribbon and the render-mode-4 emitters, the
summon creature's effect scene, the PROT-0900 screen-effect widgets, the
object-effect table and a move-VM part's motion block. Free of wgpu, winit and
cpal, so it builds for native and `wasm32` alike.

`legaia-engine-core` owns the composition - `World`'s cast / move-FX scene
graph, the field VM's screen-effect ops and the battle animation tick - and
re-exports every module here at its old path, so
`legaia_engine_core::summon` and `legaia_engine_effects::summon` name the same
module.

## What belongs here

A module moves here when its whole dependency closure inside the engine is in
this crate, `legaia-engine-vm`, or the other `World`-free crates below
`engine-core` (`legaia-engine-battle` for the spell table,
`legaia-engine-minigames` for the shared colour word). Doc links that point
back up at `engine-core` are plain code spans, since rustdoc cannot resolve a
link into a dependent crate.

## Modules

- `action_effect_script` - the battle action's 8-byte **effect-script** record
  walk (`FUN_801DEA50`): frame gates, facing rotation via `RetailRotationLut`
  (the `0x80070A2C` trunc-sine pair), the move-power record index, and the
  target band the terminator's homing seed sweeps. Driven per battle frame by
  `World::tick_battle_animations` over each actor's committed clip
  (`MonsterAnimation::effect_script`); spawns drain via
  `World::drain_battle_effect_spawns`.
- `effect_ribbon` - the battle effect-**ribbon** geometry generator
  (`FUN_801CFA48`): a seeded random walk emitting six vertices per step with a
  tapering radius, plus the packet-chain layout its render half consumes.
  Geometry only; the GPU emit is render-track.
- `effect_default_arm` / `effect_sprite_arm` - the render-mode-4 emitters
  as per-frame meshes.
- `summon` - the Seru-magic summon scene-graph driver over the stager
  overlays' move-VM part records (an engine stand-in render; the retail
  player summon draws as an ordinary battle actor - see the module docs).
- `screen_fx` - the PROT-0900 screen-effect widget family (iris mask,
  sprites, panels, letterbox) the field VM drives.
- `object_effect` - the `0x80083FF8` object-effect table.
- `part_motion` - a move-VM part's motion block between steps.

## See also

- [`docs/subsystems/effect-vm.md`](../../docs/subsystems/effect-vm.md)
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
