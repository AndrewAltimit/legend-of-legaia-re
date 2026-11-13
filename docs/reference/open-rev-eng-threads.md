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
| Why do a few strikes and casts fall under the measured frame step? | open (measured) | Medusa's strike, Windhash, Spoon's Hyper Heal and Moldy Worm's strike read lower once the ring feeds ordinary frames: on Medusa a party member mid-fade near the lens moves the replay's gate, on Moldy Worm and Devil Pump a bearing unit. Death Trigger parks in `0x19` under the capture's seats. On the step's edge (Neo Grude, King Frog, Mush Mush) the residue is a per-fight offset over the table, `-85` to `+85` across the full-party strikes and steady within one, which neither parts nor the formation's triangles read ([a heavy fight's hit frames](../subsystems/battle-stage-camera.md#a-heavy-fights-hit-frames)). Seating fade and walk off the state's ring, and a linked-polygon term, would settle them. |
| Which vsync-timed gate parts a voiced cast from retail once its frames run their full span? | open (measured) | Retail's `VSync` law and a polls' share of `3.4` over a loaded frame's unloaded cost port as an opt-in ([a frame's span](../subsystems/battle-stage-camera.md#a-frames-span)): the films' wall clock comes to retail's, the arts set reads `.8703` to `.8754` (cues `.9976` to `.9886`), the enemy casts `.9087` to `.9062` (cues `.9835` to `.9656`). Medusa's Glare loses its shout fallback `0x290`, gated on the drive word's span, so retail's word outlives the span; Super Ironhead's art frames run `6` to `8` where the share gives `9`. A recording of Glare with `0x8007BC20` every vsync would place the first. |
| What parts Gimard's late arms from retail? | open (measured) | Arm 12's stand, its coin and arm 10's victim show are ported ([cast-module](../subsystems/cast-module.md#spawn-reports)), and arm 12 reads `.946` on the Gobu Gobu film. Arm 11's camera matches retail's pass for pass (yaw within `8` units) and its puffs spawn where retail's do ([settled](re-settled-threads/battle.md#threads)), yet the arm reads `.775` plain and on `--retail-steps`: the trail's bright mass sits elsewhere in the frame, which is the puffs' jitter (`rand()` inside the draw) or their sprites' own motion. Arm 10 reads `.87` on both seats. A per-puff `rand()` watch at the emit (`0x8004A61C`) against the engine's draws would split the two. |
| Which pass raised the HUD plaque and readout bar `battle_map01_gomboo_steal_mid_cast` shows three pixels short of rest? | open (measured) | Every glide record reads retired with `elapsed` `15`, so the frame on screen is the last in-flight pass and neither ordering table the RAM holds draws the offset (the "saved on the pass after the landing pass" case of [retail-compare.md](../tooling/retail-compare.md#hud-glides-the-capture-had-landed)). The yaw counter puts the action seed some forty vsyncs back, too long for a seed-raised glide. Next: probe which `FUN_801D8DE8` call re-raises records `0x00` / `0x01` in the capture band. |
| What seats the yellow pillars `battle_menu_rugi_barra_retail` shows at Vahn and Gala? | open (measured) | A harness gap, not the engine's. The state's RAM holds four live parts, records `0x801F598C` / `0x801F59C8` (prototypes `12` / `13` of `0x801F6324`, the HP-restore cue groups of `FUN_801E22C8`, [the applier's sites](../subsystems/battle-action-exit-gates.md#fun_800402f4s-cue-group-sites)) on seats 0 and 2, each at its program's last wait (`pc 25`), while Vahn's HUD figure still rolls `1595` to `1614`. The replay opens at the prompt after that heal, so it spawns none. Next: a seed that seats the records and their program state (`+0x70` and the level ramp are not among `CastPartSeed`'s offsets). |
| Why does retail's step fall from `4` to `3` partway through Ozma's arm 21? | open (measured) | Four retail recordings of the `0xA0` cast agree to the vsync: arm 21 runs `194` vsyncs at step `4`, then `161` at `3` (the ring's newest entry under the `0x2D1` threshold, `674..712` hblanks), arm 22 at `3`, and arm 23 opens on `3`. The arm table (`cast_module_frame_step`) holds one modal step an arm, so the engine runs arm 21 at `4` throughout and its arm 23 a pass longer, which lands arm 24's 32-vsync pulse a pass early on the arm's clock: the arm-24 whiteout ([settled](re-settled-threads/rendering-camera.md)). On `--retail-steps` arm 21 reads `.487` for `.449`, `.776` with `--rand-track` too. A per-frame cost from the arm's own draw load, measured on more than one emulator, would close it. |
| Which creature seats do the directors still not carry? | open (measured) | Reading each cast's `creature.csv` against retail arm by arm names them. Carried now: Kemaro's arm-8 lift (`0x801F7640..0x801F7674`), Terra's seat on its poll pass, Mushura's fall and chase bank `+0x48`, Palma's arms `0xB` / `0xC` pitch and arm `0x10` pose pin, Kemaro's arm-16 victim freeze. Aluru's creature backing off (`z = -2307` to `-3075`, arms 15..18) is no PROT 0916 store, and the body is hidden. Zenoir's creature opens `+8` ahead in two halves ([PROT 0908](../subsystems/cast-module.md#prot-0908-zenoir)). A watch on Aluru's `+0x38`, and the pass that first ticks Zenoir's seated node, would close them. |
| Why do five enemy casts lose their cues under the console CD timing? | open (capture) | Under `LEGAIA_BATTLE_CD_TIMING=console` ([the CD stop watch](../subsystems/battle-stage-camera.md#the-cd-stop-watch)) the enemy cast films lose cues: Glare's `0x290` never plays, Lip King's Kiss of Death loses all three, Aluru Cannon, Plasma Strike and Doomsday (`0xB4`) one or two. Either a module body emits its cue off a step-dependent test (an engine defect any step change would show) or the films place the cue on PCSX-Redux's pass lattice. A mednafen film of those casts decides. The gate on making the console timing the default. |
| What does a real console's frame cost read? | partial (measured) | The entry is `max(cpu, dma + draw of the previous pass's table)` from one vsync, and the GPU half is modelled on the v2 GPU's measured costs with no free constant, which reads mednafen's waits to `8` hblanks at the median ([what a real console's frame costs](../subsystems/battle-stage-camera.md#what-a-real-consoles-frame-costs)). The CD half is PCSX-Redux's response time ([the CD stop watch](../subsystems/battle-stage-camera.md#the-cd-stop-watch)). Left: the instruction cost, two cycles an instruction on PCSX-Redux and about `.8` of that on mednafen. A real console's ring (`0x80084098`) with `gp+0x3A8` / `gp+0x3AC` in a full party's strike and a cast's voice decides both. |
| Why do setup frames at step `3` fit `4` yaw units below the walk back? | open (measured) | Over the `pre_*` states at step `2` the table's `inc * step` a pass fits exactly, and a recording's `0x51` to `0x0C` climbs `2` a pass. At step `3` the frame fits `-10` where re-armed passes give `-6` to `-9`, about `1.5` px. The endpoint climbs the counter's drift a pass, so inc alternates `1, 1, 2`, and a recording through `0x51`, `0x5A`, `0x0A` and `0x0C` at step `3` (`battle_doman_killer_wolf_strike`) reads the live yaw `7`, `8`, `9` behind the endpoint in turn, with the counter climbing `4` a pass into the setup states, so the walk back stands right on those rows and the frame's extra unit is unexplained. |
| Which framing style did the earlier actor's Spirit leave? | open (narrowed) | The earlier actor is the RAM's: `DAT_8007BD00`, the member the gauge panel last named ([settled](re-settled-threads/battle.md#threads)). What `DISPLAYED_WALK_PINS` still holds is that action's style byte `ctx[+0xD]` (style `2` raises the pitch `0x80` and drops TR y `0x100`; the two Viguro Thunder Breaks and the Ghost Knight's Windhash read it) and, on five captures, a measured yaw. A `--watch` on `ctx[+0xD]` across a party Spirit's seed would name its writer and whether a later word keeps it. |
| What is left of `minigame_dance_retail_go_out`'s `.985`? | open (measured) | The beams stand on retail's ([dance](../subsystems/minigame-dance.md#the-halls-animated-props)). The drop shadows now draw over the stage floor ([settled](re-settled-threads/rendering-camera.md)) and barely move the score: the GPU's 5-bit average leaves the hall's dark floor nearly where it was. The pixels off by more than `32` are spread over the dancers' outlines, the stage lamps and the wall panels, no one region holding more than a tenth of them. Nor is it the clip phase: every `+0x68` cursor in the RAM is the engine's own, and shifting the bodies' clips moves the score by under `.002`. A film of the lamp cells' adds would split palette from geometry. |
| Why does Zeto's idle in `zeto_big_wave_mid_cast` show one pass older than the display lag says? | open (narrowed) | A film of the cut shows the cut pass drawing the advanced cursor (`52`) on a camera already on its endpoint, so the fit's `48` is another residual. The engine's film of the cut, scored pair by pair, holds the camera on retail's to the unit and leaves the cut pair at `.546` on Zeto's pose alone, with the film's Zeto three passes ahead ([detail](#zetos-big-wave-idle-one-pass-older)); next, that film with Zeto's idle seated on the recording's cursor. |
| Which capture-band bodies still run their arms in a pass each? | open (narrowed) | PROT 0963 is [directed arm by arm](../subsystems/cast-module.md#the-stagers-of-the-enemy-cells) (pre-cast film `.543`, sound `1.000`); arms 2, 3, 6, `0x0F` and `0x10` read under `.5`, a red fill where retail draws a starfield or a white flash. PROT 0936 and 0937 are directed arm by arm, and their ray-burst stagers seat per call (`retona_songi_hyper_crush` / `_lightning` arm 2 `.951` / `.939` on `--rand-track`). PROT 0954 is ported whole; its film differs on its roulette's random icons. [PROT 0958](../subsystems/cast-module.md#prot-0958-blazing-slash) is directed arm by arm; its six damage sites still fold once at the band's exit. |
| What dims Element Change's ray fan differently from the port's? | open (measured) | `battle_rugi_rogue_element_change_mid_cast` reads `image` `.841` with every other channel `1.000`. The fan is arm 1's emitter `0x801F8FF0` dealing `0x801F9008` rays on `rand()` draws ([cast-module](../subsystems/cast-module.md#prot-0964-element-change-rogue-wind--thunder--flame)), seated on the crystal's display point; the rays' brightness differs between the two frames. A retail film of the arm's packets (colour word and blend of each ray) against the port's would say whether the dimming is the rays' own ramp or the `rand()` deal. |
| What sheds the rest of Element Strike's burn? | open (measured) | Most of the gap was the standing-in golem's after-images: a `--ghost-trace` recording ghosts seat 7 on every frame of arms 6 and 7, each ghost a draw of the burn loop. The walk takes the monster rule by category (`sltiu v0,s3,0x3` at `0x8004938C`), which the port now follows; the history-ring stamp's rule (`sltiu v0,s1,3` at `0x80047F10`) also keys on the seat, and with both Rogue Flame's arm 6 draws match retail. Arm 7 (`k = 5`) reads `1175` against `1590`. Seated in retail's Spirit stance (`--guard 0,1,2`, which the driver reads off the state when `pcsxr-state` is built), Flame reads `.925` for `.905`. |
| What is left of Koru's Gigaton Press film? | open (measured) | `koru_pre_gigaton_press` reads `.833`, `.893` on `--rand-track` and `.816` on `--retail-steps`; phase 4 goes from `.756` to `.868` on the track, so the rock shards dealt afresh each pass are most of it. Left: retail's light column reaches the floor's red ring a few passes before the engine's. A film of the column's packets across phase 4 would say whether that is its clock or its draw. The gold ellipse under Koru in the `koru_*_mid_cast` stills is not this: it is the walk's [floor rings](../tooling/retail-compare.md#a-turning-effect-parts-banks-are-a-clock). |
| What is left of the battle camera against retail? | open (measured) | Ranked by `cast-camera-rank.py`, the residue is pacing, not framing law: the frame step's cadence (retail `3` against the engine's `2` where a lone member's frames carry a monster's ghosts, a heavy arm's overrun, [a heavy fight's hit frames](../subsystems/battle-stage-camera.md#a-heavy-fights-hit-frames)), film openers and older recordings that lack the caster's heading (harness), and summon arms whose first pass lands a frame apart at close-up depths. A per-triangle ghost cost lifts lone-member strike films and overshoots on a full party, so the fixed cost stands ([a frame's draw load](../subsystems/battle-stage-camera.md#a-frames-draw-load)). Next: per-arm films of the player summons (`--retail-parts`). |
| What is left of Zenoir's Fire Claw film? | open (narrowed) | A second recording from `battle_garmel_zenoir_strike` (`--monster-pick`, seat 4, target 0) keeps the whole party, and the film reads `.785`, `.821` with `--retail-steps --rand-at 0x2B:0`, cues `8` of `8`. Two seats differ from retail's fight, neither the cast's law: retail's harvest rounds killed the other monster (seat 3, `0` HP) where the engine's seat keeps it at `279`, and the burst's roll (`91` in retail) reads `82` plain and `97` on retail's seed at `0x2B`'s entry, so the stream parts between that entry and the hit. Next: `--hit-trace`'s seed word at the hit dealt to the engine's roll, and the dead seat seated from the recording. |
| What holds Seru arms low on every film seat? | open (measured) | On the Gobu Gobu Seru films, arms that read low plain, on `--retail-steps` and on the rand track too ([three seats](../tooling/cast-sequence-oracle.md#the-seru-set-on-three-seats)). Barra's arms 6..0xB were the sky's scroll and a stage byte fired at its arm's entry ([cast module](../subsystems/cast-module.md#barras-sky)); its arm `0xE` (`.75`) runs eight-vsync frames. Gimard's arm 11 (`.78`) has its own row. Nova's arm 16 (`.72` on retail's steps) is not its tint: its camera now turns with the recording's, and what is left is the jittered rings and the victim's pose. Mushura's arm 12 chase (`.74`..`.79`) now starts from retail's stand. Terra's arm 3 (`.80`, vortex texture phase). |
| Why do Caruban's Fire Breath stills sit four yaw units off? | open (narrowed) | The camera law is settled forward by a film; what is left is the near bodies' placement or the displayed pass ([below](#caruban-fire-breath-stills-four-yaw-units-off)). A `LEGAIA_RAM_AT` film over `0x2B` would pin the pass. |
| What holds the Delilas and Cort casts' arms low on every film seat? | open (narrowed) | Seven classes are closed, the latest Blazing Slash's flames: its stager seats a cone pair per emitter call, not the whole set once ([Blazing Slash](../subsystems/cast-module.md#prot-0958-blazing-slash)). On `--retail-steps --rand-track` the films read Megaton Press `.959`, Plasma Strike `.938`, Blazing Slash `.926`, Mystic Shield `.975`, Guilty Cross `.940`, Mystic Circle `.906`; what holds plain is the deal. Blazing Slash's popup steps once a battle frame, as [retail's draw](../subsystems/battle-action-helpers.md#fun_801df6b8---damage-number-popup-renderer) does; arms `0x0D` / `0x0E` stay at `.62` / `.55`, held by the puffs' deal. Arms `0x15` and `0x18` (`.72` / `.84`) are not yet read. |
| Why does Big Wave's water sheet differ in brightness? | open (measured) | Not the sheet's own state: a `--retail-parts` film of `noaru_slippery_v2_pre_big_wave` holds the sheet `0x801F7A50` on retail's colour word, tpage and CLUT, and its `+0x78` fade and radii walk retail's within a frame. Its page scrolls one row a game unit on both sides, and zeroing the still's scroller texels moves the frame `0.24` luma. The gap is the whole upper frame: the still's rows `0..140` read `3..4` luma darker, the film's arm-6 pairs `1..3` darker through the fade-in, as one frame of the `+0x78` ramp would. A probe of the displayed frame's `+0x78` would close it. |


### Caruban Fire Breath stills four yaw units off

`open (narrowed)` `battle_rugi_caruban_2_` / `_3_fire_breath_mid_cast` read `image` `.931` / `.938`, camera `1.000`. The displayed pose is the RAM's table walked back two step-`3` passes (case `0xA` re-arms at `ceil(gap / 6)`, the yaw endpoint climbing `24` a pass), and the walls fit it best. Near bodies fit four yaw units further back (`.988` / `.982`).

The `_2_` state is saved mid draw tick: seats `2` / `3` hold view translations `+0x2C` on the camera one step-`3` pass back on the table's increments (pitch `43`, yaw `3508`, to a unit), which pins the walk back's first pass.

The law the walk back inverts holds forward to the unit. A film of the cast from the `_2_` state, with Caruban's clip `9` seated on the state's own cursor `40` and the party on the state's idle cursors, walks case `0xA` onto retail's yaw pass for pass (`3499`, `3514`, `3535`, `3556`, `3580`), with pitch, TR and focus, and hands on to the group shot `0xB` at the first hit to the unit ([the cast driver's shots](../subsystems/battle-stage-camera.md#the-cast-drivers-clip-clock)). So the four units are not the yaw law: they belong to the near bodies' own placement on the displayed frame (Vahn's and Gala's idle pose or ground pair), or to the pass the screen shows. Grade `capture`.

### A walked-back pose about twenty units of yaw off

`open (narrowed)` The yaw half is settled ([settled](re-settled-threads/battle.md#a-walked-back-pose-about-twenty-units-of-yaw-off)): `battle_map01_gomboo_steal_mid_cast`'s `3610` is the walk back's own, and two films confirm the re-arm law the walk back inverts, yaw and focus alike. What is left is the near-camera ghosts' placement below.

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

The engine's film of the cut, against the PCSX-Redux recording of
`garmel_zeto_pre_big_wave`, scored pair by pair: the camera is retail's to
the unit through the cut (yaw `0`, pitch `32`, TR z `2380` on the pass the
recording reads it, then `2492`, `2604`, a step-`4` pass each), and both
sides draw the close-up first on that pass, which retail shows two passes
later. The cut pair still reads `.546`, and its whole residue is Zeto's
pose: the film's Zeto idle cursor stands at `114` where the recording's
reads `78` (rate `8` halving to `4` on the cut on both sides), three passes
ahead. One of the three is the opening (the engine runs `0x0A..0x28` in
three passes where retail runs two), the rest the film's seat of the
state's cursor. The film cannot weigh `48` against `52` until Zeto's idle
stands on the recording's cursor at the cut. Grade `capture`.

The cut has been filmed, and it draws neither an older cursor nor a
camera short of the snap. From `garmel_zeto_pre_big_wave` (Zeto's own AI
pick, PCSX-Redux), `autorun_cast_sequence.lua` logs the camera per vsync and
`autorun_drawn_pose_film.lua` the anim ticks and decodes per pass. Arms `0`
to `4` run in one pass: the module counter `ctx[+0x279]` reads `0` after
one pass and `4` after the next, and that next pass halves Zeto's rate
(`8` to `4`), lands the cut on its endpoint (yaw `3956` to `0`, the table's
increment `140` clamped; TR z `4714` to `2380`), ticks Zeto from `72` to
`78` at the halved rate and decodes `78`. The cut is first on screen two
passes later. The mid-cast state's own cursor ring holds the same edge:
`60, 56, 52, 48, 36, 24`, rate `4` from the `48` to `52` step, so its cut
pass is the one that drew `52`, two passes behind the RAM's `60`, as the
display lag says. The fit's `48` is therefore not the drawn cursor: the
cut pass draws `52` on a camera already on its endpoint, and the image's
preference for `48` with yaw `+6` is another residual of that frame (the
body's matrix, or the metric trading sway against framing). Grade
`capture`. Next: the engine's film of the same cast scored at the cut's
pair, against a recording that opens on cast begin `0x28` as the oracle
pairs it (this one opens on the state's `0x0A`, and the oracle pairs it at
`.46` throughout).

Answered questions: [settled battle / arts / level-up](re-settled-threads/battle.md), [settled rendering / camera](re-settled-threads/rendering-camera.md), [settled world map](re-settled-threads/world-map.md); falsified readings: [battle](re-do-not-re-walk.md#battle--arts--level-up), [rendering / camera](re-do-not-re-walk.md#rendering--camera).

## Audio / BGM

Answered questions: [settled audio](re-settled-threads/audio.md); falsified readings: [audio / sound driver](re-do-not-re-walk.md#audio--sound-driver).

### How long does a summon's seek hold its voice?

*Status:* partial (measured)

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
for one (a re-key or a pitch write on the same voice). A summon cast does not disturb the calc on
PCSX-Redux: a Theeder cast (`0x82` from `party_basic_attack_vs_gobu_gobu`, `cast-sequence.py
--calc-clock`) enters `FUN_80062F98` exactly once a vsync across `1451` vsyncs, the module's seek arms
and the side-band's loads included, and every KON write comes from the flush (`0x80065F7C`). That fight
plays the battle theme, not `2013`, and mednafen's calc has not been watched over a cast. What closes
it: a state from the battle that leads into this cast, run under the same clock with a watch on that
voice's pitch and KON bits; the library holds no state of that fight before the cast.

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
