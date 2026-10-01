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
accents: 'font'        # optional: how typed accents reach the disc (see Accents)
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

`translate coverage` measures this rule against the script walk; see
[coverage](index.md#measuring-coverage).

## Packs from other builds

`export` reads the disc's `SYSTEM.CNF` boot line and exports what that build
carries:

| Build | Sections | Dialog codec |
|---|---|---|
| USA `SCUS_942.54` | all twelve | `0x1F <glyphs> 0x00` |
| PAL `SCES_*` | `scene_dialog`, `inline_text`, `monster_names` | the same framing; accented glyphs above `0x7E` pass the quality gate and export as `{xx}` escapes |
| Japan `SCPS_100.59` | `scene_dialog`, `inline_text` | count-led Shift-JIS ([`mes.md`](../../formats/mes.md#japanese-build-count-led-shift-jis-lines)) |

The `scus:` / `ui:` / `scus:cell:` sections are USA virtual addresses, so they
come from the USA disc only; the other builds' executables lay their tables
out elsewhere. The `man:` / `raw:` / `mon:` keys are disc coordinates on the
disc exported, and a pack's `game:` header names that disc. A PAL or Japanese
pack's keys do not name the same lines on the USA disc - that pairing is what
[`lift-official`](../pal-localizations.md) does for the Latin builds.

A Japanese pack's `source` is Unicode: each two-byte token that is a
Shift-JIS character decodes to that character, and every other token (the
`F0..FF` substitution escapes, and any pair that does not re-encode to itself)
is a `{xx:yy}` escape, so the text re-encodes to exactly the disc bytes. The
key's offset is the first token byte (after the count byte) and the budget is
twice the count. The Japanese pack is a reading reference: `import` refuses
every filled entry on a Japanese disc and writes nothing.

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
| `{ce:..}` | a button / icon / number symbol - see [Symbols](#symbols) |
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

Typed accents are handled one step before `encode`, by the pack's accent mode
(next section). In the default mode an accented letter is an encode error
whose message names its ASCII fold and the cell the accent font would give
it. Cyrillic, Greek and CJK have no glyph and no fold in any mode.

## Symbols

A `{ce:NN}` token draws a controller button, an icon or a number inside the
text - entry `NN` of the `0xCE` escape table
([`dialog-font.md`](../../formats/dialog-font.md#escape-table-0x80074050)).
Export writes the hex form. A pack may type the readable **alias** instead;
`encode` turns it into the same two bytes (`0xCE`, `NN`), so a pack that uses
aliases imports byte-identically, and a re-export of the patched disc reads
`{ce:NN}` again. Aliases are case-insensitive; an unknown one
(`{btn:start}`) is reported as an unknown symbol.

| Token | Alias | Draws |
|---|---|---|
| `{ce:00}`..`{ce:07}` | `{btn:x}` `{btn:circle}` `{btn:square}` `{btn:triangle}` `{btn:r1}` `{btn:r2}` `{btn:l1}` `{btn:l2}` | controller buttons |
| `{ce:08}` | `{icon:gold}` | `G` gold badge |
| `{ce:09}` / `{ce:0a}` | `{icon:single}` / `{icon:all}` | `I` / `A` target badges |
| `{ce:0b}`..`{ce:0e}` | `{num:0}`..`{num:3}` | a script counter, printed as a number |
| `{ce:0f}` | `{text:ra-seru}` | "Ra-Seru" in Japanese kana |
| `{ce:10}`..`{ce:13}` | `{icon:arms}` `{icon:head}` `{icon:body}` `{icon:legs}` | equip-slot icons |
| `{ce:14}`..`{ce:1a}` | `{icon:fire}` `{icon:thunder}` `{icon:wind}` `{icon:water}` `{icon:earth}` `{icon:light}` `{icon:dark}` | element plates |
| `{ce:1b}` | `{icon:monster}` | monster plate |
| `{ce:1c}` | `{icon:fire-cut}` | the fire plate's left edge, 12 px |
| `{ce:1d}`..`{ce:23}` | `{icon:fire2}` `{icon:thunder2}` `{icon:wind2}` `{icon:water2}` `{icon:earth2}` `{icon:light2}` `{icon:dark2}` | winged element icons |
| `{ce:24}` | `{icon:monster2}` | the monster plate, 28 px wide |
| `{ce:25}` | `{icon:fire-wide}` | the fire plate, 28 px wide |

The table lives in one place, `legaia_patcher::translation::symbols::SYMBOLS`;
the translation workbench's symbol palette and preview read it, drawing each
symbol from the sprites on the user's own disc
(`legaia_font::escape_icons`). The first list of these symbols was contributed
by Stann0x (see [`dialog-font.md`](../../formats/dialog-font.md#symbol-names-and-aliases)).

## Accents

The retail USA font draws plain letters only: its page has some accented
glyphs in the high cells, but the width table gives most of them a zero
advance, so they overprint the next letter
([`dialog-font.md`](../../formats/dialog-font.md#accented-latin-cells)). The
pack's optional `accents:` header decides what happens to an accented letter
on import:

| `accents:` | Typed `é` | `{82}` in the text | The disc |
|---|---|---|---|
| empty / `strict` | encode error, reported per character | written as the byte (it overprints on the retail font) | unchanged |
| `fold` | written as `e` | written as `e` | unchanged |
| `font` | written as the byte `0x82` | written as the byte | the [accent font](../../formats/dialog-font.md#the-accent-font) is written once |

The folds and cells come from one table, `legaia_font::latin`, which the CLI,
the ROM patcher page and the workbench all read. A Latin letter with no cell
(`ł`, `č`, `ő`) folds in both `fold` and `font` mode. A two-letter fold
(`ß` to `ss`, `æ` to `ae`) grows the line by a byte, so a line at its budget
can stop fitting under `fold`; under `font` it stays one byte.

`translate import --accents <strict|fold|font>` overrides the header for one
run. `translate stats` and `translate space --pack` list every character that
will not draw as typed under the pack's mode, with its key and character
index, and `import` reports how many accents it encoded into cells and how
many it folded. Implementation: `translation::accents`; disc-gated oracle
`crates/patcher/tests/translation_accent_font_real.rs`.

Text lifted off an official PAL disc arrives as raw `{xx}` accent bytes on the
same layout, so it draws as-is under `font` - `{d7}` (`Î`) and `{f8}` (`°`)
included, which the PAL disc itself draws as placeholder boxes
([`dialog-font.md`](../../formats/dialog-font.md#the-pal-renderer)) - and the
lift's `--fold-accents` is the same fold `fold` applies
([`pal-localizations.md`](../pal-localizations.md#accent-folding)).
