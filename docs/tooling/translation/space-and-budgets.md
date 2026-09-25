# Space and budgets

Every translated string has to fit somewhere on a disc whose layout was fixed
at mastering. This page collects every room rule in one place: how many bytes
each kind of text may take, where that room comes from, and what the importer
does when a translation needs more.

A translation's size is always its **encoded** length in bytes - markup tokens
count as the bytes they encode to ([`pack-format.md`](pack-format.md#text-markup-and-encoding)),
not as the characters you type.

## Summary

| Section | Where the room comes from | Can it grow? | When it doesn't fit |
|---|---|---|---|
| `items`, `item_types`, `spells`, `arts`, `accessory_passives` | the string's 4-byte-aligned slot in the SCUS name pools | yes: the string moves into room other names free up | stays English, reported `no free run` |
| `monster_names` | the name's room inside its monster record (7, 11 or 15 bytes) | yes: the record grows, up to 15 bytes | stays English, reported |
| `party_names` | a fixed 10-byte field | no (9-byte budget) | stays English, reported |
| `place_names` | a fixed `0x20`-byte cell | no (31-byte budget) | stays English, reported |
| `ui_menu`, `system_text` | the string's span in its pool | no - these are reached from code | stays English, reported over budget |
| `scene_dialog` | the scene MAN's compressed footprint | yes, within the footprint; by whole sectors with `--allow-relayout` | lines rolled back to English, largest growth first |
| `inline_text` (event-script carriers) | the line's own span | no | stays English, reported over budget |
| `inline_text` (streaming dungeon scenes) | the entry's sector slack | yes, within the slack; by whole sectors with `--allow-relayout` | stays English, reported |

Shorter is always fine. A string is re-terminated or a dialog line
space-padded, and nothing past the old span moves.

## SCUS name slots

The budget of a SCUS string starts from the original string's byte span. A
shorter translation is re-terminated, and bytes past the NUL are never read.

The SCUS name pools are 4-byte aligned, so each string's `budget` also claims
the 0..3 bytes of zero padding after its terminator. The padding is verified
zero per string, and never claimed past another pointed-to string. That is
about 1.5 extra bytes on average, which is what lets the tightest name tables
translate at all.

So the room a name has depends on how long the English happens to be:
`Potion` has one spare byte, `Antidote` three, `Medicine` none. A translation a
byte or two longer than English fits for some names and not for others. For
the pointer-table name sections this is only the *in-place* budget - import
does not stop there.

### Longer names

Every item, item-type, spell (name and description), Tactical Arts name and
accessory-passive string is reached only through a pointer word in its table
record. Across `SCUS_942.54` and every statically based overlay image, the
only references to these strings are the table slots the export walks (swept
with `scripts/ghidra-analysis/find-address-word-refs.py`; no literal word,
`lui` pair or `jal` outside the tables).

A name that outgrows its slot can therefore **move** (`translation::name_pool`):

1. The pools are compacted. Each run of adjacent movable strings is re-laid
   end to end in its original order, which always fits, so the bytes every
   shorter translation gives up collect into one free run per region.
2. The longer names are placed into those runs.
3. Every slot that pointed at a moved string is repointed.

It is still a same-size edit of the executable, so a PPF carries it, and no
option has to be set. A name the pools have no room for keeps its English text
and is reported (`no free run`) - shorten it, or shorten other names in the
same tables.

Import checks each move on the disc it patches: a string moves only when its
table slots are its only aligned-word references in the executable and no
`lui` pair materialises its address. Some strings never move:

- the arts-menu descriptions - the in-battle matcher finds each art's combo
  string in the bytes after the description's terminator;
- a string sharing its tail with another;
- the `system_text` / `ui_menu` pools, which are reached from code.

Moved strings start 4-byte aligned, as every retail name does.

## Monster names

A monster's name lives in its own record (`legaia_asset::monster_archive`,
PROT 867): the decoded block's `+0x00` word is the block-relative offset of
the name, and the battle loader `FUN_80054CB0` copies the whole string into
the actor's name buffer, from which the plaque and the battle lines read it.

The section exports every populated record as `mon:<id>`. The source keeps its
markup - a leading element-badge escape `^X` (exported as the `{5e:xx}` token,
drawn as badge `X - 'A'`) and a trailing ` $N` variant suffix (the plaque copy
stops at `$`) - and a translation keeps both. A monster name takes printable
glyphs only.

**In place**, the budget is the record's own room: from the name to the lowest
block-relative offset any of the record's pointer words (`+0x04`, `+0x08`, the
spell-offset array and the effect-offset table ahead of the name) addresses
above it, less the terminator. The model at `+0x04` follows the name at a
4-byte boundary, so that room is 7, 11 or 15 bytes.

**A longer name grows the record.** Whole words are inserted after the name,
and every block-relative offset at or past the insertion point is bumped -
the only words the battle loader (`FUN_800542C8`) turns into pointers. The
model, the spell blobs and the texture pool move as one piece and read the
same bytes (the model's own offsets are relative to the model,
`FUN_800268DC`).

Two ceilings bound the growth:

- **Fifteen bytes**, the longest retail name. The loader's copy into the
  actor's name buffer at `+0x1BC` is unbounded, and a duplicate enemy gets
  ` A` / ` B` / ... appended there, so fifteen bytes plus the suffix and
  terminator is eighteen of the thirty-two zero bytes that precede the
  actor's next live field (measured in a Queen Bee battle on a patched disc).
- **The largest retail decoded block and loader-kept head**
  (`monster_names::RETAIL_MAX_BLOCK` / `RETAIL_MAX_KEPT`), so no load buffer
  sees a size retail never produces.

Import decodes the slot, rewrites the name (the rest of the old span zeroed),
and re-packs the block into its fixed `0x14000`-byte slot, so no other slot
and no PROT offset moves; every stat reads back unchanged. Test:
`crates/patcher/tests/translation_monster_names_real.rs`.

## Fixed cells

Two sections are fixed-width fields with no neighbour to borrow from:

- `party_names`: a fixed 10-byte NUL-padded field, so a 9-byte budget.
- `place_names`: a fixed `0x20`-byte NUL-padded cell, so a 31-byte budget.

## UI and system pools

The `ui_menu` pools (overlay data segments) and the `system_text` pools
(executable) are overwritten in place with the same span-plus-padding budget
as a name, but they never move: code references them directly. They are tight.
A pool is 4-byte aligned with little slack, so a same-size translation of a
short label (`@Items` is six bytes) can be shorter than English but rarely
much longer, and a lifted line that overflows stays English until a
translator abbreviates it. The battle command chips are four to eight bytes
each, so a translation abbreviates. The pools themselves are tabulated on
[`ui-strings.md`](ui-strings.md).

## Dialog

Dialog room is measured per **scene**, not per line.

Same-size in place is the fast default: a shorter translation is space-padded
so the `0x1F ... 0x00` framing (and every script offset around it) never
moves. A dialog line's `budget` is therefore the English line's length - a
hint, not a bound. A line longer than its English grows through the MAN
rewriter, which relocates everything after it; the budget then becomes the
scene MAN's own footprint.

The footprint is the hard limit. Each scene MAN is stored LZS-compressed at a
fixed LBA, and the retail scene entries are sector-aligned with **zero
compressed slack**. So the whole scene's rewritten dialog must recompress no
larger than the original. Translated text is usually less repetitive than the
source, and so compresses worse, even at the same length. When a scene
overflows, lines are rolled back to English, the ones that grow the MAN most
first, until it fits.

Two consequences for pack authors:

- A line over its English length usually still lands, once the scene is
  relocated.
- A set of lines each inside its own budget can still overflow the scene when
  padded, because padding every shorter line back to the English length
  spends bytes a zero-slack footprint does not have.

So a shipped pack is the set of lines a same-size import of its full pack
actually writes, never a per-line budget filter.

`--allow-relayout` lifts the footprint limit by growing an overflowing scene
by whole 2048-byte sectors. The costs: the image grows, so there is no PPF;
and a save state made on a different layout breaks. The rewriter, the
recompression, the rollback order and the relayout are described on
[`dialog-import.md`](dialog-import.md).

## Raw carriers

`inline_text` lines live uncompressed in raw PROT entries:

- In a v12 event-script prescript, a line is a space-padded same-size
  overwrite: its budget is its own span.
- In a streaming dungeon scene, a longer line grows the uncompressed MAN chunk
  instead (relocated like a scene MAN, later chunks shifted) - within the
  entry's sector slack, or by whole sectors with `--allow-relayout`
  ([streaming dungeon scenes](dialog-import.md#streaming-dungeon-scenes)).

Every `raw:` write also passes the
[dialog-carrier gate](dialog-import.md#the-dialog-carrier-gate-raw-writes).

## Byte room is not screen room

The budget is a byte count, not a width: a list still has the column it has.
The pause menu's item list draws the quantity at a fixed column, so an item
name much longer than the longest English one runs into it. Retail never wraps
text at run time either - a dialog row breaks only at `|` or a new segment, and
anything wider than the box runs past its edge. The per-context pixel limits
(dialog row, item / shop / spell lists, name entry, battle lines) and how a
line is measured are in
[`dialog-font.md`](../../formats/dialog-font.md#line-width-and-wrapping);
`legaia_font::Font::measure` and `legaia_font::limits::TEXT_LIMITS` are the
code form.

## Checks before writing

`translate stats` checks all of this offline. On import each target is also
verified against the pack's `source`; a mismatch (wrong disc revision, or a
conflicting randomizer patch that moved the text) skips the entry with a
per-key warning rather than writing blind. A distributable pack, which has no
`source`, is checked by its `budget` hint instead
([two pack shapes](index.md#two-pack-shapes)).

## Seeing your space

`legaia-patcher translate space` reports how much room every translatable
string has and what uses it:

```bash
# The retail picture: room per name region, monster record, UI pool and scene.
legaia-patcher translate space --input DISC.bin
# What a pack uses, and what import would do with every line (a dry run).
legaia-patcher translate space --input DISC.bin --pack legaia_fr.yaml [--allow-relayout]
# One scene only - the fast re-fit an editor runs after each change.
legaia-patcher translate space --input DISC.bin --pack legaia_fr.yaml --scene 383
```

On a disc alone it lists:

- the SCUS name compaction regions and the bytes English uses in each, which
  names may move, and why the rest are pinned (arts description, a shared
  tail, extra pointer words, a `lui` pair);
- each monster record's in-place room (7 / 11 / 15) and growth cap;
- the fixed-room `ui_menu` / `system_text` pools with their slack;
- every scene MAN's compressed size against its on-disc footprint (zero
  slack across the USA disc), and the streaming dungeon scenes' sector slack.

With `--pack` it dry-runs `import` on an in-memory copy and adds, per key, the
encoded length and the outcome (`in_place`, `moved`, `grown`, `relocated`,
`relayout`, `already_applied`, or a skip class such as `over_budget`,
`no_free_run`, `rolled_back`); per name region the bytes left free after the
moves; and per scene the recompressed size, the overflow before rollback, the
rolled-back keys, whether the relocator refused it, and the sectors
`--allow-relayout` would add. Tables list the tightest regions, pools and
scenes first. `--json` prints the full report (schema `legaia-space-v1`,
documented in `translation::space`); `--section` narrows it to one section and
`--verbose` prints every row.

Every number is the importer's own: `import` records what it measures and
decides in `ImportReport::trace`, and the report reads that rather than
re-deriving the rules, so a line the report says fits is a line import writes.
The editor fast paths (`space::scene_fit`, `space::NameFitter`) run the
importer's scene planner and SCUS pass. `crates/patcher/tests/translation_space_real.rs`
checks every predicted outcome against a real import. Output carries keys and
numbers only, never game text.

`translate stats --input` and the ROM patcher's "Check pack against my disc"
give the same verdicts as a per-section summary.

### In the browser: the translation workbench

The site's translation workbench page runs the same kernel in the browser tab
on the user's own disc (`crates/web-viewer/src/translate_workbench.rs`, a
resident `Workbench` session). It shows the report as a dashboard - coverage
per section, the name regions' free bytes, monster rooms, label-pool slack and
the scenes fullest first - next to an editor that checks each line as it is
typed:

- the encoded length against the key's room, with every character the retail
  glyph set lacks marked (`space::encoded_len`'s per-character issues);
- the line's pen width in the disc's own font against the pinned
  [on-screen limit](#byte-room-is-not-screen-room) for its context, with
  substitution tokens resolved through the pack's own names, and a preview of
  the whole dialog box drawn at native resolution;
- after a pause, the scene re-fit (`space::scene_fit`) for a dialog line and
  the SCUS pass (`space::NameFitter`) for a name.

"Check against my disc" runs the whole dry run once. The workbench downloads
the working and the shareable pack; it does not patch a disc (the ROM patcher
does).
