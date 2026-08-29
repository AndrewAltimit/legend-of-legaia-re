# Worklist classification

[`port-catalog.py`](port-catalog.md) emits a worklist of addresses that are
`dumped` and `documented` but carry no `// PORT:` tag. It counts one row per
address, and an address is not the same thing as a portable function.

Ghidra promotes intra-function jump labels to fake `FUN_` entries. Overlays hold
relocated copies of the same library routine, so one routine can occupy several
rows at several VAs. Different overlays load different code at the same VA, so
one row can stand for several unrelated routines. Some rows are data regions
that were dumped through a function-shaped script. Taken together these inflate
the worklist by an amount no reader can eyeball.

`scripts/ghidra-analysis/classify-worklist.py` reads the Ghidra dumps under
`ghidra/scripts/funcs/` and assigns every worklist address one class plus a
mechanical reason. Every reason restates evidence found in a dump - the same
standard the [ignore list](port-catalog.md) is held to. The classifier never
guesses: where the heuristics disagree it emits `UNCERTAIN`, because a false
`REAL` costs a future lane a wasted investigation and a false non-portable class
silently deletes real work.

## Running it

```bash
python3 scripts/ghidra-analysis/classify-worklist.py \
    --repo /path/to/legend-of-legaia-re \
    --catalog /path/to/legend-of-legaia-re/target/port-catalog/catalog.csv \
    --out target/worklist-classification.csv \
    --ignore-out scripts/ci/proposed-ignore-additions.toml
```

`--repo` must name a checkout whose `ghidra/scripts/funcs/` is populated. That
directory is gitignored, so a fresh worktree has none and `port-catalog.py` run
inside one reports `dumped: 0` - point both `--repo` and `--catalog` at the
checkout that produced the dumps.

`--repo`, `--catalog` and `--out` are **required in every mode**, including the
two drill-downs below: the script parses its arguments before it decides what to
do, so `--explain` or `--audit-ignored` on its own exits on an argparse error
rather than running. Pass the same three, and send `--out` at a scratch path
when you only want the drill-down.

`--explain <addr>` prints the per-dump evidence for a single address: which dump
files cover the VA, each one's resolved `entry=`, image, instruction count,
whether the body contains `jr ra`, and the trailing jump target if any. That is
the fastest way to audit a verdict without opening the dumps.

`--audit-ignored` re-examines the rows the ignore list has already absorbed
under a `worklist_*` category and re-raises the ones that read as wrong, exiting
non-zero if there are any. Once a row is ignored it leaves the worklist, so
nothing else would ever re-examine it - and an ignore is exactly the verdict that
costs a real port site when it is wrong. Re-run it whenever the dump corpus gains
an image, because the usual way a merged verdict goes stale is a later dump taken
at a base the earlier run did not have.

The audit does **not** simply re-run the classifier over an absorbed row: that
re-derives the row's own evidence and reads agreement as disagreement.
[What an `--audit-ignored` re-raise means](#what-an---audit-ignored-re-raise-means)
gives the test order and the two noise shapes it exists to suppress, and
[the three kinds of ignore claim](#the-three-kinds-of-ignore-claim) says which rows
it can speak about at all.

### The three kinds of ignore claim

The `worklist_*` prefix on an ignore-list section is not cosmetic, and the audit's
scope is exactly that prefix. Three different assertions live in
[`port-catalog-ignore.toml`](port-catalog.md), and only one of them is a claim
the images can settle:

- an **address** claim - *no routine begins at this VA*: `worklist_interior`,
  `worklist_phantom`, `worklist_data`, `worklist_shared_tail`,
  `worklist_duplicate`, `worklist_misbased_print`, `worklist_uncertain`;
- a **scope** claim - *the routine is real, and the clean-room port covers it
  another way*: every unprefixed section, from `libgte` and `bios` through
  `prim_builder`, `mesh_submit`, `noop_frame` and `noop_stubs`;
- a **reachability** claim - *the routine is real, the port does not cover it,
  and retail never reaches it*: the `unreferenced` section, described below.

The [entry-boundary test](#the-entry-boundary-test) refutes an address claim by
finding a routine where the row says there is none. Against a scope claim it has
no purchase at all: such a row already describes the body, instruction by
instruction, so "an image starts a routine at this VA" is what the row says
rather than a contradiction of it. Filing a scope row under a `worklist_*` name
therefore guarantees a re-raise on every run, which is how the audit's exit code
stops meaning anything. A scope claim is checked by re-reading the body against
the engine's own implementation - a human task the reason exists to make cheap -
so scope sections stay unprefixed however they were arrived at.

The decisive check is neither the dump nor the reason string: disassemble the
mapped image at the VA and look for the `jr ra` / `addiu sp,sp,-N` pair around
it. A **leaf** has no prologue, so a missing prologue is not a missing function -
`FUN_801CF5D0` is a frameless eight-field record copy and is still the menu
overlay's first routine.

### The reachability claim

A row in `unreferenced` says the opposite of an address claim: a routine *does*
begin here, and nothing on the disc ever gets to it. That takes a different
instrument again - the entry-boundary test only looks at the VA itself, and a
call-graph sweep only looks for `jal`. The evidence is a whole-disc sweep for
every reference form at once, over `SCUS_942.54`, the based overlay images and
the raw bytes of every extracted `PROT.DAT` entry
([`address-reference-scan.md`](address-reference-scan.md)).

Its relation to the other two kinds is worth stating, because a row can only be
one of them:

| Kind | Says | Refuted by |
|---|---|---|
| address | no routine begins here | disassembling a routine at the VA |
| scope | the port covers this another way | re-reading the body against the engine's implementation |
| reachability | nothing on the disc reaches this entry | finding any reference form for the address |

The reachability row is the one to prefer over a port. A port of a routine the
runtime cannot enter is inert by construction, so it lands in the audit's
disclosed-inert section and stays there forever - a wiring worklist entry no
wiring pass can close. Settling it as a documented negative keeps the worklist
honest in both directions.

### A word hit separates a template-seated entry from an interior label

The reachability sweep and the entry-boundary test answer different questions,
and one scan shape invites reading the first as an answer to the second. An
address with **no** `jal`, **no** `j`, **no** `lui`+`addiu` and only branches
looks like an intra-function label, because a label is reached exactly that
way. But a routine seated on a
[static actor template](../reference/functions/runtime-libs.md#static-actor-templates)
scans almost identically: nothing calls it either - the actor pool copies its
address into `actor[+0xC]` and the frame walk `jalr`s that - so the only
evidence it leaves is the one word in the template.

That single aligned word is the whole difference, and it is decisive in both
directions: a label carries no copy of its own address anywhere, so a word hit
at the VA rules `INTERIOR` out. `801D2298` (field ledge-hop advance) and
`801D4098` (dance clip-driver gate) are the worked cases - each has exactly
one word hit, in its own image, immediately after the template's `ffff0000`
model-selector lead, and each is `REAL`. The scan's own triage calls both hits
`incidental-code`, because the templates sit inside code
([`address-reference-scan.md`](address-reference-scan.md#a-template-inside-a-code-region-reads-as-incidental-code)),
so the class name is not the verdict - the neighbouring words are.

Both directions of this cost something. Filing a template-seated entry as
`INTERIOR` deletes a real port site; filing a label as `REAL` because the
branches look like traffic mints a phantom row. Read the words at the hit.

## The classifier's output

The generated CSV holds addresses, class names and one-line reasons. It carries
no dump text, so it is safe to commit; the dumps themselves are Sony-derived and
are not. A checked-in copy of the current run lives beside the script at
`scripts/ghidra-analysis/worklist-classification.csv`, so the per-address
verdicts are readable without a populated `funcs/` directory. Volatile counts
live in that artifact rather than on this page.

### An empty output is not a result

Both committed artifacts - the CSV and
`scripts/ci/proposed-ignore-additions.toml` - are *generated*, and nothing
regenerates them on a schedule or checks them in a gate. So each has two
states that look alike in a diff and in a directory listing:

| What you see | Could mean | Or could mean |
|---|---|---|
| CSV with a header row and nothing else | the open worklist is empty and every row is classified | the instrument has not been run since the worklist last changed |
| proposal TOML with only its comment header | every proposal has been reviewed and merged | the same |

The two are told apart by re-running, not by reading. Before citing either
file as evidence of a clean worklist, regenerate it against a checkout with a
populated `funcs/` and see whether the file changes. The CSV shipped as a bare
header row for a while, which reads as "classified and clean" and was in fact
"unrun" - the exact `0 findings` / `findings not shown` ambiguity the gates on
this page's siblings exist to remove.

## The classes

| Class | What it means | How it is detected |
|---|---|---|
| `REAL` | A distinct, portable function entry. | A dump whose `entry=` equals the queried VA, whose disassembly begins at that VA, whose body contains `jr ra`, and which owns the frame that `jr ra` returns from. |
| `INTERIOR` | The VA sits inside another function. | The dump resolves `entry=` to a different address, is an explicit citation stub, another dumped body in the same image disassembles an instruction at this exact VA, or the body carries the [tail-fragment signature](#jr-ra-does-not-prove-a-function). |
| `PHANTOM` | No body of its own. | The only dump body is a degenerate stub, or the decompiled body is a Ghidra `caseD_` switch-case fragment. |
| `SHARED_TAIL` | A distinct body that is not independently callable. | The body has no `jr ra` and ends on an unconditional jump into code it does not own. |
| `DATA` | Not code. | The dump is a data-region, hex-blob or pointer-table listing. |
| `DUPLICATE` | The same routine as another address. | The relocation-masked instruction stream equals another entry's, or is a strict prefix of one. |
| `VA_ALIASED` | Not one port site. | Two or more extracted images hold distinct code at this VA. Dump image tags do not establish this - see [below](#an-image-tag-is-a-program-name-not-an-overlay-identity). |
| `REAL_BUT_VENDOR` | Real, but not game logic. | A BIOS vector thunk shape, or a decompiled body naming PsyQ library infrastructure. |
| `UNCERTAIN` | Needs a human. | Evidence is thin, missing, or the heuristics disagree. |

### `jr ra` does not prove a function

The obvious test for "is this a callable entry" - does the body return? - is
wrong on its own, and it fails in the direction that costs the most. Ghidra
promotes intra-function jump labels to `FUN_` entries, and a label near the
*end* of a routine yields a **tail fragment**: a body that runs to the parent's
epilogue and therefore contains that epilogue's `jr ra`. It returns, but it
returns from a frame it never built, using registers it never set.

Register and stack liveness separate the two cases. A fragment reads
callee-saved registers without ever writing them - Ghidra renders those reads
`unaff_s0..s8` / `unaff_fp` / `unaff_retaddr` - and reads the parent's frame
through slots it never wrote, rendered `in_stack_<offset>`. The classifier
requires all three conditions together before calling a body a fragment:

1. no `addiu sp,sp,-N` within the first few instructions (the body builds no
   frame of its own - one or two independent loads may be scheduled ahead of
   the allocation, but not a prologue's worth);
2. at least one `unaff_` read of a callee-saved register;
3. at least one `in_stack_` read.

Each condition alone is a false positive. `unaff_gp` is normal throughout this
codebase because of gp-relative addressing, so it is excluded from the callee-
saved set entirely. An `in_stack_` read alone is an ordinary stack-passed
argument. And a large real function can pick up a stray `unaff_` from an
incomplete decompile - the 2777-instruction body at `0x801F5748` reads eight of
them, yet opens `addiu sp,sp,-0x48` and saves `s0..s7`, so the frame test keeps
it `REAL`. That is why the frame test is the one read out of the disassembly
rather than the decompiled C.

The check runs before the `SHARED_TAIL` test, because a frameless body that
reads its parent's registers is interior to that parent whether it exits on
`jr ra` or on a jump.

### Instruction-stream normalisation

Duplicate and alias comparison run over a normalised instruction stream: the
address column is dropped and every absolute address-shaped immediate is masked.
Two streams that hash equal therefore differ at most in branch targets and
address halves - that is, they are the same routine linked at a different base.
This is what catches a library routine that appears once per overlay under a
different VA each time.

A looser mask over every hex immediate is available in the script but is not
used for classification: it also collapses genuine constant and struct-offset
differences, so it produces candidates rather than evidence.

### Dumps that do not testify

Three kinds of dump are evidence about the dump rather than about the code, and
the classifier refuses to let any of them decide a verdict. The first two are
discarded before comparison: left in, each splits a VA that every other image
agrees on, and a false split reads as `VA_ALIASED` - the class that says "this
needs per-image identity work" about a routine that needs none. The third is
checked last, and it guards the opposite error.

- **Data decoded as code.** A dump's printed addresses are a property of the
  load base it was taken at ([`dump-corpus-integrity.md`](dump-corpus-integrity.md)).
  Aim a dump at a VA whose image holds no code there and Ghidra still emits a
  disassembly - of whatever bytes are present. The signature is a body dominated
  by `$zero`-absolute loads and stores: real MIPS reaches statics through `gp`
  or a `lui`/`addiu` pair, so a long run of `lb rN,0xNNNN(zero)` is a table of
  `0x80`-high bytes being read as opcodes. Ghidra's own bad-instruction warning
  over a body too short to be a function is the second signature.
- **Gapped instruction streams.** MIPS instructions are four bytes and a dumped
  body is contiguous, so a jump in the address column means Ghidra left holes.
  Every instruction after a hole is offset against a complete dump of the same
  routine, so the two streams differ from the first hole onward whatever the
  code says. A contiguous dump outranks a gapped one.

- **Bodies with no disassembly at all.** A dump that reports `size=1 bytes, 0
  instructions` under a full C rendering is the artifact
  [`ghidra.md`](ghidra.md#decompiler-artifacts-that-have-produced-false-claims)
  catalogues: Ghidra never established the function's bounds, so the C is a
  guess about where the routine ends and nothing in the dump says the VA is an
  entry rather than a label. Such a dump cannot yield `REAL`. This one matters
  most, because every test that would otherwise catch a non-function - own
  return, exit jump, fragment shape - reads the instruction stream, and an
  empty stream passes all three silently. A false `REAL` is the classifier's
  most expensive error: `REAL` is the class the worklist acts on.

Containment is read per image and is unaffected by any of the three, so it is
evaluated over every dump; what has to be whole is the dump on the other side
of a disagreement. See [below](#containment-is-a-per-image-fact).

### Instruction streams outrank decompiled C

Some dump generations carry decompiled C with no `--- DISASSEMBLY ---` section.
Ghidra's C rendering drifts with analysis state - variable naming and the
inferred signature can differ between two dumps of identical machine code - so
the classifier treats the instruction stream as the authority. When at least one
dump at a VA has disassembly, only the disassembly dumps decide aliasing. C
bodies decide only when no dump at that VA carries instructions.

### An image tag is a program name, not an overlay identity

This is the single largest source of false `VA_ALIASED` verdicts, and it is not
a dump defect - it is a category error in reading a correct dump.

A dump's `[overlay_foo.bin]` tag names the **Ghidra program** it was taken
from. Most programs in this corpus are live-RAM captures named after a game
scenario: `overlay_cutscene_dialogue`, `overlay_shop_save`, `overlay_muscle_dome`,
`overlay_magic_level_up`, `overlay_save_ui_select`. None of those is a PROT
entry. A RAM capture spans the whole overlay region - slot A at `0x801CE818`,
slot B at `0x801F69D8`, and the always-resident executable - so the tag records
which capture the bytes came from and says nothing about which overlay owns the
queried VA.

Two consequences, and they point opposite ways:

- **One overlay, many tags.** `menu`, `save_ui_select`, `save_ui_saving` and
  `shop_save` are four captures of PROT 0899; `magic_level_up`, `magic_capture`
  and `muscle_dome` are captures whose slot A holds PROT 0898. Counting tags
  reports an alias where there is one routine.
- **One tag, many overlays.** A capture tagged for a 32 KB minigame overlay
  still covers `0x801F7088`, far past that overlay's footprint, where the
  bytes belong to whichever slot-B image was resident.

Neither error is visible in the dump. Both are settled by the bytes. Resolving
a capture's whole dump set against the extracted images says what it really
held: `overlay_cutscene_dialogue` is a **field-overlay** (0897) capture, and
`overlay_muscle_dome` is a **battle-overlay** (0898) capture with the summon
render overlay (0900) in slot B. Neither names an overlay that could be
extracted, because neither is an overlay - so a row those tags disagree about
is not waiting on the extraction pipeline.

### Static-image arbitration

Where `extracted/` is populated, the classifier resolves each dump against the
statically extracted images before any image-name test runs. A dump testifies
about a VA only if its opening instructions match some image **at that VA**;
one that matches at a different VA is mis-based and its printed address is
fiction ([`dump-corpus-integrity.md`](dump-corpus-integrity.md)).

It runs only on rows the metadata tests call `VA_ALIASED` or `DUPLICATE` -
the two verdicts that turn on which image a dump came from - and yields:

| Outcome | Verdict |
|---|---|
| Exactly one image holds the dumped bytes at this VA | `REAL`, naming the owning overlay |
| Two or more images hold distinct code there | `VA_ALIASED`, naming the overlays rather than the capture programs |
| No dump testifies | `UNCERTAIN`, with the reason narrowed by the follow-up below |
| A `DUPLICATE` peer has no matching body at its own VA | `REAL`, duplicate claim withdrawn |
| Every dump is bounded short, and one image starts a routine at the VA | `REAL`, naming that overlay |

The last row is the one case where the arbiter overrides an `UNCERTAIN` the
metadata tests reached rather than an alias. A dump Ghidra bounded short - no
`jr ra`, no exit jump, the stream simply stops - is a statement about the dump:
every test that would place the VA reads that stream, so none of them sees the
address at all. The [entry-boundary test](#the-entry-boundary-test) needs no dump,
so one image beginning a routine there settles what the truncation could not, and
two or more leave the row `UNCERTAIN`, which is never ignored. Both shapes occur:
`FUN_801D56E4`, the fishing overlay's 2-D segment clipper, is dumped 131
instructions deep and cut off mid-body, and `0x801D0290`'s only dump is the field
VM's `addiu fp,fp,2` label-call idiom printed at the wrong base, five
instructions long - while the battle overlay begins its 12-instruction PRNG leaf
at that VA.

#### Asking the images directly, with no dump involved

The arbiter above reasons *from dumps* to images. When no dump testifies - or
when the dumps disagree - the question can also be put to the images alone:
[`scripts/ghidra-analysis/locate-entry-image.py`](../../scripts/ghidra-analysis/locate-entry-image.py)
walks every based overlay in
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) and
reports two independent signals per image: a stack-frame prologue
(`addiu sp, sp, -N`) within the first few instructions, and the number of
`jal <va>` sites elsewhere in that same image.

A frame in exactly one image names the body to port from. Neither signal is
sufficient alone, which is why the tool prints both instead of a verdict:

- A **leaf** function has no frame. `0x801F6D48` is a real entry in PROT 0900
  that opens `lui t6, 0x1f80` and never touches `sp`.
- A function reached through a jump table or from `SCUS_942.54` has no
  in-overlay `jal`. `FUN_801EC3E4` has zero, and is real.
- Call-site counts can agree across aliases: `0x801E1D98` has three `jal` sites
  in **both** PROT 0897 and 0898. Only the frame separates them - 0898 holds the
  332-instruction body, while 0897's bytes there are the field-VM label-call
  idiom (`addiu fp, fp, 6`).

The frame scan must stop at a `jr ra`, or an epilogue reports its *successor's*
prologue as its own - the false positive `0x801D2D2C` produces.

#### "No dump testifies" is two different problems

The bare form of that verdict - *un-extracted overlay, or mis-based dumps* -
names two causes with opposite remedies, and reads as the first. Only one of
them is closed by extracting an overlay; the other is closed by re-dumping, and
no amount of extraction will touch it. So the arbiter asks one more question
before it settles: **if the bytes are not at this VA, are they anywhere?**

It re-checks the dump's window against every image at every offset, the same
way [`check-dump-base-integrity.py`](dump-corpus-integrity.md) does, and reports
what it finds:

| Follow-up | Reason recorded |
|---|---|
| Bytes resolve elsewhere, single site | mis-based; names the real overlay, VA and delta |
| Bytes resolve elsewhere, several sites | mis-based; names each, since one routine is linked into several overlays |
| Body too short to sign, but its Ghidra program has a measured batch delta | re-checked at that delta only, and reported if it lands |
| The dump's address column has holes | gapped stream - it can match no image as a contiguous window, so the failure says nothing about the corpus |
| Nothing resolves | the original bare reason, which now really does mean "un-extracted or unverifiable" |

The batch-delta step is worth separating from the image-tag inference this
section otherwise forbids. It uses the program name only to *propose* an
offset, measured from that program's other dumps; the verdict still comes from
comparing bytes at that offset. A program with no single dominant delta
proposes nothing.

A row that ends up `mis-based` is not port work at the address it names. The
address came from a dump printed at the wrong base, so there is no function
there to port - the routine is real, but it lives at the VA the reason names,
where it is usually already a separate row.

For the `0x801C…` / `0x801D…` printed band the resolution has been done in bulk
and committed: [`phantom-print-index.md`](phantom-print-index.md) lists the
per-program re-key deltas and, per printed address, the image and VA its bytes
occupy plus whether that VA is an entry. Check a row against that page before
re-deriving it.

The `REAL` reason is deliberately narrow. "Every dump at this VA is of PROT
0899" is not "no other overlay has code at this VA" - several do, since eight
overlays share the slot-A base. It says only that nothing has dumped them, so
they are not worklist rows. A row can therefore be a single port *site* in the
worklist's sense while the VA is aliased in the machine.

Comparison is over the canonicalised token stream shared with
`check-dump-base-integrity.py`, and each word is decoded independently: a
streaming disassembly stops at the first word capstone rejects, which would
silently truncate the compare and match a prefix.

Each image is first cut down to the bytes its entry actually **owns**. An
extracted image is the entry's `read_entry` footprint and runs into its
neighbours' sectors ([`static-overlay-pipeline.md`](static-overlay-pipeline.md)),
so left whole it answers for VAs its overlay never loads - with a neighbour's
code. The cut is where another image's head appears, which is that neighbour's
sector-aligned start. Without it, one routine appears at several VAs across
several images and the arbiter reports overlays that hold nothing there.

Two guards keep the arbiter from asserting more than it knows. A window
dominated by `$zero`-absolute loads is the data-decoded-as-code signature and
is refused outright - otherwise a dump matches an image's *data* and two images
agreeing on nothing but a table read as a genuine alias. And without
`extracted/`, arbitration is skipped entirely (`--no-static-arbitration` forces
this): the classifier falls back to dump metadata and prints a warning, because
every verdict it then reaches carries the image-tag caveat above.

### Containment is a per-image fact

The containment test - another dumped body in the same image disassembles an
instruction at this exact VA - is what catches the label-call idiom, and it is
sound only within one image. Overlays load different code at the same virtual
address, so an address can be a jump label inside a dispatcher in one overlay
and a function entry in another. That is the definition of `VA_ALIASED`, and
reading it as `INTERIOR` deletes the second overlay's routine from the worklist
with a reason that is true about the first.

The classifier therefore requires containment to hold in *every* image that
dumps a whole self-entry body here. Where one image contains the VA and another
carries a body with its own `addiu sp,sp,-N` prologue and `jr ra`, the row is
aliased. `--audit-ignored` exists because this rule was added after the ignore
list had already absorbed rows decided the other way.

## Interpreting the result

Three classes are non-portable in the plain sense: `INTERIOR`, `PHANTOM` and
`DATA` name no function to port. `DUPLICATE` and `REAL_BUT_VENDOR` name real
code that is already covered elsewhere or belongs to the host-API shim layer.
All five are proposed for the ignore list.

The other two need care, and both point the opposite way from "the worklist is
smaller than it looks":

- **`VA_ALIASED` is more work, not less.** One row stands for several distinct
  bodies that happen to share a virtual address across overlays. Removing the
  row would delete real work; splitting it needs the per-image identity that
  [`static-overlay-pipeline.md`](static-overlay-pipeline.md) exists to supply.
  It is also the class the dump corpus contaminates hardest in both directions -
  a bad-base or gapped dump inflates it, and a per-image containment hit used to
  hide inside `INTERIOR`. Treat an alias verdict as a claim to check, not a
  conclusion, and never ignore the row.

  The contamination is the common case rather than the exception, which is why
  [static-image arbitration](#static-image-arbitration) exists: read against
  the extracted images, the overwhelming majority of alias verdicts resolve to
  one overlay's routine plus a mis-based or non-testifying dump. Almost every
  such row's "other image" turns out to be a capture program at the documented
  `+0xE818` or `+0x5818` base offset. Rows that survive arbitration name two
  real overlays and are genuinely two port sites.
- **`SHARED_TAIL` is real code that is not an independent port site.** These are
  the per-case branches of a multi-entry assembly routine - the family that ends
  by jumping back into a common loop or epilogue. They port as arms of the
  enclosing routine, so they are ignore-list candidates as *rows*, but the code
  behind them still has to be written.

`REAL` is therefore the lower bound on single-site port work and `REAL` plus
`UNCERTAIN` the upper bound, with `VA_ALIASED` and `SHARED_TAIL` sitting outside
that range as work whose row count does not match its site count.

## False-positive risks

The classifier is tuned so that its errors land in `UNCERTAIN` rather than in a
confident class, but four risks remain and a reviewer should spot-check them.

**Small bodies split under alias comparison.** A body only a few instructions
past the stub threshold carries little signal. Two short jump pads at the same
VA in different overlays can normalise differently and be reported
`VA_ALIASED` when they are the same trivial stub. Check the instruction counts
in the reason string before trusting an alias verdict on a short body.

**`SHARED_TAIL` versus a genuine tail call.** A compiler may emit `j <helper>`
as a tail call from a perfectly ordinary function, which looks identical to a
fragment jumping into a shared epilogue. The classifier names the jump target
and, when a dumped body owns it, the owning function; a target that lands in the
*middle* of another routine is the label-call idiom, while a target equal to
another function's entry is a tail call and the row is really `REAL`.

**Vendor detection is narrow on purpose.** It fires only on the BIOS vector
thunk shape or an explicit PsyQ library name in the decompiled text. Vendor code
that mentions neither is classified `REAL`. That is the safe direction: an
over-eager vendor rule would silently drop game logic off the worklist.

**Image-name normalisation gates containment.** `INTERIOR`-by-containment only
fires when two dumps agree on which image they came from, and images are named
several ways across dump generations - bracketed, with a `base=` suffix, or
inferred from the filename prefix. The script normalises these, but a spelling
it does not recognise splits one image into two and the containment test then
misses. A missed containment leaves the row `REAL` rather than dropping it, so
the direction is safe, but it means the `INTERIOR` count is a floor and some of
what reads `REAL` is really aliased.

## Proposed ignore entries

`--ignore-out` writes a file shaped like
[`port-catalog-ignore.toml`](port-catalog.md), one section per class, each row
carrying its class and mechanical reason. It is a proposal for review, not a
drop-in: merge the sections into the real ignore list after spot-checking, and
note that `VA_ALIASED` and `UNCERTAIN` rows are deliberately excluded from it.

What "after spot-checking" means per class, since the classes do not carry equal
risk:

| Class | Merge policy |
|---|---|
| `PHANTOM`, `DATA`, `INTERIOR` | Merge once the disassembly agrees. |
| `SHARED_TAIL` | Merge only for a frameless body whose exit jump lands *inside* another routine - the label-call idiom. A `j` to another function's entry is a tail call and the row is `REAL`. |
| `DUPLICATE` | Merge only when a peer is ported, or is a live worklist row that names the routine stably. A peer that is itself aliased does not qualify - ignoring both ends deletes the routine. |
| `VA_ALIASED`, `UNCERTAIN` | Never merged. |

**Never merge the proposal file unread**, and `DUPLICATE` is why. A wave that
hand-checked six proposed rows found three of them wrong, all the same way:
normalisation masks absolute address-shaped immediates, which erases a `jal`
target, so *every small thunk hashes identically to every other small thunk*
and distinct routines are proposed for deletion as copies of one another. The
cost is asymmetric - an ignore row leaves the worklist permanently and nothing
re-examines it - so a wrong row here is more expensive than a wrong row
anywhere else in this layer. The generated file now carries that warning in its
own header, where it cannot be missed by someone who never opened this page.

The `DUPLICATE` rule is not hypothetical in the other direction either: a
cross-image relocated match can name a peer whose own row stands for two
routines, in which case the match identifies neither. The sharper form of the same failure is a peer address that is not a
body at all - the matching stream came from a mis-based dump *printed* at that
VA, while the VA itself hosts an unrelated routine in some other overlay. Both
are checked mechanically now: a peer qualifies only when a dump at the peer VA
carries the same body and resolves against an image there.

Reviewed rows land in `port-catalog-ignore.toml` under the same
`worklist_*` section names, which keeps a merged row traceable back to the
classifier that proposed it. A row whose mechanical reason under- or over-states
what the dump shows is rewritten on merge rather than accepted verbatim: the
`0x801F0000` and `0x801F4000` rows, for instance, read as data-region listings
to the classifier, but every doc citing them names a region boundary, and that
is what the merged reason says.

The proposal file therefore always means *outstanding* proposals: once its rows
are merged, the next run writes it back empty. Rows that stay in it are rows a
reviewer declined under the table above.

The committed CSV beside the script is a snapshot of the *open* worklist, so a
row the ignore list has absorbed leaves it. That is what `--audit-ignored` is
for: the absorbed verdicts stay checkable from the ignore list's own reason
strings plus a re-run, rather than from a CSV that would otherwise have to be
kept as a growing archive. Re-running the classifier after a merge reports only
the residue - `REAL`, `VA_ALIASED`, `UNCERTAIN`, and any `DUPLICATE` the merge
policy held back.

## What an `--audit-ignored` re-raise means

The audit and the classifier ask different questions, and conflating them makes
the audit useless. The classifier reads dump metadata. A merged ignore row was
written from those same dumps, so re-running the classifier over an absorbed row
mostly re-derives the row's own evidence - and any verdict outside the
non-portable set then reads as a disagreement when it is an agreement in
different words. Two shapes produce almost all of that noise:

- **A `worklist_misbased_print` row re-classified `REAL`.** The body *is* real,
  just not at the printed address. The entry test reads the dump's instruction
  stream, which a mis-based print reproduces faithfully; nothing in that stream
  records the base error, so the test cannot see the claim the row is making.
- **`UNCERTAIN`.** It sits outside the non-portable set, so it re-raises even
  when its reason - "decodes data as code", "every dump at this VA is
  mis-based", "no disassembly", "gapped stream" - is the finding the row was
  merged on.

So `--audit-ignored` does not re-raise on the class. It re-raises only on
evidence the merged reason cannot already contain, in this order:

| Test | Outcome |
|---|---|
| Classifier returns a non-portable class | Row stands - classifier and row agree outright. |
| A covering image starts a routine at this VA at its mapped base | **Re-raise.** The row deletes a real port site whatever the dumps say. |
| The merged reason names a true VA the dumped bytes do not resolve to | **Re-raise.** The verdict may survive, but the reason misdirects the reader. |
| A covering image exists and starts no routine here | Row stands. |
| The VA lies between the executable and the overlay slots | Row stands - no image maps it, so no routine can begin there. |
| Classifier returns `UNCERTAIN` | Row stands. No verdict is not evidence the row is wrong. |
| Nothing above applies | Re-raise, flagged as unverifiable rather than refuted. |

### The entry-boundary test

The decisive test is the second one, and it involves no dump at all. Where
[static-image arbitration](#static-image-arbitration) asks whether a *dump*
belongs at a VA, this asks whether the *image* begins a routine there - which is
the only question a mis-based dump cannot corrupt, and the only one that can
refute a row whose merged reason was read off that dump.

Three signatures, read from each image at its mapped base, any one sufficient:

- the word two back decodes `jr ra` - the predecessor's return, with its delay
  slot between;
- the word at the VA decodes `addiu sp,sp,-N`, a non-leaf prologue, which occurs
  nowhere else in this codebase, **and** no routine opened earlier in the window;
- the words before the VA carry the `$zero`-absolute data signature and the
  words at it do not - a code region opening after a header or string blob,
  which is how an overlay's first routine looks.

All three are needed. A **leaf** has no prologue, so requiring one calls every
frameless routine a fragment; a leaf sited immediately after the overlay's data
header is preceded by neither a return nor code.

#### A prologue is not a boundary

The second signature reads a prologue and needs one more condition, because a
routine may open with instructions its body needs *before* it allocates a frame.
`FUN_80021934` is the case that pins it: the predecessor's `jr ra; addiu sp,sp,0x30`
closes at `0x80021930`, then three words load a scratchpad byte and a `gp` word,
and only at `0x80021940` comes `addiu sp,sp,-0x120`. Reading that prologue as the
entry sends a caller three instructions in, onto a subtract over the registers
those loads were meant to fill - which is why the ignore list carries
`0x80021940` as interior and names `0x80021934` as the entry to port.

So the prologue branch is refused when a `jr ra` appears earlier in the window
with no body-closing transfer between it and the VA: the boundary is the word
after that return's delay slot, and anything past it is inside the routine the
return opened. A transfer out in between means another whole body closed, and the
prologue can be a real entry after all. `jal` does not count - a call returns, so
it does not end the routine it sits in.

Both ends are guarded. The word at the VA must decode and must not be padding or
a transfer out, which rejects the shape that otherwise reads as a boundary: a
body's *second* `jr ra` exit, whose predecessor pair is the first exit and its
delay slot. A window carrying the data signature at the VA itself is refused for
the same reason it is refused elsewhere - the image is answering with a table.

### Limits

The audit can only refute a row where an extracted image covers the VA. Under
`--no-static-arbitration`, or without `extracted/`, none of the image tests run
and the audit degrades to the true-VA cross-check plus the `UNCERTAIN` rule -
it reports several times as many rows, most of them unverifiable rather than
wrong. A row is also unverifiable when the VA falls inside the overlay slots but
past every extracted image's footprint; those re-raise, flagged as such.

The reverse limit matters more. "No covering image starts a routine here" rests
on the extracted corpus being complete for that slot. It is not - an overlay
nobody has extracted can hold an entry at a VA every extracted sibling uses as
interior code. That is the direction in which the audit stays silent about a
wrong row, and it is closed by extracting overlays, not by tuning the test.

The same corpus growth is what makes the audit worth re-running rather than
trusted once. Extracting an overlay can *refute* a merged row as readily as it
can support one: the row that carried `0x801D84B4` as inter-function padding was
read off the minigame images, where the VA really is a `nop` run, and the field
overlay begins a six-instruction leaf there that stores master game mode
`0x16` (CARD INIT) and raises the entry-context word - the overlay-local twin of
the SCUS scripted game-over trigger. The row was deleting that routine with a
reason true of four other images, and the audit is what surfaced it.

## The sibling worklist: cited but not dumped

`port-catalog.py --missing-dumps` is the mirror image of the worklist this page
classifies: an address some dump *cites* but that has no dump of its own. Same
inflation, one step earlier, and worth reading with the same suspicion - a
citation is not a call.

What counts as a citation is a regex over the dump text, and only four token
shapes match: `FUN_<addr>`, `func_0x<addr>`, `jal 0x<addr>` and
`jalr r,0x<addr>`. A bare `j 0x<addr>` in a disassembly does **not** match. So
a jump label contributes nothing from the instruction stream and everything
from the decompiled C, where Ghidra renders the same label as `func_0x<addr>()`.
The rows this produces are therefore the [label-call
artifact](../subsystems/script-vm.md#intra-function-label-catalogue) seen through
the citation counter, and each resolves the same way: find the enclosing body,
confirm the in-body branches that reach the VA, and record it as `INTERIOR`.

Worked examples, each read out of the enclosing body's disassembly rather than
out of its C:

| Cited VA | Cited from | What it is |
|---|---|---|
| `0x801E28C4` | field/event VM `FUN_801DE840` | Its [wait join](../subsystems/script-vm.md#the-wait-join-at-0x801e28c4) - four instructions, one `j` predecessor at `0x801E1B04`, both arms landing in the routine's own epilogue. |
| `0x801EFEA0` | tile-board walk SM `FUN_801EF2B0` | Its [render tail](../subsystems/tile-board.md#the-render-tail-0x801efea0), reached from nineteen in-body sites and running to that routine's epilogue at `0x801F03E8`. |
| `0x801ED2D4` | world-map picker `FUN_801ECD0C` | A shared epilogue: one store to `ctx[+0x54]`, then the register restores and `addiu sp,sp,0x40` unwinding a frame it never built. Ghidra minted an entry here and cut the parent's body at the jump, so `getFunctionContaining` answers with the fragment. |
| `0x801DAB7C` | initiative seeder `FUN_801DA780` | Its exit tail, reached from `0x801DAB00` / `0x801DAB34`. The citing dump prints the same routine at `0x801FF930` - a mis-based window - so the citation names a VA in the **battle-action** image, not in the image the dump was printed under. |

The last row is the one to internalise: the `j` operand is absolute in the
encoding, so a mis-based dump's jump targets stay correct while its address
column does not (see
[`call-target-integrity.md`](call-target-integrity.md)). Resolving such a
citation against the printing image yields unrelated code that reads like a
confident answer.

### The two worklists are coupled through `dumped`

A row leaves `--missing-dumps` when a file named for the address appears under
`ghidra/scripts/funcs/`. But `--missing-ports` selects on `dumped` **and**
`documented`, so dumping a non-entry that some page already mentions does not
remove the row - it **moves** it from one worklist to the other. Closing a
citation artifact honestly therefore takes two steps, not one: the dump that
resolves the citation, and an [ignore entry](#proposed-ignore-entries) carrying
the class. Skip the second and the closure reads as a regression.

### Fabricated bodies

One shape produces citations out of nothing at all, and it is worth naming
because no amount of care reading the *cited* address can catch it - the defect
is in the citing dump.

A body-repair pass that walks instructions from an address without first asking
whether the bytes are code will happily walk a data table, and Ghidra will
decode every word of it into something. The output is a dump of ordinary
appearance, and every word that decodes as a `jal` becomes a citation of an
address nothing ever called. Applied to a known data window - a module's link
base, a jump table, a string blob - a single pass can mint a worklist row per
fabricated call. PROT 0900's head window is the worked case: already recorded as
data in [`dump-corpus-integrity.md`](dump-corpus-integrity.md), it was later
re-walked into a 1692-byte "body" whose lone decoded call target,
`0x80040000`, entered the dump worklist as if it were undumped code. Nothing
calls it: at that VA `SCUS_942.54` holds an ordinary `j` in the middle of
`FUN_8003FB10`, and the roundness of the address is the first hint that no
linker put a function there.

Three tells, any one of which settles it, all read off the printed disassembly:

- **Printed addresses that are not 4-byte aligned.** MIPS instructions are
  fixed-width; an address column stepping by 2, 6 or 12 is not a decode of MIPS
  code at all.
- **Opcodes the R3000A does not implement.** `movf`, `sdc2`, `tge`, `jalx`,
  `daddi` - the same test [`dump-corpus-integrity.md`](dump-corpus-integrity.md)
  applies to short windows, and the strongest available because it needs no
  context.
- **Branch and jump targets outside every mapped image**, especially into the
  KSEG1 hardware window at `0x8C000000`+.

The repair is to replace the fabricated body with a hexdump under a header that
says the window is data. That removes the citations - hex bytes carry no
`jal 0x` prefix - while keeping the address resolvable for anyone who follows
the old citation. `ghidra/scripts/dump_wave_closeout.py` does this as its
`"data"` kind, alongside the `"label"` kind that writes the citation-pointer
file for an interior VA.
