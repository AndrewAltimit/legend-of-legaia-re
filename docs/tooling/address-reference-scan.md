# Non-`jal` address-reference scan

A routine with "no caller" is often a routine nobody scanned for correctly.
Call-graph tools look for `jal`, and this engine reaches a great deal of its
code some other way:

- through a **function-pointer table** - the menu overlay's sub-screen
  dispatch array, a window descriptor's content-renderer slot, a
  [static actor template](../reference/functions/runtime-libs.md#static-actor-templates)'s
  tick word;
- through a **LUI+ADDIU pair** that materialises the address into a register.
  Ghidra's reference manager does not auto-resolve those pairs, so a direct
  xref query returns zero hits even for a heavily used address
  ([`ghidra.md`](ghidra.md)).

And the mirror question - "why does this address have no caller at all?" - has
a second answer that no absolute scan can see: the address may not be an entry
point. An intra-function label is reached by a **PC-relative branch**, which
carries no copy of its target; that is the shape behind Ghidra's fake
`FUN_xxxxxxxx` "label-calls".

[`scripts/ghidra-analysis/find-address-word-refs.py`](../../scripts/ghidra-analysis/find-address-word-refs.py)
sweeps the disc's code images for **all five** forms at once - literal word,
`lui`+`addiu`/`ori` materialisation, `jal`, `j`, and PC-relative branch - so a
"nothing references this" result is a statement about the bytes rather than
about one tool's blind spot. The materialisation arm follows the `lui` register
the way [the shared walk](#how-both-scans-follow-a-register) does.

```bash
# One address, every form, across SCUS + the based overlay images.
scripts/ghidra-analysis/find-address-word-refs.py 8005126c

# Widen to every extracted PROT entry (raw bytes; no VA is claimed for them).
scripts/ghidra-analysis/find-address-word-refs.py 8005126c --prot

# Name the image that holds the routine, so branch hits from its slot
# siblings are marked instead of counted (see "A branch cannot cross images").
scripts/ghidra-analysis/find-address-word-refs.py 801e58a8 --home field

# "Who references this table?" - expand a VA range to its word-aligned members.
scripts/ghidra-analysis/find-address-word-refs.py --range 800705fc:80070760

# Batch a worklist, and print the neighbouring words at each hit.
scripts/ghidra-analysis/find-address-word-refs.py --file addrs.txt --context
```

## What a hit's VA means

Reporting a hit as a VA requires knowing where the containing image loads, and
conflating the three cases is how this repo has previously claimed a VA the
bytes could not support ([`call-target-integrity.md`](call-target-integrity.md),
[`phantom-print-index.md`](phantom-print-index.md)). The tool keeps them apart:

| Image | Base | What a hit is reported as |
|---|---|---|
| `SCUS_942.54` | PS-X EXE header `t_addr`, applied to file offset `0x800` | file offset + the one VA it can be |
| Overlay images | the committed map [`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) | file offset + VA, **named by image** |
| Other PROT entries (`--prot`) | none - streamed data | file offset only |

The middle row is the one that bites. Many overlays share a load base, so the
same VA is a different word in each slot-A sibling: the *image* is as much of
the answer as the offset is. The bottom row is why `--prot` hits carry no VA -
a streamed entry has no load base, and inventing one would be the phantom-VA
defect all over again.

## A branch cannot cross images

Of the five forms, four carry a copy of the address they reach and are
therefore evidence wherever they are found: a `jal` in one overlay can call a
routine in another, or in SCUS, whenever both are resident. A **branch is not
like that**. It is PC-relative, so it can only reach code in its own image -
which makes a `BR` hit evidence about *that image's* copy of the VA and about
nothing else.

Under a shared slot-A base every overlay has a byte at every VA, so this is not
a corner case. Two of the addresses on this page's own worklist read as
referenced for exactly that reason and are not: `0x801CFE20`, the FMV overlay's
MDEC-in sync wrapper, collects a branch from the **field** overlay, whose bytes
at that VA are the epilogue of an unrelated routine; `0x801E58A8`, a field
overlay list-count seed, collects one from **battle_action**. Neither branch
can reach the routine it appears to reference.

`--home <image>` makes the check mechanical. Name the image that holds the
routine - [`locate-entry-image.py`](../../scripts/ghidra-analysis/locate-entry-image.py)
answers that from the prologue - and branch hits from any other overlay print as
`br~` and tally under `branch_alias` instead of `branch`. When a target's only
hits are aliased branches the tool says so outright. The flag exits non-zero on
a name that matches no loaded overlay, so a typo cannot quietly mark every hit
an alias. SCUS is never marked: its base is unique, so a hit there is never an
aliasing artifact.

## Classification

Only a word-aligned hit can be a table entry, so unaligned ones are reported as
incidental without further analysis. For the aligned ones the verdict is drawn
from three counts over the surrounding bytes, all printed alongside it so the
verdict can be second-guessed:

| Count | Measures |
|---|---|
| `code` | `jr ra` and `addiu sp, sp, +-N` words within `+-0x200`. Real code carries several; a pointer table carries none. |
| `entry` | neighbouring words (`+-4`) that are addresses landing on a function prologue **in this same image**. |
| `ptr` / `const` | neighbouring words that are RAM-band addresses, or small constants. |

`dispatch-table` needs two or more `entry` neighbours; `template-field` is a
lone pointer among constants; `incidental-code` is a word that happens to fall
inside an instruction stream. The verdict is a triage summary, not a
substitute for reading the disassembly at the hit
([`ghidra.md`](ghidra.md#decompiler-artifacts-that-have-produced-false-claims)).

### A template inside a code region reads as `incidental-code`

`code` outranks the other two counts, and a
[static actor template](../reference/functions/runtime-libs.md#static-actor-templates)
is a small constant record **linked among the routines it seats**, not in a
separate data region. So its `+0x8` tick word collects frame markers from its
neighbours, scores `code > 0` with `entry = 0`, and prints `incidental-code`
even though `ptr = 1` among constants is exactly the `template-field` shape.
Read that verdict as "aligned word, neighbourhood is code" and go look, rather
than as "not a reference".

The tell is one word back. `FUN_80020DE0` consumes a template as
`+0x0/+0x2/+0x4` u16 fields, `+0x8` tick function, `+0xC` flags, `+0x14`
sub-state, and `+0x4` is the model selector - `0xFFFF` for the transform-node
helpers, the same `ffff0000` lead the prescript stager records open with. A
word that decodes as a routine's VA immediately after an `ffff0000` is a
template's tick slot, not a stray immediate:

| Address | Image | Template | Words at the hit |
|---|---|---|---|
| `801D2298` | 0897 `field` | `0x801F2294` | `… 00000000 · ffff0000 · **801d2298** · 00000000 …` |
| `801D4098` | 0980 `dance` | `0x801D42FC` | `… 00150000 · ffff0000 · **801d4098** · 00008082 …` |

Both are `REAL` function entries reached through their template, and both are
ported and documented - the ledge-hop per-frame advance
([`field-locomotion.md`](../subsystems/field-locomotion.md)) and the dancer
clip-driver gate ([`minigame-dance.md`](../subsystems/minigame-dance.md)). The
scan's shape for each - one aligned word hit, no `jal`, no `j`, no `lui`, and
branches only from images that cannot reach them - is the shape of a
template-seated routine, and it is worth telling apart from an interior label,
which collects **no** word hit at all
([`worklist-classification.md`](worklist-classification.md#a-word-hit-separates-a-template-seated-entry-from-an-interior-label)).

## Controls

The scan is only worth its negatives if its positives are checked, so run it
against a known answer before trusting a "nothing found":

- **Known table entry.** `8004da00` reports exactly one hit, the template word
  at SCUS `0x800767FC`, classified `template-field` - the documented reference
  ([`battle.md`](../reference/functions/battle.md)). `--expect-scus 800767fc`
  turns that into a pass/fail self-check with an exit status.
- **Known call target.** `800195a8` (the billboard projector) reports its `jal`
  sites across SCUS and five overlays and no word hits at all.
- **Known branch target.** `80051308`, an intra-function `bne` target, reports
  one `BR` site and nothing else.
- **Known dispatch table.** `--range` over the menu overlay's sub-screen
  pointer array at `0x801E4F40` recovers its single materialisation site
  ([`save-screen.md`](../subsystems/save-screen.md)).

## Limits

- **Compressed entries.** `--prot` scans raw bytes, so a reference living
  inside an LZS-compressed entry is invisible to it. Overlay *code* is stored
  raw (every row of the static-overlay map is `form = "raw"`), so this gap does
  not cover the images where callers actually live.
- **Split materialisation.** The pair scan wants one `lui` and one
  `addiu`/`ori` with the exact low half, within a few instructions. An address
  assembled in more than two steps is not a pair and will not be found. The
  register walk in the sibling sweep [below](#the-gp-relative-and-luiload-forms)
  covers the multi-step case. `table_base + index` is covered to the extent
  the bytes allow: the walk carries the `lui` register through the indexing
  `addu` and reports the array's **base** (see
  [the indexed form](#the-indexed-form-lui-addu-lw-lo)), never the element a
  runtime index selects - so a per-record negative over a table still needs
  the table *base* scanned before it means anything.
- **`$gp`, `lui`+load, and a materialised base plus a displacement.** Three
  forms this tool does not decode at all - a `disp(gp)` access, a `lui`/load
  pair whose low half rides the load instead of an `addiu`, and an access
  whose low half is *split* between the base register and the operand's own
  displacement. All three are in the sibling sweep
  [below](#the-gp-relative-and-luiload-forms); run it before reading a
  five-form negative as "nothing references this".
- **The verdict is triage.** A `dispatch-table` classification says the
  neighbours look like function entries, not that the runtime indexes them.
- **A loader parameter is not an address, and a computed one is not a
  literal.** This tool answers "who references this address". A PROT entry is
  not reached by its address but by its *index*, and an index the code forms
  with arithmetic appears nowhere as a constant - so "no image names entry N"
  is a statement about the literal, not about the loader. PROT 1221 / 1222 read
  as unreachable for exactly this reason: raw TOC `0x4C7` / `0x4C8` occur as no
  literal on the disc, because PROT 0978 forms the index as `addiu a0,s0,0x4c7`
  with a runtime `s0` (four sites, file `+0x264` / `+0x2F4` / `+0x398` /
  `+0x440`). Sweep the *base* of the arithmetic, not the value it produces.
- **Aliased branches.** A `BR` hit in an image that does not hold the routine
  is not a reference to it; pass `--home` and read `branch_alias`
  ([above](#a-branch-cannot-cross-images)).

## The gp-relative and `lui`+load forms

[`scripts/ghidra-analysis/find-gp-relative-refs.py`](../../scripts/ghidra-analysis/find-gp-relative-refs.py)
covers the two forms the five-form sweep is structurally blind to.

- **`disp(gp)`.** The point of the small-data pointer is that the address never
  appears in the instruction stream. `sw a0,0x678(gp)` carries a 16-bit
  displacement and nothing else, so no scan for the absolute address can see it
  - in either direction, writer or reader.
- **`lui rX, hi` + `lw rY, lo(rX)`.** The commonest way retail touches a named
  global puts the low half on the *load*. The five-form pair scan accepts only
  `addiu` (op `0x09`) and `ori` (op `0x0D`) as the second half, so it walks past
  every one of these.
- **`lui rX, hi` + `ori`/`addiu rX` + `<mem> rY, disp(rX)`.** The same thing
  with the low half *split* between the register and the operand, so no
  instruction carries the address or even its low half. This is how retail
  reaches every scratchpad byte and every field of a struct held in a
  register: `lui a0,0x1f80; ori a0,a0,0x314; sb v0,0xd4(a0)` writes
  `0x1F8003E8`, and `lui a0,0x8008; addiu a0,a0,0x4140; lw v1,0x59c(a0)`
  reads `0x800846DC`. The sweep decodes it by walking the register forward
  from each `lui` until something else writes it, which also recovers the
  multi-step materialisation the five-form scan lists as a limit - an
  `addiu`/`ori` result that *equals* the target is reported too. `--no-base-disp`
  turns the walk off.

The two compose into a silent negative. `gp+0x678` - the battle sound bank's
record table, [`bse-dat.md`](../formats/bse-dat.md) - has one gp-relative writer
and seven `lui`+`lw` readers, and a five-form sweep of its absolute address
`0x8007B990` reports "no word, no jump, no branch, no materialisation pair - in
any image".

```bash
# Recover $gp from the runtime's own `lui gp` / `addiu gp` pair.
scripts/ghidra-analysis/find-gp-relative-refs.py --find-gp

# Every access to one small-data slot, by displacement or by absolute VA.
scripts/ghidra-analysis/find-gp-relative-refs.py 0x678
scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x8007b990

# An address `$gp` does not reach: scratchpad, or a struct field off a
# materialised base. `--va` takes it as an absolute-form query.
scripts/ghidra-analysis/find-gp-relative-refs.py --va 0x1f8003e8

# Widen to every extracted PROT entry, and grep the dump corpus too.
scripts/ghidra-analysis/find-gp-relative-refs.py 0x5b8 --prot --dumps
```

`$gp` is written once by the runtime and never reloaded, so one constant fixes
every displacement in the image; `--find-gp` decodes that pair rather than
trusting a remembered value (retail Legaia: `0x8007B318`, from `0x80026CA8`).
A displacement scan needs no `$gp` at all, which is what makes it meaningful
over base-less PROT entries - the encoding carries no address, so it is
base-independent by construction. The cost of that is coincidence: a raw byte
scan over scene data will produce hits with `code=0` around them. Read the
disassembly at a hit before calling it a reference.

## How both scans follow a register

Every form above except `disp(gp)` starts at a `lui` and follows the register
it loads until something completes the address. What "follow" means is the
whole of the scans' precision, so it lives once, in
[`scripts/ghidra-analysis/mips_walk.py`](../../scripts/ghidra-analysis/mips_walk.py),
and the byte account's [`lui_forms`](../../crates/asset/src/byte_account.rs)
is the same walk in Rust:

| Step | Treatment |
|---|---|
| any write to the register | drops it - a stale high half is never paired with a later low one |
| `addu rB, rA, $zero` / `or rB, rA, $zero` | a copy: `rB` carries what `rA` held |
| `addu rB, rA, rX`, runtime `rX` | an index: `rB` is the high half plus `rX`, and a later `lo(rB)` completes the array's base |
| `addiu` / `ori` on a carried register | the register now holds more of the address (the multi-step and base-plus-displacement forms) |
| conditional branch | forks: fall-through and target both carry the state out of the delay slot |
| `j` / `b` | continues at the target only - the next word is another arm's |
| `jal` / `jalr` | the delay slot still sees every register (retail completes pairs there); past it only callee-saved registers survive |
| `jr ra` | ends the path |
| a `lui` in a delay slot | the walk starts where that jump goes |

Every path is bounded by the scan's window and each word is walked once per
`lui`. A hit is still a site to read, not a proof: the walk does not know which
path a register really arrived by at a join, and the `code` count plus the
disassembly settle it the same way they do for the five-form scan.

### The walk used to be lax

Before the shared walk, both scans stopped only when the register was re-loaded
by *another `lui`*. Any other write - `lw v0,x(v0)`, `ori a3,a3,0x314`, a
call's return value - left the old high half paired with whatever low half came
next. Counted over SCUS plus every mapped overlay, the lax walk formed 23,200
`lui`+memory pairs inside the pair window; the strict walk keeps 17,476 of them,
drops 6,015 and adds 291 it had missed (branch targets and copies). A sample of
eight dropped pairs read in the disassembly was eight false pairs, each a
register already rewritten: `lui v1,0x8008; lw v1,-0x42dc(v1); lbu v1,0(v1)`
had been reported as a reference to `0x80080000`.

None of this moved a recorded negative. Re-running both scans over every
address the ignore list's reachability rows and `docs/` cite as unreferenced
(89 addresses) changes no verdict: every address with hits before has hits
after, and no zero became non-zero. What moved is the hit lists behind the
positives - the scratchpad byte `0x1F800393` gains 143 sites and loses none.

### The indexed form (`lui`, `addu`, `lw lo`)

`lui at,0x801d; addu at,at,a3; lw v1,0xd9c(at)` (PROT 0970 at `0x801D055C`)
reads element `a3` of the array at `0x801D0D9C` without any instruction holding
`0x801D0D9C`. The pair scan cannot see it (the second instruction is an `addu`),
and a register walk that drops the register at its first redefinition stops
one instruction short. The shared walk carries the index and the gp script
labels the hit `[indexed by a3: array base]`; the five-form tool prints the
`addiu` variant as `addiu+index`.

The form is rarer than a lax count suggests. Over SCUS and the mapped overlays
it completes 512 accesses: 273 in SCUS, 223 in PROT 0896 (the Japanese-build
image), 9 in 0971, and single digits elsewhere. The field (0897) and menu
(0899) overlays have **none** - their compiler forms the base with `lui`+`addiu`
first and indexes the formed register, which the pair scans always saw. A lax
walk reports dozens of "indexed" sites in both, every one a register rewritten
between the `lui` and the `addu`.

Both silent-negative shapes on this page have now produced a corrected doc.
`gp+0x678` is the small-data one. The base-plus-displacement one is the camera
zone loader's scratchpad side-write at `0x1F8003E8..EB`
([`encounter.md`](../formats/encounter.md#the-scratchpad-window-0x1f8003e8eb)),
whose four readers were recorded as "consumer Unknown" for exactly as long as
the only sweep available could not see a `0xd4(a0)`.

## A third shape: the reader that carries no address at all

Some globals have no reader either sweep can find, because the consumer never
names them. The GTE's light matrix is the worked case: a render routine stages
control registers 8..12 with `ctc2`, and the *reader* is the coprocessor
itself - the `NCS` / `NCT` / `NCDS` / `NCDT` / `NCCS` / `NCCT` commands, and
`MVMVA` when its `mx` field selects matrix 1, all multiply an input normal by
that matrix with no operand naming it. Both sweeps on this page report zero
readers, correctly and uselessly.

The instrument for that shape is a census of **opcodes** rather than of
references: [`find-gte-light-consumers.py`](../../scripts/ghidra-analysis/find-gte-light-consumers.py)
sweeps SCUS plus every based overlay image for GTE command words, tallies them
by function selector, and lists every light-matrix consumer with its image and
VA. Its caveat is the mirror of the five-form scan's: a GTE command word is
four bytes with no relocation, so it occurs in data at the rate any four-byte
pattern does, and the discriminator is structural - the GTE takes no memory
operands, so a real command sits among the `lwc2` / `mtc2` / `mfc2` / `swc2`
moves that feed and drain it. Data hits have no distinct COP2 neighbours; real
ones have several.

The generalisable question to ask before either sweep: **does the consumer of
this word name it?** A `gp` displacement, a `lui`+load pair and a base plus
displacement all do, in different encodings. A coprocessor control register
does not, and neither does anything else the hardware reads implicitly.

## The retail-unreachable set

Sweeping every anchor the port catalog's audit lists as *disclosed inert* -
ported, unreachable in the engine, and disclosed as such - answers a question
the audit itself cannot: whether a row is waiting on wiring or on nothing.
Almost all of them are waiting on wiring. These are the ones that are not, and
the list is closed under the sweep - no other disclosed-inert anchor lacks a
reference.

| Address | Image | Routine | Write-up |
|---|---|---|---|
| `8005126C` | SCUS | battle actor on-screen test | [`battle.md`](../reference/functions/battle.md#unreferenced-scus-entry-points) |
| `80035274` | SCUS | item / equipment passive-name draw | [`menus.md`](../reference/functions/menus.md#80035274) |
| `80050D40` | SCUS | 12-bit angle tween | [`battle.md`](../reference/functions/battle.md#unreferenced-scus-entry-points) |
| `80025054` | SCUS | actor-template tick; unreachable through its record `0x80070614` | [`game-modes.md`](../reference/functions/game-modes.md) |
| `801CFE98` | 0970 `cutscene_str` | MDEC-**in** DMA-callback registrar (libpress residue); its MDEC-out twin `0x801CFEBC` *is* called by the same overlay | [`cutscene.md`](../subsystems/cutscene.md) |
| `801CFE20` / `801CFE5C` | 0970 `cutscene_str` | MDEC in / out sync wrappers | [`minigames-debug.md`](../reference/functions/minigames-debug.md) |
| `801D0230` | 0970 `cutscene_str` | MDEC status-word leaf; both call sites are inside the two wrappers above | [`minigames-debug.md`](../reference/functions/minigames-debug.md) |
| `801D5780` | 0897 `field` | generic arc-hop spawn | [`runtime-libs.md`](../reference/functions/runtime-libs.md) |
| `801D5C2C` | 0972 `fishing` | 3-D segment clip + projection | [`minigame-fishing.md`](../subsystems/minigame-fishing.md) |
| `801DAD6C` | 0899 `menu` | five-step menu open sequence | [`menus.md`](../reference/functions/menus.md) |
| `801DBA90` | 0898 `battle_action` | reward-banner composer | [`battle.md`](../reference/functions/battle.md) |
| `801E5834` / `801E58A8` | 0897 `field` | pooled menu-actor spawn + list row-count seed | [`menus.md`](../reference/functions/menus.md) |

Every row was checked with `locate-entry-image.py` first, so each is a routine
that begins where it is said to begin. The last two groups needed `--home`: the
only hit for `801CFE20` is a branch in the field overlay and the only hit for
`801E58A8` is one in `battle_action`, neither of which can reach the routine it
appears to name. `801D0230`'s negative is one step removed - it has real call
sites, both inside routines from this same list.

The scan settles reachability, not identity, so a row here still says nothing
about whether the decoded behaviour is right. What it says is that the row is
not wiring work: see
[`worklist-classification.md`](worklist-classification.md#the-reachability-claim)
for how such a row is recorded, and
[`port-catalog.md`](port-catalog.md#ignore-list) for where.

## Relation to the in-container scripts

Three Ghidra-side scripts cover pieces of this from inside a loaded program -
`find_lui_writers.py` (materialisation pairs in a range),
`find_addr_data.py` / `find_data_word.py` (literal address words), and
`find_terrain_emitter_caller.py` (a combined sweep against a target set), all
catalogued in [`ghidra.md`](ghidra.md#script-catalogue). They stay the right
tool while you are already in a program and want the containing *function* of
a hit, which they can name and this cannot.

What the host-side scan adds is corpus and closure: it runs against every image
at once without importing anything, it includes the raw PROT entries no Ghidra
project holds, and it covers all five forms in one pass - which is what turns
"I did not find a caller" into "no reference exists".

## See also

- [`port-catalog.md`](port-catalog.md) - the worklist this settles rows on.
- [`ghidra.md`](ghidra.md) - the LUI+ADDIU xref gap and the decompiler
  artifacts the scan's output has to be read against.
- [`static-overlay-pipeline.md`](static-overlay-pipeline.md) - where the based
  overlay images and their committed bases come from.
