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

Readings a re-audit against the disassembly or a capture recently overturned.
They are listed here as well as in the registers, because a claim that was wrong
once is the cheapest place to look for a claim that is still wrong. The list
stays short: an older correction leaves it and keeps its row in the
[settled](re-settled-threads.md) or [falsified](re-do-not-re-walk.md) register.

- **The member who finished the fight strikes the victory pose, not the leader.**
  The action SM's victory arm writes the acting slot `ctx[+0x13]` last:
  `FUN_801E295C`'s `s5` is `ctx + 0x11` (`0x801E2994`), so its `sb v1,0x2(s5)`
  stores (`0x801E66EC`, `0x801E6740`, `0x801E6760`) are `ctx[+0x13]` stores. A
  search for `0x13(` offsets missed them and read the leader as the poser
  ([settled](re-settled-threads/battle.md#battle-end---the-results-sequencers-timeline-and-who-strikes-the-pose)).
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

## Field / locomotion

No open threads. Answered questions: [settled field / locomotion](re-settled-threads/field.md), [settled text / dialog](re-settled-threads/text-dialog.md); falsified readings: [field / locomotion](re-do-not-re-walk.md#field--locomotion), [menus / UI](re-do-not-re-walk.md#menus--ui).

## Battle / rendering

| Thread | Status | What would close it |
|---|---|---|
| Why do a few strikes and casts fall under the measured frame step? | open (measured) | Medusa's strike, Windhash, Spoon's Hyper Heal and Moldy Worm's strike read lower once the ring feeds ordinary frames: on Medusa a party member mid-fade near the lens moves the replay's gate, on Moldy Worm and Devil Pump a bearing unit. Death Trigger parks in `0x19` under the capture's seats. On the step's edge (Neo Grude, King Frog, Mush Mush) the residue is a per-fight offset over the table, `-85` to `+85` across the full-party strikes and steady within one, which neither parts nor the formation's triangles read ([a heavy fight's hit frames](../subsystems/battle-stage-camera.md#a-heavy-fights-hit-frames)). Seating fade and walk off the state's ring, and a linked-polygon term, would settle them. |
| What holds the party arts' films under retail's? | open (measured) | Three frame-cost terms read off the Super and Hyper Art recordings: the base under a close shot, an arts shout's own pass, and a voice line's span counted in passes ([below](#the-party-arts-frame-cost)). |
| What does a full party cost a summon arm? | open (narrowed) | Six full-party recordings set against the lone caster's read the same on every clamped arm and `14`..`25` hblanks dearer on some arms under the clamp, `29`..`135` on four (Barra's arms 3 and 4 in both fights, Gimard's arm 6, Iota's arm 3), nothing on others (Zenoir's edge arm 7) ([the arm step table](../subsystems/battle-stage-camera.md#what-the-arm-step-table-is)). The engine hides all three members through the band, so the members it draws do not tell the arms apart; the term is not priced. An arm a longer fight runs past the lone recording's length also runs its tail on the closing frame's row (Gimard's arm 12, [the arm step table](../subsystems/battle-stage-camera.md#what-the-arm-step-table-is)). |
| What does a real console's frame cost read? | partial (measured) | The entry is `max(cpu, dma + draw of the previous pass's table)` from one vsync, and the GPU half is modelled on the v2 GPU's measured costs with no free constant, which reads mednafen's waits to `8` hblanks at the median ([what a real console's frame costs](../subsystems/battle-stage-camera.md#what-a-real-consoles-frame-costs)). The CD half is PCSX-Redux's response time ([the CD stop watch](../subsystems/battle-stage-camera.md#the-cd-stop-watch)). Left: the instruction cost, two cycles an instruction on PCSX-Redux and about `.8` of that on mednafen. A real console's ring (`0x80084098`) with `gp+0x3A8` / `gp+0x3AC` in a full party's strike and a cast's voice decides both. |
| What is left of `minigame_dance_retail_go_out`'s `.988`? | open (narrowed to the beams' edges) | The beams, shadows, clip phase and camera are retail's ([dance](../subsystems/minigame-dance.md#the-halls-animated-props), [film](../tooling/minigame-film.md)). The top band is no brightness rule: rasterised, the score panels are PCSX-Redux's rule and the engine's match the capture to the pixel; what is off sits on the lamps' beam edges at a mean luma offset of `0.6`, and on the dancers' outlines ([the top band](../tooling/retail-compare.md#the-dance-halls-top-band)). A film of the lamp cells would say whether the edges are the sweep's phase or its geometry. |
| What is left of Koru's Gigaton Press film? | open (narrowed to the frame span) | `koru_pre_gigaton_press` reads `.853`, `.914` on `--rand-track`. The column reaching the red ring early is the stream: seated only on the word retail's phase 3 ended on (`--rand-at 0x70:3`) phase 4 reads `.889` and draws retail's burst in order. The engine's stream parts inside the cast, one frame-driver draw short in `0x6E`, which is the [frame span](../subsystems/battle-stage-camera.md#a-frames-span)'s ([the rand stream](../tooling/cast-sequence-oracle.md#the-rand-stream)). The gold ellipse in the `koru_*_mid_cast` stills is the walk's [floor rings](../tooling/retail-compare.md#a-turning-effect-parts-banks-are-a-clock). |
| What is left of the battle camera against retail? | open (measured) | Ranked by `cast-camera-rank.py` over the strike, arts, Seru and enemy film sets, the residue is pacing, not framing law: where the worst films part, the engine holds retail's pose of the pass before or after to the unit (`kor_shade_pre_fatal_decision`'s `0x6E` chase, the arts openers' `0x1E` walk). What sets the pass is the frame step (retail `4` against the engine's `3` on an art's first `0x1F` pass, a heavy arm's overrun, [a heavy fight's hit frames](../subsystems/battle-stage-camera.md#a-heavy-fights-hit-frames)), film openers (harness), and summon arms whose first pass lands a frame apart. Next: per-arm films of the player summons (`--retail-parts`). |
| What is left of Zenoir's Fire Claw film? | open (narrowed to the hit pass's spawns) | The roll and the cave mist are closed; on `--retail-steps` `0x2B` is too (`.973`), once the step track keeps the film's one-row opening frame ([the step track](../tooling/cast-sequence-oracle.md#the-camera-against-the-step)). Plain, `33` frames run step `3` where retail's run `2` (frame cost). Left: `0x2C` / `0x2D` at `.80`. On the hit pass retail's effect pool takes the landing's twelve children and then two scattered ones, the port's one scattered child and then the twelve (`fxpool.csv` beside `--retail-parts`), so later deals and mirror bits part. A breakpoint on `FUN_801DFDF0` / `FUN_801DFDF8` on that pass would name the callers and their order. |
| Why does Kemaro's TR z stand `16` past retail's through arms 1 to 5? | open (mechanism found, the rule not yet general) | Arm 0 (PROT 0918) cuts the camera round over one frame; arm 1 adds `2 * scalar * step` to TR z from the next pass (`0x801F6F04`). Retail's TR z holds the cut's `3276` a pass more: the walker `FUN_8002149C` lands an angle lane only once its arc goes negative (`bgez` at `0x8002151C`), a step-`1` cut spends its arc exactly, so the task lives a pass more and stores every lane back, spending that drift. Kept alive so on every exact-arc cut, the `0x90` camera matches retail's row for row, but the `0x8F` film's arm-5 cut then spends a TR z drift retail keeps (`.933` to `.924`). `FUN_80021248`'s half-turn arc, or that drift's place against the walker, would say which. |
| What holds Seru arms low on every film seat? | open (narrowed) | The plain seat reads what `--retail-steps` does on most films ([pass count](../tooling/cast-sequence-oracle.md#the-bands-pass-count)). Still low on both: Barra's arm `0xE` (`.75`; not its frames' span, arm 12 runs the same tick gap at `.96`), Nova's arm 16 (`.83`; the victim's spin matches a watch of its `+0x46` store), Ozma's arm 21 (`.77`; sprays, camera and draws match), Viguro's arm 12 (`.75` on the rand track; its ring parts match retail's, the disc draws darker), Mushura's arm 12 chase (`.80`) and Gizam's arm 13 (`.64`, the bubbles' deal). Open draw timings: [where a film leaves retail's words](../tooling/cast-sequence-oracle.md#where-a-plain-film-leaves-retails-words). |
| Why do Caruban's Fire Breath stills sit four yaw units off? | open (narrowed) | The camera law is settled forward by a film; what is left is the near bodies' placement or the displayed pass ([below](#caruban-fire-breath-stills-four-yaw-units-off)). A `LEGAIA_RAM_AT` film over `0x2B` would pin the pass. |
| What holds the Delilas and Cort casts' arms low on every film seat? | open (narrowed) | Seven classes are closed, the latest [Blazing Slash's](../subsystems/cast-module.md#prot-0958-blazing-slash) flames. On `--retail-steps --rand-track` the films read Megaton Press `.959`, Plasma Strike `.938`, Blazing Slash `.972`, Mystic Shield `.975`, Guilty Cross `.940`, Mystic Circle `.906`; what holds plain is the deal. Blazing Slash's arms `0x0D` / `0x0E` read `.967` / `.948`, so the puffs no longer hold them; arm `0x15` does (`.784`, Gi Delilas' hop stands turned off retail's, luma matched), and arm `0x16` (`.797`, the paced flame hits leave the victim `2` HP off by the third numeral). |


### Caruban Fire Breath stills four yaw units off

`open (narrowed)` `battle_rugi_caruban_2_` / `_3_fire_breath_mid_cast` read `image` `.940` / `.951`, camera `1.000`; the walls match, and the deficit sits on Vahn's and Gala's near bodies. The displayed pose is the RAM's table walked back two step-`3` passes (case `0xA` re-arms at `ceil(gap / 6)`, the yaw endpoint climbing `24` a pass), and the walls fit it best. Near bodies fit four yaw units further back (`.988` / `.982`).

The `_2_` state is saved mid draw tick: seats `2` / `3` hold view translations `+0x2C` on the camera one step-`3` pass back on the table's increments (pitch `43`, yaw `3508`, to a unit), which pins the walk back's first pass.

The law the walk back inverts holds forward to the unit. A film of the cast from the `_2_` state, with Caruban's clip `9` seated on the state's own cursor `40` and the party on the state's idle cursors, walks case `0xA` onto retail's yaw pass for pass (`3499`, `3514`, `3535`, `3556`, `3580`), with pitch, TR and focus, and hands on to the group shot `0xB` at the first hit to the unit ([the cast driver's shots](../subsystems/battle-stage-camera.md#the-cast-drivers-clip-clock)). So the four units are not the yaw law: they belong to the near bodies' own placement on the displayed frame (Vahn's and Gala's idle pose or ground pair), or to the pass the screen shows. Grade `capture`.

At step `3` every gap a pass leaves has a root on two increments, `3` units apart, and the RAM keeps neither. Taking their midpoint in every walk back lifts these two stills to `.966` / `.977` but lowers 58 others (`zora_glare_petrify_pre` by `.038`), so the walk back keeps its single root until a film pins which root each pass stood on.

### A walked-back pose about twenty units of yaw off

`open (narrowed)` The yaw half is settled ([settled](re-settled-threads/battle.md#a-walked-back-pose-about-twenty-units-of-yaw-off)): `battle_map01_gomboo_steal_mid_cast`'s `3610` is the walk back's own, and two films confirm the re-arm law the walk back inverts, yaw and focus alike. What is left is the near-camera ghosts' placement below.

On `battle_ropeway_party_gala_flying_knee_late` the ghost's placement is the displayed camera, not the body or a near-lens draw rule ([battle-actor-rendering](../subsystems/battle-actor-rendering.md#the-near-camera-ghost-pass-fun_8004dc68)): it reads `.985` on the walk back with the arm before's heading a retail film shows (yaw `2152` against `2143`). What no RAM image holds is that earlier heading: a hit's re-face overwrites the target's, and the rings keep no older one. Grade `capture`. `battle_rikuroa_party_vahn_chain_open`, once listed beside it, was the body: the make-way step was still moving its ghosted monster inside the display lag ([retail-compare](../tooling/retail-compare.md#bystanders-the-make-way-step-is-still-moving)).

The other capture once listed here, `battle_geremi_ogre_strike` (`190` against `168`), was a locked walker nearest on ring slot `3`, whose displayed pass is the walk's last and whose table chased the walk's bearing ([retail-compare](../tooling/retail-compare.md#battle-states)); its walk back now lands on `189`. `battle_jou_cort_181_strike`'s peak about `10` units past its walk back was Cort's body, drawn on the displayed frame's ring pair once seated there ([retail-compare](../tooling/retail-compare.md#battle-states), `.870` to `.957`); on that seat its yaw sweep reads `.963` at `826`.

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

Recordings confirm both halves of the law (`inc = ceil(gap_before / 0xC)`, re-armed every pass, the yaw endpoint drifting with the counter and the focus endpoint with the bodies). A walk back that knows each earlier pass's endpoint lands on the displayed focus; one that holds the RAM's endpoint does not, by up to `36` units on a party's return walk `0x20`.
The same film shows the return's held passes arming case 8 on the knocked-back target: each pass's focus endpoint is the target's body pair as that pass read it, so the endpoint moves the whole of the target's slide.
`walker_end_shift` reads the return state too, taking case 8's whole target step there and case 7's half step as on the strike loop ([retail-compare](../tooling/retail-compare.md#battle-states)); of the corpus's four `0x20` captures `battle_melee_hit_spark` reads `image` `.919` to `.954` and `rim_elm_gimard_seru_capture_before` `.994` to `.984`, the other two do not move. Grade `capture`.
`battle_map02_poisonous_worm_strike` and `battle_nilboa_aluru_v2_strike` peak on the law's yaw, so their residue is not the camera; Aluru's was the readout bar and actor plaque one pass short of landing on screen ([retail-compare](../tooling/retail-compare.md#hud-glides-the-capture-had-landed), `0.865` to `0.988`). Grade `capture`.

### The party arts' frame cost

What is left of the party arts' films (`scripts/ci/film-sets/arts.txt`) is the frame step, in three terms. Each is read off a PCSX-Redux recording; none is ported.

**The base under a close shot.** `battle_jou_party_gala_super_ironhead_super_open` stands with one Theeder alive and costs `442` hblanks a bare `0x1E` frame where the table's full-party row is `622`. Its two-ghost frames cost `615` to `647` and run step `3`; the port's read over `0x2D0` and run `4`. Sonic Javelin's return frames (`665` to `714` under both ghosts) and Dragon Fangs' first slow motion (`673` to `707`) part the same way. The table knows the party's size and the action state, not which bodies the shot holds ([a party member's ghosts](../subsystems/battle-stage-camera.md#a-frames-cost-is-its-own-node-ticks)).

**An arts shout's own pass.** The commit that issues the shout costs over `0x2D0` itself, and the next pass is already under the clip's polls. Tri-Somersault's `--cd-trace` recording issues `SeekL` at vsyncs `44` and `205`, inside frames `1509` and `1539`, both over the clamp, and `ReadS` three vsyncs on; Super Ironhead's commit frame runs five vsyncs on step `3`. The port charges the heal-measured ramp (nothing on the cue's pass, the onset on the next), one pass behind. Charged on the commit pass, Tri-Somersault reads `.843` for `.798` and its cue `0x019` pairs, but Super Ironhead's cues fall to `14` of `18` through the base term above, so the change waits on it.

**The base is the polygons a shot links.** On the arts and strike films retail's unloaded frames read `35 + .181` a linked polygon (rms `39` hblanks, the step right on `.93` of frames held out by film) against the table's rms `84` and `.76`, and the enemy casts' Done-band settles run from `300` to `688` on a full party along the same line. Body triangles, their on-screen share and the ground grid's drawn cells, the counts the world holds, do not predict the linked count held out by film ([the step agreement table](../tooling/cast-sequence-oracle.md#the-step-agreement-table)).

The polygon pricing exists as an opt-in fed by the native window; a default needs the linked count computed where every host and a headless run agree on it, which is a draw-list builder outside the renderer and not yet written.

**A voice line's span in passes.** A frame under the polls runs `6` to `9` vsyncs on the recordings and `5` on the port's default law, so Tornado Flame's `357`-vsync line covers about `50` of retail's passes and `70` of the port's: its return state `0x20` runs on under the polls and reads `.795`, and the *HYPER ARTS!!* banner, timed on vsyncs, outlasts retail's against the action. The [span law](../subsystems/battle-stage-camera.md#a-frames-span) is the fix and is opt-in.

Counting the span in passes off the table's work (each loaded pass draining the line by the vsyncs retail's frame would run) lifts the enemy sample's step agreement from `.817` to `.860` and lowers its films (`.9417` to `.9388`): the table's work is too far from a frame's own, and Aluru Cannon's `0x2B`, loaded through all of retail's `317` vsyncs, runs out of line early. What holds is the narrow case: the same count read only at the Done band ([a spent voice line loads no Done-band frame](../subsystems/battle-stage-camera.md#a-voice-line-loads-no-done-band-frame)).

Tri-Somersault's cue `0x019` sits `7` ticks early against a `6`-tick window through the second term: the third cue of the Somersault clip stands between cursors `186` and `192`, retail reaches it one pass late because the frame after the first lone ghost runs step `3` (cursor `+18`, not `+24`), and the port's frame there is still under the voice line's polls. The replay is byte-identical under full CPU load (`meta.csv` and `sfx.csv` of two runs, one with every core busy), so the offset is this model term and no host timing.

### Xain's frame at Bloody Horns' band entry

`partial` `battle_tunnelc_xain_bloody_horns_mid_cast` (image `.855`, camera `1.000`) shows Xain square to the camera with a translucent body behind him (Vahn, seat 0, ghost bits `0x83000000` in the RAM). The RAM's pass is arm 0's, and retail's table reads that arm's case 6 from yaw `2850`, rising `68` a vsync onto `3658`. The frame on screen is two passes older, inside `0x6F`.

The direction of the turn is settled: the replay's camera framed the incoming seat in the Done band's last pass and turned down through `0` from yaw `1012`, where retail's held the finished actor's shot through the setup states and turned up ([between two actors](../subsystems/battle-stage-camera.md#how-a-frames-camera-is-chosen)). The replay now turns up too, but reaches arm 0 at yaw `3052` where retail stands at `2850`: on the chase law that is about five passes, or an earlier actor's shot other than the replay's (Noa's Spirit at yaw `2228`).

The capture now stands on the gate's own pass: past arm 0 it is placed by the close-up accumulator `ctx[+0x87C]` the arm's clip commit zeroed (`image` `.719` to `.799`), and the headless channel takes retail's live camera inside the walk arm's case-6 follow (`camera` `.878` to `1.000`, [retail-compare](../tooling/retail-compare.md)). A cast film seated on each slot's recorded facing reads `.873`, and with the last arm's double drain `.885`. No yaw over the whole turn exceeded `.76` in the image before those seats, so the rest of the residue is Xain's and Vahn's placement.

A film from a state a round earlier that reads the camera through the earlier actor's Done band would name the pose the band set out from; the earlier actor itself is [settled](re-settled-threads/battle.md#threads). Grade `capture`.

Xain's pose is half of what was left. The rings say the walk clip committed inside the display lag: the cursor ring reads `16, 0, 0, 288` and the clip-record ring the walk on its newest two slots, the idle on the third, with the position ring `(502, 192)`, `(546, 197)`, `(590, 202)`. Arm 0 queues the walk with the `+0x1DC` bump (`0x801F72C4..0x801F72D0`), so the frame on screen shows the idle on the ring's pair, where the replay showed its own idle at `176` on the RAM's pair. Posed on the rings, Xain reads `.845` (`.871` on the idle's last keyframe one slot older, so the slot the screen shows is not pinned to one keyframe). Grade `disassembly` + `capture`.

The other half is Noa (seat 1): retail draws her as the ghost, the engine solid. Her `+0x8` is clear in the RAM, which fits the ghost pass's pose one pass before the RAM's (yaw `2850`, her planar distance `1302` against a radius `1258`), and the walked-back pose the image child hands the pass (yaw `2578`) clears her too. Ghosting her needs a pose with yaw about `2986` or more at that focus and depth, the RAM's own, or a depth past `5600`. Forced, it reads `.006` higher. The image scan agrees that the screen's camera is not the walk back's: with Xain posed, the replay's own pose (yaw `3052`, focus x `535`) reads `.868`, and the walk back's (`2714`, `615`) `.742`. A film across the band's entry would give the pose.

The cast's own band entry is not the residue. A film of Bloody Horns across its entry, from `battle_tunnelc_xain_strike` on a recording that carries each slot's facing, reads `0x6F` `.996`, arm 0 `.998` and arm 1 `.920` (`1.000` with `--rand-track`), `.981` over the whole cast; its older recording without the facings skipped the walk and read `.865`.
Terio Punch's entry from `battle_tunnelc_xain_bull_charge_mid_cast` tracks retail's camera to the unit through `0x6E`, `0x6F` and arm 0 once the engine child targets the group code `8` the cast retargets to (the bare default `0x0C00` depth, [`FUN_801F0348`](../subsystems/battle-action-helpers.md#0x801f0348---target-size-camera-framing)); on the recorded seat `3` it framed Xain's own `0xD00` and ghosted Vahn. What the still lacks is therefore the replay's history ahead of the band, the earlier actor's Done band and the pose Noa's ghost test read there, and no state of this fight sits earlier.
Grade `capture`.

The film from the state (`.947`) reads `.986` on retail's own frame steps, so the rest of it is the [frame step](#battle--rendering). Retail opens the walk arm at step `2` (the state's ring holds `371..495`), the engine at `3` (its walk frames cost `600` and more), and Xain's walk stops one step further on, `(-420, 111)` against `(-401, 119)`, which moves every close-up of arms 3 to 5.
And the line `0x158` arm 2 cues (`0x801F751C`, `317` vsyncs) loads the frame on vsyncs: retail's arms 3 to 5 run seven-vsync frames under it and reach arm 6 after the clip has ended (cost `563` from vsync `384`, step `3`), where the engine's table arms run their four and arm 6 spends about `100` ticks at step `4` under the clip's load. Grade `capture`.


### Zenoir's walk over a small window

**Status: open (narrowed).** Retail's arithmetic lets Zenoir's creature step over a victim's range window (510 units a pass against a window 448 across, `+0x1F = 14`) and hold arm 8 for good ([cast-module.md](../subsystems/cast-module.md#prot-0908-zenoir)). The port hit this in about one Zenoir cast in two hundred and guards it.

The pace is retail's: every `0x86` recording on disk, from three fights (the Gobu Gobu state, the tower and Kor), runs arm 8 at step `4` on frames clamped at `0x2BC` (`700`), after three step-`3` passes, so the creature's stride is the 510 units the window can miss. Whether a retail fight misses depends only on where a victim with `+0x1F = 14` (monsters 27, 36 and 63) stands on that stride's lattice. Grade `capture`.

Evidence that closes it: a PCSX-Redux recording of Zenoir against monster 27, 36 or 63 reading the creature's `+0x34` through arm 8. The probe cannot inject a party cast from a strike state of those fights (`battle_jou_theeder_v3_strike` and kin): its seat test reads the acting byte at `0x0A`, which still names the previous actor there, so the recording needs a state saved at a party member's command prompt in one of them.

Answered questions: [settled battle / arts / level-up](re-settled-threads/battle.md), [settled rendering / camera](re-settled-threads/rendering-camera.md), [settled world map](re-settled-threads/world-map.md); falsified readings: [battle](re-do-not-re-walk.md#battle--arts--level-up), [rendering / camera](re-do-not-re-walk.md#rendering--camera).

## Audio / BGM

Answered questions: [settled audio](re-settled-threads/audio.md); falsified readings: [audio / sound driver](re-do-not-re-walk.md#audio--sound-driver).

### How long does a summon's seek hold its voice?

*Status:* partial (measured; the seek's length waits on a real console, the band ahead of the module on the cast's own pace)

The drive's two seeks ahead of the head cue are modelled
([the drive holds a summon's voice](../subsystems/audio.md#the-drive-holds-a-summons-voice)), and so is
their order against the side-band read: the selector seeks once the stage `ctx[+0x276]` drops, and the
module's seek arm polls the stage before its own seek
([the selector seeks when the side-band drops](../subsystems/audio.md#the-selector-seeks-when-the-side-band-drops)).
With those, the cast channel fits Theeder's, Nighto's and Gilium's voices `6.5` to `7.8` vsyncs after mednafen's and Orb's and Swordie's within `3.2` of it ([the seek window](../subsystems/audio.md#the-cast-channels-seek-window), [where it sits](../subsystems/audio.md#where-a-voices-lateness-sits)), inside the cast channel's seek
window. What is not modelled is the band ahead of the module: the stage's drop against arm 1 is `8` vsyncs
in on Theeder, about `7` on Gilium, `4` on Swordie and `0` on Orb, where the port's stand-in for the read
(`30` vsyncs a slot and a first-slot seek) lands it within `5` of each. The band ahead of the module is
not one length either: from the state to the flash, Theeder's `0x33` runs `7` vsyncs longer in the port
(the invoke clip's cursor holds `3` ticks after its commit and its first effect record fires at cursor
`~128`), and Swordie's `0x34` `5` longer to arm 1, while Orb's matches. `0x34`'s hold on the caster's
`+0x1D9` is the port's now: the invoke clip's natural end commits idle in the pass that crosses it, and
`0x34` leaves on the next (`0x80047A48..0x80047B58`, `0x801E4AD4..0x801E4ADC`), which is `60` vsyncs on the
`0x86` film as on retail; the first-slot seek is refitted to `7` on that band. What closes the rest: the
invoke clip's first passes after its commit against retail's (the caster's `+0x68` cursor beside
`ctx[+0x87C]`).

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

The two emulators do not disagree as two drive models. PCSX-Redux's `CdlSeekL` / `CdlSeekP`
(`src/core/cdrom.cc`) schedule the seek's completion at `0x800` CPU cycles when the previous seek has
finished and `cdReadTime * 4` (four sectors at single speed, `53` ms) otherwise, whatever the distance,
beside a comment list of per-game timing fixes; it models no seek at all. Mednafen's seek time grows
with the distance (`CalcSeekTime`). On Theeder the two read: PCSX-Redux (`0x82` cast from
`party_basic_attack_vs_gobu_gobu`) arm 1 at `433`, arm 2 one vsync later, the voice at `553`; mednafen
(`theeder_summon_mid_cast`, `drive-timeline.py`) arm 1 at `+86`, the side-band stage dropping `8`
vsyncs into it, arm 2 at `+118`, the voice at `+229`: arm 1's hold is `1` vsync against `32`, and arm 2,
which waits on the band word, `119` against `110`. The port keeps mednafen's law, the one model of a
drive; PCSX-Redux's number is not evidence about a console. What closes it is still a real console's
seek time, and the band ahead of the module above.

### Why does Theeder's score not subtract out of its recording?

*Status:* open (narrowed: a lost calc is the measured mechanism)

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
Watched over this cast, the calc is not once a vsync: mednafen loses two of `1079`, the vsyncs
whose vblank lands inside the drainer's cue key-on, which holds the calc's lock
([settled](re-settled-threads/audio.md#does-the-scores-calc-run-once-a-vsync-across-a-summon)). A
lost calc delays every later event and the flush of the notes staged the vsync before it, so a
loss between that note's key-on and its flush keys it a vsync late, as measured. What does not
fit yet is the note keyed `151` vsyncs back standing on time, which a loss after it would put a
vsync early against the render's alignment on the state's wait. The duck's re-volume is ruled
out (`seq-differential --duck` keys `2013` alike under any ramp), and so is cue contention for
the voices (the seated `--cues` run keys `2013` alike). A PCSX-Redux Theeder cast (`0x82` from
`party_basic_attack_vs_gobu_gobu`, `cast-sequence.py --calc-clock`) enters `FUN_80062F98` once a
vsync across `1451` vsyncs, which the lock does not contradict: a locked call is entered and returns
at its first branch. What closes it: a state from the battle that leads into this cast, run under
the same clock with the flush index and that voice's pitch and KON bits watched; the library holds
no state of that fight before the cast.

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
