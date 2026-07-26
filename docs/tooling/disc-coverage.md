# Disc coverage

How much of the game's own bytes the project can account for, measured against
the disc rather than against the project's own notes.

## Why this exists alongside the port catalog

[`port-catalog.py`](port-catalog.md) tracks three status columns - `dumped`,
`documented`, `ported` - over the set of addresses this project **cites**. That
is the right instrument for steering work, and it answers "what is left on the
list". It cannot answer "how much of the game is left", because its denominator
is the citation graph: an entire subsystem that nothing cites is invisible to
it, and the page can read as near-complete while that subsystem sits outside the
measurement entirely.

`scripts/ci/disc-coverage.py` takes the denominator from the disc.

The distinction is not academic. Against the citation graph the dump corpus
reads as effectively closed - zero cited-but-not-dumped addresses. Against the
executable's own bytes, a sixth of `SCUS_942.54`'s code is not inside any dumped
function at all.

## The two halves measure different things

This is the one thing to carry away before quoting a figure.

| | Kind | What a percentage means |
|---|---|---|
| **Code** | byte-exact | a byte is inside a dumped function, or it is not |
| **Data** | format recognition | an entry's format class is known; its bytes are not individually accounted for |

The data figure is an **upper bound**. Knowing an entry is a `scene_vab_stream`
is not the same as consuming every byte inside it, and no parser in the tree
reports consumed-versus-unconsumed bytes. Closing that gap - having each parser
return its consumed extent - is what would put the data half on the same footing
as the code half.

### The data denominator counts some disc bytes more than once

The data half weights each entry by the size of its extracted `.BIN`, and that
size is `max(indexed_size_sectors, footprint_sectors)`. Only the second of those
is an entry's real extent: the footprints tile `PROT.DAT` exactly, and the
runtime's own resolver uses them, while `indexed_size_sectors` measures a span
of *neighbouring* entries. See
[`prot.md`](../formats/prot.md#tocp5---tocp3--4-is-not-an-entrys-size)
for the proof.

The consequence is that for the entries where the wrong formula is larger, the
extracted file runs past the entry into the following ones, and the same disc
bytes are weighed again under each entry that overlaps them. The archive is
roughly 121 MB; the totals this page reports are roughly 2.5x that.

**This does not distort the percentages nearly as much as it distorts the
totals** - a duplicated run is counted under whichever class claimed the
over-long buffer, and the buffer usually still opens with the entry's real
header, so it usually lands in the right class. The figure to distrust is the
byte *count*; the figure to distrust *slightly* is the share.

Two habits follow. Quote the shares, not the totals, unless you have checked
which view the total came from. And when a single entry's byte weight is what
makes some residue look significant, check its footprint first - a large
"unexplained" entry is quite often a small entry with a long tail of somebody
else's data.

### A statistical class is not a verdict

`asset categorize` ends in a statistical fallback: entries no structural
detector claimed are bucketed by zero fraction and entropy into `mostly_zeros`,
`unknown_low_entropy`, `unknown_high_entropy`, `unknown_other`. Those names
describe **byte statistics**, and the report then reads a *judgement* into one of
them: `mostly_zeros` is counted under "documented placeholder / padding", on the
theory that a `>= 75 %`-zero entry is a reserved-but-unpopulated PROT slot.

That inference has already been wrong at scale. Every scene's
[field map](../formats/field-map.md) - the file carrying its collision grid,
floor heights, object placements and door triggers - is a sparse `0x12000`-byte
blob whose two `128 x 128` grids leave most of the entry at zero on a small map.
Before the `field_map` class existed those entries scattered across
`mostly_zeros`, `unknown_low_entropy` and `unknown_other` by nothing but how
crowded each scene happens to be, and the largest single block of them sat inside
the "explained placeholder" column.

So when reading the per-class table:

- A **statistical** class name is the absence of a finding, never a finding.
  `mostly_zeros` means "sparse and unclaimed", not "empty".
- The **placeholder** column is only as trustworthy as the detectors that ran
  before the fallback. A format with no detector is invisible to it in exactly
  the way an uncited subsystem is invisible to `port-catalog.py`.
- Entries of an **identical, exact size** are the cheapest lead in the table: a
  size that repeats across a hundred entries is a fixed-layout format, whatever
  its byte statistics look like. Grouping the unclaimed entries by size, and by
  their slot position within the CDNAME block, finds those clusters faster than
  reading any one of them.
- A statistical class can also swallow content by **dilution**. The
  printable-ASCII test that recognises an overlay's string table is a ratio over
  the whole buffer, so an overlay *data* image - mostly bss, with its literals
  in the first sector - falls under the threshold and lands in `mostly_zeros`
  instead. Structure at a known offset beats a whole-buffer ratio whenever one
  is available.

## How code coverage is computed

Every Ghidra dump header carries an entry address and a byte length:

```
== FUN_800402f4 800402f4 (entry=800402f4) ==
size=7904 bytes, 1976 instructions
```

so the dumped functions are real intervals over an image's address space. The
script merges them, subtracts them from the image's extent, and is left with the
genuinely un-dumped remainder.

That remainder is then split into **code** and **data**, because a PS-X EXE's
text segment carries its rodata inside the same span, and counting string tables
and jump tables as "un-decompiled code" would understate coverage badly. Each
gap is classified statistically over its whole length - the share of words
decoding to a plausible MIPS I primary opcode, and the density of `0x80xxxxxx`
words that betrays a pointer table. Gaps shorter than eight words are
inter-function alignment and count as code.

The classifier is checked against a control: a region known to be code profiles
at ~94% plausible opcodes and ~0% pointer density, and the large gaps reported
as code match that signature while the head of the segment (87% printable ASCII,
48% plausible) does not.

Dumps that report `0 instructions` and carry only decompiled C are **excluded**.
Such a dump is not evidence that its bytes are understood.

## The overlay caveat, and why rows can read "not meaningful"

`SCUS_942.54` is the only image with an unambiguous answer: one load image, one
fixed base, no aliasing.

Overlays are different. Several are loaded at the same base (`0x801CE818`), so a
dump whose entry lands in that band **cannot be attributed to one image by
address alone** - the same address belongs to the battle overlay, the menu
overlay and the field overlay at different moments. Attributing by address alone
counts a dump for every image whose span contains it.

Rather than publish a number that quietly double-counts, each overlay row
carries the share of its extents that could not be placed. Above 50% the
coverage figure is replaced by **not meaningful**, and such rows are excluded
from the ratchet baseline - a figure that moves with attribution rather than with
real coverage would produce failures nobody can act on.

### Byte-level attribution

The address ambiguity is resolved where the bytes can resolve it.
`scripts/ghidra-analysis/attribute-dump-extents.py` disassembles each extracted
image at its `static-overlays.toml` base and asks which images actually hold a
dump's bytes at the VA it prints. Its verdict per extent is committed as
`scripts/ghidra-analysis/dump-extent-attribution.csv`, which `disc-coverage.py`
reads and applies:

| Verdict | Meaning | What the gate does |
|---|---|---|
| `unique` | one image holds those bytes there | credit only that image |
| `identical` | several hold byte-identical code there | credit each of them |
| `misbased` | the bytes live at another VA entirely | credit nobody |
| `gapped` / `data` | not a coherent function body at that VA | credit nobody |
| `short` / `unresolved` / `no_disassembly` | the window cannot sign it | residue: stays ambiguous |

The key is `(entry, bytes)` - the **extent**, not the dump filename - so the file
does not rot when a dump lands, is renamed, or is re-dumped at the same address.

Two consequences worth stating plainly, because the first one used to be the
whole story and the second one never goes away:

- Most of what was being counted against the two measured overlays belongs to
  images the gate does not measure at all - the field overlay, the minigame
  overlays, the gameover overlay. Those extents now leave the row entirely
  rather than inflating it, which is what makes the outer of the two measured
  spans reportable.
- **The inner of two nested spans cannot be repaired this way.** The menu
  overlay's span lies wholly inside the battle overlay's, so every extent in it
  falls in both by construction; most of what it loses is loss to the outer
  image, and the same residue is a much larger share of what remains. That is
  structural. No amount of dumping moves it, and forcing a figure would mean
  asserting the residue belongs to the inner image when nothing in the bytes
  says so.

The residue itself is a **dump** defect rather than a corpus gap - windows too
short to sign, dumps carrying only decompiled C, gapped streams - so it is
repaired by re-dumping, not by extracting another overlay. See
[`dump-corpus-integrity.md`](dump-corpus-integrity.md) and
[`phantom-print-index.md`](phantom-print-index.md).

Attribution is **optional**. Without the CSV (`--attribution` pointing nowhere)
every overlay extent stays ambiguous by address, which is the pre-attribution
behaviour: an honest upper bound, just a much looser one.

### The two denominators in the code table

The table carries two counts that look like they should agree and do not, so it
says which is which on the page rather than leaving a reader to reconcile them:

- **dumps** is per dump *file*. With attribution present it counts only the dump
  files whose bytes the CSV places in this image.
- **VA-ambiguous** is per **distinct extent**. One extent can back dozens of dump
  files - the mis-based print batches are the extreme case - and weighting the
  ambiguity by how often the same bytes happened to be dumped measures the
  corpus rather than the image.

Per distinct extent is also the key the CSV is written on, so the report and the
artifact can be read directly against each other. Reading the same ambiguity per
dump file instead lands far lower on both rows - low enough that even the inner
nested span reads as reportable - for no reason except that the mis-based
batches are large. That is the number not to quote.

## Running it

```bash
python3 scripts/ci/disc-coverage.py              # report -> target/disc-coverage/
python3 scripts/ci/disc-coverage.py --md         # markdown to stdout as well
python3 scripts/ci/disc-coverage.py --check      # ratchet against the baseline
python3 scripts/ci/disc-coverage.py --update-baseline
```

The data half reads `extracted/PROT/categorize.json`. That file is a **cache**,
not an input the script derives - it is written by `asset categorize
extracted/PROT` and is never regenerated automatically, so a tree whose
`categorize` detectors have moved on keeps reporting the classification they
produced when the file was last written. The gate passes either way, which is
exactly what makes it easy to miss. Regenerate it before trusting a data figure,
and before taking a baseline:

```bash
./target/release/asset categorize extracted/PROT
```

## Gate behaviour

Both inputs - the dump corpus and the `extracted/` tree - are gitignored, so a
clone without disc data has nothing to measure. The script **exits 0 and reports
SKIPPED** in that case, following the same skip-and-pass convention as the
`LEGAIA_DISC_BIN` tests. CI therefore passes without disc data, and the ratchet
only has teeth on a machine that has the disc.

`--check` compares against `scripts/ci/disc-coverage-baseline.json`, which is
committed. Coverage may only go up, within a tolerance of half a percentage
point. If a dump is legitimately removed, re-run with `--update-baseline` and
say why in the commit message - the baseline moving down is a claim that needs a
reason.

A useful side effect: the report lists the largest un-dumped **code** runs in
`SCUS_942.54` by size. That is a dump worklist derived from the bytes rather
than from what anyone happened to cite, which is the one worklist the citation
graph structurally cannot produce.
