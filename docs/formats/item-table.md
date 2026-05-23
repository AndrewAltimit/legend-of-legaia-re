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
| Base pointer | `PTR_DAT_8007436C` |
| Index form | `PTR_DAT_8007436C[id*3]` (three `u32` words per id) |
| Stride | `0xC` bytes |
| Id range | `0x00..=0xFF` (256 ids) |

| Offset | Type | Field |
|---|---|---|
| `+0` | u32 | `name_ptr` — pointer to the NUL-terminated display name |
| `+4` | u32 | secondary pointer (a shared "type" string for some classes) |
| `+8` | u32 | packed price / id / type metadata |

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
