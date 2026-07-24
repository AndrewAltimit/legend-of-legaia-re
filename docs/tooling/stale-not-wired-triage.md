# Stale `NOT WIRED` triage

[`port-catalog.py --live-audit`](port-catalog.md) opens with a section titled
*Tagged `NOT WIRED` but analysed live*. Every row in it is a defect, and the two
possible defects point opposite ways: either the port was wired and nobody
removed the disclosure, or the reachability pass invented the edge that made it
read live. The first inflates the wiring worklist; the second deflates the
disclosed-inert count and hides a real gap.

This page is the per-row verdict, so the tag edits are mechanical rather than
re-derived. It is a snapshot of a worklist, not a specification - a row
disappears from it once the tag or the analysis is fixed.

## Verdicts

- **STALE-TAG** - a real, non-test caller reaches the tagged symbol. The tag
  comes off; the evidence column names the caller chain.
- **FALSE-EDGE** - the port is not reachable. The tag stays. The evidence names
  the colliding symbol the graph resolved through.
- **UNCERTAIN** - neither could be established.

## What the false edges are

Three mechanisms produce every FALSE-EDGE row, and only the first is what
`--live-audit` warns about.

**A generic method or constructor name.** `build_rust_graph` resolves `.name(`
against every in-tree method called `name` and never infers a receiver type, so
one `session.tick(...)` in the browser title driver links to all 63 in-tree
`tick` methods, and one `new(` links to all 192 in-tree `new`. This is the
documented over-approximation, and it is what makes `--not-live` a floor - but
it means a `NOT WIRED` port whose entry point is called `new`, `tick`, `add`,
`len`, `is_empty`, `default`, `normalize`, `from_byte` or `to_le_bytes` reads
live no matter how inert it is. Two of these edges can chain: the browser's
`session.tick(...)` reaches `BattleTutorial::tick`, whose own `dispatch(...)`
call then reaches `SaveScreenMachine::dispatch`, and from there every
`tick_*` sub-screen handler in `save_subscreen.rs`.

**A bare identifier matching a free function.** The `IDENT_RE` pass links a bare
identifier to any free function of that name, which is how a function value
reaches `map` / `sort_by_key`. It does not distinguish a function value from a
**struct field** of the same name: the field `stat_deltas` in
`crates/engine-core/src/seru_stats.rs` links to the free `stat_deltas` in
`crates/engine-vm/src/world_map_overlay.rs`.

**A module anchor covering more than the tag.** A `//! PORT:` tag makes the
anchor the whole file, and the file is live if any non-test `fn` in it is
reachable. `crates/engine-vm/src/vram_rect_copy.rs` is the clean example: its
module tag covers `build_packet` and `enqueue`, which nothing calls, while the
file's third routine `op43_sub12_calls` is genuinely reached from the field VM's
sub-op `0x43` arm. Neither the tag nor the edge is wrong - the anchor is too
coarse to tell them apart.

## What that means for the reachability pass

Fixing this is a change to `scripts/ci/port-catalog.py`, not to any tag, and it
is worth separating from the over-approximation the graph is deliberately built
on. Three sharpenings, in increasing cost:

1. **Exclude struct fields from the bare-identifier edge.** A `name:` inside a
   struct literal or a `self.name` projection is not a function value. This is a
   pure false-positive removal with no reachability cost.
2. **Gate an ambiguous `.name(` / `name(` edge on the calling file naming the
   callee's type.** If `boot_title.rs` never writes `EscapeTimer`, no edge from
   it can land on `EscapeTimer::tick`. This keeps the graph over-approximating
   within a file's own vocabulary while dropping cross-crate collisions on
   `new` / `tick`.
3. **Report a module anchor at the granularity the tag claims.** When a
   `//! PORT:` tag names specific addresses and the module doc marks specific
   items `NOT WIRED`, the live verdict should be read off those items, not off
   any function in the file.

Until then, the audit's first section is a prompt, and this page is the
answer for the rows it currently lists.

## Rows

| addr | crate | symbol | site | verdict | evidence |
|---|---|---|---|---|---|
| `80018db0` | engine-audio | `tick` | `crates/engine-audio/src/footstep.rs:154` | FALSE-EDGE | `.tick()` collides with `TitleSession::tick` (boot_title.rs:133); `FootstepCadence` is named only in its own file and lib.rs |
| `800198e0` | engine-vm | `(module)` | `crates/engine-vm/src/title_prim.rs:3` | FALSE-EDGE | `new(` / `.to_le_bytes()` collide with `Rect12::new` / `Rect12::to_le_bytes`; `Rect12` is named only in its own file |
| `8001fa68` | engine-vm | `(module)` | `crates/engine-vm/src/scus_core_helpers.rs:4` | FALSE-EDGE | `new(` / `default(` collide with `ActorNodePool::new` / `::default`; `ActorNodePool` is named only in its own file |
| `800203ec` | engine-vm | `(module)` | `crates/engine-vm/src/scus_core_helpers.rs:4` | FALSE-EDGE | `new(` / `default(` collide with `ActorNodePool::new` / `::default`; `ActorNodePool` is named only in its own file |
| `800203ec` | engine-vm | `new` | `crates/engine-vm/src/scus_core_helpers.rs:135` | FALSE-EDGE | `new(` / `default(` collide with `ActorNodePool::new` / `::default`; `ActorNodePool` is named only in its own file |
| `80020424` | engine-vm | `(module)` | `crates/engine-vm/src/scus_core_helpers.rs:4` | FALSE-EDGE | `new(` / `default(` collide with `ActorNodePool::new` / `::default`; `ActorNodePool` is named only in its own file |
| `80020454` | engine-vm | `(module)` | `crates/engine-vm/src/scus_core_helpers.rs:4` | FALSE-EDGE | `new(` / `default(` collide with `ActorNodePool::new` / `::default`; `ActorNodePool` is named only in its own file |
| `800204a4` | engine-vm | `(module)` | `crates/engine-vm/src/scus_core_helpers.rs:4` | FALSE-EDGE | `new(` / `default(` collide with `ActorNodePool::new` / `::default`; `ActorNodePool` is named only in its own file |
| `800421d4` | save | `(module)` | `crates/save/src/retail_inventory.rs:124` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `800421d4` | save | `add` | `crates/save/src/retail_inventory.rs:582` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `80042310` | save | `(module)` | `crates/save/src/retail_inventory.rs:125` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `800423e0` | save | `(module)` | `crates/save/src/retail_inventory.rs:125` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `800423e0` | save | `normalize` | `crates/save/src/retail_inventory.rs:553` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `80042ee0` | save | `(module)` | `crates/save/src/retail_inventory.rs:124` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `80042ee0` | save | `find_slot` | `crates/save/src/retail_inventory.rs:457` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `80042f4c` | save | `(module)` | `crates/save/src/retail_inventory.rs:124` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `80043048` | save | `(module)` | `crates/save/src/retail_inventory.rs:127` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `8004313c` | save | `(module)` | `crates/save/src/retail_inventory.rs:126` | FALSE-EDGE | `new(` / `.add(` / `normalize(` / `.len()` / `.is_empty()` collide with `RetailInventory` and `ItemWindow` methods; neither type is named outside its own file and the crate's `lib.rs` re-export |
| `80046870` | engine-vm | `(module)` | `crates/engine-vm/src/battle_helpers.rs:169` | FALSE-EDGE | `from_byte(` collides with `ScreenOrient::from_byte`; `ScreenOrient` is named only in its own file and `advance_gauge` is cited only in a doc comment (battle_action/validator.rs:152) |
| `800468a4` | engine-vm | `(module)` | `crates/engine-vm/src/vram_rect_copy.rs:4` | FALSE-EDGE | coarse module anchor: the file is live via `op43_sub12_calls` (actor_ctrl.rs:277), which no `PORT:` tag covers; the tagged `build_packet` / `enqueue` have no non-test caller |
| `80057914` | engine-vm | `(module)` | `crates/engine-vm/src/vram_rect_copy.rs:4` | FALSE-EDGE | coarse module anchor: the file is live via `op43_sub12_calls` (actor_ctrl.rs:277), which no `PORT:` tag covers; the tagged `build_packet` / `enqueue` have no non-test caller |
| `80058298` | engine-vm | `(module)` | `crates/engine-vm/src/title_prim.rs:3` | FALSE-EDGE | `new(` / `.to_le_bytes()` collide with `Rect12::new` / `Rect12::to_le_bytes`; `Rect12` is named only in its own file |
| `80058490` | engine-vm | `(module)` | `crates/engine-vm/src/title_prim.rs:3` | FALSE-EDGE | `new(` / `.to_le_bytes()` collide with `Rect12::new` / `Rect12::to_le_bytes`; `Rect12` is named only in its own file |
| `801d2ebc` | engine-vm | `(module)` | `crates/engine-vm/src/world_map_overlay.rs:19` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801d2ebc` | engine-vm | `TimerInk` | `crates/engine-vm/src/world_map_overlay.rs:437` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801d2ebc` | engine-vm | `TimerFlagEvents` | `crates/engine-vm/src/world_map_overlay.rs:467` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801d2ebc` | engine-vm | `EscapeTimer` | `crates/engine-vm/src/world_map_overlay.rs:478` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801d2ebc` | engine-vm | `tick` | `crates/engine-vm/src/world_map_overlay.rs:499` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801d6d38` | engine-core | `tick_confirm_yes_no` | `crates/engine-core/src/save_subscreen.rs:438` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801d7ea0` | engine-vm | `emit_horizon` | `crates/engine-vm/src/world_map_horizon.rs:214` | STALE-TAG | `World::tick_world_map_horizon` (world/worldmap.rs:83) -> `WorldMapController::run_horizon_emitter` (world_map.rs:209) -> `emit_horizon` (world_map_horizon.rs:214), all non-test |
| `801d8a58` | engine-core | `tick_confirm_exit` | `crates/engine-core/src/save_subscreen.rs:509` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801d98f0` | engine-core | `tick_party_picker` | `crates/engine-core/src/save_subscreen.rs:541` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dae24` | engine-core | `tick_card_driver` | `crates/engine-core/src/save_subscreen.rs:577` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801daef4` | engine-core | `tick_card_driver` | `crates/engine-core/src/save_subscreen.rs:577` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dafd4` | engine-core | `tick_save_confirm` | `crates/engine-core/src/save_subscreen.rs:622` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801db380` | engine-core | `BuyRecipientSession` | `crates/engine-core/src/shop.rs:666` | FALSE-EDGE | `new(` collides with the session constructors; all three types are constructed only inside this file's `#[cfg(test)]` module |
| `801db7f4` | engine-core | `BuyQuantitySession` | `crates/engine-core/src/shop.rs:506` | FALSE-EDGE | `new(` collides with the session constructors; all three types are constructed only inside this file's `#[cfg(test)]` module |
| `801dbc5c` | engine-core | `tick_quantity_spinner` | `crates/engine-core/src/save_subscreen.rs:667` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dbd94` | engine-core | `SellQuantitySession` | `crates/engine-core/src/shop.rs:263` | FALSE-EDGE | `new(` collides with the session constructors; all three types are constructed only inside this file's `#[cfg(test)]` module |
| `801dc6b4` | engine-core | `(module)` | `crates/engine-core/src/save_subscreen.rs:3` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dc6b4` | engine-core | `SaveScreenMachine` | `crates/engine-core/src/save_subscreen.rs:275` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dd12c` | engine-core | `tick_final_exit` | `crates/engine-core/src/save_subscreen.rs:413` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dd1b8` | engine-core | `tick_post_save_return` | `crates/engine-core/src/save_subscreen.rs:473` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801dd26c` | engine-core | `tick_pad_release_wait` | `crates/engine-core/src/save_subscreen.rs:491` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801e4f40` | engine-core | `(module)` | `crates/engine-core/src/save_subscreen.rs:3` | FALSE-EDGE | `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`; both hops are name collisions and `SaveScreenMachine` is named nowhere outside its own file |
| `801e5b4c` | engine-vm | `(module)` | `crates/engine-vm/src/world_map_overlay.rs:20` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801e5b4c` | engine-vm | `StatDelta` | `crates/engine-vm/src/world_map_overlay.rs:542` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801e5b4c` | engine-vm | `can_equip` | `crates/engine-vm/src/world_map_overlay.rs:584` | FALSE-EDGE | bare-identifier edge from `BuyRecipientSession::new` (shop.rs), itself reached only through the `new(` collision |
| `801e5b4c` | engine-vm | `stat_deltas` | `crates/engine-vm/src/world_map_overlay.rs:612` | FALSE-EDGE | bare-identifier edge onto the struct FIELD `stat_deltas` (seru_stats.rs:87), not a call; the free fn is unique in-tree |
| `801ead98` | engine-vm | `(module)` | `crates/engine-vm/src/world_map_overlay.rs:16` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801eca08` | engine-vm | `(module)` | `crates/engine-vm/src/world_map_overlay.rs:17` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801eca08` | engine-vm | `PanelGeometry` | `crates/engine-vm/src/world_map_overlay.rs:215` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801ed710` | engine-vm | `(module)` | `crates/engine-vm/src/world_map_overlay.rs:18` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801ed710` | engine-vm | `CharRecordStats` | `crates/engine-vm/src/world_map_overlay.rs:322` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |
| `801ed710` | engine-vm | `RecordsScreen` | `crates/engine-vm/src/world_map_overlay.rs:342` | FALSE-EDGE | `.tick()` collides with `EscapeTimer::tick`; no type in this file is named outside it |

The single STALE-TAG row needs a rewrite rather than a deletion. `emit_horizon`
is statically reachable through three unambiguous non-test hops, so the tag's
opening clause "reached only from tests" is false - but the rest of the same
comment is correct and explains why the port is still inert: the gate
`run_horizon_emitter` consults is never armed, because `EmitterGate::arm` has no
non-test caller. That is a runtime fact the reachability pass does not model and
cannot be expressed by the tag as the audit reads it.
