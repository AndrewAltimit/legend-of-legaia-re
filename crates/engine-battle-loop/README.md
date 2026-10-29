# legaia-engine-battle-loop

The battle loop's carried state, minus `World`: the battle session
(`BattleState`), the cast band (`CastFxState` and the stager / pending-cast
records it holds), the round flow, the battle frame clock, the message
banner and the Auto attack's per-battle state. Free of wgpu, winit and
cpal, so it builds for native and `wasm32` alike.

`engine-core`'s `World` holds one `BattleState` and one `CastFxState`
(`world.battle`, `world.casting`), and its battle loop steps them. Hosts
and tests name the types here directly
(`legaia_engine_battle_loop::battle_state::BattleState`); `engine-core`
does not re-export them.

## What lives here

| Module | Covers |
|---|---|
| `battle_state` | `BattleState`: per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues, end-of-battle latches; the retail-compare debug seeds (`InflightCastSeed`, `CaptureApproach`); `BattleBuff`, `BattleRewards`. |
| `cast_fx_state` | `CastFxState`: the active summon scene, the slot-B module phase bytes and their carried words, the cast stager and move-FX spawns. |
| `cast_band` | The cast band's records: `PendingCast`, `SummonStager`, `CasterStageRun`, `CastModuleCodeRun`. |
| `module_presentation` | The slot-B presentation hook's per-cast state. |
| `round_flow` | `RoundFlow`: the command / execution band split and the party's committed commands. |
| `frame_clock` | `BattleFrameClock`: how one-vsync ticks group into retail's battle frames (`FUN_80016B6C`). |
| `message_banner` | The top message banner's element ids and its raised line. |
| `auto_combo` | The Auto attack's disc inputs and per-seat queues (`FUN_801F0450`). |

## The split line

Everything here is data the battle loop carries plus the methods that read
or step that data alone. A method that needs a second `World` field (the
actor table, the party, the disc tables, the RNG) stays an `impl World` in
`engine-core`: an inherent `impl` has to live beside its type, and the battle
loop reaches most of the `World`, so the loop itself cannot leave
`engine-core` without its whole host coming with it.

The crate sits above every kernel it names (`engine-battle`,
`engine-battle-menus`, `engine-battle-vm`, `engine-cast`, `engine-effects`,
`engine-field`, `engine-menus`) and below `engine-core`; its row in
[`crate-layering.toml`](../../scripts/ci/crate-layering.toml) holds the
edges. Doc comments name `engine-core` items in plain code spans, since
rustdoc cannot resolve a link into a dependent crate.

## See also

- [`docs/subsystems/battle-round-loop.md`](../../docs/subsystems/battle-round-loop.md) -
  one fight end to end.
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#crate-layering) -
  where this crate sits.
