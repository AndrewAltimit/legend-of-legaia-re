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
- **A committed claim can quote a dump statistic the dump no longer reports.**
  Re-extraction makes dumps longer, and a caveat written against the old one
  keeps suppressing work - `0x8005BA38` was recorded "not a function, do not
  open a port row" and is a complete `RotTransPers`. Checker:
  `scripts/ghidra-analysis/check-dump-stat-drift.py`; the class is on
  [`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md#a-caveat-outlives-the-dump-it-was-written-against).
- **A battle `0xB5` was read in the wrong id space.** Spell `0xB5` is Lapis
  Wave; formation `0xB5` is Cort. The branch at `0x801E6D04` reads the
  formation-id byte, and the wrong reading survived because Cort casts Lapis
  Wave - the two spaces agreeing on the answer is what hid the error.
- **"Retail shots rarely roll the camera" was false, and so was the row that
  replaced it.** Retail authors a non-zero op-`0x45` slot-2 roll in eight
  scenes. The renderer's dropped `RotMatrixZ` factor was a real divergence,
  and the two linear censuses that had measured it - one strict, one
  byte-resuming, plus a raw byte scan quoting a "2 %" figure as fact - were
  each measuring their own decode gate. Settled by execution; see
  [`re-settled-threads.md`](re-settled-threads.md#does-any-retail-shot-author-a-non-zero-camera-roll).

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
| Who latches the clip-end bit for a conversation's cross-context clip pokes | partial - the latch and the spin are settled; the port stands in for the latch instead of resolving the poked actor | [details ↓](#clip-end-latch-for-cross-context-clip-pokes) |

### Clip-end latch for cross-context clip pokes

*Status:* partial. The mechanism is settled from the disassembly: `ctx[+0x62]`
is the clip-control word, bit 8 (`0x0100`) is the "end" flag the actor tick
`FUN_800204F8` latches when a clip cursor reaches an end, and the recurring
`A2 <t> <clip>` / `AC <t> 08` / `AD <t> 08` triple is "play it, clear the
latch, spin until it re-latches" (see
[`script-vm.md`](../subsystems/script-vm.md#0x2b-0x33-flag-manipulation-triplets)).
A prop-bound record reaches the latch through its own `PropAnim` tick, which
is why doors and cupboards play out correctly.

What is not resolved is the **cross-context** form an NPC conversation uses.
`A2 F8 …` pokes the player channel, so retail's spin reads the *player
actor's* `+0x62` while the dispatcher is running the NPC's record. The port's
inline-dialogue runner keeps one context per record and never resolves the
`0x80`-prefix target to a live actor, so nothing writes that bit: it cues the
player clip, parks the conversation while the clip plays, and then sets the
tested bit itself as a stand-in for the tick's write. That reproduces the
observable beat (the gesture plays, then the script continues - which is what
makes an inn stay reachable, see [`inn.md`](../subsystems/inn.md)) but it is
not the retail data path, and it cannot be right for a poke at a *third*
actor's clip.

*What would close it:* resolve the ext-target byte to the live actor the way
`func_0x8003C83C` does, run the dispatcher against that actor's own
`+0x62`/`+0x6A` words (the prop stepper already does exactly this for props),
and let the existing clip players own the latch.

The capture that would pin the sequence to check it against is narrow and
well-specified, so it is worth stating exactly rather than as "take a capture":

| What | Value |
|---|---|
| Where | Any inn, at the innkeeper conversation - the `A2 <t> <clip>` / `AC <t> 08` / `AD <t> 08` triple with `t = 0xF8` (the player channel) |
| Arm | A **write**-watch on the *player* context's `+0x62`, resolved through the same id→context path `func_0x8003C83C` uses, not on the NPC record's |
| Log | `(pc, ra, value)` per write, plus `+0x6A`, across the whole spin |
| Answers | Whether the actor tick `FUN_800204F8` is the only writer of bit 8, and what cursor values accompany the latch - i.e. what the port's clip players must reproduce instead of the stand-in write |

The two ids `func_0x8003C83C` special-cases (`0xFB` = system, and the player
channel) are the reason a naive watch on the dispatching record's context sees
nothing: retail's spin is reading a **different struct** from the one the
dispatcher is running.

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

*What this needs is capture time, not a new instrument.*
[`scripts/pcsx-redux/autorun_flag_firehose.lua`](../../scripts/pcsx-redux/autorun_flag_firehose.lua)
is already the right probe and already logs exactly what the residual asks
for: an exec breakpoint on the flag SET / CLEAR entry points with the writer's
`ra`, plus a per-VSync scene-name and game-mode poll, so a single play-forward
through a region emits the region's own SET order with the scene each write
happened in. It is designed for whole-playthrough runs, so the four unwalked
regions can be covered in one session rather than four.

Two operating notes that apply to any run of it, both already bitten:
PCSX-Redux probes **do not exit on their own** - kill on a timeout or the
process hangs indefinitely
([`pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md)) - and the
process-matching helpers in
[`shell-observer-traps.md`](../tooling/shell-observer-traps.md) exist because
`pgrep -f` matches the caller's own command line.

## Title / boot / overlays

| Thread | Status | What would close it |
|---|---|---|
| PROT 0968 identity - the one unidentified slot-B cluster entry | mostly resolved - it is the Cort battle's stage overlay; only a residency capture is missing | [details ↓](#prot-0968-identity---the-one-unidentified-slot-b-cluster-entry) |

### PROT 0968 identity - the one unidentified slot-B cluster entry

*Status:* mostly resolved. The loader chain, the selector, the module's real
extent and its call profile are all pinned from the disassembly and the disc
bytes; what is unconfirmed is a live capture showing it resident. The full
write-up is on
[`re-settled-threads.md` § PROT 0968](re-settled-threads.md#prot-0968---the-cort-battle-stage-overlay).

In short: `formation monster id 0xB5` (archive id 181, **Cort**) sets the
battle-stage id byte to `2`, and the stage-overlay path loads
`FUN_8003EC70(stage_id + 0x47)` - loader param `0x49`, extraction entry 968.
The old "`0x49` appears at no static SCUS callsite" statement was true and
uninformative: the parameter is **computed**, so no constant-parameter scan
can ever produce it.

*What would close it:* a save state taken inside the Cort fight showing
entry 968's bytes resident in the slot-B buffer at `0x801F69D8`, and the
loader-B current-id tracker `gp+0x934` (`0x8007BC4C`) reading `0x49` - the
same pair of observations that pinned 0967 for the Tetsu tutorial. Note the
formation-table watch in
[`autorun_flag_firehose.lua`](../../scripts/pcsx-redux/autorun_flag_firehose.lua)
already logs `DAT_8007BD0C[4]`, so one run through that fight confirms the
`0xB5` selector and the residency together.

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
