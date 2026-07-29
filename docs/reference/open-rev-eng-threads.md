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
- **Op-`0x35` sub-op 9 was recorded as "Queue".** It is a *start* behind an
  asset-load barrier - the arm at `0x801E0224` waits on
  `_DAT_8007BAB8 == _DAT_8007BA9C` and then makes sub-op 1's own
  `_DAT_8007BAC8` store. The one-word entry had no body under it, and it is
  what a **cutscene** changes music with, so a scene-corpus BGM sweep (which
  only runs prescripts, and those emit sub-op 1) could not see the difference.
  Falsification: [`re-do-not-re-walk.md`](re-do-not-re-walk.md#op-0x35-sub-op-9-was-never-a-queue).
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

## Battle / rendering

| Thread | Status | What would close it |
|---|---|---|
| Battle ground grid's depth cue - the far colour | partial - `DQA` / `DQB` pinned; the far colour `FC` is not | [details ↓](#battle-ground-grids-depth-cue---the-far-colour) |
| Battle-intro tile shatter - the side-face shade page | open - the page is live only during a transition, and no capture is | [details ↓](#battle-intro-tile-shatter---the-side-face-shade-page) |

### Battle ground grid's depth cue - the far colour

*Status:* two of three inputs pinned; the port draws the grid unfogged

Retail's ground-grid emitter runs **`DPCS`** per vertex (`cop2 0x780010` at
`0x801d061c` / `0x801d063c` / `0x801d0654` / `0x801d0688`, with `IR0` loaded
from `SZ >> 2` immediately before each), so the battle floor **fades with
distance**. The port's `build_ground_grid` emits `MODULATION_NEUTRAL` and no
cue at all, which is why its grass reads at full brightness all the way to the
horizon while retail's washes out.

`DPCS` is `out = c + (fc - c) * ir0`, and the port already has the kernel -
`engine_render::psx_light::depth_cue`, with a `psx_depth_cue` WGSL twin. Three
control values feed it. Two are now pinned across nine save states spanning
field, battle, battle-load and minigame phases
(`crates/mednafen/tests/gte_projection_real.rs`):

| Input | Value | Status |
|---|---|---|
| `DQA` | `-64` | pinned, and invariant across every phase measured |
| `DQB` | `320 << 16` | pinned, likewise invariant |
| `FC` (`RFC`/`GFC`/`BFC`, control regs 21-23) | **not pinned** | varies by phase; reads `(0,0,0)` in battle states and `(4096,4096,4096)` in field ones |

**What would close it.** `FC` is a *snapshot* value: a save state captures
whatever the last GTE setup left, and in a battle state that is as likely to
be the UI pass as the grid pass. So reading it out of a state is not enough -
the reading has to be attributed to the grid draw. Two routes, either
sufficient:

- A PCSX-Redux exec breakpoint on `func_0x801d02c0` that dumps control regs
  21-23 on entry. That attributes the value to the grid pass by construction,
  which a save state cannot.
- The static writer: whatever calls `FUN_8003D268`-class far-colour setters
  ahead of the mode-`0x15` render `FUN_80026f50`. The sibling stage table
  `DAT_80078C1C` is already decoded and is part of this picture - it selects
  the backdrop's depth-cue ceiling (`0x800` vs `0xC00`) and a far-colour
  scaling arm (`>>1` versus `(c - 0x010101) * 2`) via `0x8007BDA8`, read in
  `FUN_80050120` at `0x800505b8` and `0x800507fc`. Those are the arms that
  *modify* a far colour; the base it starts from is the missing piece.

Until then the grid stays unfogged, which is a visible but bounded
divergence - and preferable to fogging it toward a guessed colour, which would
be wrong in a way nothing downstream could detect.

### Battle-intro tile shatter - the side-face shade page

*Status:* three of four emitter inputs pinned; the port ticks the style but does not draw it

The tile shatter is the style the **ordinary random encounter** takes, so it is
the most-seen transition in the game and the port draws none of it. The
emitter is fully specified - see
[`cutscene.md`](../subsystems/cutscene.md#what-style-2s-emitter-builds-and-the-one-input-still-missing)
for the ten-primitive face table, the reject chain and the OT depth - and the
corner table (`[0, 1, 17, 18]`, off PROT 0979) and the GTE screen centre
(`OFX = 160`, `OFY = 114`) are both pinned.

**What is missing** is the content of the 4bpp page at VRAM `(448, 0)` that the
four semi-transparent side faces stretch over. Its CLUT at `(16, 473)` reads
as a 16-entry black-to-white brightness ramp in a battle-load state, which is
what a shade texture's palette should look like, but the page itself is sparse
there - and every catalogued state is captured before or after a transition,
never during one, which is the only window where the page is live.

**What would close it.** One save state taken *inside* a field-to-battle
transition, then `mednafen-state vram-dump` over `(448, 0)` for 64x64 texels at
4bpp. The style runs for a known, short number of frames, so the capture wants
a scripted pause rather than a human reflex - a PCSX-Redux breakpoint on
`FUN_801D0D24` with a state dump on the first hit would land it directly.

## Audio / BGM

| Thread | Status | What would close it |
|---|---|---|
| Op-`0x35` sub-op `0xA` - what the "unhalt-pause toggle" waits on | open - the arm is read, its two inputs are not | [details ↓](#op-0x35-sub-op-0xa---what-it-waits-on) |

### Op-`0x35` sub-op `0xA` - what it waits on

*Status: open.* The arm at `0x801E0264` is legible instruction by instruction
and still unnamed as a behaviour. It returns immediately when `_DAT_8007B868`
is non-zero; otherwise it **waits** (branch to `0x801DEE4C`, the restore-PC
idiom) until bit 3 of the sound flag word `_DAT_8007B750` is set, then calls
`FUN_800266E0` and `FUN_80026520` on the BGM slot `0x8007052C`, sets bit 4 of
the flag word and **clears bit 1** - the pause bit sub-op 2 sets.

What is not pinned: what writes `_DAT_8007B868`, what sets flag bit 3, and what
`FUN_80026520` does that `FUN_800266E0` does not. Until those are answered the
op has no port, which is visible on the disc: a scene's cutscene records pair
sub-op 2 with a later sub-op `0xA`, so a port that honours the pause and drops
the toggle leaves the music paused after that cutscene.

Closing it wants a writer sweep for the two globals
(`scripts/ghidra-analysis/find-address-word-refs.py`) plus a live capture over
a scene that runs the `2` / `0xA` pair.

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
