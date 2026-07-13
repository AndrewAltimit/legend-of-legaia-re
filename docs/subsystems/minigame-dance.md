# Noa dance (rhythm) minigame

The dance minigame is a rhythm / step game: a scrolling beat counter advances with the music, a per-lane step chart says which direction to press on each beat, and the player presses a d-pad direction inside a timing window around each beat. Correct presses score points and fill a "groove" gauge; the gauge selects a harder chart row; the run ends when the song timer elapses, and a final score is graded against a pass threshold.

It is one of the minigame-hub family that shares a single overlay binary with fishing / slot / Baka Fighter. This page documents **only** the dance-specific rhythm logic. The shared move-VM, actor-VM, sprite-primitive, and SDK helpers are documented in [`move-vm.md`](move-vm.md) / [`actor-vm.md`](actor-vm.md) and are not re-covered here.

All provenance is `overlay_dance_<addr>.txt` (locally-dumped, Sony-derived; not committed). Confidence is marked **Confirmed** (read directly from the dump) or **Inferred** (consistent reading not pinned by a separate observation).

## Step / rhythm state machine

The per-frame controller is `FUN_801cf470` (the overlay's dance tick). It is a `switch` on the game-state global `DAT_801d5334`. The states form a linear flow with two animation ramps and a play loop:

| State | Role |
|---|---|
| `0` | Title / mode select. Prints the menu prompt lines, moves the cursor, and reads the mode global `DAT_801d514c` (wrapped mod 4 by the `& 0x20` Start edge advancing the state). **Confirmed** |
| `1` | Setup: registers effect bundles, spawns actors, arms the "loading" flag `DAT_801d5830`, and branches the state by mode. **Confirmed** |
| `2` | Wait for the load flag `DAT_801d5830` to clear; then zeroes the run counters (gauge `DAT_801d544c`, etc.) and advances. **Confirmed** |
| `3` | Reset per-player run state: the three "remaining-step" counters `DAT_801d534c`/`+4`/`+8` are set to `3`, the chart cursors `DAT_801d574c` cleared, a start voice cue is queued. **Confirmed** |
| `4` | Intro-in: ramps the fade/curtain accumulator `DAT_801d515c` up to `0x3c`. **Confirmed** |
| `5` | Intro-out: ramps `DAT_801d515c` back to 0, then calls the actor-start helper and advances. **Confirmed** |
| `6` | Start BGM (`func_0x80026478` on a sequence id), zero the beat counters `DAT_801d581c` / `DAT_801d5820` / `DAT_801d5824`, set the lead-in countdown `DAT_801d513c = 4`. **Confirmed** |
| `7` | Lead-in countdown: decrement `DAT_801d513c`; when it reaches 0 jump straight to the play state `10`. **Confirmed** |
| `10` (`0xa`) | **Main play loop.** Per-frame the beat counters advance and `FUN_801d231c` draws the HUD; the song-end test runs here. Input judging happens in `FUN_801d1af4`, called per player from the actor handlers. **Confirmed** |
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

`FUN_801d1820(player)` - chart lookup for "what should be pressed right now":
- Compute the **intra-beat phase** `DAT_801d581c % 0x119` (mod 281). If it exceeds `0xd2` (210) the function returns 0 (dead zone between beats - no note is active). So each 281-unit beat slot has a ~210-unit acceptance window followed by a ~71-unit gap. **Confirmed.**
- Compute the **beat index** `DAT_801d581c / 0x119`. When `beat_index & 3 == 3` (every 4th beat) a special "held-sequence" entry from the per-lane progression table is checked, advancing the chart cursor `DAT_801d574c[player]` and returning the sequence symbol `3`. **Confirmed.**
- Otherwise return the chart byte `chart_base[lane*0x20 + beat_index]`, where `chart_base` is the step-chart table at `0x801d509c`, `lane` (the chart row, 0..) is `DAT_801d544c[player] / 1000` (the groove gauge selects difficulty row), and each row is `0x20` (32) bytes = 32 beats. **Confirmed.**

`FUN_801d4040(player)` maps the chart symbol to a pad-mask bit: symbol `1 → 0x80`, `2 → 0x20`, `3 → 0x10`, else 0. These three bits are the three judged directions. **Confirmed** (mapping); which physical d-pad direction each bit is is **Inferred** (they are the same three direction bits `FUN_801d1af4` masks against the live pad `_DAT_8007b874`).

`FUN_801d1960(player, lane, variant)` - the actual hit-judge, called when the player presses:
- Same dead-zone test (`phase % 0x119 > 0xd2` → return 0 = miss). **Confirmed.**
- Compute the **accuracy weight** `w = 0x1000 - (phase * 0x1000) / 0xd2`, a `0..0x1000` ramp that is maximal at phase 0 (dead-on the beat) and decays to 0 at the window edge. Stored in `DAT_801d6090`. **Confirmed.**
- Look up the chart symbol at `chart_base[lane*0x20 + beat_index]` and compare it to the pressed direction `(pressed & 0xf) + 1`. If they don't match → return 0 (wrong direction). **Confirmed.**
- If they match: advance the per-player chart cursor `DAT_801d550c[player]`. If the cursor completes the full chart for this lane (`cursor + 1 == lane + 1`) the routine resets the cursor, sets the bonus-window timer `DAT_801d6088 = 0xfa`, computes a **bonus** value `DAT_801d608c` from the per-lane value table `DAT_801d41a4` scaled by the accuracy weight `w`, and **returns 2** (sequence complete). Otherwise **returns 1** (a single hit). **Confirmed.**

So the judge has three tiers:
- **Miss** (0): outside the window, or wrong direction.
- **Hit** (1): correct direction inside the window - a single matched note.
- **Sequence / bonus** (2): a hit that also completes the lane's chart, awarding the weighted bonus from `DAT_801d41a4`.

There is no separate Perfect/Good text tier exposed by the judge itself; the *quality* of a hit is carried continuously by the accuracy weight `w` (closer to the beat → larger `w` → larger awarded points and bonus). The **scoring** routine `FUN_801d1af4` does carry a discrete Perfect tier, though: a combo hit whose streak counter `DAT_801d5334 - 0xb < 2` takes the `× 0x22` branch and raises the flag `DAT_801d538c = 1` (vs. the ordinary `× 0x19` combo). That flag is **Confirmed** to mark the top tier; which on-screen banner string it spawns is **Inferred** (capture-leaning).

### The two chart lookups diverge on exactly the beats that carry notes

`FUN_801d1820` and `FUN_801d1960` read the **same** chart with different rules on the `beat & 3 == 3` slots: the lookup substitutes the held-sequence symbol `3`, the judge reads the raw cell. That is not a corner case in the retail chart - the **lane-0 row places every one of its steps on a 4-beat boundary**, so on lane 0 the two sources never agree on a note. Higher lanes (which the groove gauge promotes into) add off-boundary steps, where they do agree.

The consequence for any host that renders the chart: **the arrow the player must press is the judged cell**, not `FUN_801d1820`'s return. The engine mirror surfaces both - [`DanceGame::judged_symbol`] (the judge's source) and [`DanceGame::required_symbol`] (the display / auto-feed source) - and the site's playable dance drives its note highway off the former.

## Scoring

`FUN_801d1af4(player)` is the score/award routine, run for each dancer (player 0 = human pad; for other indices the press is auto-fed from the chart via `FUN_801d4040`, i.e. CPU dancers auto-play). It guards on the play states (`DAT_801d5334 - 10 < 3`) and on the not-paused flag `DAT_801d5130`. **Confirmed.**

Key accumulators (all per-player, `player * 4` stride from the listed base):

- **Score** `DAT_801d53cc[player]`: incremented on a hit, scaled by the chart-row index `lane = DAT_801d544c[player] / 1000` (0/1/2). The exact per-tier increments (`FUN_801d1af4`, all `× (lane + 1)`, score then **clamped to `999`** / `0x3e7`): **Confirmed.**

  | Tier | Increment | Selector |
  |---|---|---|
  | Ordinary on-beat hit | `(lane + 1) * 3` | timing button (`pad & 0x10`), off-beat or outside the window |
  | Combo hit | `(lane + 1) * 0x19` (25) | on a `4`-beat boundary (`(beat & 3) == 3`) inside the window (`phase < 0xd2`), streak `DAT_801d5334 - 0xb >= 2` |
  | Perfect combo | `(lane + 1) * 0x22` (34) | same, but streak `< 2`; also raises the Perfect banner flag `DAT_801d538c = 1` |
  | Direction sequence complete | `+ DAT_801d608c` | a judged direction press (`pad & 0x80` / `& 0x20`) where `FUN_801d1960` returns 2; also adds the bonus base `DAT_801d6088 = 0xfa` to the gauge |

  The combo / Perfect tiers also bump the gauge `+1000`, so they self-promote the dancer to a higher (denser, higher-multiplier) lane.
- **Groove gauge** `DAT_801d544c[player]`: stepped up `+1000` on success and clamped to `[0, 2999]`. Because the chart row is `gauge / 1000`, the gauge crossing 1000 / 2000 promotes the dancer to the next (harder, higher-scoring) chart row. On a miss the gauge floors to 0 / drops a row. So the gauge is simultaneously the combo/excitement meter, the difficulty selector, and the score multiplier. **Confirmed.**
- **Step-stock counter** `DAT_801d534c[player]` (3 at run start, `FUN_801cf470` state 3; also re-armed to 3 by `FUN_801d0750`): the per-player stock of timing-button (`pad & 0x10`) scoring presses. `FUN_801d1af4` decrements it *only* inside the `& 0x10` branch and only while non-zero; at 0 that branch is skipped. It is **not** replenished during play, and the HUD reads it to draw the remaining markers (`FUN_801d2524`).

  Running to 0 **gates the `0x10`-button scoring path only - it does not end the run**: no path writes the state global `DAT_801d5334` off it, and the run ends solely on the song timer (`DAT_801d5820` → state `0xb`; see [state machine](#step--rhythm-state-machine)). The judged direction presses (`& 0x80` / `& 0x20`) are independent of this counter. **Confirmed** (`overlay_dance_801d1af4.txt`, `overlay_dance_801cf470.txt`).
- **Per-player hit-tier state** `DAT_801d548c[player]` (0/1/2/3) and timer `DAT_801d54cc[player]`: latch which direction/animation is active for the current step so a held button isn't re-judged every frame. **Confirmed.**

Player 0 additionally drives feedback: a sound cue (`_DAT_8007b6de`/`_DAT_8007b6d8` write a sequence id), a hit/combo banner via the sprite emitter `FUN_801d3fd0`, and the dancer pose switch `FUN_801d03c4`. **Confirmed.**

### Win / lose threshold

In results state `0x14`, `FUN_801cf470` copies the three player scores into display halfwords (`DAT_801c6460..`) and then, by mode, decides the outcome (see the [per-mode table](#mode-global-dat_801d514c) for the exact comparison each mode runs):
- Modes 0/1 compare the human score `DAT_801d53cc` against the opponent slot (`DAT_801d53d4` / `DAT_801d53d0`); mode 2 tests it against the fixed threshold `300` (`0x12d`); mode 3 grades nothing. **Confirmed.**
- Flag `0x50a` is **set** at entry (state 1) and **cleared on a loss** here via `func_0x8003ce34(0x50a)` (`ce34` = clear). Set = passed, clear = failed; free play (mode 3) leaves it untouched. **Confirmed.**

The high score is tracked in the save block (`_DAT_80084464` is updated when `DAT_801d53cc` exceeds it). **Confirmed.**

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
| `DAT_801d53cc[]` | u32×3 | **Per-player score**, clamped to `999` | Confirmed |
| `DAT_801d544c[]` | u32×3 | **Groove gauge**, clamped `[0,2999]`; `/1000` selects chart row | Confirmed |
| `DAT_801d534c[]` | u32×3 | Per-player step-stock (reset to 3); gates the `0x10`-button scoring path, does not end the run | Confirmed |
| `DAT_801d548c[]` | u32×3 | Current-step hit-tier latch (0/1/2/3) | Confirmed |
| `DAT_801d54cc[]` | u32×3 | Hit-tier latch timer | Confirmed |
| `DAT_801d550c[]` | u32×3 | Per-player chart cursor (advanced on each matched note) | Confirmed |
| `DAT_801d574c[]` | u32×3 | Per-player combo / held-sequence cursor (the `&3==3` slot) | Confirmed |
| `DAT_801d56cc[]` | u32×3 | Current dancer pose / animation id (`FUN_801d03c4`) | Confirmed |
| `DAT_801d568c[]` | u32×3 | Miss / wrong-press counter | Inferred |
| `DAT_801d6088` | u32 | Bonus-window timer set on a completed sequence | Confirmed |
| `DAT_801d608c` | u32 | Computed sequence-bonus points (weighted by accuracy) | Confirmed |
| `DAT_801d6090` | u32 | **Accuracy weight** `0..0x1000` (peaks on-beat) | Confirmed |
| `DAT_801d509c` | bytes | **Step chart table** (3 rows × `0x20` beats; `row*0x20 + beat`, byte = direction symbol 0/1/2/3). **Baked** into the overlay static image (PROT 0980 file offset `0x6884`), not loaded per song. | Confirmed |
| `DAT_801d41a4` | i32 table | Per-lane / per-step point values for the sequence bonus | Confirmed |
| `DAT_801d41e4` | i32 table | Per-lane held-sequence threshold table (the `&3==3` combo slot) | Confirmed |
| `DAT_801d43a0` | i16 table | Per-step world/screen anchor positions (HUD step interpolation) | Inferred |
| `DAT_801d583c` | i16 table | Easing LUT used to interpolate the dancer/marker between beats | Inferred |

The "dance points" cheat anchor at `0x801d53cc` (see [`../reference/cheats.md`](../reference/cheats.md)) is exactly `DAT_801d53cc[0]` - the human player's score. **Confirmed.**

## Key functions

| Function | Role |
|---|---|
| `FUN_801cf470` | Per-frame dance controller / state machine (the dance tick); beat clock + song-end test + grading. `overlay_dance_801cf470.txt` |
| `FUN_801d1af4` | Per-player score / award routine; reads input (human pad for player 0, chart auto-feed otherwise), drives the gauge, score, banners, and pose. `overlay_dance_801d1af4.txt` |
| `FUN_801d1960` | Hit judge: dead-zone + accuracy-weight + chart-direction match → returns 0 miss / 1 hit / 2 sequence-complete (with bonus). `overlay_dance_801d1960.txt` |
| `FUN_801d1820` | Chart lookup: the symbol that should be pressed on the current beat (incl. the every-4th-beat held-sequence slot). `overlay_dance_801d1820.txt` |
| `FUN_801d4040` | Chart symbol → pad-mask direction bit (`1→0x80`, `2→0x20`, `3→0x10`). `overlay_dance_801d4040.txt` |
| `FUN_801d231c` | Score / gauge HUD render (per-player score + groove gauge via the sprite emitter). `overlay_dance_801d231c.txt` |
| `FUN_801d03c4` | Dancer pose / animation switch driven by hit results. `overlay_dance_801d03c4.txt` |
| `FUN_801d2f38` | Textured-quad sprite emitter (HUD digits / banners / gauge); shared presentation helper. `overlay_dance_801d2f38.txt` |
| `FUN_801d3a2c` | Per-frame dance-floor draw pass (actor list + tile-grid sweep). See [Dance-floor rendering](#dance-floor-rendering). `overlay_dance_801d3a2c.txt` |
| `FUN_801d2a10` | Dance-floor tile-grid blit (scratchpad column-Y table + rect quad emit). `overlay_dance_801d2a10.txt` |
| `FUN_801d3ec0` | Two-layer step-marker lookup wrapper (scene-data `+0x10000` / `+0x12000`). `overlay_dance_801d3ec0.txt` |
| `FUN_801d3f54` | Per-cell step-marker lookup (per-row sub-list, match `(x, y)`). `overlay_dance_801d3f54.txt` |

Parser: [`legaia_asset::dance_chart`](../../crates/asset/src/dance_chart.rs) decodes the baked [step chart](#step--rhythm-state-machine) (3 rows × `0x20` beats) from the disc.

Engine port: [`legaia_engine_core::dance`](../../crates/engine-core/src/dance.rs) is the clean-room rules engine driven by that parsed chart - the beat clock (`FUN_801cf470`), the timing-window hit judge (`FUN_801d1960`), the chart lookup (`FUN_801d1820`), the symbol→pad-bit map (`FUN_801d4040`), and the score / groove-gauge award (`FUN_801d1af4`), all with the Confirmed constants above.

Runtime wiring: the engine host installs the rules engine as a suspending scene mode (`SceneMode::Dance`; `World::enter_dance` / `tick_dance` / `exit_dance`). The `play-window` viewer starts it from the `K` key (loads the dance overlay PROT 0980, `DanceGame::from_overlay`), maps the three arrows to the retail pad bits (Left/Right/Up = symbols `1`/`2`/`3`), and draws the score / groove-gauge / active-lane HUD; the song timer ends the run and restores the interrupted scene.

`DanceGame::judge_press` returns the three-way Miss/Hit/Sequence result and applies the score, gauge, and streak side effects; `DanceGame::from_overlay` starts a run straight off the disc chart (disc-gated `dance_minigame_real` auto-plays the real chart end to end). The sequence-bonus *magnitude* (the `DAT_801d41a4`-scaled award) is left to the caller since that value table is disc-resident and unmapped. The visible dance-floor / arrow rendering (the [floor cluster](#dance-floor-rendering)) is not part of the rules port - it is a separate host concern.

## Open

- The visible Perfect/Good/Miss banner *strings* each tier spawns (the `× 0x22` / `DAT_801d538c` Perfect tier and the accuracy weight `DAT_801d6090`) - the score tiers are pinned (see [Scoring](#scoring)); only the on-screen label each spawns is unmapped (capture-leaning).

## See also

**Reference** -
[Cheats](../reference/cheats.md) ·
[Move-table VM](move-vm.md) ·
[Actor / sprite VM](actor-vm.md) ·
[Tile-board grid](tile-board.md)
