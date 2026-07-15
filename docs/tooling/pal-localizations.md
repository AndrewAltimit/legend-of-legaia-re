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

## Same-size fit against the USA target

The importer enforces same-size in place against the USA disc. Official PAL
strings are frequently wordier:

- **Dialog** (budget = exact USA segment byte length): ~half of order-paired
  lines fit; the rest overflow by ~8 bytes on average (max ~40). Dialog has no
  per-line resize (segment pools interleave with script bytecode whose relative
  jumps assume fixed offsets), and `man_edit::apply_insertions` is scoped to
  partition-2 door records - it does **not** generalize to arbitrary interior
  dialog growth. The practical path is abbreviating the flagged overflow lines
  (the importer already rolls back a scene's longest lines one at a time with a
  per-key diagnostic).
- **Name tables** (budget = USA string span + 0..3 alignment padding, which the
  pack reclaims): French/German names fit ~70-76%; Italian names (wordiest) ~48%;
  accessory descriptions are worst (15-27%).

## Lifting an official translation (recommended path)

1. Export region-agnostically: accept a non-`SCUS`-named boot exe, *locate* the
   five name-table bases per exe, and use the PAL-tolerant dialog scan. Produces
   a working pack keyed by the PAL disc's own coordinates (Sony text - keep it
   local, never commit).
2. Re-key to USA coordinates using the `diff-disc` positional pairing (id-for-id
   for names, Nth-segment-per-entry for dialog) + USA budgets.
3. Budget pass (`translate stats --input USA.bin`); abbreviate the ~50% of
   over-budget lines.
4. Font patch (separate deliverable) so accents render instead of folding to
   ASCII.
5. `translate strip` → a source-free distributable `site/lang/{de,fr,it}.yaml`.

Steps 1-2 are the bulk of new code and are a natural follow-up
`translate lift-official` subcommand built on the `diff-disc` pairing.
