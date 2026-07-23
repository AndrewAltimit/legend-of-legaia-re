# Noa dance (rhythm) minigame

The dance minigame is a rhythm / step game: a scrolling beat counter advances with the music, a per-lane step chart says which button to press on each beat, and the player presses inside a timing window around each beat. Correct presses fill a "groove" gauge; the gauge selects a harder, higher-scoring chart row. **Three dancers** stand on the floor - the player plus two competitors, who play the *same* chart through the *same* judge off an auto-feed, so their scores are real runs, not a scripted curve. The **Triangle** button is not a direction at all: it is a three-per-song wildcard ("groovy move") that pays its multiplier only when it is spent on a 4-beat combo slot. The run ends when the song timer elapses; the qualifier / finals modes grade the player against a rival's score.

It is one of the minigame-hub family that shares a single overlay binary with fishing / slot / Baka Fighter. This page documents **only** the dance-specific rhythm logic. The shared move-VM, actor-VM, sprite-primitive, and SDK helpers are documented in [`move-vm.md`](move-vm.md) / [`actor-vm.md`](actor-vm.md) and are not re-covered here.

All provenance is `overlay_dance_<addr>.txt` (locally-dumped, Sony-derived; not committed). Confidence is marked **Confirmed** (read directly from the dump) or **Inferred** (consistent reading not pinned by a separate observation).

## Step / rhythm state machine

The per-frame controller is `FUN_801cf470` (the overlay's dance tick). It is a `switch` on the game-state global `DAT_801d5334`. The states form a linear flow with two animation ramps and a play loop:

| State | Role |
|---|---|
| `0` | Title / mode select. Prints the menu prompt lines, moves the cursor, and reads the mode global `DAT_801d514c` (wrapped mod 4 by the `& 0x20` Start edge advancing the state). **Confirmed** |
| `1` | Setup: registers effect bundles, spawns actors, arms the "loading" flag `DAT_801d5830`, and branches the state by mode. **Confirmed** |
| `2` | Wait for the load flag `DAT_801d5830` to clear; then zeroes the run counters (gauge `DAT_801d544c`, etc.) and advances. **Confirmed** |
| `3` | Reset per-dancer run state: the three **triangle-stock** counters `DAT_801d534c`/`+4`/`+8` are set to `3`, the triangle-schedule cursors `DAT_801d574c` cleared, a start voice cue is queued. **Confirmed** |
| `4` | Intro-in: ramps the fade/curtain accumulator `DAT_801d515c` up to `0x3c`. **Confirmed** |
| `5` | Intro-out: ramps `DAT_801d515c` back to 0, then calls the actor-start helper and advances. **Confirmed** |
| `6` | Start BGM, zero the beat counters `DAT_801d581c` / `DAT_801d5820` / `DAT_801d5824`, set the lead-in countdown `DAT_801d513c = 4`. **Confirmed** (the `func_0x80026478` call in this state is the **actor sound-source attach / re-pan** primitive, `FUN_80026478` in [`functions.md`](../reference/functions.md) - it enables the dancer's positional voice, not the BGM stream; the SEQ itself is the pre-staged `music_01` entry loaded by the mode-24 entry path below). |
| `7` | Lead-in countdown: decrement `DAT_801d513c`; when it reaches 0 jump straight to the play state `10`. **Confirmed** |
| `10` (`0xa`) | **Main play loop.** Per-frame the beat counters advance and `FUN_801d231c` draws the HUD; the song-end test runs here. Judging happens in `FUN_801d1af4`, called once per frame **for every dancer** from the actor handler `FUN_801d1358`. **Confirmed** |
| `11` (`0xb`) | "Finish" banner: pushes four banner sprite primitives, advances. **Confirmed** |
| `12` (`0xc`) | Result wipe: ramps `DAT_801d515c`; sets `DAT_801d5130` past a threshold; at the end jumps to results state `0x14`. **Confirmed** |
| `0x14` (20) | Results / grading: copies the per-player scores into display RAM, compares the player score against the win threshold, and sets the win/lose story flag. **Confirmed** |

The block guarded by `DAT_801d5334 - 10 < 3` (states 10/11/12) at the tail of `FUN_801cf470` is the **beat clock**: each frame it adds `DAT_1f800393 * 10` to the beat phase `DAT_801d581c` (wrapping every `0x2320`) and to the total-song accumulators `DAT_801d5820` / `DAT_801d5824`. `DAT_1f800393` is the frame-delta scalar the rest of the engine uses, so the clock is framerate-compensated. **Confirmed.**

Song end (state 10 only): when `DAT_801d5820` reaches the song-length limit - `0x41dc` in one mode, `0x64fc` otherwise - the state advances to `0xb` (Finish). **Confirmed.**

### Mode global `DAT_801d514c`

Four modes, `0..3`. The on-screen state-0 menu prints four labels top-to-bottom at y `0x50`/`0x58`/`0x60`/`0x68` (`s_yosenn`/`s_hosenn`/`s_setumei`/`s_asobi`) with the cursor at y `= value*8 + 0x54`, so value 0 = `yosenn` (予選, qualifier), 1 = `hosenn` (本選, finals), 2 = `setumei` (説明, how-to/demo), 3 = `asobi` (遊び, free play). In normal play the mode is chosen by the *caller* (a field script sets a story flag before entering): state 1 maps flag `0x134 → 0`, `0x135 → 1`, `0x133 → 2`, `0x428 → 3` then clears them. The state-0 cursor menu is the debug/test selector. **Confirmed** (value↔label from the state-0 layout + state-1 flag map; the English glosses of the romaji labels are **Inferred**). See `overlay_dance_801cf470.txt`.

Per-mode behaviour, all read directly from `FUN_801cf470`:

| Value | Behaviour |
|---|---|
| `0` yosenn | Versus. Grading compares the human score `DAT_801d53cc` against score slot 2 `DAT_801d53d4`; a lower score clears the win flag. |
| `1` hosenn | Versus. Grading compares `DAT_801d53cc` against score slot 1 `DAT_801d53d0`; a lower score clears the win flag. |
| `2` setumei | Short how-to/demo: shorter song limit (`0x41dc` vs `0x64fc`), the per-beat dancer-position interpolation at the head of `FUN_801cf470` is suppressed (guarded `DAT_801d514c != 2`), and state 1 routes through the load-wait state 2. Grading clears the win flag when the score exceeds `300`. |
| `3` asobi | Free play: draws the personal-best panel (`FUN_801d2f38` + `FUN_801d32f8` over `_DAT_80084464`) and cycles a start voice via `_DAT_80084468`; the grading switch has **no** branch, so free play sets no win/lose flag. |

The win/lose story flag is `0x50a` (a bit in the `DAT_80085758` flag bank). It is **set** on entry (state 1, `func_0x8003ce08(0x50a)`; `ce08` = set, `ce34` = clear, `ce64` = test - see `8003ce08.txt`/`8003ce34.txt`/`8003ce64.txt`) and **cleared on a loss** in grading (modes 0/1 when the human loses the score comparison; mode 2 when the score tops `300`). Downstream field script tests `0x50a`: set = passed, clear = failed. **Confirmed.**

## Input judging + timing windows

Two routines read the step chart and judge a press; both share the same window arithmetic.

`FUN_801d1820(player)` - the **CPU auto-feed** lookup ("what does this dancer press right now"). Its only caller is `FUN_801d4040`, whose only caller passes a non-zero player index, so it never runs for the human:
- Compute the **intra-beat phase** `DAT_801d581c % 0x119` (mod 281). If it exceeds `0xd2` (210) the function returns 0 (dead zone between beats - no note is active). So each 281-unit beat slot has a ~210-unit acceptance window followed by a ~71-unit gap. **Confirmed.**
- Compute the **beat index** `DAT_801d581c / 0x119`. On a **combo slot** (`beat & 3 == 3`) it checks this dancer's **triangle schedule** - the per-kind table `DAT_801d41e4` indexed by the dancer's triangle cursor `DAT_801d574c[player]` - against the count of combo slots it has banked (`DAT_801d578c[player]`). If the schedule is due it advances the cursor, clears the counter and returns symbol `3` = spend a triangle. **Confirmed.**
- Otherwise return the chart byte `chart_base[lane*0x20 + beat_index]`, where `chart_base` is the step-chart table at `0x801d509c`, `lane` (the chart row, 0..) is `DAT_801d544c[player] / 1000` (the dancer's own groove gauge selects its difficulty row), and each row is `0x20` (32) bytes = 32 beats. **Confirmed.**

`FUN_801d4040(player)` maps the symbol to a pad-mask bit: symbol `1 → 0x80` (Square), `2 → 0x20` (Circle), `3 → 0x10` (Triangle), else 0. The first two are the judged directions; the third is the [wildcard](#the-triangle-wildcard-the-groovy-move). **Confirmed** (mapping + button identity: the setumei script's own how-to lines name the Triangle as the three-times-only groovy move).

`FUN_801d1960(player, lane, variant)` - the actual hit-judge, called from the `0x80` / `0x20` branches of the award routine:
- Same dead-zone test (`phase % 0x119 > 0xd2` → return 0 = miss). **Confirmed.**
- Compute the **accuracy weight** `w = 0x1000 - (phase * 0x1000) / 0xd2`, a `0..0x1000` ramp that is maximal at phase 0 (dead-on the beat) and decays to 0 at the window edge. Stored in `DAT_801d6090`. **Confirmed.**
- Look up the chart symbol at `chart_base[lane*0x20 + beat_index]` and compare it to the pressed direction `(pressed & 0xf) + 1`. If they don't match → return 0 (wrong direction). **Confirmed.**
- If they match: advance the per-player chain cursor `DAT_801d550c[player]`. If the cursor closes the lane's chain (`cursor + 1 == lane + 1`, i.e. `lane + 1` matched notes in a row) the routine resets the cursor, sets the gauge award `DAT_801d6088 = 0xfa` (250), computes the **bonus points** `DAT_801d608c` from the per-kind value table `DAT_801d41a4`, and **returns 2** (chain closed). Otherwise **returns 1** (a matched note that has not closed the chain). **Confirmed.**
- The bonus is **accuracy-weighted for the human only**: player 0 gets `base/2 + (base * w) >> 13` (half the table value at the window edge, the full value dead on the beat); a CPU dancer takes the flat table value. **Confirmed.**

So the judge has three tiers:
- **Miss** (0): outside the window, or wrong direction.
- **Hit** (1): correct direction inside the window - a matched note that has not closed the chain. **It scores nothing**; it advances the cursor.
- **Chain closed** (2): the note that completes `lane + 1` matched notes, awarding the bonus from `DAT_801d41a4` and stepping the gauge `+250`.

There is no separate Perfect/Good text tier in the judge; the *quality* of a hit is carried continuously by the accuracy weight `w`. The chain cursor `DAT_801d550c` is cleared every 8 beats (`FUN_801d1358`: `beat & 7 == 0`), so a chain must be closed inside one 8-beat bar.

### The two chart lookups diverge on exactly the beats that carry notes

`FUN_801d1820` (the CPU feed) and `FUN_801d1960` (the judge) read the **same** chart with different rules on the `beat & 3 == 3` slots: the feed can substitute the triangle symbol `3` when the dancer's schedule is due, the judge always reads the raw cell. That is not a corner case in the retail chart - the **lane-0 row places every one of its steps on a 4-beat boundary** (`0,0,0,1, 0,0,0,2, ...`), so lane 0's notes sit exactly on the combo slots.

The consequence for any host that renders the chart: **the note the player must press is the judged cell**, not `FUN_801d1820`'s return. The engine mirror surfaces both - [`DanceGame::judged_symbol`] (the judge's source) and [`DanceGame::required_symbol`] (the CPU-feed / display source) - and the site's playable dance drives its note highway off the former.

## The triangle wildcard (the "groovy move")

The Triangle button (`0x10`) has its own branch in the award routine, and it **matches no chart cell at all**. It is the game's wildcard, and the setumei (how-to) script teaches it as one: its lesson lines tell the player to press it together with the last command of a combo, and that it is good for only three uses a song. `FUN_801d1af4`'s `0x10` branch, in order (**Confirmed**):

1. Gate on the hit-tier latch `DAT_801d548c[player]` being clear (nothing else being judged) **and** on the stock counter `DAT_801d534c[player]` being non-zero. The stock is set to **3** for every dancer at run start (`FUN_801cf470` state 3) and is **never replenished** mid-song.
2. Decrement the stock.
3. If the press lands on a **combo slot** (`beat & 3 == 3`) inside the window (`phase < 0xd2`): set the "landed" flag `DAT_801d570c[player] = 1`, add `(lane + 1) * 0x19` (25) to the score, and step the groove gauge a full **`+1000`** - which by itself promotes the dancer a difficulty lane. Otherwise: clear the flag and add only `(lane + 1) * 3`.
4. Return the move-clip index `lane*2 + 0x10` (the on-beat step clip, move pair `8 + lane`), set the spin counter `DAT_801d564c[player] = lane + 1`, latch `DAT_801d548c = 3` / `DAT_801d54cc = 0xf`, and arm the feedback window `DAT_801d5144 = 0x3c` (60).

The **input disruption** is step 4. The dancer now spins `lane + 1` full turns - `FUN_801d1358` advances its yaw by `(lane * 0x20 + 0x80) * frame_delta` per frame and decrements the spin counter every `0x1000` - while playing the returned move clip. The award routine is only called at all when the dancer's current clip is its idle or dance-groove loop, so **for the whole move nothing that dancer presses is judged**: 32 / 51 / 64 frames of dead input by lane. The `0x3c` feedback window is what the tutorial reads to caption the spend: for its duration the Disco King's script (`FUN_801d0750` case `0xd`) prints its praise line when `DAT_801d570c` says the triangle landed on the combo slot, and its timing-scold line when it did not.

So the wildcard's design is exactly the one the tutorial teaches: it is worth `8.3x` an ordinary press *and* a free lane promotion, but only on the combo slot, and it costs you the next second of input - so you spend it on the **last** note of a combo, not the first, and you save it for the lane where `(lane + 1)` is biggest.

The `× 0x22` (34) multiplier in the same branch is **not** a "perfect" tier: its selector is `DAT_801d5334 - 0xb < 2`, i.e. the **game state** being 11 or 12 - the post-song Finish banner / result wipe, where the pad is still read. **Confirmed** (`DAT_801d5334` is the state global the whole tick switches on).

## Scoring

`FUN_801d1af4(player)` is the score/award routine, and it is the **whole floor's** award routine: `FUN_801d1358` calls it once per frame for every spawned dancer. Its first act is to pick the pad word - the live pad `_DAT_8007b874` for player 0, and `FUN_801d4040(player)` (the chart auto-feed) for every other dancer. It guards on the play states (`DAT_801d5334 - 10U < 3`) and on the not-paused flag `DAT_801d5130`. **Confirmed.**

Key accumulators (all per-player, `player * 4` stride from the listed base):

- **Score** `DAT_801d53cc[player]`, clamped to `999` (`0x3e7`). Two sources, and only two (**Confirmed**):

  | Source | Award |
  |---|---|
  | Triangle (`pad & 0x10`) on a combo slot, in window | `(lane + 1) * 0x19` (25) + gauge `+1000` |
  | Triangle anywhere else | `(lane + 1) * 3` |
  | Direction (`pad & 0x80` / `& 0x20`) closing the lane's chain | `+ DAT_801d608c` (the kind's bonus, accuracy-weighted for the human) + gauge `+250` |
  | Direction that matches but does not close the chain | **nothing** (the cursor advances) |
  | Direction that misses | nothing (the miss counter `DAT_801d568c` rises) |

- **Groove gauge** `DAT_801d544c[player]`: `+1000` on a landed triangle, `+250` (`DAT_801d6088`) on a closed chain, clamped to `[0, 2999]`. The chart row is `gauge / 1000`, so crossing 1000 / 2000 promotes the dancer to the next (denser, higher-scoring) row. **No path in the overlay lowers it** - not on a miss, not on a dropped chain; the Disco King's tutorial says as much ("the level rises automatically"). **Confirmed** (every writer of `DAT_801d544c` in the image is an award, a clamp, or a run/tutorial reset).
- **Triangle stock** `DAT_801d534c[player]`: 3 per song - see [the wildcard](#the-triangle-wildcard-the-groovy-move). The HUD reads it directly to draw the remaining markers (`FUN_801d2524`).
- **Per-player hit-tier latch** `DAT_801d548c[player]` (0/1/2/3) and timer `DAT_801d54cc[player]` (`0xf`, decayed `2 * frame_delta`): every judged press latches the dancer so a held button isn't re-judged every frame. **Confirmed.**

Player 0 additionally drives feedback: a sound cue (`_DAT_8007b6de`/`_DAT_8007b6d8` write a sequence id), a hit/combo banner via the sprite emitter `FUN_801d3fd0`, and the dancer pose switch `FUN_801d03c4`. **Confirmed.**

### The rival dancers score for real

The two competitors are **not** on a scripted score curve. They run the same award routine, off a pad word synthesised from the same chart, and differ from the human only by their **kind** (`DAT_801d540c[slot]`, stamped from the spawn table by `FUN_801d0190` - so it is also the mesh / face-rig id). The kind indexes two baked tables that sit in the overlay's rodata just ahead of the step chart:

| Table | Shape | Role |
|---|---|---|
| `DAT_801d41a4` | `kind * 0x10 + lane * 4`, i32 | **Sequence-bonus points.** Each retail row is `k, 2k, 3k` - the `(lane + 1)` scaling is baked into the data. Kind 0 (Noa) is the richest row, the competitor kinds are progressively poorer. |
| `DAT_801d41e4` | `kind * 0x40 + n * 4`, i32 | **Triangle schedule.** Combo slots the dancer must bank (`DAT_801d578c`, incremented once per `beat & 3 == 3` by `FUN_801d1358`) before spending its `n`-th groovy move. A huge value = never. Kind 0's row is all zeros - the human's triangles are pressed, not scheduled. |

A CPU dancer therefore hits every note its own lane calls for (so its gauge climbs and promotes it), takes the flat table bonus for each closed chain, and throws its groovy moves on the disc's own schedule. Its press cadence is limited by the same latch as the human's plus the move clip it is playing.

Parser: [`legaia_asset::dance_chart::parse_tables`]. **Confirmed** (both tables read straight off the disc; the shapes are pinned by the index arithmetic in `FUN_801d1960` / `FUN_801d1820`).

### Win / lose threshold

In results state `0x14`, `FUN_801cf470` copies the three player scores into display halfwords (`DAT_801c6460..`) and then, by mode, decides the outcome (see the [per-mode table](#mode-global-dat_801d514c) for the exact comparison each mode runs):
- Modes 0/1 compare the human score `DAT_801d53cc` against the opponent slot (`DAT_801d53d4` / `DAT_801d53d0`); mode 2 tests it against the fixed threshold `300` (`0x12d`); mode 3 grades nothing. **Confirmed.**
- Flag `0x50a` is **set** at entry (state 1) and **cleared on a loss** here via `func_0x8003ce34(0x50a)` (`ce34` = clear). Set = passed, clear = failed; free play (mode 3) leaves it untouched. **Confirmed.**

The high score is tracked in the save block (`_DAT_80084464` is updated when `DAT_801d53cc` exceeds it). **Confirmed.**

## The setumei (how-to) tutorial script (`FUN_801d0750`)

Mode 2 (setumei) is driven by a dedicated actor script - the Disco King's
per-frame state machine, an 18-case `switch` on the actor's own step counter
(`param+0x9c`). It teaches the game with dialogue lines (`func_0x80036888`,
drawn at fixed y `0x78`/`0x88`/`0x98`) and advances on any face button
(`_DAT_8007b874 & 0xf0`, keying the confirm cue `0x20`). Between segments it
resets the shared dance run-state, so it is also the mode's controller, not
just a caption printer. **Confirmed** (`overlay_dance_801d0750.txt`).

The script's state-mutating beats, in order: a cursor-move demo (`DAT_801d607c`,
toggled by pad `0x1000`/`0x4000`); text pages 1..4 explaining the buttons; a
"try Square / Circle now" segment (case 7) that probes the human's press with
two direct `FUN_801d1960(0, gauge/1000, 0x100)` / `0x101` calls and praises a
correct one, timed by the progress accumulator `DAT_801d5150` reaching `900`; a
"level rises automatically" page that arms the load gate (`DAT_801d5830 = 1`,
`DAT_801d5334 = 2`); the groovy-move lesson; and a groovy-move practice segment
(case `0xd`) that reads the triangle-landed flag `DAT_801d570c` to print praise
vs a timing scold, gated by the feedback window `DAT_801d5144`. Two cases
(`0xc` / `0x11`) are countdown holds on the timer `DAT_801d6080` that clear the
beat clock (`DAT_801d581c` / `DAT_801d5820` / `DAT_801d5824`) before advancing.
This is presentation-bound (its output is Sony dialogue) and not ported; the
mutations it makes are on the same run-state globals the [RAM table](#ram-state)
lists.

## Dance-floor rendering

The scrolling dance floor (the step-lane grid the markers travel down) is drawn by a small cluster that **reuses the field engine's scene infrastructure** rather than a dance-specific renderer: the scene-data base `_DAT_1f8003ec` (the scratchpad-resolved per-scene pointer, the same one whose `+0x4000` slice is the field walkability grid - see [`field-locomotion.md`](field-locomotion.md)) and the actor-list head `_DAT_8007c36c`. The dance step layers live at fixed offsets inside that scene data: a tile grid at `+0x8000` and two step-marker layers at `+0x10000` / `+0x12000`. **Confirmed** (the offsets and the shared bases are read directly from the dump).

This cluster is what the live trace surfaced as the resident mode-24 code (the `game_mode 0x18` hits in a dance-minigame save); the pin to the dance overlay is the resident slot-A help text (`"how to dance?"` / `"Disco King"`). Note one trace artifact: the gap-set listed several *interior* addresses of `FUN_801d2a10` (`0x801d2b44`, `0x801d2c98`, `0x801d2cfc`, `0x801d2d1c`, `0x801d2d2c`) as separate entries, because sibling slot-A overlays (menu / field) have real function entries at those VAs; in the resident dance overlay they are loop-body PCs of the one function.

| Function | Role | Confidence |
|---|---|---|
| `FUN_801d3a2c` | **Per-frame floor draw pass.** Clears `DAT_801d6084`, then (when not paused: `(_DAT_8007b868 & 2) == 0 && _DAT_8007b8b8 == 0`) walks the actor list `*_DAT_8007c36c` emitting each actor via the shared sprite/actor helpers (`func_0x80024dfc` / `func_0x800204a4`; actors flagged `[4] & 0x800` also call `func_0x80017b94(actor[0x11])`), then sweeps the tile grid (bounds `DAT_1f800384..387`, cells at `_DAT_1f8003ec + ... + 0x8000`, `0x20`-byte records). | Confirmed (structure); the exact per-cell emit is Inferred |
| `FUN_801d2a10` | **Floor tile-grid blit.** Builds a 16-entry per-column Y-offset table in scratchpad (`0x1f800332+0x48`, `0x1e0` stepping `-0x20` = a 16-row column at 32-unit spacing), then nested-loops a rect (`param_1..param_3` x, `param_2..param_4` y; `param_2 << 8` fixed-point) emitting the floor quads. Hit at entry and at its loop-body PCs. | Confirmed (structure) |
| `FUN_801d3ec0` | **Two-layer step lookup.** Calls `FUN_801d3f54` against scene-data layer `+0x10000`; on a miss, retries against layer `+0x12000`. So a floor cell can carry a marker in either of two overlaid step layers. | Confirmed |
| `FUN_801d3f54` | **Per-cell step-marker lookup.** Indexes a per-row header at `base + row*4` (count at `+4`, sub-list offset at `+2`), walks the sub-list (per-row stride from a table at `row - 0x7ff84ce8`) and returns the record whose first two bytes match the requested `(x, y)`, else NULL. | Confirmed |

The lookup pair (`FUN_801d3ec0` → `FUN_801d3f54`) is the read side of the step chart in *screen space* - "is there a marker at floor cell (x,y) right now" - complementing the *beat-space* chart read in [Input judging](#input-judging--timing-windows) (`FUN_801d1820`, which indexes the baked chart `DAT_801d509c` by beat). The two are the same step data addressed two ways: by time (judging) and by floor position (rendering).

## RAM state

All globals live in the overlay's data region around `0x801d5xxx`/`0x801d6xxx`. Per-player arrays use a `player * 4` stride from the listed base (3 dancers).

| Global | Width | Role | Confidence |
|---|---|---|---|
| `DAT_801d5334` | u32 | Game-state selector for `FUN_801cf470` (states 0..0x14) | Confirmed |
| `DAT_801d514c` | u32 | Mode `0..3`: 0 yosenn / 1 hosenn (versus) · 2 setumei (how-to) · 3 asobi (free play). Set from a story flag in state 1 | Confirmed |
| `DAT_801d5130` | u32 | Pause / suppress-input flag (judging skipped when set) | Confirmed |
| `DAT_801d5830` | u32 | "Loading / pre-roll" gate (state 2 waits on it clearing) | Confirmed |
| `DAT_801d513c` | u32 | Lead-in countdown (state 7) | Confirmed |
| `DAT_801d515c` | u32 | Intro/result fade-curtain ramp accumulator | Confirmed |
| `DAT_801d581c` | u32 | **Beat phase counter**; `% 0x119` = intra-beat phase, `/ 0x119` = beat index; wraps at `0x2320` | Confirmed |
| `DAT_801d5820` | u32 | **Total-song timer**; song ends at `0x41dc` / `0x64fc` | Confirmed |
| `DAT_801d5824` | u32 | Secondary bar/loop counter (wraps `0x464`) | Confirmed |
| `DAT_801d5138` | u32 | Beat-clock hold flag (freezes `DAT_801d5820` advance when set) | Inferred |
| `DAT_801d53cc[]` | u32×3 | **Per-dancer score**, clamped to `999` | Confirmed |
| `DAT_801d544c[]` | u32×3 | **Groove gauge**, clamped `[0,2999]`; `/1000` selects chart row. Never lowered | Confirmed |
| `DAT_801d534c[]` | u32×3 | **Triangle stock** (3 per song, no refill); gates the `0x10` wildcard branch, does not end the run | Confirmed |
| `DAT_801d540c[]` | u32×3 | Dancer **kind** (spawn-table id); the row index of both scoring tables | Confirmed |
| `DAT_801d548c[]` | u32×3 | Current-step hit-tier latch (0/1/2/3); non-zero = not judged | Confirmed |
| `DAT_801d54cc[]` | u32×3 | Hit-tier latch timer (`0xf`, `-2 * frame_delta`) | Confirmed |
| `DAT_801d550c[]` | u32×3 | Direction-chain cursor (advanced per matched note; cleared every 8 beats) | Confirmed |
| `DAT_801d564c[]` | u32×3 | Groovy-move spin turns left (`lane + 1`); the input-disruption window | Confirmed |
| `DAT_801d570c[]` | u32×3 | Last triangle landed on a combo slot (drives the banner + tutorial caption) | Confirmed |
| `DAT_801d574c[]` | u32×3 | CPU triangle-schedule cursor (index into `DAT_801d41e4`'s kind row) | Confirmed |
| `DAT_801d578c[]` | u32×3 | CPU combo slots banked since its last triangle | Confirmed |
| `DAT_801d5144` | u32 | Triangle feedback window (`0x3c`, `-frame_delta`) | Confirmed |
| `DAT_801d56cc[]` | u32×3 | Current dancer pose / animation id (`FUN_801d03c4`) | Confirmed |
| `DAT_801d568c[]` | u32×3 | Miss / wrong-press counter | Inferred |
| `DAT_801d6088` | u32 | Bonus-window timer set on a completed sequence | Confirmed |
| `DAT_801d608c` | u32 | Computed sequence-bonus points (weighted by accuracy) | Confirmed |
| `DAT_801d6090` | u32 | **Accuracy weight** `0..0x1000` (peaks on-beat) | Confirmed |
| `DAT_801d509c` | bytes | **Step chart table** (3 rows × `0x20` beats; `row*0x20 + beat`, byte = direction symbol 0/1/2/3). **Baked** into the overlay static image (PROT 0980 file offset `0x6884`), not loaded per song. | Confirmed |
| `DAT_801d41a4` | i32 table | **Sequence-bonus points**, `[kind][lane]` (rows `k, 2k, 3k`) | Confirmed |
| `DAT_801d41e4` | i32 table | **CPU triangle schedule**, `[kind][n]` = combo slots banked before the n-th groovy move | Confirmed |
| `DAT_801d43a0` | i16 table | Per-step world/screen anchor positions (HUD step interpolation) | Inferred |
| `DAT_801d583c` | i16 table | Easing LUT used to interpolate the dancer/marker between beats | Inferred |

The "dance points" cheat anchor at `0x801d53cc` (see [`../reference/cheats.md`](../reference/cheats.md)) is exactly `DAT_801d53cc[0]` - the human player's score. **Confirmed.**

## Key functions

| Function | Role |
|---|---|
| `FUN_801cf470` | Per-frame dance controller / state machine (the dance tick); beat clock + song-end test + grading. `overlay_dance_801cf470.txt` |
| `FUN_801d1af4` | Per-dancer score / award routine (**every** dancer): reads input (human pad for player 0, chart auto-feed otherwise), judges directions, spends triangles, drives the gauge, score, banners and pose. `overlay_dance_801d1af4.txt` |
| `FUN_801d1960` | Hit judge: dead-zone + accuracy-weight + chart-direction match → 0 miss / 1 matched / 2 chain closed (with the kind's bonus). `overlay_dance_801d1960.txt` |
| `FUN_801d1820` | CPU auto-feed: the symbol a competitor presses this beat (its lane's chart cell, or the triangle when its schedule is due). `overlay_dance_801d1820.txt` |
| `FUN_801d4040` | Chart symbol → pad-mask bit (`1→0x80` Square, `2→0x20` Circle, `3→0x10` Triangle = the wildcard). `overlay_dance_801d4040.txt` |
| `FUN_801d231c` | Score / gauge HUD render (per-player score + groove gauge via the sprite emitter). `overlay_dance_801d231c.txt` |
| `FUN_801d03c4` | Dancer face-pose switch driven by hit results (the eye/mouth MoveImage stamp). `overlay_dance_801d03c4.txt` |
| `FUN_801d0190` | Dancer spawner: per-mode spawn table + kind descriptor table → actor list (see [Dancer bodies](#dancer-bodies-the-retail-cast--choreography-tables)). `overlay_dance_801d0190.txt` |
| `FUN_801d1358` | Per-dancer actor handler: binds idle / the dance loop, applies the judge-returned move clip + translucency bit, then hands to the shared clip driver `FUN_800204F8`. `overlay_dance_801d1358.txt` |
| `FUN_801d2f38` | Textured-quad sprite emitter (HUD digits / banners / gauge); shared presentation helper. `overlay_dance_801d2f38.txt` |
| `FUN_801d3a2c` | Per-frame dance-floor draw pass (actor list + tile-grid sweep). See [Dance-floor rendering](#dance-floor-rendering). `overlay_dance_801d3a2c.txt` |
| `FUN_801d2a10` | Dance-floor tile-grid blit (scratchpad column-Y table + rect quad emit). `overlay_dance_801d2a10.txt` |
| `FUN_801d3ec0` | Two-layer step-marker lookup wrapper (scene-data `+0x10000` / `+0x12000`). `overlay_dance_801d3ec0.txt` |
| `FUN_801d3f54` | Per-cell step-marker lookup (per-row sub-list, match `(x, y)`). `overlay_dance_801d3f54.txt` |
| `FUN_801d0750` | The **setumei (how-to) tutorial script** - the Disco King actor's per-frame state machine. See [The setumei tutorial script](#the-setumei-how-to-tutorial-script-fun_801d0750). `overlay_dance_801d0750.txt` |
| `FUN_801d231c` | Score / gauge HUD render driver: per-mode score-box → dancer-slot layout, then draws each box (`FUN_801d32f8`), the gauge level (`FUN_801d3e28`) and the beat track (`FUN_801d2524`). `overlay_dance_801d231c.txt` |
| `FUN_801d32f8` | Multi-digit number renderer: 8-place decimal split (leading-zero suppressed) → per-digit widget-U patch + emit. `overlay_dance_801d32f8.txt` |
| `FUN_801d2524` | Beat-track HUD: combo-window CLUT flash, the scrolling-note screen-x, the caps / body / stock-marker draws. `overlay_dance_801d2524.txt` |
| `FUN_801d2d98` | Count-in banner animator (`1 2 3 READY... GO!`): slide-in / hold / fade envelope + fires the intro cue `0x200` on frame `0x1e`. Envelope ported as [`dance_countin_banner_envelope`]. `overlay_dance_801d2d98.txt` |
| `FUN_801d3d78` | On-beat "good step" sting: keys two SPU voices (`0x12` / `0x13`) at tones `2r` / `2r+1`, note `0x3c+r`, for `r = rand() % 3`. `overlay_dance_801d3d78.txt` |
| `FUN_801d40dc` | Sequence-clear ("Good!") banner + two flanking stars carrying the accuracy weight (`+0x72`). `overlay_dance_801d40dc.txt` |
| `FUN_801d4098` | Actor clip-driver gate: hands the dancer to the shared clip driver `FUN_800204f8` only when its spin counter `+0x5c > 0` or its flag word `+0x10` has bit `0x1000`. Predicate ported as [`dance_clip_driver_gate`]. `overlay_dance_801d4098.txt` |
| `FUN_801d387c` | Per-dancer sprite/shadow emit dispatch: computes the fade alpha from the dancer's beat field `+0x78` (`(v > 0x4000 ? 0 : v) >> 4`, clamped `0..0xff`) then `switch`es on a draw mode `0..4`, pushing the dancer's marker/shadow quads through the hub sprite emitter `FUN_801d2f38` (semi-transparency flags `0x400` / `0x800`) or copying the transform template `DAT_801d51a0`. Render-track; documented, not ported. `overlay_dance_801d387c.txt` |
| `FUN_801d414c` | Dance scene-name stager / teardown: copies the `other1` scene-name string (`s_other1_801d518c`) into the scene-name buffer `0x80084548`, clears the pad-latch `_DAT_8007b880`, stores `DAT_801d5180` into `_DAT_80084540`, calls the scene-setup helper `FUN_80026018`, and arms `_DAT_8007ba9c = -1`. Called once from the dance tick `FUN_801cf470`. Engine scene plumbing, not rhythm logic; documented, not ported. `overlay_dance_801d414c.txt` |

Parser: [`legaia_asset::dance_chart`](../../crates/asset/src/dance_chart.rs) decodes the baked [step chart](#step--rhythm-state-machine) (3 rows × `0x20` beats) from the disc.

Engine port:
[`legaia_engine_core::dance`](../../crates/engine-core/src/dance.rs) is the
clean-room rules engine driven by that parsed chart + [scoring
tables](#the-rival-dancers-score-for-real) - the beat clock (`FUN_801cf470`),
the per-dancer handler (`FUN_801d1358`: latch decay, chain-cursor reset,
combo-slot banking, the CPU auto-feed), the timing-window hit judge
(`FUN_801d1960`), the CPU chart lookup (`FUN_801d1820`), the symbol→pad-bit
map (`FUN_801d4040`), and the score / gauge / triangle award (`FUN_801d1af4`),
all with the Confirmed constants above. It runs the **whole floor**:
`DanceGame::press` judges the human, `advance` steps the clock and runs the
competitors' auto-fed presses through the same award path, and
`dancer_score(i)` / `dancer_triangles(i)` expose their live runs.

The floor's size is a property of the **mode**, not a constant.
`DanceGame::from_overlay_for_mode` ports `FUN_801d0190`'s selection: each mode
picks a spawn table *and* how many of its records to spawn, so free play seats
**six** dancers and the how-to demo a single one, against three in the two
competitive modes. `DanceMode` carries that mapping, and the how-to mode also
forces the short song. `DanceGame::from_overlay` stays the qualifier entry
point.

Runtime wiring: the engine host installs the rules engine as a suspending scene mode (`SceneMode::Dance`; `World::enter_dance` / `tick_dance` / `exit_dance`). The `play-window` viewer starts it from the `K` key (loads the dance overlay PROT 0980, `DanceGame::from_overlay`), maps the three arrows to the retail pad bits (Left/Right/Up = symbols `1`/`2`/`3`), and draws the score / groove-gauge / active-lane HUD; the song timer ends the run and restores the interrupted scene.

`DanceGame::press` returns the full event (Miss / Hit / Sequence with its
points / **Groovy** with its landed flag, lock frames and remaining stock /
NoCharge / Ignored-while-spinning) and applies the score, gauge and latch side
effects; `judge_press` folds it to the legacy three-way result for hosts that
only want Miss/Hit/Sequence. `DanceGame::from_overlay` starts a run straight
off the disc - chart, both scoring tables, and the qualifier cast's dancer
kinds (disc-gated `dance_minigame_real` auto-plays the real chart end to end
and drives a hands-off run to watch the rivals score). The one thing the rules
layer approximates is the *length* of the input-disruption window: retail
gates re-judging on the move clip's own playback, and the port times it off
the dancer's spin (`lane + 1` turns at `0x80 + lane * 0x20` per frame), which
is the same disc-derived formula for the visible move. The dance-floor / arrow
rendering (the [floor cluster](#dance-floor-rendering)) is not part of the
rules port - it is a separate host concern.

Alongside the rules layer the port also lifts the **HUD parameterisation** -
the disc-derived arithmetic a HUD draw is fed, kept in the rules crate while the
quad emit stays the host's (as [`step_mark_effect_spawn`] / the score-banner
digit already split their render routines): the number renderer's decimal split
(`dance_number_digits` / `dance_score_digit_u` / `dance_level_digit_u`,
`FUN_801d32f8`), the beat-track combo-flash CLUT + scrolling-note screen-x
(`dance_combo_window_bright` / `dance_beat_track_note_x`, `FUN_801d2524`), the
good-step sting's two-voice pick (`dance_hit_sting_voices`, `FUN_801d3d78`), the
sequence-clear banner + star spawns (`good_banner_spawn`, `FUN_801d40dc`), the
face-stamp rig selector (`dance_face_rig`, `FUN_801d03c4`), the count-in banner's
slide/hold/fade envelope (`dance_countin_banner_envelope`, `FUN_801d2d98`), and
the per-dancer clip-driver gate (`dance_clip_driver_gate`, `FUN_801d4098` - drive
the shared clip when the dancer's spin counter is positive or its flag word
carries bit `0x1000`).

## Assets: the overlay loads none - the entry path stages PROT 1230

The dance overlay (extraction PROT 0980) issues **no texture load and no mesh
load at all**. A full sweep of the 32 KB image finds no `jal 0x8003eb98` (PROT
entry load), no `jal 0x8001f05c` (asset dispatcher) and no `jal 0x800198e0`
(TIM -> VRAM), and it never touches the global TMD pool `DAT_8007C018`. It has
exactly three PROT loads, all sound:

| raw | extraction | role |
|---|---|---|
| `0x4D1` | **1231** | the dance's SFX sample bank (`VABp`) |
| `0x41A` | **1048** | BGM (`music_01` #60 `M116` "Sol disco final 1") |
| `0x420` | **1054** | the alternate BGM (`music_01` #66 `M120` "Sol disco final 2"; a branch on `DAT_801D514C` picks the song) |

Both BGM entries are genuine `music_01` Sol-disco finals - the natural fit for
the Sol dance floor. The bank map is **piecewise** (extraction = `988 + index`
for sound-test index `<= 67`), which is what places these at #60/#66; an
earlier reading based the whole bank at extraction 990 and so mis-labelled
them as slots 58/64 (`M114` "ordinary town 2" / `M119` "opening character act
2"), titles that read oddly for a disco - that was the +2 base skew, now
corrected (see [`../reference/music-tracks.md`](../reference/music-tracks.md)).
The disc bytes fit: **both entries are short ~33-beat loops** (extraction 1048
= 291 notes over 15 840 ticks, 1054 = 266 over 15 860; both at 480 ppqn ≈ one
32-beat step-chart cycle) sized to the step chart. The full Sol-disco *floor*
set (`M112`/`M115`/`M116`/`M120`) is also the host casino scene's op-`0x35`
BGM around the minigame; the site's dance page offers them as a jukebox on top
of these two.

The art it draws with is nevertheless dance-specific: the mode-24 entry path
stages **extraction PROT 1230** (`other7`, a `prot::timpack` of **31 TIMs** -
parser [`legaia_asset::dance_art`](../../crates/asset/src/dance_art.rs)):

| VRAM rect | content |
|---|---|
| `(512, 0)` 4bpp, CLUT strip `(0, 500)` 256x1 | the **HUD page**: blue digit font, `Lv.` cells, score box, beat-track parts, note dots, `1 2 3 READY... GO! FINISH!`, and the `Miss! / Good! / Cool! / Great!! / Fever!!! / Chicken!!` banners. The 16 palettes of the row-500 strip are the CLUT ids `0x7D00..0x7D0F` the widget table names |
| `(400, 0)` / `(416, 0)` / `(432, 0)` 16hw x 128 | the three **dancer face strips** (live window on top, 4-pose eye/mouth bank in rows 64..128) |
| `(320..384, 0..192)` 16hw x 64 cells | face-part cells (alternate expressions) |
| `(512..832, 0/256)` 64hw x 256 pages | the dance hall's **venue textures**: floor tiles, brick / speaker / crate walls, the disco ball, spotlight beam cones, the crowd, dancer body art |

27 of the 31 image blocks (and the HUD CLUT row) are **byte-identical to a
live retail VRAM capture** parked in the minigame; the four that differ are
exactly the face strips whose live window the pose blit rewrites (see below) -
the diff rows are the blit destination rows, which confirms the mechanism in
pixels. **Confirmed.**

PROT 1230/1231 sit against the PROT TOC's zeroed tail, where the indexed size
formula `toc[p+5] - toc[p+3] + 4` underflows; the TOC readers fall back to the
LBA footprint for them (`legaia_prot::archive`).

### Dancer bodies: the retail cast + choreography tables

The overlay issues no mesh load, but it *names* every dancer: the spawner
`FUN_801d0190` reads a per-mode **spawn table** (`0x801D4D5C` mode 0/2,
`0x801D4D8C` mode 1, `0x801D4DBC` mode 3; 0x10-byte records
`[u32 kind, x, y, z]`) and a 5-record × `0x80`-byte **kind descriptor table**
(the table alone does not fix the cast: the same switch also loads the spawn
**count** into `$s3` - 3 for modes 0/1, **1** for mode 2, **6** for mode 3 -
so mode 2 takes only the qualifier table's first record and free play seats
six)
at `0x801D4E1C`. Parser: [`legaia_asset::dance_cast`]. Per descriptor: `+0xC`
mesh id, `+0x10`/`+0x14` pre-game idle anim + rate, `+0x18`/`+0x1C` the
in-play dance-groove loop, `+0x28..+0x80` eleven `[anim | flags, rate]` **move
pairs** the judge triggers (anim bit `0x200` = draw translucent). Kind 0's
mesh id is written *without* the scene TMD base `hw(0x8007B6F8)`, so it
indexes the resident global pool; the others get the base added, so they are
scene-pool indices in the MAN model-byte space.

The clips resolve against the **dance-hall scene module** - CDNAME block
`other7` (raw TOC `0x4CC`, extraction 1226..; the same block that carries the
dance's `efect.dat` PROT 1228 and art pack PROT 1230). Its first MOVE section
is a **60-record ANM bundle (PROT 1229)** and the descriptor anim ids are
placement-space ids into it (`record = id - 1`), pinned by the bone-count
partition being exact:

| kind | mesh | rig bones | anim ids (records) | identity |
|---|---|---|---|---|
| 0 | global pool slot 1 | 10 | idle 6, dance 18, moves 7..17 (recs 5..17) | **Noa** - her own field-view model (PROT 0874 §0 slot 1); the rig-0 face stamp reads her field atlas |
| 1 | scene TMD 58 | 11 | idle 47, dance 51, moves 48..58 | **Mary** - face-strip rig 1 (`(400,0)`); koin3's Mary (its model 63) shares her CLUTs `(192/208, 480)` |
| 2 | scene TMD 62 | 12 | idle 33, dance 36, moves 37..46 | dancer NPC - rig 2 (strip `(416,0)`, CLUT `(224,480)`); koin3 twin model 67 |
| 3 | scene TMD 61 | 12 | idle 19, dance 31, moves 20..30 | dancer recolor - rig 3 (strip `(432,0)`, CLUT `(224,481)`); koin3 twin model 66 |
| 4 | scene TMD 63 | 10 | idle 59, dance 60 (moves all 60) | **Disco King** (koin3 twin model 71) - the setumei demo dancer, also mode-2's extra spawn |

So the AI dancers are **dedicated dancer NPCs, not party members** - the
earlier "the two AI dancers are Vahn / Gala" reading is falsified by the
descriptor table. The floor casts per mode: yosenn = Noa (centre, `x 0x1800`)
+ kinds 2/3 flanking (`0x1740`/`0x18C0`, all `z 0x3480`); hosenn = Mary centre
+ Noa right + kind 2 left; setumei = Noa + the Disco King demonstrating;
asobi = six dancers (kind 3 twice + the Disco King). The host town scene
(**koin3** - the field scene the PCSX load-transition capture parks in) places
the same NPCs on its dance floor at the matching coordinates with sibling
clips in its own 95-record bundle.

The per-dancer actor handler `FUN_801d1358` binds the idle before the play
states and the dance loop during them; on a judged event `FUN_801d1af4`
returns a u32-word index into the descriptor's move array - in pair units:
pair `0`/`1` = miss reaction (Square/Circle), pair `lane*2 + 2`/`+ 3` =
sequence-complete move per difficulty lane, pair `8 + lane` = the on-beat
timing-button step. Several choreography records carry frame data past the
header's frame count (the retail cursor clamps at `frame_count*16 - 1`, so
the tail never plays); `PlayerAnmBundle::record_lenient` accepts them.

This is what the site's playable dance renders: the retail qualifier cast at
the spawn-table offsets, textured against the `other7` scene VRAM (+ Noa's
field atlas), playing the descriptor-named clips - idle before the run, the
dance-groove loop synced to the beat clock, Noa's judge-triggered move on
each press, and the two competitors dancing - and **scoring** - their own runs,
throwing their groovy move when their schedule fires
(`crates/web-viewer/src/minigames_dance.rs` `dance_body_*` /
`dance_cast_json`; `site/js/minigame-dance.js`). Not pinned: the actor yaw on
the retail floor.

### The dance hall itself

The hall the minigame plays in - the raised stage, the yellow/black checkered
dance floor, the portrait banners, the spotlight cones, the smoke columns,
the speaker / lamp fixtures - is **the `other7` scene module's own field
geometry**, not overlay art: the block is a full scene (65-mesh environment
pack, `.MAP` placed-object + terrain-tile layers, walk-ground heightfield),
and the qualifier spawn coordinates land mid-stage in its placement frame.
The venue texture pages the pack samples are the same `(512..832, 0/256)`
rects catalogued in the PROT 1230 table above. The site's playable dance
bakes that map once at disc load into a single static mesh re-based on the
human dancer's spawn (`dance_env_*` in
`crates/web-viewer/src/minigames_dance.rs`: the same `field_env` placement /
terrain resolution the play page runs, bound props posed at frame 0 of their
clip) and draws it behind the posed cast, with the retail composition - the
camera on the audience half of the hall, backface culling standing in for
retail's NCLIP pass (the audience billboard sits right behind the camera
spot), and the hall's ABE prims (spotlight glows, smoke) drawn on an
additive second pass.

### HUD widget table (`DAT_801d46cc`) + emitter geometry

Every HUD element goes through the textured-quad emitter `FUN_801d2f38`, which
indexes a **34-record x 20-byte widget table** at `0x801D46CC`: `i32 scale`
(12.12; all rows `0x1000`), `u16 texpage` (all HUD rows `0x0008` = the 4bpp
page at `(512,0)`), `u16 CLUT id`, `u8 u0/v0/w/h` cell rect, top/bottom RGB
tints, semi-transparency code. Quads draw **centred** on the emitter's
`(x, y)`. Callers patch records in place: the score-digit renderer
(`FUN_801d32f8`) rewrites widget 1's `u0 = digit * 0x10`, the gauge
(`FUN_801d3e28`) rewrites widget 7's `u0 = 0xD0 + level * 8`, and the beat
track (`FUN_801d2524`) swaps CLUTs - `0x7D08` idle / `0x7D0D` on the
every-4th-beat combo window (`phase < 0x46`) for the caps + body (widgets
16/17/30), `0x7D0E` for the scrolling notes. **Confirmed.**

Traced layout (retail 320x240): score boxes (widget 8) centred at
`(64, 20)`/`(160, 20)`/`(256, 20)` with the **human dancer in the centre box**
(digit bases `-0x20`/`0x40`/`0xA0`, 8 slots stepping 16); gauge `Lv.` at
`(88, 192)` + level digit at `(96, 192)`; beat track anchored at `(120, 192)`
(arrow at `(128, 184)`, caps at `x-4`/`x+84`, 12 body tiles stepping 8, note
`x = 120 + i*16 - (phase*16/0x119 + 5) - 4` under a hardware scissor
`[x, x+0x50)`, stock markers at `y+16`); banner spawns (`FUN_801d3fd0`, which
stores `x << 3`) at centre `(160, 120)` for the count-in / `READY...` / `GO!`
/ `FINISH!`, `(160, 128)` for `Miss!`, `(160, 144)` for the rating banners
with star sparkles flanking at `±0x38` / `±0x50`. **Confirmed.**

### Rating banners per tier (`FUN_801d1af4` body)

The award routine's tier byte `iVar8` selects the banner; tiers 3..5 are the
groovy move's, one per difficulty lane (`iVar8 = lane + 3`), and only fire when
`DAT_801d570c` says the triangle landed on the combo slot:

| tier (`iVar8`) | when | banner (widget) | sound |
|---|---|---|---|
| 1 | a missed direction | `Miss!` (10) at `(160, 128)` | cue `0x210` |
| 2 | a closed direction chain | `Good!` (11) + 2 stars (`FUN_801d40dc`; star actors carry the accuracy weight at `+0x72`) | direct-keyed sting: `FUN_801d3d78(rand() % 3)` keys VAB program 1 tones `2r`/`2r+1` at note `0x3C + r` |
| 3 | a **landed triangle** on lane 0 | `Cool!` (19) at `(160, 144)` + stars `±0x38` | cue `0x202` |
| 4 | a landed triangle on lane 1 | `Great!!` (20) + stars `±0x50` | cue `0x203` |
| 5 | a landed triangle on lane 2 | `Fever!!!` (21) | cue `0x205` |

**Confirmed** (the banner-per-tier map closes the "which on-screen label each
tier spawns" question; the `Chicken!!` cell on the HUD page has no widget
record and no traced spawner - grading-screen use is **Inferred**).

### The dancer face stamp (`FUN_801d03c4`)

The dancers on the floor are field-scene actors; the overlay animates their
**faces**. `FUN_801d03c4(dancer, pose)` does two `MoveImage` (`FUN_80058490`)
blits inside a per-dancer VRAM strip, copying an **eye cell** and a **mouth
cell** from the strip's pose bank into its live window (the rows the head
samples). Per-case rig (jumptable at `PTR_LAB_801ceec8`; frame tables are 4-byte
`[eye_u, eye_v, mouth_u, mouth_v]` records, `u` in pixels `>> 2` to halfwords):

| case | strip | frame table | eyes (w_hw x h -> dst) | mouth |
|---|---|---|---|---|
| 0 | `(0x354, 0x100)` = **Noa's field atlas** (PROT 0874 §2 entry 2 at `(852, 256)`) | `0x801D435C` (5 poses) | 6x16 -> `(0x354, 0x10C)` | 4x8 -> `(0x355, 0x11C)` |
| 1 | `(0x190, 0)` = pack strip `(400, 0)` | `0x801D4370` (4) | 13x16 -> `(0x190, 8)` | 3x8 -> `(0x192, 0x20)` |
| 2 | `(0x1A0, 0)` = pack strip `(416, 0)` | `0x801D4380` (4) | 13x16 -> `(0x1A0, 8)` | 3x8 -> `(0x1A2, 0x2F)` |
| 3 | `(0x1B0, 0)` = pack strip `(432, 0)` | `0x801D4390` (4) | 12x16 -> `(0x1B2, 0xA)` | 3x8 -> `(0x1B2, 0x29)` |

In mode 0 (yosenn) the overlay remaps dancer `2 -> 3` and `1 -> 2` - exactly
the qualifier cast's kinds (dancer slots hold kinds 0/2/3), so **rig id =
dancer kind**. The four poses are eye/mouth expression variants (open / blink
/ intense / wink). `FUN_801d1af4` switches the human's pose on a scoring
event. **Confirmed** (rigs + tables read from the image; the strip diffs
against the live capture land exactly on the blit destination rows). The
strips are the dancer NPCs' own face windows: rig 0 = Noa's field atlas, and
rigs 1..3 are sampled by the heads of the `other7` scene's dancer meshes
(Mary + the two competitors - see
[Dancer bodies](#dancer-bodies-the-retail-cast--choreography-tables));
`_DAT_8007c36c` walks the spawned actors in `FUN_801d3a2c`.

The chart's symbols are likewise not abstract notes. `FUN_801d1820`'s only caller,
`FUN_801d4040`, maps a symbol straight to a **pad-button bitmask**:

| chart symbol | 1 | 2 | 3 |
|---|---|---|---|
| pad mask | `0x80` (Square) | `0x20` (Circle) | `0x10` (Triangle) |

Symbol `3` is the three-times-only "groovy move" - the
[wildcard](#the-triangle-wildcard-the-groovy-move), not a direction. The chart's
three rows *are* difficulty tiers (`gauge / 1000`), and every dancer - human and
CPU - reads the row its own gauge has promoted it to.

## Sound

Cues go to the runtime bank (`>= 0x200`; see
[`sfx-table.md`](../formats/sfx-table.md)). The descriptor block is the scene
module's `efect.dat` at **extraction PROT 1228**; the samples are the class-2 VAB
at **extraction PROT 1231**. **Confirmed** cue sites:

| Event | Cue | Site |
|---|---|---|
| intro flourish | `0x200` | `FUN_801D2D98` |
| run start | `0x201` | `FUN_801CF470` |
| **miss** | `0x210` | `FUN_801D1AF4` |
| combo tier 3 / 4 / 5 | `0x202` / `0x203` / `0x205` | `FUN_801D1AF4` |
| confirm / cursor | `0x20` / `0x21` | `FUN_801D0750` (static table) |

An on-beat **hit fires no ring cue**: it keys voices directly through
`FUN_801D3D78(rand() % 3)`, so a good step picks one of three stings at random -
each pick keys VAB **program 1, tones `2r` and `2r + 1` together**, at note
`0x3C + r` (two voices via `func_0x80065034`, volume from the config global
`_DAT_80084580`). **Confirmed.**

## Open

- The exact SCUS mode-24 entry-path call sites that stage the art pack (1230)
  and `efect.dat` (1228): not in the 0980 image, so they live in the
  `FUN_80025980` -> `FUN_8003EBE4` chain. The entries themselves are pinned by
  content + the byte-identical VRAM capture.
- The dancers' **yaw** on the retail floor: the spawn tables pin kind + world
  position
  (see [Dancer bodies](#dancer-bodies-the-retail-cast--choreography-tables))
  but not the facing, and the actor records are not RAM-pinned live.
- The kind descriptor's third header clip slot (`desc+0x20`; present for every
  kind, consumer untraced - a results/outro pose is the natural guess).
- The `Chicken!!` banner cell's spawner (no widget record names it).
- The exact **length** of each judge-triggered move clip: it is what retail
  really gates re-judging on (the award routine is only called while the dancer
  is on its idle / dance loop), and the port times the window off the dancer's
  spin instead - see [the wildcard](#the-triangle-wildcard-the-groovy-move).
- Which of `DAT_801D514C`'s modes picks BGM 1048 vs 1054 (the branch is
  pinned, the arm-to-song mapping is not; both are short chart-sized loops -
  see the PROT-load table above).

## See also

**Reference** -
[Cheats](../reference/cheats.md) ·
[Move-table VM](move-vm.md) ·
[Actor / sprite VM](actor-vm.md) ·
[Tile-board grid](tile-board.md)
