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

- **One of the twenty ranked "un-dumped code" runs *was* code, so sixteen are
  not.** PROT 0949's `0x801F7630` run holds the bodies of an eight-arm leaf
  table (`0x801F69F0..0x801F6A0C` inclusive, bounded by an `sltiu 0x8`): six
  20-byte frameless leaves that store the phase byte in the `jr ra` delay slot,
  and a seventh of 12 bytes with no `jr ra` at all, falling into the shared
  epilogue at `0x801F76B4`. Frame matching cannot see any of them - a frameless
  leaf has no prologue to match - and the first re-read of the run over-counted
  the leaves by one for exactly that reason
  ([settled](re-settled-threads.md#measurement--corpus)).
- **The donor-tail instrument carried two restrictions the fact never had.** A
  build-buffer tail was searched only in images sharing the load base and only
  in *longer* images, and neither holds: the mastering buffer is indexed by
  **file offset**, so five cast-band images end in the menu overlay's code and
  the game-over image ends in the world-map renderer's; and `content_bytes` is
  a sector extent, so the donor need not be longer. The cross-base half was
  already written down and the instrument was never updated to it
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **A "code gap" column that ignored its own shape census.** The same
  measurement classifies each uncovered run (`no_exit`, `no_boundary`,
  `constant_table`, `return_tail`, `psyq_lib_stamp`) and then counted every
  byte of every run as a gap anyway, so the ranked dump worklist was mostly its
  own rejected shapes.
- **A phase-store census blind to a register-held pointer.** Counting `sb`
  stores at a literal `0x279` displacement misses every module that forms the
  context pointer once and stores through the saved register - which is most of
  them. PROT 0908 reads as zero phase stores that way and has six; re-measured
  over both forms the eleven player-Seru bodies run 3..10 stores each, and
  `0x801F69D8` is the arm for six of the eleven rather than five
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **"PROT 0910 has no damage wrapper and writes no HP" was the *tick's own
  frame* talking.** The wrapper is in its callee `FUN_801F81DC` - three `jal`
  sites in the tick, `li a0,0x12` / `li a1,7` / `jal 0x801DD0AC` at
  `0x801F8874`, the HP store at `0x801F8910`. A per-image verdict taken inside
  one function extent cannot say what the module does
  ([falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **Only two of the eleven player-Seru bodies heal, and the published heal
  formula belongs to neither.** PROT 0905 (Vera) restores
  `record[+0x729+slot] * 0x20 + 0xE0`, clamped to the HP pair and pushed as a
  *negated* popup at `0x801F7D0C`; PROT 0911 (Orb) restores
  `(magic_level << 6) + 0x1C0` over the party row. PROT 0903, 0910 and 0913
  were filed as heals and are damage (move type `0x12`), and 0909 was missing
  from the table entirely. The input is the per-magic **level** byte, not a
  power byte ([settled](re-settled-threads.md#battle--arts--level-up)).
- **Three damage-clamp shapes, not two - and one module picks per hit.** The
  third shape caps at `HP - 1` with an *unsigned* compare, so it can neither
  kill nor heal; seven sites take it and every one is a tick body or a body a tick calls. PROT 0910's
  applier chooses the cap from its own slash counter at `0x801F8DAC`, so kill
  capability there is a property of the hit, not of the module
  ([`cast-module.md`](../subsystems/cast-module.md#the-three-clamp-shapes)).
- **Actor `+0x1DC` is a bitfield at the reaction sites, not a counter.** PROT
  0903 and 0904 `ori` bits `4` and `1` into it; 0906 and 0908 store `1` and
  `5`. Reading it as a count made the port's comment describe an increment
  nothing performs.
- **PROT 0941's Steal carries no damage wrapper at all.** Its outcome is an
  inventory consume through `FUN_80042310`: against a party victim by rejection
  sampling over the 256-slot bag, and against a monster victim by
  `rand() % 100` versus the same `0x80077828 + id*2` table the player's Steal
  rolls on.
- **A body's head table is not the body.** PROT 0943's MP-pair writer was given
  as "a routine at `0x801F69D8`"; that VA is the `0xB5` body's *head table*,
  and the writer is the body at `0x801F6A04`. Head tables are not always at the
  image base either - 0943's `0x40` and 0944's `0x53` read `0x801F69F0`, 0950's
  `0x5A` reads `0x801F6A10`.
- **One band routine allocates a battle seat.** PROT 0940's `0x50` / `0xAE`
  claims `actor_table[ctx[+1] + 3]`, copies the monster-record pointer into
  `0x801C9348[seat]` and seeds `+0x16C` / `+0x1DE` / `+0x1DF` / `+0x1DD`; its
  `0xAC` arm blanks `actor_table[3]`'s `+0x1EF..+0x1F3` (the `s0` reassignment
  at `0x801F7648`), not the caster's `+0x0C`.
- **Actor `+0x8A` bit 0 is a suppression mask, not an enable.** The motion VM's
  gate is a `beq` at `0x80038194`: a **zero** byte runs the bytecode, and the
  bit gates the player-engaged / actor-busy / off-map early returns at
  `0x8003819C..F4`. Two op readings fell with it - op `0x06` is a home-relative
  one-tile wander (`rand() & 6` over signed 7-bit tile deltas from `+0x8C` /
  `+0x8D`), not a pad echo, and op `0x0C` fades the packed RGB tint at `+0x74`
  and the draw mode at `+0x78`, not a glide channel
  ([settled](re-settled-threads.md#field--locomotion)).
- **The field entry seat was the ambient emitter's.** `(0xA40, 0, 0xA40)` is
  the spawn of the one plain template at `0x801F271C`; the player is seated on
  **both** arms of `FUN_801D6704`, at `0x801D6F64` and `0x801D6F7C`, from the
  door operand. "Cold entry happens only at New Game" is false too - the same
  function's epilogue clears the word at `0x801D750C`, so every ordinary scene
  change takes the cold arm.
- **Monster record `+0x20` is a double-width texture-page flag.** Its primary
  reader is the model upload at `0x801F1D0C`, which widens the VRAM rect from
  `0x20` to `0x40`; 37 of 186 records set it. Three summon ticks borrow it as a
  "big model" resist proxy, which is what made it read as a per-monster
  instant-death immunity byte.
- **`ctx[+0x287]` is derived, and the port named the wrong byte for it.**
  Battle init `FUN_800513F0` computes it as `(DAT_8007BD60 >> 5) & 4` - bit
  `0x80` of the per-battle flags - so it is the **scripted-fight** flag, and
  `+0x288` is the counter-attack byte. All three action-SM reads gate on it, so
  two audio-duck arms and the attack-return arm were unreachable while the port
  seeded zero.
- **The dome arena's pre-test seed is `1`, not `0`.** `sw $s2` at `0x801CEB8C`
  with `$s2 = 1` loaded 43 instructions earlier across three `jal`s, so the
  unflagged word is course 0 with no bans. "Every seed carries `0x100`" holds
  for the three flagged seeds only
  ([falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **The dome tally screen's rows were wrong on both hosts.** Retail reads
  `[lane0, lane1, lane2, the HP accumulator 0x801D1AC8, lane3, the running
  tally 0x80084440]`, with a per-lane brightness of `[0, 1, 2, 0, 3, 3]` - four
  steps, not six. `FUN_801D1184` re-forms the `0x801D` base into a different
  register between the product and the store, which is what mis-attributed the
  lanes.
- **The koin4 coplanar sliver was manufactured by the port's own lift.** The
  offending strip is exactly one `DRAW_NUDGE` wide, because the applied lift
  `[0, -0.75, -0.75]` lies inside the second plane; zeroing the offset takes
  the measured overlap from 94.56 to 0. "Below the detection floor" described a
  defect in the repair pass, not a residue of the disc
  ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **Nothing on the disc can materialise `0x808425F8`.** No image holds a `lui`
  of `0x8084` or `0x8085`, and the delta from the container base is
  `0x00800000` rather than the `0x08000000` the earlier arithmetic assumed. The
  rebuilt-0874 wild read therefore has two candidate producers - a byte-offset
  pack walk at `0x8005255C` and a word-offset walk at `0x800525A0` - and
  neither is pinned.
- **The dashboard's Port % was wrong on both sides of the fraction.** Ignored
  rows sat in the numerator *and* the denominator, so five feature views read
  far below their real figure - `cd-io` 2.6 against 100, `field-vm` 66.2
  against 100 - and the headline moved whenever an ignore row was added and
  nothing about the port changed.
- **`FUN_801DD9D4` is 588 bytes and every dump of it stopped at 276.** The
  decompiler stops at the `jr v0` jump table at `0x801DDA88`, which the `beq`
  at `0x801DDA78` branches past; the body runs to the `jr ra` at `0x801DDC18`.
  The fix belongs in the shared dump header parser, because a CSV-only extent
  edit orphans the body ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **The CDNAME map's last block is open-ended, and one consumer read it
  literally.** `other7` spans entry 1226 to the end of the map, so an unclamped
  block range reserved 64 GiB on a scene load and Linux overcommit hid it until
  a test run met the memory watchdog. Reproduce this class with `ulimit -v` on
  the suspect test binary - an overcommitted reservation only aborts under a
  cap.

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |

Recently closed here: **what arms `_DAT_8007B8B8`**, the gate on the field
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
| Which frames gate each **capture-class** tick body's arms? | partial (the player-Seru half is measured) | [details ↓](#which-frames-gate-a-tick-bodys-arms) |
| Where does a slot-B module image's **highest** spawn record end? | mostly resolved (58 of 62 images bounded) | Its own move-VM program bounds it: walk the opcode widths to a terminator - `0x08` HALT, or an armed `0x19` / `0x1B` idle loop not immediately followed by one - and round the end up to 4, because the records are word-aligned. The old "within 4 bytes" reading was that missing alignment step, not the absence of a rule. Owed: the records ending on a non-terminating instruction (a long `0x09` WAIT), where only the next record's header fixes the end and the topmost has none - a residue that is **directional**, every miss stalling below the measured end and none overrunning it. See [`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#bounding-the-highest-record). |

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

*Status:* partial - the player-Seru half is measured; the capture-class half is not

Every arm of the band's tick bodies is decoded and ported, and the per-arm
**frame gating** is the leg that a disassembly cannot give: the dwell is a
module-resident countdown (PROT 0955's `0x801F9D28`, drawn down by the
`scratch[0x37D] * scratch[0x393]` product) whose seed is written by whichever
arm armed it.

Driving casts from pre-cast save states measures it for the player-Seru half -
PROT 0903 / 0904 / 0905 / 0907 / 0908 / 0910 / 0911, plus 0959 - and the result
is **not** one constant per arm: PROT 0907 holds 8 of its 16 arms identical
across two fights and moves the other 7. `ctx[+0x6D8]` seeds 120 and drains one
per tick on the player half, and holds a constant 20 on the capture half.
Tables in [`cast-module.md`](../subsystems/cast-module.md#frame-gating-measured).

**What is still owed** is the fourteen trampoline-reached arms of PROT
0940..0962. No state in the corpus sits inside one of those casts, and their
low action ids route through the Magic arm, so reaching them needs a state
where the enemy owns the turn and the wanted action is the one it picks.


## Audio / BGM

No open threads. The last one - a supposed second `bse.dat` record family -
resolved as a neighbouring file's tone rows left in the sector
([falsified](re-do-not-re-walk.md#bsedat-carries-a-second-record-family-with-a-resident-consumer)).

The previous thread here - op-`0x35` sub-op `0xA`, the "unhalt-pause toggle" -
resolved as the track-swap **commit** and moved to
[`re-settled-threads.md`](re-settled-threads.md#op-0x35-sub-op-0xa-is-the-track-swap-commit).

## Title / boot / overlays

| Thread | Status | What would close it |
|---|---|---|
| The browser play page holds no mode seat | open (blocked on an overlay-residency model) | The native host now seats the mode driver (`ModeSeat`, owned by `BootSession`); the browser runtime does not, so the two hosts disagree about who owns the front-end mode chain - the drift shape [`host-drift.md`](../tooling/host-drift.md) exists for. The blocker is shared with the overlay loader: nothing in the port loads an image at a base and calls an INIT plan's entry, so a seat on the browser side would have no image to enter. Closes by giving the port a residency model, or by disclosing the browser half permanently. |

The two threads that were here both closed by capture. Nothing draws the `init.pak`
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
| What actually breaks a rebuilt PROT 0874 container at battle load? | open (narrowed - the read's address is not materialisable) | [details ↓](#what-breaks-a-rebuilt-prot-0874-container) |
| What draws VRAM `(384, 0)` 320x256 (the dome panel still)? | open (emit only; arming, staging and the upload all resolved) | [details ↓](#what-draws-the-dome-panel-still) |

### What breaks a rebuilt PROT 0874 container

*Status:* open (narrowed) - the symptom is measured and two explanations of it are dead

Changing section 0's decoded size produces a measured wild read at
`0x808425F8`, and that observation stands. Two readings of it do not.

The first put the cause in the container header: `meta[1]` was read as a tail
offset inside the entry, and it is the descriptors' decompressed-size sum,
which nothing reads
([falsified](re-do-not-re-walk.md#containers--placeholder-slots)). The battle
loader's pack is a different entry entirely.

The second was the address itself. **No image can materialise `0x808425F8`**:
none of the 84 holds a `lui` of `0x8084` or `0x8085`, and the delta from the
container base is `0x00800000` rather than the `0x08000000` the arithmetic
behind the earlier reading assumed. So the pointer is computed at run time, and
two pack walks are the candidates - the byte-offset walk at `0x8005255C` over
`*0x8007B878`, and the word-offset walk at `0x800525A0` over the arena.

**What would close it:** a watchpoint on the read taken over a rebuilt
container from a **cold boot**. A mid-game state replays the RAM of the disc
that booted it, so the patched bytes under test are masked until the game
re-loads them. The byte-exactness rule the modding path follows stays
conservative rather than explained until then.


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

**What would close it:** a GPU-FIFO capture at battle teardown, bracketed on
`ctx[+0xC]` going `1 -> 2`.

## Measurement + tooling

| Thread | Status | What would close it |
|---|---|---|
| Eleven cast-band tick addresses are statically live and never entered by any ladder | open (ready work, not a question) | The replay reach export runs 52 ladders; the cast band contributes 53 tagged addresses, 52 of them live, 41 entered and 11 never - and all eleven are `cast_module_ticks.rs` bodies gated on the spell id through `World::cast_module_for` (`801f69f8`, `6a0c`, `6a14`, `6a28`, `7158`, `767c`, `77e8`, `7fa4`, `85a8`, `86a4`, `8d64`). They are not host-dead - one ladder seating a cast per PROT `0903..0966` id would convert the cluster at once. See [`reach-triage.md`](../tooling/reach-triage.md). |

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
