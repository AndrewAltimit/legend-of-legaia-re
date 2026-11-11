# Open reverse-engineering threads

The live reverse-engineering **questions** about Legaia's runtime: what is
settled, what remains, and what evidence would close each one. It is the short
page of a three-page register - most questions about the retail game are
answered, so the bulk of the register is the settled and falsified pages.

## What this page is for

Before starting a hunt, look for it here. If the question is not on this page,
it is probably already answered or already disproved - the two companion pages
below hold those, and checking them first is cheaper than re-deriving them.

| Page | Holds | Read it when |
|---|---|---|
| This page | Live hunts: `open`, `partial`, `mostly resolved` | You are picking up work, or want to know whether a question is still contested. |
| [`re-settled-threads.md`](re-settled-threads.md) | Answered questions, each carrying an evidence grade | You need the answer to something, or you are about to build on a claim and want to know how firmly it is pinned. |
| [`re-do-not-re-walk.md`](re-do-not-re-walk.md) | Falsified hypotheses, reasoning intact | A reading of the bytes looks obvious and you want to check nobody has already disproved it. |

A closed thread leaves this page: its answer moves to the settled register, and
a reading it disproved moves to the falsified register with its reasoning.
Nothing on any of the three pages counts ports, tests, or coverage. Captures
and disassembly dumps live in the linked docs and under `ghidra/scripts/funcs/`.

## What an evidence grade means

Every settled row carries one of four grades, naming what its own stated
evidence rests on:

| Grade | The row cites |
|---|---|
| `disassembly` | Instructions, addresses, opcode encodings, branch or store sequences. The strongest grade. |
| `capture` | A runtime capture, save state, probe, firehose, or disc-derived oracle. |
| `decompiled-C` | Ghidra's C output, a `FUN_x(...)` call signature, a Ghidra label or plate comment, or a claim about store order / store count / a boolean operator with no instruction behind it. |
| `inference` | Reasoning from surrounding facts, corpus absence, or analogy, with no direct evidence cited. |

`decompiled-C` marks a claim **nobody has confirmed against instructions** - not
a claim known to be wrong. The C is a rendering: dropped register arguments,
`||` printed as nested `if`s, reordered or omitted stores, and hand-written
Ghidra annotations read as fact have each put a wrong statement on these pages.
The catalogue of those rendering artifacts, which is also the grading rubric, is
[`ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).
When a `decompiled-C` row is load-bearing for something you are about to build,
re-derive it from the disassembly first.

## Status conventions

| Status | Meaning |
|---|---|
| **open** | Active hunt. A concrete next step exists; the row names it. |
| **partial** | The main result is pinned; a residual sub-question remains. |
| **mostly resolved** | The mechanism is pinned; one leg is unconfirmed. |

A row may qualify the status in parentheses - `partial (transcode closed)`,
`open (narrowed)` - naming how far it got.

## How a thread is laid out

Each area below holds a table of one-line rows: the question, its status, and
the evidence that would close it. An area with no live hunt says so and points
at the settled and falsified pages for that area. A thread whose write-up
outgrows a table cell keeps its one-liner in the table and links to a `###`
section after the table.

## Recently corrected

Readings a re-audit against the disassembly or a capture overturned. They are
listed here as well as in the registers, because a claim that was wrong once is
the cheapest place to look for a claim that is still wrong.

- **Worn gear teaches a summon, and the port never ran it.** The stat aggregator's
  grant and revoke scans were documented and tagged, but no code wrote the spell
  list: a Talisman taught nothing and a final-form Ra-Seru no Meta, Terra or Ozma.
  Running `FUN_80042558` off the disc against the port showed it, and the same
  instrument showed level-ups never rolling their stat spread
  ([method](../tooling/retail-kernel-differential.md)).
- **The record's `+0x08` array is spell experience, and `+0x00` / `+0x04` are XP
  words.** `FUN_801E70BC` compares it against `0x8007656C` (`lw 0x5D0(v0)` at
  `0x801E7208`), so "no consumer has been traced" was stale
  ([battle.md](../subsystems/battle.md#the-three-parallel-spell-arrays)).
- **The `0x75EA` CD input volume has a writer.** `FUN_8003D764` state `6`
  sets `a0 = 0x75` at `0x8003D92C` and reaches the shared `jal 0x8003EE7C` at
  `0x8003D9F4`; a scan for `117` loaded just before a call missed it, and the
  audio page's open item said the port mixed every line at unity while the
  port already mixed cues at that level ([settled](re-settled-threads/audio.md)).
- **The gold shop's 99 cap lives in the list, not the stepper.** The glitch
  page read the stackable path as capped by the stepper's `min(gold / price, 99, 99 - held)`;
  run off the disc, the stepper `FUN_801DB7F4` opened at 99 held still buys the
  copy the list seeded (`0x801DB2B4`). Its "every build has a reachable sibling"
  reading overstated it: the USA list builder marks a row built at 99
  (`0x80030F0C`) and the list refuses it before either buy path runs
  ([glitches](glitches/items-and-equipment.md), [shop](../subsystems/shop.md#screen-composition)).
- **The port's text widths were the draw layout's, not `FUN_80035F04`'s.** The
  balloon spawner and the world-map labels carried `FUN_80035F04`'s tag over
  `Font::layout`, which draws author and `0xCE` escapes with other advances;
  the measurer run off the disc matches `Font::measure`
  ([dialog-font](../formats/dialog-font.md#what-a-line-measures)). The Muscle
  Dome page's reading of the same call as a screen projection is corrected
  too: HUD element `0x1A` measures the fighter's name
  ([muscle-dome](../subsystems/minigame-muscle-dome.md#hud-elements-fun_801d8de8)).
- **Every `+0x16E` reader reads the whole halfword.** Seven port readers took
  one of the port's two halves (raw bits, tracker), so a Confuse the tracker
  held never reached the pickers and a cured Stone never put its palette back
  ([status word](../subsystems/battle-action-helpers.md),
  [palette](../subsystems/battle.md)).
- **`.MAP` and `.PCH` are field files, not sound-tool intermediates.** The
  two strings in the sound-driver cluster are referenced once each, by the
  field-file loader `FUN_8001F7C0` (`0x8001F834`, `0x8001F8CC`): they name a
  scene's walkability map and walk-on trigger sidecar, both on the retail disc
  ([settled](re-settled-threads/audio.md),
  [falsified](re-do-not-re-walk.md#audio--sound-driver)).
- **No retail entry is a truncated TMD.** The `tmd_size_prefix` class matches
  nothing on the corrected extraction: all 182 TMD-led stream entries hold the
  whole mesh, so there was never a tail for a loader to supply
  ([settled](re-settled-threads/measurement-corpus.md),
  [falsified](re-do-not-re-walk.md#containers--placeholder-slots)).
- **The captured "four-level jump" is one level.** Record `+0x00` is the
  cumulative XP and `+0x04` the next threshold, which `FUN_801E9504` rewrites
  (`0x801E9FA0`); the diff's `365 -> 730` is Vahn's L3 to L4 threshold beside a
  level byte `2 -> 3` ([settled](re-settled-threads/battle.md)).
- **A four-slot event list's beat is decided by the disc, and is rarely
  `0`.** The read index is the low byte of the entry's RAM address, and the
  battle heap places each monster block by a fixed law: ids load upward
  (`0x800543EC..0x800549F0`), each block is carved from the top of the free
  node, and the first ends at `0x800B9A50` with one member seated. Demon
  Fly's attack reads `209`, longer than its clip; the port had read `0` for
  every fight. The juggle window's measured overrun is the pass grid, not a
  cursor carry - the commit zeroes the cursor at `0x8004B068`
  ([settled](re-settled-threads/battle.md),
  [falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **The commit's "counter / guard window" is a sound-cue skip.** The anim
  commit's read of the juggle byte at `0x8004AFD4` seeds the cue-track
  cursor `+0x1F6` at `1` for a flinch (or block) re-staged inside its own
  window, so the restarted clip skips its first cue; nothing there parries
  or guards, and it reads the same `+0x1F7` as the juggle counter
  ([settled](re-settled-threads/battle.md),
  [falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **The ability-bit rebuild walks all three character records.** The stat
  aggregator `FUN_80042558` loops `k < 3` over `0x80084708 + k*0x414`
  (`0x80042D8C`) with no membership test, so a member who has left the
  party still feeds the global passive mask; the format page had said
  "each active party member", and the port walked only the active party
  ([settled](re-settled-threads/battle.md)).
- **The action seed's "camera bounds" walk moves the combatants.**
  `FUN_801EFE44` recentres the formation on the action's box - an
  Attack's target alone - so a melee opens with its target on the stage
  origin, and state `0x14` steps the bystanders out of the attacker's lane;
  the port had carried the routine as an inert event
  ([settled](re-settled-threads/battle.md),
  [falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **The party body radius is the seat stager's `0x280`.** `FUN_800513F0`
  writes it, `0x400` for Terra (`0x8005151C..0x80051538`); the port had
  used a guessed `448`, and ran the separation pass on every vsync where
  retail runs it once a battle frame
  ([settled](re-settled-threads/rendering-camera.md)).
- **The prologue's gold CLUTs are effect-script cyclers, and the law bends at
  the top.** Four mode-3 CLUT-cell cyclers grey and re-tint the CLUT rects
  `(0, 473, 320, 2)` / `(0, 476, 320, 34)`; green is `(L * 247) >> 8`, not
  `L - 1`, which parts company for `L >= 29`, and rows outside the rects stay
  ungraded ([settled](re-settled-threads/rendering-camera.md)).
- **A plain monster swing is elemental, and Astral Slash is one.** The
  melee kernel's guard ladder keys on the attacking monster's record element
  `+0x1D`, so a matching jewel halves an ordinary monster strike; the port had
  passed "non-elemental" for every swing. Astral Slash's module rolls no
  wrapper: its damage is Gaza's staged clip `0x0A`, four melee beats, which is
  why a Luminous Jewel halves it ([settled](re-settled-threads/battle.md)).
- **The "pre-`init_data` gap" is `init_data`, and only half of it boots.**
  Raw TOC entries 0 and 1 are CDNAME's `init_data` and `gameover_data`.
  `init.pak`'s `FUN_801CECD0` reads raw 0 and uploads it; raw 1 is the GAME
  OVER overlay's load, so the port no longer puts it in every scene's VRAM.
  The `title.pak` dev string is a load operand whose retail twin is
  extraction entry 0 ([settled](re-settled-threads/title-boot-overlays.md),
  [falsified](re-do-not-re-walk.md#title--boot--overlays)).
- **The `0x02018B0C` "effect bundle" is a pack of 29 TMDs.** The word is a
  type-`0x02` chunk header and its "schema offsets" are the pack's word
  offsets; the GAME OVER overlay registers the meshes
  ([falsified](re-do-not-re-walk.md#containers--placeholder-slots)).
- **A save point opens the pause menu by itself.** Op `0x49`'s `-1` rows
  (sub-ops `1` and `0x0D`) are a scripted menu-button press: the enter half
  stores handler `7` before it reads the table, so the "the park simply
  stands until Start" reading was wrong, and so was the menu-teardown leaf
  said to clear the park - it has no reference on the disc. A capture at the
  `town01` save point pins the chain and its release; every one of the
  disc's save points had saved nothing in the port
  ([settled](re-settled-threads/field.md),
  [falsified](re-do-not-re-walk.md#menus--ui)).
- **A shipped move program does keep the camera yaw factor.** The census
  that said none does walked each program to its first `HALT`, and every
  ext `0x37` is a pool-headroom guard that jumps that `HALT`; `urudre1`
  stager record 14 writes `0x0080` behind one. The port had also read the
  guard's operand against a constant "pool full", so every guarded field
  effect halted ([settled](re-settled-threads/measurement-corpus.md),
  [falsified](re-do-not-re-walk.md#rendering--camera)).
- **The slot machine's `rand % 5` picks a payline, not a sub-row nudge.**
  The `* 0x10` is the row stride of a landing-line table; forced stops land
  on any of the five lines ([settled](re-settled-threads/battle.md),
  [falsified](re-do-not-re-walk.md#battle--arts--level-up)).
- **BGM sub-op 3 pauses and sub-op 4 re-attaches.** The field VM's op-`0x35`
  arm table at `0x801CEE00` gives 3 = set pause bit 1 + `FUN_80026740` and
  4 = clear it + `FUN_80026478`, which replays the sequence from its start;
  the port had routed them the other way round, silencing the score at every
  sub-op-4 site ([falsified](re-do-not-re-walk.md#audio--sound-driver)).
- **Sub-op 2 stops and rewinds; sub-op 3 is the key-off pause.** Nothing a
  script writes resumes a track mid-phrase
  ([settled](re-settled-threads/audio.md)).
- **The menu refusal after a Door of Light arrival on `map01` is bounded.**
  The arrival tile is the cave mouth's walk-on trigger, and the menu opens on
  the first press after its script closes
  ([settled](re-settled-threads/field.md)).
- **`FUN_801E0418` redraws the title's strips behind the Load window**, from
  the title TIM (PROT 0890) - not memory-card messages from an unknown page
  ([falsified](re-do-not-re-walk.md#menus--ui)).
- **Nearer terrain hides about 1% of the overworld fog, not a fifth**, and
  the ground's bucket key is `FUN_801F89B8`'s, not the mesh leaves'
  ([falsified](re-do-not-re-walk.md#rendering--camera)).
- **The Rim Elm ambush seats its monsters from row 8, not the origin**, and
  allows Run: the formation row's header byte is `0`
  ([settled](re-settled-threads/battle.md)).
- **A dialog picker slides in; it does not resize.** The odd pager states
  count 24 frame-step units from off screen, and input opens only at count 0,
  26 vsyncs after the press ([falsified](re-do-not-re-walk.md#menus--ui)).
- **Motion ops `0x37` / `0x41` are one compass walk**, not two translate ops,
  and map01's cave-mouth walk-out is `B7 F8 00 81`, not `A2 F8 01`
  ([falsified](re-do-not-re-walk.md#field--locomotion)).

- **The text-actor walker has no glyph-stream ops.** `FUN_80031D00` was
  said to recognise `0xCC..=0xCF` inside a glyph stream; its disassembly
  compares no byte against them. It is the per-frame walker of the
  `gp+0x148` drawable list, dispatching on the node's `+0x1C` byte
  (clamped to `0x23`) through the jump table at `0x80010DC0` (`jr v0` at
  `0x800321AC`)
  ([falsified](re-do-not-re-walk.md#menus--ui)).

## Field / locomotion

| Thread | Status | State |
|---|---|---|

Answered questions: [settled field / locomotion](re-settled-threads/field.md), [settled text / dialog](re-settled-threads/text-dialog.md); falsified readings: [field / locomotion](re-do-not-re-walk.md#field--locomotion), [menus / UI](re-do-not-re-walk.md#menus--ui).

## Battle / rendering

| Thread | Status | What would close it |
|---|---|---|
| Why does a full party's summon run step `2` into the flash where retail runs `3`? | open (measured) | On the `battle_kor_spoon_strike` and `battle_tower_swordie_strike` films retail holds step `3` from the inject through `0x33` and drops to `2` on `0x34`'s entry; the engine reads `2` throughout, so its flash-in rise trails retail's and `0x34` scores `.55..58` where a lone caster's reads `.95`. The ring entries of a full party's `0x28` / `0x29` / `0x32` frames would settle it. The cast probe's `cost` column (newer than those films) reads them, but `--retail-only` on `battle_kor_spoon_strike` parks in the command flow before the inject, so a pre-cast full-party state comes first. |
| Why do a few strikes and casts fall under the measured frame step? | open (measured) | `battle_korb2_medusa_strike`, `battle_tower_guardian_windhash_mid_cast`, `battle_deroa_spoon_v2_hyper_heal_mid_cast` and `battle_map03_moldy_worm_strike` read lower once the ring feeds ordinary frames (`FrameCostRing`). On Medusa's strike the camera is exact and the difference is a party member near the lens, mid-fade in one frame and solid in the other, so the replay's gate lands at another point of that fade. `battle_map03_death_trigger_strike` parks in `0x19` under the capture's seats, so its child retries at the default step; the ladder's strikes never park. Moldy Worm's and Devil Pump's strikes: a bearing unit moves the gate. Seating fade and walk off the state's own ring would settle it. |
| What parts Gimard's late arms from retail? | open (measured) | Arm 12's stand, its coin and arm 10's victim show are ported ([cast-module](../subsystems/cast-module.md#spawn-reports)), and the walk's pose and puff law match retail's to the unit. Arm 11 (`.732`) is left to the `rand()` picks of the puffs' pose objects and jitter, and to retail's slow frames (fourteen of its passes run five vsyncs). `--rand-at` on arm 11's key would show how much the picks hold. |
| Why do Caruban's two swings land their hits where retail's do not? | open (measured) | Retail's recording holds a `rand()` word only for the hits whose spark burst shows; timed, those two (Bite's and the last swing's) now land on the engine's matching hits ([cast-sequence oracle](../tooling/cast-sequence-oracle.md#a-monsters-physical-attack)). Flap's two events draw on the replay's own stream and swap retail's verdicts: retail blocks event 1 (cues `0x0C` + `0x249` at `+39`) and plays `0x61` on event 2, the engine the reverse. Next: read the seed word at each Flap event off a `--watch 0x80009010` recording. |
| Why does `battle_geremi_ogre_strike`'s replay reach its gate on the idle? | open (measured) | Retail's capture stands the Ogre on its walk (clip `1`, cursor `120`) with `07` staged, and the walk plays on 27 vsyncs before the swing commits; the replay's walk has already ended onto the idle, so the swing commits at once (`0x1E` `.59`). The walk's phase at arrival is the replay's own: seat the walk's cursor at the gate, or place the walk start so its cycle ends where retail's does. |
| Why do the engine's hit-spark parts retire sooner than retail's on `battle_vell_nighto_49_strike`? | open (measured) | With each frame's draw load on the ring the film reads `.728` against `.729` on retail's own steps, but at most `27` spark parts are live at once in the engine against retail's `41`, so the engine's frames sit nearer the `0x1FF` threshold where the step goes `2` to `3`. A `--retail-parts` film of the strike, each burst's part count by vsync against the engine's, names which hit's burst ends early; then its parts' move-VM lifetimes against the bytecode (`asset move-disasm`). |
| Why does `battle_vell_nighto_49_strike`'s headless camera stand its yaw `51` short of retail's? | open (measured) | Before the ground snap the engine's case-6 glide reads yaw `2798` onto `2801`, against retail's live `2794` onto `2803`; the snap's strike-heading target reads `2752` (Nighto's facing `1911` against retail's `1930`, yaw counter `584` against `588`) and the glide-origin align carries the camera to `2743` (camera `.982`). Next: compare `strike_heading_target`'s inputs with the RAM's case-6 arm on a capture at accumulator `0` (`LEGAIA_RC_CAM_TRACE`). |
| Which pass raised the HUD plaque and readout bar `battle_map01_gomboo_steal_mid_cast` shows three pixels short of rest? | open (measured) | Every glide record reads retired with `elapsed` `15`, so the frame on screen is the last in-flight pass and neither ordering table the RAM holds draws the offset (the "saved on the pass after the landing pass" case of [retail-compare.md](../tooling/retail-compare.md#hud-glides-the-capture-had-landed)). The yaw counter puts the action seed some forty vsyncs back, too long for a seed-raised glide. Next: probe which `FUN_801D8DE8` call re-raises records `0x00` / `0x01` in the capture band. |
| How much of Iota's arms 5..9 is left once the sheets stop crossing? | open (measured) | The cycler rows 488 / 489 match `apply_hsv_cell` entry for entry at a mid-arm-7 state and within one pass's hue step at an arm-6 one (`LEGAIA_DIAG_CAST_VRAM_AT` against `sstate_vram.py`), the scrolled pages agree row for row, and the part sets agree. With the grab keyed a vsync past the bump ([the flip on a slow step](../tooling/cast-sequence-oracle.md#the-flip-on-a-slow-step)) each arm scores highest on its own tick. What is left: the rays' `rand()` deal, the three channel fades arm 4 parks for arm 5's end starting a pass late (the engine runs arm 4's pass on step `2`, retail on `4`), and the cycler's adds standing one vsync of velocity short (retail's part ticks once in its spawn frame). |
| Which frames of Ozma's closing arms lose the creature? | open (measured) | A retail table of arm 23 (`ot-raster.py`) links the additive rays at buckets `1083..2035`, behind the creature's `660..730`, so a ray sorts by its mean where the engine tests its near end per pixel. Drawing every part at its prims' mean depth moved Ozma by nothing and cost Meta `.06` while its flame walls crossed; with [tessellated sheets](../subsystems/renderer.md) keeping their order the reading is worth re-running. A blend pass that orders semi prims against the body's prims would close it. |
| What whites out Ozma's arm 24? | open (measured) | Retail's frame reads luma `178` at a mid-arm pair and the engine's `229`, though the arm's part counts agree record for record. The ladder's fades now sit under the HUD, as the table links them. The rays' `rand()` deal and the additive build-up on the armed wash are what is left to read apart. |
| Where do Ozma's middle arms part from retail once the camera matches? | open (measured) | On the `0xA0` film arms 7..10 hold the creature's clip, cursor, stand, facing and the camera to the unit on both sides, yet the body stands turned differently in the close-ups, converging through arm 10; PROT 0934 stores no pitch, bank or node angle on it, and the anim tick copies `+0x44..+0x4B` into the node every pass (`0x800476A8..0x800476C4`), so it is no module store. Arm 21 (`.44`) reads retail's step falling from `4` to `3` about a hundred vsyncs in with the probe's `cost` near `700`; the engine stays on `4` and builds fewer additive layers a vsync. Next: a `--retail-parts` film of arms 7..10 with the creature's node angle trio, and a GPU cost reading for arm 21. |
| What sets the white pulses' phase in Barra's arm 7? | open (measured) | The arm's cloud records `0x801F8B40..0x801F8BE8` run as retail's do: one a frame off the emitter `0x801F8B18` (`asset move-disasm slot-b`), `33` vsyncs on `+0x18` from `1024` to `-1088`, halting after `WAIT 31`; the earlier `+0x16` / `40`-vsync reading took the probe's old `s32(+0x18)` column for `+0x16`. With the [mid-walk seat](../subsystems/move-vm.md#a-part-seated-during-the-walk) each shows `832` on its seat frame as retail's does. The arm still reads `.40`: its worst pairs are white pulses retail holds at luma `214` where the engine is at `112`, a tick or two apart, so the pulse's clock is what is left. A `--rand-at` run on the arm's key, then the pulse source's own walk, would name it. |
| Why does Aluru's head tip show through arm 11's ray burst? | open (measured) | The dark triangle at the top centre of the engine's arm-11 frames is no part of the cast's scene: hiding every arm-11 record (`LEGAIA_DIAG_CAST_HIDE`) leaves it, and it is the creature's own head tip, which the engine's depth test draws in front of the additive rays `0x801F8CE4` / `0x801F8C6C` where retail's frame holds the burst over it. The same order question as Ozma's rays: retail links the rays by their ordering-table bucket against the body's prims. An `ot-raster.py` table of one arm-11 frame would give both buckets. |
| Why are Slippery's storm clouds dimmer than retail's in arm 6? | open (measured) | The clouds stand where retail's do, to a unit, on the same `+0x78` ramp, but the frame reads darker. Each record's `rand() % 4` pick differs; `--rand-at` on the arm's key would take the deal out. |
| Why does Theeder's arm 11 flash read a step ahead once a fade takes its spawning frame's step, and why does Gizam's arm 9 fall with the `0x32` hold? | open (measured) | Theeder's arm reads `.83` without the step and `.65` with it, every other pair a step out; Gizam's reads `.92` without the hold and `.79` with it, at either an even or an odd tick shift, so it is not the frame grid. Whatever the arm's walk order or `rand()` stream decides is not named yet. On the Theeder film the arm's low pairs are the lag law's: grabs `1079` (luma `36`) and `1081` (`183`) both key to shown vsync `1073` on an overrunning step-`4` frame (harness). |
| What brightens Barra's arm 7 every ten to twelve vsyncs? | open (measured) | On the `party_basic_attack_vs_gobu_gobu` film (`0x8F`) retail's whole frame jumps from luma `~130` to `~230` and decays over three or four frames, irregularly, through arm 7; the port's frames peak near `185` at other times and the arm scores `.50`. It is no `rand()` deal (a watch of `0x80009010` holds only the frame driver's discard in arm 7), no screen fade (`fade_back` `0`), not the backdrop wash (`ctx[+0x27A]` holds `0x80`), and the live part list is the same on a bright frame and a dark one. Next: the backdrop page's CLUT row `488` across a bright frame, held against the port's. |
| Where do Gimard's arm-11 flame puffs spawn from? | open (measured) | The move-VM parts agree part for part, and the puffs (effect-pool children, anim `6`) match in count and frame count, but at every anim cursor the port's sit about `20` units higher and `30` further along `+z` than retail's. The burning-body emitter is not their source (a `--pose-objects` recording logs two emits, both before arm 6). Next: a watch on the effect pool's spawn to name the op that seats them. |
| Which creature seats do the directors still not carry? | open (measured) | Reading each cast's `creature.csv` against retail arm by arm names them. Carried now: Kemaro's arm-8 lift (`0x801F7640..0x801F7674`), Terra's seat on its poll pass, Mushura's fall and chase bank `+0x48`, Palma's arms `0xB` / `0xC` pitch and arm `0x10` pose pin, Kemaro's arm-16 victim freeze. Aluru's creature backing off from `z = -2307` to `-3075` in arms 15..18 is no module store (PROT 0916 writes no creature `+0x34` / `+0x38` there) and the body is hidden. Zenoir's creature opens a half keyframe (`+8` cursor) ahead: an extra seat-pass advance at step `1`, then step `4` where retail's next pass is `3`. A watch on Aluru's `+0x38` and a step trace of Zenoir's seat pass would close the two. |
| Which capture bodies still replay their clips ahead of the arms? | open (disassembly) | A `CasterStageRow` with `arms: None` plays its clip through module phase `0`, so its first arm holds for the clip (Glare `+282` ticks before its row was placed on arm 0). The dispatch tables place the sites: Hyper Crush `0x0B` on arm 2 and `0x0D` on arm 6 (the row lists them by address, the arms run the other way), Hyper Lightning `0x0C` on arm 2 and `0x0D` on arm 3, Bloody Horns `9` / `0x0A` on arms 2 / 3, Chaos Flare `8` on arm 2 (its fifth walk cue `0x326` on that arm's entry is still missing), Melt Spray `7` on arm 2, Power Charge `0x0A` on arm 0. A recording of each cast would confirm the length once the row is placed. |
| What holds Xain's rate byte at `4` through Terio Punch? | open (disassembly + capture) | PROT 0953's punch stores `+0x21D = 1` at `0x801F6CA0`, the only store to the caster's rate in the body, and `battle_tunnelc_xain_terio_punch_mid_cast`'s cursor advances one a vsync through arms 4 to 6 as that rate would. Its `rates.csv` reads `4` at every vsync from the state on. A write watch on the byte over the first vsyncs names the writer. |
| What takes Death Game's move label off the screen? | open (measured) | `battle_jou_puera_v3_death_game_mid_cast`'s recording shows the label through arm 1 and none from the deal's faces on, with nothing but the band's own `ctx[+0x0D]` store at arm `0xFF` in the body. The engine holds it to the band's exit. The frame the label leaves on, read off the recording, then a write watch on the label's word, name the mechanism. |
| Why does an Orb's Heal land on other numbers? | open (measured) | Retail's Killer Bee goes `96 -> 46` at a hit event in `0x3E` and `+200` at `0x3F`'s applier (`FUN_800402F4(0, 0, ..)`, the static heal table), and shows no heal popup. The engine shows the `50` popup, writes neither until `0x50` and folds `+160` from the spell catalog with a popup. The `FUN_801EC3E4` apply mode for a monster hitting a monster seat is the place to start. |
| What does a real console's frame cost read? | open (measured) | The engine runs `FUN_80016B6C`'s ring over a table of PCSX-Redux's costs ([what the ring holds](../subsystems/battle-stage-camera.md#what-the-ring-holds)), where the entry is the CPU's alone. The question is one number on each side of a threshold: a full party's action reads `594` hblanks on PCSX-Redux and about `490` on Mednafen (which adds a GPU wait), and `511` is where the step goes `2` to `3`. A dump of the ring (`0x80084098`) with `gp+0x3A8` / `gp+0x3AC` over serial (Unirom's `nops`) or a cheat cartridge's memory viewer, from a real console in a full party's strike, decides which step a player's fight runs on. No emulator run can settle it. |
| Why do setup frames at step `3` fit `4` yaw units below the walk back? | open (measured) | Over the `pre_*` states at step `2` the table's `inc * step` a pass fits exactly, and a recording's `0x51` to `0x0C` climbs `2` a pass. At step `3` the frame fits `-10` where re-armed passes give `-6` to `-9`, about `1.5` px. The endpoint climbs the counter's drift a pass, so inc alternates `1, 1, 2`, and a recording through `0x51`, `0x5A`, `0x0A` and `0x0C` at step `3` (`battle_doman_killer_wolf_strike`) reads the live yaw `7`, `8`, `9` behind the endpoint in turn, with the counter climbing `4` a pass into the setup states, so the walk back stands right on those rows and the frame's extra unit is unexplained. |
| Which earlier actor did a monster's cast walk from? | open (measured) | `FUN_801DABA4` leaves spent keys and no order, so `DISPLAYED_WALK_PINS` holds a seat, a style and sometimes a yaw read off each retail frame. A word that keeps the last actor or its `ctx[+0xD]`, or the caster's heading before `0x28` turned it, would replace those pins with law. |
| What is left of `minigame_dance_retail_go_out`'s `.985`? | open (measured) | The beams stand on retail's ([dance](../subsystems/minigame-dance.md#the-halls-animated-props)). The drop shadows now draw over the stage floor ([settled](re-settled-threads/rendering-camera.md)) and barely move the score: retail's darkening is the GPU's truncating half-and-half blend, which the hosts' floating-point blend does not make. The pixels off by more than `32` are spread over the dancers' outlines, the stage lamps and the wall panels, no one region holding more than a tenth of them. Nor is it the clip phase: every `+0x68` cursor in the RAM is the engine's own, and shifting the bodies' clips moves the score by under `.002`. A film of the lamp cells' adds would split palette from geometry. |
| Where does Scythe Wind's caster stand when the walk-in arrives? | open (measured) | A film played on from `dohaty_nova_v2_pre_scythe_wind` matches the arms' camera pass for pass (yaw against TR z at the same rate, the clip cursor `8` then `16` a pass), but Nova arrives about `25` units further along in retail, after a slow frame mid-walk, so its arm-0 turn reads heading `2175` where the replay's reads `2161`, and every sweep shot's yaw sits about `9` units round. Seeding the band's step past arm 0 lost on all three Scythe Wind states. Arm 2's turn onto the victim's body pair and arm 5's double drain are ported ([cast-module](../subsystems/cast-module.md#prot-0951-scythe-wind)), so the gap left is the arrival's. A film of the walk's frame costs would say whether the replay can pace it. |
| Which capture-band bodies still run their arms in a pass each? | open (measured) | PROT 0963 has no `capture_camera_director`, so its shots, seat writes and per-arm records are absent and the pager seats the whole image at once. A `--physical --retail-only` film from a `*_mid_cast` state on `0x70` arm 0, beside the disassembly, reads it; the corpus holds no retail state for it yet. PROT 0936 and 0937 are directed arm by arm, and their ray-burst stagers seat per call (`retona_songi_hyper_crush` / `_lightning` arm 2 `.951` / `.939` on `--rand-track`). PROT 0954 is ported whole; its film differs on its roulette's random icons. [PROT 0958](../subsystems/cast-module.md#prot-0958-blazing-slash) is directed arm by arm; its six damage sites still fold once at the band's exit. |
| Why does Zeto's idle in `zeto_big_wave_mid_cast` show one pass older than the display lag says? | open (measured) | The displayed frame is PROT 0946's arm-0 cut pass, and the two passes after it match at `1.000`; a retail film of the cut pass with its camera globals and Zeto's node matrix ([detail](#zetos-big-wave-idle-one-pass-older)). |
| What dims Element Change's ray fan differently from the port's? | open (measured) | `battle_rugi_rogue_element_change_mid_cast` reads `image` `.841` with every other channel `1.000`. The fan is arm 1's emitter `0x801F8FF0` dealing `0x801F9008` rays on `rand()` draws ([cast-module](../subsystems/cast-module.md#prot-0964-element-change-rogue-wind--thunder--flame)), seated on the crystal's display point; the rays' brightness differs between the two frames. A retail film of the arm's packets (colour word and blend of each ray) against the port's would say whether the dimming is the rays' own ramp or the `rand()` deal. |
| What sheds the rest of Element Strike's burn? | open (measured) | Most of the gap was the standing-in golem's after-images: a `--ghost-trace` recording ghosts seat 7 on every frame of arms 6 and 7, each ghost a draw of the burn loop. The walk takes the monster rule by category (`sltiu v0,s3,0x3` at `0x8004938C`), which the port now follows; the history-ring stamp's rule (`sltiu v0,s1,3` at `0x80047F10`) also keys on the seat, and with both Rogue Flame's arm 6 draws match retail. Arm 7 (`k = 5`) reads `1175` against `1590`. The Flame and Wind recordings carry no `guard=` field, so their films seat no Spirit stance and the party takes twice retail's hit; `--guard 0,1,2` matches retail's HP (`.905` to `.926` on Flame). |
| Why does Plasma Strike's victim burn through arm 5? | open (measured) | Lu's charge clips `0x0C` / `0x0D` resolve seven hit events through arm 5 on both sides (`--watch` of the hit index `+0x1F4`). Retail's block roll grants all seven (`--hit-trace` `blocks.csv`); the engine's, on the same inputs (`LEGAIA_DIAG_BLOCKS`), grants four, so three connect, tint the victim with selector `2` and add `107` HP that retail never takes. The difference is the `rand()` words; `--strike-deal` does not reach a cast band's clip hits, and dealing them there would close it. |
| What is left of Koru's Gigaton Press and Ultra Charge films? | open (measured) | Ultra Charge (`nilboa_koru_pre_ultra_charge`) reads `within` `.673`, `.797` on `--retail-steps` and `.918` with `--rand-track` beside it, so its residue is the recording's frame pacing and the `rand()` deal, not a law. Gigaton Press reads `.802`, `.831` on the steps and `.834` with the track: phase 4 stays at `.743`, where retail's light column reaches the floor's red ring a few passes before the engine's, beside the rock shards each pass deals afresh. A film of the column's packets across phase 4 would say whether that is the column's own clock or its draw. |
| What is left of the battle camera against retail? | open (measured) | Ranked by `cast-camera-rank.py`, the residue is pacing, not framing law: the frame step's cadence (retail `3` against the engine's `2` where a lone member's frames carry a monster's ghosts, a heavy arm's overrun), film openers and older recordings that lack the caster's heading (harness), and summon arms whose first pass lands a frame apart at close-up depths. A per-triangle ghost cost lifts lone-member strike films and overshoots on a full party, so the fixed cost stands ([a frame's draw load](../subsystems/battle-stage-camera.md#a-frames-draw-load)). Next: per-arm films of the player summons (`--retail-parts`). |
| What is left of Zenoir's Fire Claw film? | open (measured) | Recorded from `battle_garmel_zenoir_strike` (`--monster-pick`, spell `0x2B`), the film reads `within` `.741`, `.752` on `--retail-steps`, with clock, cues and camera on retail's; `0x29` runs short only where the engine's step reads `2` to retail's `3`. What remains: retail's fight lost Vera before the turn while the engine's seat keeps her, and the burst lands off retail's on another damage roll (`91` to `88`). Next: a recording that keeps Vera, then `--rand-at` and `--retail-parts` on the burst. |
| What is left of Iota's backdrop through arm 5? | open (measured) | With the cycler's triangle on retail's phase, `--rand-track` on the `0x93` film reads arm 5 `.55`, arm 7 `.68`, arm 8 `.79` against `.43`, `.57`, `.61` on the engine's own stream, so most of what is left is the rays' `rand()` deal ([cast-sequence oracle](../tooling/cast-sequence-oracle.md#the-rand-stream)); the opening ring's size at arm entry is the rest. |
| Why do Caruban's Fire Breath stills sit four yaw units off? | open (measured) | `battle_rugi_caruban_2_` / `_3_fire_breath_mid_cast` read `image` `.929` / `.936`, camera `1.000`. The displayed pose is the RAM's table walked back two step-`3` passes: case `0xA` re-arms every pass at `ceil(gap / 6)` with its yaw endpoint climbing `24` a pass on the clip's cursor, which a forward film confirms pass for pass, and the walls fit that pose best. The bodies near the eye fit a pose four yaw units further back (or the focus about `10` units off), `.988` / `.982`; nothing in the law moves it there, so the residue is the near bodies' placement, not the camera. |
| Why does Big Wave's water sheet differ in brightness? | open (measured) | `battle_noaru_slippery_v2_big_wave_mid_cast` reads `image` `.935`; a film of the cast pairs frames whose wave sheet differs in brightness over the arms after the cut, while `zeto_big_wave_mid_cast` and `battle_jou_slippery_v3_big_wave_mid_cast` read `1.000`. The sheet's colour word and depth-cue weight in a retail film of arms 4..6 would say which. The still also draws yellow rays retail lacks: they are camera-locked kind-4 records, not the far-arm sheet, and the eight `0x801F7BF8` rings stand a step behind retail's (`+0x78` `256` lower). |


### A walked-back pose about twenty units of yaw off

`open` `battle_map01_gomboo_steal_mid_cast` peaks on a yaw beside the walk-back's (`displayed_cam_live_steps`): `3610` against `3631` (`.82` to `.91`), and nothing names the gap. The other capture once listed here, `battle_geremi_ogre_strike` (`190` against `168`), was a locked walker nearest on ring slot `3`, whose displayed pass is the walk's last and whose table chased the walk's bearing ([retail-compare](../tooling/retail-compare.md#battle-states)); its walk back now lands on `189`. `battle_jou_cort_181_strike`'s peak about `10` units past its walk back was Cort's body, drawn on the displayed frame's ring pair once seated there ([retail-compare](../tooling/retail-compare.md#battle-states), `.870` to `.957`); on that seat its yaw sweep reads `.963` at `826`.

The frame step is not what holds these strikes low: lowering it (`LEGAIA_BATTLE_PARTY_STEP=0`) leaves Aluru, Cort 181, Poisonous Worm, Gilium Lv2, `battle_melee_hit_spark` and Moldy Worm unchanged (Moldy Worm's residue was the walk back's endpoint, [retail-compare](../tooling/retail-compare.md#battle-states)).

Three strike children peak on their walk back already: `battle_map02_poisonous_worm_strike` (`3328`; focus `+-20` reads no better), `battle_noaru_gilium_lv2_strike` (`652`, on the idle's cursor `0`, which a sweep of `0..40` confirms) and `battle_korb2_medusa_strike`.
Poisonous Worm's residue is a translucent body where retail has none: monster seat `4`, ghosted (`+0x08 = 0x83000000`), which the `3328` camera seats at the frame's centre (root `SX` about `182`, `SZ` about `1939`, far from every GTE limit) and the older of retail's two ordering tables (OT `0x800F4B08`, CLUT row `485`) holds at the right edge, `x = 262..339`.
That table is not the displayed frame's: its ground-grid packets (`POLY_GT4`, CLUT `0x77C0`) fit a camera between yaw `2880` and `3050` (the lattice fit has more than one minimum), the newer table's (`0x800F099C`) the RAM's own `2551` to under a pixel, and the VRAM buffer at `y = 4` shows that framing while the displayed one at `y = 244` shows the `3328` the walk back gives. So it is the frame between the displayed one and the RAM's, and the displayed frame's packets are no longer in RAM; the ghost's position on screen has to be read off the image.
Read off the image, the walk back holds: a focus sweep of `+-120` round `(-298, 331)` at yaw `3328` peaks on it (`.867`, the next best `.863`), and the yaw is the chase law's (`2551` back two passes of `ceil(gap / 12)` a vsync at step `3`, `2882` then `3323`; the older table's grid fits `2883`).
What the three map02 strikes share instead is the sky. On every `map01` / `map02` / `map03` battle state the engine's top rows read about `+7` per channel over retail's (an offset, not a gain: `e = 0.99 r + 7.3` on red), where every other stage reads within `2`, bright ones included (`town01`, `noaru`, `stone`).
Under the block tolerance that is harmless on its own, and it puts the near-camera ghosts over the sky past it: taking the offset off the upper frame lifts `battle_map02_poisonous_worm_strike` `.867` to `.912`, `lip_kid` to `.876`, `death_wings` to `.887`. The offset was the backdrop's depth cue: the dispatcher cues a backdrop at `IR0 = weight | 1` and the `DPCS` floors, so its `0x80` words reach the GPU as `0x7F`, a 5-bit step under the texel ([battle-stage-camera](../subsystems/battle-stage-camera.md#the-grids-near-colour-and-cue-depth)). Cued that way the sky offset is gone and the three states read `.949`, `.890` and `.888`; what is left on Lip Kid and Death Wings is the near-camera ghosts' placement. Grade `disassembly` + `capture`.
The same cue costs 27 non-overworld captures one block each (`battle_jou_vera_v3_heal_mid_cast` `.954` to `.949` the most): every such block already read `10` to `20` darker in the engine before the cue moved it a unit, and the darkness is not one draw. On `jou_vera_v3_heal` it is the floor grid: retail's newer table's `POLY_GT4` packets predict the buffer at `y = 4` to under half a 5-bit step, and the displayed buffer at `y = 244` reads `1.6` steps brighter, so the frame on screen was drawn on a higher ambient base than the RAM's floor `0x20`.
`displayed_ambient_base` keeps a floored base as read, and the backdrop weight (`576`, three step-`3` passes of the latch) bounds the base's start from above only, so the displayed base needs the dim's starting value, which no word keeps. On the `bylon` / `kor` / `taiku` captures the blocks sit on the slot-1 horizon mist band's edges. Grade `capture`.
It is a placement residue, not a near-lens draw rule: the near body sits about `500` world units from the eye, where a small error in the eye's position moves it several times as far as it moves Noa or the worm. A yaw near `3100` with the same focus puts it at the edge but moves the rest of the frame (`image` `.84` to `.53`), so the eye's position, not its yaw alone, is what is off. Grade `capture`.

Recordings confirm the yaw half of the law (`inc = ceil(gap_before / 0xC)`, the endpoint drifting with the counter), which leaves the focus: its endpoint chases walking bodies, and a focus error reads as a pan. The first step is a retail `cam.csv` across a strike's walk, comparing the recorded focus two passes back with what the walk-back gives, using `walker_end_shift`. `battle_map02_poisonous_worm_strike` and `battle_nilboa_aluru_v2_strike` peak on the law's yaw, so their residue is not the camera; Aluru's was the readout bar and actor plaque one pass short of landing on screen ([retail-compare](../tooling/retail-compare.md#hud-glides-the-capture-had-landed), `0.865` to `0.988`). Grade `capture`.

### Xain's frame at Bloody Horns' band entry

`partial` `battle_tunnelc_xain_bloody_horns_mid_cast` (image `.845`, camera `1.000`) shows Xain square to the camera with a translucent body behind him (Vahn, seat 0, ghost bits `0x83000000` in the RAM). The RAM's pass is arm 0's, and retail's table reads that arm's case 6 from yaw `2850`, rising `68` a vsync onto `3658`. The frame on screen is two passes older, inside `0x6F`.

The direction of the turn is settled: the replay's camera framed the incoming seat in the Done band's last pass and turned down through `0` from yaw `1012`, where retail's held the finished actor's shot through the setup states and turned up ([between two actors](../subsystems/battle-stage-camera.md#how-a-frames-camera-is-chosen)). The replay now turns up too, but reaches arm 0 at yaw `3052` where retail stands at `2850`: on the chase law that is about five passes, or an earlier actor's shot other than the replay's (Noa's Spirit at yaw `2228`).

The capture now stands on the gate's own pass: past arm 0 it is placed by the close-up accumulator `ctx[+0x87C]` the arm's clip commit zeroed (`image` `.719` to `.799`), and the headless channel takes retail's live camera inside the walk arm's case-6 follow (`camera` `.878` to `1.000`, [retail-compare](../tooling/retail-compare.md)). A cast film seated on each slot's recorded facing reads `.873`, and with the last arm's double drain `.885`. No yaw over the whole turn exceeded `.76` in the image before those seats, so the rest of the residue is Xain's and Vahn's placement. A film from a state a round earlier that reads the camera through the earlier actor's Done band would name the pose the band set out from ([the earlier-actor thread](#battle--rendering)). Grade `capture`.

Xain's pose is half of what was left. The rings say the walk clip committed inside the display lag: the cursor ring reads `16, 0, 0, 288` and the clip-record ring the walk on its newest two slots, the idle on the third, with the position ring `(502, 192)`, `(546, 197)`, `(590, 202)`. Arm 0 queues the walk with the `+0x1DC` bump (`0x801F72C4..0x801F72D0`), so the frame on screen shows the idle on the ring's pair, where the replay showed its own idle at `176` on the RAM's pair. Posed on the rings, Xain reads `.845` (`.871` on the idle's last keyframe one slot older, so the slot the screen shows is not pinned to one keyframe). Grade `disassembly` + `capture`.

The other half is Noa (seat 1): retail draws her as the ghost, the engine solid. Her `+0x8` is clear in the RAM, which fits the ghost pass's pose one pass before the RAM's (yaw `2850`, her planar distance `1302` against a radius `1258`), and the walked-back pose the image child hands the pass (yaw `2578`) clears her too. Ghosting her needs a pose with yaw about `2986` or more at that focus and depth, the RAM's own, or a depth past `5600`. Forced, it reads `.006` higher. The image scan agrees that the screen's camera is not the walk back's: with Xain posed, the replay's own pose (yaw `3052`, focus x `535`) reads `.868`, and the walk back's (`2714`, `615`) `.742`. A film across the band's entry would give the pose.

### The map display pointer's ink

`partial` On `world_map_display_map01_retail` the pointer draws dark red with shaded
facets; the port draws its colour word as ABE `1` additive over the ground, bright
orange. Grade `capture` for the frame; the record's program is `disassembly`
([world-map](../subsystems/world-map.md#the-map-display---prot-0981)). The frame
says the word's blend does not reach the pixel. Closing it wants a packet read of
the pointer's prims under `FUN_801CE850`.

### A battle target surface's replay

`open` `battle_magic_target_gaza2_retail` (`ctx[+0x06] = 0x5C`) and the all-party
item confirm `0x66` hold a surface reached through a window on a flow byte
`BattleFlowState` does not model (`from_raw` reads both as `Idle`), so the seed
stands them on the round prompt. `BattleDrive::Menu` reaches the single-ally item
target `0x64` through the window's first row. Both states are held on disk under
`captures/` and not in the catalog until the drive can walk a window's row and the
flow byte carries them. The engine's `0x66` screen is grounded on the pad walk's
frame alone: panels up, `All Allies` plate, no score.

### Zenoir's walk over a small window

**Status: open.** Retail's arithmetic lets Zenoir's creature step over a victim's range window (510 units a pass against a window 448 across, `+0x1F = 14`) and hold arm 8 for good ([cast-module.md](../subsystems/cast-module.md#prot-0908-zenoir)). The port hit this in about one Zenoir cast in two hundred and guards it. Not settled: whether a retail fight does it, since the frame step `4` is a measurement of one recording, and a step of `3` (382 units) cannot miss. Evidence that closes it: a PCSX-Redux recording of Zenoir against monster 27, 36 or 63 reading the creature's `+0x34` through arm 8.

### Zeto's Big Wave idle, one pass older

The RAM holds Zeto's idle cursor at `60` (rate `+0x21D` `4`, Big Wave's
halving; all sixteen frame-history entries step `2`), so two passes of display
lag put the shown frame at `52`. The image reads `.962` with Zeto seated there
and `.980` seated at `48`, while a display lag one pass longer wrecks the
pan's framing (`.462`). The same extra pass costs `cort_mystic_shield_mid_cast`
and moves the other Cort capture-band states by under `.002`, so it is not a
corpus-wide lag.

It is not the double buffer's phase: the floor grid of the table under the
current record index (`0x8007BF30 + i * 0x74`, index `gp+0x434`) fits the
RAM's camera, so the state was saved mid-pass and the screen is two passes
back, as assumed (`cort_mystic_shield_mid_cast` reads the same way,
`horn_summon_mid_cast` the other: its newest floor sits under the other
record). Nor is it the module's pacing: a seat-0 Big Wave recorded on both
sides (`LEGAIA_POKE32` sends the cast down Big Wave's branch) holds arms 0 and
4 one pass each and arms 5 to 8 within two vsyncs of retail's game clock.
What is left is the pose the frame drew, and the obvious reading of it does
not hold across states: `battle_gimard_tail_fire_b`, saved mid-pass too, draws
its knockdown exactly two advances behind the RAM's cursor (`72` against
`56`), where Zeto's idle needs three. A per-pass RAM sequence of a seat-0 Big
Wave around arm 0 cannot separate the two either: the arm-0 cut moves the
camera on the same pass the rate halves, so the body packets' motion between
tables is mostly the camera's. A stretch where the camera holds still and a
body's rate or clip changes (a commit that zeroes the cursor) would show
which advance the drawn pose stands on.

That stretch exists and the drawn pose is not the odd one out. A
PCSX-Redux film of a seat-0 Big Wave (`autorun_drawn_pose_film.lua` with
`LEGAIA_SPELL=0x56`, `LEGAIA_POKE32=0x801C8FE4=1`) logs every pass of the
band `0x6E..0x71`: each seat's anim tick advances its cursor and the same
pass's decode draws the advanced value, party and monster alike, through
the caster's rate halving on the `0x70` entry and a monster commit. No pass
skips a decode or draws an older cursor, so the extra advance is not a
draw-order effect; what is left is the instant the state was saved inside
a long pass (a step-`3` pass runs across vsyncs, and a save between the
party's ticks and the monster's splits the seats) or the image metric
preferring a neighbouring sway pose. `zora_glare_petrify_pre` reads the
same way round for one seat only: Vahn fits one advance behind the RAM
(`96` of `108`), Zora two (`168` of `216`), in band `0x6F`.

A save can split the seats, but not that way round. The films put a
vsync, the instant a save state is cut, between the ticks and the decodes,
between two seats' decodes and after the flip, and once inside a seat's
tick: on `rim_elm_gimard_victory`'s art commit, Vahn's tick (which commits
clip `0x11` and halves every seat's rate) runs across a vsync before
Gimard's tick starts. The actor list ticks the nodes in descending
address order, party seat 0 (`0x800835E4`) first and the monster rows
after it, in both filmed fights, so a save inside the list leaves the
party one advance further from the screen than the monsters: Vahn two
back and Zora one, the reverse of the fits, and no split puts a seat three
back as Zeto's `48` needs.

Nor is it the decode. `FUN_8004998C` builds its pose in the battle
context's scratch block (`s2 = *(gp+0xA0C) + 0x6F4`, twelve bytes a part,
the centroid at `+0x120`), and Zeto is the last body the state's pass
decoded: all sixteen parts there are the blend of cursor `60` (frame `3`,
fraction `12`) to the halfword, angles through the short-arc guards, and
the body pair `(-1, 912)` is that pose's centroid turned by the heading.
The port's blend gives the same pose at the same cursor. Which pass the
RAM's body pair belongs to also tells where in the pass a state was cut:
Zeto's is the live cursor's (saved after the draw), and the frame driver's
order - ticks, the view build `FUN_800172C0`, the render passes, then
`FUN_80016B6C`'s `VSync`, `PutDispEnv`, `PutDrawEnv` and `DrawOTag` of the
table just built (`0x800171C0..0x80017288`) - puts the screen two tables
behind the cursor at either instant. The fit itself is sharp: Zeto seated
at `36` / `42` / `46` / `48` / `50` / `52` / `56` reads `.877` / `.948` /
`.963` / `.980` / `.970` / `.962` / `.929`, and moving the camera's depth
(the Big Wave pan's `26` a vsync) does not trade against it - at `52` the
best depth is thirteen units out (`.966`), at `48` the RAM's walk-back
stays best. The near plate filling the close-up is in both ordering tables
the RAM holds, drawn last over the chest. The other framebuffer (the draw
area, `(0, 4)`) holds a wide shot with no HUD that matches neither table,
so it is no witness to the lag.

Nor is it the draw. Both ordering tables, rasterised in retail's chain
order against the state's own VRAM (`scripts/mednafen/ot-raster.py`), match
the port's depth-tested frame at `1.000`: the newest table (record `1`, the
current index) is the port at cursor `60` on the RAM's camera, the other at
cursor `56` with the camera one pass back (TR z `52` nearer). No primitive is
missing, extra, ordered differently or drawn with another attribute, and
the two tables step the cursor `4` and the pan `26` a vsync, as the RAM's
cursor ring and tween say. The residual is the displayed frame alone, and
that frame is PROT 0946's arm-0 pass: arm 4 adds `scalar << 6` (`512`) to the
countdown `0x801F7F20` and arm 5 drains `scalar * step` (`16`) a pass
(`0x801F6F10..0x801F6F5C`), so the RAM's `496` is arm 5's first pass, the
older table arm 4's, and the screen arm 0's cut, whose TR z
`prescale(z / 2) = 2150` is the camera the image child already draws. On
that one pass the port's frame fits only off the walk: cursor `48` with
the camera's yaw `+6` reads `.993`, every other seat lower, against the
`1.000` of the two passes after it. What the cut pass draws differently
(the yaw the action SM's counter leaves before arm 0's snap lands, or a
body matrix built ahead of the snap) needs a film of the cut itself.

Answered questions: [settled battle / arts / level-up](re-settled-threads/battle.md), [settled rendering / camera](re-settled-threads/rendering-camera.md), [settled world map](re-settled-threads/world-map.md); falsified readings: [battle](re-do-not-re-walk.md#battle--arts--level-up), [rendering / camera](re-do-not-re-walk.md#rendering--camera).

## Audio / BGM

Answered questions: [settled audio](re-settled-threads/audio.md); falsified readings: [audio / sound driver](re-do-not-re-walk.md#audio--sound-driver).

### A new track word during a swap's read

*Status:* open

The poller's stage 6 latches the requested index of its own call, so a word that changes
after stage 2's read plays the old track and reports the new one loaded
(`0x800247B4..0x800247C4`, [swap](../subsystems/audio.md#the-swap-takes-a-settle-and-a-read)).
The port starts the newer word. `asset field-op-census --only 35 --context` holds 282
sub-op `1` sites, none directly followed by another, and the 78 pairs within `0x28` bytes of
one record sit on flag branches (a `JmpRel` or a `SysFlag.Test` between them), mostly in entry
prescripts, whose walk the field init finishes before the first frame. A script timeline that
holds two across a wait under `66` vsyncs would still reach it.

### Why does track `2016` score worse under `rimelm_wall_press_left`'s river?

*Status:* open (measured)

The state's `track` channel reads `0.925` because the river emitter is in the recording; the
`emitter` channel (`0.986`) still sits below the `0.998` the same track `2016` reads in
`mei_house_inside` and `menu_status_town`. The river is not the rest: the state's silenced
recording reads `1.0000` at `-44.8` dB against the resumed SPU, and a per-quarter-second,
per-side gain on the river in the full recording fits `1.00 +- 0.05` and lowers the residual by
nothing. Voice contention is not it either: the score keys no voice above 17 here, and retail
leaves 21 free beside the river's 16..18. The excess is the score's own, `7` to `9` dB above
`mei_house_inside`'s from `250` Hz to `2` kHz. The note fit's "a vsync off" flushes on the
slow-attack tone at VAG `0x20840` (ADSR1 `A0FF`) are not it alone: the clean states show
them too (`3` and `2`), where moving those notes gains nothing. Track `2016` carries no bend
and no volume or expression event past its set-up (a disassembly census of its SEQ), and its
key-ons are on the score's grid on hardware's clock: a key-on census of
`town01_field_card_boot` (`autorun_keyon_census.lua`, 900 vsyncs, 317 note-ons) puts every
note-on inside one band one vsync wide of `tick * 10 / 115`, the budget of its 86 BPM. Nor is
it the reverb setting, which the three states share (mode, depth `0x3264`, work area). What
is left is a note's sound, not its time: the next step is the pad tones' stems (VAG
`0x20840`, `0x12220`, `0x14950`) fitted against the river-subtracted recording band by band,
beside the same tones in `mei_house_inside`.

### How long does a summon's seek hold its voice?

*Status:* partial (measured)

The drive's two seeks ahead of the head cue are modelled
([the drive holds a summon's voice](../subsystems/audio.md#the-drive-holds-a-summons-voice)), and so is
their order against the side-band read: the selector seeks once the stage `ctx[+0x276]` drops, and the
module's seek arm polls the stage before its own seek
([the selector seeks when the side-band drops](../subsystems/audio.md#the-selector-seeks-when-the-side-band-drops)).
With those, every summon state's voice lands `6..10` vsyncs after mednafen's ([where it sits](../subsystems/audio.md#where-a-voices-lateness-sits)), inside the cast channel's seek
window. What is not modelled is the band ahead of the module: the stage's drop against arm 1 is `8` vsyncs
in on Theeder, about `7` on Gilium, `4` on Swordie and `0` on Orb, where the port's stand-in for the read
(`30` vsyncs a slot and a first-slot seek) lands it within `5` of each. The band ahead of the module is
not one length either: from the state to the flash, Theeder's `0x33` runs `7` vsyncs longer in the port
(the invoke clip's cursor holds `3` ticks after its commit and its first effect record fires at cursor
`~128`), and Swordie's `0x34` `5` longer to arm 1, while Orb's matches. What closes it: the invoke clip's
first passes after its commit against retail's (the caster's `+0x68` cursor beside `ctx[+0x87C]`), and
`0x34`'s hold on the caster's `+0x1D9`.

On the PCSX-Redux summon films the module's own poll arm waits `4` vsyncs where
the mednafen-fitted model holds it `22` (Nova, Freed) to `30` (Orb) ticks. The
[cast-sequence oracle](../tooling/cast-sequence-oracle.md#cd-seek-waits) absorbs
the wait itself, but what the poll's pass starts (Nova's vortex, Orb's star
burst) then runs that much behind, and the arm after the poll scores `.32`
(Nova) and `.48` (Orb) on frames that otherwise match. Which drive is right is a
real console's seek time; the frames are not evidence either way. The disassembly settles
the order and not the length: the module's seek is `FUN_8003EAE4` (`FUN_8005C160(2, ...)`, the
`CdlSetloc`, then `FUN_8005C034(0x15, ...)`, `CdlSeekL`, `0x8003EB50..0x8003EB6C`) and its poll
`FUN_8003F2B8(1)` is a bare `CdSync(1)` against `CdlComplete` (`jal 0x8005BE8C`, `0x8003F2FC`), with no
constant between them. How long the arm holds is the drive's answer, and mednafen's `CalcSeekTime`
(the law the port's model takes) is that emulator's measured drive.

### Why does Theeder's score not subtract out of its recording?

*Status:* open (measured)

`theeder_summon_mid_cast` reads `0.71` on the [cast channel](../subsystems/audio.md#the-cast-channel)
with the state's [Monaural](../subsystems/audio.md#the-monaural-states) seated and its cast offset free
to a frame step. What is left is the score's held notes, not its keying and not its sound:

- The score keys alike from its top: `seq-differential` on track `2013` reads `3354` of `3354`
  key-ons with `key 0`.
- Band by band (30 Hz to 22 kHz, octaves) the track render's level is the recording's within `1` dB
  over the first four seconds, so no part of the spectrum is missing.
- Split note by note (`audio-note-fit.py`), every note keyed after the state fits at offset `0`, and
  the gain of each falls from `1.0` to `0.55` from `2.1` to `2.9` s - the duck's `161 / 215` through
  the square taper. Two notes held from before the state, on tone `0x8008`, are what the whole-mix
  correlation loses: re-phasing those two alone lifts the first four seconds from `0.61` to `0.88`.
- At the state's own instant the port's voice for one of them (pitch `0x3AB`) stands six ADPCM blocks
  past the state's - one vsync at its pitch, `91` vsyncs after its key-on - while two others on the
  same tone keyed `151` and `69` vsyncs back stand where retail's do.

So retail keyed one note of that score a vsync later than a run from the top does. Standing the held
notes on the state's SPU instead (decoder, pitch counter, envelope, one vsync short of the match for
the port's KON lead) moved the cast row to `0.79` but cost the clean track rows a little, and it was
not kept. Two readings are ruled out. The silent history's phase: seated on the state's index,
the keying differential reads no `key` or `voice` row on `2013` and the cast row does not move. A
parked KON: the KON path never defers ([a key-on is never deferred](../subsystems/audio.md#a-key-on-is-never-deferred)).
A skipped calc would put retail's voice ahead, not behind, and the calc runs once a vsync wherever it
has been watched. What is left is a calc doubled in one vsync, or a voice whose playback stood still
for one (a re-key or a pitch write on the same voice). What closes it: a state from the battle that
leads into this cast, run under `autorun_seq_calc_clock.lua` with a watch on that voice's pitch and
KON bits; the library holds no state of that fight before the cast.

### When does a ring cue sound against the frame that pushed it?

*Status:* open (measured)

Frame-stepped under mednafen, a battle cue's voices key one game frame after the vsync its
ring slot first shows the id: `zeto_big_wave_mid_cast` at step `4` (pushes at `351`, `391`,
`399`, `407`, `419`, `483`, voices keyed at `355`, `395`, `403`, `411`, `423`, `487`) and
`zenoir_summon_mid_cast` at step `3` (`778` to `781`); `son_field_door_hop`'s field part keys
two vsyncs after its store at step `2`. Retail's mode handler runs the ager `FUN_8001698C`, the
frame body `FUN_80016444`, then `FUN_80016B6C`, which drains at its head and paces the vsync
wait after (`0x8001702C..0x80017068`), so a cue keys where the frame's work ends. The port
drains on the tick that opens the frame, which puts the cast channel's cues up to a frame step
early (Big Wave's party reactions, the reading the [cast channel](../subsystems/audio.md#the-cast-channel)
gave as four vsyncs). The recordings put the onset a whole frame step after the push (Big
Wave's first cue `3.9` vsyncs after the engine's, which pushes on retail's vsync), so the lag is
the step, not `step - 1`. Holding the key-on a battle frame step leaves every cast row's median
correlation where it was and lowers the tenth percentile on Swordie (`0.77` to `0.33`), Zenoir
(`0.68` to `0.33`), Theeder and Guilty Cross, raising it only on Big Wave (`0.48` to `0.52`):
the port's battle step is its cost model's, `2` through Big Wave's reactions where retail runs
`4`, and `4` through Zenoir's kill phase where retail alternates `3` and `4`, so a lag on the
port's step moves half the cues the wrong way. It is not ported. What closes it: the battle
frame step itself on those arms (the cost ring's table), after which the lag is the step.
The step byte is not always the frame's length either: on the PCSX-Redux film of Gaza's Neo
Star Slash (`gaza2_card_pre_neo_star_slash`) capture phase 3 holds the step byte `4` and the
cost at its `700` clamp while the frames take `6` vsyncs rising to `15` (`48` frames in
`583` vsyncs, the frame counter `gp+0x480`), so a frame that overruns its step delays its
cues by the overrun as well; the engine runs the same phase in `240` ticks.
Held on the port's own step from the arms' current cost rows, the same run reads Big Wave's
tenth percentile `0.478` to `0.522`, Zenoir's `0.682` to `0.329`, Swordie's `0.774` to
`0.332` and Theeder's `0.342` to `0.308`, the medians unmoved: the table still names a step
retail does not run through the kill phases.

The lag is not a sound-against-picture offset the port owes, and holding the cue alone would
create one. `FUN_80016B6C` drains the ring at its head (`0x80016BF8`), then waits out the frame
(`jal 0x8005FB84` at `0x80017088` / `0x800171C0`), and only after the wait swaps the display and
draws the ordering table the frame built (`0x80017230`, `0x80017254`, `DrawOTag` at `0x80017288`),
into the buffer the *next* frame's swap shows. So retail keys frame `N`'s cue at the end of `N`
and shows `N`'s picture at the end of `N + 1`: the sound leads its picture by one frame step. The
port presents a tick's picture and keys its cues on that tick. A cue held one step would trail its
picture by a step, two steps from retail's order, where the unheld cue is one step from it. The
cast channel's residual is therefore a term of the comparison (its one offset per cast cannot
absorb a step that changes arm by arm), not a cue the player hears late. What would still close
the number: the cast render delaying each ring cue by retail's step on that arm, in the oracle.

## Title / boot / overlays

No open threads. Answered questions: [settled title / boot / overlays](re-settled-threads/title-boot-overlays.md); falsified readings: [title / boot / overlays](re-do-not-re-walk.md#title--boot--overlays), [no overlay function lives below `0x801CE818`](re-do-not-re-walk.md#no-overlay-function-lives-below-0x801ce818).

## Containers / data blobs

No open threads. Answered questions: [settled measurement + corpus](re-settled-threads/measurement-corpus.md), [settled animation](re-settled-threads/animation.md); falsified readings: [containers / placeholder slots](re-do-not-re-walk.md#containers--placeholder-slots).

## Measurement + tooling

One live hunt, and it is about the instruments rather than the game: a capture-graded
claim is only as good as the save state it was measured on.

| Thread | Status | What would close it |
|---|---|---|
| Which save-state library entries still hold a patched executable? | open (narrowed) - audited and tagged; the rest need human play | A state made on a patched disc keeps that build's `SCUS_942.54` in RAM forever. `patch_taint_audit.py` tags each state's `resident_patch` at the hook sites (`states`) and over the whole executable (`scan`), so a state tagged retail is retail outside the bytes the game writes. The S1..S5 anchors, `first_town_interactive` and `teien_field_run` are re-shot retail and every capture-graded claim stands ([taint](../tooling/pcsx-redux-automation.md#patched-disc-taint)). Re-shooting the `rikuroa_*`, `dolk2_market_noa`, `cort_evolved_*`, `minigame_*_pcsx`, `battle_gaza2_*` and mednafen `overworld_battle_bg_angle_*` states on an unpatched image closes it. |

Answered questions: [settled measurement + corpus](re-settled-threads/measurement-corpus.md); falsified readings: [measurement readings](re-do-not-re-walk.md#measurement-readings).
## Adding a thread

A thread belongs here when:

1. There is something *specific* that would close it - a probe to run, a dump to read, a function to port. "Generally understand X better" is not closable; skip.
2. The next step is non-obvious from the code or git log. If `grep` would surface it, no row needed.
3. The detail lives elsewhere (a docs page, a disassembly dump). The row is the pointer, not the analysis.

When the thread closes, move its answer to the matching area of [`re-settled-threads.md`](re-settled-threads.md) with an evidence grade, move any reading it disproved to [`re-do-not-re-walk.md`](re-do-not-re-walk.md) with its reasoning, and delete the row here. Do not leave a "closed here" note behind: this page lists live hunts only.

## Related pages

- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade. Check here before opening a hunt.
- [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - the falsified hypotheses, reasoning intact.
- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) - per-function dumped x documented x ported x ignored axes. `port-catalog.py --missing-ports` is the function-level companion to this page's question-level index.
- [`docs/reference/functions.md`](functions.md) - canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row does.
- [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml) - addresses explicitly *not* worth investigating (statically-linked PsyQ infra). Disjoint from this page.
- [`docs/tooling/worklist-classification.md`](../tooling/worklist-classification.md) - classifies each `--missing-ports` row by whether it is a portable function entry at all. `INTERIOR`, `SHARED_TAIL`, `DUPLICATE` and `VA_ALIASED` rows are not work.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base, and the one dump window whose targets are therefore untrustworthy.
- [`docs/subsystems/vm-inventory.md`](../subsystems/vm-inventory.md) - every VM-shaped subsystem with its op space, port status and whether anything live calls the port.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the C-rendering artifacts that have each put a false claim into these docs.
