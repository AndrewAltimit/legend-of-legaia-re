# Stale `NOT WIRED` triage

[`port-catalog.py --live-audit`](port-catalog.md) opens with a section titled
*Tagged `NOT WIRED` but analysed live*. Every row in it is a defect, and the two
possible defects point opposite ways: either the port was wired and nobody
removed the disclosure, or the reachability pass invented the edge that made it
read live. The first inflates the wiring worklist; the second deflates the
disclosed-inert count and hides a real gap.

This page is the per-row verdict, so the tag edits are mechanical rather than
re-derived. It is a snapshot of a worklist, not a specification - a row
disappears from it once the tag or the analysis is fixed. What outlives the rows
is the mechanism list plus the fix recipe each mechanism takes, both below.

## Verdicts

- **STALE-TAG** - a real, non-test caller reaches the tagged symbol. The tag
  comes off; the evidence column names the caller chain.
- **FALSE-EDGE** - the port is not reachable. The tag stays. The evidence names
  the colliding symbol the graph resolved through.
- **Wired, inert at runtime** - the call chain is real and production-only, but
  a runtime condition means the body is never entered: a gate that is never
  armed, a table never populated, a handler never installed. The disclosure
  token comes off, because the audit measures static reachability and would keep
  reporting it stale; what replaces it states the runtime fact and names the
  missing *data*. `emit_horizon` is the worked example.
- **UNCERTAIN** - neither could be established.

A host root is `fn main` in a `[[bin]]` target, a `#[wasm_bindgen]` export, or a
method of an `impl <ExternalDispatchTrait>` block - so **a CLI subcommand is a
host root**. A preservation-track port reached only from `asset boot-overlay` or
`mdec str-plan` is wired, and "no *engine* consumer" is a different claim that
has to be written as one.

## What the false edges are

Six mechanisms produce every FALSE-EDGE row this page has recorded, and only
the first is what `--live-audit` warns about. All six are name resolution
without type inference; they differ in *which* name space collides, and that is
what decides the fix.

**A generic method or constructor name.** `build_rust_graph` resolves `.name(`
against every in-tree method called `name` and never infers a receiver type, so
one `session.tick(...)` in the browser title driver links to all in-tree `tick`
methods, and one `new(` links to every in-tree `new`. This is the documented
over-approximation, and it is what makes `--not-live` a floor - but it means a
`NOT WIRED` port whose entry point is called `new`, `tick`, `add`, `len`,
`is_empty`, `default`, `normalize`, `from_byte` or `to_le_bytes` reads live no
matter how inert it is. Two of these edges can chain: the browser's
`session.tick(...)` reaches `BattleTutorial::tick`, whose own `dispatch(...)`
call then reaches `SaveScreenMachine::dispatch`, and from there every `tick_*`
sub-screen handler in `save_subscreen.rs`.

**A method name that is unique in-tree but ubiquitous in `std`.** The receiver
gate fires only where the name is ambiguous, so a name with exactly one in-tree
definition is never gated - and if `std` also defines it on a common type, every
call site in the workspace becomes an in-edge. `Rect12::to_le_bytes` in
`crates/engine-vm/src/title_prim.rs` was the case: it was the only in-tree
`fn to_le_bytes`, so each `x.to_le_bytes()` on an integer anywhere in a
reachable function linked to it. Uniqueness reads like precision here and is the
opposite.

**A method name that is unique in-tree but shadowed by a callable local.** The
same uniqueness escape, with the shadow coming from *inside* the workspace
rather than from `std`. `BARE_CALL_RE` matches any `name(` not preceded by a
dot, and a **closure parameter** invoked as `flag(..)` reads exactly like a free
call - so `FieldNpcAmbient::select_variant`'s `flag: impl Fn(u16) -> bool`
parameter resolved onto `SlideDir::flag`, the only in-tree `fn flag`, and
reported the whole of `crates/engine-audio/src/seq_calc.rs` live. Predicate and
emitter parameters (`flag`, `pred`, `emit`, `push`) are where this recurs,
because they are the names a small `impl` block also wants.

**A duplicate free-function name.** The receiver gate is defined over
`impl_type`, and a free function has none, so free-function edges are never
gated however many definitions share the name. `countdown_frame` existed twice -
in `crates/engine-core/src/baka_fighter_chrome.rs`, called every frame by the
Baka round chrome, and in `crates/engine-core/src/dance_tutorial.rs`, called by
nothing - and the one bare call linked to both.

**A bare identifier matching a free function.** The `IDENT_RE` pass links a bare
identifier to any free function of that name, which is how a function value
reaches `map` / `sort_by_key`. It does not distinguish a function value from a
**struct field** of the same name: the field `stat_deltas` in
`crates/engine-core/src/seru_stats.rs` links to the free `stat_deltas` in
`crates/engine-vm/src/world_map_overlay.rs`. Nor from a **local binding**: a
free function called `gate` collects an in-edge from every reachable function
that merely names a local `gate`, which is why the whole of
`battle_attack_camera.rs` read live from six unrelated callers. A short,
English-word free-function name is the worst case for this pass, because the
name is short precisely where it is also common.

**An anchor covering more than the tag.** A `//! PORT:` tag makes the anchor the
whole file, and the file is live if any non-test `fn` in it is reachable. A
`PORT:` tag on a plain data struct behaves the same way, because a type anchor
with no `impl` block in its file falls back to module scope. Neither the tag nor
the edge is wrong - the anchor is too coarse to tell them apart.

A **`PORT:` tag on a `const`** is the same shape and the easiest to miss,
because the tag looks precise in source. `collect_port_anchors` recognises a
`fn`, a `struct` / `enum` / `union` / `impl` / `trait`, or an enclosing function
body; a `const` is none of those, so resolution falls through to module scope
and the tag silently claims the whole file. The audit prints these as
`anchor = module` with symbol `(module)` on the *const's* line number, which
reads like a precise anchor unless the line is opened.

## The fix each mechanism takes

Two of the five are analysis defects and were fixed in the tool. The other three
are properties of the *names and anchors in source*, and the fix belongs there -
sharpening the shared permissive graph is the wrong move and has been tried and
reverted twice.

| Mechanism | Fix |
|---|---|
| Generic method / constructor name | The receiver gate, in the strict graph. Implemented. |
| Struct field read as a function value | Field-colon exclusion, in the strict graph. Implemented. |
| Unique in-tree name shadowing a `std` method | Rename the in-tree method so no `std` call site spells it. |
| Unique in-tree name shadowed by a callable local | Rename the in-tree method so no closure parameter spells it. |
| Duplicate free-function name | Rename the copy that has no caller. |
| Free-function name that is also a common local / field name | Rename it to something the rest of the tree does not spell. |
| Coarse anchor (module tag, or a tag on a data struct) | Move the anchor to the item that ports the address. |
| Coarse anchor (tag on a `const`) | Make the `const` a `REF:` and leave the `PORT:` on the function that computes the value. |

### Writing the fix: the disclosure token is matched as text

`NOT_WIRED_RE` is `NOT\s+WIRED` against the **whole comment block**, with no
anchoring. So a tag that explains *why* an item is not an inert port -
"deliberately not `NOT WIRED:`", "this is not a `NOT WIRED` case" - re-arms the
very disclosure it is disclaiming, and the row survives the edit looking
untouched. Say it without the token: "carries no inert-port disclosure", "not an
inert port", "wired, but inert at runtime". Re-run `--live-audit` after the edit
rather than trusting the prose, because this failure is invisible in review.

### Removing a false edge can expose ports it was masking

A spurious in-edge onto a function also makes everything that function calls
read live. Renaming the colliding symbol therefore *adds* rows to the
**undisclosed inert ports** section - its callees, which were live only through
it. That is the fix working, not a regression, but the disclosures those callees
now need are part of the same edit; leaving them turns one false claim into
several silent gaps. Check both section counts before and after.

The last four are source edits, and each wants a comment saying why the name or
the tag placement is the way it is - otherwise the next refactor undoes it and
the false accusation returns. `footstep.rs`, `anim_cue.rs`, `seq_calc.rs`,
`dance_tutorial.rs`, `title_prim.rs`, `vram_rect_copy.rs` and
`world_map_overlay.rs` each carry that note now.

### The two-graph split (implemented)

Sharpening the single shared graph would be wrong, and was tried and reverted
once for that reason. The over-approximation is load-bearing for `--not-live`:
biasing every ambiguity toward "reachable" is what makes the not-live list a
hard floor. It is only *this* question - is a disclosure stale - where a
spurious edge does damage, by manufacturing a false accusation against a correct
tag.

So `build_rust_graph(strict=True)` builds a **second** graph and only the
stale-tag test reads it. Every `live` / `--not-live` / `--live-only` verdict
stays on the permissive graph, unchanged. Nothing is traded: each question
consults the graph whose error mode is safe for it.

1. **Struct fields are excluded from the bare-identifier edge.** An identifier
   immediately followed by `:` - and not `::` - is a field declaration or a
   struct-literal key, so it is not a function value reaching `map`.
2. **Ambiguous method edges take a receiver gate.** A `.name(...)` or
   `name(...)` edge onto an `impl Type` method survives only if the calling file
   names `Type`, or defines the method itself.

   The gate fires **only where the name is ambiguous** (more than one candidate
   definition). That qualifier is not cosmetic: a receiver is routinely a local
   binding whose type the calling file never spells, as in
   `ctrl.run_horizon_emitter(..)`. Gating an already-unambiguous name on the
   spelling drops a real edge - which silently removes a correct row from this
   audit, the one failure mode the strict graph must not have.

   The qualifier has a cost, recorded above as its own mechanism: an
   unambiguous in-tree name is never gated even when `std` spells it too. A
   crate-root re-export is the other soft spot - `pub use footstep::{..,
   FootstepCadence}` makes `lib.rs` a file that "names the type", so any
   `.tick(` in `lib.rs` passes the gate onto `FootstepCadence::tick`.

### Anchor granularity

**Read a module anchor at the granularity the tag claims.** When a `//! PORT:`
tag names specific addresses and specific items in the file are inert, the live
verdict should be read off those items, not off any function in the file.

The tool still reads a module tag as the whole file, and a type tag with no
`impl` block as the whole file too. Closing that inside the tool needs the
`PORT:` tag to carry item-level information, so the practical fix is to *write*
it at item level:

- A `//! PORT:` line moves onto the function that implements the address. Every
  address the module tag listed keeps an anchor, and the anchor is now precise.
- A `PORT:` tag on a plain data struct - a return type or an input record, with
  the computation in a free function beside it - becomes a `REF:` tag naming the
  function that carries the `PORT:`. The address loses nothing: it is still
  tagged, on the item that ports it.
- Dropping a blanket `//! NOT WIRED` heading un-discloses every item anchor in
  the file, so each genuinely inert item needs its own `NOT WIRED:` line in the
  same edit. Doing only half of this converts a granularity row into a
  disclosure gap, which is the worse of the two.

That third point is why a blanket module disclosure is only safe while *nothing*
in the file has a caller. The moment one item is wired, the blanket asserts
something false about it and cannot be narrowed, because per-anchor disclosure
reads the module doc unconditionally.

## What the stale tags are

The FALSE-EDGE mechanisms above are properties of *names*. A STALE-TAG has its
own recurring shape, and it is worth stating separately because the fix is not
a rename.

**A disclosure that delegates to a sibling's tag.** A leaf kernel called from
exactly one place often discloses by pointing at that caller - "called only by
`X`, which is itself inert, see the tag there". The sentence is true when it is
written and becomes false the moment `X` is wired, silently: nothing in `X`'s
edit touches the leaf, and the leaf's own text still reads as a careful,
sourced disclosure. Five of the battle-intro anchors were exactly this. When a
tag's reason is *another tag*, wiring the cited item is an edit to both.

The transferable rule is the same one the anchor-granularity section states from
the other side: **wiring a caller is not done until every disclosure that named
it has been re-read.** Grep the file set for the wired symbol's name before
closing that work, not after the next audit.

## Rows

None open.

## How the recorded rows were closed

Kept as a record of which resolution each shape took, keyed by address and site
so a recurrence is recognisable rather than re-derived.

| addr | site | verdict | resolution |
|---|---|---|---|
| `80018db0` | `engine-audio/src/footstep.rs` | FALSE-EDGE | `FootstepCadence::tick` renamed `tick_cadence`; the crate's own `lib.rs` re-exports the type and calls `spu.tick()`, so the gate passed. |
| `800198e0`, `80058298`, `80058490` | `engine-vm/src/title_prim.rs` | FALSE-EDGE | Module tag moved onto `exec_sprite_descriptor` / `exec_clear_image` / `exec_move_image`; the file was live through `Rect12::to_le_bytes`. |
| `800468a4`, `80057914` | `engine-vm/src/vram_rect_copy.rs` | FALSE-EDGE | Module tag moved onto `enqueue` / `build_packet`, each with its own `NOT WIRED:`; the file is live through `op43_sub12_calls`, which no tag covers. |
| `80053cb8` | `engine-vm/src/battle_formulas/stat_init.rs` | STALE-TAG | `LegaiaMinigames::muscle_player_fighter`, under a `#[wasm_bindgen]` root, calls `init_party_battle_stats`, which calls `equip_stat_bonuses`. |
| `801d0750` | `engine-core/src/dance_tutorial.rs` | FALSE-EDGE | `countdown_frame` renamed `tutorial_countdown_frame`; the live `countdown_frame` is the Baka chrome's same-named free function. |
| `801e5b4c` | `engine-vm/src/world_map_overlay.rs` | RESOLVED | `resolve_equip_slot` was already reached through `dev_equip_commit::commit_equip`. The rest of the address is now reached too: `equip_stat_panel` is the whole sub-draw and `baka_hub_actors::entry_list` calls it where retail's only `jal` sits. |
| `801ead98` | `engine-vm/src/world_map_overlay.rs` | FALSE-EDGE | Module tags and the impl-less type tags dropped for per-item anchors. |
| `801eca08` | `engine-vm/src/world_map_overlay.rs` | FALSE-EDGE | `cursor_step` renamed `dev_menu_cursor_step`; the live `cursor_step` is `engine-core/src/baka_cabinet.rs`'s same-named free function. |
| `801ed710` | `engine-vm/src/world_map_overlay.rs` | STALE-TAG | `records_screen` / `decompose_play_time` are reached from `dev_records_model` in `engine-shell/src/bin/legaia-engine/window/dev_menu.rs` -> `PlayWindowApp::build_dev_records_draws` -> `tick_dev_menu`. |
| `8001fa68`, `800203ec`, `80020424`, `80020454`, `800204a4` | `engine-vm/src/scus_core_helpers.rs` | FALSE-EDGE | Cleared by the receiver gate; the collisions were `ActorNodePool::new` / `::default`. |
| `800421d4`, `80042310`, `800423e0`, `80042ee0`, `80042f4c`, `80043048`, `8004313c` | `save/src/retail_inventory.rs` | FALSE-EDGE | Cleared by the receiver gate; the collisions were `RetailInventory` / `ItemWindow` methods reached through the crate's `lib.rs` re-export. |
| `80046870` | `engine-vm/src/battle_helpers.rs` | FALSE-EDGE | Cleared by the receiver gate; the collision was `ScreenOrient::from_byte`. |
| `801d71b8` | `engine-vm/src/battle_attack_camera.rs` | FALSE-EDGE | `gate` renamed `attack_camera_gate`; a free function named `gate` is reached by the bare-identifier edge from every function that merely *mentions* one. |
| `801d2ebc` | `engine-vm/src/world_map_overlay.rs` | FALSE-EDGE | Cleared when the countdown scheduler moved to `escape_timer.rs`, which has a caller; the collision was `EscapeTimer::tick`. |
| `801d6d38`, `801d8a58`, `801d98f0`, `801dae24`, `801daef4`, `801dafd4`, `801dbc5c`, `801dc6b4`, `801dd12c`, `801dd1b8`, `801dd26c`, `801e4f40` | `engine-core/src/save_subscreen.rs` | FALSE-EDGE | Cleared by the receiver gate; the chain was `session.tick(` -> `BattleTutorial::tick` -> `dispatch(` -> `SaveScreenMachine::dispatch`. |
| `801db380`, `801db7f4`, `801dbd94` | `engine-core/src/shop.rs` | FALSE-EDGE | Cleared by the receiver gate; the collision was the session constructors' `new(`. |
| `800508dc` | `engine-audio/src/anim_cue.rs` | FALSE-EDGE | `AnimCueState::tick` renamed `tick_cues`; the crate's own `lib.rs` re-exports the type and calls `spu.tick()`, so the gate passed - the same shape as `footstep.rs` above, in the same file. |
| `80062f98`, `8006320c`, `8006352c`, `80063aa8`, `800649b0` | `engine-audio/src/seq_calc.rs` | FALSE-EDGE, then wired | `SlideDir::flag` renamed `flag_bit`; the only in-tree `fn flag`, reached from the `flag(..)` closure parameter of `FieldNpcAmbient::select_variant`. The module blanket then came off for a different reason: `note-trace --seq-calc` gives the tier a real host. |
| `8001eef0`, `80025ba0`, `8003e360` | `asset/src/boot_overlay.rs` | STALE-TAG | The `asset boot-overlay` subcommand made each one a real callee of a `[[bin]]` `main`. Each tag already named the CLI as its consumer while still heading itself as inert. |
| `8002574c` | `asset/src/boot_overlay.rs` | STALE-TAG | Same subcommand reads `CARD_TIM_EXTRACTION_INDEX`. Also the `const` coarse-anchor shape: the tag reports as `anchor = module`. |
| `801cfff0`, `801d069c`, `801d0fa8`, `801d3230` | `asset/src/minigame_slot_scene.rs` | STALE-TAG | `asset slot-scene` drives the reel kernels, the composer, the clear path and the placement blit. The remaining gap is a *renderer*, not a caller, and now says so. |
| `801cf56c`, `801cf740` | `mdec/src/str_player.rs` | STALE-TAG | `mdec str-plan` calls both on `DecodeEnv`. The real gaps (the ring drops per-frame dimensions; the port decodes whole frames) are unchanged and kept as prose. |
| `801d26cc` | `engine-core/src/fishing_actors.rs` | STALE-TAG | `bite_interval` / `bite_credit_override` / `roll_hit_type` reached from `LegaiaMinigames::fishing_reel` -> `FishingSession::reel` -> `BandCheck::tick`. The file's blanket `# NOT WIRED` heading came off and ten still-inert items each took their own line. |
| `801cf00c`, `801d6704` | `engine-core/src/mode_entry_init.rs` | FALSE-EDGE | Coarse anchors: `DuelOverlayInit` is a struct with no `impl`, `FIELD_INIT_STEPS` is a `const`. Both became `REF:`; the `PORT:` stays on `duel_overlay_init` and the file's fn anchors. The file is live through `field_spawn`. |
| `801d84b4` | `engine-core/src/field_submode.rs` | FALSE-EDGE | Same shape: `CardRequest` is a struct and the file declares no `impl` at all, so the type anchor widened to a module live through `open_submode`. Now a `REF:`; `request_card_mode` keeps the `PORT:`. |
| `801d4a60` | `engine-core/src/field_actor_program.rs` | FALSE-EDGE | `step` renamed `step_scene_program`. A free `fn step` is never receiver-gated and collected edges from the live `motion_vm::step` and from every reachable function naming a local `step` (`fishing_advance_cast(&mut self, step: i32)` fired it). Exposed `entry_successor` / `lift_step`, which then needed their own disclosures. |
| `801dd0c0` | `engine-core/src/menu_item_category.rs` | Wired, inert at runtime | Statically reached via `EquipSession::best_equipment_now`'s `weapon_category_score` closure. Nothing calls `with_weapon_category`, so the table is always empty and the `is_empty()` arm short-circuits. Rewritten in the `emit_horizon` idiom. |
| `801ddc20` | `engine-core/src/field_actor_kernels.rs` | Wired, inert at runtime | `World::tick_handler_actors` dispatches it every tick, but no host installs `ActorHandler::ColourTween`, so the `a.colour_tween` guard is always `None`. The tag said as much already; only the token had to go. |
| `801cf1b0` | `engine-vm/src/battle_intro_transition.rs` | STALE-TAG | `build_intro_quad`'s only caller `tick_curtain` draws end to end. Its reason - the PROT 0979 descriptor table "which the engine never loads" - was falsified by `IntroQuadTable::parse_overlay`, which the caller's own tag already recorded. |
| `801cfbb4`, `801d0164` | `engine-vm/src/battle_intro_particles.rs` | STALE-TAG | `BattleIntro::new` seeds the grid on every particle-style transition. The module blanket came off; the file is wholly wired, so nothing in it needed a per-item line. What survives as prose is the two real gaps: no packet consumer, and computed trig standing in for the overlay's height tables. |
| `801cfda0`, `801d0370` | `engine-vm/src/battle_intro_styles.rs` | STALE-TAG | `step_particle` is reached from `tick_particle_field`, whose own tag had already been rewritten to `NOT DRAWN`. The sibling-delegation shape above. |
| `801d0e54` | `engine-vm/src/battle_intro_tiles.rs` | STALE-TAG | Same shape: `step_tile` cited `tick_tile_grid`, which by then read `WIRED, without a draw`. |
| `801d1a20` | `engine-vm/src/battle_intro_swirl.rs` | STALE-TAG | Same shape: `swirl_band_draw` cited `tick_swirl`, likewise already `WIRED, without a draw`. |
| `801e1934` | `engine-core/src/card_flow.rs` | FALSE-EDGE | `save_title_digits` renamed `block_title_digits`; the live copy is `legaia_save::card::save_title_digits`, which the browser card rack writes through. A duplicate free-function name across two crates, never receiver-gated. Its own caller `save_block_summary` has no non-test call site. |

The `world_map_overlay.rs` rows are the worked example of the whole granularity
shape: a genuinely wired item made a module blanket false, and through the
module scope it also lifted four data-struct type anchors to live.

They are also the worked example of the **opposite** failure, and that is the
more expensive one. Splitting the module tag into per-item anchors and giving
each inert item its own `NOT WIRED:` line is only correct for the items that
are actually inert - and the `801ed710` pair was not. `records_screen` and
`decompose_play_time` had a real, non-test caller in the native window's
developer-menu Records page the whole time, so writing them a per-item
disclosure turned one coarse-anchor row into two false statements in source,
each reading as a declared wiring gap. A FALSE-EDGE verdict says the port is
unreachable; before acting on one, look for the caller rather than for the
collision, because a collision can always be found and a caller cannot.

## Superseded rows

`801d7ea0` (`emit_horizon`, `crates/engine-vm/src/world_map_horizon.rs`) was the
one STALE-TAG an earlier snapshot carried, and it needed a rewrite rather than a
deletion. `emit_horizon` is statically reachable through three unambiguous
non-test hops, so a tag clause reading "reached only from tests" is false - but
the port is still inert, because the gate `run_horizon_emitter` consults is
never armed: `EmitterGate::arm` has no non-test caller. That is a runtime fact
the reachability pass does not model and cannot be expressed by the tag as the
audit reads it. See
[`live-audit-triage.md`](live-audit-triage.md#disclose-texts) for the `arm`
disclosure that pins the other half.
