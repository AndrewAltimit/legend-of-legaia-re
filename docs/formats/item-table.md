# Item-name table

The single source of truth for every item's display name lives in a static
table inside `SCUS_942.54`. The MES interpreter's item-substitution opcodes
(`0xC2 XX` / `0xC4 XX`, see [`mes.md`](mes.md)) print an item name by indexing
this table, so it is the executable's ground-truth name for weapons, armor,
accessories, consumables and key items - one shared 256-entry id space.

The same id space is what a monster record's `drop_item` byte indexes (see
[`monster-animation.md`](monster-animation.md) / the monster archive's
[`+0x48`](../subsystems/battle.md) drop field), which is how a raw drop id
becomes a readable name (`0x79` → `Healing Berry`).

## Table base + record layout

| | |
|---|---|
| Record base | `DAT_80074368` |
| Stride | `0xC` bytes |
| Id range | `0x00..=0xFF` (256 ids) |

The MES `0xC2`/`0xC4` substitution indexes the table by the **name pointer** at
record `+4` (`PTR_DAT_8007436C = DAT_80074368 + 4`), which is why the name table
reads as `PTR_DAT_8007436C[id*3]` (three `u32` words: name, type-ptr, …) — but
the record proper starts 4 bytes earlier, at `DAT_80074368`:

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | kind (`1` = equipment, `2` = item/consumable/key) |
| `+1` | u8 | (per-kind flags) |
| `+2` | u16 | **shop price** in gold — what the buy/sell UI charges; `0` = quest / found-only item the shop never prices |
| `+4` | u32 | `name_ptr` — pointer to the NUL-terminated display name |
| `+8` | u32 | secondary pointer (a shared "type" / description string for some classes) |

The shop price at `+2` is decisive (verified live: War God Band = 21000, Healing
Leaf = 100, the Ra-Seru / quest items = 0). The shop randomizer reads it to drop
quest items from the for-sale pool and to give the chest-found equipment a value
(`legaia_asset::item_names::price_slot` / `legaia_rando::item_price`).

The table's extent is found by reading until the `name_ptr` words leave the
PSX-EXE data segment (the words past `id 0xFF` are no longer valid pointers).

## Name strings

The display strings carry the same MES control prefixes as every other in-game
string: a leading `0x01` icon escape and `0xCE XX` colour-control bytes. The
parser strips control bytes (keeping printable ASCII) and trims surrounding
whitespace. A handful of ids (`0x00`, `0x12`, `0x1A`, `0x52`, `0xB9`, `0xFD`)
have empty names — reserved / gap slots; `id 0` is "no item".

## Provenance + parser

`PTR_DAT_8007436C` and the `*3`-word index form are read straight from the MES
`0xC2`/`0xC4` substitution dispatch (`docs/formats/mes.md`). The
`legaia_asset::item_names::ItemNameTable` parser resolves the table from a
`SCUS_942.54` image at runtime (PSX-EXE `t_addr` → file-offset map, identical
to the [arts-name table](art-data.md#arts-name-table-dat_80075ec4) resolver in
`legaia_art::arts_table`). The web viewer's enemy table uses it to show drop
item names; the disc-gated `item_names_real` test pins a span of ids against the
real executable.

## See also

- [Spell table](spell-table.md) - the sibling static `SCUS_942.54` name+stat table.
- [Art records](art-data.md) - the Tactical Arts records and arts-name table.
- [`reference/gamedata.md`](../reference/gamedata.md) - the curated ground-truth item/drop tables.
