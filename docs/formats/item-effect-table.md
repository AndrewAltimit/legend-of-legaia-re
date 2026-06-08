# Item-effect descriptor table

A static `SCUS_942.54` table that says **what kind of effect** each consumable
has - heal HP, restore MP, cure status, revive, raise a stat, escape a dungeon -
plus its target shape (single ally vs. whole party) and where it can be used
(field menu vs. battle). It is the sibling of the [item-name table](item-table.md)
(`DAT_80074368`) and the [spell table](spell-table.md): the three are contiguous
static data, and this table ends exactly where the spell table begins.

## What it is and is NOT

This table holds the effect **class + tier + flags**, **not** the literal
restore amounts. "Healing Leaf restores 200 HP" is split in two: this table
records `(class = heal-HP, tier = 0)`, and the `(class, tier) -> 200` mapping is
a separate, **also static** [heal-amount table](#heal-amount-table-0x8007655c).

The apply handler is the **static** `FUN_800402F4` (`ghidra/scripts/funcs/
800402f4.txt`), reached through a 132-entry jump table at `0x80014FA0` indexed by
the descriptor class byte. It handles both the field item menu and the battle
item path in one function (it branches on `game_mode == 0x15`, walking either the
char records or the battle-actor table `0x801C9370`). The HP / MP heal arms size
the restore by reading a tier-indexed `u16` table at `0x8007655C` (HP) /
`0x80076564` (MP) - so the numbers **are** on the disc, decoded by
`legaia_asset::item_effect` (`ItemEffectTable::heal_amounts` / `restore_amount`).
(This **corrects** the earlier "the amounts are a switch of immediates inside an
overlay-resident apply handler, not in the dumped corpus" reading.)

## Table base + record layout

| | |
|---|---|
| Record base | `DAT_800752C0` (file `0x65AC0`) |
| Stride | `0x4` bytes |
| Record count | `130` (subtypes `0x00..=0x81`); ends at the spell table `0x800754C8` |

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | effect **class** (action-validator arm) |
| `+1` | u8 | **tier** / sub-case (per-class selector; e.g. heal-HP `0/1/2` = `200/800/max`) |
| `+2` | u8 | **flags** (see below) |
| `+3` | u8 | constant `0x41` (`'A'`) marker across consumable-effect rows |

### Flag byte (`+2`)

| Bit | Meaning |
|---|---|
| `0x80` | base / "has an effect" - set on every populated descriptor |
| `0x20` | effect applies to the **whole party** (all-targets validator) |
| `0x04` | usable from the **battle** item menu |
| `0x02` | usable from the **field** item menu |

Healers carry `0x04 | 0x02` (field + battle); permanent stat-ups and the
field-utility items carry `0x02` only; status-cures and revive carry `0x04`
only. Key items resolve to a descriptor with **neither** usability bit set
(e.g. flag `0x89`), which is how the item menu greys them out.

### Class byte (`+0`)

Class labels are validated against the on-disc item *description* strings (item
record `+8` pointer):

| Class | Effect | Example item / on-disc description |
|---|---|---|
| `0` | heal HP, one ally | Healing Leaf - "Recover 200HP. Ally." |
| `1` | heal HP, whole party | Healing Bloom - "Recover 200HP. All allies." |
| `2` | restore MP | Magic Leaf - "Recover 50MP. Ally." |
| `3` | cure all status | Medicine - "Cure all status. Ally." |
| `4` | revive | Phoenix - "Restore life. Ally." |
| `5` | extend action gauge (one battle) | Fury Boost - "Extend action gauge for one battle." |
| `6` | permanent stat-up (tier = which stat) | Miracle Water - "All stats +4. Ally." |
| `7` | temporary stat buff (one battle) | Power Elixir - "Increase attacking power for one battle." |
| `8` | cure single status | Antidote - "Cure Venom. Ally." |
| `11`/`12`/`13` | arts book (Fire/Wind/Thunder; tier = book level) | Fire Book I - "Book of Hyper Arts. For Meta." |
| `126`/`127` | summon flute | Lippian Flute - "Flute that calls the Lippian monster." |
| `128` | field escape (dungeon) | Door of Light - "Teleport out of dungeons." |
| `129` | field warp (city) | Door of Wind - "Teleport to another city." |
| `130` | reduce encounter rate | Incense - "Decrease encounter rate for a period of time." |

Note that the class byte is meaningful **only together with the usability
flags**: many key items funnel to class `0` with no usability bit set, so
"class 0" is not by itself "an HP potion" - gate on the field/battle bits.

## Heal-amount table (`0x8007655C`)

The literal restore *amounts* the apply handler `FUN_800402F4` reads. Two
contiguous `u16[4]` sub-tables, indexed by the descriptor **tier** (`base +
tier*2`); only tiers `0..=2` are read flat.

| VA | Sub-table | Tier 0 | Tier 1 | Tier 2 | Read by |
|---|---|---|---|---|---|
| `0x8007655C` (file `0x66D5C`) | HP restore cap | `200` | `800` | `9999` | class `0` (single), class `1` (all-party) |
| `0x80076564` | MP restore cap | `50` | `200` | `20` | class `2` (MP) |

Each restore is **deficit-clamped**: `applied = min(max - current, table[tier])`,
so tier `2`'s `9999` is an effective full HP restore. Tier `3+` does **not** read
this table - those higher HP heals are **character-relative** (they scale off the
per-character `0x80084140` Seru-heal tables). **Revive** (class `4`) is also not a
flat amount: tier `0` restores `max_hp*0.4 + rand()%(max_hp/8)`, tier `>0` is a
full revive. Provenance: HP arm `0x800404b8`, MP arm `0x80040dc0`, revive arm
`0x80040f58` (`ghidra/scripts/funcs/800402f4.txt`).

Parser: `ItemEffectTable::heal_amounts()` (the two `u16` arrays) and
`restore_amount(id)` (resolves an item id through its `(class, tier)` to a
[`RestoreAmount`] - `Hp(u16)` / `Mp(u16)` / `CharRelative`). The disc-gated
`item_effect_real` test pins the table and a set of items byte-for-byte against
the engine's curated figures (Leaf `200`, Flower `800`, Berry `9999` full, Magic
Leaf `50` MP, Magic Fruit `200` MP, Healing Shroom `200`).

## Stat-up / buff items (class 5/6/7)

The stat-affecting classes do **not** read a data table — their magnitudes are
inline immediates in the apply handler's switch (`FUN_800402F4` `case 5/6/7`).
The `(class, tier)` → effect mapping is therefore pinned to the disassembly,
while each item's `(class, tier)` is parsed from the descriptor table above.

**Class 6 — permanent stat-up** (field-use, flag byte `0x82`). Adds a flat
increment to the **character record** (`0x80084708 + slot*0x414`, the same
record window the [save schema](../../crates/save/src/character.rs) maps), `tier`
selecting which stat. The amounts and caps:

| Tier | Stat | Record offset | `+delta` | Cap | Item |
|---|---|---|---|---|---|
| 0 | Max HP | `+0x11C` | `+16` | `9999` | Life Water (`0x82`) |
| 1 | ATK | `+0x124` | `+4` | `999` | Power Water (`0x83`) |
| 2 | DEF (both facets `+0x126`/`+0x128`) | `+0x126`/`+0x128` | `+4` | `999` | Guardian Water (`0x84`) |
| 3 | SPD | `+0x12A` | `+4` | `999` | Swift Water (`0x85`) |
| 4 | INT | `+0x12C` | `+4` | `999` | Wisdom Water (`0x86`) |
| 5 | Max MP | `+0x11E` | `+8` | `999` | Magic Water (`0x87`) |
| 6 | **all** (AGL/HP/ATK/SPD/INT/MP/DEF) | — | `+4` each | AGL `280`, HP `9999`, else `999` | Honey (`0x65`), Miracle Water (`0x6D`) |

**Class 7 — one-battle buff** (battle-use, flag byte `0x84`). Multiplies the
**battle-actor** stat halfwords by `6/5` (+20%, the handler's `uVar + uVar/5`),
clamped to `0xFFFF`, for the rest of the battle. `param_2` = `tier`:

| `param_2` | Stat(s) buffed | Item |
|---|---|---|
| 1 | SPD (`+0x164/+0x166`) | Speed Elixir (`0x8D`) |
| 2 | DEF, both facets (`+0x15C/+0x15E` + `+0x160/+0x162`) | Shield Elixir (`0x8C`) |
| 3 | ATK (`+0x158/+0x15A`) | Power Elixir (`0x8B`) |
| 4 | **all** — SPD + DEF + ATK + AGL (`+0x168/+0x16A`) | Wonder Elixir (`0x8E`) |

**Class 5 — Fury Boost** (`0x81`): extends the action gauge for one battle
(sets the actor `+0x1F9` gauge flag); no stat target. See the buff-selector
table in [`battle-formulas.md`](../subsystems/battle-formulas.md#stat-buff-selectors-17).

Parser: `ItemEffectTable::stat_effect(id)` → `StatItemEffect` (`ActionGauge` /
`Permanent(Vec<StatChange>)` / `BuffOneBattle(Vec<StatTarget>)`), and the raw
`stat_item_effect(class, tier)` mapping. The disc-gated `item_effect_real` test
pins every Water item, both all-stats items, the four Elixirs, and Fury Boost by
their real ids. Provenance: `case 6` lines `2380-2472`, `case 7` lines
`2473-2639` of `ghidra/scripts/funcs/800402f4.txt`.

## Indexing (Ghidra-traced)

The lookup is **double-indirected by item id -> subtype -> descriptor**. From
`FUN_8003043c` (`ghidra/scripts/funcs/8003043c.txt`):

```text
subtype    = item_name_table[id].byte(+1)      ; DAT_80074369[id*0xC]
descriptor = (&DAT_800752C0)[subtype * 4]      ; stride-4 record
arm        = descriptor[+0]                     ; effect class / validator arm
tier       = descriptor[+1]
flags      = descriptor[+2]
```

The `& 0x20` all-party test is read in `FUN_8003043c` itself (it selects the
all-targets call to the action validator `FUN_8003fb10`). The `0x02` / `0x04`
field-vs-battle usability bits are read by the field item-menu list builder
`FUN_80030628` (`ghidra/scripts/funcs/80030628.txt`), where two menu contexts
gate on `flags & 2` and `flags & 4` respectively. The battle item path reads the
class + tier straight into the actor's action context (`overlay_battle_action_801e295c`,
`addiu a0,a0,0x52c0` at `0x801e3ba4`).

## Provenance + parser

`legaia_asset::item_effect::ItemEffectTable::from_scus` resolves both tables from
a `SCUS_942.54` image at runtime (`t_addr -> file-offset` map, identical to the
[item-name table](item-table.md) resolver). The disc-gated `item_effect_real`
test pins the `(class, tier, flags)` bytes for a span of consumables against the
real executable and cross-checks each against its on-disc description.

## Engine consumption

`legaia_engine_core::items::ItemCatalog::apply_effect_flags` installs three of
the flag bits over the curated catalog from disc: the `0x02`/`0x04` usability
gates (per item id) and the `0x20` all-party flag (`ItemCatalog::is_all_party`).
The item-use session ([`legaia_engine_core::inventory_use::InventoryUseSession`])
reads `is_all_party`: a flagged restorative item (Healing Bloom `0x7A`, Healing
Fruit `0x7B`) skips target-select and fans its effect across every living ally
in one use, consuming one copy. The disc-gated `item_effect_flags_disc` test
pins the all-party flag for `0x7A`/`0x7B` (and its absence on the single-target
heals) against the real executable.

Beyond the static field/battle bits, the session also models the retail
**menu-usability gate** that `FUN_8003043c` performs: an item is only offered
(not greyed) when at least one currently-eligible target would actually benefit
from it. `FUN_8003043c` walks the live party (`+0x458` class byte) calling the
shared relevance/validity predicate `FUN_8003fb10(class, tier, target)` per
member - returning "usable" if any member's current state makes the effect do
something. The clean-room equivalent (`inventory_use::item_has_valid_target` ->
`effect_benefits_target`) greys a heal when every living ally is at full HP, a
cure when nobody carries the matching status, and a revive when nobody has
fallen, mirroring the item-relevance arms of the already-ported validator
(`legaia_engine_vm::action_validator`, the clean-room port of `FUN_8003fb10`).

The **permanent stat-up *Water* line** (class 6) is seeded into the catalog
from the installed table by `ItemCatalog::apply_stat_items` (so it appears only
when the disc is present, never as a no-op on a disc-free build);
`World::use_item` resolves each item's per-stat changes from the table at use
time and applies them through the shared persistent stat-raise path (a `Defense`
change raises both defence facets, matching the handler). The disc-gated
`water_line_stat_up_items_seed_and_apply_from_disc` test pins that Life Water
raises Max HP +16, the single-stat Waters raise their stat by +4, and Honey
raises all stats.

The **class-7 one-battle buff Elixirs** are seeded by `ItemCatalog::apply_buff_items`
(battle-only) and applied in `World::use_item` through the shared buff path
(`apply_battle_buff`, the same `×6/5` ramp + revert-on-expiry / revert-at-battle-end
bookkeeping as buff *spells*): Power/Shield/Speed Elixir ramp ATK / DEF / SPD,
Wonder Elixir ramps all four (SPD + DEF + ATK + AGL). The disc-gated
`elixir_battle_buffs_seed_and_ramp_from_disc` test pins that Power Elixir ramps a
100 ATK scalar to 120 and Wonder Elixir installs four buff trackers without
compounding. Fury Boost (class 5, action-gauge) still needs its own consumer.

## See also

- [Item-name table](item-table.md) - the sibling name/price table this indexes through.
- [Spell table](spell-table.md) - the static table immediately after this one.
- [`reference/gamedata.md`](../reference/gamedata.md) - curated item effect *amounts* (the numbers this table does not carry).
