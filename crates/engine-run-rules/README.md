# legaia-engine-run-rules

The run rules, `World`-free: the run-wide twists a player picks on the New
Game rules screen and a run's saves carry. The default rules are the retail
game. See [`docs/subsystems/run-rules.md`](../../docs/subsystems/run-rules.md).

- `rules` - `RunRules` (mode, `Difficulty`, `NgPlus` with its `CarrySet`,
  `Ironman`, `Randomizer` with its seed), the presets' `Knobs`, the run's
  summary line.
- `scaling` - New Game+'s enemy curve over the cycle and the party's level.
- `boss_rush` - the retail stages (scene, scripted row, boss, level), the run
  state machine, its clock, the intermission rows and the leaderboard.
- `setup` - `RulesSetup`, the New Game rules screen's pad-driven session.
- `save` - `RunSave`, the versioned body of the `LGXF` save block; nothing is
  written under the retail rules.

The randomizer's planners are [`legaia-randomizer`](../randomizer/README.md)'s.
The `World` side is `legaia_engine_core::world::run_rules`; the hosts' shared
half is `legaia_engine_session::run_rules`.
