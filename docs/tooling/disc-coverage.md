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

### Where an image's span comes from

A coverage figure is a fraction, and the denominator has to be the image's own
length. For `SCUS_942.54` that is the PS-X EXE header's `t_size`. For an overlay
it is `content_bytes` in
[`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml):
the PROT entry's own sector extent, `(toc[p+3] - toc[p+2]) * 2048`
([`prot.md`](../formats/prot.md)), which is exactly the slice the runtime loader
streams into the overlay window. It is disc-reproducible with no dump corpus and
no capture, and it agrees byte-for-byte with the extracted
`overlay_<label>_<entry>.bin`, because the extraction reader computes the same
span.

`content_bytes` is deliberately **not** `clean_copy_bytes`. The two answer
different questions and conflating them produces a wrong number rather than an
imprecise one:

| Field | Answers | Present on |
|---|---|---|
| `content_bytes` | how long the image **is** | every row |
| `clean_copy_bytes` | how much of it a resident RAM capture has **byte-verified** | the two `verified` rows |

On PROT 0899 they differ by `0x_f174` bytes. Measuring the menu overlay against
the shorter figure reported a coverage number for its RAM-verified prefix while
calling it the overlay, and it also hid every un-dumped run above that offset
from the worklist - the failure is silent in both directions, because a shorter
denominator makes the percentage *better*.

The same cut governs [byte attribution](#byte-level-attribution): with
`clean_copy_bytes` as the menu image's own content, the sweep answered "the menu
overlay does not hold these bytes" for every extent above `0x801E46A4`, which is
a large part of that overlay.

An overlay row without a cited own-content length is skipped rather than guessed
at, so a new row is unmeasured until someone states its length.

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
shape is work:

| Shape | What it is |
|---|---|
| `code` | genuinely un-dumped instructions |
| `padding` | every word is `nop`: inter-function alignment |
| `mostly_padding` | at least half the words are zero |
| `no_exit` | 1024 bytes or more with no `jr ra` in them |
| `return_tail` | a `jr ra` (+ `nop`) the preceding routine's analysed body stops short of |
| `bios_thunk_slot` | the delay slot of a `jr $t2` PSX BIOS-call thunk |
| `psyq_lib_stamp` | an 8-byte `Ps` + id + word record: the PSY-Q librarian's version stamp between link modules |
| `constant_table` | every word one repeated non-`nop` constant: a data table resident in the text segment |

The first three non-`code` shapes are properties of **where a function body
ends**, not of what has been analysed, so they persist however much is dumped.
`bios_thunk_slot` is the clearest case: a BIOS-call thunk is `addiu $t2, $zero,
0xA0; jr $t2; addiu $t1, $zero, N`, and because the jump target is a register
the analysed body ends at the `jr` with the delay slot outside it. The thunk is
fully understood and its last instruction still shows up as a gap.

The last two shapes are **data the linker left inside the text segment**, riding
the tiny-gap fiat: a gap under eight words counts as code without a statistical
test, so an 8-byte stamp or a four-word table reads as a 2-4 instruction
"routine" no dump can ever cover. Creating a Ghidra function over one would
assert an entry point the bytes do not support - a `zero_insns` defect
manufactured to move a number - so the honest closure is recognising the bytes.
`SCUS_942.54` carries exactly these instances: three `Ps` stamps in the code
band (ten exist image-wide; the other seven sit in the data segment) and one
`constant_table`, `crt0`'s stack-pointer table at `0x80026CD4`. Both are
documented per-window in
[`runtime-libs.md`](../reference/functions/runtime-libs.md#what-is-left-of-the-scus_94254-code-gap-is-not-code).

Together they are why the figure asymptotes short of 100%, and saying so on the
report is what stops the last fraction of a percent reading as a worklist. The
shapes are **reported, not subtracted**: the denominator stays as it was so the
ratcheted figure remains comparable across changes to this classifier.

#### The two shapes that exist because the opcode statistic is blind to them

`mostly_padding` and `no_exit` are not refinements of taste. Each names a case
the statistical test scores as code with room to spare, and each is a structural
fact about MIPS rather than another threshold to calibrate:

- A word of zeros decodes to `nop` - a plausible primary opcode with no pointer
  density - so a region that is *mostly* zeros passes the code test outright.
  The menu overlay's tail from `0x801E43E8` is 82% zeros, and the classifier
  called all 62512 bytes of it un-dumped code. That run was the largest entry on
  the worklist, and disassembling it returns hundreds of `nop`s followed by
  non-code.
- Every MIPS function body ends in `jr ra`. Measured over `SCUS_942.54`'s text
  head and the menu overlay's code region, known code carries one per ~500-750
  bytes; a data table carries none. The SCUS sound-effect descriptor table at
  `0x8006F198` is 5120 bytes of small-integer records with no `jr ra` and no
  prologue, and it scored as code.

The floor for `no_exit` is set above one function's worth of bytes so a gap
holding the *interior* of one long body is not demoted by it, and a demoted run
is not hidden: it stays in `undumped-runs.csv` under its shape, and only leaves
the ranked worklist.

#### Why the worklist classifies at a finer grain than the denominator

The statistical test answers for whatever span it is given. Over a
function-sized gap that is the right question; over a 62 KB gap spanning a code
tail, a data region and a padding region it answers for the mixture, and the
mixture is decided by whichever component is largest. So the **worklist** splits
each gap into 256-byte windows, classifies each, and merges adjacent windows of
the same class - which turns "one 62 KB un-dumped run" into the function-sized
runs a dumping session can actually consume.

The **denominator** deliberately does not do this. Re-classifying `SCUS_942.54`
in windows moves its code denominator by ~28 KB in the *other* direction - the
windowed test scores several of its rodata tables as code - and which of the two
readings of that image's rodata is right is a separate claim this instrument
cannot settle. The ratchet is written against the whole-gap classification, and
a granularity change to it would be a silent re-baselining of every figure on
the page.

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
carries the share of its extents that could not be placed. Above 50% the *upper
bound* is replaced by **not meaningful**, and such rows are excluded from the
`code` ratchet - a figure that moves with attribution rather than with real
coverage would produce failures nobody can act on.

### Two bounds, because one of them is defined for every row

Withholding the upper bound leaves nothing on the row, and "no defensible upper
bound" and "unmeasured" are different states that a blank cell would conflate.
So each image reports both ends of the interval it is actually known to lie in:

| Column | Credits | Reads |
|---|---|---|
| **covered** | every extent in the span the bytes did not place elsewhere, including residue | upper bound |
| **at least** | only the extents the bytes NAME for this image | floor |

The floor is well defined for every image, including the ones whose upper bound
is withheld, and it is the number the `code_floor` ratchet tracks. A row where
the two ends are far apart is *imprecise*, not unmeasured. A row whose floor is
`0.0%` is saying something sharper than either: nothing in the dump corpus is
attributable to that image at all, so the whole image is un-dumped, and the
dumps printing at its VAs belong to its siblings.

The [worklist](#the-per-overlay-dump-worklist) is cut against the floor for
exactly this reason - cutting it against the upper bound would hide one
overlay's gaps behind a sibling's dumps.

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

- Most of what was being counted against a given overlay belongs to one of its
  VA-alias siblings. Those extents leave the row entirely rather than inflating
  it, which is what makes the outer of two nested spans reportable.
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
  files whose bytes the CSV places in this image, or that it left as residue.
- **VA-ambiguous** is per **distinct extent**. One extent can back dozens of dump
  files - the mis-based print batches are the extreme case - and weighting the
  ambiguity by how often the same bytes happened to be dumped measures the
  corpus rather than the image.

Per distinct extent is also the key the CSV is written on, so the report and the
artifact can be read directly against each other. Reading the same ambiguity per
dump file instead lands far lower on both rows - low enough that even the inner
nested span reads as reportable - for no reason except that the mis-based
batches are large. That is the number not to quote.

## The per-overlay dump worklist

Each run writes two files next to the report in the gitignored
`target/disc-coverage/`. Both carry addresses, byte counts and shape names only -
the same things the committed docs carry - and neither is committed.

| File | What it is |
|---|---|
| `dump-worklist.md` | ranked worklist: `code`-shaped runs of 64 bytes or more, per image, largest first |
| `undumped-runs.csv` | the full inventory: every run of every shape, so a headline "un-dumped" figure can be checked against how much of it is `padding` |

A row is a run of an image's own bytes that no dump the byte attribution places
in that image covers. Columns:

| Column | Meaning |
|---|---|
| `shape` | `code` is work; `padding` / `return_tail` / `bios_thunk_slot` / `psyq_lib_stamp` / `constant_table` / `data` are structural, and persist however much is dumped |
| `ambiguous` | some dump does print at that VA, but the bytes could not place it in this image - a sibling overlay at the same base is the other candidate |
| `spans_at_start` | how many measured images map the run's start VA |

`ambiguous` is a property of each run rather than of the gap it came from: a run
is split wherever the upper-bound crediting changes, so one 40-byte residue
extent inside a 62 KB gap no longer marks the whole gap ambiguous. Start with the
`no` rows - nothing in the corpus covers those at any VA.

**Do not sum the worklist across images.** Nineteen overlays load at
`0x801CE818` and thirteen at `0x801F69D8`, so the same VA appears under several
headings holding *different* bytes each time. Each is real work; the total is not
a total.

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

The baseline has three sections. `code` holds the upper bounds that are
defensible (the rows not marked "not meaningful"); `code_floor` holds every
image's floor, including those rows; `data` holds the format-recognition share.
A **method** change moves the baseline without any dump being lost - widening an
image's span to its whole PROT entry lowers its percentage while measuring more
of it - so a `--update-baseline` for that reason has to say which method changed,
not just that the number moved.

A useful side effect: the run emits a dump worklist for **every** measured image
(see [the per-overlay dump worklist](#the-per-overlay-dump-worklist)), derived
from the bytes rather than from what anyone happened to cite - the one worklist
the citation graph structurally cannot produce.
