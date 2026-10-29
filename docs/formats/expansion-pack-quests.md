# Expansion pack: quests, challenges and dialogue

Two files of an expansion pack describe what the player is asked to do and what the pack's characters say: `quests.toml` (quests and challenges, parsed by `legaia_expansion_format::quest`) and `dialog.toml` (dialogue lines, `legaia_expansion_format::dialog`). The engine runs them through `legaia_engine_expansion::quest` (the quest tracker), `quest_challenge` (challenge rules and scores), `flags` (the pack's own flags and counters), `dialog` (the pack line on screen) and `quest_save` (the save block); the line compiler `compile_line` and the markup encoder sit in the format crate's `dialog` module, so the validator compiles every line too. `engine-core`'s `world::expansion_quests` wires those kernels to the `World`.

Every hook is the identity with no pack active: no event is built, no battle command is refused, no line starts, and a save carries no expansion block.

## Ids and namespaces

A quest names everything by id. Retail content is named by its coordinate: a runtime formation id (`FormationDef::formation_id`), a 1-based monster id, a CDNAME scene name, an item id, a retail system flag index. Pack content is named by the string id the pack gives it: an NPC, a dialogue line, a mode, a counter, a flag, a `battle.toml` formation.

Pack flags and counters are the pack's own store (`PackFlags`), saved under the pack id. A pack reads retail system flags as prerequisites but never writes one: a quest reward, a dialogue choice and a script event all write pack flags. The compiled dialogue bytes carry no field-VM flag op for the same reason (see [Dialogue on the inline runner](#dialogue-on-the-inline-runner)).

## `quests.toml`

```toml
[[quests]]
id = "tower_trial"
title = "The Rebirth Tower"
description = "..."
giver = "tower_guide"
prerequisites = { quests = ["tower_rumour"], min_level = 30 }
start_dialog = "guide_trial"
objectives = [
    { kind = "win_battles", count = 3, text = "Win three battles" },
    { kind = "reach_level", level = 40 },
    { kind = "talk", npc = "tower_guide" },
]
complete_dialog = "guide_done"
rewards = { gold = 10000, set_flags = ["tower_open"] }
```

| Key | Meaning |
|---|---|
| `id` | Pack-unique; the key the save records progress under. |
| `title`, `description` | Journal text. |
| `giver` | NPC id whose talk starts the quest. |
| `auto_start` | Start as soon as the prerequisites hold. |
| `prerequisites` | Start conditions, all of which must hold (below). |
| `ordered` | `true` (default): objectives complete in order. `false`: all at once. |
| `objectives` | The goals (below); the quest completes when every one is done. |
| `rewards` | Granted on completion (below). |
| `repeat` | Whether the quest can run again (below). |
| `start_dialog`, `complete_dialog` | Line ids shown on start and on completion. |
| `active_dialog` | Line id shown when the player talks to the giver while the quest is active. |
| `start_battle`, `start_boss` | A `battle.toml` formation or boss fought once the quest's start line is done. |

A quest with neither `giver` nor `auto_start` starts only from a script (`World::expansion_start_quest`).

### Prerequisites

| Key | Holds when |
|---|---|
| `quests` | Each listed quest has completed at least once. |
| `story_flags` | Each listed retail system flag is set. |
| `story_flags_clear` | Each listed retail system flag is clear. |
| `flags` | Each listed pack flag is set. |
| `min_level` | The present party's highest level is at least this. |

The system flags are the field VM's flag bank (ops `0x50` / `0x60` / `0x70`, `World::system_flag_test`), the one place the story records how far the player has come, so a story point is named by the flag its scene script raises there.

### Objectives

Each objective is a table with a `kind`, that kind's keys, and an optional `text` for the journal.

| `kind` | Keys | Done when |
|---|---|---|
| `defeat_formation` | `formation`, `count` | `count` battles against the formation are won. |
| `defeat_monster` | `monster`, `count` | `count` won battles had the monster in them. |
| `win_battles` | `count` | `count` battles of any kind are won. |
| `win_boss` | `boss` or `formation` (optional) | A boss battle is won (optionally that one). |
| `enter_scene` | `scene` | The player enters the scene (CDNAME name or pack map id). |
| `reach_level` | `level` | The party's highest level reaches `level`. |
| `collect_item` | `item`, `count`, `hand_in` | The bag holds `count` of the item. |
| `talk` | `npc` | The player talks to the NPC. |
| `win_mode` | `mode`, `count` | The pack mode is won `count` times. |
| `counter` | `counter`, `at_least` | The pack counter reaches `at_least`. |
| `flag` | `flag` | The pack flag is set. |

`count` defaults to 1. A `formation` is a number (a runtime formation id) or a string (a `battle.toml` formation id, resolved through `QuestWorld::pack_formation_id` to its compiled `0xE000+` id). A `win_boss` naming a `battle.toml` `boss` matches a won battle against that boss's formation; one naming a `formation`, or nothing, matches a won battle whose formation record carries a non-zero header byte (`FormationDef::header_flags`, retail's scripted and boss rows, and every pack boss's formation).

**Event objectives** (`defeat_*`, `win_*`, `enter_scene`, `talk`) count events. **State objectives** (`reach_level`, `counter`, `flag`) are re-tested against the world and latch once met. A `collect_item` objective tracks the bag live and does not latch: dropping the items takes the progress back. With `hand_in = true`, completing the quest takes the items.

In an ordered quest only the first objective that is not done takes an event, so one event never counts twice down the list; the objectives after it are re-tested only for state. The talk that starts a quest is not counted toward that quest's own `talk` objective: events reach the active quests before the giver check starts new ones.

### Rewards

| Key | Grant |
|---|---|
| `gold` | Added to the purse (clamped at 9,999,999). |
| `xp` | EXP for each present party member, unscaled. |
| `items` | `[{ item, count }]` added to the bag. |
| `cards` | Trading-card ids, handed to the card collection. |
| `set_flags` | Pack flags raised. |
| `counters` | `[{ counter, add }]` added to pack counters (`add` defaults to 1). |

Quest EXP goes through the level-up tracker per member and keeps the record words the battle EXP path keeps (cumulative and next-level EXP, the level byte, grown maxima), without the battle's 3/4 scale and living-member split.

### Repeat

| Key | Meaning |
|---|---|
| `repeatable` | The quest can be taken again after completing. |
| `cooldown_seconds` | Play-clock seconds that must pass after a completion. |
| `max_completions` | Cap on completions. |

The cooldown runs on the save's play clock (`clock.play_time_seconds`), so a "daily" quest is daily in play time and the tracker reads no wall clock.

## Challenges

```toml
[[challenges]]
id = "bare_hands"
formation = "ogre_pack"
rules = { turn_limit = 10, no_items = true, no_magic = true }
score = { base = 1000, turn_bonus = 100 }
rewards = { gold = 2000 }
```

A challenge is armed (`World::expansion_arm_challenge`) and then applies to the next battle against its `formation`, or to the next battle at all when it names none. Arming is refused when the present party already breaks a party rule, or when a named formation does not resolve.

| Rule | Effect |
|---|---|
| `turn_limit` | The battle must be won within this many rounds. |
| `no_items` | The battle command ring refuses the Item command. |
| `no_magic` | The battle command ring refuses the Ra-Seru Magic command. |
| `solo` | Only this roster slot may be in the party. |
| `level_cap` | No party member may be above this level. |
| `enemy_scale_permille` | Enemy HP and attack scale, `1000` = retail. |

The command refusals sit beside the special-battle word's Ra-Seru refusal in the command ring: the ring stays up and nothing commits. Rounds are counted as each round's execution begins. `solo` and `level_cap` are checked when the challenge is armed and again when the battle is won. `enemy_scale_permille` is answered by `QuestRuntime::enemy_scale_permille` for the battle setup to apply.

The battle's end clears or fails the challenge and disarms it. A win within the rules scores

```text
base + turn_bonus * (turn_limit - rounds) + level_bonus * (level_cap - party level)
```

with each bonus term present only when its rule is set and never negative. The save keeps the best score and the clear count per challenge; `rewards` are granted on the first clear. A loss, an escape, a win past the turn limit and a win that broke a party rule each fail the challenge with their own reason.

## `dialog.toml`

```toml
[lines.guide_intro]
speaker = "Tower Guide"
text = """
A tower rose from the mist where
nothing stood before.

Will you hear what it asks?"""
choices = [
    { label = "Tell me.", set_flags = ["tower_listened"] },
    { label = "Who are you?", reply = "Only one who watches the mist.", next = "guide_more" },
]
```

Each line of `text` is one dialog-box row and a blank line starts a new page. Rows are short: no retail text surface wraps ([`dialog-font.md`](dialog-font.md)). `speaker` is metadata for journals and tools; the box shows only the text, as retail boxes carry no name plate.

Text is the translation pipeline's markup: printable ASCII as itself, `{xx}` for any byte, `{op:arg}` for a two-byte token (`{c1:00}` is party member 0's name, `{cf:31}` a colour change), with typographic quotes, dashes and the ellipsis folded onto their ASCII glyphs. `dialog::encode_markup` is the dialog-segment subset of `legaia_translate::markup::encode`: the same grammar and the same bytes, without the patcher stack. Bytes `0x00..=0x1F` are refused as a token's opcode, since they would end the segment; the symbol aliases (`{btn:x}`) are not part of the subset.

A line may end on a menu of two to four `choices`. Each choice has a `label`, an optional `reply` (rows and pages as `text`), pack flags it raises (`set_flags`) and clears (`clear_flags`), a `next` line shown once the talk ends, and a fight (`battle`, a formation id, or `boss`) started once the talk and any chained lines are done.

### Dialogue on the inline runner

`World::expansion_say(line_id)` compiles the line (`dialog::compile_line`) into the inline interaction-script bytes an NPC talk hands the field VM's dialogue runner (`World::start_inline_dialogue`), so a pack line types, pages and opens its menu through the same pager and picker as a retail line ([`mes.md`](mes.md#multi-segment-box-packing)):

```text
page    = row+                       row = 0x1F <encoded markup> 0x00
text    = page (0x24 page)*          0x24: next page, same box
line    = text 0x21                  no menu: the talk ends
        | text <open> j0..jN-1 label*N branch*N
  open  = 0x27 / 0x28 / 0x29         two / three / four options
  jK    = i16 LE, relative to jK's own offset, to branch K
  label = 0x1F <encoded label> 0x00  immediate-labels form
  branch= [text] 0x21                the option's reply, then the end
```

The runner records the committed option on the pack line's own run (matched by the identity of the compiled buffer), and the world tick that sees the run finished applies the option's flags and queues its `next` line. A quest's `start_dialog`, `complete_dialog` and `active_dialog` lines queue the same way and start when no dialogue owns the player. A fight a choice or a quest start names waits behind them: once no line is pending or queued and the player is walking the field or the world map, the world tick starts it through `World::start_expansion_battle` or `World::start_expansion_boss`.

## Validation

Both sections implement `validate::SectionRules` ([`expansion-pack.md`](expansion-pack.md#validation)). `quests.toml` declares `quest` and `challenge` ids; `dialog.toml` declares `line` ids. The checks:

| Check | Severity |
|---|---|
| A prerequisite quest, line, formation or boss that is not defined | error |
| A line that does not compile (no text, a menu outside 2..=4 options, text with no glyph) | error |
| A quest or choice naming both a battle and a boss | error |
| A quest requiring itself; `reach_level` outside 1..=99; a turn limit or enemy scale of 0; `solo` past slot 3 | error |
| An item or monster id out of range | error |
| A quest with no giver and no `auto_start`; repeat settings on a one-shot quest; a count of 0; a row over 36 characters | warning |

## Runtime

The tracker (`QuestRuntime`) is a deterministic state machine: the same pack, progress and events give the same notices, grants and progress bytes. It reads world state through `QuestWorld` (party levels, bag counts, system flags, the play clock, named formations) and consumes `QuestEvent`s:

| Event | Raised by |
|---|---|
| `BattleEnded` | `World::finish_battle`, with the seated formation, its monsters and its boss byte. |
| `SceneEntered` | `World::set_active_scene_label`, on every scene entry. |
| `Talked` | `World::expansion_talked`: a map's `talk` trigger, and every talk to a retail NPC (`World::trigger_field_interact`). |
| `ModeWon` | `World::expansion_mode_won`, from a pack mode's host. |
| `CounterAdd`, `FlagSet`, `FlagCleared` | Scripts, dialogue choices and hosts. |
| `Refresh` | Every world tick outside battle. |

An NPC id is a pack's own name (`tower_guide`, raised by a map trigger) or a
**retail NPC**: `<scene>:<slot>`, the CDNAME scene and the placement slot the
touch post engages (`town01:12`, `World::retail_npc_id`). Every talk to a
retail villager reports its id while a pack is active, so a quest can name
one as a giver or a `talk` objective; the retail talk runs unchanged and a
pack line it raises waits for that dialogue to close. The engine logs each
id at debug level (`expansion: talked to retail NPC ...`), which is how a
modder reads off a villager's slot.

The per-tick `Refresh` re-tests state objectives and prerequisites, which is how a level up, an item gained or a story flag set reaches quests without a hook on each of those paths. After each event the tracker starts the quests whose giver was talked to, completes the quests whose objectives are done, and auto-starts what may start, repeating until nothing moves so a completion's flags can open the next quest of a chain in one call.

The tracker queues `QuestNotice`s for the host (started, objective updated, completed, challenge armed, cleared, failed; `World::take_expansion_notices`) and `Grant`s the `World` applies at once. `QuestRuntime::journal` lists the started quests with each objective's progress and target; the pause menu's Journal is built on it (`legaia_engine_expansion::journal`, [`field-menu.md`](../subsystems/field-menu.md#expansion-pack-journal-engine-extension)).

Each notice also becomes a **toast** both hosts draw on the banner rows (`legaia_engine_expansion::toast`, `World::expansion_toast`): `Quest started: <title>`, `<title>: 2/3` (or `objective 2/3 done` for a one-step objective), `Quest complete: <title>`, and the challenge outcomes. An objective that completed its quest in the same step shows as the completion alone. A reminder shows no toast; its line is the reminder.

## Saves

The save carries one pack's progress as the **expansion block**, a byte string `quest_save` owns:

```text
4      magic b"LXP1"
str    pack id
str    pack version (the version the progress was last written under)
u16    quest count, then per quest:
         str id, u8 status (0 active, 1 completed), u32 completions,
         u32 last_completed_at, u16 objective count, u32 progress each
u16    flag count, then str each
u16    counter count, then str id + i64 value each
u16    challenge count, then str id + u32 best + u32 clears each
[tail] zero or more tagged records, each u8 tag + body:
         1 = cleared bosses: u16 count, then str each
         2 = map: str map id, i16 x, i16 z, u16 facing
         3 = cards: u8 flags (bit 0: starter handed over), u64 card-stream
             state, u16 count then str id + u32 copies each (binder order),
             u16 count then str booster id + u32 unopened each,
             u16 deck count then str each, u16 seen count then str each
```

`str` is a `u16` byte length then UTF-8; integers are little-endian; maps are written in key order, so equal progress encodes to equal bytes. An armed challenge, queued notices and the line on screen are not saved.

`legaia_save` stores the block opaquely as `SaveExtV2::expansion`. An `LGSF` file carries it as the optional `LGXD` block after `LGXC`; a retail SC block carries it as a third `u16`-length segment of the `LGXE` engine-ext tail, written only when it fits whole after the `LGX2` and `LGX4` bodies ([`save-screen.md`](../subsystems/save-screen.md)). A save made with no pack writes neither, so its bytes are the bytes the engine wrote before the block existed.

Each tail record is written only when it carries something, so a block with none of them is the layout an older writer made, and such a block loads with an empty list, no map and no cards:

| Tag | Record | Written when | Owner |
|---|---|---|---|
| `1` | Cleared bosses | A once-only `battle.toml` boss is beaten | The battle runtime's `cleared_bosses` |
| `2` | Map | The save is made on a pack map | `map::MapLocation` ([maps](expansion-pack-maps.md#saving-on-a-map)) |
| `3` | Cards | The player has any card state | `CardProgress` ([cards](../subsystems/expansion-modes.md#the-card-economy)) |

The reader takes records until the block ends; a tag it does not know, or a block with bytes left over, decodes to nothing (`ExpansionProgress::decode` answers `None`), so a new record kind needs a tag the reader learns at the same time.

`World::activate_expansion` is the one activation path: it compiles the battle tables and brings the quest runtime onto the pack, adopting a dormant block saved for it and handing its cleared bosses to the battle runtime. `World::deactivate_expansion` folds the cleared bosses into the progress and parks it as the dormant block, so a re-activation picks it up.

`WorldSaveState::save_full` writes the active pack's progress; `load_full` restores it when the block's pack id is the active pack's. A block for any other pack, or one loaded with no pack active, is kept **dormant** and written back verbatim on the next save, so playing a session without the pack does not erase its progress; activating that pack later adopts the dormant block. A save without a block loads under a pack as fresh progress.
