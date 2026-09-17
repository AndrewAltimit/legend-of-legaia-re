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

- **The field view matrix is built three times a vsync, not once.** Three
  callers enter the builder on 133 of 134 sampled frames; the earlier figure was
  one caller's count. On a scene-entry frame two of the three read different
  live camera words, so "the frame's view matrix" is not yet one object
  ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **`FUN_80026F50` is another mode's view build, and `FUN_80025C24` does not
  zero the eye trio.** The first folds a ROM-constant base matrix and runs no
  focus `MVMVA`, and fires zero times in a field run; the second writes
  `(0, -0x100, 0x4024)` because an `addiu` re-bases the two stores after its
  opening `sw zero`
  ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **`[4C CF]` is the script camera-focus override, not a position broadcast,
  and its destinations do have writers.** Six `sh` in that one arm write
  `_DAT_8007B628` / `_DAT_8007B62A`, which the focus clamp negates into the
  camera's look-at; a word scan reports neither address because both forms are
  `lui`+`sh` ([falsified](re-do-not-re-walk.md#field--locomotion)).
- **The pad-remap quantisation was not what failed the compass law.** At orbit
  `0` the residual is a pure `+Z` world walk, which no heading quantisation can
  bend - it is the scene's own camera offset acting through pitch. Wiring
  retail's 45-degree ring was worth doing and was not the fix
  ([falsified](re-do-not-re-walk.md#field--locomotion)).
- **A dump window of `nop` signs for every zero hole on the disc.** `nop`
  encodes `0x00000000`, so a byte-identical window is not attribution evidence
  unless it contains something non-zero - one dump had been crediting an image
  it does not belong to with 20,060 bytes of fill
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **The field follow camera is a four-stage chain, and the loader writes none
  of it.** `FUN_801DBC20` fills the parameter block, a composer turns it into a
  staging descriptor and only the ease and the snap write the live globals; and
  the zone query retail runs is the field VM's, not the arrival actor's, whose
  query sits behind the dev gate
  ([settled](re-settled-threads.md#field--locomotion)).
- **A retail VAB is two chunks of its stream.** The chunk in front of `pBAV`
  carries the header part only; the VAG bodies are the next chunk, and its
  4-byte header is the "+4 skew" a decoder had recorded as a format property.
  Six entries put the SEQ chunk first, where a fixed alignment probe cannot
  recover the origin at all
  ([settled](re-settled-threads.md#audio)).
- **PROT 0981 is a slot-A image.** Its own `lui`+`addiu` pairs resolve 21 of 23
  at `0x801CE818` against 0 of 23 at the base a prologue vote had answered -
  a vote the instrument was casting with one voter
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **The model-pack registrar is never entered over a reused buffer.** Six
  driven routes give five entries, all over the same block, with count 5 every
  time. Its gate is the load-state word `gp+0x6AC`, not the game-mode halfword
  an earlier probe named
  ([falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **The dance count-in banner draws 160 x 32.** The record's `0xA0` / `0x20` is
  the texel cell, which the emitter halves at the caller's unit scale before
  centring - reading the pair as half-extents doubles the banner.

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |
| Is `juui1` dark in retail outside its `ColorIntensity` tint beats? | open - needs one retail capture of the scene | The scene holds **no** library state (177 identified), so nothing in the corpus shows what it looks like. Its P2 records carry `ColorIntensity` beats of rgb `0` at intensity `30` placed *after* the camera beats, so the mid-cutscene shots are script-tinted black by design. Whether the rest of the scene is also dark is the part a capture has to answer; the port currently renders it near-black throughout. |
| Which of a scene-entry frame's three view builds does the drawn geometry use? | open - the three read different words on the one frame measured | The field view matrix is built three times per vsync, from `0x801D0F98`, `0x801D185C` and `0x80016678`. On an ordinary frame that is harmless, because the camera globals do not move between the three. On a scene-entry frame two of them read different live camera words (`town0c`, vsync 397, N = 1), so which build the frame's geometry is projected against is a real question exactly on the frames a port's first drawn frame is compared on. Repeating the probe across several scene entries, logging the trio each build reads, closes it. |
| What does a field submode return to? | open - the port collapses the chain the return state is parked in | The field state machine's slot 7 is the submode **return** state: the enter half installs it at `0x801F140C`, parks it in `scene[+0x40]` at `0x801F148C`, and then `+0x50` is overwritten with the op-`0x49` sub-op's own slot. The port collapses enter and return into one step and keeps no `scene[+0x40]`, so nothing holds the state a submode is supposed to come back to. What would close it is a capture of `scene[+0x2E]` and `+0x40` across a submode enter and exit, which says whether the parked state is ever anything but the field itself. |

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
| What stages the dance widgets' second texture page at VRAM `(960, 256)`? | open - the page is drawn but is not in the minigame's own container | Thirty-one of the dance hall's thirty-four widget records carry texpage `0x0008`, the page PROT 1230 ships. Records 27, 28 and 29 carry `0x001F` - VRAM `(960, 256)`, CLUT rows 272 / 273 / 274 - and a parked minigame state's texture-page register shows them being drawn, so the page is resident. It is not a member of PROT 1230. Finding which container uploads it, by watching the upload band across the minigame's load, closes it. |

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
| Which battle-action state owns the only arm that can reach `FUN_801F3990`? | open (sharpened - the question is the caller, not the band) | The cue band has **one** reference disc-wide: a `jal` at `0x801E3E04`, in a battle-action state-machine arm at `0x801E3DD8` gated on `actor[+0x1DA] == actor[+0x1D9]`, which sets `ctx[7] = 0x3E`. Fifteen injected casts over 4200 frames reach the arm zero times, the guard zero times and the band zero times, while five streamed clips fire from module cues in the same runs as a liveness control - so the band is not merely unobserved, its one caller is. Reading that arm's owning state statically, then driving it, closes it. |

The last thread here - a supposed second `bse.dat` record family -
resolved as a neighbouring file's tone rows left in the sector
([falsified](re-do-not-re-walk.md#bsedat-carries-a-second-record-family-with-a-resident-consumer)).

The previous thread here - op-`0x35` sub-op `0xA`, the "unhalt-pause toggle" -
resolved as the track-swap **commit** and moved to
[`re-settled-threads.md`](re-settled-threads.md#op-0x35-sub-op-0xa-is-the-track-swap-commit).

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
| What writes the slot-B module selector `_DAT_8007B64A`? | open - the pager is pinned, the byte that steers it is not | The slot-B pager `FUN_8003EC70` maps its argument to extraction entry `a0 + 895` with no upper bound, and its site at `0x8005269C` passes `_DAT_8007B64A + 71`. So selector values `2` and `3` page in PROT `0968` and `0969` while `0` skips the load - which is what makes those two images' content reachable at all, and which of the two a fight gets. Nothing found so far writes the byte. A `gp`-relative sweep for its stores, then a write watch across a battle load, says whether it is script-set, formation-derived or seeded once. |
| Where does PROT 0896 link? | mostly resolved - no base fits, and a residency capture is owed | Scoring the image's own `lui`+`addiu` operands against every candidate base gives a best of 398 resolvable out of 462, where a control image scores 1147 of 1148 - so the head is not a slot-A or slot-B image at either window. It is not featureless, which an earlier reading of the same measurement said: the head is a Shift-JIS label table and the image carries a format string unique on the disc. Identity evidence and a load base are separate questions, and only the second is still open. What is left is a residency capture - a state with the entry's bytes in RAM gives the base by subtraction - and the identity work says the USA build never loads it. |

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
