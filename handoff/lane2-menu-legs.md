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

TBD

## Residue - rows the ladder visited and still did not enter

TBD

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
