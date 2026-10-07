# legaia-engine-core

The simulation half of the from-scratch engine: virtual filesystem, asset
cache, frame timing, world state, and the rules engines. Everything here
is renderer-agnostic.

No `wgpu` / windowing / audio dependencies - the asset crates (Track 1)
talk to this layer, and the render and audio crates read from it. That
constraint is what lets the browser play page share this code with the
native window.

## Contents

- [`Vfs` trait](#vfs-trait)
- [Asset cache](#asset-cache)
- [Frame timing](#frame-timing)
- [Composite `World`](#composite-world)
- [Battle helpers](#battle-helpers) - `art_strike`, `ap_gauge`, `battle_stats`, `items`, `battle_round`, `battle_input`, `battle_hud`, `inventory_use`, `tactical_arts_editor`, `man_field_scripts`, field-resident carrier SM, `cutscene`
- [Scene resources + VRAM](#scene-resources--vram)
- [Dialogue, save/load, and loot](#dialogue-saveload-and-loot)
- [Minigame rules engines](#minigame-rules-engines)
- [Smaller modules worth knowing](#smaller-modules-worth-knowing)
- [Other major modules](#other-major-modules)
- [Module index](#module-index)
- [See also](#see-also)

## What it provides

### `Vfs` trait

Source of asset bytes. Three backends:

- `DirVfs` - filesystem-backed, rooted at a directory. Used in
  development against the output of `legaia-extract`.
- `DiscVfs` - reads directly from a disc image, so end users don't need
  to extract anything ahead of time. This is the backend behind the
  shipped end-user model (`legaia-engine` pointed straight at a `.bin`).
- `MemoryVfs` - in-memory bytes, for tests and the WASM build.

All three yield raw bytes addressed by a logical name (e.g.
`"prot/0123_some_entry.bin"`). The asset crates above this layer turn
bytes into typed structures.

### Asset cache

A bounded in-memory cache keyed by Vfs name. Avoids re-decoding the same
TIM/TMD/VAB on every frame when an actor is referenced repeatedly.

### Frame timing

`frame_step::SimStepper` - the fixed-step driver every host's display loop
drains wall time through: it runs as many whole 1/60 s ticks (one
`World::tick` = one retail vsync) as have elapsed, carries the remainder, and
caps the backlog, so the script VMs advance deterministically regardless of
render rate. `world::FrameClock` (`world.clock`) holds the retail
frame-step factor, the vsync accumulators, the tick / display-frame counters
and play time.

### Composite `World`

`world::World` ties together the actor, move, effect, field, and battle
VMs. One actor table (default capacity 64) is shared across all four
script VMs; the `Host` traits are implemented by routing through this
struct. `World::tick` runs:

1. Effect pool tick.
2. Per-actor move-VM tick - only for active actors with bytecode loaded
   via `set_move_bytecode`.
3. The mode-specific top-level step - `Battle`, `Field` / `Cutscene`,
   `WorldMap`, one of the minigame modes (`Dance`, `Fishing`,
   `SlotMachine`, `BakaFighter`, `MuscleDome`), or `Menu` / `Title` (which
   run none). The first two are detailed below.

Steps 1 and 2 are skipped under `SceneMode::Menu`: retail's CARD mode runs no
master frame driver, so nothing on the actor lists advances while the pause
menu owns the frame (`mode::runs_master_frame_driver`).

Engines that want a different storage layout (ECS, custom parallelism)
implement the per-VM `Host` traits themselves; `World` is the default.

##### Layout

`World` keeps the VM contexts, the actor table and the scene-flow latches
as direct fields; everything else lives in one sub-struct per subsystem,
each a plain data struct in its own `world/*.rs` file. Access is
`world.<group>.<field>` (`world.party.money`, `world.battle.command`):

| Field | Type | Holds |
|---|---|---|
| `party` | `PartyState` | Roster, active party + leader, money, the `ItemBag` (retail's 256-slot array + its active window; map-shaped adapter over it), ability masks, tactical arts, level-up tracking, banners, save extensions, name entry. |
| `battle` | `BattleState` | Per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues, end-of-battle latches. |
| `encounters` / `seru` / `casting` | `EncounterState` / `SeruState` / `CastFxState` | Encounter session + scripted arms; capture log, registry, shiny rolls; summon / cast-module / move-FX scene graph. |
| `terrain` / `locomotion` / `props` / `npcs` | `FieldTerrain` / `FieldLocomotion` / `FieldPropState` / `FieldNpcState` | Walkability + zones + floor LUT; player movement gates and deltas; prop colliders, walk-touch, stagers; NPC positions, routes, motions, dialog bindings. |
| `field_vm` / `dialog` / `cutscene` | `FieldVmState` / `DialogState` / `CutsceneState` | Per-record channels, helper contexts, submode block; dialog panel + inline runner; narration, timeline, caption / card overlays, FMV handoff. |
| `world_map` / `carriers` | `WorldMapState` / `FieldCarrierState` | Overworld controller + entity SMs; field-scene carrier SMs. |
| `minigames` / `shops` / `menu` / `board` | `MinigameState` / `ShopState` / `MenuState` / `TileBoardState` | Minigame sessions + wallet; shop / prize-exchange sessions; pause-menu tables + warp requests; op-0x49 tile board. |
| `camera` / `presentation` / `ambient` | `CameraRig` / `ScreenFxState` / `AmbientFxState` | Camera snapshot + ease + register file; fades, tints, cinematic bars; CLUT cyclers, VDF pulse, VRAM moves. |
| `audio` / `move_vm` / `clock` | `AudioState` / `MoveVmGlobals` / `FrameClock` | BGM + SFX cue slots + battle cue queues; move-VM pools and globals; frame-step factor, vsync accumulators, tick counters. |
| `tables` / `flags` / `toggles` | `DiscTables` / `StoryFlagState` / `WorldToggles` | Disc-parsed static tables; story / system flag words; engine behaviour toggles. |
| `script_actors` / `fog` / `fog_volume` | `FieldScriptActorState` / `FogPool` / `FogVolume` | Scripted arcs, the NPC height channel and attached lights; the fog-particle pool; the volumetric-fog enhancement. |

#### `SceneMode::Battle`

The battle-action state machine step, preceded by the staged-anim commit
(`commit_staged_battle_anims`, the `FUN_8004AD80` ladder). Anim ids the
SM stages into `actor.queued_anim` play on the battle actors: equipment
weapon swings (`0xC..0xF`) directly, ids `>= 0x10` through the
per-character art bank installed via `set_actor_battle_art_bank` (with
the retail `0x10`/`0x1A` → dynamic-slot-`0x11` rewrite).

The clip's finish clears `ADVANCE_DONE` - the attack chain's
strike-pacing gate - and idle resumes. See
`docs/subsystems/battle-action.md#staged-anim-playback-the-attack-band-plays-in-engine`.

`World::enter_battle` seats combatants at the retail stage seats
(`battle_seats` - the SCUS placement tables `0x800775C8` / `0x80077608`
stamped by `FUN_800513F0`; party at negative Z facing the monsters at
positive Z). Those seats are a *starting* formation, not a home the
fight returns to: `world::battle::locomotion` drives the approach only,
and re-takes each living actor's seat pair from where the action left it
at `DoneCleanup`, so combatants stay engaged the way retail's captured
mid-battle states show them. See
`docs/subsystems/battle-action.md#where-an-action-leaves-its-combatants`.

#### `SceneMode::Field` / `SceneMode::Cutscene`

A field-VM step, preceded by `step_cutscene_timeline` when a cutscene
timeline is installed (the `opdeene` opening prologue). That spawns a
*second* `FieldCtx` (`cutscene_timeline::CutsceneTimeline`) to run the
scene MAN's partition-2 cutscene record through the same field VM, so
its camera path and actor moves play and the Rim Elm hand-off
`GFLAG_SET 26` fires by execution rather than by a hard-coded cue.

Alongside the timeline sit the scene's per-actor script channels
(`field_channels::FieldChannel`, one per MAN partition-1 placement, port
of `FUN_8003A1E4`/`FUN_8003AEB0`) - the vignette actors the timeline
halt-acquires and pokes beat by beat. A channel's own script runs only in
the load-frame spawn pre-run (`pre_run_field_channel_prologues`); retail
engages a placement context only on a touch, never on a poke. Animate cues go into
`npcs.anim_cues` (drained by the windowed render to re-target each
NPC's clip player); scripted moves go into `npcs.positions`. The
opening white flash (op `0x34` sub-0) installs the screen-effect colour
tween (`presentation.effect_tween_slot`), drawn from
`World::screen_tint_pushes` as a full-screen wash. See
[`docs/subsystems/cutscene.md`](../../docs/subsystems/cutscene.md).

**Locomotion.** In `Field` the field-VM step is followed by
`step_field_locomotion`, the free-movement player controller (port of
`FUN_801d01b0`). The held d-pad becomes a camera-relative direction
through retail's 45° eighth-turn ring remap (`World::remap_pad_direction`,
`FUN_800467e8`) at an octant derived from `locomotion.camera_azimuth`; the
mask then passes the wall-slide resolver (`World::resolve_field_slide`,
`FUN_80046494` - skids the player along a blocked wall toward the open side),
and the player actor advances in 2-unit steps with per-axis collision
against the per-scene `terrain.collision_grid`, and facing is updated.
The opt-in `World::locomotion.precise_movement` swaps in a continuous
decode - true key diagonals + analog-stick angles - through the *same*
collision, so it changes input feel without forking the physics.

Hosts feed the azimuth from `Camera::compass_azimuth_units()` (scripted
yaw + user drag-orbit + the renderer's framing bias). `Camera::distance`
is the discrete follow-camera distance preset (retail / far / farther) -
render framing only.

See
[`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md).

The collision grid (one byte per 128-unit tile, high nibble = 4 sub-cell
wall bits) is zeroed at field entry and painted by the field-VM `0x4C`
outer-nibble-7 op as the prescript runs.

**NPCs and props.** The same tick walks field NPCs through the motion VM
(`tick_field_npc_motions`: cutscene walk legs, live positions feeding the collision /
interact probes), and runs the prop walk-touch dispatch
(`check_field_walk_touch`: door-warp / player-teleport placements post on
body contact through the interact path).

**Player animation.** After the locomotion step
`field_anim::FieldPlayerAnim` (installed via `set_field_player_anim`)
advances the PROT 0874 §1 locomotion bundle's idle / walk clip pair,
switched on the movement edge and folded into the player actor's
`pose_frame` for the host's posed-mesh rebuild. See
[`docs/subsystems/field-locomotion.md`](../../docs/subsystems/field-locomotion.md).

`World::party.active_party` holds the present-party composition - the engine
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

The `World`-free battle kernels - `art_strike`, `ap_gauge`, `battle_stats`,
`accessory_passives`, `seru_trade`, `battle_sideband`, `tactical_arts_editor`,
`monster_ai`, `battle_steal`, the catalogs, encounters and level-up among
them - live in [`legaia-engine-battle`](../engine-battle/README.md) and are
re-exported here at the same paths; this crate keeps the `World` side that
composes them.

- `art_strike` - translates `ArtStrikeInfo` into an `ArtStrikeOutcome`
  (HP delta, status, scheduled SFX cues) the world drains into its
  battle event queue.
- `arts_command_input` - the retail Arts command entry: per-press
  directional buffer, per-command AP debit from the turn pool, auto-end
  when nothing is affordable, the auto-command-string preseed and its
  bare-confirm replay (no pool charge), and the Begin | Reselect review. Resolves
  the entered sequence through the `legaia-art` matchers. Costs come from
  the equipped set's `+0x74` bytes (`World::battle.swing_costs`).
- `ap_gauge` - per-character Action-Point gauge. Charges +5 on
  Spirit-press, refills per turn; backs the Spirit command and the AP
  override hook, **not** the Arts input's swing budget.
- `battle_stats` - equipment-aware stat aggregator (from-scratch port of
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
  field escape); effect *amounts* are the curated walkthrough values,
  byte-confirmed against the static `SCUS_942.54` heal-amount table
  (`0x8007655C`, `legaia_asset::item_effect`) by the disc-gated
  `item_effect_real` test.
- `shop` / `shop_catalog` - shop session state (buy/sell cursor,
  quantity, gold/inventory delta) plus the disc-sourced **gold-shop
  stock catalog**: `ShopItemData::from_scus` reads per-id buy prices
  (the sellable mask), and `shop_catalog::scene_shops` decodes a
  scene MAN's op-`0x49` stock records (`legaia_asset::shop_stock`) into
  a priced `ShopInventory`. `SceneHost::enter_field_scene` parks them on
  `World::shops.scene_shops`; `World::scene_shop_session(idx)` opens one. The **live
  trigger** is the field VM's op `0x49` sub-0: `World::try_arm_field_shop`
  recognises an inline shop record on the op's bytes and stages it on
  `World::shops.pending_shop` (Armed -> Done op-0x49 gating), so a host drains
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
  id (`legaia_asset::seru_trade::vendor_id_from_shop`, from the shop's name + stock) onto the
  `ShopSession`, so each merchant reseeds independently. Offers come from the
  shared `legaia_asset::seru_trade` kernel, so the engine and the randomizer
  preview always agree.
- `battle_round` - per-round orchestrator. `BattleRound::begin` resets
  AP, recomputes equipment-aware stats, writes attack / UDF / LDF into
  the world. `BattleRound::end` ticks status, drains tick damage,
  returns death count.
- `battle_input` - `BattleCommandSession`: the player-driven command
  picker for the live gameplay loop. A small state machine (command menu
  → target select → confirm) driven a frame at a time from `World::input`.
  Target selection reuses `target_picker`. When
  `World::battle.player_driven` is set, `World::live_battle_tick` opens
  one per party turn and parks the action SM until the player confirms;
  otherwise the loop auto-resolves with a physical Attack. The menu carries
  Attack, Arts, Magic, Item, Spirit and Run (`BattleCommand`). See `docs/subsystems/battle.md#auto-resolve-vs-player-driven`.
- `battle_flow` - the retail command-flow byte `ctx[+0x06]`, the cursor of
  the battle's *menu* SM `FUN_801D0748` (not the action SM's `ctx[+0x07]`,
  whose value space it overlaps). `flow_state_for` recomposes it each frame
  from the live `BattleCommandSession` phase plus whichever host submenu is
  open. It is the key the sparring-tutorial hook table indexes.
- `battle_tutorial` - the Tetsu sparring fight's in-battle prompt machine
  (stage overlay 967), a `(flow state × lesson)` cross-product with a
  wrong-lesson rewind. A queued box parks the whole battle tick. Prompt
  **text is read off the user's disc** (`BattleTutorialScript::from_prot`,
  installed for every host by scene entry) - only the string addresses are
  committed. It arms from the disc, not from a host: `TUTORIAL_ARM_FLAG`
  (`0x19`) is the one-shot system-flag arm `World::enter_battle` tests and
  consumes, raised by town01's Tetsu record two ops before its battle-entry
  op. `World::prime_battle_tutorial` is a debug force, not the port. See
  `docs/subsystems/battle.md#the-sparring-tutorial-prompt-machine-overlay-967`.
- `battle_sideband` - the battle side-band pass `FUN_80056208`, keyed on
  the stage id `_DAT_8007B64A`: the sparring caption's hold (stage 1) and
  the host side of the two Cort stage modules (stages 2 / 3). A pure
  transition kernel; `World::tick_battle_sideband` runs it once per live
  battle frame for both play hosts.
- `battle_stage_module` - the two boss-stage modules the side-band drives
  for formation monster `0xB5`: PROT 0968's arrival (camera walk, the boss
  dropping in, the name banner, the hand-back to round one) and PROT 0969's
  form transition (the 1-HP beat, the shake, the blanked field, the exit to
  the field). See
  `docs/subsystems/battle.md#what-the-two-boss-stage-modules-do-overlays-968--969`.
- `battle_hud` - renderer-agnostic UI model. Holds per-slot HP / MP /
  AP / status icons, a queue of `DamagePopup`s with fade timers, and a
  ringed log column. Engines feed it from `BattleEvent::ApplyArtStrike`
  (popups), `StatusEvent` (icons), and `BattleRound::begin` / `end`
  (slot rows). `engine-render::battle_hud_draws_for` turns it into the
  drawn surface. Also the two labels the drawn surface needs off the
  live world: `battle_active_actor` (the `(slot, name)` pair behind the
  top-left plaque, and the port's whole monster readout - retail draws no
  monster gauge; `None` once every formation slot is cleared, which is what
  stops the plaque drawing over the victory frames),
  `battle_plaque_element_badge` (the 20x12 badge an elemental actor wears
  in front of that name) and `encounter_banner_label` /
  `encounter_banner_enabled` (the banner is a port invention with no retail
  counterpart, so it is gated off).
- `save_menu_atlas` - the shared 256x256 sprite bake. Besides the save /
  pause chrome it carries the **battle badge cells**: the nine 48x16 status
  word tags (`band_status_badge`) and the eight element badges
  (`band_element_badge`). Three status badges decode through the row-511
  CLUT extension at `SYSTEM_UI_CLUT_EXT_TIM_OFFSET`, one TIM before the
  system-UI sheet, so a caller roots its slice there to get all nine; a
  sheet-rooted slice bakes six and the accessors answer `None` for the rest.
- `inventory_use` - `InventoryUseSession` state machine for the field
  + battle inventory flow. Filters items by `InventoryContext`,
  validates target compatibility (Revive vs alive), folds `ItemOutcome`
  through `World::use_item`.
- `tactical_arts_editor` - the field-menu Arts screen: `ChainEditor`
  (Browsing → Editing → Naming → Done) composes a directional chain into
  a per-character `ChainLibrary`. `World::chain_library` /
  `World::store_chain_library` bridge that library to `World.party.saved_chains`,
  so a chain authored in the menu serializes with `save_full` - the same
  path whether it was edited live or loaded from a save
  (`SavedChain::to_record` / `from_record` pack to the `Command` byte
  alphabet the battle side reads). In battle the chain does **not** commit
  an art by itself: retail's Arts command is the per-press
  `arts_command_input` entry. What retail preseeds that entry with is the
  character's **auto command string** (record `+0x1A7` / `+0x1B7`), not a
  named chain: loaded as the entry opens, replayed by a bare confirm without
  charging a press, wiped by the first press - see
  [`arts-command-gauge.md`](../../docs/subsystems/arts-command-gauge.md#the-auto-command-string-preseed-on-open-replay-on-a-bare-confirm).
  `build_battle_arts_rows` still reads `saved_chains` for the legacy
  submenu behind `LEGAIA_ARTS_SAVED_LIST=1`.
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
  partitions (the prescript single-consumer oracle's scanner).
  `scene_entry_ambient_installs` is the narrower one both scene hosts run:
  the subset of those installs retail's placement spawn-prologue slice
  (`FUN_8003A1E4`) executes at scene load, which is what decides whether a
  scene's ambient tree spawns. This is
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

## Scene resources + VRAM

`scene_resources` turns a scene's PROT bundle into the resources a host
needs. `build_targeted` is the runtime VRAM pre-pass: it resolves what
each scene actually uploads, rather than blanket-loading the block.

`FIELD_SHARED_BLOCKS` names the blocks that stay resident across a scene
transition - the player TMD lives here, which is why the party mesh
survives a door without a reload.

`scene_assembly` is the shared full-scene assembly kernel over those
resources: `assemble_field_scene` resolves the env pack, the `.MAP`
placement/terrain draws (object binds included, so each placed draw
carries its posing clip), the walk-ground heightfield and the coplanar
lifts - the one build the browser field-scene page and the native
`export-glb` path both read. `packet_color` carries the hybrid
textured/vertex-colour side channel, `npc_catalog` the MAN placement
walk, and `glb_export` composes all of it (plus per-NPC and animated-prop
bakes through `legaia_asset::{scene,character}_gltf`) into the
`legaia-engine export-glb` artifact set, `--items` included
(`export_equipment_item_glbs` over the shared
`battle_char_assembly::loadout` kernel) - see
[`docs/tooling/vrchat-world-export.md`](../../docs/tooling/vrchat-world-export.md).

## Dialogue, save/load, and loot

`inline_dialogue` is the opt-in dialogue runner: `step_inline_dialogue`
ports the retail dialog state machine `FUN_80039B7C` through the *real*
field VM, so dialogue advances by script execution rather than by a
reimplemented approximation. Hosts that want the simpler path can leave
it off and drive the dialog panel directly.

A pass ends where retail's parks: on the record's backward jump onto a PC
the pass already reached (`InlineDialogue::visited`). That map marks **text
segments as well as opcodes** - retail records commonly loop back onto the
opening line rather than onto an opcode - and a picker commit clears it only
from the branch target forward. Both are load-bearing: with either one
missing, real Rim Elm conversations replay without limit and cannot be
left. Covered disc-gated by `engine-shell/npc_conversation_terminates`.

`World::dialogue_owns_input` is the single "a conversation owns the pad and
the player" predicate - **both** channels, the simplified `current_dialog`
request and the runner. Locomotion, the interaction probe, walk-on
dispatch, the tile board and both hosts' pause-menu open all gate on it;
testing either field alone is the asymmetry it exists to prevent.

`save_full` / `load_full` are the disk save round-trip (LGSF): party,
story flags, money, inventory, per-character ext, saved chains.

`apply_battle_loot` is the post-battle grant kernel - XP, gold, and the
resulting level-ups. It is the runtime side the randomizer's disc-gated
oracles drive to prove a patched drop actually reaches the player.

`World::request_move_fx_spawn` raises the battle move-FX request for
non-summon casts and specials, off the parsed `move_power` table.

## Minigame rules engines

Each is a headless rules engine driven by disc-parsed tables, with the
presentation left to the host. The ones that need no `World` - `dance`,
`dance_tutorial`, `minigame_actor`, `minigame_fx`, `minigame_floor`,
`baka_*`, `slot_machine`, `fishing*`, `other_game_overlay`, `tile_board` -
live in
[`legaia-engine-minigames`](../engine-minigames/README.md) and are
re-exported here at their old paths; this crate keeps their `World` glue
and scene assembly (`dance_venue`, `dance_cast_scene`, `baka_duel_scene`,
`fishing_scene`, `fishing_venue`, `muscle_dome*`):

- `dance` - Noa's dance rhythm minigame, driven by the parsed step chart. It
  also owns the two `minigame_actor` pools the overlay's draw kernels read: the
  floor cast (spawn positions + bound clip ids off the disc's own spawn and
  kind tables) and the sprite parts a scoring judge spawns.
- `dance_venue` - what the dance overlay's entry stages around the run: the
  globals it saves and replaces over the walked-in scene (block base, view
  window, venue camera; restored on the way out) and the `other7` venue itself
  with the entry's face stamps in its VRAM, built once for every host.
- `minigame_actor` - the per-entity record the hub-band overlays spawn through
  the shared part-spawn API and read every frame, named by retail byte offset.
  Not the field actor; see the module docs for why they stay apart.
- `minigame_fx` - the **effect-part pool** the minigame overlays' one-shot
  presentation spawns land in (the fishing venue's splash, ripples and catch
  bursts), on `World::minigames.fx` and aged by `World::tick` so every host
  drains the same parts. The dance run keeps its own pool, because its spawns
  come from the judge rather than from a host.
- `baka_fighter` - the Baka Fighter duel, driven by the parsed roster +
  action tables.
- `muscle_dome` - the Muscle Dome, in two layers because retail has two state
  machines. `MuscleDomeSession` is one **leg**: an ordinary battle fought to a
  KO, direction-command ids + swing-record AP costs, a budget-gated queue
  commit, and `DomeDamageModel` - the one retail damage kernel both hosts
  resolve turns through. `DomeContest` is the **ladder above it**: the
  `(course, round)` cursor packed in the mode-24 sub-id word, story-flag
  course selection and Master-course length gating, per-leg scoring, the
  between-leg HP restore, and settlement into casino coins. A leg pays
  nothing; a contest pays. Driven by `World::report_muscle_leg` /
  `World::settle_muscle_contest`.
- `muscle_dome_scene` - the dome's 3D arena surface, `MuscleDomeSurface`:
  the arena shell, the ground grid, the lead's assembled battle form and the
  ladder's monster, posed off the session's turn edge and framed by
  `DomeCamera`. Both play hosts drive it once a frame, as
  `baka_duel_scene::BakaDuelSurface` serves the duel.

## Smaller modules worth knowing

- `music_labels` - resolves a global BGM id / `music_01` bank slot to its
  curated sound-test track label. See
  [`docs/reference/music-tracks.md`](../../docs/reference/music-tracks.md).
- `world::ambient` - the scene-entry ambient move-VM effect tree
  (`spawn_ambient_record` fan-out, `step_ambient_fx` drain) and its two
  render-tail arms: the CLUT-cell HSV cycler (`clut_cell_fx`, mode 3) and
  the cyclic VRAM-rect scroller (`world::ambient::vram_scroll`, mode 4 -
  the waterfalls). See
  [`docs/subsystems/field-ambient-fx.md`](../../docs/subsystems/field-ambient-fx.md).
- `battle_seats` - the retail stage-seat tables, consumed by
  `World::enter_battle`.
- `fishing::PrizeExchange` + `World::fishing_exchange_buy` - the
  point-exchange prize shop over the persistent fishing-points pool and
  its one-time bitmask.
- `region_encounter::EncounterRateModifiers` - statically pinned
  accessory / status encounter-rate shifts, refreshed per step.
- `live_loop` - the single kernel both hosts arm the gameplay loop
  through (`World::arm_live_loop`): scene label, encounter fallback,
  loop / player-battle flags, battle BGM. Also
  `World::scene_can_roll_encounters`, the answer to "can this scene
  produce a random encounter at all" that lets a host say so instead of
  looking broken in a town. See
  [`docs/subsystems/battle.md`](../../docs/subsystems/battle.md).
- `options` - the engine mirror of the retail options screen, plus the
  engine-only opt-in toggles (e.g. `precise_movement`, default off).
- `dev_menu` - the debug-build dev-menu (overlay 0897) EVENT FLAG editor
  value/list kernels (`edit_flag_value` / `flag_list_prev` /
  `flag_list_next`, `FUN_801dbd04` / `FUN_801db8f4` / `FUN_801db8b4`).
  Logic only; row render stays an `engine-ui` seam. See
  [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md#options-screen).
- `pause_screens` - the retail Items / Magic pause-screen sessions + view
  models (`PauseItemsSession` command/list/throw-out focus over the
  item-use flow - incl. the Throw Out Yes/No confirm and the Arrange
  bag sort - `MenuTextTables` disc text: item + spell
  names/descriptions, accessory passive lines). Feeds the `engine-ui`
  `items_screen_draws_for` / `magic_screen_draws_for` builders in both
  hosts. See
  [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md#items-screen).
- `menu_arrange` - the Items screen's Arrange rank table (menu-overlay
  `0x801E4A88`) + the retail bag-sort kernel (`FUN_801D64A8`).
- `menu_widget` - the window-widget choreography: `MenuWidgetScripts`
  resolves the window-script VM's programs out of the menu-overlay image
  (`legaia_asset::widget_script`), and `MenuWidgetState` is the
  `legaia_engine_vm::Host` window-list model the shop open / Sell
  slide-away programs run against (`MenuRuntime::tick` drives the edges,
  mirroring `FUN_801DAFD4`). See
  [`docs/formats/window-script.md`](../../docs/formats/window-script.md).
- `mode_entry_init` - the one-time **mode-entry initialisers**: the field /
  town scene init (`FUN_801D6704` - step order, BGM slot resolve, primitive
  buffer sizing, and the cold/warp player seat `SceneHost::enter_field_scene`
  applies) plus the duel-arena overlay seeds (`FUN_801CF00C`). See
  [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md#asset-descriptor-walker-fun_80020224---the-slotasset-mapping).
- `effect_ribbon` - the battle effect-**ribbon** geometry generator
  (`FUN_801CFA48`): a seeded random walk emitting six vertices per step with a
  tapering radius, plus the packet-chain layout its render half consumes.
  Geometry only; the GPU emit is render-track.
- `action_effect_script` - the battle action's 8-byte **effect-script** record
  walk (`FUN_801DEA50`): frame gates, facing rotation via `RetailRotationLut`
  (the `0x80070A2C` trunc-sine pair), the move-power record index, and the
  target band the terminator's homing seed sweeps. Driven per battle frame by
  `World::tick_battle_animations` over each actor's committed clip
  (`MonsterAnimation::effect_script`); spawns drain via
  `World::drain_battle_effect_spawns`.
- `field_regions::window_rebuild_spawns_resident` - the sub-area **window
  rebuild** placed-object sweep (`FUN_801D7B50`), complement of the
  scene-init sweep. `World::recentre_field_window` (`world/static_window.rs`)
  runs it on every camera re-centre retail runs it on and keeps the result as
  `FieldTerrain::static_window`; both play hosts gate the sweep's placements on
  that list through `field_env::placed_draw_live` (whole map by default,
  retail windowing behind the `retail_static_window` option).
- `target_picker::enemy_menu_rows` + `layout_enemy_menu_rows` - the enemy
  target-menu row dedup / labelling and the overlap-relaxation layout
  (`FUN_801D9D3C`), reached each battle frame through
  `battle_hud::battle_intro_names` on both hosts.
- `field_submode` - the field overlay's op-`0x49` **sub-screen** entry family:
  context reset + driver-actor spawn (`FUN_801D9C3C`), the smaller
  fixed-template spawn (`FUN_801DE478`), the list-panel row layout
  (`FUN_801E6984`), and the CARD mode-request leaf (`FUN_801D84B4`).
- `field_actor_kernels` - the scene-transition **teardown sweep**
  (`FUN_801D7518`, which the field initialiser runs once per actor list on a
  warp entry) and the per-actor **colour tween** (`FUN_801DDC20`) whose actors
  the sweep retires by handler address.
- `camera_view` - the layer above `camera`: which camera owns this frame
  (`resolve_field_camera` - retail follow / op-`0x45` cutscene shot /
  overworld walk / world-map top-view debug) and what its retail GTE inputs
  are, plus the matrix each host uploads (`frame_vp`) and the world-space lens
  the occlusion gate ray-casts from (`frame_eye`). The pinned follow constants
  live here, so a host never spells an angle, a depth or a focal length out
  again; the projection itself is `legaia_engine_vm::psx_camera`. See
  [`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md#gaps-the-tiers-were-blind-to-closed-by-reading-the-two-hosts-side-by-side).
- `camera_zone` - the **zone-driven follow camera**: the camera parameter
  block retail keeps at `0x8007B606..` (`CameraZoneConfig`, loaded from a MAN
  section-3 camera-region record by the port of `FUN_801DBC20`), the composer
  that turns it plus the player's position into a target pose (`FUN_801DAB90`:
  position-proportional sweeps, look-at anchors, fixed shots, the floor-height
  pitch coupling) and the per-frame ease / snap that walk the ten camera
  globals toward it (`FUN_801DB510` / `FUN_801DB8EC`), with the bearing and
  square-root LUT helpers they lean on. `Camera::zone` runs it; the pinned
  constants in `camera_view` are only the terrain-less fallback. Format +
  arithmetic: [`docs/formats/encounter.md`](../../docs/formats/encounter.md#man-section-3-the-camera-region-table).
- `camera_ease` - the field camera's smoothed **vertical-offset** step
  (`FUN_801DA390`; `player[+0x16]` is the middle slot of the `+0x14/+0x16/+0x18`
  position triple, and the eased result lands in the Y halfword of a vector):
  settled creeps by 1, unsettled takes a gap-proportional step capped at 12.
- `world_map::WorldMapController` - drives `SceneMode::WorldMap`.
- `world_map_panel_host` - the world-map band's panel screen: the
  `0x801F2B98` window system plus the six `ctx[+0x54]` panel actors and the
  travel arts, hosted on `WorldMapController::panels`. See
  [`docs/subsystems/world-map.md`](../../docs/subsystems/world-map.md#the-panel-actor-state-machines).
- `anim_cue` - `walk_anim_cues` / `AnimCueState`, the per-frame walker
  over a playing battle action's 8-slot `(frame, cue)` track
  (`FUN_800508DC`): swing, hit, footstep and knockdown SFX, the party
  `0xC8..=0xFF` band resolved into the arts-voice namespace, and the
  CD-busy fallback ring cue. It emits `AnimCueEmit` decisions; the battle
  actor tick in `world::actors` drains them into the SFX ring.
- `input::Mapping`, `scene::DefaultMapIdResolver` - host-agnostic input
  binding and scene-name → map-id resolution. (Effect lookup,
  `EffectCatalog`, is `legaia_engine_vm::effect_vm`'s; the scene host loads
  it from `efect.dat`.)

## Other major modules

- `mode` - the game-mode driver and retail mode table (`ModeSeat`, which both
  hosts enter INIT modes through).
- `scene` - the scene-loading shell: PROT asset indexing, per-CDNAME-block
  bundle resolution, BGM lookup, and `SceneHost` (`enter_field_scene`).
- `model_bank` - the retail model pool `DAT_8007C018` and the id space a
  placement's model byte and the scripted-motion VM's op `0x0E` share.
- `levelup` / `inn` / `equip_session` / `spells` - the post-battle
  `LevelUpTracker`, the inn rest session, the equipment session, and the
  spell catalog + cast resolver.
- `field_menu` / `field_menu_dispatch` / `menu_runtime` and the `menu_*`
  family - pause-menu sessions, sub-session dispatch, and list / input /
  validator leaves.
- `title` / `save_select` / `save_subscreen` / `card_flow` / `card_write` /
  `card_bu_io` - title state machine, save-slot select, and the memory-card
  I/O and write flow.
- `dialog_window` - the field dialog pager's row window, scroll and typing
  reveal.
- `screen_fx` - the PROT-0900 screen-effect widget family (iris mask,
  sprites, panels, letterbox) the field VM drives.
- `fishing` / `fishing_actors` / `fishing_hub` / `fishing_venue` - the
  fishing minigame's `PondSession`, its actors, the venue hub screen and
  venue-actor step.
- `slot_machine` / `baka_cabinet` / `baka_duel_scene` - the casino slot
  machine rules engine, and the Baka Fighter cabinet shell and 3D duel scene.
- `tile_board` / `timed_fight` / `incense_notice` - the op-`0x49` tile board,
  the turn-limited boss fight's gate, and the Incense wear-off notice.
- `encounter` / `encounter_man` / `monster_ai` / `monster_catalog` - per-scene
  random-encounter tables and trigger, and the per-monster battle AI
  (`FUN_801E9FD4`).
- `new_game` - seeds live party records from the `SCUS_942.54` starting-party
  template.
- `cd_dma` / `overlay_loader` / `stream_file` - the CD streaming host traits
  and overlay / stream loaders.

## Module index

The modules the sections above do not walk, grouped by family. The module
docs carry the retail provenance.

- **Actor model + hosts** - `actor_handler` (the actor's `+0x0C` per-frame
  handler identity), `actor_alloc_host` / `move_buffer_host` (the `World`
  impls of `engine-vm`'s allocator and MOVE-buffer host traits),
  `part_motion` (a move-VM part's motion block between steps),
  `float_tween` (the `gp+0x148` screen-position tween), `morph_weight_apply`,
  `camera_rel_glide`, `object_effect` (the `0x80083FF8` object-effect table).
- **Battle** - `battle_arts` / `battle_magic` (the Arts and Magic
  submenus), `battle_open` (the formation open banner), `battle_party_form`
  (a member's assembled battle form, once for both hosts),
  `battle_cam_inputs`, `battle_sideband_textures` (`readef.DAT` pages),
  `sfx_cue` (cue id → ring / XA clip), `spell_party_broadcast`
  (`FUN_8003053C`). The kernel modules re-exported from `engine-battle` are
  mapped in [its README](../engine-battle/README.md).
- **Field** - `field_events` (the field VM's event queue), `field_ground`
  (walk-ground heightfield as a render surface), `field_lit_mesh` (the
  light-source TMD rows' shading), `field_view_window` (the visible-tile
  crop), `field_occlusion` (the occlusion fade's visibility gate),
  `coplanar_draws`, `drop_shadow`, `fog_particles` (retail `fog_set`),
  `fog_volume` (volumetric fog, an enhancement), `clut_fx` /
  `clut_walk_anim` (scripted CLUT cells, the CLUT-walk shimmer), `vdf_pulse`
  (enhancement), `register_ramp` (op `0x43` camera zone ramp), `walk_regen`,
  `text_balloon` (`4C E1`), `place_name_banner`, `field_audio_release`,
  `field_actor_clone` (`4C 14`), `field_actor_program` (the voice-over
  scene actor), `field_submode_screen` / `field_submode_code_lock` /
  `field_submode_flag_window` (op-`0x49` submode host and two handlers),
  `field_save_screen_actor`, `scene_transition_actor` (the transition
  streaming actor), `cutscene_script_elements`.
- **Cutscene + movie** - `cutscene_narration` (subtitle roller),
  `cutscene_caption`, `movie_audio` (what a movie does to the score),
  `mdec_dma_sync`.
- **Overworld** - `overworld_curvature`, `overworld_draw_order`,
  `overworld_ground_cue`, `world_map_markers` (not retail: marker quads).
- **Menus + screens** - `menu_cues`, `menu_glyph_atlas`, `menu_input`
  (`FUN_801d688c`), `menu_item_category`, `menu_list_rows`,
  `menu_open_sequence`, `menu_validator`, `status_screen`, `spell_menu`,
  `key_rebind`, `list_order`, `name_entry`, `save_screen` (the save
  screen's host half), `debug_char_editor`, `dev_menu_host`,
  `dialog_pacing` / `dialog_picker_slide` (typewriter reveal, picker slide),
  `title_screen_atlas`, `publisher_logos`, `game_over` (party wipe → title),
  `prize_exchange` (casino sub-screen `0x20`).
- **Minigame support** - `minigame_entry` (the mode-24 door-warp id space),
  `minigame_floor`, `minigame_status` (the engine's affordance rows),
  `dance_cast_scene`, `dance_tutorial`, `fishing_scene`, `fishing_chrome`,
  `fishing_exchange_input`, `baka_fighter_chrome`, `baka_impact_fx`,
  `muscle_ringside`, `other_game_overlay` (PROT 0977 kernels),
  `casino_coin_bank` (op `4C E5`), `effect_default_arm` /
  `effect_sprite_arm` (render-mode-4 emitters as per-frame meshes).
- **Scene loading + session** - `scene_assets`, `scene_bundle`,
  `scene_live` (a headless live scene preview for viewers), `scene_name_sync`,
  `resume` (where a load resumes and a New Game starts), `chunk_install`
  (sound-stream chunk routing), `encounter_registry`, `sound_state`,
  `fade_ramp`, `retail_pad` (`FUN_8001822c`), `scus_leaf_kernels`.
- **Cheats + capture pins** - `cheats` (the play hosts' cheat mutations),
  `cheat_applier` + `ram_map` (parsed GameShark codes → engine cells),
  `capture_observations` (codified save-state findings).

## See also

- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - the
  port boundary and architecture for the engine track.
