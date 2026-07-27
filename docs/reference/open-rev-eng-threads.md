# Open reverse-engineering threads

An index of reverse-engineering **questions** about Legaia's runtime that are
still live. Rows are questions, not progress markers: each says what is
settled, what remains, and what evidence would close it.

## What this page is for

Before starting a hunt, look for it here. If the question is not on this page,
it is probably already answered or already disproved - the two companion pages
below hold those, and checking them first is cheaper than re-deriving them.

| Page | Holds | Read it when |
|---|---|---|
| This page | Live hunts: `open`, `partial`, `mostly resolved` | You are picking up work, or want to know whether a question is still contested. |
| [`re-settled-threads.md`](re-settled-threads.md) | Answered questions, each carrying an evidence grade | You need the answer to something, or you are about to build on a claim and want to know how firmly it is pinned. |
| [`re-do-not-re-walk.md`](re-do-not-re-walk.md) | Falsified hypotheses, reasoning intact | A reading of the bytes looks obvious and you want to check nobody has already spent a week disproving it. |

A falsified row is kept forever, with its reasoning: "the world-map slot-4
bodies are coastline wireframes" is a very plausible reading of those bytes,
and knowing *why* it is wrong is worth more than the row it occupies.

Nothing on any of the three pages counts ports, tests, or coverage. Detailed
captures and decompiler dumps live in the linked docs and under
`ghidra/scripts/funcs/`.

## What an evidence grade means

Every settled row carries one of four grades, naming what its own stated
evidence actually rests on:

| Grade | The row cites |
|---|---|
| `disassembly` | Instructions, addresses, opcode encodings, branch or store sequences. The strongest grade. |
| `capture` | A runtime capture, save state, probe, firehose, or disc-derived oracle. |
| `decompiled-C` | Ghidra's C output, a `FUN_x(...)` call signature, a Ghidra label or plate comment, or a claim about store order / store count / a boolean operator with no instruction behind it. |
| `inference` | Reasoning from surrounding facts, corpus absence, or analogy, with no direct evidence cited. |

`decompiled-C` marks a claim **nobody has confirmed against instructions** - not
a claim known to be wrong. Most of them are probably right. But the C is a
rendering, and every claim falsified in the last audit wave would have graded
`decompiled-C`: dropped register arguments, `||` printed as nested `if`s,
reordered or omitted stores, and hand-written Ghidra annotations read as fact
have each already put a wrong statement on these pages. The catalogue of the
seven rendering artifacts is
[`ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims);
it is also the grading rubric. When a `decompiled-C` row is load-bearing for
something you are about to build, re-derive it from the disassembly first.

## Status conventions

| Status | Meaning |
|---|---|
| **open** | Active hunt. A concrete next step exists; the row names it. |
| **partial** | The main result is pinned; a residual sub-question remains. |
| **mostly resolved** | The mechanism is pinned; one leg is unconfirmed. |

Many rows qualify the status in parentheses - `partial (transcode closed)`,
`open (narrowed)` - naming *how far* it got. Read the parenthetical.

## How a thread is laid out

Each area below opens with a table of one-line rows. A thread whose write-up
outgrows a table cell keeps its one-liner in the table and links to a `###`
section immediately after that table via **[details ↓]**; the full
analysis - every address, capture, and falsification - lives in that section,
under its own *Status:* line.

## Recently corrected

Rows the last audit wave overturned. They are listed here rather than filed
silently into the settled page, because a claim that was wrong once is the
cheapest place to look for a claim that is still wrong.

- **Debug flag `_DAT_8007B8C2` had its branch sense backwards**, and one arm
  was named for the wrong loader. Every site reads the flag with `lh` and takes
  the **zero** arm to the debug-station host trap, the **non-zero** arm to the
  index resolver. The flag is settled on that polarity, writer and all - see
  [`re-settled-threads.md`](re-settled-threads.md#_dat_8007b8c2-polarity-and-its-writer).
- **The VA-aliasing corollary was too narrow.** It read as an `0x801Fxxxx`
  problem; `801e23ec` is a settled casualty in the `0x801E` band, and its
  aliased reading had silently dropped all three initiative modifier terms.
- **The op-`0x2F` "seven byte-identical dumps" shorthand was compressing.** The
  capture-derived dumps agree with each other; the static 0897 dump is a strict
  *subset* of them, not a twin.
- **`FUN_8001EBEC`'s second pose-copy arm loads seven words**, so its range ends
  at `+0x15C`, not `+0x158`.

Two rows a prior audit flagged as the highest-risk `decompiled-C` claims on the
register - the narration-roller op's operand decode and the item-add OOB
store-order claim - have both now been **re-derived from the disassembly and
confirmed** (grade `disassembly`). The store orders and operand shapes stand as
written; both rows live on [`re-settled-threads.md`](re-settled-threads.md)
with the instruction evidence cited.

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order for the dungeons the capture corpus never walked is still owed | [details ↓](#region-story-flag-gate-families) |

### Region story-flag gate families

*Status:* structure resolved and settled; residual = play-order confirmation for the dungeons the capture corpus never walked

The per-region C1/C2 gate families - the partition-2 record-header flag lists
the spawn evaluator `FUN_8003BDE0` checks - are decoded across the chapter-2/3
regions and the Rim Elm variants, with every family's exact lists pinned by
census-file anchor tests. The full structure (Sebucus spokes, Rim Elm
opening/revisit/final bands, Uru Mais, Nivora Ravine, Karisto castle depth,
Conkram, and the `0x7`/`0xF` variant-discriminator pattern) lives on
[`re-settled-threads.md` § Region story-flag gate families](re-settled-threads.md#region-story-flag-gate-families).

**Residual.** The families for the dungeons the capture corpus never walked
(`taiku`/`doman`/`rayman`, `station`, `dohaty`/`retock`, the Karisto spokes)
are proven as structure, but their in-game play order is not yet confirmed
against a live capture. `ropeway`/`ropeway2`/`jiji` are the only spokes walked
organically, and Nivora's `0x370` has one live organic SET confirming its
play order. The generic C1/C2 seeder already drives every family, so one
dungeon-walk capture per region would close the residual.

## Title / boot / overlays

| Thread | Status | What would close it |
|---|---|---|
| PROT 0968 identity - the one unidentified slot-B cluster entry | open (narrowed to one entry) | [details ↓](#prot-0968-identity---the-one-unidentified-slot-b-cluster-entry) |

### PROT 0968 identity - the one unidentified slot-B cluster entry

*Status:* open - every other entry in the `0900..0969` slot-B cluster is
identified ([`re-settled-threads.md`](re-settled-threads.md#slot-b-overlay-cluster-09000969-per-entry-identity))

A 4 KB slot-B module (pointer-table head, 10/11 self-pointers resolve at the
link base `0x801F69D8`, 2+8 spawn calls - stager-shaped). Its loader param
would be `0x49`, which appears at no static SCUS callsite and no captured
overlay callsite; it sits between the tutorial (0967, `0x48`) and the STR-path
table (0969, `0x4A`). Identity needs either an uncaptured overlay callsite or
a residency capture. (The old "STR overlay replicated across 0967/0968/0969/0970"
reading was the over-read: the corrected 4 KB entry cannot contain the STR
dispatch code at its `+0x225C` offsets.)

## Adding a thread

A thread belongs here when:

1. There is something *specific* that would close it - a probe to run, a dump to read, a function to port. "Generally understand X better" is not closable; skip.
2. The next step is non-obvious from the code or git log. If `grep` would surface it, no row needed.
3. The detail lives elsewhere (a memory entry, a docs page, a Ghidra dump). The row is the pointer, not the analysis.

When the thread closes, rewrite the row to a `falsified` or `done - kept for reference` line if the path was instructive enough to warrant a "do not re-walk" marker; otherwise delete the row. Rotating the page is part of using it.

## Related pages

- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade. Check here before opening a hunt.
- [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - the falsified hypotheses, reasoning intact.
- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) - per-function dumped × documented × ported × ignored axes. `port-catalog.py --missing-ports` is the function-level companion to this page's question-level index.
- [`docs/reference/functions.md`](functions.md) - canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml) - addresses explicitly *not* worth investigating (statically-linked PsyQ infra). Disjoint from this page.
- [`docs/tooling/worklist-classification.md`](../tooling/worklist-classification.md) - classifies each `--missing-ports` row by whether it is a portable function entry at all. Read it before treating a bare address on the worklist as an open question: `INTERIOR`, `SHARED_TAIL`, `DUPLICATE` and `VA_ALIASED` rows are not work.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base, and the one dump window whose targets are therefore untrustworthy.
- [`docs/subsystems/vm-inventory.md`](../subsystems/vm-inventory.md) - every VM-shaped subsystem with its op space, port status and whether anything live calls the port. Several rows on this page are questions about one of its entries.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that have each already put a false claim into these docs. A `resolved` row whose evidence is decompiled C rather than instructions has not been audited against this list.
