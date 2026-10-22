# legaia-engine-battle-menus

The battle side of the menu front end, minus `World`: the command picker
the live battle loop opens per party turn, the command-flow state byte, the
formation open banner, Koru's timed fight and the Muscle Dome match session.
Free of wgpu, winit and cpal, so it builds for native and `wasm32` alike.
Hosts and tests name its modules directly
(`legaia_engine_battle_menus::battle_input::BattleCommandSession`).

## What lives here

| Module | Covers |
|---|---|
| `battle_input` | `BattleCommandSession`, the player-driven command picker, and the chip-cluster model both it and the dome project. |
| `battle_flow` | The command-flow state byte `ctx[+0x06]` (`FUN_801D0748`) as a projection of the session's phases, and the tutorial box the flow state is showing. |
| `battle_open` | The formation open banner and its hold timers. |
| `timed_fight` | Koru's turn-limit gate and its `Turns Left / HP Left` strip readout. |
| `muscle_dome_session` | One Muscle Dome leg: `MuscleDomeSession`, its command menu, ring, choreography and loadout kernels, over the contest ladder `legaia-engine-minigames` carries. |

The state machines behind them: [`battle-command-flow.md`](../../docs/subsystems/battle-command-flow.md)
and [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md).

## The split line

These modules name each other, the battle kernels below them
(`legaia-engine-battle`, `legaia-engine-battle-vm`), the Muscle Dome tables
(`legaia-engine-minigames`) and the options screen's value enums
(`legaia-engine-system`'s `option_values`, which the battle input and the dome ring
key their Select Attack mode on). No pause, shop or save screen is
reached from here, and no module of `legaia-engine-menus` names this crate:
the two are siblings at the same rank.

The Item command does not open a session of this crate's: the live loop
opens `legaia-engine-menus`' `inventory_use` session, which is shared with
the field menu. The `World`-taking battle loop that steps these sessions is
`engine-core`'s; the HUD reads of them are `legaia-engine-battle-hud`.

## See also

- [`crates/engine-menus`](../engine-menus/README.md) - the pause, shop and
  save front end beside this crate.
- [`docs/subsystems/battle-round-loop.md`](../../docs/subsystems/battle-round-loop.md) -
  one fight end to end.
