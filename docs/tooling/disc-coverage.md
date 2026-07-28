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

The distinction is not academic, and it runs in both directions. A citation
graph with no cited-but-not-dumped addresses left says nothing about the bytes
nobody cited; and closing a byte-denominated gap *widens* the citation graph,
because a newly dumped function that gets documented becomes a row on the port
worklist. A rising port worklist after a dump pass is the measurement getting
wider, not the work going backwards.

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

The header is parsed by
[`dump_header.py`](../../scripts/ghidra-analysis/dump_header.py), shared with the
attribution sweep. That sharing is not tidiness - it is the fix for a defect that
had made this page's own numbers wrong. See
[an instrument's private header regex](#an-instruments-private-header-regex-is-a-claim-about-the-corpus).

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

### What the `SCUS_942.54` gap turned out to be

Worth stating as a result rather than as method, because it is the clearest
demonstration of what this measurement is *for*.

Working the gap list until it stopped yielding produced ~95 function entries. A
five-form reference sweep
([address-reference-scan.md](address-reference-scan.md)) over every one of them,
across `SCUS_942.54`, the based overlay images and every PROT entry, splits them
three ways:

| | Share | Why it had no dump |
|---|---|---|
| **no reference of any form, anywhere** | about three quarters | Ghidra creates functions from the call graph. A routine nothing references gets no function record, so it gets no dump, so nothing cites it, so it is invisible to any citation-denominated instrument - including a dump worklist derived from one. |
| referenced from `SCUS_942.54` | about a fifth | mostly reached through the entry-stub / init path, or from a body whose own analysis was incomplete. |
| referenced only from an overlay | three | the routine is live, and the call comes from an image the SCUS-only analysis never sees - the standing "zero static callers is not dead" trap, showing up here as a coverage gap. |

The first row is the point. **The bytes find the class the citation graph
structurally cannot**, and they find it as a *majority* of what was left. A
worklist built from what the project has cited can never list a routine nobody
has cited; a worklist built from an image's own bytes lists it whether anyone
has heard of it or not.

Two cautions on quoting the first row. "No reference exists" is much stronger
than "no SCUS caller" - the sweep covers the overlays and the PROT archive - but
it is still a statement about *static* references, so a computed target
assembled some other way, or a caller in an overlay that has never been
extracted, would not appear. And an unreferenced routine is not automatically
dead code in the interesting sense: much of this band is linked-in library
material the game does not call.

### A code gap is not automatically un-analysed code

Subtracting the dumps leaves a remainder, and the obvious reading - "these are
the routines nobody has looked at" - is right for most of the bytes and wrong for
the tail. The report therefore classifies each code gap by *shape*, and only one
of the four shapes is work:

| Shape | What it is |
|---|---|
| `code` | genuinely un-dumped instructions |
| `padding` | every word is `nop`: inter-function alignment |
| `return_tail` | a `jr ra` (+ `nop`) the preceding routine's analysed body stops short of |
| `bios_thunk_slot` | the delay slot of a `jr $t2` PSX BIOS-call thunk |

The last three are properties of **where a function body ends**, not of what has
been analysed, so they persist however much is dumped. `bios_thunk_slot` is the
clearest case: a BIOS-call thunk is `addiu $t2, $zero, 0xA0; jr $t2; addiu $t1,
$zero, N`, and because the jump target is a register the analysed body ends at
the `jr` with the delay slot outside it. The thunk is fully understood and its
last instruction still shows up as a gap.

Together they are why the figure asymptotes short of 100%, and saying so on the
report is what stops the last fraction of a percent reading as a worklist. The
shapes are **reported, not subtracted**: the denominator stays as it was so the
ratcheted figure remains comparable across changes to this classifier.

### An instrument's private header regex is a claim about the corpus

Reading the header looks like the trivial part of this measurement. It was the
part that was wrong, and the shape of the error generalises to any instrument
over a corpus it did not write.

The corpus spells every header field more than one way - it was written by a
dozen dump scripts over a long period. The printed VA appears bare and
`0x`-prefixed. The entry appears as `(entry=…)`, `(entry=0x…)`, `(entry=…,
label=…)`, `(entry …)` after a `--` header, and not at all. The size line appears
with an instruction count, without one, with a trailing parenthetical, and as a
`min=`/`max=` pair instead. Each instrument grew its own regex for one subset of
that, so each silently rejected a different set of **real dumps** as
unparseable - and reported the rejects as a corpus deficiency.

Two things went wrong at once, and the second is the instructive one:

- Real dumps were dropped. Accepting only the bare-VA spelling lost 54 function
  dumps; `(entry=…, label=…)` lost 20 more; a size line with no instruction count
  lost 6. None of those files was defective in any way.
- The count was **explained wrongly**, and the explanation was plausible enough
  to survive review. The report described its rejects as "typically the ones that
  report `0 instructions` and hold only decompiled C". Not one of them reported
  `0 instructions` - the files that do report it were passing the regex and being
  *credited*, one byte each - and three of several hundred were C-only.

So the honest form is a census, not a count. The report now names each reject
class and, more importantly, separates two populations that a single number
merges:

| Kind | Classes | Why it is excluded |
|---|---|---|
| **answer** | `pointer_stub`, `nofunc_record`, `data_window`, `not_a_dump` | the corpus recording a result, not a dump. Not defective, and not work. |
| **defect** | `zero_insns`, `gapped_stream`, `no_extent`, `empty_dump` | a dump that cannot evidence its own extent. |

Four fifths of the excluded files are answers. A pointer stub is the corpus doing
the *right* thing with a mid-function address - the alternative is a file whose
name asserts an entry point that does not exist - so counting it as a missing
dump penalises exactly the handling that avoids the defect it is being counted
as.

`zero_insns` is the one class that moved the other way. A dump reading `size=1
bytes, 0 instructions` is Ghidra's "bad instruction data": it decoded nothing, so
the window is data being asked for as code. Those were being credited as covered
bytes and are now excluded, which lowers coverage very slightly and is correct.

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
- The inner of two nested spans **starts** at total ambiguity. The menu overlay's
  span lies wholly inside the battle overlay's, so every extent in it falls in
  both by construction and no address arithmetic will ever separate them. That
  much is structural, and it is the reason this row is the harder one.

That second point was previously written on this page as a conclusion - that the
inner span could never be measured at all, and that no amount of dumping moved
it. It is worth recording that as **falsified**, because the reasoning was
seductive and the error is a general one: *the starting point of a measurement was
mistaken for its limit*. Address ambiguity is total for the inner span; byte
attribution then places most of those extents in one image or the other, and the
row reports. What is structural is that the row can never be resolved *by
address*, which is a statement about one method rather than about the image.

### What the residue is decides what closes it

The residue is not one thing, and it is mostly **not** repaired by re-dumping -
another claim this page previously carried and that the numbers do not support.
Three shapes remain, and each needs a different move:

| Shape | What would close it |
|---|---|
| a few-instruction window that no image's own content reproduces at that VA | nothing cheap. Too short to search for elsewhere without inviting a coincidental hit, so it stays residue rather than being called `misbased` on evidence that cannot support the call. |
| bytes in no extracted image at any VA | an **extraction**, not a dump. Most were dumped from live RAM captures of overlays never statically extracted, or from runtime-mutated memory. |
| two dumps at one extent resolving to different images | already answered: several routines share the range. A real finding, not a gap. |

The middle row is the one with a route forward, and it is the
[static overlay pipeline](static-overlay-pipeline.md)'s job rather than this
page's.

### The signature floor guards one question, not both

The sweep asks two questions of a dump's opening window and they have opposite
sensitivities to its length:

- **at a VA**: does *this* image's own content reproduce this window at *this*
  address? A fixed-offset test between a handful of candidates.
- **anywhere**: do these bytes appear at any offset in any image? A search over
  millions of positions, which is how a mis-based print is identified.

One shared floor of eight instructions was applied to both, and for the first
question that is far too strict - a short window at a fixed VA has no
multiple-comparison problem to guard against. Relaxing the at-VA test to three
instructions while leaving the search at eight resolves most of the `short`
residue, and the relaxation is set from its own control rather than from
judgement: `--validate-short-floor` truncates every extent the full window
already resolves and re-runs the at-VA test at each short length. Over ~3000
trials it produces **no wrong answer at any length down to one instruction**, and
loses precision only in the honest direction - naming several images instead of
one, which returns `identical` and credits all of them. Three instructions is
where that curve flattens.

The generalisable point: **a confidence floor belongs to a question, not to an
instrument.** Sharing one across two questions makes it simultaneously too loose
for the sensitive one and too strict for the robust one, and only the second
failure is invisible, because it shows up as missing data rather than as a wrong
answer.

See [`dump-corpus-integrity.md`](dump-corpus-integrity.md) and
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
