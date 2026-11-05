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
| How much of Iota's arms 5..9 is left once the sheets stop crossing? | open (measured) | The cycler rows 488 / 489 at a mid-arm-7 state match `apply_hsv_cell` entry for entry (`LEGAIA_DIAG_CAST_VRAM_AT` against `sstate_vram.py`), and the part sets agree. Scoring each retail frame against engine frames `+-32` ticks away lifts arm 5's pairs from `.145..336` to `.41..60` four ticks earlier, on the steady-step arms where a retail image can read a step older than its pair (the [offset rule](../tooling/cast-sequence-oracle.md#the-pairing-rule)), so the phase score there understates the match. The rays' `rand()` deal is what those four ticks leave; `--rand-at` on the arm's key would part the two. |
| Which frames of Ozma's closing arms lose the creature? | open (measured) | A retail table of arm 23 (`ot-raster.py`) links the additive rays at buckets `1083..2035`, behind the creature's `660..730`, so a ray sorts by its mean where the engine tests its near end per pixel. Drawing every part at its prims' mean depth moved Ozma by nothing and cost Meta `.06` while its flame walls crossed; with [tessellated sheets](../subsystems/renderer.md) keeping their order the reading is worth re-running. A blend pass that orders semi prims against the body's prims would close it. |
| What whites out Ozma's arm 24? | open (measured) | Retail's frame reads luma `178` at a mid-arm pair and the engine's `229`, though the arm's part counts agree record for record. The ladder's fades now sit under the HUD, as the table links them. The rays' `rand()` deal and the additive build-up on the armed wash are what is left to read apart. |
| How much of Mule's arm 16 is the hue cycler's phase? | open (measured) | Both sides' mode-3 parts (`--retail-parts`, `LEGAIA_DIAG_CAST_PARTS`) carry one triangle of H adds on the game clock: the rising legs agree and the falling legs read retail `32` higher. The add moves `8` a tick, so a pair reading a retail image a step older than its tick (the [offset rule](../tooling/cast-sequence-oracle.md#the-pairing-rule)) already parts the whole sheet's colour; the arm's framing and part sets agree. A score scanned over the pair's offset would separate the phase from any residual. |
| What draws the lightning arcs past Aluru's blast in the engine? | open (measured) | From arm 19 through the Done band the engine draws blue arcs around the victim and the caster that no retail frame holds, on both verdict paths. No part of the cast's scene is live by then (`parts.csv` ends with arm 18's records), so it is another emitter; retail's silhouette also stands in front of the bands, which link at the framing depth where the port's overlay draws over the scene. |
| Why are Slippery's storm clouds dimmer than retail's in arm 6? | open (measured) | The clouds stand where retail's do, to a unit, on the same `+0x78` ramp, but the frame reads darker. Each record's `rand() % 4` pick differs; `--rand-at` on the arm's key would take the deal out. |
| Why does Theeder's arm 11 flash read a step ahead once a fade takes its spawning frame's step, and why does Gizam's arm 9 fall with the `0x32` hold? | open (measured) | Theeder's arm reads `.83` without the step and `.65` with it, every other pair a step out; Gizam's reads `.92` without the hold and `.79` with it, at either an even or an odd tick shift, so it is not the frame grid. Whatever the arm's walk order or `rand()` stream decides is not named yet. |
| Which creature seats do the directors still not carry? | open (measured) | Reading each cast's `creature.csv` against retail arm by arm names them: Puera's clips (retail stages `1` bare on arm 5, bumps `2` and `3` on arms 8 and 9, stores `4` on arm 10 and bumps on arm 18; the engine plays `1` then `2`), Kemaro's rise in arm 8, Aluru's creature backing off from `z = -2307` to `-3075` in arms 15..18, and Gimard's stand at arm 12. Each is a store the director's `Not carried` list names or a walk it leaves out. |
| Which capture bodies still replay their clips ahead of the arms? | open (disassembly) | A `CasterStageRow` with `arms: None` plays its clip through module phase `0`, so its first arm holds for the clip (Glare `+282` ticks before its row was placed on arm 0). The dispatch tables place the sites: Hyper Crush `0x0B` on arm 2 and `0x0D` on arm 6 (the row lists them by address, the arms run the other way), Hyper Lightning `0x0C` on arm 2 and `0x0D` on arm 3, Bloody Horns `9` / `0x0A` on arms 2 / 3, Chaos Flare `8` on arm 2, Terio Punch `0x0A` on arm 0, Melt Spray `7` on arm 2, Power Charge `0x0A` on arm 0, Death Game `9` on arm 8. A recording of each cast would confirm the length once the row is placed. |
| Why does Nova's clip `8` ring `0x03` on a block in the engine? | open (measured) | Under `--strike-deal` all three Nova strike recordings play an extra `0x03` about two ticks ahead of a block voice on a defender already holding its block clip. Retail plays that `0x03` only on the swing that lands a flinch (`battle_jou_nova_v3_strike`, vsync `90`). Not traced: the cue track of Nova's clip `8` against `seats.csv`'s cursor `+0x1F6` is the next read. |
| What rate does Water Crystals' victim fall at? | open (measured) | The hit lands on retail's arm and the first cue on its offset within `4` ticks, but retail's knockdown keeps one pace (`60` then `25` vsyncs between cues) where the engine's slows (`64` then `40`) and arm 5's settle runs `282` ticks to retail's `131`. The engine runs PROT 0949's stager `0x801F75BC` off the phase; retail's callers are its records, and a `--physical` recording opens no `rates.csv` before its next cast. A recording with the rate from its first vsync would show which arm of the ramp holds. |
| Where does the battle HUD read a zeroed maximum MP? | open (measured) | Lapis Wave stores `+0x152 = 0` on every party seat (`0x801F6D1C`) and retail's rows read `0/0`. `engine-battle-hud::world_sync` draws `tables.character_max_mp`, so the engine's read `0/602`. First step: give the fight a per-seat maximum the module can write, and restore it from the stash `+0x178` on the path retail restores it. |
| Why does an Orb's Heal land on other numbers? | open (measured) | Retail's Killer Bee goes `96 -> 46` at a hit event in `0x3E` and `+200` at `0x3F`'s applier (`FUN_800402F4(0, 0, ..)`, the static heal table), and shows no heal popup. The engine shows the `50` popup, writes neither until `0x50` and folds `+160` from the spell catalog with a popup. The `FUN_801EC3E4` apply mode for a monster hitting a monster seat is the place to start. |
| What is the pale disc below Vahn on `bubu2_field_door_hop`, and what hides his shadow? | open (measured) | An engine-only semi-transparent draw of about `17 x 18` px at `(138..155, 152..170)`, `image` `.998`, in the placed-object layer's frame. It turns dark with semi-transparency off and survives with effect billboards off; it is in neither the screen-prim list near that spot nor the draw census's on-stage families, so it is a player, NPC or effect mesh, and retail's table holds no packet there but the rock quad at bucket `2514`. Per-pixel depth hid it under the rock; the link rule lets it through. Separately, the engine's four `7F86` shadow cells land within a pixel of retail's `(145..166, 139..155)` packets and do not show: the rock's link depth stands in front of their depth. |
| What frames Koru's Done tail on its first pass? | open (measured) | `koru_pre_*` hold `0x51` on the arm's pass, RAM yaw `3445`, TR z `6481`, focus z `-174`, walking to `3300` / `5734` / `327`, and the pose one pass back inverts off the table's own increments (`3496`, `6730`, `-342`). Seating the child by hand on the floor fit (yaw `3600`, TR z `5866`, focus z `-422`) moves the frame's block agreement by about `.065`, the most of any seat tried over TR z `5866..8192`, yet Koru stands larger than retail's, so that fit trades body size for floor. The replay's own `0x20` tail passes yaw `3577` and focus z `-421` eight ticks before the gate, on a table aiming TR z `8192`. A retail film through `0x20` to `0x51` would name the pass. |
| Why do setup frames at step `3` fit `4` yaw units below the walk back? | open (measured) | Over the `pre_*` states at step `2` the table's `inc * step` a pass fits exactly, and a recording's `0x51` to `0x0C` climbs `2` a pass. At step `3` the frame fits `-10` where re-armed passes give `-6` to `-9`, about `1.5` px. The endpoint climbs the counter's drift a pass, so inc alternates `1, 1, 2`. A recording through `0x5A` at step `3` would read the passes. |
| Which earlier actor did a monster's cast walk from? | open (measured) | `FUN_801DABA4` leaves spent keys and no order, so `DISPLAYED_WALK_PINS` holds a seat, a style and sometimes a yaw read off each retail frame. A word that keeps the last actor or its `ctx[+0xD]`, or the caster's heading before `0x28` turned it, would replace those pins with law. |
| What pose does `battle_garmel_songi_hyper_wave_mid_cast` display? | open (measured) | A fit reads `.992` at pitch `10`, yaw `3970`, TR y `1251`, TR z `5051`, focus `(289, -601)`. The walk back along the RAM's table stands at pitch `6`, TR z `5171`, on the shot's origin, so the frame is about two case-6 passes older than the terminator, where every other second-pass capture shows the terminator itself. Its step ring reads `3` only on the newest pass, so a recording of this cast would show which passes the accumulator counted. |
| Which pose does `kor_field_route_walk`'s frame stand on after the walk rewind? | open (measured) | A hand-seated pose of pitch `240`, yaw `1770`, `H` `256`, eye `(0, 580, 4850)` reads `.991` over the seat's `.912`: the rewind's yaw law holds (`1769`), and the eye is `(-42, +82)` from its `(622, 4768)`. A [walk film](../tooling/walk-film.md) from the state matches the ease to the unit on every pass after it, and eye Z sits on its target `4768` in the RAM; a step of the ease cannot close `82` units in two passes, so the fit's Z is not the ease's history. The `(868, 4521)` target an earlier backing-out implied has no support in the block, whose floor is flat here; a film of the same corner walked in from the stairs would show the frame's own lead-in. |
| Why does the dance floor hide the dancers' shadows in PSX mode? | open (measured) | `World::field_drop_shadows` builds the hall's three blobs on retail's packets to a pixel (`146..174, 156..161` under the centre dancer on `minigame_dance_retail_go_in`), and with no depth they show, over the dancers' feet, where retail draws them under. With their scene depth the floor occludes all of them, though the blob's world height is the floor's less the lift. Comparing the hall bake's depth at the blob's corners with `FogView::ndc_depth` there would say which side is off. |
| What is the green wedge on the centre dancer's wrist? | open (measured) | Retail's `minigame_dance_retail_go_in` table draws `POLY_GT3`s of `0x7785` / tpage `0x3D` at `(174..180, 125..139)`, UVs about `(134..145, 5..38)`, green texels. CLUT and texels match the engine's VRAM word for word and the frame shows no wedge there. Whether the mesh's prims are dropped or their bone is posed elsewhere is unmeasured. |
| What is left of `minigame_dance_retail_go_out`'s `.891`? | open (measured) | The coloured beams across the top stand elsewhere than retail's, about a tenth of the frame's blocks. The state's newest ordering table holds only the HUD above row `60`, so the retail side of a per-draw comparison needs the other table (`--all-ots`). |
| Why does the engine's `camera.offset_ease` stand apart from retail's `_DAT_8007BCAC` after a door hop? | open (measured) | The fog sheets stand against this word (`FUN_8003F3FC` at `0x8003F448`). `concnow_field_door_hop` holds `163` where the engine's accumulator reads `233`; the child seats the word. A [walk film](../tooling/walk-film.md) from the state shows retail climbing about one unit a vsync through the `45 07` glide and freezing at `266` as the glide lands, with the footing still bobbing under it, so its input lock rises there. The engine stands frozen at `234` from the first tick: its script gate runs the record to the phase with the lock already up, and the seed ramps nothing. So the gap is the seed's phase, not the ease. |
| What deals `minigame_muscle_dome_retail_skeleton_hyper_arts`' speed lines? | open (measured) | With the frame's depth right they are what is left of its score, and they differ between retail's own two buffers, so each pass deals them afresh. Seating Vahn on the starter the rings name (`0x1A` at `224`, through the art bank) moves the frame's error by less than a unit. A retail film of the banner's packets across a few passes, against the `rand()` word, would close it. |
| Which capture-band bodies still run their arms in a pass each? | open (measured) | PROT 0936, 0937, 0958 and 0963 have no `capture_camera_director`, so their shots, seat writes and per-arm records are absent and the pager seats the whole image at once. A `--physical --retail-only` film from a `*_mid_cast` state on `0x70` arm 0, beside the disassembly, reads each one; the corpus holds no retail state for 0936, 0937 or 0963 yet, and 0958's is a Delilas state. PROT 0954 is ported whole; its film differs on its roulette's random icons. |
| Why does Zeto's idle in `zeto_big_wave_mid_cast` show one pass older than the display lag says? | open (measured) | The displayed frame is PROT 0946's arm-0 cut pass, and the two passes after it match at `1.000`; a retail film of the cut pass with its camera globals and Zeto's node matrix ([detail](#zetos-big-wave-idle-one-pass-older)). |


### A near body under an inferred retake pose

`open` `battle_doman_boogie_pump_windcutter_mid_cast` (image `0.19`) is not a ghost
defect: Freed `$2` does draw faint. The body filling the frame is Gala, the cast's
target, which `FUN_8004DC68` keeps solid (`0x8004E0E0..0x8004E10C`). Retail's next
frame holds no packet of hers on either ordering table: at the RAM's pose she is behind
the eye, `SZ` saturates and the mean `OTZ` fails the near cut, so her packets sit in the
pool off every link at `x = 1023`. On the displayed frame the child stands on
`RetailBattle::retake_stale_walk_pose`, whose TR z and focus are inferred, and there her
origin reads a view depth of `258` against the draw gate's `0xA1`
(`dispatch_draws`). A search over that pose scores `.95` with her hidden, at TR z about
`4569` against the law's `4839`, and at that depth the gate rejects her. The search is
not a mechanism and nothing pins it. A retail film of the case-6 walk ahead of the
state (`LEGAIA_PRE_TRACE`, from a state a round earlier) would say where the law's
endpoint parts. Grade `capture`.

`battle_uru2_nurga_glare_mid_cast` has the other half. The near body is the second Nurga
(`+0x8 = 0x83000000` in the RAM), and it is opaque in the child because its frame reads
action `0x6F`, outside the magic band's row arm, and the child's partly aligned pose
takes no re-derive of the bits (`redraw_prep.rs`, `mask == 0xFF`). Grade `capture`.

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

### Soren Camp's sky

`open` `son_field_door_hop` (`image` low, every state channel at 1) shows a cloud sky
the engine does not draw: twelve textured quads (`tpage 0x98`, CLUTs `0x7C80` / `0x7CC0`)
first in the frame's primitive pool, a one-object TMD at model-pool slot 62. The RAM
holds an object actor for it at `(1088, 146, 2112)`, `+0x50 = 5`, `+0x58 = 10`,
`+0x64 = 0x3E`, `+0x10 = 0x28008880`. The engine's placed list for the scene is empty,
so the mesh reaches no draw. First step: read how partition-0 record 5 seats that actor
and which assembly path owns the slot; the sky band `FUN_801F73E4` is not it, since its
gate is the overworld sweep.

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
The port starts the newer word. A linear census of the disc's op-`0x35` sub-op `1` pairs
closer than the walk's `66` vsyncs, or a script timeline that holds two, would say whether
any shipped scene reaches it.

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
