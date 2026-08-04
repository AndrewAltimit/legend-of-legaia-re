# Lane 2 handoff - party wipe returns to the title screen

## What landed

Retail's game over is **two stores and a hand-off**, not a screen and not a
menu. The port's three-row Continue / Retry / Quit panel is deleted; both
hosts now hold briefly, draw nothing, and push their title session.

The pinning is in `crates/engine-core/src/game_over.rs`'s module doc and in
`docs/subsystems/battle.md` § party wipe.

## Files touched OUTSIDE the lane's declared scope

The declared scope named `window/{run,hud,title_save_draws}.rs` and
`web-viewer/src/{play,boot_title}.rs`, but the game-over code does not live
in any of them. The real call sites are listed below with what changed, so
reconciliation can review or re-apply them.

| File | Hunk | Why it had to move |
|---|---|---|
| `crates/engine-shell/.../window/boot_cutscene.rs` | `tick_boot_ui`'s `BootUiState::GameOver` arm; `boot_ui_draws`' arm (was `game_over_draws`, now `Vec::new()`); deleted `fn game_over_draws` | the native routing + panel draw |
| `crates/engine-shell/.../window/event_handler/redraw.rs` | the `world.game_over` raise | dropped the save scan that fed `continue_enabled` |
| `crates/web-viewer/src/play_battle.rs` | `game_over_input`, `poll_game_over`, `overlay_draws`' wipe arm; deleted `game_over_continue_enabled` + `game_over_draws` | the browser routing + panel draw |
| `crates/web-viewer/src/runtime.rs` | doc comment on the `game_over` field | described the deleted panel |
| `crates/engine-render/src/tests/menu_overlays.rs` | deleted `game_over_dim_continue_when_disabled` | a test **asserting the defect** (it asserted a dimmed Continue row) |
| `crates/engine-core/tests/menu_suite_e2e.rs` | replaced `game_over_continue_outcome` + `game_over_skips_continue_when_no_save` | same - both drove the invented cursor |
| `scripts/ci/check-ui-host-drift.py` | the game-over `SIM_PAIRS` row | its `pattern_same` rule pointed at two deleted `game_over_draws_for` calls; replaced with a `symbols_all` rule pairing the two **routing** sites on `GameOverOutcome::ReturnToTitle` |

`site/js/play-app.js` (Lane 5) was **not** edited - see below.

## For LANE 5 - `site/js/play-app.js`

**No change is required for correctness.** The page's game-over block still
works: `rt.game_over_input(edge)` now ignores its argument, returns `""`
while the hand-off holds and `"quit"` once when it resolves, so `onQuit()`
fires exactly as before and `site/_content/play.html` re-runs the boot
title. The wasm export kept its name precisely so the page would not break.

Two optional follow-ups, in preference order:

1. **The comment at `site/js/play-app.js:1554-1563` is now wrong.** It says
   "the panel is a live `GameOverSession` on both hosts now, so the pad edge
   routes into it and the picked row comes back out ... Retail's destination
   on a wipe is unpinned, so the panel itself is an engine presentation".
   All three claims are stale. Suggested replacement:

   ```js
          /* Party wipe: retail's answer is the title screen and nothing else.
           * The wipe arm of FUN_8003AEB0 stores game_mode = 0x16 (CARD INIT)
           * + _DAT_8007BB00 = 1 and the title overlay takes the screen - no
           * art, no menu, no choice. The engine holds for the length of the
           * title's own fade and then reports 'quit' once; the pad word we
           * pass is ignored on purpose. Handled before the pad is fed in so
           * the same press does not also walk the player. */
   ```

2. **Optional rename.** `rt.game_over_input(edge)` is now a pure tick and the
   name is only kept as the page ABI. If Lane 5 (or a follow-up) wants
   `game_over_tick()`, the Rust side is
   `crates/web-viewer/src/play_battle.rs::game_over_input`; the page call is
   the single site at `play-app.js:1569`. Note `game_over_input(` is also
   listed in `PAD_HOST_MARKERS` in `scripts/ci/check-ui-host-drift.py` (the
   tier-5 "is this source pad-driving" marker set) - a rename must update
   that entry too or the play page silently drops out of the keyboard-table
   scan.

## Other notes for whoever owns these files

### `crates/engine-vm/src/title_overlay.rs` - wrong global in a doc comment

The module doc says:

> A conditional branch (line 397, `0x801DD97C`) routes to
> `state[+0x204] = 0x11` instead when a sentinel at `_DAT_80084500` reads
> `1` - the "skip intro / direct to attract" hand-off.

The disassembly at that site is

```asm
801dd968  lw a0,-0x4500(v0)     ; v0 = 0x80080000  ->  0x8007BB00
801dd970  beq a0,zero,0x801dda34
801dd978  li v0,0x11
801dd97c  sw v0,0x204(a2)
```

so the sentinel is **`_DAT_8007BB00`**, the title-screen entry-context word
the wipe/attract/back-to-title sites all write - not `_DAT_80084500`. The
same variant's `AttractDelay` doc adds that it "decrements an
`8 * frame_scalar` accumulator at `_DAT_8008454C`"; the accumulator the
`0x11` handler drains at `0x801DDAEC` is `-0x454c(0x80080000)` =
**`_DAT_8007BAB4`**, the screen-fade level. Both look like a
`0x8008` / `0x8007` base slip on a negative displacement. Nothing depends on
the wrong names today, but they are the two globals this lane's work rests
on, so they are worth correcting.

### `crates/engine-core/src/world/battle/teardown.rs` - BGM on a wipe

Retail's wipe arm ends with `FUN_800266E0(0x8007052C)` - the BGM pause, the
same primitive as field-VM BGM sub-op 2. The port emits that pause on the
*scripted* trigger (`vm_hosts.rs::op4c_n_e_sub_a_call_c7ec`, in scope, left
as it was) but **not** on the battle-teardown wipe (`teardown.rs:92` just
sets `self.game_over = true`, and the surrounding code may still call
`restore_field_bgm`). A wipe that hands to the title should not restore the
field track first. One-line fix, but `teardown.rs` was outside this lane.
