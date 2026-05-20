# Battle subsystem

The battle overlay (`0898_xxx_dat`) carries the battle scene loader, the per-actor state machine, and the effect VM cluster. Loaded at RAM `0x801CE818` (same load slot as the town overlay; battle and town never coexist).

## Battle scene loader (`FUN_800520F0`)

11-case state machine. Notable cases:

- **Case 6** - loads the `befect_data` bundle (PROT 0x369–0x36B).
- **Case 0xE** - initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Also fires for the field-VM op `0x3E` warp/interact path on the system context.
- **Case 0xFF** - dispatches the side-band streaming-effect handler `0x801F17F8` for `summon.dat` / `readef.dat`.

The asset-viewer's `--bundle battle` mode mirrors this loader's PROT 865–890 set so character meshes have the right CLUT bindings.

The `asset-viewer battle-scene` subcommand drives the engine-side composite end-to-end: loads the same battle bundle TMDs, builds an `engine-core::World` in `SceneMode::Battle`, spawns 3 party + 5 monster actor slots, and ticks the [battle-action state machine](battle-action.md) per frame. HUD shows the current `ActionState` (decoded into the named variant), queued action, per-slot liveness, transition counts, and any `BattleEndCause` the SM emits. Triangle cycles `queued_action`; Cross re-seeds at `ActionState::Begin`.

## Battle action state machine (`FUN_801E295C`)

16 KB / 4099 instructions / 155 outgoing calls. The action-execution dispatcher: it takes the player's selected action and runs it to completion across multiple frames.

`_DAT_8007BD24` is a **pointer** to the active battle context struct (typed `int*` in the decompile output). The pointer itself is resolved at battle entry; `*_DAT_8007BD24` = `0x800EB654` for the captured battle. The action state machine accesses fields as `(*_DAT_8007BD24)[N]` - i.e. byte N of the pointed-to struct.

The outer dispatch is `switch((*_DAT_8007BD24)[7])` - byte +0x07 of the ctx struct, which holds the **active action ID** for the currently-resolving party action slot. (Byte +0x06 holds the parallel ID for the monster action slot; only one is non-`0xFF` at a time.) The inner dispatch is `switch(actor[+0x1DE])` - the per-actor **action sub-state** (windup → execute → recover-style staging within each action).

Action IDs surfaced from save-state captures:

| ID | Action |
|---|---|
| `0x20` | Special move / capture (different sub-states) |
| `0x28` | Action-menu cursor active (player still selecting) |
| `0x35` | Magic - summon |
| `0x47` | Spirit |
| `0x50` | Martial-arts directional input mode |

The function reads battle actor pointers via `(&DAT_801C9370)[ctx[0x13]]` (resolves the active actor via `ctx[0x13]` = actor slot index, then indexes the 8-slot pointer table). It guards on `_DAT_800846C0 != 2` (game-state check). The global pointer `_DAT_8007BD24` plays the same role as the field-VM context pointer - this is a state machine, not a bytecode VM, but it shares the field VM's "context-pointer-as-VM-state" idiom.

Distinct from:
- The [field/event script VM](script-vm.md) (which doesn't run in battle).
- The [effect VM cluster](effect-vm.md) (which handles per-effect spawn/render but doesn't drive actor decisions).
- The [move-table VM](move-vm.md) (which drives Tactical Arts inputs and per-action keyframe scheduling - a layer below this one).

Found via the `overlay_battle_action.bin` import (a save state captured with the action menu open). Dumped as `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt`. The 78-function inventory of the battle overlay is in `overlay_battle_action_inventory.txt` (top 80 dumped). All 6 captured battle modes (summon / special-move / martial-arts-input / spirit / action / capture) load identical battle overlay code - only data buffers (actor table at `0x801C9370`, ctx struct at `0x800EB654`, GPU OT lists, audio scratch) differ between captures.

## Battle context struct

The active battle context lives at `0x800EB654` (resolved at battle entry; the global pointer at `0x8007BD24` is set to this address). 32-byte fixed prefix followed by a per-battle dialog/text buffer.

| Offset | Type | Use |
|---|---|---|
| `+0x00` | u8 × 6 | Battle phase/state flags (mostly `01 01 01 00 00 00` while a turn is resolving). |
| `+0x06` | u8 | Monster-slot active action ID (or `0xFF` if no monster action queued). |
| `+0x07` | u8 | Party-slot active action ID (or `0xFF`). The outer `switch((*_DAT_8007BD24)[7])` in `FUN_801E295C` keys on this. |
| `+0x09` | u8 | Turn / phase counter. |
| `+0x13` | u8 | Active-actor slot index - used to look up the actor pointer via `(&DAT_801C9370)[ctx[0x13]]`. |
| `+0x14..+0x17` | u8 × 4 | Per-action parameter bytes (target slot, sub-action, etc. - varies by action ID at +0x07). |
| `+0x18..+0x1B` | u8 × 4 | More action params (dir/elem byte at +0x18, second target at +0x1A, etc.). |
| `+0x1D` | u8 | Action context flag - `0x03` for summon and capture; `0x00` otherwise. |
| `+0x29..+0x2D` | string | Active spell/move icon glyph (`0xCE 0x14 0x20 'G' 'i' 'm' 'a' 'r' 'd' …`). |
| `+0xA9..+0xEC` | text | Battle dialog buffer (`"Vahn won the battle!|Gained …Experience and …G."`). |
| `+0x6D6..` | u8 × N | The action state machine's "PC offset" / sub-state cursor (read by `*(byte*)(ctx + 0x6D6)`). |

Only the leading 32 bytes vary between captures. Beyond `+0x40` the buffer is a long text-rendering scratch area populated when battle messages are printed. Engine port models this as a 1-of-N enum for the action-ID byte, with side-data fields populated per-action.

| Slot | Role |
|---|---|
| `0..2` | Active party members (ordered by formation). |
| `3..7` | Monster slots (up to 5 enemies per battle). |

Combatant struct fields surfaced by helpers analysed so far:

| Offset | Type | Use |
|---|---|---|
| `+0x07` | u8 | Per-actor state byte. Drives `FUN_801E295C`. |
| `+0x13` | u8 | Active-character index (read from `_DAT_8007BD24+0x13`). |
| `+0x1F` | u8 | Hit-radius / size byte. Used by `FUN_8004E2F0` (range). |
| `+0x34` / `+0x38` | i16 | Current world X / Z. |
| `+0x3C` / `+0x40` | i16 | Previous-frame X / Z (for delta tracking). |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | int* | Magic-slot list pointer (each entry is `[byte type, …]`). |
| `+0x14C..+0x152` / `+0x172..+0x174` / `+0x150..+0x158` | u16 | HP / MP / current / max - three-way mirror layout. |
| `+0x1BC..+0x1BE` | u8 | "Show damage" overlay byte triplet. |
| `+0x1DF` | u8 | Monster size byte (read from a monster record at `+0x1F` and stored here at init). |
| `+0x1EF..+0x1F3` | u8 | Magic-resistance per element (5 elements). |
| `+0x230` | u32 | Monster XP / drop record (set from `param_1[1]` in the monster init). |

## Range / line-of-sight (`FUN_8004E2F0`)

`FUN_8004E2F0(actor_a_id, actor_b_id) -> i16 distance` is the canonical battle range check, called 5+ times from the per-actor state machine. Reads `[DAT_801C9370 + id*4]` for both actors, computes a euclidean distance from `+0x34/+0x38` (or `+0x3C/+0x40` for the b-actor), then sums the two `+0x1F` size bytes (party-member size table at `0x80078878`, monster size byte read from the live actor) to get the hit radius. Final value is clamped to a per-actor cap and `0xF` per `param_2 < 3` party tier.

## Monster init (`FUN_80054CB0`)

Called from `FUN_800542C8` (secondary battle archive loader). Populates a battle-actor at `[DAT_801C9370 + (slot+3)*4]` from a monster record:

- HP / MP / SP triplets at `+0x14C..0x158` and `+0x172..0x174`.
- Magic-resistance bytes at `+0x1EF..+0x1F3` (5 elements; one nibble per element).
- Walks the spell list at `+0x4C` (count at `+0x4A`) and indexes into a per-element resistance table.
- Final XP / drop record into `+0x230`.

This is the canonical "monster spawn" path. Engine port reads the record once, populates the actor struct, and lets `FUN_801E295C` take over.

### Monster-record source layout

`param_1` is the in-RAM monster record (after the loader's offset→pointer fixups). Field map traced from `FUN_80054CB0`:

| Offset | Type | Use |
|---|---|---|
| `+0x00` | u32 | Name string pointer (disc offset → pointer; `strlen` copied into actor `+0x1BC`). |
| `+0x04` | u32 | XP / drop record pointer → actor `+0x230`. |
| `+0x08` | u32 | Record byte size (allocation footprint). |
| `+0x0C` | u16 | **HP** → actor `+0x14C/+0x14E/+0x172`. |
| `+0x0E` | u16 | **SP** → actor `+0x154/+0x156` (spirit/action gauge - AI spell-selection budget; spirit-charge source). |
| `+0x10` | u16 | **MP** → actor `+0x150/+0x152/+0x174`. |
| `+0x12` | u16 | **ATK** → actor `+0x158/+0x15A` (attacker offense in the damage routine). |
| `+0x14` | u16 | **DEF↑** → actor `+0x15C/+0x15E` (defender defense, high facet). |
| `+0x16` | u16 | **DEF↓** → actor `+0x160/+0x162` (defender defense, low facet). |
| `+0x18` | u16 | **AGL** → actor `+0x168/+0x16A` (rescaled into the accuracy/evasion seed). |
| `+0x1A` | u16 | **SPD** → actor `+0x164/+0x166` (turn-order initiative seed; buffable). |
| `+0x4A` | u8 | Magic-slot count. |
| `+0x4C` | u32[] | Spell-entry pointers (count at `+0x4A`); each entry's first byte is the element type (2/3/4/5/0xB) that sets per-element magic-resistance at actor `+0x1EF..+0x1F3`. |

All six stat names are pinned by the consumers of those actor slots - see [battle-formulas.md](battle-formulas.md#actor-stat-block--monster-record-mapping). The parser exposes them via `legaia_asset::monster_archive::MonsterRecord::{attack, defense_high, defense_low, agility, speed, spirit}`.

### Monster archive (PROT entry 867)

`FUN_800542C8` streams the records as **per-monster `0x14000`-byte LZS slots** at archive offset `(id-1)*0x14000` (the monster id is the global monster-table index, ~194 fixed slots). Each slot is `[u32 decompressed_size][Legaia LZS stream]`; the decoded block's head is the stat record above, with the name / XP / spell payloads at the block-relative offsets the loader fixes up.

The archive is **PROT entry `0867_battle_data`** (the EXTENDED footprint — the 15.9 MB archive lives in the entry's trailing-gap sectors, not its small indexed payload). The CDNAME label `monster_data` (PROT 869) is a misleading stub: it's only `0x30000` bytes and its `(id-1)*0x14000` slots don't decode. The shipped retail build takes the debug `FUN_8003E8A8(0x365)` PROT-index path (`_DAT_8007B8C2 != 0`); the alternate `data\battle\<name>` open via the `break 0x103` host trap (`FUN_800608F0`) is a build-time dev-host artifact with no matching ISO9660 file on the disc.

Pinned by a PCSX-Redux watchpoint during the Rim Elm scripted battles (`scripts/pcsx-redux/autorun_monster_record_source.lua`): the loader's relative seek `(id-1)*40` sectors + the `disc_read` CdlLOC resolve to PROT.DAT offset `0x38AF000` = entry 867, and three decoded records match the live actor stats byte-for-byte (Gimard id 10 = HP 99 / MP 20, Killer Bee id 62 = 288 / 288, Queen Bee id 63 = 888 / 888). town01's encounter formations resolve to the Rim Elm Mist-attack set (Gobu Gobu id 4, Green Slime 7, Gimard 10, Hornet 61, Killer Bee 62, Queen Bee 63, Tetsu 79 — Tetsu being the 999/999 tutorial sparring partner).

Parser: [`legaia_asset::monster_archive`](../../crates/asset/README.md) (`record(entry, id)` / `records(entry)`; CLI `asset monster-archive`). Engine bridge: `legaia_engine_core::monster_catalog::catalog_from_monster_archive`, merged into the catalog by `SceneHost::enter_field_scene` for the scene's encounter ids so triggered battles spawn real stats.

## Stat aggregator (`FUN_80042558`)

Per-frame helper that walks the 3 active party members (stride `0x414` - see [character record layout](#character-record-layout)) and:

1. Caps each character's stats at `0x3E7` (999, the in-game stat ceiling).
2. ORs the character's "active abilities" 16-byte block at `+0xF4..0x100` into a global 4×u32 bitmask at `0x80074358..0x80074368`. This is the "currently-active accessory effects" register read by every other game system.
3. For each character, calls `FUN_800432BC` / `FUN_80042DBC` to add/remove temporary spells per the active spell-slot layout at `+0x2B0`.

The 4-u32 global ability bitmask is what tells the renderer to draw "auto-counter" / "regen" / "magic up" indicators and what tells the battle dispatcher to apply post-hit effects. The read-side primitive is `FUN_800431D0(bit_id) -> bool` - `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. It's a 6-instruction hot helper cited from the action validator (`FUN_8003FB10`) and most damage / status code paths, so a clean-room port models it as `BattleState::ability_active(u8) -> bool`.

`FUN_800349EC` and `FUN_80035EA8` are the HP / MP threshold UI classifiers - given a character index they compare current vs max and return one of `2` (dead/zero) / `6` (low) / `7` (warn) / `9` (healthy). The dialog renderer keys text colour on the result.

`FUN_8003FB10` is the **action validator** that decides whether a queued action can proceed for the active actor. It sub-dispatches on `actor[+0x9A8]` (the queued-action byte) into 16+ handler arms; each arm consults a mix of per-actor state, the current target's record at `0x80084708 + tgt*0x414`, the global ability bitmask via `FUN_800431D0`, and the `0x8007BD10` actor-type table to gate the action with a 16-bit return code (action-OK, blocked, requires-target-flag, etc.). Engine reimpl wires this between the move VM and the per-actor state machine `FUN_801E295C`.

## Battle archive (`FUN_80052FA0` / `FUN_800542C8`)

Two SCUS-side archive loaders feed the battle state. Their record-walk helpers:

- `FUN_800536BC` - copies records of stride `0x1C` from the archive into runtime layout, applying delta fixups to 6 of the 7 u32 fields (offset → absolute pointer pattern: `record[+0x18..0x30]`).
- `FUN_80053898` - bubble-sort over the 7-u32-stride records keyed on parallel byte arrays.
- `FUN_80053B9C` - copies short-array records into the per-slot UI buffer at `iVar1 + 0x894 + slot*0x1E0`, OR-ing `0x8000` into each entry (the "active" flag).

Both archive loaders interact with the battle character / monster slots via the 8-actor table at `0x801C9370`.

## Character record layout

Stride `0x414` bytes per character, base `0x80084708` (so character `n` lives at `0x80084708 + n*0x414`). Surfaced by the inventory/spell helpers (`FUN_80042558`, `FUN_80042DBC`, `FUN_800432BC`, `FUN_800431FC`, `FUN_80043264`):

| Offset | Use |
|---|---|
| `+0x13C` | u8 spell-list count. |
| `+0x13D..+0x160` | u8 spell IDs (variable-length; up to 36). |
| `+0x161..+0x184` | u8 parallel spell-level / experience array. |
| `+0x196..+0x19D` | u8 equipment slot bytes (8 slots; weapon, armour, accessories). |
| `+0x141`-ish | Character name string (used by the `FUN_80036044` `0xC1` text-escape). |
| `+0x2B0..+0x37F` | Active spell-slot array (stride `0x14`, up to N entries). Populated by `FUN_80042DBC` from the spell list. |
| `+0xF4..0x100` | "Active abilities" 16-byte block - OR'd into the global 4×u32 bitmask at `0x80074358..0x80074368` by `FUN_80042558`. |
| `+0x104..0x110` | HP / MP / SP triplets (cur, max stored as separate u16s). |
| `+0x10E` | u8 - written on level-up (delta `+8` for Vahn slot in the captured pre→post pair). Likely max-HP byte component or stat-derived rank. |
| `+0x11A` | Stat-cap field (clamped to `0x3E7`). |
| `+0x11C..+0x122` | Six adjacent stat bytes (paired) - incremented by small deltas (`+1..+4`) on level-up. Likely the per-stat rank table consumed by the level-up apply path. |
| `+0x130` | u8 - incremented by `+1` on level-up (rank-style counter, e.g. number of times leveled). |
| `+0x161..+0x184` | u8 spell-level array (one byte per spell id; stride matches spell list). Magic-rank up writes here (delta `+1` per learned spell). |

**Level-up captured deltas (Vahn, pre/post a single character-level event).** Diff captured via `mednafen-state` shows the per-character side-effects:

| Offset | Width | Pre → Post | Interpretation |
|---|---|---|---|
| `+0x00` | u8 | `0x4F` → `0x73` (79 → 115) | Possibly raw level byte / per-character XP-derived counter. |
| `+0x04..+0x06` | u16 LE | `0x016D` → `0x02DA` (365 → 730) | XP word delta (+365). Matches the published level-up XP curves. |
| `+0x10E` | u8 | `0x3A` → `0x42` (+8) | Max-HP / vitality byte. |
| `+0x11C..+0x122` | 6× u8 | `67/1C/13/10/16/0B` → `6B/20/15/12/1A/0F` | Per-stat increments (`+4 +4 +2 +2 +4 +4`). |
| `+0x130` | u8 | `0x02` → `0x03` | Rank counter. |

Noa and Gala records are byte-identical across the same pair - the level-up event in this capture pair is for Vahn alone.

**Magic-rank up captured deltas (Vahn, pre/post a single magic-rank-up event).** Diff over the same record range surfaces a strict subset of the level-up footprint, focused on the spell-level table:

| Offset | Width | Pre → Post | Interpretation |
|---|---|---|---|
| `+0x08` | u8 | `0x30` → `0x3C` (+12) | Flag word - specific bit TBD. |
| `+0x9C` | u8 | `0x09` → `0x0A` (+1) | Magic-rank mirror. |
| `+0x10A` | u8 | `0x1B` → `0x11` (-10) | TBD (transient battle state, possibly post-strike). |
| `+0x161` | u8 | `0x02` → `0x03` (+1) | Spell-level byte (`+0x161..+0x184` array). Confirms magic-rank up writes here. |

## Battle main dispatcher (`FUN_801D0748`)

11 KB / 182 calls. The top of the per-frame battle loop. Routes through every active battle subsystem (rendering, AI, animation, hit detection).

## Hottest battle utility (`FUN_801D8DE8`)

3 KB / 77 incoming refs. The single most-cited battle helper - likely a per-actor utility that every state arm bottoms out into.

## Weapon / effect trail builder (`FUN_80048310` + `FUN_800485BC`)

Visual-only helpers that build the swept geometry behind a moving battle actor (sword trails, dash plumes, particle ribbons). `FUN_80048310` iterates the 16-slot per-actor frame buffer at `actor[+0x68]`, copies vertex triplets from the per-actor pose pool at `gp[0xa0c] + 0x6f4` (stride `0xC`), and calls `FUN_800485BC` twice - once for the outline, once for the base - blending two endpoint colours over N steps via a `0..N` gradient loop.

`FUN_800485BC` is a 275-instruction quad-strip emitter. It looks up the actor pose from `*(int*)(0x801C9370 + actor[+0x5A]*4) + 0x34/+0x38` (re-confirms the battle actor pointer table), reads sin/cos LUTs at `_DAT_8007B81C` / `_DAT_8007B7F8` keyed on `actor[+0x26] * 0xFFF`, runs each vertex through `FUN_800195A8` for GTE projection, and drops `0x3B808080` (GP0 G3 textured-quad) packets into the OT.

These are pure rendering helpers - no gameplay state changes. Engine reimpl can defer them until visuals matter.

## Inventory (`crates/asset` page-banked layout)

Battle reads inventory through the same page-banked structure the field VM's op `0x3B` `SET_ITEM_COUNT` writes: 16 entries × 16-bit per page × 0x414-byte stride. The page index is the high nibble of the slot byte; the entry index is the low nibble.

The page-banked inventory state lives in the 512-byte region at `[0x80085718 .. 0x80085918)` - adjacent to the fourth-flag-bank bitfield at `DAT_80086D70` (see [field VM](script-vm.md) → "fourth flag bank"). The field VM's op `0x4C` sub-3 sub-2 zeros the entire region.

## Status effects

Per-actor status conditions inflicted by enemy attacks or art `enemy_effect` bytes. The retail engine stores per-status timers and tick-damage values in the battle-actor struct around `+0x130`; the layout is per-flag and not captured in any single overlay dump.

| Kind | Source byte | Default duration | Per-turn effect |
|---|---|---|---|
| Burned | `1` | 4 turns | `max_hp / 16` HP tick damage |
| Shocked | `2` | 3 turns | 50% chance to skip turn |
| Poisoned | `3` (Other) | 6 turns | `current_hp / 8` tick damage |
| Asleep | `4` | 3 turns | Skip until hit |
| Confused | `5` | 3 turns | Random target |
| Silenced | `6` | 4 turns | Block Magic actions |
| Stunned | `7` | 1 turn | Skip one turn |
| Petrified | `8` | until cured | Skip turn entirely |

Implementation: [`crates/engine-vm::status_effects`](../../crates/engine-vm/src/status_effects.rs). The per-tick `StatusEvent` stream feeds back into the engine's HUD pipeline; engines call `World::tick_status_effects` once per round and consume `StatusEffectTracker::drain_events()` for log lines.

## AP / Spirit gauge

Each character has a per-turn AP budget that limits how many art commands they can chain. The retail engine reads this from the character record's `+0xC9` (`current_ap`) and `+0xCA` (`bonus_ap`) bytes. Pressing the Spirit button during command input adds `+5` AP exactly once per turn.

The base AP grows by 1 each 10-level milestone (level 1..9 → 4 AP, 10..19 → 5 AP, …, 60+ → 10 AP capped).

| Action constant range | AP cost | Notes |
|---|---|---|
| `0x00` Nothing | 0 | placeholder |
| `0x01..=0x05` | 0 | system actions (Item / Magic / Attack / Spirit / Escape) |
| `0x0C..=0x0F` | 0 | direction bytes (free) |
| `0x19` Regular Art Starter | 1 | |
| `0x1A` Special Art Starter | 1 | |
| `0x1B..=0x32` | 1 | per-character art body |

Implementation: [`crates/engine-core::ap_gauge`](../../crates/engine-core/src/ap_gauge.rs). The `World` carries a `[ApGauge; 3]` (one per party slot); engines call `World::reset_party_ap` at turn start.

## Battle stat aggregator

Clean-room port of `FUN_80042558`. Walks the 8 equipment slots, sums modifiers into the actor's resolved attack / UDF / LDF / accuracy / evasion, ORs equipment ability bits into the global 4×u32 mask, then folds in status-effect modifiers (Burned reduces ATK by ~12.5%, Confused halves accuracy, Asleep / Stunned / Petrified zero evasion and block actions, Silenced / Petrified block Magic).

Implementation: [`crates/engine-core::battle_stats`](../../crates/engine-core/src/battle_stats.rs). The pure function `compute_battle_stats(record, table, statuses, modifiers) -> BattleStats` is deterministic and side-effect-free - engines call it once per turn-start.

## Item catalog

Typed catalogue of inventory items the battle / field menu consults. Each entry has an `ItemEffect` describing the side-effect (Heal / Cure / Revive / Stat-up / Spirit-up / Capture / Escape / Damage / KeyItem). The vanilla catalog ships 19 entries covering every category.

`apply_effect(effect, &TargetSnapshot) -> ItemOutcome` is the pure resolver - engines fold each `ItemOutcome` into world state through whatever runtime path they have for HP / status / AP / inventory.

Implementation: [`crates/engine-core::items`](../../crates/engine-core/src/items.rs).


## Battle round lifecycle

`BattleRound::begin(&mut world, &[Option<StatRecord>; 8], &EquipmentTable, &StatusModifiers)` resets every party AP gauge, recomputes per-slot `BattleStats` through `compute_battle_stats`, and writes the resolved attack / UDF / LDF back into `World::battle_attack` / `battle_defense_split` so the strike resolver picks them up. `BattleRound::end(&mut world)` ticks every actor's status, folds Burned / Poisoned tick damage into `BattleActor::hp`, and returns the count of actors that died from tick damage this round.

The returned `BattleRound` carries per-slot `action_blocked` / `magic_blocked` arrays the action validator filters command input against (Asleep / Stunned / Petrified actors lose action; Silenced / Petrified actors lose Magic).

Implementation: [`crates/engine-core::battle_round`](../../crates/engine-core/src/battle_round.rs).

## Battle command runner

Sits between the player-input layer and the action state machine. One `BattleRunner` per battle session; engines feed it raw player commands per turn and call `tick_action` to drive the per-frame action SM.

`begin_round` delegates to `BattleRound::begin` for AP refresh + stat recompute, `push_command` / `push_chained_art` gate input against `ApGauge` and surface a typed `OutOfAp` error, `pop_command` / `pop_chained_art` refund the cost cleanly, `commit_turn` runs the queue through `resolve_action_queue` (Miracle / Super expansion) and stashes the resolved per-slot `ActionQueue`s. `end_round` drives `BattleRound::end` for tick-damage drainage.

Per-slot buffers + chained-art lists let the player switch between party members mid-turn without losing state. The runner is the **input → queue** half of the battle pipeline; the SM tick itself runs through the existing `step_battle` loop.

Implementation: [`crates/engine-core::battle_runner`](../../crates/engine-core/src/battle_runner.rs).

## BattleSession Resolve driver

`BattleSession` owns the action SM during the `Resolve` phase. After
`commit_turn` succeeds, the session builds a `ResolveDriver` queue
containing one entry per party slot whose resolved action queue is
non-empty, in slot order (`0 → 1 → 2`). Slot routing:

| Resolved queue contains | Action category byte |
|---|---|
| At least one `ActionConstant::RegularStarter` | `TacticalArts (0)` |
| Otherwise (directional commands only) | `Attack (3)` |

Each `BattleSession::tick` during `Resolve`:

1. Drains `World::pending_battle_events` into HUD popups + session events.
2. If the head-of-queue attacker hasn't been armed yet, sets
   `world.battle_ctx.{active_actor, queued_action, action_state}` and
   the attacker's `BattleActor::{action_category, active_target}` to
   point at the first alive monster slot.
3. Calls `world.tick()` exactly once.
4. Clears `ActorFlags::ADVANCE_DONE` on `AttackRecovery` (the render-side
   "recovery anim finished" edge the session simulates inline since it
   doesn't render).
5. On `Transition { from: AttackChain, to: AttackRecovery }`, applies a
   clean-room formula strike against the attacker's `active_target`:
   reads `atk` + `udf` + `acc` + `eva` off `BattleRound::stats`, rolls
   accuracy via `accuracy_roll`, folds variance via `psyq_rand_step`,
   writes the result back through `BattleActor::hp` and emits
   `SessionEvent::HpChanged`.
6. On `EndOfAction`, pops the head of the queue and re-arms next frame.

When the queue drains (no more attackers) or `StepOutcome::BattleComplete`
fires, the session drops the driver and transitions to `RoundOutro`
(queue-drained path) or relies on the routed `BattleEnd` event to land
the terminal phase (`Victory` / `Defeat`). Engines that prefer to drive
`world.tick()` themselves can skip `commit_turn` from the session and
fall through the legacy "observe events only" Resolve path.

The deterministic RNG seed used for the accuracy + variance rolls is
exposed as `BattleSession::rng_seed` (configurable via
`with_rng_seed(seed)` before `begin_round`).

End-to-end coverage:
[`crates/engine-core/tests/end_to_end_gameplay_loop.rs::battle_session_drives_action_sm_to_monster_wipe`](../../crates/engine-core/tests/end_to_end_gameplay_loop.rs)
exercises the full pipeline — encounter trigger → BattleSession setup →
`push_command` per slot → commit via `SessionInput { start: true, .. }` →
Resolve → `BattlePhase::Victory`.

## Battle HUD model

Renderer-agnostic UI state for the in-battle screen. Holds per-slot HP / MP / AP / status-icon state plus a queue of damage popups and battle-event log lines. `engine-render::battle_hud_draws_for` turns one of these into a `Vec<TextDraw>` for the GPU pipeline; engines that render via a different path (web / terminal) read the same struct directly.

The HUD is fed by `World` events:

- `BattleEvent::ApplyArtStrike` → `push_damage` / `push_heal` (per-strike popup with a fade timer).
- `StatusEvent::TickDamage` / `Cleared` → `sync_status` (replaces the slot's icon list from the `StatusEffectTracker`).
- `BattleRound::begin` / `end` → `sync_slot` (refreshes HP / MP / AP per round).

Damage popups carry a 60-frame default lifetime and an `alpha()` helper for fade-out renders. The log column rings the most recent N entries (default 6, matching the retail scrolling-log column).

Implementation: [`crates/engine-core::battle_hud`](../../crates/engine-core/src/battle_hud.rs).

## SFX bank + scheduler

Maps battle / field cue IDs (the `kind` byte the art-record `HitCue` / overlay scripts emit) to per-cue `SfxEntry` descriptors that describe how to fire a one-shot through the SPU. Engines populate the catalog at startup, then forward `ScheduledCue`-like requests through `SfxScheduler` which queues each request with its retail timing offset and dispatches when the per-frame tick reaches the firing frame.

| Cue ID | Meaning |
|---|---|
| `0x1A` | Generic SFX trigger ("play sound" hit cue). |
| `0x4C` | Hit-effect visual (no sound on its own). |
| `0x80..=0xFE` | Reserved per-character / per-art SFX IDs. |

`SfxBank::play_one_shot` delegates to the existing `VabBank::play_note` for tone lookup, pitch math, and ADSR setup; the scheduler is a frame-driven queue that returns an `SfxFireBatch` per `tick_frame` call.

Implementation: [`crates/engine-audio::sfx`](../../crates/engine-audio/src/sfx.rs).

## Inventory item-use session

State machine that drives the "open inventory → pick item → pick target → use it" flow shared between the field menu and the battle command menu. Engines own a single `InventoryUseSession` for the lifetime of the inventory screen; per-frame they push input events and drain `InventoryUseEvent`s.

Filters items by `InventoryContext` (battle vs field - `usable_in_battle` / `usable_in_field` from the catalog), validates target compatibility (Revive needs a dead target; everything else needs a live one), and folds the resolved `ItemOutcome` into the engine's world state via `World::use_item`.

Implementation: [`crates/engine-core::inventory_use`](../../crates/engine-core/src/inventory_use.rs).


## Encounter system

Per-scene random-encounter trigger. Engines own one `EncounterSession` per active field scene; the field-step path calls `on_step(rng_word)` each step the player moves. The session brackets the transition with five phases:

| Phase | Drives |
|---|---|
| `Idle` | Steady state. Steps roll against the table; safe zones suppress. |
| `Transition` | Roll succeeded; `transition_frames` (default 32) of camera-shake / fade-out. |
| `Triggered` | Engine drains the resolved `EncounterRoll` and loads the battle scene. |
| `Battling` | Battle is running; tracker is suspended. |
| `Grace` | Post-battle "no immediate re-encounter" window (`grace_frames`, default 30). |

`EncounterTable` holds the per-scene rows + 1/256 trigger rate + safe-zone rectangles. `EncounterTracker::add_rate_bias` lets accessory effects (Goblin Foot = -32, Encounter Up = +32) tune the effective rate per-roll.

Implementation: [`crates/engine-core::encounter`](../../crates/engine-core/src/encounter.rs).

## Battle target picker

Drives the post-action target cursor. Parameterised on a `TargetKind` enum constraining valid targets:

| TargetKind | Allowed targets |
|---|---|
| `SingleEnemy` | One alive monster slot. |
| `SingleAlly` | One alive party slot, **excluding** the actor. |
| `SingleAllyOrSelf` | Any alive party slot, including the actor. |
| `DeadAlly` | One fallen party slot (Revive / Resurrection). |
| `AnyAlly` | Any party slot, alive or dead. |
| `AllEnemies` / `AllAllies` | Sweep target - auto-confirm. |
| `Self_` | The actor itself - auto-confirm. |

Sweep kinds resolve in `init_cursor`; single-target picks walk valid candidates with cursor-wrap and auto-skip-dead. Implementation: [`crates/engine-core::target_picker`](../../crates/engine-core/src/target_picker.rs).

`BattleSession::push_command_with_target(world, cmd, kind, actor_slot)` is the wiring API engines drive when a command needs a target. The session charges AP up-front, opens the picker, and stashes the command in `pending_target_command`. When the picker resolves, `maybe_close_picker_with_world` writes the resolved slot to `BattleActor::active_target` (the field the action SM reads at strike time via `host.actor(actor_slot).active_target`) and admits the buffered command into the runner queue without re-charging AP. Sweep targets write a `0xFF` sentinel; cancellation drops the command without admitting it. Engines that already have a `&World` borrow at picker-open time use [`open_target_picker`]; engines that need the same active-target write at open-time (sweep / self) call [`open_target_picker_mut`].

## Encounter trigger - runtime memory layout

A pre/post encounter save pair (one frame walking the `map01` field scene; the next frame with battle just initiated, same `map01` scene) pins the runtime memory layout of an encounter trigger. The `mednafen-state diff` over `0x801C0000..0x80200000` surfaces:

| Range | Bytes changed | What it is |
|---|---:|---|
| `0x801CE808..0x801F3818` | ~133 KB | Battle overlay loaded into RAM (single contiguous region) |
| `0x801C9370..0x801C9900` | ~200-500 B | 8-slot battle actor pointer table; stride `0x60` per slot |
| `0x80083000..0x80084000` | ~600 B | Scene-bundle / sound-pool: encounter formation + BGM resolution |

The active scene-name table at `0x80084540` (CDNAME label + scene index) is **identical** between the pre-encounter and post-encounter saves - the battle is layered on top of the field scene rather than swapping it out. Engines that drive the field-to-battle transition therefore preserve the active-scene state and only resolve the formation + battle overlay.

Codified as constants in [`crates/engine-core::capture_observations::encounter_trigger`](../../crates/engine-core/src/capture_observations.rs); a disc-gated test in [`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs) (`encounter_trigger_diff_loads_battle_overlay`) exercises the real save bytes.

## Battle scene-init residency window

A separate `map01` save pair (one frame with the encounter armed but
battle not yet entered, the next frame with battle just initiated)
pins the **post-load residency window** of the battle scene-init
pipeline. Distinct from the encounter-trigger overlay swap above; this
pair brackets the loader function with concrete RAM-resident artefacts
the loader writes into.

| Range | Bytes changed | What it is |
|---|---:|---|
| `0x80124690..0x801503C4` | ~168 KB | Battle-bundle residency window. Pre-battle holds field-scene payload (sample dialog text strings visible); post-battle holds battle-bundle data (vertex / TIM / actor records). Codified as `BATTLE_BUNDLE_WINDOW`. |
| `0x801CE808..0x801D3018` | ~16 KB | Battle-overlay scratch slice. Wholesale reset on entry; distinct from the broader encounter-trigger overlay residency at `0x801CE800..0x801F4000`. Codified as `OVERLAY_SCRATCH_WINDOW`. |
| `0x800836C8` | 4 B | Per-frame actor-tick fn-pointer slot in the bundle-pool extension. Pre-battle reads `0x80024C50`; post-battle reads `0xF41D0280` = `FUN_80021DF4`. Codified as `ACTOR_TICK_FN_PTR_ADDR` / `ACTOR_TICK_FN_PTR_VALUE`. |
| `0x801FFCA0..0x801FFFFE` | ~600 B | CD I/O state slice. Rewires while the battle bundle is paged in; reliable "battle scene-init in flight" signature. |

The pair is **post-load** by design - both save frames resolve to a
state where the loader function has already returned. The loader
function (which reads PROT entry `0x05C4` + sibling Seru blobs and
populates the battle bundle) lives in an overlay slice that is not
directly visible in either snapshot. Pinning it requires a
mid-execution capture between the field→battle game-mode flip and
this residency state, which the current Mednafen workflow can't
generate without manual frame-stepping (mednafen 1.29 has no headless
mode).

Codified as constants in
[`engine_core::capture_observations::battle_init_overlay`](../../crates/engine-core/src/capture_observations.rs);
disc-gated test
`battle_init_overlay_pair_pins_battle_bundle_window_and_actor_tick_wiring`
in `crates/mednafen/tests/real_saves.rs`.

## Item-use battle-event residency

A mid-battle save pair (battle just initiated; party member about to
use a Healing Leaf) pins the **item-use sub-mode residency**:

| Address | Pre / Post | Notes |
|---|---|---|
| `_DAT_8007B8D0` | `0x8014BD30 → 0x800ABA4C` | Field-pack base pointer flips. The item-use sub-mode reseats the active scene asset buffer. |
| `0x801BA7DC..0x801BADEC` | ~660 B shift | Script-VM context block. The menu / item / target / commit pipeline rewrites the entire ctx region as it runs. |
| Actor pool slots 0..4 | per-frame motion deltas | 3 party + 2 monsters (count-2 formation). Slots 5..7 stay zero across the pair. |

The captured pair uses a **Healing Leaf** (consumable HP-restore) -
not Fire Book I (a spell-learn item). The pair therefore pins the
residency window of the item-use battle-event handler without lifting
the Fire Book-specific writer to the displayed-skills array at
`+0x185`. A second save pair specifically capturing Fire Book I use
is required to lift that writer.

Codified as constants in
[`engine_core::capture_observations::item_use_battle_event`](../../crates/engine-core/src/capture_observations.rs);
disc-gated test
`item_use_pair_pins_field_pack_base_flip_and_script_vm_ctx_shift`
in `crates/mednafen/tests/real_saves.rs`.

## Captured stat-growth observations

The `mednafen-state diff` toolkit ([`docs/tooling/mednafen-automation.md`](../tooling/mednafen-automation.md)) over a magic-rank-up + character-level-up save triplet pins the per-byte footprint for Vahn (party slot 0). The observed deltas inside Vahn's character record at `0x80084708` (stride `0x414`):

| Event | Offset | Before → After | Interpretation |
|---|---|---|---|
| Magic-rank up (pre → post) | `+0x08` | `0x30 → 0x3C` | flag word low byte (+12) |
| Magic-rank up | `+0x9C` | `0x09 → 0x0A` | magic-rank counter (+1) |
| Magic-rank up | `+0x10A` | `0x1B → 0x11` | low byte of `mp_max` (cast cost spent) |
| Magic-rank up | `+0x161` | `0x02 → 0x03` | spell-level array (`spell_levels[0]` +1) |
| Level-up, 4-level jump (pre → post) | `+0x00` | `0x4F → 0x73` | unconfirmed (jump +0x24 doesn't match a single-level granularity) |
| Level-up | `+0x04..+0x06` | `0x016D → 0x02DA` | u16 LE XP delta (+365) |
| Level-up | `+0x10E` | `0x3A → 0x42` | low byte of `sp_max` (Spirit, +8) |
| Level-up | `+0x11C..+0x12C` | six per-byte +1..+4 | per-stat increments at byte stride 2 |
| Level-up | `+0x130` | `0x02 → 0x03` | rank counter (+1) |

The retail per-Seru per-level lookup table that drives these increments is **not** in `SCUS_942.54`. A writer-search across the captured `magic_level_up` overlay returned negative for code-side `sb`/`sh` writes targeting the documented record offsets, and the "Seru struct +0x74 pointer dereference" hypothesis isn't supported by the current capture set - the `+0x74` reads in the captured overlay surface a 32-bit battle-state flag the SCUS-side handler `FUN_800480D8` writes with the constant `0x80808080`, not a stat-grant pointer. The grant table either lives in a still-uncaptured overlay (battle-data init or the Seru-equip path) or is encoded inline in a Seru PROT entry the current capture set doesn't surface. The next step toward a true per-character `StatGrowthCurve::PerLevel` vector is a runtime-watchpoint trace during a level-up, not another static grep.

Engines populate one captured observation at a time via:

```rust
let obs = legaia_engine_core::levelup::observations::vahn_mc8_to_mc9();
let tracker = LevelUpTracker::new().with_observed_curve(0, &obs);
```

`LevelUpObservation::to_curve` produces a `StatGrowthCurve::PerLevel` vector that emits the per-level *average* inside the observed range and falls back to `StatGain::default` outside it. Implementation: [`crates/engine-core::levelup`](../../crates/engine-core/src/levelup.rs).

## CDNAME → MV STR cutscene routing

`engine_core::scene::cutscene_str_for(scene_label) -> Option<&'static str>` resolves an `op*` / `edteien` CDNAME label to its paired `MOV/MVn.STR` filename. The disc carries 6 STR files (`MV1.STR..MV6.STR`); the heuristic mapping is:

| CDNAME | STR file | Scene context |
|---|---|---|
| `opdeene` | `MOV/MV1.STR` | Drake Castle opening |
| `opstati` | `MOV/MV2.STR` | Statue scene |
| `opkorout` | `MOV/MV3.STR` | Korout opening |
| `opurud` | `MOV/MV4.STR` | Urud opening |
| `opmap01` | `MOV/MV5.STR` | World map opening |
| `edteien` | `MOV/MV6.STR` | Garden ending FMV |

`cutscene_label_for_str(filename)` is the inverse (case-insensitive on the basename so `mv1.str` and `MOV/MV1.STR` both round-trip). The remaining `ed*` scenes (`edbylon`, `edbalden`, `edlast`, `edretoin`, `edkorout`, `edbubu`, `eddoman`, `edson`, `edstati3`) are dialogue-actor-overlay driven and have no FMV. The exact retail mapping table lives in the cutscene overlay (not yet captured) - when it lands, the lookup function should be updated to consult the captured map. The `legaia-engine play` and `play-window` subcommands auto-resolve the STR file when the user passes `--scene <op*|edteien>` and the extracted root contains the matching MV file.

## Equipment catalog

Vanilla equipment table covering the early-game roster. Each entry is an `EquipmentEntry` carrying id + name + slot + character restriction + `ItemModifier` + buy/sell prices. `to_modifier_table()` resolves to the `EquipmentTable` the battle stat aggregator (`compute_battle_stats`) reads.

Slots match the retail `equip[8]` byte array at character record `+0x196`:

| Slot | Index | Examples |
|---|---|---|
| Weapon | 0 | Vahn-only swords, Noa-only knuckles, Gala-only quarterstaves |
| Helmet | 1 | Cloth Cap → Mythril Helm |
| Body Armor | 2 | Cloth Robe → Plate Mail |
| Hand Guard | 3 | Cloth Wrap → Iron Gauntlets |
| Boots | 4 | Cloth Shoes → Wind Boots (ability bit 12) |
| Ring 1/2 | 5/6 | Power / Defense / Speed / Hit Rings |
| Accessory | 7 | Goblin Foot (encounter rate down) / Wisdom Ring (MP cost) / Lucky Charm (bonus EXP) |

Implementation: [`crates/engine-core::equipment`](../../crates/engine-core/src/equipment.rs).

## Seru capture + spell learning

Per-character per-Seru capture-point accumulator. Each captured Seru contributes points toward a per-character spell-learn threshold (default 100); once crossed, the spell is added to the character's learned list.

`SeruDef::learnable_mask` is a 3-bit per-character mask (bit 0 = Vahn, bit 1 = Noa, bit 2 = Gala) so single-character Seru can teach only their bearer. `record_capture` is the pure resolver; `SeruCaptureSession` drives the post-capture banner sequence (`Capturing → Announcing[i] → Done`) for engines to render.

Implementation: [`crates/engine-core::seru_learning`](../../crates/engine-core/src/seru_learning.rs).

## Tactical Arts chain editor

Menu-side state machine for composing + saving Tactical Arts command chains. `ChainLibrary` holds up to 8 saved chains per character (3..=7-byte length range, matching retail). `ChainEditor` runs a 4-phase SM: `Browsing { cursor } → Editing { working } → Naming { working, name } → Done`. Engines feed picks back to `BattleRunner::push_chained_art` at battle start.

Implementation: [`crates/engine-core::tactical_arts_editor`](../../crates/engine-core/src/tactical_arts_editor.rs).

## Battle rewards composite

`World::apply_battle_loot(formation, catalog) -> BattleRewards` is the post-victory composite that turns a defeated formation into the runtime side-effects:

- Sums each `MonsterDef::exp` and distributes the total via `World::apply_battle_xp`, which splits the pool equally among the surviving party members (integer divide, remainder dropped; dead members get zero) and runs per-character level-up checks against `LevelUpTracker::xp_table`.
- Sums each `MonsterDef::gold` and adds it to `World::money` (saturating).
- For each defeated monster with a non-`None` `drop_item` and `drop_rate_q8 > 0`, pulls one byte from `World::next_rng` and compares against `drop_rate_q8 / 256`. On hit, the item id is appended to `BattleRewards::drops` and incremented in `World::inventory`.
- Returns `BattleRewards { xp, gold, level_ups, drops }` for the engine to surface as the post-battle banner ("got N XP, M gold, level up, found Healing Leaf!").

Monster ids missing from the catalog contribute zero (silently skipped) so a partially-populated catalog still drives a battle-end transition. Implementation: [`crates/engine-core::world::World::apply_battle_loot`](../../crates/engine-core/src/world.rs).

## End-to-end gameplay loop integration test

`crates/engine-core/tests/end_to_end_gameplay_loop.rs` stitches every gameplay-side subsystem into one cycle:

1. **Boot** — load an `LGSF v2` `SaveFile` (party + story flags + money + inventory) into a fresh `World` via `load_full`. `load_full` hydrates the `LevelUpTracker` per-slot level from each record's `+0x100` byte so reloads don't roll the tracker back to L1.
2. **Field walk** — switch to `SceneMode::Field`, install an `EncounterSession` keyed to `vanilla_formation_table` at saturated trigger rate, step until `EncounterPhase::Triggered`.
3. **Encounter** — drain the formation roll, populate monster slots 3..N from the `MonsterCatalog`, flip mode to `SceneMode::Battle`.
4. **Battle SM** — drive `World::tick` while applying clean-room formula damage on every `AttackChain → AttackRecovery` transition until the action SM resolves to `BattleEndCause::MonsterWipe`.
5. **Rewards** — call `World::apply_battle_loot` to credit the per-character XP / gold split, fire drop rolls, and trigger per-character level-ups; assert at least one party slot crossed a threshold.
6. **Save round-trip** — `world.save_full().write() → SaveFile::parse() → load_full()` into a fresh `World`; assert HP/MP, level, money, story flags, and inventory survived intact.

The crate ships four test variants:

| Test | Purpose |
|---|---|
| `synthetic_party_completes_full_gameplay_loop` | The default CI cycle; hand-spins the action SM with `apply_strike`. |
| `battle_session_phase_transitions_during_loop` | Smoke around the BattleSession side; verifies the session reaches `CommandInput`. |
| `battle_session_drives_action_sm_to_monster_wipe` | Drives the same loop through `BattleSession::tick` instead of `world.tick` — `push_command` → `SessionInput { start: true }` → Resolve → `BattlePhase::Victory`. The session owns the action SM during `Resolve`. |
| `real_battle_data_encounter_drives_loop` | Disc-gated: scans an early `PROT.DAT` entry for a valid `EncounterRecord` byte pattern, installs it via `World::install_encounter_from_record`, and runs the battle through to `MonsterWipe`. Closes the synthetic-formation leak in the field → battle handoff. |
| `real_psx_memory_card_save_drives_full_loop` | Disc-gated: boots the same loop from a real Legaia memory-card save block via `Party::from_retail_sc_block` when `~/.mednafen/sav/` holds a Legaia card. |

Disc-gated variants skip silently when `extracted/PROT.DAT` / the mednafen card is missing.
