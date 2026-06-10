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
| `+0` | u8 | **intelligence** (`INT`) bonus (head accessories set it) |
| `+1` | u8 | **attack** bonus (weapons' only field; boots also add a small amount) |
| `+2` | u8 | **defense-up** (`UDF`) bonus (body armor + head accessories) |
| `+3` | u8 | **defense-down** (`LDF`) bonus (body armor + boots) |
| `+4` | u8 | **speed** (`SPD`) bonus (only boots/shoes set it) |
| `+5` | u8 | constant `0x40` |
| `+6` | u8 | **equip character mask**: bit `1` Vahn/Meta, `2` Noa/Terra, `4` Gala/Ozma; `7` = any |
| `+7` | u8 | **slot type** (`& 0x60`: `0x00` body, `0x20` head, `0x40` weapon, `0x60` footwear) + bit `0x01` = Ra-Seru |

### Which stat each `+0..+4` byte targets

The five accumulators the aggregator sums into are pre-loaded from the active
character record by `FUN_801CF5D0`
(`ghidra/scripts/funcs/overlay_shop_save_801cf5d0.txt`) before the equipment
bytes are added. Reading those record load offsets pins each byte's stat target.
The aggregator's record base is `0x80084140 + idx*0x414`; the live character
record is `0x80084708 + idx*0x414` (i.e. `+0x5C8` further), whose live-stat
block is `(AGL, ATK, UDF, LDF, SPD, INT)` at `+0x110..+0x11B` (pinned in
`legaia_save` by the "Max AGL / Max ATK / ..." GameShark cheats):

```text
equip +0  ->  DAT_801EF09C  <- record +0x6E2  =  char +0x11A  =  INT
equip +1  ->  DAT_801EF08C  <- record +0x6DA  =  char +0x112  =  ATK
equip +2  ->  DAT_801EF090  <- record +0x6DC  =  char +0x114  =  UDF
equip +3  ->  DAT_801EF094  <- record +0x6DE  =  char +0x116  =  LDF
equip +4  ->  DAT_801EF098  <- record +0x6E0  =  char +0x118  =  SPD
```

So equipment modifies ATK / UDF / LDF / SPD / INT and **never** AGL (the AGL
accumulator `DAT_801EF088` takes no equipment add). The earlier "agility /
speed pair" reading of `+0`/`+4` is **falsified**: `+0` is the INT bonus
(head gear), `+4` is the SPD bonus (footwear).

### What is pinned vs. best-effort

All five `+0..+4` stat *targets* are **pinned** from the accumulator ->
record-offset mapping above. The `+1` / `+2` / `+3` magnitudes are additionally
**byte-exact** against the curated gamedata (every weapon's `+1` equals its
`attack`, every body armor's `+2`/`+3` equal its `udf`/`ldf`). The `+6` mask
matches each item's `equip_best` / `equip_others`, and the `+7` slot byte cleanly
partitions weapons / body / head / footwear with the Ra-Seru upgrade flag. The
curated tables don't carry per-item SPD/INT bonuses, so the `+0`/`+4` magnitudes
aren't cross-checked against an external source, but their stat targets are fixed
and the disc-gated test asserts the slot invariant (INT only on head gear, SPD
only on footwear).

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

`EquipBonus::equips_party_slot(party_slot)` maps a party slot (`0` Vahn, `1`
Noa, `2` Gala) to the mask bit `1 << party_slot`, matching the retail
equip-screen gate (`a3 = 1 << char_index`).

## Engine consumption

`legaia_engine_core::equipment::DiscEquipInfo` lifts the `+6` character mask and
the `+7` slot category off the parsed table, keyed by real item ids:
`can_equip(id, party_slot)` answers the per-character equip gate, `category(id)`
returns the disc slot category. The equip session
(`legaia_engine_core::equip_session::EquipSession::new_with_restrictions`)
filters each character's per-slot item list on it - a Vahn-only weapon no longer
appears in Noa's weapon picker. For the four UI slots the `+7` byte resolves
cleanly (weapon / body armor / helmet / boots) the list is also category-gated;
the helmet/ring/accessory/hand-guard slots collapse to the disc "head" category
(see below), so they are mask-gated only. The disc-gated
`equip_modifiers_disc::disc_equip_restrictions_gate_equip_session_item_list`
test drives the whole chain on the real executable.

**Slot model (resolved):** the four `+7` categories *are* Legaia's four
armour/weapon equip slots — there is no missing "8-slot" disambiguation. Cross-
referencing the parsed table against the curated [gamedata](../reference/gamedata.md)
by item name pins it exactly:

| `+7` category | gamedata slot | count (disc = gamedata) | example items |
|---|---|---|---|
| `0x40` Weapon | weapon | 50 (incl. Ra-Seru tiers) ≥ 27 curated | Survival Knife, Mace |
| `0x00` Body | armor | **20 = 20** | (every body armour) |
| `0x20` Head | helmet | **15 = 15** | Warrior Seal, Royal Crown, Ra-Seru Helmet |
| `0x60` Footwear | shoes | **16 = 16** | (every greave / shoe) |

The disc "Head" bucket is *exactly* the 15 gamedata helmets (Legaia's
seals / clips / crowns / bands / earring / helmet / plume), not a helmet +
accessory mix — so there is no "helmet vs. ring vs. accessory" collision to
resolve. (Weapon is the only non-1:1 category, because the disc enumerates the
upgradeable Ra-Seru weapon as ~24 per-tier entries that the curated table
collapses; every disc Weapon-slot item is still a weapon.) **None of the 77
accessories ("Goods") appear in this table at all** — they are a separate
system, so the `+7` byte was never meant to classify them; where the accessory
records live is a distinct open thread, not a `+7` disambiguation problem.
Pinned by the disc-gated `legaia-gamedata` test `equip_slots_vs_disc`.

## See also

- [Item property / name table](item-table.md) - the shared table this indexes through.
- [Item-effect descriptor table](item-effect-table.md) - the consumable sibling reached through the same `+1` byte.
- [`reference/gamedata.md`](../reference/gamedata.md) - curated weapon/armor/accessory stat tables.
