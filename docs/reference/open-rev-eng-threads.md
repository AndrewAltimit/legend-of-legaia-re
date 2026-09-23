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

- **Op `0x3E` with `op0 < 100` starts a fight, not a conversation.** The arm
  reads `op0` only to fork off the door-warp, so `3E 00 02` installs a MAN
  formation row exactly as `3E FF 02` does; the engine had opened dialogue at
  all ten non-`0xFF` sites, and eleven doc sentences said the op "arms the
  interaction script"
  ([falsified](re-do-not-re-walk.md#field--locomotion)).
- **The field fog samples VRAM `(448, 0)`, its cap is a MAN header bit, and the
  port's excess density was its random numbers.** Texture page `0x27` has the
  page-y bit clear; `0x48` is every overworld's cap rather than a debug value;
  and the engine handed the spawner a raw LCG state where retail's BIOS `rand`
  returns the high half ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **`kor5`'s `0x436` needed no poke.** P2[5] writes it 3,336 vsyncs after
  `0x464` clears; the 17,500-vsync estimate was the probe re-dispatching P2[4]
  over the running chain ([settled](re-settled-threads.md#field--locomotion)).
- **The title menu has two rows**, so no title route reaches Options, and the
  "boot Options draws twice" host-drift reading was wrong on both counts
  ([falsified](re-do-not-re-walk.md#title--boot--overlays)).
- **`FUN_801F138C`'s "capture payout" is the dead-slot Item refund** of
  `FUN_801DABA4`, printed at a phantom VA
  ([falsified](re-do-not-re-walk.md#battle--arts--level-up)).

---

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure settled; play order capture-confirmed for most spokes; the residual is a card-block question with the instrument ready | [details ↓](#region-story-flag-gate-families) |
| Why does the port's field attached light leave `dolk` and `cave01` undarkened? | open - measured, a fix owed | `FUN_800195A8` adds a light's extents after the view transform, in the `_DAT_8007BF10` space six times world scale, so retail's rim radius is `H * ext / vz` (`dolk`: 201 px round `(153, 93)` at `H = 768`, `vz = 15635`). `World::field_light_draws` scales the extents at a world-unit depth - six times too large - and the rim and its darkening fills land off-screen ([`script-vm.md`](../subsystems/script-vm.md#the-extents-are-view-space-units-retail-capture)). The extents divided by the base-matrix scale on both hosts, checked against the `dolk` capture, closes it. |
| What clears the two halt bits an inn acquire sets? | open - measured, writer unknown | In `retock_innkeeper_talk_open` the second talk's acquire (`0x801E2148..0x801E21DC`, `s7 = 5`) raises `0x400` on the player and the innkeeper, and both bits are clear again 18 vsyncs later with the conversation still open ([`script-vm.md`](../subsystems/script-vm.md)). A write watch on the two actors' `+0x10` across that window, naming the store, closes it. |

Four rows closed here. **The region battle-setup half of `FUN_801D9E1C`** is
ported on both play hosts: `_DAT_8007BD60 = region[+8] & 0x1F` is the battle
backdrop variant `FUN_800513F0` loads, bits 7 / 6 re-open the Door of Light /
Wind item rows, bit 5 keeps backdrop object 1, and `region[+5]`, `[+9..+11]`
are the world-map return point, whose consumer the port does not model
([settled](re-settled-threads.md#field--locomotion)). **Op `0x43`'s arcs and
`34 10`'s attached sprites** are drawn on both hosts through
`World::script_actors`, and the arc's acquire now waits and retries instead of
skipping the op; the attached light's size is the new row above.
**`kor5`'s `0x619`** was never a chain beat: `P1[2]`'s `SET 0x619` sits in its
spawn section, which `FUN_8003A1E4` runs at every MAN-loading entry and at no
same-scene reload, and the engine had re-run that section on every talk
([falsified](re-do-not-re-walk.md#field--locomotion)). **The frame-step floor**
is the scene's: field init writes `2` and `opdeene`'s prescript raises it to
`3` through move-VM ext sub-op `0x2F` ([settled](re-settled-threads.md#audio)).

**Where a halt-acquire conversation ends** closed by capture: the inn stay ends
when its last page closes, with the innkeeper's cursor parked on the loop-back,
and the next talk's acquire succeeds. The reading that the acquire fails once
the window closes, and that `*(_DAT_801C6EA4) + 8` is a modal-window flag, are
both falsified ([falsified](re-do-not-re-walk.md#field--locomotion)); what
clears the halt bits afterwards is the new row above.

**Does P2[5] write `kor5`'s `0x436` organically** closed as yes: with only the
two trigger-tile pokes and the Gaza fight's enemy HP held at `1` (a loss is a
game over), P2[5] reaches `+0xD0D` and sets `0x436` through `FUN_8003CE08`
(`ra 0x801E3598`) 3,336 vsyncs after `0x464` clears, and P2[8] then sets
`0x6C4` on its first `(32, 86)` crossing. The 17,500-vsync estimate was the
probe's own: its `!464` leg counted pokes rather than reading the flag, so it
re-dispatched P2[4] over the running P2[5]
([settled](re-settled-threads.md#field--locomotion),
[`field-locomotion.md`](../subsystems/field-locomotion.md)).

**Is `juui1` dark in retail outside its tint beats** closed as no, on a
synthetically gated walk: the black run is the door fade, and at rest the scene
holds a dim purple vortex with the party visible
([settled](re-settled-threads.md#field--locomotion)).

**Does retail's cold entry into `conc` run the `66 DE` clear** closed as yes -
twice - and the flag turned out not to be progress at all. A single-flag write
watch armed from the memory-card load screen sees the scene load clear `0x6DE`
from `P1[1]`'s spawn prologue (`+0x10`) and from the `P1[0]` entry script
(`+0x18`), not from `P0[34]` / `P0[36]`, which are walk-on trigger scripts a
cold entry never reaches. From the first field frame `P1[0]`'s per-frame body
re-runs a `CD F8 0A 0E 33 48` player bounding-box test at `+0x100` and takes the
outside arm's `56 DE` SET at `+0x10B` every other frame; poking the player
inside tiles `10..=51` x `14..=72` stops it dead. So `0x6DE` is a **live
position predicate** - "the party is not in the plaza" - and the card-boot
state held it set because its save stands at tile `(17, 97)`. The engine side
of the same read was a defect: the ctx-`0xFB` system context kept its position
anchor at the origin outside three opening scenes, so every scene's per-frame
`CD F8` tests answered "outside"; `World::sync_field_ctx_player_anchor` now
re-seats it each frame slice
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion),
[`script-vm.md`](../subsystems/script-vm.md#a-system-flag-can-be-a-live-position-test-not-progress)).

**Does a scripted camera tile window survive into the next scene** closed here:
it does, by 78 vsyncs. A per-vsync poll across a real `map01` -> `town0c` door
finds the scene word flipping at vsync 37 while the window keeps the previous
scene's values, re-stamped only at vsync 115 to `(-7, -6, 5, 7)` and per-region
from there. The port's re-stamp-per-entry is not falsified; its *value* is - the
`FIELD_DEFAULT_VIEW_WINDOW` `(-8, -6, 6, 10)` it stamps is a later region's
window, not the one retail writes on entry
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion)). The same run retired the
walk that was meant to produce it: a Rim Elm **house door** is an intra-scene
warp - `town0c` stays - so it crosses no scene at all.

**Why a `juui1` name hijack draws black** closed here, on the door census rather
than on another probe. Of 498 clean op-`0x3F` doors, 446 are preceded by
`34 05 FF FF FF 41 00` - a white `ColorIntensity` the **departing** script runs
as its door prologue. A hijack rewrites the destination name and never runs the
prologue, so the frame it produces is the one the tint was never raised for, and
the brightly-drawn control scene comes out equally black. The entry path makes
the frame ([settled](re-settled-threads.md#field--locomotion)).

**What the field VM's `4C 14` does** closed here: it clones an actor. The op is
the only eight-byte instruction in outer nibble 1 - the `0x14` arm reads a sixth
payload byte and adds one more to the nibble's own seven - and that byte names a
cross-context source actor whose transform is copied onto a fresh pool node that
fades out on a scripted rate and retires. Six scenes issue it, 94 times; a
seven-byte reading desynced every one of those records from its first
occurrence ([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion)).

**What `4C 86` and `4C 87` do** closed with them, and neither is what their
labels said. `4C 86` spawns the **reflection controller** on descriptor
`0x801F2948`: the executing script makes itself the mirror image of the actor
its last operand byte names, and the six `s16` are the controller's mirror line
and tracking rect rather than a transform for that actor. `4C 87` retires every
live one. Neither parks - the advance sits in the retire call's delay slot -
and the same is true of `4C 9F`, which sweeps a different handler entirely
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion)).

**Which scene scripts write the story flags a census surfaces** closed here with
an answer that is not a beat: shipped scene MANs carry **developer flag-setting
menus** - "Clear all flags", "Set all flags" / "Clear" / "Exit", "=Back=" -
whose arms are genuine `51`/`61` ops over nine-flag ladders. Nine scenes carry
one. A census that counts arms therefore over-counts writers of any flag a
ladder happens to cover, which is exactly how spine flag `0x142` came to be
credited to six records instead of two
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#battle--arts--level-up)).

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

- **never walked:** `rayman`/`rayman2`, `station`/`station3`, and the Karisto
  spokes `bubu2` + `deroa`. A sweep of both emulators' state populations finds
  **no** state in any of them, and the one `rayman` state answers a different
  question: an idle firehose over it logs 0 SETs and 2 CLEARs (`0x11`/`0x12`,
  writers at `ra` `0x801D551C` / `0x801D55DC`), because a family SET fires on
  the beat, not on standing still. `chitei2` has **left** this set - not by a
  human play-forward but by the card tier, which places a save by its
  product-code suffix rather than by the card block it occupies, so re-stamping
  a staged copy reaches any save on a progression card from one calibrated
  CONTINUE ladder. What its own cold-boot anchor then measured is a warning
  about reading a firehose: over 900 vsyncs of standing in the scene, flag
  `0x19D` SETs 392 times and the 16-flag band `0x19B..0x1AA` CLEARs 12512
  times, both from the ordinary helpers - a per-frame **one-hot selector**
  living in the story-flag bank, not a progress latch. A spoke whose region
  carries one of those needs its family separated from the selector's traffic
  before any order is read off the log;
- **walked without an organic family SET** (the beats were already latched in
  the loaded state, or the region was entered mid-arc): `retock`/`retockin`
  (`0x502` never fired - its writer hangs off Eliza's talk loop, unhidden by
  `jagaroom`'s `0x33B`; `0x357` pre-latched). `doman`'s `0x3FB` (P2[4]),
  `son`'s arrival family and the `kor5` tail's `0x6C4` (P2[8]) have since been
  captured, `kor5` with its `0x436` input poked rather than played; the
  retock / doman / nilboa entry families are measured
  ([settled](re-settled-threads.md#field--locomotion)).

The generic C1/C2 seeder already drives every family. One more session from
an early-enough save (before the retock/doman/nilboa beats) closes the
walked-but-latched set. The never-walked set is now a question of whether a
card block exists for each spoke rather than of whether a probe can reach one:
`retock`, `doman`, `nilboa`, `son` and the `kor5` tail are all walkable from a
catalogued card block, and the residual spokes are the ones no card has.

**A door no longer needs a walk.** A walk-on door is an exact tile match in the
`.MAP` kind-1 trigger table, so writing the player object's position onto a
door tile crosses it in about ninety vsyncs
([`autorun_w5a_poke_walk.lua`](../../scripts/pcsx-redux/autorun_w5a_poke_walk.lua);
door tiles come from each `.MAP`'s `+0x10000` / `+0x12000` gate rows), and a
door that opens a Yes/No picker takes one confirm press on top. Paired with the
single-flag write watch
([`autorun_w5a_flag_watch.lua`](../../scripts/pcsx-redux/autorun_w5a_flag_watch.lua)),
that turns each residual spoke into one run from a card block that reaches its
region. It measures arrival, never the walk. Read what it logs with the `conc`
lesson in hand: a flag written every other frame from an entry script's
per-frame body is a position predicate, not a beat
([`script-vm.md`](../subsystems/script-vm.md#a-system-flag-can-be-a-live-position-test-not-progress)).

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
| Does Koru's timed-fight strip draw over or under the `(16, 14)` tab? | open (narrowed) - both hosts draw the strip; only its order is unpinned | `engine-core::timed_fight` gates the strip on formation slot 0 holding Koru (`0xB6`) and keeps it up for the command phase, and both hosts draw it on top of the name-plate tab that shares its seat ([`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-four-turn-strip-belongs-to-koru-not-the-dome)). The one capture of the strip never holds the plate in the same frame. A Koru-fight frame with both up, or the two emitters' ordering-table slots read off the disassembly, closes it. |
| Why does the play page's enemy-target label overprint the commit log's first row? | open - page only | On the play page the target label sits near `x = 155` and overprints row 0 of the commit log; retail's target plaque rests at `x = 205`, centred on `232` ([`battle.md`](../subsystems/battle.md#the-commit-log)). The page's plaque seat read off record `0x29`'s placement, matched against a command-phase capture, closes it. |
| Why does the overworld camera sit higher and farther than retail's? | open - both hosts | A frame pair on `map01` puts both hosts' world-map camera above and behind retail's framing (`engine-core::camera_view`'s world-map arm). The world-map view build's height and distance words, read off the disassembly and compared with the port's constants, close it. |
| Does the port run the world-map fog arm? | open - field only | Every kingdom overworld raises the fog cap to `0x48` (`FUN_8003AEB0` from MAN header bit 0), so retail runs the pool on the world map; the port's fog render is wired for field scenes only ([`field-ambient-fx.md`](../subsystems/field-ambient-fx.md)). A world-map fog frame on both hosts against a `map01` / `map03` state closes it. |

Three more rows closed here. **The commit log** is drawn on both hosts from
`FUN_801D388C`'s commit arms: records `0x2B + 3n`, columns at `x = 16`,
`name_w + 0x20` and `name_w + 0x60`, rows scrolling `170` / `146, 170` /
`146, 170, 194`; the glide from seat A and the round-start launch are not
modelled ([`battle.md`](../subsystems/battle.md#the-commit-log)). **The native
world map's white sheets** were the field fog: `--seed-party` ran the New
Game boot after a `--world-map` entry and left `map01` in field mode, so the
fog pool map01's MAN bit raises drew through the field camera. **The spoils
and shop ink** is retail's default pen, ink `7` = `(206, 206, 206)`, in every
`engine-ui` builder that had drawn full white.

Three rows closed here with a wire on both hosts. **The `0x6E` commit-confirm
screen** is staged once for the whole party, after the last member able to act,
for every party size and behind no option; its `Reselect` steps back one member
through `FUN_801D32BC(1)` - onto the **last** able member, not the first - and
refunds that member's Item ([settled](re-settled-threads.md#battle--arts--level-up),
[`battle.md`](../subsystems/battle.md#the-commit-confirm-screen-0x6e)). **`vell`'s
native fog** was invisible because the native scene VRAM lacked the PROT 0874
section-2 effect pool the fog page samples; retail keeps it resident under every
field scene, and nine retail states agree cell for cell
([settled](re-settled-threads.md#rendering--camera)). **Which host draws Koru's
strip** is both: the gate is formation slot 0, the limit is Koru's own AI arm,
and only the draw order against the tab stays open (row above).

**Does the port run retail's in-battle steal** and **how does the port step
back to an earlier member** both closed with a wire on both hosts. The steal is
the killing blow's once-per-chain roll, not a command, and a disc-gated round
reproduces the retail caption word for word; the back step is the ring's cancel
with a non-zero step counter
([settled](re-settled-threads.md#battle--arts--level-up)).

Two threads closed here before these opened, and both were questions about
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
| Does a Muscle Dome round load the class-2 bank? | open (narrowed) - the hub is measured, a round is not | At the contest hub (`minigame_muscle_dome`, mode `0x19`) slot 2 is closed and slot 6 holds PROT 0876 intact, so the hub has no class-2 bank. A round runs the battle frame driver `FUN_80046A20`, which calls the battle scene loader at `0x8004711C`, so a round is taken to stage PROT 0869 - but the only PCSX-Redux dome-round state carries a patched executable ([`audio.md`](../subsystems/audio.md#retail-capture-of-the-slot-2--slot-6-residency)). A retail dome-round state's slot records at `0x80091508`, read against the disc banks, close it. |
| What does the field init's slot-10 load serve? | open - decoded, unmodelled | `FUN_801D6704` loads raw `0x428` then `0x422` into slot 10, over slot 0's SPU base, only while the current track `*(0x8007BAC8)` is `0x814` and the one-shot latch `0x8007B9B8` is clear (`0x801D71A0..0x801D7274`). Which scene plays track `0x814`, what cues name category 10 there, and who clears the latch close it. |

**Do the hosts play the field's scripted SFX cues** closed as yes, on both.
Field-VM op `0x36` sub `0` / `4` and motion-VM op `0x09` call the ring
producers `FUN_80035B50` / `FUN_80035BAC` with ids straight from their
bytecode; the world queues each call and both hosts replay the queue onto
their SFX ring. Ids at or above `0x200` resolve through the scene prescript's
record 0, and the category-6 field bank (PROT 0876) is resident on both hosts,
refilled per mode with the class-2 bank it shares an SPU region with. A
closed slot's cue is silent, not rerouted ([settled](re-settled-threads.md#audio)).

The last three threads before it closed the same way: by fixing the instrument
rather than the engine.

**Why the port sustains more sounding voices than retail on the same track**
closed as two instrument defects stacked, with no engine change owed. The
"matched 120-frame window" was never matched - pairing frame 0 of each side
compares two different bars of one piece, and the retail window from
`s3_rimelm_freeroam` actually aligns at engine frame **3111** of a 3601-frame
trace, where both a symmetric and an intersection-only pitch score peak.
Aligned, the two sides carry the same ten packed ADSR words, the same key-on
count on nine of the ten tones, and 71 engine key-ons against 67 retail ones.
The gap that survives alignment is in the envelope channel, and that channel is
not on emulated time at all: PCSX-Redux steps its ADSR on an **audio-paced
thread**, so a per-vsync save-state capture carries an uncontrolled amount of
envelope motion - two captures of one state with no input agree on the voice
pitch register for 5285 of 6000 voice-frames and on `env_level` for 404 of 1000,
and the mean sounding count itself moves 4.041 against 3.694 when the host slows
down. The comparand that survives is the **key-on rate**, which is a register
write the score performs from the game's own vsync handler. Both explanations
the row offered - a long release tail, or keying fresh slots - are falsified,
and the engine's envelope is the side that matches the hardware formula
([settled](re-settled-threads.md#audio),
[falsified](re-do-not-re-walk.md#audio--sound-driver)).

Two threads closed here before it, and both closed by fixing the instrument
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

No open threads. The last three closed on one mechanism - the packer's buffer.

**Should byte accounting cut an inherited tail the way disc coverage does**
closed as yes, with **one** rule under both instruments. The packer writes every
entry through one buffer indexed by file offset and never cleared, so an
entry's last sector past its own content holds the bytes of the nearest
**earlier** TOC entry whose extent reaches that offset - an overlay, but also a
non-overlay donor (PROT 0898's last 1,604 bytes and 0895's last 344 are
0894's). Searching that suffix over the whole entry rather than only its last
sector reproduces the sibling cut on 82 of 83 images, and `tail_cuts` now feeds
`disc-coverage.py`, the attribution sweep and the byte account alike. The same
leg explains every scene bundle's last-sector residue (90 of 90 bundles) and
the `lzs_container`, `pack` and `bse_bank` tails that read as walker residue
([settled](re-settled-threads.md#measurement--corpus),
[falsified](re-do-not-re-walk.md#containers--placeholder-slots),
[`disc-coverage.md`](../tooling/disc-coverage.md#the-packer-buffer-leg)).
PROT 0901 was the last disagreement, and its consumer settles it: 0901's code
ends at `jr ra` on file `0x24DC`, and the run from `0x252A` opens mid-routine on
PROT 0900's epilogue, referenced only from 0900.

**Is PROT 0967 part of the slot-B module band** closed on the layout question
rather than the band one. The slot-B layout walk is selected by the **link
base**, which admits 0967 at 96.6%; its prompt pool is consumed - 28 strings
formed at 41 sites by its own code - and its 92-byte leaf `FUN_801F7628` is
reached by `jal` from `0x801F7184` and `0x801F7460` inside the image, which
also retires "a slot-B image never calls itself"
([settled](re-settled-threads.md#title--boot--overlays),
[falsified](re-do-not-re-walk.md#containers--placeholder-slots)). The cast
band - which images the cast dispatcher reaches - is a separate predicate and
stays `0903..=0966`.

**What actually breaks a rebuilt PROT 0874 container at battle load** closed
here: nothing a rebuild can do reaches the registrar, and the symptom is the
truncated pack the third reading already named. The five readings the row
retired all stand. What is new is a sixth, and it corrects the **fourth** rather
than the fifth. `FUN_8001E890` does carry an integrity check in its own frame:
entered with the load word at `2` it reads the raw container back out of
**VRAM** - four `0x40 x 0x100` rects from `(0x180, 0)` through `0x8005842C`,
`0x20000` bytes into its own scratch allocation - re-sums every word of PROT
`0x36C` at `0x8001E9C4..0x8001E9F4`, compares against the boot-time sum at
`gp+0x6B8`, and on a mismatch clears the load word and reloads from the CD
(`j 0x8001E900`). But that guard sits on the **decompress source**, not on the
pack the registrar walks, and the register-only arm never runs it - with the
word at `1` the `bne v1, v0` at `0x8001E974` jumps straight past the sum. So
"nothing in its own frame keeps it away from a foreign block" stays true of the
one arm that could walk one, and the gate's writers remain the reason it is
safe. The leg the row kept open is not constructible either: its "header size
word" and its "section-0 decoded length" are **one** field - the descriptor's
own `+0x08` size word, which both sizes the buffer and drives the length-driven
decode - so a hand-built container cannot make the two disagree, only truncate.
The probe the row left over has run, and it cannot trip the guard from RAM:
forcing the load word to `2` reaches the sum arm and a genuine mismatch enters
`0x8001EA08` and self-heals in about 45 vsyncs (clear the word, `j 0x8001E900`,
re-read from CD, `gp+0x6AC := 1` at `0x8001EB0C`), but four XOR'd words of the
resident container leave the sum byte-identical - it is taken over a
`StoreImage` VRAM read-back. Only VRAM at `(0x180 + 0x40i, 0)` or the boot sum
at `gp+0x6B8` can break it
([settled](re-settled-threads.md#battle--arts--level-up),
[falsified](re-do-not-re-walk.md#containers--placeholder-slots)).

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

## Measurement + tooling

| Thread | Status | What would close it |
|---|---|---|
| Which live ports cover only part of their routine, and which of those differ from retail? | open (narrowed) - listed, not ported | Of the 43 partial ports a tag audit found, the ones that still change behaviour are `FUN_801D1BA0`'s clip tail with `FUN_801D1EC4` (a reroll site the encounter port misses), the tile board's menu and fade states, `FUN_801DB510`'s hold gates, `FUN_801CF8AC`'s probe arms, the actor allocator's initialisation, Rula's lift, the dome hub's shade and gates, and a battle and an audio residue - each named with its blocker [details ↓](#which-live-ports-cover-only-part-of-their-routine). Each residue ported, or disclosed where it is blocked, closes it. |
| Which save-state library entries still hold a patched executable? | open (narrowed) - audited and tagged; the rest need human play | A state made on a patched disc keeps that build's `SCUS_942.54` in RAM on every later load. `patch_taint_audit.py states` tags each library state's `resident_patch`; the S1..S5 anchors, `first_town_interactive` and `teien_field_run` are re-shot retail, and every capture-graded claim re-checked against its family stands ([`pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md#patched-disc-taint)). The `rikuroa_*`, `dolk2_market_noa`, `cort_evolved_*`, `minigame_*_pcsx`, `battle_gaza2_*` states and the mednafen `overworld_battle_bg_angle_*` states re-shot on an unpatched image close it. |
| Which engine draws hand a raw LCG state where retail calls BIOS `rand`? | open - one caller fixed | `FUN_80056798` is the BIOS `A(2Fh)` thunk and returns `(seed >> 16) & 0x7FFF`; the fog spawner's low-bit tests on a raw 32-bit state cycled with period 16 until `bios_rand_shape` shaped them. Every other port that stands in for a `jal 0x80056798` and masks low bits is a candidate. An audit of those call sites against the port's RNG closes it. |

### Which live ports cover only part of their routine

*Status:* open - the list is the residue of a 316-tag audit after two rounds of fixes

The audit found 43 partial ports. Fixed since, on both hosts where a host is
involved: the encounter reroll `FUN_801DDF48` and its global `_DAT_8007B5FC`;
the tile board's event-cell flag writes and trigger exit (states `3` and `8`);
Baka Fighter's and the slot machine's face buttons; the actor tick's move-VM
step when `+0x54 < 0`; `FUN_801DBF9C`'s parameter stream, which was written one
byte low; the initiative sweep's dead-slot refund and tie list; the follow
camera's shake consumption and its mode-5 focus; the SFX ring's three
producers; the narration roller's config block; the escape timer's HUD;
`FUN_8003540C`'s unconditional clear; the two routines that had no port at
all, the strip emitter `FUN_801D31B0` and the dome course card `FUN_801D042C`;
the "nobody can act" `0x1E` -> `0x6E` round with its `ctx[+0x25]` skip count;
the `0x801F696C` swing drift; and the settle's clip tail with the timed
kind-0 warp.

What remains, each with where it is recorded:

- **`FUN_801D1BA0`'s tail and `FUN_801D1EC4`.** The clip tail and the timed
  kind-0 warp are ported, the warp landing's reroll included
  ([`field-locomotion.md`](../subsystems/field-locomotion.md#the-clip-base-and-the-settle-tail)).
  Left: the op-`4C CE` override `_DAT_8007B6AC` (read as `0`), op `0x22`'s
  clip-base write for script moves, the warp's `0x801DA7F0` actor tag and
  its `0x1F800394 & 0x80000` same-tile re-run.
- **Tile board** (`801EF2B0`): the menu and fade states.
- **`FUN_801DB510`**: the hold gates, which have no modelled writer.
- **`FUN_801CF8AC`**: the probe arms beyond the box test.
- **`FUN_80020DE0` / `FUN_80024E08`**: the allocator's initialisation from the
  spawn descriptor, and the model setter's `FUN_80020F88` re-stage.
- **Rula's lift** (`801EE328`): the kernel lifts the player to
  `Y < -0x618`, but the host holds no player handle to apply it or the
  Riremito restore.
- **The Muscle Dome hub**: `FUN_801D1610` is a subtractive `POLY_G4`
  (tpage `0x46`, ABR 2) the port approximates with alpha bands; the gates
  `_DAT_8007BC20` / `0x8007B648` are modelled open; the first-visit SFX
  `FUN_8003D53C` is not played; the slot machine's spin-up latch `0x801D3790`
  and Baka Fighter's ladder run are unported.
- **Battle**: `801F0450`'s auto-combo tail (and with it the `0x801F0518`
  write of the `0x801F696C` flag), the tag-`0x67` streak ribbon
  `FUN_801E1D98`, the War God Icon's `ctx[+0x16]` bump at
  `0x801E37AC..0x801E37BC`, and `FUN_8004AD80`'s kind ladder with
  `FUN_80048A08`'s draw.
- **Audio/anim**: `8004DA00`, `80064090`, `800480D8` and `80020F88`, each
  blocked on a host structure (no XA channel streamer, no multi-slot SEQ
  table, a crate barrier between `engine-core` and `engine-render`).

Rows of the audit not named here were not re-read after it; the audit's own
evidence column is the place to start before trusting one.

**Which references does the indexed form hide** closed at 512 accesses, none
in PROT 0897 / 0899: the counts that opened the row came from a scanner that
never dropped an overwritten register, and both reference sweeps shared the same
laxness. Re-scanning every address the ignore list and docs call unreferenced
moved no verdict ([settled](re-settled-threads.md#measurement--corpus)).

**Which slot-B record walk bounds PROT 0944's top record** closed on the
walker, not the image: nothing in any consumer loop bounds the record band, and
the Rust walk had chained zero padding as a `[model_sel 0]` record. Both walkers
now stop the chain at eight zero bytes, which moves twelve images and puts
0944's cut at `0x199C` with donor 0942 on both instruments
([settled](re-settled-threads.md#measurement--corpus),
[`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#the-chain-stops-at-zero-padding)).

**Does any shipped beat take op `0x34` sub-0's second arm** closed as no. The
arm forks on `_DAT_1F800394 & 0x800000` (`lui v1, 0x80` at `0x801DFD1C` and
`0x801DFEB0` in PROT 0897; set runs `FUN_80024E80`, clear runs
`FUN_801DE2B0`), and a writer census of bit 23 over every image and scene
carrier finds no store the disc executes; the port models the clear arm, and
its `FUN_80024E80` port stays live as `spawn_screen_fade`. The motion VM's ops
`0x10` / `0x11` fell with it - no section-1 stream authors either
([settled](re-settled-threads.md#field--locomotion)). One caveat stays on the
row it closed: a block copy into the scratchpad is not excluded by a store
census.

**Does the morph-weight spawner ever seat its handler** closed by taking the
seat, and a premise fell on the way. The spawner snapshots the rest pose and the
pool envelope steps the weight on both hosts, and the `4C D8` model operand is a
**scene-bank** index - the arm adds `*(u16 *)0x8007B6F8` at
`0x801E2DE0..0x801E2DE8` - not a global-pool slot, which is why the balden
carriers looked unseatable: all five carriers now seat, 17 of 17 blocks fit
their mesh vertex for vertex
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion),
[`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#the-model-operand-is-a-scene-bank-index)).

**Does any host draw the field screen-effect fade** closed here, and the
answer was no until it was wired: the op-`0x34` sub-0 tween published a
`ScreenTintPush` per frame on both hosts and no render surface read it. Both
hosts now composite it through one emitter,
`screen_prim::screen_effect_push_prims`, driven by a page draw-list ladder and
the native frame path. What the push's three words are is settled on the
bytes: `FUN_80024EE4` builds exactly one full-display quad, its first argument
is the ordering-table bucket, the second the ABR blend equation and the third a
GP0 colour word with red in the low byte
([settled](re-settled-threads.md#rendering--camera),
[falsified](re-do-not-re-walk.md#rendering--camera)).

**Do the field effect handlers `0x801E3E00` / `0x801E4D8C` / `0x801E5338`
have a spawner** closed as no host builds one: `World::tick_world_map` installs
entity state machines and carries no per-actor tick pointer, which is what
retail's object-effect dispatch writes at `+0x0C`. The three ports are disclosed
on that pass; the site's world-overview fog is a per-kingdom snapshot of
`actor[+0x74]`, not a run of the script.

**Do the camera-snap and ocean-only kernels run on any host** closed in two
halves: `Camera::take_camera_snap_beats` was a ladder gap and is entered now
(`opdeene` publishes the cutscene camera arm, the only page branch that reads
the bank); `FieldSceneAnim::ocean_only` is the damaged-bundle fallback and no
shipped kingdom reaches it - all three install the slot-5 CLUT walker
([falsified](re-do-not-re-walk.md#world-map--kingdom-bundles)).


**Which model owns the field screen-effect fade** closed here, and the answer is
the **push**. Retail's beat, driven and logged frame by frame, is
`FUN_80024EE4(kind, blend, packed_colour)` once per step from an effect actor
the op `0x34` sub-0 arm spawns - a `(kind, blend, packed)` triple, which is the
push's shape exactly - while the global multiply tint `DAT_8007BCB8..BA` holds
neutral `0x80` on all 900 vsyncs, so the beat is not an op `0x4C 0x12` fade at
all. The row's premise was half wrong in the port's favour and half wrong
against it: the `effect_tint` ramp it said the renderers read had **no** reader
either, so both representations were dead, and the arm the port did implement
was a third of one - retail's sub-0 is a walk-out / walk-in **pair** whose blend
and push kind come out of the sub-op byte, and whose all-zero operand clears the
live actor rather than ramping to black
([settled](re-settled-threads.md#field--locomotion),
[falsified](re-do-not-re-walk.md#field--locomotion)). What the closure does not
settle is whether anything **draws** the surviving model - the first row above.

Four rows closed here before them, three of them the Equip screen's.

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
one per kingdom map bundle for the first, and for the second every chest
script that consumes an item - seventy sites in twenty-five scenes, each one
text line into its record. The row asked for an instrument rather than an
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
