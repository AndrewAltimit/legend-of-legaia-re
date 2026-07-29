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

The five host-drift tiers below all run in the `gates` job of
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
nor about any unpaired literal.

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
| Muscle Dome damage | `symbols_all` on the shared `resolve_turn_retail` entry point. |
| save-select model | `pattern_same` over the `SaveRack` variant each host builds. |
| live-loop arming | `symbols_all` on the shared `World::arm_live_loop`. |
| pause-menu open | `symbols_all` on `FieldMenuGate` + `SceneMode::Menu`. |
| game-over panel | `pattern_same` over the `game_over_draws_for` argument list. |
| play clock | `symbols_same` on `advance_play_time` across the two menu draw sites. |

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

**Game-over panel.** The assertion is that both hosts project the live
`GameOverSession` - its cursor, and the save-scan `continue_enabled` - rather
than a pinned pair of literals. `pattern_same` is the right mode because it
does not have to name what the arguments should be.

**Play clock.** The H:MM:SS box reads `World::play_time_seconds`, and that
counter only moves if a host drives `advance_play_time`. Substituting a frame
count at the *draw* site looks identical on screen and is not: the save writes
the world's counter, so a save taken from a host that never advanced it
records the play time it was loaded with.

## Tier 4 - trait-override symmetry

[`scripts/ci/check-trait-override-symmetry.py`](../../scripts/ci/check-trait-override-symmetry.py),
with waivers in
[`scripts/ci/trait-override-waivers.toml`](../../scripts/ci/trait-override-waivers.toml).

`engine-core` hands hosts their behaviour through traits, and several give
every method a default body - `BgmDirector` gives all eight. That is a
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

Four host differences are large enough to be projects rather than wiring, and
none of them is expressible as a waiver, because a waiver names a *builder* or
a *hook* and these are whole subsystems one host does not have. Recorded here
so a future reader does not mistake a green gate suite for host parity.

They are engineering work, not reverse-engineering questions, so they are
**not** on [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md) -
that page indexes contested questions about retail behaviour, and nothing
about these is contested.

| Gap | Shape |
|---|---|
| save-model unification | The two hosts build their save-slot model separately; the tier-3 `set_card_slots_mode` row is one symptom of it, not the whole of it. |
| shared camera on web | The browser page runs its own orbit projection beside the engine's camera controller instead of consuming it. |
| MDEC on web | `crates/mdec` decodes STR video for the native `play-str` path; the play page has no video decode, so an FMV beat has nothing to show. |
| battle 3D layer on web | The browser host runs live battles and draws the HUD, command menus and banners, but not the battle *scene* - no 3D layer stands behind them. |
| screen-space PSX primitives on web | The native renderer draws PSX `POLY_FT4`/`POLY_GT4` quads in ordering-table order; the browser has no equivalent, because `SpriteDraw` cannot carry a PSX primitive. See below. |
| field-to-battle transition on web | The native play window opens a battle with the retail transition - the fade on every style, the curtain where retail selects it. The browser page cuts straight to the battle, because the transition is drawn entirely out of the row above. |

### Screen-space PSX primitives: what the web host would need

This one is worth spelling out, because the surface tier 1 measures makes it
invisible - **and it is now a shipped asymmetry rather than a latent one.**
The native play window drives the field-to-battle intro emitter
(`engine-render`'s `battle_intro`) through
`RenderTarget::SceneWithScreenPrims` on every encounter; the browser play page
has no path that could and cuts straight into the battle. The simulation half
is shared and identical on both hosts - `World::tick_encounter` runs the
transition state machine either way, so the handoff timing, the BGM swap and
the battle that opens are the same. What differs is only whether anything is
drawn during it.

That is the honest shape of this gap, and it is deliberately **not** closed by
a waiver: no gate fails, because no gate can see a capability that has no type
to be measured through. Every screen-space effect retail draws - the field-to-battle
transition styles, the move-FX afterimage streak, any `screen_fx` sprite - is a
PSX primitive: a quad whose texels come out of VRAM through a per-primitive
CLUT/texpage pair, blended by one of four fixed ABR equations, ordered by an
ordering-table bucket rather than by a depth test. `engine-render` has that pass
([`renderer.md`](../subsystems/renderer.md#screen-space-ordering-table-pass)).
The browser does not, and the reason is a type: `engine-ui`'s `SpriteDraw` is a
**semantic alias of `TextDraw`** - an axis-aligned destination rect, an atlas
source rect and one flat RGBA tint. Nothing in it can express a texpage, a CLUT,
per-vertex UVs, per-vertex colour, an ABR mode or an OT bucket, so a builder
returning `SpriteDraw` is on tier 1's surface while the *capability* is not.

The browser is not starting from nothing: the play page already uploads a
1024x512 VRAM page (`field_vram_bytes`) and already samples it with the 4/8/15
bpp + CLUT decode for **3D** meshes. What is missing is the 2D half.

Five things, in the order they block each other:

1. **A draw record that is a PSX primitive.** Four corners rather than a rect,
   four `(u, v)` pairs, a `(cba, tsb)` pair, per-vertex colour, a
   semi-transparency flag and an OT index. `engine-render`'s
   `screen_overlay::ScreenPrim` is that record, and it is renderer-agnostic
   apart from its `bytemuck` vertex struct - the shape to lift into `engine-ui`,
   not to re-invent.
2. **An ordering-table sort, shared.** `order_primitives` is back-to-front by OT
   index with LIFO tie-breaking, which is `AddPrim` + `DrawOTag`. Two hosts
   sorting the same list differently is a divergence no gate would catch, so
   this is the one piece that must be *shared code* rather than a second
   implementation.
3. **A WebGL fragment path with the same CLUT decode as the 2D pass.** The 3D
   decode already exists on the page; the 2D pass needs the same function with
   no texture-window remap.
4. **Four ABR blend modes.** WebGL2 can express modes 1-3 with `blendFunc`;
   mode 0 (`0.5*B + 0.5*F`) needs a constant blend factor, exactly as the native
   pipeline does.
5. **A framebuffer capture.** The transition styles texture their strips with a
   *captured field frame*. Native does this by reading the drawn frame back into
   the software VRAM (`vram_capture`); the browser would render to an FBO and
   blit into its VRAM texture instead - cheaper there, since it never has to
   leave the GPU.

Items 1 and 2 are the ones that decide whether this ends as one model or two.
Doing them in `engine-ui`, where both hosts already meet, is what keeps the next
transition from being written twice.

## Adding coverage

- a screen appears on the surface by existing; wire it on both hosts, or waive it;
- a paired constant joins tier 2 by being added to `CONSTANT_PAIRS`;
- a feature joins tier 3 by being added to `SIM_PAIRS` with its two sites;
- a trait joins tier 4 by having a default method body and two implementers.

Each script self-tests its own detectors on every run and refuses to report a
pass when a control fails - a "0 orphans" verdict from a classifier that
matched nothing is not a measurement. Run `--selftest` to see the controls,
`--list` for the full table.
