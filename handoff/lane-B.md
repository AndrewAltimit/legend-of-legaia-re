# Lane B handoff - repairing the defective overlay dumps

Scope worked: `ghidra/scripts/**`, `scripts/ghidra-analysis/check-dump-base-integrity.py`,
`docs/tooling/dump-corpus-integrity.md`, `docs/tooling/phantom-print-index.md`,
`docs/reference/functions/runtime-libs.md`. `attribute-dump-extents.py`,
`dump-extent-attribution.csv` and `classify-worklist.py` were read, never edited.
No dump was staged.

## The headline number

**126 of 314 previously-cited addresses are not function entry points.** Each was
audited by asking Ghidra directly, per address, in a program imported at the
image's own base, after the project database had settled. Every one reports
`INTERIOR of <function> at <entry>` with the enclosing entry **below** the
requested address.

Per program, the rate varies enormously and the variation is informative:

| Program | Audited | INTERIOR |
|---|---|---|
| `overlay_0897.bin` (field) | 163 | 101 (62%) |
| `overlay_magic_capture.bin` | 77 | 0 |
| `overlay_fishing.bin` | 10 | 7 |
| `overlay_muscle_dome.bin` | 13 | 4 |
| everything else | 51 | 14 |

The field overlay is where the phantom addresses live, and `magic_capture` -
the set the previous wave flagged as possibly unattributable - turns out to be
the cleanest set in the corpus.

**126 is a lower bound, and the bias is knowable.** The audit necessarily ran
against a database the repair passes had modified, so the number is not
independent of them. The direction is: a rebuild merges bodies and so creates
interiors, a restore splits them and so creates entries. This pass ran 376
restores against roughly 90 rebuilds, so the settled database has *more* entry
points than it started with and reports *fewer* interiors than a pristine one
would. Anyone re-deriving the figure on a fresh import should expect it to go
up, not down. (One concrete case: `0x801D0290` reported INTERIOR of
`FUN_801D01B0` on first contact and ENTRY afterwards, because an intervening
rebuild split the enclosing body.)

## What the 452 actually were

Re-graded with a sweep that recognises the header and section spellings the
corpus really uses, the cited-defective set is **369**, not 452. The missing 86
are the sweep counting the corpus's own correct answers against it: 76 pointer
stubs, 3 recorded `NOFUNC` negatives, 10 analysis outputs whose filenames end
`_<addr>.txt`. A pointer stub is the *right* handling of an interior address,
so the instrument was penalising the one practice that avoids the defect it was
scoring.

Of the 369, by what the file actually is rather than what it lacked:

| Nature | N | Repair |
|---|---|---|
| addressed stream, wrong extent | 237 | re-dump / rebuild |
| `ADDRLESS_DISASM` | 77 | whole stream, no address column |
| Ghidra decoded nothing | 26 | re-dump from a correctly-based program |
| other | 29 | mixed |

## Repaired

`--shape --cited-only`, same (fixed) instrument before and after:

| Class | Before | After |
|---|---|---|
| SOUND | 2911 | 3008 |
| NO_RETURN | 135 | 93 |
| TAIL_J | 86 | 54 |
| ADDRLESS_DISASM | 77 | 0 |
| HEADERLESS_C_ONLY | 29 | 23 |
| INTERIOR_SLICE | 20 | 87 |

**+97 sound dumps.** The `INTERIOR_SLICE` rise is the repair working, not
regressing: with 97 more sibling bodies at their true extent, 67 more dumps are
now *provably* inside one. They are the same population as the 126 above.

The remaining 279 cited-defective break down as 87 `INTERIOR_SLICE` (phantom
addresses - retire, do not re-dump) and 192 dumps that still want a re-dump.

Base axis, unchanged corpus, `break`-fold only: `NOT_FOUND` **124 -> 95**.
Twenty-nine dumps that read as "no extracted image holds these bytes" were the
comparison rejecting them on an operand the two disassemblers spell differently.

## Three findings worth carrying forward

**1. `overlay_magic_capture` is fully attributable, and slot A is PROT 0898.**
Not an inference from its `magic_level_up` sibling's label. All 77 of its dumps
share an `(entry, size)` extent with a `magic_level_up` dump; comparing the two
streams instruction by instruction, all 77 pairs are identical, and in 76 the
addressed sibling's printed addresses run `entry + 4*i` with no gap. Those
siblings resolve single-hit to `battle_action(898)`. A dump that cannot be
resolved directly can be resolved *through* one that can, whenever they share an
extent - and the extent is the key the attribution CSV is already built on.

**2. A sixth-and-a-half defect class: the dump is complete and reads as empty.**
`dump_magic_capture_overlay.py` wrote `ins` instead of `ins.getAddress(), ins`,
so its disassembly carried the instructions and nothing to key them by, and
emitted `--- DECOMPILED C ---` so the C body parsed as more disassembly. A
2781-instruction body read as zero. A human reading the file notices nothing;
only the tools do. Fixed at source and re-dumped.

**3. The frontier walk needed an upper bound, and finding that out cost a
corrective pass.** Unbounded, one forward branch beyond the routine drags the
frontier past every `jr ra` in between: a 68-byte routine became 20060 bytes, a
4-byte jump-table slot became 32256. Worse than the truncation it fixes, because
a rebuild deletes the entries inside its span and every address in the merged
body then reports INTERIOR of a fiction. Bounded now by a prologue-after-return
test plus a crossed-return budget, and `repair_truncated_dumps.py` grew a
`+addr` restore mode to undo what the first pass deleted. All rebuilds now
report `0 return(s) crossed`.

## Tool changes

- `check-dump-base-integrity.py`: `break` operand folded in `canon()`; header
  and section spellings widened; `ADDRLESS_DISASM` + `BODY_WITH_DATA` classes;
  pointer stubs / `NOFUNC` records / analysis outputs reported separately from
  defects; `--emit-base-csv` (per-dump printed VA, resolved VA, delta, image -
  what picks the program for a re-dump); `--audit-dumpers`.
- `repair_truncated_dumps.py`: frontier + frame-boundary walk with a
  crossed-return budget; interior guard that never rebuilds an interior
  address; `requested=` provenance line; `?` audit mode; `+addr` restore mode;
  verdict TSV; `NOFUNC` rebuilds marked `ENTRY STATUS UNVERIFIED`.
- `dump_pending_helpers.py`, `dump_funcs.py`, `dump_magic_capture_overlay.py`:
  named from `getEntryPoint()`, not the requested address.
- New `report_program_bases.py` - which import sits at which base. This is the
  fact a re-dump needs first and nothing else in the tree reported it.

## Not reached, with reasons

- **29 dump scripts still carry `NAME_MISMATCH`** (`--audit-dumpers` lists
  them). Only the two canonical ones (`dump_pending_helpers.py`, `dump_funcs.py`)
  and the one that produced the addressless set are fixed. The rest are one-off
  scripts; each needs the same three-line change, and none can be tested without
  re-running it.
- **4 `summon905_*` dumps** resolve (`SHIFTED`) to `0x801F7FA8`/`0x801F8078`/
  `0x801F9740`, which no imported program spans at its own base. Needs an
  import of the owning image before they can be re-dumped.
- **`801cf00c.txt`, `overlay_0897_xxx_dat_801c9688.txt`** - too short to sign
  and outside every imported span.
- **`overlay_str_fmv_0x801CFAD4.txt`** - the filename's `0x` prefix means no
  automated pass recovers an address from it.
- **1 `REBUILD_FAILED`** (`overlay_0897.bin 801f3450`), **1
  `NOFUNC_UNWALKABLE`**, **3 `RESTORE_FAILED`**.
- **`801d84b4` in `overlay_baka_fighter.bin`** walks 20060 bytes with zero
  returns crossed, where the same VA in four sibling captures walks 6024 and
  stops at a prologue. Either a genuinely huge single-exit dispatcher or a data
  region decoding as code; it wants eyes, not another heuristic.

## Two edits outside this lane's paths that someone should make

1. **`docs/subsystems/boot.md`** cites `ghidra/scripts/funcs/8003f000.txt` and
   `8003f0f4.txt`. Both filenames are phantom - the bodies are `FUN_8003EFE8`
   and `FUN_8003F08C`, which the prose already names correctly. The citation
   paths should become `8003efe8.txt` / `8003f08c.txt`.
2. **`docs/subsystems/motion-vm.md`, `cutscene.md`, `actor-vm.md`,
   `field-locomotion.md`, `world-map.md`** and several `functions/*.md` cite
   bare-named dumps that are name-mismatched (`801cf8ac`, `801d5a68`,
   `801d5c08`, `801d5d60`, `801d5e20`, `801d6058`, `801d79e8`, `801d841c`,
   `801dba20`, `801dbe9c`, `8002cdd0`, `8004f0e8`, ...). In each case the
   *claim* may be fine and the *file* is not; the fix is to re-point the
   citation at the enclosing entry's dump, one page at a time.
