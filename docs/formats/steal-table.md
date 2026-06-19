# Steal-item table

What the player steals from an enemy with the **Evil God Icon** equipped is
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

## Parser

`legaia_asset::steal_table::StealTable::from_scus` resolves the table from a
`SCUS_942.54` image (PSX-EXE `t_addr` → file-offset map, identical to the
[item-name table](item-table.md) resolver). `entry(monster_id)` returns the
`[chance, item]` pair; `steal_item(monster_id)` returns the item only when the
entry is stealable. The disc-gated `steal_table_real` test pins the
Skeleton→Incense anchor plus a span of ids against the real executable.
CLI: `asset steal-table <SCUS> [--all] [--json]` (the stolen item id is joined
to its name).

The randomizer (`legaia_rando::steal`) edits this table on a user-supplied disc
to reassign steal items; see [`randomizer.md`](../tooling/randomizer.md).

## See also

- [Item-name table](item-table.md) - the id space this table's `steal_item_id` indexes.
- [Spell table](spell-table.md) - the sibling static `SCUS_942.54` table.
- [`reference/gamedata.md`](../reference/gamedata.md) - the curated ground-truth enemy drop/steal tables.
