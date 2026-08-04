# Lane 2 handoff - the pause menu, walked by pad

## What this lane was asked, and what it found first

The replay port-entry report says 688 ports are statically live and 132 are
entered by a pad-driven playthrough. 501 rows sit in "live, never entered",
and that set conflates *"the replay does not go there"* with *"nothing wires
it."* This lane's slice was ~78 of those rows across the pause menu and save
UI, and the instrument for splitting them was supposed to be a ladder test.

The ladder could not be written first, because **the shared headless driver
could not reach the screens.** That is the lane's primary finding and it is
now closed.

## Finding 1 - `BootSession` had no sub-session stack (CLOSED)

**Evidence grade: disassembly-grounded model, measured defect.**

Retail's pause menu is two levels deep: the root list suspends itself on
confirm (`FieldMenuPhase::Suspended`) and the routed sub-screen owns the pad
until it finishes. Before this lane, `BootSession::tick` answered that suspend
by calling `menu.resume(false)` on the spot, with the comment "Headless
hosting has no sub-session UI stack". The consequence:

- a `set_pad` + `tick` driver could open the menu, move the root cursor, and
  observe the row gates;
- it could **not** enter Items, Magic, Equip, Status, Options, Load or Save at
  all - a Cross bounced straight back to browsing;
- both shipped hosts implemented the second level privately: the native
  window's `BootUiState::FieldMenu` arm
  (`crates/engine-shell/src/bin/legaia-engine/window/boot_cutscene.rs`) and
  the browser's `crates/web-viewer/src/play_menu.rs`.

So `BootSession` - the driver every oracle in the repo runs - was the weakest
of the three menu drivers, and each host carried its own copy of the stack.
That is host drift in the shared driver, not a test-only problem.

**Closed.** `crates/engine-shell/src/boot.rs` now owns the stack:

| Added | What it is |
|---|---|
| `BootSession::field_menu_sub` | the `FieldMenuSubsession` beneath a suspended root |
| `BootSession::options_state` | what the Config sub-session is built from and drained back into |
| `BootSession::set_save_rack` / `save_rack()` | the rack behind Load / Save, plus per-port block lists |
| `BootSession::save_flow()` | the shared `SaveScreenFlow` driving the two-stage card screen |
| `BootSession::last_save_commit` | the pick a finished Save / Load produced (persistence stays host-owned) |
| `BootSession::spell_level_notice` | the window-7 notice a menu cast returns |
| `tick_field_menu` / `tick_field_menu_sub` | the two-level driver: build on suspend, `tick_pad_edge`, drain via `apply_*_outcome`, `menu.resume(false)` |

`close_field_menu` drops the sub-session and resets the save flow, so a menu
closed out from under an open screen cannot resume into it.

The two hosts are **not** migrated onto this - that is deliberate, their files
are outside this lane's scope. They now have a shared driver to converge on
instead of a third copy. See "For the integration pass".

## Finding 2 - the Save row is unreachable by pad, anywhere (OPEN)

**Evidence grade: measured, with a disassembly-grounded explanation.**

Two facts collide and their union is empty:

1. `World::scene_save_allowed` comes from the scene MAN's header bit
   (`ManHeader::low_flag`). Across the disc that bit is set on the **three
   kingdom world maps only** and clear on every field scene, so the Save row
   is correctly grey in every town (`docs/subsystems/field-menu.md`, "Top-level
   pause menu"; pinned by `save_gate_field_menu.rs`).
2. A kingdom map runs in `SceneMode::WorldMap`, and **every** menu-open path in
   the port requires `SceneMode::Field`. `BootSession::tick`'s Start arm tests
   it, and the native window's Start edge tests it with the comment "on the
   world map the controller has its own" Start handler.

That handler does not exist. `WorldMapController` has no Start arm and nothing
calls `open_field_menu` from world-map mode. So the scenes that permit saving
are exactly the scenes whose mode refuses to open the menu, and there is no
pad route to the Save row in the port.

`menu_replay.rs::save_row_is_unreachable_by_pad_in_the_port` records this as a
live probe rather than a comment: it asserts the *current* shape (Start is
inert on `map01`, the row is offerable once the session is opened directly),
so the day the world-map Start handler lands the test fails and says so in its
own message.

Retail's own arrangement is worth pinning before fixing this: the menu-open
accept sits inside the **field locomotion controller** `FUN_801D01B0`, not in
a global Start handler. The world map's controller is `FUN_801E76D4`. Whether
retail's world map opens the same CARD-pair menu from its own controller, or
some other path, is not established here - it needs a disassembly pass over
`FUN_801E76D4`'s pad arm. Until then, "open the menu on the world map" is a
change with no oracle behind it.

## The ladder

`crates/engine-shell/tests/menu_replay.rs`. Nine ordered, cumulative rungs;
the run stops at the first it cannot clear and reports the stall, and the
score ratchets against `scripts/replays/menu_replay_baseline.toml` (asserted
`>=`, never auto-written).

| # | rung | what it proves |
|---|---|---|
| 1 | Start edge opens the menu | the production open path off a real pad edge, not an API call |
| 2 | root cursor over all seven rows + the gate buzzes | the gate is a confirm refusal, not a browse filter |
| 3 | Items opens, drives, backs out | the root suspends and a sub-session takes the pad |
| 4 | Magic | the spell screen builds off the disc spell catalog |
| 5 | Equip | the equip screen builds off the disc equipment table |
| 6 | Status | the party panel + its cursor |
| 7 | Options, with an edit that survives | the sub-session **drain** back into session state |
| 8 | Load: card rack -> read beat -> block grid -> commit | the two-stage save UI, end to end |
| 9 | back out to the field | the suspended scene mode is restored and the world ticks |

Rung 2 is worth reading twice. The gate is **not** a browse filter: retail's
picker walks all seven rows unconditionally and greys a blocked one, and the
refusal happens at the confirm. A first draft of this rung asserted the cursor
*skipped* the greyed row - it would have passed against a wrong model, because
the engine's separate row **mask** does remove rows and nothing in town01 uses
it. The rung now asserts both halves: all seven rows are landed on, and Cross
on the greyed Save row opens nothing.

Rung 8 reaches the card flow through **Load**, not Save. They are the same
`SaveSelectSession` over the same `SaveRack`, and Load is not scene-gated - so
the pill row, the "Now checking" beat, the 5x3 block grid and the commit are
all reachable by pad in a town even though Save is not. That is the workaround
Finding 2 forces, and it is also what a player does.

### Pad discipline

Every surface here reads `just_pressed`, so a held mask is one event. `tap()`
presses for one frame and releases on the next. Nothing in the ladder writes
session state: the fixture seeds *game* state a player would have (a new-game
party, its bag, a memory card in port 1) and everything else arrives by pad.

## Coverage delta

**Measured.** `cargo llvm-cov --release -p legaia-engine-shell --test menu_replay`
joined through `scripts/ci/replay-port-coverage.py`, against the critical-path
replay's report as the baseline.

- baseline live-unentered rows, all files: **501**
- of those, in the pause-menu / save-UI slice: **84**
- **entered by the menu ladder: 20**
- residue (in the slice, still unentered): **64**

Per file, baseline slice -> residue:

| file | before | after |
|---|---|---|
| `engine-core/src/spell_menu.rs` | 4 | **0** |
| `engine-core/src/pause_screens.rs` | 10 | 5 |
| `engine-core/src/save_select.rs` | 9 | 5 |
| `engine-core/src/equip_session.rs` | 4 | 2 |
| `engine-core/src/options.rs` | 3 | 0 |
| `engine-core/src/field_menu.rs` | 1 | 0 |
| `engine-ui/**` (five files) | 36 | 36 |
| `engine-core/src/save_subscreen.rs` | 11 | 11 |
| `engine-core/src/card_bu_io.rs` | 4 | 4 |

Non-vacuity, the way this repo means it: the ladder run reports 9/9 with per-rung
output and takes ~1.2 s; run from a directory with no `extracted/` it prints three
`[skip]` lines in 0.01 s. Both were run and the outputs differ. (`LEGAIA_DISC_BIN`
is **not** this test's gate - like `save_gate_field_menu.rs` its data source is
`extracted/`, and unsetting the variable changes nothing. Said plainly here so
nobody reads a green run as a disc-gated one.)

## Residue - and it is four different things, not one

This is the part of the lane worth more than the 20. "Live, never entered" reads
like one worklist; the 64 rows split into four causes, and only the last is
about how far a replay walks.

### A. Reachable only from host-private modules - 37 rows

Every `engine-ui` row (36) plus `save_select::SlotInfoMode`. The shared draw
builders have **no shared caller**: the only code that assembles a pause-menu
draw list lives in `crates/engine-shell/src/bin/legaia-engine/window/menu_draws.rs`
and `crates/web-viewer/src/play_menu.rs`. A `tests/` integration test cannot
import a bin's modules, so no library-level oracle can enter these - not this
ladder, and not one twice as long.

This is the same drift shape as Finding 1, one layer up: the *simulation* half
of the menu is now shared, the *draw* half still is not. Moving the assembly
into a library crate (`engine-ui` or `engine-render`) is what makes those 36
rows measurable at all. Note `check-ui-host-drift.py` already waives six of
these painters as orphans, which is the same fact arrived at from the other
side.

### B. Disclosed `NOT WIRED` - 15 rows

`save_subscreen.rs` (11) and `card_bu_io.rs` (4). Both modules say so in their
own doc comments: nothing constructs a `SaveScreenMachine` outside that
module's tests, and nothing services a `CardOp`. The engine's save UI runs on
`save_select`'s player-facing phase model instead, with `save_subscreen` kept
as the retail control-flow mirror beside it.

These were never a replay-reach question, and no ladder will ever turn them
green. They belong on a wiring worklist, not this one.

### C. A parser with no caller - 4 rows

`classify_card_directory`, `card_directory_scan`, `card_free_blocks`,
`CardIoMachine`. All four are reachable only through
`SaveSelectSession::from_card_directory`, which has **no caller anywhere in the
repo**. Both hosts build their rack from `card_port_snapshot` plus
host-scanned blocks, so a real memory-card *directory* is never parsed into a
session - the browser's `.mcr` import is the one place that would.

### D. Screen paths the ladder did not drive deep enough - 8 rows

The only rows where "extend the replay" is the right answer:

| row | what would reach it |
|---|---|
| `equip_session::apply_best_equipment` | the Equip screen's Best Equipment command |
| `equip_session::preview_candidate` | hovering a candidate in a slot's list |
| `pause_screens::target_panel_mode` | Items -> Use -> target select |
| `pause_screens::use_route_for_effect` | the same, one step further |
| `pause_screens::SpecialUseSession` (x3) | a special-route item (the landmark / warp class) |
| `field_save_screen_actor::tick` | the save screen's actor-VM display script |

Each is a few more taps on a rung that already exists. Worth doing; worth doing
*after* A, because A is 37 rows and a structural fix.

## Files touched

| File | What |
|---|---|
| `crates/engine-shell/src/boot.rs` | the sub-session stack (Finding 1) |
| `crates/engine-shell/tests/menu_replay.rs` | new - the ladder + two probes |
| `scripts/replays/menu_replay_baseline.toml` | new - ratchet, `reached = 9` |
| `docs/subsystems/field-menu.md` | the two-level model + where the stack lives |
| `docs/subsystems/save-screen.md` | the Save-row pad-route gap (Finding 2) |

Nothing outside the lane's declared scope was edited. `crates/engine-core/src/field_menu_dispatch.rs`
was in scope and needed no change - its API was already the right shape; it was
the caller that was missing.

## For the integration pass

1. **Converge the hosts onto `BootSession`'s stack.** The native window's
   `BootUiState::FieldMenu` arm and `web-viewer`'s `play_menu_*` now duplicate
   logic that lives on the session. Both were out of this lane's scope. The
   native one is the closer match (it already uses `FieldMenuSubsession` +
   `SaveScreenFlow`); the delta is that it owns `sub`, `options_state`,
   `save_flow` and the save-commit IO itself. Moving it is mostly deletion.
2. **Do not "fix" the world-map menu gate without a disassembly pass first**
   (Finding 2).
3. The ratchet line for `scripts/replays/menu_replay_baseline.toml` is printed
   by the test when the score rises. Raising it is a reviewed edit.
