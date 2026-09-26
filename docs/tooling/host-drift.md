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
for the **host** triple, and about eighty files under `crates/web-viewer/`
carry `#[cfg(target_arch = "wasm32")]` blocks that none of those four ever
sees. A page feature written inside one can name a private field, a moved
method or a deleted type and stay green through the whole ladder; the first
thing that compiles it is a wasm build, which is where one was found.

[`scripts/ci/check-wasm-target.sh`](../../scripts/ci/check-wasm-target.sh)
closes that: `cargo check --release --target wasm32-unknown-unknown -p
legaia-web-viewer`, run from the pre-commit hook whenever a staged file
belongs to a crate carrying such a block. It is a type-check rather than a
build and shares the profile `build-wasm.sh` and the CI wasm step use, so a
warm run costs about a second.

It is a hook tier for a reason worth writing down rather than an exception to
the rule above. The `ci` job **does** build the wasm target - and it carries
`if: github.event_name != 'pull_request'`, so that build happens after a merge
to `main`, not on the pull request. The comment at the top of the workflow
says the opposite ("so PR builds catch regressions before merge"); the `if` is
what runs. Until the two agree, the hook is the browser host's only pre-merge
compiler.

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
and the native minigame screens are genuinely different screen sets, and one
more is a *better* implementation on one side (the browser's floating-damage
readout uses the font fallback while the native window samples the real 24x24
cells out of VRAM, which is what the builder's own doc asks a VRAM-capable host
to do).

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
shipped source. The three things that are not ownership are the three the page
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
tick per minigame (`dance_tick`, `baka_tick`, `slot_tick`, `fishing_pond_tick`,
`muscle_tick_time_meter`), each called by its own page module, so there is no
fall-through list for an arm to skip part of. A shared frame path is the thing
this tier measures; that host does not have one.

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
different claim, and the one the row makes now.

This tier asks the next question with the only evidence a source scan carries:
for each paired kernel, the set of **engine functions** each host's body
reaches. Engine means the four wgpu-free crates both hosts link
(`engine-core`, `engine-vm`, `engine-ui`, `engine-audio`). A host's own
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

### What the pairs showed

Four states captured on both hosts: a field scene at spawn, a battle at its
command phase, the boot title card and the boot save-select.

Confirmed matching, for rows that until now were only test-asserted: the
battle's sky clear colour and stage (B3), the command ring and its two
labels, the party HUD block's content and pen, the title card's bands and
its glyph-atlas menu rows, and the field scene's geometry, textures and
player placement.

One row the pair makes visible rather than settles: **C11, the dimmed title
art behind the boot save-select.** The native draws the Load frame and the
SLOT pills over the title card at reduced brightness; the page draws the
same frame and pills over black, because it drops its title session on
`TitleOutcome::Continue` (`crates/web-viewer/src/boot_title.rs`). The two
screens are otherwise identical, which is exactly why no gate fails: the
backdrop is a session the page released, not a draw it spells differently.

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
- **The native minigame hotkeys open sessions, not scenes.** `O` and `B` run
  the slot machine and Baka Fighter over the field with status text only,
  and `L` puts the fishing camera on whatever field the player stands in;
  the standalone minigames page draws the cabinet, the arena and the pond.
  Blocking capability: the native window has no pass for either minigame
  scene (the slot cabinet mesh and reels, the Baka arena and fighters).

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

**Same override set, different bodies (tier 4's declared blind spot, now
a worked example).** `WebBgmDirector` and `AudioBgmDirector` overrode the
same six `BgmDirector` methods, and the browser's duplicate-start guard was
id-only where the native one also asks "unpaused, and a sequencer is live" -
so a track that ended or was paused never restarted on the field VM's
re-emit, and the field music did not return after a battle. The same pass
found the sequencer master volume unset (127 against native's 100), the
loop policy hard-coded, and `stop` leaving the pause gate closed. The tier
cannot see any of these; only the paired bodies can.

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
bodies and motion-VM patrol routes were on unconditionally in the browser and
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
| derived scene lights | An enhancement the browser renderer cannot express. See [below](#derived-scene-point-lights-are-native-only). |

### Derived scene point lights are native-only

`--dynamic-lighting` stages per-scene derived point lights plus their PCF
shadow maps into the wgpu renderer. It is the enhancement layer, not retail -
the faithful path is pixel-identical with it off, which is the default - and
the page's GLSL program has neither a light array nor a shadow sampler.

Blocking capability: a point-light + shadow layer in
[`site/js/webgl-shaders.js`](../../site/js/webgl-shaders.js), and a per-frame
export of the picked light set. Both are real work, and neither buys retail
fidelity: this is the one row here where the *native* host is the one running
a non-retail path, so the page being without it is a feature gap rather than
a correctness gap.

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
no announcer line (see the Baka cabinet section below), but draws the widgets
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
when the exchange report arrived.

What stays open was misnamed "sprite effects". None of the three routines
draws a sprite: the two banks `_DAT_8007B888` / `_DAT_8007B840` are the
scene's type-`0x05` / `0x0B` ANM clip banks the clip selector `FUN_800204F8`
resolves, and each routine drives a **model** through them.

- The special's afterimage (`FUN_801D49E8`) is engine state every host steps,
  and the minigames page draws it as darkened copies of the thrower's mesh.
  The native window and the play page draw the duel as labels with no fighter
  meshes, so they have nothing to ghost. That is the gap - a 3D duel surface
  on those two hosts - not the afterimage.
- The impact pair (`FUN_801D4DF8`) spawns `FUN_80021B04` effect templates at
  the winner's strike offset, and no minigame host runs that effect runtime.
- The round-start cameo (`FUN_801D6310`) spawns only on a held Triangle, and
  no host hands the duel a held pad word; it also wants scene model `0` drawn
  camera-relative plus a VRAM move, which no host does.

And the native window resolves each glyph draw's stamped cell rect without
sampling it, because its duel HUD has no textured-quad surface.

**The slot machine's paylines are one projection on all three hosts.**
`slot_machine::projected_paylines` runs the ported payline pass
(`FUN_801D3380`) and projects both endpoints through the machine's fitted
projection; both browser pages stroke those segments (`sa` / `sb` in the prims
JSON) and the native window draws them as one-pixel flat quads through
`engine-ui::ui_slot_paylines`. The native window still draws no cabinet mesh
around them.

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
both hosts still classify it at fire time and decline it (`bgm.rs` logs and
`continue`s, `play_sfx.rs` counts it on `voice_cues_dropped`). That band was
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
| play page | `tick_field_fog_prims` in `play_field_fx.rs`, from `tick_battle_intro`'s prim assembly | `resolve_field_camera(world, camera, None, [0, 0])` |

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
3D shader produces from the same matrix. The overworld fog sheets take the
same channel (`fog_puff_prim`'s depth, flat at the bottom-right point retail
sorts the sheet by), since retail links them into the ordering table with the
continent. Every other primitive keeps the flag clear and sits on the near
plane, so it passes against any scene depth as before. Pinned by
`screen_prim`'s `only_a_depth_carrying_quad_is_depth_tested`,
`world_map_markers`' `walk_camera_quads_carry_scene_depth` and
`fog_particles`' `only_overworld_sheets_carry_a_scene_depth`.

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

The dance HUD's *quad* half reads as a native-only layer, and the native's own
materialiser opens with `let Some(src) = solid_src else { return Vec::new() }`
- and the one call site passes `None`, because no host stages the dance 4bpp
page into an atlas source. So the layer draws nothing on the host that has it.
Wiring it into the second host would have produced a second empty list and a
green row.

The blocking capability is the art, not the draw: a dance sprite page resident
in a host atlas, with a solid-texel rect to point `solid_src` at. Until that
exists this is one disclosure, not two.

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

The standalone minigames page keeps its own run model,
`baka_fighter::LadderRun` behind the `baka_run_*` surface: fixed serve order
from `baka_ladder()`, a page-drawn choice sheet at fitted positions, no
score-gated secret rungs. It draws the engine chrome with the sheet's own
widget art (`baka_chrome_json`), and plays no announcer line. That page has
no `World` to tick the cabinet through (next section), which is the
blocking capability for giving it the cabinet's ladder too.

Still disclosed on the two field hosts: the in-duel pause menu (`0xBE` /
`0xBF`) stays unreached, because the duel state's pause edge `0x110`
includes Triangle, which the port binds to the special attack, so the
cabinet sees a zero pad inside the duel. The digit strips are wired on both
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
`ringside_backdrop::first_visit_hub_draw` kernel.

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

The fix is the move: `shop_overlay_stage_draws` is a `&self` builder both
passes call, and the rect comes from `shop_panel_rows` (distinct baselines in
the text) plus `shop_panel_frame_rect` (pen, inset, width, row pitch), both in
engine-ui and both called by both hosts. Sizing off the text rather than off
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

## The field SFX ring: one producer queue, two replays

The field scripts' cue producers (field-VM op `0x36` sub `0` / `4`, the motion
VM's op `0x09`) run inside `World::tick`, but the ring they write lives with
the SPU, on the host side of the `engine-core` / `engine-audio` boundary. The
world therefore queues each call as a `SfxRingOp` and **both** hosts replay the
queue: the native `BootSession::route_field_sfx` into `AudioBgmDirector`, the
browser play page's `route_field_sfx` into `PlaySfx::sched`. The two share the
other halves too - the side-band bank resolver
(`World::side_band_bank`) and the runtime-row lookup
(`runtime_sfx_descriptor_in`) are engine functions, and each host only stages
and keys. The minigames page has no field and no queue to drain.

Two things a one-host reading of this would get wrong. The ring ages by the
vsyncs one host tick spans (`display_frame_step`, one), not by the game-tick
cadence `frame_step`: a host that fed it `frame_step` would play every delayed
cue at twice retail's rate at the field cadence of 2. And a ring id is never
routed through `classify_cue` - the scheduler returns ring cues in their own
list - because every runtime-bank id (`>= 0x200`) would otherwise land on the
CD-XA voice leg and be declined.

## The slot-2 / slot-6 SFX region: one residency, two restagers

Which bank the SPU region VAB slots `2` and `6` share holds is decided once, in
the engine: `World::sync_sfx_residency` models retail's field-bank latch
`0x8007BAFC` and the region's occupant off the world's mode edges (field and
world map load PROT 0876 into slot 6, battle and the Baka duel PROT 0869 into
slot 2, fishing / slot machine / dance their own banks), and field-VM op `0x36`
sub `3` runs `World::release_field_audio`. Each play host only restages: the
native `AudioBgmDirector::sync_shared_region` and the browser play page's
`LegaiaRuntime::sync_shared_region`, both called from their `route_field_sfx`
every tick, both placing the bank above the slot-0 system bank inside the same
`SFX_BANK_SPU_BYTES` window. Both resolve a routed cue to its own slot or to
silence (`bgm::resolve_sfx_slot` / `PlaySfx::resolve_slot`) - a class-2
fallback on one host only would make a field cue audible there and silent on
the other.

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

Two retail draws were missing from **both** play hosts, so no tier failed on
them. Both now draw on both hosts through one kernel each; what is left of
them, and the third gap still open, sits here in the form a waiver takes.

**The overworld curvature table on the continent.** `FUN_800271A8` builds a
depth-indexed screen-Y table every overworld consumer adds to `SY`
([`renderer.md`](../subsystems/renderer.md#frame-setup--present)). The fog
sheets and the drop shadow add it on the CPU; the continent bends per vertex
in the native mesh shaders (`OVERWORLD_CURVE_WGSL`, staged through
`Renderer::set_overworld_curvature`) and in the page's GLSL twin
(`overworldCurve`, staged through `play_render_curve_scale` ->
`setOverworldCurve`). Both hosts take the per-frame scale from the one kernel
`overworld_curvature::frame_curve_scale`, and both shaders evaluate the table
in closed form, pinned against it by `curvature_closed_form`. Residual, the
same on both hosts: the port bends every overworld scene draw, where retail's
four SCUS lit rows (`8..11`) do not bend. The `/world-overview/` viewer's
ocean plane shares the page's renderer but not its program, and stays flat
with the rest of that viewer (it never stages a scale).

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
call in `crates/engine-shell/src/boot.rs` and requires a shipped
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
a door runs (camera globals reset, SFX queue dropped, VAB restaged) and the
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

Recorded rather than fixed; each names the host that lacks it. None is gated.

- **Narration crawl.** The native window's crawl / title-card arm `continue`s
  after the session tick, skipping the FX ticks, CLUT effects, field-event
  drain, NPC re-bind, balloon sync and play clock the page runs every frame
  (`window/event_handler/redraw.rs`, the `boot_ui` / narration arm).
- **Movie frames.** The page gates only `host.tick()` on a live FMV; its FX,
  CLUT walker, NPC clips and SFX keep advancing behind the picture.
- **Anim-cue drains.** The window drains player gestures and NPC animate cues
  in `SceneMode::Field` only; the page drains them in every mode, so overworld
  gestures play on one host.
- **Shop tick.** The window ticks the world under an open shop with a neutral
  pad; the page freezes the world.
- **Sub-tick taps.** The window sets and clears its pad straight from key
  events, so a press and release between two ticks never reaches `set_pad`;
  the page latches it.
- **Overworld CLUT walk.** Implemented twice (`window/field_render.rs`
  `WaterAnim`, the page's `step_field_vram_fx`), already differing in which
  scenes patch strip rows and which column is checked.
- **Battle camera without a render.** The page's battle camera state lives on
  its render resource and does not tick when that fails to build; the window
  removed the same gate.
- **Monster action-tag clips** are installed from each host's render path, so a
  monster with no mesh - or a native frame's first battle ticks - reads no tag
  table.
- **Baka on the play page** carries no strike clock or afterimage in its JSON;
  the minigames page does.

And absent from both hosts, so no host is "behind": the `tick_scene_programs`
XA legs and `BgmDirector::reattach_volume` (sub-op `8`, which re-attaches the
slot at `0x8007057C` - not the field-BGM slot `0x8007052C` the other sub-ops
drive - with a level whose boot value is `-1`, so no director applies it yet).

### Closed from the same list

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
  that entry and re-applies the save after the swap - the native
  `enter_field_live_from_save` order
  (`cards.rs`, `a_card_load_keeps_the_saves_story_flags_across_the_scene_entry`).
- **The op-`0x35` timed release.** Its expiry set a flag no host read. The
  expiry arm is `FUN_800266E0`'s body on the field-BGM slot - sub-op `2`'s
  pause - so `World::tick` now emits that pause and both hosts' BGM routing
  acts on it ([`audio.md`](../subsystems/audio.md#the-timed-release-is-a-scheduled-bgm-pause)).

## Adding coverage

- a screen appears on the surface by existing; wire it on both hosts, or waive it;
- a paired constant joins tier 2 by being added to `CONSTANT_PAIRS`;
- a feature joins tier 3 by being added to `SIM_PAIRS` with its two sites;
- a trait joins tier 4 by having a default method body and two implementers;
- a diagnostic joins tier 6 by being declared in `DIAG_GATES` - which is not
  optional: an undeclared `LEGAIA_DIAG_*` fails the gate.
- a boot install joins tier 13 by being a `world.install_*` / `world.set_*`
  call in the native boot - fold a new table into an engine install both
  boots call rather than a field assignment the tier cannot see.

Each script self-tests its own detectors on every run and refuses to report a
pass when a control fails - a "0 orphans" verdict from a classifier that
matched nothing is not a measurement. Run `--selftest` to see the controls,
`--list` for the full table.
