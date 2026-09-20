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

- **`FUN_801D0748` is the round state machine every battle runs, not the
  Muscle Dome's.** It has exactly one `jal` disc-wide - `0x80047014` in the
  SCUS battle frame driver `FUN_80046A20`, with no test in front of it - and
  three non-dome battle states enter it hundreds of times over 700 vsyncs,
  every entry returning to `0x8004701C`. Reading it as a dome controller was
  reading one caller's context for the routine
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **The audio oracle's "reverb" channel was the SPU *control* register.** A
  PCSX-Redux SPU-ports blob is the hardware window `0x1F801C00..` verbatim, so
  the offset being compared was `SPUCNT`: the `0xC081` that read as "retail
  routes voices 0, 7, 14 and 15" is enable | unmute | reverb-master | CD. The
  real `EON` two words earlier reads `0x00FFFFFF` - all 24 voices - on every
  frame of the same capture, and the engine's own `0` came from an oracle
  building a bare `Spu` where the shipped host builds one through
  `set_retail_reverb`
  ([falsified](re-do-not-re-walk.md#audio--sound-driver)).
- **"The engine runs half retail's voices" compared two pieces of music.** An
  engine `town01` trace plays the id that scene's prescript selects, global
  `2016`; the retail `town01` capture it was paired against holds
  `_DAT_8007BAC8 = 2000`, because that save walked in from the world map. On
  one track over a comparable stretch the two sides land in the same place -
  and the sequencer drops no notes at all across the window
  ([falsified](re-do-not-re-walk.md#audio--sound-driver)).
- **PROT 0896 has a load base, and it is `0x801D4DF0`.** "No base fits" rested
  on a resolution ratio over the image's `lui` pairs, which is one-sided: a
  base that catches few pairs scores perfectly on all of them, and on this
  image the metric ranks the refuted slot-A base first. The call-graph
  recovery lands, and three independent signals agree with it
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **The dome panel still has a draw site; it just never materialises `384`.**
  A textured primitive addresses VRAM through a packed page index, so the
  emitter's constants are `0x106` and `0x109` rather than `0x180`. It is
  `FUN_801D00F8` in the contest hub PROT 0977 - a mode later than the upload,
  in an image that is not resident when the load runs
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **A frame kernel paired by name can be an empty body.** The host-drift
  gate's tier-11 alias row paired the native `tick_field_prop_anims` with the
  browser's `drive_npc_clips` and asserted both "advance the scene's posed
  actors"; the native body was `{}`, and a `{}` body pairs with anything. The
  window does that work inline, which is a different claim
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **The port's camera visible-tile window was seeded once at camera
  construction, not at scene entry.** A scene that scripted a wide window
  handed it to the next scene's clamp. Retail's window is per *region*: a
  `town01` walk alternates between two windows as the player crosses regions,
  and neither is the field default
  ([falsified](re-do-not-re-walk.md#field--locomotion)).
- **The wall-slide oracle was not asserting the defect.** The blocker on
  `resolve_field_slide` said its rests were pinned against captures taken on
  the non-sliding stepper. Both pinned wall-press legs are slide-*neutral* -
  the resolver hands back the held cardinal at each - so the test measured
  points where the two models agree, and retail's slide fires in ordinary
  free-roam ([settled](re-settled-threads.md#field--locomotion)).
- **`0x1000` on the BGM request word is a park sentinel, not a track.** The
  resolver compares `_DAT_8007BAC8` against `0x1000` at `0x8002454C` and
  copies the pending index onto the loaded-index barrier, so the load is
  skipped; the second streaming slot carries the same test. The states that
  read `4096` are the ending ones, not the duel ones
  ([settled](re-settled-threads.md#audio)).
- **Field-VM `4C D8`'s two `u16` immediates are envelope rates, not a
  `(kind, variant)` pair.** The opcode spawns from the morph-weight descriptor
  `0x8007068C`, whose handler reads `actor[+0x3C]` / `+0x3E` as the rise and
  fall steps of the morph weight at `+0x6E`. The port's field names are the
  allocator's, not this spawner's
  ([settled](re-settled-threads.md#field--locomotion)).

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |
| Is `juui1` dark in retail outside its `ColorIntensity` tint beats? | open - the donor door is named; the state to drive it from is not | No library state is inside the scene, and the name-hijack probe cannot answer it from a world-map door: the rewritten name loads the bundle through retail's loader, but every frame is black, and a hijack into the brightly-drawn `bylon` is equally black - the entry path makes the frame, not the scene. A disc-wide census now names the door: of 368 op-`0x3F` doors, 250 are field-to-field and 41 carry a five-letter destination across 17 source scenes, and `conc2` -> `juui1` is a direct one (index 587, MAN `0x7F12`). Only `teien` of those 17 has a catalogued state, and 900 vsyncs there cross no door - so what is owed is a `conc2` state one press from it. |
| Does a scripted camera tile window survive into the next scene? | open - the per-region half is measured, the cross-scene half is not | The window at scratchpad `0x1F8003E8..EB` is a property of where the player stands, not of the scene: a 3000-vsync pad-driven `town01` walk never holds it constant and never holds the field default, alternating between `(-8, -6, 8, 12)` and `(-10, -6, 8, 14)` four times each as the player crosses regions. What no capture has crossed is a **door**: whether the next scene's entry re-stamps the window or inherits the last region's. The port re-stamps it per field entry, so a measurement that finds retail inheriting would falsify the port rather than the page. Closing it needs the same per-vsync poll run across a scene change. |

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

No open threads. The last two closed together, and both were questions about
whether a routine no capture had caught is reachable at all.

**Does any shipped actor raise `actor[+0x42]`, the mesh-renderer gate** closed
with a `yes` that splits in two. The actor allocator `FUN_80020DE0` stamps `2`
into the halfword at `0x80020EC0` when `_DAT_8007B6D0 & 2` - but that global is
the world-map **dev** counter, booted clear by `sw zero,0x3b8(gp)` at
`0x80015F64` and raised only by the debug menu and a pad-driven ring, so that
leg is dev-reachable rather than retail-reachable. The content-driven raiser is
move-VM opcode `0x10`, which writes its own `u16` operand straight into the
field (`sh $v0,0x42($s2)` at `0x8002342C`), and shipped move programs do issue
it with non-zero operands. What the 5089-zero census establishes is narrower
than "nothing raises it": across the sampled modes no *drawn* actor had the bit
up ([settled](re-settled-threads.md#rendering--camera),
[falsified](re-do-not-re-walk.md#rendering--camera)). Whether an actor a
shipped program raises it on is then drawn through one of the three brackets is
unmeasured - the residual the census leaves, and too narrow for a row.

**Is `FUN_801D0748` entered by an ordinary, non-dome battle** closed on the
bytes before the capture landed, and then again on the capture. The routine has
exactly **one** `jal` across `SCUS_942.54`, every based overlay image and every
raw PROT entry - `0x80047014`, inside the SCUS battle frame driver
`FUN_80046A20`, unconditional - so every battle frame steps it. Three non-dome
battle states enter it 350, 313 and 255 times over 700 vsyncs each, every entry
with `ra = 0x8004701C`. It is the general battle round / command SM, and the
dome is one of its callers' contexts
([settled](re-settled-threads.md#battle--arts--level-up)).

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
| Why does the port sustain more sounding voices than retail on the same track? | open (narrowed - comparand, track and window are all controlled for) | [details ↓](#the-sounding-voice-residual) |

Two threads closed here at once, and both closed by fixing the instrument
rather than the engine. **Which voices the score allocates** is now an ordinary
comparison: the trace record carries envelope level, per-side volume, the
packed ADSR config word and the reverb send per voice from all three emitters,
and `legaia-engine audio-trace --per-voice` asks the question directly. On a
track-aligned pairing the two sides share every pitch the port plays and all
seven of its tones ([settled](re-settled-threads.md#audio)). **Why the engine's
mix is quieter** closed on that oracle's own criterion: the two channels it
named as divergences were an instrument reading the SPU *control* register for
`EON`, and a pairing of two different tracks
([falsified](re-do-not-re-walk.md#audio--sound-driver)). What survives of the
level question is not a routing or allocation difference at all - the engine
renders a track from tick 0 where a retail state is frozen somewhere inside
one, which is a window difference, and it is the residual the row above
carries.

Before them, a supposed second `bse.dat` record family resolved as a
neighbouring file's tone rows left in the sector
([falsified](re-do-not-re-walk.md#bsedat-carries-a-second-record-family-with-a-resident-consumer)),
and op-`0x35` sub-op `0xA`, the "unhalt-pause toggle", resolved as the
track-swap **commit**
([settled](re-settled-threads.md#op-0x35-sub-op-0xa-is-the-track-swap-commit)).

### The sounding-voice residual

*Status:* open - narrowed to one statistic on a pairing that controls for
everything else

Three confounds had to go first, and each was worth more than the number it
produced. The comparand: a captured voice is audible by its **envelope level**,
not its phase word. The register: `EON`, not `SPUCNT`. And the **track** - a
state's scene does not decide what it is playing, `_DAT_8007BAC8` does, so a
`town01` save walked in from the world map still holds the overworld track.

With all three controlled - a per-vsync retail capture from
`s3_rimelm_freeroam`, whose `_DAT_8007BAC8` holds the `2016` that `town01`'s
own prescript selects, against an engine trace of the same scene - the two
sides agree on reverb routing, depth and work area frame for frame, and on
slots-per-note retail reads `1.002` against the port's `1.037` to `1.061`. The
one figure that does not close: over a matched 120-frame window the port
sustains a mean of `6.11` sounding voices against retail's `4.12`, while
sharing all twelve of the port's pitches and all seven of its tones. Same
score, same voices, more of them held at once.

Two explanations are open and the artifact can separate them: the port's
release tail may run longer than the SPU's, leaving drained voices counted as
sounding, or the port may key a note on a fresh slot where retail re-uses one.
A per-voice envelope-level histogram over the matched window decides it, and
the record already carries the field ([`audio.md`](../subsystems/audio.md#comparing-per-voice)).

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

**Which image holds the dev-menu row strings** closed here: PROT 0897, the field
overlay, at file offset `0xB2C`. The argument is formed by `lui`/`addiu` pairs
inside the renderer's own body, and a live `map03` window is byte-identical to
0897's head. PROT 0981 aliases the same VA as the other slot-A occupant and the
two are never co-resident, which is the whole of why the address read as
unattributable ([settled](re-settled-threads.md#world-map--kingdom-bundles)).


**Where PROT 0896 links** closed here, and with it the reading that no base
could. The image's call graph recovers `0x801D4DF0` on ten corroborating
targets; all 218 internal `j` instructions land inside the file at that base,
ten of eleven `jal` targets land on an `addiu sp, sp, -X` prologue, and three
runs of consecutive in-image VA words resolve every word - one of them holding
the base itself. The measurement that said otherwise was a resolution ratio
over `lui` pairs, which ranks a base catching few pairs first
([falsified](re-do-not-re-walk.md#measurement-readings)). The residency capture
the row asked for is not owed either: **zero** of the image's 322 SCUS-range
calls land on a function entry of this disc's `SCUS_942.54`, where the two
slot-A controls score 1203 of 1307 and 793 of 884, so it is linked against an
executable this disc does not carry and no USA loader reaches it
([settled](re-settled-threads.md#title--boot--overlays)).

**What draws VRAM `(384, 0)`** closed on a search that had been looking for the
wrong constant. The emitter is `FUN_801D00F8` in the contest hub PROT 0977, and
it never materialises `384`: a textured primitive addresses VRAM through the
packed `tpage` halfword, where x is a page index, so the constants in the code
are `0x106` and `0x109`. It writes two `POLY_FT4` quads covering `(0,-20)` to
`(320,220)`, fading on `*(0x801D1A7C)`, and the fork `_DAT_801D1AE0` that
selects them is raised by the **arena init** on re-entry rather than by the
match teardown - which is why a census bracketed on one teardown, in a mode
where the hub image is not even resident, could not see it
([settled](re-settled-threads.md#battle--arts--level-up),
[`ringside-still.md`](../formats/ringside-still.md#what-draws-it)). The still
arm has been driven and its packets read, but only under a forced latch; no
catalogued save is parked on an arena re-entry, so the natural arm is observed
in the arming disassembly rather than in a frame.

**Whether the `0x801E43E8` byte run is one table or two** closed as neither:
it is **three** tables and a pad byte. The browse map is seven bytes and stops;
`0x801E43EF` has no word, no `lui`/`addiu` pair and no branch in any image;
`0x801E43F0` is a four-byte character equip mask with three materialisation
sites of its own, and `0x801E43F4` is eight halfwords of slot pictograms with
three more. The "bytes `[7..10]` repeat the gear-slot indices" observation was
a coincidence of the pad byte plus the mask table's first three entries
([settled](re-settled-threads.md#field--locomotion)).

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


## Measurement + tooling

| Thread | Status | What would close it |
|---|---|---|
| Which model owns the field screen-effect fade? | open - the port carries two representations and the renderers read the one nothing fills | Field-VM op `0x34` sub-0 is not a missing caller: `World::op34_sub0_color_intensity_setup` is live on both hosts. The port holds the fade twice over - an `effect_tint` float ramp the renderers read, and a `screen_tint_pushes` pool nothing reads - where retail has one, the ColorIntensity tint its effect actor drives. So one representation has to become the other: either the renderers consume the pushes, or the op spawns the tween that fills the ramp. What decides it is a retail beat measured rather than more reading of the port: a `juui1` tint or a prologue vignette, with the tint word logged per frame. |

Four rows closed here at once, three of them the Equip screen's.

**What retail's equip item-info panel looks like** closed by driving one. Every
library state parked on the screen sits at slot-pick with the panel blank, so
the oracle was absent rather than disagreeing; a pad ladder that seeks the
browse cursor to each row, confirms into sub-screen `0x14` and waits for a
staged id gives one frame per row, and all seven rows - the three Goods rows
included - open a populated list. The candidate step opens both windows through
one script, `0x801E4DC8`, so the frame carries three stacked panels: the
character's name and one stat-row set, the hovered item's name / count /
description / bonuses, and a reserved passive box at `(WX, WY + 0x38)` that
only a hovered item with a passive fills
([settled](re-settled-threads.md#field--locomotion)).

**Whether the port asks the compare-category question of the wrong row** closed
as yes, and the defect is fixed. Retail's `slti v0, s0, 4` guard at
`0x801D137C` silences the four gear rows, so only the three Goods rows resolve
a category - and they do not agree with each other, because the category
follows the **hovered item**: an HP-boost accessory draws MAX HP / MAX MP while
an accessory outside the two banded ranges draws the same ATK / UDF / LDF
triple the gear rows show. The port was passing its own `EquipSlot` index as
window 25's row, which put footwear inside the guard; both hosts share the
kernel, so one fix moved both
([settled](re-settled-threads.md#field--locomotion)).

**Which list the root picker's row 3 selects, and where retail enters
sub-screen `0x15`** closed on one address: `0x801D6C4C`, in `FUN_801D6B20`'s
row-3 arm, is the only site in PROT 0899 that writes `0x15` into `DAT_801E46A4`
- so the door is the pause menu's **Status** row, and the port's Magic-screen
Square entry was its own. The scan that looked for "window `0x15`" was in a
different id space: window `0x15` is the Equip screen's party window. Inside
the screen only steps `3` and `4` are reachable; nothing writes step `2`, so
the abilities list is decoded and has no door
([settled](re-settled-threads.md#field--locomotion)).

**Which per-frame kernels the hosts share** closed by building the instrument
the row asked for: a tier that compares, per paired kernel, the set of engine
functions each host's body reaches, host helpers followed transitively. The
first thing it found was the pairing's own failure mode - a native body that
was literally `{}`, aliased to a browser kernel that drains cue lanes and
advances every NPC clip ([falsified](re-do-not-re-walk.md#measurement-readings)).
The tier's blind spot is stated rather than closed: the join is by name, so
names that are also ordinary std methods are excluded wholesale and a divergent
engine call spelled `insert` is invisible
([`host-drift.md`](../tooling/host-drift.md#tier-12---content-do-two-paired-kernels-call-the-same-engine)).

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
