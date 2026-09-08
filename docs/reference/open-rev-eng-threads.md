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

- **`FUN_801DD35C` is the title tick, not a menu dispatcher**, and
  `_DAT_8007BAB4` is its pre-attract hold rather than an "active submenu
  index" - the word is read at `0x801DDA9C`, tested `bgtz` at `0x801DDAB4` and
  drained by `frame_scalar << 3`. With it fell three more title readings: the
  sub-mode word is `0x801F0204` and `0x801DD920` is only the instruction that
  writes it; a cold boot never shows sub-mode `0x02`; and the slider
  `state[-0xEB4]` has no `[0, 0x2C]` range - both arms *converge on* `0x2C`
  and the graph seeds it `0x100` and `-0x16`
  ([settled](re-settled-threads.md#a-cold-boot-always-shows-title-sub-mode-0x10),
  [falsified](re-do-not-re-walk.md#title--boot--overlays)).
- **The summon draw does not run 35-64 times a frame.** Re-measured on the
  same catalogued state, `FUN_80048A08` is called once per **live actor** per
  rendered frame - at most `2` in the solo fight the original figure came
  from. Any budget argument built on the larger number is off by more than an
  order of magnitude
  ([falsified](re-do-not-re-walk.md#the-summon-draw-runs-35-64-times-a-frame)).
- **The world-map overlay's per-prim handler table is based at `0x801F8968`,
  not at its first non-zero word.** `FUN_80043390` materialises the base with
  `lui s4,0x8020` / `addiu s4,s4,-0x7698` and indexes it with the same
  `(flags >> 1) * 4` it uses on the SCUS table `0x8007657C` - which has the
  identical shape, words `0..7` zero. Re-basing by the zero prefix shifts
  every prim kind by eight
  ([falsified](re-do-not-re-walk.md#measurement-readings)).
- **Two dumps were reading the wrong image.** `0x801D2784` is PROT 0976
  (Baka Fighter), not 0979 - the two are byte-identical from file `0x3C68`, so
  only the operands separate them. And every `overlay_dance_*` print above
  `0x801D6818` is PROT 0972's fishing overlay: `0x801D73B8` is that image's
  file `0x8BA0`, byte for byte
  ([`overlay-va-aliases.md`](overlay-va-aliases.md#overlay_dance_-above-0x801d6818-is-the-fishing-overlay)).
- **Not every alias is a phantom.** `0x801DDA90` is a real entry in *both*
  slot-A images - the field overlay's screen-frame corner writer (slot 0 of the
  `0x801CEC40` table) and the title tick's `AttractDelay` arm - and the
  "two slices of one loop, neither an entry" reading was right about
  `0x801DDB44` only. Same shape at `0x801D0ED8`: an arena-init entry and a
  battle `jal` site.
- **The 0898 entry tables do not resolve the slot-B byte residue.** They map
  the band's *reachability* completely, which is why they looked like the
  instrument for the attribution question too; measured, they close 6 extents /
  48 bytes and name the wrong module for 10 of 15. Byte ambiguity needs a
  byte-denominated answer.
- **Four field-overlay routines were named for the wrong effect.**
  `FUN_801DD784` is the scene shutter blackout, not a cinematic letterbox;
  `0x801F27EC` oscillates one rung of the floor-height ladder at `0x1F80035C`,
  not a fade; `FUN_801CFF3C` is `FUN_801DE754` printed `0xE818` low, so the bar
  template still has exactly one spawner; and `0x801D44CC` flips the dance
  step-marker's mesh rather than facing a dancer
  ([falsified](re-do-not-re-walk.md#field--locomotion)).
- **A `jal` sweep scoped to one image answered the wrong question.** PROT 0898
  really does not call `FUN_8002C69C`; SCUS `FUN_80031D00` drives it off the
  retained widget list every frame a battle is up, so the post-battle report
  chrome is the ordinary nine-slice after all
  ([settled](re-settled-threads.md#rendering--camera)).
- **`FUN_80019D50` uploads a palette, it does not emit primitives.** One
  `LoadImage` at `0x8001A030` per call: it is the CLUT-cell HSV cycler behind
  jou's pulsating flesh, not a BGR555 cell-grid emitter. Nearby,
  `FUN_801D362C` has exactly one reference on the disc - the move VM's own
  op-`0x2F` arm - so the world-map controller does not call it directly.
- **Three menu / script gates were read too broadly.** Op `0x36`'s
  request/acknowledge gate differs per sub-arm (sub `3` is ungated and yields
  the frame); `_DAT_8007B868` *opens* the bit-15-clear arm while closing the
  bit-15-set one; and the menu entry-context byte is keyed on the record kind,
  which puts the save entry on `0x19` and leaves `0x02` to the dev character
  editor ([falsified](re-do-not-re-walk.md#menus--ui)).
- **The inline `0x1F` dialogue segment has no geometry header to find.** The
  byte is a MES line-start marker; the box's rect and row capacity belong to
  the pager `FUN_801D84D0`, and consecutive lines pack into one window.
- **The Spirit halving flag is a persistent record bit, not a battle bit.**
  It is accessory passive `0x2B` at character record `+0xF8` bit `0x800` -
  which is why it survives a save - and `ctx[+0xD]`'s second bit stamps a
  translation, not a `0x400` roll.
- **The slot-B band has no single damage shape.** The seat-0 hardcode holds for
  PROT 0958 / 0959 / 0960; 0927 and 0966 are multi-seat **stagers** that
  subtract nothing themselves, which is what made 0927 read as unable to kill.
- **The dome `INTERVAL` screen is a live `koin1` render**, so it is not the
  consumer of the `(384, 0)` still - that draw site is still open. Separately, a
  dome direction swing does not take damage from the `FUN_801DD0AC` chain: that
  path carries a move-power index and the dome's four commands map to row `0`.
- **Two measurement reflexes were wrong.** A `disc-coverage --check` floor
  regression is attribution lag - a new unattributed dump raises the
  denominator first - not lost coverage; and a `// PORT:` tag's live/inert
  bucket is a property of the **tagged item**, not of a neighbouring symbol
  that happens to be called.

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes, a shrunken residual set still owed | [details ↓](#region-story-flag-gate-families) |
| teien hedge-base ground fill (kind-2 tile-trigger cells) | open - blocked on one `teien` field-run mednafen state | [details ↓](#teien-hedge-base-ground-fill) |

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
channel**.

**The corpus cannot answer this yet, and the reason is now precise rather
than assumed.** The state index (`scripts/mednafen/state-index.py`) covers 55
scenes across both emulators and all three state populations - the live
emulator slots, the repo's probe captures, and the curated `saves/library/` -
and `teien` is one of them, but neither of its two states carries a `teien`
field frame. One is `battle-init`; the other is `field-init`, and reading its
display list settles what that means: the live ordering table holds **44
packets**, three of which are stacked full-screen untextured `POLY_F4` quads
spanning `(0,-4)..(320,228)`. That is a scene-transition fade, not a rendered
garden. The several hundred packets also present in the pool are stale bytes
from the previous frame that no ordering table links - which is exactly why
the read walks the OT rather than scanning the pool.

**What would close it:** one save state in `teien` at game mode `0x03`
(field-run), captured in **mednafen**, not PCSX-Redux. The emulator choice is
load-bearing: the question is per-cell, so answering it means joining the
frame's ground primitives against the live object grid at `*(_DAT_1F8003EC)`,
and that pointer lives in the scratchpad. Mednafen states carry
`ScratchRAM.data8`; a PCSX-Redux `.sstate` carries main RAM only. With such a
state the read is `mednafen-state display-list <state> --list` plus the grid
slice; see
[`mednafen-automation.md`](../tooling/mednafen-automation.md).

**`edteien` was tried as a proxy and does not work - do not re-try it.** The
emitter is scene-independent shared code, so a ground primitive over an
`0x0800`-only cell in any scene would be a finding about the mechanism. The
epilogue garden `edteien` has three field-run *mednafen* states drawing
teien's own texture families, and its live object grid does hold 400 cells
with `0x1000` and 45 with `0x0800` and no `0x1000`. Two things stop it being
an answer, and the second disqualifies the scene rather than the attempt. The
join cannot be made by **count**: retail's ground pass is not 1:1 with cells,
so the two hypotheses (400 vs 445 packets) predict values no texture family in
the frame is near - the largest ground-plausible family carries 651
`POLY_FT4`, its whole atlas 917, and a geometric join needs a camera transform
that is not pinned. And `edteien`'s `0x0800`-only cells are **not hedge
bases**: 36 of the 45 form a solid 6x6 block inside the walkable region, 10 run
along one row, and 3 sit outside the walkable area entirely. A solid block is a
raised platform - which is what the bit means, `0x0800` being
`CELL_ELEVATION_OVERRIDE`. Teien's case is hedge *rows* whose authored cutout
texels are what make a missing quad visible. The bit pattern matches; the
feature does not, and a raised platform would legitimately carry its own mesh,
so even a clean result there would not transfer.

Until this is measured in `teien` itself the engine must not grow a
speculative fill.

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
| What makes a Ra-Seru chip render? | open (both hosts draw nothing) | The chip surfaces are disabled on the native window and the browser play page alike, and the reason is upstream of the draw: nothing in either host produces a **magic command class** that casts into the capture pool, so the surface has no live input. Closes by driving one player Seru-magic cast end to end on either host and re-reading the pool - a draw fix without that input is untestable. |

Recently closed in this area: the battle-**intro** enemy-name banner - the
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

No open threads.

The two that were here both closed by capture. Nothing draws the `init.pak`
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
| What draws VRAM `(384, 0)` 320x256 (the dome panel still)? | open (emit only - arming and staging both resolved) | The still is staged by PROT 0978 and armed by `ctx[+0xC] = 1` at `0x800474CC` in the generic battle-end teardown `FUN_80047430` ([settled](re-settled-threads.md#the-dome-panel-still-arming)); what remains is the **emit**. Two candidate consumers are now excluded: no draw site exists statically (33 sites disc-wide materialise `0x180`, none paired with `y = 0`), and the `INTERVAL` intermission is a live `koin1` render whose ordering table samples nothing at `x = 384` ([falsified](re-do-not-re-walk.md#containers--placeholder-slots)). Needs a GPU-FIFO capture at battle teardown (`ctx[+0xC]` `1 -> 2`). |
| Which image holds the `0x801DBC30` head and the `0x801EA7A8` loop bytes? | open (narrow) | These are the only two entries in PROT 0898's dump corpus where the body at the address is a *different*, already-ported routine - so either the print is aliased or 0898 shares those bytes with a neighbour. Closes by disassembling each mapped slot-A image at both VAs and matching the frame, the same way the slot-B extents were recovered. |

## Measurement + tooling

| Thread | Status | What would close it |
|---|---|---|
| Which resident image does SCUS `jal 0x801F7B88` (at `0x800481A0`) mean? | mostly resolved | It is PROT 0920 (`cast_slippery`) at file `+0x11B0`. The gate names the image: the call is behind `_DAT_8007BDC0 != 0` (`gp+0xAA8`), 0920's own effect-drain counter, and a sweep of every form that reaches that word finds no other writer. The unconfirmed leg is *when* - the arm can only be entered by a battle ending mid-Slippery cast, and nothing in the save-state corpus has the gate up, so a residency capture pairing the call with the loader-B tracker `0x8007BC4C` is still owed. See [`cast-module.md`](../subsystems/cast-module.md#scus-calls-into-slot-b-at-one-fixed-va---and-only-prot-0920-arms-it). |
| Why does `cast_earthquake` (PROT 0935) floor at 69.7%? | open (narrowed to a shape) | The uncovered `0x801F8028..0x801F89D8` is a 2480-byte **data tail** that neither the `no_exit` nor the `data_segment` shape rule recognises, so the byte-accounting pass leaves it unclassified rather than crediting it. Fifteen other fallen rows have the same shape. Closes by naming the tail's structure - or by widening the shape rules once it is named. |
| The ten capture-class tick bodies the trampoline map names and nothing ports | open (ready work, not a question) | `0x801F726C`, `0x801F6A20`, `0x801F77E8`, `0x801F7118` and PROT 0955's six cells are each reached by a decoded trampoline arm and each has a recovered extent; what is missing is the port. Table in [`cast-module.md`](../subsystems/cast-module.md#the-ten-bodies-the-trampoline-map-names-and-nothing-ports). |

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
