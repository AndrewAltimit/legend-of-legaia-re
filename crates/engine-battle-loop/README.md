# legaia-engine-battle-loop

The battle loop's state and the steps that need nothing else, minus `World`:
the battle session (`BattleState`), the cast band (`CastFxState` and the
stager / pending-cast records it holds), the round flow, the battle frame
clock, and the `BattleState` / `CastFxState` methods and free kernels that
read or write that state alone. Free of wgpu, winit and cpal, so it builds
for native and `wasm32` alike.

`engine-core`'s `World` holds one `BattleState` and one `CastFxState`
(`world.battle`, `world.casting`), and its battle loop steps them. Hosts and
tests name the types here directly
(`legaia_engine_battle_loop::battle_state::BattleState`) and call the steps
on the field (`world.battle.set_battle_flow(..)`); `engine-core` does not
re-export them.

## What lives here

| Module | Covers |
|---|---|
| `battle_state` | `BattleState`: per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues, end-of-battle latches; the retail-compare debug seeds (`InflightCastSeed`, `CaptureApproach`); `BattleBuff`, `BattleRewards`; the result-screen reads. |
| `seat_stats` | The per-seat stat block's steps: working / base halves, the Magic buffs and their per-turn expiry, the UDF / LDF defence pick. |
| `status_word` | The act and magic gates, petrify, confusion and Mystic Shield, read off the status tracker. |
| `tutorial_flow` | The command-flow byte (`ctx[+0x06]`) and the sparring tutorial's prompt boxes and caption. |
| `hud_glides` | The commit-log launch, plaque, round-prompt and ring-open glides, the command pad hold, the intro-names sweep, the Arts list window. |
| `message_banner` | The top message banner's element ids, its raised line and the timed message's hold. |
| `auto_combo` | The Auto attack's disc inputs, flag and per-seat queues (`FUN_801F0450`). |
| `round_flow` | `RoundFlow`: the command / execution band split and the party's committed commands. |
| `frame_clock` | `BattleFrameClock`: how one-vsync ticks group into retail's battle frames (`FUN_80016B6C`). |
| `clip_ladder` | The staged-anim commit's clip-tag ladder and reaction maps (`FUN_8004AD80`). |
| `ui_cues` | The SFX cue each command-surface phase submits (`FUN_801D0748`). |
| `cast_fx_state` | `CastFxState`: the active summon scene, the slot-B module phase bytes and carried words, the cast stager and move-FX spawns. |
| `cast_band` | The cast band's records: `PendingCast`, `SummonStager`, `CasterStageRun`, `CastModuleCodeRun`. |
| `cast_seat_map` | How the slot-B modules' retail actor-table rows map onto engine battle slots. |
| `module_stagers` | The module stager spawn seat and part-pool flush, the Puera / Orb / Juggernaut / Megaton Press calls, Slippery Rain, the Fatal Decision parts. |
| `module_presentation` | The presentation hook's per-cast state, its fades, part parking and wash tail. |
| `module_seat_arm` | The seat writes a module arm makes on its seated parts, as data. |
| `orb_stager` | PROT 0911 (Orb)'s star-swarm kernels and the arms its writes key on. |
| `nova_motion`, `barra_flight` | Nova's lift / landing heights; Barra's circle, arc and dive. |

## The split line

A step lives here when it reads or writes `BattleState` or `CastFxState` and
nothing else. A step that also needs the actor table, the party, the disc
tables, the RNG, the audio queues or the action SM's context stays an
`impl World` in `engine-core`: an inherent `impl` has to sit beside its type,
and most of the battle loop reaches several of those at once, so the loop
driver, the action SM host, the cast band's module runs and the monster AI
stay with `World`. Moving more of them down means moving the state they
share (the actor table first) below `engine-core` too.

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
