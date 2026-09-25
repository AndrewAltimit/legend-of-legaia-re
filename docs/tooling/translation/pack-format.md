# Language pack format

A language pack is one YAML document: a short header, then one list of entries
per section. This page is the schema. For how much room each entry has, see
[`space-and-budgets.md`](space-and-budgets.md); for the workflow, see
[`index.md`](index.md).

## YAML schema

A **working** pack is the source-bearing shape a translator edits:

```yaml
format: 'legaia-text-pack-v1'
language: 'fr'
game: 'Legend of Legaia (USA) SCUS-94254'
contributors: ['...']
notes: '...'
sections:
  items:               # one list per section, fixed order
  - key: 'scus:str:0x80012260'   # stable provenance key
    context: 'item 0x79'          # human context, not machine-read
    source: 'Healing Berry'       # US text, markup form
    translation: ''               # fill me
    budget: 13                    # max encoded bytes for the translation
```

A **distributable** pack is the source-less shape that is committed and
shipped: the same document with `source:` and `context:` removed and only
filled entries kept.

```yaml
  items:
  - key: 'scus:str:0x80012260'
    translation: 'Baie Soin'
    budget: 13
```

Entry fields:

| Field | In | Meaning |
|---|---|---|
| `key` | both | the disc coordinate the text lives at; the import target |
| `context` | working | a human hint (the table id, the referencing ids); never machine-read |
| `source` | working | the disc's own text in markup form; import compares it with the disc before writing |
| `translation` | both | your text in markup form; empty = leave the disc byte-identical |
| `budget` | both | the maximum encoded byte length in place ([`space-and-budgets.md`](space-and-budgets.md)); in a distributable pack, also the wrong-disc hint |

## Sections and key shapes

The twelve sections, in the order a pack lists them:

| Section | Contents | Key shape | Mechanism |
|---|---|---|---|
| `items` | item names (MES `{c2:xx}`/`{c4:xx}` substitutions) | `scus:str:0x<va>` | overwrite the NUL-terminated string in `SCUS_942.54` in place, re-terminate (a short write zero-fills the rest of the old span); a longer one moves |
| `item_types` | shared item "type" strings (second record pointer) | `scus:str:0x<va>` | same |
| `spells` | spell/magic names (`{c3:xx}`) and the info window's `Name\|effect` descriptions (the pointer table `0x80075DB0` the record's `+4` byte indexes) | `scus:str:0x<va>` | same |
| `arts` | Tactical Arts names (`{c5:xx}`) and the arts-menu descriptions (record `+0x10`) | `scus:str:0x<va>` | same |
| `accessory_passives` | Goods-menu passive names + descriptions | `scus:str:0x<va>` | same |
| `party_names` | new-game roster names (Vahn/Noa/Gala/Terra) | `scus:party:<n>` | fixed 10-byte NUL-padded field |
| `scene_dialog` | NPC/event dialog in the scene-bundle MANs | `man:<prot>:0x<off>` | edit the `0x1F`-segment inside the LZS-decompressed MAN, recompress into the original compressed footprint |
| `inline_text` | dialog/narration in raw carriers (v12 event-script prescripts, streaming-MAN dungeon scenes) | `raw:<prot>:0x<off>` | space-padded same-size overwrite directly in the PROT entry; in a streaming dungeon scene a longer line grows the uncompressed MAN chunk instead |
| `ui_menu` | overlay-resident UI strings ([`ui-strings.md`](ui-strings.md)) | `ui:<prot>:0x<va>` | overwrite the NUL-terminated string in the PROT **overlay** entry in place at `file offset = va - base_va`, re-terminate (short writes zero-fill the old span) |
| `system_text` | `SCUS_942.54` system strings outside the name tables ([`ui-strings.md`](ui-strings.md#system_text)) | `scus:str:0x<va>` | same as the name tables (span + alignment padding), from pinned VA windows (`translation::ui::SCUS_STRING_POOLS`) |
| `place_names` | world-map quick-travel place names (`legaia_asset::worldmap_menu`) | `scus:cell:0x<va>` | fixed `0x20`-byte NUL-padded cell |
| `monster_names` | enemy names: the battle name plaque and every battle line that names the enemy | `mon:<id>` | rewrite the name inside monster `id`'s record in the monster archive (PROT 867) and re-pack its fixed slot; a longer one grows the record |

How much each mechanism allows, and what happens past it, is tabulated on
[`space-and-budgets.md`](space-and-budgets.md).

Three export rules shape the entry list:

- Strings pointer-shared by several table slots export once; the `context`
  lists the referencing ids. Interior pointers clamp the `budget`.
- Duplicate PROT TOC entries over the same disc bytes are deduplicated by LBA.
- The dialog sections are exported per line, as described next.

## Line granularity

The dialog sections are *line-granular*. The pager packs up to three
consecutive segments into one box ([`mes.md`](../../formats/mes.md)), so
consecutive entries in the pack are consecutive rows on screen. Translate them
as a group and keep each row inside its own budget.

## Which segments count as dialog

Whether a `0x1F` segment is exported as dialog depends on whether a script
says it is text.

Inside a MAN - a scene bundle's, or the uncompressed one leading a streaming
dungeon scene - export keeps every `0x1F` lead that an instruction on its
record's clean script walk carries as text (`man_edit::text_site` =
`Segment`, the same structural gate import applies), whatever the text reads
like: `Anyway...`, `Oh!`, `(Silence)`, a growl, a speaker line built only from
name substitutions. A lead the walk spans as operand bytes is never exported,
even when it reads as a word.

Only a lead the walk does not reach, and the rest of a raw carrier, fall back
to the prose-quality gate (`segments::qualifies`). That gate rejects
space-less runs that are not a clean word, because that is the shape of binary
noise. Raw `raw:` targets additionally pass the per-entry
[dialog-carrier gate](dialog-import.md#the-dialog-carrier-gate-raw-writes).

Blank spacer lines (spaces only) are skipped. One scanner per domain
(`segments::scan_man`, `segments::scan_raw_carrier`) feeds export,
`lift-official` and `diff-disc`, so their keys agree.

## Text markup and encoding

The glyph atlas is indexed by byte with `0x20..=0x7E` as plain ASCII
([`dialog-font.md`](../../formats/dialog-font.md)), so markup is mostly literal
text:

| Markup | Meaning |
|---|---|
| printable ASCII | maps to itself |
| `\|` | the in-game newline glyph (`0x7C`) |
| `{c1:00}` | character-name substitution |
| `{c2:79}` | item-name substitution |
| `{c3:..}` / `{c5:..}` | magic / art name substitution |
| `{cf:0n}` | color change |
| `{ce:..}` | spacing / icon escape |
| `{xx:yy}` in general | a 2-byte token (opcode + argument) |
| `{xx}` | a bare byte (`{01}` item-icon prefix, high glyph tiles) |
| `{7b}` / `{7d}` | literal `{` / `}` |

Keep every token in the translation wherever the source has it. The dialog
exports the raw source lines including substitution escapes, so translations
must keep grammatical agreement working around them (the substituted names
come from the tables you also translate).

`encode` (string → game bytes) is the exact inverse of the exporter's decode
and reports **per-character** errors: anything outside printable ASCII is not
in the retail glyph set. Common typographic lookalikes (smart quotes, en/em
dashes, ellipsis, NBSP) are folded automatically.

**Accented Latin, Cyrillic and CJK are not encodable** - the retail font
simply has no such glyphs. A full non-Latin translation needs a font patch
(new glyph tiles + width table), which this pipeline does not attempt
([`textures-and-fonts.md`](textures-and-fonts.md#font-patch-scope)).
French/Italian/etc. must be written unaccented (`Epee` not `Épée`).

Text lifted off an official PAL disc is the one exception. It arrives as raw
`{xx}` accent bytes, which *encode* fine but draw blank on the NTSC font, so
the lift offers the same ASCII fold (`--fold-accents`; see
[`pal-localizations.md`](../pal-localizations.md#accent-folding)).
