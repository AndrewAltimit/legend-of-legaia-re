# legaia-engine-core

Engine-agnostic primitives for the clean-room engine reimplementation:
virtual filesystem, asset cache, frame timing.

No `wgpu` / windowing / audio dependencies - the asset crates (Track 1)
talk to this layer, and the render and audio crates read from it.

## Contents

- [`Vfs` trait](#vfs-trait)
- [Asset cache](#asset-cache)
- [Frame timing](#frame-timing)
- [Composite `World`](#composite-world)
- [Battle helpers](#battle-helpers) - `art_strike`, `ap_gauge`, `battle_stats`, `items`, `battle_round`, `battle_runner`, `battle_session`, `battle_input`, `battle_hud`, `inventory_use`, `tactical_arts_editor`, `man_field_scripts`, field-resident carrier SM, `cutscene`
- [See also](#see-also)

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
   - `SceneMode::Battle` → battle-action state machine step, preceded by
     the staged-anim commit (`commit_staged_battle_anims`, the
     `FUN_8004AD80` ladder): anim ids the SM stages into
     `actor.queued_anim` play on the battle actors - equipment weapon
     swings (`0xC..0xF`) directly, ids `>= 0x10` through the per-character
     art bank installed via `set_actor_battle_art_bank` (with the retail
     `0x10`/`0x1A` → dynamic-slot-`0x11` rewrite). The clip's finish
     clears `ADVANCE_DONE` (the attack chain's strike-pacing gate) and
     idle resumes. See
     `docs/subsystems/battle-action.md#staged-anim-playback-the-attack-band-plays-in-engine`.
     `World::enter_battle` seats combatants at the retail stage seats
     (`battle_seats` - the SCUS placement tables `0x800775C8` /
     `0x80077608` stamped by `FUN_800513F0`; party at negative Z facing
     the monsters at positive Z).
   - `SceneMode::Field` / `SceneMode::Cutscene` → field-VM step, preceded by
     `step_cutscene_timeline` when a cutscene timeline is installed (the
     `opdeene` opening prologue): a *second* spawned `FieldCtx`
     (`cutscene_timeline::CutsceneTimeline`) runs the scene MAN's partition-2
     cutscene record through the same field VM, so its camera path + actor
     moves play and the Rim Elm hand-off `GFLAG_SET 26` fires by execution.
     Alongside the timeline, `step_field_channels` runs the scene's per-actor
     script channels (`field_channels::FieldChannel`, one per MAN partition-1
     placement, port of `FUN_8003A1E4`/`FUN_8003AEB0`): the vignette actors the
     timeline halt-acquires and pokes beat by beat (animate cues into
     `field_npc_anim_cues` - drained by the windowed render to re-target
     each NPC's clip player - scripted moves into `field_npc_positions`). The
     opening white flash (op `0x34` sub-0) drives `fade::ColorFade` on
     `World::color_fade`, drawn as a full-screen wash.
     See [`docs/subsystems/cutscene.md`](../../docs/subsystems/cutscene.md).
     In `Field` the field-VM step is followed by `step_field_locomotion` - the free-movement
     player controller (port of `FUN_801d01b0`): the held d-pad becomes a
     camera-relative direction (remapped by `field_camera_azimuth`), the
     player actor advances in 2-unit steps with per-axis collision against
     the per-scene `field_collision_grid`, and facing is updated. The grid
     (one byte per 128-unit tile, high nibble = 4 sub-cell wall bits) is
     zeroed at field entry and painted by the field-VM `0x4C` outer-nibble-7
     op as the prescript runs. The same tick walks field NPCs through the
     motion VM (`tick_field_npc_motions`: MAN-authored `0x4C 0x51` patrol
     routes + interaction-prologue runs, live positions feeding the
     collision / interact probes) and runs the prop walk-touch dispatch
     (`check_field_walk_touch`: door-warp / player-teleport placements post
     on body contact through the interact path). After the locomotion step
     the player's field animation advances (`field_anim::FieldPlayerAnim`,
     installed via `set_field_player_anim`): the PROT 0874 §1 locomotion
     bundle's idle / walk clip pair, switched on the movement edge and
     folded into the player actor's `pose_frame` for the host's posed-mesh
     rebuild. See
     [`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md).
   - `SceneMode::Title` → no further VM.

Engines that want a different storage layout (ECS, custom parallelism)
implement the per-VM `Host` traits themselves; `World` is the default.

`World::active_party` holds the present-party composition - the engine
mirror of retail's present-party list at `0x8007BD10`: `active_party[i]`
is the **roster slot** occupying battle ordinal `i`, so battle actor
slot / HUD row / VRAM texture band all key on the ordinal while the
character content (player battle file `863 + roster_slot`, equipment,
spell list, arts chains, XP / capture recipients) keys on the roster
slot - the live-verified retail banding rule (band = ordinal, file =
862 + char_id). Empty = the identity Vahn/Noa/Gala default. Install via
`set_active_party` (caps at the 3 on-screen positions, reseeds the actor
HP/MP/SPD mirrors), resolve via `party_roster_slot`; persisted through
`SaveExtV2::active_party` by `save_full` / `load_full`.

### Battle helpers

- `art_strike` - translates `ArtStrikeInfo` into an `ArtStrikeOutcome`
  (HP delta, status, scheduled SFX cues) the world drains into its
  battle event queue.
- `ap_gauge` - per-character Action-Point gauge driving Tactical Arts
  command input. Charges +5 on Spirit-press, refills per turn.
- `battle_stats` - equipment-aware stat aggregator (clean-room port of
  `FUN_80042558`). Sums per-item modifiers, ORs ability bits, folds
  status-effect modifiers (Toxic -ATK/-DEF, Confuse halves accuracy,
  Numb / Sleep / Stone / Faint zero evasion, Curse / Faint block Magic).
  `compute_battle_stats_with_passives` adds the accessory passive arms:
  ability-bit derivation + percent-of-base stat boosts + the retail clamp
  block.
- `accessory_passives` - accessory ("Goods") passive-effect catalog
  (item id → 64-slot passive index + party-wide scope, decoded from
  `SCUS_942.54` via `legaia_asset::accessory_passive`). Feeds
  `World::refresh_party_ability_bits` (per-member `+0xF4` bitfield rebuild +
  the `DAT_80074358` global-mask mirror, bit-tested by
  `World::party_has_ability`), so an equipped MP-saver reaches the MP-cost
  consumers and a Gold Boost reaches the battle-end reward path.
- `items` - typed inventory item-effect catalog, keyed by **real**
  retail item ids (the `SCUS_942.54` item table - e.g. Healing Leaf is
  `0x77`), so a live granted / shop / dropped id resolves to its effect.
  `apply_effect` resolves an `ItemEffect` against a `TargetSnapshot` to
  produce an `ItemOutcome` engines fold into world state. `vanilla()`
  models the faithful consumable subset (HP/MP restore, cure, revive,
  field escape); effect *amounts* are the curated walkthrough values
  (the on-disc effect-value table is not yet pinned).
- `shop` / `shop_catalog` - shop session state (buy/sell cursor,
  quantity, gold/inventory delta) plus the disc-sourced **gold-shop
  stock catalog**: `ShopItemData::from_scus` reads per-id buy prices
  (the sellable mask), and `shop_catalog::scene_shops` decodes a
  scene MAN's op-`0x49` stock records (`legaia_asset::shop_stock`) into
  a priced `ShopInventory`. `SceneHost::enter_field_scene` parks them on
  `World::scene_shops`; `World::scene_shop_session(idx)` opens one. The **live
  trigger** is the field VM's op `0x49` sub-0: `World::try_arm_field_shop`
  recognises an inline shop record on the op's bytes and stages it on
  `World::pending_field_shop` (Armed -> Done op-0x49 gating), so a host drains
  `take_pending_field_shop` -> drives the buy UI -> `finish_field_shop`.
- `seru_trade` - the engine side of the randomizer's `--seru-trade` toggle:
  vendors offer to swap one of a character's seru for a different one, reseeding
  every two in-game hours. `World::install_seru_trade_config` reads the disc blob
  at boot; `World::open_seru_trade` builds a `SeruTradeSession` (offer list +
  cursor + yes/no confirm) for the current party + `play_time_seconds`, and
  `World::apply_seru_trade` rewrites the chosen owner's spell list. Trading is a
  **real row in the shop menu**: an op-`0x49` merchant opens a Buy / Sell /
  Trade / Exit picker (`MenuState::ShopMenu` → `ShopTrade` → `ShopTradeConfirm`,
  driven by `menu_runtime`; the dynamic Trade row resolves via the menu-VM's
  `commit_route_override` hook). `try_arm_field_shop` stamps a stable per-vendor
  id (`seru_trade::vendor_id_from_shop`, from the shop's name + stock) onto the
  `ShopSession`, so each merchant reseeds independently. Offers come from the
  shared `legaia_asset::seru_trade` kernel, so the engine and the randomizer
  preview always agree.
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
  (`[reserved×3][count][ids]`) decoded from its trailing window.
  `scene_bgm_starts` censuses the op-`0x35` sub-1 BGM starts (the global
  `2000+i` ids behind `music_labels`), and `scene_stager_installs`
  censuses the op-`0x34` sub-3 move-VM stager installs across all three
  partitions (the prescript single-consumer oracle's scanner). This is
  the scripted-encounter hunt's faithful discriminator: it surfaces a real
  inline `[count][ids]` arm at a decoded opcode boundary instead of the
  byte-scan false positives (every `0x37`/`0x41` byte in dialog text). The
  town01 survey finds no inline `[1][0x4F]` Tetsu literal, confirming the
  indexed formation-table install path (see `encounter_record`).
- **Field-resident carrier SM.** `World` ticks the ported `FUN_801DA51C`
  entity SM (`legaia_engine_vm::world_map`) in `SceneMode::Field` as well as
  on the overworld. `install_field_carriers([FieldCarrierConfig])` places the
  scene's carriers; a `ScriptedEncounter { formation_id }` sits Idle (towns
  run a 0% random rate, so its host gate disables self-firing) until
  `engage_field_carrier(idx)` - the dialogue-accept stand-in - advances it
  Idle → Activating. The next `tick_field_carriers` runs the state-1 formation
  copy + the `case 2/3` fall-through battle handoff, resolving the carrier's
  MAN formation by index and flipping Field → Battle (returning to the field
  on victory). The Rim Elm Tetsu fight is `formation_id`
  `RIM_ELM_TRAINING_FORMATION_ID` (4); the carrier identity within the MAN
  actor-placement partition and the bytecode that advances its state remain
  open RE threads.
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
