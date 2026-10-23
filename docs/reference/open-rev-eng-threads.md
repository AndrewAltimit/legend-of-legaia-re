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

The live hunt here is the dialog font's unmeasured edge ([`dialog-font.md`](../formats/dialog-font.md)).

| Thread | Status | What would close it |
|---|---|---|
| Where does the escape-expansion buffer at `0x800740EC` end? | partial (bounded at `0x206` bytes by inference; the code sets no bound) | The zero-initialised run reaches the next initialised data at `0x800742F2` and no dumped routine addresses inside it. The expander `FUN_80036514` has no destination-length test, so the end is a property of the strings, not the code: the longest expansion of any drawn string - a disc census of every drawn string expanded with its longest substitutions, or a write watch while the longest one draws - pins it. |

Answered questions: [settled field / locomotion](re-settled-threads/field.md), [settled text / dialog](re-settled-threads/text-dialog.md); falsified readings: [field / locomotion](re-do-not-re-walk.md#field--locomotion), [menus / UI](re-do-not-re-walk.md#menus--ui).

## Battle / rendering

| Thread | Status | What would close it |
|---|---|---|
| What is left of the juggle window? | partial (the per-frame law is pinned; the four-slot read is narrowed) | Two residuals ([battle-formulas.md](../subsystems/battle-formulas.md#the-juggle-window---what-makes-a-monster-juggleable)): a four-slot list reads `entry[0x23 + 4k]` with `k` set by the monster block's heap address: both measured fights read `0` (the port's value); a `0xFF` slot would hold the window open ([monster-animation.md](../formats/monster-animation.md#event-frame-list-entry-0x100x13)); the measured window runs 1-2 ticks past `32 * beat / (speed * rate)` because the cursor's sub-frame carry at the commit is unmodelled. A capture of the commit's carry closes the second. |
| Why does Zeto's idle in `zeto_big_wave_mid_cast` show one pass older than the display lag says? | open (measured) | A retail capture of Big Wave's arms 4 and 5 with the caster's cursor and ordering tables beside each frame ([detail](#zetos-big-wave-idle-one-pass-older)). |


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

Answered questions: [settled battle / arts / level-up](re-settled-threads/battle.md), [settled rendering / camera](re-settled-threads/rendering-camera.md), [settled world map](re-settled-threads/world-map.md); falsified readings: [battle](re-do-not-re-walk.md#battle--arts--level-up), [rendering / camera](re-do-not-re-walk.md#rendering--camera).

## Audio / BGM

No open threads. Answered questions: [settled audio](re-settled-threads/audio.md); falsified readings: [audio / sound driver](re-do-not-re-walk.md#audio--sound-driver).

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
