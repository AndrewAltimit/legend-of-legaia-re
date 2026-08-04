# LANE 5 handoff - the simulation clock's denomination

## What changed (one sentence)

`World::tick` is now denominated at **one sim tick = one retail display frame
(vsync)**, which is what both hosts were already driving it at; the
`SIM_HZ = 100` premise that withheld 2 of every 5 retail frames from the
*gated* consumers is gone.

## The finding that inverts the brief

The lane brief's premise - "the player walks at 1.67x retail wall-speed on the
native host" - is **false**, and the defect runs the other way.

Neither host ever ticked at 100 Hz:

- native: `crates/engine-render/src/window.rs:152`, `EngineWindow::drain_ticks`,
  `const TICK_DT: f64 = 1.0 / 60.0`, backlog capped at 4 ticks
  (`window/event_handler/redraw.rs:34` calls `drain_ticks(dt, 4)`);
- browser: `site/js/play-app.js`, `const TICK_DT = 1000 / 60`, same 4-tick cap;
- asset-viewer apps: `EngineWindow::frames_for(dt, 8)` = `dt * 60`.

So the **ungated** consumers (player locomotion, per-actor field channels, the
prop / tile-board layers) were already at the correct 60 Hz, and the **gated**
ones (`field_frame_step == 1`: narration crawl, cutscene timeline, effect pool,
escape timer, NPC motion, CLUT / ambient game-tick banks, timed sound release)
were running at **36 Hz = 0.6x retail**.

## Sibling-owned changes I did NOT make

### 1. `crates/engine-core/src/mode.rs` - two probes on a field I re-purposed

`field_frame_accum` is no longer a fixed-point phase accumulator (there is no
phase left at 1:1). I kept it advancing once per tick so both probes below keep
working unchanged, but the honest field to probe is `World::frame`:

- `mode.rs:1203..1215` `a_frame_begin_skip_abandons_the_frame_before_the_world_ticks`
- `mode.rs:1228..1232` `init_modes_ignore_the_frame_begin_skip`

Suggested patch (both sites, mechanical):

```rust
-        let before = w.field_frame_accum;
+        let before = w.frame;
...
-        assert_eq!(w.field_frame_accum, before, "FUN_8001698C returned 1 - no frame ran");
+        assert_eq!(w.frame, before, "FUN_8001698C returned 1 - no frame ran");
...
-        assert!(w.field_frame_accum != before);
+        assert!(w.frame != before);
```

Not required for green - both pass as-is.

### 2. `crates/engine-core/src/world/narration.rs` - stale rate prose only

`step_spawned_record_contexts` (line ~690) keeps its internal
`if self.field_frame_step != 1 { return; }` gate, which is now a tautology and
still correct. Its doc comment still says "The engine's sim clock runs at
100 Hz, so stepping the timeline once per sim tick drained every `WaitFrames`
1.67x too fast". That sentence is now wrong twice over (no host ran at 100 Hz;
the gate was costing the timeline 40% of its frames, not saving it). Suggested
replacement:

```
/// One sim tick is one retail display frame (see `World::tick`), so a single
/// step per tick credits exactly one authored frame of `WaitFrames` - which is
/// what the retail record expects. The gate below names that unit; it does not
/// thin the rate.
```

The measured wall-times behind it are unchanged - see the oracle numbers below.

### 3. Tests outside my scope that use `field_frame_step` as a loop condition

All still pass; the loops just converge in fewer iterations now (the condition
is true every tick). No change required, listed for completeness:

- `crates/engine-core/tests/cutscene_timeline_synthetic.rs:17..22`
- `crates/engine-core/tests/map01_clut_fx_disc.rs:168`
- `crates/engine-core/tests/organic_jou_door_records_disc.rs:139`

Their comments say "tick until `field_frame_step` fires"; that is still an
accurate description of intent.

### 4. `crates/engine-shell/.../window/field_render.rs:710`

Comment reads "the 100 Hz sim carries ~60 vsyncs/s". Now inaccurate; the guard
itself (`if ... .field_frame_step == 0 { return; }`) stays correct. Suggested:
"one sim tick is one retail vsync (`World::field_frame_step` is 1 every tick);
the test names the unit rather than thinning the rate."

## Consequence a sibling may notice: field NPCs get 1.67x faster than on `main`

Commit `350901d1` fixed "town NPCs move really really fast" with **three**
changes. Two of them - the seat-shuttle route filter and the
`FIELD_NPC_MOTION_SPEED` amble cap - are the real fixes and are untouched. The
third, the `field_frame_step` gate on `tick_field_npc_motions`, was justified by
the 100 Hz premise and was really a 0.6x slowdown with no retail basis.

Retail settles it from the disassembly: `FUN_8003774C` loads the frame-step byte
(`0x80037868 lbu s2,0x393(s2)`) and multiplies it into every glide leg, exactly
like `FUN_801D01B0` does with the player's travel budget. It is called once per
**game tick** and scales by the vsyncs that tick spans, so its wall speed is
`glide_magnitude * 60` units/second regardless of cadence - the same
cadence-invariant identity. One call crediting one display frame at 60 Hz is
retail-exact; at 36 Hz it was 0.6x.

If villager pace still reads wrong after this, the number to change is
`FIELD_NPC_MOTION_SPEED` (a real per-vsync magnitude), not the call rate.

## What I own and finished

- `crates/engine-core/src/world/frame_tick.rs` - the denomination, with the
  retail addresses for the two-clock contract.
- `crates/engine-core/src/world/state.rs` - `field_frame_accum` /
  `field_frames` / `field_frame_step` docs + default.
- `crates/engine-core/src/world/field_movement.rs` - the locomotion call-cadence
  contract; also the walk-regen **fill** is no longer an undecoded gap (below).
- `crates/engine-core/src/world/tests/actor_cadence.rs` - `SIM_TICKS` 200 -> 120.
- `crates/engine-core/tests/opening_chain_wall_time.rs` - `SIM_HZ` 100.0 -> 60.0.
- `crates/engine-core/tests/sim_cadence_wall_speed.rs` - new units-per-second
  oracle (6 tests).
- `site/js/play-app.js` - the shared-denomination comment on `TICK_DT`.
- `docs/subsystems/field-locomotion.md` + the site mirror - the law.

## Bonus RE finding (free, and it closes a documented gap)

`docs/subsystems/field-locomotion.md` and `World::tick_field_walk_regen` both
recorded that the **fill** side of the walk-regen accumulator `_DAT_801F2274`
was undecoded ("no dump in the corpus carries the writer"). It is decoded: the
writer is inside `FUN_801D01B0` itself, at its tail `0x801D0910..0x801D0928` -

```
801d0910  lui  a0,0x801f
801d0914  lui  v1,0x1f80
801d0918  lbu  v1,0x393(v1)      ; DAT_1F800393
801d091c  lw   v0,0x2274(a0)     ; _DAT_801F2274
801d0924  addu v0,v0,v1
801d0928  sw   v0,0x2274(a0)
```

behind the step-delta-non-zero test at `0x801D08F4..0x801D090C`. So retail
credits one unit per **vsync** whose step committed - the same per-vsync
denomination as the travel budget - and the port's `+= field_frame_step` per
sim tick is the same rate. Docs and code comments updated accordingly.

Evidence: `disassembly`
(`ghidra/scripts/funcs/overlay_cutscene_dialogue_801d01b0.txt`).

## The town01 south-gate item the coordinator relayed

Two halves; I acted on one and corrected the other.

**Corrected: flag 562 is not in the seeder.** The relay said
`World::seed_free_roam_story_baseline` seeds system flag 562 for `town01`. It
does not - the function only ever touched `0x147` (327), and only for `town0c`.
The 562 seed the relay describes is in
`crates/engine-shell/tests/critical_path_replay.rs:655`, a **test** in a crate
outside this lane, where the comment says it stands in for "the town's story
beats". If Lane 4's reading is that 562 gates a content-free park, that test's
rung is the thing to revisit - whoever owns `engine-shell`, not me. Nothing was
dropped here because there was nothing to drop.

**Acted on: `town0c` now seeds 327 AND 321.** Per Lane 4, the gate script
branches on both and only the both-set arm cuts collision row 47. `town0c` is
post-Mist Rim Elm, so staging it with 327 alone produced a world no playthrough
reaches - blown-gate rubble in front of a doorway you still cannot walk through.
Both flags are now set there and both are cleared in the reset-then-seed
preamble, so picking `town01` afterwards does not leak an open gate.

**Deliberately NOT done: seeding either flag for `town01`.** `0x147` is the same
flag that seats the rock debris, and two committed oracles pin pre-Mist Rim Elm
as rocks-hidden: `field_object_visibility_disc.rs`
(`town01_cold_entry_hides_gate_rocks_and_keeps_early_scenery`) and
`free_roam_staging_disc.rs`
(`town0c_staging_swaps_gate_records_and_resets_between_picks`, whose second half
asserts picking `town01` after `town0c` re-hides `P0[18..21]`). Seeding 327 for
`town01` breaks both and shows post-Mist scenery in the pre-Mist twin. A usable
pre-Mist south gate, if that is wanted, has to come from somewhere other than
this flag pair - flag it as a separate decision.

Both oracles above re-run green with the change.

**Provenance note.** The `P0[20]` / `4C 70 18 2D 19 2E` gate-script reading is
**relayed from Lane 4**, labelled `disassembly` there; I did not re-derive it
(`legaia-engine man-scripts` only surfaces P1 dialogue-yield records, so
checking it needs a different tool path). What I did verify independently: that
562 is absent from the seeder, and that the `town01` no-seed constraint is
forced by the two oracles above. Given the relay was already wrong about where
562 lives, someone reconciling Lane 4's `south_gate_disc.rs` should confirm the
both-flags arm against that test rather than against this note.

## LANE 2 game-over patch for `site/js/play-app.js`

Checked for `handoff/lane2.md` at the end of this lane's run - see the report
for the outcome.

## Stale rate prose left in sibling files (cosmetic, listed for sweep)

- `crates/engine-shell/src/bin/legaia-engine/window/field_render.rs:710` -
  "the 100 Hz sim carries ~60 vsyncs/s".
- `legaia-engine play-window --help`, the `--screenshot-tick` doc - "Ticks
  advance at the fixed 100 Hz sim rate". The determinism claim it makes is
  still true; the number is not.
- `crates/engine-core/src/live_loop.rs:98` and
  `crates/engine-core/src/world/battle/teardown.rs:22` - "~5 s / ~3 s at the
  100 Hz sim clock". Those durations are counted in ticks, so at 60 Hz they are
  now ~8.3 s and ~5 s respectively; whoever owns them should decide whether the
  *tick count* or the *seconds* was the intent.
