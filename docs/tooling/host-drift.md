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

The six host-drift tiers below all run in the `gates` job of
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
| Muscle Dome damage | `pattern_same` over the `resolve_turn*` family each host names. |
| save-select model | `pattern_same` over the `SaveRack` variant each host builds. |
| live-loop arming | `symbols_all` on the shared `World::arm_live_loop`. |
| pause-menu open | `symbols_all` on `FieldMenuGate` + `SceneMode::Menu` + `dialogue_owns_input`. |
| game-over panel | `pattern_same` over the `game_over_draws_for` argument list. |
| dev-menu tick | `symbols_all` on `retail_packed` + `commit_equip_row` + the records-page toggle. |
| dev-records model | `symbols_all` on `record_counters` + `records_screen` across the two model builders. |
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

The row also names `dialogue_owns_input`, because *whether the menu opens at
all* is the same kind of invisible split. Retail's menu-open accept sits behind
the locomotion controller's engaged-bit branch, so Start is inert while the
player is talking; the two hosts refuse in two crates, and a host that dropped
the check would look identical in every screenshot that is not mid-conversation
(see [`field-menu.md`](../subsystems/field-menu.md#the-menu-does-not-open-at-all-while-a-dialogue-is-up)).

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
| shading law on web | The two hosts express one law in two shading languages. See [below](#the-two-hosts-do-not-share-a-shading-law). |
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

**This is not a kernel waiting to be hoisted.** The transition's simulation and
geometry already live where the browser can reach them - `engine-vm`'s
`battle_intro_styles` / `battle_intro_swirl` / `battle_intro_tiles` /
`battle_intro_transition`, all wgpu-free. What `engine-render`'s `battle_intro`
adds on top is entirely the render half: it reaches `crate::gte`,
`crate::billboard`, `crate::screen_overlay`, `crate::vram_capture`, and takes a
`&Renderer` + `RenderTarget`. Moving *it* would move the wgpu dependency, not
the logic. The five items above are the work, in that order, and item 5 is the
one with no browser analogue at all today: nothing on the page reads a drawn
frame back.

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

Meshes with genuinely no packet colour - the generated walk-ground heightfields
- keep the neutral constant and draw at the raw texel, which is the honest
answer for geometry that has no colour word to modulate by.

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

## Adding coverage

- a screen appears on the surface by existing; wire it on both hosts, or waive it;
- a paired constant joins tier 2 by being added to `CONSTANT_PAIRS`;
- a feature joins tier 3 by being added to `SIM_PAIRS` with its two sites;
- a trait joins tier 4 by having a default method body and two implementers;
- a diagnostic joins tier 6 by being declared in `DIAG_GATES` - which is not
  optional: an undeclared `LEGAIA_DIAG_*` fails the gate.

Each script self-tests its own detectors on every run and refuses to report a
pass when a control fails - a "0 orphans" verdict from a classifier that
matched nothing is not a measurement. Run `--selftest` to see the controls,
`--list` for the full table.
