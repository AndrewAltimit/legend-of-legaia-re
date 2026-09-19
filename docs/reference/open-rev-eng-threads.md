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

- **The field view matrix is built per field *frame*, not three times a
  vsync, and two of the builder's five sites are not a camera at all.** Over a
  world-map-to-town entry, 749 of 1800 captured vsyncs carried any build; of
  those, 389 ran the full three-site order and 313 only the last two. The two
  sites a sweep over `SCUS_942.54` and the slot-A images cannot reach are one
  bracket inside PROT 0901's `FUN_801F73E4`, which zeroes the yaw word in the
  first call's delay slot, draws one screen-fixed band and rebuilds
  ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **The last build before the draw does not win - it frames no geometry at
  all.** A link count already put 16810 of 31046 ordering-table links under the
  first build and 14 under the last; splitting by GPU command code gives the
  real figure, because that count was mixing attribute packets and 2D rects in
  with polygons. Of 4289 polygons over three runs, 3861 are under the first
  build, 428 under the second and none under the last
  ([settled](re-settled-threads.md#rendering--camera)).
- **A captured SPU voice is audible by its envelope level, not its phase
  word.** Mednafen's `ADSR.Phase` has no `Off` member - a key-off parks a voice
  in Release - so a phase test counts every voice a state has ever keyed. It is
  why retail snapshots read 20 to 24 of 24 voices live and why an oracle rule
  looked unsatisfiable; against the level, retail holds 4 to 9 audible voices
  and the engine 4 to 8
  ([falsified](re-do-not-re-walk.md#audio--sound-driver)).
- **The slot-B stage selector `_DAT_8007B64A` does have a writer.** The field
  entity tick `FUN_801DA51C` clears it at `0x801DA69C` and raises `1` at
  `0x801DA6A8` when system flag `0x19` is set; battle latches `3` at
  `0x801E6D2C`. Every access is `gp`-relative, which is why an
  absolute-address sweep reported none
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **The cast-cue band's door is an item, not a spell.** `FUN_801F3990` has one
  caller, action-SM state `0x3D`, entered only from `0x3C`; the Item category
  arm stores `0x3C` unconditionally while the Magic arm stores it only for
  spell ids below `0x65`, which the player Seru block cannot satisfy - so a
  cast-driven sweep could never reach the band
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **PROT 0981 is the world-map top-view debug image, not a monster-test
  harness.** The `monster_test` label is CDNAME inheritance from the block that
  opens at extraction 0978; the image's own operands are the world-map location
  table, the kingdom filter and the camera pair
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **`FUN_801EA9B0`'s `BGM CALL` arm plays a track; it does not cycle the
  index.** Cycling is `FUN_801E9F64`'s job. The arm installs a sound-test row's
  global id, which is what makes it the fourth disc-wide writer of the BGM
  request word ([falsified](re-do-not-re-walk.md#audio--sound-driver)).
- **The equip compare panel's `0x40` no-passive sentinel is not a free
  substitution.** Every class-`1` equipment row carries it, so the sentinel
  reproduces that arm exactly and nothing else; 80 of the 151 non-equipment ids
  carry a real passive index on the other arm, and a host feeding the sentinel
  unconditionally loses two of the three row sets
  ([falsified](re-do-not-re-walk.md#menus--ui)).
- **The browser play page's frame path does short-circuit.** Its early-out is
  in the page's JavaScript rather than in the Rust runtime, so a guarded frame
  runs none of the per-frame kernels and still draws
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **Two minigame citations were off by bytes rather than by reading.** The Baka
  Fighter editor's actor prototype and its sibling start four bytes lower than
  cited - the cited words are each record's `0xFFFF0000` field, not its head -
  the fishing bite tick's two per-frame map reads are unrelated (the water gate
  is the cell halfword's `0x4000` bit, while the walk-grid probe drifts the
  lure), and the halfword read as the lure's z is its **height** - the store
  takes the spawning actor's `+0x16` less `0x80`
  ([falsified](re-do-not-re-walk.md#field--locomotion)).

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |
| Is `juui1` dark in retail outside its `ColorIntensity` tint beats? | open - the donor door is named; the state to drive it from is not | No library state is inside the scene, and the name-hijack probe cannot answer it from a world-map door: the rewritten name loads the bundle through retail's loader, but every frame is black, and a hijack into the brightly-drawn `bylon` is equally black - the entry path makes the frame, not the scene. A disc-wide census now names the door: of 368 op-`0x3F` doors, 250 are field-to-field and 41 carry a five-letter destination across 17 source scenes, and `conc2` -> `juui1` is a direct one (index 587, MAN `0x7F12`). Only `teien` of those 17 has a catalogued state, and 900 vsyncs there cross no door - so what is owed is a `conc2` state one press from it. |

**Does retail's equip screen offer a Goods-slot candidate** closed here: yes,
out of the item class-`2` id space, through three builder cases the browse step
selects by writing a content id per row. Their filter carries no character mask -
the masked rule belongs to the armament half only - and 80 of 151 class-`2` ids
pass it ([settled](re-settled-threads.md#field--locomotion)).

**What consumes the fishing bite tick's per-cell fish weight** closed here: it is
the modulus of the caught fish's **size** roll, and the same value plus `0x400`
becomes the render scale of the object the catch spawns. It touches neither the
species roll nor the bite credit, so a deeper cell makes a bigger fish rather
than a different one ([settled](re-settled-threads.md#field--locomotion)).

**Which view build the frame draws under** closed here, and the answer is the
**first**. Splitting each ordering-table link by GPU command code over three
runs - including the real `map01` -> `town0c` entry the question was asked
about - gives 4289 polygons: 3861 under the first site, 428 under the second,
and none under the third or either of PROT 0901's slot-B pair, whose whole
share is attribute packets and 2D rects. The earlier link-count ranking was
mixing those in with polygons
([settled](re-settled-threads.md#rendering--camera)).

**What a field submode returns to** closed here: nothing reads the parked
word. The op-`0x49` enter stores it twice, neither `SCUS_942.54` nor any of the 86
extracted overlay images loads it at any width, and a live read watch across a
submode enter records none either - so the return rides the driver's own
handler slot and the port's collapsed chain
drops a store retail never consumes ([settled](re-settled-threads.md#field--locomotion)).

Three rows closed here at once, two of them camera. **What composes the field camera's
`TR`** - the live eye trio is the eye-space translation and the focus is MVMVA'd
through the scaled rotation into it, so there is no eye-back depth constant to
calibrate ([settled](re-settled-threads.md#field--locomotion)); four readings
fell with it, including the sibling routine that turned out to be another mode's
view build ([falsified](re-do-not-re-walk.md#rendering--camera)). **What
`edbylon` selects when the tile query misses** - nothing does: no site re-queries
on a bare tile crossing, so the block is held from wherever the player last
crossed a queried tile, and the scene's one query site is elsewhere. And **the
Throw Out cursor**, which is a bag slot; the displayed list hides empty slots
while the payload does not, so the two disagree on exactly the bags a fixture
never builds.

Before them: **what arms `_DAT_8007B8B8`**, the gate on the field
overlay's one ambient template. It is a one-shot entry-mode *argument* rather
than a latch - ten writers and 25 readers over 84 images, of which six sites
write zero - so the `0x801F271C` spawn is per-scene, not boot-only
([settled](re-settled-threads.md#field--locomotion)). With it, the **coplanar
residual tail**: the curved-shell half was answered by a display-list read
([settled](re-settled-threads.md#does-retail-stack-coincident-curved-shells)),
and the sliver half turned out to be the port's own repair pass
([falsified](re-do-not-re-walk.md#rendering--camera)). Before them, **teien's
hedge-base ground fill**, which had a false premise. Retail has no kind-2-cell draw channel at all - a live `teien`
field-run pass visits 1536 window cells and emits 370, exactly the cells
carrying `0x1000`, and none of the 42 `0x0800`-only cells
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion)). Before it, Rim Elm's
south gate. Neither of its two walk-on bands
was the mechanism the symptom suggested - the exit record is ungated and the
other record is five inert bytes; what holds a player in is a collision row the
gate object's own script paints. See
[`re-settled-threads.md` § Rim Elm's south gate](re-settled-threads.md#rim-elms-south-gate),
and the force-walk reading it falsified in
[`re-do-not-re-walk.md`](re-do-not-re-walk.md#the-reachable-bands-record-force-walks-the-player-through-the-wall).

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

- **never walked, and the corpus cannot help:** `rayman`/`rayman2`,
  `station`/`station3`, and the Karisto spokes `bubu2` + `deroa`/`chitei2`. A
  sweep of both emulators' state populations finds **no** state in `station`,
  `station3`, `bubu2`, `deroa` or `chitei2`, and the one `rayman` state answers
  a different question: an idle firehose over it logs 0 SETs and 2 CLEARs
  (`0x11`/`0x12`, writers at `ra` `0x801D551C` / `0x801D55DC`), because a
  family SET fires on the beat, not on standing still. These need a human
  play-forward, not another probe;
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
| Does any shipped actor ever raise `actor[+0x42]`, the mesh-renderer gate? | open - the gate is pinned and nothing in the sampled corpus sets it | The zero-entry result has its mechanism: each of `FUN_8002735C`'s `jal` sites is the far arm of an `lh`/`lhu r, 0x42(s0)` then `bne` pair, and the near arm takes `FUN_80029888` or `FUN_80043390` instead. Across 720 vsyncs of four states and three game modes the gate is read 5089 times and reads **zero** every time, so both table-driven renderers are unreachable in everything sampled so far. What would close it: a write watch on `+0x42` across a longer playthrough, or a static census of the writers of that displacement - either says whether retail ever takes the far arm at all. |
| Is `FUN_801D0748` entered by an ordinary, non-dome battle at runtime? | open - its arms are general battle features, and no capture has caught it in one | The routine is documented as the Muscle Dome match state machine, and its arms are not dome-specific: the Attack confirm at `0x801D15C8` after `sb 3, +0x1DE`, the arts-input entry at phase `0x50` (`0x801D1734`), the Run confirm `0x32`, and the auto-command write-back at `0x801D22BC`. All five differently-prefixed dumps of it are the same 2781 instructions of PROT 0898. What is missing is the runtime half: one PCSX-Redux exec breakpoint hit during a field encounter settles whether an ordinary battle drives it. |

The last one - **what stages the dance widgets' second
texture page at VRAM `(960, 256)`** - closed on the disc rather than on a
capture: the page is boot-resident system UI out of `PROT.DAT`'s unindexed head
gap, so no PROT entry stages it and no per-entry sweep could have found it
([settled](re-settled-threads.md#rendering--camera)).

Closed here: **who reads the GTE light matrix the per-actor render dispatcher
writes inline** - five sites, every one of them a `NCCS` or `NCCT` in SCUS's
kind-8..11 world-map handlers, with no overlay image containing one and no
`MVMVA` on the disc selecting the light matrix through its `mx` field. The
question needed a census of GTE opcodes rather than an xref query, because the
matrix is consumed implicitly by the normal-colour commands
([settled](re-settled-threads.md#rendering--camera)). It corroborates rather
than changes the renderer page: the field path applies no light source.

Before it: **where a slot-B module image's highest spawn record ends**. Its
own move-VM program bounds it, under four rules the page states - round the end
up to 4 because the records are word-aligned, let `HALT` outrank an armed idle
loop, chain `[header][program]` rather than assume one record per pointer, and
fall back to the last maximal `0x09 0x0FFF` WAIT the walk stepped over. The
"within 4 bytes for about three quarters" figure that read as evidence of no
static rule was the missing alignment step. Every image that has a highest
record now bounds it, and the residue has a direction: no miss ever
*terminates* above a measured end, so the rule never claims bytes the band does
not bound ([settled](re-settled-threads.md#battle--arts--level-up),
[`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#bounding-the-highest-record)).

Recently closed in this area: **the eleven player-Seru tick bodies**, all of
which are now ported off their own disassembly rather than off the per-entry
stager verdicts that used to stand in for them
([settled](re-settled-threads.md#battle--arts--level-up)); and the two
ready-work rows that asked the port to catch up with retail. The live loop now
stages the Seru-magic side-effect debuffs and installs the random-encounter
boost profile, having first derived the scripted-fight gate the stager reads;
and the dome seeds `0x8007BAC0` from story flags `0x536` / `0x537` / `0x538` at
entry, so a seeded course crosses out the Item chip and the top seed the
Ra-Seru chip. Before them, **what makes a Ra-Seru chip render**. The
question had a false premise - the native window drew no dome command cluster
at all - and the three gates are now in order: `ctx[+0x25F+member]`, then
`+0x16E & 0x1000`, then `0x8007BAC0 & 0x200`, of which only the third selects a
mark. The three mark emitters `FUN_801DBC30` / `FUN_801DBD04` / `FUN_801DBEC4`
are pinned cell by cell
([settled](re-settled-threads.md#battle--arts--level-up)). Before it, the
battle-**intro** enemy-name banner - the
question had a false premise, no placement record raises it, the composer
`FUN_801D9D3C` places its labels with immediates
([settled](re-settled-threads.md#the-battle-intro-enemy-name-banner),
[falsified reading](re-do-not-re-walk.md#the-battle-intro-banner-is-raised-from-a-top-seated-0x0303-placement-record)).
Before it, the `+0x0E` kind-pair mapping and the
element-badge palette selector both fell out of the widget-class table, and the
status-element badge sheet `0x18..=0x20` is pinned cell by cell. Before them,
the ground grid's depth-cue far colour and the battle-intro tile shatter's
side-face shade page closed by capture. All in
[`re-settled-threads.md`](re-settled-threads.md#battle--arts--level-up).


### Which frames gate a tick body's arms

*Status:* resolved - all fourteen trampoline arms are measured; kept here because the shape of the answer is what the next such question needs

Every arm of the band's tick bodies is decoded and ported, and the per-arm
**frame gating** is the leg that a disassembly cannot give: the dwell is a
module-resident countdown (PROT 0955's `0x801F9D28`) whose seed is written by
whichever arm armed it. The drain is **per arm** - a multiplier of the
scratchpad frame byte at `0x1F800393`, taken as a product with `0x1F80037D`,
twice it, or once it - so the product shape holds for one family, not for the
band.

Driving casts from pre-cast save states measures the player-Seru half - PROT
0903 / 0904 / 0905 / 0907 / 0908 / 0910 / 0911, plus 0959 - and the result is
**not** one constant per arm: PROT 0907 holds 8 of its 16 arms identical across
two fights and moves the other 7. `ctx[+0x6D8]` seeds 120 and drains one per
tick on the player half, and holds a constant 20 on the capture half. The
capture half is measured for twelve of the fourteen trampoline-reached arms of
PROT 0940..0962, one fight each, with PROT 0943's `0xAB` reproduced twice; an
exec breakpoint on the single `jal 0x801F2160` site inside PROT 0898 (at
`0x801E50C8`) is one module tick, which is what makes a per-arm figure
readable. Three arms park rather than gate, and PROT 0950's arm 6 has no gate at
all - it ends on countdown expiry. Tables in
[`cast-module.md`](../subsystems/cast-module.md#frame-gating-measured).

The last two arms - PROT 0943's `0x40` and PROT 0944's `0x53` - needed a
different **caster**, not a different state. Both stage clip `0x0B`, and SCUS's
anim commit resolves a staged clip by indexing the caster's own spell-entry
offset array with it, so a ten-entry monster reads its record's name text as a
pointer. Logging the unmapped access rather than pausing on it, and forcing the
cast on a twelve-entry caster, walks both bodies through arms 0..4 in 1, 9, 40,
8 and 32 ticks. No monster record's magic slots name either id, so retail never
performs either cast ([settled](re-settled-threads.md#battle--arts--level-up)).


## Audio / BGM

| Thread | Status | What would close it |
|---|---|---|
| Which voices does the engine's score allocate, against retail's? | open - the comparand decides, and the two masks differ in content | [details ↓](#which-voices-the-score-allocates) |
| Why is the engine's mix quieter than retail's? | open (sharpened - master volume is exonerated, reverb routing is not) | Seeding the captured envelope-control word and level moved the PCM oracle's retail reference from `rms 2..87` to `579..16633`, and three scenarios then read much quieter on the engine side. Master volume is not the cause: both sides hold `(0x3FFF, 0x3FFF)` over `town01`. The first real divergence is **reverb routing** - the engine's `EON` mask is `0` against retail's `0xC081`, which routes voices 0, 7, 14 and 15 - and the engine runs fewer voices (mean 4.83, max 9) than retail (mean 9.78, max 19). What blocks the next step: the trace record carries only `active` and `pitch`, so per-voice volume and envelope level cannot be asked of it until the record is widened. |

The last thread here - a supposed second `bse.dat` record family -
resolved as a neighbouring file's tone rows left in the sector
([falsified](re-do-not-re-walk.md#bsedat-carries-a-second-record-family-with-a-resident-consumer)).

The previous thread here - op-`0x35` sub-op `0xA`, the "unhalt-pause toggle" -
resolved as the track-swap **commit** and moved to
[`re-settled-threads.md`](re-settled-threads.md#op-0x35-sub-op-0xa-is-the-track-swap-commit).

### Which voices the score allocates

*Status:* open - the comparand decides; what it decides is not yet a match

Two questions were tangled here and both have moved. The engine half is closed:
a driver that drops into a scene from cold owes three separate steps, and
skipping any one reads from outside as silence
([`audio.md`](../subsystems/audio.md#the-cold-scene-entry-sequence-and-what-each-missing-step-sounds-like)).
And the comparand's own half turned out to be an instrument defect rather than a
property of save states - a phase-keyed audibility test reported every voice a
state had ever keyed, which is what made the superset rule look unsatisfiable
([falsified](re-do-not-re-walk.md#audio--sound-driver)).

Against the envelope level the comparison is ordinary: retail holds 4 to 9
audible voices across the 19 audio-trace scenarios and the engine 4 to 8. A
per-vsync PCSX-Redux retail trace of a `town01` entry, fed back through
`--retail-jsonl`, puts the engine at 4 voices against retail's 8 on the first
compared frame - comparable, not converged.

So what is left is **which** voices the score allocates, not whether the
comparand can decide. Two things narrow it. `VoiceStartAddrMismatch` is not a
fidelity axis on this comparand at all: retail's voices start in
`0x8008..0xC968` and the engine's staged bank in `0x1000..0x1BD70`, two
independent SPU-RAM allocators, so the addresses cannot agree and are not
supposed to. `MasterVolumeMismatch` stays a hard failure. What would close it is
a per-voice comparison on one scenario - program, pitch and envelope per slot -
saying whether the engine is short of voices, playing different ones, or
allocating the same score across fewer slots.

## Title / boot / overlays

No open threads. The last one - **the browser play page holding no mode
seat** - closed by being taken: the browser runtime now owns a
`legaia_engine_core::mode::ModeSeat` and enters MAIN INIT and CARD INIT through
it, as `BootSession` does natively. The thread had assumed a residency model was
the blocker; what it actually needed was the seat, because an INIT mode lasts
one frame *inside* `ModeSeat::enter` and hands off to RUN before returning. That
is also why no post-call sampler can witness one on either host, and why the
parity witness is the edge **count** rather than a mode word
([settled](re-settled-threads.md#title--boot--overlays)).

The two threads before it both closed by capture. Nothing draws the `init.pak`
WARNING screen - the TIM is uploaded to VRAM `(704, 0)` and given descriptor 1
of the table at `0x801F369C`, and no call site ever passes that id
([settled](re-settled-threads.md#title--boot--overlays)). And a cold boot always
shows title sub-mode `0x10`: the boot image raises `_DAT_8007BB00`
unconditionally, so the `0x02` two-row menu is unreachable
([settled](re-settled-threads.md#a-cold-boot-always-shows-title-sub-mode-0x10)).
Two readings fell with the second - the sub-mode word's address and the slider
clamp, both on
[`re-do-not-re-walk.md`](re-do-not-re-walk.md#title--boot--overlays).

The previous thread here - PROT 0968 identity, the one slot-B cluster
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

## Containers / data blobs

| Thread | Status | What would close it |
|---|---|---|
| What actually breaks a rebuilt PROT 0874 container at battle load? | mostly resolved - the entry question is answered; one leg is untested | [details ↓](#what-breaks-a-rebuilt-prot-0874-container) |
| What draws VRAM `(384, 0)` 320x256 (the dome panel still)? | open (emit only; arming, staging and the upload all resolved) | [details ↓](#what-draws-the-dome-panel-still) |
| Is the `0x801E43E8` byte run one table or two? | open - its tail repeats the gear-slot indices exactly | The equip browse column reads rows `1` and up out of `0x801E43E8`, `00 01 00 04 05 06 07`. Bytes `[7..10]` of the same run are `00 01 02 04` - precisely the four gear-slot indices - so the run either continues into a second, differently-shaped table or the browse map is a prefix of one longer array. Six other sites in PROT 0899 index the same base, and reading what each of them takes out of it is what decides the shape. |
| Where does PROT 0896 link? | mostly resolved - no base fits, and a residency capture is owed | Scoring the image's own `lui`+`addiu` operands against every candidate base gives a best of 398 resolvable out of 462, where a control image scores 1147 of 1148 - so the head is not a slot-A or slot-B image at either window. It is not featureless, which an earlier reading of the same measurement said: the head is a Shift-JIS label table and the image carries a format string unique on the disc. Identity evidence and a load base are separate questions, and only the second is still open. What is left is a residency capture - a state with the entry's bytes in RAM gives the base by subtraction - and the identity work says the USA build never loads it. |

**Which image holds the dev-menu row strings** closed here: PROT 0897, the field
overlay, at file offset `0xB2C`. The argument is formed by `lui`/`addiu` pairs
inside the renderer's own body, and a live `map03` window is byte-identical to
0897's head. PROT 0981 aliases the same VA as the other slot-A occupant and the
two are never co-resident, which is the whole of why the address read as
unattributable ([settled](re-settled-threads.md#world-map--kingdom-bundles)).

### What breaks a rebuilt PROT 0874 container

*Status:* mostly resolved - the symptom is measured, five explanations of it are dead, and the remaining bracket is a leg no route has exercised

Changing section 0's decoded size produces a measured wild read at
`0x808425F8`, and that observation stands. Five readings of it do not.

The first put the cause in the container header: `meta[1]` was read as a tail
offset inside the entry, and it is the descriptors' decompressed-size sum,
which nothing reads
([falsified](re-do-not-re-walk.md#containers--placeholder-slots)). The battle
loader's pack is a different entry entirely - neither of its two walks reads
0874, because `*0x8007B878` is the `vdf` pack (PROT 0872) and `gp+0xA8C` the
`etmd` pack (PROT 0871).

The second was the address itself. **No image can materialise `0x808425F8`**:
none of the 84 holds a `lui` of `0x8084` or `0x8085`, and the delta from the
container base is `0x00800000` rather than the `0x08000000` the arithmetic
behind the earlier reading assumed. So the pointer is computed at run time.

The third was the disc the test needs. **A container with a different section-0
decoded size is not constructible**: the LZS decode is length-driven by the
descriptor, `FUN_8001ED60` sizes the section-0/1 buffers from the container
header words held at `gp+0x69C` / `gp+0x6C8`, and no shipped patcher path
changes that size. So a header-byte-exact rebuild whose decoded size differs
simply gets a truncated pack, and the "hand-build the failing disc" plan has
nothing to build.

The fourth was that the read needed a patched disc at all. Retail's own
`*(gp+0x6BC)` is the same heap block in 97 of 98 catalogued states, sane in
37 of 37 field states and reads as another allocation in 61 of 61 battle
states, because the battle load reuses the block. The model-pack registrar
inside `FUN_8001E890` reads that pointer at `0x8001EAFC`, takes its count off
`+0x00` at `0x8001EB10` with no clamp, and loops `jal 0x80026B4C` at
`0x8001EB4C` once per entry - and it is reached on all three arms of the fork
above it, so nothing in its **own frame** keeps it away from such a block.

The fifth was the conclusion drawn from that. **The registrar is never entered
over one.** Breakpointing the entry, the gate, the registrar and every
`tmd_register`, and write-watching both words, over six routes - a door warp, a
boss fight resolving to the field, a field walk into an encounter, a cold boot
into NEW GAME, a cold boot through CONTINUE into a card load, and a
field-to-battle transition - gives five entries, always over `0x8014D53C`, with
the registrar reading count 5 every time. What keeps it safe is the gate's
writers rather than its own frame: the fork is on `gp+0x6AC`, `FUN_80016230`
zeroes that word on every mode step outside 2/3, and PROT 0978's post-battle
restore writes `0` and then `2` - so a post-battle entry always decompresses
first, and the register-only arm only ever runs with the field pack intact. The
field-to-battle route enters the routine zero times, with the word already zero
from its own mode step
([settled](re-settled-threads.md#battle--arts--level-up)).

**What is left** is the one leg no route exercises: a cold boot of a *rebuilt*
disc whose section-0 decoded length and header size word disagree. The third
reading above says such a container cannot be built through any shipped patcher
path, so closing this needs the container written by hand and booted cold,
rather than a probe on retail.


### What draws the dome panel still

*Status:* open - the **emit** is the only unresolved leg

The still is staged by PROT 0978 and armed by `ctx[+0xC] = 1` at `0x800474CC`
in the generic battle-end teardown `FUN_80047430`
([settled](re-settled-threads.md#the-dome-panel-still-arming)). Three candidate
consumers are excluded: no draw site exists statically (33 sites disc-wide
materialise `0x180`, none paired with `y = 0`); the `INTERVAL` intermission is
a live `koin1` render whose ordering table samples nothing at `x = 384`; and an
1800-VSync GPU-call census across a teardown records no display-origin flip, no
`MoveImage` and no blit at `x = 384`
([falsified](re-do-not-re-walk.md#containers--placeholder-slots)).

The **upload** is now measured, and it is not the shape the doc described: **four** `LoadImage` calls of 64x256 at `x = 384 / 448 / 512 / 576`
- texture pages 6..9 - keyed on raw TOC `0x36C`. The "320x64, y-stepped"
geometry belongs to the `int.tim` family (`0x4C7` / `0x4C8`), which did not run
in the census, so it is not this still's upload.

Why it did not run is now known, and it moves the bracket. `FUN_801F6B24` holds
**two** dispatchers, not one walk: `_DAT_8007BAC0` picks between a 19-arm
field-restore table at `0x801F6AD8` and a 12-arm `int.tim` panel-still table at
`0x801F6AA8`. An ordinary battle teardown takes the field-restore table - four
uploads, one hit each - and the panel-still table records zero hits, which is
the census result rather than a missing consumer. The residency gate is
load-bearing in that measurement: an ungated breakpoint at the same VA counts
hundreds of thousands of phantom hits from whatever else occupies slot B.

**What would close it:** a GPU-FIFO capture bracketed on `ctx[+0xC]` going
`1 -> 2` during a **played** Muscle Dome match. No state in the library sits
inside a dome match - the one that looks playable is the course card with a null
battle context - and a load-transition state never pages 0978 in, so the capture
needs a match driven far enough to reach a teardown with `_DAT_8007BAC0` set.

## Measurement + tooling

| Thread | Status | What would close it |
|---|---|---|
| What does retail's equip **item-info** panel look like? | open - the capture library holds no frame that draws one | Both library states parked on the equip screen sit at slot-pick with the panel blank, so the port's version of window 24's item panel has no reference frame to be graded against - not a disagreement, an absent oracle. Closing it needs a driven capture: a pad ladder past slot pick into the candidate list, with VRAM taken on a frame where the panel is populated. Until then, the panel's geometry is the port's reading of the window record rather than a measured match. |
| Does the port's Equip screen ask the compare-category question of the wrong row? | open - the port's slot index runs one step past retail's above footwear | Retail's browse column is a two-table slot map - weapon, helmet, body, footwear, then three Goods - and its `slti v0, s0, 4` guard silences the four gear rows, so a compare category is resolved for the Goods rows only. The port's `EquipSlot::HandGuard` sits at index `3`, which pushes every later index one step past retail's row, so the screen asks the question of footwear where retail does not. Both hosts share the kernel, so this is a fidelity gap rather than drift; a driven capture of the retail screen on each of the seven rows settles what each should show. |
| Which list does the root picker's row 3 select, and where is retail's own entry to sub-screen `0x15`? | open - the port reaches the screen by a route retail may not use | The reorder screen's mechanism is pinned - a two-press latch in one cursor word - but the port enters it from the Magic screen with Square, and retail's own entry (the record screen's character picker) is not. Nor is it settled which of the three per-character lists the root picker's row 3 selects. What would close both: the window-script program that opens window `0x15`'s descriptor, read out of the widget-script scanner (`legaia_asset::widget_script`), which names the screen that raises it. |
| Which per-frame kernels do the two hosts' frame paths actually share? | open - the pairing is by name, and two families are known not to pair | The host-drift gate pairs a frame path's kernels across the native window and the browser page by **name**. Two families are known to break that: the minigame extras / UI ticks do not carry equal content across the hosts, and the battle event drain empties cue lanes the presentation tick does not name. Neither is a missing call, so no tier flags either. What would close it is a per-kernel content comparison of the two paths - the pairing evidence a name-keyed tier cannot generate. |

Closed here: **which shipped scenes carry field-VM ops `4C EA` and `4C 52`** -
one and three respectively, `map03` for the first and `geremi` / `ropeway` /
`ropeway2` for the second. The row asked for an instrument rather than an
answer, and the instrument is the general one it predicted: a disc-wide opcode
census over every scene MAN and event-script carrier, whose zeros separate "no
fixture drives this arm" from "the disc contains nothing to drive it"
([settled](re-settled-threads.md#measurement--corpus),
[`field-op-census.md`](../tooling/field-op-census.md)).

Before it, **eleven cast-band tick addresses statically
live and never entered by any ladder** - closed the way its own row predicted,
by seating a cast per PROT `0903..0966` id. Two of the eleven turned out not to
be cast bodies at all but AoE sweep stagers reached only through the AoE entry,
which is why a ladder over the ordinary cast path could never have converted
them; the rest were second arms of modules a ladder already entered once. Read
a reach cell as a claim and not a measurement: the page audit checks addresses,
not the prose beside them, so a cell can keep describing work a ladder did
([`reach-triage.md`](../tooling/reach-triage.md)).

Recently closed here: **when SCUS's `jal 0x801F7B88` runs** - a probe driving a
Slippery cast catches it firing 212 times in one cast, across phases 6..10 and
`0xFF`, always with the battle-running signal `_DAT_8007BD71 = 0xFF`
([settled](re-settled-threads.md#measurement--corpus)). With it, the
**dashboard's Port %**, which was dividing *and* multiplying by rows the ignore
list removes, and the **donor call sites** `slot_b_module`'s call-site filter
admitted: cutting every call site and record target at the image's own content
end drops the credited pointer in five of the six images, and PROT 0945's never
resolved to a record at all.


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
