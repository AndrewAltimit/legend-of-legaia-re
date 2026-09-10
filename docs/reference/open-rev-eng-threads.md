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

- **Seventeen of twenty "un-dumped code runs" were never code.** The
  bytes-derived dump worklist ranked each overlay image's uncovered runs, and
  the three that held real routines were all slot-A. The rest are the image's
  own data tail or a *neighbouring* image's bytes sitting at the same file
  offset - a build-buffer leftover every extracted image ends in. The residue
  measurement had no way to say so, which is why it read as work
  ([settled](re-settled-threads.md#measurement--corpus),
  [falsified](re-do-not-re-walk.md#measurement-readings)).
- **A slot-B module's data tail is a spawn-record band, and its regions
  interleave.** The uncovered span is `[i16 model_sel][u16 flags][move-VM
  bytecode]` records addressed by the consumer's own `lui`/`addiu`, not an
  opaque blob; 62 of 64 images carry one. Two readings fell with it - that the
  tail was un-dumped code above the frame partition, and that an image is laid
  out head-table / code / data in that order. Records sit *between* bodies in
  at least two images, so "everything past the last function is data" swallows
  five real routines
  ([`slot-b-module-layout.md`](../formats/slot-b-module-layout.md)).
- **`FUN_801DA390` eases a camera height, not a yaw.** `0x801DA3B4` reads
  `ctrl[+0x4A]`, `0x801DA3B8` reads the actor's `+0x16`, and the routine
  subtracts them - and `+0x16` is the Y of the `(+0x14, +0x16, +0x18)` position
  triple, not an angle. The same `+0x16` had three incompatible readings across
  the docs (heading, facing, footing); nothing on the disc masks it as an angle
  ([falsified](re-do-not-re-walk.md#field--locomotion)).
- **A `sh` in a jump-table arm's delay slot is still that arm's store.** Three
  world-map horizon-gate subs were credited to the wrong instruction because
  the write sits in the delay slot of the `j` that leaves the arm - and one
  op-`0x4C` arm was read as writing one word when it writes two.
- **Retail's title screen does not run under mode `0x10`.** The front end is a
  six-store chain and the title's own mode is the **card** mode `0x17`; `0x10`
  is one frame of logo INIT. The hand-off between INIT handlers is each
  handler's own store, not a table's `next` field
  ([settled](re-settled-threads.md#title--boot--overlays)).
- **A dome round is an ordinary battle; the dome *hub* is the OTHER mode.** The
  `0x14` reading was true of the hub (PROT 0977, sub-id 5) and false of a
  round. Separately, magic is **not** forbidden on the Master course: the only
  writers of the `0x200` restriction bit key on the *first enemy monster id*,
  and no dome ladder reaches one
  ([falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **A cast costs MP, not AP.** The Ra-Seru chip's cast path writes the action
  queue and the phase and spends no AP, so every "the pennant pays for a cast"
  argument was about the wrong currency.
- **`FUN_80058490` is `MoveImage`, not a sound-driver lane.** It moves a VRAM
  rect to `(0xE0, 0x1DC)` - CLUT row `y = 476` - which makes the table behind
  it a CLUT map rather than a cue list. An engine defect follows: the battle
  SFX cue kind is being read out of those CLUT bytes.
- **`FUN_8003E8A8` returns a sector *count*.** `0x8003E90C` is
  `subu s0,v0,s2` over `TOC[idx+3]` and `TOC[idx+2]`, and `0x8003E948` returns
  it; the LBA is the side effect at `gp+0x8f0`. Two loader rows and
  `FUN_8005E4D4`'s argument order were reversed against it. And the libcd
  directory cache is at `0x801CB408` - `0x801C4BEC` was the offset half of a
  `lui`/`sw` pair pasted into the high half of an address.
- **A live port wore another routine's address.** `engine-core::dialog`
  carried `PORT: FUN_8001FD44` and implements nothing of it; the address is the
  name-based scene-change packet, which the field VM's op-`0x3F` arm ports.
  Nothing gates this class - a `// PORT:` tag naming the wrong routine passes
  every check ([`port-provenance.md`](../tooling/port-provenance.md)).
- **Twelve feature views were measuring one blob.** A BFS from a feature root
  spills through the title tick into the save UI, the effect spawner and the
  move VM, so `title-screen`'s 754 anchors were a strict subset of
  `muscle-dome`'s 781. Localized, the same feature measures 72.
- **`Insn::extended` is not the field VM's op-`0x43` sub-op.** It is a
  cross-context target marker (`0x80`); the sub-op is `InsnInfo::ActorCtrl`,
  and the real sites number 311 across **ten** ending scenes, not the eight the
  docs listed.

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |
| What arms `_DAT_8007B8B8`, the gate on the field overlay's one ambient template? | open | The MAIN INIT `FUN_801D6704` spawns the descriptor at `0x801F271C` exactly once, at `0x801D6FD8`, and only while `_DAT_8007B8B8 == 0`; the same word is the mode-entry prologue's field-state latch. Closes by naming every writer in load order - which decides whether the template is a boot-only spawn or a per-scene one. |
| Nothing on the disc references the descriptors at `0x801D5C08` / `0x801D5D60` | open (a negative; needs a consumer or an ignore row) | Both look like actor templates in the field overlay's own data, and a sweep of all five reference forms over 84 images finds no word, `lui` pair, `jal`, `j` or branch that reaches either. Either a list-driven `jalr` seats them - which a target sweep structurally cannot see - or they are dead authored data; a retained-list dump at scene entry separates the two. |

Recently closed here: **teien's hedge-base ground fill**, which had a false
premise. Retail has no kind-2-cell draw channel at all - a live `teien`
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

### Coplanar residual tail: same-position curved-shell stacks

*Status:* partial - the same-position curved-shell half is **answered** by a display-list read; the sliver half remains

After the cross-draw coplanar kernel's per-family lifts and repair pass
(`engine-core::coplanar_draws`; the whole model is in
[`renderer.md`](../subsystems/renderer.md#coplanar-surfaces-retails-ordering-model-the-ports-depth-policy)),
the corpus sweep (`DIAG_ALL=1` on `engine-core/tests/coplanar_residual_disc.rs`)
still reports a small tail, dominated by two shapes.

First, **same-position stacks of curved shells** - two different env TMDs
placed at one translation whose curved surfaces coincide (jouine/jouind's
flesh-cave walls, chitei2's res41/res45 slope). A per-draw *translation*
cannot separate two coincident curved surfaces everywhere (any direction is
tangent to some part of the shell), so the offset API is structurally the
wrong tool.

**Retail does not draw both copies.** Reading the libgpu ordering table out of
field-run save states inside `jouine` and `jouind` (`mednafen-state
display-list --coincident`) finds **zero** screen-coincident groups among
surfaces of at least 16 px² in either scene's live frame - 1218 packets walked
in `jouind`, 972 in `jouine`, every surface submitted exactly once. The scripts
swap these meshes as state/morph variants rather than stacking them, which is
what the thread suspected. The one place coincidence does appear is not a mesh
stack: a small ordering table in the `jouine` image holds four groups of
**three** copies of a single quad, all in one texture family
(`clut=0x7F86 tpage=0x001F`), forming a 2x2 patch - the multi-pass
semi-transparency idiom, one mesh drawn three times. Its members share a
material; two different env TMDs would not.

Two format properties decide whether such a report means anything, and both
produce false positives when ignored. Retail **double-buffers**: ordering
tables come in pairs holding frame N and frame N-1 with near-identical packet
counts, so merging a pair makes every surface appear stacked with itself
(`--all-ots` does this deliberately; the default walks one). And distant
geometry projects to 1-3 pixel slivers that coincide with each other constantly
without saying anything about meshes, hence the `--min-area` floor.

**What this evidence does and does not cover.** Each read is one frame, so it
is one camera position: a surface outside that view contributes no packet, and
its absence from the report is not evidence about it. What makes the result
load-bearing anyway is the scale of the negative - a stacked shell would double
*many* adjacent surfaces at once, not one, and across 1218 and 971 walked
packets no surface anywhere on either screen is submitted twice. For cave
interiors, whose walls are the dominant on-screen geometry, both shells being
off-camera in both frames is implausible. It is not impossible, and a second
field-run state per scene at a different camera would retire the caveat - **the
corpus does not contain one**: the curated library's `jouine`/`jouind`
field-run states are byte-identical backups of the two read here (a library
filename is the sha256 of its contents), not new viewpoints. `chitei2` is
**not** covered by the state corpus, so its res41/res45 slope is asserted only
by the two `jou` scenes' result, not measured directly.

Second, **sub-cluster
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
| Which frames gate each capture-class tick body's arms? | open (ready work, not a question) | Every arm of the twelve trampoline-reached bodies is decoded and ported, but the per-arm frame gating - PROT 0955's module-resident countdown at `0x801F9D28` and the `scratch[0x37D] * scratch[0x393]` product it is drawn down by - is pinned by shape, not by a measured frame count. Closes on a capture of one natural cast per body; table in [`cast-module.md`](../subsystems/cast-module.md#the-twelve-bodies-the-trampoline-map-names). |
| Where does a slot-B module image's **highest** spawn record end? | open (bounded below, unbounded above) | Every record but the topmost is bounded on both sides by the next consumer pointer. The highest has none: it carries no length, nothing computes an address past it, and walking the move-VM opcode widths to `0x08` HALT lands within 4 bytes of the true boundary for about three quarters of the band's inner records and misses the rest - so it is not a static bound. Closes by finding a length the runtime itself uses, or by measuring one image's topmost record live. See [`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#the-one-span-the-band-cannot-bound). |
| The eleven player-Seru modules (PROT 0903..0913) have no ported tick body | open (ready work) | The `0x801CF4EC` arms are tick bodies of 3396..7260 bytes - `ctx+0x279` phase machines with damage wrappers - not the data stagers the cast-module page's per-entry verdicts name (those verdicts describe each module's *stager*). None of the eleven is ported, so a player Seru-magic cast reaches `run_cast_module_code` and finds a body for only 2 of the 11 spell ids. |

Recently closed in this area: **what makes a Ra-Seru chip render**. The
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
| What draws VRAM `(384, 0)` 320x256 (the dome panel still)? | open (emit only; arming, staging and the upload all resolved) | [details ↓](#what-draws-the-dome-panel-still) |

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

What the **upload** is is now measured, and it is not the shape the doc
described: **four** `LoadImage` calls of 64x256 at `x = 384 / 448 / 512 / 576`
- texture pages 6..9 - keyed on raw TOC `0x36C`. The "320x64, y-stepped"
geometry belongs to the `int.tim` family (`0x4C7` / `0x4C8`), which did not run
in the census, so it is not this still's upload.

**What would close it:** a GPU-FIFO capture at battle teardown, bracketed on
`ctx[+0xC]` going `1 -> 2`.

## Measurement + tooling

| Thread | Status | What would close it |
|---|---|---|
| *When* does SCUS `jal 0x801F7B88` (at `0x800481A0`) run? | mostly resolved (image pinned; the frame is not) | The callee is PROT 0920 (`cast_slippery`) at file `+0x11B0`, named by its gate `_DAT_8007BDC0 != 0` (`gp+0xAA8`), 0920's own effect-drain counter, which no other writer reaches in any reference form. The arm also requires `_DAT_8007BD71 == 0xFF` - battle **running**, `0xFE` being ending - which falsifies "a battle ends mid-cast": it fires on ordinary in-battle frames while the budget is non-zero (123 hits on a victory ladder, all before the end signal). Owed: one PCSX-Redux state taken *inside* a Slippery cast; the corpus's only Slippery state is mednafen. See [`cast-module.md`](../subsystems/cast-module.md#scus-calls-into-slot-b-at-one-fixed-va---and-only-prot-0920-arms-it). |
| 31 cast-band tick addresses are statically live and never entered by any ladder | open (ready work, not a question) | The replay reach export runs 47 ladders and reports 91 live-but-never-entered addresses; `cast_module_ticks.rs` is 31 of them, all gated on the spell id through `World::cast_module_for`. They are not host-dead - one ladder seating a cast per PROT `0903..0966` id would convert the whole cluster at once. Until then the reach figure understates the band and nothing says which of the 31 would actually run. See [`reach-triage.md`](../tooling/reach-triage.md). |
| The dashboard's Port % denominator counts rows the ignore list removes | open (a tool fix, not a question) | `port-catalog.py --dashboard` divides by a worklist that still includes `port-catalog-ignore.toml` rows, so the headline percentage moves when an ignore row is added and nothing about the port changed. Closes by taking the ignore set out of the denominator - and by re-taking the committed baseline once, since the figure shifts. |

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
