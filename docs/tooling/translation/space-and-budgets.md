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
| `ui_menu`, `system_text` | the string's span in its pool | yes: the string moves and every instruction or word that reaches it is rewritten | stays English, reported `no free run` (or over budget when pinned) |
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
- the `system_text` / `ui_menu` pools, which are reached from code and move
  by their own rules ([below](#moving-a-label)).

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
as a name. A pool is 4-byte aligned with little slack, so in place a short
label (`@Items` is six bytes) can be shorter than English but rarely much
longer; the battle command chips are four to eight bytes each. The pools
themselves are tabulated on [`ui-strings.md`](ui-strings.md).

### Moving a label

A label longer than its span **moves**, like a name, but these strings are
reached straight from code rather than through a table slot, so every
instruction that forms the string's address has to follow it
(`translation::code_refs`, `translation::code_strings`). Per image, import:

1. finds every site that forms the address of a pool string: an aligned
   pointer word (a message table, a screen-element record's payload
   pointer), a `lui` pair completed by an `addiu` / `ori` / load / store and
   followed down every path (a branch forks the walk, a `jal` delay slot still
   sees the register), and a `$gp`-relative instruction;
2. compacts each run of adjacent movable strings, as the name pools are
   compacted, and places the longer strings into the free runs that leaves
   or into the image's spare room (below);
3. rewrites every site to the new address.

Which address a string may take depends on its references. A pointer word
takes any address. A **private** `lui` pair - `lui rX` completed once by an
`addiu rX, rX, lo` / `ori` with nothing reading `rX` between, no branch
leaving or landing in the gap, the `lui` not in a delay slot - has both halves
rewritten, so it can move anywhere. A `lui` whose high half serves several
completions keeps it: only the low half changes, so the new address must have
the same `%hi`. A `$gp` form stays within its signed 16-bit displacement.

A string stays where it is (and the space report names the reason) when:

- nothing in its own image forms its address (`no_reference`): it is reached
  as an offset from something else;
- an adjacent string has no reference (`neighbour_unreferenced`): it may be
  the base that one is reached from;
- something forms an address inside it (`interior_reference`);
- another image that can be resident with it forms its address
  (`foreign_reference`): the executable for an overlay string; the overlays
  for an executable string; the battle overlay and the slot-B battle modules
  for each other;
- it shares bytes with another string (`tail_shared`);
- it is one of three or more strings at one stride, padded past their own
  alignment, with an unreferenced member (`fixed_stride`): a table indexed
  by arithmetic.

The room a moved string can take:

| Image | Room |
|---|---|
| an overlay | its own pools' compaction, and the ledger's translation region in that image (the menu overlay has one) |
| `SCUS_942.54` | its own pools' compaction, and the free runs the name pools' compaction leaves once the names are placed |

A string that finds no room keeps its English text and is reported
`no free run`. Shortening other labels in the same image frees room.

The move is a same-size edit of the image, so a PPF carries it. The disc
oracle `crates/patcher/tests/translation_code_strings_real.rs` imports a
length-shuffled and an all-longer pack and decodes, on the patched disc, every
site that formed each pool string's address on the retail disc: each must now
reach the text import says it wrote. Under PCSX-Redux the moved menu command
labels and a moved `system_text` empty-list message draw from their new
addresses (the menu overlay reloads from the disc when the menu opens; the
executable's edits need `legaia-patcher scus-pokes` over a save state, which
`autorun_menu_screen_dump.lua` applies from `LEGAIA_POKES`).

## Where the text lives

The memory map of every translatable category, and what room it has:

| Category | Carrier and residency | Addressing | Room past its own |
|---|---|---|---|
| `items` (and item types, spells, arts, accessory passives) | `SCUS_942.54` name pools, always resident | a pointer word in each table record | the name regions' compaction ([above](#longer-names)) |
| `monster_names` | the monster archive (PROT 0867), one record per monster, streamed per battle | the record's `+0x00` block-relative offset | the record grows, up to fifteen bytes ([below](#monster-names)) |
| `place_names` | `SCUS_942.54` quick-travel cells, always resident | fixed `0x20`-byte cells | none needed: 31 bytes each |
| `ui_menu` | overlay data segments, resident while their overlay is | `lui` pairs and pointer words in the overlay's code | its pools' compaction; the menu overlay's translation region |
| `system_text` | `SCUS_942.54` system pools, always resident | pointer words, `lui` pairs, `$gp` forms | its pools' compaction; the name pools' free runs |

`place_names` is one of three carriers a place name has; the world-map label
table (a 24-byte field, so 23 characters) and each scene's entry banner are
the others ([`place-names.md`](../../formats/place-names.md)), and
`--rename-location` writes all three.

## Sharing room with mods: the space ledger

Mods need spare bytes too: every hand-assembled code hook in the randomizer
writes a routine into a region the retail game never reads
([`randomizer.md`](../randomizer.md#the-injected-code-arena-budget)). One table,
`legaia_patcher::space_ledger::REGIONS`, lists every such region once with its
owner, and the two owners never share one:

- a **mod** region is written only by the flags that claim it, each of which
  refuses to write unless its region is still all-zero on the disc. Translation
  never places a string in a mod region, even one that is zero on the disc at
  hand, so a mod applied after a language pack still finds its room;
- a **translation** region is used only by relocated strings, and only over the
  bytes still zero on the disc being patched (`space_ledger::translation_spans`),
  so a second import, or anything else already written there, is never
  overwritten.

That is why a language pack and the mods compose in either order: neither
places anything in the other's room. The one crossing is deliberate: a mod
routine that loads a label the pack then moves is itself a reference to that
label, so the import rewrites it like any other, and `--seru-trade` reads the
shop's "Quit" label from the instruction pair that loads it rather than from
its retail address. Import also tries the smallest edit first - only the
growing strings move, into spare room - and compacts an image's pools only
when that leaves one without room. `translation_code_strings_real.rs` applies a
pack and the menu-overlay mods in both orders and checks both. `translate space --verbose` lists every region,
its owner and how much of it is still zero.

The translation region is the menu overlay's zero fill between the save-menu
atlas and the save-slot icon sheet (PROT 0899, `0x801ED340..0x801EE120`).
No instruction in any image forms an address inside it, neither save-screen
card buffer reaches it, and it reads zero in every library capture with the
overlay resident - the title's card load, a shop, the pause menu. A moved
menu label is read only while its overlay is resident, and the overlay loads
whole (the loader reads the entry's full sector extent), so the region is
there whenever the label is.

Two menu-overlay mod regions sit inside the save screen's card buffers
([`save-screen.md`](../../subsystems/save-screen.md#which-buffer-the-sum-runs-over)):
the `--show-super-arts` description run (`0x801E65F4..`) inside the card-read
buffer `0x801E5120..0x801E7120`, and run-C (`0x801E74E0..`, `--seru-trade` and
`--show-super-arts`) inside the save compose buffer `0x801E7120..0x801E9120`.
The description run reads non-zero in library captures of the title's card
load. Both features read their bytes only while the overlay was reloaded for
the shop or the pause menu, which is why they work, but the ledger records the
overlap and translation never uses either region.

### What composes with what

| Growth | Room it uses | Mods writing the same image | Composes |
|---|---|---|---|
| longer names (`items` sections) | SCUS name pools | `--delilas-challenge` points three custom item names at code caves outside the pools | yes |
| longer `system_text` | SCUS system pools, then the name pools' free runs | every SCUS mod arena (`--shiny-seru`, `--equipment-drops`, `--flee-exp`, `--enemy-ally`, `--seru-trade`, `--delilas-challenge`, `--show-super-arts`) | yes: the arenas are never used for text |
| longer menu-overlay labels | 0899 pools, 0899 translation region | `--seru-trade`, `--show-super-arts` (run-C, the description run) | yes |
| longer battle-overlay labels | 0898 pools | `--enemy-hp-bar`, the arts hooks (code outside the pools) | yes |
| longer monster names | the record, inside its `0x14000` slot | `legaia-patcher monster-model` re-packs the same block; `--monster-stats` / `--enemy-stat-scale` write fields in it | yes, while the re-packed block fits its slot |
| longer place names | none (31-byte cells) | `--rename-location` writes the same cells | the later write wins |

## Room that is not used, and why

Each place a larger text pool could live was measured:

- **The SCUS mod arenas** are the only verified-dead executable bytes outside
  the live tables, and the code hooks already claim them; see
  [`randomizer.md`](../randomizer.md#the-injected-code-arena-budget) for why
  there is no further region to grow into.
- **An overlay's uninitialised data** is written at runtime. The menu
  overlay's two card buffers are the measured case above; a zero run is dead
  only when nothing forms an address in it and nothing reaches it from a base
  below it, and only a capture of every state that uses the overlay says the
  second.
- **The `DMY.DAT` annex** holds records the player-file loader streams by
  descriptor offset. Text read by address needs a RAM window that is resident
  whenever the string is drawn; the executable has no free window of that
  size and the overlay slots belong to the overlays, so a pool there would need
  a new loader and a new home at once.
- **A larger overlay entry** grows only through the whole-sector relayout (no
  PPF), and the bytes it adds load after the image, where the slot's other
  occupants live.

So the room is where the importer already looks: bytes the English layout
wastes (compaction), bytes a shorter translation gives up, and the one
reserved translation region.

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

- a **room per category** table for the five categories a translator and a
  modder share room in (`items`, `monster_names`, `place_names`, `ui_menu`,
  `system_text`): the strings, how many can grow, the bytes English uses and
  its room, the free bytes a longer string can still take, the spare bytes the
  [space ledger](#sharing-room-with-mods-the-space-ledger) reserves, where the
  strings live, how the game reaches them, and (`--verbose`) which mods write
  the same room;
- per code image (the executable and each overlay with a `ui_menu` pool), how
  many strings can move and why the rest are pinned; `--verbose` adds every
  ledger region, its owner, and the bytes still zero on this disc;
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
documented in `translation::space`, with `categories`, `code_images` and
`spare_regions` beside the per-key rows); `--section` narrows it to one section and
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
per section, the room per category (with what your pack leaves once checked),
the name regions' free bytes, monster rooms, label-pool slack and the scenes
fullest first - next to an editor that checks each line as it is
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
