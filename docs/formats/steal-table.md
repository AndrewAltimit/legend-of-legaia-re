# Steal-item table

What the player steals from an enemy with the **Evil God Icon** equipped - once
per battle, from the first monster a party member fells, see
[the steal attack](#the-steal-attack) - is
looked up in a static per-monster table inside `SCUS_942.54` - **not** in the
PROT 867 `battle_data` monster record. An exhaustive offset scan of the decoded
record (correlated against ground-truth steal data) finds no steal field there:
the reward block at `+0x44..+0x49` holds only gold / exp / drop. The steal item
lives in this separate executable table, which is exactly why every record-only
search came up empty (the long-open thread in
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

## Table base + record layout

| | |
|---|---|
| Base address | `DAT_80077828` (file offset `0x68028` in `SCUS_942.54`) |
| Index form | `DAT_80077828 + monster_id*2` (1-based monster id) |
| Stride | `0x2` bytes |
| Id range | monster ids `1..` (entry `0` is a reserved sentinel) |

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | `steal_chance_pct` - steal success chance, percent |
| `+1` | u8 | `steal_item_id` - stolen item id (the [item-table](item-table.md) id space; `0` = none) |

The id space is the same one [`item-table.md`](item-table.md) names and a
monster record's `drop_item` indexes, so a raw steal id becomes a readable name
the same way (`0x8a` → `Incense`). A `steal_chance_pct` of `0` (or a `0` item)
means the enemy can't be stolen from.

**Field order is chance-then-item** - the reverse of the drop fields in the
monster record (`+0x48 item / +0x49 chance`). Reading the table with the
record's item-first order shifts every chance by one monster, so the order
matters.

## Provenance

Pinned from a live player-steal RAM capture (Evil God Icon equipped; the
`player_steal_skeleton_pre` / `_banner` save pair): the Skeleton (monster id
`13`) entry at `0x80077842` is `1e 8a` = **30% Incense**, exactly the steal the
banner shows. The base, stride, and `[chance, item]` order are confirmed
byte-exact against the complete published steal table (item **and** chance
columns) across every resolvable monster id - zero mismatches. The table is
static rodata in the executable's data segment, resident in RAM and identical to
the file bytes, so it resolves the same way as the
[item](item-table.md)/[spell](spell-table.md) name tables.

## Who reads it

Two routines on the disc read the table, and nothing else does: the
`lui`/`addiu` pair forming `0x80077828` occurs exactly twice across
`SCUS_942.54` and every PROT entry (`find-gp-relative-refs.py --va 0x80077828
--prot`), at `0x8004B53C` in `SCUS_942.54` and at `0x801F79F0` in PROT 0941.

### The steal attack

The player's steal is **not** a command and does not happen on the hit. It
lives in one arm of the battle anim commit `FUN_8004AD80`
(`0x8004B29C..0x8004B65C`, see `ghidra/scripts/funcs/8004ad80.txt`; the
routine's whole row is in [`functions/battle.md`](../reference/functions/battle.md)), which
runs when a monster seat (`>= 3`) commits the end of its **knockdown** clip -
the installed entry's tag byte is `4` (`0x8004B0A4`) - with live HP
`+0x14C == 0`. So the steal lands as the slain monster finishes falling, and
the table is read only for a monster that died.

The arm first checks the monster's cell of the stolen band `0x801C8FE0`
(below). With the cell empty it runs the steal attack:

| Order | Test | Site |
|---|---|---|
| 1 | acting seat `ctx[+0x13] < 3` (a party member holds the action) | `0x8004B3C8` |
| 2 | latch `ctx[+0x27] == 0`, then **set it to 1** | `0x8004B3D4..0x8004B3E4` |
| 3 | `mult = 2` if any member whose `+0x14C != 0` carries record `+0xF8` bit `0x20000` (passive `0x31`, Items Up), else `1` | `0x8004B3E8..0x8004B484` |
| 4 | special-battle word `_DAT_8007BAC0 == 0` | `0x8004B488` |
| 5 | acting seat's action category `+0x1DE == 3` (Attack) | `0x8004B4BC` |
| 6 | acting member's record `+0xF4` bit `0x10000` (passive `0x10`, Steal Attack - the Evil God Icon) | `0x8004B500` |
| 7 | `rand() % 100 < chance * mult` (unsigned `sltu`) | `0x8004B514..0x8004B584` |
| 8 | the bag holds fewer than `99` of the item (`FUN_80042F4C`) | `0x8004B5BC..0x8004B5D0` |

Three consequences follow from the order. The latch is set at step 2,
**before** the killer is checked at all, and its only writer on the disc is
that one `sb` (a byte scan of `SCUS_942.54`, the battle overlay and every
cast / summon module finds no other store to `+0x27`, zero or otherwise) - so
a battle gets **one** steal attempt, and the first monster any party member
fells spends it whether or not the killer could steal. Items Up doubles the
chance for the whole party, not only for its wearer. And a stack already at
`99` drops the steal silently: no caption, no item.

On success the caption is composed into `0x80077A08`: template
`0x80077A64` copied by `FUN_8003CA78`, the item-name token `{0xC2, id}`
appended by `FUN_8003CB54`, the two-byte tail `0x8007B698` appended by
`FUN_8003CAC4`. The arm stores the buffer into `0x800774AC` - the `+0x14`
payload pointer of screen-element record 91 (`0x80077498`) - raises HUD
element `0x5B` (`FUN_801D8DE8(0x5B, 0)`), writes `ctx[+0x18] = 0x5B`, and adds
the item with `FUN_800421D4(id, 1)`. Record 91 is the full-width bar that
slides from `(16, 236)` up to the active-actor bar's seat `(16, 194)`, width
288 - the bottom bar the `player_steal_skeleton_banner` capture shows the line
in.

### The stolen band - PROT 0941 and the thief's return

PROT 0941's enemy Steal (`0x51`, `FUN_801F730C`) is the band's writer: the
item a thief took lands at `0x801C8FE0 + (seat - 3) * 4` (`0x801F7960` for a
bag steal, `0x801F7A84` for a steal off a monster seat, which also bumps
`0x801C8FE0 + (seat + 5) * 4` at `0x801F7AF0`). When that thief dies, the same
`FUN_8004AD80` arm hands the item over instead of rolling a steal attack:
`FUN_800421D4(item, 1)`, the cell cleared, and a caption built the same way
from `0x80077A4C` + tail `0x8007B694` (loot taken from the party) or
`0x80077A38` + tail `0x8007B690` (loot taken from a monster). When
`ctx[+0x18]` already reads `0x5B`, the pointer is swapped for the whole-line
`0x80077A70` caption instead (`0x8004B35C..0x8004B378`).

The port runs both arms in `legaia_engine_core::battle_steal`, reached from
the knockdown-end commit both hosts tick; the templates are read off the
user's executable through `legaia_asset::battle_ui_strings`. The disc-gated
`steal_attack_round_disc` test drives a real Skeleton fight through the live
round and asserts the item lands and the caption is raised, with a contrast
pass that spends the attempt without the passive and grants nothing.

## Parser

`legaia_asset::steal_table::StealTable::from_scus` resolves the table from a
`SCUS_942.54` image (PSX-EXE `t_addr` → file-offset map, identical to the
[item-name table](item-table.md) resolver). `entry(monster_id)` returns the
`[chance, item]` pair; `steal_item(monster_id)` returns the item only when the
entry is stealable. The disc-gated `steal_table_real` test pins the
Skeleton→Incense anchor plus a span of ids against the real executable.
CLI: `asset steal-table <SCUS> [--all] [--json]` (the stolen item id is joined
to its name).

The randomizer (`legaia_patcher::steal`) edits this table on a user-supplied disc
to reassign steal items; see [`randomizer.md`](../tooling/randomizer.md).

## See also

- [Item-name table](item-table.md) - the id space this table's `steal_item_id` indexes.
- [Spell table](spell-table.md) - the sibling static `SCUS_942.54` table.
- [`reference/gamedata.md`](../reference/gamedata.md) - the curated ground-truth enemy drop/steal tables.
