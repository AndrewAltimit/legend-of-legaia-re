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

### Which crate an anchor sits in decides whether it can be wired at all

`engine-core` has no `[[bin]]` and no `#[wasm_bindgen]` entry point, and
neither does `engine-vm`, `engine-ui` or `legaia-asset`. Reachability is
measured from *host* roots - `engine-shell`, `web-viewer`, `asset-viewer` - so
an anchor in a library crate can only become live through an edit in a crate
that has a root. There is no arrangement of library-side code that makes one
reachable.

Two consequences worth carrying into how work is scoped. A pass scoped to
library files can produce `CORRECT` and `DELETE` verdicts but structurally
cannot produce a `WIRE`; if it reports one, check what it actually edited.
And a wiring effort should be partitioned by the **host** whose flow is
missing rather than by the crate the port happens to live in - the same
partition-by-call-site rule the feature views use, applied one level up.

The exception is a port whose natural home *is* a rooted crate. The SC block
checksum below is one: it was reproduced in `legaia-save`, which `save-tool`
roots, so it wired the moment it existed.

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

**The corollary, which bites when a module later gets partly wired.** A
module-level marker declares *every* port site in the file inert - that is the
point of it, and it is right for a wholly-inert module such as `mdec::st_ring`,
which discloses once and then tags seven addresses inside function bodies. It is
wrong the moment the module becomes **mixed**. Wiring the player ledge hop made
three kernels in `field_ledge_hop_arc.rs` live while two spawners stayed inert;
the module heading kept claiming the whole file, so the audit reported the three
live kernels as stale-tagged - a clean measurement regression caused by correct
work, with nothing wrong in the tool.

The fix belongs in the source, not the regex: a mixed module must not carry the
module-level marker at all. Give the inert members their own per-function tags
and retitle the section so it documents wiring status without asserting a
blanket disclosure. Widening the regex would not help, and narrowing it would
break the wholly-inert case it exists for.

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
| `801d0748` | `hp_left` / `turns_left` | `crates/engine-core/src/muscle_dome.rs` | FALSE INERT |
| `801d092c` | `max_qty` | `crates/engine-core/src/fishing.rs:627` | FALSE INERT |
| `801d0b90` | `tick_walk_regen` | `crates/engine-core/src/walk_regen.rs:86` | WIRE |
| `801d0c3c` | `first_visible` | `crates/engine-core/src/fishing.rs:602` | FALSE INERT |
| `801d4040` | `symbol_pad_bit` | `crates/engine-core/src/dance.rs:219` | DELETE |
| `801d6f90` | `is_available` | `crates/engine-core/src/fishing.rs:614` | FALSE INERT |
| `801d712c` | `select_owned_rod` | `crates/engine-core/src/fishing.rs:705` | FALSE INERT |
| `801d8258` | `arm` | `crates/engine-core/src/world_map.rs:78` | DISCLOSE |
| `801da9f8` | `OptionsPhase` | `crates/engine-core/src/options.rs:406` | FALSE INERT |
| `801dd0c0` | `category_check` | `crates/engine-core/src/menu_item_category.rs` | WIRED |
| `801e1208` | `classify_card_directory` | `crates/engine-core/src/save_select.rs` | WIRE |
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
| `801d829c` | `build_camera_angle_tween` | `crates/engine-vm/src/battle_camera.rs` | WIRE |
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

## The two rows the audit's undisclosed section was last down to

Both closed, and they closed in opposite directions - which is the point of
keeping them together. Neither could be settled from the audit row itself.

**`timed_fight_turns_left`** (`801d0748`,
`crates/engine-core/src/muscle_dome.rs`) is `DISCLOSE`, and the reason is a
*deliberate* non-read rather than a missing host. The strip it feeds is Koru's
timed fight, gated on the formation cell holding
`TIMED_FIGHT_MONSTER_ID`; a dome round is an ordinary battle that ends on a
knockout, so `MuscleDomeSession` must not consult a turn limit. The row is the
counter-example to reading an undisclosed inert port as work: the wire would be
a bug. Its prerequisite is a host that draws that one fight's `Turns Left /
HP Left` strip, which needs the formation-cell gate the engine does not carry.

**`tile_for_slot`** (`801e1934`, `crates/asset/src/save_icon.rs`) is `WIRE`,
and the audit is not scoped to say so - it sits in `legaia-asset`, outside this
page's two crates, and reached it only because the browser card rack was
open-coding the mapping it ports. `Runtime::save_block_icon` in
`web-viewer::cards` wrote `slot as usize` where retail's VRAM x is
`0x3C0 + slot * 4` halfwords. The map is the identity, which is exactly why it
had been open-coded and why the port read inert; routing the call through it
costs nothing and makes the sheet's slot arithmetic have one owner. **An
identity-valued port is the easiest kind to leave unwired and the cheapest kind
to wire.**

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
- `is_available`, `select_owned_rod`, `hp_left` / `turns_left` - from
  `build_hud` in `window/hud.rs`, itself from `handle_redraw`.
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

**Every row in this section is landed.** The section stays because each row
records the analysis that shaped its wire - what the call site had to be and
why the obvious placement was wrong. Where each one lives now:

- `minigame_return_warp` - both halves of the two-part wire exist in
  `crates/engine-core/src/world/frame_tick.rs`: the Baka tally drains into
  `World::minigame_winnings` and the warp pair (`arm_minigame_warp` /
  `minigame_return_warp`) banks it into the casino coin bank on the
  `enter_baka_fighter` / `exit_baka_fighter` path.
- `fmv_post_play_handoff` - consumed by `apply_fmv_handoff` in
  `crates/engine-shell/src/bin/legaia-engine/commands/run.rs`, with the
  `CardInit` / `ModeZero` arms disclosed as modes the engine does not have.
- `build_strip` - `build_reel` in `crates/engine-core/src/slot_machine.rs`
  builds both permuted 20-slot strips per reel in retail's interleaved draw
  order; `SlotMachine::new` builds all three reels and seeds the display
  strip from the symbol half.
- `field_actor_dir_blocked` - the actor arm sits in the per-axis step gate in
  `crates/engine-core/src/world/field_movement.rs`, covered by the disc-gated
  collision oracle.
- `tick_walk_regen` - `World::tick_field_walk_regen` runs it from the field
  frame tick in `frame_tick.rs`, gated on the retail `0x20` step cost.
- `advance_battle_mode` - called from the battle loop driver
  (`crates/engine-core/src/world/battle/loop_driver.rs`).
- `validate_action` / `item_count_gate` - `WorldActionValidator` in
  `crates/engine-core/src/world/battle/validator_host.rs` implements
  `ActionValidatorHost`; `World::action_validity_mask` accumulates the
  per-slot validity byte and the target pickers read it through
  `battle_target_rows` in `command_flow.rs`.

The `DELETE` row below is likewise applied: the free `symbol_pad_bit` is gone
and the `// PORT: FUN_801d4040` tag sits on `DanceDir::pad_bit` in `dance.rs`.

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
- **`card_directory_scan` / `card_free_blocks`** - the reason recorded here
  (no runtime card-image backend) **no longer holds** and the source no
  longer says it. The browser card rack (`web-viewer::cards`) mounts raw
  card images and runs the scan/budget pair. Its sibling
  `classify_card_directory` is now wired too, for the reason
  [below](#the-index-space-mismatch-was-the-wire-not-the-blocker) - which
  is the second time this bullet's reason has been outgrown rather than
  found wrong. Read the tags in `save_select.rs`, not this bullet.
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

**`build_camera_angle_tween`** (`801d829c`) was recorded here twice, wrongly
both times, and is now **`WIRE`, landed**. The first reason read "the engine has
no per-frame angle walker"; the engine has one, `engine-shell`'s
`window/battle_cam.rs` `BattleCamera` / `Glide`. The second said adopting the
table would change that walker's arithmetic, because the builder emits step
*counts* where the walker uses rates.

It does not emit step counts. The arming routine `FUN_80021248` signs each
record's first halfword by comparing the endpoint against the live global
(`0x80021378`, `0x800213D8`), which is only meaningful for an **increment** - so
the builder's fourth argument is the tween's duration and its output is the
per-frame increment. That makes retail's law arrive-together with one shared
duration, which is exactly what `Glide::linear` was recomputing. It now builds
its rate table from the port, and takes the 12-bit shortest-arc yaw and the TR.z
projection prescale from it as well. What is still absent is only the
*producer*: the walker's endpoints come from the traced phase framings rather
than from retail's arming path.

The lesson generalises: `delta / param` reads as either "how many steps" or
"how far per step", and a kernel dumped on its own cannot tell you which. The
consumer can.

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

## The battle cluster: what a re-read of an already-disclosed block finds

The `engine-vm` battle band (the `battle_action/` leaves, the intro styles,
the camera and cursor kernels, the cast dispatchers) sits in the audit's
*disclosed* section, so it never reached the table above. Re-reading it against
the dumps settles the block as disclosure rather than wiring - and turns up
six disclosures that named the wrong blocker. They are recorded because each
is the failure the preamble warns about: a reason that reads correct, that the
next audit agrees with, and that sends the reader looking for a port that
already exists.

| Anchor | The clause that was wrong | What holds instead |
|---|---|---|
| `801dceac` `target_group_aim` | `bearing_12bit` "is itself unwired for want of the arctan LUT" | It is live on every enemy-cursor step, over `approx_arctan_lut`. Both are wired now - see below. |
| `80046a20` `gauge_colors` | the HUD's bar colour is "a constant of the widget" | It is a per-frame index, from the readout-tint siblings. |
| `801d829c` `build_camera_angle_tween` | "the engine has no walker" | The native battle camera is one - see above. |
| `801f0450` (arts auto-combo) | the caller is the unported flow SM `FUN_801D388C` | The caller is the action SM's own `Begin` arm. |
| `801dba04` / `801db81c` / `801da34c` | "`FUN_801D0748` is not ported" | Its state space is `engine-core::battle_flow`, and it is live. |
| `801e22c8` `expand_cue_group` | neither cue table "is extracted by a parser" | Both are, as `move_power::EffectAuxTables`. The blocker is the caller - see below. |

Three shapes produced all six, and each is worth recognising on sight. The
cue-table row is a fourth, milder one: a reason that quantifies over *both*
inputs when only one is absent. Naming the two separately is what turns it
into a one-region parser extension rather than an open-ended data hunt.

**A kernel with a substitute input reads as blocked by the input it does not
use.** `bearing_12bit` takes its arctan table as a parameter precisely so a
host without the disc table can pass a computed one, and `bearing_12bit_approx`
is that host-facing form. A disclosure written from the retail data dependency
rather than from the port's signature will miss it every time.

**The engine can hold the same retail decision twice, under two addresses.**
`FUN_80046A20` picks a gauge-primitive colour; `FUN_800349EC` picks the
readout-text colour. They share a code space and a threshold shape, so the
presence of one reads as the absence of the whole idea. The surviving gap was
the drawn bar, not the selection - a much narrower thing than the old reason
claimed.

**An address-keyed catalog cannot separate VA-aliased twins.** `801d388c` is
reported ported and live; that row is the Muscle Dome overlay's routine at the
same VA, and the battle flow SM really is unported - so here the *disclosure*
was right and the catalog is the trap. One address up, `801d0748` is the
opposite: the battle command SM genuinely is ported, and three disclosures said
it was not. Read the crate and the file, never the flag, before writing either
sentence.

### The one accessor, and what it did and did not unblock

The `WIRE` the block wanted was one host accessor:
`BattleActionHost::actor_position(slot) -> Option<(i16, i16)>`, the actor
`+0x34`/`+0x38` seat, implemented on `BattleHostImpl` off the world actors'
move state. It landed, and with it the whole facing block at
`0x801E4334..0x801E43A4` in `magic_cast_begin` - both the single-target arm and
the `target_group_aim` group arm, which `FUN_801E7320`'s class-`7` / class-`8`
target codes reach in production.

Two things surfaced only once the kernel had a caller, which is the standing
argument for wiring over disclosing:

- `FUN_801DCEAC`'s extent output is **floored** at `0x400`, not capped at it
  (`slti` / `beq` at `0x801DD094`). The port had the compare backwards, and no
  existing test could see it because every synthetic group was narrower than
  the bound.
- the group walk's monster liveness gate reads the actor's `+0x4` prim word,
  which the port leaves at its default - so a faithful transcription would have
  produced a kernel that always answered `None`. The summon-fade sweep at
  `0x801E4B50` zeroes `+0x4` and writes `+0x21C = 0xFF` in the next two
  instructions, so the port reads the `+0x21C` twin.

The same block turned up one more instance of the shape this page keeps
finding - **the engine holding a retail decision twice**. The action SM's
`Begin` arm seeds its turn cursor from the formation-advantage byte
`ctx[+0x290]` and then latches it, and the port does both; but nothing writes
`BattleActionCtx::formation_advantage` in production, because `engine-core`
rolls, seeds and latches its *own* `World::battle_formation` copy at battle
entry before the SM's first step. So the seed's `0` arm is what runs and its
other two arms are unreachable. Closing that is a one-line mirror at battle
entry plus a decision about which of the two latch sites survives - both in
`crates/engine-core/src/world/`, so it is a sibling lane's edit.

**`approach_distance` was not unblocked**, and the claim here that it would be
is withdrawn. Its blocker is not geometry: it clamps a *requested* step length,
and the port's attack band (`attack_advance` / `attack_short_step`) polls
`range_check` rather than requesting a distance, so no call site holds a value
for it to clamp. `FUN_801DF570` is also not called from the action SM at all -
its caller is `FUN_801DEA50`.

### `expand_cue_group`: the table was not the prerequisite

Corrected on the same principle. The row above says only the group table was
missing; it is parsed now (`EffectAuxTables` reads `0x801F6470` alongside the
two effect tables, and `battle_cue_group_real.rs` composes the pair off the real
overlay). That did **not** produce a caller. Retail's only caller is the
damage-application primitive `FUN_800402F4`, which reaches `jal 0x801e22c8` from
eleven branches and picks the group id per branch - eight literals, two
computed, and exactly one forwarding its own `param_2`. The port models
`FUN_800402F4` as
the `apply_damage` host hook, whose parameters are the primitive's arguments and
not its per-branch choices, so no honest single call site existed until that
dispatch was ported. A disclosure that names a table when the blocker is a
dispatch is the same error this page records for the panel painters.

**Closed by porting the dispatch, not the applier.** The eleven branches differ
only in three literals apiece - tint, actor-state word, group id - plus one
`per_target` loop flag, all readable off the `jal` sites' own instruction
streams. `battle_cue_group::cue_group_for` is that table, and the SM's state
`0x3F` selects a site from the acting actor's `+0x1E8` / `+0x1E9` pair before
handing the expansion to the host's effect pool and SFX scheduler. The applier's
1976 instructions of stat arithmetic stay behind the `apply_damage` hook; the
dispatch that was the actual blocker is ~30 lines.

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

### The card cluster's second re-read: the block carries a checksum

The cluster's "one gap, an asynchronous card backend" reading held, but a
re-read of the *byte* half found the port writing blocks retail refuses. The
SC payload carries an **additive checksum** at
[`RETAIL_BLOCK_CHECKSUM_OFFSET`](../formats/save-record.md): the composer
`FUN_801E1934` sums the block and stores at `+0x1FFC`, and the load path's
state 5 (`FUN_801DD35C` at `0x801df880`) re-sums and routes a mismatch to the
"Damaged data." arm. `legaia-save` had asserted in four places that no such
word existed, so every in-place edit it performed left the word stale.

Two things generalise from it. The claim was **negative** - "there is no
checksum" - and a negative is not settled by reading the writer, only by
reading the *reader*; nothing had looked at the load path. And the cluster's
inert-ness hid it: because no host ran the card flow, no test round-tripped a
real block through both directions, so the defect sat behind the same wall the
wiring worklist describes. **An inert cluster's disclosures can be right about
the wiring and wrong about the bytes** - re-read both halves.

Four further claims fell with it, all corrected in
[`save-screen.md`](../subsystems/save-screen.md): `DAT_80084140` is the live
game-state window the block is composed *from*, not a save-block existence
table (the array walked at `+0x1818` is the item bag); the read and compose
buffers are two distinct `0x2000` regions (`0x801E5120` / `0x801E7120`), not
one used both ways; and `FUN_801DAFD4` is the shop's Buy/Sell/Quit picker
rather than a save-slot confirm.

### The index-space mismatch was the wire, not the blocker

`classify_card_directory`'s disclosure named the right fact and drew the
wrong conclusion from it. The fact: its class array is keyed by the save
number in a directory frame's **filename**, while the browser card rack's
5x3 preview grid is keyed by **physical block**, and on a real card the two
disagree - retail files a save under the save-select list position it was
standing on and lets the BIOS place the file wherever, so `-03` can sit in
block 1. The conclusion drawn was "adopting it means re-keying the grid".

Re-keying the grid is one thing a host could do with the walk. It is not
the thing the host needed. **A card's filenames must be unique** - the BIOS
directory is keyed by them - so any host that addresses a block has to ask
the card which save numbers are already spoken for before it claims one,
and that question is exactly what this walk answers. Retail never asks it
because it writes the number it is already standing on; that is why the
routine looks like a screen-building pass and is also a uniqueness oracle.

The rack was deriving the number from the block alone, which produced two
defects on any card retail had written: a duplicate filename whenever the
two spaces collided, and, on an overwrite, title digits that disagreed with
the filename the frame kept (an overwrite does not re-claim the frame).
`LegaiaRuntime::card_save_index` now takes the block's own number when it
has one and otherwise picks a free one off this walk's class array.

The transferable part: **a disclosure that names a mismatch should say what
the mismatch blocks, not just that it exists.** "The two spaces disagree"
is a fact about the port; "so no host can adopt it" was an inference nobody
checked, and it survived because the inference sounds like the fact.

### The prefix was wrong, and only an inert kernel could hide it

The same re-read found the walk could not have matched anything.
`CARD_SAVE_PREFIXES` spelled the separator `PRO_`; the retail literals in
the menu overlay's data segment (`0x801EF03C` / `0x801EF054`, PROT entry
0899 file `0x20824` / `0x2083C`) and the directory frames of real cards
both spell it `PRO-`. Four places in
[`save-screen.md`](../subsystems/save-screen.md) carried the same typo.

Two things made it invisible, and both are general:

- **Every fixture was built from the constant under test.** Six unit tests
  asserted the walk's behaviour by constructing frames out of
  `CARD_SAVE_PREFIXES` itself, so all six passed against a prefix that
  matches nothing. A self-referential fixture cannot see a wrong literal;
  it can only see a wrong *rule*.
- **The writer and the matcher were in different crates and only one was
  live.** `legaia_save::card::LEGAIA_SAVE_FILENAME_PREFIX` had it right the
  whole time, because the browser rack writes through it and real cards
  proved it. Nothing compared the two. The USA entry is now taken from that
  constant rather than respelled, so one retail literal has one owner.

This is the [second re-read's](#the-card-clusters-second-re-read-the-block-carries-a-checksum)
lesson landing a second time in the same cluster: an inert cluster's
disclosures can be right about the wiring and wrong about the bytes.

### `card_message_rows` named a string table that is a sprite table

`engine-ui`'s `card_message_rows` (`801e0418`) is still `DISCLOSE`, but one
of its two stated prerequisites was wrong about the mechanism. It read
"`msg_slot` indexes the slot argument retail's drawer (`FUN_801E2EE4`)
resolves against the menu overlay's message pointers ... even a wired host
would have five rows and no text."

`FUN_801E2EE4` draws no text. Its 4th argument is `(index & 0x3FF)` into a
20-byte-stride **sprite descriptor** table at `0x801E50A8` (PROT 0899 file
`0x16890`), and the routine builds one `0x34`-byte four-vertex GP0 packet
from it - tpage `+0x04`, CLUT `+0x06`, texel origin `+0x08`, extent `+0x0A`,
two RGB triples at `+0x0C` / `+0x10` scaled by the caller's brightness -
then links it into the OT through `FUN_8003D2C4`. The first two records read
`254x148` and `254x16`: the messages are pre-rendered image strips in VRAM.

[`functions/menus.md`](../reference/functions/menus.md) already classed
`801E2EE4` with its two sibling GPU-primitive emitters, so the disclosure
contradicted the repo's own function directory - which is the cheapest
possible check and the one that was skipped. The prerequisite is a parser
for that descriptor table plus its page resident in engine VRAM, i.e. a
disc-derived asset on the same footing as the save-icon sheet, not a text
corpus nobody has.

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

### What still chooses which painter runs

The painters' `WIRE` is about the dispatch, and the dispatch is real:
`World::tick_submode_screen` calls `HubPainter::for_window` on whatever record
index the open screen carries. What picks that index is a separate question,
and the answer is narrower than the row above reads.

Retail decides it through a panel **descriptor**: a state machine installs one
(`FUN_801E9B3C`) and the descriptor names the record. The port records the
install as `HubAction::InstallPanel(<descriptor VA>)` and
`World::apply_submode_actions` ignores that action, so the index comes from the
opener's argument instead. `World::open_coin_counter` passes record `1`; the
field-VM op-`0x49` path (`op49_menu_request`) passes `None` for every sub-op it
does not resolve elsewhere. So record `1` is the only index a production frame
reaches, and the entry list's record `3` paints when a caller names it.

The descriptors the ported state machines install (`0x801F3340`, `0x801F3360`,
`0x801F3370`, `0x801F3294`, `0x801F3388`, `0x801F2A88`) sit outside the
`0x801F2C0C` panel-window record table, so the descriptor -> record mapping is a
second format, and it is unread. Reading it is what would let a screen select
its own painter the way retail does.

## The field / motion / camera block

Another *disclosed*-section cluster, and the one where the disclosures were
least trustworthy. Every anchor here stayed inert, so the audit's own verdict
never moved - but a majority of the reasons blamed something the tree already
contains, which is the failure this page exists to catch. Sorted by what the
old reason got wrong.

**Reasons that named an existing symbol as missing.**

| Anchor | The old reason claimed | What is actually there |
|---|---|---|
| `post_touch` (`8003d038`) | the collision path posts no touches and cannot identify the actor it hit | `World::field_prop_dir_probe` reports the touched placement; `World::check_field_walk_touch` posts from the locomotion step |
| `motion_pause_kick` (`8003c9ac`) | no view can be projected to gate the sweep | both gates and the default-move table are projectable from the per-slot maps |
| `state_pick` (`801f1f4c`) | the engine models neither actor `+0x50` nor `+0x54` | `Actor::state_50` and `Actor::state_54`, at those offsets |
| `field_audio_release_steps` (`801d8450`) | no per-voice stop, no `0x80091508` table | `SustainedSfx::stop_voice` and `SeqResourceTable::release`, the module's own two `REF:` addresses |
| `submode_panel_rows` (`801e6984`) | the context block `open_submode` cannot reach | `open_submode` is live and seeds `World::submode_context`, which is read every frame |
| `field_actor_plan` (`8003bc08`) | the engine has no `+0x10` flag word | `move_vm::ActorState::flags` is that word, tested in production on pool actors |
| `tick_reflection` (`801e5154`) | the actor carries none of the fields this reads | `ActorState` carries all but `+0x64`, at retail offsets |
| `refresh_object_grid_marks` (`80017bec`) | the engine keeps no `.MAP` image | three of four regions are resident, and the collision grid is mutated live |
| `passive_hud_icons` (`801d095c`) | the projection host does not exist | `Camera::transform`, already placing effect billboards |
| `step_scene_program` (`801d4a60`) | `_DAT_8007BC20` has no counterpart | modelled by four ports; live source `AudioOut::xa_active()` |

Each reason is now rewritten to name the prerequisite that does hold. The
pattern across them is one shape: **the reason described the subsystem the
routine came from rather than the thing the port is waiting on.** Where a
subsystem is largely present, that phrasing lands on a piece of it that exists,
and the tag reads as correct forever after.

**Reasons that named a lane boundary.** Three tags gave "wiring would edit
`engine-core/src/world/**`, owned elsewhere" (or the equivalent) as the
blocker. A file-ownership statement is not a structural fact, it stops being
true the moment the wave lands, and it tells the next pass nothing. Those are
replaced by the storage or dispatch prerequisite in each case.

**One reason that restated the audit.** `expand_battle_id`'s "nothing
dispatches to it yet" is the form the preamble rules out. The real position is
sharper and partly *negative*: retail's caller is the battle-init formation
resolve `FUN_80055B6C`, which the engine has no analogue of (it resolves a
typed `FormationDef`, so no formation cell can be found empty), and the
non-zero-id arm reads `DAT_8007b7fc`, a global with **no writer anywhere in
retail**. Wiring that arm means adding a debug hook, not finding a caller.

**One row is not a wiring question at all.** `spawn_arc_helper`
(`801d5780`) is inert in the port because `FUN_801D5780` is inert in *retail*:
it has no `jal`, no `j` and no literal address word anywhere in `SCUS_942.54`,
the base-mapped overlay images, or the extracted PROT entries. Its three
siblings in the same module are the controls that make that a real zero -
`FUN_801D2404` and `FUN_801D25EC` are each found by `jal`, and `FUN_801D2298`
as a table word. The bytes are a complete routine (field overlay `0897_xxx_dat`
at file `0x6F68`, `addiu sp, sp, -0x28`), so this is shipped dead code, not a
mis-read address.

A `NOT WIRED:` reason that names a consumer the engine has yet to grow is wrong
for this shape, because it implies wiring could close the row and nothing can.
Two traps sit on the re-check: `ghidra/scripts/funcs/801d5780.txt` is a
wrong-image import whose header resolves `entry=801d56fc`, and the VA is
aliased - in the cutscene images it is a different function's entry. Read the
field-overlay bytes, not a dump.

**The one row worth a wire - and what wiring it turned up.** The
`field_ledge_hop_arc` anchors sat behind a real gap rather than a missing
reason: `World::try_field_ledge_hop` was live and classified an authored ledge
correctly, posted a `FieldLedgeHop`, and **nothing read it** -
`step_field_vertical` cleared the record at the top of the next frame. The port
had no ledge hop at all, a player-visible absence rather than a cosmetic one.

The named prerequisite (promote the record to a session with cursors that
outlive the frame) was right but incomplete, and the incompleteness is the
lesson: `advance_hop_session` (`FUN_801d2298`) **writes no position**. It is
the tick of the *paired* helper, the phase / SFX / movement-lock machine. The
record that moves the player is the arc helper, ticked by `FUN_801d5c08`,
which was not in the corpus at all. Neither tick has a caller, so no
call-graph question could have surfaced it; what does is the **template word**
- an actor template's `+0x08` is its tick pointer, and reading the three
templates the setup allocates from names all three ticks in one step. A row
whose port is "a clip on a spawned pool actor" should be read that way before
its prerequisite is called small.

Both ticks now run from `World::step_field_vertical`, and the hop is covered
end to end (`field_ledge_hop_wired.rs` synthetic, `field_ledge_hop_disc.rs`
against a real scene's authored geometry).

### Re-reading the rewritten reasons found five more that do not hold

The rewrite above replaced reasons that named an existing symbol. Re-checking
the replacements against the disassembly found that half of them describe the
gap at the wrong size - three too large, one at the wrong subsystem entirely.

| Anchor | The reason claimed | What holds instead |
|---|---|---|
| `post_touch` (`8003d038`) | the wait-for-touch arm at `0x8003882C` is unported, and the engine's slice of `FUN_80038158` is "ops `0x04` / `0x0D` plus the static MAN decode" | that arm **is** op `0x05`, ported and live; only its four-instruction mailbox head was missing. Now wired - see below |
| `spawn_arc_with_emitter` (`801d25ec`) | its callers are "the non-player arcs", which the world model has no actor-pool counterpart for | one named caller: field-VM op `0x43` sub-`0`/`1`/`0xA`/`0xB` at `0x801DF5AC`. What is missing is one keyed channel, not a pool |
| `fade_ramp` (`80020c14` / `80025000`) | wiring needs the retail system-actor pool behind the fade spawn | `FadeRamp` *is* the `+0x7C` block and a world field can hold it; the pool is only what `spawn_fade` needs for concurrent fades |
| `ease_camera_yaw` (`801da390`) | the engine has no `_DAT_8007BCAC` accumulator, so wiring is a fidelity-mode decision | the **target** is the harder half: nothing writes the zone angle either |
| `reset_pool` (`8003cda8`) | the host builds a fresh `RampScheduler` per scene | true, and it makes a call site provably unobservable - `new()` and `reset_pool()` leave byte-identical state |

**Op `0x43` sub-`0`/`1`/`0xA`/`0xB` is halt *and* arc.** The port's arm stops at
the halt-acquire, which is why `FUN_801D25EC` reads as caller-less. Retail runs
the arc unconditionally on the acquire's success side (`0x801DF410` takes the
PC-advance path only on failure), building the landing triple from the operand's
two tile bytes and falling back to the actor's own position when both are zero.
The port already forwards those coords to `FieldHost::field_halt_acquire_apply`,
so the hook is live; the gap is that `World::field_ledge_hop` is the *player's*
single `Option`, and this entry arcs whichever actor the script runs on.

**The camera row's two inputs are both absent, and one is upstream of the
other.** `zone_angle` is the camera-zone record's `+0x4A`, and its retail writer
is field-VM op `0x4C` outer-nibble-4 sub-9 - the same opcode that writes
`_DAT_8007BCAC` on its delta arm. The port dispatches that opcode but `World`
overrides none of its three host hooks (`op4c_n4_sub9_default_write` /
`_default_ramp` / `_delta_write_or_ramp` keep their no-op defaults), so an
accumulator added alone would have nothing to ease toward.

### The touch mailbox is now wired end to end

`FUN_8003D038` posts an actor id into `DAT_80073F1C`; the reader is the head of
the ambient VM's `0x05` wait arm, which rewrites the wait cursor to
`duration - DAT_1F800393` when the mailbox names its own actor and its
`0x801C6470` record byte is not the `0x8C` sentinel, then clears the mailbox.
The port had the countdown but not the head, so an NPC parked in a wait ignored
being walked into for the whole authored duration.

`AmbientMotion::pending_touch` carries the mailbox per channel (`None` = the
`0xFF` empty sentinel; only one actor can ever match a global post, so the
information is the same), `AmbientMotion::take_touch_wake` is the head, and
`World::post_ambient_motion_touch` posts from the locomotion step beside
`check_field_walk_touch`. Covered by `ambient_touch_wake.rs` (disc-free): an
NPC in the contact box turns, the same NPC out of it does not, and one whose
arena byte is the sentinel does not.

That also retires the blocker `motion_pause_kick`'s **superseded** reason gave
in the `WIRE` section above ("the port's field collision path does not post
touch events"). Its source tag had already moved on to a different and correct
reason - a requested-move channel field NPCs do not carry - so the row does not
move; the page's older text is what was stale.

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
  two-point draws; neither `engine-ui`'s draw list nor its PSX screen-space
  primitive set (`screen_prim`, a textured and a flat **quad**) has a line
  kind. Read the qualifier: there *is* a screen-space primitive path, live on
  both hosts, and a line kind alone would still not reach `payline_prims`,
  whose endpoints are model-space and want a GTE projection pass first.
- **No minigame effect-part pool.** `step_mark_effect_spawn`,
  `good_banner_spawn`, `splash_burst`, `ripple_spawn`,
  `dance_hit_sting_voices`. Partly closed: the dance's two spawn wrappers feed
  `engine-core::minigame_actor::MinigameActorPool` through
  `DanceGame::spawn_sprite_part`, and both hosts draw what it emits.
- **No dancer / fish actor records.** Closed for the dance -
  `engine-core::minigame_actor::MinigameActor` is the record and `DanceGame`
  holds two pools of it (the floor cast, and the sprite parts). Still open for
  the fishing rows: `roll_wander_target`, `step_facing`, `fish_camera`,
  `float_actor_tick`. `dance_face_rig` was never blocked on a record - see its
  own tag.
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

## Not on this page either: the render / GTE cluster

`engine-render`'s disclosed-inert rows - the billboard projector, the GTE
axis-rotation builders and view-rotation build, the clip / packet-colour leaf
kernels, and the actor-bind / actor-cull / afterimage / battle-tick /
battle-sideband / mode-transition passes - were swept the same three ways as
the rows above and came back **honest**: zero host-crate references for every
anchor symbol, and no `FALSE INERT`. They are recorded here only for what the
sweep found *inside* the disclosures, because both findings are shapes this
page exists to name.

**A disclosure can name the wrong level.** The three `GteMat3::rot_*` builders
were disclosed as "GTE-oracle-only", i.e. as having no consumer but the tests.
They have one: `camera_view_rotation`, the port of retail's own composition
pass `FUN_8001CF50`, which sits three lines below them in the same file and is
itself inert. Naming the tests as the blocker points a reader at coverage;
naming the composition pass points at the camera that has to exist first.

**"Has no source" can mean "has an inert producer".** The afterimage streak's
half-width was disclosed as a word `engine-core` "does not model". It does:
both of the streak's projection inputs are battle-context words
(`ctx[+0x1144]`, `ctx[+0x6C6]`) written by `FUN_801DEA50`, which is ported as
`engine-core::action_effect_script`. When a disclosure says a value has no
source, check whether the retail *writer* is already ported and inert - that
turns an open-ended reason into a named one.

That row said "the prerequisite is that module's caller". **It is not** - see
[the infrastructure cluster](#the-infrastructure--leaf-kernel-cluster) below,
which measured `FUN_801DEA50`'s references: the caller is `FUN_80047430`, it
is ported, and it is live. The stepper is since wired (the effect-script block
rides the disc action entries as
`MonsterAnimation::effect_script`, the cursor lives on the world actor, and
`World::tick_battle_animations` drives the walk - see
[`battle-action.md`](../subsystems/battle-action.md#the-per-action-effect-script-fun_801dea50));
what the streak is still waiting on is narrower: the terminator's
`ctx[+0x1014]` install and per-target `+0x1144` homing block are computed by
the kernel but have no engine-side context words to land in yet.

## The infrastructure / leaf-kernel cluster

Around forty-five disclosed-inert anchors spread over `engine-core`'s SCUS
leaf kernels, overlay/CD/MDEC plumbing, mode entry, cutscene elements and
effect ribbon; `engine-vm`'s SCUS helpers, VRAM rect copy, title primitives,
panel backread and world-map overlay leaves; plus single anchors in `asset`,
`mdec` and `engine-audio`. None is `FALSE INERT`. What the cluster produced
instead is a **measurement**, because every anchor was put through the
five-form reference scan
([`address-reference-scan.md`](address-reference-scan.md)) before its
disclosure was read, and the scan disagreed with the disclosure often enough
to be the point of the exercise.

### Two anchors are retail-unreachable

`FUN_801CFE20` and `FUN_801CFE5C` - the FMV overlay's `DecDCTinSync` /
`DecDCToutSync`-shaped wrappers, ported as `engine-core::mdec_dma_sync` - have
**no reference of any form** across all 1234 images, including the raw bytes of
every extracted PROT entry. The decode loop reaches the
blocking waits through the DMA kick routines `FUN_801CFFDC` / `FUN_801D0070`
instead, which call `FUN_801D0100` / `FUN_801D0198` directly. The module had
described the wrappers as the entries "every decode step funnels its channel
waits through"; that is now recorded the other way round, as code the game
links and never calls. This is the bucket a wiring worklist has no slot for:
the honest verdict is neither `WIRE` nor a prerequisite, but "no host call
could correspond to anything".

### Nine disclosures named a blocker that already exists

Each of these read as a correct disclosure and would have survived another
audit. They are listed with what the scan or a catalog lookup found instead.

| Anchor | The reason said | The measurement says |
|---|---|---|
| `801dea50` `action_effect_script` | the caller is the battle-action SM `FUN_801E295C` | no reference of any form inside that overlay image; both `jal`s are in the anim-node tick `FUN_80047430`, ported and live |
| `800265e8` `seed_boot_offset_table` | nothing in the corpus indexes `0x800917B0` | `FUN_8002630C` indexes it by VAB slot for `SsVabOpenHead`; the words are the per-slot SPU bases, already ported |
| `80020224` `walk_descriptor_pairs` | MAIN_INIT is documented but not ported | MAIN_INIT is ported, as `engine-core::mode_entry_init` |
| `80031ae4` `float_tween` | the label emitter `FUN_80032434` is not ported | it is ported; and the sibling draw pass `FUN_80031D00` named alongside it is ported **and live** |
| `801d841c` `save_screen_spawn` | nothing wants a flash element at all | it is not a flash element - descriptor `0x800706BC` names the save/load screen driver; and `FUN_801ED308` calls it, ported and live |
| `801d5e20` `shift_primitive_colours` | no caller | the field VM's op `0x4C` nibble-E sub-6 arm, whose host hook has an empty body |
| `801e5b4c` `aggregate_slot_stats` | the engine's equip screen has its own aggregator | the retail consumer is the hub entry list's sub-draw; the marker its live port emitted is now the sub-draw itself |
| `800468a4` `enqueue` | the field-VM hook has no renderer | that is one route; the actor tick's kind-7 draw arm is the other, and it is live |
| `8001fa00` `init_identity_index_list` | the emitter that pops the list is unported | true, but the *seeder* is MAIN_INIT, which is ported |
| `80035c00` `set_pair` | writing it from the menu host would invent state | the writers are three sites in the battle action resolver, not a menu |

The shape worth generalising: **a disclosure is most often wrong about the
half of the chain it did not have to look at.** Seven of the ten got the
engine side right and the retail side wrong, and the retail side is the one a
scan can settle mechanically.

### The rest measured honest

`panel_backread_loader` (one reference, the unported `FUN_80025358`),
`morph_weight_apply` (the template word at descriptor `0x8007068C + 8`, exactly
as disclosed), `effect_ribbon` (`FUN_8001ADA4` case 4, as disclosed),
`cutscene::sprite_stack_pop` (`FUN_801D629C` at `0x801D648C`), `gameover_banner`
(caller live, mode 18 never entered), `title_prim`, `overlay_loader`,
`chunk_install`, `cd_dma`, `input`, `scene_name_sync`, `mode_entry_init`,
`move_no_effect_guard`, `spawn_move_actor`, `scus_core_helpers`,
`monster_archive::animation`, `new_game`, `player_anm`, `strv2_decode` and
`seq_events`. `mode::other_warp_init_stage` is honest with a sharper edge: it
has **no `jal` anywhere**, and its one reference is the mode-table slot
`mode_table[24] + 0x10` at `0x800709DC`, a table `legaia_asset::mode_table`
already parses from the disc.

### One `WIRE`, since closed

`save_screen_spawn` (`801d841c`), whose call site is `PanelActorHost`'s handler
for the fade/flash actor's phase-1 arm in
`crates/engine-core/src/world_map_panel_host.rs`. The handler saved and cleared
the tint triple and stopped, dropping the spawn.

Reading the callee before wiring it changed what the wire *is*. `FUN_801D841C`
allocates from descriptor `0x800706BC`, whose handler word is the in-field
save/load screen driver, and writes `1` to `+0x5C` of the **returned** actor -
that driver's save-vs-load discriminator. So the arm is a save-screen hand-off,
and the routine's old name was for a reading of the bytes that the descriptor
table falsifies. Wiring it also identified the actor's parking releaser: the
menu overlay's save-side UI, which is the only other writer of the two globals
the two halves share. Both are written up in
[`world-map.md`](../subsystems/world-map.md#the-save-screen-hand-off).

## The minigame cluster's disclosures were mostly wrong about *what* blocked

A pass over the whole minigame slice - `baka_fighter*`, `dance`, `fishing*`,
`slot_machine`, `muscle_dome`, `minigame_floor`, `other_game_overlay` and
`engine-ui`'s `other_game_hud` - produced **no** new `WIRE` and **no** new
`FALSE INERT`. Every anchor really is unreached. What it did produce is six
disclosures whose named prerequisite already existed somewhere in the tree, and
two of those turned out to be port defects rather than wording. The corrected
texts live on the anchors; what belongs here is the pattern, because it is the
one a future audit will hit again.

### The repeated blocker is a Rust-side quad sink, named six different ways

Three subsystems each disclosed the *same* gap as a different missing artefact:

| Anchor | What its reason claimed was missing | What actually exists |
|---|---|---|
| `hud_widget_quad` (`801d5ed0`) | "`parse_baka_hud`, which no host calls" | both hosts call it; the browser also decodes the PROT 1203 art pack |
| `dance_face_rig` (`801d03c4`) | no face pages resident, no blit pass | `legaia_asset::dance_art` has both, run per frame by the browser dance page |
| the `other_game_hud` emitters | (correctly) no engine-side dome HUD renderer | - |

The single real blocker under all three is that every host consumes the *parsed
descriptor geometry* and composes its quads in JavaScript, so no Rust caller
ever asks a ported emitter for a packet. One sink closes the block; three
separately-worded reasons hid that.

#### What the sink needs, so the next attempt does not re-derive it

The sink is not a wrapper. Three things have to arrive together, and a shim
that satisfies the audit without them is worse than the honest disclosure:

- **A texel source on the native side.** The shape already exists - the play
  window calls `minigame_fx::dance_quad_draws` every frame with the live
  `DanceHudQuad` list - but it passes `solid_src: None`, because the dance
  sprite page is not uploaded, so the sink materialises nothing. The fishing
  HUD degrades the same way (`FishingHudAtlas::solid_src: None`). Adding a
  second emitter into that path reaches a dead end, not a renderer; the
  prerequisite is the overlay's 4bpp page resident in engine VRAM.
- **A quad-shaped request on the web side.** The dome page's HUD is a 2D
  canvas blitter: `muscle_hud_json` hands it sheet **rects** and the page's own
  `blit(src, pal, u, v, w, h, dx, dy, ...)` decides the destination. Consuming
  emitted quads means the page asking for `xy` per packet, which is a change in
  the page's JavaScript, not only in the wasm surface.
- **Somewhere to get the anchors.** Even with both of the above, `(x, y,
  scale)` is not disc-derived. Every call site of the three emitters - 9 / 31 /
  23 of them - is an immediate inside PROT 0977's own hub screens
  (`0x801CF2C0 .. 0x801D0324`), none of which is ported. So a sink makes each
  widget's **extent**, gouraud ramp and CLUT disc-derived while its
  **placement** stays the page's. That is real progress and it is worth doing;
  it is not "the retail HUD", and a wire that lands should say so.

`dance_face_rig` has a second twist worth keeping: the browser resolves the rig
from the disc **cast table**'s per-dancer kind, which on the qualifier floor is
already `0/2/3` - the exact output of the overlay's hard-coded slot -> rig
remap. The two agree, so the selector is redundant rather than missing. A
disclosure that says "no host" reads as work; "the host arrives at the same
answer from disc data" reads as a closed question.

### Three reasons were wrong about the *arithmetic*, not just the caller

All three are the failure direction the preamble warns about - an unwired
kernel's reading is never exercised, so a misreading survives:

- **`other_game_overlay::cue_position`** decoded `_DAT_80084580` as a
  party-block coordinate and returned a "positional pair". It is the
  voice/SFX **volume** config, and the pair fills `vol_l` / `vol_r` of
  `FUN_80065034(voice, level, program, tone, note, 0x40, vol_l, vol_r)` - a
  signature the SCUS cue drainer `FUN_80016B6C` pins by filling the same eight
  slots from a cue descriptor. Now `cue_volume`, with the other six slots named.
- **`dance_hit_sting_voices`** dropped two arguments of that same primitive
  (`level = 2`, `program = 1`). The program is what makes the browser page's
  `tones[1]` bank lookup correct rather than a guess - and
  `minigame-dance.md` had recorded it correctly all along, which is the
  reminder to grep `docs/` before re-deriving. The page now takes the whole
  triple from the kernel, so the row is wired; see below.
- **`minigame_slot_scene::sin_4096` / `cos_4096`** reproduced the two SCUS
  quadrature tables with `.round()`. The retail entries are
  `trunc(0x1000 * sin)`: truncation matches all 4096, rounding matches 2088.
  This one is not an inert kernel - the effect VM's spawn-leg rotation and the
  reel geometry both read it, and each multiplies the LSB by a radius - so it
  was wrong *output*, not just a wrong reading, and the engine-vm test that
  should have caught it was itself pinned to the port's rounded numbers rather
  than to the disc. `engine-render::billboard::psx_sin` had the same table
  right the whole time: two reproductions of one table, one of them wrong, and
  nothing compared them. The disc-gated
  `minigame_polar_trig_tables_disc` oracle now checks the reproduction entry
  for entry instead of only checking that the disc truncates.

### Three rows that looked like three gaps are one, and one is unwireable

- **`polar_offset` / `walk_grid_overhead` / `water_tile_class`.** The polar
  helper's reason said no engine code decodes its two quadrature tables. They
  are static SCUS rodata that `FUN_80026be0` publishes at boot, `legaia_asset`
  already names and synthesises them, and the play window materialises one. Its
  reason was also wrong about the callers: the slot machine's reel renderer
  reads the tables inline and never calls it, while every real caller is a
  facing-relative offset in the fishing overlay - including the cast that
  *creates* the lure point the other two rows wait on. One gap, three rows.
- **`marker_template`.** Its reason said the step-layer record lookup is not
  ported. `FUN_801D3EC0(1, x, z)` asks for sub-table kind **1** of the `.MAP`
  region block - the same tile-trigger records `engine-core::field_regions`
  decodes - so only the tile-actor sink is missing.
- **`project_segment` (`801d5c2c`) cannot be wired at all.** A five-form
  reference sweep finds zero references to it anywhere, and its band holds no
  pointer table, so retail never executes it. Its old reason named a missing
  line primitive, which implied a call site could exist; none can. Its 2-D
  sibling `clip_segment_2d` *is* live retail code with one caller, so the pair
  is asymmetric and had been disclosed as symmetric.

The transferable rule: when a reason names an artefact, check the artefact
before checking the caller. Four of these six named artefacts were already in
the tree, two of them cited by name three files away.

### Two of the six close on a shared kernel, not on the sink

Neither needed the quad sink, and both are now live:

- **`dance_hit_sting_voices`.** The browser dance page already held both named
  prerequisites and only ever recomputed the triple; it now asks the kernel,
  which is what makes the bank index a read of the retail `program` argument
  instead of a literal that happened to agree. Reading the *caller* while
  wiring it also corrected the subsystem doc: `FUN_801D1AF4` reaches the sting
  from four sites, and only one passes `rand() % 3` - the three groovy-move
  tiers each pass a literal `5`, a sting outside the random space that the
  page's `r > 2` bound had been dropping entirely.
- **`other_game_hud::decimal_slots`.** Its reason said "reached only through
  `decimal_quads`", which was true and hid that the fill is *shared*:
  `FUN_801D1308` and the fishing overlay's `FUN_801D76E0` open with the
  identical eight-slot loop, register allocation apart. `number_digit_cells`
  now takes its slots from here, which puts the row on the live fishing HUD
  path on both hosts. The two retail routines diverge only after the fill -
  one emitter and a patched descriptor column against two emitters and two pen
  pitches - so the emit halves stay separate, and delegating the whole routine
  would have been the silent behaviour change.

The second one also removed a port-side deviation nobody needed: the fishing
field clamped a negative value to zero. Retail needs no guard - the fill leaves
the slots blank and the draw loop's `bltz` skips the one negative slot - so a
negative value draws nothing, in both routines.

### One more latent name collision, defused

`other_game_overlay`'s free `sfx_cue` shared its name with
`MenuInput::sfx_cue`. Nothing had fired - the free function is inert and the
method is a method - but it is the
[`description_source`](#a-latent-duplicate-free-function-name-landmine-defused)
shape exactly, and the first bare `sfx_cue(..)` call anywhere would have turned
a correct disclosure into a false accusation. Renamed to `arena_voice_cue`,
which also says what it builds.

### Two rows that read as dead code and are not

`mirrored_sprite_pass` (`801d49e8`) and `editor_tick` (`801d4fc8`) have no
`jal` anywhere, which invites the `project_segment` verdict. Both are wrong for
it: each address is the callback word of a `0x18`-byte actor prototype in the
Baka overlay's rodata (`0x801D7688` and `0x801D7670`, adjacent records). They
are spawnable; what never happens for the editor is its *band* gate. "No `jal`"
is not "unreachable" until the literal-word form has been checked too - which
is the whole point of sweeping five reference forms rather than one.

## The SCUS battle-kernel block: one wire, and where its reach really ends

`scus_battle_helpers` disclosed four arithmetic kernels behind four different
missing halves. One of the four closed, and it is worth recording as a shape
rather than as a row, because the clause that gave way was not the one the
disclosure leaned hardest on.

`bgr555_to_grey` (`8004ce2c`) named three prerequisites: no per-actor palette
copy, no `actor[+0x220..=+0x223]` status latch, no mid-battle CLUT re-upload
path. The first two were accurate and are now supplied by
`engine-core::battle_status_clut` (the copy, the latch) armed from
`BattleHud::sync_status`, the one per-slot-per-frame call every host already
makes with the tracker in hand. The third was already close to false when it
was written: the native window's face-stamp pass had been mutating the stashed
battle VRAM mid-battle and re-uploading it with the resident-generation
bookkeeping - it moved texels rather than CLUT rows, which is a different
*payload* on an existing *path*.

**A disclosure that lists three blockers is three claims, and they can be at
three different ages.** Re-read each on its own; one being solidly true does
not carry the others.

The wire's own limits, stated so the next reader does not re-derive them:

- The **Rot** arm of the same pass (status bits `0x08`/`0x10`/`0x20`, latches
  `+0x221..=+0x223`) is still out. It applies its ramp over a per-character
  index window read from the 3-pair table at `DAT_80078630`, and no crate in
  the workspace parses that table.
- The pass is live but **does not fire in ordinary play**, for a reason that
  sits upstream of every address on this page: the port has no monster-side
  `enemy_effect` source. The only production `stage_art_profile` call is the
  party-caster path `World::arm_party_art`, so status flows party -> monster
  and never monster -> party, and rows `481..=483` are the party's. Reachable
  and non-trivial is not the same as exercised; both are worth saying.

## See also

- [`port-catalog.md`](port-catalog.md) - the catalog, the `live` axis and the
  audit that produces the input to this page.
- [`worklist-classification.md`](worklist-classification.md) - the sibling
  classification for the `--missing-ports` worklist.
