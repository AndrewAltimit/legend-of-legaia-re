# Falsified RE readings - do not re-walk

Hypotheses about Legaia's runtime that were disproved, kept with their
reasoning intact. The reasoning is the deliverable: each of these is a
*plausible* reading of the bytes, and knowing why it is wrong is worth more
than the row it occupies. Check here when a reading looks obvious - somebody may
already have spent a week disproving it.

Every entry has the same three parts: the **tempting reading** and why it is
tempting, **why it is wrong** (the decisive instruction, capture or count), and
**what is true instead**, with a link to the page that owns the answer. Each
area opens with a table of one-line entries (`Thread | Verdict | Why`, the *Why*
cell starting "Plausible: ..."); an entry that needs more room is a `###`
section after the table.

Rows here are terminal. If new evidence reopens one, move it back to
[`open-rev-eng-threads.md`](open-rev-eng-threads.md) rather than editing the
verdict in place - the falsification trail is what makes the row useful. The
answered questions are in [`re-settled-threads.md`](re-settled-threads.md).

Two falsification classes recur often enough to name up front. **VA aliasing**:
a bare virtual address (VA) is not an identity, because the overlay code images
the game loads at runtime share RAM windows, so slot-A and slot-B overlays host
different code at the same VA and a dump labelled by address can be a different
function entirely. **Ghidra's collapsed switch**: a jump table's arms can render
as bare `break`s or as fake `FUN_x` calls, inventing opcode semantics that the
raw table does not have. Both have produced multiple entries below.

**Index.** The areas, and the longer entries in each:

- [World map / kingdom bundles](#world-map--kingdom-bundles)
  - [Slot 4 is a GTE vertex pool walked by an unpinned "cluster-A command stream"](#slot-4-is-a-gte-vertex-pool-walked-by-an-unpinned-cluster-a-command-stream)
  - [The world-map slot-4 landmark meshes are the consumer of the lit prim handlers](#the-world-map-slot-4-landmark-meshes-are-the-consumer-of-the-lit-prim-handlers)
  - [Op `4C 12` is the scene-entry screen fade](#op-4c-12-is-the-scene-entry-screen-fade)
  - [The field runs no hardware light source](#the-field-runs-no-hardware-light-source)
- [Battle / arts / level-up](#battle--arts--level-up)
  - [The scripted boost profile is "the international release's" profile for every fight](#the-scripted-boost-profile-is-the-international-releases-profile-for-every-fight)
  - [`FUN_801F3D3C` installs a queued-magic follow-up routine](#fun_801f3d3c-installs-a-queued-magic-follow-up-routine)
  - [The dynamic-slot rewrite was never "`0x10` and `0x1A` only"](#the-dynamic-slot-rewrite-was-never-0x10-and-0x1a-only)
  - [A level-up is not a heal](#a-level-up-is-not-a-heal)
  - [The flash ramp is the Arts announcement banner](#the-flash-ramp-is-the-arts-announcement-banner)
  - [The battle message banner has no interior fill](#the-battle-message-banner-has-no-interior-fill)
  - [A kind named by its seat can name the wrong record](#a-kind-named-by-its-seat-can-name-the-wrong-record)
  - [`FUN_801DBB8C` is not the party readout's registration](#fun_801dbb8c-is-not-the-party-readouts-registration)
  - [The item window shows the pill](#the-item-window-shows-the-pill)
  - [The magic chip's gate reads the weapon byte](#the-magic-chips-gate-reads-the-weapon-byte)
  - [`FUN_801DBC30` is not the battle name plate](#fun_801dbc30-is-not-the-battle-name-plate)
  - [The dome victory caption is not a prize](#the-dome-victory-caption-is-not-a-prize)
  - [Muscle Dome was never a card battle](#muscle-dome-was-never-a-card-battle)
  - [Op-0x4E sub-op family - every sub-op 0..9 is a compare](#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare)
  - [Gaza 2 0x51 park - the two falsified generators](#gaza-2-0x51-park---the-two-falsified-generators)
  - [The cast-module blocker was named wrong](#the-cast-module-blocker-was-named-wrong)
  - [The attack camera was never an arm choice](#the-attack-camera-was-never-an-arm-choice)
  - [The case-6 party arm is the battle-over framing](#the-case-6-party-arm-is-the-battle-over-framing)
  - [The Done band is not idle](#the-done-band-is-not-idle)
  - [The battle-intro banner is raised from a top-seated `0x0303` placement record](#the-battle-intro-banner-is-raised-from-a-top-seated-0x0303-placement-record)
  - [`0x801CFA48` is a mid-function citation aliased to another overlay](#0x801cfa48-is-a-mid-function-citation-aliased-to-another-overlay)
  - [`_DAT_8007BD84` is a mode word the melee kernel branches on](#_dat_8007bd84-is-a-mode-word-the-melee-kernel-branches-on)
  - [The slot cabinet is in neither the art pack nor any prim a traced slot function emits](#the-slot-cabinet-is-in-neither-the-art-pack-nor-any-prim-a-traced-slot-function-emits)
  - [The slot's `DAT_801d4134 * 0x10` is a sub-row landing nudge](#the-slots-dat_801d4134--0x10-is-a-sub-row-landing-nudge)
  - [A phase-gated effect draw is the candidate for the arena's object-1 dust decal](#a-phase-gated-effect-draw-is-the-candidate-for-the-arenas-object-1-dust-decal)
  - [`ctx[+0x26]` is a boss phase counter and `ctx[+0xD]` is a dead store](#ctx0x26-is-a-boss-phase-counter-and-ctx0xd-is-a-dead-store)
  - [`0x801E3A20..0x801E3A64` is the Miracle continuation](#0x801e3a200x801e3a64-is-the-miracle-continuation)
  - [The Miracle marker is armed by an input recognizer (`FUN_801E91E8`'s caller)](#the-miracle-marker-is-armed-by-an-input-recognizer-fun_801e91e8s-caller)
  - [`DAT_8007BD10` is a per-slot control-mode byte](#dat_8007bd10-is-a-per-slot-control-mode-byte)
  - [The spell record's `+0x01` effect-class byte is undecoded](#the-spell-records-0x01-effect-class-byte-is-undecoded)
  - [The slot-B module band shares a library tail](#the-slot-b-module-band-shares-a-library-tail)
  - [The slot-B band: four readings the whole-band dump overturned](#the-slot-b-band-four-readings-the-whole-band-dump-overturned)
  - [The summon draw runs 35-64 times a frame](#the-summon-draw-runs-35-64-times-a-frame)
- [Audio / sound driver](#audio--sound-driver)
  - [Op-`0x35` sub-op 9 was never a queue](#op-0x35-sub-op-9-was-never-a-queue)
  - [The 256-slot SPU-address run that was really a CLUT](#the-256-slot-spu-address-run-that-was-really-a-clut)
  - [bse.dat: three readings the gp+0x678 trace overturned](#bsedat-three-readings-the-gp0x678-trace-overturned)
  - [`bse.dat` carries a second record family with a resident consumer](#bsedat-carries-a-second-record-family-with-a-resident-consumer)
- [Title / boot / overlays](#title--boot--overlays)
  - [The title sub-mode word lives at `0x801DD920`, and `0x02` is a screen a player can see](#the-title-sub-mode-word-lives-at-0x801dd920-and-0x02-is-a-screen-a-player-can-see)
  - [The attract sequence can be armed from any title sub-mode](#the-attract-sequence-can-be-armed-from-any-title-sub-mode)
  - [There is exactly one master-mode-`2` writer, at `0x801DFC00`](#there-is-exactly-one-master-mode-2-writer-at-0x801dfc00)
  - [The title slider `state[-0xEB4]` is clamped to `[0, 0x2C]`](#the-title-slider-state-0xeb4-is-clamped-to-0-0x2c)
  - [The title screen is loaded before the mode table is consulted](#the-title-screen-is-loaded-before-the-mode-table-is-consulted)
  - [`FUN_801DD35C` lives in an unindexed PROT.DAT gap between entries 899 and 900](#fun_801dd35c-lives-in-an-unindexed-protdat-gap-between-entries-899-and-900)
  - [`FUN_8003EAE4`'s flags are consumed by an untraced CD driver](#fun_8003eae4s-flags-are-consumed-by-an-untraced-cd-driver)
  - [`0x801CE9C0` is an entry point in no image, so mode 16 is a stripped dev path](#0x801ce9c0-is-an-entry-point-in-no-image-so-mode-16-is-a-stripped-dev-path)
  - [`FUN_801CE9C0` draws the publisher logos](#fun_801ce9c0-draws-the-publisher-logos)
  - [SCEA unfolds as a 2x2 grid of 32-row strips](#scea-unfolds-as-a-2x2-grid-of-32-row-strips)
  - [`0x801D06E0` is a SHARED_TAIL with no `jr ra`](#0x801d06e0-is-a-shared_tail-with-no-jr-ra)
- [Containers / placeholder slots](#containers--placeholder-slots)
  - [Assets named by the entry the over-read window started in](#assets-named-by-the-entry-the-over-read-window-started-in)
  - [Pochi-fill slots as stale mastering scratch](#pochi-fill-slots-as-stale-mastering-scratch)
  - [Concatenated sub-streams in a `scene_tmd_stream` entry](#concatenated-sub-streams-in-a-scene_tmd_stream-entry)
  - ["The backdrop shell is drawn once, so no completion exists"](#the-backdrop-shell-is-drawn-once-so-no-completion-exists)
  - ["Field-pack" was never a format](#field-pack-was-never-a-format)
  - [The prescript's "per-scene secondary header" is the next entry](#the-prescripts-per-scene-secondary-header-is-the-next-entry)
  - [PROT 0892: a 12 MB LZS container, or a truncated DATA_FIELD stream](#prot-0892-a-12-mb-lzs-container-or-a-truncated-data_field-stream)
  - [PROT 1221 / 1222 have no loader, because no image names raw TOC `0x4C7` / `0x4C8`](#prot-1221--1222-have-no-loader-because-no-image-names-raw-toc-0x4c7--0x4c8)
- [Field / locomotion](#field--locomotion)
  - [Op-0x43 sub-3..6 as a timed sound-register ramp](#op-0x43-sub-36-as-a-timed-sound-register-ramp)
  - [The reachable band's record force-walks the player through the wall](#the-reachable-bands-record-force-walks-the-player-through-the-wall)
  - [270 undumped field-overlay functions (recomp dispatch-entry seeds)](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds)
  - [`FUN_801F12D0` read from the `overlay_0897` dump](#fun_801f12d0-read-from-the-overlay_0897-dump)
  - [A second stage-id writer "at `0x801FD514` in the 0897 band"](#a-second-stage-id-writer-at-0x801fd514-in-the-0897-band)
  - [`0x801D84B4` is inter-function padding](#0x801d84b4-is-inter-function-padding)
  - [`FUN_801dfb10` is a scripted player-turn state machine](#fun_801dfb10-is-a-scripted-player-turn-state-machine)
  - [A scene with no `0x3F` in its MAN has no door](#a-scene-with-no-0x3f-in-its-man-has-no-door)
  - [A frontier-ladder scene that will not walk is a scene or a seat problem](#a-frontier-ladder-scene-that-will-not-walk-is-a-scene-or-a-seat-problem)
  - [Field-VM op `0x45` sub `0xC0` returns the operand `s16` as the next PC](#field-vm-op-0x45-sub-0xc0-returns-the-operand-s16-as-the-next-pc)
  - [Op `0x43` subs 0/1/A/B resume at an operand `s16`](#op-0x43-subs-01ab-resume-at-an-operand-s16)
- [No overlay function lives below `0x801CE818`](#no-overlay-function-lives-below-0x801ce818)
- [Menus / UI](#menus--ui)
  - [The save screen's block grid has a sixteenth Return cell](#the-save-screens-block-grid-has-a-sixteenth-return-cell)
  - [`0x801E5AE8` is a shared armament placer that `FUN_801D71F0` calls](#0x801e5ae8-is-a-shared-armament-placer-that-fun_801d71f0-calls)
  - [The disc's item population is far below 128, so a half-window bag cannot fill](#the-discs-item-population-is-far-below-128-so-a-half-window-bag-cannot-fill)
  - [Item-effect flag `0x40` is consumed by the item-info panel `FUN_801D0F1C`](#item-effect-flag-0x40-is-consumed-by-the-item-info-panel-fun_801d0f1c)
- [Rendering / camera](#rendering--camera)
- [Measurement readings](#measurement-readings)
  - [Two overlay lengths that were the neighbour's sectors](#two-overlay-lengths-that-were-the-neighbours-sectors)

## World map / kingdom bundles

| Thread | Verdict | Why |
|---|---|---|
| `FUN_801E3E00` is a world-map atmospheric fog-RGB actor tick whose `+0x74` is the per-kingdom haze | falsified (it is the attached light's keyframe script) | Plausible: the word sat in an overworld actor's tick slot in one capture and `+0x74` holds a colour. Its only reference is a `jal` at `0x801E450C` inside `FUN_801E4470`, the tick of the op-`0x34` sub-1 attached light; `+0x74` / `+0x88` are the light pool's two colours, and 0 of 98 states hold the word in RAM. It was already live as `field_actor_billboard::attached_sprite_script_tick`. |
| The overworld ground's far colour comes from `0x1F800398` or a script | falsified (a literal `0x100` per channel) | Plausible: the emitter reads its colour from that scratch word and sets no far colour itself. `0x1F800398` is `RGBC`, rewritten per frame from `0x8007B7B0`; the far colour is `SetFarColor(0x100, 0x100, 0x100)` at `0x801F729C`. The fitted value near `0xEF` was low because a bucket is keyed on the cell's farthest corner while `IR0` reads corner `(x1, z0)` (inference). |
| The overworld walk camera is its own pose, sliding between two zoom anchors | falsified (it is the field zone camera) | Plausible: two resident overworld states held two different pitch / depth pairs, and the top-view controller has a zoom input. The overworld is a mode-`0x03` field-run scene; on all three resident overworld states the live camera words equal the zone composer `FUN_801DAB90`'s staging descriptor at `0x801F3580`, and the two "anchors" are two region records' compositions ([`world-map.md`](../subsystems/world-map.md#walk-view-camera-retail-model-ram-pinned)). |
| A kingdom scene reaches `ClutWalkAnim::Ocean` | falsified (it is the arm for a bundle whose slot-5 CLUT-walk table fails to parse) | Plausible: the arm is named for the ocean and lives on the kingdom path, so a ladder into `map01` looks like the way to enter it. All three retail kingdoms ship a slot-5 table, and all three install the CLUT walker; only a modified or damaged disc reaches the fallback. |
| Slot-4 → cluster-A converter site | falsified | There is no slot-4 → cluster-A converter. The cluster-A pool (`DAT_8007C018`) is filled exclusively by `FUN_80026B4C`, reached only from `FUN_8001f05c` **case `0x02`** (TMD pack) and **case `0x09`** (bare TMD). Slot-4's type byte is **`0x05`**, whose `FUN_8001f05c` case merely allocates the MOVE buffer `_DAT_8007B888` and never calls `FUN_80026B4C`. So slot-4 bytes never become cluster-A TMDs; the `DAT_8007C018` kingdom entries are the scene's own type-`0x02` field-file TMD pack(s), installed by the single `FUN_80020224` descriptor-walk. |
| World-map outline / coastline reading | falsified | Visual inspection plus the slot-4 record-semantic work refuted the "world-map overlay outlines / coastline wireframe" interpretation. The replacement guess - "small object-local 3D meshes" - is falsified too: slot 4 is the scene's animation bank (below). Treat any future "kingdom border lines" claim with suspicion. |
| Walk-view decoration layer = walk-bit (`0x1000`) cells plus a `FLAG_MESH_DRAWN` test | falsified (the gate is `0x2000` alone) | Plausible: every tree / prop cell carries both bits, and the one family the walk-bit sweep wrongly admitted (record 408, a wall down every river) happened to lack flag `0x2`, so a flag test "fixed" it. But the resident kernel (`FUN_801F69D8`, PROT 0901, `andi 0x2000` at `0x801F6ECC`) tests only the draw bit, never `+0x12 & 0x2` - and the big enterable mountains sit on cells with `0x2000` and **no** `0x1000` (the mesh is the ground there), so the walk-bit sweep drew every tree and dropped every mountain while looking complete ([world-map.md](../subsystems/world-map.md#placing-the-continent-terrain-engine-port)). |
| `FUN_80024E08` performs the `0xF0` model-id split | falsified (the split is in its callers) | Plausible: the resolver is the routine that turns a model id into a pool pointer, so the id space's one discontinuity looks like its business. It does no splitting. Two callers do - `0x800393B8`, and `0x8003A2DC` inside the placement installer `FUN_8003A1E4` - each picking `DAT_8007B824` or the scene base `DAT_8007B6F8` before the call. A port tag naming the resolver for the split describes a routine that never sees the fork. |
| `FUN_80026B4C` is the TMD pointer table's only writer | falsified (PROT 0976 writes it directly) | Plausible: the registrar is the only routine that *installs* an entry, and every install path does go through it, so "only writer" and "only installer" predict the same thing everywhere an install happens. `0x801CF1E0` in PROT 0976 stores a zero into a slot without it. A liveness rule that enumerates the registrar's calls therefore has a hole exactly where a slot was cleared rather than filled. |

### Slot 4 is a GTE vertex pool walked by an unpinned "cluster-A command stream"

*Falsified by disassembly, with a live state agreeing.*

The reading: each 8-byte slot-4 record is a vertex `(i16 x, y, z, attr)` in
an object-local pool, indexed by a command stream nobody could find, drawn
in place by the world-map render library; `attr` render-unused.

Why it looked right: `FUN_80044C14`, a real per-kind primitive handler, does
load two words into the GTE vertex registers exactly that way, and a capture
showed the slot-4 window read in place with GTE-shaped return addresses.

What is true: there is no stream. `_DAT_8007B888` (the type-`0x05` buffer)
has six references image-wide, none in a render path; the entry's field
boundaries fall on **nibbles** (three 12-bit translations, three 8-bit
angles); `attr` is the Y / Z rotation, read every frame; and the capture's
return addresses sit inside the animated-mesh renderer `FUN_8001B964`, not a
prim handler. The `attr`-is-unread register-width argument swept a function
that never sees these bytes. See
[`re-settled-threads.md`](re-settled-threads/world-map.md#kingdom-slot-4---per-record-semantic).

### The world-map slot-4 landmark meshes are the consumer of the lit prim handlers

*Falsified twice over.*

The renderer's kinds 8..11 are the only handlers with an `NCC*` light op, and
the presumed consumer was the kingdom bundles' slot-4 "landmark meshes". Slot
4 is the scene's animation bank, not a mesh library; and exec breakpoints on
all four handlers over two kingdom overworlds, a field scene and a battle
return zero while the overworld's real handlers (PROT 0901's eight
replacements) fire in the same run. A kingdom overworld never enters the SCUS
prim-dispatch family at all. See
[`re-settled-threads.md`](re-settled-threads/battle.md).

### Op `4C 12` is the scene-entry screen fade

*Falsified by a reference scan.*

Every scene's arrival arm drops the op's word to `0` and ramps it back, which
reads like a fade from black, and the port staged it as a multiply over the
whole field frame. Its one reader disc-wide is the fog particle update
`FUN_8003F3FC`; `retona_field_card_boot` holds the word at `27` over a
full-brightness frame. See
[`cutscene.md`](../subsystems/cutscene.md#the-op-0x4c-0x12-tint-op-0x4c-0x12--the-effect-colour-op-0x34-sub-0).

### The field runs no hardware light source

*Falsified by a frame.*

The reading rested on a `town01` sweep that sampled only the kind-11 body and
on a GTE-opcode census that found the light handlers' consumers but not who
reaches them. The dispatcher selects by `flags >> 1`, so every TMD group with
flags `0x10..=0x17` goes through `NCCS` / `NCCT`; `cave01`'s walls are all such
groups, and its retail frame is the light's. A white back colour (`town01`'s
op `4C 8A`) is what made the town's lit rows look baked. See
[`renderer.md`](../subsystems/renderer.md#the-light-source-rows).

## Battle / arts / level-up

| Thread | Verdict | Why |
|---|---|---|
| The loss window is a pause-menu window descriptor | falsified (a SCUS placement record) | Plausible: it opens and closes like a pause-menu window. `FUN_801D8DE8` ids index the placement table `0x80076C10`, not the menu's descriptor table, and `0x41` / `0x42` share one record shape. |
| The battle pose decoder unwraps angles as `FUN_8001D088` does | falsified (the guards differ at half a turn) | Battle tests `> 0x800` (`slti 0x801`), the field helper `>= 0x800`, so the battle blend has its own kernel. |
| The battle ground shadow's `+0x6A` skip flag is an unmodelled divergence | falsified (the port already drew what retail draws) | `FUN_800480D8` raises it only around the after-image walk (`0x80048258`) and clears it before the body draw unless `+0x5A == 7`; a battle body's `+0x5A` is its pool slot (0..6), so bodies always cast a shadow and ghosts never do. Ported as `body_shadow_skip`. |
| The ghost pass's recompute misses come from the driver's `gp+0x330` gate | falsified (the gate passes on every frame) | The byte is the battle-load stage `0x8007B648`; it reads `0xFF` in 59 and `0x84` in 1 of 60 battle states, negative for the `lb` at `0x800470EC` in all of them. The two misses remain unexplained. |
| A second Rot replaces the first limb | falsified (limbs accumulate) | Plausible: one status word per actor reads as one slot. The applier ORs into `+0x16E` at `0x801E173C`; a tracker that overwrites makes the special-battle wipe rule's `0x38` test unreachable. |
| Op `3E FF` makes a scripted fight unescapable | falsified (the formation row's header byte does) | Plausible: most scripted fights refuse Run. The arm (`0x801E070C..0x801E0788`, PROT 0897) writes `sys+0x8A`, `sys+0x94` and `_DAT_8007B5FC` and requests mode `0xE`, nothing else; `ctx+0x287` comes from the row's `record[+0]`, and three `3E FF` rows (the Rim Elm ambush, the Tetsu spar, `deene` row 11) carry `0`. |
| `_DAT_8007BAC0` is a scripted-fight no-gold flag | falsified (it is the special-battle word) | Plausible: it zeroes gold. It is non-zero only in an arena leg and the two Ra-Seru-forbidden fights - a boss row leaves it alone - and it also zeroes EXP and suppresses drops, steals, absorb, spell XP and monster flee. |
| The battle ground shadow is `FUN_80028158` case 1 | falsified (mode word 1: shape 0 in the XZ plane) | Plausible: the call passes `1`. `FUN_80048A08` passes `a1 = 1` (`0x80049234`) and `a2 = 0x18`; the shape index is `(a1 >> 3) & 0xF = 0` and the `1` is the plane. 73 of 73 captured shadow blocks reproduce as the ordinary shape-0 ring with inner radius 0. |
| `FUN_8004DC68` is a target-highlight dim measured from the acting actor | falsified (a near-camera ghost pass) | Plausible: it sets semi-transparency bits on bodies near one point during actions. The point is on the camera's view axis - the focus pulled back by `dist*25/128` along the yaw - and bodies within `dist/4` go to blend `3`; during a cast it ghosts the caster's side. A RAM replica matches 277 of 279 seated slots. |
| Party entry 8 is a recover clip whose negative speed is a backstep | falsified (the downed kneel, root speed `0`) | Plausible: it follows the knockdown and the files carry negative root speeds. Entries 7 and 8 have speed `0` in every file; the negative speeds are the knockdown's, the block's and some flinches'. Three states read a dead member at `+0x1D9 = +0x1DA = 8`, and no action-SM store names 7 or 8. |
| `+0x1DC` bit 3 (the root-motion latch) is set by the knockdown's tag-`4` chain | falsified (by the tag-`8` commit) | Plausible: the latch holds a downed body still. The tag-8 commit raises it (`0x8004BF28..0x8004BF4C`); the knockdown's own commit clears `+0x1DC`. |
| `FUN_801E7320` picks a monster target as `rand % count + party_count` | falsified (`+ 3`) | Plausible: the engine compacts monsters after the party. Retail's pool slots are fixed - monster `k` sits at slot `3 + k` whatever the party size - and the resolver adds the literal (`addiu a0,v1,0x3` at `0x801E73B8`). |
| The battle-intro label's X is a GTE projection the renderer owns | falsified (the monster's battle world X) | Plausible: the label sits over its monster on screen. `FUN_801D9D3C` reads `lhu a0,0x34(v0)` at `0x801D9E00` / `0x801D9ED4` on the monster's pool actor - world X, which the engine already had. |
| The Baka Fighter impact pair draws no sprite, and runs on the ambient effect runtime | falsified (template A is the `0x4000` sprite arm; the duel seats its own parts) | Plausible: templates B are stage meshes and the ambient runtime seats move-VM records. Template A's op `0x23` makes a draw-kind-4 node on `FUN_8002A5A4`'s sprite arm (a `0xA0`-square quad stepping 16 cells), and `FUN_801D4DF8` passes rodata addresses straight to `FUN_80021B04`, which the scene-stager-only ambient runtime never sees. |
| `+0x90` / `+0x92` / `+0x94` are the move VM's tween sources | falsified (the per-tick rates of `+0x78` / `+0x72` / `+0x7A`) | Plausible: ops `0x0D` / `0x0F` / `0x11` write them from operands like a tween seed. `FUN_80021DF4`'s default motion block adds them into the depth-cue level, render scale and `+0x7A` (`0x80022B18..0x80022B7C`); under dispatch 3 the same words are the CLUT-cell HSV channels. |
| `ctx[+0x25]` is the battle's round counter | falsified (it is the round's skip count; the round counter is `+0x28A`) | Plausible: a byte cleared at every round's start reads like a counter of rounds, and a census of `sb ...,0x25` stores in PROT 0898 finds dozens. All but two store a GPU packet's `u` / `v` byte; the context byte's writers are the clear at `0x801DAB84` and the dead-slot sweep's bump at `0x801DAC2C`. |
| `ctx[+0x16]` has one writer, the War God Icon refill | falsified (two: the refill and a per-stage bump) | Plausible: the refill at `0x801E3A20..0x801E3A64` is the obvious Attack x2 counter write and sits by itself at the end of the stream. The stage site's tail also re-reads record `+0xF4 & 0x2000` and increments the byte at `0x801E37AC..0x801E37BC`, but only when it is already non-zero (`beqz` at `0x801E37B4`), which is what lifts it to `2` on the second pass ([`battle-action.md`](../subsystems/battle-action.md#the-war-god-icons-per-stage-bump)). |
| `0x801F696C`'s `0x801F0518` store belongs to `FUN_801F0450`'s auto-combo tail | falsified (it heads the auto-fill arm) | Plausible: the routine was known as the auto-combo assembler and its tail was undecoded. The store is the first instruction past the `+0x16E & 0x404` veto (`bnez` at `0x801F0508`), so it runs for every party slot the auto-fill arm takes; the art insertion tail starts at `0x801F0B4C` ([`battle-action.md`](../subsystems/battle-action.md#the-art-insertion-tail-0x801f0b4c0x801f1274)). |
| The auto-combo pool arm halves its AP budget under record `+0xF8 & 0x800` | falsified (the halving is the art tail's Spirit cost) | Plausible: `0x800` is the AP-Used-Down passive, and the pool arm spends the AP gauge `actor[+0x154]`. The routine's only `andi 0x800` is at `0x801F0D00`, inside the art insertion tail, and halves that tail's per-arrow Spirit cost (`srl s1,v0,1` at `0x801F0D0C`); the pool arm never tests the bit. |
| The art insertion tail's first draw is `rand() % 10` | falsified (`rand() % 5`) | Plausible: the divide is the `0x66666667` reciprocal, which is `/10` in its usual form. Here the high word is shifted by one more (`sra a0,t4,1` at `0x801F0BB0`), which makes it `/5`; the walk starts at bank record `rand() % 5 + 0xB`. |
| `0x801F696C`'s Miracle writer is at `0x801EF5A8` | falsified (the store is `0x801EF5B8`) | Plausible: the Miracle arm's `li v0,1` pair sits there. `0x801EF5A8` is `sb v0,0x28d(v1)`; the flag's `sw v0,0x696c(v1)` is the delay slot of the `j` at `0x801EF5B4`. The head clear (`0x801EED88`) and the one reader (`0x801E3840`) were not on the old row at all. |
| `FUN_801F138C` pays a captured monster's item into the bag | falsified (it is `FUN_801DABA4`'s dead-slot Item refund, at a phantom VA) | Plausible: the credit reads `actor[+0x1DF]` behind an `actor[+0x1DE] == 1` test, and a capture is the obvious reason a monster's slot would hand over an item. `0x801F138C` is a mis-based print of PROT 0898's `recompute_battle_order`; `+0x1DE == 1` is the Item category, and the refund returns a fallen fighter's committed item (`0x801DAC44..0x801DAC5C`). |
| The commit-confirm `Reselect` returns to the first party member | falsified (the last member that can act) | Plausible: "reselect" reads as starting over. By `0x6E` the forward walk has moved the cursor past the party, and `FUN_801D32BC(1)` steps one member back - the capture holds `ctx[+0x13] = 1`, `ctx[+0x1F] = 1` on a one-member party. |
| Retail pre-picks the arts target | falsified (state `0x5A` is a target cursor) | Plausible: the port committed an art straight to its default target. `0x5A` walks `+0x1DD` with Left / Right through `FUN_801D8D00` (`0x801D21D4..0x801D2268`). |
| The Begin / Reselect confirm follows each member's arts entry | falsified (one party-wide screen after the last able member) | Plausible: the port drew it after every arts entry. It is raised once, from any command, when `FUN_801DB81C` equals the party count - so it followed no one's Magic or Item in the port. |
| The in-battle steal is a command, or rolls on the hit | falsified (it rolls on the kill, once per strike chain) | Plausible: the steal table is per-monster like the drop table, and "steal" reads as an action a character takes. The roll lives in `FUN_8004AD80`'s knockdown-end arm, and the latch it sets before looking at the killer means an action's first kill spends that action's attempt ([settled](re-settled-threads/battle.md)). |
| The steal latch `ctx[+0x27]` is set once per battle and has one writer | falsified (the action SM clears it at every strike chain's exit) | Plausible: a byte scan for a store to `+0x27` finds exactly one `sb`. The clear is `sb zero, 0x16(s5)` at `0x801E3A84` with `s5 = ctx + 0x11`, in the delay slot of the `0x1E -> 0x1F` arm - a displacement no `0x27(reg)` scan matches. A write-watch caught it going `1 -> 0` at the chain's end. |
| Hub arms 4 / 5 / 6 draw the ROUND banner when a dome leg opens | falsified (course card + title art; the banner is arm `0x15` only) | Plausible: the envelope has the banner's shape and timing. Arms 4 / 5 call `FUN_801D042C`; arm 6 only drains the backdrop and starts the load. |
| `FUN_801DD4B0` is the physical-damage wrapper and `FUN_801DD6B4` the spell one | falsified (the other way round) | Plausible: the two wrappers are near-identical and sit next to each other. The stat each loads decides it: `+0x168` (INT) in `801DD4B0`, `+0x158` (ATK) in `801DD6B4`. |
| `FUN_801E93C8` re-arms the arts gauge / `FUN_80046870` is a charge gauge | falsified (an anim-rate restore; a cooldown top-up) | Plausible: both run around the art commit, where a gauge would be re-armed. Neither touches the AP gauge's fields. |
| `ctx[+0x28B]` has four writers, none in the battle overlay | falsified (five; the fifth is the tick's own clear in PROT 0898) | Plausible: the four raises in `FUN_8004AD80` were found by a SCUS scan; the clear at `0x801E263C` is a `sb zero` inside the overlay's banner tick, which that scan never read. |
| Which pass draws the ringside still is unsettled on the retail side | falsified (`FUN_801D00F8`, two `POLY_FT4` packets at texture pages `0x106` / `0x109`, measured live) | Plausible: the still's loader (`FUN_801F6B24`, PROT 0978) is traceable without its consumer. The draw is the contest hub's `FUN_801D00F8` (PROT 0977); port `engine_ui::ringside_backdrop` ([`ringside-still.md`](../formats/ringside-still.md)). |
| Spine flag `0x142` is set by six records | falsified (two are story writers; four are menu rows) | Plausible: all six decode as clean `51 42` / `61 42` arms in shipped carriers, and a flag-writer census cannot tell a story beat from a menu row - same opcode, same operand. Four are rows of a **developer flag-setting menu**: rikuroa `P1[10..12]` is a nine-flag Set ladder with its mirrored Clear ladder, and dolk2 `P1[1]` / dolk `P1[26]` have the same shape. The story writers are rikuroa `P2[50]` and dolk2 `P1[0]`. |
| The cast-arm countdown drains by the `scratch[0x37D] * scratch[0x393]` product | falsified (the multiplier is per arm) | Plausible: one arm does drain by that product, and a second that agrees makes it look like the band's rule. Each arm carries its own multiplier of the frame byte at `0x1F800393` - the product, twice it, or once it - so arms measured as "constant 4" and "constant 8" are `1x` and `2x` a byte that happened to read 4. A per-arm constant and a per-arm multiplier of a varying byte are indistinguishable in a single fight. |
| PROT 0950's arm 6 is frame-gated like its siblings | falsified (it has no gate; it ends on countdown expiry) | Plausible: twelve neighbouring arms in the band gate on a frame count, and a run that stops two ticks short of expiry looks like a gate firing. The arm tests nothing; the countdown runs out. The image's real fault is a different arm walking the actor table past a one-monster seat fill. |
| PROT 0905 (Vera) leaves the HP restore to the band's shared heal fold | falsified (it writes HP itself, as a second owner) | Plausible: every other healing module routes through the band's one heal fold, so a module that also has the amount looks like a caller. Vera stores the restored HP in its own tick as well, so a port that runs both applies the heal twice per frame. With a single owner, `amount = 0` on the fold's side is correct, not a missing value. |
| PROT 0952 carries no spawn record because its spawn sites load `$a2` from a saved register | falsified (the sites are not 0952's) | Plausible: a pointer arriving in a saved register does defeat the `lui`/`addiu` recovery, and the image has no record the parser can bound - "the parser cannot see it" and "there is nothing to see" predict the same output. Both sites sit in 0952's **inherited tail** and their pairs resolve to `0x801F8348` / `0x801F836C` - two of PROT 0951's records, past 0952's own image end. 0952 has no band because it has no records. |
| The slot-B spawn record's `+0x02` is a flags word | falsified (`reserved`) | Plausible: `[i16 model_sel][u16 ?]` is the shape of a record header, and a halfword beside a selector reads as flags. It is zero in all 1027 of the band's records and nothing on the disc reads it. |
| Both gates of the `0x801E6218` block jump to `0x801E6814` | falsified (they are conditional branches) | Plausible: the block's two exits do reach the same tail. They are `beqz 0x801E6158` and `bnez 0x801E6168`, and their **not-taken** edges are the tail's only entry - which matters because the second gate reads the byte `0x801E6210` / `0x801E6214` increments. The LATCHED verdict on the block stands. |
| `FUN_801D65F8` is a positional SFX helper | falsified (it is a VRAM blit) | Plausible: it takes a small packed argument and is called from the Baka Fighter round SM, where a positional cue would fit. It blits a `6 x 0x18` VRAM `RECT` from `(0x340 + (byte0 >> 2), 0x80 + byte1)` to `(0x340, 0x86)` through `MoveImage` - the same error class as `FUN_80058490`: an id-shaped argument into a graphics routine. |
| No dome round raises the magic bit, so magic is *not* forbidden on the Master course | falsified (`FUN_801CEA6C` seeds the word per course) | Plausible: the `0x200` bit's SCUS writers key on the first enemy monster id, the dome ladder tops out at monster `0xAA`, and `FUN_801D0088` writes only the low byte - all true, and not the whole writer set. `FUN_801CEA6C` seeds the word itself - `0x101` / `0x111` / `0x321` over a zero at `0x801CEBA0` / `0x801CEBB4` / `0x801CEBC8` on story flags `0x536` / `0x537` / `0x538`, last match winning - so retail bars Item on every seeded course and crosses out the Ra-Seru chip on the top one. Enumerating one bit's writers is not enumerating the word's. |
| A cast is an AP spend, so the dome pennant's AP budget pays for one | falsified (a cast costs MP) | Plausible: every other dome command is an AP spend, and the pennant geometry is linear in AP cost. The cast path `0x801D1408..0x801D1528` writes `+0x1DF[0]`, `+0x1DE = 2`, `+0x1E7 = 9` and phase `0x46` and touches no AP field; the cost is the spell table's `+3` byte discounted by the record's `+0xF4` bits. |
| A slot-B tick body that sweeps the whole actor row is a never-kill clamp, and only `0x801F85A8` / `0x801F8D64` sweep one | falsified (three more sweep it, and they can kill) | Plausible: the two known whole-row bodies both clamp, so "whole row" and "clamped" look like one property. Three **tick** bodies - `0x801F726C`, `0x801F69EC` and `0x801F69D8` - sweep the whole row with the kill-capable shape A. |
| Past its `sltiu` bound, a capture-class tick body returns `Done` | falsified (it returns **Busy**) | Plausible: an out-of-range phase index looks like a terminal state. The saved register is seeded `1`, and 0949's `beqz` at `0x801F6AA4` lands *past* the `move s7,zero` that would clear it, so retail leaves the body busy. The port returns `Done` on all eleven bodies as a disclosed anti-softlock divergence. |
| PROT 0955's four bodies are the band's only stat-block writers | falsified (eight images write the block) | Plausible: 0955 is the module whose whole choreography is buffs, so it looked like the band's designated stat writer. Also writing `+0x150..+0x16D`: 0940 (`0x801F78B8`), 0942 (`0x801F7D34`), 0943 (`0x801F69D8`), 0945 (`0x801F69F8`), 0954 (`0x801F6A58`), and 0925 / 0956 at `+0x16C` only. |
| `0x801E6218` is an unported multi-cast sweep | falsified (it is latched, and already ported) | Plausible: a documented sentence named the address as remaining work, the shape a worklist row has. It is a latched arm the port implements as `battle_action::done`, and a five-form reference sweep over 84 images finds nothing that reaches the address. |
| The eleven player-Seru arms off `0x801CF4EC` are data stagers | falsified (they are tick bodies) | Plausible: the cast-module page's per-entry verdicts say "data" for PROT 0903..0913 - but those verdicts describe each module's **stager**, and the `0x801CF4EC` table's arms are a different routine in the same image. They are 3396..7260-byte `ctx+0x279` phase machines with damage wrappers; ports `cast_seru_ticks_a` (0903..0908) and `cast_seru_ticks_b` (0909..0913). |
| The battle AI's companion pick reads the acting actor | falsified (it reads seat 0 and writes seat 1) | Plausible: every other per-actor routine in the file takes the actor it acts for. `FUN_801EED1C`'s character-id-4 arm reads `actor_table[0]` and writes `actor_table[1]`, so a table built around the acting actor describes the wrong seat's stats. |
| Selectors `0x10..=0x83` of the battle applier are a stat-up / status-clear / queue-end / item family | falsified (116 of them are the epilogue) | Plausible: a 132-slot jump table with decoded arms at the bottom reads as a large opcode space with an undecoded upper half. The table at `0x80014FA0` has 15 distinct targets, and 116 slots point at the shared epilogue `0x800421A8`. The only body above `0x0E` is slot `0x82` (`0x800421A0`), the Incense window top-up (see the Incense row under field / locomotion). |
| A learned skill is inserted at the head of the displayed list | falsified (both list writers insert in order) | Plausible: a head insert is the cheap implementation and would explain "newest first" in a menu. The party-slot learned-Arts list (`+0x74D`, selector `0x0B`) and the displayed-skill list (`+0x186`, arm `0x80041FB4`) both walk to an ascending position first. |
| `FUN_80043264` scans all eight equipment slots | falsified (three) | Plausible: the routine sits beside the equip helpers and the block it indexes is eight slots wide. Its counter starts at `li v1,0x5` and runs while `slti v1,0x8`, so it reads `char +0x19B..+0x19D` - the accessory ("Goods") slots only. |
| `0x801F7D34` is a body of PROT 0943 (`cast_curse`) | falsified (PROT **0942**, `cast_power_up`) | Plausible: the dump is named `overlay_cast_curse_0943_801f7d34`. The bytes are 0942's: an 876-byte four-arm tick for action `0x52` that writes the caster's `+0x156` AGL base as `record[+0x0E] * 3/2`. A dump filename prefix is not evidence of which image a routine belongs to. |
| SCUS's `jal 0x801F7B88` runs only when a battle ends mid-cast | falsified (it runs on ordinary in-battle frames) | Plausible: the call sits in a teardown-shaped neighbourhood, and no corpus state has its gate up. The arm requires `_DAT_8007BD71 == 0xFF` - battle **running**; `0xFE` is the ending state - and fires per frame while PROT 0920's effect budget is non-zero (123 hits on one victory ladder, all *before* the end signal). |
| The battle exit (the `0x66` escape teardown / the results sequencer's `+336` frame) is a **white-out** | falsified (a fade to black) | Plausible: the fade template ramps black -> white, and the ramp's end colour reads as the screen's. The template is kind `2`, and the fade actor's tick passes the kind as `FUN_80024EE4`'s second argument, which the emitter folds into the draw-mode packet's ABR bits (`sll a3,a1,0x5; ori a3,a3,0xe` at `0x80024FB0`) - the law the intro styles obey (`abr == 1` white-out, `abr == 2` to black). Kind 2 is `B - F`, so a rising ramp subtracts more each frame and the scene, result windows included, darkens to black. |
| Retail plays a victory **fanfare BGM** when a battle is won | falsified (the battle theme runs through the results; the only jingle is cue `0x50` on a level-up) | Plausible: `FUN_8003CE08(0x35)` at `0x8004EECC` in `FUN_8004E568`'s results frame looks like a music start. It is the story-flag setter (`DAT_80085758[idx >> 3] \|= 0x80 >> (idx & 7)`). The frame's only cue is `FUN_8004FCC8(0x50)` behind the level-up test; the hero's "fanfare" is a `monster.snd` voice clip streamed into slot 7. |
| The member who landed the killing blow strikes the victory pose | falsified (the leader poses) | The pose actor is `ctx[+0x13]`; every store to it in the battle overlay is a round-boundary zero or the magic menu's MP-cost scratch, and the three-member `noa_levelup_banner` capture reads `0` with seat 0 posing while Noa levelled. |
| A party melee's whole sound is the `XA27` channel-4 sting (`0x10C`) | falsified (an ordinary swing is the `XA30` grunt; the sting needs `_DAT_8007BD84 != 0`) | Plausible: the sting is the loudest cue on a strike, and the decompiled C flattens the fork into "grunt first, sting dropped behind its CD read". `FUN_801EC3E4` selects **one** of the two on `_DAT_8007BD84`: zero takes the grunt at `0x801EEB44` and the re-read at `0x801EEB60` then skips the cue; non-zero branches over the grunt into the `0x10C` path (`bne v0,zero,0x801EEB70` at `0x801EEAC8`). The word is an effect handle, zeroed by the battle-start sweep and the round reset and set only by PROT 0940's stager ([details ↓](#_dat_8007bd84-is-a-mode-word-the-melee-kernel-branches-on)). |
| Move-VM op `0x2F` extension dispatcher - per-overlay copies? | falsified (one copy, field overlay 0897 only) | Plausible: the call VA is fixed, and every slot-A overlay loads there. Every other mapped slot-A overlay and the title overlay carry unrelated bytes at the call VA and no JT at `0x801CE868`, so op `0x2F` runs only while 0897 is resident and battle-side move records cannot use it. The capture-derived `_801d362c` dumps (world-map / dialog / cutscene labels) are identical to each other; the `0897` static dump is a strict subset of them (Ghidra does not follow the JT flow). See [move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#overlay-residency---one-copy-in-the-field-overlay-only). |
| "`FUN_801F3894` spirit/magic damage roll" (state-`0x3D` chain caller) | falsified (VA-aliased dump) | Plausible: a dump exists at that entry and its body is a damage roll. The `overlay_0897_801f3894` dump is `FUN_801DD0AC` byte-for-byte under a double VA shift. The real state-`0x3D` callee `FUN_801F3990` is a cast **audio-cue dispatcher**; spirit damage is state `0x3E`'s inline formula. `801Exxxx` dumps are suspect too, not just the `0x801F` band: `801f0348` and `801e23ec` are aliased (the latter's reading drops all three initiative modifier terms); `0x801F1ED4` holds (verified from the 0898 image); `0x801F45A4` is unverified. See [battle-formulas.md](../subsystems/battle-formulas.md#initiative-key-seeding-fun_801da780). |
| A level-up **refills** the live HP / MP pools (the captures' "settle" phase at `+0x106` / `+0x10A`) | falsified (the settle write is the battle-end resync; both currents stand still) | [details ↓](#a-level-up-is-not-a-heal) |
| A streamed signature-attack cast module cannot run from a party slot because "a party actor has no monster block" | falsified (it has a first-class equivalent) | `FUN_8004AD80` resolves the staged raw anim index down two arms, and the party one (`DAT_801C9360[slot]`) carries the indices PROT 960 stages. The module's only monster-block touch is a hardcoded **seat-0** write unrelated to the caster. [details ↓](#the-cast-module-blocker-was-named-wrong) |
| An art whose attack camera films flat has the wrong **arm**, so the fix is to select a better-choreographed one | falsified (every arm is timed for a ~20-frame swing, and not one is spare) | [details ↓](#the-attack-camera-was-never-an-arm-choice) |
| `FUN_801D5854` case 6's `0x801D5CFC` arm is the per-action **party** framing, gated on `DAT_8007BD71 == 0xFE` as "the in-battle state" | falsified (`0xFE` is the battle-END signal; a running fight takes the `0x801D64C4` arm for everyone) | [details ↓](#the-case-6-party-arm-is-the-battle-over-framing) |
| The Done band (`0x50` / `0x51`) is idle for the camera - keep it on the far framing so the "per-action close-up" does not own half the fight | falsified (retail re-arms case `6` / `8` per category there; the close-up was the wrong arm) | [details ↓](#the-done-band-is-not-idle) |
| Navmesh / per-scene navigation data | falsified | `0x80108EA4..0x80109550` is per-scene GPU primitive scratch, not a 24-byte stride navmesh. Pointer hunts find zero RAM cells pointing into the window. Real per-scene region / collision / event-trigger data lives in the field-file preamble (a count + `u16` offset table + records - **not** the scene texture pack at block `+4` (the former "field-pack schema", which is an `asset::pack` of TIMs); see [field-pack](../formats/field-pack.md)); the collision grid is the `+0x4000` MAP region; the encounter-record path lives at `actor[+0x94]`. |
| Op-`0x4E` sub-ops 4..8 "absolute jump" / "rand -> next PC" readings | falsified (all sub-ops 0..9 are the 7-byte compare-and-skip) | [details ↓](#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare) |
| `801d58f0` / `801d63b0` as single shared port blockers | falsified (VA-aliasing artifact) | Plausible: a bare-VA catalog key aggregates every overlay's references into two top blockers. The two addresses host different code in different overlays (byte-verified: 80/228/124/308/1 B and 208/1036 B across 0897/baka/cutscene/debug-menu/fishing/slot/dance). They are tracked per overlay as `overlay_<label>_<addr>` identities; catalog ignore category `va_aliased_overlay_local`. |
| A monster's after-image ghosts (`FUN_80049348`) fire on **any non-idle clip tag** | falsified (the gate is two record bytes, `+0x77` and `+0x87`) | Plausible: the party gate is committed slot `>= 0x11` ("an art is playing"), so "ring id = clip tag + 0x10" reads as the same idea. The anim tick stamps `record[+0x77] + 0x10` (`+0x87 == 1` forces `0x11`); `+0x77` is the attach-key byte, zero on almost every monster entry, and no idle entry on the disc qualifies. Gating on "a clip is staged" ghosts every monster through its approach walk and idle loop. When retail reads a record byte, port the byte, not the state that usually accompanies it ([battle-action.md](../subsystems/battle-action.md#the-after-image-ghost-walk-fun_80049348)). |
| Charm battle softlock = unbounded reroll in `FUN_801E7320` | falsified (cannot spin from any reachable state) | Plausible: the reroll loops are unbounded in isolation. Every reachable caller state has an exit: the scheduler `FUN_801DABA4` never seeds a dead actor (predicate `+0x14C != 0 && !(+0x16E & 0x4)`), the acting `0x380` monster is itself an in-band self-pick exit (`0x801E73E8` clears `+0x1DE`), and a band with zero living members means the previous `0x5A` already fired the wipe. The defect is downstream, in the `0x5A` victory arm's roster indexing ([battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)). |
| Gaza 2 `0x51` park: clamp asymmetry as a standalone retail generator | falsified (amplifier only; its exhibit was a phased mid-action state) | [details ↓](#gaza-2-0x51-park---the-two-falsified-generators) |
| Gaza 2 `0x51` park: the Final Heal revive lands "at the worst possible moment" (mid-drain) | falsified on the Gaza 2 move set (12/12 revives found the accumulator already drained) | [details ↓](#gaza-2-0x51-park---the-two-falsified-generators) |
| Muscle Dome as a **card battle** with a per-fighter "score out of 108" | falsified (it is a 4-turn battle; the readout is the opponent's HP percentage) | [details ↓](#muscle-dome-was-never-a-card-battle) |
| Muscle Dome awards a **Seru** on a win | falsified (a leg pays nothing; a contest pays casino coins) | [details ↓](#the-dome-victory-caption-is-not-a-prize) |
| `FUN_801DBC30` blits the party panels' name plate | falsified (its page + CLUT resolve to the `etim` red cross-out X) | [details ↓](#fun_801dbc30-is-not-the-battle-name-plate) |
| The retail party HUD carries HP / MP gauge bars | falsified (no bar primitive in either readout's packet run) | [details](../subsystems/battle.md#the-party-status-readout---and-it-has-no-gauge) |
| Screen-element kinds named by what sits at their seat (`0x32`/`0x33` = "the roster panels") | falsified (naming by seat named the wrong record) | [details ↓](#a-kind-named-by-its-seat-can-name-the-wrong-record) |
| The battle message banner (and every class-0 frame) has no interior fill | falsified (a `POLY_GT4` marbled fill covers the frame; only a `SPRT`-only sweep misses it) | [details ↓](#the-battle-message-banner-has-no-interior-fill) |
| `FUN_801E2524` / `FUN_801E2650` are a full-screen flash / fade ramp | falsified (they are the **Arts announcement banner**) | [details ↓](#the-flash-ramp-is-the-arts-announcement-banner) |
| The battle per-actor draw `FUN_80048A08` runs **35-64x per frame** during a summon | falsified (once per live actor per rendered frame) | [details ↓](#the-summon-draw-runs-35-64-times-a-frame) |
| The slot-B cast band applies damage with **one** shape, seat-0 hardcoded | falsified (true of PROT 0958 / 0959 / 0960 only) | Plausible: the seat-0 write is the whole damage path in the three Delilas modules. The band splits: the capture-class ticks read the caster's own `+0x1DF` through the `0x801CF56C` trampoline and clamp per victim, and two images apply damage to a **row** of seats. The rule is per-module, not per-band - [cast-module.md](../subsystems/cast-module.md#the-seat-0-hardcode-and-where-it-does-not-hold). |
| PROT 0927 can never kill, so it needs no death path | falsified (it is a stager; its tick is the killer) | Plausible: nothing in 0927's own image subtracts HP. That image is a multi-seat **stager** - it seats the enemy row from `ctx[+1]` and stages clips. The damage lands in the tick it stages: `0x801F6A84` clamps the subtraction with `sltu`, like every other capture-class tick. A stager's image is not the whole spell - [cast-module.md](../subsystems/cast-module.md#the-two-aoe-sweeps). |
| `ctx[+0xD]` variant `2` stamps a `0x400` camera **roll** | falsified (it is the translation `TR.y`, not a rotation) | Plausible because variant `1` is a `0x800` yaw and a per-action camera that yaws would naturally also roll. The byte is two independent bits, not an enum: bit 0 adds `0x800` of yaw and bit 1 adds `0x80` of pitch **and** drops `TR.y` by `0x100`. Nothing in the arm writes a Z angle. See [`battle-action.md`](../subsystems/battle-action.md#the-three-movers). |
| The Spirit **halving** flag is a battle-actor `+0x16E` bit | falsified (it is the character record's `+0xF8` bit `0x800`) | `+0x16E` is where every other per-battle affliction bit lives, so a "Spirit is halved" bit reads as belonging there. It does not: the halving is passive `0x2B` (*AP Used Down*), an accessory bit in the persistent per-character ability bitfield at record `+0xF8`, tested at `0x801EF364` in the queue builder and again by the status panel at `0x801D4520`. A per-battle bit could not survive the save, and this one does. |
| A dome direction swing takes its damage from `FUN_801E09F8` -> `FUN_801DD0AC` | falsified (that chain carries a move-power **index**, and the dome's rows are zero) | The chain is real and it is the monster-special damage path, so a dome swing entering it looks like the answer. What travels is an index into the move-power table, and the dome's four direction commands map `0x0C..0x0F` to row `0`, whose power bytes are zero - the chain would deal nothing. The dome resolves its own exchange; see [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-dd0ac-chain-is-not-a-direction-swings). |
| `^H` is an exception to the element-badge caret bijection (Cort has no badge letter) | falsified (the map is a zero-exception bijection) | The census that produced the exception was over shipped monster **names**, and no shipped name happens to carry `^H`; absence from the corpus read as absence from the encoding. The escape decoder has no special case: badge index = `letter - 'A'` for the whole `0x8B..=0x92` strip, and element -> caret is the fixed permutation `[4, 3, 0, 2, 1, 5, 6, 7]`. Parser `MonsterRecord::plaque_badge`. |
| `battle_gimard_tail_fire_a/_b` are frames of a **party** Seru summon | falsified (the enemy Gimard's Tail Fire) | The acting-actor plaque top-left reads `Gimard`, the pill readout is Vahn at 154/180 after `DAMAGE 16`, and the states' loader-B id is `5` (PROT 0900, the move-FX module), not a stager. A party summon draws no label (and no readout while its seats are hidden); its frames are the `*_summon_mid_cast` states. Reading these two as the player-summon reference put the enemy's chrome on the player's cast. |
| `FUN_801DBF9C` (the `0x29` party trigger) applies the spell's outcome | falsified (it stages the anim stream and the summon sub-route) | No store in it reaches HP, MP or a target; it writes `+0x1E0..+0x1E2` (`9`, `0x12`, `0xFF`) for any id `>= 0x25` and copies an overlay anim-pair list below that. The outcome is the streamed module's - the summon stager's strike for a Seru id. [details](re-settled-threads/battle.md#the-party-cast-trigger-is-a-params-stager) |
| `FUN_801DC0A0(actor, id)` stages the cast clip | falsified (it is the cast-effect driver) | The summon band calls it with `0x12` every frame of `0x33` / `0x34` while the caster's `+0x1D9` reads `9` (`gimard_summon_start`); the clip stage is the SM's own `+0x1DA` store at `0x29` / `0x2A`. |
| "The port never latches the art id, so `actor[+0x1DB]` reads `0x00` all fight" | falsified (it latches `0x01`, then `0x0D`, then `0x0C`) | Those are the rolled arm swings a basic Attack queues, and a retail mid-swing Attack state reads the same band. `0x1A..=0x2D` - the per-art camera's band - is reached by action-constant queue bytes, i.e. an arts chain, so the camera not arming on two arm swings is retail. [details](../subsystems/battle-action.md#three-readings-the-port-already-satisfied) |
| "The port's `0x51` Done-band residency is unbounded" | falsified (bounded at `ctx[+0x6D8] = 0x3C`, as retail) | The 60-70 frames a sample shows is **one** action's countdown plus the HP-bar settle freeze; a multi-action sample sums several of them. Counting frames in a band without splitting them by action reads a per-action budget as a park. [details](../subsystems/battle-action.md#three-readings-the-port-already-satisfied) |
| "The sparring tutorial reopens the command session, so the cursor cannot move" | falsified (the cursor walks `0..5`; a *waiting* prompt box parks the tick) | The session is reopened only on a rejected resolution. What pins the cursor on screen is retail's own `ctx[+0x6B2]` box guard, which the port reproduces. [details](../subsystems/battle-action.md#three-readings-the-port-already-satisfied) |
| An action returns its combatants to their authored formation seats | falsified (retail leaves them on the ground the action ended on) | Plausible: the formation is authored, so a home seat reads as the rest position. Two library states of one solo fight read the authored formation 1600 apart; two later ones read the same pair ~300 apart and both far off it, with every actor's `+0x3C`/`+0x40` pair within ~110 units of its live `+0x34`/`+0x38`. The port commits the seat from the live pair at `DoneCleanup`. [details](../subsystems/battle-action.md#where-an-action-leaves-its-combatants) |
| Staged ids `0x10` and `0x1A` **alone** install at dynamic slot `0x11`, every other art-bank id at `0x10` | falsified (`0x10`, `0x1A` and every art constant `>= 0x1B` install at `0x11`; only the base ids `0x11..=0x19` take `0x10`) | [details ↓](#the-dynamic-slot-rewrite-was-never-0x10-and-0x1a-only) |
| PROT 0910 writes no HP, because its tick body carries no damage wrapper | falsified (the wrapper is in its callee) | Plausible: the tick's own extent holds no `jal` into the damage family and no `+0x14C` store. `FUN_801F81DC` is the applier - three `jal` sites in the tick, the wrapper call `li a0,0x12` / `li a1,7` / `jal 0x801DD0AC` at `0x801F8874`, and the HP store at `0x801F8910`. A function extent bounds the measurement, not the module. |
| A module's phase stores are its `sb` writes at a literal `0x279` displacement | falsified (most store through a formed pointer) | Plausible: the phase byte is `ctx + 0x279`, and a census keyed on the displacement is exact wherever the base is the incoming argument. Most modules form the context pointer once into a saved register and store through that: PROT 0908 reads as **zero** phase stores this way and has six. Counting both forms, the eleven player-Seru bodies run 3..10 stores each. |
| The cast band uses two damage-clamp shapes, and every tick body takes the kill-capable one | falsified (three shapes; seven tick sites take the third) | Plausible: a census that windows each `jal` finds two shapes, differing in whether the clamp precedes or follows the store. A third caps at `HP - 1` with an *unsigned* compare - neither killing nor healing - and all seven of its sites are tick bodies. Two of them park the roll in a saved register more than a hundred instructions before the apply, outside any window. |
| The wrapper's return is the damage a module applies | falsified for PROT 0910 (it is shifted first) | Plausible: every other body in the band stores the wrapper's return through its clamp unchanged. `srl s1,s1,2` at `0x801F8898` rewrites the register the clamp and the stores use, so the applied figure is `wrapper_return >> 2` - a live capture reads 427 from the wrapper and 106 into the victim. |
| Nighto's resist roll forks the module's phase | falsified (only the kill roll does) | Plausible: the arm rolls twice and ends in two different phases, so pairing the second roll with the second phase is the economical reading. The `beqz` at `0x801F7E04` tests the **kill** roll `0x801F8534`; the confuse leg's `sb 0xF,0x279(...)` at `0x801F7E28` is unconditional. `0x801F853C` only suppresses the victim writes, so a resisted cast still advances to phase 15 - captured twice as `13 -> 15`. |
| Monster record `+0x20` is a per-monster instant-death immunity byte | falsified (it is a double-width texture-page flag) | Plausible: three summon ticks read it before an instant-death roll and force the resist when it is set, and 37 of 186 records carry it - bosses plus the Evil Fly / Death Wings / Demon Fly family. Its primary reader is the model upload at `0x801F1D0C`, which widens the VRAM rect from `0x20` to `0x40` through `FUN_80055468`; the ticks borrow it as a "big model" proxy. A field's meaning is its primary reader's, not its most interesting reader's. |
| PROT 0941's `0x51` Steal deals damage like its band neighbours | falsified (zero damage wrappers) | Plausible: the band's arms are overwhelmingly damage bodies, and an enemy Steal that also hurts is an ordinary design. Its outcome is an inventory consume through `FUN_80042310` - against a party victim by rejection sampling over the 256-slot bag at `0x80085958`, against a monster victim by `rand() % 100` versus `0x80077828 + id*2`, the table the player's Steal rolls on. |
| PROT 0943's MP-pair writer is the routine at `0x801F69D8` | falsified (that VA is the body's head table) | Plausible: `0x801F69D8` is the slot-B load base, six other images really do put a tick body there, and a table of code pointers disassembles. In 0943 the base holds the `0xB5` body's **head table**; the writer is the body at `0x801F6A04`. Head tables are not pinned to the base either - 0943's `0x40` and 0944's `0x53` read `0x801F69F0`, 0950's `0x5A` reads `0x801F6A10`. |
| PROT 0940's `0xAC` arm blanks the caster's `+0x0C` | falsified (it blanks seat 3's action queue) | Plausible: `s0` holds the caster on entry and the stores are a short run of zeros. `s0` is reassigned to `0x801C9370` at `0x801F7648`; the blanked bytes are `actor_table[3]`'s `+0x1EF..+0x1F3`. A backward-only scan for the base misses a reused register. |
| The arena's pre-test seed of `0x8007BAC0` is zero | falsified (it is `1`) | Plausible: the word is zero before the arena runs and the seeding arms fire only on a set story flag, so the unflagged path reads as "leave it alone". `sw $s2` at `0x801CEB8C` stores `1`, with `$s2` loaded 43 instructions and three `jal`s earlier. That is course 0 with no bans, not "no seed"; "every seed carries `0x100`" holds for the three flagged seeds only. |
| The dome tally screen draws its four lanes, then the totals | falsified (the HP accumulator sits between lanes 2 and 3) | Plausible: a scoreboard that lists its lanes in order and totals underneath is the expected shape. Retail's six rows are `[lane0, lane1, lane2, the HP accumulator 0x801D1AC8, lane3, the running tally 0x80084440]`, at brightnesses `[0, 1, 2, 0, 3, 3]` - four steps, not six. `FUN_801D1184` re-forms the `0x801D` base into a different register between the product and the store, which is what mis-attributes the lanes. |
| The player-Seru wrapper sites pass the caster seat in `a1` | falsified for the player half (`a1` is a baked `7`) | Plausible: the capture half does load it - `lbu a1,0x13(...)` - and one rule for both halves is the tidy reading. The player-half sites bake the literal: `addiu a1,zero,7` at `0x801F74A8` and `0x801F8880`. The summon always occupies seat 7 on that half, so constant and field agree in retail and disagree once a port seats a summon elsewhere. |
| PROT 0941's Steal floors its bag draw while the battle context's `+0x11` reads 4 | falsified (the gate is `DAT_8007BD10[1]`) | Plausible: `$s5` is the battle-context pointer in most of the band, so `lbu v1, 1($s5)` reads as a context field. In this module `$s5` is formed at `0x801F77B0` as `0x8007BD10`, the present-party list, so the gate at `0x801F77E8` is `DAT_8007BD10[1] == 4` - **battle seat 1 holding roster character 4**, the split-bag condition. PROT 0941 makes no access at `+0x11`; its context pointer is `*(0x8007BD24)`. Read the register's own formation, not its usual meaning. |
| `0x8007B83C` is `FUN_8001E890`'s `== 2` gate | falsified (that word is the game mode) | Plausible: the routine does compare a word against 2, game mode 2 is the scene-load mode, and the mode word takes that value across a load at the right moment. The compared word is `gp+0x6AC` = `0x8007B9C4`, the pack's own load-state; `0x8007B83C` is `gp+0x524`. The three-state reading of the arm depends on which word it is. |
| PROT 0943's `0x40` and PROT 0944's `0x53` fault before their first tick | falsified (the body ticks; the fault is downstream) | Plausible: the emulator pauses on an unmapped read with no module frame on the stack, in every post-turn state of the corpus. Both arms stage clip `0x0B`, and SCUS's anim commit indexes the **caster's** spell-entry array with it, so a caster with ten entries reads its own name text as a pointer. Logging the access instead of pausing on it shows both bodies walking all five arms. |
| Battle context `+0x276` is a per-module gate | falsified (it is the side-band applier's stage) | Plausible: the summon modules test it before their head cue. Its writers are the applier SM and two battle routines; the modules only poll it, so it is open by construction when a cast wants it. Nothing on the disc writes it from a tutorial flag, and a port that does silences the melee sting in exactly those battles. |
| The SPU's slow cast-voice release is an envelope defect | falsified (it is the reverb tail) | Plausible: a one-second decay after key-off is what a wrong ADSR shift sounds like. The envelope is tick-exact - shift `0` linear is `-0x4000` per tick, pinned against an independent model for shifts 0, 15 and exponential - and the dry SPU is silent four samples after key-off. The tail is the room: every voice is routed through a retail reverb preset the resampler installs. |
| `0x1F80037D` is a second per-frame byte | falsified (it is the game-speed **rate** scalar) | Plausible: the mode-INIT core reset reloads it in the same breath as `0x1F800393`, and at a steady 60 fps both bytes sit at small constants. `0x80055FBC` writes it the literal 8 and nothing touches it per frame; `0x1F800393` is written every frame by the pacer, which picks 1 to 4 off elapsed time. Elapsed time is their **product**, so a countdown read against either byte alone reads as a constant multiple of the other. |
| `FUN_801DD4B0` is a damage path of its own | falsified (it is `FUN_801DD0AC`'s non-summon arm, term for term) | Plausible: it is a separate entry with its own frame. Its arithmetic matches the shared kernel's non-summon arm instruction for instruction - driving both against one seat returns 251 against 251, so no capture separates them. `FUN_801DD6B4` is the one that differs: a physical-stat kernel that bypasses defence. |
| "Move `0x36`" and "move `0x37`" are move-VM sub-opcodes | falsified (they are `actor[+0x1DF]` action ids) | Plausible: the move VM has a dense opcode space and both numbers fall inside it. They are entries in the battle action queue, a different id space with a different dispatcher; the move VM's arms at those numbers are unrelated. |
| `_DAT_8007B64A` has no writer | falsified (the field entity tick writes it) | Plausible: an absolute-address sweep over `SCUS_942.54` and every overlay image really does find nothing, and that sweep is the instrument these questions are usually settled with. Every access to the byte is `gp`-relative, a form the word scan cannot see; the `gp`-relative sibling finds 14. `FUN_801DA51C` clears it at `0x801DA69C` and raises `1` at `0x801DA6A8` off system flag `0x19`, and battle latches `3` at `0x801E6D2C` ([settled](re-settled-threads/battle.md)). |
| The arm at `0x801E3DD8` sets `ctx[7] = 0x3E`, and a spell can reach it | falsified (the arm **is** state `0x3D`, and only an item opens it) | Plausible: the arm's own exit store is the next state, so reading the store as the arm's identity is off by one. The jump table settles it: base `0x801CED44`, the word holding the arm at `0x801CEE38`, index `0x3D`. The band is not spell-reachable: the Magic category arm sets the predecessor state only for spell ids below `0x65`, which the player Seru block `0x81..0x8B` fails, while the Item arm sets it unconditionally. Fifteen injected casts reach the arm zero times. |
| The battle selectable scans test an action-state byte where `4` means removed or done | falsified (it is a seat index) | Plausible: the scans sit beside the action state machine, run per seat, and compare a per-seat byte against a small constant - a state enum by shape, with `4 = removed / done` a state nothing writes. `DAT_8007BD10` holds the per-slot roster character id: `FUN_801DA34C` indexes it and subtracts one to reach a character record. `4` is the AI-companion seat, so the term excludes a seat the player does not command ([settled](re-settled-threads/battle.md)). |
| `FUN_801D0748` is the Muscle Dome's match state machine | falsified (it is the round SM **every** battle runs) | Plausible: every capture that catches it is a dome one, five differently-prefixed dumps of it carry the dome label, and its arms are the arms a dome round needs. The routine has exactly one `jal` disc-wide - `0x80047014` in the SCUS battle frame driver `FUN_80046A20`, with no test in front of it - so every battle frame steps it, and three non-dome battle states enter it hundreds of times over 700 vsyncs each. A routine named from the contexts of its captures is named after its callers ([settled](re-settled-threads/battle.md)). |
| No draw site exists for the dome panel still at VRAM `(384, 0)` | falsified (the emitter never materialises `384`) | Plausible: 33 sites disc-wide form `0x180`, none paired with `y = 0`. A textured primitive addresses VRAM through the packed `tpage` halfword, where the x coordinate is a **page index** - `384 / 64 = 6` - so the emitter's constants are `0x106` and `0x109`, and no search for the pixel column can hit it. It is `FUN_801D00F8` in the contest hub PROT 0977, an image that is not resident when the upload runs ([settled](re-settled-threads/battle.md)). |
| `0x801F90DC` is retail-unreachable, so its port owes no host | falsified (five slot-B images reference it) | Plausible: the address sits in a group of wiring rows that are retail-unreachable. Two `lui` materialisations and three branches reach it across PROT `0913` / `0927` / `0928` / `0934` / `0951`. What blocks the port is image **ownership** - which module the code belongs to - not reachability. |
| The Arts banner selector `ctx[+0x28B]` is raised in the battle overlay, and an overlay sweep finding no writer means there is none | falsified (all four writers are in SCUS `FUN_8004AD80`) | Plausible: the banner's reader `FUN_801E2524` and everything around it are overlay code, so the overlay is where a raiser is looked for. Every `sb ...,0x28b` on the disc is at `0x8004ADDC` / `0x8004B774` / `0x8004B80C` / `0x8004B87C`. "Unfound in X" is a statement about where the search ran, not "no raiser". |
| The three `ctx[+0x28B]` raises are alternatives - one per starter kind | falsified (they run in sequence; the side-array pick overwrites the seat flag's write) | Plausible: three stores of three constants in three arms of one routine read as a selection. `0x8004B774` stores `3` and **falls through** to `0x8004B7D8`, which reads the queue-builder side array `0x801F6990` at `0x8004B804` and stores over it at `0x8004B80C`. |
| `FUN_801D84C0` builds the four battle party-name panel labels, measuring each with `FUN_8003CBF8` | falsified (it builds the four **battle-result messages**; `FUN_8003CBF8(buf, 0xC1, 1)` locates the name escape) | Plausible: the four buffers are text, one per display slot, and the routine pairs with a panel opener. Resolving the pool strings its two arms copy and append (`0x801F4C38..0x801F4CC4`) gives a victory line with spoils, a defeat line and the two escape outcomes; the `0xC1` call returns an offset the roster arm then patches with a participant id, not a width. Every patch reads the **first** seat, so a per-seat reading of the caption ids is wrong too. |
| `FUN_801D32BC` is a turn-order choice - retail's cursor order over initiative | falsified (it is the command window's **member cursor**) | Plausible: it steps a seat index over living actors, which is what a turn order does. Its six call sites are the round reset `0x801D8910` and `FUN_801D388C`'s cases `0x10`, `0x11`, `0x21` and a tail pair - command input. Initiative is the execution order; the command order is the slot scan. Port `battle_cursor_pose` (both directions; the backward step drives `world::battle::member_step`). |
| The six `baka_fighter_chrome` NOT WIRED anchors are the duel's digit strips | falsified (they are the keyframe-editor band and sprite passes) | Plausible: the digit strips were the visible gap and the cluster sits in the same image. The anchors are `anim_slot_install` / `delete` over `DAT_801DBF44`, `impact_effect_pair` (`FUN_801D4DF8`), `sprite_blit` (`FUN_801D65F8`, a `MoveImage`), `mirrored_sprite_pass` (`FUN_801D49E8`) and `editor_tick` (`FUN_801D4FC8`); they need an eight-slot key array, per-action keyframe TRS and an actor pool, not digit art. |
| ... and the six-anchor row's "sprite passes" | falsified (they are model passes) | Plausible: the anchor names say "sprite" and the rows sit beside `MoveImage` blits. `FUN_801D49E8` is the afterimage of a 3D fighter and `FUN_801D6310` a 3D walk-on. The editor rows are unreachable on a retail disc and tagged `REPLACED-BY` ([`minigame-baka-fighter.md`](../subsystems/minigame-baka-fighter.md#the-developer-keyframe-editor)). |
| A clobber of the resident PROT 0874 pack trips `FUN_8001E890`'s checksum | falsified (the sum is over a VRAM read-back, not RAM) | Plausible: the routine re-sums the container and reloads on a mismatch, so corrupting the container looks like the way to fire it. Four XOR'd words of the resident copy left the sum byte-identical (`0x7BF74962`): the words summed come back from VRAM through `StoreImage` (`0x8005842C`). Only VRAM at `(0x180 + 0x40i, 0)` or the boot sum at `gp+0x6B8` can break it. |
| A monster drop is granted per dead enemy at the record's chance | falsified (at most one item per battle) | Plausible: each record carries its own item and chance byte, so a per-record roll is the natural reading. `FUN_8004E568` rolls every seat but keeps only the **last** winner's item, a zero one included, and then clears the drop on `rand() & 3 != 0` unless a seat reads 100 or Items Up applies - a lone 10% enemy drops 2.5% of the time ([`battle-formulas.md`](../subsystems/battle-formulas.md#the-victory-drop-roll), [settled](re-settled-threads/battle.md)). |
| `FUN_801E91E8` is the Miracle-command token lookup | falsified (it is the already-learned-Seru check) | Plausible: it returns a 1-based position in a per-character byte list gated on `ctx[+0x25F + slot]`, which is what a command-token search looks like. The list it scans is the learned-spell list at `+0x704` that `FUN_801E92DC` prepends to, compared as `id - 0x80`; the gate byte is the Ra-Seru marker; and its one caller is the killing-blow Seru absorb in `FUN_801EC3E4` ([settled](re-settled-threads/battle.md)). |
| A set `ctx[+0x269]` routes the Done band into the capture cinematic `0x68..0x6B` | falsified (it is the absorbed Seru, taught in the same action) | Plausible: the byte is set only when a Seru is involved and the capture states sit next door. The Done band reads it at `0x801E6224` and calls `FUN_801E92DC` at `0x801E6234` to teach it; nothing branches to `0x68` on it. The absorb roll reads record `+0x3E` / `+0x3F`; the port's field is `absorbed_seru` ([settled](re-settled-threads/battle.md)). |
| State `0x3E`'s arm at `0x801E3F2C` is a Spirit arm | falsified (it is the class-5 gauge-extension item's arm) | Plausible: it bumps the spirit gauge and sits in the state the table labels "Spirit - fire". The arm is taken on committed effect class `5` (`0x801E3E80..0x801E3E88`), the item class that extends the gauge for a battle, and what it stages is `min(0x120, base * 7 / 5 + 8)` plus a spirit bump; its `(rand() % 2) * 2` camera draw is unconditional, so a port without the arm desynchronises every later battle draw ([settled](re-settled-threads/battle.md)). |
| The battle action SM's writes to `0x8007B790` / `0x8007B792` / `0x800840BC` are a screen shake | falsified (they are the orbit camera's pitch, yaw and eye Y) | Plausible: they are short nudges in the run and spell-exit states, and the failed-run message wobbles the view. The words are the GTE rotation trio's pitch and yaw and the eye trio's Y ([`memory-map.md`](memory-map.md)): the failed run (`0x65`) and the run's Done arm (`0x50`) turn the yaw `2 * step` a pass, the escape backs the eye Z off `32 * step`, and the spell exit (`0x2E`) snaps a camera left pitched past `400` back to pitch `0`, eye Y `0x500` ([`battle-action.md`](../subsystems/battle-action.md#state-table)). |
| `_DAT_8007B888` / `_DAT_8007B840` are Baka Fighter's sprite archives, and `FUN_801D49E8` is a mirrored sprite pass with a yaw at `+0x78` | falsified (ANM clip banks, and the special's afterimage) | Plausible: the rows sit beside `MoveImage` blits and a pair of copies reads as a mirror. The two words are the scene's type-`0x05` / `0x0B` clip banks the clip selector `FUN_800204F8` reads, so every one of these rows drives a 3D model; `+0x78` is a depth-cue level `FUN_8001B964` reads at `0x8001BC7C`, and the two copies are ghosts three and six frames behind the thrower ([settled](re-settled-threads/field.md)). |
| Baka Fighter's round-start cameo is a party model, scene model `0` | falsified (scene model 3, the ring girl) | Plausible: the prototype record's `+0x04` half is `0` on the disc, and `FUN_80020DE0` copies it into the model word. The cabinet init stamps scene-bank base `+ 3` into that half at runtime (`0x801CF2C8..0x801CF2D8`), and a Triangle-held capture sees the spawn store at `0x80020E70` write `3`: PROT 1203's fourth TMD, a ring girl whose cell blit is a wink. The earlier "effect-actor arc" reading of `FUN_801D6310` falls with it ([settled](re-settled-threads/field.md)). |
| Baka Fighter's block `+0x0C` is `0` idle / `1` windup / `2` committed | falsified (strike state: armed / landed / consumed) | Plausible: three values on a move's lifecycle read as its phases. The resolver's arms require `1` before booking an exchange (`0x801D36DC`, `0x801D3730`, `0x801D378C`), the damage kernel writes `2` (`0x801D3EB0`), and a commit zeroes it; `1` means the winner's strike keyframe has been crossed ([settled](re-settled-threads/field.md)). |
| A battle `ui_element(id, 0)` raise is an `efect.dat` script spawn | falsified (it is a HUD screen element) | Plausible: several ids coincide with effect-script ids. `FUN_801D8DE8` never reaches the pool spawner `FUN_801DFDF0`; the id indexes the placement table `0x80076C10`, so raises such as `0x43`, `0x4C`, `0x0F`, `0x52` and `0x59` are HUD elements, not spawns ([`battle-action.md`](../subsystems/battle-action.md)). |
| Retail shows an art-learned text banner | falsified | Plausible: a learn is the kind of event a text line announces. The build loop marks every accepted art (`0x801EF788`) and the learn result only becomes queue byte `0x19` / `0x1A` (`0x801EF6F0`); a `0x1A` commit raises the `NEW ARTS!!` sprite banner. A "Learned X!" string has no retail source. |
| `FUN_801D5778` launches the commit log off-screen at round start | falsified (a ring exit or return launches it) | Plausible: the log disappears around the round's start. The flow tail table at `0x801CE948` calls the copy loops on steps `5 / 7 / 9 / 0x2A / 0x30` (out) and `0x2B / 0x31 / 8` (in); Begin exits without one. |
| The Auto command replays the saved command string | falsified (the Auto flag rebuilds the queue) | Plausible: `FUN_801DA34C` reloads the record's string on the Attack confirm, and the review screen shows it. The fighter's swings come from `FUN_801F0450`'s rebuild under `ctx[+0x266 + seat]` at every round's state `0x00`. |
| The intro banner dedups a monster name by overwriting one glyph | falsified (it drops a character and appends `* N`) | Plausible: the label keeps its visual width. The routine truncates the instance letter and `strcat`s the rodata string at `0x801CECA8` (`0x801D9E60`). |
| Battle action state `0x00` runs once per battle | falsified (every round) | Plausible: the formation arm reads like battle setup. Begin sets flow `0xFE`, whose arm clears `ctx[7]` at `0x801D3224`. |

### The scripted boost profile is "the international release's" profile for every fight

**Tempting reading.** The battle loader `FUN_80054CB0` boosts an enemy's ATK / UDF / LDF / INT as it
installs the record, choosing between two profiles on `ctx[+0x287]`. A live Gaza capture reproduces
the flag-set profile (`ATK x5/4, UDF/LDF x2, INT x9/8`) byte-for-byte and the curated bestiary
matches it for 120+ enemies, so it reads as "the profile the US/PAL build uses" for every fight.

**Why it is wrong.** `ctx[+0x287]` is the **scripted-fight flag** (a formation row's non-zero header
byte). Gaza is a scripted fight, and the bestiary is authored from boss-profile numbers. Every
random-encounter save state carries `+0x287 == 0` and the *other* profile (`ATK x1, UDF/LDF x7/4,
INT x5/4`): a world-map Gobu Gobu with record `17/15/14/10` fights as `17/25/24/12`.

**What is true.** The profile is per fight. `battle_stats()` is the flag-set (scripted) block and
`MonsterRecord::battle_stats_random` the flag-clear one; see
[battle.md](../subsystems/battle.md#monster-record-source-layout).

### `FUN_801F3D3C` installs a queued-magic follow-up routine

**Tempting reading.** `FUN_801F3C34` / `FUN_801F3D3C` are a "queued-magic follow-up" latch: the
installer picks a record out of `0x801F6870` by `[actor class][level band]`, stores its byte `0` as a
follow-up id and its word `1` as a **routine pointer** at `0x800775B4`, and the reader stays silent
while a follow-up is pending.

**Why it is wrong.** The table index is not a class - it is the summon record's **element** byte
(`(*0x801C9358)[+0x1D]`), the byte the affinity scale reads. The word is a **banner string** pointer:
the table's strings name the stat and the percent, and the reader's own install value `0x801CFA20` is
the text "No effect.". The "seven-entry jump table the dump does not cover" is the per-element
base-vs-record compare inside the same function.

**What is true.** Byte `0` is the **percent** the damage finisher's per-element switch shaves off the
target on every hit - the Seru-magic element debuffs. Mechanism:
[battle-formulas.md](../subsystems/battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch).

### The dynamic-slot rewrite was never "`0x10` and `0x1A` only"

**Tempting reading.** The decompiled C of `FUN_8004AD80` shows two `0x11` assignments, which read as
the whole set: staged ids `0x10` and `0x1A` install at dynamic slot `0x11`, every other art-bank id
at `0x10`.

**Why it is wrong.** The slot register `s2` is written in **delay slots**, which the C folds away:

- `_li s2,0x10` under the `0x1A` test at `0x8004B720` is the default;
- `0x8004B76C` (the `0x1A` arm) and `0x8004BB58` (the `0x10` test) set `0x11`;
- the art-constant arm - entered for every staged id `>= 0x1B` at `0x8004BB5C` - sets `0x11` in the
  delay slot of its name-width call (`jal 0x80035f04 ; _li s2,0x11` at `0x8004BBBC..0x8004BBC0`)
  before the install at `0x8004BC4C`.

A live Tri-Somersault capture agrees: `+0x1D9` reads `0x11` under `0x27`, `0x1F` and `0x2B`, and
`0x10` under the `0x19` starter.

**What is true.** `0x10`, `0x1A` and every art constant `>= 0x1B` install at `0x11`; only the base
ids `0x11..=0x19` take `0x10` (`resolve_staged_anim`). The `+0x1D9 == +0x1DA` equality checks compare
slot numbers, so the slot an id lands on decides whether the SM sees its clip as committed.

### A level-up is not a heal

**Tempting reading.** A *multi-level* capture triplet's third frame writes the live current-pool
cells (`+0x106` / `+0x10A`) right after a level-up, which reads as a refill
(`capture_observations::char_level_up`) - every level-up a free full heal.

**Why it is wrong.**

- **Disassembly.** `FUN_801E9504` stores to exactly eleven addresses: the record window's `hp_max` /
  `mp_max`, its six battle stats, the displayed-level byte and its actor-table mirror, and two
  globals. No live-window cell is among them, and the routine's only `jal` is the BIOS `rand`.
  Identical in both dumps of the routine.
- **Capture.** The **single-level** `noa_levelup_*` triplet reads Noa at `164/182` HP and `16/16` MP
  going into the fight and `164/221` / `16/21` once the L2 -> L3 level-up has settled in the field.
  Both maxima move by the growth amount; neither current moves.

**What is true.** The `+0x106` / `+0x10A` write is the battle-end resync of the live pools, not a
grant ([level-up.md](../subsystems/level-up.md)). A write that lands *near* an event is not a write
*by* it; a frame-window capture cannot separate the two, only the routine's store set can.

### The flash ramp is the Arts announcement banner

**Tempting reading.** `FUN_801E2650` scales a percent into grey, replicates it into RGB, picks GP0
`0x2C` or `0x2E`, and emits quads whose extent is driven by a level byte. On the arithmetic alone
that is a full-screen flash / fade overlay walked by a "brightness level".

**Why it is wrong.** The quads are **textured**. Every arm writes texpage `0x27` = `(448, 0)` under
CBA `0x7703`; decoding that page at 4bpp through that sub-palette shows the emitter's three 24-tall
rows are the words `SUPER`, `HYPER`, and `MIRACLE` + `NEW`, directly above the `DAMAGE` / `HIT` /
`TOTAL` labels on the same sheet. The second quad's texel rect is fixed for every position and reads
`ARTS!!`.

**What is true.** The four `ctx[+0x28B]` values compose `NEW ARTS!!` / `HYPER ARTS!!` /
`MIRACLE ARTS!!` / `SUPER ARTS!!`, each as two halves sliding in from opposite screen sides to a
per-banner seam. `ctx[+0x28C]` is the slide's clock, and the four "layers" are a ghost trail behind
the moving word. A routine that emits textured primitives is not characterised until its texels are
decoded. Geometry:
[`battle-action.md`](../subsystems/battle-action.md#arts-announcement-banner-fun_801e2524--fun_801e2650);
sheet layout: [`effect.md`](../formats/effect.md#the-battle-value-readouts-glyph-sheet-lives-here-too).

### The battle message banner has no interior fill

**Tempting reading.** A walk of the `rim_elm_gimard_seru_capture_after` and `noa_levelup_banner`
display lists finds the class-0 border sprites and the glyph run and nothing else inside the frame
rect, so the scene shows through the banner - and through the battle-intro enemy-name labels, which
share the frame.

**Why it is wrong.** That walk keeps only `SPRT` packets. The same two states carry, ahead of the
border, a run of opaque gouraud textured quads (`POLY_GT4`, code `0x3C`) over the whole frame rect:
widget record `3`'s 32x32 blue-marbled patch at texels `(128, 0)` on CLUT `(32, 511)`, tiled in
32-pixel columns from the frame origin, grey `0x40` at the top edge and `0x88` at the bottom. The
emitter is `FUN_8002BDC4`, called for every class-0 node from `0x8002D7E8`, and each state's
framebuffer shows the navy fill under the text.

**What is true.** A gold border over a blue marbled interior. A primitive-type filter on an
ordering-table walk is a claim about which primitives exist; "nothing else" needs every GP0 code
checked. Geometry and the band law:
[`battle.md`](../subsystems/battle.md#the-full-width-message-banner).

### A kind named by its seat can name the wrong record

**Tempting reading.** Name each undecoded `+0x0E` kind value by the surface it sits under: `0x0303`
"full-width message rows", `0x0404` "framed windows", `0x2B2B` "the status bar", `0x32`/`0x33` "the
roster panels".

**Why it is wrong.** The three roster-panel placement records (6, 78, 79) carry kind `0x07`;
`0x33`/`0x34`/`0x35` are sibling kinds that add the level / status marker on top of the same panel
chain. The kinds converge - `0x33`'s chain hops `+0x0E` and then walks into `0x08` -> `0x09`, the
panel plate `0x07`'s chain ends on - so a seat-based name cannot separate two records that draw the
same pixels, and no further capture can either.

**What is true.** The kind is a table index: `0x800732A4 + kind * 0x0C` (`FUN_8002C69C` at
`0x8002C7A0`). The other seat names survive the decode. Resolution:
[`re-settled-threads.md`](re-settled-threads/battle.md#the-chrome-kind-byte-is-an-index-into-the-widget-class-table).

### `FUN_801DBB8C` is not the party readout's registration

**Tempting reading.** The battle overlay's `FUN_801DBB8C` registers one retained SCUS text actor
through `FUN_8003541C`, stashes the handle at `_DAT_801F4E0C`, and sits among the party-panel build
and teardown leaves - so it reads as the readout's own registration, the actor the arts input parks
at `y = 230`.

**Why it is wrong.** Its one caller is `FUN_801D0748` at `0x801D1660`, on the ring's `0x28 -> 0x50`
arm, immediately after `FUN_801D388C(9)` has built the arts-entry screen. The arguments are
`(0, 0xC, 0, -146, 36, 138, 144, 3)`: a 138x144 box parked one screen to the left.

**What is true.** It registers the arts list window the Triangle page slides in. The party readouts
are placement records 7 and 6 / 78 / 79, opened by the sub-draw script through `FUN_801D8DE8` like
every other chrome element. A registration is identified by the rect it registers and the transition
that calls it, not by the leaves it is compiled beside.

### The item window shows the pill

**Tempting reading.** Retail's item-use *action* shows the full-width pill (`captures/tetsu_idle`),
and so does the ring the window opens from, so "pill while the item window is up" is the obvious
interpolation.

**Why it is wrong.** The sub-draw step the `0x28 -> 0x3C` arm runs (`FUN_801D388C(5)`, `0x801D13F0`)
is `06/0 4E/0 4F/0 07/1`: the roster panels come **back up** and the bar parks.

**What is true.** The bar returns at the window's target step (`0x64`, step `0x18`), re-pointed at
the member the cursor names. The magic window (step 7) has the same shape. A menu state's surfaces
are a table row, not a neighbour's: read the step, not the frames either side of it.

### The magic chip's gate reads the weapon byte

**Tempting reading.** `FUN_80053CB8` writes `ctx[+0x25F + member]` after an `lbu` at `+0x760` off
`0x80084140 + (char_id - 1) * 0x414`, and `0x80084140 + 0x760` is the live record's `+0x198` - the
equipment byte the save-record table names `weapon_id`. So "the element chip is live when a weapon is
equipped".

**Why it is wrong.** `player_steal_skeleton_pre` has the gate at `1` with `+0x198 = 0` and
`+0x199 = 1`. The store at `0x80054270` is the **second** arm, reached only for `char_id == 2` (the
`beq v0,a3` at `0x800541E4` on `DAT_8007BD10[member]`) - Noa's byte, not the weapon rule.

**What is true.** The first arm (`0x800541E0..0x80054218`) reads `+0x761` = record `+0x199`, the
Ra-Seru slot, and in twenty-nine states the gate equals that byte's non-zero test. One `lbu` in a
two-arm predicate is not the predicate; check a reading against a state whose bytes disagree with it.

### `FUN_801DBC30` is not the battle name plate

**Tempting reading.** `FUN_801DBC30` sits in the battle overlay next to the party-name panel's open
and teardown leaves, takes an `(x, y)`, and lays down one `0x40 x 0x10` textured quad: a fixed-size
strip at a caller-supplied seat, in the function group that builds the name buffers. Its `x-8` bias
even seems to explain the panels' 8-pixel text inset.

**Why it is wrong.** `tpage 7` resolves to VRAM page `(448, 0)` and CLUT `0x7704` to `(64, 476)` -
the `etim` effect page, not the system-UI sheet the battle chrome samples. The texel span
`(0, 96)`-`(63, 111)` decodes out of a battle VRAM dump as the **red cross-out X**, the mark retail
lays over a command chip the actor cannot pick; the same rect is pinned under that name for the
Muscle Dome's forbidden Item chip.

**What is true.** The name plates are 3-slice runs off the resident system-UI sheet's page
`(896, 256)`, and the party readout draws no bar at all. A primitive builder is identified by the
page and palette it samples, not by the neighbourhood it is compiled into. See
[battle.md](../subsystems/battle.md#battle-screen-chrome-packet-pinned).

### The dome victory caption is not a prize

**Tempting reading.** `FUN_801D8DE8` case `0x59` composes a victory line out of a per-character
label from the table at `0x801F4DFC` plus a spell name from the shared spell-name table at
`ctx[+0x269] + 0x80` - the player Seru-magic block. Read alone that is an award message: a won dome
leg grants a Seru.

**Why it is wrong.**

- The table is **shared**. `0x801F4DFC` is the battle-family per-character label table,
  byte-identical across the battle-action, magic-capture, magic-level-up and dome overlays, and the
  composer is the ordinary cast-caption builder reached by any cast in any battle. Its presence in
  the dome overlay is residency (as with the `0x801F4D34` / `0x801F4B8C` sibling tables).
- The arena has no capture writer: there is no `record_capture` analogue anywhere in the overlay.

**What is true.** The whole reward path in PROT 0977 is `FUN_801D0F60`: it settles the score tally
and, once per save on the Master-course final fight, hands over item `0xCD`. The tally is paid by the
*shared* minigame-exit routine `FUN_80026018` into the casino coin bank `0x800845A4`, saturating at
9,999,999. Before crediting anything a message mentions, find the *writer* of the thing credited. See
[minigame-muscle-dome.md](../subsystems/minigame-muscle-dome.md#contest-settlement--the-one-shot-prize).

### Muscle Dome was never a card battle

Four readings, each wrong for a different reason.

- **"A hand of four cards."** *Tempting:* `FUN_801d388c` case `9` builds four slots in a
  `do { } while (< 4)` loop, which reads like a deal. *True:* the four slots are the four **d-pad
  directions**, always command ids `0xC..=0xF`, each carrying that fighter's own AP cost. Nothing is
  drawn, discarded or reshuffled; the arena is an ordinary battle whose command string is bounded by
  AP, and its presentation is the standard battle command cluster.
- **"A score of `hp * 0x6C / max`."** *Tempting:* the compiler renders `x 100` as a shift-add chain -
  `sll 1` (2x), `addu` (3x), `sll 3` (24x), `addu` (25x), `sll 2` (**100x**) at
  `0x801d0f38..0x801d0f4c` - and stopping at the fourth instruction yields 25, folding the wrong
  pair `0x6C` (108). *True:* `x 100`. A multiplier read off a shift-add chain is correct only if the
  whole chain is consumed; Ghidra's C prints `* 100`, and a second dump of the same code at another
  load base (`overlay_0896_801f04b0.txt`) reproduces it.
- **"Rendered in phase `0x6e`, per fighter."** *True:* the computation lives in the phase-`0x14` arm;
  `0x6e` only re-stamps the two globals `0x14` wrote. The record read is `DAT_801c937c` - actor-table
  index 3, the first **enemy** slot - so there is one number on screen, the opponent's. The match SM
  contains exactly two ratio computations and both are that `x 100`.
- **"Four turns is the whole dome leg."** *Tempting:* the arm draws a `Turns Left / HP Left` strip
  (format string at PROT 0898 file offset `0x0`): `4 - ctx[+0x28a]` (the shared battle turn counter,
  bumped by `FUN_801e295c` case `0xff`) and the first enemy's HP percentage. *True:* the arm is gated
  on `*(u8*)0x8007BD0C == 0xB6`, and `0x8007BD0C` is the four-slot **monster-id formation cell**, not
  a battle-type byte. The dome stages its opponents into that cell out of a 29-round table topping
  out at id `0xAA`, so no dome round reaches the strip; it belongs to monster `0xB6`, Koru, whose
  four-turn timed kill the curated boss table records independently. A byte compared against a small
  constant is not a mode tag until its writer is found. See
  [minigame-muscle-dome.md](../subsystems/minigame-muscle-dome.md#the-four-turn-strip-belongs-to-koru-not-the-dome).

A separate widget must not be folded into this one: `FUN_801d8de8` is the **shared battle status
plate** (dumped under ten overlays), drawing each fighter's own HP/MP `cur`/`max` numerals from
`+0x172`/`+0x14e` and `+0x174`/`+0x152`. It computes no percentage and is not dome-specific.

### Op-0x4E sub-op family - every sub-op 0..9 is a compare

**Tempting reading.** The decompiled C shows bare-`break` arms for sub-ops 2..9, which read as
"absolute jump" (5..8) and "rand -> next PC" (4).

**Why it is wrong.** Those arms are Ghidra's collapsed switch: each raw loader ends `j 0x801e0b40` /
`j 0x801e0b3c` with the operand pointer staged in the delay slot (the same class of trap as the
label-call idiom).

**What is true.** The raw 12-entry jump table at `0x801CEE30` (field overlay, PROT 0897 file
`+0x618`) routes **every** sub-op 0..9 to a value loader that joins the shared 7-byte
compare-and-skip continuation at `0x801E0B40`:

| sub | loader | state value |
|---|---|---|
| 0 / 1 | `0x801E0A40` / `0x801E0A70` | char-record HP / MP `(cur, max)` pair - the only scaled form (`max * arg >> 8`) |
| 2 | `0x801E0AC0` | char level byte `+0x130` |
| 3 | `0x801E0AEC` | party gold `_DAT_8008459C` |
| 4 | `0x801E0AFC` | **BIOS `Rand() & 0xFF`** - a random-chance branch |
| 5..8 | `0x801E0B0C` | **slot table `0x801C6460[sub - 5]`** (s16; the read side of the `4C CA/CB/CC` slot writes) |
| 9 | `0x801E0B34` | coin bank `_DAT_800845A4` |

Sub-ops 10/11 keep the 9-byte u32 gold/coin form; 12..15 fall through (PC += 7). Ports:
`field_disasm::decode_subops` (single 0..=9 compare arm), `engine-vm` `field/step/flow.rs` and
`FieldHost::op4e_char_level` / `slot_table_read`. cave01's `P2[12]` spawn gate is the live sub-5
exemplar.

### Gaza 2 0x51 park - the two falsified generators

Two "ordinary play" generators of the `0x51` HP-readout desync, both falsified. What is true instead
is in
[re-settled-threads.md](re-settled-threads/battle.md#endless-camera-orbit---the-0x19-attack-approach-park).

**Clamp asymmetry as a standalone generator.**

- *Tempting:* `FUN_801EC3E4` has two overkill clamps (accumulator vs displayed bar at `0x801EDB70`,
  live HP vs itself at `0x801EEA10`), and a plain capture shows "live HP 266 / bar 0 / zero
  accumulator".
- *Wrong:* the clamps can only disagree when the bar already lags live HP **at action start**, and
  the previous party-targeted action's own `0x51` settle wait guarantees it does not. From a synced
  start, credits exceeding the starting bar also exceed starting HP, so both sides floor together (a
  consistent kill). The exhibit is per-strike **phased crediting** - paired stores `0x801EDB40` /
  `0x801EDB58` credit the action total and the accumulator per strike while live HP commits once at
  `0x801EEA10` - a transient that closes with a death commit ~90 vsyncs later.
- *True:* the asymmetry is an amplifier only. A per-frame watchpoint cannot tell an absorbing desync
  from the inside of a healthy multi-strike resolution; only survival past the action's commit and
  settle wait counts.

**The Final Heal revive "at the worst possible moment".**

- *Tempting:* the assigning seed (`0x800410BC`) is real and the discard arithmetic stands, so a
  revive landing mid-drain - the killing hit credits the whole bar, the readout is mid-drop at state
  `0x50` - would strand the readout.
- *Wrong:* credits land per strike *early* in the resolution; `0x50` arrives after the remaining
  targets resolve and effects tear down; and the quarter-step drain empties any accumulator within
  ~35 rendered frames. Three captures (`autorun_gaza2_acc_discard.lua`, Lost-Grail-armed party, no
  harness HP / readout / accumulator writes, ~84k vsyncs) drive twelve retail `FUN_801E6968` revives
  across cast-path, kernel-path, single-target and party-wide kills: every assign hits `+0x10 == 0`,
  with margins of 143-280 vsyncs.

### The cast-module blocker was named wrong

**Tempting reading.** A streamed signature-attack module "stages the caster's monster-block entries
by raw index, and a party actor has no monster block", so it cannot run from a party slot.

**Why it is wrong.** `FUN_8004AD80` resolves `actor+0x1DA` down two arms - monster seats through
`DAT_801C9348[slot-3]+0x4C`, **party seats through `DAT_801C9360[slot]`** - and the indices PROT 960
stages resolve on both. The module's one monster-block access is a hardcoded **seat-0** write to a
single clip's root-motion field, unrelated to who is casting.

**What is true.** Three other things block it:

- That seat-0 read walks past `magic_count` into words the loader never fixed up, so a seat-0 monster
  with `magic_count <= 13` - Che Delilas has 12 - turns the pointer into a bare offset and the
  following store lands a halfword in PSX **kernel RAM**. Retail never trips it because the module is
  only reached with Lu (16 entries) in seat 0.
- Battle state `0x70` re-enters the module every frame and advances only on a zero return, with no
  timer and no bail-out. One of its four phase gates needs a clip of at least 23 keyframes, which
  Gala's party index `0x0D` fails at 17 of 19 equippable section-2 ids.
- The damage call and both HP writes are hardcoded to actor slot 0, not the chosen target.

A structural-sounding impossibility ("X has no Y") stops the search before the real failures are
enumerated. Full account in
[`randomizer.md`](../tooling/randomizer.md#casting-a-sibling-signature-attack-from-a-party-slot).

### The attack camera was never an arm choice

**Tempting reading.** `FUN_801D71B8` dispatches the per-art attack camera through three
per-character jump tables. The arms visibly differ in choreography - one commits a single framing
for the whole swing, others change shot two or three times - and 37 of the tables' 54 slots point at
a bare return. So an art that films flat has a poor arm, and the fix is to point its slot at a
richer one.

**Why it is wrong.**

- **Every arm is timed, not merely shaped.** A shot change is an `slti` immediate against the
  animation cursor `actor[+0x22C][+0x68]`, which counts sixteenths of a keyframe. The arm span
  carries thirteen such tests with immediates `64` / `97` / `112` / `144` / `160` / `176` / `192` /
  `224` / `240` / `272` - keyframe 4 through **17**, sized for retail's ~20-frame swings. On a
  46-100 frame clip any arm finishes its choreography inside the wind-up and holds one shot for the
  rest of the move.
- **Not one arm is spare.** The 37 dead slots all point at one shared epilogue (`0x801D828C`), a
  return. The 17 live slots reach 13 distinct arms, no arm is live in more than one character's
  table, and each of the 13 is already some art's camera. A retarget can only alias an arm another
  art uses, and a re-time then mistunes that art too.

**What is true.** The arms admit re-timing in place: scale an arm's immediates and its shots land at
the same beats of the longer swing. Where a host art's own arm carries no cursor test there is
nothing to scale, and the move is a **swap** of two slots inside one character's table - it leaves
the set of live arms unchanged and the borrowed arm reachable from exactly one slot, which makes
re-timing it safe. The arms differ in shot *count* and agree on clip *length*, so the choice axis is
orthogonal to the defect; empty slots in an index are not slack in what it indexes. Full account in
[`randomizer.md`](../tooling/randomizer.md#delilas-party-swap).

### The case-6 party arm is the battle-over framing

**Tempting reading.** `FUN_801D5854` case 6 forks at `0x801D5CF4` on `DAT_8007BD71 == 0xFE` and at
`0x801D5CFC` on `ctx[+0x13] < 3`. The fork is keyed on a party seat, the `0x801D5CFC` arm reads the
acting actor's anim id (`actor[+0x1DB]`) through a per-character script - eye `prescale(0x500)`
straight behind the actor at `-5 x actor[+0x3E]` - and `0xFE` beside `0xFF` reads as one live state
beside another. So it is the per-action party framing, and everything else takes `0x801D64C4`.

**Why it is wrong.** `DAT_8007BD71` is the **battle-end signal**. Its writers are the action SM's
`0x5A` wipe scans (`0x801E65D8` party wipe, `0x801E6674` monster wipe, beside the cause in
`_DAT_8007BD2C`), the `0x66` escape teardown (`0x801E5A94`, right after `ctx[7] = 0x67`) and the
capture-effect module (`0x801F7318`); SCUS `0x80056014` zeroes it at battle init, and the effect-VM
walker (`FUN_801E0088`) runs only while it reads `0xFF`. Twelve battle save states - five Begin/Run
prompts, the arts-input close-up, the tutorial open, two mid-strike frames and three
`ctx[7] == 0x19` approach parks - all read `0xFF`.

**What is true.** During a fight the `0x801D5CFC` arm is unreachable for anyone; the anim band its
script keys on (`0x11..=0x18`) is the win-pose band, so it is the end-of-battle framing
(`ActionFraming::battle_over`, `false` while a fight runs). The three `0x19` parks with Gaza acting
confirm the `0x801D64C4` arm byte-exact: `TR (0, 0x500, prescale(ctx[+0x6D0]))`, yaw
`ctx[+0x6DA] - actor[+0x46]`, focus the negated `+0x34/+0x38` pair. Applied to a running fight, the
wrong arm puts the eye 2048 projection units behind the actor - inside whichever combatant has closed
to melee range - with `ndc.y` past `-2` for the other actor. See
[battle.md](../subsystems/battle.md#battle-camera-exact).

### The Done band is not idle

**Tempting reading.** `FUN_801E295C`'s Done band (`0x50` cleanup, `0x51` fade-down) is where a fight
rests - an auto-resolved fight spends about half its frames there - so treating it as "an action is
executing" keeps the per-action close-up up for half the fight. Hence: the band is idle, and belongs
on the far framing (case 9), where the formation and the idle orbit live.

**Why it is wrong.** The close-up it argues from is the battle-over arm applied to a running fight
(see [above](#the-case-6-party-arm-is-the-battle-over-framing)). With the in-fight arm, case 6 sits
at `prescale(ctx[+0x6D0])` - `4915` for a `0xC00` depth - with both combatants in frame, while the
far framing over a formation collapsed by a melee sits at its `0x800` floor, `3276`: closer than the
arm it stands in for.

**What is true.** Retail's `0x50` / `0x51` arms fork on the category
(`0x801E5E90..0x801E5EF4`, `0x801E5FC0..0x801E6018`): Run -> orbit, Attack -> case 8, party slot
over a dead target -> case 8, else case 6, re-armed every pass. `zora_glare_petrify_post`
(`ctx[7] == 0x51`) reads case 6's pose on the caster and `evil_medallion_rage_battle`
(`ctx[7] == 0x0A`, between actions) reads the far framing. See
[battle.md](../subsystems/battle.md#battle-camera-exact) and
[re-settled-threads.md](re-settled-threads/battle.md#the-done-band-framing-and-the-two-orbit-writers).

### The battle-intro banner is raised from a top-seated `0x0303` placement record

*Falsified by capture.*

**Tempting reading.** The intro banner is one of the placement records that park at `(16, -24)` and
live at `(16, 14)` with kind `0x0303` - records 67, 69..75, 89, 101, 102 - with the runtime
overwriting the disc width with the measured enemy name and sliding the element down from the park
seat. Every *other* battle-HUD element does come from that table through `FUN_801D8DE8`, which
forwards the record field for field and glides it park-to-live, and record 68 really does get its
width overwritten at runtime (disc `w = 0`, spawned at the measured name width).

**Why it is wrong.** `FUN_801D9D3C` places the intro labels itself with immediates and never reads
the table.

**What is true.** The width is a call argument, not a table write, and there is no slide: the labels
appear and vanish at one seat. See
[`battle.md`](../subsystems/battle.md#the-battle-intro-enemy-name-banner) and
[`re-settled-threads.md`](re-settled-threads/battle.md#the-battle-intro-enemy-name-banner).

### `0x801CFA48` is a mid-function citation aliased to another overlay

*Falsified by disassembly.*

**Tempting reading.** The world-map per-actor render dispatcher's `0x2000` arm targets
`FUN_801CFA48`, a mid-function address inside `FUN_801CF88C` in the menu / battle-action dumps -
VA-aliased to some other overlay's routine, so no clean dump exists to port from. Slot-A overlays
share base `0x801CE818`, and `overlay_0897` dumps do print bodies at nearby VAs that are label
artifacts.

**Why it is wrong.** `0x801CFA48` opens with `addiu sp,sp,-0x70` in
`overlay_battle_action_0898.bin` and in no other extracted overlay image, and the routine's 12-word
signature occurs in exactly one image on the disc.

**What is true.** It is the lightning effect-ribbon emitter (`THERNDER1` in PROT 0973's dev harness),
resident only with the battle overlay. The artifact is the neighbouring name `FUN_801CFB94` - a
branch label inside this routine colliding with a real entry in PROT 0970. See
[`battle-action.md`](../subsystems/battle-action.md#overlay-local-prng-fun_801d0290).

### `_DAT_8007BD84` is a mode word the melee kernel branches on

*Falsified by disassembly.*

**Tempting reading.** The melee kernel tests the word against zero and forks on it, so it reads as a
mode flag; and "the grunt's `s7` latch always passes".

**Why it is wrong.** Its only non-zero writer disc-wide is PROT 0940's Cort "Mystic Shield" stager
at `0x801F7678`, storing a `FUN_80021B04` return; `FUN_8004CE2C` dereferences it at `+0x10` / `+0x56`
/ `+0x72` and clears it when it fires cue `0x10D`. The two SCUS writers only zero it. The latch does
not always pass either: `s7` must equal `actor[+0x1F3]`, and only one of fourteen reaching
definitions loads it.

**What is true.** It is an **effect-instance handle**; callers that read it as a flag are testing the
handle for null. See
[`re-settled-threads.md`](re-settled-threads/battle.md#what-a-normal-party-attack-sounds-like).

### The slot cabinet is in neither the art pack nor any prim a traced slot function emits

*Falsified by a second read of the container.*

**Tempting reading.** PROT 1200's TIM list holds no cabinet art, and no slot function emits a large
untextured quad - both true.

**Why it is wrong.** PROT 1200 has **three** descriptors; the TIM list is only descriptor 0.

**What is true.** Descriptor 1 is a 2160-byte untextured TMD that *is* the cabinet, spawned as an
ordinary actor by the slot init and drawn by the shared TMD renderer - which is why no slot function
emits it. See [`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md).

### The slot's `DAT_801d4134 * 0x10` is a sub-row landing nudge

*Falsified by disassembly.*

**Tempting reading.** `rand % 5` scaled by `0x10` reads like a fraction of a reel row (rows are
`0x100` apart), added so a stop does not look mechanical.

**Why it is wrong.** The product is a row index into the 16-byte-stride table at `0x801D3630`.

**What is true.** The table's words choose which of the five paylines a forced stop lands its target
on; a forced target is not always on the middle row, and there is no visual offset. See
[`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md#reel-landing---fun_801d2114--fun_801d2440).

### A phase-gated effect draw is the candidate for the arena's object-1 dust decal

*Falsified by disassembly.*

**Tempting reading.** The decal is absent from a mist-free arena capture, so an effect draw gated on
a battle phase puts it there.

**Why it is wrong.** No effect path touches it.

**What is true.** Object 1 is ordinary backdrop geometry that the SCUS battle loader `FUN_800513F0`
trims from both backdrop actors' part lists when `_DAT_8007B64B` is zero; the mist-free capture is
the default, not a phase gate. See
[`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md).

### `ctx[+0x26]` is a boss phase counter and `ctx[+0xD]` is a dead store

*Falsified by disassembly.*

**Tempting reading.** `ctx[+0x26]` has one increment site (`0x801E6D3C`) in the Cort form-change
arm, so it counts boss phases; `ctx[+0xD]` has "no port field and no reader".

**Why it is wrong.** `ctx[+0x26]`'s unload reader `0x801E61B4` passes it as a UI element id, and its
only assignment anywhere is `0x65`, the level-up banner. `ctx[+0xD]` has three readers inside
`FUN_801D5854`.

**What is true.** `ctx[+0x26]` is a UI element id; `ctx[+0xD]` is the per-action camera angle
variant. Both in
[`re-settled-threads.md`](re-settled-threads/battle.md#two-battle-context-bytes-read-wrong).

### `0x801E3A20..0x801E3A64` is the Miracle continuation

*Falsified by disassembly.*

**Tempting reading.** The block sits at the end of the strike stream and re-arms a counter, the
shape of a Miracle Art's continuation.

**Why it is wrong.** `s5` is `ctx + 0x11`, so `0x5(s5)` is `ctx[+0x16]`, and the guard chain
(`ctx[+0x13] < 3`, record `+0xF4 & 0x2000`, counter zero) names the War God Icon.

**What is true.** It is the Attack x2 second pass - the refill that lifts `ctx[+0x16]` from `0` to
`1`. It is one of that byte's two writers; the stage site's bump at `0x801E37AC..0x801E37BC` is the
other.

### The Miracle marker is armed by an input recognizer (`FUN_801E91E8`'s caller)

*Falsified by disassembly.*

**Tempting reading.** `ctx[+0x25F + slot]` gates the Miracle path, and `FUN_801E91E8` reads it, so
that routine's caller is the recognizer that arms it.

**Why it is wrong.** `+0x25F` has one `sb` in the corpus, in `FUN_80053CB8` at battle-actor seeding,
from the Ra-Seru equipment byte.

**What is true.** No input recognizer writes it. `FUN_801E91E8`'s caller is the killing-blow Seru
absorb; it stages the absorbed Seru into `ctx[+0x269]`, which the Done band teaches (port field
`absorbed_seru`).

### `DAT_8007BD10` is a per-slot control-mode byte

*Falsified by disassembly.*

**Tempting reading.** A per-seat byte compared against `4` to pick out an AI-driven member reads as a
control mode.

**Why it is wrong.** Three routines index character records with it as
`0x80084140 + (byte - 1) * 0x414` (`0x801EF344`, `0x80053CEC`, `0x801E39CC`).

**What is true.** It is the slot -> roster character id table. "`== 4` = an AI-driven member" is
right in effect - character 4 is the AI companion - and wrong about the mechanism.

### The spell record's `+0x01` effect-class byte is undecoded

*Falsified by this repo's own source (`inference`), with the bytes confirmed against the disc
(`disassembly`).*

**Tempting reading.** Nothing turns the spell record's `+1` byte into a module, so it reads as
undecoded; and the player Seru-magic block "all shares `cat = 0x32 / sub = 0`".

**Why it is wrong.** `legaia_asset::spell_names::SpellEntry::sub_class` reads `+1` beside `+0`, and
`World::spell_table_sub_class` serves it to the battle host. By `SCUS_942.54`, `0x83` Vera is
`0x00 / 0x03` and `0x89` Orb is `0x01 / 0x04`; only the nine enemy-side spells are `0x32 / 0x00`.

**What is true.** The byte is decoded; it is the sub-id the capture-class dispatcher keys on (see the
slot-B readings below and [`spell-table.md`](../formats/spell-table.md)).

### The slot-B module band shares a library tail

*Falsified by disassembly and byte comparison.*

**Tempting reading.** PROT 0958 and 0959 hold the same words past file `~+0x2A00`, which reads as a
library object linked into several modules.

**Why it is wrong.** Every one of the 64 module images ends in a byte-identical, same-file-offset
run of *another* extracted image, ending exactly at the shorter image's length - and nine of them end
in **PROT 0899's** bytes, a slot-A overlay at a different base. Linked code could not come from an
image at another base.

**What is true.** The tails are mastering residue. The routine four 12288-byte images carry at
printed VA `0x801F9458` is the menu overlay's at `0x801D1298`, and a worklist row keyed on such an
address names an image that only holds the residue. See
[`cast-module.md`](../subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes).

### The slot-B band: four readings the whole-band dump overturned

*Falsified by disassembly.*

| Reading | What is true |
|---|---|
| "No hard-coded `jal` into the capture-class band was found" | The sweep looked for a table keyed `id - 0x81`; the capture-class dispatcher `FUN_801F2160` keys on the spell record's `+1` sub-id and jumps through `0x801CF56C` |
| "0957's trampoline `0x801F9BA8` is reached by nothing" | It is arm 22 of `0x801CF56C`, at `0x801F233C` |
| "A slot-B module is two functions and has no internal `jal`" | 196 framed functions across the 64 images, 25 of them with internal calls; the two-function shape holds for the first thirteen only |
| "PROT 0965 is a shifted sibling of 0967" | An over-read past the entry's real size; 0965 is the Doomsday module |

Function extents in the band come from frame matching, not from counting prologues against `jr ra`
words - a frameless leaf, an early `jr ra`, or a `jr ra` word in a data tail breaks the count. See
[`cast-module.md`](../subsystems/cast-module.md).

### The summon draw runs 35-64 times a frame

*Falsified by capture.*

**Tempting reading.** During a player Seru-magic summon the battle per-actor draw `FUN_80048A08`
fires 35-64 times per frame - a figure from a real exec-breakpoint run - and a summon has many mesh
groups, so it reads as a per-part driver walking them.

**Why it is wrong.** On the same catalogued state
(`scripts/pcsx-redux/autorun_enemy_move_render_path.lua`, `gimard_burning_attack`, 400 vsyncs) the
draw never exceeds **2** per rendered frame - Vahn solo against one monster. The same probe reads 6
in a 3-vs-3 and 2 in a 1-vs-1.

**What is true.** One call per **live actor** per rendered frame; the group walk is inside the call.
The move-VM / `FUN_801F7088` hypothesis stays retired. A budget or scheduling argument built on the
larger figure is off by more than an order of magnitude; see
[`effect-vm.md`](../subsystems/effect-vm.md).

## Audio / sound driver

| Thread | Verdict | Why |
|---|---|---|
| The BGM tail banks must be dropped on every track restage | falsified (a track change closes neither) | Plausible: a restage reloads the score, so dropping the banks with it reads as hygiene. `FUN_800243F0`'s stream arm never calls `FUN_8001FF58`; slot 11 closes in the field init. A director that drops on every load is the defect. |
| Op-`0x35` sub-op 3 resumes the BGM and sub-op 4 stops it | falsified (3 pauses, 4 re-attaches and replays from the top) | Plausible: the legacy labels read that way and a 3-then-4 pair looks like resume-then-stop. The arm table at `0x801CEE00` (PROT 0897, indexed `sub - 1` behind `sltiu 0xB`) gives 3 = set pause bit 1 + `FUN_80026740`, 4 = clear bit 1 + `FUN_80026478`. Following the legacy labels silences the score at each of the 64 sub-op-4 sites on the disc. |
| A scene-local BGM id loads PROT `current_scene + 6 + id` from the scene's own block | falsified (it loads global slot 2) | Plausible: `FUN_800243F0` forms exactly that index and stores it. It is only the change-test index; the load arm overwrites the index it loads with `*(0x8007BC64) + 2` (`0x800245A4..0x800245BC`). Retail stages no scene bank. |
| The credits bank overwrites every resident bank below `0x6C810` | falsified (it spans `0x1010..0x641C0`) | Plausible: VAB 10 opens at slot 0's base. By its size it covers slots 0 to 3 and stops short of slot 4 / 7 at `0x65010`. |
| `FUN_8001FF58` releases a SEQ slot | falsified (it closes a VAB slot) | Plausible: the teardown op that calls it also stops a sequence, and slot numbers look alike across the two tables. The routine reads the resource record's `+0x8` VAB id and calls `FUN_80068C80` (`SsVabClose`), clearing `+0xB` in the delay slot. |
| A cue whose category's slot is closed falls back to the class-2 bank | falsified (it is silent) | Plausible: a fallback bank is what a single-bank port reaches for. The drainer tests the slot's `+0xB` enable byte and skips the cue (`0x80016CE4..0x80016CEC`); the VAB close zeroes it. |
| The Muscle Dome's hub holds the class-2 bank | falsified (it holds the field bank; slot 2 is closed) | Plausible: the dome's rounds run the battle frame driver, which loads PROT 0869. A retail state at the hub (mode `0x19`) has slot 2 closed and slot 6 open over PROT 0876's header, the bytes the field states hold ([`audio.md`](../subsystems/audio.md#retail-capture-of-the-slot-2--slot-6-residency)). |
| The engine keys on about half retail's key-on rate | falsified (a pairing of the track's opening bars against a mid-track window) | Plausible: `0.244` against `0.488` comes off the key-on rate over equal-length traces. A trace only as long as the retail window has nowhere to slide; aligned (engine frame `3111` / `3112`) the ratios are `1.148` and `1.060` ([`audio.md`](../subsystems/audio.md)). |
| The engine keys on about 15% more often than retail (ratio `1.148`) | falsified (`292` key-ons against `292`, counted exactly) | Plausible: it is the residual left once the windows are aligned. It counts envelope edges, which a capture reads off a host-clocked SPU, and it aligns on sounding pitches. A breakpoint census of `SpuSetKey` against the engine's per-voice key-on counters, aligned on key-on timing, agrees on every count ([`audio.md`](../subsystems/audio.md#count-the-key-ons-not-the-edges)). |
| Retail BGM voices sound at about twice the engine's volume | falsified (a mednafen read-out scale, plus a real `107` vs `100` gap) | Plausible: a mednafen state's `(Voices[n].Sweep[0]).Current` reads about `2.3x` an engine voice volume on the same notes. Mednafen keeps a sweep's `Current` at the register doubled (`(Control & 0x7FFF) << 1`; the states' own `Regs` shadow reads `0x0850` where `Current` reads `4256`), so `legaia_mednafen::spu` halves it. The remaining `1.15x` is the sequence volume: retail runs BGM at `SsSeqSetVol` `107` against a velocity scale of `100`; the port carries it as `RETAIL_BGM_SEQ_VOL` ([`audio.md`](../subsystems/audio.md)). |
| `FUN_80035B50` is a delay queue | falsified (a round-robin ring of four) | Plausible: a scheduler with an `enqueue` is the obvious port shape for a cue producer. Retail writes the next slot with no free-slot search and a fifth cue replaces one; the delay is set by the sibling `FUN_80035BAC`. |
| The port sustains about half again retail's sounding voices, so its release tail runs long or it keys fresh slots | falsified (both explanations, and the statistic) | Plausible: the comparand (envelope level, on the track the save holds) looks sound, so the figure reads as a real residual. Two instrument defects sit under it. The windows are not aligned - the retail capture pairs with engine frame `3111`, not frame 0 - and the emulator steps its envelope on an audio-paced thread, so a per-vsync capture's envelope statistics move with host speed: two captures of one state disagree on `env_level` for six of ten voice-frames. |
| The native BGM director's `stop` leaves the resume gate open | falsified (it was writing one of two pause representations) | Plausible: a resume that does not resume looks like a gate nobody closed. Two representations of "paused" exist; a `stop` that writes only one leaves unwritten the half the resume consults. |
| The blocking bank for the Seru cast voice is `XA27` / `XA28` / `XA29` | falsified (seventeen other files) | Plausible: those three are exactly the clip slots the dispatcher's `1 / 3 / 5` remap produces, a real but unused corner of the table. The cue id is hardcoded in each cast module's own image - 62 of 64 - and the ids that occur resolve to `XA7`, `XA9..15`, `XA18..20`, `XA22`, `XA23`, `XA25` and `XA34`. |
| The XA cue-duration table has `0x40` entries | falsified (`0x110`) | Plausible: `0x40` covers every arts-shout id, and the reader's one comparison is against `0x100` rather than a length. Measured off the executable the table runs to index `0x10F`, with interior zero runs (`0x37..0x40`, `0x78..0x88`, `0xE8..0x10A`) that make a short read look like the end. The reader bounds nothing above `0x100`, so a truncated table silently drops every cast cue. |
| The sequencer's pause freezes sounding notes | falsified (it keys them off) | Plausible: a pause that resumes where it stopped is a freeze in every other sequencer, and the play cursor really is preserved. The script's pause is sub-op 3 (`FUN_8006275C` raising slot flag `0x2`); the per-tick `FUN_80062F98` sees the flag and calls `FUN_800638D8`, which releases every voice the channel owns through `FUN_800684CC`. No script word resumes from the kept cursor: sub-op 4's `FUN_800628F0` resets it to the start for every mode, and sub-op 2 is a stop and rewind (`FUN_800641EC`). |
| `FUN_80058490` is a sound-driver lane, so the table feeding it is a cue list | falsified (it is `MoveImage`) | Plausible: it is called with a small id from a compact table in a battle context, the shape of a sound-cue dispatch, and the values sit in a plausible cue range. It moves a VRAM rect to `(0xE0, 0x1DC)` - CLUT row `y = 476` - so `0x801F6418` holds VRAM **x** coordinates, a palette column. |
| `FUN_80068D94` as "`SsSepOpen` / SEP loader" (with `FUN_80068B98` as "`SsSeqOpen`") | falsified (it is the VAB-open head) | Plausible: it validates a magic, reads a count at `+0x12`, `SsSpuMalloc`s and patches a pointer table - the shape of a SEP/track loader, with the magic read as 'VAP'. The compare is `0x564142` against `word >> 8` plus low byte `0x70` - `pBAV`, the **VAB** magic - and `+0x12` is `ps`. The "per-track pointer table" is the ProgAtr table receiving the program -> packed-tone-page rank map ([`vab.md`](../formats/vab.md#program-slots-vs-packed-tone-pages)). Correct roles: [`audio.md`](../subsystems/audio.md#ssapi-seq-management-layer-above-libspu). |
| The entry that matches "`[u32 format == 2][u16 spu_addr[256]]`, every address `>= 0x8000`" is `monster.snd` | falsified (it is `summon.dat`; `monster.snd` is a multi-bank VAB two entries away) | [details ↓](#the-256-slot-spu-address-run-that-was-really-a-clut) |
| Op-`0x35` sub-op 9 is a **queue**, triggered by the next scene entry | falsified (it is a start behind an asset-load barrier) | [details ↓](#op-0x35-sub-op-9-was-never-a-queue) |
| `_DAT_8007B910` is the live screen brightness | falsified (it is the live **audio level**) | Plausible: the cell ramps down during a summon and back up when the action ends, and a summon does visibly dim the screen. All 26 dumped read sites end in a volume setter, none in a draw primitive. The dim rides the separate accumulator `_DAT_8007B440` (`FUN_801ED308` -> the wipe emitter `FUN_8003479C`); the two ramp together. Answer: [`re-settled-threads.md`](re-settled-threads/audio.md#_dat_8007b910-is-the-live-audio-level-not-screen-brightness). |
| `FUN_800684CC` keys a voice off by VAB id | falsified (it keys off by owner halfword) | Plausible: a routine that takes an id and stops voices reads as a bank teardown, and `SsVabClose` is the libsnd call with that shape. The argument is the owner halfword `seq \| track << 8` that key-on stamps at `0x8006661C`, so it stops the voices one sequence's track owns. Passing a VAB id stops whatever shares the number. |
| The scene-VAB parse offset is why the audio oracle never converges | falsified (those scenes carry no bank at all) | Plausible: a parse handed offset `0`, where no entry on the disc begins with the magic, fails every time - a good explanation for an oracle that converges on none of its candidates. The nineteen scenes the oracle qualifies carry no VAB entries, so the figure does not move when the parse offset is right. |
| `FUN_801EA9B0` cycles the BGM index | falsified (it plays the row; cycling is `FUN_801E9F64`) | Plausible: the dev menu's `BGM CALL` row shows a number that moves, and the routine is the row's action dispatcher. The arm at `0x801EACBC..0x801EAD20` reads the cursor `_DAT_801F2E90`, indexes the 10-byte sound-test rows at `0x801F2E94` and installs the row's global id - the fourth disc-wide writer of `_DAT_8007BAC8`, not a "world-map region change" writer. Stepping the cursor is the input half, `FUN_801E9F64`. |
| A mednafen save reports nearly every voice non-`Off` because nothing keys them off wholesale | falsified (there is no `Off` phase; the predicate was talking) | Plausible: every snapshot reports 20 to 24 of its 24 voices live, so a rule asking an engine frame's mask to be a superset looks unsatisfiable and `0 converged` reads as a fact about the comparand. `ADSR.Phase` has no `Off` member - it is `0` Attack through `3` Release, and a key-off parks a voice in Release - so the predicate counts every voice the state ever keyed. Against `ADSR.EnvLevel != 0` retail holds 4 to 9 audible voices against the engine's 4 to 8 ([settled](re-settled-threads/audio.md)). |
| The dev sound-test table's row `i` carries global BGM id `2000 + i` | falsified (one id is missing from the run) | Plausible: the 71 rows open at `2000` and the sound-test track numbering is `2000 + i` everywhere else. The rows carry `2000..=2043` and then `2045..=2071`: sound-test track 44 has no row, so above the gap the cursor and the id it installs differ by one. Deriving an id from the cursor names the wrong track for 27 of the 71 rows. |
| Retail's per-voice reverb mask is `0xC081`, routing voices 0, 7, 14 and 15 | falsified (that word is `SPUCNT`) | Plausible: the trace field is named `reverb_mode`, the value is stable across every frame, and `0xC081` reads as a sparse voice mask. The PCSX-Redux SPU-ports blob is the hardware window `0x1F801C00..0x1F801DFF` **verbatim**, so offset `0x1AA` is `0x1F801DAA` = `SPUCNT`: enable, unmute, reverb-master, CD audio. The real `EON` two words earlier reads `0x00FFFFFF` - all 24 voices - on every frame of the same capture ([settled](re-settled-threads/audio.md)). |
| The engine routes no voices through the reverb tank | falsified (both shipped hosts do; the **oracles** did not) | Plausible: an engine-side trace reports `EON = 0` on every frame, from the engine's own code. Both hosts build their SPU through `StreamResampler::new` -> `set_retail_reverb`; a trace or PCM oracle that builds a bare `Spu` measures an engine nobody runs. An oracle that constructs its own subject has to construct the shipped one. |
| The engine runs about half retail's concurrent voices | falsified (two different pieces of music) | Plausible: mean 4.83 against 9.78 over the same scene name, twice, is the shape of a real deficit. A save's *scene* does not decide what it is playing - `_DAT_8007BAC8` does. The retail `town01` capture holds `2000`, the overworld track (the save walked in from the world map), while the engine trace plays the `2016` the scene's own prescript selects. Same track, comparable stretch: 9.67 / 18 against 9.78 / 19 ([settled](re-settled-threads/audio.md)). |
| Retail doubles about one note in six across two voice slots | falsified (slots-per-note is the arrangement's, not a side's) | Plausible: retail `1.158` against the port's `1.002` looks like an allocator difference. On a track-aligned pairing the ratio **reverses** - retail `1.002`, the port `1.037` to `1.061` - so the statistic belongs to the piece. Nor is the doubling town SFX in the retail window: a second capture in the same town, on the same walk, reports `1.002`. |
| The `nilboa` duel states read BGM `4096` | falsified (they read `2009`; the `4096` states are the ending ones) | Plausible: most states carry `2000..=2068`, so a `4096` in the corpus reads as an anomaly belonging to whichever states were being opened. `4096` = `0x1000` is the **park sentinel** both streaming slots share; it appears where a save has no field track owed - the endings ([settled](re-settled-threads/audio.md)). |
| `FUN_8001E54C`'s types `2` / `0xC` are VRAM rect uploads, and cases `1` / `3` decode LZS | falsified (SEQ installs and VAB transfers; no LZS arm) | Plausible: the installer is reached from scene loads that also upload textures, and a staging copy looks like a blit. Type `2` detaches and closes the SEQ player, copies the payload into the slot's staging buffer and opens it with `SsSeqOpen` through `FUN_80026410`; `0xC` opens the fixed record `0x800705AC`; `1` / `3` are `FUN_8002630C`. All 90 disc streams with a score past offset 0 carry it as a type-2 chunk ([settled](re-settled-threads/audio.md)). |
| `FUN_801D4A60` parks on a BGM request / ack pair | falsified (the side-band sound-bank pair) | Plausible: the guard at `0x801D4B58` polls a request word against an acknowledge, the BGM handshake's shape. The pair is `_DAT_8007BABC` / `_DAT_8007BAA0`, the side-band bank the engine runs as `World::audio.sound_stream` ([`field-locomotion.md`](../subsystems/field-locomotion.md#openers-and-closers)). |
| Every `music_01` entry is a self-contained `[VAB][SEQ]` pair | falsified (75 of 81 are) | Plausible: every entry that carries a track is a pair. The installer walk over all 81 finds slot 72 a score with no bank, 76..79 one-sector fills and 80 a bank with no score ([settled](re-settled-threads/audio.md)). |
| `FUN_8003EAE4` streams a whole XA clip | falsified (it only seeks) | Plausible: it takes a clip id, sets the XA drive-state word and is called on every battle action that voices a line. It issues `CdlSetloc` then `CdlSeekL` (`li a0,0x15` at `0x8003EB68`), installs no callback and issues no read, and its callers store `0` into `_DAT_8007BC20` straight after; the XA ring is installed only by `FUN_8003D53C` (`0x8003D714` / `0x8003D72C`). |
| `FUN_8004DA00` arms on `ctx[+0x276] != 0` | falsified (zero passes) | Plausible: a non-zero byte reads as an "enabled" gate. `beq v0,zero,0x8004DA60` at `0x8004DA50` continues when the byte is zero and sends a non-zero byte to the `-1` latch store - the same polarity as `FUN_8004FCC8`. |
| The field init's slot-10 load is a bank of ending-scene sound effects | falsified (it is the credits theme) | Plausible: slot 10 sits among the SFX slots and the load runs only in two ending scenes. The arm starts a sequence from the loaded score itself and latches the BGM loader off; see [settled](re-settled-threads/audio.md). |

### Op-`0x35` sub-op 9 was never a queue

**Tempting reading.** Field-VM op `0x35` sub-op 9 stashes a BGM track for a
later trigger, and scene entry is that trigger. The op sits next to the pause /
resume / stop control words, a scene's *entry* script never uses it, and its arm
opens by comparing two globals - which reads as "is a slot free yet?".

**Why it is wrong.** The arm at `0x801E0224` compares `*0x8007BAB8` (the index
the resolver produced) against `*0x8007BA9C` (the index actually loaded). The
mismatch branch goes to `0x801DEE4C`, which is `move s8,s4` - the dispatcher's
restore-PC idiom, so the script re-runs the same instruction next frame. That
is a wait on the asynchronous asset load. When it clears,
`sw v0,-0x4538(a1)` writes `*0x8007BAC8 = id`, the identical store sub-op 1
makes. Sub-op 11 (`_DAT_8007BA9C = -1`) is the barrier's arming half.

**What is true.** Sub-op 9 is sub-op 1 plus a wait, and it is what a
**cutscene** changes music with mid-scene. A scene-corpus BGM sweep cannot see
the difference: it runs only prescripts, which emit sub-op 1 exclusively. The
audible symptom of the queue reading is a cutscene that plays silent while its
score starts over the *next* scene the player walks into. Full arm and the
port's routing:
[`script-vm.md`](../subsystems/script-vm.md#sub-op-9-is-a-start-not-a-queue).

### The 256-slot SPU-address run that was really a CLUT

**Tempting reading.** A `[u32 mode == 2]` header followed by 256 `u16`s all
`>= 0x8000` identifies a packed monster sound bank, and the one entry matching
it is `h:\mpack\monster.snd`. `0x8000` is the boundary an SPU sample address
clears once the reserved low region is skipped, the leading `2` reads as a
format word, and the matching entry's CDNAME label names the sound cluster.

**Why it is wrong.** The match is `summon.dat` (extraction 893,
[`summon-readef.md`](../formats/summon-readef.md)): header word mode `2`, then
`0x200` bytes of **BGR555 CLUT with the STP bit forced on every non-zero
entry**, which sets bit 15 of all 256 halfwords. The values **repeat**
(`0x8000 0x8000 0x8000 0x8000 0x8001 0x8001 ...`), and SPU sample addresses are
strictly increasing - a monotonicity check rejects it where a threshold check
cannot.

**What is true.** `monster.snd` is extraction **891**: `FUN_8003E104` does
`li v0,0x37d` (raw TOC `0x37D` = extraction 891) beside the
`h:\mpack\monster.snd` path string (see `ghidra/scripts/funcs/8003e104.txt`).
Entry 891 is a 206-bank multi-VAB archive, so the monster SE bank is a
**multi-bank VAB**, not a bespoke address table; attributing the
`vab_multi_bank` class to "the `level_up` cluster" reads the same CDNAME `+2`
shift off an extraction filename
([`cdname.md`](../formats/cdname.md#numbering-space)). The
`monster_sound_bank` class is kept and pinned at zero matches so the shape
stays named. A threshold predicate over a fixed-size run identifies a shape,
not a format; where the shape encodes an ordering (addresses, offsets, LBAs),
assert the ordering.

### bse.dat: three readings the gp+0x678 trace overturned

*Falsified by disassembly.*

| Reading | Why it looked right | What is true |
|---|---|---|
| `bse.dat` record `+4` is a `u32 v` taking only 0 and 2 | Reading `+4..+7` as one word; `+5..+7` are zero in every retail row | `+4` is a `u8` category (the VAB-slot selector) plus three bytes no reader touches; "0 and 2" describes the authored defaults, and the cue router rewrites the byte per cue |
| `bse.dat` is loaded once at sound-init and held for the session | `FUN_8001FA88` has the shape of a sound-init routine and also loads the per-set `.dpk` | Its one caller on the whole disc is battle init `FUN_800513F0`; the bank reloads per battle, and the buffer slot `0x8007B8D0` is repointed at every field load |
| The `bse_bank` detector's "`u32` at `+4` under `0x100`" gate is a value bound | It reads as a range check on the trailing field | A category byte can never exceed `0xFF`, so the bound can only fail on a non-zero trailer - it is a zero-trailer test |

The consumer looks untraced because every reader forms `0x8007B990` with
`lui` + `lw`, a pair the five-form address scan does not accept. Details:
[`re-settled-threads.md`](re-settled-threads/audio.md#bsedat-record-columns-and-the-gp0x678-consumers).

### `bse.dat` carries a second record family with a resident consumer

*Falsified by disc bytes.*

**Tempting reading.** Bytes repeat on a fixed stride inside the loaded buffer,
and a matching run in a second entry looks like a shared footer - a second
record family whose consumer is still to find.

**Why it is wrong.** Both runs are the same builder's fill inside `VagAtr` tone
rows belonging to a *different file* left in the sector: 888's tail equals
886's and 1063's; 1062's equals 1056's.

**What is true.** There is no second family and no consumer. The companion
reading - that `FUN_8001FA88`'s **dev** branch loads PROT `0x37A`, tying the
`.dpk` name to the `sound_data2` family - fails the same way: that is the
retail branch, and `0x37A` is `bse.dat`. See
[`re-settled-threads.md`](re-settled-threads/audio.md).

## Title / boot / overlays

| Thread | Verdict | Why |
|---|---|---|
| `FUN_801D362C` is a cutscene dialogue routine | falsified (the move VM's `0x2F` extension dispatcher) | Plausible: it sits among routines a cutscene exercises. Its only reference is the move VM's op-`0x2F` arm; sub-op `0x2F` (`0x801D45D4`) stores the frame-step floor, which is how `opdeene`'s prescript installs its cadence of 3 ([`actor-vm.md`](../subsystems/actor-vm.md)). |
| The narration crawl's geometry is a per-scene table, with 18 px line spacing in `opdeene` | falsified (a config block seeded by the script; pitch fixed at 16) | Plausible: a pixel capture fits each scene's window and speed, and `opdeene`'s lines measure about 18 px apart. The roller reads `*0x801C6EA4` `+0x4C..+0x52`, which `FUN_8003A024` resets and a `CC F8 E8` op seeds before each block; the pitch is `addiu s3,s3,0x10`, and no retail store produces 18. |
| The title menu's third row opens Options | falsified (the title menu has two rows) | Plausible: the port's `TitleOutcome` enum carries an `Options` arm. The tick wraps the cursor with `andi v1,v1,0x1` at `0x801DDC00` and the confirm branches on row 0 against everything else; retail reaches Options from the pause menu. |
| The native window draws the boot Options screen twice, and its tests read the unframed copy | falsified (it drew once, unframed; no test read it) | Plausible: two copies of one screen is a common host-drift shape. The defect is a single unframed draw: a bare session painted at a fixed pen that never reaches the framed screen. |
| Mode 18 has no hand-off store, so the front-end chain skips it | falsified (`0x801CE8DC` in PROT 0902) | Plausible: a three-form scan for stores to the mode word finds one for every other INIT handler and none for this one. The store is `li v0,0x13` at `0x801CE8D4` and the store at `0x801CE8DC`, inside `0x801CE844`; the scan's corpus has to reach PROT 0902 to see it. |
| The retail title screen runs under game mode `0x10`, which leads load / new-game -> field | falsified (the title's own mode is the **card** mode `0x17`) | Plausible: `0x10` is a real mode on the path from boot to the title, so a trace that samples the mode word early sees it. `0x10` is one frame of logo INIT. The chain is six stores, each written by the handler that hands off rather than by any table's `next` field: `0x8001D5B8` -> `0x10`, `0x801CEC94` -> `0x11`, `0x801CF4D4` -> `0x16`, `0x80025974` -> `0x17`, `0x801DFC00` -> `0x02`, `0x80025E50` -> `0x03`. |
| The Muscle Dome is not an OTHER-game mode | falsified for the **hub**, true for a round | Plausible: a dome fight is an ordinary battle under the battle mode. The dome *hub* - the contest screen between fights - is PROT 0977, sub-id 5, game mode 24, the OTHER-game mode. |
| A mode-change edge clears three `gp` words | falsified (four) | Plausible: three of the four sit adjacent at `gp+0x538` and `gp+0x55C` and read as one cleared block. The edge at `0x800161F4` also clears `0x8007B938`; `gp+0x564` and `gp+0x494` are mode **copies** and are not cleared. |
| Title sub-mode `0x10` selects between a prompt and a menu | falsified (it is one state) | Plausible: the screen visibly changes between a "press start" prompt and a two-row menu. Retail's `0x10` is a single state that polls the card, polls Start and runs the cursor; the prompt-vs-menu split is a port-side phase and is not retail's. |


### The title sub-mode word lives at `0x801DD920`, and `0x02` is a screen a player can see

*Falsified by disassembly, with a cold-boot capture agreeing.*

**Tempting reading.** `FUN_801DD35C`'s init writes sub-mode `0x02`, so `0x02`
is the cold-boot screen, and the word it writes is at `0x801DD920`, the address
the dump prints on that line. `0x02` really is a complete two-row menu (rows at
y 107 and 120, confirm mask `0x44`, advancing to `0x14`).

**Why it is wrong.** `0x801DD920` is the **instruction** address of the
`sw v0,0x204(a2)` that performs the store, with `a2 = 0x801F0000` from a `lui`
four instructions earlier. And the `0x02` store is overwritten with `0x11`
whenever the entry word `_DAT_8007BB00` is non-zero, which the boot `init.pak`
raises unconditionally at `0x801CEB84`.

**What is true.** The sub-mode word is `0x801F0204`, and a per-vsync cold-boot
poll never observes `0x02`. See
[`re-settled-threads.md`](re-settled-threads/title-boot-overlays.md#a-cold-boot-always-shows-title-sub-mode-0x10).

### The attract sequence can be armed from any title sub-mode

*Falsified by disassembly.*

**Tempting reading.** The attract countdown at `0x801EF16C` is a state-struct
field, and several arms of the tick touch state-struct words, so any idle
sub-mode lets a global timer fire.

**Why it is wrong.** The countdown is decremented, and its underflow writes
`_DAT_8007B83C = 0x1A`, only inside the extent `0x801DDB0C..0x801DDD94`; every
other arm reaches the shared epilogue without touching it.

**What is true.** That extent is sub-mode `0x10` `AttractIdle` and nothing
else. The preceding sub-mode `0x11` spends a *different* accumulator
(`_DAT_8007BAB4`) and hands to `0x10`, which makes the two look like one timer
in a capture.

### There is exactly one master-mode-`2` writer, at `0x801DFC00`

*Falsified by disassembly.*

**Tempting reading.** `0x801DFC00` is in `LaunchGame` (`0x06`), the NEW GAME
exit, and a single "leave the title into the field" writer is the shape the
mode graph suggests.

**Why it is wrong.** `LaunchFade` (`0x16`) writes the same master mode at
`0x801DFAFC` on the **load** route (`state[-0xEA8] == 1`).

**What is true.** There are two writers, one per route, and both arms clear
`_DAT_8007BB00` as they go. The second is the CONTINUE path - the one a
save-file boot takes.

### The title slider `state[-0xEB4]` is clamped to `[0, 0x2C]`

*Falsified by disassembly.*

**Tempting reading.** `0x2C` appears as a bound in both of the slider's arms,
and a value bounded above by `0x2C` with a natural floor of `0` reads as a
range.

**Why it is wrong.** The decreasing arm subtracts `frame_scalar << 3`, then
`slti v0,v0,0x2c` / `beqz`: a result **below** `0x2C` is forced back **up** to
`0x2C` (`0x801DFC78..0x801DFC98`). The increasing arm adds the same step and
forces anything at or above `0x2D` back **down** to `0x2C`
(`0x801DFCA4..0x801DFCC4`). There is no `0` floor, and the seeds the graph
writes are `0x100` (`0x801DD88C`, `0x801DE094`) and `-0x16` (`0x801DECB8`) -
both outside the supposed range.

**What is true.** Both arms converge on the single value `0x2C` from their own
side: the cell is a settling animation parameter, not a bounded slider
position.


### The title screen is loaded before the mode table is consulted

*Falsified by disc bytes and `main()`'s disassembly.*

**Tempting reading.** None of the 28 mode-table rows carries a name resembling
"title", so the screen runs ahead of the dispatcher from its own
pre-mode-dispatch boot load.

**Why it is wrong.** The one pre-loop overlay load in `main()` is
`0x8001612C jal 0x8003ebe4` with `a0 = 0`, which is extraction 0895
(`init.pak`, the publisher logos) reached by mode 16 `READ`.

**What is true.** The title overlay PROT 0899 is loaded by mode 22 `CARD`
(`0x800258B4`, `a0 = 4`) and its tick runs as a spawned actor under mode 23.
See
[`re-settled-threads.md`](re-settled-threads/title-boot-overlays.md#title-screen-mode-table-prot).

### `FUN_801DD35C` lives in an unindexed PROT.DAT gap between entries 899 and 900

*Falsified by TOC arithmetic and a byte search.*

**Tempting reading.** The tick's bytes sit past where entry 899 seems to end
and before 900 begins, so they occupy a gap the TOC does not index.

**Why it is wrong.** An entry's size is the sector span to the next, so the TOC
partitions `PROT.DAT` without gaps (899 ends at sector 47301, where 900
begins). The coordinate comes from the superseded entry-size expression.

**What is true.** The tick's 48-byte prologue occurs exactly once on the disc,
inside entry **0899** at file `+0xEB44`.

### `FUN_8003EAE4`'s flags are consumed by an untraced CD driver

*Falsified by disassembly.*

**Tempting reading.** The routine stores three flag cells and nothing visible
reads them, so a CD driver outside the dumped corpus must.

**Why it is wrong.** `gp+0x910` and `gp+0x890` have no reader anywhere in SCUS
or the 1233 PROT entries - every access is a store.

**What is true.** The CD-callback sequencer is `FUN_8003D764`, and it reads
none of the three cells; it dispatches on `gp+0x928`, which only
`FUN_8003D53C` writes and which `FUN_8003EAE4` merely reads as a no-op entry
gate. See
[`re-settled-threads.md`](re-settled-threads/battle.md#fun_8003eae4-is-a-seek-plus-bookkeeping---and-the-driver-is-not-untraced).

### `0x801CE9C0` is an entry point in no image, so mode 16 is a stripped dev path

*Falsified by disassembly.*

**Tempting reading.** SCUS `FUN_8002612C` (mode 16) jumps to `0x801CE9C0`,
which sits mid-way through the debug-menu overlay's `FUN_801CE97C`, so the mode
is a retail-stripped path jumping into slot A blind.

**Why it is wrong.** It is VA aliasing at the shared slot-A base: the debug
menu is only one tenant of that address.

**What is true.** `0x801CE9C0` is a clean function entry in PROT **0895**
(`init.pak`, file `+0x1A8`), the publisher-logo pass that mode 16 exists to
run. See [`boot.md`](../subsystems/boot.md).

### `FUN_801CE9C0` draws the publisher logos

*Falsified by disassembly.*

**Tempting reading.** The routine handles all four logo TIMs, so it is the
logo draw.

**Why it is wrong.** Its four `FUN_800198E0` calls are `LoadImage` wrappers
over rects it forms from each TIM's header, and the routine contains no
primitive emit.

**What is true.** It uploads them. The quads come from the sprite-descriptor
table at `0x801F369C`, emitted by `FUN_801CFBB8`
([settled](re-settled-threads/title-boot-overlays.md#the-publisher-logo-quads)).

### SCEA unfolds as a 2x2 grid of 32-row strips

*Falsified by disassembly.*

**Tempting reading.** A `(2, 2)` strip grid fits the visible content, and it
is pixel-equivalent on screen.

**Why it is wrong.** The fit is to the pixels, not to the descriptors retail
emits.

**What is true.** Retail's descriptors 2 and 3 are two 64-row halves of the
256x128 TIM, and PROKION's halves are 127 rows, not 128.

### `0x801D06E0` is a SHARED_TAIL with no `jr ra`

*Falsified by disassembly.*

**Tempting reading.** The worklist row classifies it as a tail exiting
`j 0x801dee50`, which is what the address holds in another slot-A tenant.

**Why it is wrong.** Same cause as the `0x801CE9C0` entry above: the address
is read in the wrong image at the shared base.

**What is true.** In PROT 0895 at the same base it is a 22-instruction leaf
with its own `addiu sp,sp,-0x18` frame, four `TestEvent` calls and its own
`jr ra` at `0x801D0730`.

## Containers / placeholder slots

| Thread | Verdict | Why |
|---|---|---|
| PROT 0897's `0x801F2E94` table has a six-byte stride | falsified (73 ten-byte sound-test rows) | Plausible: the bytes repeat on a short period. The consumer indexes `(n + 1) * 10` and wraps on the last row's `'X'` sentinel. |
| PROT 0897's `0x801F3340` run is pointer tables | falsified (window programs) | Plausible: the run is full of in-image words. They are eight-byte `[i16 opcode][i16 window][u32 operand]` instructions for the overlay's own interpreter `FUN_801E9B3C`, and the operands are what look like pointers. |
| PROT 0945 loses nothing to the donor cut | falsified (it loses a credited pointer like the other five) | Plausible: its donor routine's three spawn calls resolve to nothing when read at the call. `0x801F7F2C` completes each pair in the call's delay slot; read there, they resolve to three of the donor's records, which the content-end cut drops. |
| PROT 0967 carries a 2,992-byte un-dumped code run | falsified (92 bytes of one leaf, a 1,757-byte prompt pool and 1,143 bytes of PROT 0966's tail) | Plausible: the shape classifier calls the window `plausible_mips`, and the image is a slot-B module outside the dumped band. The window is mostly another module's bytes, visible as such once the inherited tail is cut; `disc-coverage.py` reports no un-dumped code there. |
| The PROT 0898 head block carries a `switch` jump table from `0x9B4` | falsified (nine runs of jump-table words below `0xDF8`, not one) | Plausible: one run of in-image VA words after a string pool reads as one table. Two of the nine runs have a bound `jr` consumer (`0x801EA9FC` over 179 arms, `0x801EB558` over five). Nine is itself a run count, not a table count - see the twenty-two-table row below. |
| PROT 0970 is the world-map top-view debug image | falsified (it is the STR / MDEC FMV overlay; the top-view image is 0981) | Plausible: both are slot-A images at the same base, and both sit in a CDNAME block whose labels inherit forward. 0970's own operands are the STR play loop's - the decode context at `0x801D19A0`, the two slice buffers, the STRv2 VLC table - and 0981's are the world map's. |
| PROT 0974's 10 KB of readable text is a string blob | falsified (an 81-record `0x84`-stride roster) | Plausible: the run is legible text with separators, and the text is Japanese in a USA-build image, which suggests a foreign build on top. The loop operands in `FUN_801CED68` walk a fixed stride from `0x801CEF40`, and 34 of the image's 37 SCUS `jal` targets land on this disc's function heads (a genuinely foreign image scores 0 of 42). Shift-JIS stored as LE `u16` is evidence of an encoding only. |
| PROT 0975's `0x801D4138` run is a jump table, or data, or a function interior | falsified (it is PROT 0972's code) | Plausible: each reading fits one property of the run - no prologue, no `jr ra`, no caller. The packer's buffer is indexed by file offset and never cleared, so a short entry ends in the previous image's bytes at the **same file offset**: the 1,760 bytes from `0x5920` are byte-identical to 0972 there. Byte accounting does not cut such a tail the way disc coverage does, so the run reads as this image's residue. |
| `FUN_8001E890`'s own frame keeps the registrar away from a foreign block | falsified (the checksum guards the other buffer, on the other arm) | Plausible: the routine re-sums PROT `0x36C` and compares against the boot-time sum at `gp+0x6B8`. It sums the **VRAM read-back** it is about to decompress from, not the pack the registrar walks; and the register-only arm never reaches it, because `bne v1, v0` at `0x8001E974` jumps past the sum whenever the load word is `1`. The gate's writers are what keeps the walk safe. |
| A rebuilt PROT 0874 with a different section-0 decoded size can be hand-built to reproduce the wild read | falsified (the container is not constructible that way) | Plausible: the symptom shows on a rebuilt container, and "make the section bigger" is what a size-sensitivity test looks like. The LZS decode is length-driven by the descriptor and `FUN_8001ED60` sizes the section-0 / section-1 buffers from the container header words at `gp+0x69C` / `gp+0x6C8`, so a header-byte-exact rebuild whose decoded size differs yields a **truncated** pack. No shipped patcher path changes that size. The producer to bracket is retail's own unclamped registrar over a battle-load pointer. |
| A scene bundle's descriptor offsets can fall outside the entry | falsified (all 668 are inside) | Plausible: a format that stores absolute offsets into a streamed container produces exactly that. The out-of-range offsets are measured against the over-reading entry-size expression; against the corrected extent every descriptor in all 102 tables lands inside its own entry. |
| PROT 0874's `meta[1]` (`0x2CBA0`) is a VDF tail offset past the LZS payload inside the entry | falsified (the entry is `0x19800` bytes; `0x2CBA0` is 78 KB past its end) | Plausible: the battle loader does walk a flat `[count][offsets]` pack through `FUN_8001FBCC`, and the character pack does carry a second meta word shaped like an offset. The loader's constants `0x368..0x36B` are raw TOC `872..875` = extraction `870..873`, so the pack it walks is `vdf` (extraction 872), while the character pack is extraction 874 = raw `0x36C`. And `0x2CBA0` is the **sum of the descriptors' decompressed sizes** (`0xB49C + 0x41E0 + 0x1D524`). See [`character-mesh.md`](../formats/character-mesh.md). |
| The dome panel still is uploaded as 320x64 rows stepped down in `y` | falsified (four 64x256 columns) | Plausible: a 320-wide destination and a y-stepped upload is how the neighbouring `int.tim` family (`0x4C7` / `0x4C8`) loads. The measured upload is **four** `LoadImage` calls of 64x256 at `x = 384 / 448 / 512 / 576` - texture pages 6..9 - keyed on raw TOC `0x36C`. |
| The `scene_v12_table` load path reads the `.MAP`, the `.PCH` and `efect.dat` | falsified (that is the **dev** branch) | Plausible: the three-file read is in the routine, and nothing in it is marked dev. The fork is on `_DAT_8007B8C2` at `0x8001F87C`, and retail takes the other leg: one `FUN_8003E800` read of `0x28` sectors from `0x8001F9A4`. `FUN_800608F0`, reached from the dev leg, is the `break 0x103` host trap. |
| Both `record[0] + 0x5C` accesses live in PROT 0900 | falsified (one is PROT 0901) | Plausible: the two hits sit close together in a window read as one entry. One is past 0900's real end and belongs to 0901 - the entry-size over-read. A sweep of 518,656 words across 84 images finds no reader of that displacement on a `record[0]` at all. |
| There is no `jal` at `0x801F78D0` in the world-map overlay | falsified (the test read the delay slot) | Plausible: the address holds no call, which is a clean negative. The call is at `0x801F78CC` and `0x801F78D0` is its delay slot; three slot-B images hold one. The conclusion the negative supported happens to survive. |
| Pochi-fill slots are stale mastering scratch, and some parse as valid TIMs | falsified (every slot is one 2048-byte sector; 0 of 266 carry a TIM) | [details ↓](#pochi-fill-slots-as-stale-mastering-scratch) |
| The world-map kingdom bundle is PROT `0085` / `0244` / `0391` | falsified (it is `0086` / `0245` / `0392`) | [details ↓](#assets-named-by-the-entry-the-over-read-window-started-in) |
| The battle-form character pack holds seven atlases inside PROT `1204`, the last truncated, with CLUT row 496 skipped | falsified (eight whole atlases in PROT `1205`; 496 is the eighth, not a gap) | [details ↓](#assets-named-by-the-entry-the-over-read-window-started-in) |
| The title TIM ships as three multi-bank duplicates in PROT `0888` / `0889` / `0890` | falsified (one copy, in `0890` at `0x14228`) | [details ↓](#assets-named-by-the-entry-the-over-read-window-started-in) |
| `scene_tmd_stream` entries can hold two or more concatenated sub-streams (the "two-list" shape) | falsified (one stream per entry; 0 of 182 hold a second) | [details ↓](#concatenated-sub-streams-in-a-scene_tmd_stream-entry) |
| The stage backdrop renders as half a bowl because bytes are missing - mirror it to recover them | falsified (the half is authored; 182 of 182, and nothing is unread) | [details ↓](#the-backdrop-shell-is-drawn-once-so-no-completion-exists) |
| ...and therefore nothing completes it, so drawing a second copy is a regression | falsified (retail links **two** backdrop actors; the second carries a per-stage transform) | [details ↓](#the-backdrop-shell-is-drawn-once-so-no-completion-exists) |
| PROT 0968 is a 4 KB module (pointer-table head, 10/11 self-pointers, 2+8 spawn calls) | falsified (its own content is 2600 bytes; the rest is stale buffer) | Plausible: the entry really is 2 sectors. Only file `0x00..0xA28` is 0968's; the trailing 1496 bytes are 0967's bytes at the *same* file offsets, cut mid-string at the sector boundary, and **nothing in 0968's own window references them** - no `jal`, no `j`, no materialisation. The quoted structural figures span both modules. Full accounting on [`re-settled-threads.md`](re-settled-threads/title-boot-overlays.md#prot-0968---the-cort-battle-stage-overlay). |
| The literal `0x801F69D8` in `SCUS_942.54` is a cross-image reference naming 0968's loader callsite | falsified (it is the slot-B base constant) | Plausible: it is the only literal-word hit outside the shared-base band, so an aliasing argument cannot dismiss it. It is the SCUS global `0x80010390` holding the **slot-B overlay load address**, twin of `0x8001038C` for slot A, read by `FUN_8003EC70` and never written - it names the slot, not a tenant. The real callsite is not findable by constant: the stage-overlay parameter is **computed** (`stage_id + 0x47`), so `0x49` occurs nowhere. |
| Battle `DAT_8007BD0C == 0xB5` at `0x801E6D04` is a test on the Lapis Wave **spell** id | falsified (it is the **formation monster** id - Cort) | Plausible: spell `0xB5` is Lapis Wave, and Cort is its caster. Formation `0xB5` is monster-archive 181, Cort. The byte the branch reads is `*(u8 *)0x8007BD0C`, the formation id array, and its guard is an HP-reached-zero test on the first enemy actor - a form-transition trigger, not a cast. |
| The Muscle Dome `INTERVAL` screen is the `(384, 0)` 320x256 still | **reversed** (true on a re-entered hub; the first visit draws a live render) | A first-visit intermission is `koin1`'s own scene, with no primitive sampling a page at `x = 384`. Visits two and three of a dome run read latch `_DAT_801D1AE0 = 1`, hub arm `0x0A`, level `8`, with both still packets (tpages `0x106` / `0x109`, colour `0x2C080808`) in the primitive pool, emitted by `FUN_801D00F8` in PROT 0977 ([settled](re-settled-threads/battle.md), [`ringside-still.md`](../formats/ringside-still.md#on-a-natural-re-entry)). Anchor kept: [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-interval-screen-is-a-live-render-not-the-still). |
| `init.pak` carries five logo TIMs | falsified (four) | Plausible: five plausible TIM headers parse out of the region. The fifth sits **inside** logo 3's pixel data, where image data produces a header-shaped run. Counting parseable headers counts images only if each parse consumes what it claims. |
| The type-`0x14` FLAG descriptor is a property of every count-4 and count-5 bundle | falsified (28 of 105 tables, at every count from 4 to 7) | Plausible: every count-4 and count-5 bundle in a small sample carries the FLAG slot last. Across all 105 descriptor tables the FLAG slot appears on 28, whose counts run 4 to 7, and in all 28 it is the **last** entry. The position is the rule, not the count. |
| `urudre1`'s 584 unexplained bundle bytes are sector padding | falsified (562 of them are high-entropy residue) | Plausible: the run is smaller than a sector, sits at the end of the entry, and the pack's payload hashes identically to the standalone copy either way. The payload claim holds exactly: the pack's last member's TIM closes on byte 343,480, the same SHA-256 as `0456_urudre1.BIN`. What follows is the packer's buffer, an earlier entry's bytes at the same offset ([settled](re-settled-threads/measurement-corpus.md)). |
| The PROT 0898 head is nine jump tables | falsified (twenty-two; two more sit above it) | Plausible: nine runs of in-image VA words is what a scan of the bytes shows. Tables that abut with no pad word merge into one run; read off their dispatches - each `sltiu` bound times four - twenty-two bases tile the head, 850 arms. |
| A scene bundle's last-sector residue is its walker's unread tail | falsified (it is the packer's buffer) | Plausible: the bytes sit right after the last stream the walker parses. Byte `k` of that run is byte `k` of the nearest earlier TOC entry whose extent reaches `k`, for 80,337 of 80,337 bytes across 90 bundles, and the same holds for the `lzs_container`, `pack` and `bse_bank` tails. The donor need not be an overlay. |
| The packer-buffer run is confined to an entry's last sector | falsified (PROT 0976's tail is `0x98C` bytes of 0970) | Plausible: every scene bundle's run fits its last sector. Searching the suffix over the whole entry reproduces 82 of the sibling rule's 83 cuts. |
| PROT 0970's 3,152-byte run above the init flag is uninitialised data, or code | falsified (two MDEC command packets and the register-pointer block) | Plausible: the image has a real `.bss` hole just below, and the shape classifier calls the run `plausible_mips`. `0x801D0D58` is the quant packet (`0x40000001`), `0x801D0DDC` the IDCT packet (`0x60000000`), and `0x801D0E60..0x801D0E9B` fifteen DMA / MDEC register pointers. |
| `FUN_801CFCDC` stages MDEC output rects into `0x801D0D5C` / `0x801D0D9C` | falsified (it uploads the quant tables) | Plausible: two sixteen-word destinations filled from a caller pointer look like a rect pair. They are the luma and chroma matrices of the quant packet behind the header at `0x801D0D58`. Nor does it copy both packets: only the quant packet is written; the IDCT packet at `0x801D0DDC` is static and is sent as is. |
| PROT 0899's 3,552-byte zero run at `0x1EB28` is uninitialised data | falsified (inter-asset fill) | Plausible: a long zero run inside an overlay usually is `.bss` (0970's is). No instruction in any image forms an address inside this one; it lies between the save-menu atlas and the save-slot icon sheet. |
| PROT 0967's prompt pool has no consumer | falsified (twenty-eight strings formed at forty-one sites) | Plausible: no literal pointer word names it. The module's own code forms each string address with `lui`/`addiu` between file `0x278` and `0xA7C`. |
| A slot-B image never calls itself with `jal` | falsified (PROT 0967 does) | Plausible: it holds across the cast band, which is why function extents there are recovered by frame matching. 0967 reaches its frameless leaf `FUN_801F7628` by `jal` from `0x801F7184` and `0x801F7460`. |
| PROT 0900 and 0901 are shifted copies of one image | falsified (an old entry-size artifact) | Plausible: a byte comparison of the two matches. It compares 0901 with itself through the over-reading entry size; 0901's own content ends at file `0x252A`, where 0900's bytes begin. |
| PROT 0901's inherited tail starts at `0x26B0` | falsified (`0x252A`, donor 0900) | Plausible: `0x26B0` is the sibling rule's cut. 0901's code ends at `jr ra` on `0x24DC`, and the run from `0x252A` opens mid-routine on 0900's epilogue and is referenced only from 0900. |
| PROT 0780 (`edteien`)'s residue is a scene-event-script walker's tail | falsified (the walker never started) | Plausible: the class is right and the bytes are unclaimed. Its prescript holds two records, below the standalone count floor, so a content-only walker refuses the entry; the loader reads the records positionally, and so does `record_ranges_positional`. |

### Assets named by the entry the over-read window started in

*Status:* falsified - each asset is where it always was; only the `(entry,
offset)` name for it was wrong

**Tempting reading.** An asset is recorded as "PROT `N` offset `K`" because a
scan positioned on entry `N` finds it there, and each such name comes with
corroboration:

- The kingdom bundle "at `0x1800` of entry 85" has a table there, with the
  right count and the right first descriptor offset.
- The battle-pack atlases have a consistent stride from a consistent base, a
  truncated last member is normal at the end of a container, and the missing
  CLUT row reads as a deliberate gap in a 490..497 run.
- The title TIM's three "duplicates" are **byte-equal**, which is what a
  multi-bank duplicate looks like.

**Why it is wrong.** Under the superseded entry size a reader positioned on
entry `N` sees entries `N+1`, `N+2`... The coordinate is right about the disc -
`start_lba(N)*0x800 + K` is where the bytes are - and wrong about which entry
owns them. Entry 85's `0x1800` is entry 86's offset 0, and the block layout
(`.MAP` / v12 header / prescript / bundle) makes entry 85 the prescript.
`0x25804` is 1204's own length plus 4, i.e. entry 1205 offset 4; "seven" and
"truncated" are both where the window stopped. The three title-TIM arithmetics
resolve to one absolute offset.

**What is true.** The kingdom bundles are `0086` / `0245` / `0392`; the
battle-form pack is eight whole atlases in `1205`, CLUT row 496 being the
eighth; the title TIM is one copy in `0890` at `0x14228`. Byte-equality between
two `(entry, offset)` pairs shows duplication only once the pairs resolve to
different absolute offsets, and a member count is a property of the framing (a
chunk chain, a descriptor count), not of where a buffer ends. Corrected
coordinates and the two invariants that hold them:
[`prot.md`](../formats/prot.md#a-entry-offset-pair-is-only-a-coordinate-if-the-offset-is-inside-the-entry).

### Pochi-fill slots as stale mastering scratch

*Status:* falsified - the corrupting pages came from the **next** entry, reached
through an over-reading size expression

**Tempting reading.** Reserved-but-unused filler holding leftover bytes from an
earlier master is an ordinary thing to find on a PSX disc, and the hazard has a
reproducible exhibit: two `64x256` pages uploading to framebuffer `(768,0)` and
`(832,0)` erase a ground atlas on every run, with the sweep positioned on a
pochi slot.

**Why it is wrong.** Every one of the 266 `Class::PochiFiller` entries is
exactly one 2048-byte sector of fill, and **none** carries a parseable TIM
header. There is no stale image in a pochi slot to upload.

**What is true.** The corrupting pages belong to the `scene_tmd_stream` entry
that *follows* the pochi slot; a sweep reaches them through the entry-size
expression that spans into neighbouring entries
([`prot.md`](../formats/prot.md)). An over-reading reader makes the next
entry's bytes look like the current entry's content, and a symptom that
reproduces every time confirms only that something is wrong at that step, not
which entry owns the bytes. Slot contents: [`pochi.md`](../formats/pochi.md).

### Concatenated sub-streams in a `scene_tmd_stream` entry

*Status:* falsified - one entry holds one stream; the "second sub-stream" is the
next PROT entry

**Tempting reading.** `0006_town01` is two concatenated
`[chunk0 TMD][type-0x01 TIM chunks][terminator]` sub-streams, the second at
`0x14000` with its own leading TMD `0x2c20` and TIM chunks at `0x16c24` /
`0x1ee48`. The second block opens on a `0x800` boundary after zero padding and
carries a valid Legaia TMD followed by two well-formed type-0x01 chunks. The
walker `FUN_8001FE70` returns `param_1 + 1`, just past the terminator, which
reads as the hook a sector-indexed "multi-sub-stream caller" would use. The
shape reproduces exactly on the town0b and town0c clusters.

**Why it is wrong.** Entry 0006 is exactly `0x14000` bytes, so the second block
is PROT entry **0007**, whose own leading TMD is `0x2c20` and whose tail chunks
are at `0x2c24` / `0xae48` - the recorded offsets minus the length of entry
0006. The replication does not test the shape: those clusters are four-entry
runs of one layout (TMD bodies `0x383c` / `0x2c20` / `0x2998` / `0x3af8`, two
`0x8220` TIM chunks each), so every over-read spills into a sibling of the same
shape.

**What is true.** Across the corrected corpus, 0 of 182 `scene_tmd_stream`
entries hold a second sub-stream and 0 yield a post-terminator chunk; there is
no multi-sub-stream caller to find. A structural feature that only ever appears
at the end of a buffer is a claim about the reader's bounds until it is shown
somewhere that is not immediately before another entry of the same class.
`sub_streams` and `WalkSource::Continuation` stay in
[`scene_tmd_stream.rs`](../../crates/asset/src/scene_tmd_stream.rs) as
regression detectors, with disc-gated coverage in
`crates/asset/tests/scene_tmd_stream_real.rs`. Corrected layout:
[`scene-bundles.md`](../formats/scene-bundles.md#one-entry-one-stream-the-falsified-two-list-shape).

### "The backdrop shell is drawn once, so no completion exists"

*Status:* falsified - retail draws the shell **twice**. The authored half is
real; "therefore nothing completes it" does not follow

**Tempting reading.** Any `scene_tmd_stream` entry opens in a mesh viewer as
half a bowl - a sky dome, a distant mountain ring and a far ground ring, sheared
off along a plane through the origin. Two readings follow, and both are wrong:

- *A whole map got halved, so find the missing bytes.*
- *The file holds a complete half and the runtime links one background actor,
  so no completion exists and drawing one is a regression.* This one has two
  supports. A `Ry(180deg)` duplicate plants a second village wall across
  `town01`'s open `-X` side, which in retail is open sea. And a four-angle
  stage-battle capture set reads the distant mountains as covering "44-81% of
  the horizon columns, not a ring".

**Why it is wrong.**

- *Missing bytes.* The half is authored and nothing is dropped. Over object 0,
  all **182** entries put at most **8%** of the shell's X or Z extent on the far
  side of `X = 0` / `Z = 0` (widest `0.079`, `0048_vell`); the open side is `-X`
  in 129 entries, `-Z` in 49 and `+X` in 4, never `+Z`, the side the party is
  seated on. Every one of the 378 objects has
  `vert_top + n_vert * 8 == normal_top` exactly, and the parsed body accounts
  for the whole declared chunk0 size: no unread vertex block, no second
  primitive list, no second sub-stream.
- *One actor.* `FUN_800513F0` registers the backdrop TMD **once** and allocates
  **two** actors from the same descriptor; the second carries a transform.
- *The `town01` wall.* `town01` (stage id 4) is on the mirror list: retail
  completes it by reflecting in the YZ plane. `Ry(180deg)` is the right
  transform for its siblings `0006` / `0009` and the wrong one for `0007` /
  `0008`. The artifact falsifies one transform on one stage, not completion.
- *The 44-81% figure.* It measures band *thickness* against a 9-18 px threshold,
  and the ring's height varies from a few pixels to ~45 across the arc. Measured
  for *presence* of a mountain band above the horizon, the four angles read
  **98 / 100 / 100 / 100%** of columns.
- *The captures refute a single copy.* Project `map01`'s drawn objects through
  each capture's exact camera (yaw `_DAT_8007B792`, pitch `32`,
  `TR = (0, 1280, 7680)`, `H = 256`, all read from the save state): one copy
  covers 100 / **71.9** / 100 / 99.7% of the 320 columns, two copies 100%
  throughout. Three yaws cannot separate the models. Capture **b**, at yaw
  334.7deg, can: one copy leaves columns `0..89` with no mountain geometry, and
  retail has a mountain band in **90 of those 90**.

**What is true.** Retail draws two copies of one authored half-shell, the
second under a per-stage transform. Mechanism, evidence and the per-stage
table:
[`battle.md`](../subsystems/battle.md#backdrop-shell---two-copies-of-one-mesh).
Two neighbouring things this is not:

- The `+0x10` mesh puzzle - walk-visible `.MAP` cells naming a pack mesh no
  layer draws. That family is `0x0011`; stamping it draws a wall down every
  river. What keeps it out is the `0x2000` draw gate its cells never carry, not
  the `FLAG_MESH_DRAWN` bit - see the world-map table above.
- The site's assembled map view or the engine's field renderer. Those exclude
  `scene_tmd_stream` entries and build the scene from the environment mesh pack
  plus the `.MAP` placements
  (`crates/web-viewer/tests/field_scene_assembly.rs`).

Before a capture statistic closes a thread, state the reading it would refute
and check that the samples can tell the two readings apart: a file sweep is
silent about the runtime, and a statistic pooled over angles hides that only
one angle carries information.

### "Field-pack" was never a format

*Falsified by disc bytes.*

**Tempting reading.** Magic `0x01059B84` opens a Legaia-specific bundle - a
97-entry strict schema, a byte-identical ~91 KB template block shared by every
carrier, and the per-scene payload in a preamble ahead of packed TIMs/TMDs. The
word occurs exactly once on the disc, the table after it is regular, and two
carriers do share tens of kilobytes byte-for-byte.

**Why it is wrong.** The word is a DATA_FIELD chunk header,
`(TIM_LIST = 0x01) << 24 | 0x059B84`, and `0x059B84` is town01's payload
length - unique because every carrier's length differs. The "schema" is that
chunk's `asset::pack` table `[u32 count = 96][u32 word_offset[96]]` (`0x60` is
the count), every member a PSX TIM; the clusters' "byte sizes" are word deltas.
The shared block is town01 and town0c carrying the same three leading Rim Elm
atlases. The "preamble" is the superseded over-reading entry size reading the
block's prescript and asset table ahead of the real entry.

**What is true.** It is a scene's texture pack behind a chunk header. A carrier
sits at raw-TOC `+4` of its CDNAME block, loaded by `FUN_800255B8` /
`FUN_8002541C`. See [`field-pack.md`](../formats/field-pack.md).

### The prescript's "per-scene secondary header" is the next entry

*Falsified by disc bytes.*

**Tempting reading.** After a `scene_event_scripts` prescript, a small
`(count, descriptor[count])` table sits at the next `0x800` boundary,
alternating `(type, size)` and runtime-buffer offset pairs - a per-scene
secondary header.

**Why it is wrong.** For 99 of the 101 carriers the first `0x800` boundary at
or past the last record is already at or past the entry's end - there are no
bytes there to be a header. The over-reading entry size appends the neighbour,
as with the `.PCH` "+0x800 prescript" and the pochi "stale TIM" readings.

**What is true.** For all 101 carriers the **next PROT entry** begins with
exactly that table at its own offset 0 (87 `scene_asset_table`, 14 the count-4
form). See
[`scene-bundles.md`](../formats/scene-bundles.md#scene_event_scripts---prescript-only).

### PROT 0892: a 12 MB LZS container, or a truncated DATA_FIELD stream

*Falsified by disassembly and disc bytes.*

**Tempting reading.** Two readings of one entry: a 12 MB container whose
content is unpinned, or a truncated DATA_FIELD stream whose final chunk the
runtime continues by DMA. The second matches the stream detector's own
criteria.

**Why it is wrong.** The 12 MB figure is the superseded
`toc[p+5] - toc[p+3] + 4` span (5,977 sectors). The three "leading chunks" are
an `asset::pack`'s header words (`2`, `3`, `0x208B`) read as chunk headers, and
the "over-large fourth header" is a word inside member 0's pixels.

**What is true.** The entry is 33 sectors, 67,584 bytes, and retail walks it as
a pack (`FUN_8002574C`, `0x8002581C..`). See
[`re-settled-threads.md`](re-settled-threads/text-dialog.md).

### PROT 1221 / 1222 have no loader, because no image names raw TOC `0x4C7` / `0x4C8`

*Falsified by disassembly.*

**Tempting reading.** A sweep of every image for the literals `0x4C7` / `0x4C8`
as a load index finds no loader, so the entries are unloaded.

**Why it is wrong.** `0x4C8` is never a literal anywhere; the index is
`s0 + 0x4C7` with a runtime `s0` (`addiu a0,s0,0x4c7` at `0x801F6C3C` in PROT
0978). A literal-only sweep cannot see a computed index - the same blind spot
as gp-relative addressing.

**What is true.** They are the dome's `int.tim` / `int2.tim` stills, loaded by
PROT 0978.

## Field / locomotion

| Thread | Verdict | Why |
|---|---|---|
| korb3 parks on a modal timeline at `pc=0x0144` and edlast's helper spins on a jump-to-self | falsified (the ladder printed the entry script's pc) | Plausible: the ladder report prints a parked pc for each scene. That pc is the entry script's, not the pad holder's. In korb3 the holder is `P2[15]`, sign text at the picker seat (the first-visit `P2[1]` at retail's arrival tile); in edlast it is `P2[1]`, a credits record of about 14100 vsyncs that ends on a held-pad poll. Neither is a port stall. |
| Critical-path rung 5 reopens with a route avoiding keikoku's inner doorways or a flag seed | falsified | Plausible: a route round the beat bands, or a seeded flag, would skip the turn-back. `P2[7]` has no header gate and every arm pushes the player back out; a flood that avoids the beat bands reaches no second door from any of the four mouths. |
| Circle casts in retail fishing | falsified (Cross / Square cast; Circle abandons) | Plausible: Circle is the port's cast binding and a confirm on many PSX screens. The cast start (`0x801CF99C`) and the power lock (`0x801CFBA4`) test `& 0xC0`; Circle appears only in the shared tail's abandon edge `& 0x21` at `0x801D0318`. The port keeps Circle as a port binding. |
| Fishing menu rows 2 / 3 snapshot the points bank | falsified (they seed the tackle cursor) | Plausible: the rows open the point-exchange screens. They copy the lure index `_DAT_80084450` into the shared sub-screen cursor `0x801D90DC` (`0x801D0680`); the port field keeps its old `snapshot_points` name. |
| Only cutscene-class records lock the player's locomotion | falsified (every stepped record does) | Plausible: the port locked the pad only under a modal timeline, and a helper record looks like background work. The per-actor tick steps every spawned record with `+0x100` through `FUN_80039B7C`, which holds the engaged bit the pad controller tests; being modal decides only the camera. |
| Motion-VM ops `0x37` / `0x41` are TranslateY / TranslateX | falsified (one compass-walk arm) | Plausible: two ops that each move an actor along one axis. Both land in one arm of the walk kernel `FUN_8003774C` (`0x8003789C..0x800379F8`): the axis pair comes from the table at `0x80073F14[b0 & 7]`, the rate is `0x80` or `0x40`, and the budget is `(b1 & 0x3F) * (4 << sel)`. |
| `A2 F8 01` is map01's cave-mouth walk-out | falsified (it picks the walk clip) | Plausible: it sits where the walk starts and names the player channel. It only selects the clip; the move is `B7 F8 00 81`, a compass walk of 16 vsyncs. |
| Critical-path rung 5 (pad-walking the Ravine out of another door) clears | falsified (retail turns the player round) | Plausible: the rung passes when keikoku `P2[7]`'s inner-doorway band is crossed. That record's first-visit arm ends in `B7 F8 00 C1` at `+0x12D` - a compass walk one tile back - and the pass depends on playing it as a one-tick yield. With the walk played the run stops at (56, 28). |
| The motion-pause kick `FUN_8003C9AC` pauses NPCs while a dialogue runs | falsified (it is a standing-clip request) | Plausible: it fires when an interaction engages and reloads `+0x5C` / `+0x88` on every moving-class actor. It copies byte `0` of the actor's `0x801C6470` record - the standing move - into the requested-move pair, and `FUN_8003BC08` runs the scripted VM before the clip consumer `FUN_800204F8`, so a walker mid-step asks for its walk clip again before the request is read. The engine's patrol pause during dialogue is its own choice. |
| `+0x10 & 2` in `FUN_8003BC08`'s height arm is a freeze an actor sets on itself | falsified (it is the visibility cull's bit) | Plausible: the arm skips on it and nothing in the motion ops writes it. `FUN_801D79E8` rewrites it on every call - set outside the region box or the view window, clear inside (`0x801D7B08..0x801D7B30`) - and a write-watch sees both stores fire for every `town01` placement. |
| The NPC glide rate is "pad-held × 6" | falsified (the frame-step scalar × 6) | Plausible: `0x1F800393` has been catalogued among the pad words. It is the per-frame tick byte, read by `lbu 0x393` at `0x8003BCAC`. |
| `FUN_801D79E8` is a draw / unlink / movement-init helper that emits glyph cells | falsified (the per-actor visibility cull) | Plausible: the two dumps named after the address show glyph-cell calls and a digit field. Neither holds this routine: `funcs/801d79e8.txt` is `FUN_801D6E18`, and `overlay_0897_801d79e8.txt` starts mid-instruction. The real body - matching `overlay_cutscene_dialogue_801d79e8.txt` and the PROT 0897 image at `0x801CE818` - tests the region box and view window and writes `+0x10` bit 1. |
| Rula / Riremito are dev-table-only handlers with no player-facing installer | falsified (the Door of Wind / Door of Light run them) | Plausible: no word or `lui`/`addiu` pair anywhere on the disc references their id-table slots. The table at `0x801F33B4` is indexed by **value** (`FUN_801F159C`, `jalr` at `0x801F1634` on `+0x50`): the pause-menu session `FUN_801ED308` installs `0x29` / `0x2B` from its phases 6 / 7 after a Door's Use screen returns code 4 / 5. |
| The travel arts scan a visited-map table | falsified (the resident CDNAME define table) | Plausible: the scan looks up a map number and prints `UNFIND MAP NUMBER` on a miss. `FUN_80019788` returns `0x80088758`, the count is `0x8007B806`, and each `0x10`-byte record is a scene name with its `s16` define at `+0xC`; a retail image holds 125 of them, `init_data` through `other7`. |
| The Door of Light returns the party to the tile it last stood on | falsified (the region record's triple) | Plausible: from a cave the destination is next to the entrance. It is `0x80084628` / `24` / `2C`, which the region record wrote - `0x55 @ (37, 109)` in `cave01`, though the party entered from `(37, 110)` - refreshed by `FUN_801F1278` at `0x801F12F8` when the menu button is pressed; the capture sees the triple change on that frame. |
| The entry banner is `FUN_801EE5D4`'s panel script `0x801F32B4` | falsified (the MAN loader spawns it) | Plausible: the actor reads the scene name `_DAT_801C6EA0` right beside the script call. The script is one `op 5` (close every panel) and a terminator; the actor copies the name into `_DAT_8007B44C` for the save screen. The banner is the `4C E1` balloon `FUN_8003AEB0` spawns at `0x8003BB40..0x8003BBDC`, armed by system flag `2`. |
| `0x800228B0..0x80022B80` is the anim tick's dispatch-`5` handler | falsified (it is the default motion block that arms 3 and 5 skip) | Plausible: the range follows the dispatch compare ladder. Arms 3 and 5 both branch past it (`beq` to `0x80022B80` at `0x800228A8` / `0x800228B0`); arm 5 is `0x80021FB4..0x800226D8`. The handler reading also runs the block for dispatch 3 whenever `+0x9C` is zero, which retail does not. |
| `kor5`'s `0x619` is set by a talk body, or never by retail's entry | falsified (a spawn-section write) | Plausible: a cold entry sets it, a post-chain capture holds it clear, and a probe watching a retail entry sees no set. `P1[2]`'s `SET 0x619` sits before its first `0x21`, so `FUN_8003A1E4` runs it at each MAN-loading entry; `FUN_8003AEB0` skips the spawn loop on a same-scene reload (`0x8003B8A0`). Re-running the section on every talk is wrong. |
| `FUN_801D1EC4`'s warp timer walks the actor to its destination, and the crossing tile is `(0x8007BDD8, 0x8007BDDC)` | falsified (the timer only counts; the crossing tile is `BDC8` / `BDCC`) | Plausible: read from a dump printed at the phantom VA `0x801C36AC`. The timer half tags an actor and subtracts the frame delta; the landing seats the player once. `0x8007BDD8` is the clip base. |
| `FUN_801D1EC4` is the clip base's writer, on four arms | falsified (the pad step writes it every frame; `FUN_801D1EC4` on one arm) | Plausible: four `sw ...,-0x4228` stores sit together from `0x801D21AC`. Three are in the hop phase machine `FUN_801D2298` (`6` / `7` / `1`). The pad controller (`0x801D0424..0x801D04A4`) and the system channel (`0x80039D94`) also write it. |
| Player `+0x5C` is a dash counter and `+0x10 & 0x1000000` an interact request | falsified (the clip id, and the party-bank select) | Plausible: the controller reads `+0x5C` as a gate and raises the bit on moving frames. `FUN_800204F8` binds `+0x5C` as a 1-based record id and tests the bit at `0x8002053C` to pick the party bundle. |
| The player walks in place through a kind-0 warp | falsified (the player idles) | Plausible: the pad controller is skipped for the warp, so the last clip it picked looked like it would stay bound. The system channel stores base `2` every tick at `0x80039D94`, so the settle binds idle; a capture holds clip id `2` for the whole warp with Down held ([`field-locomotion.md`](../subsystems/field-locomotion.md#retail-capture-of-the-warp)). |
| Holding run never changes the player's clip | falsified (the capture held the wrong buttons) | Plausible: a locomotion capture held a run and saw the walk record throughout. It held Square and Circle; the run mask is `0x48`, Cross or R1, and a watch under R1 or Cross sees base `3` and clip id `3`. |
| `FUN_801d5718` is the field's walkability sampler, and the ignore list's `801d5718` a grid copy | falsified (the sampler is `FUN_801D56C4`; `0x801D5718` is an interior address) | Plausible: `0x801D5718` is a real instruction inside the sampler, so a citation at it reads correctly. It is `FUN_801D56C4`'s row-index `sll`; the routine other images carry at `0x801D5718` is the battle image's placement landing copy, which copies `+0x02` / `+0x04` from `src+0x0A` / `+0x0C` plus `+0x06`, `+0x0A`, `+0x14`. |
| A failed op-`0x43` acquire skips the op | falsified (it waits and retries) | Plausible: advancing past the op keeps a script moving. A failed acquire branches to `0x801DEE4C` (`move s8,s4`), leaving the PC on the op. |
| The inn conversation ends because the re-acquire fails once the window has closed | falsified (it ends at the last page's close) | Plausible: the record's tail loops back over the acquire, and a refusing acquire leaves the caller halted. In `retock_innkeeper_talk_open` the cursor parks on the loop-back when the last page closes, and the next talk's acquire at `0x801E2148` succeeds (`s7 = 5`). `0x801E1ECC..0x801E1F54` is sub `0`'s arm, not this one. |
| `*(_DAT_801C6EA4) + 8` is a modal-window flag | falsified (it reads `0` through a whole conversation) | Plausible: it gates the dispatcher's halted-target early-out, which a conversation needs skipped. Across both `retock` conversations it reads `0` on every sampled vsync; its writers set it only around a placement's spawn-section pre-run (SCUS `0x8003B73C` / `0x8003B928`, `0x801E2820` / `0x801E2BBC`). |
| `FUN_801D1344` in the world-map overlay is different code from the dialog overlay's | falsified (one routine of PROT 0897) | Plausible: the dumps carry different capture labels, and the world map reads like its own overlay. The world map is a subsystem of PROT 0897; the `world_map`, `dialog` and `cutscene_dialogue` dumps have one extent and the same calls (`0x801D1470`, `0x801D15B0`, `0x801D16F4`) - the player actor's tick. |
| The play page and the native window walk `town01` along different paths | falsified (both stop at `(2588, 2898)` under one pad script) | Plausible: a native reference frame disagrees with the page. `field_walk_host_parity.rs` drives both hosts from one script to the same stop; the disagreeing reference came from a stale binary (`inference`). |
| Op `0x3E` with `op0 < 100` is the field interact that arms an actor's interaction script | falsified (it installs a scripted battle, the same body as `op0 = 0xFF`) | Plausible: `0x3E` with `op0 >= 100` is a door and `0xFF` a boss fight, so the remaining range looks like the talk case, and the pointer it stores in `sys[+0x94]` looks like a script. The arm reads `op0` only at `0x801E06FC` / `0x801E0704` to fork off the door-warp; the pointer is a MAN formation-table row, and the body ends in the reroll and the battle mode request. None of the ten non-`0xFF` sites opens dialogue ([settled](re-settled-threads/field.md)). |
| `FUN_801E58A8` seeds a list's row count | falsified (it picks an actor's anim clip) | Plausible: `+0x5C` looked like a count and `_DAT_8007B8F8` like a page count. The value lands in the word the clip selector `FUN_800204F8` binds, the same arithmetic ends `FUN_801D1BA0` on the player, and `_DAT_8007B8F8` is the party leader, whose times-7 is the stride into the locomotion clip bank. |
| `FUN_801D25EC`'s chained second actor is an emitter | falsified (a release watcher) | Plausible: a second allocation after a motion helper reads like a particle trail (the port routine keeps the name `spawn_arc_with_emitter`). The second template `0x801F22AC` carries handler `FUN_801D5D60`, which clears the halt bit `0x400` when the arc lands and optionally runs the follow-camera ease. |
| `kor5`'s P2[5] needs about 17,500 vsyncs to write `0x436` | falsified (3,336 after `0x464` clears) | Plausible: a probe run stalls that long without the write. Its `!464` leg counts its own pokes instead of reading the flag, so after the battle's reload it pokes the trigger tile again, re-dispatches P2[4] over the running P2[5] and locks the player. |
| Nothing frees the tile board's cell buffer | falsified (teardown state `0xE` frees it) | Plausible: the per-scene control-block reset zeroes `_DAT_8007B450` and never touches `DAT_801F35C0`. The walk SM's state `0xE` calls `FUN_80017B94` on the cell buffer and the tile-actor table (`0x801EFE78` / `0x801EFE88`). |
| `4C 86` sites are talk records that install the mirror on an interact press | falsified (they install from the spawn prologue at scene entry) | Plausible: the records are player-raised and sit next to talk scripts. All ten sites run before the record's first `0x21` park. A run that seats only one of three controllers is a port defect - a channel stepper that moves the channel list out while a script runs makes every cross-context id resolve to nothing - not evidence of a press. |
| A tile poke cannot cross the world map, and `kor5`'s P2[4] / P2[5] never spawn | falsified (both walk; the pokes landed during the movement lock) | Plausible: pokes made while the player is locked fail, and a crossing under the lock is consumed, not deferred. A poke after the lock clears dispatches P2[4] at once, and `korb2 -> kor -> korout -> map03 -> doman` crosses by pokes alone. The `0x6C4` writer is P2[8], not P2[4] / P2[5]. |
| `0x8007C348 + 4*i` is a per-channel actor pointer table | falsified (a free-stack index, seven list sentinels and the player slot) | Plausible: index 7 lands on the player. `+0x00` is the free-stack top, `+0x04..+0x24` seven list sentinels written by `FUN_8001E1B4`, `+0x1C` the player (written by field MAIN_INIT at `0x801D6D7C`), `+0x28` on the 143-entry free stack. |
| Motion-VM ops `0x37` / `0x41` chase a target along one axis | falsified (an eight-direction compass walk) | Plausible: each moves the actor toward something, and a one-axis reading produces plausible walks. The step comes from the compass table at `0x80073F14`. |
| `_DAT_1F800384` is a packed camera word | falsified (the scratchpad region box `x0, z0, x1, z1`) | Plausible: the camera path reads it every frame. PROT 0901 restamps its low two bytes at `0x9A0..0x9AC` on the world map, which a camera word would not survive. |
| Field-VM `4C 14` is seven bytes, like every other sub-op of its nibble | falsified (it is eight) | Plausible: outer nibble 1 advances `s8` by seven in its own prologue, so every arm under it looks fixed-width. The `0x14` arm at `0x801E0E80` reads `lbu a0,6(s6)` and exits `j 0x801E3624` with `addiu s8,s8,1` in the branch delay slot, so the eighth byte is consumed even when the source id resolves to nothing. A seven-byte executor desyncs 94 sites in six scenes from their first occurrence (the disassembler decodes eight, so listings never show it), and `0x08` is not an opcode the VM has an arm for. |
| `4C 87` registers a callback, and both it and `4C 9F` park the script until it fires | falsified (both retire, both advance) | Plausible: `FUN_8003CF40` takes an actor list and a handler VA, the shape of a registration API. The routine walks the list and ORs `8` into `+0x10` on every match - a retire sweep that writes nothing else - and the shared exit `0x801E2DC4` carries `addiu s8,s8,2` in the call's delay slot, so the advance has already run. Fifteen scenes issue `4C 9F`, 140 times; a parked reading strands every one. |
| `4C 9F` and `4C 87` sweep the same handler | falsified (they sweep different ones) | Plausible: the two arms are the same five instructions. The VA each forms differs - `0x801E2548` forms `LAB_801DA930`, the floor-height-ladder oscillator, and `0x801E2284` forms `0x801E5154`, the reflection controller's tick. Reading `4C 87` as a ladder retire attributes a scene's mirror teardown to an elevation LUT. |
| `FUN_801E573C` is a six-axis rotation setter, and its `+0x90` is the source | falsified (both ends were backwards) | Plausible: the spawner's argument order puts the executing context first and the resolved actor second, and six `s16` into consecutive halfwords read as a transform. The tick decides which end is which: `FUN_801E5154` loads `+0x90` into `a3` and `+0x94` into `a2`, reads `a2` and writes `a3`. The named actor is the source, the executing script is the image, and the six words are the controller's mirror line and tracking rect. |
| The reflection callback has no spawner on the disc | falsified (the spawner is `FUN_801E573C`) | Plausible: a reference scan for `0x801F2950` finds nothing. That is the descriptor's handler word; the descriptor base `0x801F2948` is materialised at `0x801E5780` by an ordinary `lui`/`addiu` pair. Scan for a table's base, not a word inside it. |
| A context's `+0x10` player bit means the context carries the player's pose | falsified (it means the player raised the record) | Plausible: the bit is set on exactly the contexts a player-facing beat runs in, so "this context is the player" gives the right answer wherever one actor is involved. Every shipped `4C 86` sits in a player-raised talk record naming `0xF8`, so that reading pairs the player with themself and the reflection tick walks them onto the mirror line each frame (two cold spawns end inside a wall). The destination is the executing channel's placement; the bit only decides when no channel executes. |
| An all-zero op-`0x34` sub-0 operand is a ramp target (fade to black) | falsified (it clears the effect) | Plausible: the arm's other operands are an RGB target, so all-zero reads as "target black", which is also what a fade-out looks like. The arm stores zero to the live-actor cell instead (`sw zero,-0x49d4`) and leaves without spawning, so the beat ends rather than ramping. A white target under blend 2 also loses an eighth of its duration: a capture reads 57 frames for the word `0x41`. |
| The renderers read an `effect_tint` ramp, so the fade's two representations have one live reader each | falsified (neither had a reader) | Plausible: a ramp is the representation a renderer would naturally consume. No renderer reads such a ramp, so swapping one representation for the other repairs nothing - neither has a reader. The measured beat is a `(kind, blend, packed)` triple, and that push is the model the port keeps. |
| A Rim Elm house door is a scene change | falsified (an intra-scene warp) | Plausible: the screen fades, the camera cuts and the player is somewhere else, as in every scene change. The scene name word does not move - `town0c` stays across the door - so a capture staged on one says nothing about scene boundaries. |
| Flag `0x5D6` has no script writer | falsified (`koin4` `P1[15]` self-latches it) | Plausible: the script census, a native flag-helper sweep, the move-VM ext sub-ops and the motion-VM census all come back clean. The census walk stops at the record's first `0x1F` text segment, and a raw scan for the bytes `D6 05` misses the encoding `55 D6`. Behind the text sit `48` and `55 D6`, with a `P2[3]` twin. The native negative stands. |
| The port's `FIELD_DEFAULT_VIEW_WINDOW` is the window retail stamps on scene entry | falsified (it is a later region's) | Plausible: the value is read off a live walk, so it is a real retail window - but not the entry one. A poll across a `map01` -> `town0c` door catches the entry stamp at `(-7, -6, 5, 7)`, 78 vsyncs after the scene word flips, with `(-8, -6, 6, 10)` arriving later as the player crosses into another region. |
| Nothing in 84 images references `0x801D5C08` or `0x801D5D60` | falsified (both are template handler words) | Plausible: an all-forms reference sweep returns clean. Run `--tables-only`, it drops hits classed as incidental code - and these two are data words, the `+0x08` handlers of the field-overlay templates at `0x801F227C` and `0x801F22AC`, spawned through `FUN_80020DE0` at `0x801D245C` / `0x801D2634` / `0x801D57C0` and `0x801D2760`. |
| teien's hedge-base cells are filled by an unpinned kind-2-cell draw channel | falsified (retail draws nothing there either) | Plausible: the hedge rows carry object-grid bit `0x0800` and not the `0x1000` ground-draw bit, the port's ground pass gates on `0x1000`, and the result is visible black under the hedges - so an unfound emitter keyed on `0x0800` explained the symptom exactly. A live `teien` field pass visits 1536 window cells and emits 370: every `0x1000` cell and none of the 42 `0x0800`-only cells. Only 8 of 84 images touch `*(0x1F8003EC)` at all, only PROT 0900 / 0901 hold a per-cell pass, and each one's `andi 0x800` reads an **object record's** `+0x12`, not a cell bit. |
| `FUN_801DA390` eases a camera yaw | falsified (a vertical offset) | Plausible: the routine is a two-input ease inside the camera controller, and a yaw is the camera quantity most often eased. `0x801DA3B4` reads `ctrl[+0x4A]`, `0x801DA3B8` reads the actor's `+0x16`, and `+0x16` is the Y of the position triple, so the target is a height difference. Port: `ease_camera_offset`. |
| Actor `+0x16` is a heading / a facing / a footing byte | falsified (it is Y, and it always was) | Plausible: three consumers of the field each suggest a different quantity from their own context. Nothing on the disc masks `+0x16` as an angle: 0 of 77 accesses in PROT 0897 are angle-masked, and SCUS's four masked accesses are `actor[+0x96]`. |
| The dance step markers are the gap in `field_actor_plan` | falsified (a different pool) | Plausible: the markers are per-cell field actors and `field_actor_plan` is the per-cell actor planner, so a missing marker draw looks like a missing plan row. The markers come from their own per-cell pool; a planner row for them draws nothing. |
| `FUN_801D6058` handles a cutscene element | falsified (a field-overlay template, spawned once) | Plausible: it is reached from a spawn table and its `+0x1A` arm looks scene-scripted. It is the `+0x08` handler of the plain descriptor at `0x801F271C`, spawned exactly once by the field MAIN INIT `FUN_801D6704` at `0x801D6FD8` and gated on `_DAT_8007B8B8 == 0`. |
| `Insn::extended` carries the field VM's op-`0x43` sub-op | falsified (it is a cross-context target marker) | Plausible: the field is set on exactly the instructions that have a sub-op, so a census keyed on it returns a plausible site list. The value is `0x80`, a marker meaning "this op targets another context"; the sub-op is `InsnInfo::ActorCtrl`. Counted by sub-op the family has 311 sites across ten ending scenes. |
| The fishing bring-up rewrites the lure index | falsified (the rod index) | Plausible: two nearby routines each probe a small band of bag items and each rewrite a persistent index, so one description covered both. `FUN_801CF070` walks `0xA0 + _DAT_80084454` (rods); `FUN_801D712C` walks `0x9D..0x9F` and rewrites `_DAT_80084450` (lures). Two indices, two item bands. |
| The `.PCH` sidecar's `+N` fixup words are filled in at runtime | falsified (nothing writes them) | Plausible: the header is a runtime-fixup header and the words are zero on disc, which is what an unfilled fixup slot looks like. All 97 on-disc tables carry zero there and no writer exists, so the words are reserved, not deferred. |
| "~270 undumped field-overlay functions" (recomp dispatch-entry seed list) | falsified (not a function inventory gap) | [details ↓](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds) |
| Rim Elm's reachable south-gate band force-walks the player through the wall | falsified (the record is five inert bytes) | [details ↓](#the-reachable-bands-record-force-walks-the-player-through-the-wall) |
| Scene-bundle type-6 descriptors are "all small placeholders" | falsified (12 are walker tables) | Plausible: the modal slot is a 4-byte `count = 0` filler (85 of 97 bundles), and the three kingdom tables read as a "kingdom slot 5" special. The 80 / 172 / 516-byte type-6 payloads (`garmel`, `dohaty`, the `geremi` / `rayman` / tunnel / `son` / `edson` family) parse as the same CLUT-walk table, installed identically for every bundle - the water / waterfall shimmer. The `rayman`-family carrier is the count-4 MAN-less variant the strict detector rejects; resolve by type byte ([field-ambient-fx.md](../subsystems/field-ambient-fx.md#mechanism-1---the-scene-walker-table-bundle-type-6-slot)). |
| Move-VM loop op `0x19` "retires past itself (size 2), loops back to the saved PC" | falsified (both halves inverted by the C rendering) | Plausible: the decompiled C renders it that way. The raw arm (`80023070.txt` `0x800235DC` + the `0x80024150` epilogue) loops while the decremented count has not underflowed, retires on underflow with size 1, and loops back to saved + 2 (the epilogue adds `a2 = 2` after the PC store) - re-running the `0x18` would re-seed the counter forever. jou's 15-instance cycler fan-out is the disc witness. Likewise ext `0x1E` returns size 4, hidden behind a `func_0x801d4a3c()` label-call return ([move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#self-modifying-bytecode-ops-0x04--0x1b--0x1e)). |
| Field-VM op `4C` nE sub-3 "syncs the resolved actor's position to the active camera" | falsified (copy direction inverted) | Plausible: the handler tail (`0x801E3178..0x801E31AC`) does refresh the camera-scroll globals, but that tail is a player-ctx-only side path. The op body (`0x801E3108`) copies the operand-resolved actor's `+0x14/16/18` position and `+0x26` facing into the executing ctx - the seat primitive of every mid-visit crowd swap (dolk2 `P2[11]`'s eight `CC <crowd> E3 <day>` pairs). See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging). |
| Extraction-0874 §2 F-variant pixels are written by a pause-menu-path uploader (and then: are a parked wrap-scroll phase) | falsified twice | Plausible: 6 of 6 pause captures hold the variant, and its 3 words equal row 273's content, which reads as a +2-row scroll park. The whole pause walk issues zero image transfers (DMA2 chain-walk + GP0 PIO hook) and plain field saves carry the variant; the strip is not shift-invariant and the wrap-scroll installer ops never fire across the s2 -> s3 flip window, so the row-273 equality is coincidence. The writer is the town01 opening record's one-shot `4C 60` face-frame stamp ([settled](re-settled-threads/field.md)). |
| Field-VM op `0x43` sub-3..6 is a **sound** register ramp: four target values, a `ticks` duration and a `curve` | falsified on all four counts (it is a camera-register *zone* ramp) | [details below](#op-0x43-sub-36-as-a-timed-sound-register-ramp) |
| Prologue gold grade = per-node `+0x74`/`+0x78` depth-cue crush | falsified (grade is a palette-space collapse; the nodes carry no `IR0`) | Plausible: `FUN_8002735C` does load per-node DPCS far colour + `IR0`, and the motion / move VMs carry op `0x0C` writers of those fields. The opening never uses them: a live recomp capture reads node `+0x78` (`IR0`) = 0 on every node at every beat, and the `opdeene` MAN motion section has no op `0x0C`. The grade is a load-time CLUT / TMD palette collapse `L=max(r,g,b) -> (L, max(L-1,0), L>>1)` ([cutscene.md](../subsystems/cutscene.md#full-scene-sepia-grade-the-gold-prologue-look)); the far-field crush is that law seen through dark authored gouraud. |
| `FUN_801DD784` is a cinematic **letterbox** | falsified (it is the scene **shutter blackout**) | Plausible: the tick eases two full-width bars in from the top and bottom of the screen, as a letterbox does. The bars do not stop: they meet in the middle and hold, and the template it ticks is `0x801F2858`, the one the field VM installs for a scene-change blackout. |
| `0x801F27EC` is the fade family | falsified (it is one rung of the scene floor-height ladder) | Plausible: the address ticks a small monotone value toward a target, the shape of every fade in this overlay, and sits in the band the fade actors are allocated from. The tick writes `0x1F800314 + 0x48 + actor[+0x50] * 2` = `0x1F80035C + rung * 2`, the scene's 16-entry elevation LUT, so what oscillates is a floor height. Installed by field-VM op `0x4C` nibble-9 subs `0..2` via `FUN_801DDE34`; see [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#nibble-9-is-the-floor-height-ladder-not-a-fade). |
| `FUN_801CFF3C` is a second spawner for the bar template `0x801F2858` | falsified (it is `FUN_801DE754` printed `0xE818` low) | Plausible: the dump writes `+0x54` / `+0x9E` and the operand triple exactly as the known spawner does, and a second spawner (a scene-entry default) is a reasonable shape. It is the known spawner: `0x801DE754 - 0x801CFF3C = 0xE818`, the field overlay's base-offset re-key, and the two dumps match instruction for instruction. The bar template has one spawn site, field-VM op `43 0C`. |
| `0x801D44CC` is a per-dancer facing script | falsified (it is the step-marker mesh flipbook) | Plausible: it is called once per dancer per frame and reads the dancer record, so a facing update fits. It indexes the marker actor's `+0x50` - the `clip - 6` value the floor pass stamps when it spawns a step marker - and selects a mesh row from it. Dancers are posed elsewhere. See [`minigame-dance.md`](../subsystems/minigame-dance.md#the-sprite-part-emit-dispatch). |
| `FUN_801D414C` runs on both dance edges and stages `other1` | falsified (one edge, and it stages nothing) | Plausible: the routine sits between the hall's enter and leave paths and touches the globals both do. It runs on the exit edge only, and it is a restore - it puts back what entering the hall displaced. Nothing in it names `other1`. |
| `_DAT_8007B880` is the dance pad latch | falsified | Plausible: the word changes every frame while a player is dancing, as a latched pad word does. The judge reads the ordinary per-frame pad edge words; `0x8007B880` is written by the hall's own animation clock and read by nothing that resolves a note. Gating a note on it accepts and rejects the wrong beats. |
| `0x801D518C` holds the literal `other1`, so the dance hall is `other1` | falsified (it is a BSS **saved-caller** name slot; the venue is `other7`) | Plausible: the cell holds an `otherN` string while the dance overlay is resident. The slot is where the entry path parks whichever venue name last passed through it, so it reads `other1` in a state that entered from elsewhere. The dance venue is `other7`, block base `0x4CC`. |
| `FUN_80019D50` is a BGR555 cell-grid emitter | falsified (it is the **CLUT-cell HSV cycler**) | Plausible: it walks a rectangular region of BGR555 halfwords and rewrites each one, a cell-grid emitter's inner loop. The halfwords are palette entries: the routine rotates hue / saturation / value over one CLUT cell block and pushes the result with a single `LoadImage` at `0x8001A030`, where a grid emitter would emit primitives. Port `engine-core::clut_cell_fx`; see [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-clut-cell-hsv-cycler-the-pulsating-flesh). |
| The world-map controller calls `FUN_801D362C` (the move-VM `0x2F` extension dispatcher) directly | falsified (one reference disc-wide, and it is the move VM's own arm) | Plausible: the world map animates through the same overlay-resident helpers the extension sub-ops wrap, so a direct call is a short path. The address has one reference on the whole disc - SCUS `0x80023AE0`, inside the move VM's op-`0x2F` arm. The world map reaches the dispatcher only by running a move-VM script. See [`move-vm-overlay-ext.md`](../subsystems/move-vm-overlay-ext.md#one-caller-and-it-is-ported). |
| Actor `+0x8A` bit 0 enables the scripted motion VM | falsified (it suppresses it) | Plausible: a per-actor byte tested at the top of a VM tick reads as an enable, and the actors that move do carry a non-zero byte. The gate is a `beq` at `0x80038194`, so a zero byte runs the bytecode; the bit gates the player-engaged / actor-busy / off-map early returns at `0x8003819C..F4`. The inverted polarity makes every un-flagged actor inert instead of ordinary. |
| Motion op `0x06` echoes the pad | falsified (a home-relative one-tile wander) | Plausible: the arm reads a live input-shaped source and writes a direction, which is the shape of a follow-the-player op. It draws `rand() & 6` and steps within a box of signed 7-bit tile deltas taken from the actor's home at `+0x8C` / `+0x8D`, so it never leaves one tile of where it was placed. |
| Motion op `0x0C` is a glide channel | falsified (it fades a tint and a draw mode) | Plausible: the op schedules a two-word ramp on the actor, and the neighbouring ops ramp position. The words are `+0x74`, the packed RGB tint, and `+0x78`, the draw mode - scheduler kind 3. The speed multiplier the locomotion code reads is `+0x72`, one field away. |
| The field player is seated by the cold-entry arm only at New Game | falsified (every ordinary scene change takes it) | Plausible: the arm is gated on a word a fresh boot leaves zero, so the gate reads as a boot-time discriminator. `FUN_801D6704`'s own epilogue clears the word at `0x801D750C`, so the next scene entry finds it zero again. The seat `(0xA40, 0, 0xA40)` is not the player's - it is the spawn of the one plain ambient template at `0x801F271C`; the player is seated on both arms, at `0x801D6F64` and `0x801D6F7C`, from the door operand. |
| Motion ops `0x0F`, `0x15` and the wander `0x06` have authored carriers | falsified (a census over every MAN finds none) | Plausible: the ops are decoded, dispatched and ported, and a decoded op with a table slot reads as a used op. Over all 573 MAN tail-section-1 variants, `0x0E` appears 215 times in four scenes and `0x13` and `0x16` in one scene each; `0x15` appears zero times, as do `0x00`, `0x06`, `0x0F`, `0x10`..`0x12`, `0x1A`..`0x1F` and `0x20`. No disc data reaches them, so a missing consumer is not a port gap. |
| `_DAT_8007BACC` has no writer, so the recentre-window arm is unreachable | falsified premise, surviving conclusion | Plausible: a five-form reference sweep returns nothing. There is a writer - `sw zero,-0x4534($v0)` at `0x801D6F50`, in the delay slot of the `jal 0x800567A8` before it, a position a call-target sweep steps over. The arm stays unreachable for a different reason: the only store is a zero. |
| `FUN_801DBE9C`'s zone query is how retail picks a camera region | falsified (that leg is dev-only) | Plausible: the arrival actor does run the query, load the hit and install the miss defaults, a complete and self-consistent camera account. The query sits behind `_DAT_8007B868 != 0`, the dev / dual-mode gate, which is zero in retail. Retail's query is the script-driven `FUN_801DE3E0`, reached from three field-VM arms and the op-`0x45` LOAD. |
| `FUN_801DBC20` writes the live camera globals | falsified (it writes the parameter block) | Plausible: it is the routine a camera-region record is handed to, and the globals change after it runs. It writes `0x8007B607..0x8007B627` and nothing else. The live globals are written by the ease `FUN_801DB510` and the snap `FUN_801DB8EC`, off a staging descriptor a third routine composes; loading the record alone yields a block, not a pose. |
| Nibble-3 sub-8 and sub-D are one helper with a sub-op discriminator | falsified (they call different routines) | Plausible: the two arms share the tile arithmetic exactly, and one host hook keyed on the sub-op byte gives the right answer for sub-8. Sub-8 is `FUN_801DE3E0`, the camera-zone query; sub-D is `FUN_800180EC`, the walk-region attribute refresh, which never touches the camera. |
| `[4C C4]` is a sub-tile broadcast | falsified (it is a camera-zone query at a named tile) | Plausible: the caller passes two coordinate bytes somewhere. The arm at `0x801E2878` calls `FUN_801DE3E0(x & 0x7F, z & 0x7F)`, the same query-and-load as nibble-3 sub-8, so a scene can frame a shot from a camera-region record the player is not standing in. |
| `[4C CF]` is a position broadcast | falsified (it is the script **camera-focus override**) | Plausible: the arm resolves each operand byte to a world coordinate, a tile centre or zero, as a position broadcast would. The values go to `_DAT_8007B628` / `_DAT_8007B62A`, and the focus clamp `FUN_801DAA50` stores their negation into the camera focus at `0x801DAB68` / `0x801DAB84` when non-zero - zero is "no override", which is why the arm clears both before writing either. |
| `_DAT_8007B628` / `_DAT_8007B62A` have no writer | falsified (six `sh` in one field-VM arm) | Plausible: an absolute-word reference scan returns nothing for either address. Both forms are `lui`+`sh` pairs, which a word scan cannot see; the `gp`-relative sweep finds all eight references - six stores in the `[4C CF]` arm and two `lh` reads in the clamp. |
| The port's compass residual is its pad quantisation | falsified (it is the scene's camera offset through pitch) | Plausible: a pad remap quantised to four cardinals does produce heading error, and zeroing the camera focus moves the failing figure. At orbit `0` the residual is a pure `+Z` world walk, which no heading quantisation can bend; what bends it is `town01`'s roughly 14.8-degree camera offset acting through the camera pitch. The 45-degree pad ring is correct and does not remove it. |
| Fifteen scenes raise the camera re-query flag | falsified (eight) | Plausible: fifteen scene MANs carry a word whose low five bits decode to `0x16`. Only eight carry the literal `[2E 16]` operand in a coherent record - `ropeway`, `station`, `tunnela`, `tunnelb`, `tunnelc`, `nilboa`, `nilboa2`, `noaru` - and the other seven sit inside desynced records. Count operand bytes, not decoded bits. |
| `0x801EFE7C` is a third octant derivation on the tile board | falsified (a restore) | Plausible: it stores into `gp+0x2D8` like the walker's two other sites. `0x801EF320` saves the octant into `0x801F35C4` on board entry and `0x801EFE7C` puts it back on exit; the walker's real second band is cells `0x0B..0x0E`, whose delay-slot `sll v0,v1,1` at `0x801EF8C8` re-uses `v1 = cell - 3` and lands on octants 0/2/4/6 after the `& 7`. |
| A black frame from a name-hijacked scene load says the scene is dark | falsified (the entry path is black, not the scene) | Plausible: rewriting a door's inline name does load the named bundle - the load trace's VAB, BGM and packet count all move with it - so a frame taken afterwards looks like a frame of that scene. A hijack into `bylon`, a scene the game draws brightly, measures identically black over 1000 vsyncs (non-black fraction 0.005 vs 0.003, the player sprite). From a world-map door the frame says nothing about the destination. |
| The `0x4C` outer dispatch bound is tested at `0x801E0C44` | falsified (one instruction later) | Plausible: the `srl v1,s3,4` that forms the nibble is at `0x801E0C44`, and a reader citing the arm's first instruction lands there. The `sltiu v0,v1,0x10` is at `0x801E0C48`, its `beqz` exit lands on the nibble-B error arm, and the table base `0x801CEE60` is the `lui`+`addiu` pair at `0x801E0C50`/`54`. |
| The two `0x4C` error arms share a preamble | falsified (a tail) | Plausible: both print through `jal 0x8001A068` with one string each, so one shared head looks like the natural shape. Each arm forms its own string pointer (`0x801CEC98` for nibble F, `0x801CECAC` for B) and F jumps into B's last three instructions at `0x801E3558`. No scene carries a coherent `4C Bx`; the 27 `4C FF` totals are all decode-desynced. |
| The fishing water class is read after the walk-grid probe reports the `0x4000` bit | falsified (two unrelated reads of the same point) | Plausible: both reads happen once per frame in the same tick, both take the lure's position, and `0x4000` appears in both - once as a cell-word bit and once as a buffer displacement. The water gate is bit `0x4000` of the `+0x8000` per-tile cell halfword (`andi v1,v1,0x4000` at `0x801D3374`), and its class word is rebuilt by `FUN_800180EC` at `0x801D3384`. `FUN_801D7030`'s probe over the `+0x4000` walk grid feeds the credit nothing: its hit drifts the lure's `x` accumulator by `frame_delta << 11`, signed by the low bit of the lifetime cast counter. |
| The Baka Fighter editor's actor record starts where its callback was cited from, or the band is five records from `0x801D7618` | falsified twice (eight, from `0x801D75DC`) | Plausible: a record's `0xFFFF0000` filler reads as its head, and `0x801D7618` - the fourth record's `+0x0C` - as a band start. Eight `0x18`-byte records tile `0x801D75DC..0x801D769C`, bounded below by an 8-byte-stride action-name string table and above by the `Vahn` roster pool, carrying `FUN_801CF388` / `801D3468` / `801D3390` / `801D6310` / `801D3F44` / `801D6F18` / `801D4FC8` / `801D49E8`. `FUN_80020DE0` spawns one per site: `0x801CF184` takes `0x801D75DC`, `0x801D01C4` takes `0x801D7624`. |
| `0x801D918E` is the fishing lure's z | falsified (it is the lure's **height**) | Plausible: the cell sits between the lure's x and a third halfword, so it reads as the second of a planar pair. The store is `sh a0, 2(a2)` at `0x801CFCEC`, and it writes the spawning actor's `+0x16` less `0x80` - a Y. The lure's z is the halfword at `0x801D9190`, whose 24.8 copy is `0x801D917C`. |
| The port's camera visible-tile window is seeded at scene entry | falsified (it was seeded once, at camera construction) | Plausible: retail stamps the default at scene entry and then one of two writers replaces it. A default stamped once in `Camera::new` is only ever overwritten, so a scene that scripts a wide window hands it to the next scene's clamp. The port restamps on each scene entry (test `scene_entry_restamps_the_field_draw_context_view_window`). |
| `resolve_field_slide` cannot be wired because its rests are pinned on the non-sliding stepper | falsified (both pinned legs are slide-neutral) | Plausible: a resolver that widens a held direction moves where a player rests against a wall, and wall-press oracles pin those rests. At both pinned rest positions the resolver returns exactly the held cardinal, so the oracles sit where the two models agree. Retail's slide fires in ordinary free-roam - 15 widenings in 276 calls down one town wall - and the port's locomotion step calls the resolver ([settled](re-settled-threads/field.md)). |
| The scene-authored octant word is read only inside the field overlay | falsified (its primary readers are in `SCUS_942.54`) | Plausible: the writer set is entirely field-overlay, and so are the two readers an absolute-word scan finds. The SCUS pad remapper `func_0x800467E8` loads the word twice (`0x800467E8`, `0x80046840`), which is what makes the octant a rotation; the walker's restore at `0x801EFE7C` is a writer the same scan misses. Both forms - `imm(gp)` and `lui`+load - are invisible to an absolute-word scan. |
| Retail's cold entry into `conc` runs `P0[34]` / `P0[36]`, whose body clears flag `0x6DE` | falsified (those are walk-on trigger scripts a cold entry never reaches) | Plausible: a fresh engine entry executes them and the clear is in their bodies. A write watch across retail's card-boot entry sees the clears come from `P1[1]`'s spawn prologue and the `P1[0]` entry script, and then `P1[0]`'s per-frame body re-SETs the flag every other frame from a player bounding-box test - so a state holding it set says where the party stood. |
| Seeding a system script's position anchor once at scene load is equivalent to reading the player each time | falsified (the script install resets it to the origin) | Plausible: the player does not move during a load, so one seed looks sufficient. The script install resets the ctx-`0xFB` context's anchor to the origin (a seed survives only in three opening scenes' load-frame pre-run), so with a one-time seed every per-frame `CD F8` box test disc-wide answers from tile `(-1, -1)`. Retail resolves `0xF8` to the live player on each evaluation. |
| The `juui1` walk is blocked by the pad ladder's budget | falsified (it is blocked by a record gate) | Plausible: a two-scene pad walk is long and encounters break it. `conc2` P2[20] and its spawners carry `C1 = [0x3E1]` / `C2 = [0x3E5]`, and the only card save that reaches `conc2` already holds `0x3E1` set, so no budget reaches the door; a tile poke crosses a door in about ninety vsyncs anyway. |
| `4C D8`'s model operand is a slot in the global TMD pool, and `balden`'s `99` / `100` / `109` / `110` are simply not in it | falsified (it is a **scene-bank** index) | Plausible: `FUN_801D77F4` reads `DAT_8007C018[slot]` with no adjustment. The op's arm has already added `*(u16 *)0x8007B6F8` at `0x801E2DE0..0x801E2DE8`, and read that way all seventeen blocks fit their mesh vertex for vertex. Read as raw slots, `jagaroom` / `garmel` bind battle effect models on seven of eight sites. |
| The morph block is a fixed-pitch slab of records | falsified (retail walks it with three pitches) | Plausible: a record array is normally walked with one stride. The spawner's size sum steps `0xC`, its rest-pose copy `n_vert * 8`, and the apply pass `n_vert * 0x60`; they agree only when a block holds one record, which every shipped block does. |
| `FUN_8001FA00` seeds a cutscene sprite list | falsified (its one caller seeds the fog-particle pool's free stack) | Plausible: an identity index list under a top index is a generic free-list seed, and the name was picked from a neighbour. Its only `jal` is MAIN INIT's at `0x801D7384` (PROT 0897), passing `(pool, pool + 4, 0x50)`. |
| `0x801F21B4` is one twelve-row probe table whose later rows have no caller | falsified (three tables, each with a consumer) | Plausible: the 192 bytes share one row shape and the locomotion reads only the first four rows. Three consumers form three bases - the actor probes at `0x801F21B4`, the wall probes at `0x801F2214` and the facing compass at `0x801F2254` - and the "uncalled" rows were the second and third tables. |
| A talk acquire's face-at bind is always the conversation's own actor | falsified (it names any actor) | Plausible: in the captured inn talk the bind `0x33` is the innkeeper's own, so turning the player toward the speaker looks right. The kernel resolves the bind like any cross-context id (`0x80037E00..0x80037EA8`), and a disc census finds 40 of the 146 placement-record acquires naming another actor, and 885 more in object and cutscene records with no own actor at all ([settled](re-settled-threads/field.md)). |
| Op `0x42` mode 1 tests a screen mode | falsified (it tests the held pad) | Plausible: `_DAT_8007B850` sits among the display globals and the op's other mode tests a flag. It is the packed held pad, compared `& 0xF000` against a compass table at `0x801F28D0` and against the face buttons for `op1` `8..11` ([settled](re-settled-threads/field.md)). |
| `FUN_801DB510` returns at once while `0x8007B606` is clear | falsified (it pins the focus and runs the shake tail) | Plausible: the byte is the follow camera's enable, and a disabled camera doing nothing is the natural reading. `beq` at `0x801DB558` goes to the pin leg `0x801DB820`, which sets the focus to the negated player position and falls into the shake tail; only composing and easing are skipped ([settled](re-settled-threads/field.md)). |
| `FUN_80046870` is a brightness / fade ramp, and `gp+0x2E8` a frame cooldown | falsified (it is the Incense window, counted in walk ticks) | Plausible: the routine adds `0x40` to a word and caps it at `0x100`, a fade's shape. It is item class `0x82`'s arm (`0x800421A0`), the walk tick drains the word, and the region encounter roll skips entirely while it is non-zero (`0x801DA174`) - Incense suppresses encounters outright rather than lowering a rate ([settled](re-settled-threads/field.md)). |
| Menu window 8 is a generic notify window whose template nothing stages | falsified (the art-learned notice) | Plausible: no `jal` reaches its operands' writer from the menu. The template `0x801E4700` is resident menu-overlay data, and `FUN_80035C00` - two stores into `gp+0x858` / `gp+0x860` - is called by the Hyper-Art book arm at `0x8004208C` with the roster slot and the art id ([settled](re-settled-threads/field.md)). |
| `FUN_801DD330`'s `0x30` is an init word and its `1` a slot selector | falsified (a window id and an exit sub-screen) | Plausible: small constants passed to a picker read as flags. `0x30` is patched into bytes `+5` / `+9` of the script at `0x801E4E08` as the settings window's id, and `1` is stored as the sub-screen on exit - the root command picker ([settled](re-settled-threads/field.md)). |
| `*(u16*)(_DAT_801C6EA4 + 8)` means "no modal window / dialog open" | falsified (a re-entrancy bracket around one call) | Plausible: the motion gate reads it and dialogue is the obvious modal state. Every writer is a set-1 / clear-0 pair around a single call - `FUN_8003AEB0` (`0x8003B73C` / `0x8003B928`) and the field VM in PROT 0897 (`0x801E2820` / `0x801E282C` around `jal 0x8003CF7C`, `0x801E2BBC` / `0x801E2BD8` around `jal 0x8003A1E4`) - and nothing holds it across a frame. |
| Move-VM op `0x13` spawns a render-mode-6 keyframe-mesh child | falsified (it sets draw kind 4) | Plausible: the op stages a child-looking block. It writes `+0x5A = 2` and `+0x56 = 4`; `+0x56` is the draw kind `FUN_8001ADA4` switches on (`lhu v0,0x56(s0)` at `0x8001AE60`), and `+0x5A` the part-tick mode. The `+0x9E` bits then pick the emitter (`0x4000` -> `0x8002A5A4`, `0x2000` -> `0x801CFA48`). |
| The move-VM's anim-block stores below `+0xAC` land where their offsets say | falsified (six arms wrapped their offsets to 8 bits) | Plausible: each store's displacement reads as an actor offset. Six arms of `FUN_80023070` (`0x13`, `0x1F`, `0x23`, `0x24`, `0x26`, `0x42`) address the block through `s1 = actor + 0x80` (`0x80023088`); an offset wrapped to 8 bits (`0xFC` for `+0xA8`) sends every store below `+0xAC` to a slot nothing reads. Take the offsets from the `sh` / `sw` stores. |
| PROT 0923's ribbon node packs `+0x9C = 0x040C` (cap 12, total 4) | falsified (the disc stores a plain cap; the engine's glide wrote the rest) | Plausible: the value is what the ported decoder reads back after a tick. A summon translation glide applied to every node treats `+0x9C` / `+0x9E` as its clock, adding `0x400` to the cap per tick and clearing the `0x2000` ribbon flag at its latch; retail reads those words as a glide only inside the mode-3 arm (`0x800226E8..0x800228A0`). |
| The effect ribbon's trig tables are `1 << 13` fixed point | falsified (`1 << 12`, and `_DAT_8007B7F8` is the cosine view) | Plausible: the lateral offset shifts by 13. The tables are the standard `1 << 12` pair and `lw v1,-0x4808(t8)` at `0x801CFD0C` walks X on the cosine, so the `>> 13` halves the offset rather than normalising it. |

### Op-0x43 sub-3..6 as a timed sound-register ramp

**Tempting reading.** `FUN_8003C6A4` scales four byte operands `* 0x80 + 0x40` and stores two
trailing halfwords, which reads as "four targets, a `ticks` duration and a `curve`". The destinations
are unnamed `0x8007Bxxx` globals and the op's sub-dispatcher neighbours do sound work.

**Why it is wrong.** The tick decides it. The descriptor at `&DAT_80074304` carries handler
`0x80037018` in its `+8` word, which `FUN_80020DE0` copies to the new actor's `+0x0C`. That routine
reads `+0x88`/`+0xC8` and `+0x8A`/`+0xCA` as an AABB over the player's position, `+0x80`/`+0x84` as
two endpoints, and `+0x8C` as the destination store width (`1` = `sb`, `2` = `sh`, `3`/`4` = `sw`).
Nothing counts down. The spawn and the tick are two halves of one actor, not two separate gaps.

**What is true.** It is a camera-register zone ramp: the byte operands are a tile rectangle, the two
halfwords are the ramp's endpoints, and the register lerps on the player's Z as he crosses the zone
(and back when he returns). The four destinations are field camera-configuration registers the
field-overlay camera composer reads into the camera descriptor: `B60C` pitch, `B610` yaw, `B614`
eye-space Z, `B618` GTE `H`. `_DAT_8007B60C`'s `0x1B8` default is the pitch `FUN_80025C24` seeds at
field entry, which identifies the register. Details:
[script-vm.md](../subsystems/script-vm.md#0x43-sub-36---the-camera-register-zone-ramp),
[motion-vm.md](../subsystems/motion-vm.md#fun_80037018-is-not-a-slot-of-this-pool).

### The reachable band's record force-walks the player through the wall

**Tempting reading.** Record 10 covers the only band a player can stand in; its timeline runs for
two frames and the player moves `+8` in `z` (one locomotion step) before it ends. So the record
force-walks the player through the sealed band and then runs the `0x3F`, which would also explain
how retail crosses a wall that blocks ordinary locomotion.

**Why it is wrong.** Record 10's entire body is `21 21 26 FE FF`: no walk op and no `0x3F`. The
two-frame end is the choreography-wrap rule acting on a `Nop` + `JmpRel`-to-self park, and the `+8`
is one frame of the player's own pad locomotion, visible only because the modal-timeline install
stops it.

**What is true.** The `0x3F` lives in record 0, on the other band, and that band is opened by a
collision paint rather than crossed by a scripted walk - see
[Rim Elm's south gate](re-settled-threads/field.md#rim-elms-south-gate). A timeline that ends
without doing the thing is evidence about the runner only if the record contains the thing:
disassemble the record first.

### 270 undumped field-overlay functions (recomp dispatch-entry seeds)

**Tempting reading.** A PSXRecomp runtime capture of the slot-A overlay window over a boot-to-town
session yields ~312 "call targets" in the `0x801CC000+0x29000` band, ~270 of them absent from
`ghidra/scripts/funcs/` and [`functions.md`](functions.md) - a large undumped-function backlog for
PROT 0897.

**Why it is wrong.** Triage of every address against the disc overlay images and the captures' own
resident bytes fails the premise on three independent axes:

- **They are dispatch entries, not call targets.** The capture records every PC where the recomp's
  dispatcher entered interpretation: indirect-call targets, but also return sites (the instruction
  after a `jal` + delay slot), interrupt-resume PCs (mid-loop, weighted by hot loops) and `jr`-table
  case labels. Against the resident image only ~1/4 of the entries are call-shaped; the rest sit
  mid-function or mid-loop.
- **The PC tables span overlay generations; only the byte snapshot is coherent.** PCs accumulate
  across the whole session (title, FMV, menus, field), so the list mixes title-overlay,
  cutscene-overlay (0970) and menu PCs with field ones. One source capture's resident bytes match
  the disc 0897 image at only ~16% (a title-era occupant); dozens of listed PCs land inside 0897's
  data head (debug strings + pointer tables); and two entries marked as already known resolve to the
  cutscene overlay's STR dispatch `FUN_801CEA3C` and the actor-VM jump table `0x801CED70` - another
  overlay's function and a data address.
- **No image claims them as functions.** A sweep of all mapped slot-A overlay images plus the slot-B
  field library for prologues / static `jal` targets at the listed addresses yields two coincidental
  hits (both in the never-resident slot-machine image) and a handful of `j`-target labels.

**What is true.** The list is not a function inventory and the gap it implies does not exist. A seed
list from a recomp's interpreter dispatcher needs per-hit resident-image resolution (for example a
mode-gated `dirty_exec_hot` window) before any identity claim, and a "new function" claim needs a
prologue or a static-call witness in the image that was resident. The undumped-code question for
0897 is answered by the [port-catalog dashboard](../tooling/port-catalog.md).

### `FUN_801F12D0` read from the `overlay_0897` dump

**Tempting reading.** `0x801F12D0` falls inside more than one overlay's load window, so the routine
has dumps under several labels, and the readef / summon applier's slot sequencing can be read from
`ghidra/scripts/funcs/overlay_0897_801f12d0.txt`. Its tail `jal 0x801daba4` is close enough to the
real control flow to look right.

**Why it is wrong.** That dump is a mid-function fragment: it opens at
`801f12d0 lw v1,-0x6c84(v0)` with no `addiu sp,sp,-N` in the window, yet closes restoring `s0`-`s3`
and `ra` from a frame it never established. Its 47 instructions hold none of the slot-streaming
logic - no `+0x277` base-slot read, no bit-7 file test, no `base+2` / `base+3` staging arms.

**What is true.** Read `overlay_muscle_dome_801f12d0.txt`: 330 instructions, a proper prologue, the
bit-7 test at `801f1644` and both staging arms. For any VA several overlays map, the instruction
count in the dump header is the first filter, and callee-saved restores with no matching save plus
a missing prologue is the fragment test. Corpus-wide picture:
[`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md).

### A second stage-id writer "at `0x801FD514` in the 0897 band"

**Tempting reading.** The base-tag-less dump `overlay_0897_xxx_dat_801fd150.txt` shows a routine at
`0x801FD150` that writes `_DAT_8007B64A` mid-battle and scans records for `0xE7`. It is internally
coherent - clean prologue, sane guard logic, real (base-independent) SCUS `jal` targets - so it
reads as a new function with an unknown "`0xE7` record scan".

**Why it is wrong.** The overlay-local jumps give it away: `j 0x801E6A7C` / `j 0x801E6CE8` point
`0x16000` below the "function", and are intra-function only at the real base. The store's word pair
`24020003 a082b64a` occurs in no PROT entry but 0898, at file `0x18510` = `0x801E6D28` under the
tagged base.

**What is true.** The writer is real but neither new nor at that coordinate. The printing re-keys to
`FUN_801E6968`, the Lost Grail Final Heal sweep (`0xE7` is the Lost Grail item id, consumed from the
accessory slot); the writer is its tail arm, store at `0x801E6D2C`. It is listed in
[`overlay-va-aliases.md`](overlay-va-aliases.md#0x801fd4c0) and the shifted-alias table of
[`battle-action.md`](../subsystems/battle-action.md). A dump without a base tag proves content,
never coordinates ([`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md)); before
naming a function in an aliased band, grep the reference indexes for the re-keyed address as well
as the printed one.

### `0x801D84B4` is inter-function padding

**Tempting reading.** The VA is alignment `nop` in every overlay that maps it, so no routine lives
there. It is padding in the fishing, dance, debug-menu and slot-machine extractions (17 consecutive
`nop`) and in the baka-fighter image (32, with one stray `sllv zero,zero,zero`), and the one dump
that resolves an entry here is the field overlay's, whose header reads `entry=801d8308` - interior.

**Why it is wrong.** None of that is about the field overlay's bytes. In `overlay_field_0897.bin`
at base `0x801CE818`, `jr ra` at `0x801D84AC` with `addiu sp,sp,0x20` in its delay slot closes the
predecessor, and a six-instruction leaf follows. The field image carries exactly one
`jal 0x801D84B4`, and two base-tagged field-overlay dumps hold the same seven-word body.

**What is true.** The leaf stores master game mode `_DAT_8007B83C = 0x16` (22, CARD INIT), raises
the entry-context word `_DAT_8007BB00 = 1` and returns - the overlay-local twin of the SCUS scripted
game-over trigger `FUN_8003C7EC`. A padding verdict is per image: slot A holds a different overlay
per game mode, so `nop` in four of them says nothing about the fifth.

### `FUN_801dfb10` is a scripted player-turn state machine

**Tempting reading.** A dump at `0x801DFB10` decodes to a coherent routine - a player-input lock, a
per-frame `+0x16` angle rotation, a story-flag `0xb` gate - and the described behaviour is accurate,
so nothing in the write-up looks wrong.

**Why it is wrong.** No routine exists at `0x801DFB10`. The address is a phantom of the
`overlay_0897_xxx_dat` import's `+0xE818` base error. The printed VA is interior in every image
that covers it: the fall-through of `bnez v0,0x801dfb28` in the battle overlay, a branch label in
the field overlay, and the delay slot of `jal 0x8003ce64` in the menu overlay.

**What is true.** The bytes are field (0897) `0x801EE328`, the world-map `ON RULA` travel-art
actor, documented and ported under that VA. The `overlay_0896` batch prints the same routine at
`0x801E8B10` (its `+0x5818` delta), and the two phantoms resolving to one VA is the check that pins
it - see [`phantom-print-index.md`](../tooling/phantom-print-index.md).

### A scene with no `0x3F` in its MAN has no door

**Tempting reading.** The chapter-1 frontier ladder's per-partition walk finds no `0x3F` in `uru` /
`urudre1..3` and nothing in `jouine`, its tile sweep fires no transition, and 160 executed record
bodies reach no scene change. Three instruments agree, so those five scenes are sealed and may be
treated as one-way.

**Why it is wrong.** Each instrument is right only about what it measures:

- `uru`'s `0x3F` is `(2, "MAP03")`, upper-case, which a lower-case-only label gate rejects.
- The others sit `0x2DC` (`urudre1`) / `0x124C` (`urudre2`) / `0x2034` (`urudre3`) bytes into record
  bodies, past inline `0x1F` text the fall-through walk desyncs on; `jouine`'s FMV op is at `0x1A8F`.
- The tile sweep stops at 48 deduped gate-1 tiles, while `uru` carries 118 with its exit band at
  positions 63..66.
- A 24-tick post-step budget cannot reach a tail behind 300+ frames of explicit waits.

**What is true.** All five scenes have exits, carried by the scene's `.PCH` sidecar; `uru`'s fires
live. `jouine` has no `0x3F` because its exit is the FMV hand-off `4C E2 08`, on the FMV dispatch
table. Ask the `.PCH` before calling a scene sealed. See
[the uru / mais chain and jouine exits](re-settled-threads/field.md#the-uru-mais-chain-and-jouine-exits).

### A frontier-ladder scene that will not walk is a scene or a seat problem

**Tempting reading.** Twenty-eight consecutive scenes of the chapter-1 closure report zero driven
tiles, so either those scenes or the engine's scene-entry path (the destination case fold, the
wait-discounting timeline cap) is broken.

**Why it is wrong.** Three isolation builds give byte-identical results with either change reverted,
and each "broken" scene walks on a fresh host.

**What is true.** One player-actor bit is latched: `tower`'s ledge-hop steering lock, leaked across
a scene change. The failure is ordered by closure position, not by scene. Before blaming a scene,
walk it first in the sweep order and alone. See
[`re-settled-threads/field.md`](re-settled-threads/field.md).

### Field-VM op `0x45` sub `0xC0` returns the operand `s16` as the next PC

**Tempting reading.** The decompiled C renders the arm as "APPLY ... then absolute jump": the `s16`
operand looks like the next PC.

**Why it is wrong.** Retail's arm at `overlay_0897` `0x801DF210` exits `j 0x801E3624` with
`addiu s8, s8, 4` in the delay slot, and the `s16` at `operand+1` goes to `FUN_801DE084` as the
apply trigger - the same call the CONFIGURE arm makes with its `s16` at `operand+2`. The sibling
arms advance the same way (`0x40` by `+0x14`, `0x80` by `+2`).

**What is true.** Sub `0xC0` is a four-byte instruction that falls through. The field VM stores no
absolute PC anywhere, so every control-flow field shifts with its record and `man_edit` may resize
a record containing this op. Under the jump reading every record whose trigger is `0` restarts from
byte 0 - which makes `urudre2` read as one-way and four `keikoku` records do nothing - and the op
looks like an unrelocatable `AbsoluteRef`.

### Op `0x43` subs 0/1/A/B resume at an operand `s16`

**Tempting reading.** The arm carries three `s16` operands, and one reads as a resume PC, giving
instruction widths of 5 / 9 bytes.

**Why it is wrong.** The sub table at `0x801CEDA8` vectors subs 0/1/A/B to `0x801DF384`, whose
shared exit `0x801DF5B4` is `j 0x801E3624` / `addiu s8, s8, 8` (plus `+2` at `0x801DF534` for
`sub >= 0xA`).

**What is true.** The three `s16`s are `FUN_801D25EC` arguments; the sub-A/B `+7` is negated into
the target's Y (`0x801DF524..0x801DF530`). Instruction widths are 8 / 10 bytes (9 / 11 extended).

## No overlay function lives below `0x801CE818`

**Tempting reading.** An undocumented address in the `0x801C0164`..`0x801CE000` band is an
overlay-resident function awaiting a doc entry. Overlay code is usually described as living "at
`0x801C0000+`", and dumps in that band disassemble cleanly, carry function-shaped prologues and
epilogues, and are filed under `overlay_<label>_801c....txt` names.

**Why it is wrong.** Every occupant of the slot-A overlay window bases at `0x801CE818` and every
slot-B occupant at `0x801F69D8` (see
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml)), so no extracted overlay
image contains any VA below `0x801CE818`. The measurement agrees: disassembling every extracted
image at its mapped base and asking which of those addresses is a `jal` target, a `j` target or an
instruction boundary returns nothing for the whole band.

**What is true.** They are printed addresses from imports based at `0x801C0000`, and the true VA of
each is `printed + delta`. The deltas are constant per import, so the band resolves mechanically
([`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md) tabulates them):

- `+0xE818` into the field (0897) or menu (0899) overlay;
- `+0xD018` into fishing (0972), through the 0971 over-read tail;
- `+0x9818` into dance (0980);
- `+0x5818` for the `overlay_0896_*` family.

Worked examples, each documented under its real address: `0x801C6FEC` is the fishing reel
tug-of-war `FUN_801D4004`; `0x801C56B4` is the hooked-fish handler `FUN_801D26CC`; `0x801C2704` is
menu-overlay `FUN_801D0F1C`.

The same failure extends above `0x801CE818`, where the printed address lies inside a real overlay's
span and cannot be rejected on range alone. There, resolve the dump's bytes to an image and offset,
and separately ask whether the printed VA is a `jr ra`-preceded boundary in any image. A VA that is
only ever a `j` or branch target is an intra-function label, not a port site. A printed address is
evidence about the import; which image, which offset and whether it is a function at all are
answered from the extracted image at its mapped base.

## Menus / UI

| Thread | Verdict | Why |
|---|---|---|
| Op `4C E5` is an XP add clamped to [0, 9999999] | falsified (the casino coin delta) | `0x801E328C..0x801E32E4` adds a signed 24-bit value to the coin bank `0x800845A4`, caps it above at 9999999 with no lower clamp, sets system flag 8 and advances 5. A host that leaves the op a no-op never charges cabinet fees or the Earth egg's price. |
| Coin-counter record 10 is a plain frame with no painter | falsified (its painter is `FUN_801E6F70`) | The record's `+0x18` word is `0x801E6F70`, the entry panel; the prize exchange's windows draw through it, not through a host-invented layout. |
| `FUN_80035C00`'s callers are the battle action resolver staging a reaction byte | falsified (the item / spell effect applier) | Plausible: the pair it writes sits beside battle-reaction state. All three calls are in `FUN_800402F4` - two after a menu-cast spell-level bump (`+0x729`, threshold table `0x8007656C`), one after a Hyper-Art book's art insert, skipped in battle - and the pair feeds menu windows 7 and 8. The port carries it as values (`SpellLevelNotice`, `pending_art_notice`). |
| Dialog picker states `0x11` / `0x13` / `0x15` / `0x17` resize the box | falsified (they slide it in) | Plausible: the box appears at a different place and size from the reading box it follows, and the state sits between the press and a usable menu. Width and height are fixed from the first drawn frame; the state counts the origin in from off screen (`0x801D92F4`, `0x801D9350`, shared tail `0x801D93F4`), and the pager skips the box while the count holds the `0x309` sentinel. |
| `FUN_801E0418` draws memory-card message strips from an unidentified 8bpp page | falsified (the title TIM's strips, redrawn behind the Load window) | Plausible: it lives in the menu overlay beside the card code, draws a two-choice stack with the unselected row dimmed, and PROT 0899 carries no TIM for its page. The records address the title TIM's wordmark, NEW GAME, CONTINUE, TM and copyright strips; the page is PROT 0890's title TIM, byte-equal in VRAM, and the routine is gated on `_DAT_8007BB00` (the Load window opened from the title). |
| The shop tail probe `FUN_80042F4C(0xFF)` tests for an empty slot and is almost always satisfied | falsified (`0xFF` is the Platinum Card) | Plausible: `0xFF` reads like a sentinel. `FUN_80042F4C` counts a matching bag slot, an empty slot is id `0`, and item `0xFF` is the Platinum Card, which no mednafen library state carries in an equipment block; a capture on Retock's Items Shop emits 13 rows with it and 10 without. The "new in this town" reading of the band falls with it. |
| The shop buy list has no alt-ink, so the casino kernel can ink its rows | falsified (the list kernel inks class `0xA000` rows 5) | Plausible: the casino prize list has its own ink rule. `FUN_80032A44` inks class-`0xA000` rows 5 even over a dimmed row (`0x80033548..0x800335A0`). |
| `_DAT_8007B43C`'s seeds 4 / 5 come from a save-completion screen | falsified (the Door of Light / Door of Wind Use screens) | Plausible: the menu overlay writes them near the save code. `FUN_801D8A58` consumes item `0x88` and stores 4 (`0x801D8B6C`); `FUN_801D8B90` consumes `0x89` and stores 5 (`0x801D8D3C`). `FUN_801ED308` is the pause-menu session, not a brightness fade. |
| The USA dialog font is ASCII-only, so an NTSC font patch targets the menu atlas at `0x11218` with widths at `0x80074050` | falsified (the dialog font is the `0x7F40` TIM, widths at `0x80073F1C`) | Plausible: the menu atlas is the obvious glyph sheet, and `0x80074050` sits beside the width table. `0x80074050` is the `0xCE` escape table, and the dialog font already carries 32 inked high cells. |
| PAL accent capitals are a game-specific block at `0xD0..0xD6` | falsified (CP850 positions) | Plausible: FR / IT capitals sit near there. Lifted text writes `Â` `0xB6`, `È` `0xD4`, `Î` `0xD7`, `Ì` `0xDE`, `Ô` `0xE2`. |
| The battle command ring is drawn as UI-icon sprites, with no text to translate | falsified (the chips are placement-record payload strings) | Plausible: the chips sit on blue plate art beside a D-pad glyph. Each chip's word is the `+0x14` payload of its screen-element placement record: `Attack`, `Item`, `Run`, `Begin`, `Auto`, `Command` in the executable's small-data pool `0x8007B658..0x8007B690`, the Ra-Seru names in the battle overlay ([`battle.md`](../subsystems/battle.md#where-the-words-come-from)). |
| The play page draws the fishing point-exchange screen | falsified (the page opened, bought and closed it inside one call) | Plausible: the page composed `fishing_exchange_draws` every frame and a unit test drew it. Nothing held the exchange open across a frame, so the compose always returned empty. Both hosts drive the screen through `engine-core::fishing_exchange_input`. |
| Retail's shop quantity control is a nine-row list whose cursor **is** the quantity | falsified (it is a pair of pad steppers) | Plausible: the port's own shop screen was that list, and a screen read back off the port looks like a screen read off retail. `FUN_801DB7F4` and `FUN_801DBD94` step a single value at `DAT_801E46B4` by `+1` / `-1` / `+10` / `-10`, clamped, with the bound `min(gold / price, 99, 99 - held)`. The list reading caps every purchase at 9, the list's own row count. |
| The retail item bag holds 72 slots | falsified (256, behind an active window) | Plausible: 72 is what a cheat database's item page enumerates, and a UI page is easy to mistake for a capacity. The array at `0x80085958` is 256 slots and every accessor checks the window pair `gp[+0x2D2]` / `gp[+0x2D4]`, which `FUN_8004313C` alone writes; a lone character sees one 128-slot half. A real three-member memory-card block carries items up to index 159, so a 72-slot bound drops 88 of them on a lift ([`inventory.md`](../subsystems/inventory.md)). |
| A `-1` row of the op-`0x49` submode table leaves the driver actor untouched, so the park simply stands until the player presses Start | falsified (the enter half stores handler `7` first, and the menu opens by itself) | Plausible: the table read returns on `-1` (`0x801F1468..0x801F1470`) before it installs a handler, and a standing park explains the kind-`0x0D` screens. `FUN_801F1278` has already stored `+0x50 = 7` (`0x801F140C`) and `+0x54 = 0` (`0x801F141C`) by then, and handler `7` is the state pick that opens the pause-menu session. A PCSX-Redux capture at the `town01` save point shows the menu (game mode `23`) open with no Start press. A close-tick fallback for sub-op `1` built on the standing-park reading makes every save point save nothing. |
| The pause menu's close clears the op-`0x49` park through the leaf `FUN_8003540C` | falsified (the leaf has no reference of any form on the disc) | Plausible: it zeroes the park and the window-list head side by side, which is what a menu teardown would do. `find-address-word-refs.py 8003540c --prot` finds no word, `jal`, `j`, branch or `lui` pair for it; its twin `FUN_800353E0` runs only from the scene loaders. The park is resumed by the submode dispatcher's retire arm, which writes Done while it is live. |
| Actor VM = "the title screen's sprite-walk interpreter", with an ANM-trigger opcode | falsified (it is the menu overlay's window-widget script interpreter) | Plausible: the sprite-VM framing suggests a "trigger animation" op. `FUN_801D6628` is resident in PROT 0899 (the menu overlay), and its base materialisation `lui 0x801e / addiu 0x4738` indexes the **window descriptor table** - instruction byte 1 is a window id, not a sprite-actor slot. No arm of the 13-way dispatch hands off an ANM id (`see ghidra/scripts/funcs/overlay_menu_801d6628.txt`). Programs are overlay-resident data ([window-script.md](../formats/window-script.md)), so there is no per-scene carrier to find. |
| Op `0x36`'s request/acknowledge gate covers subs `0` / `2` / `3` | falsified (sub `3` is ungated, and sub `1` has a *different* gate) | Plausible: the three subs are one protocol, and the C renders the arms in a shape that supports one gate over all of them. Sub `0` and sub `2` halt at PC unless `_DAT_8007BABC == _DAT_8007BAA0` (`0x801E0340`, `0x801E03A8`); sub `1` stores only when the pair is equal **or** the acknowledge cell reads the idle sentinel `-1` (`0x801E0374..0x801E037C`); sub `3` - the teardown `FUN_801D8450` - is ungated and yields the frame rather than falling through. A script that waits on the wrong arm deadlocks. See [`script-vm.md`](../subsystems/script-vm.md#overlay-0897-command--submenu-support-functions). |
| `_DAT_8007B868` only *skips* the bit-15-set arm of op `0x36` | falsified (it points the two halves in opposite directions) | Plausible: a single "disable" flag matches the first arm met - non-zero skips the whole bit-15-set sub-switch and advances the op (`bnez v0,0x801DF898` at `0x801E031C`). On the bit-15-**clear** arm the same word *bypasses* the equality test instead of adding one (`0x801E03E8..0x801E0410`). Retail boots the word `0`, so the asymmetry never shows in a capture. |
| The shop buy-row layout is untraced, and `build_price_gated_rows` is the port of it | falsified (the layout is pinned, and that builder is a different routine) | Plausible: the builder dims unaffordable rows, which is a real retail rule. Retail's builder (case `0x0B`) also splits the walked rows at `record_count - 3`, stages the rows **below** the split into `0x801C6220` tagged `0x3000`, writes the last three straight out tagged `0xA000` (ink 5), and appends the staged group afterwards - so the on-screen order is not the record order and the top strip is highlighted. The dim rule is `purse < price` **or** `held >= 99`. See [`shop.md`](../subsystems/shop.md#the-last-rows-come-first). |
| Menu sub-screen `0x02` is the save entry | falsified (`0x02` is the dev character editor; save is `0x19`) | Plausible: the entry-context byte reads by position, and `0x02` is what the sentinel row produces. The byte is keyed on the **record kind**: `0x00` shop -> `0x1A`, `0x01` save -> `0x19`, `0x07` casino -> `0x20`, `0x0D` -> `0x04`, and the sentinel `1` -> `0x02`, the debug character-parameter editor. See [`save-screen.md`](../subsystems/save-screen.md#debug-character-parameter-editor-fun_801d6e18). |
| The inline `0x1F` dialogue segment carries a geometry header | falsified (the `0x1F` is a MES line-start marker and nothing follows it but glyphs) | Plausible: a port that renders only a segment's first line has unexplained box geometry, and an unparsed header would supply it. The box's rect, pens and advance hand belong to the pager (`FUN_801D84D0`, row capacity `_DAT_801F2740 = 3`), and consecutive `0x1F` lines pack into one window. See [`field-menu.md`](../subsystems/field-menu.md#dialog-reading-box-fun_801d84d0). |
| PROT 0898 never calls `FUN_8002C69C`, so the post-battle report windows are not the nine-slice | falsified (the caller is in SCUS, one hop away) | Plausible: a `jal` sweep of the battle overlay finds nothing, and the overlay really does not call it. `FUN_80031D00` (SCUS) drives the window emitter with `jal 0x800323E4` off the **retained widget list** every frame a battle is up, so the report chrome is the same nine-slice as every other window. A sweep scoped to one image cannot answer a question about a shared driver. See [`level-up.md`](../subsystems/level-up.md#fun_8002c69c-does-run-in-battle---the-jal-sweep-was-blind-to-its-caller). |
| The sparring prompt is an undecoded Yes/No box | falsified (it is the ordinary 4-option picker) | Plausible: the prompt reads as binary on screen, so a dedicated two-way confirm widget is the natural guess. The script emits `3E FF <row>`, the standard option-picker sequence of the field VM; only the row indices are specific to it. Its install coordinate is on [`encounter.md`](../formats/encounter.md). |
| The item bag has no writer outside the five SCUS helpers | falsified (the pause menu zeroes slots directly) | Plausible: the five helpers are the whole add / remove / query surface and all respect the active window. The pause menu's Throw Out confirm `FUN_801D8734` stores zero straight into the bag at `0x801D88FC` and `0x801D8910`. A port that models the bag as "whatever the five helpers did" keeps a thrown-away item. |
| The item menu's Throw Out cursor is a display row | falsified (it is a bag slot) | Plausible: the cursor drives a list, and on an unholed bag row and slot are the same number - which is every bag a fixture builds. The list hides empty slots while the payload does not: on a bag holed at slots 1/3/6 the cursor takes 0, 2, 4, 5, 7, 8, and removing by row throws away the wrong stack. |
| The Point Card applier is blocked on an unported counter | falsified (the counter exists) | Plausible: a port tag named the counter as missing. `World::minigames.point_card` is the `0x800845B4` bank. The arm stays unentered for a different reason: no item on the disc carries the class that reaches it. |
| The bag-row builders are missing their gate tables | falsified (both tables are parsed) | Plausible: a builder that cannot answer "is this sellable" looks table-shaped. Sell price is the item record's `+2` through `shop_catalog::ShopItemData`, and the discard gate is the equipment record's `+7` bit 0 plus the item-effect not-discardable kind. The defect under that reading was a sell list drawn id-sorted while the commit walked slots. |
| The screen that opens windows 25 and 41 is the shop's equipment-buy recipient flow `FUN_801DB380` | falsified (two screens, and that one opens neither) | Plausible: the two windows draw the same shape of stat comparison, and the recipient flow does draw a compare panel. Window 25 is named by one open command only - the Equip screen's candidate step, sub-screen `0x14`, script `0x801E4DC8` - and window 41 by the shop-entry script `0x801E4E64`; the recipient sub-screen adds only window 36 over the set already up. Under the merged reading window 25 has no place on the Equip screen at all. |
| A host may pass the `0x40` no-passive sentinel and get the identical screen | falsified (true of the class-`1` arm only) | Plausible: every equipment bonus row on the disc carries `0x40` at `+5`, so the sentinel reproduces the equip screen on every equipment id. The category byte has two sources, and only one is that table: 151 of the 255 non-zero ids take the item-effect arm, 80 of them carrying a real passive index at `+3`. A host feeding the sentinel unconditionally loses the HP / MP and SPD / INT / AGL row sets. |
| The equip browse row is the equip-byte index, with row `0` a Best-Equipment row | falsified (a two-table slot map; row `0` is the **weapon** row) | Plausible: the port's slot list is the equip-byte array in order, and a leading row that is not one of the four gear slots reads as a convenience entry. Row `0` takes a per-character halfword from `0x8007B42C` (`2, 3, 2` - Vahn and Gala's weapon byte is `2`, Noa's `3`), and rows `1` and up index `0x801E43E8`, `00 01 00 04 05 06 07`. The order is weapon, helmet, body, footwear, three Goods, and the `slti v0, s0, 4` guard silences exactly the four gear rows ([settled](re-settled-threads/field.md)). |
| The equip candidate list is already category-gated per slot | falsified (true of the armament half only) | Plausible: the armament rows are masked - cases `0xE` / `0xF` / `0x10` read a per-character mask of their own. The three Goods rows go to different builder cases (`0x1C` / `0x1D` / `0x1E`, selected by the eight-byte content-id table the browse step writes), and their filter is item class `2` plus an item-effect byte, with no character term. Copying the armament rule onto all seven slots offers the wrong candidates for three of them. |
| Window 35 shows a quantity x price line | falsified (it is quantity / **bound**) | Plausible: a shop quantity prompt printing two numbers over a running total reads as unit price times count. The second number is `DAT_801E46B8`, the purchase bound the picker's phase 0 fills with `min(gold / price, 99, 99 - held)`, and the glyph between the pair is a separator, not a multiplication sign. The price appears once, in the total, beside the currency pictogram ([settled](re-settled-threads/field.md)). |
| Bytes `[7..10]` of the `0x801E43E8` run repeat the four gear-slot indices | falsified (a pad byte plus another table's first three entries) | Plausible: `00 01 02 04` looks like the gear indices again, as a longer array would. The run is seven bytes; `0x801E43EF` is alignment with no word, no `lui`/`addiu` pair and no branch anywhere in the corpus; `0x801E43F0` is a four-byte character equip mask and `0x801E43F4` eight halfwords of slot pictograms, each with three materialisation sites of its own ([settled](re-settled-threads/field.md)). |
| Window `0x15` is the list-reorder screen | falsified (two id spaces; the screen is sub-screen `0x15`) | Plausible: one number, one menu system, and a scanner that reports both. Window `0x15` is the **Equip** screen's party window, opened by the slot-browse's own script `0x801E4DA0` at `0x801D9ACC`. The reorder page is **sub-screen** `0x15`, written into `DAT_801E46A4` by exactly one site disc-wide, `0x801D6C4C` in the root picker's row-3 arm. State the id space when searching for a bare id. |
| `FUN_801F1E48` is a hub sub-menu state machine | falsified (it is the Incense wear-off notice) | Plausible: it sits in the submode driver's slot table beside real menus and runs three states. Entry kind `0x0B` reaches it from the walk tick's Incense zero edge (`0x801D0CEC..0x801D0D24`); it shows one window naming Incense and hands back ([`script-vm.md`](../subsystems/script-vm.md)). |
| The overworld runs no walk-regen tick, so Incense neither drains nor gates there | falsified | Plausible: a port's world map can run with no walk tick at all. Retail's kingdom maps run PROT 0897's `FUN_801D1344`, which calls the walk tick at `0x801D16EC`; the `sebucus_overworld_resident` state holds the walk tick, the Incense encounter test and the notice byte-identical to the 0897 image ([`world-map.md`](../subsystems/world-map.md)). |
| The developer menu's CAMERA row shows `_DAT_80089120` / `_DAT_80089118` | falsified (it shows the follow switch and the region-box centre) | Plausible: a camera row would print camera coordinates. Neither address is formed in the row's draw (`0x801EB3D8..0x801EB560`); it prints the switch `0x8007B606` as `OFF` / `ON` (strings at `0x801F318C`) and, while on, the centre of the region box `0x1F800384` (`000 000` for the `0x7F7F0000` sentinel). The port is `DevMenuRow::Camera`. |
| `FUN_801E6984` is the developer menu's MAP_CHANGE list | falsified (it paints panel record 14) | Plausible: it draws a numbered list. Its only descriptor is `0x801F3304`, installed at `0x801EF144` by submode slot `0x23` (`FUN_801EF014`, op-`0x49` sub-op 4) - a flag-backed numbered picker that only `kor`, `kor3` and `kor4` reach. |
| The op-`0x49` submode screens read the raw pad word | falsified (they test packed masks) | Plausible: a host can write the raw word into `World::input` and tests that feed the packed constant directly still pass. The handlers test Legaia's packed mask (Cross `0x40`, the d-pad in the high byte); on the raw word Cross does nothing, Down accepts and Right backs out. |
| A field dialog box ends after at most three rows, and `ImplicitNextPage` is a pager pause | falsified (the window scrolls; only a control byte ends a page) | Plausible: the box shows three rows and `_DAT_801F2740 = 3`. The pager tests for another line after each row (`0x801D8AB4`) and scrolls a full window in state `0x0C` without a press; a capture sees page 2's fourth line scroll in with the press column at zero. |
| Pager state `0x0D` advances to a page-full wait `0x0E` | falsified (`0x0D` completes a confirmed page, `0x0E` scrolls overflow, `0x19` waits) | Plausible: the states are consecutive. `0x0D` (`0x801D8C64`) is entered on a confirm, `0x0E` (`0x801D8D28`) scrolls rows through at speed `0x25`, and the wait is `0x19`. |
| The reading box's frame moves with the row scroll | falsified (only the rows scroll) | Plausible: the draw adds a y term. In `FUN_8002C69C(x, y + d, 0xF4, h - 8)` the `d` / `h` terms come from the collapse ramp `0x274C` (`0x801D9970..0x801D99A0`); the rows alone add `scroll >> 4`, clipped by two draw-area packets on the same ordering-table entry. |
| The Baka hub's panel-window table starts at `0x801F2C0C` | falsified (it starts at `0x801F2B98`, four records earlier) | Plausible: the records read from there parse cleanly, and two cross-check against known painters - inside the same shifted frame, so the check cannot see the shift. `FUN_801E9B3C` forms the base at `0x801E9B70` with stride `0x1C`; name entry is records 13 / 14, not 9 / 10. |

### The save screen's block grid has a sixteenth Return cell

*Falsified by disassembly.*

- **Tempting reading.** The screen carries a mode-`4` arm, which reads as a
  Return cell past the fifteen blocks.
- **Why it is wrong.** Both cursor words are clamped (`col <= 4`, `row <= 2`)
  and the linear seed's single writer stores `col + row*5`, so no cursor value
  selects the arm.
- **What is true.** The arm is real but unreachable. See
  [settled](re-settled-threads/title-boot-overlays.md#the-dead-return-view-mode).

### `0x801E5AE8` is a shared armament placer that `FUN_801D71F0` calls

*Falsified by bytes.*

- **Tempting reading.** The dump of `FUN_801D71F0` shows a jump out to
  `0x801E5AE8`, which reads as a call into a shared helper.
- **Why it is wrong.** The dump is mis-based by `0xE818`: the body prints low
  while its `j` targets print true, so a self-jump reads as an outbound call.
- **What is true.** `FUN_801D71F0` is the phantom print of `FUN_801E5A08`, and
  `0x801E5AE8` is that routine's own inline placer. The sibling reading
  "`FUN_801D71F0` is a dead add-item copy" has the right verdict for the wrong
  reason: the routine is dead, but it is the equip applier and its
  `FUN_800421D4` call is a refund.

### The disc's item population is far below 128, so a half-window bag cannot fill

*Falsified by disc bytes.*

- **Tempting reading.** A lone character sees one 128-slot half of the bag, and
  a small item population would make filling it impossible.
- **Why it is wrong.** The static item-name table carries 250 non-empty names
  over its 256 ids.
- **What is true.** The half-window OOB is unreached in normal play because of
  progress during the solo phase, not the size of the id space. See
  [`re-settled-threads.md`](re-settled-threads/title-boot-overlays.md#full-window-item-add-oob-reachability).

### Item-effect flag `0x40` is consumed by the item-info panel `FUN_801D0F1C`

*Falsified by disassembly.*

- **Tempting reading.** The panel branches on a `0x40` right where the
  accessory-passive block is chosen, and the five `0x40` subtypes are the
  battle specials.
- **Why it is wrong.** `FUN_801D0F1C` contains no `andi 0x40`. The instruction
  is `slti a0, 0x40` at `0x801D107C` / `0x801D1110`, a magnitude test on the
  record's `+3` passive index against the no-passive sentinel - a different
  field, and not a mask.
- **What is true.** The bit's only readers are the target-side forks at
  `0x801D18E0` (items) and `0x801D1C50` (spells) in PROT 0898. See
  [`re-settled-threads.md`](re-settled-threads/battle.md).

## Rendering / camera

| Thread | Verdict | Why |
|---|---|---|
| `FUN_8001CF50` builds the field / cutscene camera rotation | falsified (a per-node camera-relative rotation) | Plausible: it rebuilds a rotation from the camera globals `0x8007B790/92/94`. Its SCUS callers `FUN_8001ADA4` and `FUN_8001B964` enter it only when `node+0x52 & 0x780` (`0x8001B374`, `0x8001BA0C`); bits `0x80` / `0x100` / `0x200` drop an axis and `0x400` locks the node to the camera. Every other node uses the camera matrix `FUN_800172C0` builds through `FUN_80026988`. The roll-factor finding stands; only its routine changes. |
| `FUN_8001D088`'s counter `_DAT_8007BD28` and journal `0x800891A8` are bookkeeping that does not affect the pose | falsified (they drive a re-blend) | `FUN_8001BE80` zeroes the counter, tests it against `0xC00` at `0x8001C164` and re-blends from the journal (`0x8001C170..0x8001C1CC`); 5098 of 337597 gated half-frame samples on the disc take the retry. |
| Every field prop clip takes the plain, unscaled step | falsified (119 of 549 posed props carry the gate) | Clip byte `+1` bit 0 is set per clip: 2243 of 3634 records across 78 PROT entries, and not by record index. |
| The port spawns two kind-4 nodes on map01 where retail holds seven | falsified (a probe that never ticked the ambient tree) | Plausible: the count was measured on the port. The probe saw only the nodes the scene-entry first run leaves; with the ambient tick running the port holds six to eight. What the count hid was the motion block, without which the drifting puffs stay at their spawn point. |
| Nearer terrain hides about a fifth of the overworld fog's light | falsified (about 1%) | Plausible: an opaque-coverage count over the ordering table finds that share in front of the fog. Accounted per primitive, 18.6 of those points are two screen-space sprite families at the top of the frame (CLUTs `0x7F8D` / `0x7FC1`); the continent covers 1.0% on `keikoku_chest_preload`. Draw order is not the main cause of a denser haze. |
| The overworld continent's ordering-table key is the mesh leaves' `max(SZ) >> shift` / `AVSZ` choice at `gp-0x2D1` | falsified (the ground is `FUN_801F89B8`, keyed `(max SZ >> 5) + 14`) | Plausible: the mesh leaves do choose their key that way, and the landmarks sit on the continent. The ground cells are drawn by the bulk emitter `FUN_801F89B8`, called by `jal` from `0x801F733C`, which keys with no shift (`0x801F8DC8..0x801F8E20`). |
| Hub variant-2 packets subtract and variant-1 packets add | falsified (the GPU blends only STP texels) | Plausible: the emitters mark variant 2 ABR 2 (`B - F`) and variant 1 ABR 1. Every variant-2 palette and CLUT row 503 are STP-free, so those packets draw opaque, and only row-502 palettes `0`/`2`/`6`/`8` blend, additively. Pin `hub_palette_stp_real`. |
| A field attached light's extents are world units | falsified (view space, six times world scale) | Plausible: they sit beside a world-space offset in the spawn record. `FUN_800195A8` adds them after the parent has gone through the view matrix carrying `_DAT_8007BF10 = 24576 * I`; the retail `dolk` and `cave01` rims match `H * ext / vz`, and the world-unit reading puts them six times too wide ([`script-vm.md`](../subsystems/script-vm.md#the-extents-are-view-space-units-retail-capture)). |
| The field fog sheets sample VRAM `(448, 256)` | falsified (`(448, 0)`) | Plausible: page `0x27`'s x field is 7 and the effect pages near it live in the lower half. The page-y bit (bit 4) is clear, so the sheets read rows `0x40..0x6F` of the `(448, 0)` page of the PROT 0874 effect pool. A census that reads the wrong rect passes trivially. |
| The fog cap `0x48` is a debug value | falsified (it is every overworld's cap, from the MAN header) | Plausible: retail field scenes sit at `0x18`. `FUN_8003AEB0` raises it when `MAN[1] & 4` or `& 1`; `map01` / `map02` / `map03` and `opurud` do. |
| The port's fog runs dense because the spawner compares a stale live count | falsified (the stale count is retail's; the cause was the random numbers) | Plausible: the render walk zeroes and recounts the live word, so the spawner does compare last frame's count. The excess comes from handing the spawner a raw LCG state whose low nibble cycles with period 16, where retail's BIOS `rand` returns the high half. |
| `FUN_801D31B0` is shared by the dialog, cutscene and world-map overlays and reads its instruction operands | falsified (one field-overlay caller; the operands are padding) | Plausible: capture dumps labelled with those modes carry it. They are 0897-hosted modes; the one reference on the disc is the `jal` at `0x801D44C8`, and the routine overwrites `a1` at `0x801D31C4` before reading it. |
| The strip slab's `+0x18..+0x1E` are UV bounds | falsified (box half-extent and wobble) | Plausible: sub-ops `0x2B` / `0x2D` write them next to texture state. The texture rect is `+0x0C..+0x12`; `+0x18` / `+0x1A` size the billboard box and `+0x1C` / `+0x1E` are the wobble amplitude and frequency. |
| `0x801D32F8` / `0x801D3444` / `0x801D3748` are move-VM extension sub-handlers | falsified (interior addresses) | Plausible: they are listed beside `FUN_801D31B0`. Every word of the jump table at `0x801CE868` lands in `0x801D3680..0x801D48FC`; none of these is one. |
| The page may skip "sky" meshes because they read as a wall from the follow camera | falsified (neither the native window nor retail filters them) | Plausible: the full-map viewer needs the filter. On the play page it removes 45 scenes' matching draws, among them the opening crater shell, leaving the Seru tableau in a navy void. |
| The page's yellow strips in the battle command phase are the billboard outline pass | falsified (the retained field ground heightfield) | Plausible: the outline is flat and bright. `FX_OUTLINE` is off with zero vertices in the frame; the strips are town01's walk ground drawn outside the draw list through the battle camera, which the native window never does. |
| `ScreenTintPush::kind` selects which of retail's screen-effect quads is pushed | falsified (`FUN_80024EE4` emits exactly one full-display quad; `kind` is the ordering-table bucket) | Plausible: the spawner passes three words and names the first `kind`, and a screen-effect layer is usually a family of quads. The routine builds one `POLY_F4` from the display rect and adds it at `OT + a0*4`; the second word is the ABR equation and the third a colour with red in the low byte. |
| `FUN_80029888` zeroes the GTE light block | falsified (it writes the far-colour trio from registers) | Plausible: the routine is three back-to-back `ctc2` in a GTE setup path, and Ghidra renders each as a helper call with the control-register number buried in an argument. The three are cr21 / cr22 / cr23 - `RFC` / `GFC` / `BFC` - and the sources are `t4` / `t5` / `t6`, not zero. The routine that zeroes anything is `FUN_8003D190`, whose three `ctc2 zero` target cr5 / cr6 / cr7, the **translation** vector; the battle-intro swirl rolls about X and Z with it. |
| The field follow camera's pitch, yaw and height are scene-invariant constants | falsified (each is a per-scene, per-tile output) | Plausible: one save state pins all three, and a second scene often agrees because neighbouring regions share a camera record. Over the walkable state population the pinned height matches 12 of 19 states, the pitch 8 and the yaw 1. Retail's arrival handler queries the MAN section-3 zone table and hands the hit's camera-region record to the config loader. |
| The koin4 coplanar sliver is a residue below the detection floor | falsified (the port's own repair pass makes it) | Plausible: the scene's other coplanar families clear and the remaining strip is tiny. The strip is exactly one `DRAW_NUDGE` wide because the lift applied to that family, `[0, -0.75, -0.75]`, lies inside the second plane - zeroing the offset takes the measured overlap from 94.56 to 0. A repair pass is part of the instrument measuring the defect, so its artifacts read as findings. |
| `FUN_801D629C` is a per-fog-particle actor | falsified (it is the spawner) | Plausible: it runs per frame, reads the player's tile and touches particle state. It maps the tile to a MAN section-4 region record and pops one record from the pool at `_DAT_8007B7E0`; the per-particle work is SCUS's `FUN_8003F3FC`. The dump of it taken from the 0896 image is a fragment of a different routine, with no prologue. |
| The fog draw emits two GP0 **line** packets, command `0x9000000` | falsified (one textured quad; `0x09` is the packet's word count) | Plausible: `0x09000000` is written into the packet's first word, where a GP0 command would sit. That word is the ordering-table tag, whose top byte is the packet **length**: nine words, a `POLY_FT4` body. The command byte is `0x2E`, written by the caller at `0x8003F77C`, and the emitter stages four UV words into the packet's `+0x0C` / `+0x14` / `+0x1C` / `+0x24` slots - the corners of one textured, semi-transparent sheet. |
| `_DAT_80089118` is the camera focus Z and `_DAT_80089120` the X | falsified (the other way round) | Plausible: both are written the same way by the same routine and scroll by the same step, so the arithmetic fits either labelling. `0x80089118` takes the negated actor `+0x14` (X) and `0x80089120` the negated `+0x18` (Z), which makes the world-map top view scroll X with Left / Right and Z with Up / Down. |
| `FUN_80026F50` is the field view builder | falsified (it is another mode's) | Plausible: it is the same five-call view-build shape over the same globals. It folds the ROM-constant base matrix at `0x80010B84` (a 4x scale) rather than the live `0x8007BF10`, copies the eye trio as sign-extended **low halfwords**, and runs no focus `MVMVA`. It fires zero times across three field runs of 719 vsyncs each. |
| `FUN_80025C24` only zeroes the camera eye trio | falsified (it writes three different values) | Plausible: the first store is `sw zero` at `0x80025C28`. The `addiu v0,v0,0x40b8` after it re-bases the next two stores, so the trio lands as `(0, -0x100, 0x4024)` with an angle trio `(0x1B8, 0x64, 0)` - which is why the entry seeds differ between the two writers. |
| The field view matrix is built once a frame | falsified (three times a vsync) | Plausible: one caller and one count from it, and a per-frame view build is what a renderer normally does. Three callers enter it - two in the field overlay, one in SCUS - on 133 of 134 sampled vsyncs. On a scene-entry frame two of the three read different live camera words, so "the frame's view matrix" is not a single object. The row "built three times every vsync" below refines the count. |
| `FIELD_CAM_DEPTH` cannot be derived, only calibrated | falsified (it falls out of the scale) | Plausible: the constant was fitted to make one scene's framing match while the composed eye trio was not fed to the view. The eye trio *is* the eye-space translation, in GTE units the base matrix does not scale, so a renderer drawing at `1x` reproduces the frame by dividing the trio by that scale - the perspective divide is invariant under a uniform scale of the whole eye-space vector. |
| The dance count-in numerals are animated by `FUN_801D2D98` | falsified (that routine draws the banner; `FUN_801D3FD0` spawns the numerals) | Plausible: the count-in is one visual event, and `0x77` / `0x78` appear in the emitter's operands where widget ids would. Those two are **y seats** - the emitter clears `a2` at `0x801D2EBC` / `0x801D2EEC` / `0x801D2F04` - and `1 2 3`, `GO!` and `FINISH!` come from the sprite spawner. |
| The field view matrix is built three times every vsync | falsified (two or three times per **field frame**, and the first site is optional) | Plausible: one site entered on 133 of 134 consecutive vsyncs reads as "every frame". Over 1800 captured vsyncs of a world-map-to-town entry only 749 carry a build at all; of those 389 run the full three-site order and 313 only the last two. A static town scene splits 504 / 396 the same way. The builder runs per field frame. |
| The last build before the draw wins, so the trio's last reader is the frame's camera | falsified (the last build frames no geometry at all) | Plausible: the last write before a read is the one a read sees - true of the GTE registers, and silent on when primitives are emitted. Ranked by ordering-table link, 16810 of 31046 fall under the first build and 14 under the last, a count that mixes attribute packets and 2D rects with polygons. By GPU command code, of 4289 polygons over three runs 3861 are under the first build, 428 under the second and **none** under the last ([settled](re-settled-threads/rendering-camera.md)). |
| The builder's two extra call sites are in a resident field-render module | falsified (they are PROT 0901's world-map bracket) | Plausible: the capture records them returning into the slot-B window, and slot B does host render code. Exactly one statically extracted overlay image holds `jal 0x800172C0` inside that window - PROT 0901, the world-map render module - and the run is a world-map one (`map01`, mode `0x03`). Both sites are one bracket in `FUN_801F73E4`: save the yaw word `_DAT_8007B792`, zero it in the first call's delay slot, draw one screen-fixed band, restore and rebuild. A field scene entry has at most the three field sites. |
| Each landmark TMD passes once per frame through `FUN_8002735C` | falsified (the near arm takes every one) | Plausible: that renderer walks the per-mode descriptor table, and the dispatcher's case 5 names it - behind a gate. On `map03` the `+0x42` test fires 756 times and takes the near arm every time; a census over 720 vsyncs of four states records zero entries against 10621 into the per-prim leaf ([settled](re-settled-threads/rendering-camera.md)). |
| `FUN_80029888` is reached whenever `actor[+0x7A] != 0` on the overworld | falsified (0 of 5089 gate hits) | Plausible: `+0x7A` is the choice between the two near-arm leaves. It is the **inner** choice: nothing reaches either leaf's env-mapped half unless the outer `+0x42` gate is non-zero, and across four sampled states every one of 5089 gate reads is zero. |
| Nothing on the disc raises `actor[+0x42]`, the mesh-renderer gate | falsified (three writer families) | Plausible: 5089 gate reads across four states and three game modes are all zero - a statement about the *sample*. The allocator `FUN_80020DE0` writes `2` at `0x80020EC0` when the world-map dev counter has bit 1 up, move-VM op `0x10` writes its own `u16` operand into the field at `0x8002342C`, and field-VM `4C C2` writes its operand byte at `0x801E26FC` - nine scenes issue it with `1`. What survives: across the sampled modes no **drawn** actor has the bit up ([settled](re-settled-threads/rendering-camera.md)). |
| Move-VM ext `0x19` / `0x1A` are writes of five operand words into the "world struct" | falsified (`0x19` adds; `0x1A` computes a yaw row) | Plausible: `0x17`, `0x19` and `0x1A` are all eight halfwords wide and address the same `0x14`-stride row. `0x19` loads each halfword and `addu`s its operand (`0x801D3C5C..0x801D3CDC`); `0x1A` reads only `op[3..=7]` as a yaw and a vector, rotates the vector through the trig LUT and stores `0`, the yaw, `0x400` and two offset clip words (`0x801D3CE0..0x801D3D80`). The row is the object-effect parameter table `FUN_8001C204` reads ([settled](re-settled-threads/rendering-camera.md)). |
| `FX_OUTLINE` draws the page's yellow strips | falsified (it is off on both hosts) | Plausible: the strips are flat untextured quads, which is the outline pass's look. `play_battle_fx.rs` sets it `false` and the native pass sits behind `LEGAIA_DIAG_FX`, so the strips have another producer. |
| The engine's `fade::FadeState` is the retail fade ramp | falsified (a target-latching model disagrees on a ramp's last frames) | Plausible: both model the one `+0x7C` block. Retail does not latch on the target: it keeps accumulating and clamps on the delta's sign, a delay of `n` suppresses `n - 1` frames, and a hold of `0` retires the frame after the ramp lands. A `REPLACED-BY` marker would hide the difference. |
| Actor render mode `4` (`+0x56`) is set from an asset, so a census of carriers finds its users | falsified (every write of `4` is code) | Plausible: other render-mode values arrive in data. The four stores are move-VM arms `0x80023460` / `0x800237E4` / `0x80023F98` and `0x8004D574`; no carrier holds the value. |
| A fog half-sheet is a world-space quad `0x80` units tall | falsified (a view-space billboard) | Plausible: the particle has a world position and a world quad lands at the right place. `FUN_8003F86C` `RTPT`s its corners through a matrix whose rotation is the base matrix with `RotMatrixX(0x400)` folded in and whose translation is the particle's view point, so the sheet is screen-aligned at the particle's depth; projecting world corners foreshortens it by the pitch's cosine, about `0.84` on `map01` ([settled](re-settled-threads/rendering-camera.md)). |
| The overworld fog reads brighter than retail's because of its colour law | falsified (the colour matches packet for packet) | Plausible: a brighter frame suggests a brighter colour. Every one of the 104 fog packets in `keikoku_chest_preload`'s walked table equals the engine's `grey * tint * brightness >> 15` for its record rolled back two passes; the "72 records at median 41" figure is the pool's current ages, not the ages the drawn table was built from. The residue is draw order ([open](open-rev-eng-threads.md#battle--rendering)). |
| `FUN_800271A8` builds a depth ramp for a GTE reset | falsified (the overworld screen-Y curvature table) | Plausible: it fills a `0x4000`-entry quadratic ramp. It then `RTPS`es points along that ramp and keeps `SY - 0x78` in a second table that the overworld leaves, the fog emitter and the drop shadow add to `SY` ([settled](re-settled-threads/world-map.md)). |
| `FUN_800460AC` is a GTE `NCDS` billboard helper, and `FUN_8001C394` draws a 2x2 gouraud grid | falsified (an `RTPT` grid projector and four textured quads) | Plausible: the pair runs GTE ops and emits a grid. `cop2 0x280030` is `RTPT` over a 3x3 grid `0x20` apart, stored from scratchpad `0x1F800020`; `FUN_8001C394` emits `POLY_FT4` (`0x2E`) with the blob texture - the field drop shadow, port `World::field_drop_shadows` ([settled](re-settled-threads/rendering-camera.md)). |
| `opdeene`'s black plants come from the sepia law or from retail's rewrite missing their words | falsified (a native-only restage keyed on colour) | Plausible: 1565 of the scene's prims carry colour words the grade takes to black. Retail's words are identical at the crawl's start and middle and its billboards draw at `(98, 94, 42)`. A restage that gives the prologue's dim ambient to every `0x80`-colour vertex catches the authored billboard words too; bisecting the draw list by pack slot removes every silhouette ([settled](re-settled-threads/rendering-camera.md)). |
| `FUN_801E0080` is a battle-arena sprite scatter over unparsed disc pools | falsified (it is the effect-VM walker) | Plausible: two script-driven pools and a quad emit read as a self-contained emitter. The disc's one call (`jal` at `0x80048128`) names `0x801E0080`, two words ahead of the walker's prologue at `0x801E0088`; the pools are a zeroed battle-heap slice, and the scripts are `efect.dat` (PROT 0873), which the effect VM parses ([`effect-vm.md`](../subsystems/effect-vm.md)). |
| `FUN_801CF754` is a render cull | falsified (it is the field contact broad phase) | Plausible: a `±0x180` window over the actor list reads like visibility. The window is centred on the player, and the table it fills is read only by the contact probe `FUN_801CFC40`; nothing that draws reads it ([`renderer.md`](functions/renderer.md)). |
| `ctx+0x272` is a battle scene-teardown byte, and `gp+0x9F5` a debug byte | falsified | `FUN_80046A20` raises `ctx+0x272` every frame (`0x80047104`), so the first body drawn runs the frame's global passes; `gp+0x9F5` is `0x8007BD0D`, the formation's second monster id, measured over 25 states ([`battle.md`](../subsystems/battle.md)). |
| No shipped move program keeps the camera yaw factor | falsified (`urudre1` stager record 14 writes `0x0080`) | Plausible: a disc-wide census over every op-`0x15` site finds none. The census walks each program to its first `HALT`, and the carrier sits on the taken side of an ext `0x37` branch that exists to jump that `HALT`. See [`renderer.md`](../subsystems/renderer.md#camera-relative-nodes-fun_8001cf50). |
| No store writes the render-mode-4 selector bits `+0x9E & 0x6000` | falsified | A census of stores misses the move VM's own: op `0x42` ORs `0x2000` (`0x80023FBC`, through `actor + 0x80`) and op `0x23` `0x4000`. Five shipped op-`0x42` programs sit in slot-B images; 60 of 188 live mode-4 nodes over 97 states use `0x4000` ([`effect-vm.md`](../subsystems/effect-vm.md)). |
| `FUN_801D6910` / `693C` / `6968` / `6994` build a vertex packet | falsified (they fill a camera-glide record) | Plausible: four small setters writing consecutive halfwords. The record is `0x80070764`, and `FUN_80021248` consumes it (`0x801D044C`, `0x801D4738`). |
| Baka roster record `+0x44` is a spawn Y | falsified (it is the stand-off) | Plausible: it sits among position-shaped fields. The duel init places each fighter at X = -/+(`+0x44` + 200) (`0x801D005C..0x801D00F4`). |
| Baka state `0x6D` is the match-result state | falsified (it is the secret opponent's tally variant) | Plausible: it follows the duel. The ordinary tally exit is `0x801D0A38`; `0x6D` is its variant gated on `0x801DBF06`, and the win flourish is stored at both (`0x801D0AA4`). |

## Measurement readings

Falsified claims about the *instruments*, not about the game. Each was a
plausible reading that shaped what work looked worth doing.

| Thread | Verdict | Why |
|---|---|---|
| `FUN_801DD088` is reached by a PC-relative branch, so it may be an interior label | falsified (the one branch hit is another image's) | Plausible: the reference sweep reports one branch to `0x801DD088` and no `jal`. That branch is in PROT 0898 at the aliased VA `0x801DD078`, not in PROT 0899; `find-address-word-refs.py --prot --home 0899` reports `branch_alias=1`, every other form zero. The address is a real fourteen-instruction leaf starting after the previous routine's epilogue, with no reference of any form. |
| A retail battle load raises a first-chance exception in PCSX-Redux | falsified (only a patched image does) | Plausible: one such exception is on record, and the runner's isolated profile zeroed the exception mask on that reading. With the mask armed on a staged retail image the same field-to-battle load reaches battle mode clean; the recorded exception is `w3a/registrar_sol`, whose patched `0898` jumped into an arena the retail state held as zeros. |
| PROT 0896's `jal` misses are a mis-based dump | falsified (a foreign-build link) | Plausible: the `0x801C5818` import is mis-based. Imported at its own base `0x801D4DF0` the misses remain - rebasing cannot move a decoded target - and each lands mid-body in this disc's executable ([`call-target-integrity.md`](../tooling/call-target-integrity.md#the-image-at-its-own-base-still-misses)). |
| The Baka print of `FUN_801D0F1C` matches the menu dump for its first thirty-nine instructions | falsified (sixty-three) | Plausible: one branch among them differs, which reads as the end of the match. That branch's operand differs by exactly the image delta, and operand for operand the two agree through the sixty-third instruction ([`minigame-baka-fighter.md`](../subsystems/minigame-baka-fighter.md)). |
| Writing `0` to `_DAT_8007B458` selects Yes on the name-entry confirm | falsified (the frame still shows No) | Plausible: it is the confirm's cursor word, and the interpreter driver writes it. On the recompiler path the S3 re-shoot takes, the frame after the write still shows the cursor on No; the driver keys presses on the name-entry actor's own sub-state instead ([`pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md#re-shooting-the-s1s5-anchors-on-an-unpatched-image)). |
| A reference sweep may pair a `lui` with any later memory access until the register is reloaded by another `lui` | falsified (6,015 of 23,200 pairs were false) | Plausible: it is how the idiom usually reads. A register overwritten by an unrelated instruction in between still pairs, so `lui v1; lw v1, ..(v1); lbu v1, 0(v1)` reports a reference to the `lui` page. The strict walk drops the register on any write. |
| PROT 0897 / 0899 hold 23 / 44 indexed-form accesses the byte account cannot see | falsified (they hold none) | Plausible: a Python scan reports them. It is the lax pairing of the row above. |
| `FUN_801D84B4` dumped from PROT 0972 / 0976 is code | falsified (padding followed by data) | Plausible: a dump exists under that label and the byte account credits it by name - 22 KB across the two images. It has no prologue and no `jr ra`; the attribution CSV calls the window `zero_window`. |
| `0x801C9688` / `0x801C2B2C` are PROT 0897 relocation copies of the world-map emitters | falsified (they are `FUN_801D7EA0` / `FUN_801D1344` printed `0xE818` low) | Plausible: the bytes match the emitters exactly, which is what a copy looks like. They are one routine each, printed under a mis-based dump. |
| `0x801F90DC` is a Baka Fighter item-acquisition caption | falsified (a mis-based print of the menu overlay's item-info panel `FUN_801D0F1C`) | Plausible: the operands check out, and a waiver that checks only operands lets the tag stand. Every attribution row for that VA is `misbased`. |
| PROT 0929's run has no spawn site and slot-B `$a2` records are always formed directly | falsified (a `lui` above a branch with the `addiu` in the call's delay slot; also register copies and switch-arm delay slots) | Plausible: the layout walk finds every other record by a direct `lui` / `addiu` pair into `$a2`. |
| The fog rows need a composing ladder that does not exist | falsified (it exists, green and unlisted) | Plausible: rows that sit unconverted across exports usually lack a ladder. The ladder is in the tree and green but absent from the export's list. Check the fixture directory before costing a new fixture. |
| A composed-overworld ladder would convert the three field effect handlers | falsified (none of the three has a production constructor) | Plausible: the three ticks look like ordinary field effects, so composing their scene reads as the missing step. No production path constructs an actor on any of them - the gap is a **constructor**, not a ladder - and a port tag that does not disclose that makes the rows read as merely unentered. |
| `sell_quantity_draws_for` has no native call site | falsified (it has one) | Plausible: a grep reports nothing. The grep ran through a `\| head` pipe truncated before the site, and a truncated search reads exactly like an empty one ([`shell-observer-traps.md`](../tooling/shell-observer-traps.md)). |
| The page never replays a camera snap, so the snap beats are a page gap | falsified (the feature was dead on **both** hosts) | Plausible: a side-by-side read finds the beats on one host and not the other, the shape of a real drift row. The native arm watched `pending_field_events` after the camera had already drained the configure events, so it saw none either. A one-host read reports the host it looked at second. |
| The Muscle Dome hub's title art is drawn only by the page | falsified (both hosts draw it) | Plausible: the constant lives in a page-facing module and the native hub is thinner. Both hosts reach the same builder; what differs is which frames each draws it on. |
| The minigames page gates the catch HUD on a phase the engine does not have | falsified (retail gates on one word, and the port matches) | Plausible: the page has an extra predicate the native path lacks, and an extra predicate is usually the drift. Retail gates the depth readout and the tension bar on the single word `DAT_801d91b4`, set at the hook; the page's extra gate is its own idle phase, a session-model difference rather than a fidelity one. |
| A reach-triage cell that says a row is not driven is a measurement | falsified (it is prose next to an address) | Plausible: the rows carry addresses and a checker runs over the page, so the cells look audited. The audit resolves the addresses a row cites and says nothing about the sentence beside them: nine of eleven "content not driven" rows were already entered by a canonical ladder. Re-measure a reach cell before planning work off it. |
| A mode-seat parity check can compare the two hosts' mode words | falsified (an INIT mode never survives the call that enters it) | Plausible: both hosts expose a mode word. The seat's entry call resolves an INIT mode and hands off to RUN before returning, so no post-call sampler on either host observes one; and the word one host exposes is a front-end flag rather than the mode index. The witness that works is the edge **count**. |
| The two 24-instruction tally helpers at `0x801D14B0` and `0x801D6710` are different routines | falsified (one routine, linked into two overlays) | Plausible: a byte comparison of the two extents reports nothing in common. The relocated `lui`/`lw` pair that materialises the gate word comes **first** - `0x801D1AB4` in PROT 0977, `0x801DBF00` in the Baka Fighter image - so the comparison diverges on instruction one and never recovers. The other 22 instructions are identical. |
| PROT 0900 ships the same ground emitter twice | falsified (a depth-cued / flat **pair**) | Plausible: the two bodies are near-identical for hundreds of bytes, sit in one image, and are chosen between by a single global. The difference is two instructions: `FUN_801F69EC` runs `GTE.dpcs` at `0x801F6C44` and stores the depth-cued colour register (`swc2 $22,4($t5)`, `0x801F6C4C`), where `FUN_801F6D48` stores a plain colour word (`sw $s2,4($t5)`, `0x801F6F88`). The selector's non-zero arm calls `SetFarColor` first. |
| A slot-B module carries un-dumped code above its frame partition | falsified (it is the spawn-record band) | Plausible: the uncovered run scores as plausible MIPS, because a record's move-VM bytecode contains a `lui $rt,0x80xx` word by accident and the `in_data_segment` test rejects any run that does. It is `[i16 model_sel][u16 reserved][bytecode]` records, addressed by the consumer's own `lui`/`addiu` and handed to `FUN_80021B04` / `FUN_80050ED4` in `$a2`; 62 of 64 images carry one. |
| PROT 0943 and 0961 hold a second region of *their own* code above the record band | falsified (it is the donor's residue) | Plausible: a dump prints framed bodies at `0943 +0x135C..+0x17E0` and `0961 +0x1C60..+0x1D90`, above each image's records, and interleaved code and data is an ordinary layout. 0943 is byte-identical to 0942 from file `+0x1037` up, so its own content ends there and those bodies are 0942's; 0961 ends at `+0x1918` the same way, donor 0960. A dump printing at an address under two images' names does not say which owns it. |
| `0x801F99D8` is where a slot-B module's content ends | falsified (it is `base + 0x3000`) | Plausible: several images stop having recognisable content there, which looks like a structural boundary. It is the end of the four 12 KB images in the band; images of other sizes end elsewhere. |
| `0x801C4BEC` is the libcd directory-entry cache | falsified (`0x801CB408`) | Plausible: the address appears in dumps and is in the right region for overlay-adjacent scratch. It is the *offset* half of a `lui at,0x801d` / `sw ...,-0x4bf8(at)` pair pasted into the high half of an address; `FUN_8005DEA0` forms no address in `0x801C4***` at all. |
| `engine-core::dialog` ports `FUN_8001FD44` | falsified (it implements nothing of it) | Plausible: the address is a real routine in a related area, and no gate checks the pairing. `FUN_8001FD44` is the name-based scene-change packet; the code that implements it is the field VM's op-`0x3F` arm. A `// PORT:` tag naming the wrong routine passes every check that exists ([`port-provenance.md`](../tooling/port-provenance.md)). |
| A PCSX-Redux `.sstate` carries main RAM only | falsified (it also carries a 64 KiB hardware blob) | Plausible: a reader that exposes main RAM and nothing else reads as the format's whole surface. The blob holds the scratchpad at its own offset 0 (file `0x01080034` in one measured state), so the per-cell scratchpad joins a mednafen state answers are answerable in PCSX-Redux too, through `legaia_pcsxr`'s `scratchpad()` accessor. |
| The port-tag checkers read a tag's continuation lines | falsified (they read only the opening line) | Plausible: the tag format documents a wrapped list. The correct rule is narrow - a following comment line continues the list only when the text so far ends in a separator and the line starts with an address token. Measured when the rule landed it changed 0 of 879 addresses, where the naive "read to the first blank line" rule would add 47 addresses nobody claimed. |
| The dump-extent attribution CSV lags the corpus | falsified (it does not) | Plausible: attribution lag is a real, documented failure of the neighbouring instrument. The CSV is current; the number that looked like lag has another source. |
| The disc-coverage report's excluded dumps are "typically the ones that report `0 instructions` and hold only decompiled C" | falsified (zero of them reported `0 instructions`) | The files that *do* report `0 instructions` pass the header regex and are credited a byte each. Of the excluded set, three are C-only and four fifths are not dumps at all - pointer stubs, recorded negatives, data windows, analysis output. The count was real; the sentence attached to it had not been checked against the files. |
| The inner of two nested overlay spans "cannot be repaired ... no amount of dumping moves it" | falsified | Address ambiguity is total for the inner span - every extent in it falls in both by construction. Byte attribution places most of those extents in one image or the other, and the row reports. The **starting point of a measurement was mistaken for its limit**. |
| The unattributable residue "is repaired by re-dumping, not by extracting another overlay" | falsified | Re-dumping repairs almost none of it. What remains is windows a few instructions long that no image reproduces at that VA, bytes in no extracted image at any VA (which needs an *extraction*), and extents where two dumps genuinely disagree (which is an answer). Count a residue from the artifact, not from its class names. |
| `0x8005BA38` is "not a function - the dump reports `size=1 bytes, 0 instructions`" | falsified (it is a complete `RotTransPers`) | The routine is 11 instructions: load `VXY0`/`VZ0`, `RTPS`, store `SXY2` / `IR0` / the GTE `FLAG` word, return `SZ3 >> 2`. The row quoted an empty dump; **a claim quoting a dump statistic decays silently** when the dump improves. Siblings: a "truncated dump" at 752 bytes that is 1528, and `0x8003D38C`'s ignore row, whose *verdict* survives - it is one instruction past the real entry `0x8003D388` - but whose stated evidence does not. Checker: `scripts/ghidra-analysis/check-dump-stat-drift.py`. |
| "About 2 % of retail camera beats set a non-zero roll" | falsified (the figure was the scan's own filter) | The number comes from a **byte scan** - decode an op-`0x45` CONFIGURE at every offset of every scene MAN - which finds 4257 "sites" where control flow reaches 371. Junk sites set a junk roll almost every time, so the scan applies a "credible" filter and measures roll over the survivors; the ratio is a property of that filter. A strict linear sweep reports the opposite (zero non-zero rolls) by reaching 21 sites and none of the eight real ones. Executing the records gives the answer - retail *does* roll, in eight scenes: [`re-settled-threads.md`](re-settled-threads/rendering-camera.md#does-any-retail-shot-author-a-non-zero-camera-roll). |
| An over-strict header regex is one instrument's bug | falsified (every instrument had its own) | A private header regex per tool, over a corpus that spells all four header fields several ways, makes each tool silently reject a different subset of **real dumps** and report them as a corpus deficiency. One shared parser replaces them; see [`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md#not-every-file-in-funcs-is-a-dump). |
| A zero-reference SCUS function is a safe code cave once the five-form address scan and hours of live probing clear it | falsified for `FUN_800605C8` (boot-live) | The libapi VBlank-tier slot has no static reference of any form and every live probe runs clean with it overwritten - yet a **cold boot** parks at boot mode `0x10` the moment its body changes, because the kernel/libapi init invokes it before any save state's world exists. Save-state probes cannot exercise boot; a claimed cave must also pass a cold-boot watch (`scripts/pcsx-redux/autorun_boot_watch.lua`, bisect via disc variants). Neighbouring CD-arm caves `FUN_8003EDAC` / `FUN_8003F210` pass the same cold-boot test. |
| PROT 0977 `0x801D1EF0` is data past the last `jr ra`, carrying no function | falsified (it is the arena settlement bring-up) | A sector-granular PROT extent truncates the body, so a missing `jr ra` is not evidence of data - the tail still carries RAM-page `lui`s and calls `0x8006BCB4` / `0x80026018` / `0x80024EE4`. Same shape in PROT 0902 (`0x801CED68`, a third function) and PROT 0979. |
| SCUS `0x80045CB4` is interior to a body 11 128 bytes back and nothing references it | falsified (entry `FUN_80045BB4`, 256 bytes back, referenced) | Its address is word 12 of the bank-3 primitive-handler table at `0x8007668C` (kinds 8..19); the routine is a 1272-byte frameless GTE emitter ending in `j 0x80045E54`. The table reproduces from the SCUS bytes. |
| `overlay_0897_xxx_dat_801f138c.txt` is a routine at `0x801F138C` | falsified (it is `FUN_801DABA4` printed `0x167E8` too high) | Its absolute `j 0x801db0f0` / `j 0x801db0f8` targets give the true base, and an instruction-by-instruction diff against `overlay_battle_action_801daba4.txt` differs only in the 33 PC-relative branch operands. Reading it as a second monster-record `+0x1C` consumer double-counts one routine. |
| The highest spawn record's end is unbounded, because a move-VM walk lands within 4 bytes of it in only about three quarters of images | falsified (the near-miss was the missing alignment step) | Plausible: a rule that is right most of the time and off by a few bytes the rest reads as an approximation of no rule. The records are word-aligned, so the walk's end rounds up to 4 - after which 991 of 1027 extents are exact and 58 of 62 tops are bounded. |
| A build-buffer tail can only come from an image sharing this one's load base, and only from a longer one | falsified (both restrictions) | Plausible: a donor at the same base makes the printed addresses line up, and a shorter donor cannot reach a longer image's end. The mastering buffer is indexed by **file offset**, so five cast-band images end in the menu overlay's code and the game-over image ends in the world-map renderer's; and `content_bytes` is a sector extent, so a donor of nominally equal length still supplies bytes. |
| An uncovered run in an overlay image is a code gap | falsified (the same measurement classifies most of them otherwise) | Plausible: a run of bytes no dump covers is the definition of a gap, and ranking runs by size is the obvious worklist. The instrument also shape-classifies each run - `no_exit`, `no_boundary`, `constant_table`, `return_tail`, `psyq_lib_stamp` - and a total that counts every byte of every run anyway ranks mostly its own rejected shapes. |
| An image's frame partition bounds its own content | falsified (a partition can hold a donor's whole function) | Plausible: frame matching recovers real function extents, and a function that starts and ends inside this image's bytes looks like this image's. PROT 0949's partition holds `0x801F8504`, which is 0948's stager sitting in 0949's inherited tail. What bounds own content is the record chain's top, not the highest frame. |
| A VA that appears in two neighbouring images names one routine measured twice | falsified (compare the bytes) | Plausible: the band's images share a load base and a build buffer, so the same VA printing in two of them is usually residue. `0x801F81DC` is **two routines**: a 272-byte stager in PROT 0951 and a 2040-byte applier in PROT 0910, differing at the first instruction. Six other "also in" cells around it are residue, verified pair by pair. Diff the bytes; assume neither way. |
| Correcting a function's extent in the attribution CSV corrects the corpus | falsified (it orphans the body) | Plausible: the CSV is where extents are read from. The extent is produced by the shared dump header parser and regenerated from it; a CSV-only edit leaves the dump still claiming the short extent, so the whole body becomes VA-ambiguous between the two lengths. Fix the parser, then regenerate. |
| A decompiled function's dump covers its body | falsified for `FUN_801DD9D4` (all seven dumps stop at 276 of 588 bytes) | Plausible: seven independent dumps agree on a length. The decompiler stops at the `jr v0` jump table at `0x801DDA88`, which the `beq` at `0x801DDA78` branches past; the body runs on to the `jr ra` at `0x801DDC18`. Agreement across dumps of one program is agreement about the *program*. |
| A feature view's Port % is a property of the port | falsified (the ignore list sat on both sides of the fraction) | Plausible: a percentage that moves when work lands is what a progress figure should do. With ignored rows counted in the numerator *and* the denominator, the headline also moves when an ignore row is added and nothing about the port changes - `cd-io` read 2.6 against a true 100, `field-vm` 66.2 against 100. |
| Every slot-B record-end miss stalls below the measured end, so the rule cannot over-claim | half falsified (the direction is not uniform) | Plausible: a walk that dies on a halfword it cannot decode intuitively stops *early*. An independent walker over the 1023 pointer-credited extents finds 990 exact and **33 misses: 14 walk past the end** before dying on a non-opcode halfword, 13 stop exactly on it and 6 stop below. What survives is the property the consumers need: no miss ever **terminates** above the end, because only a terminator - `0x08` HALT or an armed idle loop - produces a claim at all. A direction measured on one walker is a property of that walker. |
| A backward scan of a function finds every source of the value it stores | falsified (the value can arrive through a pointer argument) | Plausible: for a register-formed constant the scan is exhaustive. `FUN_801D6704` seats the player from a stack pair that `FUN_8003AEB0` fills **through a pointer argument** - `a1 = sp + 0x20`, set in the `jal`'s delay slot at `0x801D6DAC`, with the writes `sh v0,($s6)` / `sh v1,2($s6)` at `0x8003B7D0` / `0x8003B7D4` off `_DAT_80073EF4` / `_DAT_80073EF8`. Scanning `FUN_801D6704`'s own 3604 bytes for `0x80073EF4` returns nothing. |
| An open-ended CDNAME block's range is safe to use as a length | falsified (it reserves the rest of the address space) | Plausible: every other block in the map is bounded by the next `#define`. `other7` runs from entry 1226 to the end of the map, so the un-clamped range feeds a `Vec::with_capacity` of 64 GiB on a scene load - which Linux overcommit grants silently. Reproduce this class with `ulimit -v` on the suspect binary: an overcommitted reservation only aborts under a cap. |
| `asset overlay scan`'s recovered base is a vote | falsified when the image offers one prologue | Plausible: the instrument is a vote over recovered prologues and reports a winner. On PROT 0981 it answers `0x801D58B8` off a single prologue; decoding the image's own `lui`+`addiu` pairs against each candidate scores 21 of 23 at slot-A `0x801CE818` and 0 of 23 there. Nothing in the output says how many prologues the margin rests on. |
| The browser play page emits no audio | falsified (an observer-order defect) | Plausible: the probe reads zero in every block on every scene, which is what silence looks like. The listener was registered **before** the mixer installed its own `onaudioprocess`, so it watched a handler that was later replaced. Intercepting the setter gives 82 non-zero blocks of 82 on town01 and 76 of 76 on the boot chain. |
| A dump whose window matches the image confirms coverage there | falsified (a `nop` encodes `0x00000000`) | Plausible: a byte-identical window is the strongest attribution evidence there is. A window of `nop` matches every zero hole on the disc: a 20,060-byte dump signs for a 131,172-byte zero run in an image it does not belong to, and two dumps at one address "agree" because both windows are zeros. Attribution needs a non-zero discriminator; the sweep classes a zero window separately. |
| PROT 0896 holds no strings to anchor it | falsified (its head is a label table) | Plausible: base fitting found no base and no Shift-JIS *code*. The head is a Shift-JIS label table and the image carries a format string unique on the disc, so the entry has identity evidence whether or not a base fits its address-forming pairs - the two questions are separate. |
| A port's eye-Z figure quoted against retail names a state that exists | falsified (one of the pair had no state) | Plausible: a pair of numbers reads as one measurement, and the free-roam half reproduces exactly. There is no `town01` mode-5 state in the library, so the `town01` half quotes a framing nothing measured. Label every relayed figure with the state it was taken on. |
| PROT 0981 is a monster-test harness | falsified (it is the world-map top-view debug image) | Plausible: the entry's CDNAME label reads `monster_test`. Labels inherit forward from the block that opens at extraction 0978, so the name says which block the slot is in and nothing about its content. The image's own operands are world-map ones - the location table `DAT_80073EE0`, the kingdom filter, the camera pair - and its entry `0x801CE850` is the top-view prologue ([settled](re-settled-threads/world-map.md)). |
| `0x801CE9C4` is a 324-byte routine | falsified (it is a `jr $v0` arm inside `FUN_801CE850`) | Plausible: the corpus prints a body at the address and the bytes are real. The tick at `0x801CE850` bounds its own dispatch with `sltiu a0, 6` over a six-word table at `0x801CE838`, and all six words land inside that one 3164-byte body. Nothing about the image's load base depends on it. |
| `0x801D388C` hosts a Muscle Dome routine as well as the battle flow state machine | falsified (one routine; the dumps differ only in prefix) | Plausible: two dumps with different overlay prefixes at one VA is the signature of slot-A aliasing. PROT 0977 is `0x3800` bytes from `0x801CE818` and ends at `0x801D2018`, below the address; the `overlay_muscle_dome_` and `overlay_battle_action_` dumps are the same instructions of PROT 0898. A dump prefix records the capture a dump came from, not the image its bytes belong to. |
| The browser play page's frame path has no early-out | falsified (the early-out is in the page's JavaScript) | Plausible: the Rust runtime's `tick_frame` has none. The page gates the whole call to `tick_frame` behind its own condition and draws outside it, so a guarded frame runs none of the per-frame kernels and still paints - the same shape as the native loop's `continue` arms, one language further out. A drift tier that reads only Rust cannot see it. |
| `0x801CF344` may be a phantom print from a mis-based dump | falsified (it is PROT 0897 data) | Plausible: the `0x801C****` / `0x801D****` band is full of addresses whose printed VA belongs to no runtime image. This one is the field overlay's own file offset `0xB2C`, formed by `lui`/`addiu` pairs inside the renderer's body, and a live `map03` window is byte-identical to 0897's head. It looks unattributable because PROT 0981 aliases the same VA and the two are never co-resident ([settled](re-settled-threads/world-map.md)). |
| The native minigame step runs fishing venue actors and dance count-in spawns the browser has no twin for | falsified (both reach the browser) | Plausible: the waiver was written from the native side's call list, and a kernel named in one host only is the ordinary shape of drift. The dance count-in spawns reach the browser. The waiver's replacement list - the dance sequence-clear spawns, the Baka round chrome and the effect-pool ageing, all on the ground that only the native window owns `window/minigame_fx.rs` - is itself corrected by the next two rows. A waiver naming the wrong members hides the reason they are missing. |
| ... and its correction named the effect pool as the one reason | falsified again (the dance spawns were never the pool's) | `DanceGame::judge_press` spawns the three sequence-clear parts into the **run's** own pool, not the host pool. Under the one-cause reading the native window spawned a duplicate set and drew both - banner and stars twice per cleared sequence - the play page drew neither, and the minigames page drew them from retail's widget cells. The pool is world state and all three hosts drain it. A waiver that names one cause for a group has to hold for each member. |
| ... and the fishing venue actors did reach the browser | falsified (they were native-only) | Only the native window ticked the venue actors until the kernel became `fishing_venue::tick_fishing_venue_on_host`, which both play hosts call. The Baka round chrome listed as missing reaches both browser pages; only its `glyph_u` cell rect was native-only ([`host-drift.md`](../tooling/host-drift.md)). |
| No load base makes PROT 0896 self-consistent | falsified (`0x801D4DF0`, on the call graph) | Plausible: a window slid across every address the file's `lui`+`addiu` pairs form, with a pinned control holding all but two of more than twelve hundred, finds no base. A resolution ratio is **one-sided** - a base whose high halves catch few pairs scores perfectly on all of them - and on this image it reports 65 of 65 at the refuted slot-A base against 110 of 177 at the true one. The call graph carries the answer: ten corroborating `jal` targets, 218 internal `j` all landing in-file, and three in-image VA word runs ([settled](re-settled-threads/title-boot-overlays.md)). |
| PROT 0974's uncovered run is a pointer table | falsified (sparse `.bss`) | Plausible: the run is long, low-entropy and sits where a table would, and "pointer table" is a residue class this corpus carries. It is 81 % zero with a single RAM word in it - an uninitialised data segment, as a code image's tail usually is. The zero fraction is the cheapest test of a residue class. |
| The `0x2399C` run in PROT 0897 is a halfword table | falsified (it is the field overlay's **data segment**) | Plausible: its head is a table - the locomotion probe footprints. The segment is file `0x2399C..0x25000`, 5732 bytes, with 231 distinct sites in the image forming addresses inside it; the probe table is its first 192 bytes, twelve rows, of which the locomotion reads four. Do not name a segment after the first structure in it. |
| A frame kernel paired by name does comparable work on both hosts | falsified (an empty body pairs with anything) | Plausible: the host-drift gate's tier 11 pairs a step on each host under a written reason. A native `tick_field_prop_anims` with an empty body paired with a browser twin that drained the ANIMATE cues and advanced every NPC clip, under an alias reason asserting both "advance the scene's posed actors". Tier 12 compares the engine call sets instead, and states its own hole ([settled](re-settled-threads/measurement-corpus.md)). |
| `engine-core` has no host hook for field-VM op `0x34` sub-0 | falsified (the hook is live) | Plausible: a port tag said so. `World::op34_sub0_color_intensity_setup` is reached on both hosts. The gap behind the tag was a **representation** conflict, not a missing call - and not the one the blocker named either: it said the renderers read an `effect_tint` ramp while the op filled a push pool nothing read, when *neither* representation had a reader. A blocker naming a missing caller directs effort at a caller that exists. |
| The port's field direction ring is byte-identical to retail's | falsified (retail's is `u32[8]`) | Plausible: the eight values agree. The remapper reads `DAT_800766FC` with `lw` and steps it by `addiu 4` (`0x80046818` / `0x80046834`), so retail's ring is eight **words**; the port's `FIELD_DIR_RING` is `[u16; 8]`. The *values* agree on all 64 (octant, direction) cells, which is the claim worth making. |
| 32 battle-presentation names are web-only - the native host never calls them | falsified (31 have native call sites) | Plausible: a `.name(` scan finds none. Path calls (`Type::name(`) are the native host's usual form, so scan `[.:]name(`; the 32nd, `packet_color::hybrid`, is the page's WebGL vertex-colour stream, which the wgpu fragment shader replaces. |
| The play page has no Records dev-menu page | falsified (it has one, from the same builder) | Plausible: the page's menu shows no Records row at the moment compared. `play_dev_menu.rs` builds it with the native builder and a drift-gate row pairs them; what differs is **when** each host builds the list. |

**Generalises to:** a measurement instrument has no oracle, so a number it prints
is believed on the strength of its *explanation*. Check the explanation against
the files, not against its own plausibility - several rows above are a correct
count with a wrong story attached, and the story is what directs effort.

### Two overlay lengths that were the neighbour's sectors

*Falsified by TOC arithmetic.*

| Reading | Why it looked right | What is true |
|---|---|---|
| `arena_init` (PROT 0977) own content is about `0x4800` bytes | The file the superseded entry size produces is that long and disassembles cleanly to the end | The entry is `0x3800`; the extra `0x1000` is PROT 0978's two sectors |
| The battle overlay (PROT 0898) is `0x28800` of `0x29800` bytes, with a diverging `0x1000` `.bss` tail | A RAM capture matches the first `0x28800` and the tail differs, which is what `.bss` does | The entry is `0x28800`; the diverging tail is PROT 0899's first two sectors |
| `0x801D2784` is PROT 0979's battle-intro transition tail | The dump is labelled 0979 and 0979 is the battle-intro overlay | The bytes are PROT 0976 (Baka Fighter). The two images are byte-identical from file `0x3C68` to the end of the smaller one, so no attribution sweep separates them - the **operands** do: the routine reads `0x801DBED8..0x801DBEF0` and calls `0x801D6710`, all past 0979's own `0x4000` and inside 0976's `0xE000`, bracketed by 0976's documented emitters `801D6480` / `801D6770`. Slot-A residue rule: a byte range shared by two images belongs to the one whose addresses it names. |
| `0x801DDA90` and `0x801DDB44` are two slices of one loop, neither a function entry | Both look like fragments - no prologue, each ending in a `j` to a shared tail | Right for `801DDB44` only (it is `0x24` into slot 4's arm, at the `j 0x801DDBC8` and its delay slot). `801DDA90` is slot **0** of the eight-entry `jr` table at `0x801CEC40` that `FUN_801DD9D4` dispatches through, and that table word is its only reference on the disc. A frameless routine reached only through a table is still a routine. |
| `FUN_801E59B0` gives its two trig tables two different angle indices | The C renders two subscripted loads with different-looking expressions, and sin at `angle` with cos at `angle + 0x400` is the textbook rotate: `vec[0] * t1[angle] + vec[1] * t2[(angle + 0x400) & 0xFFF]` | One index, both tables, components swapped. The instruction stream computes `i = (angle + 0x400) & 0xFFF` once at `0x801E59B0..0x801E59BC` and reuses it at `0x801E59C8` and `0x801E59E4`: the body is `(vec[1] * t_a[i] + vec[0] * t_b[i]) >> 12`. `0x8007B81C` and `0x8007B7F8` are table **pointers** the routine `lw`s, not the tables. |
| The PROT 0898 entry tables resolve the slot-B attribution residue | The three link-time tables are the right instrument for *reachability* and map the whole band | They close 6 extents / 48 bytes, and ten of fifteen table-derived attributions name a different module than the bytes do. The residue is a **byte**-denominated ambiguity between images that share bytes; a table that names an entry says nothing about which image the bytes at that entry belong to. |
| A `disc-coverage --check` floor regression means coverage was lost | A ratcheted floor going down is a regression for every other gate in the tree | It is attribution lag. The gate's denominator is the **disc** and its numerator is what the attributed dump corpus claims: a new, not-yet-attributed dump raises the denominator before the numerator, so the percentage falls while the corpus grew. Re-attribute, then re-read the floor. It is not a worktree artifact, and re-running in the main checkout does not clear it ([`disc-coverage.md`](../tooling/disc-coverage.md)). |
| `801D84C0` is wired because `panel_anchors` is called | The address appears in a tag on a module whose exported function is called from a live host | The bucket is a property of the anchor, not of the address. The `// PORT:` tag carrying `801D84C0` sits on `panel_labels`, a different item, and the live-audit walks from the **tagged item**. Reading liveness off a neighbouring symbol keeps an inert port's audit green. |
| The world-map overlay's per-prim handler table is based at `0x801F8988` | The first non-zero word of the table is there, and the eight words before it are zero | `FUN_80043390` loads `0x801F8968`: `lui s4,0x8020` / `addiu s4,s4,-0x7698` at `0x800435F4..0x800435F8`, plus the same `(flags >> 1) * 4` index it adds to the SCUS table `0x8007657C` - which has the identical shape, words `0..7` zero and `8..19` populated. Re-basing by the zero prefix shifts every kind by eight. |

The first two rows are readings of files cut with the superseded entry size;
`crates/asset/data/static-overlays.toml` records both corrections. The general
law is on [`prot.md`](../formats/prot.md); the measured consequence - the TOC
is a gapless partition - is on
[`re-settled-threads.md`](re-settled-threads/measurement-corpus.md).

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts.
- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that produced several of the readings above.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base.
