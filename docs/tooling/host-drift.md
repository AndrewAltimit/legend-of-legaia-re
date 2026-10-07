# Host drift: keeping three hosts running one engine

The port ships one engine behind more than one framebuffer:

| Host | Crate | Driven by |
|---|---|---|
| native play-window | [`crates/engine-shell`](../../crates/engine-shell/) | wgpu via [`crates/engine-render`](../../crates/engine-render/README.md) |
| browser play page | [`crates/web-viewer`](../../crates/web-viewer/README.md) `runtime.rs` + `play_*.rs` | [`site/js/play-app.js`](../../site/js/play-app.js) |
| browser minigames page | same crate, `minigames*.rs` (`LegaiaMinigames`) | per-minigame modules under `site/js/` |

A feature wired into one and not another is invisible in a diff, because no
file holds two of the columns. That is the whole failure class these gates
exist for, and it is a class with several distinct shapes - each gate below
answers exactly one of them and is deliberately silent about the rest.

Related pages: [`shipped-bundle-freshness.md`](shipped-bundle-freshness.md)
(the bundle a host actually runs), [`port-catalog.md`](port-catalog.md)
(whether a ported function is reached at all).

## Where the gates run

The host-drift tiers below all run in the `gates` job of
[`.github/workflows/main-ci.yml`](../../.github/workflows/main-ci.yml) and from
[`scripts/git-hooks/pre-commit`](../../scripts/git-hooks/pre-commit) as a fast
local mirror. CI is the authority: a gate that lives only in a hook is one
`LEGAIA_SKIP_PRECOMMIT=1`, or one clone that never ran
`scripts/ci/install-hooks.sh`, away from being fiction, and neither leaves a
trace in the repository.

That job is disc-independent by construction - every gate in it reads the
repo's own sources, docs and site fragments - so it stays green on a runner
that has no `extracted/`.

### The one class of gate allowed to be hook-only

"Every gate runs in both places" is the rule, not a description of the whole
gate corpus, and the exception has to be stated or it becomes an excuse. A
gate may be hook-only when **its input is gitignored** - the Ghidra dump
corpus under `ghidra/scripts/funcs/`, or `extracted/`. Such a gate cannot
measure anything on a runner, and the hook is the only place the bytes exist,
so "it cannot run in CI" argues for wiring it into the hook rather than for
wiring it nowhere.

Two obligations come with that exemption. The gate must **self-skip** where
its inputs are absent, so a clone without disc data passes rather than fails.
And where skipping is free it should appear in CI anyway - a step that reports
`SKIPPED` is a step whose deletion someone would notice, which a hook-only
gate is not. The disc-coverage ratchet is wired that way.

Currently hook-only under this exemption: the three dump-corpus integrity
checks (`check-dump-stat-drift.py`, `check-dump-base-integrity.py`,
`check-jal-target-integrity.py`) - see
[`dump-corpus-integrity.md`](dump-corpus-integrity.md) and
[`call-target-integrity.md`](call-target-integrity.md).

### The browser host is also the one nothing here compiles

Every tier below reads sources. Every cargo gate beside them - `cargo fmt`,
`cargo clippy --all-targets --workspace`, `cargo test`, `cargo build` - builds
for the **host** triple, so the `#[cfg(target_arch = "wasm32")]` blocks that
run through much of `crates/web-viewer/` are code none of those four ever
sees. A page feature written inside one can name a private field, a moved
method or a deleted type and stay green through the whole ladder; the first
thing that compiles it is a wasm build, which is where one was found.

[`scripts/ci/check-wasm-target.sh`](../../scripts/ci/check-wasm-target.sh)
closes that: `cargo check --release --target wasm32-unknown-unknown -p
legaia-web-viewer`, run from the pre-commit hook whenever a staged file
belongs to a crate carrying such a block. It is a type-check rather than a
build and shares the profile `build-wasm.sh` and the CI wasm step use, so a
warm run costs about a second.

It runs in two places. The pre-commit hook runs it on a staged file in such a
crate, and the `pr-lint` job of `.github/workflows/main-ci.yml` runs it on
every push to a pull request, beside `cargo fmt` and `cargo clippy`, so the
browser host is compiled before a merge rather than only after one.

### A wasm export is a property of its impl block, and no compiler reads that

Compiling the browser host is still not the same as exporting from it. A
method the page calls through `wasm-bindgen` exists in JavaScript only if it
sits in the `#[wasm_bindgen] impl` block; the same method in a plain
`impl LegaiaRuntime` compiles, type-checks for `wasm32`, passes clippy and the
drift tiers, and is `not a function` in the browser. That is how the play
page's save-select backdrop accessor shipped undrawn - the page's call sat
behind a `try` that fell back to `null`, so nothing reported it. The only
instrument that sees this is a headless run of the **built** bundle, which is
therefore the check a new page export owes before it counts as wired.

## Tier 1 - reachability: does a screen reach both hosts?

[`scripts/ci/check-ui-host-drift.py`](../../scripts/ci/check-ui-host-drift.py).

The surface is derived, not listed: every `pub fn` in
[`crates/engine-ui`](../../crates/engine-ui/README.md) whose return type
mentions one of the crate's draw-record types is one screen's geometry
builder, and a host "has" that screen when its shipped source reaches the
builder. No hand-maintained list of screens can fall out of date.

**Record types on the surface.** `TextDraw` / `SpriteDraw` are the terminal
records a renderer consumes; `HudDraw`, `HudQuad`, `DigitCell`, `BarFrame` and
`ComparePanelField` are intermediate - a resolved screen that still needs a
host-owned atlas or font. Both count. A builder that *takes* draw records and
returns them is a transform over a draw list rather than a projection of a
model, and is excluded.

**Reachability is transitive, over every `fn`.** Builders compose, and they
compose through methods as often as through free functions: engine-ui's
`FishingBanners::service_frame` takes the four banner builders as function
pointers, and `HudDraw::resolve_bar` is what reaches `bar_frame` /
`power_bar_frame`. A builder-to-builder graph reports six wired builders as
unused, so the graph spans every `fn` the crate defines. Non-builder functions
are graph nodes only; nothing classifies them.

Per builder: **both hosts** → ok; **native only** → DRIFT (fails);
**web only** → informational; **neither** → ORPHAN (fails).

**A shared composer counts for the hosts that call it.** Some builders have no
host call site at all: the shop-family screens' only caller is
[`crates/engine-screens`](../../crates/engine-screens/README.md), the
projection both hosts share. The gate's `SHARED_COMPOSERS` table names each
such crate with its entry points, and a builder named in the crate's source is
credited to exactly the hosts that **call** an entry (`shop_overlay_frame`).
The credit is per host, not per crate: a host that stops calling the entry
loses every builder behind it, so a one-host composition still reads as
`DRIFT`.

Every orphan is named on stdout, waived or not. A bare count cannot tell "the
same six as yesterday" from "a builder's last caller was deleted this
morning", which is how deleting `RecipientWindowRects::active_compare`
orphaned window 25's entire painter chain without a line of output changing.

**Waivers** live in
[`scripts/ci/ui-host-drift-waivers.toml`](../../scripts/ci/ui-host-drift-waivers.toml)
and are validated in both directions - a waiver for a deleted builder, for a
builder now wired on both hosts, or in the wrong bucket, each fails. See
["What a waiver may say"](#what-a-waiver-may-say).

### What tier 1 does not prove

It asks whether a host's source *names* a builder. It says nothing about what
the host passes, whether the call is reached at runtime, or whether the two
hosts render the result the same way. The web bucket is also one label for two
surfaces on purpose: both browser pages ship in one cdylib, so splitting them
would not find a gap - it would manufacture eighty, because the minigames page
is a different screen set rather than a second copy of the play page. The real
cost of that collapse is a *model* question, which tier 3 answers.

### The tier's denominator is `engine-ui`, and a screen can live outside it

Tier 1 enumerates **`engine-ui` draw builders** and asks which hosts name each.
A screen whose content comes from an `engine-core` or `engine-vm` kernel
instead is not in the denominator at all, so a panel one host draws and the
other does not is not a `DRIFT` row, not an `ORPHAN` row, and not a count that
moved - it is simply absent.

That is not a hypothetical. The shop's two *pens-returning* windows are both in
this class, because a kernel that hands back pens rather than a draw list has
no `engine-ui` builder to be enumerated:

* window 35, the buy-quantity readout
  (`engine-core::shop::shop_buy_quantity_panel`, `FUN_801D5510`) - it drew on
  the browser page alone, so a native-window buyer sized a stack with no held
  count, no unit price and no running total on screen.
* window 39, the sell-list item detail
  (`engine-core::shop::shop_sell_detail_panel`, `FUN_801D5AE8`) - likewise, so
  a native-window seller picked a row with no name, no description, no price
  and no accessory-passive lines beside it.

Both draw on both hosts. Window 39 carried a prerequisite window 35 did not,
and it is worth naming because it is the sort a "just call the kernel" reading
misses. Its passive lines route through `shop::item_passive_index`, whose
equipment arm needs the equip record's `+5` byte, so a host that holds
equipment restrictions as `equipment::DiscEquipInfo` cannot resolve the
equipment-arm passive index until that byte is kept beside `+6` / `+7`. The
missing piece was never the call site - it is
`DiscEquipEntry::passive_index` and `DiscEquipInfo::row_passive_index`, which
the shop's window-39 painter reads on both hosts.

The general shape: **tier 1 is silent about every screen whose content kernel
is not an `engine-ui` builder.** A cheap sweep that finds them is "public
content-shaped `fn` in `engine-core` / `engine-vm` named by exactly one host's
sources" - both shop windows surface on it. Read the hits, though, rather than
counting them: most one-host kernels are correct, because the minigame pages
and the native minigame screens are genuinely different screen sets.

## Tier 2 - paired constants: do paired values agree?

`CONSTANT_PAIRS` in the same script. Each row names a constant on each host
that both feed to the *same* shared kernel - an `engine-ui` builder for the
screen rows, `swap_bgm` for the BGM transition row - and the two initialisers
must normalise to one token stream. Formatting, comments and import aliasing are
noise; a changed digit, a dropped row and a reordered table are all value
changes. The normaliser's control suite pins both directions, because a
normaliser that collapsed everything to `""` would report every pair equal.

Proves the two named constants carry equal values and that neither was renamed
out from under the pairing. Proves nothing about how either host *uses* them,
nor about any unpaired literal - and nothing about the **space** a paired value
lands in. The shop pen pair stayed equal while one host extended the builders'
stage-placed rows into a surface-pixel list and the other scaled them: a paired
constant pins a value, not the transform applied after it. That split is a
tier-3 `SIM_PAIRS` question (the `build_hud` / `play_overlay_draws_json` row).

### A deleted pair is not always lost coverage

A pair exists because a value exists twice. The better outcome is that it
exists **once**, in a crate both hosts already depend on - at which point the
row comes out of `CONSTANT_PAIRS` and nothing is left to check. The pinned
menu window-descriptor rect table and the near-fullscreen sub-screen rect went
that way: both are now single constants in `engine-ui::pause_menu`, read by
the shared pause-menu composition.

So a missing pair reads two ways, and they are opposites. Before restoring
one, look at where the constant went: a *shared* constant needs no pair, a
*re-duplicated* one needs its row back. The checker cannot tell them apart -
it only reports that a named constant is gone - so the reasoning belongs in
the comment where the row used to be, which is where it now sits.

## Tier 3 - simulation: do both hosts feed the same kernel?

`SIM_PAIRS`, the simulation twin of tier 2. Each row names a feature and, per
host, the **injection site** where that host hands a model to a shared kernel.
Three assertion modes:

| mode | assertion |
|---|---|
| `symbols_all` | each named symbol appears in both bodies |
| `symbols_same` | each named symbol appears in both, or in neither |
| `pattern_same` | the *set* of regex captures is equal across the two |

`pattern_same` is the mode that does not need the answer in advance: it says
the two sites must agree without saying what they must agree on, so it keeps
working when the right set changes.

A row may carry `blocked_on`, marking a divergence that is known and being
closed elsewhere. The marker is validated in both directions like a waiver: a
`blocked_on` row that diverges reports without failing, and a `blocked_on` row
that has gone **clean** fails, demanding the marker be deleted. A pending row
therefore cannot decay into a permanent exemption.

Proves the two sites mention (or omit) the same kernels. Does not prove the
arguments are equal, that the calls run in the same order, or that either site
is reached at runtime.

### Rows on the surface today

| feature | assertion |
|---|---|
| Muscle Dome damage | `pattern_same` over the `resolve_turn*` family each host names. |
| save-select model | `pattern_same` over the `SaveRack` variant each host builds. |
| live-loop arming | `symbols_all` on the shared `World::arm_live_loop`. |
| pause-menu open | `symbols_all` on `FieldMenuGate` + `SceneMode::Menu`. |
| menu-open precondition | `symbols_all` on `field_menu_open_allowed`, across all three open sites. |
| party wipe | `symbols_all` on `GameOverOutcome::ReturnToTitle` across the two routing sites. |
| dev-menu tick | `symbols_all` on `retail_packed` + `commit_equip_row` + the records-page toggle. |
| dev-records model | `symbols_all` on `record_counters` + `records_screen` across the two model builders. |
| play clock | `symbols_same` on `advance_play_time` across the two menu draw sites. |
| walk-ground render surface | `symbols_all` on `field_ground::render_positions` (the sink) and `render_indices` (the winding) across the native mesh builder and the play page's ground exports. |
| visible-tile crop | `symbols_all` on `field_view_window::field_view_cells` + `framing_is_retail` (whether a frame crops), `terrain_draw_visible` (the terrain list) and `field_ground::crop_indices` (the ground) across the native redraw / ground re-upload and the play page's crop exports. |

The last three exist because each named a divergence the reachability tier
could not see, and each divergence was a *model* one rather than a missing
screen.

**Pause-menu open.** Retail gates the root list's last two rows on two
scene-scoped values - the op-`0x49` entry context and the MAN header's
save-allow bit - and suspends field dispatch while the menu owns the frame. A
host that opens the menu without sampling them into a `FieldMenuGate` draws
every row white and opens every row, which lets a player Save in one of the
scenes whose header forbids it (see [`save-screen.md`](../subsystems/save-screen.md)).
Both open sites call the same builder, so tier 1 was green throughout.

**Menu-open precondition.** *Whether the menu opens at all* is the same kind of
invisible split, so it is a row of its own: every host that turns a Start edge
into an open menu must ask `World::field_menu_open_allowed` rather than spell
the test out locally. That predicate carries both halves - the scene mode, and
the locomotion controller's engaged bit, which is why Start is inert while the
player is talking (see
[`field-menu.md`](../subsystems/field-menu.md#the-menu-does-not-open-at-all-while-a-dialogue-is-up)).
Three hosts each wrote their own copy and all three said `mode == Field`, which
is how the overworld lost the pause menu: retail runs one locomotion controller
across the field and the kingdom overworlds, and the port splits that one retail
mode into `Field` + `WorldMap`.

**Party wipe.** The pairing is on the *routing* sites, not the draw sites,
because retail's wipe arm has exactly one exit store - so a host that offers the
player a row here has invented a destination. The panel that used to sit in this
slot was exactly that, drawn from a pinned literal pair on one host and from a
live cursor on the other: two pictures of a menu retail never shows.

**Play clock.** The H:MM:SS box reads `World::clock.play_time_seconds`, and that
counter only moves if a host drives `advance_play_time`. Substituting a frame
count at the *draw* site looks identical on screen and is not: the save writes
the world's counter, so a save taken from a host that never advanced it
records the play time it was loaded with.

## Tier 4 - trait-override symmetry

[`scripts/ci/check-trait-override-symmetry.py`](../../scripts/ci/check-trait-override-symmetry.py),
with waivers in
[`scripts/ci/trait-override-waivers.toml`](../../scripts/ci/trait-override-waivers.toml).

`engine-core` hands hosts their behaviour through traits, and several give
every method a default body - `BgmDirector`, in
`crates/engine-core/src/scene/host.rs`, is the one to look at. That is a
deliberate convenience (a test stub can implement it with an empty block) and
a silent failure mode: a host that never types `start_owned_vab` loses every
global-pool track, which is every real music cue, with no compile error and
nothing in the diff.

The rule: **for every `engine-core` trait with default method bodies that more
than one host implements, the set of overridden methods must match.** Pure
syntax, no call graph. Comparison is pairwise over every implementer, so
intra-host asymmetry is a finding too - which is how the `audio-trace` parity
oracle's missing owned-VAB hooks surfaced.

Proves that a defaulted hook one implementer overrides is overridden by all of
them. Does not prove the overrides *do* the same thing, nor say anything about
a hook every host leaves defaulted - that is a host-identical gap, not drift,
and reachability of it is the [port catalog's](port-catalog.md) question.

## Tier 5 - input: does any page carry its own keyboard table?

`check_page_key_tables` in the same script.

The three hosts share one keyboard layout, served out of the engine by
`pad_bindings_json`
([`legaia_engine_core::input::Mapping::web_default`](../../crates/engine-core/src/input.rs)),
and the whole point of serving it is that a page cannot write a second one
down. A page that does write one down never looks like a table: it looks like
a `switch` on `e.key`, or an object literal indexed by `e.key.toLowerCase()`.
The minigames page carried exactly that, binding `A` / `S` / `D` to the three
face buttons while the engine binds them to Left / Down / Right, and printing
labels that said so - so the page and the engine contradicted each other key
for key on the same three buttons, and a rebind reached neither.

The rule, on **pad-driving** sources only (a `site/` file that reaches an
engine input entry point - `main.js` closing a dialog on Escape is ordinary
web UI and out of scope):

- `KeyboardEvent.key` may not be read at all. It is the layout-dependent
  character property; the engine's table is keyed by `code`, so a `key`
  comparison cannot be reconciled with a binding even in principle.
- `KeyboardEvent.code` may be compared to a literal only for a key the PSX pad
  has no button for (`NON_PAD_CODES` in the gate). Those cannot contradict a
  binding - the pad has no Escape.
- A **set membership test** on a bindable code - `held.has('Enter')`,
  `pulse.includes('Space')` - is the same defect in a third shape. The literal
  never touches `event.code`, so the two rules above are blind to it. The
  bindable set is parsed from `KEY_NAME_DOM_CODES` in the engine rather than
  restated here, so the gate cannot drift from the table it polices.

That third rule exists because the first two passed a live bug. The play page
decided whether Start was pressed with `p.has('Enter')`, so binding Start to
Space bound it on both hosts and in the engine's served table - and in every
handler except the one that opens the pause menu. The page read the engine's
table correctly and then dispatched on a key name anyway.

The fix a finding asks for is always the same: resolve the code through
`legaiaPadButtonOf` and dispatch on the pad **button**. Printed key labels go
the same way, through `legaiaPadKeysFor`, so a rebind relabels the page rather
than making it lie. Both live in
[`site/js/pad-bindings.js`](../../site/js/pad-bindings.js), the one place any
page adopts the engine's table.

There is deliberately **no waiver file** for this tier. A waiver names a
blocking capability, and no capability is missing here: the table is exported,
the adoption helper ships, and the answer is always to type the lookup.

## Tier 6 - diagnostics: is a debug draw off on BOTH hosts?

Every tier above asks whether a host **reaches** a surface. None can see the
shape where both hosts reach it and only one of them turns it off.

That shipped, and a user reported it. The effect billboards carry a tinted
wireframe outline so a spawn stays readable when its texels are not resident.
The native window gates it behind `LEGAIA_DIAG_FX=1`; the browser twin had no
gate at all. Retail draws no such rectangle, so every play-page fight stamped
an opaque red-ish box around every effect sprite - up to 25 at once. Both hosts
called the builder, the constants matched, the sim pairs matched, no page
carried a key table: **all five tiers above passed.**

`DIAG_GATES` in [`check-ui-host-drift.py`](../../scripts/ci/check-ui-host-drift.py)
declares every `LEGAIA_DIAG_*` gate in the engine crates and whether it is
`additive` - whether it draws something retail does not. The asymmetry is the
whole point:

| kind | example | a host missing it |
|---|---|---|
| subtractive | `NOFX` (suppress the layer), `NOSEMI` (blend off), `LAYERS` / `PLACE_RANGE` (draw a subset) | renders retail-correctly; it just cannot bisect |
| additive | `FX` (outline strips), `HUD` (diagnostic text over the frame) | **paints it in normal play, for every user** |

Only additive gates require a twin. A WASM module has no process environment,
so the browser twin is a module static a page or devtools console flips - which
is why this cannot be checked by looking for the env name on both sides. The
check is that the twin's *initialiser* reads false.

Validated both ways, like the waiver files: an undeclared `LEGAIA_DIAG_*` fails
(declare what it draws), and a declared gate that no longer appears fails (drop
the row). What it does **not** prove: that the two gates suppress the same
draw, that the twin is wired to anything, or that no un-gated debug draw exists
under some other name.

One gate declares `web_toggle = None` with a reason rather than a twin:
`LEGAIA_DIAG_HUD` reads its env inside the shared `engine-ui` leaf, so it is
one implementation both hosts call, and `std::env::var` answers `Err` under
`wasm32` - default-off by construction rather than by a second toggle.

## Tier 7 - render kernels: same draw list, same kernel, every surface

`RENDER_KERNEL_RULES` in
[`check-ui-host-drift.py`](../../scripts/ci/check-ui-host-drift.py).

Every tier above measures a UI screen, a constant, a simulation injection
site, a trait hook or a keyboard table. None of them asks the question that has
shipped every bug in the table below: two surfaces assemble the same kind of
draw list and only one runs the kernel that makes it correct.

| what shipped | who missed it |
|---|---|
| coplanar draw lifts never computed | browser play page |
| hand-rolled white vertex streams | Muscle Dome's three bodies |
| synthetic Lambert on both shader paths | every site 3D page |
| ground heightfield left out of the coplanar soup | every host |
| occlusion-fade radius staged at a different value | browser play page |

The last one is tier 2's; the other four are this tier's, and each was
invisible in a diff because no file held two of the columns.

### Why this is not another `SIM_PAIRS` row

The denominator. A tier-3 row names **two** function bodies by hand, so a
third surface that grows the same draw list is outside the measurement by
construction - and this tree has **five** render surfaces, not two:

| surface | assembles |
|---|---|
| native `play-window` | `field_render.rs` + `geometry.rs` |
| browser play page | `play.rs` |
| browser field-scene viewer | `field_scene.rs` (+ `scene_geom.rs` for the world map) |
| browser dance-hall venue | `minigames_dance.rs` |
| browser fishing venue | `minigames_fishing_scene.rs` |

The last two resolve `EnvDraw`s and instance env-pack meshes exactly like the
first three, and the tier-3 coplanar rows named three of the five. So here the
surface is **derived**: every non-test source under the render roots. A new
surface joins the measurement by existing.

### The two rule kinds

| kind | assertion |
|---|---|
| `requires` | a file whose comment-stripped source matches `trigger` must also match every `requires` pattern |
| `forbids` | a file matching `trigger` may not match `forbids` inside any 3-line window |

The 3-line window is the statement scale: these kernels are written as
`self.flat` / newline / `.extend(...)` as often as on one line, and a
line-scoped detector misses the multi-line half. Comments are stripped first,
in both languages, for the same reason tier 1 strips them - a doc comment
naming `coplanar_draw_offsets` is prose, not a wiring, and under-counting
"satisfied" is the safe direction.

### Rules on the surface today

| kernel | rule |
|---|---|
| cross-draw coplanar lifts | resolving `EnvDraw`s requires `draw_plane_summaries` + `coplanar_draw_offsets` |
| walk-ground heightfield sink | emitting the heightfield's vertices requires `GROUND_SINK` |
| packet-colour stream fill | a packet-colour stream may not be filled with white |
| placement tilt composition | reading `placement_rot_y` requires `rot_x` + `rot_z` |
| shared value layout -> shared quad emitter | resolving `battle_value_readout` requires `battle_numerals` + one of its prim builders |
| world-map markers through the shared quad kernel | reading the marker seams (`world_map_entity_markers` / `world_map_player_marker`) or `marker_quads` requires `marker_quads` + `world_map_marker_prim` |
| field attached lights through the shared prim kernel | reading `field_light_draws` requires `light_pool_prims` |
| retained field ground pass gated off in battle | a file that both uploads a field ground (`uploadGround`) and drives a battle frame (`play_battle_active`) requires `setGroundEnable(false)` |

**The ground-pass rule is about a retained pass, which no draw list shows.**
The WebGL renderer draws `uploadGround`'s mesh inside `renderAssembled`
before the placements, so a draw-list bisect that filters every battle draw
out still paints it. Its trigger is an `\A`-anchored lookahead over both
halves (the uploader and the battle driver) so one scan decides it; the
unanchored form re-scans the file from every position.

**The heightfield rule triggers on the emitter, not the type.** An early draft
keyed on `WalkHeightfield` / `walk_heightfield` and reported four files that
only pass the struct on - one of them a log line reading `hf.positions.len()`.
The trigger is the vertex walk (`for p in &hf.positions`, `.clone()`), which is
what "this file is a render site for the ground" actually looks like.

**The white-fill rule is a prohibition because there is no legal case.** The
shader reads `a_flat_rgba` as `texel * rgb * 255/128`, so a fabricated stream
of white is `texel * 2`; the one correct fill for geometry with no colour word
is `MODULATION_NEUTRAL` (`0x80`). The rule has to distinguish that from the
*flag* byte, which is legitimately `255` on every textured vertex -
`[c[0], c[1], c[2], 255]` is correct and `[255u8; 4]` is not, and the control
suite pins both.

**A retired rule is worth one line: the fade quad.** The rule "resolving the
intro fade ramp requires `fade_prim`" existed while two surfaces each resolved
`intro_fade(...)` and could each hand-roll the packet. The whole transition
emission - fade included - is single-assembler now (`engine-ui`'s
`battle_intro`, ticked by both hosts), so the question the rule asked can no
longer be posed and the rule is deleted rather than left matching nothing.
See [the section below](#the-version-of-this-tier-that-needs-no-rule).

**A shared layout is not a shared draw.** The battle damage numerals and the
`N HIT` / `TOTAL` counter resolved through one kernel
(`engine-vm::battle_value_readout`) on both hosts - and the native window
sampled retail's 24x24 cells out of VRAM while the browser restyled the same
digits in the dialog font. Every tier saw one model, because both hosts named
the kernel; what differed was the record family the resolved cells became. The
rule closes the gap the layout kernel left: a surface that resolves the layout
must also reach `engine-ui::battle_numerals` and one of its prim builders, so a
font path is an explicit before-the-atlas fallback rather than the draw.

**The tilt rule earns its place on measured data, not on plausibility.** The
comment it replaced said "the handful of disc placements carrying a real X/Z
tilt". Over 49 field scenes that is 94 of 1667 placements - and `juui1` tilts
all nine of its by a quarter turn about X.

### `blocked_on` and `exempt`

`blocked_on` marks a divergence that is known and being closed elsewhere, and
is validated in both directions exactly like a waiver: an entry whose file has
gone clean **fails**, demanding the entry be deleted, and so does one whose
file no longer assembles that draw list at all.

`exempt` is the stronger claim - the rule does not apply to that file - and it
must rest on the **data**, not on the schedule. The two on the surface are the
world-map walk placements, whose records carry `rot_x = rot_z = 0` across the
retail corpus: the yaw-only path there is not a shortcut, it is what the disc
says.

### What tier 7 does not prove

It asks whether a file that assembles a draw list *names* the kernel. It says
nothing about the arguments, the order, or whether the call runs. A host can
name `coplanar_draw_offsets` and drop the map on the floor, and this tier will
report it clean - that is tier 3's shape of question, one function body down.
It also cannot see a kernel nobody has written a rule for; the rule list is
the claim, and it grows one defect at a time.

### The version of this tier that needs no rule

A rule is what you write when two surfaces each assemble the list. When there
is only **one** assembler, the question this tier asks cannot be posed - and
that is the cheaper fix wherever the surfaces are genuinely the same screen.

The pause menu went that way. Its draw-list assembly used to live twice: in
`engine-shell`'s `window/menu_draws.rs` + `window/title_save_draws.rs` and in
`web-viewer`'s `play_menu.rs`. Two of the divergences that had already grown
there are exactly this tier's shape - one host resolved a title tab through
the descriptor painter and the other through a pinned-pen label builder, and
the Items screen's Use-route confirm framed before the screen's window set on
one host and after it on the other. Neither is expressible as a `requires`
rule, because both are *ordering and choice* inside one assembly rather than a
kernel a file does or does not name.

The assembly now lives once, in `engine-ui::pause_menu`, with each host
resolving its own rects and projecting its own model into the shared view
structs. Two `CONSTANT_PAIRS` rows retired with it (see
[tier 2](#a-deleted-pair-is-not-always-lost-coverage)), and the composition
became reachable from a library test - `engine-ui/tests/pause_menu_compose.rs`
- which it could not be while it sat inside a binary's private module, since
a `tests/` target cannot import one.

The dance hall has a native twin of the browser venue surface: while a dance
runs, `window/minigames.rs` draws the same venue in place of the walked-in
scene. Both take the draw list, the coplanar lifts, the VRAM (face stamps and
HUD page included) and the camera from one kernel,
`engine-core::dance_venue::DanceVenue::build`, so only the mesh build and the
instance transform are per host.

The reason this is not the answer everywhere: it needs the two surfaces to be
the same screen with the same model. The five *render* surfaces above are not
- the dance hall and the fishing venue bake venue-specific geometry - so there
the rule is the instrument, and this is not a plan to replace it.

## Tier 8 - ownership: does the host hold the engine type, or only read it?

`OWNED_TYPES` in
[`check-ui-host-drift.py`](../../scripts/ci/check-ui-host-drift.py).

Every tier above asks whether a host **reaches** something: a builder, a
constant, a kernel, a gate, a hook. None can see a host that reaches an engine
type's *outputs* while holding none of the type - there is no builder to miss,
no constant to pair, and an injection site that does not exist cannot diverge
from one that does.

The camera is the worked case and it is
[below](#gaps-the-tiers-were-blind-to-closed-by-reading-the-two-hosts-side-by-side):
one absence produced a projection difference, a simulation difference and two
missing screens at once, with all seven tiers green.

Ownership is a **field declaration or a construction** in that host's own
shipped source. Holding the session (`BootSession`, a field, a construction or
a type alias of it) owns every type the session's own fields hold: both hosts
reach the scene host and the camera through their session, so they own them
through it. The three things that are not ownership are the three the page
had: a `use` line, a match arm, and a borrowed parameter. Two shapes look like
constructions and are not - `-> Camera {` is a return type and
`impl Trait for Camera {` is an impl block - and the control suite pins every
one of these directions, because a detector that accepts a `use` line reports
every type as owned by everybody.

Declared rather than derived, for the same reason tiers 2 and 3 are: "which
engine types must a host own" is a judgement about the architecture. The
derived version of the question - every `engine-core` type one host constructs
and the other only names - reports 26 rows over this tree, and reading them is
what settles it: they are types **both** hosts use, where one happens to name a
constructor (`PadButton::from_name`) or to hold a session the other reaches
through its runtime. None is a Camera-shaped absence, and a tier that failed on
all 26 would be asserting 26 architectural claims nobody made. A row here is a
pinned juncture and does not claim to be a census.

Scope: it proves each named type is constructed or held by both hosts, and that
the type still exists. It proves nothing about how either host drives it.

## Tier 9 - variant coverage: does each host answer every variant?

`ENUM_COVERAGE` in the same file.

A shared enum's variant can be **entered on both hosts and answered on one**.
Four minigame `SceneMode`s shipped that way: the shared scene host drains the
mode-24 door warp for either host, and the browser landed in each with a frozen
field and no screen, because their native presentation is text lines and a
hand-rolled 3D scene rather than an `engine-ui` builder tier 1 enumerates.

The variant list is **derived from the enum's own source**, so a variant added
tomorrow is measured; what is declared is the enum and the per-(variant, host)
waivers. Both waiver directions are validated: a waiver for a variant the host
now names fails, and so does one naming a variant or host that does not exist.

A host "answers" a variant by naming it **qualified** (`SceneMode::Fishing`).
The qualification is the whole rule - an unqualified `Fishing` matches a
session type, a module name and half the fishing HUD, and a detector that
accepted it would report every host as covering everything.

Scope: it proves each host's shipped code mentions the variant. It does not
prove the arm draws anything, that two arms agree, or that the variant is
reachable at runtime.

## Tier 10 - entry symmetry: is the phase armed only from a debug key?

`HOTKEY_SOURCES` / `HOTKEY_ONLY_WAIVERS` in the same file.

A gap that reads as "one host has it" can turn out to be "one host's *debug
path* has it", and no tier above can tell those apart because both end at a
live call site. The dance count-in is the case: a complete shared kernel the
native window drove from a count-in driver of its own, reached only by the `K`
/ `U` hotkeys, so **neither** host counted in when a player walked into the
hall.

For every `world.<method>()` the native key arms call, this asks whether any
call site exists outside `crates/engine-shell/src/bin/` - in the shared engine
crates, or on the browser hosts. Derived over the hotkey sources, so a new
hotkey arm joins the measurement by existing.

The call test matches `.method(` / `::method(` rather than a bare name, because
a bare match counts the method's own `pub fn` as its caller and makes the tier
vacuous. `#[cfg(test)]` bodies and `tests/` files are out of the scan for the
same reason they are out of tier 1: a unit test is not a host.

### What a hotkey waiver may say, and the distinction it has to keep

Two different answers put a row here, and only one of them is work:

- **a debug probe**, with no retail counterpart and no business on a
  player-reachable entry. The effect-pool marker spawners are this: they exist
  so the pool's ageing can be watched.
- **a superseded stand-in**, where the production route moved to another
  mechanism. `World::spawn_field_stager` stages an ambient record into the
  older `SummonScene` pool while the op-`0x34` sub-3 route runs
  `World::spawn_ambient_record_at` (the full `FUN_80021B04` port) on both
  hosts; `World::spawn_summon` parses a stager overlay while the battle cast
  band produces `casting.active_summon` through `SummonScene::spawn_parts`.

Both take a waiver naming the ARM or the mechanism. What neither may say is
"not wired yet" - that is the third answer, a phase whose shared caller is
genuinely **missing**, and it belongs in `HOTKEY_ONLY_BLOCKED`, which is
validated like every other pending marker here: an entry that goes clean fails.
The dict ships empty, because both rows drafted for it turned out to have a
live replacement. "The shared caller is elsewhere" is not "the shared caller is
missing", and collapsing the two is how a superseded probe acquires a wiring
task nobody owes.

## Tier 11 - frame path: does an early exit skip a kernel the frame still draws?

`FRAME_PATHS` / `WEB_PAGE_FRAME` in the same file, `[[frame_arm]]` and
`[[frame_kernel]]` rows in `scripts/ci/ui-host-drift-waivers.toml`.

Every tier above measures what a host **has**: a builder it reaches, a
constant it declares, a kernel it names, a type it owns, a variant it answers,
an entry it arms. This one measures what a host **runs this frame**, and the
difference is a failure class none of them can see.

Both hosts drive the engine through one frame path - the native window's
redraw tick loop, the browser runtime's `tick_frame` - and both paths
short-circuit. The native loop `continue`s out of five arms; the browser
runtime `return`s out of three; the page's own `_frame` gates the whole call
to `tick_frame` behind a fourth kind of arm in JavaScript. The **draw** does
not short-circuit with them: the native draw passes run after the loop
whatever an iteration did, and `tick_frame` returns to a page that draws
either way. So an arm that skips a per-frame kernel does not skip the draw
that reads that kernel's answer, and the host paints last frame's decision for
as long as the arm is taken.

The case it was written for is the field party readout. `FUN_801D0D38` reads
its suppress global at its first instruction and jumps to its epilogue when it
is set; the port splits that into a decision kernel stepped once per tick and
a draw pass that reads the decision back. The native window's boot-UI arm
`continue`d **before** the step, so on every pause-menu frame the kernel never
saw the state its predicate was written to suppress on, and the readout stayed
painted under the menu. Tiers 1-10 were green throughout: no builder was
missing (both hosts call the same one), no constant was paired, no injection
site diverged (both hosts *have* the call), no render kernel was absent, no
type unowned, no variant unanswered, no entry hotkey-only.

### The two questions, and the third source the first one needs

1. **Completeness.** For each early exit, the fall-through kernels that come
   after it are the ones that arm skips. Each must either be called inside the
   arm's own block, or be listed in that arm's `[[frame_arm]]` waiver with a
   reason - which is how "this arm deliberately freezes the world" gets
   written down once instead of re-derived per reader.
2. **Pairing.** Each host's frame path is a list of per-frame kernels, and a
   step one host takes every frame while the other never does is a simulation
   the two hosts do not share. Names differ across the two crates, so a
   `[[frame_kernel]]` alias row pairs `tick_minigame_extras` with
   `tick_minigame_ui`, and a `host_only` row declares the ones that really are
   one host's - with a reason that says *where the other host does the same
   work*. The claim a `host_only` row makes is about the frame path, which is
   what the checker measures, not about a capability.

The browser's frame path is **two** files, and the Rust half is the one
without the interesting arm. `site/js/play-app.js::_frame` calls
`rt.tick_frame()` inside `if (advance && !menuOpen && !shopOpen &&
!namingOpen)` and calls `this._drawOverlay()` outside it, so a guarded frame
runs no kernel at all and still draws - all nineteen at once. A Rust-only scan
sees none of that, which is how "the browser page has no early-out" came to be
written down while the page carried the same defect the native window did. The
tier therefore walks the JS function too, takes the guards `tick_frame` sits
inside and the draw does not, and treats each as an arm of the browser host
whose skip list is the whole Rust path.

### What the tier does not prove

That a kernel an arm runs runs *correctly* there, that the draw pass reads
what the kernel wrote, or that two paired kernels do the same thing - that
last one is tier 3's question, asked of hand-named pairs. It proves only that
no arm silently drops a step the fall-through path takes, and that neither
host's per-frame list has grown a member the other's has not.

It also covers two of the three hosts, and for a structural reason rather than
an oversight: the minigames page has no frame path to walk. It exports one
tick per minigame (`dance_tick`, `baka_tick`, `slot_step`, `fishing_pond_tick`,
`muscle_tick_time_meter`), each called by its own page module, so there is no
fall-through list for an arm to skip part of. A shared frame path is the thing
this tier measures; that host does not have one.

What it does share is the clock. The page's one animation loop asks the
engine's `frame_step::SimStepper` (`LegaiaMinigames::drain_sim_steps`, the
play page's `play_drain_sim_steps` and the native redraw's drain) how many
60 Hz game frames each display frame runs. Fishing takes the count as its
step budget (its frame function steps the pond in a loop and draws once); the
other four run their frame function at most once, and not at all on a frame
the stepper answers `0` - each of them draws inside it, so a catch-up frame
would be an extra 3D or raster pass, and a slow machine plays them in slow
motion rather than spiralling. The page used to step every game once per
`requestAnimationFrame` - and fishing rounded its own wall-clock gap up to at
least one frame - so on a 120 Hz display every minigame on the page ran at
twice retail speed.

Both halves are derived from the sources rather than declared, so a new arm or
a new kernel joins the measurement by existing. The ratchet is the `skips`
list on each waiver row: one that no longer matches is stale and fails, and a
kernel added to the fall-through path lands in no arm's list and fails on
every arm at once - which is exactly the moment to decide, per arm, whether
the new step belongs there.

### The fix a disclosure does not replace

An arm's waiver says the frame may draw without those kernels. It does not
say the frame may draw their *stale answers*, and for a decision kernel the
two are different claims. The page's guard is the page's field freeze and
cannot move; what moved instead is the reader. Both hosts' field-party-readout
and passive-badge draw builders now ask the suppress gate on the **draw** path
as well as on the tick, so a decision that went stale on a guarded frame
cannot reach the screen whichever host skipped the step.

The predicate they ask is one kernel,
`legaia_engine_core::world_map_panel_host::field_hud_suppressed`. It used to be
an enumeration spelled out in each host, and the copies had drifted: the page's
was missing all three of the window-side terms, and the native window gated the
badge column on a different test again - which put the badges over every dialog
box, every cutscene beat and every fight. Retail reaches the badge routine
`FUN_801d095c` from exactly one place, `FUN_801D0D38`'s `jal` at `0x801D130C`,
eight bytes above the epilogue the suppress arm jumps to, so the badges carry
the readout's suppression exactly (see `ghidra/scripts/funcs/overlay_0897_801d0d38.txt`).
Each host now answers only the term the world cannot: whether a host-side panel
with no `World` state behind it owns the frame.

## Tier 12 - content: do two paired kernels call the same engine?

`check_frame_content` / `frame_kernel_pairs` in the same file,
`[[frame_content]]` rows in `scripts/ci/ui-host-drift-waivers.toml`.

Tier 11 pairs a frame path's kernels by **name** (or by an alias row). That
answers "does each host take this step", and it is silent on what the step
does on each side - which is exactly the question the pairing invites a reader
to assume it answered. The case that proves the point was on the surface the
whole time: the native window's `tick_field_prop_anims` was an **empty body**,
`pub(super) fn tick_field_prop_anims(&mut self) {}`, aliased to the browser's
`drive_npc_clips`, which drains the op-`0x4B` ANIMATE cues, advances every NPC
clip player and drains the player move cues. A `{}` body pairs perfectly with
anything, and the alias row's reason asserted that both sides "advance the
scene's posed actors" - a sentence no line of native code supported. The
window does that work; it does it inline in its own tick loop, which is a
different claim, and the one the row made next. Both hosts now name the
step (`drain_anim_cues` natively) and both bodies call the one engine kernel
`World::drain_field_anim_cues`; the `[[frame_content]]` row carries the one
remaining difference, the page advancing its clip players inside the step.

This tier asks the next question with the only evidence a source scan carries:
for each paired kernel, the set of **engine functions** each host's body
reaches. Engine means the six wgpu-free crates both hosts link
(`engine-core`, `engine-vm`, `engine-ui`, `engine-audio`, `engine-session`,
`engine-screens`). A host's own
helpers are followed transitively, so a step spelled as five private methods
is compared against a twin that inlines them.

### What it proves, and the blind spot it is built on

It proves that two paired bodies reach the same engine surface, or that the
difference is written down with a reason and both lists. It does **not** prove
they call it with the same arguments, in the same order, or under the same
guard - those are tier 3's question and the side-by-side audit's, and a
difference this tier cannot see is not evidence of agreement.

The join is by name, and that carries one deliberate hole: nothing here can
tell `world.clear()` from `Vec::clear()`. Names that are also ordinary std /
collection / iterator methods are therefore excluded wholesale
(`STD_METHOD_NAMES`), so a genuinely divergent engine call spelled `insert`
is invisible. Stating the hole is the point - the alternative is a report
where two thirds of every row is `len`, which is a report nobody reads.

### The ratchet is the difference, not the pair

A `[[frame_content]]` row carries the exact `native_only` / `web_only` lists
it was written against. When the difference moves - a call added to either
side, or one half closed - the row goes stale and fails, which is what stops a
content waiver from outliving the thing it described. A row is not a licence
for the difference; it is a statement of **where the other host does the same
work**, in the form the waiver rules above require.

## Tier 14 - save routing: does every save go through the retail screen?

`check_save_io_routes` / `SAVE_IO_ROUTES` / `PAGE_CARD_RACK_RULES` in the
same file.

Retail moves save bytes in one place, the card driver behind the save screen
([`save-screen.md`](../subsystems/save-screen.md)). Each host owns the bytes
behind that screen - the native save directory and `--card` image, the
page's card rack - and one **commit applier** that turns the screen's
`SaveCommit` into I/O (`apply_save_commit` / `apply_card_save_commit`
natively, `apply_card_outcome` on the page), plus the write a Save's commit
beat asks for (`write_save_commit` / `service_card_save`). The tier pins every shipped
call of each rack primitive (`write_slot_save`, `read_slot_save`,
`write_save_into_card`, `MountedCard::save_at`, the page's
`write_session_into_card` / `load_session_from_card`) to its applier, so a
hotkey or page button that saves or loads around the screen fails as `SAVE
BYPASS`. A primitive with no shipped call at all fails too: a renamed applier
must not leave the tier checking nothing.

The page half is a text check on `site/_content/play.html`: port 1 starts
with the browser card, the card is formatted by the engine
(`formatted_memory_card`), and both the disc load and a trap recovery
remount the rack. The native window's port 1 is its save directory
whatever the player does; without those three the page's Save screen opened
on two empty ports.

Not covered: the page's save bar imports a `.lgsf` or a card block and
resumes it directly. That is page chrome for getting a save into the browser
at all - the native twin is `legaia-engine load` - and it moves no bytes
into a card.

## What a screenshot pair adds to a green row, and what it does not

Every tier here is a source measurement, and a row closed by one is a claim
about code, not about pixels. The two hosts can be paired by a gate, reach a
kernel by a ladder, and still put different frames on screen - so a closed
row deserves a pair of pictures at the same state at least once.

The recipe, because it took several wrong turns to find:

| host | driver |
|---|---|
| browser play page | headless Chromium through `playwright-core`, against `python3 -m http.server` over `site/`, with `--use-angle=swiftshader`; the page's `window.__playRuntime` / `window.__playState` hooks say what state the frame is in, and `#play-canvas` is the element to screenshot |
| native window | `legaia-engine play-window --screenshot <png> --screenshot-tick N`, with `--pad-script` / `--key-script` for input - an offscreen readback, not a screen-scrape |

Three traps, each of which cost a run:

- **`--pad-script` cannot confirm a boot-title row.** `Down` moves the title
  cursor; neither `Cross` nor `Start` confirms it. The title's confirm lives
  in the keyboard handler, so the entry is `--key-script "<tick>:Z"` - the
  same split the flag's own help warns about for the minigame entries.
- **A multi-save `.mcr` does not insert itself.** The page's import raises a
  block picker whose buttons live in `#save-status`, and until one is
  clicked no `kind: "card"` session exists, the card rack stays hidden and
  `boot_title_has_save_data()` stays false - so the title's CONTINUE row is
  dim and the boot save-select is unreachable. That is the page working as
  designed, not a defect; a driver that clicks the rack instead measures a
  dim row and concludes wrongly.
- **Two hosts at the same tick are not at the same frame.** The window is
  960x699 on this display where the page canvas is 960x720, so the vertical
  field of view differs and the native framing reads as "closer". A framing
  difference between the two is not evidence of a camera drift unless the
  surfaces match: both hosts read the same
  `options_state.camera_distance` (`window/run.rs`, `runtime.rs`), so that
  knob is not the difference.
- **The same knob is not the same value.** Each host *persists* the
  camera-distance preset on its own: the window in `legaia-options.toml` in
  its working directory (a `T` in one run's `--key-script` is still in force
  in the next), the page in `localStorage`. Pin it on both before a pair -
  on the overworld it decides how much of the far continent sits inside the
  white haze. A "the page washes the castle and far mountains out, the
  window draws them" report on `map01` dissolved this way: with the preset
  and the tick pinned, the two hosts' frames match, haze included, through
  hundreds of idle ticks while the haze builds.

### What the pairs showed

Four states captured on both hosts: a field scene at spawn, a battle at its
command phase, the boot title card and the boot save-select.

Confirmed matching, for rows that until now were only test-asserted: the
battle's sky clear colour and stage (B3), the command ring and its two
labels, the party HUD block's content and pen, the title card's bands and
its glyph-atlas menu rows, and the field scene's geometry, textures and
player placement.

One row the pair made visible: **C11, the dimmed title art behind the boot
save-select.** The native draws the Load frame and the SLOT pills over the
title card at reduced brightness; the page used to draw them over black,
because it dropped its title session on `TitleOutcome::Continue`. The page now
parks that session as the backdrop (`boot_title_backdrop_draws_json` in
`crates/web-viewer/src/boot_title.rs`, held to the native output by
`title_backdrop_parity`), so both hosts compose the screen the same way.

The yellow strips the pair raised - in the battle command phase the page
carried a run of **flat yellow strips** along the ground and a grass-and-gravel
patch around the monster that the native frame does not - are the page's
**field ground heightfield**, not a battle draw. `renderAssembled` in
`site/js/webgl-tmd.js` draws the ground uploaded by `uploadGround` as a
*retained* pass ahead of the placements, whatever the frame's draw list holds,
and `_rebuild` uploads the field scene's ground cells there; the battle frame
never turned that pass off, so town01's terrain drew through the battle camera,
sampling the battle VRAM. The bisect that pins it: with the whole battle draw
list filtered out of `renderAssembled`, the strips and the patch still draw.
The native window draws its heightfield only in the non-battle branch of
`window/event_handler/redraw.rs`, and a retail command-phase frame of the same
stage (`v0_1_battle_command_menu`, `mednafen-state vram-dump --display-crop`)
shows bare gravel. The page now turns the pass off while a battle draws and
back on for every non-battle frame; the billboard-outline candidate the first
reading suspected (`play_battle_fx.rs`, alpha `0` as the untextured flag) was
off on both hosts throughout and draws nothing in this frame. Tier 7 carries
the rule, **retained field ground pass gated off in battle**: a render
surface that uploads a field ground and drives a battle frame through the
same renderer must reach `setGroundEnable(false)`.

### A second pass: frames matched by engine frame, with retail as the third

A later audit shot the non-battle screens on both hosts from one starting
point and, where a mednafen library state holds the same screen, cut the
retail display crop (`mednafen-state vram-dump --display-crop`) as the third
frame. Four recipe additions made the pairs comparable:

- **Match by engine frame, not by wall clock.** Headless Chromium over
  SwiftShader runs the page at a few frames a second, so a timed wait lands
  on a different beat each run. The page's `window.__playState.frame` counts
  the same sim ticks the native `--screenshot-tick` names, so a driver waits
  for a frame number and the two hosts meet on one beat.
- **Clip to `#play-canvas`, at a device scale that makes it 960 wide.** The
  canvas displays at about 655 CSS pixels in the page layout; a screenshot at
  that size resamples every glyph, and colour and position diffs read as
  host differences.
- **The page's keys are the web layout.** `Mapping::web_default` puts Circle
  on `X` and the d-pad on `WASD`, so a driver pressing `S` for Circle walks
  the cursor down a menu instead of backing out of it.
- **The native stage is integer-scaled.** At the runner's 960x699 window the
  stage is 2x and centred (the `stage_transform` floor), so native frames
  compare against page frames only after cropping the stage out and halving
  it. The 3D pass draws inside that stage rect too, so the crop holds the
  whole picture.

What the pass fixed, each asserted through a host entry and re-shot on a
rebuilt bundle:

| row | shape | fix |
|---|---|---|
| prologue floor culled on the page | a kernel one host ran inline | the heightfield's triangle reversal moved into `engine-core::field_ground`; the page uploaded the builder's raw winding, which is the facing the cutscene camera's NCLIP pass discards |
| sky and backdrop shells skipped on the page | a filter one host added | the page dropped every draw the full-map viewer's sky classifier matched; the opdeene crater shell is one, so the prologue tableau stood in a navy void |
| fishing exchange drawn on one host | a hand-off inside one call | see [the exchange section](#the-fishing-point-exchange-sub-screen-one-session-on-two-hosts-a-query-on-the-third) |
| minigame status rows in surface pixels natively | a pen read in the wrong space | the native window now scales them through the stage the page uses |
| field clear colour | two non-retail constants | both hosts read `battle_stage_clear::scene_clear` every frame, and a field frame clears to retail black |

The pass also left two rows open. The first has since closed:

- **Field fog sheets were not visible in native frames** - two scene-VRAM
  builders, one missing an upload. The page draws from the host's
  `SceneResources`, whose field entry adds the PROT 0874 section-2
  effect-texture pool; the native window builds its own scene VRAM and did
  not, so the fog quads (texture page `0x0027` = `(448, 0)`, CLUT
  `(0, 473)`) sampled zero words and every fragment was discarded. Retail
  holds that pool resident in field VRAM (nine PCSX-Redux states, read with
  `scripts/pcsx-redux/extract_vram_from_sstate.py` - the PCSX-Redux side
  does have a VRAM reader). The window now layers the pool under its build;
  see [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#where-the-fog-texels-come-from).
  The shape to look for: a texture upload one host's resource builder makes
  and the other's does not reads as "the draw is broken", because the quads
  are there and only the texels are missing.
- **The native minigame hotkeys opened sessions, not scenes.** `O` and `B`
  ran the slot machine and Baka Fighter over the field with status text only,
  while the standalone minigames page drew the cabinet and the arena. Both
  have since closed on the native side: the duel draws the engine's
  `baka_duel_scene` surface, and the slot machine draws its own cabinet scene
  against its art-pack VRAM (`engine-ui::ui_slot_cabinet`, see
  [`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md#who-draws-the-machine)),
  the same builder the play page's screen-prim pass and the standalone
  page's CPU rasteriser now draw it with.

That pass also named a third: 3D that has to line up with the 2D stage did
not, at a window size that is not a stage multiple (the naming screen's actor
larger and further left than its windows, the field party HUD floating),
because the native 3D pass filled the window while the stage is integer-scaled
and centred. The native window now hands the renderer its stage rect
(`Renderer::set_scene_viewport`, `scene_viewport_for`): the 3D pass and the
screen-primitive overlay draw inside it at the stage's 4:3, while text and
sprites keep the whole surface they are already positioned in. The page's
canvas is a stage, so both hosts draw 3D into the same frame. A window smaller
than one stage keeps the whole surface. A frame captured into VRAM (the
battle-intro field capture) is cropped to the same rect, since retail's
framebuffer is the display and not the letterbox around it.

## Gaps the tiers were blind to, closed by reading the two hosts side by side

One pass over both hosts, domain by domain, with every tier above green,
found the classes below. Each is recorded for the shape it has, because the
shape is what the next sweep should look for.

Four of them are now tiers rather than only prose: the missing controller is
tier 8, the unanswered `SceneMode` is tier 9, the phase only a debug launcher
ran is tier 10, and one layout kernel feeding two letterform families is the
fifth render rule. The rest below are still read-and-look shapes - "a queue only
a consumer empties" and "a typed channel narrowed at one host" are decidable
per instance and not from a source pattern, which is why they are written up
here and not gated.

**Same override set, different bodies (tier 4's declared blind spot, as a
worked example).** Two `BgmDirector` implementations, one per host, can
override the same six methods and still disagree inside them: a
duplicate-start guard that asks only for the id, where the other also asks
"unpaused, and a sequencer is live", never restarts a track that ended or was
paused, so the field music does not return after a battle; an unset sequencer
master volume plays 127 against 100; a `stop` that leaves the pause gate
closed silences the next start. The tier cannot see any of these; only the
paired bodies can. The shape is closed for BGM by having one body: both play
hosts run `engine-session`'s `AudioBgmDirector`.

**A `SceneMode` with no `engine-ui` builder is invisible to tier 1.** Four
minigame sessions install themselves on both hosts from a scene's own door
warp (`World::minigames.pending_warp`, drained by the shared scene host), and
the page landed in each with a frozen field and no screen, because their
native presentation is text lines and a hand-rolled 3D scene rather than a
builder the surface enumerates. The page now draws them; the shape to
remember is that a mode can be reached without a single builder being named.

**A queue only a consumer empties.** `World::pending_field_events` was drained
on the page solely by the BGM router, which hands every non-BGM event back -
so a browser session's queue grew with every camera beat, item grant and
dialog open, and with audio down it grew with every BGM op too. The native
window drains it every frame. No tier looks at what a host *fails to
consume*.

**Opposite defaults on one knob.** The three-probe wall footprint, solid NPC
bodies and live NPC motion were on unconditionally in the browser and
off by default natively (`--edge-collision` / `--solid-npcs` / `--live-npcs`
opt-ins). The same engine, the same scene, two different games in a town.
Native now defaults them on with `--no-*` opt-outs; a paired-defaults row
would have caught it and none exists.

**A typed channel narrowed at one host.** The web strike-SFX scheduler was
`u8` end to end, so every cast cue the engine emits above `0xFF` was dropped
by a `try_from` that looked like ordinary defensive code. The native
scheduler is `u16` and classifies. Width mismatches on a shared event type
are a diff-visible shape once named, and nothing had named it.

**A host with no controller at all.** The browser play page framed the field
with its own spherical orbit projection while the native window consumed
`engine_core::camera::Camera`. Read as "two projections" that is a rendering
difference; it was not. The page held **no `Camera`**, so nothing on that host
routed the op-`0x45` Configure beats into a controller, advanced the mover,
wrote the follow focus back into the retail camera globals
(`_DAT_80089118/20`), or reset them on scene entry - and the azimuth the page
fed the locomotion compass came from its own orbit yaw rather than from
`compass_azimuth_units`. The page also had no cutscene glide (that type sat in
the wgpu-linked renderer crate), so every `apply > 0` beat snapped, and no
overworld top-view camera. One controller-shaped absence produced a
projection difference, a simulation difference and two missing screens at
once; the shape to look for is a host that *reaches* an engine type's outputs
without ever *owning* the type.

The camera lives in two shared leaves now:
`legaia_engine_vm::psx_camera` (the retail GTE projection, the cutscene glide
and the frame convention) and `legaia_engine_core::camera_view` (which camera
owns this frame, and what its inputs are). Both hosts call
`resolve_field_camera` and upload `frame_vp`. The page keeps exactly one
camera of its own - the debug orbit vantage on `F3`, which the native window
has too - and it is an explicit override, not the default projection.

**Same camera, different controls.** That parity is about the *type*; the
**inputs** were a separate question, and nothing asks it. The page steered its
orbit with three knobs - drag yaw, drag pitch, wheel zoom - and the native
window had one, so its vantage could only be pitched by editing a constant and
zoomed in three preset steps. Worse, the knob both hosts *did* have ran at
different rates: `0.006` rad/px on the page against `0.008` natively, writing
the same `Camera::manual_orbit`, which the retail follow camera and the
movement compass both read. So the identical drag turned the view by different
amounts *and* remapped the d-pad differently, on a field the simulation
consumes - a control-feel divergence that no tier looks at, because both sides
are live call sites into the same engine field. The window now takes the
page's three knobs at the page's rates and clamps
(`window::camera::debug_orbit`). They are JS literals on the other side, so no
paired-constant row can bind them; the pairing is a test that quotes the
page's handlers by name.

The three gestures now steer the **engine** follow camera by default on both
hosts, and only the `F3` debug vantage keeps a host-local knob: horizontal drag
is `Camera::orbit_by`, vertical drag `Camera::tilt_by`, the wheel
`Camera::zoom_by`, and a double-click `Camera::reset_follow_knobs`
(`engine-core::camera::follow_knobs`). The setters carry the cutscene lock
themselves - a running timeline drops the gesture - so neither host can
re-derive the gate and get it wrong on one side, and the clamps are engine
constants rather than a JS literal and a Rust literal that happen to agree.

The general shape: a tier that pairs *types* or *call sites* says nothing
about the numbers feeding them, and an input rate is exactly the kind of
number nobody writes a constant pair for because it "only affects feel".

**Simulated and never drawn.** The page ticked the field move-VM effects
every frame and drew none of them, because its only FX draw call sat inside
the battle branch. Tier 7 asks whether a render surface names a kernel; it
does not ask whether a simulated layer has a draw site at all.

**A phase only a debug launcher ran.** The dance minigame's pre-song count-in
(`FUN_801cf470`'s below-10 states) and the Disco King how-to tutorial actor
were both complete shared kernels, and the native window drove them - from a
count-in *driver of its own*, reached only by the `K` / `U` hotkeys. The
player-reachable entry is the mode-24 door warp, which the shared scene host
drains straight into `World::enter_dance`, so **neither** host counted in when
a player walked into the hall. A gap that reads as "one host has it" can turn
out to be "one host's debug path has it", and the tiers cannot tell those
apart because both end at a live call site. The cure was to move the phase
into the world tick, where the entry point is shared: `World::enter_dance`
arms `minigames.dance_countin` and the dance tick holds `DanceGame::advance`
off until it clears. The same reading applies to the three in-world minigames
that load a track of their own (`MinigameSubId::bgm_id`) - the native window
started them from its hotkey launchers, and the door warp started none on
either host until the warp drain began queueing an op-`0x35` start.

**Same layout, different letterforms.** The battle's damage numerals and the
`N HIT` / `TOTAL` counter shared a layout kernel
(`engine-vm::battle_value_readout`) and every gate saw one model - while the
native window sampled retail's 24x24 cells out of VRAM and the page restyled
the same digits in the dialog font. A shared *layout* is not a shared *draw*:
the quads now come out of `engine-ui::battle_numerals` as `ScreenPrim`s and
both hosts push them through their own `screen_prim` pass, with the font
builders left as the explicit before-the-atlas fallback on each.

**One gap stated for "both hosts" was two different gaps.** The dance
count-in banner drew as placeholder letterforms on both hosts with its
geometry pinned to the instruction, and the shared statement - "no host stages
the dance page, so there is nothing for a textured quad to sample" - was true
of one host. The native window hosts the dance over the scene the player
walked in from and keeps drawing that scene behind the HUD, so it had neither
the page nor a quad emit. The browser play page already replaced its whole
VRAM texture with the hall's for the dance, and already drew the hall, so it
had the texels the whole time and lacked only the emit. A single sentence
covering both hosts hid a two-to-one difference in what each needed, and
fixing the expensive host fixed the cheap one on the way past without anybody
having to notice which was which. When a gap is phrased "neither host does
X", check what each host would need *separately* before believing the
symmetry.

The cure keeps the asymmetry where it is real and shares the rest: the
staging kernel is one function (`engine-core::dance::stage_dance_hud_vram`,
fed by the run's own widget table), the emit is one builder
(`engine-ui::ui_dance::dance_countin_prims`), and the **predicate** that picks
between retail's sprite and the placeholder is one world field
(`World::minigames.dance_hud_art_staged`) written by whichever host owns the
VRAM. Left per-host that predicate would have been two spellings of "did the
upload work", which is the shape a silent one-host regression hides in - and
the tiers cannot see it, because both spellings end at a live call site.

**A waiver's blocking capability can be falsified by a doc this repo already
carries.** Two equip-screen painters were waived as orphans on one shared
premise: retail opens windows `2 / 24 / 25` from one script while the port's
equip flow is built on `2 / 21 / 22 / 23`, window 25's rect overlaps window
21, and therefore "closing it means moving the whole screen onto the
descriptor-table layout, not adding a draw call". That premise reads the
script as *the* Equip screen's. It is one **step** of it: `field-menu.md`'s own
window-to-screen map puts script `0x801E4DC8` on sub-screen `0x14`, the
candidate list, which runs after `0x12` has picked the character and `0x13`
has browsed the slots - so it opens 24 and 25 *on top of* four windows already
up, and overlapping window 21 is the point. One of the two is adopted on
exactly those terms now, on both hosts, with no layout move.

The waiver format did its job here - it named something concrete enough to
check - and what it could not do was notice that another page in the same repo
had already answered it. A blocking capability is a claim like any other:
worth re-deriving before it is inherited, especially when two waivers share
one, because then a single wrong sentence holds two rows shut.

The same change closed a gap neither waiver named: the equip screen spelled
every candidate `Item 3A`, because the item name / description resolver lived
inside the *Items* screen's session builder. Sharing a panel between two
screens surfaces that immediately - the panel wants a description, and there
was nowhere to get one.

**A bind-time filter on state that changes later.** Both hosts bind a mesh
per field placement once, at scene entry, and both skip drawing any actor
whose live position is the off-map hide box. The native window uploads every
placement; the page also skipped *uploading* a placement that was
header-parked and still parked at bind time, under a comment that called this
the native rule. A header-parked placement is exactly the actor a cutscene
seats mid-visit - Noa or Gala materialized beside the player (`CC <ch> 37`),
`bylon`'s Maya stepping onto the stairs (`A3 3F 53 32`) - so on the page the
world had the actor on stage and nothing drew it, for the whole visit. Every
tier was green: the accessors, the hide-box predicate and the per-frame draw
were shared; only the page's one-time filter read the same predicate at the
wrong moment. The shape to look for is a host filter evaluated once over state
the engine keeps changing.

**One mask applied to two index spaces.** The page keeps its terrain and
placed draws in one list and gates each against its own engine mask: placed
draws by `field_placement_live`, terrain draws by the visible-tile crop
`field_terrain_live`. The tag that chose the second was the `else` of the
overworld-decoration test, not of the placed test, so every non-decoration
placed draw carried a terrain index too. At retail framing the crop then hid
placements by the mask entry of whichever terrain tile shared their index -
`town01` lost 30 of its 35 placed draws to it. Both masks came from shared
kernels and the native window asks each for its own layer; only the page's
tagging crossed them. The shape to look for is a mask whose index is
assigned by a branch that was written for a different property.

**A cue fired before the filter that swallows its edge.** Both hosts ask
`menu_cues::menu_edge_blip` for the pause menu's blip, and both run the save
screen's refusal box and block grid over the pad edge. The native window
fires the cue after those filters; the page fired it off the raw press before
`play_menu_input` ran them, so the press that dismissed a save error, or a
blocked Load on an empty block, blipped on the page only. `play_menu_input`
now fires the cue on the filtered edge. The shape to look for is a side
effect a host derives from an input before the shared kernel has decided
whether that input counts.

**A write that is durable on one host only.** A Save on the native window
writes the card file in the same commit (`card.persist()`); the page changed
the card in wasm memory and stored it back only from the Export button, so a
reload before Export lost the save. The runtime raises a per-port store latch
(`card_take_written`) on every in-game Save and the page stores the card over
its browser session when the latch fires. The same pass found the opposite
direction on the native window: a failed save-folder Load or Save only logged
and closed, where the card port and the page raise the shared refusal box.

**A stateless replay standing in for a stateful kernel.** The minigames page
drew the Muscle Dome first visit by replaying `FirstVisitHub` with no input
to a JS tick count. Every row it drew was the kernel's, so nothing looked
wrong in a still, but a replay cannot take a press or hand back the CD-XA
lines a tick started: the page skipped the whole visit on any key and played
no announcer, where both play hosts tick the hub live. It now resets and
steps a live hub (`muscle_first_visit_reset` / `_step`), and the hub's two
lines stage on the announcer lane.

Two more rows the same pass found are closed. The minigames page counted in
on its own READY / 1 / 2 / 3 / GO timeline; it now steps the engine's
`dance::CountIn` (`dance_countin_step`), which runs retail's states 3 to 5 -
READY, then `GO!` - on all three surfaces, with the `1 2 3` moved to where
the disassembly puts them, the song's end
([`minigame-dance.md`](../subsystems/minigame-dance.md#the-count-in-state-by-state)).
And the page's track per game (`GAME_BGM`) is read from the engine
(`minigame_bgm_id`, over `MinigameSubId::standalone_bgm_id`) instead of
spelled as literals beside it. The song-end `3 2 1 FINISH!` now runs on
all three surfaces off the overlay's own part programs
(`dance::FinishCountdown`), and the minigames page gained the how-to mode:
`dance_start_mode(2, ..)` installs the engine's `DanceTutorial` beside the
one-dancer run, stepped per frame as `World::step_dance_tutorial` does and
drawn through the shared `ui_dance::dance_tutorial_draws_for`.

Still open from that pass:

- **The page refuses the pause menu while any dialog box is up**
  (`_hudState.dialog`), on top of the engine's `field_menu_open_allowed`
  the native window asks alone. No retail evidence decides it yet.

## A rule spelled beside the shared predicate

A second side-by-side pass, with the first pass's rows closed, found a shape
the first did not name: a host that **calls** the shared predicate and then
adds its own test next to it. Both hosts reach the same engine function, so
every tier reads the pair as parity; the difference lives in the extra
clause, and one host has it while the other does not.

- **The menu-open gate.** The native window asked
  `World::field_menu_open_allowed` *and* `!menu_runtime.is_open()` *and* its
  own `narration` local; the page asked only the predicate, and read its
  pause-menu Start before its shop. So on the page Start opened the pause menu
  over an open shop. The shop, prize-counter, narration and title-card
  refusals are inside the predicate now.
- **The picker staging rule.** Both hosts seeded the free-roam story baseline
  on a direct scene entry, each with its own test for "is this a picker
  visit". The page's test (no opening chain, no timeline) is exactly what the
  prologue skip defeats - the skip tears both down before the host enters
  `town01` - so on the page the skip staged `town01` as a picker visit and
  dropped the opening's scripted silent-dawn BGM pause. The page also staged
  an overworld label, which the native entry never did. Both ask
  `World::stage_picker_entry` now, which also reads the skip's own marker
  (`entering_town01_opening`).
- **The scene-kind branch.** The page's `enter_field` routed an overworld
  label through the world-map entry; the native save Load did not, so a save
  written on a kingdom overworld - the only place saving is legal - came back
  as a plain field scene. `BootSession::enter_scene_live` is the one branch
  the native Load takes now.
- **The play clock.** Each host kept its own origin and high-water mark
  beside the shared `advance_play_time`, and each got a different edge wrong:
  the native one counted the title screen, the page's froze after a second New
  Game. The origin and mark are world state now (`World::tick_play_clock`),
  reset by `begin_new_game`.

The cure each time is to move the extra clause into the kernel, not to copy it
to the other host: a copy is two spellings again, and the next edge case goes
wrong on one of them.

The same pass found three host-local defaults that changed the game on one
host only. The native player-battle mode, on by default, seeded an empty save
with demo items, saved chains and a fabricated `Art1B` record for every
character; it is behind `LEGAIA_DEMO_BATTLE_SEED=1` now. The native window
installed the party cast trigger's anim-pair lists only for player-driven
battles, where the page installs them for every battle. And the native window
latched the Field / Battle mode edge once per display frame while the page
latched it per sim tick - and the battle load behind that edge installs the
party's clips, art banks and art records, so a catch-up frame ran its first
battle ticks without them natively. On input, the page let OS key auto-repeat
into its "just pressed" set, so a held direction scrolled every page menu,
and the native window had no focus-loss arm, so a key held across an alt-tab
stayed down.

A later scripted pass - the same pad script driven through `play-window
--pad-script` / `--key-script` and through the page with its tick stepped by
hand, frames paired by engine tick - found field walking, NPC dialogue, the
pause menu and its Options popups, the overworld, and a thousand ticks of an
auto-resolved battle frame-identical up to filtering. What it did find sat in
host state around the engine:

- **A session choice the next entry re-armed.** The native `F7` (random
  encounters off) cleared the world toggle only, and the live-loop arming a
  card Load or New Game runs through `field_live_opts` raised it again. The
  page's "No encounters" is a session flag every entry reads; `F7` now writes
  the native one.
- **A capture that wrote the player's config.** A `--screenshot` /
  `--key-script` run persisted every toggle it pressed, so a scripted `F8`
  left every later launch at night. Scripted runs write neither the options
  nor the bindings file now.
- **A handler key no script could press.** `--key-script` had no names for
  `F4` / `F8` / `F9`, so the native half of an enhancement-toggle comparison
  was unreachable; a test reads the handler's `KeyCode` arms and fails on any
  key the script cannot name.

## What a waiver may say

Both waiver files are validated for staleness on every run, so they cannot
name work that is done. What no checker can validate is the *prose*, and that
is where this repo has been burned: four shop waivers once asserted "the
browser play page has no shop host" about a tree where `play_shop.rs` had been
draining `take_pending_field_shop` for months. Undone work wearing the
language of an exemption survives every re-derivation, because the bucket is
re-derived and the reason is not.

So a waiver must name a **blocking capability** - something that does not
exist yet and would have to, spelled concretely enough to recognise when it
lands. "A `muscle_hud_quad_*` wasm export plus turning the page's blit into a
quad draw" is a blocking capability. "Not wired yet" is undone work.

If the answer is "someone has to type the method body", write the body.

## Known gaps no gate fails on

These host differences are large enough to be projects rather than wiring, and
none of them is expressible as a waiver, because a waiver names a *builder* or
a *hook* and these are whole subsystems one host does not have. Recorded here
so a future reader does not mistake a green gate suite for host parity.

They are engineering work, not reverse-engineering questions, so they are
**not** on [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md) -
that page indexes contested questions about retail behaviour, and nothing
about these is contested.

| Gap | Shape |
|---|---|
| shading law on web | The two hosts express one law in two shading languages. See [below](#the-two-hosts-do-not-share-a-shading-law). |
| battle body blend modes | Both hosts draw a whole battle body's semi-transparency; two residues differ in override keying and ordering. See [below](#a-battle-bodys-blend-mode-reaches-both-hosts-with-two-residues). |
| save rack port 1 | The native rack's first port is an engine-format save directory; the page's two ports are both memory-card images. See [below](#the-save-racks-first-port-differs-per-host). |

### The frame loop rules are engine-side

Both play hosts run one engine frame: `legaia_engine_session::BootSession::tick` - the mode seat's frame, the camera's half before the world tick, the world tick, the tick's BGM events, the camera's half after it, the SFX queue dropped on a door, the field SFX routing, and the mode word adopted. The browser page holds a `BootSession` over its own audio output and declares at install what it does itself (driving the pause menu's sub-screens, the field CD-XA lane, the per-tick queue drains).

### One pause menu, one press rule, one shop step

The pause menu's root picker is the session's own `BootSession::field_menu` on both hosts. A menu-button press goes through `BootSession::press_field_menu` - the scripted op-`0x49` press, the Start edge through `World::field_menu_open_allowed` (which holds the dialogue engagement, the narration crawl and title card, the locks and the shop), and the deny buzz - and the close through `BootSession::close_field_menu`.

The page used to keep a private `FieldMenuSession` with its own resume mode and re-spell the open beside the session's (gate sample, mode switch, seat), so one press had two answers; a title Continue / Options row could be refused on the page and never natively, because the page ran it through the press predicate and the window through the bare builder. Both now open the title rows through `open_field_menu` directly. What the page keeps in `PlayMenu` is only its sub-screen driver (the open sub-session, the card flow, the parked Load label, the latched key name).

A shop, the prize exchange and the inn prompt step through one per-tick kernel on both hosts, `MenuRuntime::step_field_session`: the tick on that tick's pad edges, the blip, and the unpark of the suspended op-`0x49` once the whole runtime has closed. The page stepped the shop once per display frame, so the tick-counted open fade and window slides ran at the monitor's rate (twice retail speed at 120 Hz); it now calls `play_shop_input` once per drained sim step, the frame's edges on the first. It also released the merchant op as soon as `shop_session` cleared, while the native window waited for the whole runtime - so on the page the field script could resume under a buy list's quantity or recipient screen.

What remains host-side is each host's display loop (winit's redraw natively, `requestAnimationFrame` on the page) and the steps it runs around the session's tick. Four of the rules that turn display frames into ticks are kernels in `engine-core::frame_step`, called by both hosts - the frame model is in [`engine.md`](../subsystems/engine.md#the-frame-model):

| Rule | Was | Kernel |
|---|---|---|
| Ticks per display frame | Native carried any backlog past its four-tick cap into the next frame, so sustained slow frames kept running four ticks a frame with the accumulator growing; the page dropped it. | `SimStepper` (the page through `play_drain_sim_steps`). |
| Camera around the world tick | The page published the compass azimuth after the scene tick, so its d-pad remap read the camera one tick late. | `camera_before_world_tick` / `camera_after_world_tick`. |
| Cutscene glide clock | Both hosts floored the glide's step count at 1, so every idle redraw advanced it and a glide ran twice as fast at 120 Hz. Native also kept the glide across a door whose destination opened on a timeline; the page reset it. | `CutsceneGlide` (reset on every entry; retail's `FUN_80025C24` kills the mover). |
| Move-VM strips | Native drained them per redraw (blinking off on idle redraws above 60 Hz, stacking below it); the page cached the last tick's drain (losing the earlier ticks of a catch-up frame). | `MoveVmGlobals::strip_frame`. |

The page also ran its tick tail short on a scene-entry tick (no rig rebuild, merchant poll or NPC clip step) and dropped the frame's remaining ticks after a door; it now runs the tail and keeps ticking, as the native loop does.

Under a shop or the prize exchange both hosts now freeze the whole frame tail, not only the world tick. Retail runs those screens at game mode `0x17` with the field overlay swapped out for the menu overlay, so none of the field code is resident.

The native loop runs only the menu session on the tick's edges, the unpark when it closes, and the SFX scheduler step, then skips the ocean and CLUT cyclers, the effect scene-graphs, the event drains, the party readout's kernel and (through a count of ticks that ran the tail) the NPC clip playheads - the page's `tick_frame` skip. The earlier note that the native tail "carries the SFX scheduler step the shop's own cues ride" was a misreading: no shop screen raises a cue on either host, so the step moved to the frozen arm for the same reason the pause-menu arm has it (delayed cues already queued keep ageing).

The field party readout's kernel is not a residue. A suppressed tick of `FieldPartyHud::tick` stores nothing but its cached decision - retail's suppress arm returns before the timer and position stores - and both hosts' draw paths re-ask the suppress gate, so stepping it under an overlay (native) and not stepping it (page) leave the same timer, the same cached position and the same picture. `a_suppressed_tick_changes_no_state` pins that.

The SFX scheduler steps once per sim tick under the pause menu and a shop on both hosts: natively in the frozen arms (`tick_menu_sfx`), and on the page through `play_tick_overlay_sfx`, which the frame loop calls while `_updateFieldMenu` or `_updateFieldShop` holds the field. That is retail's shape - mode `0x17`'s per-frame handler `FUN_80025F74` still runs the cue drainer `FUN_80016B6C` ([`audio.md`](../subsystems/audio.md#the-scheduler-under-a-menu-overlay-screen)). The page used to step it under neither, so a cue already delayed when the screen opened waited out the screen and fired late.

### Minigame launchers and the audio tail

- **The slot launcher.** Every native minigame hotkey arms the mode-24 door warp itself (`World::request_minigame_warp`, the call the page's `play_mg_debug_warp` makes): `O` now does too, so the slot session is the one a cabinet installs, with the balance assigned from the coin bank. The private `SlotMachine` with a frame-derived seed, the coin purchase through the counter's quote and the fronted dev stake are gone. A launcher is the `0x3E` arm on both hosts, not the cabinet record around it: an empty bank is refused at the walked door by that record's coin-bank compare (the field VM runs it on both hosts), and a bank below three coins that reaches the machine meets its own state-1 gate.
- **The BGM tail and the side-band memo.** One kernel, `legaia_engine_audio::bgm_tail::BgmTail`, holds the reward and side-band banks' placement, residency and retry memo, and both directors drive it ([`audio.md`](../subsystems/audio.md#the-banks-that-borrow-the-bgm-regions-tail)). The native director used to drop both banks on every owned-bank upload and the page only when a track overran them; the page also cleared its side-band memo on every scene change, where the native memo keyed on the BGM generation. The shared rule is retail's: a track change closes neither bank, a track that overruns one drops it, and the field init drops the reward bank.
- **Minigame XA lines.** The Baka announcer lines and the dome hub's two lines join the field's prestage list (`World::queue_xa_prestage`, from `baka_fighter_chrome::announcer_xa_prestage` and `muscle_ringside::hub_xa_prestage`), so the page stages them ahead of use as it stages a scene's op-`0x36` lines.
  The dome intro line starts on the frame the leg opens, and the scene host drains a door warp in the same tick as the field step that armed it, so a list filled at the hub's entry gave the page no lead. The hub's lines are listed earlier on both paths: when a scene whose MAN carries a `3E 69` door loads (`field_xa::scene_minigame_door_xa_prestage`; koin1), and at a launcher's `World::request_minigame_warp`, which the page's `play_mg_debug_warp` drains on the spot. The native window reads clips synchronously and drops the list.
- **The coin counter.** Both hosts drew an invented layout for op-`0x49` sub-op 6 (a heading, a caret line and a Yes/No placed on the prize exchange's menu-overlay windows). Both now draw the field overlay's own two panels - the entry panel `FUN_801E6F70` (record 10) and, during the confirm, the three-line panel over it (record 11) - from one engine layout, `field_submode_screen::coin_counter_lines`, through `engine-ui::ui_text_lines::pen_line_draws_for` ([`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md#the-coin-exchange-counter-is-a-field-overlay-screen)). The window frames and the two sprites are text stand-ins on both.

### The save rack's first port differs per host

Where a Load lands and how a New Game starts are one engine entry per host
(`engine-core::resume`, reached by native `BootSession::resume_save` /
`start_new_game` and the page's `play_resume_save` / `play_new_game`; the
two `check-ui-host-drift.py` pairs on `resume_card_load` and
`enter_new_game` pin it), and both hosts open every title through `TitleSession::for_front_end`
with a fresh rack scan - see
[`save-screen.md`](../subsystems/save-screen.md#where-a-load-lands-and-when-continue-is-live).

What still differs is what backs the rack's ports. The native window mounts
its `.lgsf` save directory as port 1 and the `--card` image as port 2; the
page mounts two memory-card images and keeps its `.lgsf` sessions in the save
bar, where they resume through an import rather than through the Load
screen. So a native player can Load an engine-format save from the retail
save-select and a page player cannot, and the page's title Continue is live
only when a card is inserted. Closing it means giving the page a port backed
by its stored sessions (the rack snapshot, the block read, the Save write and
the export path), which is storage work rather than wiring.

### Enhanced lighting, shadow maps included, on both hosts

Enhanced lighting is one engine-side source of truth,
`legaia_engine_ui::scene_lighting`, on both hosts: the emissive tags are set
by the same `tag_emissive_*` calls on every mesh build, the light list is
derived by the same emitter + clustering kernels over each host's own scene
assembly, the frame's lights are the same `nearest_lights` pick around the
player (props placed through the same `World::field_npc_live_anchor`), the
mood is the same `TimeOfDay::mood` over the same persisted option, and the
glow quads are the same `glow_vertices`. The page asks for all of it per
frame through `play_lighting_frame`, against the camera basis of the VP it
draws with; the moods are not constants on the page at all.

The PCF **shadow** term is per host by necessity - it is GPU work over each
host's own draw list. Native renders one depth layer per picked light
(`stage_scene_lights_and_shadows`); the page does the same in
`TmdRenderer._renderLightShadows` ([`site/js/webgl-tmd.js`](../../site/js/webgl-tmd.js)):
a `DEPTH_COMPONENT24` texture array, one downward cone per light rebuilt in
the page's `(x, -y, z)` frame, the ground and every placement drawn depth-only
through a position-only program bound at the main program's `a_position`
slot, and a 3x3 hardware-compared PCF in the main shader. The cone uses a
GL-style projection, whose window depth equals the native 0..1 depth for the
same near and far, so the compare bias and the polygon offset carry over
unchanged; the five constants are paired by `check-ui-host-drift.py`. The
page's "Lamp shadows" box is the native `Y`, and both mean **shadows only**:
off, the lamps keep lighting the scene unshadowed and no shadow map is drawn
(native stages each light's `color.w = 0` and skips the pass; the page leaves `u_shadow.x`
at 0). Turning the lamps themselves off is the enhanced-lighting toggle (`I`,
the page's lighting box), which is the one knob both hosts read for that.

The shadow array sits on its own texture unit for the program's life. A
`sampler2DArrayShadow` left at the default unit 0 shares it with the VRAM
`TEXTURE_2D`, which WebGL rejects at every draw of every page that builds a
`TmdRenderer` - not only the play page.

### A battle body's blend mode reaches both hosts, with two residues

Two retail writers put a whole battle body into a semi-transparent blend
mode through the top byte of its tint colour word: the near-camera ghost
pass `FUN_8004DC68` (mode `3`, a body near the camera or a caster's ally
during a magic cast) and the capture / defeat fade (mode `1`, additive).
`FUN_80043390` ORs the word's ABE bit into every packet of the body and its
ABR mode into the packets' tpage bits, so every prim draws semi-transparent,
the GPU still honouring each texel's STP bit
([battle.md](../subsystems/battle.md#the-near-camera-ghost-pass-fun_8004dc68)).

Both hosts reproduce that through one kernel, `engine-core::battle_body_blend`
behind `BattleActorDrawPlan::apply_body_blend`, which rewrites a body's TSB
words from the draw plan's colour word: the native override builder applies it
to the posed mesh and, while the word raises ABE, to a re-upload of the rest
mesh (`event_handler/redraw_passes.rs`), and the page re-sends the stream on a
blend-key change (`web-viewer::play_battle_body_blend`, with
`TmdRenderer::updateSceneMeshCbaTsb` rebuilding the per-ABR semi tail). What
still differs:

- **native:** the posed override is keyed by TMD index, so two bodies sharing
  one TMD share one override;
- **page:** blend-pass ordering is per mesh, not per prim, so a blended body's
  prims do not interleave with other meshes' semi prims the way one ordering
  table would.

### The Ra-Seru chip's cross-out: one atlas cell, one engine read

No longer a gap; kept here because the shape of the fix is host-specific. In the Rim Elm ambush and against monster `0xAF` the special-battle word
carries `0x200`, and retail's command ring crosses the Ra-Seru chip out with
the red `etim` quad (`FUN_801DBC30(0xF8, 0x42)`) and refuses its arm. Both
play hosts take the refusal and the greyed chip from the engine
(`battle_hud::battle_magic_chip`, `World::tick_battle_command`), and both
draw the X through one call: `engine-ui`'s
`battle_command_ui::battle_command_menu_sprites` draws the chip plates and,
switched by `battle_hud::battle_raseru_cross_out` (the ring is up under the
bit), places the mark at `RASERU_MARK_ANCHOR` after them.

The X is a sprite out of the chrome atlas, not a VRAM screen primitive,
because the browser page draws the chips on its 2D overlay canvas above the
GL view - a screen primitive there would sit under the plate it marks. Its
texels live on the battle effect page (PROT 870, page `(448, 0)`, CLUT
`(64, 476)`), so each host bakes them into the atlas with
`save_menu_atlas::add_cross_out_mark` right after `build_atlas`
(native `window/run.rs`, page `play_menu.rs`), and a host that skips the
bake gets `BattleChromeRects::cross_out = None` and no mark. See
[battle.md](../subsystems/battle.md#the-ra-seru-forbidden-bit-of-the-special-battle-word).

The status marks ride the same shape. The one bake also seats the Rot stamp
and the Curse plate (`BattleChromeRects::rot_stamp` / `curse_plate`), the
ring builder takes `battle_hud::battle_ring_marks` - a `RingMarks` from
`engine-vm`, so neither host copies it field by field - and the arts entry's
stamps come from `arts_input::arts_input_rot_stamp_draws` off
`ArtsInputView::status`, called by both hosts after the entry chrome.

### The minigame side-channel step is paired; its contents are not

Tier 11 pairs `tick_minigame_extras` (native) with `tick_minigame_ui` (page) as
one frame kernel, which is true of the *step* and says nothing about the work
inside it. Tier 12 is what measures the inside, and the pair carries a
`[[frame_content]]` row with both difference lists. Taken sub-step by sub-step,
most of that work does reach both hosts.

| Native sub-step | Browser play page | Minigames page |
|---|---|---|
| `drain_minigame_sfx_cues` | `drain_minigame_sfx_cues_web` | per-game cue drain |
| `stage_dance_hud_art` | `sync_dance_hud_residency` + `ensure_minigame_art` | `dance_art_*` exports |
| `tick_fishing_actors` | `tick_fishing_actors` | pond session only |
| `tick_baka_chrome` | `tick_baka_ui` | `baka_chrome_json` |
| `tick_muscle_hub` | `tick_muscle_hub` | `muscle_*` exports |
| effect-pool ageing | `World::tick` | own pool, own tick |

The native `tick_dance_side` sub-step is not in the table because it no
longer exists: everything it did was the duplicate spawn below.

The two `tick_muscle_hub` steps used to be the same hub timer state machine
written twice - the first visit, the leg-open ROUND card, the INTERVAL +
tally screen and the re-entered backdrop, with their leg edges, as eight
fields on each host. They are one engine kernel now,
`muscle_ringside::HubTimers::tick`, which each host drives once a frame and
reads its draws from; what stays per host is how the XA line and the tally
voices it returns get played.

The **effect-part pool** is no longer a native-only sink. It is
`legaia_engine_core::minigame_fx::MinigameFxPool` on
`World::minigames.fx`, aged inside `World::tick` (so every host that ticks
the world drains the same parts), drawn through one builder
(`engine-ui::minigame_fx::fx_part_draws`) that the native HUD, the play page's
overlay composition and the minigames page's fishing canvas all run. The
minigames page keeps its **own instance** of the same type rather than sharing
the world's, because that page drives the session types directly and holds no
`World` at all - same model, same bound, same ramp, one more owner.

Three things the move settled that the old row had wrong:

**The dance sequence banner was never the pool's.** `DanceGame::judge_press`
issues the three `FUN_801d3fd0` spawns into the **run's own** part pool when
the human closes a chain, which is gameplay rather than host presentation. The
native window spawned the same set a *second* time into its host pool and drew
both lists, so every cleared sequence painted `GOOD!` and its two stars twice
at one seat. The duplicate is gone; the run's parts are the only ones, and the
play page draws them now - it had no emit site for them at all, while the
minigames page had been drawing them from retail's own widget cells the whole
time.

**The fishing splash is a session event, not a venue actor.** The strike splash
spawns from the session's cadence-match `PondEvent::Splash`, which every host
sees, so `World::tick_fishing` spawns it and the venue's wander / line actors
are not a prerequisite. What does need those actors is the wander-retarget
ripple and the catch-celebration bursts, because their *seats* come from the
actors.

**The venue actors are one engine kernel on both play hosts.**
`legaia_engine_core::fishing_venue::tick_fishing_venue_on_host` is the whole
venue frame - the wander fish and its retarget ripple, the `.MAP` floor solve,
the reeling line and its catch bursts, the sub-screen sway - with its actors on
`World::minigames.fishing_venue`, and it returns the venue camera's writes
(`VenueCameraWrites`) for the host to apply to its own engine camera. The
native `tick_fishing_actors` and the play page's `tick_fishing_actors` are each
that one call plus the apply, so the ripples and bursts land in the shared pool
on both hosts and the play page's prize panel sways on the same phase. The
logic used to live inside the native window, which made every one of those
native-only. The minigames page runs the pond without a `World` or a venue
scene, so it has no actors to step; its strike splash comes from its own tick's
events.

**The pool does not borrow the dance's emit dispatch.**
`legaia_engine_core::dance::sprite_part_emit` (`FUN_801d387c`) is the *dance*
overlay's routine, and running the fishing overlay's parts through it would
assert a shared draw dispatch the dump corpus does not show. A pool part's
pair is stage pixels, already through whatever shift its own producer applies.
The fade ramp is shared, because that ramp is the port's decision either way.

**The Baka round chrome reaches all three hosts.** A table row here once read
`tick_baka_chrome` as having no browser twin on either page; both pages were
already consuming the duel's chrome frame. The frame is the duel's own
(`BakaFight::chrome_frame`, stepped inside the rules tick). The two play hosts
do the same two things with it: sound the announcer line it fired - the native
window through `AudioBgmDirector::play_xa_clip`, the play page through its
CD-XA clip path (`tick_baka_ui`) - and print its widgets through the shared
label kernel (`baka_fighter_chrome::chrome_labels`). The minigames page plays
the same lines through its own XA output and draws the widgets
from the duel's own art: `baka_chrome_json` carries each glyph
draw's texel column, stamped by the same `baka_fighter_chrome::glyph_u` the
native window resolves its draws with, and the page samples widget 5's strip
there - where it used to compute `(g % 10) * 24` itself, which disagrees with
retail's byte store past the tenth cell.

**The duel's strike timing is one engine clock on all three hosts.** The
exchange is booked on the frame the winner's strike keyframe is crossed
(`BakaFight`'s `StrikeClock`, the retail combat tick's `FUN_801D6E5C` lookup),
and each host stages the same clip headers through
`baka_fighter::roster_clip_headers` - the native window and the play page off
their PROT index, the minigames page off its disc - so a double-step clip
strikes at the same frame everywhere. The minigames page also poses the swing
off that clock (`baka_state_json`'s `clock`), where it used to start the swing
when the exchange report arrived. Both browser pages read the duel's state
through one builder, `minigames::baka_state_json_for`; the play page's own
copy carried no `clock` and no `ghosts`.

**The duel's 3D surface is one engine kernel on all three hosts.**
`engine-core::baka_duel_scene` builds the buffers (both fighters, two ghost
copies each, the four arena walls, the floor), poses them from the fighters'
display clips and the afterimage passes, and owns the arena camera; the
native window (`refresh_baka_duel_gpu`) and the play page
(`play_mg_baka_scene_*`) each drive one `BakaDuelSurface`, upload what it
posed and draw it under `DuelCamera::vp_raw`. Before it, the native window
drew the duel as labels only, and the play page drew the fighter meshes
under a fitted orbit camera, timed each clip off its own tick and drew no
ghosts - so "both play hosts draw labels only" was true of one of them.
The standalone minigames page now drives a `BakaDuelSurface` over its own
`BakaFight` (`baka_scene_*`) - before, it held the fight but no surface and
posed its own buffers under a fitted camera, with one authored wall, a floor
tiled from the wall's texture, no cameo and no impact parts. Both browser
export sets flatten the surface through `minigames::duel_surface`, the
`baka_state_json_for` shape applied to the buffers. The two shapes that hid
the gap: the page's fight carried the clip headers, cameras and impact
templates, so it *stepped* everything the surface draws and every state
read-out agreed; and the page stopped ticking at the deciding exchange, so
the result close-up and pinned flourish never ran there either. A state
oracle cannot see either - only a frame can. The PLAYER SELECT pick was a
skin on that page as well, where retail seats it as the player's roster
record (`minigame-baka-fighter.md`, Site presentation).

What stays open was misnamed "sprite effects". The afterimage and the cameo
draw no sprite: the two banks `_DAT_8007B888` / `_DAT_8007B840` are the
scene's type-`0x05` / `0x0B` ANM clip banks the clip selector `FUN_800204F8`
resolves, and each of those routines drives a **model** through them. The
impact pair is half sprite after all: one of its two templates is a
draw-kind-4 node on the render dispatcher's `0x4000` sprite arm.

- The special's afterimage (`FUN_801D49E8`) is engine state every host steps,
  and every host now draws it as darkened copies of the thrower's mesh.
- The impact pair (`FUN_801D4DF8`) is engine state every host steps too:
  `BakaFight`'s booking arms spawn its four `FUN_80021B04` templates as
  `engine-core::baka_impact_fx` parts, run on the duel's tick through the
  shared move-VM kernels. The two play hosts draw them through the duel
  surface - the flip-book flash as sprite-arm quads (`FUN_8002A5A4`'s port),
  the prop flashes as additive copies of stage TMDs `1` / `2` - and the play
  page re-uploads the mesh attributes when `play_mg_baka_scene_attr_generation`
  moves. The standalone minigames page steps the same parts and draws none:
  it has no surface to draw them into.
- The round-start cameo (`FUN_801D6310`) spawns on a held Triangle: both
  play hosts hand the duel the held word through `World`'s duel tick, and
  the duel surface draws the ring girl camera-relative and applies the wink
  blit to its VRAM. The standalone minigames page passes no held word, so
  its duel never spawns one.

And the native window resolves each glyph draw's stamped cell rect without
sampling it, because its duel HUD has no textured-quad surface.

**The slot machine's paylines are one projection on all three hosts.**
`slot_machine::projected_paylines` runs the ported payline pass
(`FUN_801D3380`) and projects both endpoints through the machine's fitted
projection; both browser pages stroke those segments (`sa` / `sb` in the prims
JSON) and the native window draws them as one-pixel flat quads through
`engine-ui::ui_slot_paylines`. Around them all three hosts draw the machine
itself through `engine-ui::ui_slot_cabinet` - the standalone minigames page
by rasterising the same list on the CPU (`engine-ui::screen_prim_raster`),
because its slot panel is a 2D canvas.

### A `web-ahead` builder is not by itself a gap

Tier 1 prints a `web-ahead (informational)` line for every `engine-ui` builder
the browser play page calls and the native window does not. The line answers
"which host calls this function", which is not the same question as "which host
draws this screen" - and for the battle value readout the two answers differ.

`battle_combo_cluster_draws_for` and `battle_value_readout_draws_for` are the
readout's **font fallback**: they restyle retail's `N HIT` / `TOTAL` cluster and
its damage numerals in the dialog font, for a host that cannot sample the battle
effect atlas. Their own doc comments say a host that can sample VRAM should skip
them and draw the real 24x24 cells off texture page `0x27` / CLUT `0x7703`.

Both hosts do exactly that whenever the atlas is resident, through one shared
kernel: `engine-vm::battle_value_readout` fixes the layout,
`engine-ui::battle_numerals` turns it into quads, and each host supplies only the
seat (the struck actor's projected position, which needs that host's camera).
The native chain is `redraw::battle_value_readout_prims`; the page's is
`play_battle::battle_value_readout_prims`. So the *screen* reaches both hosts,
drawn from retail's own art, and the `web-ahead` pair is the page's extra arm for
the frames before its VRAM exists.

The native window lacked that extra arm, and the `web-ahead` line was the only
place it showed: with no battle VRAM uploaded it drew no readout at all where
the page drew fallback text. It has one now - `redraw::battle_value_readout_draws`
- and the wiring had one requirement, which is why the disclosure named it: the
two arms must stay mutually exclusive, because both drawing at once renders
every number twice. Each checks the same `battle_vram` residency, opposite ways,
the way the page's own `battle_value_readout_has_atlas` gate does.

Doing it needed the layout split out from the emit. The native prim builder had
the layout inline, so there was nothing for a second emit to consume - which is
the general shape of a "narrow window" disclosure that stays open: the gap is
not the missing draw call, it is that the code the draw call would need is
fused to the arm that already exists.

### One-shot voices on the minigames page

The Muscle Dome's between-leg tally keys a voice per drained lane, and it names
no cue id: `FUN_801D1288` resolves a whole `(voice, VAB id, program, tone, note,
fine, vol_l, vol_r)` set, which is why the catalog path
(`SfxBank::play_one_shot`) could never sound it.
`legaia_engine_audio::VoiceAttr` + `key_on_voice_attr` are that shape.

The standalone minigames page used to have no `Spu` at all to key into. Its
whole audio surface was offline: a track rendered to interleaved PCM
(`music01_bgm_render`) and per-cue PCM decoded out of a VAB (`muscle_sfx_pcm`,
`slot_sfx_pcm`), each handed to an `AudioBufferSourceNode`. That covers a cue
that names itself by id and nothing else.

It owns one now - `LegaiaMinigames::minigame_audio_open` builds a `WebAudioOut`
from a user gesture and stages SFX program banks per descriptor slot out of one
allocator at the top of SPU RAM, the way the play page's `play_sfx` does - and
both of the native window's firing paths reach it: `minigame_sfx_cue` for an
id-keyed catalog cue and `muscle_tally_voice` for the explicit attr set, the
latter driven from the INTERVAL screen's own tick because the page replays
`ScoreTallyRamp` from that tick rather than stepping it per frame. The page
side is `MgSpu` in `site/js/minigame-bgm.js`, gated on the same site sound
toggle the offline path checks.

Two things the port keeps deliberately unshared, and they are not drift:

- **The music stays offline.** A rendered track on an `AudioBufferSourceNode`
  mixes with the SPU at the device rather than inside it. Moving it onto the
  mixer would be a second BGM path, not a shared one.
- **A runtime-bank cue id still answers `false`.** The static descriptor table
  is byte-indexed (`DAT_8006F198 + id*8`); ids at `0x200 +` come from the
  per-battle `bse.dat` bank ([`bse-dat.md`](../formats/bse-dat.md)), which this
  page stages nothing for. The dance's `COUNTIN_INTRO_CUE` is one of those, and
  the export reports the miss rather than keying static row `0` by truncation -
  the width trap the play page's `u8` scheduler was caught by once already.

The in-world dome on the **play** page was named by the same row and needed
the same thing. Sitting beside a live SPU is not the same as having a door
into it: `play_sfx` keyed cues by id and nothing else, so that host's tally
ran its ramp and drew its rows in silence. It has `key_on_voice_attr` now,
resolving the attr set's VAB id as an SFX slot with the scene BGM bank behind
it - the native director's own two-step. All three hosts narrow the cue's
eight arguments through one kernel (`VoiceAttr::from_cue_words`), because the
voice-slot clamp is not cosmetic: a slot at or above `NUM_VOICES` keys
nothing and reports success.

### Scripted mesh re-bind (op `0x0E`), and what it took

The scripted-motion VM's op `0x0E` re-binds the actor's mesh: the operand is
compared **unsigned** against `0xF0`, below which it resolves against the
scene's model-bank base `*(u16*)0x8007B6F8` and at or above which it resolves
`operand - 0xF0` against `*(u16*)0x8007B824` and raises the translucent draw
bit; either way `FUN_80024E08` zeroes the anim cursor `+0x5C`, stores the id at
`+0x64` and reloads the mesh.

**It is wired on both hosts.** The port decodes the op
(`ambient_motion_ops::step_op_model_swap` -> `AmbientEffect::ModelSwap`) and
`World::apply_ambient_motion_effects` records the id on
`FieldNpcState::models`, keyed by placement slot - the port's stand-in for
retail's `actor[+0x64]` store, because the port's hosts hold the uploaded mesh
rather than the actor. Each host reads it back through
`World::field_npc_live_model` and resolves the bytes through the loaded
scene's own model bank (`SceneHost::model_bank`,
[`model_bank::SceneModelBank::tmd_bytes`]): the native window in
`upload_assets`, with a per-frame `rebind_live_npc_models` for a swap the
script makes after the upload ran, and the browser through the
`play_npc_live_model` export its NPC draw consults each frame. One world
field, one resolver, two upload paths.

This was a project rather than a wiring job because of the measurement, which
is what the resolver had to satisfy
(`crates/engine-core/tests/ambient_motion_op_census_disc.rs`, disc-gated):

| scene | sites | distinct targets | targets that are some placement's spawn model | scene model bank | targets that resolve in it |
|---|---|---|---|---|---|
| `bubu1` | 9 | 9 | 0 | 173 | 9 |
| `koin3` | 100 | 10 | 0 | 77 | 10 |
| `edbubu` | 6 | 6 | 0 | 160 | 6 |
| `other7` | 100 | 10 | 0 | 65 | 10 |

**Zero of 215 sites** names a model that some placement in the same scene
already binds, so the obvious cheap implementation - a
`(bank, model id) -> uploaded mesh` map built from the placements a host
already uploads - covers nothing at all, and that is why the bytes come out of
the bank instead.

**All 215 resolve**, which is the half that was measured wrongly first. The
earlier version of this table compared the operand against
`SceneResources::tmds.len()` and reported banks of `1` and `0` for `koin3` and
`other7`, from which "op `0x0E`'s bank is a different pool" read as the likely
answer. It is not: `SceneResources::tmds` is a **magic scan over the scene's
raw entries**, blind to a TMD inside an LZS-compressed bundle descriptor, and
both scenes carry their models in exactly that - a type-`0x02` descriptor of 77
and 65 members. The bank op `0x0E` indexes is `DAT_8007C018`, the global
registered-TMD array, whose per-scene window is the loader's own registration
order. `crates/engine-core/tests/model_rebind_live_disc.rs` is the wiring's
own oracle: over `koin3` it decodes every authored operand and asserts each
one resolves to bytes that parse as a TMD with objects.

The `>= 0xF0` half stays unresolved on purpose. Those five meshes live in
PROT 0874, which is not one of the scene's entries, so `tmd_bytes` returns
`None` there and each host keeps the placement's spawn mesh rather than
drawing nothing; `legaia_asset::character_pack` is that half's reader when a
host wants it. No authored site on the disc takes that arm.

### The cast-voice leg

A Seru cast speaks on **both** hosts, through one engine channel and one
staging kernel. The section used to sit under "known gaps" as a symmetric
decline; what follows records what was wrong about the reading that kept it
there and what carries the voice now.

The cast-audio dispatcher `FUN_801F3990` (ported as
`engine-vm::battle_cast_cue::cast_audio_cue`) still emits its cue band - the
player leg `char_kind * 0x10 + 0xF8 ..`, the enemy leg `0x20C..0x20E` - and
both hosts still classify it at fire time and decline it (the shared
director reports it as `voice_declined`, which the page counts on
`voice_cues_dropped`). That band was
not raised on either measured retail cast (`capture`, N = 2, exec
breakpoints on all three routines - [`../subsystems/cast-module.md`](../subsystems/cast-module.md#the-casts-own-cd-xa-voice)),
so voicing it would add a sound retail does not make. Its decline is the
same on both hosts and is not the cast's voice.

#### The bank is per-module, not per-character

An earlier reading of this section named `XA27` / `XA28` / `XA29` / `XA34` as
the staging list, derived by running `char_kind` through
`classify_cue`'s slot arithmetic. That is the wrong producer, and so the
wrong bank.

The cast's voice is **not** raised by the dispatch cue at all. Every one of the
sixty-four slot-B cast modules (PROT `0903..0966`) calls the cue dispatcher
`FUN_8004FCC8` itself, with its **own** cue id: a literal in 62 of the 64
(the other two, PROT `0936` and `0937`, form the id at runtime), reproducible
straight off the extracted entries by decoding the `jal` and its argument. So
a cast's voice is a property of the spell's module, not of who cast it.

Worked examples, each confirmable against `DAT_800788B8`: PROT `0903` names
cue `0x134`, which `classify_cue` resolves to `FUN_8003D53C(6, 4, 686)`; PROT
`0905` names `0x131` -> `(6, 1, 568)`; PROT `0911` names `0x161`.

The real bank is what those ids resolve to, and it is much wider than four
files - `clip_slot + 1` is the `XA<n>.XA` number:

| clip slot | file |
|---|---|
| `6` | `XA7.XA` |
| `8`..`0xE` | `XA9.XA`..`XA15.XA` |
| `0x11`..`0x13` | `XA18.XA`..`XA20.XA` |
| `0x15`, `0x16` | `XA22.XA`, `XA23.XA` |
| `0x18` | `XA25.XA` |
| `0x21` | `XA34.XA` |

Seventeen files, not four, and `XA27`..`XA29` are not among them. Three of the
ninety literal ids (`0x21`, `0x22`, `0x56`) sit below the dispatcher's `0x100` XA threshold and take
the dispatcher's SFX-queue path instead, so they are not voice clips at all.

The span half of the problem was also wrong in the same direction:
`XA_CUE_DURATION_ENTRIES` capped `DAT_800788B8` at `0x40` entries, which drops
every cue from `0x140` up - that is, most of this table. It is `0x110` now.

#### What carries the voice

The producer is the engine's, so both hosts get it for free. At the two
arming seams (`World::arm_summon_stager`, `World::arm_capture_cast_module`)
the cast band scans the paged module's own bytes for its head cue
(`engine-vm::battle_cast_cue::module_head_cue` - the first dispatcher call
inside the image's content, its `a0` valued from the delay slot or the most
recent write, the two coin-flip modules resolved through the world's `rand`)
and runs it through the dispatcher's CD-XA arm (`admit_voice_cue`). What
comes out is a `FUN_8003D53C` triple on `AudioState::battle_xa_cues`, the
`(clip, channel, dur)` channel the melee kernel's `XA27` / `XA30` requests
already ride into each host's `play_xa_clip`.

Staging is **lazy, per cast, one channel span**. Neither host decodes the
seventeen files: a request for a `(slot, channel)` the clip bank does not hold
reads that file from its first sector to the starter's stop point
(`read_span_sectors(dur)` = `(dur * 150 + 149) / 60`) and decodes that one
channel (`XaClipBank::decode_channel_span`), kept under `LAZY_CLIP_CAP`,
oldest out. The native window reads the sectors off the disc image
(`AudioBgmDirector::set_xa_lazy_source`, armed at boot); the play page owns the
disc bytes, so the engine lists the span it wants
(`play_xa_stage_requests_json`) and `play-app.js` slices it and hands it back
(`play_xa_install_span`) one request per frame, after which the request
replays. Both then play through the same XA mixing path as the arts shout,
with the same modelled CD-response delay.

**The two gates were mis-read, and one of them was fabricated in the port.**
`ctx[+0x276]` is not "the module's decline gate": it is the battle context's
side-band applier stage - the per-turn `summon.dat` / `readef.DAT` streaming
machine's phase byte ([`../formats/summon-readef.md`](../formats/summon-readef.md)),
seeded `1` by `FUN_801DABA4` each turn and stepped to `0` by `FUN_801F12D0` -
and a summon module polls it itself before installing its actor record, only
then raising its head cue (PROT 0903: `lbu 0x276` at `0x801F6CC0`, `jal
0x801F19EC` at `0x801F6D3C`, the cue at `0x801F6E50`). For the cast's own
voice the gate is open by construction; the port's side-band is resident, so
its window is zero frames wide and the engine passes `0`. The engine had been
feeding that byte from `battle.tutorial.is_some()`, which no retail writer
supports - the melee sting was silenced in tutorial battles where retail plays
it; it is `side_band_streaming` now. `FUN_8003DE7C(1)` is the read-span
countdown `gp+0x91C` (stepped by the frame-speed byte), not a bare
drive-idle test: a cast inside the previous clip's span plays nothing, and
that decline the engine reproduces through `battle_xa_busy_frames`.

Oracles: `engine-core/tests/cast_voice_head_cue_disc.rs` (the scan reproduces
the doc census on all 64 images; arming PROT 0903 / 0905 raises the captured
`(6, 4, 686)` / `(6, 1, 568)`, a cast inside the span raises nothing),
`engine-shell/tests/cast_voice_lazy_stage.rs` (the native span read yields
each clip's own share of sectors - `XA7.XA` is a stereo 37.8 kHz eight-channel
interleave), and the `play_xa` unit tests for the page's deferred request.

Two neighbours that are **not** this path, so a reader does not merge them:

- The **arts shout** (`FUN_8004C140`, `XA2` / `XA4` / `XA6`) plays on both
  hosts.
- `cast_item_give` (`FUN_8003D53C(char_kind + 0x19, 0, 0x5A)`) is dropped
  earlier still, at the `BattleActionHost` trait default - no host overrides
  it, so that cue never reaches audio at all.

### Per-actor pitch and roll

The scripted-motion VM's ops `0x15` and `0x16` tween the actor's X and Z Euler
angles (`+0x24` / `+0x28`), and retail composes all three through `RotMatrix`
(`FUN_80026988`) in the per-actor render dispatcher `FUN_8001ADA4` - which
hands the composer `actor+0x24` whole (`addiu a0,s0,0x24` / `jal 0x80026988`
at `0x8001af04`), so the heading at `+0x26` is simply the middle angle of the
triple. **Both** hosts used to draw a field NPC with a single `Ry(heading)`,
so neither showed a tilt; the gap was recorded here as a browser one, which
was half right at best.

What was missing was not a draw call on either host but the *data*: `World`
published a per-slot heading and nothing else. It publishes the pair now
(`FieldNpcState::tilts`, written by `tick_field_npc_motions` from the
channel's `AmbientMotion::pitch` / `roll`, read through
`World::field_npc_tilt`), and each host composes the triple through a builder
it already had - `engine-ui`'s `battle_intro::placement_rotation` natively,
`placementModelEuler` on the page, the same pair the **placement** tilt kernel
row already ties together. A slot with no tilt keeps the cheaper yaw-only
matrix on both hosts, which is almost every slot.

The disc-wide carrier census
(`crates/engine-core/tests/ambient_motion_op_census_disc.rs`) bounds the whole
class: over every scene MAN's tail-section-1 streams, op `0x15` has **zero**
authored sites and op `0x16` has 45, all in one scene - `juui1`, which is the
same scene the placement-tilt kernel row names for tilting all nine of its
static placements about X. So this is one scene's worth of presentation, and
`crates/web-viewer/tests/play_npc_tilt_parity.rs` (disc-gated) pins it there:
the tilt reaches the page's accessor, the pitch stays zero, `town01` reports
an all-zero array, and the two hosts' compositions agree entry for entry.

### Screen-space PSX primitives across the two hosts

Every screen-space effect retail draws - the field-to-battle transition styles,
the move-FX afterimage streak, any `screen_fx` sprite - is a PSX primitive: a
quad whose texels come out of VRAM through a per-primitive CLUT/texpage pair,
blended by one of four fixed ABR equations, ordered by an ordering-table bucket
rather than by a depth test
([`renderer.md`](../subsystems/renderer.md#screen-space-ordering-table-pass)).

This was for a long time a capability only the native window had, and *why it
stayed invisible* is the part worth keeping: `engine-ui`'s `SpriteDraw` is a
**semantic alias of `TextDraw`** - an axis-aligned destination rect, an atlas
source rect and one flat RGBA tint. Nothing in it can express a texpage, a CLUT,
per-vertex UVs, per-vertex colour, an ABR mode or an OT bucket. A builder
returning `SpriteDraw` is therefore on tier 1's surface while a whole capability
beside it is not, and no gate can fail on a type that does not exist.

#### What both hosts share

The model is `engine-ui`'s `screen_prim` - the wgpu-free leaf both hosts already
link:

| piece | what it is |
|---|---|
| `ScreenPrim` / `ScreenQuad` / `FlatQuad` | four corners, four `(u, v)` pairs, a `(cba, tsb)` pair, flat **or** per-vertex colour, a semi-transparency flag, an OT index |
| `abr_mode` | the blend-equation selector, TSB bits 5..=6 |
| `order_primitives` | `AddPrim` + `DrawOTag`: descending OT index, LIFO within a bucket |
| `build_geometry` | the only public route from a primitive list to something drawable |
| `ScreenVertex` + the `SCREEN_VERTEX_OFF_*` offsets | one byte layout, read as bytes by wgpu and by WebGL2 alike |
| `fade_prim` / `display_rect_flat_quad` | the display-rect packets the transition family emits |

The sort needs no gate of its own, because the shape of the API removes the
second place to do it: neither host is ever handed a primitive list.
`build_geometry` runs the ordering-table walk itself and returns a vertex
buffer, an index buffer and a run table, so by the time either host sees the
data the order is already baked into the index buffer. `engine-render`'s
`screen_overlay` re-exports the module at its old path (so native call sites and
tests read unchanged) and pins its display-rect constants against
`vram_capture`'s with a compile-time assertion rather than a comment; the play
page reads the same three arrays through `play_screen_prim_vertex_bytes` /
`_indices` / `_runs`.

The browser was not starting from nothing on the GPU side either: the play page
already uploads a 1024x512 VRAM page (`field_vram_bytes`) and already samples it
with the 4/8/15 bpp + CLUT decode for **3D** meshes. Its screen-prim pass runs
that same decode with the texture-window remap dropped (screen sprites never use
GP0(E2)), and binds the four ABR equations through `TmdRenderer._setSemiBlend` -
the page's *existing* blend table, not a second copy of it. `blendColor` carries
mode 0's `0.5` and mode 3's `0.25`, which is why WebGL2 needs no shader
pre-scale where the native pipeline uses one.

#### The style bodies draw on both hosts, from one emitter

The page draws the whole transition now - confetti, tile shatter, curtain,
swirl, spin-up ring, backdrop and fade - and the closure is worth recording
because it required both of the reasons the gap existed to fall together:

1. **The emitter moved out of the wgpu-linked crate.** `battle_intro` (with
   the `gte` arithmetic and `vram_capture` blit it reaches) lives in
   `engine-ui` now, re-exported at its old `engine-render` paths so native
   call sites read unchanged. The one genuinely renderer-bound step - turning
   "the frame just drawn" into RGBA bytes - stayed per-host:
   `engine-render::battle_intro::update_field_capture` wraps
   `Renderer::capture_rgba` natively; the shared emitter itself only exposes
   `land_capture_rgba` / `refresh_captured_page`.
2. **The page reads its own frame back.** After the field 3D pass and before
   the screen-prim pass, the page `gl.readPixels` the frame it just drew,
   hands it to the `play_intro_land_capture` export (rows arrive bottom-up;
   the emitter's blit flips them), and re-uploads its VRAM texture in the same
   frame so the first styled frame samples the field, not stale texels. The
   readback is a one-shot per transition; the curtain's per-frame intermediate
   afterwards rides the ordinary `field_vram_take_dirty` re-upload.

Two page-side specifics keep this honest rather than symmetric. The page has
**one** VRAM texture where the native window keeps the transition's captured
clone separate, so during the window its field meshes sample the same texture
the capture rects landed in - invisible in practice, because the emitter's
opaque `backdrop_prim` (now emitted on both hosts) covers the display from the
first armed frame. And `field_vram_bytes` returns the emitter's captured clone
while one is live, snapping back to the pristine scene page when the
transition drops.

The simulation half was never the gap: `World::tick_encounter` runs the
transition state machine on both hosts, so the clock, the BGM swap and the
battle that opens were always identical. What differed - how much of the
window got drawn - no longer does.

#### The field fog sheets ride the same pass

The field fog pool ([`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-fog-pool-spawner-records-render-pass))
is the one draw list retail runs *inside* its field render pass rather than
from an actor: `FUN_8003F348` ages, moves and emits every live record in the
same routine, so the port's render step has to be a draw-path call. It is
one on both hosts - `World::fog_render_step(&view)` - and the two call sites
are the two things a reader should compare:

| host | call | camera |
|---|---|---|
| native window | `take_field_fog_prims` in `window/event_handler/redraw_passes.rs`, before the renderer borrow, composited with the rest of `screen_prims` | `resolve_field_camera(world, camera, None, aabb centre)` |
| play page | `tick_field_fog_prims` in `play_field_fx.rs`, from `tick_battle_intro`'s prim assembly | `resolve_field_camera(world, camera, None, aabb centre)` |

Both wrap the quads through `screen_prim::fog_puff_prim`, so the blend class
(semi-transparent, the record's ABR `1`) and the `POLY_FT4` vertex order come
from one function. Two shared simplifications, deliberately identical rather
than per-host: the camera resolves **without** the cutscene view, so a
scripted camera beat projects the fog through the follow pose on both hosts;
and the screen-primitive pass composites over the finished 3D frame at the
near plane, so the sheets never depth-sort against scene geometry the way
retail's one ordering table sorts them (`view_z >> 5` is carried on the quad
for the day the pass can honour it). The gate the pass tests is the same
script word on both hosts (`World::fog.gate`, written by
`set_ambient_particles_enabled`), and the oracles are
`crates/engine-shell/tests/w1h_fog_gate_census.rs` (native) and
`crates/web-viewer/tests/w1h_fog_page_prims.rs` (page).

#### The volumetric ground fog is one bank, two shader transcriptions

The ground-fog enhancement ([renderer](../subsystems/renderer.md#volumetric-ground-fog-enhancement))
is simulated in exactly one place - `World::tick_fog_volume`, inside
`World::tick` - and switched by exactly one setting,
`OptionsState::volumetric_fog`, which both hosts push through
`apply_to_world` (native `F9`, the page's "Ground fog" box). What each host
owns is the draw, and the two draws differ only where the hosts' renderers do:

| host | draw site | matrix of the bank's space |
|---|---|---|
| native window | `stage_fog_volume` in `window/event_handler/redraw_passes.rs` -> `Renderer::set_fog_volume` (WGSL, `renderer/fog_volume.rs`) | field: the scene camera (already Y-negated); battle: camera x `battle_stage_model()` |
| play page | `_drawFogVolume` in `site/js/play-app.js` -> `webgl-fog-volume.js` (GLSL), fed by `play_fog_volume_header` / `_density` / `_mesh` | the frame's own VP x `diag(s, -s, s)`, `s = 1` field / the battle world scale |

A `SIM_PAIRS` row pins both draw sites to `World::fog_volume_frame`. The
recipe's numbers ride the frame (`FogSpace::shader_constants`, the header's
tail), so the GLSL carries no constant of its own to drift; the shader *body*
is the residue no gate reads - the two must be edited together. Both draw
after the 3D scene and before the screen-primitive layer, and neither draws
over a minigame venue. The soft intersection is the one step that differs by
construction: the native renderer samples its depth target in a pass split out
of the scene pass, the page blits its default framebuffer's depth into a
texture. Where the page wrote log-of-w depth that frame (`renderer.lastLogDepth`,
`LOG_DEPTH_GLSL`), its sheets write and test the same encoding and the fade
decodes `w = 2^(depth * LOG_DEPTH_RANGE)`; otherwise both hosts invert the
frame matrix's own depth mapping.

#### A battle backdrop that changes with the last step, not with the host

Both hosts pick the battle backdrop through one kernel,
`SceneHost::battle_stage_entry`: the stage variant the region reader stored on
the last field step (`_DAT_8007BD60`), else the scene's default stage stream.
So a fight forced before the player has taken a step (`--battle`, the page's
`debug_force_battle` at spawn) draws the default stream, and one forced after a
walk draws the region's variant. In `town0b` entered directly (no story flags)
those are a night stage (entry 15) and a daylit one (variant 3, entry 18): the
page's "daytime backdrop" was a fight forced after a walk, and the native window
draws the same daylit stage under `LEGAIA_BATTLE_STAGE=3`. Compare two hosts'
battles only from the same region state.

#### The field screen-effect wash had a producer and no consumer

The field VM's op `0x34` sub-0 arm spawns a colour-tween actor
([`cutscene.md`](../subsystems/cutscene.md#what-the-beat-looks-like-measured)),
and the tween's per-frame `FUN_80024EE4(a0, a1, packed)` call is retail's
scene-entry fade-from-black and its door prologue's fade-to-black. The port
simulated that envelope on **both** hosts and drew it on neither: the pool
published a push per frame, the arm was ported, tagged, entered by a ladder
and pinned by a disc-gated oracle, and no render surface read the pool.

None of the tiers could fail on it, and the reason is worth keeping: every
tier here compares two hosts. A feature absent from both is symmetric, so a
drift gate reports parity - correctly - while the frame stays blank. What
finds this shape is the reach export's producer/consumer join
([`reach-triage.md`](reach-triage.md)), not a parity gate.

Both hosts now composite it through one emitter:

| host | call site | source |
|---|---|---|
| native window | `handle_redraw`'s `screen_prims` assembly, `window/event_handler/redraw.rs` | `World::screen_tint_push_args` |
| play page | `tick_battle_intro`'s prim assembly, `play_battle.rs` | the same |

The emitter is `screen_prim::screen_effect_push_prims`, and it exists as a
named kernel because the three arguments are three separate ways to get the
wash wrong and two of them look right in a diff: `a0` is an ordering-table
bucket and `a1` an ABR equation - two `i16`s a call site can swap and still
compile - and `packed` is a **GP0** colour word with red in the low byte,
the opposite channel order from every other kernel in this module. A
scene-entry fade ramps through grey, where a lost swap is invisible.

Ladders: `crates/web-viewer/tests/w4b_screen_effect_page_prims.rs` enters a
shipped scene whose own entry script issues the instruction and reads the
page's uploaded primitives back (count, blended run, non-black colour); the
native call site is held by the tier-3 row, its draw section living in a
`bin/` target no integration test links.

### The two hosts do not share a shading law

The 3D geometry is shared - the same TMD, the same mesh builders in
`legaia-tmd`, the same camera matrix out of `battle_cam_script::battle_vp`.
The **fragment arithmetic** is not: `engine-render` ships WGSL and the browser
ships GLSL, written separately, and nothing pairs them. Tier 1 is silent here
by construction, because both hosts do reach a builder; tier 2 pairs named
constants, and a shading term written into a shader body is not one.

The law, on the native side, is stated in
[`renderer.md`](../subsystems/renderer.md): a textured prim is
`texel * packet_colour / 128` through the GTE depth cue, an untextured prim is
its packet colour directly, and **neither applies a light source**. The
synthetic Lambert is a viewer aid.

The browser's fragment shader (`site/js/webgl-shaders.js`) used to apply a
Lambert term off the screen-space geometric normal on *both* paths. On the
untextured path it was a visible divergence rather than a subtle one: a battle
stage's sky dome and mountain arc are flat/gouraud panels that sweep through
every azimuth, so `0.45 + 0.55 * dot(n, -light)` painted repeating vertical
lighter bands across them that the native window does not draw.

Both paths are now the retail law. Neither host applies a light source, and
the browser has no light uniform left to apply one with - `u_light`,
`u_normal_sign` and the world-position varying they needed are gone.

The overworld's **vertex** stage is the same shape of pair: the per-vertex
curvature (`overworld_curve_clip` / `overworldCurve`) and the continent's flat
bucket depth (`overworld_flat_depth` / `overworldFlatDepth`) are each written
once in WGSL and once in GLSL, both reading the frame's `clip.w`-to-`SZ`
factor. The CPU kernels they mirror are pinned against retail
(`overworld_curvature`, `overworld_draw_order`), and the WGSL flat depth
against its kernel on a GPU (`engine-render`'s `overworld_flat_depth_gpu`);
the GLSL twin is compiled but not run by any test, so an edit to one shader
has to be carried to the other by hand.

The ground's depth cue joins the same pair (`overworld_ground_cue` /
`overworldGroundCue`, [`overworld_ground_cue`](../../crates/engine-core/src/overworld_ground_cue.rs)):
each re-projects the cell's `(x1, z0)` corner from the flat-depth references
and runs retail's `DPCS` arithmetic toward the literal far colour on the packet
colour. The WGSL cue is GPU-tested in the same file. The GLSL twin has a
host-free check, `web-viewer`'s `overworld_ground_cue_glsl`: it reads the
function out of `site/js/webgl-shaders.js`, requires the same corner, lifts the
literals from each statement of a fixed shape and evaluates that arithmetic
against `ground_cue_color` over every 16-bit `SZ1`. It does not execute GLSL,
so a new term fails the shape match rather than being evaluated.

### What the textured half needed, and why it was not a one-line removal

The Lambert was standing in for something the page did not upload. Retail
modulates each texel by the prim's baked colour word; `VramMesh::colors` has
always carried it, and every browser exporter threw it away and sent
`[255, 255, 255]` for textured verts. Dropping the term alone would have left
every textured surface at flat full brightness - a *different* wrong answer.

The stream is `a_flat_rgba`, and it now means one thing everywhere: **the
prim's packet colour**, with the alpha byte saying which job it does.

| flag | meaning |
|---|---|
| `255` | textured - sample VRAM, then `texel * rgb * 255/128` |
| `0` | untextured - fill with `rgb` |

Three traps sit in that redefinition, and all three have already fired:

**The hybrid builder reads two arrays, not one.** An untextured vert's colour
is its *fill* and only `VertexShading::colors` carries it; a textured vert's is
its *modulation* and only `VramMesh::colors` carries it - `VertexShading`
reports white there by design. `crates/web-viewer/src/packet_color.rs` is the
one place that joins them, with a regression test that a textured vert does not
come back white.

**The unbound-attribute constant is 0x80, not white.** A draw with no colour
stream reads the context-global generic attribute, and white there is
`texel * 255/128` - every un-coloured mesh ~2x too bright. `TmdRenderer`'s
`_setNeutralPacketColor` is the only place that value is written.

**A page that fabricates vertices has to invent a colour word, and zero is not
dead.** Baka Fighter's tiled arena floor wrote `[0, 0, 0, 255]`; under the old
shader the RGB lanes were ignored on the textured path, under the new one the
floor multiplied to black and the arena lost its ground. Muscle Dome's battle
grid wrote white, which is the 2x case. Both write `0x80` now.

**A stream can be white without any of the three above.** The Muscle Dome's
three bodies - the arena shell, the assembled fighter, the monster - each
exported a hand-written stream. Two returned `vec![255u8; n * 4]` outright;
the third built a hybrid stream but read `VertexShading::colors` for both
halves, which is trap one wearing a different coat. None of them reached
`packet_color`, so the sweep that converted the exporters never saw them, and
the page drew its whole arena at `texel * 255/128`.

What let it sit there is that the wrongness is invisible in the shape of the
data. The accessor tests assert `flat_rgba().len() == n * 4`, and a white
stream satisfies that exactly; on screen `texel * 2` reads as *over-lit*, so
the frame does not point at a dropped colour word either. The check that
catches it has to assert the stream's **content** - that a textured vert does
not come back white - which is now a disc-gated test per body
(`muscle_web_real.rs`). The transferable rule: for a stream that is allowed to
be uniform, length parity is not coverage.

Meshes with genuinely no packet colour - the generated walk-ground heightfields
- keep the neutral constant and draw at the raw texel, which is the honest
answer for geometry that has no colour word to modulate by.

### The `.glb` export is a fourth surface, and it had the same hole

A downloaded model is a render the project ships without ever drawing, and it
sat outside every tier: no gate pairs an exporter against a shader. The same
omission that had the browser sending `[255, 255, 255]` for textured verts had
the three `.glb` exporters emitting no `COLOR_0` at all, so a model whose
colour lives entirely in the packet word - a summon's sword blade is a
near-white texture ramp tinted per vertex - exported as flat white while the
canvas beside it drew a flame gradient. The word is not a fine-tuning term on
this data: across the 32 summon casts, the raw texel differs from
`texel * colour / 128` by **12%** of full scale on the average vertex and by
up to **90%** on individual ones.

Two lessons the shading tiers already imply and this repeated. Presence is not
content: a `COLOR_0` accessor full of `1.0` passes any "does the attribute
exist" check while carrying nothing, so the assertions compare *values* against
the stream the canvas uploads. And the encoding has to admit the over-bright
tail - `0xFF / 128` is `1.99`, which a normalized-ubyte attribute cannot hold,
so the accessors are float. Convention + reasoning: `legaia_asset::gltf_color`.

The hole had a second, subtler layer: **colour space**. The canvas multiplies
display bytes; a glTF viewer sRGB-decodes the texture, multiplies `COLOR_0`
in linear light, and re-encodes - so shipping the raw display-space ratio
rendered every dark packet word a gamma stop too bright (near-black `0x18`
clothing fills drew as mid-gray, ~50/255 of error on dark words), while the
site viewers beside it stayed dark. The same value-level tests that catch a
dropped stream did not catch this, because both sides carried the same
numbers - the defect was in what the number *meant* to the consumer. The
exporters therefore bake `COLOR_0` through the sRGB EOTF
(`gltf_color::srgb_ratio_to_linear`), and the probes decode with the inverse
before comparing words.

### A draw list can reach every host and still carry half a mesh

The textured / vertex-colour split is a second axis the tiers do not see. A
Legaia TMD mixes two prim families, the builders come in pairs
(`tmd_to_vram_mesh` for the textured half, `tmd_to_color_mesh` for the
untextured), and a surface that runs one builder gets a mesh that parses,
uploads, draws and passes every count-shaped assertion - with its roofs
missing. The world-map stamps sat there on **both** hosts at once: the
overview page fed its `pack_mesh_*` accessors the textured builder alone
while the field-scene page next to it used the hybrid kernel, and the native
window's world-map branch drew `world_map_terrain_draws` off the textured
list only while its field branch drew both halves. Rim Elm's four huts
rendered as open rings on every surface, which is what let it read as
"authored that way" rather than as a dropped family; the Uru Mais temple,
whose mesh is colour prims only, was not drawn at all.

The retail answer is one dispatch row: `FUN_80043390` picks the per-prim
renderer by the group's `flags >> 1`, and the untextured slots are populated
in the world-map overlay's table as in the SCUS one
([world-map.md](../subsystems/world-map.md)). The port answer is one kernel
per surface family - `scene_assembly::build_hybrid_pack_mesh` for the pack
stamps, the colour-mesh bridge for the native branch - and a disc-gated test
per surface that asserts the **untextured corner count**, not the total
(`world_map_pack_hybrid_real.rs`, `world_map_pack_untextured_real.rs`). A
total passes with either half missing.

### The monster bestiary is a viewer, and says so

`site/_content/monsters.html` carries its **own** WebGL program with a
two-sided key light and a gamma curve, and it keeps it. That page is a
bestiary card, not a game frame, and its preview is the browser sibling of the
asset-viewer's `MESH_SHADER_SRC`. Counting shader programs rather than pages is
what keeps this from reading as a missed conversion: the site ships four, and
only the shared `TmdRenderer` one claims to be retail.

The transferable point: **two hosts reaching one builder is not two hosts
computing one thing.** Where the shared artefact is *data* (a draw list, a
matrix, a rect) the gates can pair it. Where it is a *law* expressed twice in
two shading languages, only a rendered frame from each host, at the same scene
and the same camera, can compare them.

## Field script actors reach both hosts through one accessor each

The field VM's op `0x43` scripted arcs and op `0x34` sub-1 attached lights
([`script-vm.md`](../subsystems/script-vm.md#0x34-sub-1-is-an-attached-light))
are simulated once, in `World::script_actors`, and each host only reads.
NPC height is the one read a host could get wrong silently: the NPC position
map carries X / Z, so a host that samples the floor under an NPC itself draws
an arcing NPC on the ground with every tier green. Both place NPCs through
`World::field_npc_render_y` - the native window's field NPC pass and the play
page's `play_npc_transforms` - and the lights ride each host's screen-prim pass
through the tier-7 rule above. The arc's follow camera (`FUN_801DB510` /
`FUN_801DAA50` from the release watcher) is engine-side too: the shared
`Camera` tick reads `World::script_arc_follow_camera`, so both hosts' follow
views take it without a host line.

## Three one-host decisions moved onto one kernel

Each of these was once listed as a gap no gate fails on. Each was one
decision with two implementations, and each now has one.

### The overworld markers are screen primitives on both hosts

The world map draws a kind-coded post and base cross at every placed entity,
and a facing-ticked post for the player while the party leader's mesh is
missing. It is a port marker, not a retail draw (retail binds each placement
to its own actor model, which is still open). The native window used to draw
it as world-space **lines** through the renderer's line pipeline, which the
browser play page has no counterpart for.

`legaia_engine_core::world_map_markers` now owns the whole marker: the
segments, colours and sizes, and the projection through the frame's own
`camera_view::frame_vp` at the display's 4:3, emitting each segment as one
quad (two triangles) one display pixel wide. Both hosts wrap the quads through
`engine-ui::screen_prim::world_map_marker_prim` onto the screen-primitive pass
they already share - the native `world_map_marker_prims` and the page's
`play_world_map_markers` - and both resolve the camera with
`resolve_field_camera(.., None, ..)`, so the draw step never advances a
cutscene glide. The line pipeline is left carrying only the env-gated slot-4
inspection wireframe, which is a diagnostic rather than a render path.

The one input each host answers locally is whether the party leader's mesh
drew: the native window from its upload set, the page from whether its player
rig resolved.

The move cost the markers their occlusion, on both hosts at once: the line
pipeline they left was depth-tested, and the screen-primitive pass had no
depth channel. What the marker stands in for - the placement's own actor
model - goes through the ordering table with the terrain, so a nearer
mountain hides it. The channel is back on both hosts: the kernel's quads carry
their corners' scene depth through the frame matrix (`MarkerQuad::depth`),
`ScreenVertex` carries it with `screen_prim::FLAG_DEPTH_TESTED`, and both
passes run the depth test armed for the whole list with no depth write. The
native overlay already drew inside the scene render pass with the depth
attachment bound, and maps the depth through the scene's reversed-Z remap
(`1 - z`); the page's pass draws into the default framebuffer the 3D pass
just filled and puts the depth straight into `gl_Position.z`, the value its
3D shader produces from the same matrix. The fog sheets take the same
channel on the field and the overworld alike (`fog_puff_prim`'s depth: the
particle's own on the field, flat at the sheet's ordering-table bucket on the
overworld), since retail links them into the ordering table the meshes sort into;
the continent's cells draw at their own buckets' depths on both hosts, from
one kernel (`legaia_engine_core::overworld_draw_order`, see
[`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#closing-the-draw-order-flat-per-primitive-terrain-depth)).
A screen-prim depth is only comparable with the mesh pass when both are taken
at the matrix the meshes draw with: the overworld walk frame composes a 6x
world scale the field view the fog projects through does not, and the sheets'
depth sat six times too near until `World::field_fx_view` scaled it. Every other primitive keeps the flag clear and sits on the near
plane, so it passes against any scene depth as before. Pinned by
`screen_prim`'s `only_a_depth_carrying_quad_is_depth_tested`,
`world_map_markers`' `walk_camera_quads_carry_scene_depth` and
`fog_particles`' `field_and_overworld_sheets_carry_a_scene_depth`.

### An overworld label enters the world map on both hosts

Which entry a scene takes is the scene's own property, and one engine
predicate answers it: `scene::is_world_map_scene` (the three `mapNN`
labels). The page's `enter_field` and the in-world door transition both route
an overworld label through the world-map entry by name; the native
`play-window --scene` only did so under `--world-map`, so `--scene map01`
entered the overworld as a plain field scene. The window now asks the same
predicate, and the flag only forces the world-map entry for another label.

### The native window consumes the camera resolver

`compute_scene_camera` used to answer "which camera owns this frame" from its
own `match`, reaching `camera_view::resolve_field_camera` only for the world
map. It now hands the resolver the glided cutscene view and draws whatever
`frame_vp` returns, for the cutscene, both world-map vantages, the field
follow camera and `HostDebugOrbit` alike - the call the page makes.

Two arms stay in the window, and neither is a second answer to that question.
**Battle** has its own kernel (`battle_cam_script::battle_vp`, stepped against
the battle phase model in `window/battle_cam.rs`), which the page runs too; a
`FieldCameraFrame` variant for it would carry the battle phase state and share
nothing the kernel does not already share. The **`F3` debug orbit** is this
host's own vantage, as the page's orbit is its own.

### The boot Options screen is the pause menu's, on both hosts

The retail options screen is a framed pause-menu sub-screen. The page's title
always reached it through the menu runtime; the native title did **not**. It
installed a bare `OptionsSession` and painted its rows unframed at a fixed pen
(`window/boot_cutscene.rs`), so the native window drew the screen once, in a
form the page never had. (This page used to say the native window drew it
*twice*, framed and unframed, and that the window's boot-UI tests read the
fixed-pen copy; neither was so - no test read it, and the framed screen was
never reached from the title.)

The native title's Options row now opens the pause menu straight onto the
Options sub-screen through the picker's own confirm routing
(`open_menu_row_from_title`, the twin of the page's `play_menu_open_row`), and
the unframed copy and its `BootUiState` arm are gone. No fallback is kept: a
disc with no parsed menu window table still frames the screen, at the
`MENU_WINDOW_FALLBACK` rects.

Neither route is reachable from retail's title, and that is the larger
correction: the title menu is **two** rows. Its tick wraps the row counter
with `andi v1,v1,0x1` at `0x801DDC00` and its confirm arm branches on row `0`
against everything else (see `ghidra/scripts/funcs/overlay_title_801dd6b8.txt`),
so `TitleSession` steps a two-row space (`TITLE_MENU_ROWS`) and never yields
`TitleOutcome::Options`. The screen retail reaches Options from is the pause
menu. Both hosts' title arms for the outcome are the same route regardless,
so the enum's third answer cannot mean two things.

Both hosts carry one flag for a title-opened menu (`menu_from_title` natively,
`PlayMenu::from_title` on the page): the sub-screen's exit calls
`resume(true)` and the title comes back, instead of a root picker the title
never showed. The page sets it for the title's Continue row too - the native
window runs Continue as a standalone save-select, whose back-out also lands on
the title. Continue used to skip the flag because a card Load parks its scene
label on the menu for the page to collect, and closing would drop it; the exit
now keeps the menu open exactly when a label is parked, and the page closes it
once it has taken the label. Without the flag a backed-out Continue landed on
the pause root.

## A shared builder can be starved by its caller's slice

Tier 1 asks whether a host *reaches* a builder. Nothing asks whether it hands
the builder the same **inputs**, and a builder given a short slice does not
fail - it produces a smaller answer that reads like a deliberate fallback.

The battle HUD's nine status-element badges are the worked example. Six decode
with the system-UI sheet TIM's own sixteen palettes; `Stone`, `Rage` and
`Faint` sit on row-511 sub-palettes 16 / 17 / 18 and decode with nothing but
the CLUT-only continuation TIM one file *earlier* in `PROT.DAT`
(`save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET`). The native window roots its
atlas slice there; the browser rooted its at the sheet, which puts that TIM
behind the slice start where `build_atlas` cannot reach it. Both hosts called
the same builder with the same signature, the bake succeeded on both, and three
of nine cells came back `None` on one of them - whereupon the HUD did what it
is supposed to do with a `None` cell and drew its labelled text tag.

The measurement, on the real disc: **9/9 badges ext-rooted, 6/9 sheet-rooted.**

What makes that assertable is the contrast, not the count. `crates/web-viewer/tests/battle_hud_badges.rs`
bakes the atlas from both bases and requires the second to resolve *strictly
fewer*, so the test keeps measuring the base choice rather than silently
passing if the builder ever started inventing cells. A count alone would also
have passed on the day the constant was wrong, since 6 badges is a perfectly
plausible number.

The generalisation: when two hosts share a builder whose output *degrades*
rather than errors, the honest oracle runs the builder twice - once the right
way, once the suspected wrong way - and asserts they differ.

## The map viewer animates off a live world, not a bake

The site's game-world page and the asset viewer's full-map button draw a field
scene through `web-viewer::field_scene`. Every tier above treats it as a
render surface (tier 7 derives it), and every one stayed green while it showed
a different scene from the play hosts: it baked the draw lists once, against
the floor-height ladder the MAN header ships, and ran its own copy of the
ambient-tree spawn in a private `World` beside them. What the scene's scripts
move on the world tick never reached it - the ladder waves (`jouina`,
`tunnela`, Rim Elm's tide), `concnow`'s entry-time ladder replacement (its
flesh mounds stood flat), the prop bank's clips (the Rim Elm windmill, whose
parts the viewer did not even pose at rest, so they heaped on its hub), and the
op-driven VRAM effects only a running world issues. No gate could see it: the
missing input was a world, not a call.

The viewer now owns a world: `engine-core::scene_live::LiveScene` enters the
scene through `SceneHost` the way the play pages' picker does and ticks it
headless, and the viewer reads it through the play page's own kernels
(`FloorWave`, `World::placed_floor_offsets`, `field_ground::live_render_positions`,
`PropAnimBank::pose_key` with `field_env::posed_prop_offsets`,
`World::step_field_vram_effects`, `World::morphed_env_tmd`). Its private
ambient spawn is gone. Its actors are the play page's too: the
`play_npc_*` state and answers live in one type,
`web-viewer::field_actors::FieldActors`, which both pages hold over their own
world, and `site/js/field-actors.js` is the one JS path that poses and places
them. The gate is behavioural, not textual: the disc-gated
`crates/web-viewer/tests/field_scene_anim.rs` runs the viewer and
`LegaiaRuntime` side by side and requires the floor-wave offsets, the ground
positions, every prop's pose key, every actor's transform and clip state, and
every VRAM texel the animation writes to agree tick for tick. VRAM is compared as the **written** set, not the image:
the play runtime uploads pages of its own over the scene-host VRAM the viewer draws,
so the two images differ before the first tick.

## Five shapes a side-by-side read finds that no tier fails on

A domain-by-domain read of the three hosts' per-frame steps *and* draw passes,
taken with every tier above green, sorts its findings into five shapes. Each
is named here for the shape, with one worked anchor, because the shape is what
the next sweep should look for - and because four of the five are invisible to
every gate by construction.

### One animation, two clocks

The same kernel advanced on the simulation tick by one host and on the
animation frame by the other. The two agree exactly at 60 fps and nowhere
else, so nothing about the code is wrong to read and the difference only
exists at run time - which is also why the *symptom* names the display and
not the code: the name-entry caret blinks at half speed on a 120 Hz monitor
and at double on a throttled tab.

That caret is the plain case, and the fix is the shape's general one: a modal
overlay freezes the field tick, so the host has to spend the frame's sim
steps on the frozen clock instead of letting the overlay ride the refresh.
[`site/js/play-app.js`](../../site/js/play-app.js) drains its fixed-timestep
accumulator once per display frame, above the overlay arms rather than
inside the tick branch, and hands the count to the name-entry step - the
press lands on the first step and the rest only advance `World::frame`, which
is exactly what the native window's catch-up ticks do. The battle move-FX
streak schedule and the target-cursor pulse phase are the same shape and are
still on the worklist.

### One decision, two inputs

A per-frame predicate both hosts run, fed from different state. Tier 3 pairs
the *kernel*; the operand set is per host, so the gate is silent.

The camera-occlusion fade's arming gate is the anchor, and it shows what the
fix has to look like: splitting the predicate into the half the **world** can
answer and the half only the **host** can. `field_occlusion::fade_armed` holds
the world half (field mode, no scripted shot owning the camera) beside
`player_body_centre`, which is the point the gate ray-casts to *and* the focus
the shader takes - one kernel, so the two cannot name different points. What
stays per host is genuinely per host: a master toggle, a boot or pause UI
owning the screen, a debug vantage, a VR first-person eye.

Leaving the world half per host is what produced the divergence: the native
window excluded the boot UI, the world map, the cutscene camera and the debug
orbit, while the page excluded only battle and the minigames, so a pause menu
or a scripted shot kept dissolving the walls behind it.

### A GL state word the engine does not own

The page sets some frame state in JS where the native window takes it from the
world. No Rust symbol is missing, so no tier can name it, and the only
durable fix is to move the *decision* into the engine and leave the GL call
as the thing that applies it.

Retail's GTE **NCLIP** winding rejection is the worked case:
`camera_view::nclip_cull_mode` is the one place that says when it arms (the
in-engine cutscene camera, off the overworld), the native window hands the
word to `Renderer::set_backface_cull` and the page hands the same word to
`TmdRenderer.setNclipCull`. Before that the page's assembled pass called
`disable(CULL_FACE)` unconditionally. The battle clear colour - the sky on one
host, a hard-coded near-black in
[`site/js/webgl-tmd.js`](../../site/js/webgl-tmd.js) on the other - is the
same shape and is still open.

The page's prologue grade was a third instance and the subtlest, because
nothing was missing that a reader would look for: the grade's *multiply* half
reached the page, and its **palette-collapse** half - the law retail applies
to the scene's uploaded CLUT entries and TMD packet words at load - had no
uniform in [`site/js/webgl-shaders.js`](../../site/js/webgl-shaders.js) to
land on. The engine had composed both arms and exported both; one of the two
had no consumer, which reads in a diff as a fully wired feature.

### A whole pass one host has no uploader for

Not wiring: a surface that exists on one host only, because the other has
nothing to draw it with. The Baka cabinet's digit strips and payout sheet are
the project-sized one left. The world-map entity and player markers were on
this list as a line overlay; what took them off was emitting them as the quads
the page's screen-primitive pass already draws
([above](#the-overworld-markers-are-screen-primitives-on-both-hosts)). The retail dance HUD frame was on this list and is not any
more, and what took it off was not a new uploader: the frame is text, and
every decision in it (digits, `Lv.` label, which beat cells) had simply been
spelled out inside one host's draw block. Moving the resolution into
`DanceGame::hud_frame_rows` left each host three lines of layout. Before
calling a one-host surface a project, check whether the other host is missing
the *paint* or only the *decision*.

### A host-side handle the world does not hold

A per-battle or per-scene resource each host allocates for itself, which the
engine's own teardown cannot release because it never knew about it. The
browser's summon actor seat is the worked example: `World::finish_battle`
restores the engine actor table, the host-side slot index survives it, and the
next fight hands the same seat out twice.

## Shapes a "the page is missing it" reading gets backwards

A side-by-side read produces sentences of the form "the native window does X
and the page does not". Several live cases were the other way round, and each
was only visible from the *event*, the *sign* or the *sink* - never from the
two call sites. Two are written out below; three more are in
[what a one-host feature can also mean](#what-a-one-host-feature-can-also-mean).

### The feature was dead on both hosts

The native window carried an arm for `apply == 0` Camera Configure beats - the
snap beats retail's mover commits immediately - and the page carried none, so
it read as a page gap. It was neither: `Camera::route_camera_events` consumes
every `FieldEvent::CameraConfigure` off the world queue during the session
tick and does **not** restore it to `pending_field_events`, and the window's
arm sat in its own later drain of that same queue. The arm could not fire, the
bank it filled was always empty, and the replay it fed was a no-op.

The lesson generalises past cameras: where a host watches a queue another
layer already drained, "one host has the arm" says nothing about whether
either host has the behaviour. Follow the event to the drain that consumes
it - the bank now lives on `Camera` itself, beside the consumer, which is
what makes both hosts' replay reach it.

### The hand-off was the bug

The `F3` debug orbit is one vantage on both hosts, and only the page handed
the engine camera anything on the way out of it - which reads as the page
compensating for something the window does not need. It was: the window
composes its vantage as `fixed diagonal + Camera::manual_orbit` and its drag
writes that same field in both modes, so nothing drifts and nothing needs
reconciling. The page kept a private yaw while the toggle was on and then
wrote its **negation** into `manual_orbit` - and since the two track together
with the toggle off, cycling `F3` flipped the orbit by twice its value.

`Camera::debug_orbit_by` is the shared setter now (un-gated by the cutscene,
because a dev vantage ignores whoever owns the scripted camera, but still
field-only so a drag on the overworld cannot rewrite the locomotion compass).
A host-side hand-off between two representations of one quantity is worth
reading as a defect report rather than as parity work: the fix is usually to
delete the second representation.

## What a one-host feature can also mean

Three more rows of a side-by-side audit turned out not to be page gaps. The
pattern each one breaks is worth naming, because all three read identically
from the two call sites.

### The native was the host projecting through the wrong camera

The move-FX streak and the weapon trail were listed as "native projects them
through the fallback orbit camera, the page through the phase camera". True,
and the native is the side that was wrong: its *scene* pass already chose
between `battle_dome_camera_mvp` (a stage-dome fight) and `battle_camera_mvp`
(an auto-framed orbit for a stage-less one), while both FX passes called the
orbit unconditionally. In every scripted fight that projected the swing ribbon
through one framing and the swordsman holding it through another.

`battle_scene_mvp` is the one selector now, and all three native passes take
it. A trail is attached to a body; where the body's camera is chosen is where
the trail's has to be chosen too - and a second call site that "happens to
agree today" is the shape to look for, not a second value.

The same row had a sibling: the native gated its phase-camera *tick* on
`battle_stage_mesh.is_some()`. That puts a host render resource in the arming
condition of a simulation kernel, so the two hosts stepped the same script on
different frames and a fight whose stage failed to build ran with no camera
state at all. The gate is `world.mode` on both hosts now; which matrix a
stage-less battle renders with stays a draw-side question.

### The sink was already dead

The dance HUD's *quad* half once read as a native-only layer while the
native's own materialiser returned an empty list - no host staged the dance
4bpp page, so the layer drew nothing on the host that had it, and wiring it
into the second host would have produced a second empty list and a green row.
What closed it was the art, not the draw: both hosts now stage the page and
publish one residency claim (`World::minigames.dance_hud_art_staged`) that
both draw paths read.

The Muscle Dome hub has the same shape one level up. `HUB_TITLE_ART` and
`HubScreen::opponent_card()` read as minigames-page-only features, and both
sit behind generic screen-index accessors that page's draw loop never selects:
it calls the quad export with screens 0, 2, 3 and 4 and the envelope export
with 0, 1 and 3, so the title art (quad screen 1) and the opponent card
(envelope screen 2) are reachable from a test and from nothing else. Adding
them to a second host would have added a second unreached arm. A one-host
*export* is not a one-host *screen* - check which arguments the draw loop
actually passes.

### The two hosts were running different engines

`catch_hud_draws` takes a `depth`, and two of three hosts passed a literal `0`
- which read as a shortcut until the cause surfaced: those two drove one
session type and the third drove another. Both modelled the same minigame; only
the second carried retail's line depth `DAT_801d9298`. The "missing" value had
nowhere to come from. The fix that mattered was not a depth field on the first
engine but deleting it - [one session type](#one-minigame-one-session-type).

## One minigame, one session type

Fishing was modelled twice in `engine-core`: a deterministic cast -> fight ->
done loop on the native window and the browser play page, and `PondSession` -
the retail loop with a shore idle, cast wind-up, lure flight, the pre-hook band
roll off the spawn page, the fish behaviour sub-state machine off `BiosRand`,
line record and depth - on the minigames page. The first picked its species
from the locked cast power and pulled at a steady rate, so the two play hosts
and the minigames page played two different games while every gate stayed
green: each host was internally consistent.

`PondSession` is now the only session. The play hosts reach it through one
engine entry, `SceneHost::enter_fishing_from_overlay` - the door warp's own,
which both debug launchers (`L` natively, the play page's Fish button) now
call - and it does four things neither launcher did before:

- decodes the species, spawn and cadence tables together
  (`fishing::FishingTables`), so a play host can hook off the spawn page;
- runs the bring-up's rod scan *and* the lure gate, writing both corrected
  indices back, so the old per-launcher fixed rod stat is gone;
- seeds the session from the persistent save-block words on
  `World::minigames` (`fishing_points`, `fishing_best_points`,
  `fishing_best_fish`, `fishing_lure`, `fishing_rod`, `fishing_casts`,
  `fishing_prizes_purchased`), which `exit_fishing` banks back in full - the
  migration for what used to be a points-only bank plus a cast counter one
  host wrote and another host's session never read;
- picks the venue from the departure scene the way the driver's setup state
  does (`fishing::venue_for_departure_scene`) and attaches the `other1`
  venue map the minigames page casts into (`SceneHost::fishing_venue_map`), so
  the lure's walk-grid drift and water class come off the same bytes on every
  host. The native window's own lure actor, and its cast-counter increment,
  are gone with it.

`World::tick_fishing` drives the session from the pad (Circle casts and locks,
Cross / Square reel, D-pad and reel edges feed the strike credit) and parks
each frame's `PondEvent`s on `World::minigames.fishing_events`. Both play hosts
seed their banner one-shots from that list and the minigames page from its own
tick's return - one event-to-banner map, including the recast banner the
minigames page used to derive from a phase diff. The catch HUD's inputs are
one derivation, `PondSession::catch_hud`, on all three hosts; the play hosts
used to hand it a fight's reel progress as the line record.

What stays per host is what each host owns. The minigames page drives the
session directly, with no `World`: it has no field scene to suspend, and its
persistent words come from the visitor's memory card. The two play hosts run
the venue actors (the wandering fish, the line actor, the floor solve and
camera publish) through one engine kernel,
`fishing_venue::tick_fishing_venue_on_host`, which reads the session's events
and its lure; the minigames page has no venue scene and no venue pass. The
play hosts' status rows are one engine text (`PondSession::status_rows`) with
each host's own key names. The strike credit's pad nudge is one kernel on all
three, `fishing_actors::bite_pad_nudge`: `World::tick_fishing` counts it off
the engine pad and the minigames page's `fishing_pond_tick` off the packed
pressed word its script assembles.

### The venue hub: one session kernel, three hosts

The venue's five-row menu, its two help pages and the tackle list open from
the idle shore on Triangle / Select, as retail's state `0x0C` does, and they
live on the session itself (`PondSession::hub_step`,
`engine-core::fishing_hub`), so all three hosts reach the same screens by
stepping the one session they already share. The layout is one kernel
(`FishingHub::lines`, disc text off the overlay through
`FishingHubText::from_overlay`) and the draw is one composition
(`engine-ui::ui_fishing_hub`, over `ui_text_lines`).

What stays per host is what each host owns. The two play hosts run the hub
through `World::tick_fishing_hub`: the tackle screen counts the live bag,
row 3 opens the world's exchange sub-screen, row 4 leaves the venue as each
host's quit key does. The minigames page has no bag and no world exchange: its
tackle screen sees every tackle item as held (the stand-in its HUD's lure
count already uses), row 3 hands back to the menu and scrolls the page's own
prize panel into view, and it draws the `fishing_hub_json` lines with the
browser's font. On every host the help footers' `0xCE` button escapes draw
no glyph, and the cursor icon is the `>` stand-in.

## The Baka cabinet's ladder: one on the field hosts, another on the standalone page

The native window and the browser play page run the retail ladder: the
cabinet shell (`baka_cabinet::BakaCabinet`, the `FUN_801CF388` port) that
`BakaFight` carries takes the packed pad edge once a match is decided, walks
the tally into the "NEXT GAME / PAY OUT" choice, seats the next rung through
its install state (`BakaFight::install_rung`) and leaves through the exit
state, whose end is the return warp that banks the coins. The ladder never
outlives the mode-24 visit - every cabinet exit runs through state `0x1F4` -
so it needs no save representation; see
[`minigame-baka-fighter.md`](../subsystems/minigame-baka-fighter.md#the-ladder-in-the-port).
Both hosts draw the choice sheet and the round chrome (`BakaChrome`: intro
card, ROUND banner, countdown) through one label kernel pair
(`baka_cabinet::choice_sheet_labels`, `baka_fighter_chrome::chrome_labels`,
emitted by `ui_baka_strips::baka_widget_label_draws_for`), and both play the
chrome's announcer line through their CD-XA clip path. The play page follows
a newly seated rung by bumping its scene generation, so the opponent's mesh
and duel VRAM are rebuilt.

The **front end** reaches all three hosts through one kernel. The scene
host's mode-24 arm boots every fight at the cabinet's attract card
(`BakaFight::with_attract`), so the native window and the play page open on
the title card and the player select rather than on the duel as Vahn; the
cabinet emits its own widget cells (`BakaFight::cabinet_cells` - PRESS START,
PLAYER SELECT, the choice sheet) which both play hosts label through the same
`choice_sheet_labels` call, and the duel surface poses the select camera and
lineup (`baka_duel_scene::SELECT_CAMERA` / `SELECT_LINEUP`). The standalone
page opens the same cabinet (`baka_start_cabinet`, `baka_cabinet_pad`,
`baka_cabinet_json`), draws it through the same surface with the sheet art,
and hands the seated pick to its own ladder run once the cabinet leaves the
front end. Two shapes the per-host versions had wrong: the page fitted its
select camera and line-up by eye and drew cursor arrows retail does not
draw; and the page fired the duel's hit a second time by event name in JS
while `baka_tick` already drained the same cue into its SPU.

The standalone minigames page runs the same cabinet for the whole run.
The cabinet's per-frame step is a `World`-free kernel, `BakaFight::frame`
(engine-minigames), which the world tick calls for both play hosts and the
page calls through `baka_frame`: front end, duel throw, result tally, the
NEXT GAME / PAY OUT sheet (drawn from the cabinet's own cells), the next
rung and the score-gated secret rungs. The page used to leave the cabinet
after its select for a ladder of its own, `LadderRun` behind `baka_run_*`,
with a second pot, an HTML choice menu and a free starting rung; that model
is deleted, and the page keeps only the winnings accumulator the play hosts
keep on the world. Both seed a cabinet with `BAKA_RNG_BASE` folded with a
frame count (the world frame, the page's stepped-frame count), as slots
rack on the overlay's literal `SLOT_RNG_SEED`.

Still disclosed on the two field hosts: the in-duel pause menu (`0xBE` /
`0xBF`) stays unreached: the port feeds the cabinet a zero pad inside the
duel, since the duel state's pause edge `0x110` includes Triangle, the
button the round setup's cameo test reads held. The digit strips are wired on both
(`baka_fighter_chrome::hud_digit_placements` under
`ui_baka_strips::baka_digit_strip_draws_for`). The cue queue was fixed
separately: the minigames page never drained `BakaFight::cues`, so its duel
was silent while the queue grew for the length of a run.

### The standalone page has no World to tick through

The same page advances `DanceGame` directly (`dance_tick` -> `advance`)
rather than through `World::tick_dance`, which also runs the pre-song
count-in, the cue SFX and the mode fallback. It is not a missed call: that
page has no `World` and no `SceneHost` in its loop at all - it drives
standalone rules engines against JS state. Blocking capability: a `World` on
the minigames page, which is the whole play-page runtime, so the practical
answer is the reverse - fold the standalone games into the play page and
retire the second host. Until then, a kernel that hangs off `World` reaches
two hosts and not three, and that is what a `host_only` row on this page
means.

What the page does share is every `World`-free kernel under those ticks. The
slot machine (`SlotMachine::frame`), the Baka cabinet (`BakaFight::frame`) and
the dome's command flow (`MuscleDomeSession::select_input`) are one step on
all three hosts. The dance floor's bodies are posed by the run's own clip
driver (`DanceGame::advance_body_clips`, stepped every frame through
`dance_body_clips_tick`, count-in included) through the engine's cast surface
(`DanceCastSurface::frame`, `dance_scene_*`), not by a page-side animator
that picked its own moves.

### Ringside still on the standalone dome page

The Muscle Dome hub's ringside still
([`ringside-still.md`](../formats/ringside-still.md#in-the-port)) is drawn by
the native window and the browser play page, and not by the standalone
minigames page. The two play hosts share every piece of it -
`muscle_ringside::HubBackdrop` for the level, `ringside_backdrop::ringside_still_quads`
for the packets, `still_sheet_rgba` for the sheet - and both arm it off
`MinigameState::muscle_ringside_still`, which `World::exit_muscle_dome` writes
at the leg's end. That is the blocking capability: the standalone page has no
`World`, so no leg of its ends through the pick, and its INTERVAL screen is a
stateless per-tick sample (`muscle_hub_screen_json`) with no backdrop clock to
ride. The same answer as above applies - a `World` on the minigames page, or
the page folded into the play page.

The first visit (the emitter's other arm - the brick wall and its shade -
under the intro strip, title zoom and course card) is not in that position:
its clock is `muscle_ringside::FirstVisitHub`, which needs no `World`, so the
standalone page replays it by tick through `muscle_first_visit_json` and
draws the frame the play hosts draw, from the same
`ringside_backdrop::first_visit_hub_draw` kernel. What it does not do is
sound the two announcer lines the walk starts (`FUN_8003D53C(0x1E, 0xB,
0xA9)` at arm `0`, `FUN_8003D53C(0x1F, round, 0x54)` at arm `0x15`): the
native window and the play page hand `HubFrame::xa` to their CD-XA clip path,
and the standalone page has no XA path at all. The walk still waits the
lines' modelled span (`FirstVisitHub`'s `_DAT_8007BC20`), so the timing
matches the play hosts and only the voice is missing. Blocking capability:
an XA clip decoder on that page (the play page's lives in `play_xa.rs`,
behind its `World` runtime).

The dome's command ring has the same shape one level down. Its chip marks
(the red cross-out X is `FUN_801DBC30`, placed by the engine as
`battle_party_panel::cross_out_mark`) are drawn by the standalone page only,
because only that page draws the ring as chips: the native window and the play
page present the dome's selection as text rows. Blocking capability: a chip
ring on the play hosts' dome HUD.

The play page also had a second gap under the first: its hub screens drew only
inside the arena's own frame, which ends when the leg does, so the INTERVAL +
tally screen - a between-legs screen - never reached the page at all. The page
now draws the hub list on the field frames that follow a leg too.

## The fishing point-exchange sub-screen: one session on two hosts, a query on the third

`World::minigames.fishing_exchange` is a live `Option<PrizeExchange>` with its
own cursor, and what *drives* it is one engine kernel,
`World::fishing_exchange_input` (`engine-core::fishing_exchange_input`):
toggle (banking the live session's points first), cursor moves floored at the
first visible row, venue switch, buy at the cursor. The native window maps its
keys onto that kernel's `ExchangeInput`; the browser play page maps its prize
panel's buttons onto the same inputs through `play_fishing_exchange_input`, and
`play_fishing_prize_buy` puts the cursor on a row and buys through it too.

The **composition** is shared: `legaia_engine_ui::ui_fishing_exchange` holds
the header, the row columns, the ink rule and the one-time tag, and both play
hosts draw the open sub-screen through it on their 320x240 stage - the native
window from its HUD pass, the page from `play_fishing_hud_json`.

It used to be a screen on one host only, for a reason no gate could see: the
page carried the same compose call, but its only opener,
`play_fishing_prize_buy`, opened the world sub-mode, committed the buy and
closed it again inside one call, so no HUD compose ever found it open. The page
now holds it open across frames and the DOM panel follows the engine's open /
venue state (`play_fishing_exchange_state_json`);
`crates/web-viewer/tests/play_screen_parity_disc.rs` asserts the rows join the
HUD draw list while it is open. The native window, for its part, drew the rows
in raw surface pixels at a pen that is a stage position - top-left and a third
of the page's size - until they went through the stage transform. The
standalone minigames page still lays its own list out from the JSON
(`fishing_exchange_json`); it has no `World` to hold the session on.

The tag is the part that has to stay separated from the ink.
`PrizeExchange::is_available` folds three independent refusals together -
price, the owned-stack cap and the one-time latch - so a host that reads it as
"already bought" prints `sold` beside every one-time prize a fresh save cannot
yet afford. `exchange_row_tag` reads `is_latched` on its own, and a test pins
the three cases.

The play page's cursor is the world's own: a panel Buy moves
`PrizeExchange::cursor` onto the row before buying, and the canvas draws that
cursor. What the page does not have is a *keyboard* path to Up / Down on the
open sub-screen - its arrow keys stay the pad, which the pond is still reading
- so the panel's buttons are the page's input surface for it.

The panel's pen also differs by one term: the native adds the venue overlay's
idle sway (`FUN_801d03b0`), an actor the browser hosts do not install, so the
page draws the panel at its resting top-left.

## A screen can be one host's frame and the other host's borrow

Three of the parity rows this page tracks were not a missing feature on either
host. They were the same screen composed in two places, and what kept them
apart was a borrow, a hand-off or a stale sentence.

### The frame belonged to a pass that could not size it

The shop / inn panel drew bare on the native window and framed on the browser
play page for three programs running. Nobody chose that. The window builds its
HUD text in one `&self` pass and its chrome sprites in another, and the panel
was assembled inline inside the text pass - so the sprite pass had no row count
to size a frame from, and adding one meant either duplicating the panel build
or moving it.

The fix is the move: the shop-family overlay is one frame
(`legaia_engine_screens::shop_overlay_frame`) the native redraw builds once
and hands to both passes, and the rect comes from `shop_panel_rows` (distinct
baselines in the text) plus `shop_panel_frame_rect` (pen, inset, width, row
pitch), counted over the fallback panel's own rows only - not over the floor
window or the code lock riding the same stage group. Sizing off the text rather than off
the session is what lets a screen that grows a row grow its frame on both
hosts, without either re-deriving a row count from a session shape the other
does not hold.

The general form: when a host splits its frame into passes with different
borrows, a feature can be absent from one pass for a reason that has nothing to
do with the feature. Ask which pass owns the geometry before reading the
absence as a decision.

### The hand-off released the thing the next screen needed

Retail composes the boot save-select over the title art at a dim. The native
window keeps it because its boot state machine holds the title session
alongside the save-select state. The browser play page reached the same screen
through the pause menu's own Load row and released the title session at that
hand-off, so the panel was composed over black.

The session is parked instead of dropped, and both hosts draw the bands through
one `title_band_sprites` kernel with a `TitleBandState`. Two gates on the
parked session are the native window's own: it is live only while the
save-select is the open sub-screen, and it is suppressed for `NowChecking` /
`SlotPreview`, the two phases retail pivots to black for.

### A declaration is a claim, and claims go stale

Two `[[frame_content]]` declarations said something no longer true of the code
they described.

One said the play page "has no Records page at all". It ships one, behind the
same Square toggle, through the same `records_screen_draws_for` builder, with
its model pinned to the native one by a `SIM_PAIRS` row. The real difference is
where each host builds the list - the window caches its dev-menu draws at tick
time, the page builds them in the draw pass where it holds the surface size -
which is the `tick_field_party_hud` shape, not a missing screen.

The other said the native window "does that same work" for all thirty-two
web-only engine calls in the battle-presentation pair. Thirty-one have a native
call site, most of them in `window/battle.rs`. The thirty-second is
`packet_color::hybrid`, and it is not presentation work the window skips: it is
the CPU-side per-vertex colour stream the page owes WebGL, which the wgpu
renderer resolves in its fragment shader instead.

Neither correction changes a byte of engine behaviour, and that is the point.
The gate ratchets the two *lists* and fails when they move; it cannot check the
prose, so a reason that was true when written outlives the thing it described.
Re-derive a declaration's reason whenever you touch the pair it names.

### A refusal the player cannot see is not a refusal

A save commit the host cannot honour - writing into a mounted card image the
native window has no writer for, or a browser card write that fails - used to
answer with a log line. The screen closed and nothing had happened, which from
the player's seat is indistinguishable from a save that worked.
`SaveScreenFlow::refuse` raises a shared notice instead, drawn by one pair of
engine-ui builders over the menu root. It is the port's own screen: retail's
save UI only ever talks to a card and reports a card it cannot use through the
card driver's result word, which does not model a second backend's failures.

## Frame-paired against retail: spoils banner, overworld, scene VRAM

Retail frames come from library states (`mednafen-state vram-dump
--display-crop`; PCSX-Redux states through
`scripts/pcsx-redux/extract_vram_from_sstate.py`), native frames from
`play-window --screenshot-every` cropped to the 2x stage and sampled back to
320x240, page frames from headless Chromium over a locally built bundle.

| Screen | Retail reference | What pairs | What differs | Host |
|---|---|---|---|---|
| Battle spoils banner | `noa_levelup_banner` | window rect `(9..309, 153..207)` identical on native; the XP / gold columns right-aligned the same way | text ink: retail's default string ink is CLUT-7 `(206, 206, 206)`, the native frame draws `(255, 255, 255)` - `battle_spoils_draws_for` passes pure white; interior is flat where retail is a vertical gradient (the documented menu-window approximation) | both (shared `engine-ui` builder) |
| Battle letterbox | none (retail has no letterbox) | - | the native window clears the area outside the 2x stage to the battle stage clear colour, a light blue, where the field clears it black | native |
| Overworld walk | `keikoku_chest_preload`, `sebucus_overworld_resident`, `karisto_overworld_resident` | the party panel's position and rows | both hosts frame the walk from a much higher, farther camera than retail's behind-the-leader view; the native window also draws horizontal white sheets across the terrain that neither the page nor any of the three retail frames shows (present with `--no-entry-pulse` too) | camera: both; sheets: native |
| Field scene VRAM (fog page) | nine PCSX-Redux field / world-map states | the effect pool's fog cells and CLUT row hash-identical on both builds; `dolk`'s own texels survive on the `(448, 0)` page | nothing since the host's field entry layers the pool under its build (it wrote it over, which is the VRAM the page draws from) | both |

The dialogue box, shop, inn and FMV are still not frame-paired: a talk,
shop or inn needs a positioned walk-to-NPC input on both hosts, which no
harness provides, and the retail references (`v0_1_tetsu_dialogue_accept`
for the dialogue box: border rows 9 and 63, columns 31..287, the same
`(206, 206, 206)` ink) have no port frame to pair with.

## The field SFX ring: one producer queue, one replay

The field scripts' cue producers (field-VM op `0x36` sub `0` / `4`, the motion
VM's op `0x09`) run inside `World::tick`, but the ring they write lives with
the SPU, on the host side of the `engine-core` / `engine-audio` boundary. The
world therefore queues each call as a `SfxRingOp`, and one routine replays the
queue for both play hosts: `AudioBgmDirector::route_world_sfx`, called from the
native `BootSession::route_field_sfx` and the browser page's `route_field_sfx`.
The side-band bank resolver (`World::side_band_bank`) and the runtime-row
lookup (`runtime_sfx_descriptor_in`) are engine functions under it. The
minigames page has no field and no queue to drain.

Two things a one-host reading of this would get wrong. The ring ages by the
vsyncs one host tick spans (`display_frame_step`, one), not by the game-tick
cadence `frame_step`: a host that fed it `frame_step` would play every delayed
cue at twice retail's rate at the field cadence of 2. And a ring id is never
routed through `classify_cue` - the scheduler returns ring cues in their own
list - because every runtime-bank id (`>= 0x200`) would otherwise land on the
CD-XA voice leg and be declined.

## The slot-2 / slot-6 SFX region: one residency, one restager

Which bank the SPU region VAB slots `2` and `6` share holds is decided once, in
the engine: `World::sync_sfx_residency` models retail's field-bank latch
`0x8007BAFC` and the region's occupant off the world's mode edges (field and
world map load PROT 0876 into slot 6, battle and the Baka duel PROT 0869 into
slot 2, fishing / slot machine / dance their own banks), and field-VM op `0x36`
sub `3` runs `World::release_field_audio`. The restage is the director's
`AudioBgmDirector::sync_shared_region`, run every tick from `route_world_sfx`
on both play hosts, placing the bank above the slot-0 system bank inside the
`SFX_BANK_SPU_BYTES` window. A routed cue resolves to its own slot or to
silence (`bgm::resolve_sfx_slot`); with one resolver, a class-2 fallback
cannot make a field cue audible on one host and silent on the other.

The minigames page keeps its own lazy per-slot staging
(`LegaiaMinigames::stage_sfx_slot`, `prot_index_for_slot`), so its slot 2 is
PROT 0869 for every game rather than the fishing / slot-machine / dance bank the
residency names. The only catalog cues it fires are the Baka duel's
(`BakaFight::take_cues`, drained in `baka_tick`) and the Muscle Dome tally's
voice attrs, and both games hold PROT 0869 in slot 2 on every host, so no cue it
plays differs; a category-`2` cue added to fishing, the slot machine or the
dance on that page would.

## Screen prims under the HUD: one sort, two layerings

The browser play page draws every screen-space primitive through one
ordering-table pass over the finished 3D frame, and its party HUD is a layer
above the whole canvas; the native window composited the same primitives as a
tail **after** its sprite and text overlays, so its HUD sat under them. The
two agreed on every frame nothing darkened - until the field attached light
(op `0x34` sub-1) projected at retail's scale and its subtractive mask started
darkening the native HUD while the page's stayed bright. Retail's frame (the
`dolk` capture behind `attached_light_retail_capture_disc.rs`) keeps the HUD
bright.

The native `RenderTarget::SceneWithScreenPrims` carries a second list,
`under_overlay`, drawn after the scene's meshes and before its 2D overlays and
sorted on its own by the shared builder. The field scene's own effects go
there as one list - the fog sheets, the move strips and the light pools - so
they order against each other exactly as the page's single pass orders them.
Transitions, fades and battle readouts stay in the tail.

## A side-by-side pass over one set of both-host features

The drift tiers are green over every feature below, and each one was then shot
on both hosts at the same scene and moment: `play-window --screenshot-every N
--screenshot-dir` cropped to the integer stage, against headless Chromium on
the play page, with retail references from `captures/` where one exists. Two
recipe rules came out of it, on top of the ones [above](#a-second-pass-frames-matched-by-engine-frame-with-retail-as-the-third).

- **Prove the page is this tree's bundle before reading a frame.** On a shared
  runner, `python3 -m http.server <port>` started detached on a port an older
  worktree's server still holds exits without a word, and the driver's
  requests land on that other tree. A whole pass of page frames came out of an
  earlier build that way - no attached light in `dolk` or `cave01`, a retired
  fishing session's prompt text - and read as page drift. The guard is the
  bundle stamp: the served `wasm/SOURCE_STAMP.json` must equal the tree's own
  (`check-wasm-freshness.py` writes it) before the first frame counts.
- **In a scripted span, pair by state, not by tick.** The page's
  `__playState.frame` and the native `--screenshot-every` tick agree on a
  settled field, but not through `vell`'s entry walk, where the page ran about
  fifty ticks ahead of the native frame of the same number; a battle's command
  phase is paired by which prompt is up.

| Feature | Native | Page | Verdict |
|---|---|---|---|
| `vell` fog underlay | sheets drawn, textured | same | same on settled frames; an additive glow by the player at the frame's edge reads whiter natively (not chased) |
| `dolk` / `cave01` attached light, HUD above screen prims | darkness mask, rim on the player, HUD bright | same | same; the page's subtractive run is now pinned by `attached_light_page_prims.rs` |
| prologue sepia (`opdeene`) | sepia tableau | same tint | same billboards since the lit-row mask; the page stages no lit-row ambient - see [the prologue meshes](#a-prologue-mesh-set-drawn-black-natively) |
| narration crawl | 1x glyphs, rows at the window's `h / 240` | 1x glyphs, rows at the canvas's `h / 240` | both wrong against retail, differently; fixed on both, and a native frame now lays its rows over the retail frame's line for line |
| "It was the Seru." caption | scaled by `h / 240` in window pixels | the overlay canvas is the stage | native larger than the stage and off centre; fixed |
| `0x6E` Begin / Reselect, commit log, target plaque | labels, log rows, plaque at `x = 0xE8 - w / 2` | same | same |
| Koru strip | not shot (no formation-0xB6 entry on either host short of the dome) | - | not paired |
| scripted battle (op `0x3E`), talk entry at the interaction cursor | not shot (no positioned walk-to-NPC input on either host) | - | not paired |
| fishing (`PondSession`) | status rows, digits, venue camera | status rows, digits, gauge fills, field camera | gauge fills were page-only; fixed. The venue pass is disclosed native-only ([fishing](#one-minigame-one-session-type)) |
| slot / Baka face buttons | prompts named the old buttons | same prompts | both hosts' prompt text was stale against the kernel; fixed |
| title menu | two rows | two rows | same (the lit row follows each host's card state) |
| battle letterbox, spoils ink | black letterbox, spoils text all `(206, 206, 206)` | canvas is the stage; the banner fell between two shots | native matches retail; page not shot |

### Three shapes no tier fails on

**Two hosts agreeing on a surface-pixel law for a stage element.** The crawl
and the caption were laid out by scaling a 240-line Y into each host's surface
and drawing the glyphs at 1x. A pair check could never flag it - the page's
canvas is 720 lines, the native window 699, and both put the rows about three
surface pixels apart per stage line with glyphs a third of retail's size.
Retail prints stage-sized glyphs on a 16-line pitch
(`captures/crawl1_capture`). The crawl and the title card are now one engine-ui
builder in stage pixels (`cutscene_text_stage_draws`), scaled through each
host's stage transform.

**A hint string is a paired constant with nothing pairing it.** The slot
machine's `Stopping` prompt said Cross stops a reel and the Baka duel's said
the D-pad attacks, on both hosts, after the kernel moved to Square / Cross /
Circle per reel and Square / Circle / Cross per attack type with Triangle as
the special. The play page's own button title named Z as the fishing cast key
where the page binds cast to X. The kernel's tests were green throughout; the
text is host-side and duplicated. Fishing is the model the others lack: its
rows are one engine text (`PondSession::status_rows`) taking each host's key
names.

**A draw one host carries on a side channel.** The fishing gauges resolve to
frames on both hosts, but only the page filled them - from a `bars` payload it
emits beside the shared text list - while the native window passed the shared
consumer no solid texel and drew empty gauges. The native window now hands
`fishing_hud_draws_for` the font's solid texel, so both hosts fill the same
frames.

### A prologue mesh set drawn black natively

In the `opdeene` jungle the native window drew a set of plants - the twisted
branches and the dark bushes - as black silhouettes, where the page and the
retail frame (`captures/crawl1_capture`) draw them pale and textured. Both
hosts share the sepia word law (`prologue_sepia_word`); the difference sat
upstream of it, in a native-only restage. Fixed.

The silhouettes are the scene pack's **one-quad billboards** (pack slots
`4 8 9 10 11 13 16 28`, each a single `FT4` on descriptor row 4): dropping
those slots from the native draw list removes every silhouette, and dropping
the multi-prim plants whose words the grade takes near black does not. Their
disc word is exactly `0x80`, retail rewrites it to `(98, 94, 42)` (the resident
copies in `run_w3a_captures.sh opdeene`'s checkpoints, and `FT4` packets at
that colour in the same frames' ordering tables), and their texture pages and
CLUT rows match retail texel for texel under the palette law. The native
builder then gave the prologue's dim lit-row ambient (`0x20`,
`DAT_8007B788`) to every vertex whose colour read `0x80` - the marker the mesh
builder uses for the light-source rows, which carry no word - so an authored
`0x80` was restaged along with them, and the grade's packet half took `0x20`
to `V = 2`. The restage now keys on a per-vertex lit-row mask
(`legaia_tmd::mesh::tmd_to_vram_mesh_filtered_lit`, applied by
`engine-core::fade::apply_prologue_lit_ambient`). The page stages no lit-row
ambient at all, so its light-source rows still draw at neutral where the
native window draws them at the dim ambient.

## Gaps absent from both hosts: overworld curvature, ground shadow

Retail draws missing from **both** play hosts fail no tier. The ones below now
draw on both hosts through one kernel each; what is left of them sits here in
the form a waiver takes.

**The overworld curvature table on the continent.** `FUN_800271A8` builds a
depth-indexed screen-Y table every overworld consumer adds to `SY`
([`renderer.md`](../subsystems/renderer.md#frame-setup--present)). The fog
sheets and the drop shadow add it on the CPU; the continent bends per vertex
in the native mesh shaders (`OVERWORLD_CURVE_WGSL`, staged through
`Renderer::set_overworld_curvature`) and in the page's GLSL twin
(`overworldCurve`, staged through `play_render_curve_scale` ->
`setOverworldCurve`). Both hosts take the per-frame scale from the one kernel
`overworld_curvature::frame_curve_scale`, and both shaders evaluate the table
in closed form, pinned against it by `curvature_closed_form`. The port bends
every overworld scene draw where retail's four SCUS lit rows (`8..11`) do not
bend, and that changes no pixel: no overworld TMD on the disc carries a lit-row
group ([`renderer.md`](../subsystems/renderer.md#frame-setup--present)). The `/world-overview/` viewer's
ocean plane shares the page's renderer but not its program, and stays flat
with the rest of that viewer (it never stages a scale).

**The object-effect clip on a raised `+0x42`.** Field-VM `4C C2 1` sends a
placed actor through `FUN_8001C204` and `FUN_8002735C`, which clip its
polygons to the slab its object-effect row stages
([`renderer.md`](../subsystems/renderer.md#what-a-raised-0x42-draws)). Both
hosts ask one kernel per placed / NPC draw (`World::object_effect_mesh_clip`)
and discard outside the slab: the native mesh shaders through
`EFFECT_CLIP_WGSL`, the page's field program through `u_eclip_m` /
`u_eclip_b` (`play_effect_clip`, which negates the page model's row 1 back to
retail's frame). The player's draws ask the same kernel under
`ActorTintKey::Player` - native at each player mesh push, the page through
`play_effect_clip` kind `2`.

**The field drop shadow.** `FUN_8001C394` (called from the animated-actor
renderer `FUN_8001B964`) is ported as `engine-core::drop_shadow` and walked by
`World::field_drop_shadows`. The native window's `field_drop_shadow_prims` and
the play page's `field_drop_shadow_prims` both wrap its cells through
`screen_prim::drop_shadow_prim` into their field screen-prim pass with
per-corner scene depth, which keeps the blob under the actor and over the
ground. `crates/engine-core/tests/drop_shadow_retail_capture_disc.rs` matches
the kernel's packets to retail's exactly on three captured frames. The ignore
list's `render_pipeline` scope row, which read the routine as replaced by the
rasteriser with no drawing mechanism behind it, is gone, and so is the
`libgte` row that read `FUN_800460AC`'s `RTPT` (`cop2 0x280030`) as `NCDS`.

**The battle ground shadow and the default draw-kind-4 arm.** Both were
missing from both hosts for one reason: their geometry is `FUN_80028158`'s,
which neither host could build. It is ported as `engine-core::effect_default_arm`
([`effect-vm.md`](../subsystems/effect-vm.md#the-default-arms-draw)), and both
hosts reach it through lists they already drew or now draw side by side:
`World::active_effect_kind4_draws` carries every live default-arm node next to
the ribbons and sprite-arm quads, and `World::battle_ground_shadows` - one disc
per battle body, judged by the draw plan the host's own actor pass uses - is
drawn right after it by the native part pass and folded into the play page's
battle FX frame. The minigames page draws no battle and has neither.

**The `opdeene` plant silhouettes** ([above](#a-prologue-mesh-set-drawn-black-natively))
were not a gap of this kind: the page drew them right, and the native defect
is fixed. The earlier reading here - that the grade's packet half takes 1565
of the scene's baked-row prims to `V = 0` and retail's rewrite might not reach
them - is falsified by a mid-crawl capture: the resident colour words are the
same at the crawl's black start and mid-crawl with the jungle lit, every one of
them on the sepia curve, and retail holds its own exactly-black words
(`2712` of `18425`) and draws those meshes dark where they are in view.

## A side-by-side pass over boots, hand-offs and door entries

A pass over the two play hosts' boot, their post-movie hand-off and the
minigame doors found five gaps with every tier green. Four of them share one
shape: **the host-side step lived next to one entry, and the other entry
skipped it.** The engine kernel was shared each time; the call around it was
not.

### A boot install only one host ran

The native boot installed six static-SCUS progression tables one read at a
time - the XP curve and the Noa / Gala threshold divisors, the stat-growth
curves, the victory-pose table, the XA cue durations, the magic-XP thresholds
and the accessory passives. The page's `load_disc` installed none of them.
Every consumer has a disc-free fallback, so nothing failed: the page levelled
on the flat placeholder growth, never levelled a summon, granted no accessory
passive (no Ivory Book on the capture roll), played no melee grunt or cast
voice (a zero cue duration), and skipped the victory pose's `rand()` - so its
RNG stream left the native one's after every battle. Both boots now call one
engine entry, `World::install_retail_progression_tables`, and
`crates/web-viewer/tests/play_boot_tables_parity.rs` drives the page's own
`load_disc` against it.

This one is gated: **tier 13** takes every `world.install_*` / `world.set_*`
call in `crates/engine-session/src/boot.rs` and requires a shipped
`crates/web-viewer` source to call the same method, or a `[[boot_install]]`
waiver in `ui-host-drift-waivers.toml`. Run over the old boot it fails on
`install_magic_xp_thresholds` and `set_accessory_passives`. It cannot see the
other four, which the old boot wrote as field assignments - which is the
argument for the single entry: a table installed through a method both boots
must call is one the tier can pair.

### The hand-off that is a scene entry too

Retail's post-FMV dispatch enters a new scene (`town01` -> movie 1 ->
`town0b`) without the field VM's transition op, so no `SceneEntered` event
follows it. The page rebuilt its render state on the hand-off anyway; the
native window only logged the outcome - it kept drawing the trigger scene's
meshes, kept its camera shot, and left the trigger scene's VAB bank staged.
The page, for its part, keyed its camera reset on `SceneEntered` alone and
carried the trigger scene's shot too. Both now treat the hand-off as the entry
it is: `BootSession::apply_pending_fmv_handoff` runs the same session-side swap
a door runs (camera globals reset, SFX queue dropped) and the
window rebuilds on `FmvHandoffOutcome::Entered`; the page resets the camera on
the hand-off tick
(`crates/web-viewer/tests/play_fmv_real.rs`,
`the_fmv_handoff_resets_the_camera_like_a_door`). The page also reopens the
sequencer's pause gate when a movie ends, as the window's drain always did; it
had only stopped the XA, so a movie that handed back to a scene with no BGM
start of its own left the score paused. That half is wasm-only (the headless
runtime has no audio output) and rests on reading the two drains.

### A held word into an edge-driven runtime

`MenuRuntime::tick` filters no repeats - every screen it drives moves a cursor
or commits on the input it is handed. The page hands it one edge per press;
the native window handed it the **held** pad word, so one key press lasting a
few ticks stepped the shop cursor several rows or walked through the buy
confirm. Both hosts now decode through
`menu_runtime::menu_input_from_pad_edges`, and the window passes the edge it
already computed for every other modal screen. Nothing here was a missing
call - both hosts called the same runtime with a well-formed `MenuInput` - so
the shape to look for is a shared kernel whose **input contract** (edge or
level) one host does not honour.

### A launcher decode the door never ran

Two minigame entries did host-side work in each host's debug launcher that the
shared door entry (`SceneHost::drain_minigame_warp`) never did - the phase
shape [tier 10](#tier-10---entry-symmetry-is-the-phase-armed-only-from-a-debug-key)
exists for, except that here the missing step was data, not a `World` phase,
so the tier had nothing to pair:

- **The fishing point exchange.** Both hosts' launchers decoded the two venue
  pages beside the session tables; the door decoded none, so the exchange was
  unusable after walking into the venue on either host.
  `SceneHost::enter_fishing_from_overlay` now decodes them into
  `World::minigames.fishing_prize_venues`
  (`play_fishing_host.rs`, `a_door_entered_fishing_session_has_the_prize_exchange`).
- **The Muscle Dome fighter.** The native `M` launcher read the lead's swing
  costs, live HP, AGL pool and INT / UDF / LDF; the door fielded flat `0x1E`
  costs, a 120 AP pool and a 60 / 40 / 40 stand-in profile - on both hosts,
  since the door is the shared path. `SceneHost::dome_lead_fighter` is the one
  builder now (`play_minigames_host.rs`,
  `arena_door_warp_draws_the_muscle_dome_and_start_leaves`).

### Open drift the same pass found

Every row this pass recorded as open has since closed (below, and in
[the third pass](#options-battle-chrome-and-minigame-huds-a-third-side-by-side-pass)).

The audio rows of the same pass are closed or settled in
[their own section](#audio-legs-one-kernel-per-decision).

### Closed from the same list

- **Monster action-tag clips.** Each host used to install a monster's
  archive-order clip table from its own render build, so a monster with no
  mesh - or a native frame's first battle ticks - read no tag table.
  `SceneHost::install_battle_monster_action_clips` installs them from the
  engine tick the battle is up, for either host. The shop tick and the
  sub-tick taps from the same list are closed in
  [the menus section](#menus-saves-and-minigame-exits-one-call-per-decision).
- **NPC clip advance off the field.** The window advanced its NPC clip
  players only in `SceneMode::Field`, the page in every mode, so an NPC came
  back from a fight on a different clip frame per host. When the players run
  is the world's decision now, `World::field_npc_clips_advance`, which both
  hosts ask. So is the playhead itself: `World::tick_npc_clips` steps a
  world-owned cursor under each actor's `+0x62` word and both hosts pose from
  it (`World::sync_npc_clip`), where each used to free-loop its own player -
  which looped every treasure chest's lid. Only where they pose (the page's
  per-tick step, the window's draw pass keyed on its pose cache) stays per
  host.
- **The field frame tail.** Three one-host tails moved onto engine kernels in
  `engine-core`'s `world/field_frame_tail.rs`, each called by both hosts:
  `World::tick_effect_scene_graphs` (summon / move-FX / field-FX),
  `World::step_field_vram_effects` (op-`0x43` stamps and rect copies, the
  ambient move-VM tree, CLUT-cell one-shots and blend fades) and
  `World::drain_field_anim_cues`. With them: the native narration crawl /
  title-card arm no longer returns after the scene tick, so the whole tail
  runs under the crawl as it does on the page (only the pad freeze and the
  Start gate remain); the page freezes its tail while a movie holds the frame,
  as the window's zero-tick loop does; and the window drains ANIMATE cues every
  tick in every mode instead of in its field draw pass. The native CLUT step
  also carried a defect of its own: it skipped every call while four effect
  lists were empty, a test that left out the `4C DB` blend fades (a lone
  blend fade never stepped natively) and that left each list's tick backlog
  banked for the next effect to consume at once. The headless session is a
  third driver of the same tail: `BootSession::tick` runs its world side
  through `World::step_world_frame_tail` whenever the caller does not drain
  the queues itself, so replays and ladders execute what the hosts do
  ([`reach-triage.md`](reach-triage.md#what-a-pad-only-ladder-structurally-cannot-execute)).
- **The page's screen-prim camera centre.** Fog, drop shadows, move strips and
  light pools resolved the follow camera with a pinned `[0, 0]` fallback focus
  on the page and the scene AABB centre natively; the page passes the AABB
  centre now.
- **Battle camera without a render.** The page held the phase-scripted
  camera's state on its battle render build, so a fight whose build failed
  ran no camera at all, and both hosts carried their own copy of the input
  derivation. The derivation is `engine-core::battle_cam_inputs` now and the
  state is `BattleState::camera`, stepped from `World::tick`; both hosts only
  read `World::battle_cam_pose`
  ([`battle.md`](../subsystems/battle.md#the-resting-yaw-is-the-orbit-and-battle-init-zeroes-it)
  carries the camera's port note beside the phase script).
- **Battle-intro names and the commit-log launch**, absent from both hosts.
  The flow-`0x0A` enemy-name banner and the commit log's slide off the ring
  are engine state (`BattleState::intro_names_frames`,
  `BattleState::commit_log_launch`) read by one builder each
  (`battle_hud::battle_intro_names`, `battle_hud::battle_commit_log`), and
  both hosts pass them through the shared battle HUD frame.
- **The battle message banner.** Screen elements `0x59` (Seru absorbed) and
  `0x65` (magic level increased) share one string, the context buffer
  `ctx + 0x1F9`; neither host drew either, and the two drains threw away the
  `UiElement` event that would have told them. The line now lives on
  `World::battle.message_banner` from raise to unload, and both hosts read it
  through one engine function, `battle_hud::battle_banner_message`, which also
  replaced the two hand-copied level-up / capture readers
  ([`battle-action.md`](../subsystems/battle-action.md#the-battle-message-banner-elements-0x59-and-0x65)).
  The **art-learned** line is not in it on purpose: retail announces a new
  art with the `NEW ARTS!!` sprite banner, which both hosts already draw.
- **A HUD raise that spawned an effect.** `BattleActionHost::ui_element` routed
  every raise into the effect pool. The id is a placement-record index for
  the HUD spawner `FUN_801D8DE8`, which calls no effect routine, so each raise
  played an unrelated `efect.dat` script on both hosts. Effect scripts now
  reach the pool only through `World::route_battle_effect_spawns`, the one
  routing both hosts call (each had carried its own copy of that loop).
- **Minigame purses in a save.** `World::save_full` / `load_full` dropped the
  casino coins, the Point Card bank and the fishing record, so a save / load
  zeroed them on either host. Retail keeps all nine words in the save block's
  live-state window; they now ride there and in the `LGSF` `LGX7` block
  ([`save-screen.md`](../subsystems/save-screen.md#the-minigame-purses-are-live-state-words-too)).
- **Card load order.** The page loaded a card save and then entered its scene,
  whose picker story baseline cleared system flags `0x141` / `0x147` in every
  resumed save. The runtime now parks the loaded save, skips the baseline for
  that entry and lands it through the shared card-load kernel
  (`resume::resume_card_load`: story flags, entry, whole save) the native
  `BootSession::resume_save` calls
  (`cards.rs`, `a_card_load_keeps_the_saves_story_flags_across_the_scene_entry`).
- **The op-`0x35` timed release.** Its expiry set a flag no host read. The
  expiry arm is `FUN_800266E0`'s body on the field-BGM slot - sub-op `2`'s
  pause - so `World::tick` now emits that pause and both hosts' BGM routing
  acts on it ([`audio.md`](../subsystems/audio.md#the-timed-release-is-a-scheduled-bgm-pause)).

## Audio legs: one kernel per decision

A side-by-side read of the two play hosts' audio paths found each of the rows
below. Every fix moves the decision into an `engine-core` / `engine-audio`
kernel both hosts call, so the two cannot drift on it again.

- **Field CD-XA voice lines.** Field-VM op `0x36`'s XA arm and the
  scripted-scene programs' voice state both start `FUN_8003D53C` one-shots, and
  neither reached a host: the op pushed an event nobody read, and
  `World::tick_scene_programs` counted its legs into a report nobody consumed.
  Both now land on `World::push_field_xa_cue`, drained by both hosts'
  `route_field_sfx` into their `play_xa_clip`; the drive's busy span feeds the
  program's `xa_busy` wait. The programs' other XA leg (`FUN_80019794`) is a
  seek-ahead that plays nothing ([`audio.md`](../subsystems/audio.md#streamed-cue-census-fun_8003eae4--fun_80019794)).
- **Cues raised off the field VM.** The world map's map-display cue (`0x20`)
  and the developer menu's cursor / confirm cues are `FUN_80035B50` calls in
  retail. Each was queued on its controller and dropped by both hosts; both now
  ride the SFX ring (`World::tick_world_map`, `DevMenuSession::route_sfx`).
- **A settled duck.** Both hosts re-applied the battle duck only while the
  level moved, so a track started under a held duck played at full volume.
  `legaia_engine_audio::duck::duck_apply` re-applies a level resting below the
  reference every frame on both.
- **A carried global track.** A door under a `music_01` track restaged the
  scene VAB over the region that track's own samples occupy, while both
  directors kept the track playing as a duplicate start. Neither host stages a
  scene bank any more ([below](#the-spu-map-one-layout-kernel-and-no-scene-bank)),
  so a door leaves the region to its track.
- **The reward bank.** The page placed PROT 0889 above the BGM but not above
  a staged side-band bank, and dropped it on every scene change where the
  native director kept it. It now takes the highest tail end, and both keep it
  until a BGM restage re-owns the region.
- **Owned-bank order.** Both directors uploaded a global track's VAB before
  parsing its score; both now validate both halves first.
- **Direct scene entry.** The native `enter_field_live` restaged no VAB,
  dropped no queued cue and kept the dedupe latch; the page's `enter_field`
  did all three. `BootSession::restage_audio_for_direct_entry` makes the same
  three moves.
- **Movie audio.** Which movies silenced the BGM, and what their end gave
  back, was decided per host: the native window paused only under a movie
  with an XA track, the page at every install, and the page reopened the
  gate on every finish - so a track the script had paused came back after an
  unplayed movie on the page alone. Both now consult
  `legaia_engine_core::movie_audio::MovieScore`: the attract releases the
  score and restarts the title theme (retail's release and `CARD INIT`
  pair), a movie with audio ducks only a sounding score, and every end
  reopens only what that movie closed
  ([`audio.md`](../subsystems/audio.md#movies-and-the-score)).
- **One pause latch.** The native director kept a `paused` bool beside the
  output gate it drove, and the movie path wrote the gate around it, so after
  a movie the two disagreed and sub-op `0xA` detached a track the page kept.
  The native director now reads the gate (`AudioOut::sequencer_paused`), as
  the page always did.
- **Menu blips.** The native window blipped a cancel on any Start in the
  pause menu, a sub-screen included; the page only where Start closed the
  menu, and spelled the edge-to-cue rule in its own script. Both now ask
  `legaia_engine_core::menu_cues::menu_edge_blip` (the page through
  `play_menu_edge_blip`), and the cue ids live there.
- **A blip ages nothing.** The page fired a menu blip by enqueueing it and
  ticking the scheduler, which advanced every other queued cue a frame per
  blip. It now keys a one-cue batch (`SfxFireBatch::immediate`), leaving the
  queue's clock to the per-tick drain, as the native window does.
- **Title to Load.** The page handed the score over before entering the
  save's scene, off the pre-load world, and the entry then cleared the dedupe
  latch under it, so the scene's own start restarted the track. The call now
  arms the hand-off and the page's scene entry performs it after the save
  lands - the native enter, load, then stop and restore order.
- **Door-tick order.** Replaying the new scene's entry ring ops before the
  SFX queue is cleared fires its zero-delay entry cues into the old queue and
  drops its delayed ones, and routing the BGM last stages a bank against the
  outgoing track. Both hosts now run `BootSession::tick`, which routes the BGM
  after the scene tick, clears on the swap, and only then replays the ring.
- **Late audio.** The page's output exists only after a user gesture, and
  every start routed before it was dropped; the scene stayed silent until its
  script started music again. `audio_init` now starts the world's current
  track.

Two rows stay open on purpose:

- **A late output restarts a paused track.** The engine keeps no copy of the
  script's pause bit, so a page whose audio comes up after the scene's script
  paused its track starts that track anyway. The native window has its
  output from the first frame and never meets the case.

- **Op `0x35` sub-op `8`.** `FUN_80019898` replays the sequence bound to the
  record at `0x8007057C` through `FUN_80026478`. In every captured state that
  record either names a sequence whose channel holds no stream or is inactive,
  so the retail effect is silence, and the trait's default no-op is the
  faithful director behaviour ([`audio.md`](../subsystems/audio.md#sub-op-8-replays-an-empty-record)).

Closed since:

- **Cast-voice latency on the page.** A cast voice whose `(slot, channel)` the
  clip bank lacked was sliced out of the disc by the page's script and
  installed a frame or more after the cast asked for it; the native window
  reads it synchronously. The engine now lists the round's committed spells'
  cast voices at the round's start (`World::drain_battle_xa_prestage`, every
  candidate of a coin-flip module included), and the page stages each one
  silently (`prestage_xa_clip`), so the cast's own request finds it resident.
  The native window drains the list and drops it. A monster's cast is picked
  at its own dispatch, so it has no lead time and keeps the page's lazy path.

## The SPU map: one layout kernel, and no scene bank

Both play hosts lay SPU RAM out through `legaia_engine_audio::spu_layout`: the
region constants they re-export (`SFX_BANK_SPU_BYTES`, `SPU_RESERVED_BYTES`),
`upload_owned_bank` for every track bank, and `upload_resident_sfx` /
`upload_shared_region` for the slot-0 system bank and the slot-2 / slot-6
region above it.

- **The credits bank.** The ending theme's bank (`0x631B0` bytes of bodies)
  did not fit either host's BGM region, so its last seven bodies were dropped
  and three of the score's programs sounded nothing on both. `upload_owned_bank`
  now lays a bank too large for the region across the SFX region, as retail
  opens VAB 10 at slot 0's base
  ([`audio.md`](../subsystems/audio.md#where-the-credits-bank-lands-in-spu-ram)).
  While it is resident the director drops its resident SFX banks and keys no cue
  against the credits bank (`AudioBgmDirector::reclaim_sfx_region`, at the
  upload); the next track that fits re-stages them. Both play hosts run that
  director.
- **No scene bank.** Both hosts staged the scene block's first VAB-bearing
  entry on every scene entry, and skipped it under a carried global track
  (`scene_bank_restage_wanted`). Retail stages no scene bank: a bank loads only
  with its track, and a scene-local id plays a global fallback track
  ([`audio.md`](../subsystems/audio.md#a-scene-local-id-loads-a-fallback-track-not-a-scene-bank)).
  Neither host stages one now, the gate is gone, and
  `SceneHost::route_bgm_events` routes a scene-local start through
  `start_owned_vab` on `SCENE_LOCAL_BGM_FALLBACK_ID`'s entry for both.

## Battle FX: the effect ribbon and the summon seat order

- **The effect ribbon.** Move-VM op `0x42` nodes (the lightning ribbon
  `FUN_801CFA48` builds) were drawn by neither battle host. Both now draw
  `World::active_effect_ribbons` - one engine list, rebuilt from each live
  node's state every frame (`legaia_engine_core::effect_ribbon`) - composed like
  a mesh part: the native window uploads each mesh in its part pass
  (`build_summon_and_move_fx_part_draws`), the page bakes it into its FX
  billboard stream (`build_battle_fx`). The list, the mesh and the transform
  are the engine's; only the upload differs. In play the carrier is Gilium's
  summon (PROT 0923).
- **Summon seat before the scene tick.** The native window seated a player
  cast's summon creature before ticking the effect scene graphs, the page
  after; the page now takes the native order (`tick_world_effects`), so a cast
  ticks its creature's first frame on the same tick on both hosts.
- **One builder for the battle labels.** The native window used to merge the
  SCUS labels at boot and the battle overlay's on top only when a player-driven
  battle was requested; the page read the overlay's first and merged the SCUS
  ones on top. Both now install `battle_open::battle_ui_strings_for_disc`
  (overlay half, then SCUS half) at disc load. Retail has no merge at all -
  each label is a string its drawer addresses in its own image - and the two
  label sets (`battle_ui_strings::SCUS_LABELS`, `OVERLAY_LABELS`) share no key
  (`the_two_label_halves_share_no_key`), so the order cannot change a label.

## Menus, saves and minigame exits: one call per decision

A second side-by-side read, over the menu, save and minigame-exit paths, found
each row below. As with the audio legs, the fix is a kernel both hosts call,
or - where the defect was one host deviating from a behaviour the other already
had - the deviating host adopting it.

- **Submode screens were cross-wired on both hosts.** `World::input` holds the
  host's **raw** PSX word (`PadButton`, Cross `0x4000`), while the op-`0x49`
  submode family tests the **packed** masks `FUN_8001822C` builds (Cross
  `0x0040`). Cross did nothing on a coin cabinet, Down accepted and Right backed
  out. The tests fed the packed constant straight into `set_pad`, so they
  asserted the defect. `World::submode_pad_words` now converts through
  `dev_menu::retail_packed`, the conversion the Incense notice and both dev
  menus already used.
- **Card Save.** The browser rack wrote cards through page-private code; the
  native `--card` port refused a Save. Both call
  `engine-core::card_write::write_save_into_card` now, and the native window
  writes the image file back (`MountedCard::persist`).
- **LGSF resume point.** The native slot files carried the resume trailer and
  the page's export did not; the page's two imports loaded first and entered
  after, so the picker baseline cleared flags `0x141` / `0x147` in the import -
  the card-Load defect on a second path. `export_save` writes the trailer and
  both imports park the save for the scene entry.
- **Leaving a Muscle Dome leg.** Only the native `M` hotkey reported the left
  leg (the run / give-up path); the `Start` escape both hosts share did not, so
  the page kept the contest open with its tally. `World::leave_muscle_dome`
  reports and settles, and both paths call it.
- **A shop freezes the field.** Retail runs a shop as a menu-overlay session:
  the field overlay is swapped out under it (a prize-shop save state holds game
  mode `0x17` with PROT 0899 resident). The page froze the world; the native
  window ticked it with a neutral pad, so NPCs walked behind the buy list.
  `MenuRuntime::suspends_field` names the rule (an inn prompt is a field
  dialogue and is not suspended) and the native loop skips its tick under it.
- **Sub-frame key taps.** A key pressed and released between two native
  redraws never reached a tick; the page latched it in its `pulse` set.
  `input::PadTapLatch` is that rule as a type, and the native window feeds the
  frame's first tick through it.
- **Party HUD pad.** The native HUD read the window's held keys, the page read
  the word the world was handed; the native HUD now reads `World::input` too.
- **Dance end.** `World::exit_dance` already queues the hall track's restart
  (`restore_minigame_bgm`); the native window then started it a second time.
- **Developer menu CAMERA row.** Absent on both; `DevMenuSession::tick_host`
  is the one call both hosts make, and it carries the row
  ([`world-map.md`](../subsystems/world-map.md#the-camera-row)).
- **The pause-menu stack.** The native window, the headless `BootSession` and
  the page each carried the whole root-list step, the sub-screen step (with
  the Status screen's Arts-editor extension) and the outcome match that folds a
  finished screen into the world; the headless copy had dropped the window-8
  notice. `field_menu_dispatch::tick_root_list`, `tick_open_subsession` and
  `finish_subsession` are those three steps, and all three drivers call them;
  what stays per host is the rack I/O, the key table and the options store
  (`SubsessionHandoff`).
- **Four small copies.** The shop's owns-the-pad test (`MenuRuntime::is_open`
  on both, where the page spelled out two of its terms), the naming prompt's
  pad decode (`NameEntryInput::from_pad_edge`), the dev menu's EQUIP commit
  (inside `DevMenuSession::tick_host`), the demo tile board's install
  (`World::install_demo_tile_board`) and the options' simulation knobs
  (`OptionsState::apply_to_world`) are one call each now.
- **Minigame hotkeys.** The native `B` / `M` launchers built their sessions
  directly, started the track through the director rather than the world's
  minigame swap, and installed Vahn's arts whoever led; they now arm the door
  warp, and so does `O` now ([above](#minigame-launchers-and-the-audio-tail)).

Two rows of the same read were already closed when re-checked: the minigame
purses in `save_full` and the card-Load order (both in the section above).
The scene-entry place-name banner is closed on both hosts at once: it is not
the slot-`0x2E` fill-fade actor this list used to name (that actor's panel
script `0x801F32B4` only closes every panel), but a text balloon the MAN
loader spawns, seated by `engine-core::place_name_banner` from
`SceneHost::load_scene` and drawn by the balloon builders both hosts already
share. Still open, and on neither host: the world-map location labels
([`place-names.md`](../formats/place-names.md)).

## Options, battle chrome and minigame HUDs: a third side-by-side pass

A read of both hosts over the options knobs, the battle chrome and the
minigame HUDs. Each closed row below is one engine call both hosts make now,
or the deviating host adopting the other's behaviour.

- **The camera-distance preset.** The native `T` stepped the preset and
  persisted it; the page's export stepped only its camera, had no caller, and
  the next options apply put the stored value back. Both step the option now
  (`OptionsState::cycle_camera_distance`), put the returned preset on their
  camera and persist, and the page carries a control for it.
- **VR saved precise movement off.** The page's VR first-person drive turned
  precise movement on and off through the persisting setter, so leaving VR
  stored "off" over the player's own choice. It lays a session-only override
  over the option now (`set_precise_movement_override`), which every options
  apply re-asserts and nothing writes to the store.
- **The occlusion gate's body centre.** The page rebuilt
  `field_occlusion::player_body_centre` inline, half-height literal and all;
  it calls the kernel now. The equal result today was a coincidence of two
  constants, which is the shape tier 3 exists for, so both this and the
  camera-distance cycle are `SIM_PAIRS` rows.
- **The HUD over a party wipe.** Retail's frame after the wipe store is the
  title overlay fading in, and the native window's boot-UI arm owns the whole
  HUD for the hold. The page silenced only its post-battle list, so the party
  strip, the plaque and the command chips stayed painted over the frozen
  frame; its whole overlay list is empty for the hold now.
- **The colour grade a frame late.** The native window stages the screen
  tint, the prologue grade and the depth-cue ramp after its tick loop. The
  page read `play_cutscene_state_json` once, before its ticks (the pad lock
  and the menu gate need that value), and staged the same read after them,
  so the scene-entry fade and the prologue grade ran one frame behind. It
  re-reads after each tick now. A tier cannot see this one: both hosts reach
  the same export, and the difference is which side of the tick the read
  sits on - [one decision, two inputs](#one-decision-two-inputs) again.
- **The minigame status rows.** The slot, Baka, Muscle Dome and dance
  affordance rows were written out once per host and had drifted in wording
  (a Muscle Dome turn boundary telling the player to press Cross, where
  retail's turn top is automatic and `World::tick` advances it with no press;
  the slot exit reading "quit" on one host and "leave" on the other) and in
  space (the native window drew the Muscle Dome rows, the dance beat track and
  the Baka digit strip in raw surface pixels, top-left at a fraction of the
  page's size). One row builder per game lives in
  `engine-core::minigame_status`, one draw kernel in
  `engine-ui::ui_text_lines::status_row_draws_for`, and each host applies its
  one stage transform; tier 1 enumerates the kernel, so a host that stops
  calling it fails. Only fishing already had this shape
  (`PondSession::status_rows`).
- **A nameless item in the shop.** On a load without the executable the
  native shop printed `item 42` and the page `Item 2A`, each spelling its own
  fallback. One shop label helper reads `MenuState::item_label` now, the
  engine's `Item 2A` form, in the composition both hosts call
  (`legaia_engine_screens`).
- **The damage numerals' font fallback on the page.** Both hosts sample
  retail's 24x24 cells off the battle VRAM once it exists and fall back to the
  font before. The page's fallback could never draw: its layout read the
  battle camera through the render-gated `play_battle_camera_vp`, which
  answers empty on exactly the frames the fallback is for. The layout now
  reads the engine's pose (`World::battle_cam_pose`) directly.
- **The battle trail and move-FX streak aspect.** Both passes project into
  the 320x240 stage, and the native window built their camera at the
  surface's aspect, so on any window that is not 4:3 (the runner's 960x699
  included) the trail and the streak sat off the bodies. They use the stage's
  4:3 now, the aspect the scene pass draws at inside its stage viewport and
  the page's `battle_vp` call already used.
- **The target cursor's cue.** The pulse toward white on the pointed-at
  monster and the dim on the rest were hand-copied numbers in each host's
  draw pass; both read `battle_action::cursor_cue` now, under a `SIM_PAIRS`
  row.
- **Baka on the play page** was listed here as carrying no strike clock or
  afterimage. Both play hosts draw the duel through the shared
  `BakaDuelSurface::frame`, ghosts and impact effects included, and the
  page's `play_mg_baka_state_json` shares the minigames page's builder; the
  row was stale.
- **The CLUT-walk shimmer.** The overworld ocean and the field water /
  waterfall walkers were implemented three times - the native `WaterAnim`,
  the page's rebuild plus its `FieldSceneAnim` step, and the field-scene
  viewer's own resolve - and the copies had drifted: the page never ran the
  Drake-complement pass on a field scene, and tested strip coverage at column
  0 where a source cell can sit at any x. `engine-core::clut_walk_anim` is the
  resolve, the park (both layers), the ocean-head fallback and the per-game-
  tick step; all three surfaces call it, and a `SIM_PAIRS` row holds both play
  hosts' scene rebuilds to `ClutWalkAnim::install`. `legaia_asset::clut_walk`
  stays the parser.
- **A battle body's blend off its rest mesh.** The capture / defeat fade and
  the near-camera ghost apply the colour word's blend to every prim retail
  draws, posed or not. The native window applied it to posed meshes only, and
  its cue gate asked `pose_frame` for the same reason, so a body drawn from
  its rest mesh stayed opaque and un-cued there. Both hosts now go through
  `BattleActorDrawPlan::apply_body_blend` (the native window keeps each battle
  body's rest mesh CPU-side and re-uploads it blended while the word raises
  ABE) and `BattleActorDrawPlan::tint_cue_applies`, under two `SIM_PAIRS`
  rows.
- **The Muscle Dome arena in 3D.** The page posed the arena, the fighter
  and the monster in its script (swing clips picked off the turn edge, an
  orbit framing) and the native window drew no 3D dome at all. Both play
  hosts now drive `muscle_dome_scene::MuscleDomeSurface` - the seat, the
  choreography, the pose and `DomeCamera::vp_raw` - and upload what it
  returns, under two `SIM_PAIRS` rows. The standalone minigames page keeps
  its own dome panel (`minigame-muscle.js`).
- **The battle-intro arming.** The PROT 0979 load and relocation, the
  curtain table and tile-corner fallbacks, the shade-pack parse and the two
  `IntroEnv` seeds were written out on both hosts, with only the style inputs
  shared. `BattleIntro::arm_for_battle` (`engine-ui::battle_intro`) is the
  arming now; the page adds only its bottom-up capture flip. A `SIM_PAIRS`
  row holds both `arm_battle_intro` sites to it.
- **The Seru-trade screen's text.** The offer list's title, owner rows and
  empty-list line and the confirm question were formatted once per host.
  `seru_trade::trade_screen_text` is the text now, read by the one trade
  draw both hosts reach (`legaia_engine_screens`).
- **The shop root and Options row models.** Each host mapped
  `shop_menu_rows` onto its own label and ink table, and both left Quit
  white where retail's root window (`FUN_801D4868`) greys it with Sell on an
  empty bag; `menu_runtime::shop_root_labels` is the table now, and the
  engine-only Trade row takes the same rule. The Options screen's rows, the
  hand's row offset and the Key Config rows come from
  `OptionsSession::screen_model`; a host only borrows them into the view
  types and places the popup, under a `SIM_PAIRS` row. The shop root needs
  none: its one reader is the shared shop composition
  (`legaia_engine_screens`).
- **The save-select overlay sequence.** Each host sequenced the screen's
  overlays itself - the pills and their hand, the "Now checking" beat, the
  preview grid and info panel or its caption, the confirm messagebox - with
  the text and sprite halves in separate functions, and the page returned
  before every phase overlay when the chrome atlas was absent, where the
  native window still printed them. `SaveScreenFlow::overlay_model` is the
  sequence and `save_select_overlay_draws` (`engine-ui`) the one composition
  of both halves; its text half draws with or without the atlas. Both doors
  to the screen (boot Continue -> Load, the pause menu's Load / Save rows) go
  through it on both hosts; the `SIM_PAIRS` save-select row holds the calls.
- **Small copies moved onto one call.** The spoils line's leader name
  (`World::battle_spoils_leader`) and a save's resume point
  (`SceneHost::current_resume`, behind the native session's wrapper and the
  page's card and LGSF writers) and the name-entry view's cursor mapping and
  caret blink (`NameEntry::cursor_cells`, `name_entry::caret_on`) were each
  written out once per host.

### Open from the same pass

Recorded rather than fixed. Each names the host that lacks it or the copy
that could drift; none is gated.

- **World-map marker gates**, read and left as they are. Both hosts emit the
  markers through `marker_quads` and differ in two predicates, neither of
  which is drift: the player stand-in asks each host whether its own leader
  mesh drew (the native drained spawn slots, the page's player rig), and the
  native boot-panel gate has no page twin because no page boot panel draws
  over a world-map frame.
- **The render toggles** - PSX rasterisation and enhanced lighting, its
  shadow maps included - reach the page. See
  [the render toggles on the page](#the-two-opt-in-render-toggles-on-the-page).
- **The fishing wander readout (native only), left as a debug aid.** A
  dev-menu readout of `FUN_801d2050`'s tracked points, not a retail surface;
  nothing a player sees depends on it, so the page's dev menu carries no twin
  by choice.
- **The dance HUD quads draw on both hosts.** Both stage the hall's HUD page
  and emit `MinigameState::dance_hud_quads` through the shared
  `ui_dance::dance_hud_prims` (the play page from its battle prim pass); a
  side-by-side of the two shows the same score boxes and `Lv.` readout. What
  the pair still hid was host-identical: the quad list emitted the box frames
  before the digit runs, and since one ordering-table bucket holds the whole
  HUD and retail's `AddPrim` prepends, the opaque frames covered every score
  on both hosts. `DanceGame::hud_draw_quads` emits in `FUN_801d231c`'s order
  now - digits, then frames, then gauges
  ([minigame-dance](../subsystems/minigame-dance.md#hud-render-driver-fun_801d231c)).

### The two opt-in render toggles on the page

The play page carries both of the native window's render toggles as
checkboxes: PSX rasterisation (off by default, session-only) and enhanced
lighting (the persisted option, on by default, with a time-of-day selector
beside it). Both land in
[`site/js/webgl-shaders.js`](../../site/js/webgl-shaders.js), the one GLSL
program every 3D page shares, and both are the identity when off: the shader
gates each on a uniform that stays zero unless the play page stages it, and an
off frame of `town01` reads back byte-identical to the frame before the
toggles existed (re-measured tick-locked for enhanced lighting: zero differing
pixels against the pre-lighting shaders on the same bundle).

- **PSX rasterisation** is the native `psx_params` word as one `u_psx` vector
  (framebuffer width and height, snap on, dither on), staged by
  `TmdRenderer._applyRenderToggles` from both `render` and `renderAssembled`.
  The vertex stage snaps the projected position to the framebuffer's pixel
  grid after the overworld bend (`psxSnapClip`, twin of `psx_snap_clip`); the
  fragment stage ends in the PSX 4x4 ordered dither down to 15-bit colour
  (`psx_dither`). The page dithers where native does - both passes of an
  untextured prim, the opaque pass of a textured one, never the textured
  blend pass (a raw texel) or the after-image ghost. Two orientation details
  hold the hosts together: `gl_FragCoord` counts rows from the bottom where
  wgpu's position builtin counts from the top, so the dither row and the light
  pool both read `frag_top_px`; and the page snaps its untextured prims too,
  where native's colour-mesh vertex stage does not (the page draws both
  halves of a hybrid mesh in one program, and snapping one half would open
  cracks between them).
- **Enhanced lighting** is the native `dyn_light` + `scene_point_gain` without
  the shadow term: the mood's ambient floor, a `|N.L|` key light and a
  screen-centred pool, capped at 1.3x over the baked shading, plus up to eight
  point lights (half-Lambert wrap, `(1 - (d/r)^2)^2` attenuation) up to 1.9x,
  applied after the texel modulate and before the grade and cue; an emissive
  prim (TSB / blend bit 13) draws at the emissive gain plus its light, and a
  lit-window prim (TSB bit 12) turns its glass texels to lamp light by the
  mood's window glow (`dyn_window`, after `dyn_light` on both hosts).
  Textured prims light off smoothed per-vertex normals that
  `computeSmoothNormals` (`webgl-math.js`, the twin of
  `legaia_tmd::mesh::compute_smooth_normals`) derives on the CPU - only while
  the toggle is on, and again after a posed mesh's positions change, as
  native's posed mesh build does. Untextured prims and the ground light off
  the facet normal, as native's colour mesh and heightfield do. The law's
  constants are JS values interpolated into the GLSL, paired with their
  native twins in `scene_lighting.rs` by `check-ui-host-drift.py`'s constant
  table (as is the dither matrix); the mood and the lights come from the
  engine per frame, in the retail frame, and the page flips Y into its own.
  The glow quads draw in a second small program, additive, depth-tested.

The point lights' shadow maps reach the page too - see
[enhanced lighting on both hosts](#enhanced-lighting-shadow-maps-included-on-both-hosts).
Native has no in-game PSX toggle - the `LEGAIA_PSX_RENDER` environment
variable is its whole switch - so the page's checkbox is the one interactive
control for it.

## A tick-locked side-by-side pass

The earlier passes read the two hosts' source, or paired screenshots taken
at roughly the same moment. This one drives both hosts with the **same pad
script on the same world tick** and compares the frames, so any difference
is a difference in what the hosts do, not in when the picture was taken.

- **Native:** `play-window --screenshot-every N --screenshot-dir ... --pad-script ...`
  (a `TICK:BUTTON` edge or a `A-B:BUTTON` hold per entry; file names carry
  the world tick).
- **Page:** a headless Chromium driver pauses the view before its first
  frame (a setter on `window.__playView` calls `setPaused(true)` the moment
  the page creates it), then steps exactly one tick per frame through
  `view.step()`, writing the page's held / pulse key sets from the same
  script and screenshotting `.play-canvas-wrap` (the GL canvas plus the
  overlay canvas above it) on the same ticks. The page's keyboard layout is
  `Mapping::web_default` (`X` is Circle, `V` Square, `C` Triangle), not the
  desktop one - driving it with the native keys reads as a broken Circle.
  A GPU-backed Chromium (`--use-angle=vulkan`) keeps the run to minutes.

Two things look like drift and are not. `--seed-party` resets the story
flags to a fresh New Game, which changes what a scene's entry script does
(`cave01` fades in from black with it and opens lit without it); the page's
picker has no such seed, so a fair comparison drops the flag. And the pause
menu's play clock is wall time on both hosts (`World::tick_play_clock`), so a
tick-stepped page and a free-running window show different `TIME` values for
the same tick.

On those terms the field walk and camera, the dialogue box (typewriter, page
hand, row scroll), the pause menu's Items / Magic / Equip / Status / Options
screens, the battle open (`Begin | Run`, the command ring, `Auto | Command`,
the High / Low / Left / Right arts arm and the `Begin | Reselect` confirm),
the swing with its numerals and `HIT` / `TOTAL` cluster, and the spoils
banner and field return match frame for frame. The gaps that did not are below.

### The field readout over the battle transition

The page drew the party readout over the whole field-to-battle shatter and
the black hold after it. The suppress kernel
(`world_map_panel_host::field_hud_suppressed`) had no term for the
transition, so both hosts produced the readout; the native window painted it
under the intro's backdrop, which hid it by luck, and the page's overlay
canvas sits above the GL canvas. Retail draws none: the intro is PROT 0979
`field_battle_intro`, a slot-A overlay at `0x801CE818` - the slot the field
overlay (0897), and so `FUN_801D0D38` itself, lives in. The kernel now
suppresses while the encounter session sits in its `Transition` phase
(`field_battle_transition_active`), for both hosts.

### Sticky renderer state staged on one draw branch

The page draws its field, battle and minigame frames through one
`TmdRenderer`, and five of its setters store a value the renderer keeps until
the next call: the NCLIP cull word, the prologue colour grade and depth-cue
ramp, the palette-collapse half of the grade, and the overworld curvature.
All five were staged inside the field branch of `_frame` only, so a battle
drew under whatever the last field frame left:

- every fight entered from the **overworld** kept the overworld's screen-Y
  bend, which folded the mountains out of the battle backdrop and curled the
  sky down to the ground line;
- every battle kept the field's NCLIP cull armed on the stage dome, so the
  victory orbit - whose eye leaves the dome - looked straight through a shell
  the native window draws.

The native window stages all five once a frame ahead of its mode branches.
The page now does the same in `_stageFrameState`, called ahead of every
branch. This is the [GL state word](#a-gl-state-word-the-engine-does-not-own)
shape with a second half: the engine owned each decision and both hosts
asked it, but one host asked on one branch. No Rust tier can see it, so
`scripts/ci/check-js-sticky-frame-state.py` reads the two js files: every
`TmdRenderer` `set*` method whose body makes no `gl.` call is sticky, and
every play-page call of one must sit in `_stageFrameState` unless the setter
is classified as branch-owned with a reason. It runs in the pre-commit hook
when `site/` is touched and in CI.

### A second program in the scene's depth buffer, in another depth space

The page's mesh program writes `log2(w) / LOG_DEPTH_RANGE` to `gl_FragDepth`
on its perspective frames (`LOG_DEPTH_GLSL` in
[`site/js/webgl-shaders.js`](../../site/js/webgl-shaders.js)), because WebGL2
cannot select the native renderer's float reversed-Z buffer. The
enhanced-lighting glow program - the lamp halos and light shafts - is drawn
into the same buffer with the depth test on, and kept the rasterised
`gl_FragCoord.z`: about 0.99 at field distances against a scene written near
0.4, so LEQUAL rejected nearly every halo fragment. The night Genesis Tree
bloomed on the native window and barely glowed on the page; nothing failed,
the shader compiled, linked and drew into a test it lost. The glow program
now writes the same log depth under the same flag.

[`scripts/ci/check-js-depth-space.py`](../../scripts/ci/check-js-depth-space.py)
reads every GLSL fragment source under `site/js/` and requires a
`gl_FragDepth` write, unless the shader is waived with the reason its depth
never meets the log buffer (the overworld sea backdrop, which draws first; a
replay script no page loads). It runs in the pre-commit hook when `site/` is
touched and in CI.

### An effect billboard's semi-transparency enable

Retail sends every effect-pool child as prim code `0x2E`, a textured quad
with the GP0 semi-transparency bit set; the atlas entry stores only a
one-byte page, so the port's prim-ABE enable (TSB bit 15) has to be forced on
by whoever builds the quad. The native builder did; the page's battle and
field FX builders pushed the bare page, so no billboard triangle reached the
page's blend pass and every effect texel drew opaque - the battle's landing
dust (ABR 3, `B + F/4` at the envelope's low brightness, a faint haze on the
native window) sat on the floor as dark solid puffs. All three builders now
take `EffectSprite::packet_tsb`, and a `SIM_PAIRS` row in
`check-ui-host-drift.py` pins the call at each.

### The intro emitter stepped per draw

The tile shatter ran about two ticks ahead on the page: on the tick the
native window still showed the captured field frame, the page already drew
the first tiles lifting. Every intro style integrates its working set once per
`BattleIntro::tick` call, and the native window called it once per **redraw**
- a redraw can drain several world ticks, the capture redraw first among them
- while the page calls once per world tick. The page was right and the
native window was behind. Both now call `BattleIntro::advance_to(elapsed)`,
which steps once per clock unit the transition entity advanced and re-emits
the cached frame when it has not moved, so the picture is a function of the
clock alone (`the_frame_depends_on_the_clock_not_the_draw_cadence`). A
shared kernel whose state advances per call is still a host-cadence
dependency: sharing the code does not share the call rate.

### The opening crawl under a screen-effect push

Where `opdeene`'s timeline runs an op-`0x34` sub-0 push, the native window
composited every push over its text overlay and the page drew every push
under its overlay canvas. Retail decides per push: the glyphs sit at
ordering-table bucket `1` and a push at bucket `0` draws over them, a deeper
one under them ([cutscene.md](../subsystems/cutscene.md#a-push-in-front-of-the-text-or-behind-it)).
Both hosts now split on `screen_prim::screen_effect_push_prims_split` (the
`SIM_PAIRS` row names it); the page applies the over-text half to its overlay
canvas's pixels (`play_text_layer_washes_json`).

### Open from the same pass

- **The victory camera** was host-identical and is fixed in the engine: the
  battle-end sequence now frames the pose actor the way retail's results
  sequencer does ([battle.md](../subsystems/battle.md#the-victory-camera)).
  What still differs from retail there is the pose itself - the engine strikes
  the win pose once and hands back to the idle loop, so the camera films the
  character's back where retail's held pose has turned to face it.
- **The overworld leader** draws at roughly half the native window's size
  on the page, and the native size matches the retail frame of
  `keikoku_chest_preload`. The page poses the leader from the world-map
  clip bank; that path belongs to the world-map walk animation work.

## Shops, save points, movies and audio switching

A side-by-side read of the areas the tick-locked pass did not drive: the
shop and inn screens, the save-point menu press, the title and New Game
flow, the world map's menus, movies, BGM / SFX switching, the level-up
banner and the cheats panel. Closed rows:

- **Shops were silent on both hosts.** Retail keys a blip for every shop
  edge - the list kernel's cursor / confirm / buzz / cancel and the quantity
  and recipient screens' own purchase, sale and equip cues
  ([`shop.md`](../subsystems/shop.md#sound)). Neither host raised any, and
  the engine's quantity and recipient events named the cues in comments
  only. `MenuRuntime::take_ui_cue` is the decision now, and a `SIM_PAIRS`
  row holds both hosts' shop steps to it.
- **Start closed the pause menu outside the kernel on the page.** A
  root-level Start called `play_menu_close` directly, so it dismissed the
  save-point notice (which retail holds for Cross or Circle) and, under an
  op-`0x49` kind-`0x0D` ready check, closed the menu and released the
  parked script as answered where the kernel opens the Yes / No confirm.
  Start reaches `tick_root_list` on the page now, as it always did natively.
- **The scripted press's narration gate was a native-local term.** The
  native window added "no narration crawl" beside
  `World::scripted_menu_open_pending`; the page did not. The kernel holds it.
- **Catch-up ticks behind a screen.** The page ran up to four field ticks a
  frame and opened a shop or a scripted menu only at the next frame's top,
  so the field walked, pad live, behind a screen the engine had already
  opened. Its tick loop ends the field's run on the tick that opens one.
- **A shop's opening edge.** The native window handed a shop opened this
  tick the same tick's pad edge - the press that had just closed the
  merchant's line and run the script into op `0x49` - so that one press
  also confirmed the picker's first row. The opening step takes no edge.
- **Late BGM events.** The native window's field-event drain dropped every
  non-spawn event, BGM included, after the session tick's routing pass had
  run; a dance song ending on its own queued the hall's track there, so the
  chart loop played on over the hall. Both hosts' drains open with a late
  routing pass, which tier 12 pairs by content.

### Open from the same pass

- **The movie clock is written twice.** The native window paces a movie
  with audio off the XA cursor (`cutscene_av::due_video_frame`); the page
  re-implements that rule in `play-fmv.js` and adds a one-second stall
  fallback to the wall clock, which the native window lacks, so a stalled
  cursor holds frame 0 natively. The two STR decoders also differ: the page
  counts assembled frames and assumes 4-bit XA, the native one counts
  decoded frames and reads the bit depth.
- **A scene-script movie skips on the page only.** The page tests the skip
  button on every movie; the native window only on the title attract.
- **Paced per display frame on the page.** The game-over panel steps once
  per display frame on the page and once per sim tick natively, and the boot
  title catches up to thirty ticks a frame where the shared stepper allows
  four.
- **An overworld Load arms the top-view debug chord on the page only**
  (`enter_field_core`); the native in-game Load does not.
- **The inn prompt freezes the field on the page.** The page freezes on
  `MenuRuntime::is_open`, the native window on `suspends_field`, which
  excludes an inn. `InnSession` has no production caller
  ([`inn.md`](../subsystems/inn.md)), so no player reaches it.
- **The shop panels' rows** (`title`, rows, gold footer per state) and the
  inn prompt's text are written once per host over shared leaves; they agree
  today.
- **The level-up banner without menu chrome** draws in raw surface pixels
  natively and stage-scaled on the page. Only a run with no system-UI atlas
  takes that path.
- **Cheats.** Every cheat is one `World::cheat_*` call on both hosts; the
  hosts differ in which exist (restore HP / MP is page-only, a GameShark
  cheat file native-only) and when they apply (the native flags once at
  boot, the page's panel between any two ticks).
- **Absent on both:** a dialogue text blip, a door cue and any footstep cue
  (the page runs a footstep timer that keys nothing), and the casino prize
  counter's cues.

## Adding coverage

- a screen appears on the surface by existing; wire it on both hosts, or waive it;
- a paired constant joins tier 2 by being added to `CONSTANT_PAIRS`;
- a feature joins tier 3 by being added to `SIM_PAIRS` with its two sites;
- a trait joins tier 4 by having a default method body and two implementers;
- a diagnostic joins tier 6 by being declared in `DIAG_GATES` - which is not
  optional: an undeclared `LEGAIA_DIAG_*` fails the gate.
- a renderer setter that stores frame state is staged in the play page's
  `_stageFrameState`, or classified in
  `check-js-sticky-frame-state.py`'s `BRANCH_OWNED` with its reason;
- a boot install joins tier 13 by being a `world.install_*` / `world.set_*`
  call in the native boot - fold a new table into an engine install both
  boots call rather than a field assignment the tier cannot see.

Each script self-tests its own detectors on every run and refuses to report a
pass when a control fails - a "0 orphans" verdict from a classifier that
matched nothing is not a measurement. Run `--selftest` to see the controls,
`--list` for the full table.
