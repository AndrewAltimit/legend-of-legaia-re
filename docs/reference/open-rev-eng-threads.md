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

- **`FUN_80029888` does not zero the GTE light block.** Its three `ctc2` at
  `0x800299EC..0x800299F4` write cr21/22/23 - the far-colour trio - from
  registers, not zero. The routine that zeroes anything is `FUN_8003D190`, and
  its three `ctc2 zero` target cr5/6/7, the **translation** vector. The
  battle-intro swirl rolls about X *and* Z
  ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **The cast-voice bank was named from the wrong end.** Each slot-B module
  hardcodes its own literal cue id near its head - 62 of the 64 images, the
  other two forming it at run time - so the bank is a property of the *module*,
  not of the caster. It is seventeen files (`XA7`, `XA9..15`, `XA18..20`,
  `XA22`, `XA23`, `XA25`, `XA34`), not the `XA27`/`XA28`/`XA29` trio the
  host-drift page listed
  ([settled](re-settled-threads.md#audio)).
- **The XA cue table is `0x110` entries and its reader bounds nothing.**
  `FUN_8004FCC8` tests only `id >= 0x100`, then indexes `DAT_800788B8` at
  `id - 0x100` with no upper bound (`sltiu v0,s0,0x100` at `0x8004FCD4`, the
  `lhu` at `0x8004FD44`). The table runs to index `0x10F` with several interior
  zero runs; a port constant of `0x40` truncated it and silently dropped every
  cast cue.
- **The port's shop screen was read back as retail's.** `FUN_801DB7F4` /
  `FUN_801DBD94` are pad steppers on `DAT_801E46B4` (+1 / -1 / +10 / -10,
  clamped) whose bound is `min(gold/price, 99, 99 - held)`. The "nine-row list
  whose cursor *is* the quantity" describes the port's own screen, and while it
  stood the port capped every purchase at 9
  ([falsified](re-do-not-re-walk.md#menus--ui)).
- **`RETAIL_INVENTORY_SLOTS = 72` was a cheat page's display bound.** The bag is
  one 256-slot array at `0x80085958` reached only through an **active window**
  (`gp[+0x2D2..+0x2D6]`, written solely by `FUN_8004313C`). A real three-member
  card holds items up to index 159, so 88 of them were dropped on every lift
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **Enemy Steal has a third acceptance test nothing modelled, and its consume
  is window-bounded.** The test is the item's `+2` shop price being non-zero, so
  the quest/found-only ids are unstealable; and `FUN_80042310` scans only
  `[gp+0x2D2, gp+0x2D4)` and returns the `0x100` sentinel, so a steal outside
  the window banners a success and removes nothing
  ([settled](re-settled-threads.md#battle--arts--level-up)).
- **An actor's heading is the middle of a triple.** `FUN_8001ADA4` hands
  `actor+0x24` whole to `FUN_80026988` (`addiu a0,s0,0x24` / `jal 0x80026988` at
  `0x8001AF04`), so pitch at `+0x24` and roll at `+0x28` ride beside the yaw a
  reader taking the halfword alone sees
  ([settled](re-settled-threads.md#field--locomotion)).
- **`_DAT_8007B854` is the ambient-particle master gate, not an input lock.**
  Field-VM op `0x4C` outer nibble 3 raises it at `0x801E0F38` and clears it at
  `0x801E0F44`, both stores in a `j` delay slot off the 16-entry table at
  `0x801CEEB8`. Six references exist disc-wide and none is pad state
  ([settled](re-settled-threads.md#field--locomotion)).
- **`FUN_801F6B24` is two dispatchers, not one walk.** `_DAT_8007BAC0` picks
  between a 19-arm field-restore table at `0x801F6AD8` and a 12-arm `int.tim`
  panel-still table at `0x801F6AA8`; both run from phase 2 because SCUS's
  `FUN_80025358` owns states 0 and 1. That is why a teardown census recorded
  zero panel stills while recording four field-restore uploads.
- **The cast-arm countdown drain is per arm.** It is a per-arm multiplier of the
  scratchpad frame byte at `0x1F800393` - a product with `0x1F80037D`, twice it,
  or once it - so "the `scratch[0x37D] * scratch[0x393]` product" held for one
  family only, and the constants 4 and 8 measured on two arms were `1x` and `2x`
  a byte that read 4
  ([falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **`DAT_8007BB38` is the id just issued.** `FUN_80026B4C` publishes it through
  `gp[+0x820]` *before* the increment, so a walker bound taking it as the next
  free index reads one entry long.
- **"0978 / 0979 / 0980 are dance variants" named one of the three.** Only 0980
  is the dance overlay; 0978 is `field_back_read` and 0979 the battle-intro
  ([falsified](re-do-not-re-walk.md#title--boot--overlays)).
- **Every scene-bundle descriptor offset is inside its entry.** All 668 of them
  over 102 tables - the "offsets fall outside the file" reading was the
  over-reading entry-size expression, not the bundles
  ([falsified](re-do-not-re-walk.md#containers--placeholder-slots)).
- **The type-`0x14` FLAG descriptor of every count-4/5 bundle is the pochi fill
  file**, and the dispatcher answers `0x14` without reading the payload - so a
  descriptor that resolves to filler is the format working, not a corrupt table.
- **Two in-world minigames started the wrong track.** Their loader indices are
  PROT 1043 / 1048 / 1054, which the **piecewise** `music_01` map sends to global
  ids 2055 / 2060 / 2066; a flat `990 + slot` base gave 2053 / 2058 / 2064
  ([settled](re-settled-threads.md#audio)).
- **The dance count-in banner is a sprite record, not text.** Record 0 of the
  20-byte table at `0x801D46CC` - texel cell `0xA0` x `0x20`, texel seat
  `(0x48, 0x90)`, CBA `0x7D0A` - seated by `FUN_801D2F38`, which halves the cell
  at the caller's unit scale, so it draws 160 x 32. Its animator samples once
  per three vsyncs.
- **The field follow camera's three "constants" were one state's values.** Over
  the walkable state population the pinned `H` holds in 12 of 19, the pitch in 8
  of 19 and the yaw in 1 of 19; retail derives all three per scene and per
  player tile from the MAN section-3 camera-region record.
- **A rebuilt PROT 0874 with a different section-0 size is not constructible.**
  LZS decode is length-driven by the descriptor, so the container cannot be
  hand-built into the shape the wild-read hypothesis wanted, and no shipped
  patcher path changes that size
  ([falsified](re-do-not-re-walk.md#containers--placeholder-slots)).
- **Ghidra's own decompiler stops early, and a CSV-only extent edit orphans the
  body.** `FUN_801DD9D4` is 588 bytes and every dump of it stopped at 276, at the
  `jr v0` jump table the preceding `beq` branches past
  ([falsified](re-do-not-re-walk.md#measurement-readings)).

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
| Which frames gate each **capture-class** tick body's arms? | mostly resolved - twelve of the fourteen trampoline arms measured | [details ↓](#which-frames-gate-a-tick-bodys-arms) |

Closed here: **where a slot-B module image's highest spawn record ends**. Its
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

*Status:* mostly resolved - twelve of the fourteen trampoline arms are measured; two fault before their first tick

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

**What is still owed** is two arms: PROT 0943 arm `0x40` and PROT 0944 arm
`0x53`. Both trip an 8-bit read at the same garbage address before their first
tick in every post-turn state the corpus holds, so measuring them needs a state
where the enemy owns the turn and picks that action - a pre-turn or boss-turn
state, or a pad ladder from one that casts the spell.


## Audio / BGM

No open threads. The last one - a supposed second `bse.dat` record family -
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
| What actually breaks a rebuilt PROT 0874 container at battle load? | open (narrowed to one entry question about one block) | [details ↓](#what-breaks-a-rebuilt-prot-0874-container) |
| What draws VRAM `(384, 0)` 320x256 (the dome panel still)? | open (emit only; arming, staging and the upload all resolved) | [details ↓](#what-draws-the-dome-panel-still) |

### What breaks a rebuilt PROT 0874 container

*Status:* open (narrowed) - the symptom is measured, four explanations of it are dead, and one entry question is left

Changing section 0's decoded size produces a measured wild read at
`0x808425F8`, and that observation stands. Four readings of it do not.

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
37 of 37 field states and **garbage in 61 of 61 battle states**, because the
battle load reuses the block. The model-pack registrar inside `FUN_8001E890`
reads that pointer at `0x8001EAFC`, takes its count off `+0x00` at `0x8001EB10`
with no clamp, and loops `jal 0x80026B4C` at `0x8001EB4C` once per entry - and
it sits on the **un-gated** side of the `bne v1,2` at `0x8001EA54`, so nothing
in its own frame keeps it away from a garbage block.

**What is left** is one entry question: is `0x8001EAFC` ever entered while
`*(gp+0x6BC)` holds that battle-load garbage? An exec breakpoint on it and on
`0x8001EB4C`, logging `$a0`, the count and the `gp+0x6BC` word per hit, answers
it. A random encounter and a door warp over 900 vsyncs record zero entries; the
routes not yet covered are a cold boot into New Game and the first scripted
fight, a memory-card load, and a scene change that re-streams the party pack. If
it is never entered over garbage, the wild read has a different producer and the
bracket moves to the two pack walks at `0x8005255C` and `0x800525A0`.


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

No open threads. The last one - **eleven cast-band tick addresses statically
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
