# legaia-engine-battle

The battle engine's kernels: pure functions and self-contained state types
that never touch `World`. Free of wgpu, winit and cpal, so it builds for
native and `wasm32` alike.

The stateful half stays in `legaia-engine-core`: `World`'s battle state and
the `world::battle` submodules (the round loop driver, command flow, cast
band, monster turn picker, victory and teardown) own the composition and call
down into this crate. `engine-core` re-exports every module here at its old
path, so `legaia_engine_core::monster_ai` and `legaia_engine_battle::monster_ai`
name the same module.

## What belongs here

A module moves here when its whole dependency closure inside the engine is
itself in this crate - it may use `legaia-engine-vm`, the asset crates and
its siblings, but nothing that reaches `World`. A kernel tangled with `World`
stays in `engine-core` until its pure half is split out. Doc links that point
back up at `engine-core` are written as plain code spans, since rustdoc cannot
resolve a link into a dependent crate.

## Modules

**Combat rules**

- `monster_ai` - the per-monster-id scripted-cast `switch` of the action
  picker `FUN_801E9FD4`, the recent-target ring, and the battle-scoped
  cooldown state.
- `battle_steal` - a slain monster's death spoils: the Evil God Icon's steal
  attack and the return of a thief's loot (`FUN_8004AD80` death arm).
- `art_strike` - Tactical-Art strike applier (`ArtStrikeInfo` to HP delta,
  status, SFX cues).
- `ap_gauge` - the Action-Point gauge behind Tactical Arts input.
- `battle_stats` - equipment-aware stat aggregator.
- `accessory_passives` - accessory ("Goods") passive effects as bits of the
  per-character ability word.
- `battle_events` - the event queue the action SM emits through its host.
- `move_power` - engine wrapper over the move-power table (PROT 0898).

**Catalogs + encounters**

- `spells` / `retail_magic` - spell catalog and cast resolver; the retail
  player Seru-magic table pinned from `SCUS_942.54`.
- `monster_catalog` - monster definitions and formation tables.
- `encounter` / `encounter_man` / `encounter_record` / `encounter_registry`
  / `region_encounter` - per-scene encounter tables (built from MAN bytes),
  the retail encounter record, and the step- and region-keyed triggers
  (`FUN_801D9E1C`).

**Growth**

- `levelup` - post-battle level-up tracker: per-slot XP against per-level
  thresholds and the stat-growth rows.
- `magic_xp` - Seru-magic spell XP.
- `seru_learning` / `seru_stats` / `seru_trade` - Seru capture + learning,
  per-Seru stat grants, and the runtime side of `--seru-trade`.
- `tactical_arts` / `tactical_arts_editor` - learn-on-use tracking and the
  Arts chain editor.

**Per-frame passes**

- `battle_anim` - per-actor battle clip playback.
- `battle_afterimage` - Super / Miracle Art after-image ghosts.
- `battle_body_blend` - whole-mesh semi-transparency.
- `battle_effect_clut` / `battle_status_clut` - the effect palette stage and
  the status recolour pass of `FUN_8004CE2C`.
- `battle_sideband` - the side-band tick keyed on the battle stage id
  (sparring caption, Cort's two boss-stage modules).
- `battle_seats` - the authored stage seats battle setup stamps into every
  combatant (`FUN_800513F0`).
- `battle_return_flags` - MAIN INIT's back-from-battle story-flag arm.

## See also

- [`docs/subsystems/battle.md`](../../docs/subsystems/battle.md),
  [`battle-action.md`](../../docs/subsystems/battle-action.md),
  [`battle-formulas.md`](../../docs/subsystems/battle-formulas.md).
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
