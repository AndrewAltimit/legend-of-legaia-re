# Lane B handoff - the pause menu's draw half, hoisted

## The problem this lane was given

Wave 1's menu ladder measured 84 live-unentered rows in the pause-menu / save-UI
slice and entered 20 of them. The largest residue bucket was not "the replay
does not walk far enough": **37 rows were reachable only from host-private
modules.** Every `engine-ui` row (36) plus `save_select::SlotInfoMode`. The only
code that assembled a pause-menu draw list lived in
`crates/engine-shell/src/bin/legaia-engine/window/menu_draws.rs` (+
`window/title_save_draws.rs`) and in `crates/web-viewer/src/play_menu.rs`.

A `tests/` target cannot import a binary's modules. So no library-level oracle
could enter those anchors - not the wave-1 ladder, and not one twice as long.
The gap was structural.

## What moved into `engine-ui`

New module `crates/engine-ui/src/pause_menu.rs` (~800 lines), plus
`crates/engine-ui/tests/pause_menu_compose.rs`.

| Moved | Was |
|---|---|
| `MenuRects` - descriptor id -> rect / pen / frame box | a private method trio on each host |
| `MENU_WINDOW_FALLBACK` (pinned rect table) | two byte-identical 23-row constants |
| `MENU_SUBWINDOW_CONTENT` | two identical constants |
| `stage_transform` | `save_select_stage` + `stage_transform`, identical arithmetic |
| `pause_screen_draws` - per-screen window set, tab painter, content builders, sprite pass, stage scale | two private copies |
| `equip_screen_compose` - the owned-model -> `EquipScreenView` borrow | two copies |
| `spell_level_notice_draws` (window 7) | two copies |

Nine screens go through the one composition: top level, Status, Options, Items
(with the throw-out and both Use-route confirms), the window-14 target panel,
Magic, Equip, the three generic near-fullscreen screens (Arts editor, spell
target-select, inventory stand-in) and the kind-`0x0D` notice / ready pair.

`engine-core::pause_screens` gained `EquipScreenModel` + `equip_screen_model` -
the third descriptor-window screen model, beside the existing
`items_screen_model` / `magic_screen_model`. That is the ~120-line projection
(eight slot labels, the candidate list with bag counts, a full
`compute_battle_stats` pass with the hovered item installed) that was written
out verbatim on both hosts. Two copies of a stat preview is two chances to
preview a different number.

## What each host still owns, and why

`engine-ui` deliberately does **not** depend on `engine-core`.
`docs/subsystems/engine.md` states the layering rule explicitly -
`engine-render -> engine-ui, asset, tim, font (wgpu; no engine-core dep)`, and
"`engine-render` / `engine-audio` are leaf presentation crates ... they do not
depend on `engine-core`". `engine-render` re-exports `engine-ui` wholesale, so
a dependency here would be one there. Reversing that is a real architectural
decision and not this lane's to make unilaterally.

The cost is that the **session -> view projection** stays per host:

| Host-owned | Why |
|---|---|
| Asset resolution (font, chrome atlas, window table) | the assets are the host's |
| Projecting a live session into the `engine-ui` view structs | needs `engine-core` types |
| The Equip phase-tag crossing (a 3-arm match, once per host) | two enums, no shared crate names both |
| The inventory target-select stand-in's layout | walks `InventoryUseSession`'s bag directly |
| The Load / Save sub-screen | see below |

**The Load / Save sub-screen is out of the hoist on purpose.** It is the
save-select surface, and the native window reaches the *same* screen from the
boot Continue -> Load path (`BootUiState::SaveSelect`). Both native entries
already share `save_select_phase_text_draws` +
`save_select_chrome_sprite_draws`. Hoisting only the pause half would have
forked the native save screen in two - the opposite of the goal. The right
hoist for that screen is boot + pause + browser onto one composition, which is
its own piece of work; the browser's `build_save_select` is still a third copy
and it differs from the native pair in two visible ways (which pills draw
during the confirm phases, and whether the block grid stays visible under the
confirm messagebox). **Inferred** that the browser's reading is the more
retail-faithful one - retail's mode-2 pill relocation happens on commit and
the confirm is post-commit - but that is not measured and I did not act on it.

## Drift the hoist closed (measured)

Two divergences that had already grown between the two copies, both of the
tier-7 shape and neither expressible as a `RENDER_KERNEL_RULES` row (they are
*ordering and choice inside one assembly*, not a kernel a file does or does not
name):

1. **The title tab used a different painter per host.** Native resolved it
   through `painter_at(table, tab_id, TitleTab)` -> `title_tab_draws_for`,
   falling back to the pinned pen. The browser called `tab_label_draws`
   unconditionally on every sub-screen. A table whose tab renderer moved (a
   modded disc) put the label in two different places depending on host.
   Pinned by `the_title_tab_follows_the_descriptor_painter`.

2. **The Items screen's Use-route confirm framed in a different sprite
   order.** Native emitted its frame between the window set and
   `items_screen_sprites_for`; the browser emitted it *before* the window set,
   so the item-list frame painted over the modal. The shared composition now
   orders frames -> screen content -> modals on both.

A third defect the hoist exposed, present **identically** on both hosts and so
invisible in any host-vs-host diff: the four modal window ids (9 throw-out,
10 / 12 Use confirm, 14 target panel) were guarded with
`if pen == (0, 0) { fallback }`, and the guard never fired. An id missing from
the fallback table resolves to `MENU_SUBWINDOW_CONTENT`, whose origin is
`(18, 18)`, not `(0, 0)`. So a disc-less run drew all four at the
near-fullscreen origin. The four ids are now rows in the shared fallback table
and the dead guards are gone; pinned by
`the_modal_windows_have_their_own_pinned_rects`.

One behaviour changed as a side effect and is worth knowing: the window-7
spell-level notice is now always stage-scaled. Previously the native window
skipped the scale for it whenever the Save sub-screen was open (the notice was
appended before an `if !is_save_sub` scale).

## Coverage delta

**Measured.**

```
cargo llvm-cov --release -p legaia-engine-ui --test pause_menu_compose \
    --json --output-path target/b-cov.json
scripts/ci/replay-port-coverage.py --json target/b-cov.json
```

Joined against `port-catalog.py`'s liveness verdict, restricted to anchors
under `crates/engine-ui/` using the script's own resolution:

| | N |
|---|---|
| statically live `engine-ui` port anchors | **72** |
| entered by `pause_menu_compose.rs` | **35** |
| live, still not entered | **37** |
| not observable in this binary | **0** |

Read the denominator before the number. **72 is every live `engine-ui`
anchor**, not wave 1's 36-row slice - that slice was measured from the
engine-shell ladder's coverage JSON, which is not reproducible here, so the two
counts are not the same fraction of the same set. What is comparable is the
direction: before this lane no `tests/` target could reach a pause-menu
assembler at all, so no library-level run could enter any of them.

The 37 that stayed are all **other screens**, and none is a pause-menu screen:

| what | rows |
|---|---|
| fishing HUD (`ui_fishing.rs`) | 11 |
| Muscle Dome arena hub (`other_game_hud.rs`) | 6 |
| save-select (`ui_title_save/**`) | 6 |
| shop descriptor windows + the two waived compare painters | 9 |
| records screen / dev menu / name entry | 4 |
| module-scope anchors on files the test does not touch | 1 |

Of those, **only the save-select six are in this lane's subject matter**, and
they stayed for the stated reason: that screen's composition is shared with the
boot Continue -> Load path and hoisting half of it would have forked it.
**Inferred** from the itemisation - not separately measured - that the
pause-menu portion of wave 1's 36-row residue is now essentially fully entered,
since nothing left in the 37 is a pause screen.

## Waivers

`scripts/ci/ui-host-drift-waivers.toml` has **four** `[[waiver]]` blocks, not
the six the brief quotes - `git show HEAD:` confirms four at the branch point,
so the six is a stale number from an earlier tree, not something this lane
closed.

**The hoist makes none of the four false**, and each premise was re-checked
against the tree rather than believed:

| waiver | premise | verdict |
|---|---|---|
| `key_rebind_draws_for` | the options screen has no row that opens a sub-screen and `OptionsSession` carries no binding-edit state | holds - `OptionsState::rows()` maps `OPTIONS_DISPLAY_ROWS` with no sub-screen route; `KeyRebindSession`'s only non-module caller is `engine-core/tests/menu_suite_e2e.rs` |
| `count_panel_draws_for` (win 24) | the port's Equip screen is a slot list with no item-info panel, so there is no rect to add a count to | holds - the shared composition frames `EQUIP_SCREEN_WINDOWS` (2 / 21 / 22 / 23); no window 17 or 24 |
| `equip_compare_panel_fields` (win 25) | the port prints its stat compare through `equip_screen_draws_for`'s `stat_compare` rows, and win 25's rect overlaps party window 21 | holds - `stat_compare` is still the path, now fed by the shared `equip_screen_model` |
| `choice_panel_draws_for` (win 46) | blocked on the casino prize-exchange window set (43 / 44 / 45 / 46) | holds - untouched by this lane |

Deleting a waiver that is still true would be the mirror of the failure the
file exists to prevent, so all four stay.

**Two `CONSTANT_PAIRS` rows were deleted**, which is the other half of the
instruction. `MENU_WINDOW_FALLBACK` / `WINDOW_FALLBACK` and
`MENU_SUBWINDOW_CONTENT` / `SUBWINDOW_CONTENT` no longer exist as per-host
constants, so the checker failed on four unresolvable pairings. A pair proves
two copies agree; one copy needs no proof. The reasoning is recorded where the
rows used to be, and `docs/tooling/host-drift.md` gained a section on it -
because a missing pair reads two ways (shared, or re-duplicated) and the
checker cannot tell them apart.

## Files touched

| File | What |
|---|---|
| `crates/engine-ui/src/pause_menu.rs` | new - the shared composition |
| `crates/engine-ui/src/lib.rs` | `pub mod pause_menu` |
| `crates/engine-ui/tests/pause_menu_compose.rs` | new - the library-level oracle |
| `crates/engine-core/src/pause_screens.rs` | `EquipScreenModel` + `equip_screen_model` |
| `crates/engine-shell/src/bin/legaia-engine/window/menu_draws.rs` | rewritten as projection only |
| `crates/engine-shell/src/bin/legaia-engine/window/title_save_draws.rs` | `field_menu_chrome_sprite_draws` -> the shared sprite half; `save_select_stage` delegates |
| `crates/engine-shell/src/bin/legaia-engine/window/boot_cutscene.rs` | the `FieldMenu` draw arm |
| `crates/engine-shell/src/bin/legaia-engine/window.rs` | dropped the pinned rect table + pen / frame helpers |
| `crates/web-viewer/src/play_menu.rs` | builders became projections; dropped the pinned table + the equip projection |
| `scripts/ci/check-ui-host-drift.py` | two `CONSTANT_PAIRS` rows retired, with the reason in place |
| `docs/subsystems/field-menu.md` | "Where a pause screen is assembled" |
| `docs/tooling/host-drift.md` | tier 2 "a deleted pair is not always lost coverage"; tier 7 "the version that needs no rule" |

## `legaia-engine-core` does not build its tests at the branch point

**Measured, and not this lane's.** `crates/engine-core/src/world/tests/field_npc_motion.rs:180`
constructs `WalkTouchEvent::Warp { target_map: 3 }`; the variant was renamed to
`{ sub_id: u8 }` in `man_field_scripts/npc_motion.rs` and this caller was not
updated. So on `wave/re-closeout-0804`, before this lane existed,
`cargo test -p legaia-engine-core` and
`cargo clippy -p legaia-engine-core --all-targets` are both red at the compile
step (`git show HEAD:` confirms both sides).

The rename is **semantic, not cosmetic**, and that is the part worth handing
on. Applying the obvious one-token fix (`sub_id: 3`) makes the crate compile
and then leaves exactly one failing test:

```
world::tests::field_npc_motion::walk_touch_warp_posts_once_per_contact_and_queues_transition
  left: None   right: Some(3)   "the door-warp queues through the same path the 0x3E op uses"
```

That is consistent with the rename's own comment - `PlacementKind::Portal` is
gated by `is_genuine_warp` to `op0` in `100..=106`, "so its payload is a
minigame sub-id, whatever the field is still called" - i.e. a walk-touch
`Warp` no longer queues a scene transition, and the test is now asserting the
pre-rename model. Resolving it is a `world/**` + `man_field_scripts*` question
and belongs to whichever lane owns the rename.

**The fix is not in this lane's diff** - it was applied locally to measure the
above and then reverted, so the branch-point state is preserved exactly. What
that temporary state did buy is the verification below.

One formatting change to an off-limits file **is** in the diff:
`crates/engine-core/src/man_field_scripts/npc_motion.rs:1039`, a `cargo fmt`
reflow of the rename's own call site picked up by `cargo fmt --all`. The branch
point is fmt-dirty there; reverting it would leave `cargo fmt --all -- --check`
red. Zero semantic content.

## For the integration pass

1. **The save-select screen is the remaining three-way copy.** boot
   `SaveSelect`, field-menu `Save`, browser `build_save_select`. Hoisting it is
   the same shape as this lane and would make the `ui_title_save` rows
   enterable; resolve the two divergences named above while doing it.
2. **The Equip phase-tag crossing is the residue of the layering rule.** If a
   future wave decides `engine-ui` may depend on `engine-core`, the projection
   halves collapse into the composition and both hosts drop to a handful of
   lines. That is a documented-architecture change, not a refactor.
3. Do not restore the two deleted `CONSTANT_PAIRS` rows without checking which
   way the constant went.
