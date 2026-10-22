# legaia-engine-battle

The battle engine's kernels: pure functions and self-contained state types
that never touch `World`. Free of wgpu, winit and cpal, so it builds for
native and `wasm32` alike.

The stateful half stays in `legaia-engine-core`: `World`'s battle state and
the `world::battle` submodules (the round loop driver, command flow, cast
band, monster turn picker, victory and teardown) own the composition and call
down into this crate. Consumers name the kernels here directly
(`legaia_engine_battle::monster_ai`).

## What belongs here

A module moves here when its whole dependency closure inside the engine is
itself in this crate - it may use `legaia-engine-battle-vm`,
`legaia-engine-system`, `legaia-render-kernels`, the asset crates and
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
- `encounter` / `encounter_man` / `encounter_formation` / `encounter_registry`
  / `region_encounter` - per-scene encounter tables (built from MAN bytes),
  the retail encounter record, and the step- and region-keyed triggers
  (`FUN_801D9E1C`).

**Growth**

- `victory` - the battle-end presentation's phase constants, the
  `VictorySequence` state and the win-pose tier + pick (`FUN_8004E568`); the
  walk over a `World` is engine-core's.
- `levelup` - post-battle level-up tracker: per-slot XP against per-level
  thresholds and the stat-growth rows.
- `magic_xp` - Seru-magic spell XP.
- `seru_learning` / `seru_stats` / `seru_trade` - Seru capture + learning,
  per-Seru stat grants, and the runtime side of `--seru-trade`.
- `tactical_arts` / `tactical_arts_editor` - learn-on-use tracking and the
  Arts chain editor.

**Battle input state**

- `target_picker` - the target cursor over the live formation, plus the
  enemy target-menu row dedup / labelling and its overlap-relaxation
  layout (`FUN_801D9D3C`).
- `arts_command_input` - the retail Arts command entry: directional buffer,
  per-command AP debit from the turn pool, the auto-command-string preseed
  and the Begin | Reselect review.
- `battle_arts` / `battle_magic` - the Arts and Magic submenus.

**Per-frame passes**

- `battle_anim` - per-actor battle clip playback.
- `battle_afterimage` - Super / Miracle Art after-image ghosts.
- `battle_attach_swap` - the per-channel model table of a party seat and the mesh-variant swap (`FUN_8004CCD4`) that rewrites it; the battle draw set.
- `battle_party_form` - a party member's battle form: the player-file
  assembly (or the PROT 1204 fallback), band pixels and palette, idle and
  action clips, art bank, over a `PartyFormDisc` reader and a `VramSink`
  (`VramWriteLog` records the writes for a renderer). Reading the roster off
  a `World` and installing the forms on the actors is engine-core's
  `battle_party_form`.
- `battle_body_blend` - whole-mesh semi-transparency.
- `battle_effect_clut` / `battle_status_clut` - the effect palette stage and
  the status recolour pass of `FUN_8004CE2C`.
- `battle_sideband` - the side-band tick keyed on the battle stage id
  (sparring caption, Cort's two boss-stage modules).
- `battle_stage_module` - those two boss-stage modules for the Cort fight:
  PROT 0968 (arrival) and PROT 0969 (form transition), one phase machine on
  `ctx[+0x289]` driving the first monster seat.
- `battle_tutorial` - the sparring-tutorial prompt machine of the stage
  overlay (PROT 967): the `(flow state, lesson)` dispatch, the box styles and
  rects, and the prompt corpus parser `BattleTutorialScript::from_overlay`.
  The disc read that feeds it is `engine-scene-host::disc_readers::battle_tutorial`.
- `battle_seats` - the authored stage seats battle setup stamps into every
  combatant (`FUN_800513F0`).
- `battle_return_flags` - MAIN INIT's back-from-battle story-flag arm.
- `anim_cue` - `walk_anim_cues` / `AnimCueState`, the per-frame walker
  over a playing battle action's 8-slot `(frame, cue)` track
  (`FUN_800508DC`): swing, hit, footstep and knockdown SFX, the party
  `0xC8..=0xFF` band resolved into the arts-voice namespace, and the
  CD-busy fallback ring cue. It emits `AnimCueEmit` decisions; engine-core's battle
  actor tick (`world::actors`) drains them into the SFX ring.
- `sfx_cue` - cue id to the 4-slot pending ring / XA clip (`FUN_8004FE5C`).

**State held on `World`**

- `battle_hud_state` - the battle HUD model: per-slot HP / MP / AP / status
  rows, the damage-popup ring, event log and combo readout, plus the
  per-phase sub-draw steps (`subdraw_steps`, `placement_record`).
- `encounter_state` - the per-scene encounter session, the scripted-encounter
  arm and the roll gates.
- `seru_state` - Seru capture + magic-learning state: capture log and
  registry, this battle's captures, shiny rolls and magic level-ups.

## See also

- [`docs/subsystems/battle.md`](../../docs/subsystems/battle.md),
  [`battle-action.md`](../../docs/subsystems/battle-action.md),
  [`battle-formulas.md`](../../docs/subsystems/battle-formulas.md).
- [`crates/engine-core`](../engine-core/README.md) - the `World` side.
