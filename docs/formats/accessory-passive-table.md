# Accessory passive-effect table

The static data behind every accessory's ("Goods") passive effect - max-HP/MP
percent boosts, stat percent boosts, status-nullify guards, elemental guards,
AP / encounter / loot / escape modifiers. It completes the item-data triple:
the [item property table](item-table.md) (`DAT_80074368`) routes an id's `+1`
subtype byte either to the [equip stat-bonus table](equipment-table.md)
(`kind == 1`) or to the [item-effect descriptor table](item-effect-table.md)
(`kind == 2`), and the **passive effect** rides those same two records as a
third field.

There is **no separate per-accessory effect record**. The static side is a
**64-slot passive-effect index space** (`0x00..=0x3F`): one index = one
effect, and an equipped item's index becomes a **bit position** in the
per-character ability bitfield. The accessory's index is the descriptor's
`+3` byte - the byte previously documented as a "constant `0x41` (`'A'`)
marker", which is in fact this index's **no-passive sentinel** on consumable
rows. Likewise the equip record's `+5` byte - previously "constant `0x40`" -
is the equipment-side index slot, carrying the sentinel on every retail row.

## Indexing (Ghidra-traced)

From the per-frame stat aggregator `FUN_80042558`
(`ghidra/scripts/funcs/80042558.txt`, static `SCUS_942.54`), which walks each
active party member's eight equipment-slot bytes at char record `+0x196`:

```text
kind = item_table[id].byte(+0)                  ; DAT_80074368[id*0xC]
if kind == 1: index = equip_bonus[sub].byte(+5) ; DAT_80074F6D[sub*8]
if kind == 2: index = descriptor[sub].byte(+3)  ; DAT_800752C3[sub*4]
if index < 0x40:
    char[+0xF4 + (index>>5)*4] |= 1 << (index & 0x1F)
```

- `char +0xF4..+0x103` is the per-character 4×u32 **ability bitfield** (the
  word the MP-cost ability bits `0x10`/`0x20` = indices `0x04`/`0x05` live
  in - see `engine-vm::battle_formulas::mp_cost_after_ability_bits`).
- The aggregator then ORs all three members' bitfields into the **global**
  4×u32 mask at `DAT_80074358..0x80074368` (bit-tested by `FUN_800431D0`),
  which is how the party-wide passives are consumed.
- Index `>= 0x40` grants nothing. Retail data: every equip-bonus row carries
  `+5 = 0x40` (no equipment grants a passive; the `kind == 1` arm is latent)
  and every consumable descriptor carries `+3 = 0x41`. Only accessory and
  quest-item descriptor rows carry live indices.

The same gate (`descriptor +3`, `< 0x40`) is read by the menu overlay's Goods
detail panel `FUN_801D0F1C` (`ghidra/scripts/funcs/overlay_menu_801d0f1c.txt`)
and the static description resolver `FUN_80034250`
(`ghidra/scripts/funcs/80034250.txt`) to fetch the display text below.

## Passive name/description table (`0x8007625C`)

A static 64-record table gives each index its menu name, effect description,
and scope:

| | |
|---|---|
| Record base | `0x8007625C` (file `0x66A5C`) |
| Stride | `0xC` bytes |
| Record count | `0x40` (indices `0x00..=0x3F`) |

| Offset | Type | Field |
|---|---|---|
| `+0` | u32 | **scope**: `1` = party-wide (one wearer benefits the whole party), `0` = wearer-only |
| `+4` | u32 | pointer to the effect **name** string (e.g. `"HP Boost 1"`) |
| `+8` | u32 | pointer to the effect **description** string (e.g. `"Increase max HP 10%"`; `0x7C` `'|'` is the line break) |

Party-wide scope is set on exactly indices `0x30..=0x37` and `0x3B..=0x3F`
(the battle-end / encounter / escape modifiers).

## The 64 passive indices

Names/descriptions are the on-disc strings; the index assignments are
byte-verified against the curated [gamedata](../reference/gamedata.md)
accessory table (`accessory_passives_vs_disc`).

| Index | Name | Effect | Items |
|---|---|---|---|
| `0x00` | HP Boost 1 | max HP +10% | Life Ring, Mei's Pendant |
| `0x01` | HP Boost 2 | max HP +25% | Life Armband, Minea's Ring |
| `0x02` | MP Boost 1 | max MP +10% | Magic Ring, Yuma's Ring |
| `0x03` | MP Boost 2 | max MP +25% | Magic Armband |
| `0x04` | MP Used Down 1 | consume 25% less MP | Spirit Jewel |
| `0x05` | MP Used Down 2 | consume 50% less MP | Spirit Talisman |
| `0x06` | ATK Boost | ATK +20% | Power Ring |
| `0x07` | UDF Boost | UDF +20% | Scarlet Jewel |
| `0x08` | LDF Boost | LDF +20% | Azure Jewel |
| `0x09` | UDF & LDF Boost | both +20% | Guardian Ring |
| `0x0A` | SPD Boost | SPD +20% | Speed Ring |
| `0x0B` | INT Boost | INT +20% | Wisdom Ring, Star Pearl |
| `0x0C` | AGL Boost | AGL +20% | Vitality Ring |
| `0x0D` | Attack x2 | attack twice in a row | War God Icon |
| `0x0E` | Guard Attack | penetrate enemy defense | Unholy Icon |
| `0x0F` | Counterattack | counter at a fixed rate | Warrior Icon |
| `0x10` | Steal Attack | steal items and attack | Evil God Icon |
| `0x11` | First Attack | first turn in battle | Speed Chain |
| `0x12` | Last Attack | last turn in battle | Slowness Chain |
| `0x13` | Bull's Eye | hit rate up | Target Chain |
| `0x14` | Shield Boost | block rate up | Defender Chain |
| `0x15` | Zero Defense | neither side can block | Guardian Chain |
| `0x16` | Poison Guard 1 | nullify venom | Cure Amulet |
| `0x17` | Poison Guard 2 | nullify venom + toxin | Pure Amulet |
| `0x18` | Rot Guard | nullify rot | Forest Amulet |
| `0x19` | Curse Guard | nullify curse | Magic Amulet |
| `0x1A` | Stone Guard | nullify stone | Stone Amulet |
| `0x1B` | Numb Guard | nullify numb | Nature Amulet |
| `0x1C` | Master Guard | nullify all status | Wonder Amulet |
| `0x1D` | Earth Guard | Earth defense up | Earth Jewel, Earth Egg, Earth Talisman |
| `0x1E` | Water Guard | Water defense up | Deep Sea Jewel, Water Egg, Water Talisman |
| `0x1F` | Fire Guard | Fire defense up | Burning Jewel |
| `0x20` | Wind Guard | Wind defense up | Tempest Jewel |
| `0x21` | Thunder Guard | Thunder defense up | Madlight Jewel, Ra-Seru Egg |
| `0x22` | Light Guard | Light defense up | Luminous Jewel, Light Egg, Light Talisman |
| `0x23` | Dark Guard | Dark defense up | Ebony Jewel, Dark Stone, Dark Talisman |
| `0x24` | All Guard | defense vs all elements | Rainbow Jewel |
| `0x25` | HP After | recover HP each turn | Life Grail |
| `0x26` | MP After | recover MP each turn | Magic Grail |
| `0x27` | Final Heal | full revive on death | Lost Grail |
| `0x28` | AP Boost 1 | AP accrual +10% | Mettle Ring, Zalan's Crown |
| `0x29` | AP Boost 2 | AP accrual +25% | Mettle Armband, Seru Flame |
| `0x2A` | Maximum AP | AP stays at 100 | Mettle Goblet, Fire Droplet |
| `0x2B` | AP Used Down | consume 50% less AP | Mettle Gem |
| `0x2C` | Arts Power | arts power up | War Soul |
| `0x2D` | Rage | unpredictable behavior | Evil Medallion |
| `0x2E` | Magic Boost | magic (Seru absorb) accrual up | Ivory Book |
| `0x2F` | EXP Boost | XP after battle up | Crimson Book |
| `0x30` | Gold Boost† | gold +25% after battle | Golden Book |
| `0x31` | Items Up† | item drop rate up | Bronze Book |
| `0x32` | First Draw† | allies attack first more | Golden Compass |
| `0x33` | Ambush Down† | ambush rate down | Silver Compass |
| `0x34` | Escape Boost† | escape rate up | Chicken Heart |
| `0x35` | Safe Escape† | defense up while escaping | Chicken Safe |
| `0x36` | No Escape† | enemies can't escape | Chicken Guard |
| `0x37` | Great Escape† | assured escape (non-boss) | Chicken King |
| `0x38` | HP Walk | recover HP while walking | Life Source |
| `0x39` | MP Walk | recover MP while walking | Magic Source |
| `0x3A` | AP Walk | gain AP while walking | Mettle Source |
| `0x3B` | High Encounter† | encounter rate up | Bad Luck Bell, Nemesis Gem |
| `0x3C` | Low Encounter† | encounter rate down | Good Luck Bell, Evil Talisman |
| `0x3D` | Seru Encounter† | encounter Seru only | (unused item id `0xFD`) |
| `0x3E` | (Point Get)† | shop points worth 10% of price | - (see below) |
| `0x3F` | (Secret Buy)† | buy secret items | - (see below) |

† = party-wide scope (`+0 = 1`).

Quest items share indices with their purchasable twins (Mei's Pendant = Life
Ring, Minea's Ring = Life Armband, each Egg / Talisman = the matching Jewel) -
which is what first exposed the index space: the talisman descriptor rows
carry the same `+3` byte as the jewels'.

Indices `0x3E`/`0x3F` have populated description records, but the Point Card
(`0xFE`) / Platinum Card (`0xFF`) descriptor rows carry the `0x41` sentinel -
the cards are special-cased by item id (the menu's Goods panel branches on
`id == 0xFE` to render the points balance; see
`overlay_menu_801d0f1c.txt`), so these two indices are display-text-only.

## Stat-boost magnitudes (pinned from `FUN_80042558`)

The percent stat boosts (`0x00..=0x0C`) are applied **inline** by the
aggregator when it rebuilds the effective stat block from the base stats;
there is no magnitude table. Per index: `+10%` = `base/10`, `+25%` =
`base>>2`, `+20%` = `base/5`, added to the copied base, then capped
(HP `9999`, AGL `0x118` = 280, others `999`):

```text
char +0x104 (eff max HP) = +0x11C (base) [+ base/10 | + base>>2]
char +0x108 (eff max MP) = +0x11E (base) [+ base/10 | + base>>2]
char +0x110..+0x11B (AGL,ATK,UDF,LDF,SPD,INT) = +0x122..+0x12D [+ base/5]
```

The remaining indices are point-of-use flags: each consumer tests the
bitfield bit where the mechanic lives (MP cost at cast time - battle overlay
`0x801E3D0C`; the [steal table](steal-table.md) for Steal Attack; status /
elemental guards in the battle damage path; encounter rate in the field step
roll; loot in the battle-end reward resolver `FUN_8004E568`).

### Talisman spell grants (same function)

A separate arm of `FUN_80042558` watches the equip slots for specific **item
ids** (not passive indices) and grants/revokes battle spells: the five
Talismans `0x72..=0x76` grant the summon spells `0x9A/0x9B/0x9C/0x9D/0x99`
(Palma / Mule / Horn / Jedo / Juggernaut), and the top-tier Ra-Seru weapons
`0x09/0x11/0x19` grant `0x9E/0x9F/0xA0`. The spell-slot list is maintained at
`char +0x13D` via `FUN_800432BC` / `FUN_80042DBC`. So a Talisman is "guard
passive by index + summon by id" - the curated `summon_seru` class is this
arm, not a passive index.

## Provenance + parser

`legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus` resolves
the property / descriptor / equip-bonus index bytes and the `0x8007625C`
records from a `SCUS_942.54` image (`t_addr -> file-offset` map, identical to
the [item-name table](item-table.md) resolver); `stat_boosts(index)` mirrors
the aggregator arithmetic and `bit_location(index)` the bitfield placement.
The disc-gated `accessory_passive_real` test pins the per-item indices, the
table text, the scope flags, and the retail equip `+5` sentinel invariant;
`legaia-gamedata`'s `accessory_passives_vs_disc` cross-validates every
curated accessory effect class against its decoded index.

## Engine consumers

The clean-room engine consumes the table through
`legaia_engine_core::accessory_passives::AccessoryPassives` (item id →
passive index + party-wide scope flags, built from the same parse at boot).
`World::refresh_party_ability_bits` is the port of the aggregator's bitfield
pass: each party member's record `+0xF4` field is rebuilt from the eight
equipment slots, all members OR into the engine's global-mask mirror
(`World::party_ability_mask`, bit-tested by `World::party_has_ability` - the
`FUN_800431D0` port), and the per-member word 0 feeds the MP-cost consumers
(`MpCostModifier::from_ability_flags`), so an equipped MP-saver halves /
quarter-shaves the live cast cost. The percent stat boosts apply inside
`compute_battle_stats_with_passives` (percent of the **base** stat window,
truncating division, retail clamp block) and the max-HP boost lands on the
live battle actor in `World::seed_party_battle_stats`. Disc-gated coverage:
`engine-core/tests/accessory_passives_disc.rs`.

## See also

- [Item property / name table](item-table.md) - the shared table this indexes through.
- [Item-effect descriptor table](item-effect-table.md) - the `+3` byte lives in its records.
- [Equipment stat-bonus table](equipment-table.md) - the `+5` byte lives in its records (sentinel-only in retail).
- [Steal table](steal-table.md) - what Steal Attack (`0x10`, Evil God Icon) steals.
- [`reference/gamedata.md`](../reference/gamedata.md) - curated accessory effects (the validation ground truth).
