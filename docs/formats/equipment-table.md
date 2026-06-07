# Equipment stat-bonus table

A static `SCUS_942.54` table giving every equippable item (weapon / body armor /
head accessory / footwear) its passive stat bonuses, the characters that can
equip it, and its slot. It is the equipment analogue of the
[item-effect descriptor table](item-effect-table.md): both hang off the shared
[item property table](item-table.md) (`DAT_80074368`) and are reached through
the item's `+1` byte, which is overloaded per `kind` (effect subtype for
consumables, bonus-table index for equipment).

## Indexing (Ghidra-traced)

From the equip-effect aggregator `FUN_801CF650`
(`ghidra/scripts/funcs/overlay_menu_801cf650.txt`), which walks a character's
five equipment slots and sums their bonuses into the equip-screen preview:

```text
kind        = item_table[id].byte(+0)            ; DAT_80074368[id*0xC]; 1 = equipment
bonus_index = item_table[id].byte(+1)            ; DAT_80074369[id*0xC]
record      = (&DAT_80074F68)[bonus_index * 8]   ; stride-8 record
; applied only when kind == 1; record[+0..+4] sum into five stat accumulators
; (DAT_801EF09C / 08C / 090 / 094 / 098).
```

## Record layout (8 bytes, stride `0x8`)

| | |
|---|---|
| Record base | `DAT_80074F68` (file `0x64F68`) |
| Stride | `0x8` bytes |
| Index | item property `+1` byte (per equippable id) |

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | stat bonus 0 - the head-gear stat (set by head accessories) |
| `+1` | u8 | **attack** bonus (weapons' only field; boots also add a small amount) |
| `+2` | u8 | **defense-up** (`UDF`) bonus (body armor + head accessories) |
| `+3` | u8 | **defense-down** (`LDF`) bonus (body armor + boots) |
| `+4` | u8 | stat bonus 4 - the footwear stat (only boots/shoes set it) |
| `+5` | u8 | constant `0x40` |
| `+6` | u8 | **equip character mask**: bit `1` Vahn/Meta, `2` Noa/Terra, `4` Gala/Ozma; `7` = any |
| `+7` | u8 | **slot type** (`& 0x60`: `0x00` body, `0x20` head, `0x40` weapon, `0x60` footwear) + bit `0x01` = Ra-Seru |

### What is pinned vs. best-effort

The `+1` / `+2` / `+3` fields are **byte-exact** against the curated gamedata:
every weapon's `+1` equals its `attack`, every body armor's `+2`/`+3` equal its
`udf`/`ldf`. The `+6` mask matches each item's `equip_best` / `equip_others`,
and the `+7` slot byte cleanly partitions weapons / body / head / footwear with
the Ra-Seru upgrade flag. The `+0` and `+4` fields are the remaining two
battle-stat bonuses (the agility / speed pair; `+0` appears only on head gear,
`+4` only on footwear) - the curated tables don't carry those per item, so they
are exposed raw rather than named to a guessed stat.

Note that boots/shoes spread bonuses across `+1` (a small attack bump), `+3`
(`LDF`), and `+4`, so a walkthrough that lists only "two defense numbers" for a
boot is reading `+1` and `+3`, not `+2`/`+3` - the byte positions here are the
ground truth.

## Provenance + parser

`legaia_asset::equip_stats::EquipStatTable::from_scus` resolves the property +
bonus tables from a `SCUS_942.54` image at runtime (`t_addr -> file-offset` map,
identical to the [item-name table](item-table.md) resolver). The disc-gated
`equip_stats_real` test pins the attack / defense bytes, equip masks, and slot
types against the real executable and the curated gamedata.

## See also

- [Item property / name table](item-table.md) - the shared table this indexes through.
- [Item-effect descriptor table](item-effect-table.md) - the consumable sibling reached through the same `+1` byte.
- [`reference/gamedata.md`](../reference/gamedata.md) - curated weapon/armor/accessory stat tables.
