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
about one tool's blind spot.

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
  assembled in more than two steps, or reached as `table_base + index`, is not
  a pair and will not be found - which is why a per-record negative over a
  table needs the table *base* scanned too before it means anything.
- **The verdict is triage.** A `dispatch-table` classification says the
  neighbours look like function entries, not that the runtime indexes them.
- **Aliased branches.** A `BR` hit in an image that does not hold the routine
  is not a reference to it; pass `--home` and read `branch_alias`
  ([above](#a-branch-cannot-cross-images)).

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
