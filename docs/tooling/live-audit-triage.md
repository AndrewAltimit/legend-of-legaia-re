# Live-audit triage: `engine-core` and `engine-vm`

Per-anchor verdicts for the `engine-core` and `engine-vm` rows of the
"undisclosed inert ports" section of
[`port-catalog.py --live-audit`](port-catalog.md#the-audit). Each anchor gets
a verdict, with the evidence that produced it.

This is a working page: it exists so the wiring work is mechanical rather than
re-derived. It is keyed by address + site, so a row survives edits to the
surrounding file, and it names no counts of project state.

Reproduce the input with:

```bash
python3 scripts/ci/port-catalog.py --live-audit   # -> target/port-catalog/live-audit.md
```

## Verdict vocabulary

| Verdict | Meaning |
|---|---|
| `FALSE INERT` | The port **is** on a production path. The uncorrected audit could not see the edge; no source change is wanted, and the corrected audit agrees. |
| `WIRE` | Genuinely unreached, and a host call site should exist. The row names that call site. |
| `DISCLOSE` | Genuinely unreached for a structural reason. The row supplies the exact `// NOT WIRED:` text to paste. |
| `DELETE` | Redundant with an existing symbol that already covers the same retail routine. |
| `VERIFY` | Inertness could not be settled here, usually because a concurrent lane held the file. The row says what was checked and what is still open. Do not paste a tag on these. No row carries it now. |

A `DISCLOSE` reason must say *why* there is no caller. "No caller" restates the
audit. The useful form names what must exist first - a host screen, a state
shape, an id space the engine does not carry.

**A verdict here is a hypothesis with evidence attached, not a settled fact.** Several
have since been overturned by work that held the disassembly: the `minigame_return_warp`
`WIRE` rested on state that no production code writes (corrected in place below), the
`build_strip` row described a build the port already performed correctly, the
`symbol_pad_bit` `DELETE` cited a rustdoc link as a call site, and the `motion_pause_kick`
reason named a touch-event gap the port does not have. Re-check a row against the dumps
before acting on it, and correct it here when it does not hold - a stale verdict in this
file propagates into source as a `NOT WIRED:` tag, which is the failure direction hardest
to detect later.

**The four analysis defects this triage found are fixed in the tool**, and the
verdicts below are stated against the corrected audit. The order mattered:
acting on the uncorrected audit would have written false `NOT WIRED:` tags into
the tree, which is the failure direction hardest to detect later - a wrong
disclosure looks exactly like a correct one, and the next audit agrees with it.
The `FALSE INERT` rows are kept as the regression set that any future change to
the reachability pass has to keep green.

### How each verdict was settled

Three independent checks, because the audit's own graph was what stood under
suspicion:

1. **Reverse-edge walk** over the audit's graph, to the point where each chain
   stops.
2. **Textual sweep** for every anchor symbol and every intermediate caller
   across the host crates (`engine-shell`, `web-viewer`, `asset-viewer`,
   `engine-render`, `engine-ui`). Positive control: the same sweep finds the
   known host references to `confirm_menu`, `select_owned_rod`, `step_clut_fx`,
   `score_percent`, `persistent_hud_draws` and `exit_slot_machine`, so a zero is
   a real zero.
3. **Corrected-reachability re-run** of `port-catalog.py --live-audit` with all
   four defects fixed, diffed against the uncorrected run section by section.

Check 3 flipped one row that checks 1 and 2 had left as `DISCLOSE`
(`from_model_sel`) - the reason a corrected graph, not hand-tracing, is what a
tag should be written against.

The correction moves anchors in one direction only. Every anchor the corrected
audit calls inert was called inert before it, so nothing became a *newly*
claimed wiring gap and `--not-live` stayed a floor.

## Analysis defects this triage found

Four classes of audit false negative, each verified against a positive control,
and all four now fixed in `scripts/ci/port-catalog.py`. Together they account
for every `FALSE INERT` row below. The mechanisms are kept here because each one
is a shape that can recur - a new externally-dispatched trait, a new tag on a
struct with no `impl` - and because they are what the regression set tests.

### Trait default methods are invisible as call targets

`build_rust_graph` records a function's `impl_type` only from `impl` blocks
(`_impl_spans` scans the `impl` keyword). A default method in a `trait` body
therefore has `impl_type = None` and lands in `free_by_name`, while a caller
writing `host.method(...)` is matched by `METHOD_CALL_RE` against
`methods_by_name` only. The two never meet, so a trait default method has zero
in-edges no matter how many hosts run it.

Positive control: `op4c_n5_sub0_set_actor_model` has two definitions. The
`impl TestHost` copy resolves live from `op_4c_n5` in
`crates/engine-vm/src/field/step/menu_ctrl/nibble_5_6_7.rs`, proving the caller
is reachable; the `trait FieldHost` default copy at
`crates/engine-vm/src/field/host.rs:1226` has zero in-edges from the same call.

**Fixed** by scanning `trait Name { }` bodies alongside `impl` blocks and giving
a default method its trait's name as `impl_type`. It is listed among both the
methods and the free functions, so the change only ever adds edges.

### winit `ApplicationHandler` callbacks are unreachable from `main`

The GUI hosts hand a struct to `event_loop.run_app(&mut app)`. winit then calls
`window_event` / `resumed` / `about_to_wait` on it. That dispatch crosses an
external crate, so the graph has no edge into it, and the whole tree below those
callbacks is unreachable - including `handle_keyboard`, `handle_redraw`,
`build_hud` and everything in `engine-core` they call.

Verified by walking reverse edges from each affected anchor: every chain
terminates at an `impl ApplicationHandler` method and at nothing else. This
affects `crates/engine-shell/src/bin/legaia-engine/window/` and every
`asset-viewer` GUI app.

**It was not a root-set gap.** The `[[bin]]` root is followed correctly:
`main` is reachable and so is `cmd_play_window`, the function that builds the
app. The chain died one call later, at `event_loop.run_app(&mut app)` - an
external winit call the graph cannot follow into the `impl ApplicationHandler`
block.

This is the same defect behind the `engine-audio` and `engine-shell` rows
reported from the other triage lane - `classify_cue` and `SfxScheduler::enqueue`
reached from `handle_redraw`, and `advance_ocean_animation` reached from
`redraw.rs`. All three chains terminate at `PlayWindowApp::window_event`.

**Fixed** by adding the methods of an `impl ApplicationHandler for T` block to
the root set, which resolves those rows and the `FALSE INERT` rows below in one
change. `EXTERNAL_DISPATCH_TRAITS` names the traits treated this way; the root
table in [`port-catalog.md`](port-catalog.md#roots) carries the family and why
it is deliberately over-permissive.

### Type anchors need an `impl` block in the same file

`compute_live` marks a `type` anchor live only when some method in an
`impl <TypeName>` block **in the same file** is reachable. A tag sitting on a
plain data struct whose behaviour lives in free functions, or in an `impl` of a
*different* type, can never be live.

Three anchors below hit this: `MapObject` and `ClutCellFx` have no `impl` block
at all, and `OptionsPhase` is a phase enum whose state machine is
`OptionsSession::tick`.

**Fixed** by falling back to the file's module scope when the file gives the
tagged type no `impl` block at all. A type that *does* have one keeps the
precise rule, so the fallback only reaches the anchors the precise rule could
never have settled.

The fallback has one residual: an `impl` block that declares **no method**.
`ActorExit` in `world_map_panel_actors.rs` is the case. It carries an `impl`,
so the precise rule applies; that `impl` holds one associated `const` and no
`fn`, and `type_scope` is built from functions, so the precise rule has nothing
to resolve. The fallback does not rescue it, by design - the file gives the
type an `impl`. The anchor is inert by construction, whatever the port is
wired to.

Widening the fallback again would be the wrong fix. A `type` anchor claims the
*behaviour* at that address lives on the type, so a method-less `impl` under a
`PORT:` tag is the analysis correctly reporting that the behaviour is
somewhere else. The resolution is to put it back: `ActorExit::apply` performs
retail's four terminal-arm stores and `PanelActorHost::retire` calls it, where
the host had open-coded three of them and dropped the fourth. Read a
method-less type anchor as a question about the port, not as a false negative.

### The module-disclosure regex misses the markdown-heading form

`MODULE_NOT_WIRED_RE` was `^\s*//!\s*\**\s*NOT\s+WIRED`. It accepted
`//! NOT WIRED:` and `//! **NOT WIRED**` but not `//! # NOT WIRED`, because `#`
is not in the `\**` class. `crates/engine-vm/src/scus_core_helpers.rs` carries a
thorough module-level disclosure under exactly that heading, and its four
function anchors were therefore reported as undisclosed gaps.

The sibling `NOT_WIRED_RE` (no anchoring) does match it, which is why that
file's *module* anchors landed in the audit's first section while its *function*
anchors landed in the undisclosed section - a split that is itself the tell.

**Fixed** by widening the leading run to `[#*\s]*`. The four
`scus_core_helpers.rs` functions move to *disclosed inert*, where the module doc
already put them in prose, and need no per-function line. One knock-on: `new` in
the same file is analysed live, so the widened disclosure now shows it in the
audit's first section - a granularity row, not a wrong tag.

A related near-miss, left alone: `// PARTLY WIRED:` (used on `select_owned_rod`)
matches neither regex. It is moot for that anchor, which the corrected audit
resolves live, but a second use of the spelling would go unrecognised.

### An import alias erases every `Alias::assoc_fn` edge under it

`build_rust_graph` resolves a `Qual::name` call site by the qualifier **as
written**: `by_qual[(qual, name)]`, then a module named `qual`, then free
functions of that name - and never methods. So a call written through a
`use ... as` alias looks for an `impl <alias>` that nothing declares, finds no
free function either (the target is a method), and contributes **no edge at
all**.

`crates/engine-core/src/dev_menu_host.rs` is the live case. It imports the
ported row model as `DevMenuRow as RetailRow`, because the host declares its
own `DevMenuRow` for the row subset the engine keeps state for, and then calls
`RetailRow::from_index(..)`. The `.is_closed(..)` sibling three lines away
resolves fine - a method call is matched on the method name, with no qualifier
to mis-resolve - which is why one of the two halves of the same row model read
as wired and the other did not.

The fix is a `use` of the real type name scoped to the function body, which
shadows the outer one for exactly that call. The general rule: **an aliased
qualifier is invisible to the reachability pass.** This is the `Qual::name`
counterpart of the free-function name collision in
[`stale-not-wired-triage.md`](stale-not-wired-triage.md), and it fails the
opposite way - a collision manufactures edges, an alias erases them. An alias
introduced to dodge a name collision silently converts every associated-function
call under it into a non-edge, so it costs a real edge to buy a fake one.

## The world-map dev-menu row model and the panel exit

Six anchors across two files, all of them cases where the port and its host
both existed and the *last* call was missing.

| addr | symbol | verdict |
|---|---|---|
| `801ead98` | `DevMenuRow`, `from_index`, `is_closed` | `WIRE` |
| `801ed308` / `801ed590` / `801ee5d4` | `ActorExit` | `WIRE` |

**The row model.** `DevMenuSession::row_is_closed` had no non-test caller: the
row list built each row's label with `DevMenuRow::label()` and never asked the
gate, so the ported `CLOSED` decision was hosted and then not consulted. The
label selection now runs through `DevMenuSession::row_label`, which is what
retail's own list body does - the two gated arms of `FUN_801EAD98` pick the
*string pointer* on `_DAT_8007B868` rather than drawing a label and deciding
afterwards. Chain: `PlayWindowApp::handle_redraw` -> `tick_dev_menu` ->
`build_dev_menu_draws` -> `row_label` -> `row_is_closed` -> `retail_row` ->
`from_index` / `is_closed`. The alias defect above is why `from_index` needed
one further change after that.

**The panel exit.** Three of the five addresses the `ActorExit` tag names are
anchored to it (`PORT_ADDR_RE` reads the addresses on the tag's own line, and
the tag wraps). The type is the method-less-`impl` case above; the wire is
`ActorExit::apply`, called by `PanelActorHost::retire`, reached from
`PanelActorHost::tick` -> `World::tick_world_map_panels` ->
`World::tick_world_map` -> `World::tick`.

Two things fell out of writing it. The host wrote the `scene[+0x2E]` sentinel
into its `scene[+0x3E]` mirror, which the disassembly separates cleanly -
`FUN_801ED308`'s exit arms store `-1` to `+0x2E` (`0x801ED53C`) and only its
`case 5` zeroes `+0x3E` (`0x801ED52C`) - and it dropped the `ctx[+0x50] =
next_handler` store entirely, recording the id in the frame instead. Both are
now retail's.

And the disclosure those anchors inherited from their module - "the id
dispatcher `FUN_801F159C` is not ported" - no longer holds:
[`baka_hub_actors::hub_dispatch`](../subsystems/world-map.md#the-panel-actor-state-machines)
ports it. What is still missing is the *table*, not the dispatcher:
`hub_dispatch` takes `PTR_FUN_801F33B4[state]` as a caller-supplied closure and
only seven of the 52 slots are read out. The sub-list, text-box and flag-window
exits hand back to `0x1A`, which is one of the seven; the fade/flash exits pick
`0x29` and `0x2B`, which are not. A disclosure that names a dispatcher when the
blocker is a table is the same error this page records for the panel painters.

## `engine-core` anchors

| addr | symbol | site | verdict |
|---|---|---|---|
| `8001d7f8` | `sync_scene_name` | `crates/engine-core/src/scene_name_sync.rs:73` | DISCLOSE |
| `8001e54c` | `install_chunks` | `crates/engine-core/src/chunk_install.rs:86` | DISCLOSE |
| `80021b04` | `from_model_sel` | `crates/engine-core/src/summon.rs:236` | FALSE INERT |
| `80024e80` | `spawn_fade` | `crates/engine-core/src/fade.rs:161` | DISCLOSE |
| `80026018` | `minigame_return_warp` | `crates/engine-core/src/world/frame_tick.rs:910` | WIRE |
| `80038050` | `confirm_menu` | `crates/engine-core/src/dialog.rs:409` | FALSE INERT |
| `8003a55c` | `MapObject` | `crates/engine-core/src/field_regions.rs:270` | FALSE INERT |
| `8003ebe4` | `(module)` | `crates/engine-core/src/overlay_loader.rs:3` | DISCLOSE |
| `8003ebe4` | `load_overlay_a` | `crates/engine-core/src/overlay_loader.rs:180` | DISCLOSE |
| `8003ec70` | `(module)` | `crates/engine-core/src/overlay_loader.rs:3` | DISCLOSE |
| `8003ec70` | `load_overlay_b` | `crates/engine-core/src/overlay_loader.rs:212` | DISCLOSE |
| `800520f0` | `battle_stage_overlay_entry` | `crates/engine-core/src/overlay_loader.rs:135` | DISCLOSE |
| `801cea3c` | `fmv_post_play_handoff` | `crates/engine-core/src/cutscene.rs:205` | WIRE |
| `801cf0d8` | `build_strip` | `crates/engine-core/src/slot_machine.rs:172` | WIRE |
| `801cf0d8` | `cash_out` | `crates/engine-core/src/slot_machine.rs:973` | FALSE INERT |
| `801cfc40` | `field_actor_dir_blocked` | `crates/engine-core/src/world/field_movement.rs:676` | WIRE |
| `801d06c8` | `buy` | `crates/engine-core/src/fishing.rs:656` | FALSE INERT |
| `801d0748` | `score_percent` | `crates/engine-core/src/muscle_dome.rs:193` | FALSE INERT |
| `801d092c` | `max_qty` | `crates/engine-core/src/fishing.rs:627` | FALSE INERT |
| `801d0b90` | `tick_walk_regen` | `crates/engine-core/src/walk_regen.rs:86` | WIRE |
| `801d0c3c` | `first_visible` | `crates/engine-core/src/fishing.rs:602` | FALSE INERT |
| `801d4040` | `symbol_pad_bit` | `crates/engine-core/src/dance.rs:219` | DELETE |
| `801d6f90` | `is_available` | `crates/engine-core/src/fishing.rs:614` | FALSE INERT |
| `801d712c` | `select_owned_rod` | `crates/engine-core/src/fishing.rs:705` | FALSE INERT |
| `801d8258` | `arm` | `crates/engine-core/src/world_map.rs:78` | DISCLOSE |
| `801da9f8` | `OptionsPhase` | `crates/engine-core/src/options.rs:406` | FALSE INERT |
| `801dd0c0` | `category_check` | `crates/engine-core/src/menu_item_category.rs:118` | DISCLOSE |
| `801e1208` | `classify_card_directory` | `crates/engine-core/src/save_select.rs:218` | DISCLOSE |
| `801e295c` | `advance_battle_mode` | `crates/engine-core/src/world/battle/monster_ai.rs:414` | WIRE |
| `801e3af0` | `card_directory_scan` | `crates/engine-core/src/save_select.rs:398` | DISCLOSE |
| `801e3ba0` | `card_free_blocks` | `crates/engine-core/src/save_select.rs:422` | DISCLOSE |
| `801e4794` | `step_clut_fx` | `crates/engine-core/src/world/effects.rs:923` | FALSE INERT |
| `801e4c58` | `ClutCellFx` | `crates/engine-core/src/world/effects.rs:852` | FALSE INERT |

## `engine-vm` anchors

| addr | symbol | site | verdict |
|---|---|---|---|
| `8001fa68` | `list_append_u16` | `crates/engine-vm/src/scus_core_helpers.rs:307` | DISCLOSE |
| `80020424` | `alloc_list_head` | `crates/engine-vm/src/scus_core_helpers.rs:174` | DISCLOSE |
| `80020454` | `alloc_and_append` | `crates/engine-vm/src/scus_core_helpers.rs:204` | DISCLOSE |
| `800204a4` | `free` | `crates/engine-vm/src/scus_core_helpers.rs:236` | DISCLOSE |
| `80021b04` | `spawn_move_actor` | `crates/engine-vm/src/move_vm/spawn.rs:136` | DISCLOSE |
| `80024e08` | `op4c_n5_sub0_set_actor_model` | `crates/engine-vm/src/field/host.rs:1226` | FALSE INERT |
| `8003c9ac` | `(module)` | `crates/engine-vm/src/motion_pause.rs:3` | DISCLOSE |
| `8003c9ac` | `motion_pause_kick` | `crates/engine-vm/src/motion_pause.rs:77` | DISCLOSE |
| `8003fb10` | `validate_action` | `crates/engine-vm/src/battle_action/validator.rs:178` | WIRE |
| `80046898` | `item_count_gate` | `crates/engine-vm/src/battle_action/validator.rs:160` | WIRE |
| `801d829c` | `build_camera_angle_tween` | `crates/engine-vm/src/battle_camera.rs` | DISCLOSE |
| `801d9d30` | `apply_shake` | `crates/engine-vm/src/battle_camera.rs` | DISCLOSE |
| `801e0088` | `child_billboards` | `crates/engine-vm/src/effect_vm/pool.rs:742` | FALSE INERT |
| `801e0088` | `pass2_brightness` | `crates/engine-vm/src/effect_vm/pool.rs:287` | FALSE INERT |
| `801e36c4` | `exec_centered_bar` | `crates/engine-vm/src/title_prim.rs:407` | DISCLOSE |
| `801e373c` | `init_card_state` | `crates/engine-vm/src/title_prim.rs:307` | DISCLOSE |
| `801e373c` | `exec_card_init` | `crates/engine-vm/src/title_prim.rs:470` | DISCLOSE |
| `801e3ee0` | `exec_centered_text` | `crates/engine-vm/src/title_prim.rs:437` | DISCLOSE |
| `801f0348` | `camera_height_from_size_class` | `crates/engine-vm/src/battle_formulas/round.rs:481` | DELETE |

The four battle-camera rows were unsettleable while that lane held the files;
see [the battle-camera rows](#the-battle-camera-rows) for how they resolved.

## `FALSE INERT` evidence

Grouped by which defect hid the edge. None of these want a source change, and
all of them resolve live against the corrected audit - which is what makes this
section the regression set. A reachability change that flips any row here back
to inert has reintroduced one of the four defects.

**winit dispatch.** Each of these is reached from an `impl ApplicationHandler`
callback in `crates/engine-shell/src/bin/legaia-engine/window/`:

- `confirm_menu` - from `handle_keyboard` in `window/event_handler/keyboard.rs`.
- `cash_out` - from `World::exit_slot_machine`, itself from `handle_keyboard`.
- `buy`, `first_visible` - from `World::fishing_exchange_buy` /
  `World::open_fishing_exchange`, both from `handle_keyboard`.
- `is_available`, `select_owned_rod`, `score_percent` - from `build_hud` in
  `window/hud.rs`, itself from `handle_redraw`.
- `max_qty` - from `PrizeExchange::buy`, wired above.
- `step_clut_fx` - from `apply_world_clut_fx` in `window/field_render.rs`,
  itself from `handle_redraw`.
- `child_billboards` - from `World::active_effect_sprites`, from
  `build_effect_billboards` in `window/event_handler/redraw_passes.rs`.
- `pass2_brightness` - from `child_billboards`, wired above.

**Debug-path host edge.** `from_model_sel` is reached from `handle_keyboard`,
which calls `World::active_field_fx_render_nodes` -> `special_render_nodes` ->
`from_model_sel` behind the field-FX debug keybinding. The edge is real
production code in the shipped binary, so a `NOT WIRED:` tag would be false.
Read it with a caveat, though: the caller consumes only `node.mode` for a log
line, so the routing this port exists for - excluding `SoundEmitter` from the
mesh draw list and sending it to the audio host - still has no consumer. This
was the one row the first two checks got wrong; only the corrected-reachability
re-run caught it.

**Trait default method.** `op4c_n5_sub0_set_actor_model` is the `FieldHost`
default body. The production implementor,
`FieldHostImpl` in `crates/engine-core/src/world/vm_hosts.rs`, does not override
it, so the default body is what runs in the field VM.

**Type-anchor granularity.**

- `MapObject` has no `impl` block; the ported routine is the free function
  `parse_map_objects` in the same file, which the audit resolves live.
- `ClutCellFx` has no `impl` block; its behaviour is `World::step_clut_fx` plus
  the free `read_cell`, both wired through winit as above.
- `OptionsPhase` is a phase enum with no `impl`; the state machine is
  `OptionsSession::tick`, which the audit resolves live through the
  `web-viewer` WASM roots.

### Effect of the `fishing.rs` rewrite

The fishing presentation half now lives in `crates/engine-ui/src/ui_fishing.rs`
and the `engine-core` remainder is the rules half. Every `engine-core`
`fishing.rs` anchor still listed is `FALSE INERT`: the consumer that lane added
is `build_hud`, which sits under the winit callback tree.

`select_owned_rod` additionally already carries a `// PARTLY WIRED:` note
stating precisely this. The audit still does not recognise that spelling; it is
moot here only because the anchor now resolves live on its own.

The `ui_fishing.rs` anchors in the `engine-ui` block of the same audit are the
same situation - `persistent_hud_draws` is called from `window/hud.rs`. That
block is out of scope here, but the sibling lane should not treat those rows as
wiring gaps either.

## `WIRE` rows: the call site that should exist

**`minigame_return_warp`** (`80026018`). **This row's original reasoning was wrong and
is corrected here**, because acting on it as written produces a double credit.

It claimed "the state they touch already exists". It does not: `World::minigame_winnings`
is assigned a non-zero value only in tests, so nothing in production fills it, and the
`exit_slot_machine` path already performs its own coin assignment. Wiring the pair beside
that call therefore credits the bank twice.

What retail actually does, read off the dumps: `FUN_801D239C` (`0x801d2894..0x801d28bc`)
drains each Baka tally into the prize accumulator `_DAT_80084440`, and `FUN_80026018`
(`0x80026050..0x80026078`) adds that accumulator into the **casino coin bank**
`0x800845A4`, clamped at 9,999,999 - not party gold `0x8008459C`. The port pays the tally
into `World::money` instead, which is why the accumulator never fills.

So this is a real `WIRE`, but a **two-part** one that no single lane can land: the Baka
tally must be redirected from party money to a coin accumulator (in
`world/frame_tick.rs`), *and* the warp call sites added. Either half alone is wrong - the
call sites alone credit zero, the redirect alone loses the prize.

**`fmv_post_play_handoff`** (`801cea3c`). Nothing consumes `FmvHandoff`
anywhere. `commands/run.rs` reads `World::active_fmv()`, logs it and skips.
After playback completes it should call the handoff and apply the result. The
`Field` / `ResumeField` arms are cheap - a scene label plus a door word. The
`CardInit` and `ModeZero` arms need target modes the engine does not have, so
those can stay unhandled with a note.

**`build_strip`** (`801cf0d8`). `SlotMachine::new` does not build retail's two
permuted 20-slot strips. Build both at session start - `STRIP_PROBE_PRIMARY`
with base `0`, `STRIP_PROBE_SECONDARY` with `slot_payout::BONUS_VALUE_BASE` -
and feed the display strip from them. Medium: it changes what the reels show,
so the slot-machine tests move with it.

**`field_actor_dir_blocked`** (`801cfc40`). The wall arm
(`World::field_dir_blocked`) is called from the locomotion step in the same
file; the actor arm is called only from tests and from the disc-gated oracle
`crates/engine-shell/tests/field_collision_discriminator.rs`. Add it to the same
per-axis step gate so NPCs block the player. Small, but it changes movement, so
land it with the collision oracle green.

**`tick_walk_regen`** (`801d0b90`). No per-frame caller. It belongs in the field
frame tick, gated on the same step counter retail drains by `0x20` per call.
Small: the party gauges it bumps are already on `World`.

**`advance_battle_mode`** (`801e295c`). The battle-action state machine's
`case 0xFF` should call it. Small - it is a one-line wrapping increment - but it
needs the `0xFF` pseudo-action to be decoded in the action dispatch first.

**`validate_action`** / **`item_count_gate`** (`8003fb10`, `80046898`).
Nothing implements `ActionValidatorHost`. The engine greys battle commands with
ad-hoc per-menu gates (`battle_magic`'s MP check, `battle_input`'s command-row
selectability). Implement the host for `World` and route the command-row and
target-row selectability passes through `validate_action`, keeping the per-slot
validity bitmask - the menu greying reads the mask, not the return value.
Largest of the `WIRE` rows: it replaces existing gates, so it needs the retail
arm semantics preserved case by case. `item_count_gate` follows for free as its
arm-`0x82` callee.

## `DELETE` row

**`symbol_pad_bit`** (`801d4040`, `crates/engine-core/src/dance.rs:219`).
`DanceDir::pad_bit` in the same file has identical arms (`0x80` / `0x20` /
`0x10`), cites the same `FUN_801d4040`, and is the copy the live path uses -
`World`'s dance tick references it from `world/frame_tick.rs`. The free function
adds only the "any other symbol scores 0" fallback for raw chart bytes, which
the chart decoder never produces because it converts symbols to `DanceDir`
first.

Delete the free function and move the `// PORT: FUN_801d4040` tag onto
`DanceDir::pad_bit`, so the address keeps its anchor.

## `DISCLOSE` texts

Paste as `// NOT WIRED:` above the anchor, or `//! NOT WIRED:` for a module
anchor. Wrap to the file's comment width.

- **`sync_scene_name`** - the engine changes scene by label through the scene
  host, and carries no staged-name / active-buffer / scene-index-word triple for
  this bridge to resolve between. Wiring it needs a name-based scene-change
  packet path, which the dialog port routes around.
- **`install_chunks`** - the engine resolves scene sub-assets through the typed
  `legaia_asset` dispatcher and uploads VRAM and VAB directly from those.
  Nothing produces retail's `[type, size, data]` side-band chunk list, so the
  walker has no stream to walk.
- **`spawn_fade`** - the engine's fades are host-driven state, not entries in a
  fixed-capacity system-actor pool. The `slot_free` argument models a pool
  allocation outcome that no engine caller can supply an answer for.
- **`load_overlay_a` / `load_overlay_b` / the module** - the host trait is
  already implemented (`OverlayLoaderHost for ProtCdDmaHost` in
  `crates/engine-core/src/cd_dma.rs`); what is missing is the caller. The engine
  has no mode-table overlay-residency model - it resolves PROT entries on demand
  and keeps no `gp+0x924` / `gp+0x934` cache pair - so no dispatcher exists to
  route a paired parallel load through.
- **`battle_stage_overlay_entry`** - the engine carries no per-formation stage
  id, so nothing produces the `_DAT_8007B64A` value this maps. The one battle
  that pages a stage overlay is primed by the host instead, through
  `World::prime_battle_tutorial`.
- **`arm` (`EmitterGate`)** - the arming wrapper's retail caller sources its
  parameters from the world-map trigger globals, which the engine's world-map
  controller does not implement. Its consumer `emit_horizon` is correspondingly
  gated off, which is why that sibling's own tag is right despite being called
  every frame.
- **`category_check`** - the item-category favor score drives retail's
  per-character item-menu ordering and greying. The engine's item menu has no
  favor pass, so there is no ordering for the score to affect.
- **`classify_card_directory` / `card_directory_scan` / `card_free_blocks`** -
  the reason recorded here (no runtime card-image backend) **no longer
  holds** and the source no longer says it. The browser card rack
  (`web-viewer::cards`) mounts raw card images and runs the scan/budget
  pair. What blocks the classifier is an **index-space mismatch**: the rack's
  grid is keyed by physical block and the classifier keys by the filename's
  save index, and nothing makes the two agree on a player's card. Adopting
  it means re-keying the grid. Read the tags in `save_select.rs`, not this
  bullet, for the current form - it is kept only to record that the earlier
  reason was outgrown rather than wrong at the time.
- **`list_append_u16` / `alloc_list_head` / `alloc_and_append` / `free`** - the
  module doc already carries the full reason under its `# NOT WIRED` heading;
  the audit compares per anchor, so each function needs its own line. Short
  form: the engine's actor storage is a generational `Vec` pool, not a retail
  free-stack, and `list_append_u16`'s retail caller `FUN_8003F3FC` is not
  ported.
- **`spawn_move_actor`** - the host side is ready (`impl MoveSpawnHost for
  World` in `crates/engine-core/src/actor_alloc_host.rs`), but nothing in the
  engine spawns move-VM actors: the field and battle paths construct actors
  through the world's own pool, so only tests drive the retail spawn.
- **`motion_pause_kick` and the module** - the port's field collision path does
  not post touch events, so the retail caller `FUN_801D5B5C` has no engine
  analogue to tail-call this from. Same root cause as the existing disclosure on
  `motion_vm.rs`'s `post_touch`.
- **`exec_centered_bar` / `exec_centered_text` / `exec_card_init` /
  `init_card_state`** - the engine's title and save screens are drawn by
  `engine-ui`'s `ui_title_save` draw-list builders, not by replaying the retail
  overlay's primitive descriptors, so no host supplies a `PrimHost`. The same
  reason covers `exec_clear_image` / `exec_move_image` /
  `exec_sprite_descriptor` in that file, which is where its three SCUS-helper
  addresses are tagged now that the module tag has been split onto them.

## The battle-camera rows

These carried a `VERIFY` verdict while the battle-camera lane held
`crates/engine-vm/src/battle_camera.rs` and
`crates/engine-vm/src/battle_formulas/round.rs`. That lane has landed. All are
still inert against the corrected audit - every caller is `#[cfg(test)]` in
the same file or in `battle_formulas/tests.rs`, and the host-crate sweep returns
zero, the same sweep that finds `battle_render_mesh`'s two real host call sites.
What the lane landed touched neither symbol, so each now settles on its own
reason.

`battle_camera.rs` no longer carries module-level `PORT:` lines: both of its
addresses were tagged twice, once on the file and once on the function that
implements it, and the file-wide anchor said nothing the per-function one did
not.

**`camera_height_from_size_class`** (`801f0348`) is `DELETE`, on the
`symbol_pad_bit` precedent. Its sibling `camera_height_for_frame` in the same
file is the whole of `FUN_801F0348`, is wired through
`BattleActionHost::camera_bounds`, carries its own `PORT: FUN_801f0348` tag, and
inlines the `<< 7` + clamp rather than calling the helper. Deleting the helper
loses no coverage and costs the address no anchor.

**`build_camera_angle_tween`** (`801d829c`) is `DISCLOSE`. The builder emits a
9-record `{step_count, endpoint}` step table that retail's per-frame walker then
advances; the engine frames the battle camera by a per-action snap at action
seed and has no per-frame angle walker, and the routine that arms retail's
(`FUN_80021248`) is documented but unported. Nothing exists to consume a step
table.

**`apply_shake`** (`801d9d30`) is `DISCLOSE`. It previously read `WIRE` on
`BattleActionHost::screen_shake` as the half-built call site; that verdict is
**withdrawn**, and the reason is the worked example of why a verdict on this
page is a hypothesis. The host method's name is a misnomer. The SM arm it
mirrors is `overlay_battle_action_801e295c.txt` `0x801E4938..0x801E497C`: it
tests the camera pitch `DAT_8007B790` against `0x191` and, when it is at or
above, zeroes the pitch and stores the **absolute** value `0x500` into
`_DAT_800840BC`. That is a framing snap to the close-up pose - `0x500` is the
1280 the close-up framings hold - written into one component of the camera
translation trio.

`apply_shake`'s `amplitude` is a different quantity: a `1..=0x15` shift count
read from `_DAT_8007B630`, whose only retail writer is a field-VM opcode
(`overlay_0897_801de840.txt` `0x801E2134`, a 3-byte instruction whose operand
byte becomes the global). Routing a translation value into it is a category
error, so that arm cannot be the missing caller. The port's field VM does not
model `_DAT_8007B630`, which leaves the amplitude a permanent zero - the value
at which the routine degenerates to backing its own previous offset out of the
accumulators. Wiring it means modelling that opcode first.

`round.rs` already carries `NOT WIRED` disclosures on two neighbouring
functions, so the house style for that file is established either way.

## Known false positives the correction introduces

Both are the accepted over-approximation direction, and both are named here
because a reader looking for the row will otherwise not find it.

**`arm`** (`801d8258`, `crates/engine-core/src/world_map.rs:78`) keeps the
`DISCLOSE` verdict above but no longer appears in the audit at all. Making the
winit tree reachable made `route_camera_events` in `engine-core/src/camera.rs`
reachable, and its `.arm(` call on a `CameraMover` resolves by name to
`EmitterGate::arm` as well, because receiver types are not inferred. That is
audit cause 2 - a method-name collision - and it hides a genuine gap. The
verdict stands on the hand evidence, not on the tool.

**`new`** (`crates/engine-vm/src/scus_core_helpers.rs:135`) was tagged
`NOT WIRED` by the widened module disclosure yet analysed live, and surfaced in
the audit's *first* section for a while. `new` is the most collision-prone name
in the workspace, so it was always audit cause 4 - anchor granularity - and not
a wired port; the receiver gate has since cleared it, and
[`stale-not-wired-triage.md`](stale-not-wired-triage.md#how-the-recorded-rows-were-closed)
records the collision it resolved through.

## The menu / save / memory-card cluster

Forty-eight inert anchors across `card_bu_io.rs`, `card_flow.rs`,
`save_select.rs`, `save_subscreen.rs`, `pause_screens.rs`,
`menu_open_sequence.rs`, `menu_list_rows.rs`, `spell_menu.rs`,
`spell_party_broadcast.rs`, `target_picker.rs`, `equipment.rs`,
`panel_backread_loader.rs`, `menu_actor_seed.rs` and `title_prim.rs`. None is
`FALSE INERT` (a symbol-by-symbol sweep of the host crates returns zero
non-doc references for all forty-eight). Exactly one took a `WIRE`; the rest
settle `DISCLOSE`, and that outcome is worth stating rather than leaving as
an absence, because one of them looks wireable and is not:

- **`spell_targets_group`** would route a group spell past the target picker,
  and the applier on the other side heals exactly one roster member - so
  wiring it alone makes group spells heal nobody.

**`root_menu_confirm_route` is the `WIRE`.** The first read of it stopped at
"returns a retail sub-screen id the engine has no space for, so a caller
keeps only the buzz/advance bit and drops the payload" - and dropping a
payload *is* the usual tell. It was the wrong tell here: the seven ids are
distinct, so a caller can resolve the confirmed row **through** the id rather
than beside it, which is what `FieldMenuSession` now does. The gate inputs
turned out to be the real question, and only one of the two was missing
anything (below).

The card cluster is the `world_map_panel_actors` shape again: ten anchors
whose per-anchor lines were verbatim identical. They are one gap - an
asynchronous card backend behind `save_select::CardIoMachine` - and the
module now says so once, with the per-anchor lines citing it.

### What the cluster's re-read changed

Reading these disclosures against the disassembly overturned a claim that had
propagated into two subsystem docs and two source files: the pause root's
gated rows were labelled Save-then-Load, and they are **Load-then-Save**. The
menu overlay's own rodata pool settles it - `FUN_801CFD68` hands the string
primitive `0x801CEA00` for row 5 and `0x801CEA08` for row 6, and those cells
hold `@Load` and `@Save`. Three consequences, all now corrected in
[`save-screen.md`](../subsystems/save-screen.md):

| Was | Is |
|---|---|
| row 6 gated on `_DAT_800846A8` | gated on `_DAT_8007B6A8`, the per-scene save-allow flag |
| `0x18` saves, `0x19` loads | `0x18` loads, `0x19` saves |
| entry-context byte `0x01` is a load | it is a field script's save point |

The gate address was a plain arithmetic slip (`lui 0x8008` + `lbu -0x4958` is
`0x8007B6A8`; `0x800846A8` is the escape counter). The direction was not - it
was a reading of the op selector that nothing had cross-checked against the
labels, and the labels are the cheaper evidence.

One row's disclosure got *better* out of this rather than merely different.
The Save gate is the MAN header bit
[`ManHeader::low_flag`](../formats/encounter.md), which `legaia-asset` already
parses and the engine then drops - so "the engine has no analogue" was wrong
and the real prerequisite is two named edits: carry the flag through scene
load onto the world, then hand it to `FieldMenuSession` at open. Both are
made, so the anchor is live; see
[`field-menu.md`](../subsystems/field-menu.md#top-level-pause-menu).

The two gate inputs did not turn out to be the same kind of gap, which is
the transferable part. The save flag was a **carry** gap - the datum existed,
parsed, and simply had no route to its consumer, so the fix was plumbing and
the gate now fires on real scenes. The entry-context kind is a **model** gap:
retail keeps one global pointer whose first byte is the armed op-`0x49`
sub-op, and the port deliberately replaced that global with a per-context
tagged park, so no single place holds the byte to read. Its consumer is live
and honest (it reads what the world can answer) but cannot reach the blocking
kind until the op-`0x49` arm records its sub-op. A disclosure that lumps the
two together as "both inputs are missing" reads as one piece of work and is
two.

### A latent duplicate-free-function-name landmine, defused

`engine-core`'s `menu_list_rows::description_source` and `engine-ui`'s
`ui_menu_window_painters::description_source` were two free functions of one
name over different id spaces. A free-function edge is never receiver-gated,
so the painter's first *non-test* call would have made the `engine-core`
kernel read live and converted its correct disclosure into a false accusation
in the audit's first section. Nothing had fired yet because the painter's
copy is called only from its own tests. The `engine-core` copy is now
`row_description_source`, per the recipe in
[`stale-not-wired-triage.md`](stale-not-wired-triage.md#the-fix-each-mechanism-takes).

Worth generalising: a name collision is a defect *before* it produces a row,
and the cheapest time to find one is while reading a disclosure, not after
the audit accuses it.

## Not on this page: the world-map panel cluster

The largest single block of *disclosed* inert `engine-vm` anchors was the
world-map band's panel screen - `world_map_panel_actors.rs`,
`world_map_overlay.rs`, `world_map_panel.rs` and `travel_art_actor.rs`. It never
appeared here, because this page triages the **undisclosed** section and every
one of those anchors carried an honest `NOT WIRED:` line naming the same
missing thing: a panel-window host on `WorldMapController`.

That is the shape a disclosure worklist hides. Each tag was individually
correct, and re-reading them produces no work; what closes the block is
building the one host they all name, which is now
`legaia_engine_core::world_map_panel_host` (see
[`world-map.md`](../subsystems/world-map.md#the-panel-actor-state-machines)).
When a disclosure reason repeats verbatim across a dozen anchors, read the
repetition as the worklist item.

## The op-`0x49` submode actor family (`baka_hub_actors`)

These rows sit in the audit's *disclosed* section rather than the undisclosed
one, and they are recorded here because their shared disclosure was wrong about
what blocked them - a shape this page exists to catch.

The tag read "the engine has no field system-actor pool, so no code path
produces an actor with a `+0x50` handler id". The engine **does** have that
pool. `World::man_load_actor_reset` spawns an `ActorHandler::SubmodeDriver`
actor on every MAN load (retail's `FUN_801D9C3C` at `0x8003B444`) and
`HandlerKernel` classed it `Unported`, so the actor sat in the pool with
nothing to run. The blocker was a missing *dispatch arm*, one file away, not a
missing subsystem.

| addr | symbol | verdict |
|---|---|---|
| `801f159c` | `hub_dispatch` | `WIRE` - `World::tick_handler_actors` -> `World::tick_submode_screen` |
| `801f0adc` | `coin_exchange` | `WIRE` - handler slot `0x25`, opened by `World::open_coin_counter` |
| `801f1138` / `801f1e48` / `801f1fdc` / `801f1d90` / `801f20b0` / `801f2134` | the state machines | `WIRE` - slots `0x27` / `0x32` / `0x28` / `0x13` / `0x1a` / `0x00` |
| `801f16c0` / `801f17d8` / `801f1890` / `801f1950` / `801f1a1c` / `801f1ab0` / `801f1b64` | the panel painters | `WIRE` - panel-window records, not handler slots |
| `801f90dc` | `acquisition_caption` | `DISCLOSE` - see below |

The painters are the row worth reading twice. They are **not**
`PTR_FUN_801F33B4` slots; they are the `+0x14` callback of a `0x801F2C0C`
panel-window record. A disclosure that names the wrong table names the wrong
blocker.

`acquisition_caption` keeps its disclosure on new evidence rather than on the
old reason: `0x801F90DC` has no reference anywhere in the field overlay's bytes
- neither table holds it - and it sits in the resident slot-B band whose widget
descriptors `engine-core::screen_fx` pins at `0x801F8FE4..0x801F902C`. What has
to exist first is a base-confirmed dump of the image that really owns that VA.

## The dance / fishing minigame block

The `dance.rs` / `dance_tutorial.rs` / `fishing_actors.rs` / `fishing_chrome.rs`
cluster is the largest single-subsystem group in the *disclosed* inert list, and
its size invites the assumption that it is `FALSE INERT` because both minigames
are playable. It is not: the playable halves are `dance::DanceGame` and
`fishing::PondSession`, and those *are* live. What the cluster holds is the
presentation and actor half of the same two overlays, which the port does not
have hosts for. Four rows did turn out to be resolvable, and they are the shape
worth looking for in the rest.

| Row | Verdict | What settled it |
|---|---|---|
| `roll_hit_type` (`801d26cc`, `fishing_actors.rs`) | `DELETE` | Duplicate of the live `fishing::band_roll`; the wrapper now delegates, so one kernel has one implementation. |
| `bite_interval` (`801d26cc`) | `WIRE` | `BandCheck::tick` was approximating the strike modulus with the length readout; the ladder is the real one. |
| `bite_interval_bias` (`801d26cc`) | `WIRE` | Same call site, after correcting the kernel - see below. |
| `clear_catch_slots` (`801d746c`, `fishing_chrome.rs`) | `DELETE` | Same table as `fishing::ReelCadence`'s ring; `reset` now calls it. |
| `dance_scene_stage` (`801d414c`, `dance.rs`) | `WIRE` (partial) | Its `clear_pad_latch` field has an engine equivalent; `World::enter_dance` / `exit_dance` apply it. |

**`bite_interval_bias` was wrong, not just unwired.** It modelled retail's
`li s1, -0x64` as a bias added to the strike credit. The instruction is an
assignment into the register that already holds the credit base, so the far band
*replaces* the base rather than offsetting it. The kernel is now
`bite_credit_override`, returning `Option<i32>`. This is the failure direction
the page's own preamble warns about: an unwired kernel's arithmetic is never
exercised, so a misreading survives until something calls it.

The remaining rows are genuine gaps with sharp prerequisites, and they group
into four:

- **No line primitive.** `clip_segment_2d` and `project_segment` clip
  two-point draws; neither `engine-ui`'s draw list nor `engine-render`'s VRAM
  pipeline has a line kind.
- **No minigame effect-part pool.** `step_mark_effect_spawn`,
  `good_banner_spawn`, `splash_burst`, `ripple_spawn`,
  `dance_hit_sting_voices`.
- **No dancer / fish actor records.** `dancer_emit`, `dancer_fade_weight`,
  `dance_clip_driver_gate`, `dance_face_rig`, `roll_wander_target`,
  `step_facing`, `fish_camera`, `float_actor_tick`.
- **No retail-coordinate HUD surface.** `hud_draws`, `dance_hud_draws`,
  `dance_score_box_slots`, `dance_hud_widget_quad`, the three digit-glyph
  selectors, `centred_panel`. Both hosts lay their dance and fishing readouts
  out at their own pens rather than in 320x240 framebuffer coordinates.

Two of those have a *named* host outside `engine-core`: the browser page
already loads the dance floor's dancer meshes and the fishing venue's
walk heightfield, so `dance_face_rig` and `walk_grid_overhead` want a call in
`crates/web-viewer/`, not a new subsystem. `bite_pad_nudge` is a third: it
wants `PondInput` to carry the retail pressed-pad word instead of a
pre-counted `edge_bonus`, which is a signature change its browser caller has
to move with.

## See also

- [`port-catalog.md`](port-catalog.md) - the catalog, the `live` axis and the
  audit that produces the input to this page.
- [`worklist-classification.md`](worklist-classification.md) - the sibling
  classification for the `--missing-ports` worklist.
