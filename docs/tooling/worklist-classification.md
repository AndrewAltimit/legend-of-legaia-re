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

`--explain <addr>` prints the per-dump evidence for a single address: which dump
files cover the VA, each one's resolved `entry=`, image, instruction count,
whether the body contains `jr ra`, and the trailing jump target if any. That is
the fastest way to audit a verdict without opening the dumps.

The generated CSV holds addresses, class names and one-line reasons. It carries
no dump text, so it is safe to commit; the dumps themselves are Sony-derived and
are not. A checked-in copy of the current run lives beside the script at
`scripts/ghidra-analysis/worklist-classification.csv`, so the per-address
verdicts are readable without a populated `funcs/` directory. Volatile counts
live in that artifact rather than on this page.

## The classes

| Class | What it means | How it is detected |
|---|---|---|
| `REAL` | A distinct, portable function entry. | A dump whose `entry=` equals the queried VA, whose disassembly begins at that VA, and whose body contains `jr ra`. |
| `INTERIOR` | The VA sits inside another function. | The dump resolves `entry=` to a different address, is an explicit citation stub, or another dumped body in the same image disassembles an instruction at this exact VA. |
| `PHANTOM` | No body of its own. | The only dump body is a degenerate stub, or the decompiled body is a Ghidra `caseD_` switch-case fragment. |
| `SHARED_TAIL` | A distinct body that is not independently callable. | The body has no `jr ra` and ends on an unconditional jump into code it does not own. |
| `DATA` | Not code. | The dump is a data-region, hex-blob or pointer-table listing. |
| `DUPLICATE` | The same routine as another address. | The relocation-masked instruction stream equals another entry's, or is a strict prefix of one. |
| `VA_ALIASED` | Not one port site. | Two or more images dump distinct bodies at this VA. |
| `REAL_BUT_VENDOR` | Real, but not game logic. | A BIOS vector thunk shape, or a decompiled body naming PsyQ library infrastructure. |
| `UNCERTAIN` | Needs a human. | Evidence is thin, missing, or the heuristics disagree. |

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

### Instruction streams outrank decompiled C

Some dump generations carry decompiled C with no `--- DISASSEMBLY ---` section.
Ghidra's C rendering drifts with analysis state - variable naming and the
inferred signature can differ between two dumps of identical machine code - so
the classifier treats the instruction stream as the authority. When at least one
dump at a VA has disassembly, only the disassembly dumps decide aliasing. C
bodies decide only when no dump at that VA carries instructions.

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
misses. This direction is safe (a missed `INTERIOR` stays `REAL`), but it means
the `INTERIOR` count is a floor.

## Proposed ignore entries

`--ignore-out` writes a file shaped like
[`port-catalog-ignore.toml`](port-catalog.md), one section per class, each row
carrying its class and mechanical reason. It is a proposal for review, not a
drop-in: merge the sections into the real ignore list after spot-checking, and
note that `VA_ALIASED` and `UNCERTAIN` rows are deliberately excluded from it.
