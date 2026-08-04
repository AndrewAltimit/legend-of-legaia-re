# Lane F handoff - retail opens the pause menu on the overworld

## The question

Wave 1 measured that **the Save row has no pad route anywhere in the port**:
only the three kingdom overworlds set the MAN save bit, every menu-open path
required `SceneMode::Field`, and a kingdom overworld runs as
`SceneMode::WorldMap`. It declined to widen the gate without first settling
whether retail's world map opens the menu at all, because retail's menu-open
accept lives inside the *locomotion* controller `FUN_801D01B0` and whether
`FUN_801E76D4` carries its own accept was unanswered.

**Answer: retail opens the pause menu on the overworld, and always has.** The
question was mis-framed - `FUN_801E76D4` is not the overworld's controller, so
it never needed an accept. The port's gate was the defect.

## The evidence

### 1. `FUN_801E76D4` is the top-view debug renderer, not a controller

Its second test branches to the function's own epilogue. The dump is 2330
instructions, the function spans `0x801E76D4..0x801E9B3C`, and `0x801E9B14` is
where the register restores start:

```text
801e7794  lbu   v0,0x2b94(v0)      ; DAT_801F2B94 (top-view flag)
801e779c  beq   v0,zero,0x801e9b14 ; not top view -> epilogue
...
801e9b14  lw    ra,0x44(sp)
801e9b34  jr    ra
801e9b38  _addiu sp,sp,0x48
```

With top view off it does nothing at all, and entering top view needs the
debug flag `_DAT_8007B98C` retail leaves clear. Its whole call set is 48
`jal 0x8001aa68` + 21 `jal 0x80056788` (string/format) plus five leaves -
there is no field-VM tick, no actor step, no camera follow in it.

`docs/subsystems/world-map.md` described a third behaviour, *"Normal-walk path
(`DAT_801F2B94 == 0`): standard per-frame world-map update (field VM tick,
actor step, camera follow via motion VM)"*. That is **falsified** by the branch
target above; it was the load-bearing premise behind wave 1's residual.

**disassembly-grounded.**

### 2. The overworld runs the ordinary field chain

`FUN_801D1344` (`jal 0x801D01B0` at `0x801D16F4`) is the per-frame player tick
for both, and its head is the map-view fade:

```text
801d1344  lui  v0,0x8008
801d1348  lw   v0,-0x450c(v0)   ; _DAT_8007BAF4, the map-view fade ramp
801d1368  beq  v0,zero,0x801d138c
801d1378  or   v0,v0,v1         ; player+0x10 |= 0x80000 (suppress locomotion)
801d137c  jal  0x800196a4       ; the fade tick -> game_mode 0x0C (MAPDSIP)
801d1384  j    0x801d185c       ; and skip the jal to FUN_801D01B0
```

The controller itself proves it runs on the overworld, twice over, and both
arms are gated on the same byte the Save row is gated on:

| Site | Arm | Only reachable when |
|---|---|---|
| `0x801D0354` | base-step selector `s4 = 5` (the slower overworld step) | `_DAT_8007B6A8 != 0` |
| `0x801D01FC..0x801D024C` | L1 arms `_DAT_8007BAF4 = 1`, entering the top-down map view | `_DAT_8007B6A8 != 0` |

Both would be unreachable code if a `_DAT_8007B6A8` scene never entered
`FUN_801D01B0`. The second closes a loop with the fade head above that cannot
close any other way: something on the overworld has to set that ramp, and this
is the only pad arm that does.

**disassembly-grounded.**

### 3. The captures agree

| Capture | Scene | Reports |
|---|---|---|
| `sebucus_overworld_resident` | `map02` | `field-run` (game mode `0x03`) |
| `karisto_overworld_resident` | `map03` | `field-run` |
| `keikoku_chest_preload` | `map01` | `field-run` |
| `menu_status_field` | `map01` | `mode_17` - the pause menu, **open on the overworld** |
| `menu_options_field` | `map01` | `mode_17` - likewise |

`menu_equipment_field` is the third of that set. So the overworld is not a
distinct game mode at all, and retail's own capture library already held a
pause menu standing open on one.

The save-allow byte matches its documented polarity, read straight out of main
RAM at `0x8007B6A8`: `1` on `map01` / `map02` / `map03`, `0` in `town01`.

**capture-grounded.**

### 4. And the Save row's gate really is that byte

```text
801d6cb0  lw    v1,0x46bc(v0)     ; root cursor
801d6cb8  bne   v1,v0,...         ; row 6?
801d6cbc  _lui  v0,0x8008
801d6cc0  lbu   v0,-0x4958(v0)    ; -> 0x8007B6A8
801d6cc8  beq   v0,zero,0x801d6ce8 ; clear -> buzz 0x23, stay
801d6cdc  li    v0,0x19            ; set  -> SFX 0x20, sub-screen 0x19
```

Note `lui 0x8008` + a **negative** displacement: this is the transcription
hazard `ghidra.md` catalogues, and `0x800846A8` is the wrong name it once
produced here. Resolved as a pair it is `0x8007B6A8`.

**disassembly-grounded.**

## The root cause in the port

`crates/engine-core/src/mode.rs` already models retail correctly - mode 3
(`MainMode`) is `SceneMode::Field` and its comment names `map03` explicitly as
a mode-3 scene, while MAPDISP 12/13 is the world-map *display*. But
`SceneHost::enter_world_map_scene` puts the **walkable** kingdom overworld into
`SceneMode::WorldMap` too, so that one variant carries two different retail
states: retail's mode-3 walkable overworld and retail's mode-12/13 map display.

The menu gate then read `SceneMode::WorldMap` as "not the field", which is true
of the display mode and false of the walkable one - and the walkable one is the
only place Save is legal. That is the whole defect.

Collapsing the two back apart is a cross-cutting refactor (`frame_tick`,
`field_movement`, `world_map`, the renderer, both hosts) and is **not** what
this lane did.

## What changed

| File | Change |
|---|---|
| `crates/engine-core/src/world/save.rs` | new `World::scene_mode_takes_menu_open` (the mode partition) and `World::field_menu_open_allowed` (the whole precondition, engaged bit first) |
| `crates/engine-shell/src/boot.rs` | `tick`'s Start arm calls the predicate instead of testing `SceneMode::Field`; `open_field_menu`'s doc states the builder/gate split |
| `crates/engine-core/tests/world_map_menu_gate.rs` | new - the mode partition (disc-free) + the scene chain (disc-gated) |
| `crates/engine-shell/tests/menu_replay.rs` | rung 10 (Save by pad on `map01`); wave 1's probe inverted into its closed form; two new sibling pins; stale rung-2 doc corrected |
| `scripts/replays/menu_replay_baseline.toml` | ratchet 9 → 10 |
| `docs/subsystems/save-screen.md` | the Save-row section rewritten around the accept's real siting |
| `docs/subsystems/world-map.md` | `FUN_801E76D4` re-titled; the "normal-walk path" claim marked falsified with the branch target |
| `docs/subsystems/field-menu.md` | new "Which scenes the menu opens in" |
| `crates/engine-core/src/world/tests/field_npc_motion.rs` | **not this lane's change** - unblocking a pre-existing compile break, see below |

`World::field_menu_open_allowed` is deliberately the *pad-route* gate, not a
guard inside `open_field_menu`. The builder stays permissive so headless
drivers and oracles can construct the session from any mode - which wave 1's
probe relied on - while every Start edge routes through one predicate.

## What the ladder's Save rung proves

Rung 10 boots `map01`, and from there **everything is pad**: Start opens the
menu, the cursor walks to Save, Cross opens a `SaveSelectMode::Save` session
over the same rack rung 8 read through, Cross on the mounted pill crosses the
"Now checking" beat, the 5x3 block grid appears, Cross on a block raises the
overwrite prompt, and the prompt is answered.

Two details it asserts rather than routes around:

- the prompt **defaults to No** (`ConfirmOverwrite { cursor: 1 }`), and the
  rung fails if a commit arrives without that default having been observed - a
  rung that just pressed Cross twice would have asserted the destructive-write
  guard away;
- the commit is `SaveCommitKind::Save`, not Load. Rung 8 reaches the same
  screen in the read direction; rung 10 is the first time the **write**
  direction is reachable by pad anywhere in the port.

Score is now 10/10 (`[rung 10] committed save: port 0 cell 0`).

The siblings keep the widening honest, because "Save is now reachable" is one
assertion a broken build could satisfy by enabling Save everywhere:

- `a_town_still_refuses_the_save_row_after_the_open_gate_widened` - `town01`
  opens the menu, the cursor lands on the greyed Save row, and Cross opens
  nothing. The open gate widened; the row gate did not.
- `start_stays_inert_in_a_suspended_mode` - Battle / Cutscene / Fishing still
  answer Start with nothing. The `SceneMode::Field` literal was originally
  added to stop Start mid-fight freezing the fight; the widening is one mode
  wide.

## Verification

- `cargo test --release -p legaia-engine-core` - full suite.
- `cargo test --release -p legaia-engine-shell` - full suite (the edit is in
  `boot.rs`, which every oracle in that crate drives, so the targeted
  two-test run was not sufficient on its own).
- `cargo fmt --all -- --check`, `cargo clippy -p legaia-engine-core -p legaia-engine-shell --all-targets -- -D warnings`.
- `check-md-links.py`, `check-doc-density.py`.

**Non-vacuity, by mutation.** Reverting `scene_mode_takes_menu_open` to
`SceneMode::Field` alone fails 3 of the 4 new engine-core tests - including
the disc-gated `a_kingdom_overworld_both_opens_the_menu_and_offers_save` - and
`a_town_opens_the_menu_but_greys_the_save_row` still passes, which is the
contrast that shows the two gates are independent.

**Disc-gating, by contrast.** These tests key on `extracted/`, **not** on
`LEGAIA_DISC_BIN` - unsetting the variable changes nothing, exactly as wave 1
flagged for `menu_replay`. Run from a directory without `extracted/`:
`menu_replay` prints five `[skip]` lines in 0.00 s where the real run scores
10/10 in 0.82 s, and `world_map_menu_gate` skips its two scene-chain tests
while the two disc-free mode tests still run and pass.

One trap worth recording: the first no-`extracted/` run of
`world_map_menu_gate` "failed", because the on-disk test binary was still the
one built during the mutation check - `cargo test -p legaia-engine-shell`
rebuilds the engine-core *library* but not engine-core's own test targets. A
stale artifact reads exactly like a result.

## Two pre-existing breaks on the branch, inherited not caused

Both arrived with `3134444c` ("the walk-on cabinet arms the minigame door
too"), which renamed `WalkTouchEvent::Warp { target_map }` to `{ sub_id }` and
changed what the arm posts.

1. **`cargo test -p legaia-engine-core` did not compile.**
   `crates/engine-core/src/world/tests/field_npc_motion.rs:180` still
   constructed `Warp { target_map: 3 }` (`error[E0559]`), so the crate's own
   lib-test target was unbuildable on the branch as handed over. I could not
   run my own required gate without fixing it.

   The fix is not a rename: the test also asserted
   `pending_scene_transition == Some(3)`, which is the model that commit
   deliberately falsified. I applied that lane's own documented model, mirroring
   its sibling pin `field_op_3e_warp_arms_the_minigame_door_not_a_scene_change`
   - the contact now posts `pending_minigame_warp == Some(3)` and
   `pending_scene_transition` must stay `None` - and renamed the test to
   `..._arms_the_minigame_door`. The edge-latch half is untouched. **Please
   have the owning lane confirm this reading.**

   Note the shape: `631a9bb9`'s own message says the arm-correcting lane was
   briefed on `-p legaia-engine-core` while its edit lived in `engine-vm`. The
   miss here is the mirror image - the edit landed in `engine-core` and this
   caller went unbuilt anyway.

2. **`cargo fmt --all -- --check` fails on
   `crates/engine-core/src/man_field_scripts/npc_motion.rs:1036`** (a
   `Warp { sub_id: target_map }` struct literal rustfmt wants on one line).
   That file is off limits to this lane and may be live in a sibling worktree,
   so I did **not** touch it - I formatted only my own files. `fmt` is clean
   for everything this lane wrote; the one remaining diff is that file.

## For the integration pass

1. **Both hosts still spell the mode test out locally**, and both files were
   outside this lane's scope:
   - `crates/engine-shell/src/bin/legaia-engine/window/event_handler/redraw.rs`
     gates its Start edge on `mode == SceneMode::Field` - with a comment
     asserting "on the world map the controller has its own" handler, which is
     the claim §1 falsifies. Swap the condition for
     `self.session.host.world.field_menu_open_allowed()`.
   - `crates/web-viewer/src/play_menu.rs` / the play page's Start edge needs
     the same swap.

   Until then the shared driver and the ladder open the menu on the overworld
   and neither shipped host does. This is the one place the fix is incomplete.

2. **Add `field_menu_open_allowed` to the tier-3 host-drift row** in
   `scripts/ci/check-ui-host-drift.py` (the pause-menu-open row currently pins
   `FieldMenuGate` / `SceneMode::Menu` / `dialogue_owns_input`). That is what
   makes item 1 impossible to half-do.

3. **A doc claim outside this lane's scope looks wrong.**
   `docs/subsystems/field-locomotion.md` step 2 calls the
   `0x801D01FC..0x801D024C` arm the "action button ... (talk / examine)". Three
   things do not fit that label:

   - it is gated on the overworld flag `_DAT_8007B6A8`, so it cannot fire in a
     town, where talking plainly works;
   - its button is `_DAT_8007B874 & 4` = bit 2 = **L1** under Legaia's packed
     pad mask, not a face button (the layout is `FUN_8001822C`'s, which puts
     the face/shoulder byte in bits 0-7);
   - its load-bearing store is missing from the description entirely -
     `sw v1,-0x450c(v0)` at `0x801D0238` sets `_DAT_8007BAF4 = 1`, the
     map-view fade ramp `FUN_800196A4` walks up to `game_mode 0x0C` (MAPDSIP).
     That store sits in a branch delay slot, which is exactly the "reordered
     or dropped stores" decompiler artifact `ghidra.md` catalogues.

   The real talk/examine arm is elsewhere in the same function - the touch
   dispatch at `0x801D07C0..0x801D08DC` over the probe table `DAT_801F2254`,
   which that page documents separately. So this arm reads as the overworld's
   **open-the-map-view** button.

   One loose end for whoever picks this up: both this arm and the menu-open
   accept add `player.flags |= 0x01000000` **only** when `_DAT_8007B6A8` is
   set (`0x801D0244` and `0x801D0310`). The page glosses that bit as
   "talk / examine"; a bit set by the overworld map-view button and by a menu
   open, and only on the overworld, is unlikely to mean that.

4. `docs/reference/re-do-not-re-walk.md` should take the falsified
   "`FUN_801E76D4` is the world-map controller / has a normal-walk path"
   reading, and `re-settled-threads.md` the settled question. Both are outside
   this lane's declared doc scope.
