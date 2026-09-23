# Official PAL localizations (structure + alignment)

Three official PAL localizations of *Legend of Legaia* exist alongside the
NTSC/USA reference disc. This page documents their structure, how they align to
the USA disc coordinate space, and the encoding of their accented text - the
groundwork for lifting the official French / German / Italian translations into
the [translation pipeline](translation.md). It contains **no game text** (byte
values, offsets, counts and encodings only).

The cross-region measurement tool is `legaia-patcher translate diff-disc`
(`legaia_patcher::translation::diff`); it is region-agnostic and emits counts and
byte values only. Its two discs are named `--input` (the target the importer
would patch, i.e. USA) and `--other` (the disc aligned against it) - not the
`--from` / `--target` pair its `lift-official` / `fit-report` siblings take:

```bash
legaia-patcher translate diff-disc --input <USA.bin> --other <PAL.bin>
```

## Region ids

| Region | Boot exe (`SYSTEM.CNF`) | exe `t_addr` | Name-table bases |
|---|---|---|---|
| USA (NTSC, reference) | `SCUS_942.54` | `0x80010000` | the coordinate space itself |
| France (PAL) | `SCES_019.44` | `0x80010000` | measured, pinned |
| Germany (PAL) | `SCES_019.45` | `0x80010000` | measured, pinned |
| Italy (PAL) | `SCES_019.46` | `0x80010000` | measured, pinned |
| Spain (PAL) | `SCES_019.47` | - | unmeasured, located at run time |
| Europe, English (PAL) | `SCES_017.52` | - | unmeasured, located at run time |

All measured builds are `PS-X EXE`; the PAL exes are 2-4 KB larger (extra
code), so `pc0` and the data segment shift up relative to USA. The Spanish and
EU-English discs were not available when this page was measured; the lift
accepts them and finds their tables by search (below), and a fan-patched disc
of any of these builds - the USA disc included - is lifted the same way. The JP
discs (`SCPS_*`) are refused: their text is not the Latin codec.

### Mastering-metadata quirks

Two oddities in the PAL masters' identification metadata (the site's
disc-identity panel, `site/js/disc-info.js`, reads both fields):

- **The PAL exes carry the NORTH AMERICA region mark.** The `PS-X EXE`
  header string at `+0x4C` reads `Sony Computer Entertainment Inc. for
  North America area` on all three PAL discs. The system-area license text
  (sectors 0..15) is the authority - it correctly says
  `Sony Computer Entertainment Euro pe`, and it is what the console
  validates. Region identification must not trust the exe header string.
- **France's PVD creation date has a zeroed year** (`0000-04-04`); USA's is
  well-formed. Parsers should treat a `0000` year as absent, not as data.

## Structural parity with USA

The PAL discs are **1:1 with USA at the container level**:

- identical ISO 9660 file tree (`SYSTEM.CNF`, the boot exe, `PROT.DAT`,
  `DMY.DAT`, `CDNAME.TXT`, `MOV/MV1..6.STR`, `XA/XA1..34.XA`);
- `PROT.DAT` header identical (`file_num=1236`, `header_sectors=3`,
  **1233 usable entries**), identical TOC head, same scene-block boundaries;
- `CDNAME.TXT` byte-identical USA↔Germany; France/Italy differ only in a few
  in-place label bytes (same `#define` block structure, same 1233-index space);
- ~95% of PROT entries are byte-identical in size to USA (51-64 of 1233 differ);
  nearly every difference is exactly **+1 sector** (a scene whose localized MAN
  recompresses slightly larger), with a few larger deltas in the audio /
  `battle_data` / overlay carriers. No entry is added, removed, or reordered.

**Consequence: a USA PROT coordinate names the same logical asset on every PAL
disc** - entry `i` (scene block, MAN, overlay) is the same thing on all four.
The relative-`entry_disc_lba` check aligns only a fraction by index and is *not*
a parity signal: the sector-growth of the ~5% differing entries cumulatively
shifts every later absolute LBA even though the TOC index space is identical.

### Disc size delta

The PAL discs are ~34 MB larger than USA. The growth is almost entirely
**XA streamed audio** (`XA/*.XA`, ~+30 MB - PAL 50 Hz re-timing); `MOV/*.STR`
and `DMY.DAT` are byte-identical, `PROT.DAT` grows ~150 KB, the exe ~4 KB, and
the remainder is ISO sector overhead / padding. The localization did not
restructure or re-author assets.

## What the executables do not share

The 1:1 parity above is a statement about the **containers**. The boot
executable is not code-identical, and two of the differences change play:

- **No battle-load stat boost.** `SCUS_942.54`'s `FUN_80054CB0` multiplies
  ATK / UDF / LDF / INT as it installs each enemy (two profiles keyed on the
  boss switch). Every PAL executable copies the record in untouched - see
  [battle.md](../subsystems/battle.md#no-boost-on-the-pal-executables).
- **Larger victory spoils.** The PAL spoils routine has neither the NTSC-U
  second gold halving nor the 3/4 EXP cut: half the record gold and the whole
  record EXP reach the party - see
  [battle-formulas.md](../subsystems/battle-formulas.md#regional-difference---the-pal-executables-pay-more).

Both are code-only: the monster archive (`PROT 0867`) reads the same stats and
rewards on all four discs. The JP original (`SCPS_100.59`) behaves like PAL on
both counts, so both are NTSC-U additions rather than PAL removals. Anything that fingerprints a disc by its executable
bytes (`scus-pokes`, the static-overlay map, the port-catalog denominators) is
USA-only by construction; a PAL image needs its own bases.

## Name-table alignment (SCES data segment)

The five SCUS name tables (`docs/formats/item-table.md`, `spell-table.md`,
`art-data.md`, `accessory-passive-table.md`, `new-game-table.md`) exist in each
SCES exe at **shifted, language-specific VAs**. The pointer-table region shifts
by roughly a constant per language (France `+0x8E0`, Germany `+0xFF4`, Italy
`+0xDC4`) with small local drift, so each table must be *located* (fingerprint
its language-independent stats/meta columns against USA), not shift-computed:

| Table (USA VA) | France | Germany | Italy |
|---|---|---|---|
| item names `0x8007436C` | `0x80074C4C` | `0x80075360` | `0x80075130` |
| spell/magic `0x800754C8` | `0x80075DA8` | `0x800764BC` | `0x8007628C` |
| Tactical Arts `0x80075EC4` | `0x800767A4` | `0x80076EB8` | `0x80076C88` |
| accessory passive `0x8007625C` | `0x80076B3C` | `0x80077250` | `0x80077020` |
| new-game party `0x80078C4C` | `0x80079508` | `0x80079C78` | `0x80079A14` |

Record layout (stride, count, field order) is unchanged from USA, and each
record carries the VA of its own string, so **`id N ↔ localized name for id N`
is a clean id-for-id mapping** once the base is located. The string *pool*
itself repacks per language (localized strings differ in length), so string VAs
are not a constant offset from USA - only the pointer tables are followed.

## Dialog-corpus alignment

The `0x1F`-segment dialog corpus (scene-bundle MANs + raw event-script
carriers, `docs/formats/mes.md`) is walked by PROT entry index - the same index
space on every disc. Segment byte *offsets* never match between USA and PAL (a
localized string has a different length, so the decompressed MAN repacks), but
the line *order* is the script's, not the text's, so **lines pair by position**:
the Nth qualifying segment of entry `i` on USA corresponds to the Nth on the
PAL disc.

Measured with `diff-disc` (PAL-tolerant scan on both discs): the corpus totals
match within ~1% and **~99% of lines are order-pairable per entry**, with ~1.5%
needing reconciliation (scanner-marginal short runs, coincidental hits, the
occasional localizer line split/merge). The strict "whole-entry segment count
must match exactly" metric reads far lower (one marginal disagreement fails an
entire 300-line scene) and is only a conservative lower bound.

## Accented-text encoding

The PAL discs keep the **same markup/control framing** as USA: `0x1F` segment
lead, `0x00` terminator, the same 2-byte opcodes (`0xC1..0xC5` substitution,
`0xCE` spacing, `0xCF` colour). Only the glyph atlas is extended above `0x7E`.

**Accented Latin is a single high byte on a CP437-aligned layout.** The
byte→glyph mapping is IBM CP437 for the lowercase accents and the capitals CP437
carries:

| byte | glyph | | byte | glyph | | byte | glyph |
|---|---|---|---|---|---|---|---|
| `0x80` | Ç | | `0x8A` | è | | `0x94` | ö |
| `0x81` | ü | | `0x8B` | ï | | `0x95` | ò |
| `0x82` | é | | `0x8C` | î | | `0x96` | û |
| `0x83` | â | | `0x8D` | ì | | `0x97` | ù |
| `0x84` | ä | | `0x8E` | Ä | | `0x99` | Ö |
| `0x85` | à | | `0x90` | É | | `0x9A` | Ü |
| `0x87` | ç | | `0x93` | ô | | `0xE1` | ß |

Capital-accented glyphs CP437 lacks occupy a small **game-specific block around
`0xD0..0xD6`** (e.g. Italian `È` at `0xD4`). None of the accent bytes fall in
the `0xC0..0xCF` two-byte-opcode window, so glyph space and control space stay
disjoint (`ß`=`0xE1` is safely above it). Per-language accent subsets: German
needs 7 cells (ä ö ü ß Ä Ö Ü), French ~14, Italian ~10; the union is ~40 cells.

### Font-patch scope for NTSC

The NTSC dialog-font atlas already indexes cells `0x20..=0xFF` (16×14 tile page;
menu-glyph atlas at `PROT.DAT` offset `0x11218`, plus the VRAM dialog font - see
[`dialog-font.md`](../formats/dialog-font.md), [`boot.md`](../subsystems/boot.md));
the high cells simply carry no glyph in the USA build. Rendering official PAL
text on NTSC therefore needs **no structural change** - only (1) drawing the
~40-cell accented-glyph union into the existing high cells and (2) setting each
new cell's width byte in the font width table (`SCUS 0x80074050`). This is the
concrete form of the "accented scripts need a font patch" caveat in
[`translation.md`](translation.md).

## Lifting an official translation

`legaia-patcher translate lift-official --from <PAL.bin> --target <USA.bin>
-o <pack.yaml>` re-keys the official localized text onto the USA coordinate space
the importer patches (`legaia_patcher::translation::lift`):

1. Detect the source build from `SYSTEM.CNF`'s `BOOT` line (the table under
   [Region ids](#region-ids)); a measured build brings pinned bases, an
   unmeasured one none.
2. *Locate* each of the five name-table bases in the source exe. A pinned base
   is accepted when it validates against the **USA-populated id set** (the same
   ids the USA table names also resolve to name-shaped strings on the source
   exe - a count-agnostic, language-independent check), else a `+-0x2000`
   window around it is searched. An unpinned base is searched from the USA VA
   (`-0x1000..=+0x4000`, covering the measured PAL drifts and the JP shift).
   Pointer validity alone does not pick a base: the item table's
   `[ptr, ptr, meta]` records read as the accessory table's `[meta, ptr, ptr]`
   four bytes in, and one record off the true base validates on every populated
   id but the first. A search therefore ranks the validating candidates by how
   many of their **meta bytes** (every record byte outside the pointer words:
   stats, ids, scope) equal the USA table's - the same on every build - and the
   measured PAL discs vouch for it: the unpinned search lands on every pinned
   base (`translate_lift_official_real.rs`). The party template is found by
   the same fingerprint (its eight stats per record).
3. Re-key: name tables id-for-id (`usa_string_va -> source_string`), party
   names by fixed field, and dialog **structurally** - each MAN (a scene
   bundle's, or the uncompressed one leading a streaming dungeon scene) is
   walked record by record with the field-VM disassembler, and a line's
   coordinate is `(record ordinal, ordinal among that record's text leads)`,
   the same on every build because the script is one program with different
   strings. The scan-ordinal pairing `diff-disc` reports (the Nth qualifying
   segment of an entry against the Nth) is only the fallback for a line the
   walk does not place - an operand run, text past a record's first decode
   error, an entry whose MAN has a different record shape on the source disc.
   The distinction is not academic: the Spanish disc renders many a chest
   line as a bare `{c2:xx}` item token, which fails the segment quality gate
   on that side only, and from there every scan ordinal in the scene names the
   *previous* line - a shift the pair counts never show, and one the lift
   report now measures (`structural` / `by scan ordinal` / `shifted`). Both
   scans use the accent-tolerant gate on both sides, so the coincidental
   high-byte hits in the binary regions both discs share land on both lists.
   A raw carrier outside the ten streaming dungeon scenes has no walk, and a
   line one side reduced to a bare item token drops out of that side's gated
   scan; what still pairs it is the entry's **ungated** framing list (every
   `0x1F .. 0x00` run), used only when both discs count the same number of
   framings and only where the script byte ahead of both leads agrees.

`--fold-accents` additionally rewrites the accent cells onto plain ASCII, so the
lifted text renders on an unmodified NTSC font (see
[Accent folding](#accent-folding) below).

It emits a **filled working pack** (source = USA text, translation = official
localized text, USA byte budgets) - Sony text, kept local, never committed.
Across FR/DE/IT all four pooled tables locate at 100% valid fraction with zero
unmapped strings, and the dialog corpus pairs at **98.5-99.8%** per PROT entry.
Accents decode to single-byte `{xx}` markup escapes the codec round-trips
exactly; they still need a font patch to render.

### Accent folding

Lifted text keeps the PAL accent bytes verbatim (the markup codec round-trips
them as `{82}`-style escapes), and they encode onto the USA disc without
complaint - but the NTSC build has no glyph in those cells, so they draw blank
until the atlas is patched. `translate lift-official --fold-accents` (and the
in-browser transfer, where it is the default) folds the accent block onto the
plain-ASCII glyphs the USA font does have: `é` -> `e`, `ß` -> `ss`, `Ü` -> `U`,
across the CP437 layout above plus the `0xD0..=0xD6` capitals. Folding is
one-byte-for-one-byte except `ß`, which grows a line by one byte and can push a
tight line over budget.

The fold deliberately leaves the **non-accent** high cells alone: the retail
atlas uses a few symbol cells above `0x7E` (they occur in the USA disc's own
spell names), so those bytes already render and rewriting them would lose a
glyph. Both counts are reported - folded and left-raw - so nothing changes
silently. Implementation: `translation::markup::fold_high_glyphs` /
`translation::lift::fold_pack_accents`; disc-gated oracle
`crates/patcher/tests/translate_lift_official_real.rs`.

### How the string pools pair

The overlay-resident `ui_menu` pools, the `system_text` SCUS strings and the
`place_names` cells are lifted too, by a pairing of their own. A pool is
pinned by a USA-coordinate VA window (`translation::ui`), and the same pool on
the other build sits near the same file offset (an overlay's data segment
opens the same way on every build; a SCUS window is carried by the located
name tables' drift) but not at it - its strings are longer or shorter. Both
windows are cut into NUL-delimited chunks, junk included, and chunks that are
byte-identical and unique on both sides (a stat label, a proper noun, a run of
build-invariant pointer bytes) are **anchors** that partition the two lists;
inside a partition the chunks pair by ordinal when both sides count the same,
and otherwise by an in-order alignment (`align_chunks_dp`): the source side
may carry extra chunks anywhere, a pair must agree in shape (string against
string, pointer bytes against pointer bytes, length within the ratio a
translation stays inside), and an identical pair outranks a pair whose
leading glyph class agrees (the `@` of a menu label), which outranks the
rest, with the length closest to a translation's usual growth breaking ties.
A head or tail alignment is the wrong model for a real pool: the Spanish menu
overlay carries an extra variant of one label in the middle of the shop
strings (`Tienes ` ahead of `@Tienes `), and aligning from the head slid every
later label by one. "String" is its own test here, not the dialog gate: a
UI string may abbreviate with a `/` (`p/ equipar`) or be one punctuated word
behind a control token (`{ce:13}Pernas.`), both of which the dialog gate
refuses, and one refused string on one build failed the whole pool. A chunk
the window's end cuts short is dropped rather than paired, and where a SCUS
pool can sit at either of two offsets the window sharing more
build-invariant chunks with the USA one wins before the pair count is
compared (an in-order alignment pairs *some* text in any window, including
the item descriptions a wrong window lands on). An unpaired label stays
vanilla. The place-name table is found by its first cell, the home town's
name.

### Lifting a fan translation

A community translation shipped as a **binary patch** (xdelta, PPF) cannot be
turned into a pack by reading the patch - a patch is bytes, not text. The disc
it produces can: apply the patch to the disc it was built for (the patch's own
header names it - an xdelta's `printhdr` prints the source filename; its
adler32 is over the *target* window, so it verifies the output, not the
source), then

```bash
legaia-patcher translate lift-official --from <patched.bin> --baseline <retail.bin> \
    --target <USA.bin> --language pt-BR --fold-accents -o <pack.yaml>
```

`--language` stamps the pack: a patch's language is not on the disc, so the
lift would otherwise use the build's own code (`es` for a patched Spanish
disc, `en` for a patched USA one). `--baseline` names the retail disc the
patch was built on and blanks every line whose text that disc also carries -
at the same key, or anywhere in the same PROT entry (a line that pairs by scan
ordinal can pair a retail line on the patched disc and its neighbour on the
retail one). Without it, lines the patch left untranslated lift as the
underlying build's official text (Spanish under a partial Portuguese patch)
and the pack is not publishable; with it, what survives is the translator's
own text and those lines stay vanilla on import. A line with **no prose of
its own** - only control tokens and punctuation, such as a chest line the
patch reduced to its item token - is kept even when the baseline carries it:
the retail disc has the same bytes because the construction is the same in
both languages, not because the translator skipped the line, and blanking it
put the English line (`the <item>!`) back into the middle of a translated
sentence. On the Brazilian Portuguese patch that is 633 lines. Everything else is the
official-lift path: a patched USA disc keeps the USA bases (drift `0`, found
by the same search), a patched PAL disc keeps its build's. The baseline
comparison runs on the raw bytes and the fold after it, so a line the patch
only re-accented is the translator's work and survives. A patched font atlas
draws its own cells: the fold covers the CP437 accent block and the CP850
cells a Portuguese patch redraws over CP437's box-drawing rows (the ordinal
indicators, the inverted punctuation, the accented capitals), and any cell
outside both comes through as a raw `{xx}` escape - both counts are reported,
and a line still carrying one after the fold is best dropped than shipped,
since the USA atlas draws nothing there. The lifted text also loses its
trailing pad spaces (a same-size patch pads a shorter line to its slot); they
draw nothing and only cost budget on the target. A translation
that rewrote the game's structure (moved PROT entries, re-laid the ISO) still
lifts as long as the PROT TOC on the patched disc is consistent - the walk is
by entry index, and the entry sizes come from that disc's own TOC.

The Spanish disc measured this way: every table locates at 100% pointer
validity with a `+0xDF4` drift (between Italy's and Germany's), the party
template fingerprint matches, and the dialog corpus pairs at 97.8% (MAN) /
99.1% (raw) - the same band as the measured three.

## Fit rate against the USA target

`legaia-patcher translate fit-report --from <PAL.bin> --target <USA.bin>` measures
fit under two budgets (counts only, no text):

- **per-string** (the old same-size constraint): a line fits iff its encoded
  bytes are `<=` its own USA segment span. ~48-51% of MAN dialog lines fit; name
  tables fit 36-60% (Italian, the wordiest, lowest).
- **per-MAN** (the generalized rewriter): a whole scene MAN fits iff *all* its
  official lines, grown to full length, relocate + validate + recompress within
  the MAN's on-disc compressed footprint at the same LBA (no disc relayout - see
  [man-relocation.md](../formats/man-relocation.md)).

**The decisive constraint is sector alignment, not string length.** The USA
scene-bundle PROT entries are sector-aligned with **zero** compressed slack, and
each compressed MAN already fills its footprint, so growing *any* line overflows
and whole-MAN in-place growth fits only a small fraction (3.5-7.1% of MAN lines,
9-13 of 79 MAN entries). The residual is **not** a few long lines to abbreviate:
it is ~65-70 large scene MANs (holding most of the corpus) that each need a
**sub-sector** amount of extra compressed room - **every residual deficit is
under one 2048-byte sector** (max ~1.4 KB across all three languages). This is
exactly the **+1-sector-per-entry** growth the PAL discs applied at mastering.

### Full-ISO relayout (closes the residual)

`translate import --allow-relayout` grows each residual sector-crossing scene MAN
by whole sectors so its full-length dialog imports **byte-faithfully** instead of
being abbreviated - the same operation the official PAL discs did at mastering.
This is safe because of the disc's reference graph (below), not by luck.

**Why a relayout is safe.** Two structural facts (proven by diffing USA against
all three PAL discs):

- The `PROT.DAT` internal TOC stores **PROT.DAT-relative** LBAs, not absolute disc
  LBAs (entry-0's TOC start is identical on every disc despite `PROT.DAT` sitting
  at a different disc LBA). Growing an interior entry needs only an internal-TOC
  start-LBA shift, not a disc-wide LBA cascade.
- **No file is located by a hardcoded absolute LBA** in the executable: every file
  (`PROT.DAT`, the boot exe, `XA*`, `MV*`, `DMY.DAT`) is found by ISO9660
  name/directory lookup - no post-`PROT.DAT` file's disc LBA appears as a
  little-endian literal in any USA or PAL executable. Shifting files after
  `PROT.DAT` is safe once the directory records / path tables / PVD are fixed.

**The cascade.** When `PROT.DAT` grows by `G` sectors, the complete set of
references reduces to one rule: **every ISO9660 LBA value `> prot_lba` gains `+G`;
`PROT.DAT`'s directory-record size gains `+G*2048`; the PVD volume-space size
gains `+G`.** Concretely: the PROT-relative internal TOC start LBAs of entries
after a grown one; the single PVD's volume space (LE @80 + BE @84); the LE/BE path
tables (the `MOV`/`XA` directory extents live after `PROT.DAT`); the root +
`MOV` + `XA` directory extents' file-record LBAs (+ each moved directory's self
`.` record). Every sector after `PROT.DAT` is relocated - its sync + MSF header
(`BCD(lba+150)`) rewritten - but Form 1 EDC/ECC do **not** cover the header, so a
pure relocation needs no ECC recompute; EDC/ECC are recomputed only for sectors
whose user data changes.

**Layers.** The disc-level relayout is `legaia_iso::relayout::grow_prot_dat`
(generic ISO9660 + ECMA-130; embeds no game bytes) driven by
`DiscPatcher::grow_prot_entries` (rebuilds `PROT.DAT` with the PROT-relative TOC
shift). Above it, the importer builds each grown scene-bundle payload: because a
scene MAN (asset type `0x03`) is never the last sub-asset, growing its compressed
footprint inserts `ceil(deficit/2048)` sectors after the MAN, shifts every later
sub-asset, and bumps their `scene_asset_table` descriptor `data_offset`s (+ the
MAN decompressed-size word). The PROT entry **index space is preserved**, so every
same-size index-keyed edit (the randomizer features) still resolves after a
relayout. Disc-gated oracle: `crates/patcher/tests/translate_relayout_import_real.rs`
(all three PAL languages) asserts the patched image re-parses, every relocated
sector is EDC/ECC-valid + MSF-correct, and every applied line is present at full
length. See [disc.md](../formats/disc.md#full-iso-relayout) for the reference
graph.

What the rewriter still refuses is not a scene but a handful of keyed lines
whose `0x1F <text> 0x00` framing is a coincidence inside an instruction's
operands (an actor index `0x1F` followed by printable bytes and a zero -
`man_edit::text_site` reports them `Operand`); the exporter emits them like any
segment, the lift pairs them with whatever the other disc has at the same
place, and the importer skips them on every path with a diagnostic. Every
genuine line is relocatable: a clean walk of its record reaches it, so the
references that cross it are known. A scene that once read as "structural" was
one of three decoder gaps - a bare text segment ending the walk, a partition-0
record walked from the partition-1 header formula, or a mis-sized op (`34 2x`,
the shop form of `49 00`, `4C 14`, `4C D2`) - and each is closed in
[`man-relocation.md`](../formats/man-relocation.md#generalized-interior-text-growth).

The `raw:` corpus has no such residual. Every raw line on the disc lives in the
record region of the uncompressed MAN that leads one of the ten streaming
dungeon scenes (`data_field_streaming`: `dolk2`, `rikuroa`, `rayman`,
`station`, `balden2`, `ropeway2`, `taiku`, `doman`, `taiku2`, `nilboa2` -
[`data-field.md`](../formats/data-field.md)), so the importer rewrites that
MAN with the same relocator, re-headers the chunk and shifts the chunks behind
it (`translation::stream_man`). Growth lands in the entry's own trailing sector
slack when it fits - a same-size-image write, PPF-safe - and otherwise in the
relayout. The bound is the loader's `0x62C00`-byte asset arena the entry is
block-copied into, whose top the VDF morph applier borrows as scratch; the
importer keeps a 64 KiB headroom under it. Unlike the LZS MANs there is no
mastered precedent: the Spanish disc's text for these ten scenes fits the USA
footprints, so none of them grew at mastering.

### In the browser

The site's [ROM patcher](../../site/_content/tooling/rom-patcher.html) exposes
the same lift as a second language path: the visitor picks *"Official
translation from my own PAL disc"*, supplies their PAL `.bin` alongside their
USA one, and the WASM entry point `lift_official_pack(usa, pal, fold_accents)`
runs the lift **in the tab** - neither disc is uploaded, and the lifted pack
lives in page memory (downloadable, since it is the visitor's own disc text).
The result is handed straight back to `patch_rom`'s `lang_pack` argument, so it
inherits the documented dialog-before-randomizer / names-after ordering and the
same per-section coverage report every other pack gets. Accent folding is on by
default there; the relayout path is CLI-only (it grows the image).

The honest headline for that path is this page's fit numbers: roughly a third of
the dialog corpus lands in place, the rest is reported as over-budget or
non-recompressing rather than dropped quietly.

### Recommended path to a distributable pack

1. `translate lift-official --from <PAL.bin> --target <USA.bin> -o <pack.yaml>`
   -> working pack (scratchpad only).
2. `translate fit-report --from <PAL.bin> --target <USA.bin>` -> the residual
   budget picture.
3. `translate import --input <USA.bin> --pack <pack.yaml> --allow-relayout
   --output <patched.bin>` -> byte-faithful dialog for every scene MAN
   (no `--patch`: a relayout grows the image, so it is not a same-size PPF
   overlay).
4. Font patch (separate deliverable) so accents render instead of folding to
   ASCII.
5. `translate strip --pack <pack.yaml> -o site/lang/<lang>.yaml` -> a source-free
   distributable pack (`site/lang/{de,fr,it}.yaml`).
