# legaia-engine-battle-hud

The battle HUD's reads of a running `World`. Every function here takes a
`&World` (or fills the HUD model from one) and answers what the in-battle
screen draws this frame; none of them writes the world.

## What lives here

- **`sync_battle_hud_rows`** - refreshes the `BattleHud` model
  (`legaia_engine_battle::battle_hud_state`) from the live battle: party and
  enemy rows, the Muscle Dome's rows, the intro and stage banners.
- **Phase and command surface** - `battle_hud_phase`, `battle_command_surface`:
  which sub-draw the frame is in, off the action and command state machines.
- **Command chips and commit log** - `battle_command_chips`,
  `battle_chip_highlight`, `battle_commit_log`, `battle_combo_style`.
- **Plaques and their glides** - the actor-name, target and target-select
  plaques, the readout bar, the message bar, the magic and Ra-Seru chips and
  every seat offset the placement-table glides produce
  (`battle_action_plaque_dy`, `battle_readout_bar_dy`, `battle_chip_offsets`,
  ...).
- **Chrome** - the Begin tab, breadcrumb, panel visibility, the element badge
  table (`ELEMENT_TO_PLAQUE_BADGE`), the encounter and message banners.
- **Actor names** - the enemy target rows and the intro name labels.

The retail behaviour behind each read is in
[`battle-hud.md`](../../docs/subsystems/battle-hud.md).

## Place in the layering

```text
engine-shell / web-play                         hosts
engine-screens, parity                          host-shared top
engine-battle-hud                               the battle HUD's reads of World
engine-core                                     World, the tick, battle loop
```

Nothing in `engine-core` names this crate, so a HUD change rebuilds only the
crates above it. The two battle-screen facts the simulation needs for its own
decisions stay in `engine-core` as `BattleState` methods: `raseru_forbidden`
(the command ring refuses the Ra-Seru press) and `result_windows_dy` (the
results banners carry the windows' raise glide). The glide arithmetic both
sides share is `LogLaunch::seat_dy` in `engine-battle-vm`.

The battle-state fields the reads take are `pub` on `BattleState` for this
reason. The glide and command steps the tests drive a frame at a time
(`tick_battle_command`, `step_action_plate_glides`, ...) stay internal to
`engine-core`; the tests reach them through
`engine-core::world::test_drivers::BattleTestDrivers`, which exists only
under engine-core's `test-drivers` feature, enabled by this crate's
`[dev-dependencies]` alone.

## Tests

Unit tests live in `src/tests.rs`. `tests/*.rs` build as one `integration`
binary (`cargo test -p legaia-engine-battle-hud --test integration foo::`);
they drive a `World` through a battle and assert on what the HUD reads back.
Tests that read a disc skip and pass when `LEGAIA_DISC_BIN` is unset.
