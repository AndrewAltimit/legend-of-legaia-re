# legaia-engine-core

Engine-agnostic primitives for the clean-room engine reimplementation:
virtual filesystem, asset cache, frame timing.

No `wgpu` / windowing / audio dependencies - the asset crates (Track 1)
talk to this layer, and the render and audio crates read from it.

## What it provides

### `Vfs` trait

Source of asset bytes. Two backends:

- `DirVfs` - filesystem-backed, rooted at a directory. Used in
  development against the output of `legaia-extract`.
- (Planned) `DiscVfs` - reads directly from a disc image, so end users
  don't need to extract anything ahead of time.

Both yield raw bytes addressed by a logical name (e.g.
`"prot/0123_some_entry.bin"`). The asset crates above this layer turn
bytes into typed structures.

### Asset cache

A bounded in-memory cache keyed by Vfs name. Avoids re-decoding the same
TIM/TMD/VAB on every frame when an actor is referenced repeatedly.

### Frame timing

`FrameClock` - fixed-step driver that targets the PSX's nominal NTSC
60 Hz. Returns the number of logical ticks elapsed since the last call,
so the host can drive the script VMs deterministically regardless of
render rate.

### Composite `World`

`world::World` ties together the actor, move, effect, field, and battle
VMs. One actor table (default capacity 64) is shared across all four
script VMs; the `Host` traits are implemented by routing through this
struct. `World::tick` runs:

1. Effect pool tick (every frame, every mode).
2. Per-actor move-VM tick - only for active actors with bytecode loaded
   via `set_move_bytecode`.
3. Mode-specific top-level VM:
   - `SceneMode::Battle` → battle-action state machine step.
   - `SceneMode::Field` / `SceneMode::Cutscene` → field-VM step. In
     `Field` this is followed by `step_field_locomotion` - the free-movement
     player controller (port of `FUN_801d01b0`): the held d-pad becomes a
     camera-relative direction (remapped by `field_camera_azimuth`), the
     player actor advances in 2-unit steps with per-axis collision against
     the per-scene `field_collision_grid`, and facing is updated. The grid
     (one byte per 128-unit tile, high nibble = 4 sub-cell wall bits) is
     zeroed at field entry and painted by the field-VM `0x4C` outer-nibble-7
     op as the prescript runs. See
     [`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md).
   - `SceneMode::Title` → no further VM.

Engines that want a different storage layout (ECS, custom parallelism)
implement the per-VM `Host` traits themselves; `World` is the default.

### Battle helpers

- `art_strike` - translates `ArtStrikeInfo` into an `ArtStrikeOutcome`
  (HP delta, status, scheduled SFX cues) the world drains into its
  battle event queue.
- `ap_gauge` - per-character Action-Point gauge driving Tactical Arts
  command input. Charges +5 on Spirit-press, refills per turn.
- `battle_stats` - equipment-aware stat aggregator (clean-room port of
  `FUN_80042558`). Sums per-item modifiers, ORs ability bits, folds
  status-effect modifiers (Burned -ATK, Confused halves accuracy,
  Asleep / Stunned / Petrified zero evasion, Silenced / Petrified
  block Magic).
- `items` - typed inventory item-effect catalog. `apply_effect`
  resolves an `ItemEffect` against a `TargetSnapshot` to produce an
  `ItemOutcome` engines fold into world state.
- `battle_round` - per-round orchestrator. `BattleRound::begin` resets
  AP, recomputes equipment-aware stats, writes attack / UDF / LDF into
  the world. `BattleRound::end` ticks status, drains tick damage,
  returns death count.
- `battle_runner` - `BattleRunner` sits between player input and the
  action SM. `begin_round` / `commit_turn` / `end_round` bracket each
  turn; `push_command` / `push_chained_art` gate input against
  `ApGauge`; `commit_turn` resolves the queue through
  `resolve_action_queue` (Miracle / Super expansion). Per-slot buffers
  preserve state across `active_party_slot` switches.
- `battle_session` - `BattleSession` composes the runner + round + HUD
  into a single state machine. Owns the action SM during the `Resolve`
  phase: on `commit_turn` it builds a per-slot `ResolveDriver` queue,
  arms `world.battle_ctx`, calls `world.tick` once per `BattleSession::tick`,
  applies clean-room formula damage on `AttackChain → AttackRecovery`
  transitions, and advances to the next attacker on `EndOfAction`. The
  `Resolve → RoundOutro → Victory / Defeat` transition observes the
  routed `BattleEnd` event. See `docs/subsystems/battle.md#battlesession-resolve-driver`.
- `battle_input` - `BattleCommandSession`: the player-driven command
  picker for the live gameplay loop. A small state machine (command menu
  → target select → confirm) driven a frame at a time from `World::input`.
  Target selection reuses `target_picker`. When
  `World::battle_player_driven` is set, `World::live_battle_tick` opens
  one per party turn and parks the action SM until the player confirms;
  otherwise the loop auto-resolves with a physical Attack. v0.1 enables
  only the Attack command. See `docs/subsystems/battle.md#auto-resolve-vs-player-driven`.
- `battle_hud` - renderer-agnostic UI model. Holds per-slot HP / MP /
  AP / status icons, a queue of `DamagePopup`s with fade timers, and a
  ringed log column. Engines feed it from `BattleEvent::ApplyArtStrike`
  (popups), `StatusEvent` (icons), and `BattleRound::begin` / `end`
  (slot panels). `engine-render::battle_hud_draws_for` turns it into
  `TextDraw`s.
- `inventory_use` - `InventoryUseSession` state machine for the field
  + battle inventory flow. Filters items by `InventoryContext`,
  validates target compatibility (Revive vs alive), folds `ItemOutcome`
  through `World::use_item`.
- `tactical_arts_editor` - the field-menu Arts screen: `ChainEditor`
  (Browsing → Editing → Naming → Done) composes a directional chain into
  a per-character `ChainLibrary`. `World::chain_library` /
  `World::store_chain_library` bridge that library to `World.saved_chains`,
  so a chain authored in the menu serializes with `save_full` and is
  offered in the next battle via `build_battle_arts_rows` - the same path
  whether it was edited live or loaded from a save (`SavedChain::to_record`
  / `from_record` pack to the `Command` byte alphabet the battle side reads).
- `man_field_scripts` - opcode-aware walk of a scene MAN's partition-1
  field-VM scripts (record 0 = scene-entry system script, records 1.. =
  per-actor interaction scripts). `walk_partition1_scripts` bounds each
  record to its own bytes, runs the `legaia_engine_vm::field_disasm`
  linear walker from each record's `1 + N*2 + 4` first-opcode offset, and
  reports every `Yield` site with the inline encounter-record
  (`[reserved×3][count][ids]`) decoded from its trailing window. This is
  the scripted-encounter hunt's faithful discriminator: it surfaces a real
  inline `[count][ids]` arm at a decoded opcode boundary instead of the
  byte-scan false positives (every `0x37`/`0x41` byte in dialog text). The
  town01 survey finds no inline `[1][0x4F]` Tetsu literal, confirming the
  indexed formation-table install path (see `encounter_record`).
- `cutscene` - FMV index ↔ `MV*.STR` filename mapping. The retail
  field-VM `0x4C 0xE2` op writes a 16-bit FMV index to
  `_DAT_8007BA78` and kicks game mode `StrInit` (26); the world
  records it as `pending_fmv_trigger` plus a `FieldEvent::FmvTrigger`
  event. The next `World::tick` consumes the pending trigger and, for a
  playable slot, flips into `SceneMode::Cutscene` (suspending the field
  VM) exposing the FMV via `World::active_fmv()`; the host plays the
  resolved STR and calls `World::finish_cutscene()` to return to the
  field. Cut/missing slots drain as a no-op.

## See also

- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - the
  clean-room boundary and architecture for the engine track.
