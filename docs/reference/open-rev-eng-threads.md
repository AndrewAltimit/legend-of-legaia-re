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
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |
| teien hedge-base ground fill (kind-2 tile-trigger cells) | open | [details ↓](#teien-hedge-base-ground-fill) |

Recently closed here: Rim Elm's south gate. Neither of its two walk-on bands
was the mechanism the symptom suggested - the exit record is ungated and the
other record is five inert bytes; what holds a player in is a collision row the
gate object's own script paints. See
[`re-settled-threads.md` § Rim Elm's south gate](re-settled-threads.md#rim-elms-south-gate),
and the force-walk reading it falsified in
[`re-do-not-re-walk.md`](re-do-not-re-walk.md#the-reachable-bands-record-force-walks-the-player-through-the-wall).

### teien hedge-base ground fill

*Status:* open - the port is byte-faithful to every pinned draw channel; the question is whether retail has one more

Under teien's hedge maze the cells along the hedge rows carry only
object-grid bit `0x0800` (kind-2 tile-trigger presence - the height-override
platform records `FUN_80019278` reads), not the `0x1000` ground-draw bit, so
neither retail's pinned ground-quad emitter (`FUN_801f6d48`, gate
`(cell & 0x1000) != 0`) nor the port's `build_walk_heightfield` (same gate,
verified byte-exact against retail's load-time recompute in `FUN_80017BEC`
for kor5/teien/town01) emits ground there. Through the hedge sprites'
authored cutout texels that reads as black holes along the hedge bases from
free-camera angles retail's fixed camera may never reach. If retail really
shows grass in those cells, the filler is an **unpinned kind-2-cell draw
channel**. What would close it: a mednafen/PCSX-Redux save state inside
teien + the display-list read (a RAM image carries the frame's libgpu OT -
see `docs/tooling/mednafen-automation.md`), checking whether any ground
prim covers a `0x0800`-only cell. Until then the engine must not grow a
speculative fill.

### Coplanar residual tail: same-position curved-shell stacks

*Status:* open - the mitigation stack resolves the flat-plane corpus; the residual is a structurally different shape

After the cross-draw coplanar kernel's per-family lifts and repair pass
(`engine-core::coplanar_draws`; the whole model is in
[`renderer.md`](../subsystems/renderer.md#coplanar-surfaces-retails-ordering-model-the-ports-depth-policy)),
the corpus sweep (`DIAG_ALL=1` on `engine-core/tests/coplanar_residual_disc.rs`)
still reports a small tail, dominated by two shapes. First, **same-position
stacks of curved shells** - two different env TMDs placed at one translation
whose curved surfaces coincide (jouine/jouind's flesh-cave walls, chitei2's
res41/res45 slope): a per-draw *translation* cannot separate two coincident
curved surfaces everywhere (any direction is tangent to some part of the
shell), so the offset API is structurally the wrong tool. Open questions:
does retail even draw both copies (they may be state/morph variants the
scripts swap), and if it does, which wins per OT bucket? A display-list read
from a save state inside jou would answer both. Second, **sub-cluster
slivers** - wall/kerb strips whose per-plane area inside one mesh falls
below the detection floor or fragments across the cluster quantization
(koin4 keeps one sub-100-area example the regression test bounds). Neither
shape is angle-stable shimmer of a whole floor - the class the kernel
exists for and now clears.

### Region story-flag gate families

*Status:* structure resolved and settled; residual = play-order confirmation for the dungeons the capture corpus never walked

The per-region C1/C2 gate families - the partition-2 record-header flag lists
the spawn evaluator `FUN_8003BDE0` checks - are decoded across the chapter-2/3
regions and the Rim Elm variants, with every family's exact lists pinned by
census-file anchor tests. The full structure (Sebucus spokes, Rim Elm
opening/revisit/final bands, Uru Mais, Nivora Ravine, Karisto castle depth,
Conkram, and the `0x7`/`0xF` variant-discriminator pattern) lives on
[`re-settled-threads.md` § Region story-flag gate families](re-settled-threads.md#region-story-flag-gate-families).

**Residual.** Poll-tier playthrough captures
(`captures/state_poll/2026-07-29T20-20-05Z` / `2026-07-29T22-21-04Z` /
`2026-07-29T22-53-56Z`, mined with save-state-load frames screened out by
their mode-churn + inventory-rewrite signature) confirm live play order for
`retona`, `dohaty`, `taiku`, the Sebucus teien→tower→geremi spine, `korb3`,
the `kor5` chain head (`0x43A → 0x436`) and the `map03` hub latch — the
observed orders live in the settled page's play-order-captures paragraph,
alongside the earlier organic `ropeway`/`ropeway2`/`jiji` walks and Nivora's
`0x370` SET. Still owed:

- **never walked:** `rayman`/`rayman2`, `station`/`station3`, and the Karisto
  spokes `bubu2` + `deroa`/`chitei2`;
- **walked without an organic family SET** (the beats were already latched in
  the loaded state, or the region was entered mid-arc): `retock`/`retockin`
  (`0x502` never fired; `0x357` pre-latched), `doman` (`0x3FB` did not fire),
  `nilboa`'s entry family, `son`, and the `kor5` tail `0x6C4`.

The generic C1/C2 seeder already drives every family. One more session from
an early-enough save (before the retock/doman/nilboa beats) closes the
walked-but-latched set; the never-walked set needs the walks themselves.

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
| The battle-**intro** enemy-name banner - which placement record raises it | mostly resolved (chrome, seat and frame law captured on the same surface mid-fight; only the record identity is owed) | [details ↓](#the-battle-intro-enemy-name-banner) |

### The battle-intro enemy-name banner

*Status:* the chrome, the seat and the frame arithmetic are captured - on the
same banner surface, mid-fight. What is owed is only which placement record
the intro instance raises.

The thread opened as "chrome and seat unknown, blocked on a live frame", on
the premise that no manifest state catches the surface. That premise was
wrong about the surface rather than about the intro: a corner-sweep of the
whole save library finds the battle's **full-width top message banner** live
in two states - `rim_elm_gimard_seru_capture_after` (the mid-battle Seru
"captured!" banner) and `noa_levelup_banner`.

Both draw the same thing, and it is the frame the thread described. It is the
widget table's **class-0 9-slice window**, tile-set 0, sub-palette 2: 4x4
corners and 24x4 / 4x24 edges cut from one 32x32 patch at texels `(160, 0)`,
content pen `(16, 12)`, frame origin `(8, 4)`, interior 20 tall, top and
bottom edges tiled 24 wide from `x = 12` with the last tile clipped. Every
placement record whose kind byte is `0x03` / `0x04` / `0x44` frames itself
that way. Table and law:
[`battle.md`](../subsystems/battle.md#the-widget-class-table---where-every-chrome-sprite-comes-from).

One sub-claim in the old row is **falsified** by those frames: there is no
blue interior. Retail draws the border sprites and the glyph run and nothing
else - no fill primitive of any kind under the window, so the scene shows
through. The blue-marbled 32x32 patch that made "gold border over a blue
interior" a natural reading is the fill the framed *menu* windows use.

Residual: the intro instance is transient, so which top-seated `0x0303`
record it raises - the candidates park at `(16, -24)` and live at `(16, 14)`,
and the runtime overwrites the disc width with the measured enemy name - is
still capture-owed. The runbook is unchanged: drive
`v0_1_battle_loading_tetsu` forward under PCSX-Redux and dump main RAM on the
first frames after the mode flips. The read-out is mechanical -
[`scripts/mednafen/widget-draw-sweep.py`](../../scripts/mednafen/widget-draw-sweep.py)
joins any frame's sprites back to the widget records that drew them.

Two operating notes apply to any run: PCSX-Redux probes **do not exit on their
own** ([`pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md)), and
`pgrep -f` matches the caller's own command line
([`shell-observer-traps.md`](../tooling/shell-observer-traps.md)).

Recently closed in this area: the `+0x0E` kind-pair mapping and the
element-badge palette selector both fell out of the widget-class table, and the
status-element badge sheet `0x18..=0x20` is pinned cell by cell. Before them,
the ground grid's depth-cue far colour and the battle-intro tile shatter's
side-face shade page closed by capture. All in
[`re-settled-threads.md`](re-settled-threads.md#battle--arts--level-up).


## Audio / BGM

No open threads. The most recent one - op-`0x35` sub-op `0xA`, the
"unhalt-pause toggle" - resolved as the track-swap **commit** and moved to
[`re-settled-threads.md`](re-settled-threads.md#op-0x35-sub-op-0xa-is-the-track-swap-commit).

## Title / boot / overlays

No open threads. The last one - PROT 0968 identity, the one slot-B cluster
entry without a residency capture - closed by capture: the
`cort_evolved_battle_first_menu` PCSX-Redux state (first command menu of the
evolved-Cort fight, before any cast) shows the loader-B tracker `0x8007BC4C`
reading `0x49` and entry 968 100% byte-resident at `0x801F69D8` over its own
`0xA28` extent, with the field-side ladder states bracketing the page-in to
the battle load. See
[`re-settled-threads.md` § PROT 0968](re-settled-threads.md#prot-0968---the-cort-battle-stage-overlay);
the instrument is
[`check-0968-residency.py`](../../scripts/mednafen/check-0968-residency.py),
which reads either emulator's states (mednafen via `mednafen-state`,
PCSX-Redux `.sstate` via `pcsxr-state`, dispatched on file extension).

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
