# Expansion pack: Tactical Arts and Seru magic

`arts.toml` adds new Tactical Arts to an [expansion pack](expansion-pack.md), and chains that end a turn's arts in a pack finisher; `spells.toml` adds new Seru magic. The parsers are `legaia_expansion_format::{arts, spells}`; the runtime is `legaia_engine_expansion::{arts, spells}`, wired to the `World` by `engine-core`'s `world::expansion_arts`. Every hook is the identity with no pack active (or a pack with neither section): the arts input walks retail's bank alone, the art bank gets no extra clip, the spell catalog no extra id, and a save carries no arts record.

## At a glance

| Property | Value |
|---|---|
| Files | `arts.toml`, `spells.toml` at the pack root; both optional |
| Declares | `art`, `art_chain`, `spell` ids |
| References | `battle.toml` monsters (`learn.capture`), bosses, quests and flags (`requires`), items (`learn.item`) |
| Animation | The hero's own retail art clips, by action constant or name; no animation data is carried |
| Storage | Learned set beside the character records; tail record `5` of the [expansion block](expansion-pack-quests.md#saves) |
| Shown in | The battle arts list (Triangle), the battle label, the battle and field Magic lists, the Status page's arts screen (Triangle), the Journal's Arts page |
| Sample | [`packs/rebirth/arts.toml`](../../packs/rebirth/arts.toml), [`packs/rebirth/spells.toml`](../../packs/rebirth/spells.toml) |

## How a pack art runs

Retail's arts queue ([battle-action-queue.md](../subsystems/battle-action-queue.md)) holds direction swings `0x0C..=0x0F`, the starters `0x19` / `0x1A` and the character's art constants `0x1B..=0x32`. The anim commit `FUN_8004AD80` turns any staged byte `>= 0x10` into art-bank record `byte - 0x10` ([art-data.md](art-data.md#action-constants)). Noa uses all 24 constants, so a pack art cannot take one.

A pack art therefore lives on the queue bytes above them. Each **segment** of each pack art of a character takes one byte from `0x33` up, in key order, below the queue's mark bit (`0x7F` at most: 77 clips per character). At battle entry the engine appends to the hero's art bank, at record `byte - 0x10`, a copy of the segment's retail clip - keyframes, effect script, cue track - with the pack's power bytes and beats over the clip's own entry head (`+0x00..+0x04`, `+0x10..+0x14`). The strike loop stages the byte like any other, the commit plays the copy, and the damage kernel's hit gate fires on its beats.

The commit latches `+0x1DB` with the **retail constant** the segment borrows, not the pack byte. Every reader of the latch - the per-art attack camera ([battle-attack-camera-table.md](battle-attack-camera-table.md)), the art record's side data, the slow-motion arms - therefore treats a pack segment as the retail art whose clip it plays. The pack art itself is remembered per actor (`ArtsRuntime::playing`) for the label and the hit seams below.

### Matching: a second walk after retail's

The queue builder `FUN_801EED1C` normalises the entered arrows with one walk over the character's bank ([`legaia_art::tokenize_bank`](../../crates/art/src/tokenize.rs)): for each start position from the tail, every record from `0x1B` up is tried, and a normal record's match writes the `0x19` starter over the run's last arrow and its constant after it. A full match of a one-arrow record ends the walk at that start (`0x801EF420`), and every Super Art record on the disc carries a one-arrow stub combo (`Down` for all of Vahn's) - so a record placed after them would be cut off by any input holding that arrow.

Known pack arts therefore walk **second** (`legaia_art::tokenize_bank_over`): the same walk, over the queue the retail walk produced, with a bank whose retail rows are blank and whose pack rows sit at the index of each art's first byte, past retail's 24. Only arrows the retail walk left can match, so a retail art matches exactly as without a pack, and with no pack art known the second pass does not run. A pack art's further segments are inserted behind its first byte after the retail finish (Miracle replacement, Super tail-replace). `R U R D U` for Rebirth's Ember Cascade on Vahn:

```text
entered     R  U  R  D  U
swings      0D 0F 0D 0E 0F
retail walk 0D 0F 0D 0E 0F             no retail art
pack walk   0D 0F 0D 0E 19 33          starter over the last arrow, first byte
segments    0D 0F 0D 0E 19 33 34 00    second segment (Somersault's clip)
```

Two rules follow from the order, which the validator cannot check without the disc's combos:

- A pack art whose commands equal a retail art's never fires: the retail walk takes the arrows first.
- A pack combo that **contains** a retail combo loses those arrows to the retail art. Rebirth's combos avoid every retail combo of their hero as a substring.

The proof is `world::tests::expansion_arts`: every retail combo, alone and behind a leading arrow, builds the same queue, starter marks and art list with the pack active and its arts known as with no pack, over a bank shaped as the disc's (the Super stubs included); `expansion_arts_disc` runs the same build on the disc's own bank in a real battle.

### Chains

`[chains.<key>]` is the pack's Super Art: when the arts a turn performs (retail constants and pack arts, in queue order) **end with** the chain's `arts`, the queue gets `0x1A` and the finisher's segment bytes appended. The `0x1A` carries the Super tail-replace's starter mark (`4`), so its commit raises the same `SUPER` banner and freeze a retail Super Art does. The pass runs after the retail finish, so a retail Super Art resolves first and a chain reads what it left; the longest matching chain wins. The Rebirth chain on Vahn, `R U R D U` then `L R L` (Hyper Elbow), Hyper Elbow already known:

```text
0D 0F 0D 0E 19 33 34 0C 0D 19 29 1A 35 00
               ^^^^^       ^^^^^ ^^^^^
           Ember Cascade  Hyper  Rebirth Blaze
```

A finisher with no `commands` is reached only through its chain.

### What it costs

Retail stores no per-art AP: the builder computes `multiplier x arrows` and charges the Spirit gauge `actor[+0x170]` ([arts-command-gauge.md](../subsystems/arts-command-gauge.md#what-an-art-costs-in-ap)). A pack art states its cost (`ap`); the engine charges the sum of the pack arts a turn performs, a chain's finisher included, halved under AP Used Down (`+0xF8 & 0x800`) as retail's are. The arrows themselves are paid from the entry pool as for any input.

### Damage

A hit's power byte is the segment's (`power`), read by the damage kernel exactly as a retail art's: `U12..U28` strike the upper defence, `L12..L28` the lower, and the multiplier is one of `12, 18, 20, 22, 28` ([art-data.md](art-data.md#power-encoding)). The hit count is the borrowed clip's beats (at most four), or the segment's `beats`. On top of the roll:

| Seam | What the pack sets | Where |
|---|---|---|
| Damage percent | `damage_pct` (`1..=400`) of each landed hit, 9999 cap | `World::expansion_scale_art_hit` |
| Element | `element` (`0..=6`): the hit scales by the affinity matrix `[element][target element]` | same |
| Rider | `rider = { status, chance }`: one `rand()` per damaging hit on a monster | `World::expansion_art_hit_rider` |

## `arts.toml`

```toml
[arts.ember_cascade]
character = "vahn"
name = "Ember Cascade"
commands = "RURDU"
ap = 30
segments = [
    { art = "Pyro Pummel", power = ["U22", "L22"] },
    { art = "Somersault", power = ["L28"] },
]
element = 2
learn = { level = 55, requires = { bosses = ["cinder_warden"] } }

[chains.rebirth_blaze]
character = "vahn"
arts = ["ember_cascade", 0x29]
finisher = "rebirth_blaze"
```

### `[arts.<key>]`

| Key | Default | Meaning |
|---|---|---|
| `character` | `vahn` | `vahn`, `noa` or `gala` |
| `name` | - | The label and list name, at most 20 characters |
| `description` | empty | The Journal line; `\|` breaks it |
| `commands` | empty | Arrows `L R U D` (spaces, commas and dashes ignored), `2..=9` of them; empty for a chain-only finisher. Vahn and Gala swing the weapon on Left and the Ra-Seru arm on Right, Noa the other way round |
| `ap` | `0` | Spirit cost, `0..=100` |
| `segments` | - | `1..=8` retail clips, in order |
| `damage_pct` | `100` | Percent of each hit |
| `element` | none | `0` earth, `1` water, `2` fire, `3` wind, `4` thunder, `5` light, `6` dark |
| `rider` | none | `{ status = "venom", chance = 40 }`; statuses as [items](expansion-pack-items.md#effects) name them |
| `learn` | known at once | [Learning](#learning) |

A segment is `{ art, power, beats }`: `art` names the retail art whose clip plays, by action constant (`0x1B..=0x32`) or name (`"Somersault"`, matched against the character's art table, case-insensitive); `power` lists up to four power bytes, raw (`0x1F`) or labelled (`"L28"`), in place of the clip's own, index by index; `beats` lists up to four ascending clip frames the hits land on, in place of the clip's own.

### `[chains.<key>]`

`character`, `arts` (one to six links: a retail constant, or a pack art key of the same character) and `finisher` (a pack art key of the same character).

## `spells.toml`

```toml
[spells.hearthfire]
name = "Hearthfire"
description = "Gola Gola, fed on|tower embers."
base = 0x8C
mp = 96
power = 210
growth = 8
element = 2
learn = { capture = "hearth_gola", requires = { bosses = ["cinder_warden"] } }
```

| Key | Default | Meaning |
|---|---|---|
| `name`, `description` | - / empty | Menu name (at most 20 characters) and the Magic window's line (`\|` breaks it) |
| `base` | - | The retail spell whose cast plays: a player Seru id `0x81..=0xA0` ([spell-table.md](spell-table.md)) |
| `mp` | the base's | MP cost |
| `power` | `100` | Percent of the base cast's rolled damage or restore |
| `growth` | `0` | Extra percent per magic level above 1 |
| `element` | the creature's | `0..=7`, the attacker row the affinity matrix reads |
| `target` | the base's | `one_enemy`, `all_enemies`, `one_ally`, `all_allies` |
| `status` | none | A rider each struck monster may take: `{ status, chance }` |
| `characters` | everyone | Who may learn it |
| `learn` | known at once | `capture` (a `battle.toml` monster key) plus the [learning](#learning) conditions |

### How a pack spell casts

A pack spell is cast **as its base**. Choosing its row arms the action SM's Magic band on the base spell id, so the band, the summon module (PROT `903..`, [cast-module.md](../subsystems/cast-module.md)), the creature, its camera and its effects run unchanged. The pack's numbers enter at the seams that take one, keyed on the cast in flight (`SpellRuntime::casting`, set at the dispatch, cleared by the next action and at the battle's end):

| Seam | Retail source | Pack |
|---|---|---|
| MP debit at state `0x28` | spell table `+3` | `mp` (`BattleActionHost::spell_mp_cost`) |
| Magic level the roll scales by | record `+0x161` byte of the base id (`FUN_801DD864`) | the pack spell's own level |
| Summon element | the creature's record element | `element` (`summon_attacker_element`, the summon branch of `FUN_801DD0AC`) |
| Magnitude | the summon roll / module restore | times `power + growth * (level - 1)` percent, 9999 cap |
| Status | the Seru side-effect stager | the rider, one `rand()` per struck monster |
| Spell XP | the base's record slot (`FUN_801E70BC`) | the pack spell's own level, on the base id's thresholds |
| Label | the spell table name | the pack name |

The shiny bonus of a base Seru the character holds does not apply to a pack cast.

### Menu ids

The battle and field Magic lists are lists of spell ids. While the pack is active each pack spell has a fixed menu id from `0xC0` up (above the spell table's last record, `0xBD`), in key order, and the spell catalog holds an entry for it: the base's, with the pack's name, MP and target shape. The battle Magic window draws the base's icon and the pack's description and level for that id. At most 63 pack spells fit the range. Deactivating the pack removes the entries.

### Seru capture

A pack monster is a retail monster with the pack's numbers ([battle](expansion-pack-battle.md)); a capture of it is a capture of its base id. At the battle's end the engine maps each captured id through the fight's formation (`CompiledFormation::variants`) to the pack monster it seated. When some pack spell's `learn.capture` names that monster, every present member who may learn the spell and meets its conditions learns it (a toast says so), and the capture is taken out of the retail registry's list, so the base Seru's own spell is not also banked. A capture of anything else goes to the retail registry as before. The base must be a monster the engine's capture paths can take (a Seru-type monster).

## Learning

`learn` is the same in both files: `level` (the character's own), `requires` (the [quest prerequisite](expansion-pack-quests.md#queststoml) shape: quests, pack and story flags, bosses, party level) and `item` (held, not used up). Every set condition must hold; an empty `learn` is known from the start. The engine re-checks after every quest step and battle and at activation and load (`World::expansion_refresh_learning`); a newly learned art or spell raises a toast. A spell with `learn.capture` is learned only by the capture, its other conditions gating it. Scripts and tests teach directly with `World::expansion_teach_art` / `expansion_teach_spell`.

A pack art matches the arts input only once known, and the battle arts list (Triangle) lists the known ones after the character's retail arts; with any known, the pages run on until every row has shown (`arts_list_pager_ext`, an engine extension), and with none the pager is retail's three-page one.

## Saves

Tail record `5` of the expansion block, written only once a pack art or spell is known:

```text
5      tag
u16    learned art count, then str key each (key order)
u16    known spell count, then u8 roster slot + str key + u8 magic level (1..=9) + u32 spell XP each
```

The arts and spells known are part of the pack's progress, so they save, park and adopt with it. A New Game starts them over; a New Game+ keeps them through `World::expansion_learned` before the reset and `World::expansion_carry_learned` after it (an art or spell the pack no longer defines is dropped; a spell keeps the higher level).

## Validation

| Check | Severity |
|---|---|
| Art: empty or long name, commands outside `L R U D` or shorter than 2 / longer than 9, two arts with one combo, AP over 100, no segment or more than 8, a segment art that is not the character's retail art, more than four power bytes or beats, a power byte outside `0x0C..=0x1F` / `U12..L28`, beats not ascending, `damage_pct` outside `1..=400`, element past 6 | error |
| A character's segments past its 77 clip slots | error |
| Chain: no links or more than six, a link or finisher of another character or not defined | error |
| An art with no commands that no chain names; beats and power of different lengths | warning |
| Spell: empty or long name, a base outside `0x81..=0xA0`, power outside `1..=500`, element past 7, more than 63 spells, a `capture` naming no pack monster | error |
| Spell power past 999% by level 9, two spells on one capture, an empty `learn` | warning |
| Rider chance outside `1..=100`, a learn level outside `1..=99`, a bad `requires` or `item` | error |

## Limits

- A pack art plays retail clips only; it cannot move a hero in a way no retail art of theirs does. A clip a hero's battle form lacks (a stripped equipment variant) is skipped with a warning and that segment plays nothing.
- The validator cannot see retail combos; the two matching rules above are the author's to keep. The workbench's Arts tab checks a combo against the disc's arts table.
- A pack spell keeps its base's presentation, except where its `target` widens a one-enemy base to `all_enemies`: there the cast runs as a player Seru whose own shape is every enemy - of the base's element first, then an elementless one (the catalog files captured Seru as neutral), then any - nearest the base in MP (`World::widened_presentation`; Gimard widens to Gola Gola), so the creature and camera strike the row the damage folds on. The roll the pack's `power` scales is that Seru's. With no such Seru the base's module plays, aimed at one.
- The pause menu lists a hero's known pack arts (name, commands and AP) under the saved chains of the arts screen Triangle opens from the Status page (`World::expansion_known_arts_lines`); the Journal's Arts page lists the known arts and spells. The port draws no retail Moves page for either to join.

## Rebirth's arts and Seru

[`packs/rebirth/arts.toml`](../../packs/rebirth/arts.toml) gives each hero one art learned on the climb (level 55, after the Cinder Warden) and one Rebirth finisher (level 80, after the Azure Seraph) reached by chaining that art into a retail one:

| Hero | Art | Commands | Clips | Finisher (chain) |
|---|---|---|---|---|
| Vahn | Ember Cascade, 30 AP, fire | `R U R D U` | Pyro Pummel U22 L22, Somersault L28 | Rebirth Blaze: Tornado Flame (Miracle) U28 L28 U28 L28, Power Slash L28, 140% (Ember Cascade, Hyper Elbow) |
| Noa | Gale Waltz, 30 AP, wind, Numb 20% | `L U L D U` | Mirage Lancer U22 U22, Swan Driver L28 | Rebirth Tempest: Super Tempest, Dragon Fangs, 140% (Gale Waltz, Sonic Javelin) |
| Gala | Iron Avalanche, 30 AP, thunder | `R U R D D` | Battering Ram L22, Super Ironhead U28 L28 | Rebirth Thunder: Thunder Punch (Miracle), Heaven's Drop, 140% (Iron Avalanche, Ironhead) |

Against retail ([`data/gamedata/arts.toml`](../../data/gamedata/arts.toml), [art-data.md](art-data.md)): a four- or five-arrow regular art costs 24-30 AP for one or two beats at `x20`-`x28`, a Super Art 54-66 AP for four ascending beats. The mid arts sit at the top of the regular tier with one beat more; the finishers cost 60 AP across the two arts and land five beats at `x28` and 140%, just above the Super tier, as the reward for chaining.

[`packs/rebirth/spells.toml`](../../packs/rebirth/spells.toml) adds four tower Seru, each taught by capturing its pack monster once that floor's keeper is beaten:

| Seru | Base (disc MP, target) | MP | Power, growth | Element, rider | Capture, gate |
|---|---|---|---|---|---|
| Hearthfire | Gola Gola `0x8C` (40, all) | 96 | 210%, +8 | fire | Hearth Gola (2F), Cinder Warden |
| Verdant Bloom | Iota `0x93` (90, all) | 140 | 180%, +6 | earth, Numb 15% | Verdant Iota (4F), Verdant Colossus |
| Grove Fang | Kemaro `0x90` (72, one) | 100 | 240%, +8 | earth, Venom 35% | Grove Kemaro (4F), Verdant Colossus |
| Tempest Dive | Barra `0x8F` (85, one) | 125 | 260%, +10 | wind | Storm Barra (6F), Iron Regent |

The strongest retail capturable Seru is Gilium (`0x95`, 160 MP, all enemies); the Ra-Seru evolutions Meta, Terra and Ozma cost 240. The tower Seru roll about twice their base for two to three times its MP, between the two.

## See also

- [Expansion packs](expansion-pack.md) - the format, ids and validation.
- [Art data](art-data.md) - art records, power bytes and the action constants.
- [Battle action queue](../subsystems/battle-action-queue.md) - the queue builder a pack art extends.
- [Arts command gauge](../subsystems/arts-command-gauge.md) - the two AP gauges.
- [Spell table](spell-table.md), [cast modules](../subsystems/cast-module.md) - what a pack spell's base runs.
- [Battle formulas](../subsystems/battle-formulas.md) - the summon roll a pack spell scales.
