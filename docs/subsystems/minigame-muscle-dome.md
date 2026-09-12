# Muscle Dome minigame

The **Muscle Dome** is an arena contest fought as a ladder of **ordinary Legaia battles**. The player picks one of three courses and fights its fixed sequence of rounds - 8 / 8 / 13 - one real monster per round, each staged into the ordinary battle formation cell; between legs the arena settles a running score. The player fields one party character, and each turn the fighter enters a directional command string under an AP budget - the same input the normal battle command screen takes - which plays out through the shared battle-action path. It is **distinct from the fishing / slot / dance / Baka Fighter minigame-hub family** - it does not share their controller library.

The course roster is disc data and is decoded below: see [Course ladder](#course-ladder-the-opponent-per-course-round). A round ends on a knockout and is **not** turn-limited - see [What ends a leg](#what-ends-a-leg-a-knockout-and-nothing-else) - and the `Turns Left / HP Left` strip is **not** the dome's, see [The four-turn strip belongs to Koru](#the-four-turn-strip-belongs-to-koru-not-the-dome).

It is **not a card battle**. The "hand of four cards" reading came from the deal loop building four slots; those four slots are the four *direction commands* `0xC..=0xF`, always the same four, each carrying that fighter's own AP cost. Nothing is drawn, discarded or reshuffled.

Instead, the Muscle Dome runs **inside the battle-action overlay** (PROT entry **0898**, base `0x801CE818` - the same overlay [`move-power.md`](../formats/move-power.md) reads): its match SM `FUN_801d0748` and all its data tables (deck `0x801f4b8c`, sub-draw script `0x801f4d34`, victory messages `0x801f4dfc`) are resident there, so they are statically extractable from the disc (parser [`legaia_asset::muscle_dome`], disc-gated `muscle_dome_real`).

This matches the design - the arena reuses the battle engine wholesale (its fighters are battle actors, entered directions resolve through the battle-action path). The "`overlay_muscle_dome.bin`" Duckstation capture was that battle-overlay slot resident during the arena, **not** a separate overlay; the `0977` "Ronginus" entry is only the mode-24 sub-id-5 door/init slot (arena roster + `other6` paths), not the match SM.

### The dump set is the whole battle overlay - a filename prefix is not dome evidence

`ghidra/scripts/funcs/overlay_muscle_dome_*.txt` is the **entire battle-action overlay** dumped at the arena, not a set of dome-unique functions. The shared battle context `_DAT_8007bd24` these dumps read is the **same** context the main battle system, magic-capture, Baka Fighter, dance and fishing overlays use. So the great majority of the "`overlay_muscle_dome`" functions are **shared battle-system code**, documented in [`battle-action.md`](battle-action.md) / [`battle-formulas.md`](battle-formulas.md), not dome findings. The cross-check is mechanical: an entry whose body is also dumped under `overlay_battle_action_` / `overlay_magic_capture_` / `overlay_baka_fighter_` / `overlay_dance_` / `overlay_fishing_` is shared, not dome-unique.

Representative confusables (each dumped under several non-dome overlays, so **shared battle**, not dome):

| Address | What it is | Belongs to |
|---|---|---|
| `FUN_801d0748` | the round driver - **byte-identical to the main battle round loop** (also under `overlay_battle_action_` / `overlay_magic_capture_` / `overlay_magic_level_up_` / `overlay_0898_`); this page documents only its *dome role* (`ctx+6` match phases) | [`battle-action.md`](battle-action.md) |
| `FUN_801d32bc` | next/prev **living-actor cursor** (skips actors with 0 HP at `+0x14c` or a set status mask `+0x16e & 0xf84`; steps `ctx+0x13/0x20/0x21/0x1f`) | [`battle-action.md`](battle-action.md) |
| `FUN_801d84c0` | **battle-outcome message builder** ("won the battle / Gained Experience", "is out of strength", "escaped") into `ctx+0xa9/0x129/0x159/0x189` via the SCUS `strcpy`/`strcat` pair | [`battle-action.md`](battle-action.md) |
| `FUN_801f44a0` | pushes one entry into an 8-slot **damage/number-popup ring** (`ctx+0x83c` value / `+0x318` param / `+0x85c` timer, counter `+0x262 & 7`) - also under dance / Baka Fighter / fishing / slot / debug-menu | [`battle-action.md`](battle-action.md) |
| `FUN_801f3c34` | The Seru-magic **"No effect." banner pass** - not a guard: it rejects nothing and touches no queue state. At the summon's return-from-fade it finds the cast spell's magic level and, when that is `>= 3` and the side-effect stager left nothing pending in `0x801F6960`, installs the "No effect." string at `0x800775B4` and fires banner `0x66` - also dumped under dance / Baka Fighter / fishing / slot | [`battle-formulas.md`](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch) |
| `FUN_801f3d3c` | The Seru-magic **side-effect stager** the pass above pairs with: stages the element-keyed stat debuff (percent by magic level) the damage finisher applies per hit, behind the scripted-fight suppression roll and the base-vs-record compare. See [The side-effect pair](#the-side-effect-pair) | [`battle-formulas.md`](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch) |

Two entries are dumped **only** under `overlay_muscle_dome`, and both were
carried here as "dome-vs-shared unconfirmed". Neither is the dome's; a caller
sweep settles both, and the filename was the only thing that ever tied them to
the arena.

- **`FUN_801f2410`** - the **cast colour-wash emitter**. Its only callers on the
  disc are the two cast dispatchers' epilogues, `FUN_801F1ED4` at `0x801F2144`
  and `FUN_801F2160` at `0x801F23F4`, each gated on `ctx[+0x27A] != 0`
  ([`cast-module.md`](cast-module.md)). It builds screen-wide `POLY_G4` packets
  - GP0 code `0x38` OR'd into the colour word, tag length `8`, right-hand
  vertices pinned to `x = 0x13F` - straight onto the scratchpad packet cursor
  `*(0x1F8003A0)`, colouring them `ctx[+0x27E..+0x280] * ctx[+0x27A] / 255` and
  scrolling them by `ctx[+0x32A]`, which `ctx[+0x27C]` selects a mode for.
  It is cast presentation, not a HUD number emitter and not the dome's.
- **`FUN_801f2e10`** - the oriented-quad "beam" emitter (see
  [Key functions](#key-functions)). It has **no reference of any kind** in SCUS
  or in any overlay image; its only callers on the whole disc are **11 `jal`
  sites inside PROT 0909**, a slot-B summon module, whose `FUN_801F7948`
  (`0x801F7980` onward at base `0x801F69D8`) paces widening pairs of beams
  against the counter `_DAT_8007BD1C`. It is summon-cast presentation.

The genuinely dome-unique controller / presentation set is the
[Key functions](#key-functions) table below (`FUN_801d0748`'s dome arms,
`FUN_801d388c`, `FUN_801d5854`, `FUN_801d8de8`, and the panel helpers);
everything else in the dump directory is battle-overlay furniture.

### The side-effect pair

`FUN_801F3C34` and `FUN_801F3D3C` are two halves of one mechanism, not two
unrelated helpers, and reading either alone invites the "AI decision" guess.
They share their whole preamble - resolve the acting actor out of
`&DAT_801C9370` by `ctx[+0x13]`, take its queued action byte `+0x1DF`, scan
the caster's spell-id array (character record `+0x13D`, `0x20` entries) for
that action, read the parallel magic-level byte at record `+0x161`, and bail
when the level is below `3`. Both then raise the same banner id `0x66`
through `FUN_801D8DE8(0x66, 0)`.

What separates them is which side of the latch each one is on:

- `FUN_801F3D3C` runs at cast time from inside the spell's summon module and
  **stages** the cast's side effect: it selects the `[summon element][level
  band]` record of the table at `0x801F6870`, writes the record's percent to
  `0x801F6960` (the value the damage finisher's per-element switch shaves
  off the target on every hit) and its banner-string pointer to
  `0x800775B4`, and seeds the hold `0x801F6964 = 0xB4`.
- `FUN_801F3C34` runs at the summon's return-from-fade, **reads**
  `0x801F6960`, and when it is still zero installs the "No effect." string
  (`0x801CFA20`) instead - the miss is announced, the hit was announced by
  the stager.

The stager's gates - the scripted-fight suppression roll on `ctx[+0x287]`
and the per-element base-vs-record compare - and the table itself are on
[`battle-formulas.md`](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch).
Two earlier readings of this pair are retired there: the index into
`0x801F6870` is the summon record's **element** byte, not an "actor class",
and the record's second word is a **banner string**, not a follow-up routine
pointer (`0x801CFA20` is the text "No effect."). In the dome the pair runs
exactly as in any battle; nothing about it is arena-specific.

Port: `engine-vm::move_no_effect_guard` (the banner pass, live from state
`0x36`) and `engine-vm::seru_side_effect` (the stager + finisher switch, pure
kernels). `see ghidra/scripts/funcs/overlay_muscle_dome_801f3d3c.txt`.

## Entry from the field

The arena is reached by the **mode-24 minigame door-warp**: field-VM op `0x3E` with `op0 = 105` (`sub_id 5`), which sets game mode `0x18` and loads the door/init slot PROT 0977. The mechanism, its `sub_id` -> overlay table and the return warp are in [`script-vm.md` § 0x3E WARP](script-vm.md#0x3e-warp-mode-24-minigame-door-warp); the port's id decoder is `legaia_engine_core::minigame_entry::MinigameSubId`.

A disc-wide walk of every scene MAN puts every dome door in `koin1` P1[9] - three sites in one record, which is the **course menu**: each arm debits its entry fee through `0x4C 0xE5` and latches one of the course flags before its own `0x3E 0x69`. That is why one record carries three warps. Notably `taiku` carries none: its only `0x3E` sites are `0xFF` interacts, so it is not the arena's door despite the label. Census test: `crates/engine-core/tests/minigame_entry_census_disc.rs`.

**Why `FUN_801cf870` has no caller.** The hub is not called; it is the `+0x08` tick word of the static 24-byte actor template at `0x801D1A20`. The sub-id-5 init `FUN_801CEA6C` materialises that template and spawns an actor from it (`jal FUN_80020DE0` at `0x801CEAFC`), and the per-frame pool walk reaches it through `jalr actor[+0x0C]` in `FUN_8002519C`.

The match SM `FUN_801D0748` is the exception to that shape, and the reason is the one this page already makes: the arena reuses the battle engine, so the match runs as an ordinary battle and its driver is reached through the **battle** overlay's own template (`0x800767DC`, tick `FUN_80046A20`), not through the 0977 slot. `FUN_80046A20` is also the second of only two writers of game mode 24 on the disc: with `_DAT_8007BAC0 & 0x100` set it stores `0x18` rather than the field's `0x2`, which is what returns a finished round to the arena instead of to the field.

## Two state machines, not one

The dome runs **two** state machines stacked, and confusing them for one is
what makes a port of it feel wrong.

The inner one is the battle: `FUN_801D0748` in the battle-action overlay
(PROT 0898), the shared round driver, which plays a leg out and ends it on a
knockout ([What ends a leg](#what-ends-a-leg-a-knockout-and-nothing-else)).
Almost nothing in it is dome-specific - it has exactly **one** contest-gated
arm, at `0x801D322C`, where a `lw` of the mode-24 sub-id word `_DAT_8007BAC0`
and a `beq …, zero` skip the whole block unless a contest is running. That
block is the flee arm: on action state 5 (`actor+0x1DE == 5`) and a formation
monster that is not one of the four unfleeable ids, it stores
`_DAT_80084448 = 4`, which is how running reaches the arena.

The outer one is the **contest** - the ladder run above the legs - and it
lives entirely in the arena roster/init overlay (PROT 0977, slot-A base
`0x801CE818`):

- `FUN_801CEA6C` is its entry, and is re-entered after **every** leg. A
  zero sub-id word means a fresh contest; a non-zero one means a leg just
  finished, and the only thing that arm does before the common tail is
  `word += 1` (`0x801CEC00`).
- `FUN_801CF870` is its per-frame hub, dispatching `DAT_801D1A78` through a
  **51-entry jump table at `0x801CE990`**. Fourteen states are real - `0`,
  `1`..`6`, `0x0A`..`0x0C`, `0x14`..`0x16` and `0x32`; the other 37 route to
  the table's default arm. Every state but `0x32` falls through the same tail
  at `0x801D00B8`, which re-packs `(course, round)` into the word.

### The cursor: `(course, round)` packed in one word

Both the course and the round live in the low byte of the mode-24 sub-id word
`_DAT_8007BAC0`:

| Quantity | Expression | Site |
|---|---|---|
| course | `((word - 1) & 0xFF) >> 4` | `0x801CEBD4`, `0x801CEC30` |
| round | `(word - 1) & 0xF` | `0x801CEC18` |
| next leg | `word + 1` | `0x801CEC00` |
| re-pack | `(word & ~0xFF) + 1 + (course << 4) + round` | `0x801D00B8` |

The re-pack leaves every byte above the low one alone, which is why the three
course-entry seeds can carry `0x100` / `0x300` in them and survive a whole
contest untouched. Decoded course/round land in `DAT_801D1A90` /
`DAT_801D1A94`, which is the same pair `FUN_801D1510` indexes the ladder with.

### Which course opens

`FUN_801CEA6C` seeds the word `1` and then lets three story flags overwrite it
in order, so the highest unlocked course wins:

| Flag | Seed | Course |
|---|---|---|
| *(none set)* | `0x001` | 0 - Beginner |
| `0x536` | `0x101` | 0 - Beginner |
| `0x537` | `0x111` | 1 - Expert |
| `0x538` | `0x321` | 2 - Master |

The pad-driven three-column picker in `FUN_801D0CD4` is a dev screen, not this.

### How long a course runs

The course descriptor's own round count is the length - 8 / 8 / 13 - except on
the **Master course**, and only there: the whole clamp block at
`0x801CED28..0x801CEDA4` sits behind `bne course, 2`. Three story flags each
shorten it, and each is consulted only once the run has actually reached its
threshold:

| Reached round | Missing flag | Course stops at |
|---|---|---|
| 8 | `0x378` | 8 |
| 11 | `0x382` | 11 |
| 12 | `0x471` | 12 |

Retail applies them in that order and lets a later one overwrite an earlier
one, so a run missing all three stops at 12 rather than at 8. That is
reproduced rather than tidied. "Course exhausted" is then `round >= cap`
(`0x801CEDB8`).

### Which arm decides a leg was survived

Neither of the two this page used to guess at. It is a single byte test at
`0x801CEDD8` (and again at `0x801CEE1C`): `DAT_8007BD60 & 0x80`. The battle's
own state-`0x5A` party-wipe scan clears the bit, and the shared minigame-exit
routine `FUN_80026018` re-raises it (`ori 0x80`, `0x800260A4`) on the way back
out - so on arena re-entry the bit reads "the party is still standing".

That settles the last open input of `settle_contest`: `continuing`
(`DAT_801D1ADC`) is **derived**, not prompted. The latch has exactly three
writers - zeroed on every arena entry (`0x801CECE0`), zeroed at settlement
(`0x801D1058`), and raised at `0x801CEE08` on the one path that reaches it,
which is *course exhausted **and** survived*. There is no continue prompt to
build.

The resulting hub routing after a leg:

| Leg | Next hub state |
|---|---|
| survived, course not exhausted | `0x0A` - the between-leg tally screen |
| survived, course exhausted | `0x32` - settle, latch **up** |
| not survived | `0x32` - settle, latch down |
| ran (`_DAT_80084448 == 4`) | `0x32` - settle, latch down, `DAT_801D1A74 = 1` |

### The intermission is per fight, not per turn

The table above is a table of **legs**. No turn boundary reaches any row of
it, because the two boundaries are written by different arms and only one of
them leaves the battle:

| Boundary | Written at | What happens |
|---|---|---|
| turn | `0x801E67E8..0x801E6810` | `ctx[6] = 0x14` (the round driver's turn-top arm) and `ctx[+0x28a] += 1`; the driver re-enters its own command cluster `0x28` |
| leg | `0x801E65D8` / `0x801E6674` | `DAT_8007BD71 = 0xFE` - party wipe (cause `5`, `-0x42D4`) / monster wipe (cause `0`); the exit selector routes to arena mode `0x18` |

Both writes are in `FUN_801E295C`, the shared battle-action SM. While a leg
runs the game is in battle mode, so `FUN_801CF870`'s hub - the only thing that
draws the INTERVAL heading and the score tally - is not executing. Retail has
**no beat between turns at all**: the command cluster comes straight back.

So the intermission is a fight boundary twice over. The hub has to be running
to draw it, and even then only the first row above reaches state `0x0A`; a
lost, run-from or course-exhausting leg settles instead.

The trap the split hides is that both boundaries reach a host as "the turn's
playback ended". A host that keys the hub screen on the match phase alone
draws an intermission every turn. The verdict therefore lives in one shared
place, `engine-core::muscle_dome::leg_boundary_raises_interval`, called at the
leg boundary by the native window (`tick_muscle_hub`) and by the browser dome
page (through the `muscle_leg_shows_interval` binding);
`MusclePhase::ends_turn` / `ends_leg` name the same split on the session side.
Locked by `engine-core/tests/muscle_intermission_cadence.rs` and
`web-viewer/tests/muscle_page_cadence.rs`.

## What a cleared leg is worth

`FUN_801D1184` computes four count-up rows, and they do **not** all mean the
same thing. Three are scaled `× max_hp / 100` (retail's `0x51EB851F`
reciprocal multiply); the fourth is not scaled at all:

| Row | Value | Global |
|---|---|---|
| round | `round * 2 * max_hp / 100` | `DAT_801D1ACC` |
| turns | `min(turns_taken, 8) * max_hp / 100` | `DAT_801D1AD0` |
| outcome | `DAT_801D1A5C[min(outcome, 3)] * max_hp / 100` | `DAT_801D1AD4` |
| score | `score_table[course][round - 1]` | `DAT_801D1AAC` |

`DAT_801D1A5C` is `[8, 12, 4, 2]`. `turns_taken` is `_DAT_80084444` and
`outcome` is `_DAT_80084448` - the same word the flee arm sets to 4.

The tally screen `FUN_801CF074` then drains all four, one `step_scale` step
per lane per frame with a voice blip per step - and the *sinks* are what makes
this one mechanism instead of two. The first three lanes
(`0x801CF0DC` / `0x801CF150` / `0x801CF1C8`) all drain into the **same**
accumulator `DAT_801D1AC8`, which hub state `0x0C` then adds to the fighter's
HP. Only the fourth (`0x801CF244`) drains into the coin tally `_DAT_80084440`.

So three of the six rows on that screen are healing, not score, and a dome
contest costs no permanent HP.

### The between-leg restore

Hub state `0x0C` (`0x801CFE7C..0x801CFEA8`) does
`hp_cur = min(hp_max, hp_cur + DAT_801D1AC8)` on the `+0x6CC` / `+0x6CE` pair
of the game-state window `0x80084140`. That pair is the lead party record's
own `+0x104` / `+0x106` HP fields (`0x80084708 - 0x80084140 = 0x5C8`).

`FUN_801D0ED8` does the wider restores. It refills HP/MP/SP to their maxima at
contest start, and - only when `course != 0`, behind a `bnez` at `0x801D0EE8`
- first zeroes the four equipment bytes `+0x75E`/`+0x75F`/`+0x760`/`+0x762`.
So "no equipment" is an **Expert/Master** rule; the Beginner course keeps its
gear. Settlement (`0x801D0FDC`) restores the whole saved SC block.

Two details separate it from the between-leg restore above. It is a **one-shot**:
the arena entry reaches its `jal` at `0x801CEBF0` only on the `_DAT_8007BAC0 == 0`
side of the `bnez` at `0x801CEB58`, and a re-entered arena jumps to `0x801CEC00`
instead - so a leg boundary never refills. And the four bytes it zeroes are the
*gear* slots only (record `+0x196` armour, `+0x197` head, `+0x198` weapon,
`+0x19A` leg gear): the Seru-lock byte `+0x199` and the three accessory bytes
`+0x19B..+0x19D` are untouched, so a stripped fighter keeps its accessories and
its summon access. Port `engine-core::muscle_dome::apply_contest_start_restore`,
handed to the host by `DomeContest::take_start_restore` and applied in
`World::enter_muscle_dome`.

## Contest settlement + the one-shot prize

The `0977` door/init slot carries the **contest settlement** routine
`FUN_801D0F60` (file `+0x2748`; historically mis-cited as `FUN_801C2748` from
a `0x801C0000`-band import). After a contest leg it restores the SC block
(`FUN_8001A8B0`) and settles the running score tally `_DAT_80084440`:

- **Not continuing** halves the tally (signed `/2`); continuing keeps it and adds the per-`(course, round)` score-table cell `DAT_801d1860 + course*0x40 + (round-1)*4`.
- **Gave up** (`DAT_801D1A74`, raised only by the flee path) zeroes the tally and drops the continue latch `DAT_801d1adc`. When the give-up landed on **round 1** it also sets flag `0x130 + course` - the three flags curated lore knows as the Muscle Paradise / Chicken King trigger ("run from the first battle in all three difficulties").
- Continuing sets flag `0x50A`; giving up sets flag `0x35`. Both are cleared at the top of every settlement.
- On the **Master-course final fight** (round counter `DAT_801d1a94 >= 0xD`) with the one-shot flag-bank bit `FUN_8003CE64(0x6CB)` still clear, it awards item `0xCD` (the **War God Icon**) via `FUN_800421D4(0xCD, 1)` - the once-per-save first-clear prize.

The tally is then paid by the tail call to `FUN_80026018`, the **shared**
minigame-exit routine - nothing dome-specific: `casino_coins += tally`
saturating at `0x0098967F` (9,999,999), on the coin bank `0x800845A4`
(`0x80026058..0x80026078`).

That closes the reward question. A dome **leg** pays nothing at all; a dome
**contest** pays coins. The victory caption's spell id (`ctx+0x269 + 0x80`) is
a *string* index into `0x801F4DFC`, the shared battle-family cast-caption
label table resident in every battle overlay and read by any cast - it is not
a Seru award, and treating it as one put an invented capture on a dome win.

Engine port: `engine-core::muscle_dome::{DomeContest, settle_contest}`, driven
by `World::report_muscle_leg` / `World::settle_muscle_contest` on the native
host and by the `muscle_contest_*` bindings on the browser host - one shared
model, no per-host ladder rule. `see ghidra/scripts/funcs/overlay_0977_slotA_801d0f60.txt`.

The score table's shape follows from that expression: three courses of sixteen
`i32` cells, at file offset `0x3048` of the raw `0977` entry
(`0x801D1860 - 0x801CE818`), reachable with the same fixed-offset read
`engine-ui::other_game_hud::parse_sprite_table` already performs on that entry.
Parser `engine-core::muscle_dome::parse_score_table`.

A course's row summed is what a cleared run banks - each cell exactly once,
the non-final legs through the tally screen and the last one at settlement.
That is the join the curated `casino.toml` `reward_coins` column belongs to:
Beginner 818 and Expert 1532 match the disc exactly, and the Master course's
row sums to **13830**, correcting the 13856 the walkthrough table carried.

### The arena's per-frame voice cue (`FUN_801D1288`)

The same overlay keys one SPU voice per frame, rotating over `0x10 ..= 0x13` on
the free-running counter `DAT_801D1AE4 & 3`. Its call is
`FUN_80065034(voice, 0, 0, 1, 0x3C, 0x40, vol, vol)`, and that eight-argument
shape is pinned by the SCUS cue drainer `FUN_80016B6C`, which fills the same
slots from a cue descriptor: `(voice, level, program, tone, note, 0x40, vol_l,
vol_r)`. So the arena cue is program `0`, tone `1`, note `0x3C`, at level `0`.

Both volume slots are `(_DAT_80084580 << 0xf) >> 0x10` - the **voice/SFX volume
config**, which the cold reset `FUN_8001FFA4` seeds to `200`, so a freshly
booted game keys it at `100` per channel. Reading that pair as a *position*
derived from a party-block word is **falsified**: `FUN_80016B6C` passes the
identical expression into the identical two argument slots for every ordinary
SFX cue in the game, and the dance overlay's own direct key-on `FUN_801D3D78`
does the same. Port: `engine-core::other_game_overlay::cue_volume`.

The whole contest runs on a shared context block at `_DAT_8007bd24` (referred to below as **ctx**). The fighters are ordinary battle actors reached through the global actor pointer table `&DAT_801c9370` (the same table the main battle system uses), so an entered direction ultimately resolves through the battle action machinery against actor records.

## Arena backdrop (extraction 1225)

The contest's 3D environment is an ordinary **battle backdrop** in the standard
carrier shape ([`battle.md` § Battle background](battle.md#battle-background)),
resident in the dome's own data file:

- The `0977` door/init overlay loads the dome's data file by its dev path
  `data\field\other6.lzs` - a string literal in the entry's own pool
  (extraction `0977_other_game`, alongside its `mini_battle_flag %d` /
  `round %d level %d` traces and the arena's monster-name roster).
  CDNAME maps `other6` to raw TOC index **1222**, i.e. extraction block
  **1220..=1225** (`legaia_prot::cdname::block_for_extraction_index`, the +2
  filename skew - [`cdname.md`](../formats/cdname.md#numbering-space)).
- The block's tail slot, extraction **1225**, is its only `scene_tmd_stream` -
  the battle-backdrop carrier the battle init walker `FUN_8001FE70` records
  into `_DAT_8007B864`: a leading arena-shell TMD (2 objects, 367 verts;
  object 0 = the ring shell, authored at `X >= 0` with the open side facing
  `-X` - the same half-stage authoring rule as `town01`'s dome) followed by
  two type-`0x01` TIM chunks (`0x8220` bytes each, the town01-dome chunk
  shape): 4bpp 256x256 pages at framebuffer `(768, 0)` / `(832, 0)` with CLUT
  rows **473** / **479**.
- `(832, 0)` through CLUT `(0, 479)` is exactly the constant address the
  battle **ground grid** `func_0x801d02c0` samples, and that page's
  `(192..255)^2` window is the dome's plain dirt tile; the rest of the two
  pages is the arena furniture (chain-link fence, dirt/stone flooring, the
  tiered ring wall).
- The stream carries two **semi-transparent prim sets** (ABE set, ABR mode
  1 = additive), in two different TMD objects. The shell (object 0) owns
  the lamp-glow quads (mode `0x3F`, page `(768, 0)` window
  `(48..109, 161..251)`, CLUT x 112 of row 473). **Object 1 is a separate
  12-quad dust-decal object** (mode `0x2F`, page `(832, 0)` window
  `(128..190, 192..253)`, CLUT x 16 of row 479) ringing the wall base.
- **The dust decal is not part of the live match's visible backdrop.** Its
  texels are genuinely bright - the CLUT ramp at `(16, 479)` climbs to
  whitish `(208, 208, 248)`, window average luminance ~70/255 - so *any*
  draw of it is conspicuous: opaque it reads as a solid dark "mist" band,
  and even a correct ABR-1 additive draw reads as a white cloud band. The
  retail match capture shows a mist-free interior (fence, wooden wall,
  light-grey floor; capture: the `minigame_muscle_dome_pcsx` scenario run
  forward into the live match). The lamp glows stay with the shell and blend
  additively.

  **The path that would draw it, and the gate that does not.** Object 1 is
  dropped by the battle scene loader itself, one object at a time, and the
  rule is not dome-specific - see
  [Object 1 is trimmed by the loader](#object-1-is-trimmed-by-the-loader-_dat_8007b64b).

Confidence: the load chain, carrier shape and texture address are **Confirmed**
(disassembly + structural decode of the entry); that a live contest's
`_DAT_8007B864` holds this stream is **Inferred** - the dome runs as a battle
with the battle overlay resident and 1225 is the only backdrop-shaped stream in
the file its init loads, but no dome-battle save-state byte-match has been
taken. The file's other slots: 1220 = an LZS container whose section 0 is
the dome's **hub UI art** - two plain TIMs uploading the pages at
`(320, 0)` / `(320, 256)` with CLUT rows 502/503 (the Welcome / INTERVAL /
ROUND / course-name strips; see
[HUD chrome](#hud-chrome-texture-sources-capture-pinned)),
1221/1222 = the two ringside panel stills
([below](#inttim--int2tim---the-ringside-panel-stills)), 1223/1224 = pochi
fillers ([`pochi.md`](../formats/pochi.md)).

### `int.tim` / `int2.tim` - the ringside panel stills

Extraction **1221** and **1222** are not blobs. Each is exactly `0x28000`
bytes, which is `320 * 256 * 2` to the byte, and each is a **16-bit BGR555
still** that the runtime uploads as one VRAM rectangle at `(384, 0)`.

The overlay that reads them is PROT **0978** (`field_back_read`, slot-B base
`0x801F69D8`), whose own string pool names both files by their dev paths -
`h:\prot\field\other6\tim\int.tim` at file `+0x20` and
`…\int2.tim` at `+0x44` - putting them in this bundle's directory. Its
streamer `FUN_801F6B24` (file `+0x14C`) is a 12-arm phase machine on a jump
table at `0x801F6AA8`, counter `_DAT_8007B6C8`.

Nothing materialises the raw TOC index as a literal, which is why a
literal-only sweep finds no loader. The index is **computed**:

```text
801f6b90  lhu   v0,0x4824(v0)      ; party slot 0 hp_max_record  (+0x11C)
801f6b98  lhu   v1,0x480e(v1)      ; party slot 0 hp_curr_live   (+0x106)
801f6ba4  srl   v0,v0,1
801f6bac  sltu  s0,v1,v0           ; s0 = current HP < max/2
...
801f6c3c  addiu a0,s0,0x4c7        ; raw TOC 0x4C7 + s0
801f6c40  jal   0x8003e8a8         ; LBA resolver
```

Raw `0x4C7` / `0x4C8` are extraction **1221** / **1222** under the
[+2 correction](../formats/cdname.md#numbering-space), so `int.tim` is the
default and `int2.tim` is the **below-half-HP** variant, chosen from the lead
character's live record ([`save-record.md`](../formats/save-record.md)). The
same `s0` selects between the two dev path strings on the dev branch
(`lh` on `_DAT_8007B8C2` at `0x801F6C38`), which is what ties each index to
its filename. Four such `addiu a0,s0,0x4c7` sites exist, at file `+0x264`,
`+0x2F4`, `+0x398` and `+0x440` - one per strip.

The upload is four passes over the same rect. `FUN_8003E964` seeks sector
`0 / 0x14 / 0x28 / 0x3C` of the entry, `FUN_8003E800` reads 20 sectors
(`0xA000` = `320 * 64 * 2`) into the staging buffer, and `FUN_800583C8`
(`LoadImage`) uploads it with the rect's `y` set to `0 / 0x40 / 0x80 / 0xC0`
in the `jal` delay slot. The rect itself lives at `0x801F735C` and its other
three fields are written once, at `0x801F6BE4` / `0x801F6BF0` / `0x801F6BFC`:
`x = 0x180` (384), `w = 0x140` (320), `h = 0x40` (64).

The files' own layout matches the display window. The first five sectors of
both are the halfword `0x5862` repeated - 5,120 halfwords, i.e. exactly
**16 scanlines of 320** - then one zero sector, then pixels. So the 240-line
picture occupies rows 16..255 of the 256-row rectangle, and the constant fill
is a deliberate top pad rather than dev filler.

Both stills are finished pre-rendered scenes of characters at the ring's
rope-and-mesh fence, and the `int2` variant differs by their reaction, which is
consistent with the HP test that selects it. **Confirmed** (disassembly) for
the loader, the index arithmetic, the variant selector and the upload geometry.
What the panel is used for on screen is still **Inferred**, and capture has
now ruled out the obvious candidate: the between-match `INTERVAL` / `ROUND`
screens show a ringside scene that looks exactly like the stills, but they are
a **live `koin1` render**, not the rect - see
[what a live teardown shows](#what-a-live-teardown-shows).

#### What arms the load

`ctx[+0xC]` is a battle-teardown state byte, and the SCUS post-battle routine
`FUN_8004E568` drives the last two of its three values from two places
(`0x8004E670..0x8004E6E4` and `0x8004F840..0x8004F8B0`, the same block twice):

| `ctx[+0xC]` | What runs |
|---|---|
| `1` | Free the four enemy record buffers `0x801C9348[0..3]` (each gated on `ctx[+0x02 + i] != 0`) through `FUN_80017B94`, free the side-band stream buffer `*0x8007BD74`, then write `2`. |
| `2` | Tick `FUN_80025358` once a frame, storing its "still loading" return in `ctx[+0xB]`. That is the staged load of PROT 0978, whose `FUN_801F6B24` streams the still into the space the `1` arm just freed. |

The arm itself is neither of those. Sweeping `SCUS_942.54` and every statically
based overlay image for the `li rX, 1` / `sb rX, 0xC(rY)` pair returns exactly
one site, `0x800474CC` - the **per-frame battle anim-node tick**
`FUN_80047430`, which writes `ctx[+0xC] = 1` for an enemy node only (`node[+0x5A] >= 3`) and under two further
gates - `gp[+0xA48] & 0x80` set, and the per-battle enemy id byte `gp[+0x9F4]`
not `0xB5`. The same two instructions also set the node's early-return flag
(`node[+0x10] |= 8`), so the enemy stops ticking in the same frame the load is
armed. `gp[+0xA48] |= 0x80` comes from the battle-end spoils path at
`0x8004EDE0` (`FUN_8004E568`) and from a pad-gated branch at `0x80046D98`
(`FUN_80046A20`, under `_DAT_8007B98C != 0` and pad mask `0x100`).

So the load is a **battle-teardown** step: the still is streamed into slot B
after the enemy records are freed, on either battle-end path. The
`ctx[+0x7] == 0x67` gate on the first of the two blocks is not a separate
condition on the streamer - it is the **escape** path's entry into the same
shared teardown. `0x67` is written in one place: case `0x66` of the
battle-action SM `FUN_801E295C` (`0x801E5A84`), the successful-escape teardown,
which spawns the fade template at `DAT_801C9070` and raises the battle-end
signal `DAT_8007BD71 = 0xFE`. `0x67` has no case body of its own
([`battle-action.md`](battle-action.md)), so it is a terminal hold meaning "the
party ran and the fade is playing". The second block
(`0x8004F7A4..0x8004F8B0`) is the ordinary victory tail, reached under
`gp[+0xA54] >= 0x100`, `ctx[+0xB] == 0` and `ctx[+0x6CE] == 1`; a third tick
site sits in the battle side-band pass `FUN_80056208` at `0x80056428` (stage
`_DAT_8007B64A == 1`, phase `ctx[+0x289] == 3`).

**What is still open is only the draw.** No image on the disc materialises
`384` as a primitive or display coordinate outside the two loaders themselves:
sweeping every statically based overlay image plus `SCUS_942.54` for
`addiu rX, zero, 0x180` / `ori rX, rX, 0x180` yields 33 sites, and the only
ones that pair it with `(384, 0)` are `FUN_801F6B24`'s own two rect writes
(`0x801F6BE4`, `0x801F6FC8`), the dev VRAM round-trip utility `FUN_8001E890`
(which `StoreImage`s `(384, 0)` 256x256 back out in four `0x8000`-byte strips),
and the battle scene loader `FUN_800542C8` at `0x80054928`, whose rect is
`(384, **256**)` - a different page.

Read with `disasm-overlay-fn.py extracted/overlays/overlay_field_back_read_0978.bin
--base 0x801F69D8 --addr 0x801F6B24`.

#### What a live teardown shows

Capture reproduces the arming chain exactly and adds the one figure the static
read could not supply - **which** entry the `ctx[+0xC] == 2` arm stages. Probe
`scripts/pcsx-redux/autorun_battle_teardown_hook.lua`, run on an ordinary
self-resolving fight (`rim_elm_gimard_victory`) and on a Muscle Dome match:

| Observation | Ordinary victory | Dome match (escaped) |
|---|---|---|
| `0x800474CC` arm hits | 1, `ra = 0x800252BC` | 1, same `ra` |
| `gp[+0xA48]` | `0x00` -> `0x80` on the arm frame | already `0x80` from the hub |
| `ctx[+0xC]` | `1` next frame, `2` two frames later | same shape |
| loader-B tracker `0x8007BC4C` | `-1` -> **83** four frames after the arm | `-1` -> **83** |

The tracker holds `extraction - 895`, so `83` is **PROT 0978** - the load is
byte-pinned live, on both battle-end paths, and `ra = 0x800252BC` is the
actor-list tick iterator the settled `FUN_80047430` caller finding names.

#### The teardown streams the *other* family - `0x36C`, not `0x4C7`

PROT 0978 carries **two** streaming families behind its 12-arm phase machine,
and they differ in every field that matters. Each has its own rect
initialiser writing the same `RECT` at `0x801F735C`:

| | panel-still family | field-restore family |
|---|---|---|
| rect init | `0x801F6BE4..0x801F6C20` | `0x801F6FC8..0x801F6FF0` |
| rect | `x = 0x180`, `w = 0x140`, `h = 0x40` | `x = 0x180`, `w = 0x40`, `h = 0x100` |
| stepped field | `y` `+= 0x40` (`0x801F6D2C`) | `x` `+= 0x40` (`0x801F70F8`) |
| bytes per strip | `0xA000` | `0x8000` |
| raw TOC index | `0x4C7` / `0x4C8` (`int.tim` / `int2.tim`) | `0x36C` |

An exec-breakpoint census of the libgpu entry points across the ordinary
victory (`rim_elm_gimard_victory`, probe
`scripts/pcsx-redux/autorun_gpu_call_census.lua`) catches the teardown load
in the act, and it is the **second** family. Four `LoadImage` calls, from the
four `jal 0x800583C8` sites at `0x801F7078` / `0x801F7108` / `0x801F7190` /
`0x801F7224`, upload `(384, 0)`, `(448, 0)`, `(512, 0)`, `(576, 0)`, each
`64 x 256` - `0x8000` bytes apiece, `0x20000` in all - on the four vsyncs
after the tracker flips to `83`. Sixty-four by two hundred fifty-six at those
origins is not a picture rect: it is **PSX texture pages 6, 7, 8 and 9**.

Raw `0x36C` is extraction **874** under the
[+2 correction](../formats/cdname.md#numbering-space) - the head of the
`player_data` block, whose four `0x8000` chunks the arm re-seeks at sectors
`0`, `0x10`, `0x20`, `0x30`. So the entry's own strings are literal: the
battle-teardown arm is the **`FIELD BACK READ NOW`** path, restoring the
field party's texture pages over the VRAM the battle borrowed. The panel
stills are a different arm of the same machine and did not run here.

This corrects the upload geometry recorded above for the teardown: the four
passes are vertical `64 x 256` strips stepping `x`, not horizontal `320 x 64`
bands stepping `y`. The `320 x 64` reading is the *panel-still* family's, and
it stands for that family.

#### The `INTERVAL` screen is a live render, not the still

The `INTERVAL` scoreboard and the `ROUND 2` card sit over a ringside scene that
matches the stills' decoded content closely enough to look like the answer. It
is not. Walking the frame's **real ordering table** - `mednafen-state
display-list` over the main-RAM dump each checkpoint of
`autorun_muscle_hud_capture.lua` writes - decodes 1822 packets of live `koin1`
geometry (971 `POLY_GT4`, 406 `POLY_GT3`) across 22 texture families, and none
of them sits on a page whose x origin is `384`.

The same sweep does find the `(384, 0)` page being sampled, but **only before
the first match**: every checkpoint from the dome hub through the first battle
carries one family with `tpage 0x0006` / `clut 0x7702` - 27 packets covering
screen `x[25,151] y[91,101]`, a ~127x11 label strip, not a 320x256 backdrop.
From the first battle-end onwards - the point at which PROT 0978 streams the
still over that page - no checkpoint's ordering table carries a family there at
all.

**What is still open is the emit**, and three channels are now ruled out over
roughly 14,000 vsyncs spanning an ordinary battle end, two dome matches and
their intervals: no `DISPENV`-shaped rect at `(384, 0)` with a screen-sized
`w`/`h`; no libgpu blit sourcing `(384, 0)` (GP0 `0x80..0x83` with the command
word's low 24 bits zero and a transferable size - the loose form of that test
reports a false hit for every `0x80`-prefixed KSEG0 pointer that happens to
precede the constant `0x180`, which is why the filter lives in the scanner);
and no ordering-table family on the page after the still lands.

Two method notes the negatives depend on. A **periodic RAM sweep cannot see a
one-shot** - a blit that runs on the frame a screen opens is gone before the
next sample, and a miss reads exactly like an absence, which is why the probe
takes `LEGAIA_SCAN_MODES` to sweep every vsync inside named modes. And a
**two-byte tpage needle is not separable from data** even after a
screen-coordinate filter, so RAM-scanned `(384, 0)` tpage candidates are not
evidence of a draw; the ordering-table walk is what carries weight here.

#### A call-site census closes the sampling question

The way past both method limits is to watch the libgpu **entry points**
rather than RAM, because a one-shot cannot fall between two samples that do
not exist. `autorun_gpu_call_census.lua` breakpoints `LoadImage`
(`0x800583C8`), `StoreImage` (`0x8005842C`), `MoveImage` (`0x80058490`),
`PutDispEnv` (`0x800589D0`), the GP1 command issue (`0x8005A094`) and the
direct GP0 FIFO write (`0x8005A0D0`), and reads each call's `RECT` / command
word as it is made. Over 1800 vsyncs spanning a battle, its teardown and the
return to field:

| Call | Count | At `x = 384` |
|---|---|---|
| `PutDispEnv` / GP1 `0x05` display start | 641 / 641 | **0** |
| `MoveImage` | 632 | **0** |
| `LoadImage` | 214 | 4 - the teardown's texture-page strips |
| `StoreImage` | 5 | 4 - the dev round-trip `FUN_8001E890` reading it back |
| direct GP0 list carrying `0x80` / `0xA0` / `0xC0` | 0 | 0 |

No display origin is ever moved to the page, nothing blits out of it, and
the only writers are the teardown upload and a dev readback. Together with
the ordering-table result above, that answers the sampling question the way
the geometry already suggested: `(384, 0)` is not a still's display rect on
this path but four ordinary texture pages, sampled by ordinary
tpage-addressed primitives - the `tpage 0x0006` family the sweep already
found. What remains open is only the **panel-still family's** own consumer,
on whatever arm loads `int.tim` / `int2.tim`.

Where to look next is in the entry's own strings: PROT 0978 opens with
`f_read %d size %d KB` and **`FIELD BACK READ NOW`**, which reads as a
background-read cover rather than an arena screen.

### Object 1 is trimmed by the loader (`_DAT_8007B64B`)

Nothing in the arena's own code touches the backdrop. `_DAT_8007B864` - the
pointer `FUN_8001FE70` writes at `0x8001FEC0` after copying the stream's leading
TMD chunk out to its own buffer - has exactly **two** references on the whole
disc, and the second is the SCUS battle scene loader `FUN_800513F0` reading it
at `0x80051A5C`. The arena-init overlay 0977 and the battle-action overlay 0898
reference it in no form (word, `lui`+load, `jal`, `j`, branch, `gp`-relative).

What `FUN_800513F0` does with it, in order:

1. `FUN_80026B4C(_DAT_8007B864, 0)` at `0x80051A60` - magic-checks `0x80000002`,
   relocates **every** object via `FUN_800268DC`, and registers the whole TMD
   into the shared model bank `0x8007C018[n++]` (`n` at `0x8007B774`).
2. Writes the returned slot index into the backdrop template `0x8007680C+4`
   (`0x80051A80`) and spawns **two** backdrop actors from it
   (`FUN_80020DE0` at `0x80051A7C` / `0x80051AA8`), parked at battle-ctx
   `+0x106C` and `+0x1070`. The spawn copies `template+4` into `actor+0x64`
   (`0x80020E70`), and `FUN_80021B04` binds every object of that model into the
   actor's part array `actor+0x44` - count at word `0`, one descriptor per slot
   after it.
3. Tests the byte `_DAT_8007B64B` at `0x80051ABC`. **When it is zero** - and only
   then - it decrements both actors' part counts (`0x80051AD4..0x80051B10`) and
   then shifts each list down one slot from index `1`
   (`0x80051B14..0x80051BAC`). The net effect for any object count is the same:
   **object index 1 is removed from the draw list.** A non-zero byte skips both
   loops and every object stays.

`_DAT_8007B64B` has **one writer on the disc**: the field overlay's battle
handoff `FUN_801D9E1C` at `0x801DA0AC`, `= (s2[+8] >> 5) & 1` - bit 5 of the
handoff's per-encounter setup byte, written only on the
`(*_DAT_801C6EA4)[+0x5F] >= 0xC` arm. It is one of three options packed into
that byte: bit 7 clears the render flag `0x00100000` on the actor's `+0x80`
(`0x801DA0B8`) and bit 6 takes a third arm (`0x801DA0D8`). (The
field-battle-intro overlay 0979 reads the byte too, at `0x801CF700`; SCUS reads
it a second time at `0x80046D34` as `gp+0x333`.)

So the "candidate: a phase-gated effect draw" guess is **falsified**. There is no
effect path for object 1 at all. It is ordinary backdrop geometry that the
loader trims by default, and it draws only for an encounter whose record sets
bit 5 - a per-battle background option, not a dome feature.

**The arena really does leave the byte clear** - measured, not inferred.
`scripts/pcsx-redux/autorun_w4d_dome_decal_flag.lua` drives the dome contest
(`koin1`) from its load-transition state through the mode chain
`0x03 -> 0x18 -> 0x19 -> 0x14 -> 0x15` and breakpoints the consumer. The battle
scene loader `FUN_800513F0` is entered once, from `ra = 0x80046F7C`, with
`_DAT_8007B64B` reading `0x00`; at the test `0x80051ACC` the loaded byte is `0x00`,
so the loader takes the fall-through arm and **trims object index 1** from both
backdrop actors. A width-1 write watch on the byte logs **zero** writes across the
whole capture - the field handoff `FUN_801D9E1C` never runs on the arena's path, so
nothing sets bit 5 and the dust decal is not drawn. The mist-free frame capture and
the site panel's omission of the decal both agree with the mechanism.

Site consumer: the minigames page's dome panel draws the shell + the retail
ground grid through `legaia_web_viewer` (`muscle_arena_*` / `muscle_vram`),
with the shell's ABE lamp glows routed through the renderer's two-pass PSX
blend (`site/js/minigame-muscle.js`, `semiTwoPass`) and the object-1 dust
decal omitted per the capture above (`muscle_arena_hybrid` filters it).

The shell, the assembled fighter and the monster all upload their prims'
baked **packet colour** on the `a_flat_rgba` attribute, because the page
shades the retail way - `texel * colour / 128`, no light source. The dome's
own packet colours run well under the neutral `0x80` (much of the shell sits
near `0x60`), so this is the difference between a lit interior and a washed
one. Uploading white instead does not read as *unlit*, it reads as
**over-lit** at `texel * 255/128`, which is why nothing about the frame
points at a dropped colour stream; `legaia_web_viewer::packet_color` is the
one place the two halves (textured = modulation off the mesh, untextured =
fill off the shading) get split correctly.

## Retail presentation

Retail presents the contest as a **standard battle** - the normal Legaia
battle chrome with the course restrictions applied - not a bespoke card UI.
Capture evidence: retail captures of the contest entry, the command menu and
an art playing out.

- **Intro card.** Contest entry opens on a pure black frame with a single
  centred line of white cursive script, soft blue-white glow:
  "Welcome to the Muscle Dome!". Then the fight begins.
- **Fighter roster.** The player fields Vahn, Noa or Gala in their normal
  **assembled battle form** (fighter form, [`character-mesh.md`](../formats/character-mesh.md)) -
  not the PROT 1204 Baka form.
- **Command menu.** The standard battle command cluster: two bevelled gold
  chips top-left ("Begin" and the fighter's name); on the right the **Item**
  chip **crossed out with a red X** (the course forbids items), below it
  "Attack" (left) + a grey D-pad glyph + the character's **Ra-Seru name**
  ("Meta" for Vahn - the magic command) + "Spirit"; chips are blue-marble
  plates with gold borders. Bottom: the pointed blue status plate (fighter
  name, gold "HP" `cur/max`, teal "MP" `cur/max`) with the pointed **AP**
  plate above-right (red "AP" label, orange gauge, remaining-points
  numeral). Which chips are live is not a course table: it is the
  special-battle word's restriction bits and the fighter's own status /
  Ra-Seru gate, and the Ra-Seru chip's bit is never raised by a dome round -
  see [What makes a Ra-Seru chip render](#what-makes-a-ra-seru-chip-render).
  The curated course rules (`data/gamedata/casino.toml`: no equipment, no
  items on every course, magic forbidden on Master) are walkthrough labels;
  the *magic* half of them has no writer on the disc.
- **Arts banner.** A committed directional sequence that performs a
  Tactical Art raises the art-class banner during playback - block-capital
  orange-gradient text with a dark outline ("HYPER ARTS!!") over white
  radial speed-line rays, the attacker's gold name chip top-left and the
  defender's blue name chip bottom-right. The commit path appends raw
  direction ids `0xC..=0xF` into `actor+0x1df`, so the recognition happens
  on the battle-action side as it does for a normal battle's input string.
- **The enemy's HP is *not* shown here, and there is no mist.** A normal
  Legaia battle never draws the enemy's HP, and the dome is no exception:
  the `Turns Left / HP Left` strip that would print it is gated on formation
  slot 0 `== 0xB6`, which no dome round can satisfy
  ([The four-turn strip belongs to Koru](#the-four-turn-strip-belongs-to-koru-not-the-dome)).
  The earlier reading of this bullet - that the dome is the one battle that
  shows it, "which is what makes a timed-out leg scoreable" - is **falsified
  twice over**: the strip is Koru's, and a dome leg has no timeout to score
  ([What ends a leg](#what-ends-a-leg-a-knockout-and-nothing-else)). The
  per-fighter status plate is the usual one (no enemy plate). The arena
  interior is mist-free (see
  [Arena backdrop](#arena-backdrop-extraction-1225) for the ABE-prim defect
  that fakes a mist band).

Site consumer: the minigames page's dome panel mirrors this presentation -
the intro card, the command cluster with Item crossed out, the AP/status
plates and the arts banner - with the fighter body from the assembled
battle form (`muscle_fighter_*`), the chrome drawn from the disc sources
below (`muscle_hud_json` / `muscle_hud_sheet_rgba`), and the queue -> art
resolution done against the SCUS arts-name table's combo strings
(`muscle_round_arts_json`, kind labels joined from the curated gamedata
arts table). The **Ra-Seru (magic) chip** is a real command class on both
hosts: `MuscleDomeSession` carries the ring gates, the learned-Seru list
priced by the ability-bit discount, the MP gate at the pick and the debit at
the play-out, and the cast resolves through the same
`engine-core::spells::cast_spell` rule the regular battle's cast band folds
with. Neither host crosses the chip out any more, because retail does not:
its bit is never raised by a dome round
([the gates](#what-makes-a-ra-seru-chip-render)). The sibling gap - "the
ported rules resolve each queued command as a basic strike" - is closed:
see [the queue the dome resolves](#the-queue-the-dome-resolves-is-the-tokenizers).

The two hosts reach the chip from different buttons, and that is the one
disclosed difference: retail opens the list from the ring's **Right** chip,
which the port's collapsed selection cannot spare (the four directions are
the input screen's), so the browser minigames page - which keeps a ring
screen - binds Right, and the native `play-window` binds **Triangle**. Both
land in `MuscleDomeSession::select_input`, so the rules are one.

## HUD chrome texture sources (capture-pinned)

The dome match's entire on-screen chrome resolves to five disc sources, and
its screen geometry to one SCUS-static table. Provenance: a live PCSX-Redux
dome battle (the `minigame_muscle_dome_pcsx` scenario driven forward with a
scripted pad, capture tool
`scripts/pcsx-redux/autorun_muscle_hud_capture.lua`) snapshotted at the
command cluster, an enemy art, and a player HYPER ARTS!! playback; the GP0
packet stream was read out of the live prim arena and every texture page was
byte-matched between the snapshot VRAM and the disc bytes.

**Layout.** The screen-element placement table at SCUS `0x80076C10`
(24-byte stride,
80 records, initialised data in `SCUS_942.54` at file `0x67410`) carries per
element: two sprite/style selector bytes (`+0`/`+1`), two screen anchors
`(x, y)` at `+2`/`+4` and `+0xA`/`+0xC` (the glide endpoints the
`FUN_801db7b0` slide moves between), width/height at `+6`/`+8`, per-variant
style bytes at `+0xE`/`+0xF`, a kind byte at `+0x10` and a text pointer at
`+0x14` (rewired at runtime - `FUN_801d8de8`'s labelled cases write it).
Confirmed anchors: element 8 = the Item chip arriving at `(204, 34)`,
9 = Attack at `(160, 66)`, `0xA` = the Ra-Seru chip at `(248, 66)`,
`0xB` = Spirit at `(204, 98)`, 0..5 = the Begin / Run centre-menu chips,
7 / `0x34` = the 288-wide status plate at `(16, 236 -> 194)`,
`0x29`/`0x2A` = the opponent name chip at `(200, 162)`. Sprites emit
through the SCUS text-actor pipeline (`FUN_8003541C`), not the battle
overlay.

**Textures.** Per element family, `(page, uv)` from the captured packets,
byte-matched to its disc source:

| Chrome | Page / CLUT | Piece rects (texels) | Disc source |
|---|---|---|---|
| Chip / plate 3-slice art | `(896,256)`; CLUT row 511 sub-pal 4 (blue) / 12 (gold) | caps `(208,v)`/`(216,v)` 8×20, body `(192,v)` 16×20; blue `v=0`, gold `v=64` | boot-gap TIM `PROT.DAT 0x18E0` ([`boot.md`](boot.md#pre-init_data-system-ui-gap-menu-glyph-atlas--boot-cursors)) |
| D-pad glyph | `(896,256)`; sub-pal 7 | `(0,112)` 16×16, drawn 15×15 between Attack and the Ra-Seru chip | same TIM |
| AP plate | `(896,256)`; sub-pals 4 + 1 | label `(128,64)` 24×16, trough `(128,80)` 56×16, end box `(176,64)` 16×16, cap `(184,80)` 8×16; drawn at `(208..312, 172)`. The end box's value numeral at a full gauge is the baked "100" tile `(64,136)` 16×6 (sub-pal 1). The meter itself is **not on this sheet** - see "The meter is not a tile" below | same TIM |
| Status plate row | `(896,256)`; sub-pals 4 / 1 / 5 | plate slices at `y=188`, HP badge `(208,86)` 16×10 at `(80,194)`, MP badge `(224,86)` at `(192,194)`, `/` separator `(96,64)` 8×16 | same TIM |
| Chip / caption text | `(896,0)`; menu-atlas bank sub-pal 13 = CLUT `(208,510)` | 16×16 cells drawn 14×15; cell = ASCII − 0x20, column-major 16/row; pen advance = glyph texel width (`i`/`m`/`M` +1, space 5 - capture-measured) | boot-gap ASCII font TIM `PROT.DAT 0x7F40` |
| Small digits | `(960,256)`; sub-pal 13 | `u = digit*8`, `v=208`, 8×12 | menu-glyph atlas `PROT.DAT 0x11218` |
| Red cross-out X | `(448,0)`; CLUT row 476 sub-pal 4 | `(0,96)` 64×16 drawn over the forbidden chip (`(196,30)` for Item) | `etim` (extraction 0870) third TIM at file `+0x10450` |
| Arts banner words | `(448,0)`; sub-pal 3 | SUPER `(3,152)` 105×24, HYPER ARTS!! full row `(0,176)` 216×24, MIRACLE `(0,200)` 127×24, NEW `(132,200)` 64×24; the pinned draw: two FT4s covering `(52,144)-(268,178)` - 1:1 wide, 24 texels stretched to 34 px | same `etim` TIM |
| Damage numerals + words | `(448,0)`; sub-pal 3 | digits 24×24 cells at `v=64`, `u=(d−1)*24`, `0` at `u=216`; DAMAGE `(0,224)` 52×14, HIT `(0,240)` 32×16, TOTAL `(32,240)` 48×16; hit numbers drawn 24×23, tally row 16×15 | same `etim` TIM |
| "Welcome to the Muscle Dome!" / INTERVAL / ROUND / hub digits | `(320,0)` + `(320,256)`; CLUT rows 502/503 | geometry = the PROT 0977 sprite descriptor table at VA `0x801D170C` (file `+0x2EF4`, 17 × `0x14`-byte records; parser `legaia_engine_ui::other_game_hud::parse_sprite_table`): record 3 = the Welcome strip `(0,224)` 240×18, 16 = INTERVAL `(0,192)` 192×32, 0 = ROUND `(0,0)` 144×32, 1 = the 24×32 hub digit strip | extraction **1220** (`other6.lzs` slot 0): LZS section 0 = `[12-byte header][TIM -> (320,0), CLUT row 502][TIM -> (320,256), CLUT row 503]`, byte-identical to the live course-menu VRAM |

The CLUT-bank packing rule the capture pinned: a gap TIM's 16-row CLUT
block uploads **packed into one VRAM row** as 16 side-by-side sub-palettes
(widget bank -> row 511, menu-atlas bank -> row 510), which is what the
packets' CLUT words (`0x7FC4` = `(64,511)`, `0x7FCC` = `(192,511)`,
`0x7F8D` = `(208,510)`, ...) address.

### The meter is not a tile

The AP plate's orange meter has **no source rect**, on this sheet or any
other. Retail draws it as two untextured 3-px gouraud strips - the same
`FUN_8002C0B0` fill the status screen's AP gauge uses, dark `(0x80,0x20,0x10)`
to gold `(0xC0,0xA0,0x40)` and back, over the span pinned in the arts-input
table below. Only the *value* is art: the 6-px digit strip has no 3-digit
seat, so the sheet carries one baked "100" tile for the end box.

That leaves a trap in the sheet's neighbourhood, because the "100" tile is
16×6 - exactly a plausible fill-tile shape, sitting two rows under the digit
strip. Read as a fill and stretched across the trough, it draws a wide "100"
where the meter belongs and leaves the value box blank, which is what the
browser dome page did. The plate is one widget with two callers (command
menu, direction entry); both take their fill from
`legaia_engine_ui::arts_input`'s span + gouraud endpoints, so neither can
re-acquire a tile.

## Arts command input (packet-pinned)

The dome's Attack command runs the **standard battle arts input** verbatim -
the same `FUN_801D0748` state `0x50` gauge-input arm and `FUN_801D388C`
case-`9`/`0xB` accounting [`arts-command-gauge.md`](arts-command-gauge.md)
documents. This section pins the *presentation*: what the input screen and
its Triangle arts list draw, from where. Provenance: a live dome match in
the static recomp (savestate + scripted pad over the debug TCP server,
[`recomp-differential.md`](../tooling/recomp-differential.md)), read out
with the runtime's `gpu_frame_dump` per-frame GP0 packet ring - every rect,
palette and screen seat below is byte-read from captured SPRT / FT4 /
shaded-quad words, cross-checked against a full-VRAM dump of the same
moment.

**Flow** (phase byte `ctx+6`, captured transitions): command cluster
(`0x28`) -> Attack opens an **Auto | Command** pick (`0x78`, chips at the
Attack / Ra-Seru element anchors) -> Command opens the input screen
(`0x50`). Directions append commands (each press debits `ctx+0x6dc` by the
command's `+0x74` cost and appends to `actor+0x1df` - RAM-verified per
press); entry **ends by itself** the moment no command is affordable
(`0x50 -> 0x5a` on the exhausting press, no confirm). `0x5a` reviews the
committed bar; any press reaches the **Begin | Reselect** menu (`0x6e`);
Begin plays the round out, Reselect returns to a clean input. The previous
round's pennants persist in the bar when the input reopens and clear on
the first fresh press. **Triangle** cycles the learned-arts list: closed ->
page 1 -> ... -> last page -> closed; it is inert when the character's
learned-art constant ([`art-data.md`](../formats/art-data.md#learned-art-constant))
names no art. The right-hand AP plate reads the **Spirit gauge**
(`actor+0x170`) and never moves during entry - the input budget's visible
form is the bar itself filling with pennants.

**Input screen pieces.** All from the boot-gap widget TIM's page
(`(896,256)`; sub-palette = row-511 CLUT x/16) unless noted:

| Piece | Sub-pal | Rects (texels) | Screen seats |
|---|---|---|---|
| Direction chip | 6 | body `(215,96)` 24x26, caps `(200,96)`/`(239,96)` 15x26 | body anchors: High `(216,26)`, Left `(176,58)`, Right `(256,58)`, Low `(216,90)`; caps at body -15 / +24 |
| Chip label strip | 5 | `u=104` 24x18; `v`: Left 20, Low 40, Right 84, High 104 (Arms 0, RaSeru 64 sheet-read) | FT4 at body `+ (0,4)` |
| Diamond ends | 5 | `(192,24)` / `(204,24)` 9x18 | body -9 / +24, `y+4` |
| D-pad glyph | 7 | `(0,112)` 16x16 | FT4 `(220,62)`-`(235,77)` |
| Input bar | 6 | left end `(240,0)` 16x18, body tile `(224,0)` 16x18, arrow end `(192,44)` 18x18 | y=188, x `0..128` at a 100-AP pool |
| Command pennant | 5 | caps `(192,24)` / `(216,24)` 9x18 + the label strip between | slot `n` at x = 16 + spent-AP-before, width `cost - 6`, y 192 - see [the cost law](#the-pennant-geometry-is-linear-in-the-commands-ap-cost) |
| AP plate | 4 | the pinned label/trough/end/cap pieces | `(208,172)`; fill = two 3-px **gouraud strips** x `235..285`, y `177..183`, RGB `(128,32,16)` dark <-> `(192,160,64)` orange (dark-orange-dark sheen) |
| Triangle caption | own TIM | green Triangle circle: the 64x32 button-glyph gap TIM at `PROT.DAT 0x7B00` (uploads `(928,352)`, own CLUT `(304,511)`), local rect `(48,0)` 16x16 | glyph `(162,154)` open / `(12,170)` closed; caption text (white font) "Button: View Next page" / "Button: View Hyper Arts list" at glyph `+ (16, 2)` |

The status plate is parked off-screen during input (its draws move to
`y=230`, below the 228-line display window).

**Arts list window** (Triangle): rect `(6,28)`-`(160,188)`. Interior =
the system-UI panel tile `(128,0)` 32x32 (sub-pal **2** - the same
`OVERLAY_SYSTEM_UI_PANEL_INTERIOR` region the pause menu tiles,
[`field-menu.md`](field-menu.md)), tiled 32x32 as shaded-textured quads
under a per-window vertical gouraud, `0x40` top -> `0x88` bottom. Borders
(sub-pal 2): edge strips `(164,0)`/`(164,28)` 24x4 and `(160,4)`/`(188,4)`
4x24, corners at `(160,0)`/`(188,0)`/`(160,28)`/`(188,28)` 4x4. Five rows
per page at `y = 36 + 30n`: art name (battle font, 14x15 glyphs) and AP
cost (menu-atlas 8x12 digits, right-aligned ending x=152) through the
**orange sub-palette 15** of CLUT row 510, and the art's command string at
`(44 + 12k, y+14)` as 12x12 menu-atlas arrow glyphs at `v=208`,
`u`: Up 208, Down 220, Right 232, Left 244. Name / AP / command string are
the SCUS arts-name table's own columns
([`art-data.md`](../formats/art-data.md#arts-name-table-dat_80075ec4)).

Still unpinned here: the review / Begin-Reselect screens' piece decomposition
(screenshot-read only). The pennant's cost law and spawn anchor are pinned
[below](#the-pennant-geometry-is-linear-in-the-commands-ap-cost); so is the
**Auto arm**.

### The `dd0ac` chain is not a direction swing's

`FUN_801E09F8` has exactly one `jal 0x801dd0ac` (`0x801E188C`), and its `a0`
is not the queued byte - it is `map[queued byte]`, read from the byte table at
`0x801F4E63` (`lui`+`addiu 0x4e64`, then `lbu a0,-0x1(v1)` at
`0x801E1874..0x801E1888`). `FUN_801DD0AC` in turn uses `a0` as a **26-byte
stride index** into the move-power table at `0x801F4F5C`
(`0x801DD1A4..0x801DD1BC`) and takes the row's `+0` halfword arithmetic-shifted
right by two as its power. So `0x801F4E63` is the move-id → power-index map
([`move-power.md`](../formats/move-power.md)), and the whole chain is
"queue byte → power row".

That map's four **direction** entries are zero: `map[0x0C..=0x0F] = 0`, and the
disc ships move-power row 0 as 26 zero bytes. A bare swing therefore resolves
at power **zero** through this route, which is what says the route is not where
a swing's damage comes from - it is the arts / magic / summon route (the map's
44 non-zero entries include every art constant: `0x1B` → 12, `0x1F` → 15,
`0x25..0x28` → 16..19, and so on). A swing's tier is the melee kernel's own
per-command scalar `0x801F64EC[(id - 0x0C) % 5]` = `12 / 18 / 20 / 22 / 28` -
the same five-value scale an art record's power byte decodes to. *(Evidence:
the map and table bytes plus the two `a0` derivations are `disassembly`; that
the swing's damage is therefore `FUN_801EC3E4`'s is `inference`.)*

Port: `DomeDamageModel::damage` falls back to
`battle_formulas::command_power_scalar` for a byte with no move-power row,
instead of resolving it at zero.

### The queue the dome resolves is the tokenizer's

Selection appends **raw direction ids** to `actor+0x1DF`, but what plays out is
the retail action queue: the same tokenizer pass the battle's Arts command
runs, so a matched art's constant replaces the swings it consumed and indexes
its own move-power row. The dome is a restricted normal battle sharing that
machinery, so the port runs the same pass -
`MuscleDomeSession::install_art_catalog` + `tokenized_queue`, over
`legaia_art::tokenize`. Without a catalog the string stays four swings, which
is retail's own answer for a character with no arts.

The catalog is built through one shared filter, `muscle_dome::art_catalog_for`
(this character's rows, real arts only, two arrows or more, grid order), and
installed on the arena-door warp path and at the native window's own dome
entry. The **minigames page's** standalone dome panel still installs none: its
only art source is the SCUS arts-*name* table, which carries a display index
rather than an action constant, so it owes the per-character art-record decode
(PROT `0x05C4`) before it can build the same rows. Its arts banner is
unaffected - that runs off the name table's combo strings.

### The pennant geometry is linear in the command's AP cost

`30` is not a threshold and not a table index - it is the zero point of one
subtraction, and it appears exactly twice in `FUN_801D388C` (`0x801D3B6C` and
`0x801D3B98`). The pennant itself has no cost special-case at all. Case `0xB`,
the per-press arm (called from `FUN_801D0748` at `0x801D1F90`), copies the
pressed direction chip's record into the pennant's:

```
0x801D3D00/0x801D3D08   pennant width = chip width          (= cost - 6)
0x801D3D0C/0x801D3D18   landing x     = ctx[+0x6D8]
0x801D3D10/0x801D3D14   landing y     = 0xC0                (immediate)
0x801D3D1C..0x801D3D38  style         = chip style + 6
```

The cursor `ctx[+0x6D8]` is seeded to `16` on the gauge build (`0x801D3A3C` /
`0x801D3A44`) and advanced by the command's **own** `+0x74` cost, not by 30
(`0x801D3D68..0x801D3D74`); the `cost - 6` width is set in case `9` at
`0x801D3B44`. So pennant `n` sits at `x = 16 + sum(cost of 0..n-1)`, is
`cost(n) - 6` wide, and lands on `y = 192`. The captured `x = 7` is the left
diamond cap, drawn at `anchor - cap_width` = `16 - 9`. The same algebra
reproduces the bar: record `0x0F` is anchored at `16` and `pool - 6` wide, so a
100-AP pool spans `0..128` end-to-end, which is the captured figure.

Where the cost geometry *does* branch is the **direction chip** the pennant is
copied from. Case `9`'s four-iteration loop pulls each chip's seat x from the
12-byte-stride array at `0x80076BBC` (immediately before the placement table;
SCUS file `0x673BC`, values `176 / 216 / 216 / 256` - the captured chip
anchors) and then subtracts `(cost - 30) * K[slot] / 2`, with
`DAT_8007B650 = [2, 1, 1, 0]` (SCUS file `0x6BE50`, immediately followed by the
`Auto` / `Command` strings, which is what fixes the file/VA pairing):

| slot | command | anchor x | `K` | how it widens |
|---|---|---|---|---|
| 0 | `0x0C` arm (Vahn / Gala) | 176 | 2 | right edge pinned at 200, grows left |
| 1 | `0x0F` High | 216 | 1 | centred on 228 |
| 2 | `0x0E` Low | 216 | 1 | centred on 228 |
| 3 | `0x0D` arm (Noa) / Right | 256 | 0 | left edge pinned at 256, grows right |

i.e. `K` makes the arm chip grow *away* from the D-pad glyph between them.

The shift lands on **both** of the record's `x` seats, not just the resting
one. The 12-byte array's `+0` halfword goes to the record's seat A `x`
(`sh v0,0x2(a1)` at `0x801D3B54`) and its `+2` halfword to seat B `x`
(`sh v0,0xa(a1)` at `0x801D3B60`), and the same `(cost - 30) * K / 2` product
is subtracted from each - `0x801D3B64..0x801D3B8C` for seat A and
`0x801D3B90..0x801D3BC4` for seat B. The disc rows read
`(352, 176) / (392, 216) / (392, 216) / (424, 256)`: seat A is the off-screen
glide start and seat B the resting anchor, which is why the four "anchor x"
values in the table above are the `+2` field. In each case the halving is the
compiler's signed divide-by-two (`srl 31` / `addu` / `sra 1`), so it truncates
toward zero rather than flooring.

Port: `engine-ui::arts_input` carries the law
(`ArtsInputFrame::chip_anchor` / `chip_w` / `pennant_w`, `CHIP_WIDEN_K`,
`COST_WIDTH_BIAS`), fed the per-(character, weapon) cost row both hosts
already resolve.

**The spawn anchor is the pressed chip, not the fighter.** `0x801D3CE8` /
`0x801D3CF0` and `0x801D3CF4` / `0x801D3CFC` copy the chip record's seat-B
`(x, y)` verbatim into the pennant record's seat A, then `0x801D3D40` clears the
seat-mode byte and `0x801D3D48` sets a 24-frame glide before
`FUN_801D8DE8(0x20 + n, 0)` spawns it. Nothing on that path reads an actor
screen position. `FUN_801D9BBC` is the per-frame stepper; the registration is
`FUN_801D8DE8` -> `FUN_801DB7B0`, which takes the glide's *start* from the
just-created node (`0x801DB7FC` / `0x801DB810`) and its target from the
record's other seat.

**There are exactly nine pennant seats**, placement records `0x20..0x28` (ids
`05 05` .. `0d 0d`, `h = 0x0C`, seat-B `y = 0xC2`, kind `0`, no string), and the
bar-clear loop frees exactly the handles with id `5..13`
(`0x801D3C50..0x801D3C88`). Nine is the floor of the `0x120` AP clamp over the
30-AP minimum. A mod that lowers a cost below 30 lets `ctx[+0x19]` run past 8
and `0x801D3CF0` writes into record `0x29` - the opponent-name chip.

The pennant carries **no text**: indices `0x20..0x28` land on
`FUN_801D8DE8`'s default arm, so `+0x14` stays the record's static `0` and
`FUN_8003541C` skips its measure loop. The label between the caps is a sprite
strip selected by the style byte (`chip icon + 6`), the chip icons being
`DAT_801F4B94 = [0D 10 11 0C]`.

One pixel-level constant is still inferred rather than measured: the bar's
record is anchored at seat-B `y = 194` and the pennant's landing `y` is the
immediate `192`, so the pennant should draw two pixels above the bar row - the
captured `BAR_Y = 188` predicts a pennant top edge of 186. Confirming that (and
the off-class widths) wants a placement-table read on
`arts_bar_offclass_gala_nail` / `arts_bar_astral_sword_vahn` diffed against
`arts_bar_ideal_gala_club`.

### The Auto arm picks nothing - it replays a saved string

There is no command picker, no RNG roll, no fixed cycle and no favored-class
rule behind Auto. The string it commits was **already in the actor's queue**
before the Auto/Command menu opened, copied there out of the character's own
save record, and Auto's whole job is to skip the editing screen.

The two halves, both in the battle overlay:

- **Load.** `FUN_801DA34C` is called at `0x801D15C8`, on the phase-`0x28`
  **Attack confirm** - in the same breath as the `actor+0x1DE = 3` stamp
  (`0x801D15CC`) the saver below requires - and before that arm branches on the
  option global. Gated on
  `_DAT_8007BD04`, it copies **16 bytes** out of the acting character's record
  (`0x80084140 + 0x414*(char-1)`, stride matching the `0x414` party records)
  into `actor+0x1DF..+0x1EE`. It prefers record field **`+0x1A7`** when
  `actor+0x156 < actor+0x154` and field **`+0x1B7`** otherwise, falls back from
  `+0x1A7` to `+0x1B7` when the first byte is zero, and zero-fills the queue
  when neither is set (`0x801DA3CC` / `0x801DA41C` / `0x801DA4C4` /
  `0x801DA51C`). The two AP-band slots exist because the round budget is not
  the pool: `FUN_801D88CC` writes `actor+0x154 = (7 * actor+0x156) / 10 + 8`
  (clamped at `0x120`) on the restricted arm and `= actor+0x156` on the other
  (`0x801D8954..0x801D89BC`).
- **Save.** `FUN_801DA59C(fighter)` at `0x801D22BC`, on the **confirm at queue
  review** (phase `0x5A`), copies the 16-byte queue back into the same record
  field - `+0x1A7` or `+0x1B7` by the same `+0x156 < +0x154` test - for a live
  actor whose action state `+0x1DE == 3` (`0x801DA638` / `0x801DA69C`).

Both halves are already in the function directory as the battle system's
command-block restore / persist pair
([`functions/battle.md`](../reference/functions/battle.md)); what was missing is
that they *are* the Auto arm - nothing else supplies its string.

So Auto = *repeat the last string you confirmed for this character, in this AP
band*. Two corollaries fall out of the same code and are worth stating because
they read as separate features:

- The doc's "previous round's pennants persist in the bar when the input
  reopens" is this same reload, seen from the Command side.
- Confirming in the entry screen with **nothing entered** accepts the reloaded
  string outright: `0x801D1FA0` checks the same `_DAT_8007BD04`, that no
  direction has been entered yet (`ctx+0x19 == 0`) and that `actor[+0x1DF]` is
  non-zero, then measures the string's length into `ctx+0x19` and jumps to
  phase `0x5A`.

What the `0x78` arm itself does is only bookkeeping: the Command branch
(`s2 & 0x2000`) calls `FUN_801da34c` again, sets phase `0x50`, writes the
per-fighter Auto flag `ctx[+0x266 + ctx[+0x13]] = 0` and opens the entry screen
(`FUN_801dbb8c`); the Auto branch (`s2 & 0x8000`, or the confirm mask
`*(0x800846D0)`) sets phase `0x5A` and writes that flag `1`. The flag is a
*memo*, not a mode: its only behavioural reader is the cancel arm of phase
`0x5A` at `0x801D23A0`, which returns to `0x78` when it is set and to `0x50`
when it is clear; the two other readers (`0x801D5198`, `0x801D54BC`) only gate
a HUD element. Whether the pick screen is shown at all comes from the option
global `*(0x800846C4)` at `0x801D15DC` - `0` opens the `0x78` menu, `1` goes
straight to review, `2` takes a third arm.

Because the dome runs this screen verbatim, its geometry is **not** dome
data: the composition (chip anchors, D-pad seat, bar and pennant seats, AP
plate span, list window) lives in `legaia_engine_ui::arts_input`, which the
battle hosts draw through directly and which the dome page builds its
`arts_input` JSON from. The per-command AP price is shared the same way -
both screens read the equipped set's `+0x74` bytes through
`legaia_asset::battle_char_assembly::swing_command_costs`
([`arts-command-gauge.md`](arts-command-gauge.md#reading-it)).

What is still *not* unified is the session object: the dome keeps its own
`MuscleDomeSession` budget / spent / queue triple while the battle runs
`engine_core::arts_command_input`. The two model the same retail state
(`ctx+0x6dc`, `ctx+0x6d8`, `actor+0x1df`) in the same units, so the seam is
a refactor rather than a question.

### Three marks for "you cannot pick this", and the gates that raise them

The dome's course restriction and a plain unavailable command are **not the
same widget**. There are in fact **three** mark emitters, all in the battle
overlay and all called from the phase-`0x28` cluster arm of `FUN_801D0748`:

| Emitter | Source rect on the `etim` page | CLUT | Raised by |
|---|---|---|---|
| `FUN_801DBC30` | `(0,96)` 64x16 - the red cross-out X | `0x7704` | the special-battle word's restriction bit |
| `FUN_801DBD04` | `(80,96)` 32x24 | `0x770B` | `actor+0x16E & 0x38 == 0x38` (Attack) |
| `FUN_801DBEC4` | `(120,96)` 64x16 | `0x7700` | `actor+0x16E & 0x1000` (Ra-Seru, magic sealed) |

All three take `(x, y)` and emit one `POLY_FT4` (tag `0x09000000`, code
`0x2C808080`, tpage `7`) covering `(x-8, y-4)` to `(x+0x37, y+0xB)` - a
64x16 screen quad at the chip's plate box, which is why a 32x24 source is
stretched into it. Each returns early when `ctx+0x6CE` is non-zero.

A command that is merely **unavailable** draws none of the three: it keeps
its chip and its label becomes a single `-` (`FUN_801D8DE8` record `0xA`'s
own blank arm), which the shared cluster builder
`legaia_engine_ui::battle_command_ui` implements for both battle hosts. A
fighter carrying no Ra-Seru is that case, not a crossed-out chip.

The earlier reading that a restricted caller "most likely" expressed itself
with the `-` glyph is superseded: the marks coexist, and the X is the one a
course restriction draws.

### What makes a Ra-Seru chip render

The ring is direction-selected, and each arm carries its own gate. Pad bits
are the Legaia mask's (`Up 0x1000`, `Right 0x2000`, `Down 0x4000`), and
**Attack is the configured confirm button** (`0x800846D0`), not Left:

| Chip | Arm | Refuses when |
|---|---|---|
| Item (Up) | `0x801D1364..0x801D137C` | `special & 0x100` |
| Ra-Seru (Right) | `0x801D1400..0x801D1454` | `ctx[+0x25F + member] == 0`, then `actor+0x16E & 0x1000`, then `special & 0x200` |
| Attack (confirm) | `0x801D1534..0x801D156C` | `actor+0x16E & 0x38 == 0x38` |
| Spirit (Down) | `0x801D1670..0x801D1690` | never |

`special` is the word at `0x8007BAC0`, and the arena **does** own its high
bits. On the fresh-entry side of the `bnez` at `0x801CEB58`, `FUN_801CEA6C`
seeds the whole word from three story-flag tests (`jal 0x8003CE64`): flag
`0x536` writes `0x101` (`0x801CEBA0`), `0x537` writes `0x111` (`0x801CEBB4`)
and `0x538` writes **`0x321`** (`0x801CEBC8`) - the last of which carries bit
`0x200`. The seed before the three tests is `0` (`0x801CEB8C`), and the tests
run in order with the last match winning. Note that **all three** seeds carry
bit `0x100`, so a seeded dome visit forbids the **Item** chip on every course;
and the low byte is where the course itself comes from,
`((word - 1) & 0xFF) >> 4` at `0x801CEBD4..0x801CEBE8` giving `0` / `1` / `2`.
Thereafter only the low byte moves: `FUN_801D0088`
(`0x801D00B8..0x801D00E4`) writes `(old & ~0xFF) + (course << 4) + round + 1`
and the per-leg bump at `0x801CEC08` adds one, so the seeded high bits survive
the whole visit.

The `0x200` (magic-forbidden) bit therefore has **three** raisers on the disc.
Two are in `SCUS_942.54`'s battle init, keyed on the first enemy's monster id
at `0x8007BD0C`: `0x800519DC..0x80051A04` for monster `0xAF`, and
`0x8005200C..0x8005205C` for a first enemy in `0x3D..=0x3F` while the mode word
`0x80084540` is `0xC` or `0x15`. Neither fires for a dome round - the ladder
tops out at `0xAA`
([Course ladder](#course-ladder-the-opponent-per-course-round)). The third is
the arena's own seed, and `0x321` decodes to course `2` (Master), so **once the
Master course is unlocked, every dome round in that visit crosses out the
Ra-Seru chip**. *(Evidence: `disassembly` - the two SCUS arms, the arena's four
seed stores and its low-byte stamp, and an unwindowed store sweep of
`0x8007BAC0` over `SCUS_942.54` plus all 83 mapped overlays: 13 stores, 5 of
which clear the word. The earlier "only two writers" reading came from
`find-gp-relative-refs.py`, which caps `lui`-to-use pairing at 24 instructions
and cannot see the four seed stores 34..49 instructions past their `lui`. That
`0x538` is the Master unlock is inference from the three arms' course indices
0/1/2; the literal `0x321` and its `0x200` bit are not.)*

The member gate `ctx[+0x25F + member]` is the same byte the battle command
ring's element chip reads - written once by the party battle-actor init
`FUN_80053CB8` from the record's Ra-Seru equipment slot, mirrored at
`engine-core::battle_hud::battle_member_has_raseru`. So "does the chip
render live" has one answer across the dome and the ordinary battle.

Taking the chip writes `ctx+6 = 0x46` and, at the confirm, the picked spell
id straight into `actor+0x1DF[0]` with `actor+0x1DE = 2` and
`actor+0x1E7 = 9` (`0x801D1A14..0x801D1A34`, `0x801D14A4`, `0x801D14C0`).
Nothing on that path reads `ctx+0x6D8` / `ctx+0x6DC`: **a cast spends MP,
not AP**, and it replaces the whole direction string rather than joining it.
The cost is the static spell table's `+3` byte
(`DAT_800754C8 + id*12`) discounted by the character record's ability
bitfield `+0xF4` - bit `0x20` halves it, bit `0x10` takes a quarter off
(`0x801D1A38..0x801D1B70`) - and the confirm arm compares it against
`actor+0x150`, refusing without committing when the gauge is short
(`0x801D1C0C..0x801D1C28`). The debit itself is the shared band's
(`FUN_801E295C` state `0x28`), not the ring's.

Port: `engine-core::muscle_dome`'s `DomeRing` / `ChipMark` /
`DomeMagic` carry the gates and the marks, `MuscleDomeSession::commit_cast`
the pick, and `MuscleDomeSession::select_input` the shared surface both
hosts drive. A dome cast resolves through `engine-core::spells::cast_spell`,
the same rule the regular battle's cast band folds with, so the two cannot
disagree about a spell's outcome.

### The command cluster is the battle cluster

The dome's chip anchors are the same cluster the battle command menu draws,
which is what the element table says: element 8 Item `(204, 34)`, 9 Attack
`(160, 66)`, `0xA` the Ra-Seru chip `(248, 66)`, `0xB` Spirit `(204, 98)`.
Run each through the plate law (`plate = (rec.x - 8, rec.y - 6)`) and they
are exactly `legaia_engine_ui::battle_command_ui::CLUSTER_COMMAND`'s four
arms - `(196, 28)` / `(152, 60)` / `(240, 60)` / `(196, 92)` about a centre
of `(228, 70)`. Two independent captures, one geometry; the dome page and
the battle hosts draw it from the one module.

Site consumer: `legaia-web-viewer::minigames_muscle` (`muscle_hud_json` +
`muscle_hud_sheet_rgba`) decodes these sources per sheet/sub-palette and the
dome panel draws the chrome from them - including the whole
[arts command input](#arts-command-input-packet-pinned) (`arts_input`
pieces + `muscle_arts_list_json`); the disc-gated oracle is
`crates/web-viewer/tests/muscle_web_real.rs`
(`muscle_hud_chrome_decodes_from_the_disc`). Still fitted on the page: the
banner's speed-line rays (retail draws untextured polys), the SUPER/MIRACLE
word composition (atlas layout; only the HYPER strip's draw is
packet-pinned), and the chips' glide-in motion.

The **hub screens** draw on both hosts through the shared
`engine-ui::other_game_hud` emitters. The browser dome page reaches them via
`muscle_hub_quads_json` (screen-selected by the page); the native
play-window bakes the two hub page TIMs per referenced sub-palette into a
sprite atlas and runs the same builders itself (`window/minigames.rs`,
`muscle_hub_sprite_draws`): the intro card + ROUND banner over an open leg,
the INTERVAL heading + six-row score tally between legs, the tally fed the
same `DomeContest` rows / tally / coin-bank model on both hosts. *Between
legs* is the whole of it - see [The intermission is per fight, not per
turn](#the-intermission-is-per-fight-not-per-turn) for why a turn boundary
draws no hub screen and where the shared verdict lives. Both hosts run each
screen on the hub controllers' own counters - see
[the hub screen envelopes](#the-hub-screens-are-envelopes-not-frame-counts).

### The hub screens are envelopes, not frame counts

A hub screen is not "up for N frames at full brightness". Each is a **fade-in
at its own rate, a hold, and a fade-out**, and two of the four holds end early
on a pad press - so a single per-screen frame count cannot describe any of
them. `FUN_801CF870` dispatches its phase byte `DAT_801D1A78` through the
51-entry jump table at `0x801CE990`; the counter family
`DAT_801D1A70 / 1A7C / 1A80 / 1A84 / 1A88 / 1A8C` lives entirely inside the
PROT 0977 image (no other image references any of them) and every step is
scaled by the adaptive frame-skip factor `_DAT_1F800393`, so the figures below
are ticks, and frames at the normal cadence.

`DAT_801D1A80` is a **brightness level**, not a tick count: the emitter
`FUN_801D050C` scales each stored channel by `c * a3 / 256` (`mult` then
`sra 8`), and the counter clamps at `0x80` - a PSX textured primitive's
neutral modulation. A host that draws a hub screen at `0x100` therefore draws
it at *twice* retail's brightness.

| Screen | Phases | Fade in | Hold | Skippable | Fade out |
|---|---|---|---|---|---|
| "Welcome to the Muscle Dome!" strip | `0` / `1` / `2` | `+dt*4`, 32 ticks | `0x7B` = 123 ticks (`slti 0x7b`, `0x801CF9A0`) | no | `-dt*4` |
| Course-title art | `3` | scale ramp `0x1640` -> `0x1000` at `dt<<7`, 13 ticks | - | - | - |
| ROUND banner | `4` / `5` / `6` | `+dt*2`, 64 ticks | `0xB4` = 180 ticks (seed `li v1,0xb4`, `0x801CFB68`) | yes | `-dt*4` |
| Opponent / ROUND-n card | `0x15` / `0x16` | `+dt*2`, 64 ticks | `0x3D` = 61 ticks (`slti 0x3d`, `0x801CFFB8`) | yes | `-dt*2` |
| INTERVAL + score tally | `0x0A` / `0x0B` / `0x0C` | `+dt*4`, 32 ticks | the tally roll (data-dependent) | no | `-dt*2` to the `0x40` floor (`slti 0x40`, `0x801CFDAC`), then `-dt*4` |

A skippable hold reads the arm's pad-edge snapshot `DAT_801D1A9C`
(`_DAT_8007B874 | _DAT_8007B938`, stored at `0x801CF8C4`) and leaves on any
bit of `& 0xF4` (`0x801CFBE0`, `0x801CFFE4`).

The tally roll has its own literals: the four lanes' tick counters
`DAT_801D1AB8 / 1ABC / 1AC0 / 1AC4` each test `slti 0x11` (a 17-tick lead-in)
and reseed to `0x10`, and the arm writes the cue ring
`DAT_8007B6D8 = [0x202, 0x202, 0x202, 0x203]` alongside the parallel vsync
countdown `DAT_8007C338 = [0, 0x1E, 0x3C, 0x5A]` - the four "ka-ching" cues
staggered 0 / 30 / 60 / 90 frames (`0x801CFCAC..0x801CFCEC`).

Port: `engine-core::muscle_dome::HubScreen` carries the envelope, the native
window ticks it in `tick_muscle_hub`, and the browser page samples the same
kernel through `muscle_hub_screen_json` - so neither host picks a count or a
brightness of its own.

Two instrument notes fall out of pinning this. `find-address-word-refs.py`
reports **no reference at all** for `0x801D1A80`: it scans the address
*materialisation* forms (`lui`+`addiu`/`ori`) and not `lui rX,hi` +
`lw/sw rY,lo(rX)`, which is how MIPS touches a global - for a data address,
reach for `find-gp-relative-refs.py` instead. And the committed dump
`overlay_0977_slotA_801cf870.txt` stops at `0x801D0094` while the function
runs to the `jr ra` at `0x801D00F0`, so arm `0x0B`'s body, the HP restore and
the shared draw tail every arm jumps to are absent from it; the figures above
come from disassembling the image.

## Sound

**Cues.** The match SM fires its UI blips through the one-arg cue funnel
`FUN_8004fcc8`, whose `< 0x40` leg enqueues `id - 1` as the static descriptor
row (`sltiu $s0, 0x40` at `0x8004FD94`, `addiu $a1, $s0, -1` at `0x8004FD9C`,
ring append at `0x8004FE28`; [`sfx-table.md`](../formats/sfx-table.md)).
`FUN_801d0748` carries **37** immediate call sites - ids `0x21` (15 sites),
`0x22` (7), `0x23` (15) - i.e. static rows `0x20`/`0x21`/`0x22`, whose category
byte routes them to the slot-0 system bank (extraction PROT **0868**). *(The
earlier "34 sites - 13 / 7 / 14" count is superseded; a `jal 0x8004fcc8` census
over the disassembled body returns 37, and both undercounted classes were the
outcome arms.)* Which blip is which is
[pinned below](#which-blip-is-which-0x21--0x22--0x23). The **melee impact** is the shared battle
path's: an entered direction resolves through the same battle-action machinery as an
ordinary strike, and the shared battle/duel bank's impact cue is static row
`0x09` (category 2 -> extraction PROT **0869**; pinned at the top of the Baka
duel damage kernel `FUN_801D3B18`, the same bank the battle scene loader
stages). The dome's basic swing commands map to move-power record 0, whose
per-move sound-cue byte (`+0x0d`) is **0** - so no per-move cue overrides the
shared impact.

### Which blip is which (`0x21` / `0x22` / `0x23`)

Every site is inside a `ctx+6` phase arm (or the input pre-pass that runs before
the dispatch at `0x801D0C84`), and the arms' address ranges come straight from
the compare chain `0x801D0C84..0x801D0DCC`. Reading the sites in context gives
one meaning per id, consistent across all 37:

| id | static row | meaning | representative site |
|---|---|---|---|
| `0x21` | `0x20` | **accept / confirm** - a direction press taken into the string, and every confirm that advances the phase byte | `0x801D20D4` (phase `0x50`: the accepted press, and the exhausted-budget commit to `0x5A`) |
| `0x22` | `0x21` | **highlight moved** - the pre-pass fires it when the pressed direction differs from the latched `ctx+0x880`, and the target-select arms when the cursor changes seat | `0x801D082C` (pre-pass, `s2 & 0x8000`) |
| `0x23` | `0x22` | **refused / back** - a direction the actor's status mask `+0x16E` blocks, and every press of the cancel mask `*(0x800846D4)` | `0x801D1EA0` (phase `0x50`: `+0x16E & 8` set, so the press is rejected) |

Per-arm, with the arm each site's address falls in:

| phase | `0x21` (accept) | `0x22` (move) | `0x23` (refuse/back) |
|---|---|---|---|
| pre-pass | - | `082C` `08D0` `0988` `0A40` (one per direction bit `0x8000` / `0x2000` / `0x4000` / `0x1000`) | - |
| `0x32` | `1158` | - | `1130` |
| `0x28` | `15D0` | - | - |
| `0x78` | `175C` (Command) `17CC` (Auto) | - | via the shared tail `2968` |
| `0x50` | `20D4` | - | `1EA0` `1EE8` `1F30` `1F78` (one per blocked direction) + `2104` (cancel) |
| `0x5A` | `22B0` | - | `2320` |
| `0x5B` / `0x5C` / `0x5D` | `24AC` / `2690` / `2828` | - / `263C` / - | `2560` / `26F4` / - |
| `0x5E` / `0x64` / `0x65` | `29B8` / `2AA4` / `2D6C` | - / - / `2BE8` `2C78` | `2968` / `2A1C` / `2B58` |
| `0x66` / `0x67` / `0x6E` | `2E5C` / `2FDC` / `31A4` | - | `2DD0` / `2F10` / `3054` |

Two caveats the address-interval method has to state. `0x801D20D4` is a `j`
target from twice inside its own arm, which is harmless; `0x801D2968` is a `j`
target from phase `0x78`'s cancel branch at `0x801D1720`, so it is a **shared
back tail** and belongs to both arms. Those are the only two cue sites any `j`
in the function targets.

The arm boundaries the attribution rests on. `FUN_801d0748` dispatches `ctx+6`
through a **compare chain**, not a jump table, so the arms are laid out in
source order rather than numeric order - which is why `0x32` precedes `0x28` and
`0x78` sits between `0x28` and `0x3C`:

| phase | arm | phase | arm | phase | arm |
|---|---|---|---|---|---|
| `0x00` | `0x801D0DD0` | `0x50` | `0x801D1D84` | `0x64` | `0x801D2A00` |
| `0x0A` | `0x801D0DE0` | `0x5A` | `0x801D21CC` | `0x65` | `0x801D2B3C` |
| `0x0B` | `0x801D0E3C` | `0x5B` | `0x801D23F0` | `0x66` | `0x801D2DB4` |
| `0x14` | `0x801D0EC4` | `0x5C` | `0x801D2590` | `0x67` | `0x801D2EF4` |
| `0x1E` | `0x801D102C` | `0x5D` | `0x801D278C` | `0x6E` | `0x801D3024` |
| `0x32` | `0x801D10F8` | `0x5E` | `0x801D28C0` | `0xFE` | `0x801D31E8` |
| `0x28` | `0x801D1188` | `0x3C` | `0x801D17DC` | default | `0x801D3290` |
| `0x78` | `0x801D16E8` | `0x46` | `0x801D19F8` | | |

The pre-pass ahead of the dispatch has one arm per direction bit and its own
gate: the `0x4000` arm is skipped when `ctx+0x275 < 4` and the `0x1000` arm when
`ctx+0x275 < 3`, so a panel with fewer than four slots takes fewer directions
and plays no blip for the missing ones.

**BGM.** The arena loads **no BGM track of its own** - a full sweep of the muscle-dome function dumps finds no streaming-loader call (`8001fc00`) and no BGM-id write. It inherits the **battle theme** its entry set, exactly as it reuses the battle engine wholesale: the music is whichever `music_01` battle track the mode-24 sub-id-5 arena setup (the `0977` door/init slot) had playing when the contest starts. There is no dedicated muscle-dome cue to pin; this is the same "host-scene-inherited BGM" shape as the [slot machine](minigame-slot-machine.md), one class up (battle rather than field). The engine/site can represent it with the standard battle theme (`M26B1`, global BGM `2026`).

The *field* track the player hears on the way in is the host scene's, and that
scene is now named: the arena's three door-warps (`0x3E`, `op0 = 105`) all sit
in `koin1`'s MAN (PROT 543), alongside the slot-machine and Baka Fighter doors,
and `koin1`'s casino-floor entry arm plays global BGM `2018`
([`minigame-slot-machine.md` § Sound](minigame-slot-machine.md#sound)).

## Match state machine

The per-frame controller is `FUN_801d0748` (`overlay_muscle_dome_801d0748.txt`). It is the largest function in the overlay and drives one **leg** - not the contest, which is the arena hub's ([Two state machines](#two-state-machines-not-one)):

1. **Read input.** It folds the current pad-edge masks (`_DAT_8007b874` and `_DAT_8007b938`) into a single press mask `s2`. The four card-selection directions are the standard PSX face/d-pad bits `0x8000`, `0x2000`, `0x1000`, `0x4000`; the controller maps the pressed direction to one of the four queued input slots `ctx+0x1114 / +0x1118 / +0x111c / +0x1120` and records the chosen direction in `ctx+0x880`.
2. **Dispatch on the phase byte `ctx+6`.** This byte is the match phase. Confirmed phase values include `0x00`, `0x0a`, `0x0b`, `0x14`, `0x1e`, `0x28`, `0x32`, `0x3c`, `0x46`, `0x50`, `0x5a`, `0x5b`, `0x5c`, `0x5d`, `0x5e`, `0x64`, `0x65`, `0x66`, `0x67`, `0x6e`, `0x78`, `0xfe`. Phases advance by writing the next value back into `ctx+6` (`s3`). The terminal/idle phases `0x1e / 0x32 / 0x6e / 0xfe` also tick a spin/azimuth global at `_DAT_8007b938+2` each frame (the rotating dome camera). **(Confirmed: the dispatch is a `ctx+6` switch.) (Inferred: the exact ordering of phases is the deal → select → confirm → resolve → turn-top loop; individual phase semantics below are partially confirmed.)**

   The loop closes back on the turn-top arm `0x14`, never on a score screen - see [The intermission is per fight, not per turn](#the-intermission-is-per-fight-not-per-turn).
3. **Run the presentation + camera.** Most phase arms call the presentation driver `FUN_801d388c` (command/sprite layout, see below) and the camera director `FUN_801d5854`, then play a UI/SFX cue through `func_0x8004fcc8`.

A small number of phase arms are confirmed by content:
- Phase `0x14` arm (`0x801d0ef0..0x801d1010`): the **turn-top** arm. It resets the direction handles, then - gated on the dome battle type - computes and stamps the `Turns Left / HP Left` strip (below). The shared battle-action SM parks the phase byte here at the end of every turn.
- Phase `0x3c` / `0x46` / `0x50` arms: write the chosen action id into the fighter actor's `+0x1dd` (action) and `+0x1de` (action-state) fields and kick the battle action - this is **commit the entered command string and play it out**.
- Phase `0x6e` arm (`0x801d3010..0x801d3178`): the confirm / reselect menu (`FUN_801db8f4(0x98,0x58)`, cursor result via `FUN_801dba04`). It **re-stamps** the strip from the two globals the `0x14` arm already wrote; it computes nothing. The whole function contains exactly **two** ratio computations and both are the `× 100` chain in the `0x14` arm.

## The four-turn strip belongs to Koru, not the dome

The `Turns Left / HP Left` strip is a real battle-overlay HUD element and its
arithmetic is pinned, but it is **not the Muscle Dome's**. It is the HUD of
the one turn-limited boss fight in the game.

| Piece | Where |
|---|---|
| Format string | `"      Turns Left:          HP Left: "` at PROT 0898 file offset `0x0` = overlay VA `0x801CE818` (extraction `overlays/overlay_battle_action_0898.bin`). |
| Gate | `*(u8*)0x8007BD0C == 0xB6`; every draw site tests it first and returns otherwise (`0x801D0F18`: `lui v0,0x8008` / `lbu v1,-0x42f4(v0)` / `addiu v0,zero,0xb6` / `bne`). |
| Turns-Left digit | `DAT_801f6958 = 4 - ctx[+0x28a]`, drawn at x=`0x68`, **1** digit. |
| HP-Left number | `DAT_801f6959 = DAT_801c937c[+0x14c] * 100 / DAT_801c937c[+0x14e]`, drawn at x=`0xd2`, **3** digits. |
| Draw calls | `func_0x8003541c` registers the label; one `func_0x8003563c` per number. Both are register-and-draw primitives - `8003563C` is the per-actor draw-record queue append ([`script-vms.md`](../reference/functions/script-vms.md)), **not** a bar/gauge routine. |

`0x8007BD0C` is not a battle-type byte. It is the **four-slot monster-id
formation cell** the rest of this repo already models - the cell the
encounter reader fills (`[`encounter.md`](../formats/encounter.md),
`engine-core::encounter_record`), the cell the capture harness watched go
`00 00 00 00` -> `04 04 00 00` on a two-monster encounter
(`engine-core::capture_observations::battle_init_overlay::FORMATION_CELL_ADDR`),
and the cell the charm and shiny-Seru patcher hooks read as "first monster
id". So the gate says *the first enemy is monster `0xB6`*, and monster `0xB6`
is **Koru** (PROT 867 slot `(0xB6-1) * 0x14000`; the neighbouring `0xB5`
tested at `0x801D0DEC` is the final-form Cort, which is why
`engine-core::overlay_loader` already special-cases `0xB5` there).

Three independent facts settle it:

- The dome stages its own opponent into that same cell, and its highest
  roster id is `0xAA` - see [Course ladder](#course-ladder-the-opponent-per-course-round).
  No dome round can ever satisfy `== 0xB6`.
- `0x8007BD0C` has **one** writer in the arena overlay (`FUN_801D1510`) and
  **zero** writers in the battle overlay, which only reads it.
- The curated boss table (`data/gamedata/casino.toml`'s sibling
  `bosses.toml`, walkthrough-derived) records Koru as a *four-turn timed
  kill whose failure is a game over* - exactly a `4 - turn` readout with the
  boss's own HP percentage next to it.

Three facts the earlier readings of the *arithmetic* got wrong, each corrected from the disassembly, and all still standing:

- **The multiplier is 100, not `0x6C`.** The compiler emits the `× 100` as a shift-add chain at `0x801d0f38..0x801d0f4c` - `sll 1` (2x), `addu` (3x), `sll 3` (24x), `addu` (25x), `sll 2` (**100x**). Reading only the first three instructions yields `0x6C` (108). Ghidra's own C prints `* 100`, and an independently based dump of the same code (`overlay_0896_801f04b0.txt`) reproduces it.
- **The arm is phase `0x14`, not `0x6e`.** `0x14` computes and stamps; `0x6e` (and the input-pad arm around `0x801d2900`) only re-stamps the globals.
- **The percentage is the *opponent's*, not each fighter's own.** It reads `DAT_801c937c`, which is actor-table index 3 - the first **enemy** slot, since the party occupies 0..=2. There is one number on screen, not one per fighter.

`ctx+0x28a` is the shared **battle turn counter**: the battle-action SM's case `0xff` (`FUN_801e295c`) does `ctx[6] = 0x14; ctx[+0x28a] += 1`, i.e. it bumps the counter and parks the round driver on the strip arm. Enemy AI in the same overlay keys its behaviour off the same byte. That is shared battle machinery; only the `0xB6`-gated strip arm is Koru's.

## What ends a leg: a knockout, and nothing else

The arena has no battle loop of its own, so it has nothing to bound. It picks
the opponent and hands the round to the ordinary battle, which ends the way
every battle ends.

`FUN_801D1510` (`0x801D1510`, arena overlay) is the whole handoff. It resolves
the round through the course descriptor and the roster, stores the id into
formation slot 0, clears slots 1..3, and sets the global game-mode word:

```mips
801d1564  lui   a1,0x8008
801d1574  addiu a0,a0,0x1a08     ; a0 = course descriptor table
801d158c  lw    v1,0x4(v1)       ; course_desc[course].first_round
801d1598  lbu   a0,0x4(v0)       ; roster[round].monster_id
801d159c  addiu v0,a1,-0x42f4    ; v0 = 0x8007BD0C, formation slot 0
801d15a4  sb    zero,0x1(v0)     ; clear slots 1..3
801d15b8  sh    v0,-0x47c4(v1)   ; game_mode = 0x14 (BATTLE INIT)
801d15bc  sb    a0,-0x42f4(a1)   ; slot 0 = monster_id
```

That `sh` is the arena overlay's **only** write of `0x8007B83C`, and mode
`0x14` is `BattleInit`, whose initializer `FUN_80055B6C` builds the battle
scene from the very cell the line above filled.

From there the round is an ordinary battle:

| Step | Where |
|---|---|
| End detection | The `0x5A` end-of-action gate of `FUN_801E295C` walks the actor table; with no combatant standing on a side it sets the battle-end signal `DAT_8007BD71 = 0xFE` (party wipe: cause `5`; monster wipe: cause `0`). See [battle.md](battle.md#party-wipe--the-game-over-overlay). |
| Exit routing | `FUN_80046A20` (SCUS) picks the next mode. With `_DAT_8007BAC0 & 0x100` set it stores `0x18` (mode 24 OTHER) at `0x80046E50` rather than the field's `0x2` at `0x80046E0C` - which is what returns a dome round to the arena. |

**The turn counter is a counter, not a budget.** `ctx+0x28a` has exactly one
writer in the battle overlay - the increment at `0x801E6800`/`0x801E6810` -
and every one of its reads selects *scripted per-turn enemy behaviour* (turn
`0` openers at `0x801DAAD4` / `0x801EB994`, parity alternation at
`0x801EA0B8` / `0x801EB4C8`, a five-entry per-turn action table at
`0x801EB538`, turn-`1`/`3` dispatch at `0x801EBE08` / `0x801EEDB0`) or draws
Koru's countdown. No read of it reaches the battle-end signal.

So the leg-end condition is the HP fields the win/lose phases already branch
on, and there is nothing else. This is a negative result: it is not that the
timeout arm has yet to be found, it is that the only two writers of the
end signal are KO scans.

## Course ladder: the opponent per (course, round)

The arena's opponent is **pinned to a real monster id** by two adjacent
tables in the PROT 0977 door/init entry, immediately after the score table:

| Table | VA | File offset | Shape |
|---|---|---|---|
| Round roster | `0x801D1920` | `+0x3108` | 29 x 8 bytes: `{ u32 name_ptr; u32 monster_id }` |
| Course descriptors | `0x801D1A08` | `+0x31F0` | 3 x 8 bytes: `{ i32 round_count; ptr first_round }` |

The descriptors are `(8, 0x801D1920)`, `(8, 0x801D1960)`, `(13,
0x801D19A0)` - contiguous, 8 + 8 + 13 = 29, and the counts match the
populated-cell counts of the score table's three rows exactly.

`FUN_801D1510` (file `+0x2CF8`) is the installer, and its tail is the whole
chain in eight instructions: index the descriptor by the course
(`DAT_801D1A90 << 3`), take its `+4` round pointer, index that by the round
(`DAT_801D1A94 << 3`), `lbu` the `+4` id, clear formation slots 1..3, write
`0x14` to the stage word `0x8007B83C`, and `sb` the id into formation slot 0
at `0x8007BD0C`. One enemy, no formation variety.

`FUN_801D0CD4` reads the other two fields: it walks all three descriptors to
draw the course menu (`+0x00` count as the loop bound, each round's `+0x00`
name pointer through the text drawer at `0x80036888`) and clamps the round
counter against the count.

The 29 ids, in course/round order:

| Course | Rounds | Monster ids |
|---|---|---|
| 0 Beginner | 8 | `13 0D 10 49 62 4B 86 8B` |
| 1 Expert | 8 | `14 06 6D 3C 81 49 50 8B` |
| 2 Master | 13 | `81 86 3C 49 4B 4D 8B 8A A4 A3 A2 A9 AA` |

Resolved against the monster archive (PROT 867, slot `(id-1) * 0x14000`)
the names reproduce the curated `[[muscle_dome_course]]` line-ups in
`data/gamedata/casino.toml` **29 of 29, in order** - an independent,
walkthrough-derived cross-check that the tables are what they look like.
Parser: `legaia_engine_core::muscle_dome::parse_course_ladder`.

The score table's own rows corroborate the same shape: 8 / 8 / 13 populated
`i32` cells summing to 818 / 1532 / 13830, and 818 and 1532 are the exact
`reward_coins` of the curated Beginner and Expert rows.

## Distinguish it from the status-plate readouts

`FUN_801d8de8` is a **different widget** and must not be collapsed into the strip. It is the shared battle **status-plate composer** (dumped under ten overlays - dance, fishing, slot, Baka Fighter, debug menu, magic capture …), and its numeric work is four `func_0x8003563c` registrations at `0x801d959c..0x801d9648`, one per plate field: `+0x172` / `+0x14e` (HP `cur` / `max`, 4 digits) and `+0x174` / `+0x152` (MP `cur` / `max`, 3 digits). Its elems `0x52` / `0x53` stage the fighters' `+0x170` gauge values into `_DAT_800773c8` / `_DAT_800773e0`.

So a dome match shows exactly one of the two: the per-fighter `cur/max` **numerals + bars** on the status plate (shared battle chrome, no percentage anywhere). The **single percentage** the phase-`0x14` arm of `FUN_801d0748` computes belongs to the *other* readout - Koru's strip - and its `== 0xB6` gate keeps it off every dome frame. Calling it "dome-only" inverts what the gate says.

Auxiliary per-frame helpers the controller calls every frame:
- `FUN_801d3444` - animates the round **time meter**: ramps a 0..0xc counter `DAT_801f4e0a` up by the frame delta while the phase tag `ctx+6 == 'P'` (0x50) and an enable flag is set, drains it otherwise, and maps it to the bar Y `counter * 160 / 12 - 0x92`. Core ramp + mapping ported as `engine-core::muscle_dome::time_meter_step`. (`overlay_muscle_dome_801d3444.txt`.)
- `FUN_801d9bbc` - advances every **active animated sprite handle** (`ctx+0x1074[]`, up to 0x28 entries) one linear-ease step toward its target screen position over a per-handle frame count (`ctx+0x11B4 + i*0xC` records: total/elapsed frames + target/start positions; arrival snaps and deactivates). Per-handle step ported as `engine-core::muscle_dome::SpriteGlide::step`. (`overlay_muscle_dome_801d9bbc.txt`.)

The glide step has **no producer** in the engine: nothing writes a
`(start, target, total)` record, so no session reaches it - the same shape as
`camera_rel_glide`'s declined row. The kernel's contract is pinned by
`crates/engine-core/tests/w1b_dome_leg_ladder.rs`, and the wire it is waiting
on is a host that spawns the pennant / chip sprites this table animates.

## Direction commands + selection

The fighter's four selectable actions are its four **direction commands**, laid out by `FUN_801d388c` case `9` / `0x2c` (the **deal** step):

- There are **four slots**, built in a `do { … } while (uVar17 < 4)` loop - one per d-pad direction, and always the same four command ids `0xC..=0xF`.
- Each slot's command id comes from a small **deck-order table** at `&DAT_801f4b8c` / `&DAT_801f4b94` (a per-slot move-index list); the per-slot screen layout (X/Y/size) is read from a parallel layout table walked at stride 6. **(Confirmed: 4-slot loop reading `&DAT_801f4b8c`/`&DAT_801f4b94`.)**
- Each slot carries an **AP cost** read from the fighter record: the loop loads a per-move cost byte (stored into `ctx[uVar17 + 0x14]`), normalises it against a `0x1e` baseline, and uses it both to size the slot's sprite and to debit the turn's point budget.
- For party character index `2` the slot order is swapped (slots `0` and `3` exchange), i.e. the layout is mirrored for one of the fighters.

The **turn's point budget** lives at `ctx+0x6dc`, seeded from the fighter record field `+0x154` (the character's available "spirit"/AP pool); the running spent total is `ctx+0x6d8`. The number of directions already entered this turn is the **selection index `ctx+0x19`**, and the slot currently being committed is `ctx+0x1a`.

`FUN_801d388c` case `0xb` is **commit one entered direction**:
- It rejects the commit if the remaining budget `ctx+0x6dc` is smaller than that direction's cost (`ctx[ctx+0x1a + 0x14]`) - you cannot overspend.
- Otherwise it spawns the entered-command sprite, **records the chosen move id into the fighter actor's queue** at `actor+0x1df + ctx+0x19` (an in-actor list of queued action ids), debits the cost from `ctx+0x6dc`, adds it to `ctx+0x6d8`, and increments `ctx+0x19`.

So selection = repeatedly press a direction, which appends that slot's move id into the actor's `+0x1df` action queue while there is budget left - exactly the normal battle command-string input, bounded by AP instead of by a fixed string length.

## Round resolution

`FUN_801d388c` (`overlay_muscle_dome_801d388c.txt`, the 7820-byte presentation driver) is a large `switch(param_1)` over **presentation/animation step ids** (0..0x31). It does *not* itself compute damage; it lays out the command and label sprites, runs the deal/commit loops above, and at its tail walks a **per-step script-record table** `PTR_DAT_801f4d34[param_1]`:

The record is variable-stride - `[u8 count][u8 anim_sel][u8 panel_id/bind_count]`
followed by `count` `(u8 elem_id, u8 mode)` pairs (reader `FUN_801d388c`):

```
record = PTR_DAT_801f4d34[step]
record[0] = sub-draw count
record[1] = side/animation selector (1/2/3 → the panel-sprite reset/teardown pair FUN_801d99bc / FUN_801d9ae8; see Key functions - a full-table rebuild and a full-table release, not slides)
record[2] = DUAL ROLE:
              (a) active-panel id - compared against ctx+0x275; record[2]+ctx+0x275 == 6
                  triggers a panel-swap reset of ctx+0x880..0x883, AND
              (b) count of the leading sub-draw handles bound back to the four input
                  input slots (ctx+0x1114[]); the first record[2] sub-draws are the
                  directional selection sprites, flagged +0x1d = 2
record[3+2k], record[4+2k] = (element id, mode) pairs fed to FUN_801d8de8 for each sub-draw
```

Each sub-draw calls the HUD/element renderer `FUN_801d8de8(id, mode)` (see below). When the global `_DAT_800846c8` is set, the returned sprite handles are also stashed into `ctx+0x1114[]` and some are flagged `+0x1d = 2` (the four directional selection sprites), tying the drawn sprites back to the input slots.

The **resolution of the queued command string** happens when the match controller advances into the commit phases (`0x3c`/`0x46`/`0x50` in `FUN_801d0748`): it walks the actor's `+0x1df` action queue, sets the actor's `+0x1dd`/`+0x1de` (action / action-state), and lets the shared battle-action path play each queued action and apply its effect to the opponent actor record (HP at actor `+0x14c`, max-HP at `+0x14e`). The `+0x1df` queue is re-zeroed at the start of each round (`FUN_801d388c` case `3` clears `+0x1e7`/`+0x1de`; case `0xb` re-seeds the budget and re-walks the queue).
**(Confirmed: queue lives at actor+0x1df, budget gating.) (Confirmed: per-command damage uses the shared `battle_formulas` *unmodified* - there is no dome-local scaling.** The match controller `FUN_801d0748` is byte-identical to the main battle round driver, and a queued action reaches damage exactly as an ordinary battle action does, with no dome-specific arithmetic on the way.)**

That chain is `actor+0x1df` → `FUN_801e09f8` → `FUN_801dd0ac`, and it carries
**arts and magic, not bare direction swings** - see
[what the swing chain is not](#the-dd0ac-chain-is-not-a-direction-swings).

The `func_0x80035f04` calls throughout are the shared screen-projection helper (project a world position to screen), used to anchor the command and label sprites over the 3D fighters.

### HUD elements (`FUN_801d8de8`)

`FUN_801d8de8(elem_id, mode)` is the **HUD element renderer** the sub-draw script calls per `(elem_id, mode)` pair. It switches on `elem_id` through an 80-entry `jr` table at `0x801CEB68` (`sltiu v0,elem_id,0x50`; `overlay_muscle_dome_801d8de8.txt`, dispatch ~`0x801d8ec0`). The `mode` byte (the pair's second value) is consumed by the shared post-switch layout tail - it selects the sprite/anchor variant and, for `0x59`, gates the reward branch. The active fighter's character-record id is `charid = (&DAT_8007bd10)[ctx+0x13]`; the opponent uses `ctx+0x21`. The labelled cases:

| `elem_id` | HUD element |
|---|---|
| `0x0A` | Current fighter Spirit / move name → `_DAT_80076d14`; blank-gated on the per-fighter flag `ctx[fighter+0x25F]` (blank = `&DAT_801f4bc6`, else name string `s_Spirit_801f4b98 + charid*0xA + 6`). |
| `0x0B` | "Spirit" heading string (`_DAT_80076d2c = s_Spirit_801f4b98`). |
| `0x0E` | Spirit-name second panel → `_DAT_80076d74` (same blank-gate as `0x0A`). |
| `0x16`–`0x19` | The four direction-command portraits; sets `_DAT_8007bb8c = charid-1`, frame = `elem_id-0x13`. |
| `0x1A` | Formatted number (score / count) - `func_0x80035f04` on actor `+0x1BC` → `_DAT_80076e86`/`_DAT_80076e94`. |
| `0x52` | Player HP-bar value: copies actor `+0x170` into the char record, sets `DAT_8007bd00 = charid-1` and `_DAT_800773c8`. |
| `0x53` | Opponent HP-bar value (opponent actor `+0x170` → `_DAT_800773e0`). |
| `0x58` | Opponent Spirit name → `_DAT_80077464` (blank-gated, keyed on the opponent id). |
| `0x59` (`mode`/`param_2 == 0`) | Victory banner assembly: `func_0x8003ca78(ctx+0x1F9, "…acquired the power of…")` + reward spell name (`DAT_800754d0[(ctx+0x269)+0x80]`) + suffix `DAT_801f4c28`. |

Every other `elem_id` falls to the shared layout tail (sprite emit + optional bar draw) without a case-specific label.

## Opponent + scoring

- The fighters are battle actors in `&DAT_801c9370`; the active fighter index is `ctx+0x13`, the player party member id is `ctx+0x20`, and the opponent id is `ctx+0x21` (clamped to ≤ 2 in `FUN_801d8de8`). The character→record mapping uses `&DAT_8007bd10` (per-actor character id) to index the 0x414-byte party records.
- The opponent's deal is built by the **same** deal/commit code paths (`FUN_801d388c` cases `9`/`0x2c`/`0xb`) keyed on the opponent's `ctx+0x13`; the AI simply commits commands from its own move set against the same budget rule. There is **no separate scripted AI table** in this overlay - the opponent uses the shared selection logic with its own record. **(Inferred from the symmetric use of `ctx+0x13` across both fighters; no dome-specific AI scorer was found.)**
- The opponent itself is not chosen by the match code at all - it is a monster id staged into the ordinary formation cell before the battle starts, per (course, round); see [Course ladder](#course-ladder-the-opponent-per-course-round).
- A leg ends on the fighter HP fields, which the win/lose phases (`0x64`/`0x65`/`0x66`/`0x67`) branch on, and on nothing else ([What ends a leg](#what-ends-a-leg-a-knockout-and-nothing-else)). The `4 - ctx[+0x28a]` / opponent-HP-percentage strip is **not** part of that - it is the Koru fight's ([The four-turn strip belongs to Koru](#the-four-turn-strip-belongs-to-koru-not-the-dome)).
- Separately, the shared status plate draws each fighter's own HP/MP `cur`/`max` from record fields `+0x172`/`+0x14e`/`+0x174`/`+0x152` (`FUN_801d8de8`) - unrelated numbers, no percentage. **(Superseded: an earlier revision of this line put the readout in phase `0x6e`, scaled it by `108`, sourced it from the fighter's own record, and glossed `func_0x8003563c` as "the bar/gauge primitive". All four are wrong; the strip section has the disassembly.)**
- **Caption, not reward:** `FUN_801d8de8` case `0x59` composes a victory *message* from the label table at `0x801f4dfc` plus a spell name from the static spell-name table `DAT_800754d0` (12-byte stride, indexed by `ctx+0x269 + 0x80`). Reading that as "the dome awards a Seru" is **falsified**: `0x801F4DFC` is the shared battle-family cast-caption label table, byte-identical across the battle-action / magic-capture / magic-level-up / dome overlays and reached by any cast, and nothing in the arena overlay grants an item but the one-shot War God Icon. A leg pays nothing; a contest pays coins - see [Contest settlement](#contest-settlement--the-one-shot-prize).

## RAM state

All offsets are relative to the context base `_DAT_8007bd24` unless noted otherwise. Globals outside the context are listed with their absolute address.

| Address / offset | Type | Role | Confidence |
|---|---|---|---|
| `_DAT_8007bd24` | ptr | Muscle Dome context base (**ctx**) | Confirmed |
| `ctx+0x00` | u8 | fighter count (loop bound for per-fighter HUD draws) | Inferred |
| `ctx+0x06` | u8 | **match phase id** (the `FUN_801d0748` dispatch byte) | Confirmed |
| `ctx+0x0d` | u8 | camera/view sub-mode (selects `FUN_801d5854` view offsets) | Inferred |
| `ctx+0x13` | u8 | active fighter index into `&DAT_801c9370` | Confirmed |
| `ctx+0x14 … +0x17` | u8[4] | per-slot AP cost cache | Confirmed |
| `ctx+0x19` | u8 | **directions entered this turn** (selection index) | Confirmed |
| `ctx+0x1a` | u8 | deal slot currently being committed | Confirmed |
| `ctx+0x1b`, `ctx+0x1c` | u8 | sprite step / advance used during the deal layout | Inferred |
| `ctx+0x1e` | u8 | pending HUD element id to redraw | Inferred |
| `ctx+0x1f` | u8 | panel-layout variant (1/2/3 → different on-screen panel arrangement) | Confirmed |
| `ctx+0x20` | u8 | player party member id | Confirmed |
| `ctx+0x21` | u8 | opponent id (clamped ≤ 2) | Confirmed |
| `ctx+0x269` | u8 | awarded spell/seru id (offset into spell-name table at `+0x80`) | Confirmed |
| `ctx+0x275` | u8 | active panel id (vs `PTR_DAT_801f4d34` record `[2]`, whose byte doubles as the count of leading sub-draw handles bound to the input direction slots) | Confirmed |
| `ctx+0x6b2` | u16 | per-frame tick counter (bumped each `FUN_801d388c` call) | Confirmed |
| `ctx+0x6d6` | - | scratch sub-block used for HUD layout (`pbVar10` base) | Inferred |
| `ctx+0x6d8` | u16 | **points spent this round** | Confirmed |
| `ctx+0x6dc` | u16 | **remaining point budget** (seeded from record `+0x154`) | Confirmed |
| `ctx+0x880` | u32 | chosen direction bitmask (`0x8000`/`0x2000`/`0x1000`/`0x4000`) | Confirmed |
| `ctx+0x884` | u32 | latched input mask for the round | Inferred |
| `ctx+0x1074[0..0x27]` | ptr[40] | active animated **sprite-handle** array | Confirmed |
| `ctx+0x1114 … +0x1120` | ptr[4] | the four directional **card-slot** sprite handles | Confirmed |
| `ctx+0x11b4[0..0x27]` | u8[40] | per-handle "active" flags (walked by `FUN_801d9bbc`) | Confirmed |
| actor `+0x14c` | u16 | fighter current HP | Confirmed |
| actor `+0x14e` | u16 | fighter max HP | Confirmed |
| actor `+0x154` | u16 | fighter point/AP pool (seeds the round budget) | Confirmed |
| actor `+0x1dd` | u8 | current action id | Confirmed |
| actor `+0x1de` | u8 | action state | Confirmed |
| actor `+0x1df + n` | u8[] | **queued card/action ids** for the round | Confirmed |
| `&DAT_801c9370` | ptr[] | global actor pointer table (fighters) | Confirmed |
| `&DAT_8007bd10` | u8[] | per-actor character id → party-record selector | Confirmed |
| `&DAT_801f4b8c` / `&DAT_801f4b94` | u8[] | hand deck-order / move-index tables | Confirmed |
| `&PTR_DAT_801f4d34` | ptr[] | per-step **sub-draw script-record** table | Confirmed |
| `&DAT_800754d0` | ptr[] | shared spell-name pointer table (reward name source) | Confirmed |
| `_DAT_8007b874`, `_DAT_8007b938` | u32 | pad-edge masks folded into the press mask | Confirmed |
| `_DAT_800846c0` | u32 | global contest sub-mode flag (gates camera/HUD arms) | Inferred |
| `_DAT_800846c8` | u32 | "store handles back into card slots" enable | Confirmed |
| `DAT_801f4e0a` | u8 | round time-meter counter (0..0xc) | Confirmed |

## Key functions

| Address | Role | Provenance |
|---|---|---|
| `FUN_801d0748` | Per-frame **match** controller: reads pad, dispatches on `ctx+6`, drives direction pick / commit / resolve. It owns no score loop and no hub screen - both belong to the arena's own SM ([Two state machines](#two-state-machines-not-one)) | `overlay_muscle_dome_801d0748.txt` |
| `FUN_801d388c` | Card/presentation driver: deal-hand (4 slots), commit-card, per-step sprite layout, runs the `PTR_DAT_801f4d34` sub-draw script | `overlay_muscle_dome_801d388c.txt` |
| `FUN_801d5854` | Camera / view director: 10-way (`param_2` 0..9) switch computing the dome view transform per phase | `overlay_muscle_dome_801d5854.txt` |
| `FUN_801d8de8` | HUD / element renderer: draws labels, HP/stat bars, card numbers, and the reward message; returns a sprite handle | `overlay_muscle_dome_801d8de8.txt` |
| `FUN_801d3444` | Round time-meter bar animation | `overlay_muscle_dome_801d3444.txt` |
| `FUN_801d9bbc` | Advances active sprite handles toward target screen positions | `overlay_muscle_dome_801d9bbc.txt` |
| `FUN_801d99bc` | Panel-sprite table hard reset + rebuild: zeroes all `0x28` handle slots (ptr `ctx+0x1074`, flags `ctx+0x11b7`/`ctx+0x11b4`) and the 16-word scratch `DAT_801c8fa0`, then re-creates the panel sprites | `overlay_muscle_dome_801d99bc.txt` |
| `FUN_801d9ae8` | Panel-sprite teardown: for each of the `0x28` slots with flag `ctx+0x11b7` set and a live handle at `ctx+0x1074[i]`, destroys the sprite via the shared object destructor `FUN_800319a8(handle+8)` and clears its slot, then zeroes the 16-word scratch `DAT_801c8fa0` | `overlay_muscle_dome_801d9ae8.txt` |
| `FUN_801f19ec` | Fighter model installer: relocates a TMD model bundle, uploads it, and binds it to a dome actor | `overlay_muscle_dome_801f19ec.txt` |
| `FUN_801f2e10` | **Oriented-quad "beam" emitter** (render-track): draws one textured `POLY_FT4` between two endpoints - angle+length from an atan2 helper (`func_0x80019b28`) plus the SCUS sin/cos LUTs (`_DAT_8007b7f8` / `_DAT_8007b81c`), width jittered per-edge via BIOS `rand` (`func_0x80056798`), a random 32px texture column, greyscale tint from an arg, OT depth from an arg. Touches no dome ctx, and it is **not the dome's**: its only callers on the disc are 11 `jal` sites in the slot-B summon module PROT 0909 (see [The dump set is the whole battle overlay](#the-dump-set-is-the-whole-battle-overlay---a-filename-prefix-is-not-dome-evidence)). Listed here only because the dump directory is where it was found | `overlay_muscle_dome_801f2e10.txt` |

## Hand deck decoded

The deck tables are decoded from the battle-overlay rodata (parser
[`legaia_asset::muscle_dome`]: `hand_command_ids` / `hand_sprite_ids` /
`victory_message_count`; disc-gated `muscle_dome_real`):

- `&DAT_801f4b8c[0..4]` - the four hand **command ids**, the
  direction-command ids `0xC..=0xF` (the weapon-swing runtime slots the
  Tactical-Arts queue stages). A card *is* one of the four basic strike
  commands; the commit path appends this id verbatim into the fighter's
  `+0x1df` action queue.
- A card's **cost** is `DAT_801c9360[char][cmd]+0x74` - the same per-command
  AP byte the Arts gauge reads as the arm width, copied at battle load from
  the equipment sections' swing records (`FUN_800557B8`;
  `legaia_asset::battle_char_assembly::SwingAnimation::cost`). Retail value
  set: favored `0x1E` / off-class `0x2A` / far `0x36`, disc-validated.
- `&DAT_801f4b94[0..4]` - per-slot card **sprite ids** (with a `+2`
  "unlearned" face variant gated on the character record's per-move flag at
  `record+0x18C+move_id`).
- `&DAT_801f4b84[move_id]` - the per-move display lookup the sub-draw path
  uses (presentation).

## Engine port

The match rules run from-scratch as `legaia_engine_core::muscle_dome`
(`MuscleDomeSession`): the four-direction deal (deck command ids +
per-fighter AP costs), the budget-gated commit into the `+0x1df`-model queue
(`FUN_801d388c` case `0xb` accounting: reject overspend, debit `ctx+0x6dc`,
accrue `ctx+0x6d8`), win/lose on the HP fields, and the Seru reward id
(`ctx+0x269 + 0x80`). The course ladder is `parse_course_ladder` +
`course_score_cell`, both reading the raw PROT 0977 entry.

A turn resolves each fighter's **whole** queued string in order - the
player's, then the opponent's - matching how a retail battle turn plays one
actor's `+0x1df` string to completion before the next actor acts. The strings
are not interleaved command-by-command.

Damage goes through one shared kernel, `muscle_dome::DomeDamageModel`,
installed on the session by whichever host started the contest: the
move-power record via the id → index map, the arts/physical predamage roll
(`FUN_801dd0ac`), the element-affinity scale (`FUN_801dd864`) and the damage
finisher (`FUN_801ddb30`), on a PsyQ `rand()` stream in retail call order,
with the defender's `+0x170` gauge accruing per hit. The native play-window
and the browser page resolve through this same kernel; neither carries a
damage rule of its own, and a session with no model installed resolves to no
damage rather than to invented constants.

The **Ra-Seru (magic) command class** is the session's too: `DomeRing` carries
retail's three chip gates and `ChipMark` its three mark emitters, `DomeMagic`
the learned list with its ability-bit price, `commit_cast` the pick (which
clears the direction string and leaves the AP budget alone, because retail's
arm never reads `ctx+0x6D8` / `ctx+0x6DC`), and the debit lands at the
play-out, where the shared band's `0x28` charges it. The outcome runs through
`engine-core::spells::cast_spell` - the same rule an ordinary battle's cast
band folds with. `select_input` is the one selection surface both hosts drive;
the loadout comes from `magic_loadout_for`, the door both native dome entry
paths install through. See
[What makes a Ra-Seru chip render](#what-makes-a-ra-seru-chip-render).

The world hosts the contest as the suspending `SceneMode::MuscleDome`
(play-window `M` key; Left/Right/Up/Down enter the four directions, Triangle
opens the Ra-Seru list, Cross confirms/continues). A KO of the opponent inside
the limit credits the reward Seru through the engine's capture kernel.

The opponent is the disc's own: both hosts resolve `(course, round)` through
`parse_course_ladder` to a monster id and read that monster's PROT 867
record for the stat block the damage kernel takes. The play window has no
course-select screen, so it walks the Beginner course one round per contest;
the browser page fills its foe picker from the ladder. The stand-in constants
survive only as the fallback for a disc whose ladder or archive does not
decode, and a log line says so when they are used.

Documented host models, each disclosed rather than presented as retail:

- The opponent **acts** through the same selection logic, greedily in deal
  order out of the player's own direction deck. Retail has no dome-specific
  AI table, and the monster's own action stream is not modelled - only its
  stats are.
The session bounds a leg by **nothing but a knockout**, which is what retail
does - see [What ends a leg](#what-ends-a-leg-a-knockout-and-nothing-else).
`TIMED_FIGHT_TURN_LIMIT` survives as the numerator of Koru's own countdown
(`timed_fight_turns_left`), reachable by no dome session.

Disc-gated oracles: `engine-core/tests/muscle_dome_minigame_real.rs` (real
deck + the lead's real swing costs drive a leg to a decision through the
world tick), `engine-core/tests/dome_leg_ends_on_ko_real.rs` (the arena's
sole game-mode write is `BattleInit`, the arena holds the only write of the
formation cell and the battle overlay only reads it, and the ladder tops out
below the timed fight's id) and `web-viewer/tests/dome_ladder_and_hub_real.rs`
(the ladder decodes to 29 real monster records, its round counts agree with
the score table, and every hub draw row's cited call site still holds a `jal`
to the emitter it names).

## Open

- The exact phase ordering and meaning of every `ctx+6` value - partially confirmed. The **input chain is now capture-pinned**: `0x1e` menu idle -> `0x28` command cluster -> `0x78` Auto|Command -> `0x50` direction entry -> `0x5a` queue review -> `0x6e` Begin|Reselect -> `0xfe/0xff` playback -> `0x1e` (recomp phase-byte watch across a driven round); the deal/interval arms outside that chain remain to be walked.
- ~~The Auto arm's command picker~~ **resolved**: there is no picker. Auto commits the 16-byte string `FUN_801DA34C` had already reloaded out of the character record (`+0x1A7` / `+0x1B7`, chosen by the `actor+0x156 < actor+0x154` AP-band test) and `FUN_801DA59C` saves back on the review confirm - see [The Auto arm picks nothing](#the-auto-arm-picks-nothing---it-replays-a-saved-string). ~~Still open on the same screen: the pennant/bar geometry for off-class (non-30) costs~~ **resolved** - the pennant is `cost - 6` wide at `x = 16 + spent-AP-before` and the chip recentres by `(cost - 30) * K[slot] / 2`, see [the cost law](#the-pennant-geometry-is-linear-in-the-commands-ap-cost).
- ~~The per-arm assignment of the three UI cue ids~~ **resolved**: `0x21` = accept/confirm, `0x22` = highlight moved, `0x23` = refused-or-back, over **37** call sites (not 34) - see [Which blip is which](#which-blip-is-which-0x21--0x22--0x23) for the per-arm table and the two shared tails.
- A live `_DAT_8007B864` byte-match during a dome contest, to upgrade the arena-backdrop residency (extraction 1225) from Inferred to capture-Confirmed.
- ~~What arms the panel-still load~~ **resolved**: `ctx[+0xC] = 1` is written only by the per-frame battle anim-node tick `FUN_80047430` (`0x800474C4`) for an enemy seat under `gp[+0xA48] & 0x80`, and `ctx[+0x7] == 0x67` is the escape path's entry into the same shared teardown, not a condition of its own - see [What arms the load](#what-arms-the-load). Still open on the same page: no draw site for the VRAM rect the stills land in, and no image outside the loaders names `(384, 0)` at all.
- ~~Which runtime path (if any) draws the backdrop stream's **object-1 dust
  decal**~~ **resolved, and the "phase-gated effect draw" candidate is
  falsified**: no effect path touches it. The SCUS battle scene loader binds the
  whole backdrop TMD to two spawned actors and then removes object index 1 from
  both part lists unless the byte `_DAT_8007B64B` is set - see
  [Object 1 is trimmed by the loader](#object-1-is-trimmed-by-the-loader-_dat_8007b64b).
  Its one writer is the field battle handoff `FUN_801D9E1C`, and a live capture of
  a dome contest closes the last inferred half: the byte reads `0x00` at the
  loader's test and takes zero writes all run, so the trim arm runs.
- The per-step script table `&PTR_DAT_801f4d34` (battle-overlay rodata at file offset `0x2651c`) is fully decoded: the record shape is `[u8 count][u8 anim_sel][u8 panel_id/bind_count]` + `count`×`(elem_id, mode)` (see [Round resolution](#round-resolution)), and the individual sub-draw `elem_id`s are labelled by the `FUN_801d8de8` census in [HUD elements](#hud-elements-fun_801d8de8) (Spirit / move-name panels, the four hand-card portraits, the HP-bar values, and the victory reward banner).
- ~~Which arm of `FUN_801D0CD4` / `FUN_801D0068` decides that a leg was *survived*~~ **resolved**: neither - it is the single byte test `DAT_8007BD60 & 0x80` at `0x801CEDD8`, cleared by the battle's own `0x5A` party-wipe scan and re-raised by the shared minigame-exit routine. `continuing` (`DAT_801D1ADC`) is therefore derived, not prompted: its one raising writer sits behind *course exhausted **and** survived*. See [Which arm decides a leg was survived](#which-arm-decides-a-leg-was-survived).
- ~~The retail *dome* leg-end condition~~ **resolved**: a knockout, and nothing else. The arena hands the round to an ordinary battle (`FUN_801D1510` sets game mode `0x14`) and the only writers of the battle-end signal are the `0x5A` KO scans; the turn counter never reaches them. See [What ends a leg](#what-ends-a-leg-a-knockout-and-nothing-else).
- ~~Whether card resolution applies any dome-specific damage scaling~~ **resolved**: it uses the shared `battle_formulas` unmodified - `FUN_801d0748` is byte-identical to the main battle round driver and a card resolves with no dome-local scaling (see [Round resolution](#round-resolution)). The `FUN_801e09f8` → `FUN_801dd0ac` half of that chain carries the arts / magic ids only; a bare direction swing's tier comes from the melee kernel instead ([why](#the-dd0ac-chain-is-not-a-direction-swings)).

- ~~What makes a Ra-Seru chip render, and why the port crossed it out~~
  **resolved**: three gates, none of them a course table - the member's own
  Ra-Seru marker `ctx[+0x25F + member]`, the sealed-magic status bit
  `actor+0x16E & 0x1000`, and bit `0x200` of the special-battle word
  `0x8007BAC0`. Its two `SCUS_942.54` writers key on the first enemy's monster
  id and never fire for a dome round, but the arena's own entry seed raises it
  for the Master course (`0x321` at `0x801CEBC8`). The red X is that third
  bit's alone, and it
  has two siblings the captures had folded together. See
  [What makes a Ra-Seru chip render](#what-makes-a-ra-seru-chip-render) and
  [the three marks](#three-marks-for-you-cannot-pick-this-and-the-gates-that-raise-them).

## See also

**Reference** -
[Tile-board grid](tile-board.md) ·
[Battle action SM](battle-action.md) ·
[Spell table](../formats/spell-table.md) ·
[Overlay capture](../tooling/overlay-capture.md)
