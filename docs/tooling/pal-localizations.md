# Official PAL localizations (structure + alignment)

Three official PAL localizations of *Legend of Legaia* exist alongside the
NTSC/USA reference disc. This page documents their structure, how they align to
the USA disc coordinate space, and the encoding of their accented text - the
groundwork for lifting the official French / German / Italian translations into
the [translation pipeline](translation.md). It contains **no game text** (byte
values, offsets, counts and encodings only).

The cross-region measurement tool is `legaia-rando translate diff-disc`
(`legaia_rando::translation::diff`); it is region-agnostic and emits counts and
byte values only.

## Region ids

| Region | Boot exe (`SYSTEM.CNF`) | exe `t_addr` |
|---|---|---|
| USA (NTSC, reference) | `SCUS_942.54` | `0x80010000` |
| France (PAL) | `SCES_019.44` | `0x80010000` |
| Germany (PAL) | `SCES_019.45` | `0x80010000` |
| Italy (PAL) | `SCES_019.46` | `0x80010000` |

All are `PS-X EXE`; the PAL exes are 2-4 KB larger (extra code), so `pc0` and
the data segment shift up relative to USA.

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

`legaia-rando translate lift-official --from <PAL.bin> --target <USA.bin>
-o <pack.yaml>` re-keys the official localized text onto the USA coordinate space
the importer patches (`legaia_rando::translation::lift`):

1. Detect the source region from `SYSTEM.CNF`'s `BOOT` line (SCES_019.44/.45/.46
   = FR/DE/IT).
2. *Locate* each of the five name-table bases in the SCES exe, verified against
   the **USA-populated id set** (a candidate base is accepted when the same ids
   the USA table names also resolve to name-shaped strings on the PAL exe - a
   count-agnostic, language-independent check with a windowed-search fallback).
3. Re-key positionally: name tables id-for-id (`usa_string_va -> pal_string`),
   dialog by the `diff-disc` Nth-segment-per-entry pairing, party names by fixed
   field.

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
`crates/rando/tests/translate_lift_official_real.rs`.

### What a lift does not cover

The overlay-resident `ui_menu` pools (pause-menu / options / shop / battle
system labels) are **not** lifted. They are pinned by USA-coordinate VA windows
(`translation::ui`) and the PAL overlays place their pools elsewhere, so a lift
leaves those entries empty and the labels stay English. The name tables, the
party roster, and the whole `0x1F` dialog corpus are covered.

## Fit rate against the USA target

`legaia-rando translate fit-report --from <PAL.bin> --target <USA.bin>` measures
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
relayout. Disc-gated oracle: `crates/rando/tests/translate_relayout_import_real.rs`
(all three PAL languages) asserts the patched image re-parses, every relocated
sector is EDC/ECC-valid + MSF-correct, and every applied line is present at full
length. See [disc.md](../formats/disc.md#full-iso-relayout) for the reference
graph.

The 22 scene MANs the generalized rewriter cannot grow at all (an absolute-ref op
/ section-region segment / validate divergence in `apply_text_edits`) still fall
back to abbreviation - a limitation of the in-MAN rewriter, orthogonal to the
relayout, which supplies the room but cannot produce grown bytes for them.

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

1. `translate lift-official` -> working pack (scratchpad only).
2. `translate fit-report` -> the residual budget picture.
3. `translate import --allow-relayout --output <patched.bin>` -> byte-faithful
   dialog for every non-structural MAN (no `--patch`: a relayout grows the image,
   so it is not a same-size PPF overlay).
4. Font patch (separate deliverable) so accents render instead of folding to
   ASCII.
5. `translate strip` -> a source-free distributable `site/lang/{de,fr,it}.yaml`.
