#!/usr/bin/env python3
"""UI host-drift checker: does every shared screen reach BOTH hosts?

The engine ships the same game UI through more than one framebuffer:

* **native** - `legaia-engine play-window` (`crates/engine-shell`, wgpu via
  `crates/engine-render`),
* **web** - the browser pages built from `crates/web-viewer`. That is two
  surfaces, not one: the **play page** (`runtime.rs` + `play_*.rs`, driven by
  `site/js/play-app.js`) and the **minigames page** (`minigames*.rs`,
  `LegaiaMinigames`), which calls `engine-core` session types directly and
  never goes through `World`.

The web bucket is deliberately one label here, and saying why matters,
because collapsing it is where this gate's blind spot lives. The
*reachability* question below - "does a host's shipped source reach this
builder" - is answered per binary, and both web surfaces ship in one
`legaia-web-viewer` cdylib. Splitting them would not find a gap; it would
manufacture 80 of them, because the minigames page is a different screen set
rather than a second copy of the play page.

What the collapse really hides is a *model* question - the two web surfaces
can feed one shared kernel two differently-built models, and the pause-menu
reachability test can never see it. That is what [`SIM_PAIRS`] is for; see
"The third question" below.

`crates/engine-ui` is the wgpu-free leaf both hosts share: every screen's
geometry is a `pub fn ..._draws_for(...) -> Vec<TextDraw>` (or `SpriteDraw`)
builder there, and a host "has" a screen exactly when it calls that builder.
That makes the set of engine-ui draw builders a **machine-checkable feature
surface** - no hand-maintained list of screens to fall out of date.

The failure this catches: an engine wave adds a screen to engine-ui, wires it
into the native window, and the browser play page silently drifts a release
behind. Nothing about that is visible in a diff. Here it is a red CI run.

A host reaches a screen transitively too. Builders compose - the party info
panel folds in the AP gauge, the pen-only tab alias delegates to the tab
painter - so "is this screen on screen" is a question about the call graph, not
about which name the host happens to type. Host references seed the used-set and
then propagate along engine-ui's own internal call edges. Counting only the
shallowest wrapper made every composed widget read as unused, which is a defect
of the instrument rather than a gap in the port, and it rewarded a host for
naming a wrapper over calling the thing that draws.

The graph spans **every** `fn` engine-ui defines, not only the builders. Limiting
it to builder-to-builder edges invents orphans wherever the composition runs
through a method or a private helper, and engine-ui's fishing HUD is exactly
that shape: `FishingBanners::service_frame` takes the four banner builders as
function pointers and `HudDraw::resolve_bar` is what reaches `bar_frame` /
`power_bar_frame`. Both are `impl` methods, so a builder-only graph reported six
wired builders as unused - six waivers that would each have asserted a gap that
does not exist, which is worse than the silence it replaced. Non-builder `fn`s
are graph nodes only; nothing classifies them.

Classification per builder:

* used by both hosts              -> ok
* used by native, not by web      -> DRIFT (fail, unless waived)
* used by web, not by native      -> web-ahead (info only)
* used by neither                 -> ORPHAN (fail, unless waived)

Every orphan is **named** on stdout, waived or not. A count is not a finding: for
as long as this gate printed `6 unused` and nothing else, deleting a builder's
only caller was invisible - which is how `RecipientWindowRects::active_compare`
was removed and left window 25's painter chain with no consumer at all. A count
went from 5 to 6 and no line of output changed.

Waivers live in `scripts/ci/ui-host-drift-waivers.toml`; each needs a reason.
They are validated in both directions, which is what keeps the file honest:

* a waiver naming a builder that no longer exists   -> fail (stale)
* a waiver for a builder now wired on both hosts    -> fail (close it out)
* a `web_missing` waiver whose builder is not
  actually native-only any more                     -> fail (wrong bucket)

So the waiver file cannot rot into a lie: it only compiles as long as it
describes the real state of the two hosts.

## The second question: do both hosts feed the builder the same model?

Reachability is only half of "shared". Two hosts can call one builder with
divergently-constructed arguments and everything above stays green forever -
same screen, different geometry, no diff to look at. That blind spot is
structural: the checks above ask whether a host's source *names* a builder,
never what it passes.

The general form of the second question is not decidable from source text -
"does `assets.pen(id)` equal `self.menu_window_pen(id)` at runtime" is a
question about two programs, not two token streams. So this file does not
attempt it. What it does instead is pin the one part of the divergence that
*is* exactly decidable, and which is where the duplication actually sits:
**geometry constants that exist twice, once per host.**

The browser play page used to carry a 23-row pinned window-rect table whose
doc comment said it was "byte-identical to the native window's
`MENU_WINDOW_FALLBACK`". That sentence was the entire guarantee - a prose
assertion of the kind this repo has already watched go false in the waiver
file, where a bucket is re-derived from source every run but a *reason* is
not. [`CONSTANT_PAIRS`] turns those sentences into a check: each pair names a
constant on each host, and the two initialisers must normalise to the same
token stream.

A pair is the second-best outcome. The best one is that the constant exists
**once**, in a crate both hosts already depend on, at which point there is
nothing to pair and the row comes out of the table - which is what happened to
that window-rect table and to the near-fullscreen sub-screen rect: both now
live in `legaia_engine_ui::pause_menu` beside the composition that reads them.
Deleting a pair is therefore not always a loss of coverage; check which way it
went before restoring one.

Be precise about what that does and does not establish:

* it DOES prove two named constants carry equal values, and that neither was
  renamed or deleted out from under the pairing;
* it does NOT prove the two hosts *use* the constants the same way, that they
  build the same model, or that any un-paired literal agrees.

A narrow check that says so is worth more than a broad one that implies more
than it measured. Adding a pair is how the covered set grows.

## The third question: do both hosts feed the same MODEL to a shared kernel?

A geometry constant is the easy half of "same model". The hard half is the
simulation: two hosts can call one `engine-core` kernel having built its inputs
differently, or call different kernels entirely, and every check above stays
green - the screen is reachable, the rects agree, and the numbers on it come
from somewhere else.

[`SIM_PAIRS`] is the simulation twin of [`CONSTANT_PAIRS`]. Each row names a
feature and, per host, the **injection site** where that host hands a model to
the shared kernel, plus what must be true of both sites at once. Three
assertion modes, in increasing strength:

* `symbols_all`  - each named symbol must appear in both bodies,
* `symbols_same` - each named symbol must appear in both or in neither,
* `pattern_same` - the *set* of regex captures must be equal across the two.

`pattern_same` is the one that does not need the answer up front: it says the
two sites must agree without saying what they must agree on, so it keeps
working when the right set changes.

Scope, stated as narrowly as the constant check above:

* it DOES prove the two named sites mention (or omit) the same kernels, and
  that neither site was renamed or deleted out from under the pairing;
* it does NOT prove the arguments are equal, that the calls run in the same
  order, or that either site is reached at runtime.

A row may carry `blocked_on`, which marks a divergence that is known and being
closed elsewhere. That marker is validated in both directions exactly as a
waiver is: a `blocked_on` row that diverges reports and does not fail, and a
`blocked_on` row that has become **clean** FAILS, demanding the marker be
deleted. So a pending row cannot rot into a permanent exemption - the moment
the work lands, the gate says so.

## The fifth question: is a debug draw off on BOTH hosts?

Every question above asks whether a host *reaches* a surface. None of them can
see the shape where both hosts reach it and only one of them turns it off.

That shipped. The effect billboards carry a tinted wireframe outline so a spawn
stays readable when its texels are not resident; the native window gates it
behind `LEGAIA_DIAG_FX=1`, and the browser twin had no gate at all. Retail
draws no such rectangle, so every play-page fight stamped an opaque red-ish box
around every effect sprite - up to 25 at once - and a user reported it as a
rendering bug. Both hosts called the builder, the constants matched, the sim
pairs matched, no page carried a key table: **all four tiers above passed.**

[`DIAG_GATES`] declares every `LEGAIA_DIAG_*` env gate in the engine crates and
whether it is `additive` - whether it draws something retail does not. The
asymmetry is the whole point:

* a **subtractive** gate (suppress a layer, blend off, draw slots [a,b) only)
  can only remove pixels, so a host without it still renders retail-correctly
  and merely cannot bisect;
* an **additive** gate paints what retail never paints, so a host without it
  paints that thing in normal play, for every user.

So only additive gates require a twin. A WASM module has no process
environment, which is why this cannot be checked by looking for the env name on
both sides - the browser twin is a module static a page or console flips, and
the check is that its *initialiser* is false. Validated both ways, like the
waivers: an undeclared `LEGAIA_DIAG_*` fails (declare what it draws), and a
declared gate that no longer exists fails (drop the row).

Scope, stated as narrowly as the tiers above:

* it DOES prove every diagnostic env gate is declared, and that each additive
  one has a browser twin whose initialiser reads false;
* it does NOT prove the two gates suppress the *same* draw, that the twin is
  wired to anything, or that no un-gated debug draw exists under another name.

## Three more questions the tiers above were blind to

Each of the three closes a shape that a side-by-side reading of the two hosts
found with every tier green, and each is written up in
`docs/tooling/host-drift.md` with the bug that motivated it.

**Ownership** ([`OWNED_TYPES`]). A host can reach an engine type's outputs
without ever holding the type. The play page framed the field with a spherical
orbit of its own and held no `engine_core::camera::Camera`, so nothing there
routed the op-`0x45` beats into a controller, advanced the mover or wrote the
retail camera globals back - one absence producing a projection difference, a
simulation difference and two missing screens. No tier above can see a type
that is *absent*: there is no builder to miss, no constant to pair and no
injection site to diverge from. Ownership is a field declaration or a
construction in that host's own shipped source; a `use` line, a match arm and
a borrowed parameter are not.

**Variant coverage** ([`ENUM_COVERAGE`]). A shared enum's variant can be
entered on both hosts and answered on one - four minigame `SceneMode`s shipped
that way, reached through the shared scene host's door-warp drain and drawn
only natively, because their presentation is text lines and a hand-rolled 3D
scene rather than an `engine-ui` builder tier 1 enumerates. The variant list is
derived from the enum's own source, so a variant added tomorrow is measured;
what is declared is the enum and the per-(variant, host) waivers. A host
"answers" a variant by naming it *qualified* - an unqualified match would count
a same-named session type and report everything covered.

**Entry symmetry** ([`HOTKEY_SOURCES`]). The dance count-in and the how-to
actor were complete shared kernels that only the native window drove, from a
driver of its own reached by the `K` / `U` hotkeys - so *neither* host counted
in when a player walked into the hall. A gap that reads as "one host has it"
can be "one host's debug path has it", and the tiers cannot tell those apart
because both end at a live call site. For every `world.<method>()` the native
key arms call, this asks whether any call site exists outside the native
`bin/` tree. A debug probe is a legitimate answer and takes a waiver; the
waiver names the ARM (or the mechanism that superseded it), never a schedule.

Usage:

    python3 scripts/ci/check-ui-host-drift.py            # check, exit 1 on drift
    python3 scripts/ci/check-ui-host-drift.py --quiet    # findings only
    python3 scripts/ci/check-ui-host-drift.py --list     # full surface table
    python3 scripts/ci/check-ui-host-drift.py --selftest # detector control suite

Exit status: 0 = clean, 1 = drift / stale waiver / constant mismatch /
sim-pair mismatch, 2 = self-test failed.
"""

import argparse
import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

REPO = Path(__file__).resolve().parent.parent.parent
UI_SRC = REPO / "crates" / "engine-ui" / "src"
WAIVERS = Path(__file__).resolve().parent / "ui-host-drift-waivers.toml"

# Source roots per host. `engine-render` counts as native: it re-exports
# engine-ui wholesale and wraps some builders in GPU-resident batches, so a
# call there is still the native window reaching the screen.
HOSTS = {
    "native": [
        REPO / "crates" / "engine-shell" / "src",
        REPO / "crates" / "engine-render" / "src",
    ],
    "web": [REPO / "crates" / "web-viewer" / "src"],
}

# A draw builder is a public fn whose return type mentions one of engine-ui's
# own draw-record types - that is exactly "projects a view into
# renderer-agnostic geometry", i.e. one screen's (or one screen fragment's)
# layout.
#
# `TextDraw` / `SpriteDraw` are the terminal records a renderer consumes.
# The rest are engine-ui's *intermediate* records - a resolved screen that
# still needs a host-owned atlas or font to become quads:
#
#   HudDraw           the fishing HUD's item list (ui_fishing)
#   HudQuad           a PROT 0977 textured-Gouraud quad (other_game_hud)
#   DigitCell         one placed digit of a numeric readout (ui_fishing)
#   BarFrame          a resolved gauge bar (ui_fishing)
#   ComparePanelField one field of an equip stat-compare panel
#
# They belong on the surface for the same reason the terminal records do: a
# host "has" the screen exactly when it reaches the projection, and which
# record type the projection stops at is an engine-ui implementation detail.
# Leaving them off is not a smaller claim, it is a silent one - the whole
# window-25 compare-panel chain and the whole fishing HUD were invisible to
# this gate while every other pause screen was covered.
#
# Signatures here are routinely multi-line, so the return type is read from
# the span between the fn keyword and the opening brace of the body rather
# than from a single-line pattern.
DRAW_RECORDS = "TextDraw|SpriteDraw|HudDraw|HudQuad|DigitCell|BarFrame|ComparePanelField"

# One more return shape, and it is a shape rather than a named record: a
# builder whose projection stops at a **screen rect**. Two live in engine-ui -
# `painter_rect` (descriptor -> `PainterRect`) and `guarded_box_rect`
# (`FUN_801E4140`'s bottom-clip guard, `-> Option<(i32, i32, i32, i32)>`) -
# and neither was watched: the record-name rule never matches a bare tuple,
# and `PainterRect` was not on the list, so the gate would not count them as
# screens and would not accept a waiver for them either. That is the worst of
# the three outcomes - not "wired", not "waived", but *silent*: a rect builder
# could lose its only host caller with no line of output changing, which is
# the exact failure the orphan naming above was added to stop.
#
# Kept out of [`DRAW_RECORDS`] on purpose. That constant also feeds
# [`TRANSFORM_PARAM_RE`], whose job is "this fn consumes what screens
# produce", and a rect is the commonest *model* a real screen takes - the
# module docstring says so ("every real screen takes a model (a session, a row
# list, a rect, a font layout)"). Folding rects into the transform test would
# reclassify most of the pause-menu painter chain as plumbing.
RECT_RETURNS = r"PainterRect|\(\s*i32\s*,\s*i32\s*,\s*i32\s*,\s*i32\s*\)"

BUILDER_RE = re.compile(r"^pub fn (?P<name>[a-z0-9_]+)\s*[<(]", re.MULTILINE)
DRAW_RET_RE = re.compile(rf"->[^;{{]*(?:{DRAW_RECORDS}|{RECT_RETURNS})")

# Every `fn` engine-ui defines, at any indentation: free functions, `impl`
# methods and private helpers alike. These are the nodes of the internal call
# graph. Only builders are ever classified; the rest exist so a composition
# that runs through a method is not mistaken for an unused screen.
ANY_FN_RE = re.compile(r"\bfn\s+(?P<name>[a-z0-9_]+)\s*[<(]")

# ...but returning quads is not sufficient. A function that *takes* draw
# records and hands them back is a batching transform inside the draw
# pipeline, not a projection of a model into a screen. Counting those as
# screens is a defect of the instrument: nothing about a host "having" or
# "not having" one describes a gap between the two hosts, and the surface
# they inflate is exactly the surface a waiver file then has to explain.
#
# The shape the return-type rule alone could not see: `sprite_draws_for(
# requests: &[SpriteRequest], anchor)` - draw records in, draw records out,
# no model anywhere in the signature. It read as an unwired screen for as
# long as the gate has existed, and the waiver written for it says so in
# prose ("a generic anchor-translate helper ... not a screen").
#
# Deliberately keyed on the crate's own draw/request record types appearing
# in the PARAMETER list, which is the narrowest statement of "this consumes
# the thing screens produce". Over the current surface it reclassifies
# exactly one function; every real screen takes a model (a session, a row
# list, a rect, a font layout) and is untouched. `--selftest` pins both
# directions.
#
# The intermediate records count here too, and the case that proves it is
# `fishing_hud_draws_for(font, items: &[HudDraw], captions, atlas, origin)
# -> Vec<TextDraw>`. HudDraw records in, TextDraw records out: it is the
# fishing HUD's *renderer*, and the screens are `persistent_hud_draws` /
# `catch_hud_draws`, which take a model and return the item list. Counting the
# renderer as a screen would have reported DRIFT, since the browser host walks
# the same `HudDraw` list through its own projection - a true observation
# about the rendering half, which this gate has already said it cannot decide,
# reported in the one bucket that means "a screen is missing".
TRANSFORM_PARAM_RE = re.compile(rf"\b(?:SpriteRequest|{DRAW_RECORDS})\b")

LINE_COMMENT_RE = re.compile(r"//.*$", re.MULTILINE)

# Host source files the paired-constant check reads. Named here rather than
# discovered, because a pair is a claim about two specific declarations.
NATIVE_WINDOW = "crates/engine-shell/src/bin/legaia-engine/window.rs"
NATIVE_HUD = "crates/engine-shell/src/bin/legaia-engine/window/hud.rs"
NATIVE_DEV_MENU = "crates/engine-shell/src/bin/legaia-engine/window/dev_menu.rs"
WEB_PLAY_DEV_MENU = "crates/web-viewer/src/play_dev_menu.rs"
WEB_PLAY_MENU = "crates/web-viewer/src/play_menu.rs"
WEB_PLAY_SHOP = "crates/web-viewer/src/play_shop.rs"
NATIVE_BGM = "crates/engine-shell/src/bgm.rs"
WEB_RUNTIME = "crates/web-viewer/src/runtime.rs"
# The play page's BGM director (the browser twin of NATIVE_BGM's
# `AudioBgmDirector`), carved out of runtime.rs so the audio lanes own it.
WEB_BGM = "crates/web-viewer/src/play_bgm.rs"
# The occlusion-fade tunables are the one paired set whose web half is a
# plain script rather than a wasm crate - the browser play page holds them
# in its GLSL module. `const NAME = <value>;` parses identically either way,
# which is what lets one checker cover both.
NATIVE_OCCL = "crates/engine-render/src/occlusion_fade.rs"
WEB_SHADERS = "site/js/webgl-shaders.js"

# Geometry constants that exist once per host and must agree. See the module
# docstring for the scope of the claim: equal values, nothing about use.
#
# A pair earns its place by being a number the two hosts each hand to the
# SAME shared kernel - an engine-ui builder for the screen rows, an
# engine-audio one for the transition row. That is what makes a divergence a
# feature that behaves differently on the two hosts rather than an unrelated
# coincidence of two equal integers - `hud.rs`'s `BATTLE_HUD_PEN` is also
# `(8, 60)` and is deliberately NOT paired with the level-up pen, because
# nothing says the battle HUD and the level-up banner must move together.
CONSTANT_PAIRS: list[dict[str, object]] = [
    # Two pause-menu rows used to sit at the head of this list - the pinned
    # window-descriptor rect table and the near-fullscreen sub-screen rect.
    # They are gone because the constants are: both live once in
    # `legaia_engine_ui::pause_menu` (`MENU_WINDOW_FALLBACK` /
    # `MENU_SUBWINDOW_CONTENT`), read by the shared composition both hosts
    # call. A pair proves two copies agree; one copy needs no proof. The
    # engine-ui rect table is exercised by `tests/pause_menu_compose.rs`; the
    # disc side is pinned separately by the disc-gated `menu_windows_real`
    # test, which asserts the same rects against its own literal list rather
    # than against this constant.
    {
        "what": "field shop / inn overlay pen - shop_draws_for's `pen` argument",
        "native": (NATIVE_HUD, "SHOP_OVERLAY_PEN"),
        "web": (WEB_PLAY_SHOP, "SHOP_PEN"),
    },
    {
        "what": "level-up banner pen - level_up_draws_for's `pen` argument",
        "native": (NATIVE_HUD, "LEVEL_UP_BANNER_PEN"),
        "web": (WEB_PLAY_SHOP, "LEVEL_UP_PEN"),
    },
    {
        "what": "capture banner pen - capture_banner_draws_for's `pen` argument",
        "native": (NATIVE_HUD, "CAPTURE_BANNER_PEN"),
        "web": (WEB_PLAY_SHOP, "CAPTURE_PEN"),
    },
    {
        "what": "dev-menu list pen - the origin both hosts hand to "
        "dev_menu_list_draws_for / dev_menu_cursor_xy for the developer row "
        "list",
        "native": (NATIVE_DEV_MENU, "DEV_MENU_PEN"),
        "web": (WEB_PLAY_DEV_MENU, "DEV_MENU_PEN"),
    },
    {
        "what": "dev-records pen - the origin both hosts hand to "
        "records_screen_draws_for; the page's footprint only fits the 320x240 "
        "stage from this exact origin",
        "native": (NATIVE_DEV_MENU, "DEV_RECORDS_PEN"),
        "web": (WEB_PLAY_DEV_MENU, "DEV_RECORDS_PEN"),
    },
    {
        "what": "records-page heading strings - the `RecordsLabels` each host "
        "hands to records_screen_draws_for (kept out of engine-ui so no game "
        "text lives there, which is exactly what makes them a per-host "
        "duplicate)",
        "native": (NATIVE_DEV_MENU, "RECORDS_LABELS"),
        "web": (WEB_PLAY_DEV_MENU, "RECORDS_LABELS"),
    },
    {
        "what": "BGM transition click-guard ramp - the `fade_in_samples` "
        "argument each host's BGM director hands to swap_bgm. Long enough and "
        "the incoming track's intro is inaudible, which on a cutscene sting is "
        "the whole cue; the browser held a 22050-sample serial cross-fade here "
        "long after the native host had measured that down to two frames",
        "native": (NATIVE_BGM, "TRANSITION_FADE_IN_SAMPLES"),
        "web": (WEB_BGM, "TRANSITION_FADE_IN_SAMPLES"),
    },
    # The camera-occlusion fade is not an engine-ui screen - the shared
    # kernel here is a pair of hand-written twin shaders, so these four
    # numbers are the whole model and nothing downstream would catch a
    # divergence. They were unpaired while the radius was retuned twice.
    {
        "what": "occlusion-fade circle radius in WORLD units - projected per "
        "frame by each host (radius_px / occlRadiusPx) so the see-through-wall "
        "hole tracks the character through zoom",
        "native": (NATIVE_OCCL, "OCCL_RADIUS_WORLD"),
        "web": (WEB_SHADERS, "OCCL_RADIUS_WORLD"),
    },
    {
        "what": "occlusion-fade rim feather, as a fraction of the radius - "
        "the band each host's screen-door keep probability ramps across",
        "native": (NATIVE_OCCL, "OCCL_FEATHER_FRAC_OF_RADIUS"),
        "web": (WEB_SHADERS, "OCCL_FEATHER_FRAC_OF_RADIUS"),
    },
    {
        "what": "occlusion-fade screen-door keep floor at the circle centre - "
        "how transparent a faded wall gets (occl_params.y on both hosts)",
        "native": (NATIVE_OCCL, "OCCL_MIN_KEEP"),
        "web": (WEB_SHADERS, "OCCL_MIN_KEEP"),
    },
    {
        "what": "occlusion-fade view-depth clearance - how far in front of the "
        "player a fragment must sit to fade (occl_params.z on both hosts)",
        "native": (NATIVE_OCCL, "OCCL_DEPTH_MARGIN"),
        "web": (WEB_SHADERS, "OCCL_DEPTH_MARGIN"),
    },
]

# Simulation injection sites that must agree across hosts. See the module
# docstring's "third question" for the scope of the claim, and for what a
# `blocked_on` marker does and does not buy.
#
# A row's `sites` map a host label to `(repo-relative path, fn name or None)`.
# `None` means the whole file is the site, which is right when a host's
# injection is a call made from a place the pairing should not pin.
NATIVE_BOOT = "crates/engine-shell/src/boot.rs"
NATIVE_SAVE_HELPERS = "crates/engine-shell/src/bin/legaia-engine/window/save_select_helpers.rs"
NATIVE_ASSETS = "crates/engine-shell/src/bin/legaia-engine/window/assets.rs"
NATIVE_FRAME_TICK = "crates/engine-core/src/world/frame_tick.rs"
NATIVE_BOOT_CUTSCENE = "crates/engine-shell/src/bin/legaia-engine/window/boot_cutscene.rs"
NATIVE_REDRAW = "crates/engine-shell/src/bin/legaia-engine/window/event_handler/redraw.rs"
NATIVE_FIELD_RENDER = "crates/engine-shell/src/bin/legaia-engine/window/field_render.rs"
NATIVE_GEOMETRY = "crates/engine-shell/src/bin/legaia-engine/window/geometry.rs"
WEB_BOOT_TITLE = "crates/web-viewer/src/boot_title.rs"
WEB_MINIGAMES_MUSCLE = "crates/web-viewer/src/minigames_muscle.rs"
WEB_PLAY_BATTLE = "crates/web-viewer/src/play_battle.rs"
WEB_PLAY = "crates/web-viewer/src/play.rs"
NATIVE_REDRAW_PASSES = (
    "crates/engine-shell/src/bin/legaia-engine/window/event_handler/redraw_passes.rs"
)
NATIVE_CAMERA_MOD = "crates/engine-shell/src/bin/legaia-engine/window/camera.rs"
WEB_PLAY_CAMERA = "crates/web-viewer/src/play_camera.rs"
NATIVE_BATTLE = "crates/engine-shell/src/bin/legaia-engine/window/battle.rs"
WEB_PLAY_ARENA = "crates/web-viewer/src/play_minigame_arena.rs"
WEB_PLAY_FISHING = "crates/web-viewer/src/play_fishing.rs"
WEB_FIELD_SCENE = "crates/web-viewer/src/field_scene.rs"
NATIVE_TITLE_SAVE = (
    "crates/engine-shell/src/bin/legaia-engine/window/title_save_draws.rs"
)

SIM_PAIRS: list[dict[str, object]] = [
    {
        "what": "field screen-effect washes, native vs play page - the field "
        "VM's op `0x34` sub-0 arm spawns colour-tween actors whose per-frame "
        "`FUN_80024EE4(layer, blend, packed)` push IS the scene-entry "
        "fade-from-black and the door prologue's fade-to-black. Both hosts "
        "ticked the tween and NEITHER drew it: the pool had a producer and no "
        "consumer, so every scene entry simulated a fade in the clear. The "
        "three arguments are also three separate ways to get it backwards - "
        "`layer` is an ordering-table bucket and `blend` an ABR equation "
        "(two `i16`s a call site can swap), and `packed` is a GP0 colour word "
        "with red LOW, the opposite of every other kernel here. Both sites "
        "must emit through `screen_effect_push_prims`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_BATTLE, "tick_battle_intro"),
        },
        "mode": "symbols_all",
        "symbols": ["screen_effect_push_prims"],
    },
    {
        "what": "battle-intro style inputs, native vs play page - the style "
        "selector reads three retail globals (`DAT_8007BD60`, `DAT_8007BD0C`, "
        "`DAT_80084540`) and not one of them is a host choice: they are "
        "properties of the rolled formation row and the loaded scene. Both "
        "hosts resolved them inline and disagreed on `formation_slot0`, which "
        "is the input EVERY id-keyed style override keys on - the page went "
        "straight from the formation-table lookup to the bare row index, with "
        "no live-monster-table leg, so an in-battle re-arm fed the selector a "
        "row index and drew the default style. Both sites must read "
        "`SceneHost::battle_intro_style_inputs`",
        "sites": {
            "native": (NATIVE_BATTLE, "arm_battle_intro"),
            "web": (WEB_PLAY_BATTLE, "arm_battle_intro"),
        },
        "mode": "symbols_all",
        "symbols": ["battle_intro_style_inputs"],
    },
    {
        "what": "dance HUD frame rows, native vs play page - which rows the "
        "frame carries, at which 320x240 seats, in which pen, is the engine's "
        "decision (`DanceGame::hud_frame_rows`, the presentation half of "
        "`FUN_801d231c`): digit suppression, the `Lv.` label and the rival "
        "track's chart sampling are all retail's, not a host's. The whole "
        "resolution was written out longhand inside the native window's dance "
        "block, so the play page - same run, same `DanceGame` - drew a plain "
        "status line and no frame at all. Both sites must read the rows",
        "sites": {
            "native": (NATIVE_HUD, "build_hud"),
            "web": (WEB_PLAY_ARENA, "dance_status_draws"),
        },
        "mode": "symbols_all",
        "symbols": ["hud_frame_rows"],
    },
    {
        "what": "fishing prize one-time latch, native vs play page - "
        "`is_available` folds three independent refusals together (price, "
        "owned cap, one-time latch), so a host that reads it as the latch "
        "labels every unaffordable one-time prize on a fresh save as already "
        "taken. Each host answered the latch its own way - one re-tested "
        "availability with the other two gates forced open, one shifted the "
        "purchased mask by hand, and the play page never asked at all and "
        "exposed only the folded bool. Both sites must read "
        "`PrizeExchange::is_latched`",
        "sites": {
            "native": (NATIVE_HUD, "build_hud"),
            "web": (WEB_PLAY_FISHING, "play_fishing_prizes_json"),
        },
        "mode": "symbols_all",
        "symbols": ["is_latched"],
    },
    {
        "what": "camera-occlusion fade focus + arming, native vs play page - "
        "the fade has two halves that must name the SAME world point: the "
        "visibility gate ray-casts to it and the host stages it as the "
        "shader's focus. Each host spelled the point out locally, and the "
        "page's focus read the actor's own `world_y` while its gate read the "
        "floor tier under it, so on any tile where those differ the dissolve "
        "hole sat off the character. Its arming operands had drifted the same "
        "way - the native window excludes the boot UI, the world map, a "
        "scripted shot and the debug orbit, the page excluded only battle and "
        "the minigames. Both sites must reach the shared kernels",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_CAMERA, "play_occlusion_focus"),
        },
        "mode": "symbols_all",
        "symbols": ["player_body_centre"],
    },
    {
        "what": "`apply == 0` Camera Configure snap beats, native vs play "
        "page - retail's mover snaps the live camera globals to a snap beat "
        "immediately, and the field VM runs until yield, so a snap+glide pair "
        "committed in ONE tick must glide FROM the snapped pose. The beats "
        "are banked by `Camera::route_camera_events`, which is the only thing "
        "that drains `FieldEvent::CameraConfigure` off the world queue - a "
        "host watching its own later event drain for them sees none. Both "
        "cutscene interps must replay the shared bank",
        "sites": {
            "native": (NATIVE_CAMERA_MOD, "replay_camera_snap_beats"),
            "web": (WEB_PLAY_CAMERA, "resolve_camera_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["take_camera_snap_beats"],
    },
    {
        "what": "save-select phase layout, native vs play page - which pills "
        "draw, whether the pill cursor shows, and whether the block grid and "
        "its info panel stay up are one decision per `SelectPhase`, and the "
        "two hosts answered it differently for exactly one phase pair: retail "
        "raises the overwrite / delete prompt FROM the preview (see "
        "docs/subsystems/save-screen.md), so a confirm is a `SlotPreview` "
        "wearing a messagebox. The native window drew neither the grid nor "
        "the panel under it. Both sites must read `save_select::phase_layout`",
        "sites": {
            "native": (NATIVE_TITLE_SAVE, "save_select_chrome_sprite_draws"),
            "web": (WEB_PLAY_MENU, "build_save_select"),
        },
        "mode": "symbols_all",
        "symbols": ["phase_layout"],
    },
    {
        "what": "shop / inn / prize / coin overlay stage transform, native "
        "vs play page - every builder in that group places in the retail "
        "320x240 stage, so a host that composites its output has to scale it "
        "through the shared `pause_menu::stage_transform` before it reaches a "
        "surface-pixel draw list. The native window did not: it extended the "
        "group straight into `build_hud`'s list, so the whole shop UI drew at "
        "a third its size in a 960x720 window while the identical builders "
        "filled the browser tab. The pinned `SHOP_OVERLAY_PEN` / `SHOP_PEN` "
        "pair is blind to it by construction - the two pens ARE equal and the "
        "split is in the transform applied after them, which is the general "
        "lesson: a paired constant pins a value, not the space it lands in. "
        "Both composition sites must reach the transform and the scale pass",
        "sites": {
            "native": (NATIVE_HUD, "build_hud"),
            "web": (WEB_PLAY_SHOP, "play_overlay_draws_json"),
        },
        "mode": "symbols_all",
        "symbols": ["stage_transform", "scale_stage_text_draws"],
    },
    {
        "what": "field / overworld / cutscene camera, native vs play page - "
        "which camera owns a frame and what its retail GTE inputs are is one "
        "engine question (`camera_view::resolve_field_camera`), and the "
        "matrix each host uploads is that resolution projected "
        "(`camera_view::frame_vp`). The play page shipped the opposite: it "
        "held no `engine_core::camera::Camera` at all, framed the field with "
        "its own spherical orbit projection and re-mapped the op-0x45 "
        "cutscene params onto it - so the two hosts' cameras, their cutscene "
        "shots and the azimuth each fed the locomotion compass all diverged "
        "with no shared symbol to notice it. Both sites must reach the "
        "resolver, not a camera of their own",
        "sites": {
            "native": (NATIVE_REDRAW_PASSES, "compute_scene_camera"),
            "web": (WEB_PLAY_CAMERA, "resolve_camera_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["resolve_field_camera"],
    },
    {
        "what": "direct scene entry's camera reset, native vs play page - a "
        "scene picker / dev warp / load-from-save enters a scene by CALL, "
        "not through the in-world `SceneEntered` tick event both hosts "
        "already answer with `reset_globals_for_scene_entry`. The scene host "
        "clears the world's camera state on either path, but the follow view "
        "composes from the engine `Camera`'s globals, so a scene picked "
        "mid-cutscene kept the interrupted shot's pitch / yaw / eye trio and "
        "its focus latch and was framed by the old scene's camera. Both "
        "direct-entry sites must run `Camera::reset_for_scene_entry`",
        "sites": {
            "native": (NATIVE_BOOT, "enter_field_live"),
            "web": (WEB_RUNTIME, "enter_field"),
        },
        "mode": "symbols_all",
        "symbols": ["reset_for_scene_entry"],
    },
    {
        "what": "coplanar draw lifts, native vs play page - every host that "
        "assembles a field scene from EnvDraws must run the cross-draw "
        "coplanar kernel (`draw_plane_summaries` + `coplanar_draw_offsets`) "
        "and apply its lifts, or that host alone z-fights on every "
        "placement/terrain pair that meets on one world plane. The play page "
        "shipped exactly this gap: `build_field_render` resolved the same "
        "draws as the native shell and the field-scene viewer but never "
        "computed the lifts, so koin6's inn floor shimmered only in the "
        "browser play page (angle-dependently - invisible in a diff and in "
        "any single screenshot taken from the lucky angle)",
        "sites": {
            "native": (NATIVE_FIELD_RENDER, "compute_coplanar_env_offsets"),
            "web": (WEB_PLAY, "build_field_render"),
        },
        "mode": "symbols_all",
        "symbols": ["draw_plane_summaries", "coplanar_draw_offsets"],
    },
    {
        "what": "coplanar draw lifts, play page vs field-scene viewer - the "
        "two web surfaces assemble the same scene through the same resolver "
        "calls, so both must hand the combined draw list to the same "
        "coplanar kernel (see the native pairing above for the failure this "
        "catches). The field-scene viewer's whole assembly now lives in the "
        "shared kernel `engine-core::scene_assembly::assemble_field_scene` "
        "(which the native export-glb path also reads), so the viewer side "
        "of this pair is checked at the kernel",
        "sites": {
            "web_play": (WEB_PLAY, "build_field_render"),
            "web_viewer": (
                "crates/engine-core/src/scene_assembly.rs",
                "assemble_field_scene",
            ),
        },
        "mode": "symbols_all",
        "symbols": ["draw_plane_summaries", "coplanar_draw_offsets"],
    },
    {
        "what": "title attract hand-off, native vs play page - retail's "
        "`AttractIdle` (`0x10`) arm hands the screen to `fmv_id 0` and comes "
        "back to the menu, so a host that arms the countdown must also drive "
        "the session out of `TitlePhase::Attract` or the title freezes there "
        "forever. Both hosts go through the same three session calls; what "
        "they do between `mark_attract_started` and `finish_attract` is "
        "theirs (the window decodes the movie, the play page has no "
        "STR/MDEC playback and says so), and pinning the calls is what stops "
        "one host arming a countdown it cannot return from",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "service_title_attract"),
            "web": (WEB_BOOT_TITLE, "boot_title_step"),
        },
        "mode": "symbols_all",
        "symbols": ["attract_pending", "mark_attract_started", "finish_attract"],
    },
    {
        "what": "ground-heightfield sink, native vs play page - the walk-ground "
        "grid shares its plane with the env pack's authored floor art (koin6: "
        "both at y=0 with different tessellations), so every render site must "
        "sink it by the shared GROUND_SINK or that host's floors z-fight as "
        "wedge streaks from steep cameras while every other host is clean. "
        "Both build it through `field_ground::render_positions`, which is "
        "where the sink lives",
        "sites": {
            "native": (NATIVE_GEOMETRY, "heightfield_to_vram_mesh"),
            "web": (WEB_PLAY, "field_ground_positions"),
        },
        "mode": "symbols_all",
        "symbols": ["render_positions"],
    },
    {
        "what": "ground-heightfield winding, native vs play page - the grid's "
        "builder winds opposite to the scene TMDs, and the cutscene camera's "
        "NCLIP pass (mode 2) discards exactly that facing, so a host that "
        "uploads the builder's order loses the whole floor under a scripted "
        "shot (the page did, in the opdeene prologue). Both reverse it through "
        "`field_ground::render_indices`",
        "sites": {
            "native": (NATIVE_GEOMETRY, "heightfield_to_vram_mesh"),
            "web": (WEB_PLAY, "field_ground_indices"),
        },
        "mode": "symbols_all",
        "symbols": ["render_indices"],
    },
    {
        "what": "ground-heightfield sink, play page vs field-scene viewer - "
        "same property as the native pairing above, across the two web "
        "surfaces' ground exporters",
        "sites": {
            "web_play": (WEB_PLAY, "field_ground_positions"),
            "web_viewer": (WEB_FIELD_SCENE, "field_scene_ground_positions"),
        },
        "mode": "symbols_all",
        "symbols": ["render_positions"],
    },
    {
        "what": "Muscle Dome damage - the arena's per-exchange damage must come "
        "off the same battle-formula kernel on both hosts, or the same command "
        "deals different numbers in the window and in the browser. Both hosts "
        "install a `DomeDamageModel` and resolve through it, so the assertion "
        "is on the shared entry point, not on the formula leaf underneath it. "
        "It is `pattern_same` over the whole `resolve_turn*` family, not a "
        "name check on one of them: both hosts once named `resolve_turn_retail` "
        "and passed, while only the native side handled the kernel-absent "
        "return - so the browser contest hung in `Resolve` forever with the "
        "gate green. Naming the same resolvers is the property that matters, "
        "and it does not need the right set stated up front",
        "sites": {
            "native": (NATIVE_FRAME_TICK, "tick_muscle_dome"),
            "web": (WEB_MINIGAMES_MUSCLE, "muscle_resolve"),
        },
        "mode": "pattern_same",
        "pattern": r"(resolve_turn\w*)",
    },
    {
        "what": "pause-menu open through the mode seat - retail opens the menu "
        "by writing the mode word (`CARD INIT` stages the menu overlay and "
        "hands the word to `CARD MODE` at `0x80025974`), not by calling the "
        "menu, and going through `ModeSeat::enter` is what runs the "
        "mode-change edge with it. A host that only lets `adopt_world_mode` "
        "follow the world's scene mode reaches the same word by a different "
        "road: the chain of mode words is identical, the edge count is not, "
        "so no trace comparison catches it and only the paired call sites do",
        "sites": {
            "native": (NATIVE_BOOT, "open_field_menu"),
            # The browser's open calls the seat through one helper, so the
            # helper is the site: naming `play_menu_open` would pin the call
            # to `seat_open_card_menu` and not the seat call underneath it.
            "web": (WEB_RUNTIME, "seat_open_card_menu"),
        },
        "mode": "symbols_all",
        "symbols": ["request_card_mode"],
    },
    {
        "what": "scripted mesh re-bind (motion-VM op `0x0E`) - the world holds "
        "the actor's new model id per placement slot and each host resolves it "
        "to bytes through the scene's model bank. A host that resolves it "
        "anywhere else is resolving against `SceneResources::tmds`, which is a "
        "magic scan blind to a TMD inside an LZS bundle descriptor - which is "
        "where the four scenes that script a re-bind keep all of their models",
        "sites": {
            "native": (NATIVE_ASSETS, "upload_assets"),
            # The page asks through its own export, which is the body that
            # reads the world field; `play_npc_mesh` calls that export.
            "web": (WEB_PLAY, "play_npc_live_model"),
        },
        "mode": "symbols_all",
        "symbols": ["field_npc_live_model"],
    },
    {
        "what": "save-select model - which rack a host declares decides how "
        "many pills the screen shows and what each one addresses, so the two "
        "hosts must declare the same kind. No host sets the card-slots flag "
        "any more: `SaveSelectSession::for_rack` derives it from the "
        "`SaveRack` variant, and the driver around the second stage is the "
        "shared `save_screen::SaveScreenFlow` - so the assertion is on the "
        "rack kind each host builds, which is the one thing left that a host "
        "still chooses",
        "sites": {
            # The rack builder takes an optional mounted card image now
            # (`play-window --card`, port 2); `disk_save_rack` is the
            # test-only unmounted wrapper over it, so the row follows the
            # body that still declares the kind.
            "native": (NATIVE_SAVE_HELPERS, "disk_save_rack_with_card"),
            "web": (WEB_PLAY_MENU, None),
        },
        "mode": "pattern_same",
        "pattern": r"SaveRack::(\w+)",
    },
    {
        "what": "enemy Steal table - PROT 0941's `0x51` arm resolves a MONSTER "
        "victim against the static `DAT_80077828` table, so a host that boots "
        "a world without installing it draws nothing where the other host "
        "draws an item, and the two RNG cursors diverge from that battle on. "
        "Neither install is inside `World::arm_live_loop` (they are disc-table "
        "installs, which each host does for itself from its own disc source), "
        "so the live-loop pairing below cannot see them; they sit in different "
        "crates' boot paths and neither is reached by the other's tests",
        "sites": {
            "native": (NATIVE_BOOT, "enter_field_live"),
            "web": (WEB_RUNTIME, "load_disc"),
        },
        "mode": "symbols_all",
        "symbols": ["set_steal_table"],
    },
    {
        "what": "live-loop arming - the browser twin of `enter_field_live`. "
        "Every `World::set_*` one host installs before running the live "
        "gameplay loop and the other does not is a table the two simulations "
        "disagree about (drops, prices, spells, battle BGM). Both hosts now "
        "delegate to the shared `World::arm_live_loop`, so the assertion is "
        "that each still routes through it - scanning for `set_*` in the host "
        "bodies would pass trivially once the calls moved into the kernel",
        "sites": {
            "native": (NATIVE_BOOT, "enter_field_live"),
            "web": (WEB_PLAY_BATTLE, "arm_live_battles"),
        },
        "mode": "symbols_all",
        "symbols": ["arm_live_loop"],
    },
    {
        "what": "pause-menu open - retail gates the root list's last two rows "
        "on two scene-scoped values (the op-`0x49` entry context and the MAN "
        "header's save-allow bit) and suspends the field while the menu owns "
        "the frame. A host that opens the menu without sampling them into a "
        "`FieldMenuGate` draws every row white and opens every row, so a "
        "player can Save in one of the 96 scenes whose header forbids it; a "
        "host that does not switch the world into `SceneMode::Menu` leaves "
        "field dispatch running under the menu. Both are invisible in a diff, "
        "because the two open sites live in different crates",
        "sites": {
            "native": (NATIVE_BOOT, "open_field_menu"),
            "web": (WEB_PLAY_MENU, "play_menu_open"),
        },
        "mode": "symbols_all",
        "symbols": ["FieldMenuGate", "SceneMode::Menu"],
    },
    {
        "what": "menu-open precondition - every host that turns a Start edge "
        "into an open menu must ask `World::field_menu_open_allowed` rather "
        "than spell the test out locally. Three hosts each wrote their own "
        "copy and all three said `mode == Field`, which is how the OVERWORLD "
        "lost the pause menu: retail runs one locomotion controller "
        "(`FUN_801D01B0`) across the field and the kingdom overworlds, and "
        "the port splits that one retail mode into `Field` + `WorldMap`. The "
        "premise the copies rested on - that `FUN_801E76D4` is the "
        "overworld's controller with a Start handler of its own - is false; "
        "it is the top-view debug renderer. The symptom was silent in the "
        "worst way: the Save row is legal in exactly the three scenes no host "
        "would open the menu in, so the SAVE direction was unreachable by pad "
        "anywhere in the port while every oracle stayed green",
        "sites": {
            "native_window": (NATIVE_REDRAW, "handle_redraw"),
            "native_boot": (NATIVE_BOOT, "tick"),
            "web": (WEB_PLAY_MENU, "play_menu_open"),
        },
        "mode": "symbols_all",
        "symbols": ["field_menu_open_allowed"],
    },
    {
        "what": "party wipe - both hosts must route it to the title screen "
        "and nowhere else. Retail's wipe arm has exactly one exit store "
        "(`game_mode = 0x16` + `_DAT_8007BB00 = 1`), so a host that offers "
        "the player a row here has invented one. The panel that used to sit "
        "in this slot was exactly that, and the browser drew it from a pinned "
        "`(1, false)` while the native window drew it from a live cursor - "
        "two pictures of a menu that never existed. Pairing the routing "
        "sites, not the draw sites, is what keeps a second destination from "
        "reappearing on one host only",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_PLAY_BATTLE, "game_over_input"),
        },
        "mode": "symbols_all",
        "symbols": ["GameOverOutcome::ReturnToTitle"],
    },
    {
        "what": "BGM start - a music change must install the incoming track "
        "immediately. `swap_bgm` does; `crossfade_to` is a serial fade that "
        "parks the new sequencer and rolls the old one down to silence first, "
        "so the track has not begun a fade-length after the script asked for "
        "it. The browser held the crossfade long after the native host had "
        "measured it out, and the two calls live in different crates, so the "
        "difference is invisible in a diff - audible only on a cutscene sting, "
        "which is nearly all intro",
        "sites": {
            "native": (NATIVE_BGM, "start_inner"),
            "web": (WEB_BGM, "play"),
        },
        "mode": "symbols_all",
        "symbols": ["swap_bgm"],
    },
    {
        "what": "dev-menu tick - both hosts drive the shared `DevMenuSession` "
        "off their world's pad pump, and three pieces are each a silent "
        "cross-wire if one host drops them: the raw-to-packed pad conversion "
        "(`retail_packed` - without it Up arrives as PACK_TRIANGLE and Cross "
        "as PACK_DOWN), the EQUIP row's bag commit (`commit_equip_row` - "
        "without it the row steps an id and never equips), and the Square "
        "records-page swap (`RECORDS_TOGGLE`)",
        "sites": {
            "native": (NATIVE_DEV_MENU, "tick_dev_menu"),
            "web": (WEB_PLAY_DEV_MENU, "tick_dev_menu"),
        },
        "mode": "symbols_all",
        "symbols": ["retail_packed", "commit_equip_row", "RECORDS_TOGGLE"],
    },
    {
        "what": "dev-records model - both hosts assemble the records page "
        "from the same two kernels: the record-relative counter reads "
        "(`record_counters`, the save-block rebase) and the retail "
        "clamp/decompose model (`records_screen`). A host that reads the "
        "record fields itself, or skips the clamp, shows different numbers "
        "for the same save",
        "sites": {
            "native": (NATIVE_DEV_MENU, "dev_records_model"),
            "web": (WEB_PLAY_DEV_MENU, "dev_records_model"),
        },
        "mode": "symbols_all",
        "symbols": ["record_counters", "records_screen"],
    },
    {
        "what": "play clock - the H:MM:SS box the pause menu draws reads "
        "`World::play_time_seconds`, and that counter only moves if a host "
        "drives `advance_play_time`. The browser substituted `world.frame / 60` "
        "at the draw site instead, which reset on every page load, ignored a "
        "loaded save's hours, and - because the *save* writes the world's "
        "counter, not the drawn proxy - recorded the LOADED play time in every "
        "save taken from the browser. Asserted across the two "
        "`field_menu_draws_for` call sites, so the reader and the writer are "
        "pinned together",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, None),
            "web": (WEB_PLAY_MENU, None),
        },
        "mode": "symbols_same",
        "symbols": ["advance_play_time"],
    },
]

BLOCK_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
USE_STMT_RE = re.compile(r"\buse\s+[^;]*;")
CONST_DECL_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+{}\b", re.MULTILINE)


def const_initialiser(text: str, name: str) -> str | None:
    """The source of `const NAME ... = <here>;`, or None if not declared.

    Scans for the terminating `;` at nesting depth zero rather than taking the
    line, because these initialisers are multi-line array and block
    expressions. String and char literals are skipped so a `;` inside one
    cannot end the scan early.
    """
    m = CONST_DECL_RE.pattern.format(re.escape(name))
    decl = re.search(m, text, re.MULTILINE)
    if not decl:
        return None
    eq = text.find("=", decl.end())
    if eq < 0:
        return None
    i, depth, n = eq + 1, 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            i = n if nl < 0 else nl
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            lit = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if lit:
                i += lit.end()
                continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == ";" and depth == 0:
            return text[eq + 1 : i]
        i += 1
    return None


def normalise_value(src: str) -> str:
    """Reduce an initialiser to its value tokens.

    Drops comments, drops `use ...;` (both tables open with an alias import
    whose spelling is a host's own business), drops trailing commas before a
    closer, and collapses whitespace - so formatting, rustfmt directives and
    import aliasing cannot read as a value change, while a single changed
    digit always does.
    """
    src = BLOCK_COMMENT_RE.sub(" ", src)
    src = LINE_COMMENT_RE.sub(" ", src)
    src = USE_STMT_RE.sub(" ", src)
    src = re.sub(r",\s*(?=[)\]}])", "", src)
    src = re.sub(r"\s+", "", src)
    return src


def check_constant_pairs() -> list[str]:
    """Compare every [`CONSTANT_PAIRS`] entry; return one message per problem."""
    problems: list[str] = []
    for pair in CONSTANT_PAIRS:
        values: dict[str, str] = {}
        missing = False
        for host in ("native", "web"):
            rel, name = pair[host]  # type: ignore[index]
            path = REPO / rel
            if not path.is_file():
                problems.append(f"CONSTANT {name}: host source {rel} is missing.")
                missing = True
                continue
            raw = const_initialiser(path.read_text(encoding="utf-8"), name)
            if raw is None:
                problems.append(
                    f"CONSTANT {name}: no `const {name}` in {rel}. Renamed or "
                    f"deleted? Update CONSTANT_PAIRS in "
                    f"{Path(__file__).name} - an unresolvable pair checks nothing."
                )
                missing = True
                continue
            values[host] = normalise_value(raw)
        if missing or len(values) != 2:
            continue
        if values["native"] != values["web"]:
            nrel, nname = pair["native"]  # type: ignore[index]
            wrel, wname = pair["web"]  # type: ignore[index]
            nat, web = values["native"], values["web"]
            problems.append(
                f"CONSTANT DRIFT {nname} != {wname} ({pair['what']}):\n"
                f"      native {nrel}:\n        {first_difference(nat, web)}\n"
                f"      web    {wrel}:\n        {first_difference(web, nat)}\n"
                f"      Both hosts feed these to the same engine-ui builder, so "
                f"the screen now renders differently on the two hosts. Make them "
                f"agree, or move the value into a crate both hosts depend on."
            )
    return problems


def first_difference(this: str, other: str, window: int = 60) -> str:
    """`this` windowed around where it first departs from `other`.

    A 23-row table printed from its start is unreadable and, worse, useless:
    the two renderings agree for hundreds of characters, so a head-truncated
    dump shows two identical-looking lines and leaves the reader to diff by
    eye. The divergence is the only part worth printing.
    """
    i = 0
    while i < min(len(this), len(other)) and this[i] == other[i]:
        i += 1
    lo = max(0, i - window // 2)
    hi = min(len(this), i + window)
    return ("..." if lo else "") + this[lo:hi] + ("..." if hi < len(this) else "")


def signature_end(text: str, start: int) -> int:
    """Index of the body's `{` for the `fn` at `text[start]`, or -1 if none.

    Scans at bracket depth zero so a `;` inside an array return type
    (`-> [f32; 4] {`) does not read as a bodyless trait declaration, and a
    genuinely bodyless `fn f(..);` does.
    """
    i, n, depth = start, len(text), 0
    while i < n:
        c = text[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and c == "{":
            return i
        elif depth == 0 and c == ";":
            return -1
        i += 1
    return -1


def site_source(rel: str, fn_name: str | None) -> tuple[str | None, str]:
    """The source a [`SIM_PAIRS`] site names: one fn body, or the whole file.

    Returns `(text, problem)`; `text` is None when the site cannot be
    resolved, in which case `problem` says why. An unresolvable site is
    always an error - a pairing that cannot find its own anchor checks
    nothing, and would otherwise pass forever after a rename.
    """
    path = REPO / rel
    if not path.is_file():
        return None, f"host source {rel} is missing"
    text = path.read_text(encoding="utf-8")
    if fn_name is None:
        return strip_comments(BLOCK_COMMENT_RE.sub(" ", text)), ""
    m = re.search(rf"\bfn\s+{re.escape(fn_name)}\s*[<(]", text)
    if not m:
        return None, f"no `fn {fn_name}` in {rel} (renamed or deleted?)"
    brace = signature_end(text, m.start())
    if brace < 0:
        return None, f"`fn {fn_name}` in {rel} has no body"
    return strip_comments(BLOCK_COMMENT_RE.sub(" ", fn_body(text, brace))), ""


def sim_pair_divergence(pair: dict) -> tuple[list[str], list[str]]:
    """Evaluate one [`SIM_PAIRS`] row.

    Returns `(hard_errors, divergences)`. A hard error (an unresolvable site,
    a malformed row) always fails; a divergence fails unless the row carries
    `blocked_on`.
    """
    sites: dict = pair["sites"]  # type: ignore[assignment]
    errors: list[str] = []
    bodies: dict[str, str] = {}
    for host, (rel, fn_name) in sites.items():
        text, problem = site_source(rel, fn_name)
        if text is None:
            errors.append(f"SIM {pair['what']!r}: {problem}")
        else:
            bodies[host] = text
    # A row may name any number of sites from two up. This used to demand
    # EXACTLY two and return `([], [])` otherwise - so a three-site row was
    # not a failure and not an error, it was silently unevaluated, and the
    # gate reported clean while checking nothing. That is the worst shape a
    # checker can have, and it hid behind the fact that every row happened to
    # be a pair when the guard was written. A row with fewer than two sites is
    # now a hard error, because it cannot compare anything either.
    if len(bodies) < 2 and not errors:
        errors.append(
            f"SIM {pair['what']!r}: needs at least two resolvable sites, got {len(bodies)}"
        )
    if errors:
        return errors, []

    hosts = sorted(bodies)
    a, b = hosts[0], hosts[1]
    mode = pair.get("mode")
    diffs: list[str] = []

    def where(host: str) -> str:
        rel, fn_name = sites[host]
        return f"{rel}::{fn_name}" if fn_name else rel

    if mode in ("symbols_all", "symbols_same"):
        for sym in pair.get("symbols", []):  # type: ignore[union-attr]
            seen = {h: re.search(rf"\b{re.escape(sym)}\b", bodies[h]) is not None for h in hosts}
            if mode == "symbols_all" and not all(seen.values()):
                missing = [h for h in hosts if not seen[h]]
                diffs.append(
                    f"`{sym}` is not called at {', '.join(where(h) for h in missing)}"
                )
            elif mode == "symbols_same" and len(set(seen.values())) > 1:
                has = [h for h in hosts if seen[h]]
                lacks = [h for h in hosts if not seen[h]]
                diffs.append(
                    f"`{sym}` is called at {', '.join(where(h) for h in has)} "
                    f"but not at {', '.join(where(h) for h in lacks)} - "
                    f"all must, or none may"
                )
    elif mode == "pattern_same":
        pat = re.compile(pair["pattern"])  # type: ignore[arg-type]
        found = {h: {m.group(1) for m in pat.finditer(bodies[h])} for h in hosts}
        # Every site must agree with the union, so a third host cannot carry a
        # stray match that a pairwise `a != b` comparison would never look at.
        union: set[str] = set().union(*found.values())
        for h in hosts:
            extra = sorted(found[h] - set().union(*(found[o] for o in hosts if o != h)))
            missing = sorted(union - found[h])
            if extra:
                diffs.append(f"only {where(h)}: {', '.join(extra)}")
            if missing:
                diffs.append(f"missing at {where(h)}: {', '.join(missing)}")
    else:
        errors.append(f"SIM {pair['what']!r}: unknown mode {mode!r}")
    return errors, diffs


def check_sim_pairs() -> tuple[list[str], list[str]]:
    """Compare every [`SIM_PAIRS`] row.

    Returns `(problems, pending)`. `pending` lists the rows whose divergence
    is disclosed by `blocked_on`; a `blocked_on` row with NO divergence is a
    problem, not a pass - the marker has outlived the gap and must go.
    """
    problems: list[str] = []
    pending: list[str] = []
    for pair in SIM_PAIRS:
        errors, diffs = sim_pair_divergence(pair)
        problems.extend(errors)
        if errors:
            continue
        blocked = pair.get("blocked_on")
        if diffs and not blocked:
            problems.append(
                f"SIM DRIFT ({pair['what']}):\n"
                + "".join(f"      {d}\n" for d in diffs)
                + f"      The two hosts hand different models to the same kernel. "
                f"Make the sites agree, or record why not with `blocked_on`."
            )
        elif diffs:
            pending.append(f"{pair['what']}\n      " + "\n      ".join(diffs))
        elif blocked:
            problems.append(
                f"STALE blocked_on ({pair['what']}): the two sites now agree, so "
                f"the marker describes a gap that is closed. Drop `blocked_on` "
                f"from the row in {Path(__file__).name} - a pending marker that "
                f"outlives its gap is a permanent exemption wearing a temporary "
                f"name."
            )
    return problems, pending

# A host "has" a screen when its *shipped* code draws it. Both native roots
# carry `#[cfg(test)]` modules inside `src/` - `engine-render/src/tests/` is a
# whole directory of them - and a builder exercised only by a unit test is
# precisely the not-wired case this gate exists to surface. Counting those
# references made the gate report test coverage as wiring, which let four
# `web_missing` waivers assert "native calls it" about builders no host called.
# The two-directional waiver validation could not catch that: it re-derives the
# bucket from the same over-counted signal.


def is_test_source(path: Path) -> bool:
    """Is this file test-only code rather than shipped host code?

    Deliberately **path-only**: a `tests/` directory component or a `tests.rs`
    file name, which is the split-out `mod tests;` convention every test module
    under these roots follows (`engine-render/src/tests/` is a whole directory
    of them, and it is where all six mis-bucketed references lived).

    Sniffing file *contents* for `#[cfg(test)]` was tried and rejected: plenty
    of shipped files carry an inline test module, so a content rule drops real
    host code from the scan - it excluded `engine-render/src/lib.rs`,
    `engine-render/src/renderer.rs` and `engine-shell/.../window.rs` among
    others. That direction is the dangerous one. Over-counting "used" only
    makes the gate nag about a screen that is in fact wired; under-counting it
    invents ORPHANs and, worse, lets a waiver be written asserting a gap that
    does not exist. The path rule cannot do that, because a file under
    `src/tests/` is never a host draw site.
    """
    return "tests" in path.parts or path.name == "tests.rs"


def strip_comments(text: str) -> str:
    """Drop `//`-style comments.

    Doc comments name sibling builders constantly (`[`shop_draws_for`]`), and
    a mention in prose is not a wiring. Stripping them keeps the checker
    conservative in the safe direction: it under-reports "used", so it can
    nag about a screen that is in fact wired, but it never stays silent about
    one that is not.
    """
    return LINE_COMMENT_RE.sub("", text)


def is_screen_signature(signature: str) -> bool:
    """Is this `fn` signature one screen's geometry builder?

    `signature` is the source span from the `fn` keyword up to (not
    including) the body's opening brace. Two conditions, and the second is
    the one a return-type-only rule was missing:

    1. it returns quads (`TextDraw` / `SpriteDraw`), and
    2. it does **not** take quads - a function fed the crate's own draw or
       request records is a transform over a draw list, not a projection of
       a model into one.
    """
    if not DRAW_RET_RE.search(signature):
        return False
    arrow = signature.rfind("->")
    params = signature[:arrow] if arrow >= 0 else signature
    return not TRANSFORM_PARAM_RE.search(params)


def collect_builders() -> dict[str, str]:
    """Map builder name -> `path:line` where it is defined."""
    out: dict[str, str] = {}
    for path in sorted(UI_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in BUILDER_RE.finditer(text):
            # The signature runs from the fn keyword to the body's opening
            # brace; anything past that is the body and must not be sniffed
            # for a return type.
            brace = signature_end(text, m.start())
            if brace < 0:
                continue
            if not is_screen_signature(text[m.start() : brace]):
                continue
            line = text[: m.start()].count("\n") + 1
            rel = path.relative_to(REPO)
            out[m.group("name")] = f"{rel}:{line}"
    return out


def fn_body(text: str, brace: int) -> str:
    """The source between `text[brace]` and its matching close brace.

    Rust-aware enough to brace-match: comments, string / raw-string / char
    literals are skipped so a `format!("{}", ..)` or a lone brace in a string
    cannot unbalance the scan. Everything else is counted, which is all the
    caller needs - it only greps the result for identifiers.
    """
    n = len(text)
    i, depth = brace, 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            i = n if nl < 0 else nl
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if c == "r" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + 1
            while j < n and text[j] == "#":
                j += 1
            if j < n and text[j] == '"':
                term = '"' + "#" * (j - i - 1)
                end = text.find(term, j + 1)
                i = n if end < 0 else end + len(term)
                continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : i]
        i += 1
    return text[brace + 1 :]


def collect_fn_names() -> set[str]:
    """Every `fn` name engine-ui defines, at any indentation."""
    out: set[str] = set()
    for path in sorted(UI_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in ANY_FN_RE.finditer(text):
            out.add(m.group("name"))
    return out


WORD_RUN = re.compile(r"[0-9A-Za-z_]+")


def word_set(text: str) -> set[str]:
    r"""Every maximal `[0-9A-Za-z_]` run in `text`.

    `re.search(rf"\b{re.escape(n)}\b", text)` is true for an identifier `n`
    exactly when `n` is one of these runs - Python's `\b` is a word/non-word
    transition, so "matches as a whole word" and "is a maximal word run" are
    the same predicate. Asking it this way costs one pass over the text
    instead of one compiled search per candidate name, which is the whole
    difference between this gate costing half a minute of CPU and costing a
    second: the two host trees crossed with the engine-ui name set was 173k
    regex searches, 92% of the run.

    The run class has to be `[0-9A-Za-z_]+` and not the Rust-identifier
    `[A-Za-z_][0-9A-Za-z_]*`. The latter tokenises `1foo` as `foo` and would
    report a match where `\bfoo\b` finds none - a numeric literal silently
    promoted to a call edge, in the direction that hides drift.
    """
    return set(WORD_RUN.findall(text))


def collect_call_graph(names: set[str]) -> dict[str, set[str]]:
    """Map engine-ui fn name -> the engine-ui fn names its body references.

    This is the engine-ui-internal half of the call graph. It spans free
    functions, `impl` methods and private helpers, because a composition edge
    is an edge wherever it is written - see the module docstring for the six
    builders a builder-only graph reported as unused while both hosts drew
    them.

    Nodes are keyed by bare name, so two `impl`s that both define `service`
    merge into one node. That over-counts "used", which is the safe direction
    this file's `is_test_source` docstring argues for at length: the failure
    it can produce is a nag about a screen that is in fact wired, never a
    waiver asserting a gap that does not exist.
    """
    refs: dict[str, set[str]] = {n: set() for n in names}
    for path in sorted(UI_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in ANY_FN_RE.finditer(text):
            name = m.group("name")
            if name not in refs:
                continue
            brace = signature_end(text, m.start())
            if brace < 0:
                continue
            body = strip_comments(fn_body(text, brace))
            refs[name] |= word_set(body) & names
            refs[name].discard(name)
    return refs


def seed_transitively(uses: dict[str, set[str]], refs: dict[str, set[str]]) -> None:
    """Propagate each host label along the builder call graph, to a fixpoint.

    If a host draws builder A and A composes builder B, the host draws B. Cycles
    are harmless - the loop only ever adds labels, so it terminates.
    """
    changed = True
    while changed:
        changed = False
        for name, callees in refs.items():
            hosts = uses.get(name)
            if not hosts:
                continue
            for callee in callees:
                if not hosts <= uses[callee]:
                    uses[callee] |= hosts
                    changed = True


def collect_uses(names: set[str]) -> dict[str, set[str]]:
    """Map builder name -> set of host labels that call it."""
    uses: dict[str, set[str]] = {n: set() for n in names}
    for host, roots in HOSTS.items():
        for root in roots:
            if not root.is_dir():
                continue
            for path in root.rglob("*.rs"):
                if is_test_source(path):
                    continue
                body = strip_comments(path.read_text(encoding="utf-8"))
                for name in word_set(body) & names:
                    uses[name].add(host)
    return uses


def load_waivers() -> dict[str, dict]:
    if not WAIVERS.is_file():
        return {}
    data = tomllib.loads(WAIVERS.read_text(encoding="utf-8"))
    out: dict[str, dict] = {}
    for entry in data.get("waiver", []):
        name = entry.get("builder")
        if name:
            out[name] = entry
    return out


# Detector control suite for `is_screen_signature`. Real signatures, copied
# from `crates/engine-ui/src` (multi-line ones flattened - the caller feeds it
# a raw source span either way).
SELFTEST_SCREENS: list[tuple[str, str]] = [
    # A model in, quads out: the ordinary screen shape.
    ("shop_draws_for",
     "pub fn shop_draws_for(font: &legaia_font::Font, title: &str, rows: &[ShopRow<'_>], "
     "cursor: usize, gold: Option<i32>, pen: (i32, i32)) -> Vec<TextDraw>"),
    # A glyph layout is a model of text, not a draw record - the sibling of
    # `sprite_draws_for` that must stay counted, or the surface loses a
    # screen fragment both hosts really do share.
    ("text_draws_for",
     "pub fn text_draws_for(layout: &legaia_font::Layout, pen: (i32, i32), "
     "color: [f32; 4]) -> Vec<TextDraw>"),
    # Sprite-returning screens are screens: the parameters are a model.
    ("equip_screen_sprites_for",
     "pub fn equip_screen_sprites_for(view: &EquipView<'_>, rects: &SaveMenuAtlasRects, "
     "origin: (i32, i32), scale: u32) -> Vec<SpriteDraw>"),
    # Tuple returns count too - the painter family returns quads beside
    # pictogram / cursor requests.
    ("sell_quantity_draws_for",
     "pub fn sell_quantity_draws_for(font: &legaia_font::Font, rect: PainterRect, "
     "selected: bool, heading: &str, quantity: u32, held: u32, unit_price: u32) "
     "-> (Vec<TextDraw>, Option<PainterPictogram>, Option<PainterSprite>)"),
    # Intermediate records are screens: a model in, a resolved screen out,
    # still needing the host's atlas or font to become quads. These four are
    # the whole reason the surface widened.
    ("persistent_hud_draws",
     "pub fn persistent_hud_draws(points: i32, best_points: i32, rod_index: u32, "
     "lure_count: i32) -> Vec<HudDraw>"),
    ("number_digit_cells",
     "pub fn number_digit_cells(style: i32, x: i32, y: i32, value: i32) -> Vec<DigitCell>"),
    ("bar_frame",
     "pub fn bar_frame(x: i32, y: i32, value: i32, segments: i32, style: i32) -> BarFrame"),
    ("equip_compare_panel_fields",
     "pub fn equip_compare_panel_fields(view: &EquipComparePanelView<'_>, "
     "pen: (i32, i32)) -> Vec<ComparePanelField>"),
    # Rect returns are screens: the projection stops one step earlier than a
    # quad, at the rect a caller then paints into. Both shapes are here
    # because the record-name rule matches neither, which is how the two below
    # went unwatched - not counted, and not waivable either.
    ("painter_rect",
     "pub fn painter_rect(descriptor: &MenuWindowDescriptor) -> PainterRect"),
    ("guarded_box_rect",
     "pub fn guarded_box_rect(x: i32, y: i32, w: i32, h: i32) "
     "-> Option<(i32, i32, i32, i32)>"),
]

SELFTEST_TRANSFORMS: list[tuple[str, str]] = [
    # The shape the return-type-only rule could not see.
    ("sprite_draws_for",
     "pub fn sprite_draws_for(requests: &[SpriteRequest], anchor: (i32, i32)) "
     "-> Vec<SpriteDraw>"),
    # Same shape with the other two record types, so the rule is not pinned
    # to one name.
    ("rebatch_text",
     "pub fn rebatch_text(draws: &[TextDraw], origin: (i32, i32)) -> Vec<TextDraw>"),
    ("merge_sprites",
     "pub fn merge_sprites(a: &[SpriteDraw], b: &[SpriteDraw]) -> Vec<SpriteDraw>"),
    # Not a draw builder at all - no quads out.
    ("scale_stage_text_draws",
     "pub fn scale_stage_text_draws(draws: &mut [TextDraw], stage_origin: (i32, i32), "
     "stage_scale: u32)"),
    # A rect in the PARAMETER list is a model, not a transform input: rects
    # are deliberately kept out of `TRANSFORM_PARAM_RE`, so this stays a
    # screen-shaped signature that simply returns no geometry.
    ("enemy_target_menu_rows_y",
     "pub fn enemy_target_menu_rows_y(host_box: Option<(i32, i32, i32, i32)>) -> i32"),
    # The intermediate-record renderer: HudDraw list in, TextDraw list out.
    # A projection of a screen it did not build - the screens are
    # `persistent_hud_draws` / `catch_hud_draws` above.
    ("fishing_hud_draws_for",
     "pub fn fishing_hud_draws_for(font: &legaia_font::Font, items: &[HudDraw], "
     "captions: &FishingCaptions<'_>, atlas: &FishingHudAtlas<'_>, origin: (i32, i32)) "
     "-> Vec<TextDraw>"),
    ("compare_panel_draws_for",
     "pub fn compare_panel_draws_for(font: &legaia_font::Font, "
     "fields: &[ComparePanelField]) -> Vec<TextDraw>"),
]

# Control suite for `signature_end`, the scan the whole call graph rests on.
# Each case is `(label, source, should_find_a_body)`.
SELFTEST_SIGNATURES: list[tuple[str, str, bool]] = [
    ("ordinary fn has a body", "fn f(a: i32) -> u8 { 0 }", True),
    ("array return type's `;` is not a terminator",
     "fn tint(x: u8) -> [f32; 4] { [0.0; 4] }", True),
    ("trait declaration has no body", "fn on_scene_enter(&mut self, s: &str);", False),
    ("generic fn has a body", "fn f<T: Into<u8>>(v: T) -> Vec<TextDraw> { vec![] }", True),
]

# Control suite for the sim-pair comparators, over synthetic bodies so the
# modes are pinned independently of whatever the tree happens to look like.
# Each case is `(label, mode, body_a, body_b, extra, should_diverge)`.
SELFTEST_SIM: list[tuple[str, str, str, str, object, bool]] = [
    ("symbols_all: both call it", "symbols_all",
     "let d = damage_finish_lazy(&f);", "damage_finish_lazy(&g)", ["damage_finish_lazy"], False),
    ("symbols_all: one host misses it", "symbols_all",
     "let d = damage_finish_lazy(&f);", "let d = ad_hoc_damage(&g);",
     ["damage_finish_lazy"], True),
    ("symbols_same: neither calls it is agreement", "symbols_same",
     "nothing()", "nothing_else()", ["set_card_slots_mode"], False),
    ("symbols_same: exactly one calls it is drift", "symbols_same",
     "s.set_card_slots_mode(true)", "s.reset()", ["set_card_slots_mode"], True),
    ("pattern_same: same set in any order", "pattern_same",
     "w.set_a(); w.set_b();", "w.set_b(); w.set_a();", r"\.(set_[a-z0-9_]+)\s*\(", False),
    ("pattern_same: a missing installer is drift", "pattern_same",
     "w.set_a(); w.set_b();", "w.set_a();", r"\.(set_[a-z0-9_]+)\s*\(", True),
]


# Control suite for the page-key detector (tier 5), over synthetic page
# sources. `(label, source, should_flag)`. Both directions matter: a detector
# that flagged every `.key` would fail the play page's scene-group records,
# and one that flagged nothing would have passed the minigames page's real
# `{a:1,s:2,d:3}` table for as long as it existed.
SELFTEST_PAGE_KEYS: list[tuple[str, str, bool]] = [
    ("e.key read is a page-side table", "const k = e.key.toLowerCase();", True),
    ("event.key read is a page-side table", "if (event.key === 'a') go();", True),
    ("e.code against a bindable key is a page-side table",
     "if (e.code === 'KeyA') go();", True),
    ("e.code against a non-pad key is fine", "if (e.code === 'Escape') close();", False),
    ("a plain object's .key field is not a KeyboardEvent",
     "scenes.filter(s => s.category === g.key);", False),
    ("dataset.key is an attribute, not a KeyboardEvent",
     "const k = kbd.dataset.key;", False),
    ("resolving through the engine table is the fix",
     "const b = window.legaiaPadButtonOf(e.code); if (b === 'Circle') cast();", False),
    ("a bindable code in a set membership test is a page-side table",
     "const startEdge = p.has('Enter');", True),
    ("...including the key this rule was written for",
     "if (held.has('Space')) pause();", True),
    ("a non-key literal in a set test is fine",
     "if (seenScenes.has('town01')) skip();", False),
    ("testing the pad bit is the fix",
     "const startEdge = (padMaskOf(p) & window.legaiaPadButton('Start')) !== 0;", False),
]


def _selftest_page_key_case(src: str) -> bool:
    """Run the tier-5 detectors over one synthetic source; True when flagged."""
    if EVENT_KEY_RE.search(src):
        return True
    if any(m.group(1) not in NON_PAD_CODES for m in EVENT_CODE_LITERAL_RE.finditer(src)):
        return True
    codes = bindable_dom_codes()
    return any(m.group(1) in codes for m in KEY_SET_LITERAL_RE.finditer(src))


def _selftest_sim_case(mode: str, a: str, b: str, extra: object) -> bool:
    """Run one synthetic sim-pair comparison; True when it diverges."""
    hosts = ["native", "web"]
    bodies = {"native": a, "web": b}
    diffs: list[str] = []
    if mode in ("symbols_all", "symbols_same"):
        for sym in extra:  # type: ignore[union-attr]
            seen = {h: re.search(rf"\b{re.escape(sym)}\b", bodies[h]) is not None for h in hosts}
            if mode == "symbols_all" and not all(seen.values()):
                diffs.append(sym)
            elif mode == "symbols_same" and seen["native"] != seen["web"]:
                diffs.append(sym)
    else:
        pat = re.compile(str(extra))
        found = {h: {m.group(1) for m in pat.finditer(bodies[h])} for h in hosts}
        if found["native"] != found["web"]:
            diffs.append("pattern")
    return bool(diffs)


# Control suite for the constant-pair normaliser. A normaliser that collapsed
# everything to "" would report every pair equal and the check would be
# theatre, so both directions are pinned: noise must vanish, values must not.
#
# Each case is `(label, source_a, source_b, should_match)`.
SELFTEST_CONSTANTS: list[tuple[str, str, str, bool]] = [
    (
        "formatting and import aliasing are noise",
        "{ use legaia_asset::menu_windows::window_ids as w;\n"
        "  [ (w::TAB_ITEMS, (16, 12, 60, 12)),\n"
        "    (w::TAB_MAGIC, (16, 12, 60, 12)) ] }",
        "{use foo::bar as w; [(w::TAB_ITEMS,(16,12,60,12)),(w::TAB_MAGIC,(16,12,60,12)),]}",
        True,
    ),
    (
        "comments are noise",
        "(18, 18, 284, 200) // the near-fullscreen stage rect",
        "(18, 18, 284, 200) /* same rect, different note */",
        True,
    ),
    (
        "one changed digit is a value change",
        "(8, 140)",
        "(8, 141)",
        False,
    ),
    (
        "a dropped table row is a value change",
        "[(w::A, (1, 2, 3, 4)), (w::B, (5, 6, 7, 8))]",
        "[(w::A, (1, 2, 3, 4))]",
        False,
    ),
    (
        "a reordered table is a value change (id order is the table)",
        "[(w::A, (1, 2, 3, 4)), (w::B, (5, 6, 7, 8))]",
        "[(w::B, (5, 6, 7, 8)), (w::A, (1, 2, 3, 4))]",
        False,
    ),
]


# Control suite for `word_set`, which stands in for the per-name
# `re.search(r"\bNAME\b", body)` the reachability pass used to run. The
# substitution is the reason this gate costs a second instead of half a
# minute, and it is only sound if the two predicates agree on every string -
# so each case is checked twice: against the stated expectation, and against
# the regex it replaced. A control that only asked "does word_set say yes"
# would pass just as happily for a tokeniser that had drifted along with it.
SELFTEST_WORDS: list[tuple[str, str, str, bool]] = [
    ("plain call", "let v = foo(bar);", "foo", True),
    ("method position", "self.model.foo();", "foo", True),
    ("path position", "engine_ui::foo(&font)", "foo", True),
    ("prefixed name", "let v = draw_foo(bar);", "foo", False),
    ("suffixed name", "let v = foo_draws_for(bar);", "foo", False),
    # The one an identifier-shaped tokeniser gets wrong: `[A-Za-z_][\w]*`
    # finds `foo` inside `1foo`, `\bfoo\b` does not.
    ("digit-prefixed run", "let v = 1foo;", "foo", False),
    ("digit-suffixed run", "let v = foo2;", "foo", False),
    ("absent", "let v = bar(baz);", "foo", False),
]


def run_selftest() -> int:
    failures = 0
    for label, text, name, want in SELFTEST_WORDS:
        got = name in word_set(text)
        ref = re.search(rf"\b{re.escape(name)}\b", text) is not None
        if got == want and ref == want:
            print(f"  ok    word set: {label}")
        else:
            print(
                f"  FAIL  word set: {label} - word_set={got}, "
                f"regex={ref}, expected {want}"
            )
            failures += 1
    for name, sig in SELFTEST_SCREENS:
        if is_screen_signature(sig):
            print(f"  ok    {name}: counted as a screen")
        else:
            print(f"  FAIL  {name}: dropped from the surface (expected a screen)")
            failures += 1
    for name, sig in SELFTEST_TRANSFORMS:
        if is_screen_signature(sig):
            print(f"  FAIL  {name}: counted as a screen (expected a transform)")
            failures += 1
        else:
            print(f"  ok    {name}: excluded as a draw-list transform")
    for label, a, b, want in SELFTEST_CONSTANTS:
        got = normalise_value(a) == normalise_value(b)
        if got == want:
            print(f"  ok    constants: {label}")
        else:
            verdict = "matched" if got else "differed"
            print(f"  FAIL  constants: {label} - normaliser {verdict}")
            failures += 1
    for label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) == want:
            print(f"  ok    signature: {label}")
        else:
            print(f"  FAIL  signature: {label}")
            failures += 1
    for label, mode, a, b, extra, want in SELFTEST_SIM:
        if _selftest_sim_case(mode, a, b, extra) == want:
            print(f"  ok    sim pair: {label}")
        else:
            print(f"  FAIL  sim pair: {label}")
            failures += 1
    for label, src, want in SELFTEST_PAGE_KEYS:
        if _selftest_page_key_case(src) == want:
            print(f"  ok    page keys: {label}")
        else:
            verdict = "flagged" if not want else "passed"
            print(f"  FAIL  page keys: {label} - detector {verdict}")
            failures += 1
    for label, init, want in SELFTEST_DIAG:
        if initialiser_is_off(init) == want:
            print(f"  ok    diag toggle: {label}")
        else:
            print(f"  FAIL  diag toggle: {label}")
            failures += 1
    for label, rule, src, want in SELFTEST_RENDER:
        if _selftest_render_case(rule, src) == want:
            print(f"  ok    render kernel: {label}")
        else:
            verdict = "stayed silent" if want else "fired"
            print(f"  FAIL  render kernel: {label} - detector {verdict}")
            failures += 1
    for label, src, name, want in SELFTEST_OWNERSHIP:
        if owns_type(src, name) == want:
            print(f"  ok    ownership: {label}")
        else:
            verdict = "claimed ownership" if not want else "saw none"
            print(f"  FAIL  ownership: {label} - detector {verdict}")
            failures += 1
    for label, src, enum_name, variant, want in SELFTEST_VARIANT_USE:
        if bool(re.search(rf"\b{enum_name}::{variant}\b", src)) == want:
            print(f"  ok    variant use: {label}")
        else:
            failures += 1
            print(f"  FAIL  variant use: {label}")
    for label, src, name, want in SELFTEST_CALL_FORM:
        if bool(re.search(rf"[.:]\s*{name}\s*\(", src)) == want:
            print(f"  ok    call form: {label}")
        else:
            failures += 1
            print(f"  FAIL  call form: {label}")
    for label, src, anchor, word, arm, want in SELFTEST_FRAME:
        got = _selftest_frame_case(src, anchor, word, arm)
        if got == want:
            print(f"  ok    frame path: {label}")
        else:
            failures += 1
            print(f"  FAIL  frame path: {label} - skips {got}, expected {want}")
    for label, kernel, src, api, want in SELFTEST_CONTENT:
        got = _selftest_content_case(kernel, src, api)
        if got == want:
            print(f"  ok    frame content: {label}")
        else:
            failures += 1
            print(f"  FAIL  frame content: {label} - reached {got}, expected {want}")
    total = (
        len(SELFTEST_WORDS)
        + len(SELFTEST_SCREENS)
        + len(SELFTEST_TRANSFORMS)
        + len(SELFTEST_CONSTANTS)
        + len(SELFTEST_SIGNATURES)
        + len(SELFTEST_SIM)
        + len(SELFTEST_PAGE_KEYS)
        + len(SELFTEST_DIAG)
        + len(SELFTEST_RENDER)
        + len(SELFTEST_OWNERSHIP)
        + len(SELFTEST_VARIANT_USE)
        + len(SELFTEST_CALL_FORM)
        + len(SELFTEST_FRAME)
        + len(SELFTEST_CONTENT)
    )
    if failures:
        print(
            f"\nself-test: {failures} of {total} case(s) failed - the surface this "
            f"gate measures is not the set of screens, so its verdict means nothing"
        )
        return 2
    print(f"\nself-test: all {total} cases pass")
    return 0


# --------------------------------------------------------------------------
# Tier 5 - no page-side keyboard table
# --------------------------------------------------------------------------
#
# The three hosts share one keyboard layout, served out of the engine by
# `pad_bindings_json` (`legaia_engine_core::input::Mapping::web_default`).
# The whole point of serving it is that a page cannot write a second one down
# - and a page that writes one down does not look like a table: it looks like
# a `switch` on `e.key`, or an object literal indexed by `e.key.toLowerCase()`.
# The minigames page carried exactly that for a long time, binding A / S / D
# to the three face buttons while the engine binds them to Left / Down /
# Right, and printing labels that said so. Nothing failed, because no gate
# asked.
#
# The rule, on the pages that drive pad input:
#
#   * `KeyboardEvent.key` may not be read at all. It is the layout-dependent
#     character property - the engine's table is keyed by `code`, so a `key`
#     comparison cannot be reconciled with a binding even in principle.
#   * `KeyboardEvent.code` may be compared to a literal only for keys the PSX
#     pad has no button for. Those cannot contradict a binding, and the pad
#     has no Escape.
#
# Scoped to pad-driving sources: a file that mentions an engine input entry
# point. `main.js` closing a dialog on Escape is ordinary web UI and is not
# in scope - the gate is about pad bindings, not about keyboards.
SITE_ROOT = REPO / "site"

# A source is in scope when it reaches the engine's pad surface at all.
PAD_HOST_MARKERS = (
    "pad_bindings_json",
    "legaiaPad",
    "legaiaAdoptPadBindings",
    ".set_pad(",
    "_menu_input(",
    "_shop_input(",
    "boot_title_step(",
    "game_over_input(",
)

# `KeyboardEvent` accesses: an event-shaped identifier, not `g.key` on a
# plain object (the play page's scene-group records have a `key` field, and
# `kbd.dataset.key` is an attribute).
EVENT_KEY_RE = re.compile(r"\b(?:e|ev|evt|event)\.key\b")
EVENT_CODE_LITERAL_RE = re.compile(
    r"\b(?:e|ev|evt|event)\.code\s*[=!]==?\s*['\"]([^'\"]*)['\"]"
)

# A page may also stash key codes in a Set and later ask `held.has('Enter')`.
# The literal never touches `event.code`, so the two detectors above are blind
# to it - and that is not hypothetical: the play page decided whether Start was
# pressed with `p.has('Enter')`, which meant binding Start to Space bound it
# everywhere except the one handler that opens the pause menu. Dispatch must go
# through the pad BUTTON, so a bindable code in a set membership test is the
# same defect wearing a different shape.
KEY_SET_LITERAL_RE = re.compile(r"\.(?:has|includes)\(\s*['\"]([^'\"]+)['\"]\s*\)")


def bindable_dom_codes() -> set[str]:
    """The `KeyboardEvent.code`s the engine's own vocabulary binds.

    Parsed from `KEY_NAME_DOM_CODES` in `crates/engine-core/src/input.rs` rather
    than restated here, so this gate cannot drift from the table it polices.
    An unreadable table yields an empty set, which disables only this detector.
    """
    src = REPO / "crates" / "engine-core" / "src" / "input.rs"
    if not src.is_file():
        return set()
    text = src.read_text(encoding="utf-8", errors="replace")
    m = re.search(r"KEY_NAME_DOM_CODES[^=]*=\s*\[(.*?)\];", text, re.DOTALL)
    if not m:
        return set()
    return {c for _, c in re.findall(r"\(\s*\"([^\"]+)\"\s*,\s*\"([^\"]+)\"\s*\)", m.group(1))}

# Keys with no PSX pad button, so a page-side comparison against one cannot
# disagree with a binding. Kept short on purpose: every addition is a key the
# engine then may not bind.
NON_PAD_CODES = {
    "Escape",
    "Backspace",
    "Tab",
    "Delete",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    *(f"F{n}" for n in range(1, 13)),
}


def pad_host_sources() -> list[Path]:
    """Every `site/` source that reaches the engine's pad surface."""
    if not SITE_ROOT.is_dir():
        return []
    out = []
    for path in sorted(SITE_ROOT.rglob("*")):
        if path.suffix not in (".js", ".html") or not path.is_file():
            continue
        if "wasm" in path.parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if any(m in text for m in PAD_HOST_MARKERS):
            out.append(path)
    return out


def check_page_key_tables() -> tuple[list[str], int]:
    """Tier 5. Returns `(problems, files_scanned)`."""
    problems: list[str] = []
    sources = pad_host_sources()
    for path in sources:
        rel = path.relative_to(REPO)
        text = strip_comments(BLOCK_COMMENT_RE.sub(" ", path.read_text(encoding="utf-8")))
        for m in EVENT_KEY_RE.finditer(text):
            line = text.count("\n", 0, m.start()) + 1
            problems.append(
                f"PAGE KEY TABLE {rel}:{line}: reads `KeyboardEvent.key`. "
                f"A pad-driving page must resolve `event.code` through the "
                f"engine's table (`pad_bindings_json` / `legaiaPadButtonOf`) "
                f"and dispatch on the pad BUTTON - `.key` is the "
                f"layout-dependent character and cannot be reconciled with a "
                f"binding."
            )
        for m in EVENT_CODE_LITERAL_RE.finditer(text):
            code = m.group(1)
            if code in NON_PAD_CODES:
                continue
            line = text.count("\n", 0, m.start()) + 1
            problems.append(
                f"PAGE KEY TABLE {rel}:{line}: compares `event.code` to "
                f"{code!r}, which the engine may bind. Resolve it through "
                f"`legaiaPadButtonOf` and dispatch on the button, or - if it "
                f"really is not a pad control - use a key the pad has no "
                f"button for (NON_PAD_CODES in this gate)."
            )
        codes = bindable_dom_codes()
        for m in KEY_SET_LITERAL_RE.finditer(text):
            code = m.group(1)
            if code not in codes:
                continue
            line = text.count("\n", 0, m.start()) + 1
            problems.append(
                f"PAGE KEY TABLE {rel}:{line}: tests set membership of "
                f"{code!r}, a key the engine binds. Dispatch on the pad BUTTON "
                f"instead - e.g. `padMaskOf(pulse) & legaiaPadButton('Start')` "
                f"- so a key bound to that button anywhere reaches this handler "
                f"too, and a rebinding follows it."
            )
    return problems, len(sources)


# --------------------------------------------------------------------------
# Paired diagnostic draw gates.
# --------------------------------------------------------------------------
#
# Every `LEGAIA_DIAG_*` env gate in the engine crates, declared by whether it
# changes what is DRAWN and, when it draws something retail does not, which
# browser-side toggle is its twin.
#
# `additive` is the field that matters. A *subtractive* gate (suppress a
# layer, blend off, draw only slots [a,b)) can only ever remove pixels, so a
# host missing it renders retail-correctly - it just cannot bisect. An
# *additive* gate paints something retail never paints, so a host missing it
# paints that thing unconditionally, in normal play, for every user.
#
# `web_toggle` names the `web-viewer` symbol carrying the twin. A WASM module
# has no process environment, so the browser twin is a module static a page or
# a devtools console flips - which is why this cannot be checked by looking for
# the env name on both sides.
DIAG_GATES: list[dict[str, object]] = [
    # --- additive: draws something retail does not -------------------------
    {
        "env": "LEGAIA_DIAG_FX",
        "additive": True,
        "web_toggle": "FX_OUTLINE",
        "note": "per-billboard wireframe outline strips + per-sprite log",
    },
    {
        "env": "LEGAIA_DIAG_HUD",
        "additive": True,
        "web_toggle": None,
        "waiver": (
            "the gate lives in the shared engine-ui leaf (`ui_overlay::"
            "diag_hud_enabled`), so it is one implementation both hosts call "
            "rather than a per-host twin. `std::env::var` answers Err under "
            "wasm32, which makes the browser default-off by construction."
        ),
        "note": "battle-event log + pose/HP diagnostic text over the frame",
    },
    # --- subtractive / logging only ----------------------------------------
    {"env": "LEGAIA_DIAG_NOFX", "additive": False, "note": "suppress the effect layer"},
    {"env": "LEGAIA_DIAG_NO_GHOSTS", "additive": False, "note": "suppress the battle after-image ghost pass (A/B attribution)"},
    {"env": "LEGAIA_DIAG_NOSEMI", "additive": False, "note": "semi-transparent blend off"},
    {"env": "LEGAIA_DIAG_LAYERS", "additive": False, "note": "draw only the named layers"},
    {"env": "LEGAIA_DIAG_PLACE_RANGE", "additive": False, "note": "draw only placements [a,b)"},
    {"env": "LEGAIA_DIAG_MESHTEX", "additive": False, "note": "mesh/texture bind log"},
    {"env": "LEGAIA_DIAG_PLACE", "additive": False, "note": "placement-resolve log"},
    {"env": "LEGAIA_DIAG_POSE", "additive": False, "note": "per-actor pose/AABB log"},
    {"env": "LEGAIA_DIAG_CAMERA", "additive": False, "note": "camera-solve log"},
    {"env": "LEGAIA_DIAG_CUTCAM", "additive": False, "note": "cutscene-camera log"},
    {"env": "LEGAIA_DIAG_BATCAM", "additive": False, "note": "battle-camera log"},
    {"env": "LEGAIA_DIAG_BATDRAW", "additive": False, "note": "battle draw-list log"},
    {"env": "LEGAIA_DIAG_TIMELINE", "additive": False, "note": "narration-timeline log"},
    {"env": "LEGAIA_DIAG_MEI", "additive": False, "note": "test-only NPC-entry log"},
]

DIAG_ENV_RE = re.compile(r'"(LEGAIA_DIAG_[A-Z0-9_]+)"')

# Roots swept for gate *declarations*. Tests are included deliberately: a gate
# introduced in a test still has to be declared, because the next wave will
# reach for it from the engine.
DIAG_ROOTS = [
    REPO / "crates" / "engine-shell",
    REPO / "crates" / "engine-render",
    REPO / "crates" / "engine-core",
    REPO / "crates" / "engine-ui",
    REPO / "crates" / "engine-vm",
    REPO / "crates" / "web-viewer",
]


def discover_diag_gates() -> set[str]:
    """Every `LEGAIA_DIAG_*` name appearing as a string literal in the crates."""
    found: set[str] = set()
    for root in DIAG_ROOTS:
        if not root.exists():
            continue
        for path in root.rglob("*.rs"):
            found.update(DIAG_ENV_RE.findall(path.read_text(encoding="utf-8", errors="replace")))
    return found


def initialiser_is_off(init: str) -> bool:
    """Does a toggle's initialiser text mean "off"?

    Split out of [`web_toggle_defaults_off`] so it can be run against synthetic
    inputs - a checker that only ever sees the one real file cannot show it
    would notice the file changing.
    """
    return bool(re.search(r"\bnew\s*\(\s*false\s*\)|^false$", init))


# Control suite for the toggle-initialiser detector. A tier that cannot tell
# `new(false)` from `new(true)` proves nothing about the hosts.
SELFTEST_DIAG: list[tuple[str, str, bool]] = [
    ("atomic off", "std::sync::atomic::AtomicBool::new(false)", True),
    ("atomic on", "std::sync::atomic::AtomicBool::new(true)", False),
    ("cell off", "Cell::new(false)", True),
    ("bare off", "false", True),
    ("bare on", "true", False),
    ("expression", "cfg!(debug_assertions)", False),
]


def web_toggle_defaults_off(symbol: str) -> tuple[bool, str]:
    """Does `symbol` exist in web-viewer and initialise to false?

    Returns `(ok, detail)`. The initialiser test is the whole point: a toggle
    that exists but defaults on is the defect this tier was written for,
    wearing the shape of the fix.
    """
    pattern = re.compile(
        r"\b" + re.escape(symbol) + r"\b[^=;]*=\s*([^;]+);",
        re.S,
    )
    for path in (REPO / "crates" / "web-viewer" / "src").rglob("*.rs"):
        text = path.read_text(encoding="utf-8", errors="replace")
        m = pattern.search(text)
        if not m:
            continue
        init = " ".join(m.group(1).split())
        if initialiser_is_off(init):
            return True, f"{path.relative_to(REPO)}: {init}"
        return False, f"{path.relative_to(REPO)}: initialiser is `{init}`, expected false"
    return False, f"no `{symbol}` found in crates/web-viewer/src"


def check_diag_gates() -> tuple[list[str], int]:
    """Tier 6: an additive diagnostic must be default-off on BOTH hosts.

    Returns `(problems, additive_count)`.
    """
    problems: list[str] = []
    declared = {str(g["env"]): g for g in DIAG_GATES}
    found = discover_diag_gates()

    for env in sorted(found - set(declared)):
        problems.append(
            f"UNDECLARED DIAG GATE {env}: add it to DIAG_GATES in "
            f"{Path(__file__).name} and say whether it is `additive` (draws "
            f"something retail does not). An additive gate needs a default-off "
            f"twin in crates/web-viewer, or the browser draws it always."
        )
    for env in sorted(set(declared) - found):
        problems.append(
            f"STALE DIAG GATE {env}: declared in DIAG_GATES but no longer "
            f"appears in any engine crate. Drop the row."
        )

    additive = 0
    for env, gate in sorted(declared.items()):
        if not gate.get("additive"):
            continue
        additive += 1
        toggle = gate.get("web_toggle")
        if toggle is None:
            if not str(gate.get("waiver", "")).strip():
                problems.append(
                    f"{env}: additive with no `web_toggle` needs a `waiver` "
                    f"saying why the browser cannot draw it."
                )
            continue
        ok, detail = web_toggle_defaults_off(str(toggle))
        if not ok:
            problems.append(
                f"DIAG DRIFT {env}: its browser twin `{toggle}` is not "
                f"default-off - {detail}. The native gate suppresses a draw "
                f"retail never makes; a browser that cannot suppress it "
                f"stamps that draw on every user's frame."
            )
    return problems, additive


# --------------------------------------------------------------------------
# Tier 7 - render kernels: same draw list, same kernel, on every surface
# --------------------------------------------------------------------------
#
# Every tier above measures a UI screen, a constant, a sim injection site, a
# trait hook or a keyboard table. None of them asks the question that has now
# shipped five separate bugs: **two surfaces assemble the same kind of draw
# list and only one of them runs the kernel that makes it correct.**
#
# The five, each invisible in a diff because no file held two of the columns:
# the play page resolved the same EnvDraws as the native shell and never
# computed the coplanar lifts; the Muscle Dome bodies hand-rolled white vertex
# streams the converter sweep could not see; `webgl-shaders.js` applied a
# synthetic Lambert on both its paths; the ground heightfield was left out of
# the coplanar soup; an occlusion-fade radius was staged at a different value
# on the browser than the gate it fed (that last one is tier 2's).
#
# What makes this tier different from `SIM_PAIRS` is the denominator. A
# `SIM_PAIRS` row names TWO function bodies by hand, so a THIRD surface that
# grows the same draw list is outside the measurement by construction - and
# there are five render surfaces in this tree, not two: the native window, the
# browser play page, the browser field-scene viewer, and the two minigame
# venue bakers (dance hall, fishing venue), each of which resolves `EnvDraw`s
# and instances env-pack meshes exactly like the other three.
#
# So the surface here is **derived**: every non-test source under the render
# roots. A rule states an implication over it - "a file that does X must also
# do Y" - or a prohibition - "a file that does X may not contain Z". A new
# surface joins the measurement by existing.
#
# Two rule kinds:
#
#   requires  a file whose comment-stripped source matches `trigger` must also
#             match every pattern in `requires`.
#   forbids   a file matching `trigger` may not match `forbids` inside any
#             3-line window (the statement scale - these kernels are written
#             as `self.flat\n    .extend(...)` as often as on one line).
#
# Comments are stripped first, in both languages, for the same reason tier 1
# strips them: a doc comment naming `coplanar_draw_offsets` is prose, not a
# wiring, and the conservative direction is to under-count "satisfied".
#
# `blocked_on` marks a known divergence being closed elsewhere and is
# validated in both directions exactly like a waiver: a `blocked_on` path that
# has gone clean FAILS, demanding the entry be deleted. `exempt` is the
# stronger claim - the rule does not apply to that file at all - and needs a
# reason about the DATA, not about the schedule.

RENDER_ROOTS = [
    REPO / "crates" / "engine-shell" / "src" / "bin",
    REPO / "crates" / "engine-render" / "src",
    REPO / "crates" / "web-viewer" / "src",
    REPO / "site" / "js",
]

RENDER_KERNEL_RULES: list[dict[str, object]] = [
    {
        "kernel": "cross-draw coplanar lifts",
        "why": "a surface that resolves EnvDraws and does not rank their "
        "coplanar clusters z-fights on every placement/terrain pair that "
        "meets on one world plane - view-angle-dependently, so it survives a "
        "diff and any single screenshot taken from the lucky angle",
        "trigger": r"\bresolve_(?:placed_)?env_draws\b",
        "requires": [r"\bdraw_plane_summaries\b", r"\bcoplanar_draw_offsets\b"],
    },
    {
        "kernel": "walk-ground heightfield sink",
        "why": "the generated ground grid shares its plane with the env "
        "pack's authored floor art (koin6: both at y=0, different "
        "tessellations), so a render site that emits the heightfield's "
        "vertices without GROUND_SINK draws wedge streaks along the grid's "
        "cell diagonals while every other host is clean",
        # The EMITTER, not every file that names the type: a log line reading
        # `hf.positions.len()` is not a render site, and an early draft that
        # triggered on the type name reported four files that only pass it on.
        "trigger": r"for\s+\w+\s+in\s+&(?:mut\s+)?hf\.positions\b|\bhf\.positions\.clone\(\)",
        "requires": [r"\bGROUND_SINK\b"],
    },
    {
        "kernel": "packet-colour stream fill",
        "why": "the shader reads `a_flat_rgba` as `texel * rgb * 255/128`, so "
        "a fabricated stream of white is `texel * 2` - the mesh reads as "
        "over-lit rather than as having lost its colour word, and the "
        "accessor tests pass because length parity is not coverage. The one "
        "legal fill for geometry with no packet colour is the neutral "
        "constant (`packet_color::NEUTRAL` / `MODULATION_NEUTRAL` = 0x80)",
        "trigger": r"\bflat_rgba\b|\bpacket_color\b",
        "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255|0x[fF][fF]u8|0x[fF][fF])\s*[;,]",
    },
    {
        "kernel": "placement tilt composition (Rx*Ry*Rz)",
        "why": "a placement record carries three authored angles "
        "(`+0x08` / `+0x0A` / `+0x0C`) and retail composes all three "
        "(`FUN_80026988`). A surface that reads only the yaw draws every "
        "tilted object upright: measured over 49 field scenes, 94 of 1667 "
        "placements tilt, and juui1 tilts all nine of its by a quarter turn "
        "about X",
        "trigger": r"\w*placement_rot_y\b",
        "requires": [r"\w*placement_rot_x\b", r"\w*placement_rot_z\b"],
        "exempt": {
            "crates/web-viewer/src/scene_geom.rs":
                "world-map WALK placements, whose records carry rot_x = "
                "rot_z = 0 across the retail corpus (see the "
                "`legaia_asset::field_objects::Placement::rot_x` doc) - the "
                "yaw-only path is not a shortcut there, it is the data",
            "site/js/world-overview-app.js":
                "consumer of the same walk-placement accessors; see above",
        },
    },
    {
        "kernel": "shared value layout -> shared quad emitter",
        "why": "a shared LAYOUT is not a shared DRAW. The battle damage "
        "numerals and the `N HIT` / `TOTAL` counter resolved through one "
        "kernel (`engine-vm::battle_value_readout`) while the native window "
        "sampled retail's 24x24 cells out of VRAM and the browser restyled "
        "the same digits in the dialog font - one model, two letterforms, "
        "and every tier green because both hosts named the kernel. A surface "
        "that resolves the layout must emit through the shared quad builder "
        "(`engine-ui::battle_numerals`), leaving a font path as an explicit "
        "before-the-atlas fallback rather than as the draw",
        "trigger": r"\bbattle_value_readout\b",
        "requires": [
            r"\bbattle_numerals\b",
            r"\b(?:digit_run_prims|combo_cluster_prims|digit_quad|readout_quad)\b",
        ],
    },
    {
        "kernel": "world-map markers through the shared quad kernel",
        "why": "the overworld entity / player markers were native-only for as "
        "long as the native window drew them as world-space lines through a "
        "pipeline the browser has no counterpart for. They are now one-pixel "
        "quads out of `engine-core::world_map_markers::marker_quads`, wrapped "
        "by `screen_prim::world_map_marker_prim` onto the screen-prim pass "
        "both hosts run; a surface that reads the marker seams must go "
        "through that pair, or the marker set is two implementations again",
        "trigger": r"\bworld_map_(?:entity_markers|player_marker)\b|\bmarker_quads\b",
        "requires": [r"\bmarker_quads\b", r"\bworld_map_marker_prim\b"],
    },
    {
        "kernel": "field attached lights through the shared prim kernel",
        "why": "the op `0x34` sub-1 light pools (`World::field_light_draws`, "
        "the `FUN_801E3984` port) are untextured semi-transparent gouraud "
        "polygons at an additive or subtractive ABR mode; a surface that "
        "reads them and builds its own quads picks its own blend, vertex "
        "order and triangle split, which is exactly the per-host decision "
        "`screen_prim::light_pool_prims` exists to take once",
        "trigger": r"\bfield_light_draws\b",
        "requires": [r"\blight_pool_prims\b"],
    },
    {
        "kernel": "retained field ground pass gated off in battle",
        "why": "the WebGL renderer draws its field ground heightfield "
        "(`uploadGround`) as a RETAINED pass inside `renderAssembled`, ahead "
        "of whatever the frame's draw list holds - so a surface that uploads "
        "a field ground and also draws a battle frame through the same "
        "renderer paints the field terrain through the battle camera, "
        "sampling the battle VRAM, unless it turns the pass off. The play "
        "page did exactly that: town01's ground cells showed up as flat "
        "yellow strips and stray grass/gravel patches around the monster in "
        "every forced battle, while the native window only draws its "
        "heightfield in the non-battle branch",
        # Both halves in one file: the uploader of a field ground AND a
        # battle frame driver. An `\A`-anchored lookahead keeps it one
        # `trigger` and one scan (unanchored, it re-scans per position).
        "trigger": r"(?s)\A(?=.*\buploadGround\s*\().*\bplay_battle_active\b",
        "requires": [r"\bsetGroundEnable\s*\(\s*false\s*\)"],
    },
    # RETIRED: "screen-space fade quad" (resolving `intro_fade(...)` requires
    # `fade_prim`). The whole transition emission - fade included - became
    # single-assembler when the `battle_intro` emitter moved to `engine-ui`
    # and both hosts started ticking it: no render surface resolves the ramp
    # any more, so the rule matched nothing and was deleted rather than left
    # standing vacuous. See host-drift.md "The version of this tier that
    # needs no rule".
]

BLOCK_COMMENT_ANY_RE = re.compile(r"/\*.*?\*/", re.DOTALL)


def render_sources() -> list[Path]:
    """Every non-test render-surface source, `.rs` and `.js` alike."""
    out: list[Path] = []
    for root in RENDER_ROOTS:
        if not root.exists():
            continue
        for ext in ("*.rs", "*.js"):
            out.extend(p for p in root.rglob(ext) if not is_test_source(p))
    return sorted(out)


def strip_all_comments(text: str) -> str:
    """Drop `/* */` and `//` comments - the same conservative direction as
    tier 1, applied to both languages this tier scans."""
    return LINE_COMMENT_RE.sub("", BLOCK_COMMENT_ANY_RE.sub("", text))


def rule_findings(rule: dict, text: str) -> list[str]:
    """Findings for one rule against one file's comment-stripped source.

    Empty when the file does not trigger, or triggers and complies. The
    `forbids` kind reports one entry per offending statement window so the
    output names the line, not just the file.
    """
    trigger = str(rule["trigger"])
    if not re.search(trigger, text):
        return []
    out: list[str] = []
    for pat in rule.get("requires", []):  # type: ignore[union-attr]
        if not re.search(str(pat), text):
            out.append(f"does not reach `{pat}`")
    forbids = rule.get("forbids")
    if forbids:
        lines = text.splitlines()
        for i in range(len(lines)):
            window = "\n".join(lines[max(0, i - 2) : i + 1])
            if re.search(str(forbids), window):
                out.append(f"line {i + 1}: {lines[i].strip()[:80]}")
    return out


def check_render_kernels() -> tuple[list[str], list[str], list[tuple[str, int, int, int]]]:
    """Tier 7. Returns (problems, disclosed-blocked notes, per-rule counts).

    A count row is `(kernel, running, blocked, exempt)`: how many surfaces
    assembling that draw list run the kernel, how many are disclosed as a
    divergence still being closed, and how many the rule provably does not
    apply to. The three are printed separately because collapsing them is how
    a matrix reads clean while a surface is missing.
    """
    problems: list[str] = []
    pending: list[str] = []
    counts: list[tuple[str, int, int, int]] = []
    sources = render_sources()
    texts = {p: strip_all_comments(p.read_text(encoding="utf-8", errors="ignore")) for p in sources}
    for rule in RENDER_KERNEL_RULES:
        kernel = str(rule["kernel"])
        blocked: dict = rule.get("blocked_on", {})  # type: ignore[assignment]
        exempt: dict = rule.get("exempt", {})  # type: ignore[assignment]
        clean = 0
        seen_blocked: set[str] = set()
        seen_exempt: set[str] = set()
        for path, text in texts.items():
            rel = path.relative_to(REPO).as_posix()
            if not re.search(str(rule["trigger"]), text):
                continue
            findings = rule_findings(rule, text)
            if rel in exempt:
                seen_exempt.add(rel)
                if not findings:
                    problems.append(
                        f"STALE EXEMPT {kernel} / {rel}: the file now satisfies "
                        f"the rule, so the exemption claims nothing. Drop it."
                    )
                continue
            if not findings:
                clean += 1
                if rel in blocked:
                    seen_blocked.add(rel)
                    problems.append(
                        f"STALE BLOCKED {kernel} / {rel}: the divergence is "
                        f"closed. Drop the `blocked_on` entry."
                    )
                continue
            detail = "; ".join(findings)
            if rel in blocked:
                seen_blocked.add(rel)
                pending.append(f"{kernel} / {rel}: {blocked[rel]}")
                continue
            problems.append(
                f"RENDER KERNEL {kernel}: {rel} assembles this draw list but "
                f"{detail}. {rule['why']}."
            )
        for rel in blocked:
            if rel not in seen_blocked:
                problems.append(
                    f"STALE BLOCKED {kernel} / {rel}: the file no longer "
                    f"assembles this draw list (renamed or deleted?). Drop the "
                    f"`blocked_on` entry."
                )
        for rel in exempt:
            if rel not in seen_exempt:
                problems.append(
                    f"STALE EXEMPT {kernel} / {rel}: the file no longer "
                    f"assembles this draw list. Drop the exemption."
                )
        counts.append((kernel, clean, len(seen_blocked), len(seen_exempt)))
    return problems, pending, counts


# Positive control. A rule engine that matched nothing would report every
# surface clean, which is the failure mode this whole file exists to refuse -
# so each case pins one direction of one detector against a synthetic source.
SELFTEST_RENDER: list[tuple[str, dict, str, bool]] = [
    (
        "requires (lookahead trigger): battle surface that never gates the ground pass",
        {"trigger": r"(?s)\A(?=.*\buploadGround\s*\().*\bplay_battle_active\b",
         "requires": [r"\bsetGroundEnable\s*\(\s*false\s*\)"]},
        "this.renderer.uploadGround(p, u, c, i);\nif (rt.play_battle_active()) draw();",
        True,
    ),
    (
        "requires (lookahead trigger): battle surface that gates it, either order",
        {"trigger": r"(?s)\A(?=.*\buploadGround\s*\().*\bplay_battle_active\b",
         "requires": [r"\bsetGroundEnable\s*\(\s*false\s*\)"]},
        "if (rt.play_battle_active()) this.renderer.setGroundEnable(false);\nthis.renderer.uploadGround(p, u, c, i);",
        False,
    ),
    (
        "requires: triggering file that reaches the kernel",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "let (t, _) = resolve_env_draws(&e, &r, lut);\nlet o = coplanar_draw_offsets(&t, &p);",
        False,
    ),
    (
        "requires: triggering file that does NOT reach the kernel",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "let (t, _) = resolve_env_draws(&e, &r, lut);\nout.append(t);",
        True,
    ),
    (
        "requires: non-triggering file is not a finding",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "fn draw_hud() { let x = 1; }",
        False,
    ),
    (
        "requires: the kernel named only in a comment does not count",
        {"trigger": r"\bresolve_env_draws\b", "requires": [r"\bcoplanar_draw_offsets\b"]},
        "// runs coplanar_draw_offsets later\nlet (t, _) = resolve_env_draws(&e, &r, lut);",
        True,
    ),
    (
        "forbids: multi-line white fill of a packet-colour stream",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "use crate::packet_color;\nself.flat\n    .extend(std::iter::repeat_n([255u8; 4], n).flatten());",
        True,
    ),
    (
        "forbids: single-line white fill of a packet-colour stream",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "use crate::packet_color;\nlet flat = vec![255u8; n * 4];",
        True,
    ),
    (
        "forbids: a resolved stream is not a finding",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "let flat = crate::packet_color::hybrid(&mesh, &shading);",
        False,
    ),
    (
        "forbids: the textured FLAG byte 255 is not a white fill",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "// packet_color\nlet mut flat = Vec::new();\nflat.extend_from_slice(&[c[0], c[1], c[2], 255]);",
        False,
    ),
    (
        "forbids: a white literal far from any packet-colour stream",
        {"trigger": r"\bpacket_color\b", "forbids": r"\bflat\b[\s\S]{0,140}?\[\s*(?:255u8|255)\s*[;,]"},
        "use crate::packet_color;\nlet flat = pc(&m);\nlet a = 1;\nlet b = 2;\n"
        "let c = 3;\nlet tint = [255u8, 0, 0, 255];",
        False,
    ),
]


def _selftest_render_case(rule: dict, src: str) -> bool:
    return bool(rule_findings(rule, strip_all_comments(src)))


# --------------------------------------------------------------------------
# Tier 8 - ownership: does each host OWN the engine type, or only read it?
# --------------------------------------------------------------------------
#
# Every tier above asks whether a host *reaches* something - a builder, a
# constant, a kernel, a gate. None of them can see the shape where a host
# reaches an engine type's outputs without ever holding the type.
#
# That shipped, and it produced four different-looking symptoms at once. The
# browser play page framed the field with a spherical orbit of its own while
# the native window consumed `engine_core::camera::Camera`; read as "two
# projections" it is a rendering difference, and it was not. The page held no
# `Camera`, so nothing on that host routed the op-`0x45` Configure beats into
# a controller, advanced the mover, wrote the follow focus back into the
# retail camera globals or reset them on scene entry - and the azimuth it fed
# the locomotion compass came from its own orbit yaw. One absence, a
# projection difference, a simulation difference and two missing screens.
#
# Tier 1 cannot see it (no builder is missing), tier 2 cannot (no constant is
# paired), tier 3 cannot (an injection site that does not exist cannot
# diverge from one that does), tier 7 cannot (both surfaces name their own
# kernels). What distinguishes the state is **ownership**: a field declaration
# or a construction, in that host's own shipped sources.
#
# Declared rather than derived, for the same reason [`CONSTANT_PAIRS`] and
# [`SIM_PAIRS`] are: "which engine types must a host own" is a judgement about
# the architecture. The derived version of the question (every `engine-core`
# type one host constructs and the other only names) reports 26 rows over this
# tree, and every one is a type BOTH hosts use where one names a constructor
# (`PadButton::from_name`) or holds a session the other reaches through its
# runtime - noise, not a Camera-shaped absence. A declared row is a pinned
# juncture; it does not claim to be a census.
#
# Scope, as narrowly as the tiers above:
#
# * it DOES prove each named type is constructed or held as a field by both
#   hosts' shipped code, and that the type still exists in `engine-core`;
# * it does NOT prove the two hosts drive it the same way, tick it at all, or
#   read the same outputs off it.

OWNED_TYPES: list[dict[str, object]] = [
    {
        "type": "Camera",
        "defined_in": "crates/engine-core/src/camera.rs",
        "why": "which camera owns a frame and what its retail GTE inputs are "
        "is one engine question; a host that renders a field without holding "
        "the type re-implements the mover, the compass azimuth and the "
        "op-0x45 cutscene beats in its own projection, and every other tier "
        "stays green while it does",
    },
    {
        "type": "SceneHost",
        "defined_in": "crates/engine-core/src/scene/host.rs",
        "why": "the scene host is what drains the per-frame queues the field "
        "VM fills - the minigame door warp, the staged menu warp, the BGM "
        "route. A host that keeps a bare `World` instead reaches the same "
        "world state and none of the drains",
    },
    {
        "type": "MenuRuntime",
        "defined_in": "crates/engine-core/src/menu_runtime.rs",
        "why": "the save/load menu's disk-facing runtime. A host that opens "
        "the pause menu without one has the screens and no slot state behind "
        "them",
    },
]

# Ownership evidence, in the host's own comment-stripped source: a field
# declaration whose type mentions the name, or a construction of it. A bare
# mention is deliberately NOT ownership - `use` lines, match arms and doc
# links are exactly what the page had for `Camera` while owning nothing.
def owns_type(text: str, name: str) -> bool:
    field = re.search(
        # A field DECLARATION on its own line, and never through a `&`: a
        # borrowed parameter is the shape this must not accept, and a
        # multi-line signature puts one on its own line too.
        rf"^\s*(?:pub(?:\([^)]*\))?\s+)?[a-z_][a-z_0-9]*\s*:\s*[^,;=()&]*\b{name}\b",
        text,
        re.MULTILINE,
    )
    if field:
        return True
    if re.search(rf"\b{name}\s*::\s*(?:new|default|from_\w+|with_\w+)\s*\(", text):
        return True
    # A struct literal is a construction - but `-> Camera {` is a return type
    # and `impl Trait for Camera {` is an impl block, and both put the same
    # two tokens next to each other. The control suite pins all three.
    for hit in re.finditer(rf"\b{name}\s*\{{", text):
        prefix = text[max(0, hit.start() - 40):hit.start()]
        if re.search(r"->\s*$", prefix) or re.search(r"\b(?:impl|struct|enum|for)\b[^\n]*$", prefix):
            continue
        return True
    return False


def host_shipped_sources() -> dict[str, list[tuple[str, str]]]:
    """Per host label, every shipped (non-test) source as `(rel path, text)`."""
    out: dict[str, list[tuple[str, str]]] = {}
    for host, roots in HOSTS.items():
        rows: list[tuple[str, str]] = []
        for root in roots:
            if not root.is_dir():
                continue
            for path in sorted(root.rglob("*.rs")):
                if is_test_source(path):
                    continue
                rel = path.relative_to(REPO).as_posix()
                rows.append((rel, strip_comments(path.read_text(encoding="utf-8"))))
        out[host] = rows
    return out


def check_owned_types(shipped: dict[str, list[tuple[str, str]]]) -> tuple[list[str], list[str]]:
    """Every [`OWNED_TYPES`] row: both hosts must own the type.

    Returns `(problems, pending)`; a row carrying `blocked_on` reports instead
    of failing, and a `blocked_on` row that has gone clean fails.
    """
    problems: list[str] = []
    pending: list[str] = []
    for row in OWNED_TYPES:
        name = str(row["type"])
        defined_in = REPO / str(row["defined_in"])
        if not defined_in.is_file() or not re.search(
            rf"pub (?:struct|enum) {name}\b", defined_in.read_text(encoding="utf-8")
        ):
            problems.append(
                f"STALE OWNED TYPE {name}: {row['defined_in']} no longer defines it "
                f"(renamed or moved?). Re-point or drop the row."
            )
            continue
        missing: list[str] = []
        for host, rows in shipped.items():
            if not any(owns_type(text, name) for _rel, text in rows):
                missing.append(host)
        blocked = row.get("blocked_on")
        if missing and not blocked:
            problems.append(
                f"UNOWNED TYPE {name}: {', '.join(missing)} reaches this type's "
                f"outputs without holding one ({row['why']}). Give that host the "
                f"type, or record why not with `blocked_on`."
            )
        elif missing:
            pending.append(f"{name}: not owned by {', '.join(missing)} - {blocked}")
        elif blocked:
            problems.append(
                f"STALE blocked_on ({name}): both hosts own the type now, so the "
                f"marker describes a gap that is closed. Drop `blocked_on`."
            )
    return problems, pending


# --------------------------------------------------------------------------
# Tier 9 - enum coverage: does each host answer every variant of a shared enum?
# --------------------------------------------------------------------------
#
# The mode word is the case that motivated this. A `SceneMode` variant can be
# entered on both hosts - the shared scene host installs it from the scene's
# own door warp - and drawn on one, because the presentation of four of them
# is a text panel and a hand-rolled 3D scene rather than an `engine-ui`
# builder tier 1 enumerates. The browser landed in each with a frozen field
# and no screen, with every tier green.
#
# The variant list is **derived** from the enum's own source, so a variant
# added tomorrow joins the measurement by existing - which is the property
# tier 3's hand-named site pairs cannot have. What is declared is the enum and
# the per-variant waivers.
#
# A host "answers" a variant when its shipped sources name it *qualified*
# (`SceneMode::Fishing`). The qualified form is the point: an unqualified
# `Fishing` matches a session type, a module name and half the fishing HUD,
# and a rule that accepted it would report every host as covering everything.
#
# Scope:
#
# * it DOES prove each host's shipped code mentions every variant of the named
#   enum by its qualified name, and that no waiver names a variant that is
#   gone or now covered;
# * it does NOT prove the host's arm draws anything, that the two arms agree,
#   or that the variant is reachable at runtime.

ENUM_COVERAGE: list[dict[str, object]] = [
    {
        "enum": "SceneMode",
        "source": "crates/engine-core/src/world/types.rs",
        "why": "a mode both hosts can ENTER (the shared scene host drains the "
        "mode-24 door warp for either) and only one can DRAW leaves the other "
        "host's player in a frozen field with no screen - the shape four "
        "minigame modes shipped in",
        "waivers": {
            # (variant, host) -> reason
            ("Fishing", "web"): {
                "blocked_on": "the play page's minigame screen dispatch "
                "(`ActiveGame::of_mode`, play_minigames.rs) maps the four "
                "door-warp games and returns None for Fishing; the page's "
                "fishing host keys on the installed PondSession instead, "
                "so the mode word reaches no page-side arm",
            },
        },
    },
]


def enum_variants(rel: str, name: str) -> list[str] | None:
    """Top-level variant names of `pub enum <name>` in `rel`, or None."""
    path = REPO / rel
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8")
    m = re.search(rf"pub enum {name}\s*\{{", text)
    if not m:
        return None
    depth, i, body = 1, m.end(), []
    while i < len(text) and depth:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                break
        body.append(c)
        i += 1
    src = strip_comments(BLOCK_COMMENT_RE.sub("", "".join(body)))
    out: list[str] = []
    nest = 0
    for line in src.splitlines():
        stripped = line.strip()
        if nest == 0:
            hit = re.match(r"^([A-Z][A-Za-z0-9]*)\s*(?:\{|\(|,|=|$)", stripped)
            if hit:
                out.append(hit.group(1))
        nest += line.count("{") + line.count("(") - line.count("}") - line.count(")")
    return out


def check_enum_coverage(
    shipped: dict[str, list[tuple[str, str]]],
) -> tuple[list[str], list[str], int]:
    """Every [`ENUM_COVERAGE`] row against both hosts. `(problems, pending, n)`."""
    problems: list[str] = []
    pending: list[str] = []
    checked = 0
    for row in ENUM_COVERAGE:
        name = str(row["enum"])
        variants = enum_variants(str(row["source"]), name)
        if not variants:
            problems.append(
                f"STALE ENUM ROW {name}: {row['source']} defines no such enum "
                f"(renamed or moved?). Re-point or drop the row."
            )
            continue
        waivers: dict = row.get("waivers", {})  # type: ignore[assignment]
        seen: set[tuple[str, str]] = set()
        for host, rows in shipped.items():
            blob = "\n".join(text for _rel, text in rows)
            for variant in variants:
                checked += 1
                if re.search(rf"\b{name}::{variant}\b", blob):
                    if (variant, host) in waivers:
                        problems.append(
                            f"STALE ENUM WAIVER {name}::{variant} ({host}): the host "
                            f"names the variant now. Drop the waiver."
                        )
                        seen.add((variant, host))
                    continue
                entry = waivers.get((variant, host))
                if entry is None:
                    problems.append(
                        f"UNANSWERED VARIANT {name}::{variant} ({host}): no shipped "
                        f"source on this host names it ({row['why']}). Answer it, or "
                        f"waive the pair with a reason."
                    )
                    continue
                seen.add((variant, host))
                pending.append(
                    f"{name}::{variant} ({host}) - {entry.get('blocked_on') or entry.get('reason')}"
                )
        for key in waivers:
            if key not in seen:
                problems.append(
                    f"STALE ENUM WAIVER {name}::{key[0]} ({key[1]}): no such variant "
                    f"or host. Drop the waiver."
                )
    return problems, pending, checked


# --------------------------------------------------------------------------
# Tier 10 - entry symmetry: is a World phase armed only from a debug hotkey?
# --------------------------------------------------------------------------
#
# The dance minigame's pre-song count-in and the Disco King how-to actor were
# both complete shared kernels, and the native window drove them - from a
# count-in driver of its own, reached only by the `K` / `U` hotkeys. The
# player-reachable entry is the mode-24 door warp, which the shared scene host
# drains into `World::enter_dance`, so NEITHER host counted in when a player
# walked into the hall.
#
# A gap that reads as "one host has it" can turn out to be "one host's debug
# path has it", and no tier above can tell those apart, because both end at a
# live call site. This one asks a different question: for every `World`
# method the native window's keyboard arms call, is there a call site anywhere
# outside the native `bin/` tree - the shared engine, or the browser hosts?
# If not, the phase exists in the engine and only a key press reaches it.
#
# Derived over the hotkey sources, so a new hotkey arm joins the measurement
# by existing. A debug-only spawner is a legitimate answer and takes a waiver;
# what the waiver may not say is "not wired yet".
#
# Scope:
#
# * it DOES prove every `world.<method>()` the hotkey arms call has (or does
#   not have) a call site outside `crates/engine-shell/src/bin/`;
# * it does NOT prove that site is player-reachable, that it passes the same
#   arguments, or that a phase with no hotkey arm at all is wired.

# The native window's key-driven arms. `--key-script` delivers scripted keys
# through these same handlers, which is why they are the tier's denominator.
HOTKEY_SOURCES = [
    "crates/engine-shell/src/bin/legaia-engine/window/event_handler/keyboard.rs",
    "crates/engine-shell/src/bin/legaia-engine/window/minigames.rs",
]

# Where a shared (i.e. not hotkey-only) caller may live: the engine crates the
# browser hosts share, and the browser hosts themselves.
SHARED_CALLER_ROOTS = [
    REPO / "crates" / "engine-core" / "src",
    REPO / "crates" / "engine-vm" / "src",
    REPO / "crates" / "engine-ui" / "src",
    REPO / "crates" / "web-viewer" / "src",
]

# A hotkey-only `World` method, with the reason it is one. Each entry is
# validated in both directions: a name that is no longer hotkey-called, or has
# gained a shared caller, fails as stale.
HOTKEY_ONLY_WAIVERS: dict[str, str] = {
    "spawn_debug_effect": "an effect-pool probe with no retail counterpart - "
    "it seats a marker billboard at the player so the pool's ageing can be "
    "watched. Retail spawns effects from the move VM; nothing about this arm "
    "belongs on a player-reachable entry, on either host.",
    "spawn_debug_effect_model": "the model-emitting twin of the probe above, "
    "same reasoning: it exists to prove the pool emits a model rather than a "
    "billboard for a chosen TMD index.",
    "active_field_fx_render_nodes": "a read accessor the hotkey uses for its "
    "own on-screen readout of what the pool currently holds; it arms no phase "
    "and changes no world state.",
    "spawn_field_stager": "superseded, not unwired: the production op-0x34 "
    "sub-3 route is `World::spawn_ambient_record_at` (the full `FUN_80021B04` "
    "port with the op-0x25 fan-out, the CLUT-cell integrator and the VDF "
    "morph envelope), reached from `vm_hosts::effect_anim_trigger` on both "
    "hosts. This entry stages the same record id into the older "
    "`SummonScene` pool and is kept as the native window's probe over it.",
    "spawn_summon": "superseded, not unwired: the production producer of "
    "`World::casting.active_summon` is the battle cast band "
    "(`world/battle/cast_band.rs`, `SummonScene::spawn_parts`), and the "
    "faithful player-summon render is the battle-actor TRS-keyframe path in "
    "`legaia_engine_vm::anim_vm`. This entry parses a chosen stager overlay "
    "instead and exists for the native window's summon probe.",
}

# The other half: a phase that SHOULD have a shared entry and does not yet.
# Validated exactly as `blocked_on` is elsewhere - an entry that goes clean
# FAILS, demanding the marker be deleted - so a pending wire cannot rot into a
# permanent exemption. What an entry may not say is "not wired yet": it names
# the shared caller that is missing.
HOTKEY_ONLY_BLOCKED: dict[str, str] = {
    # Empty today. Two rows were drafted here and moved to the waivers above
    # once their production route was read: both entries are superseded
    # stand-ins with a live replacement, not phases awaiting a wire. That is
    # the distinction this dict exists to keep - "the shared caller is
    # missing" is a different claim from "the shared caller is elsewhere",
    # and only the first one is pending work.
}


def check_entry_symmetry() -> tuple[list[str], list[str], int]:
    """Tier 10 over the hotkey sources. `(problems, pending, methods_scanned)`."""
    problems: list[str] = []
    pending: list[str] = []
    hot_parts: list[str] = []
    for rel in HOTKEY_SOURCES:
        path = REPO / rel
        if not path.is_file():
            problems.append(
                f"STALE HOTKEY SOURCE {rel}: no such file (moved?). Re-point the row."
            )
            continue
        hot_parts.append(strip_comments(BLOCK_COMMENT_RE.sub("", path.read_text(encoding="utf-8"))))
    hot = "\n".join(hot_parts)
    methods = sorted(set(re.findall(r"\bworld\s*\.\s*([a-z_][a-z_0-9]*)\s*\(", hot)))

    shared_parts: list[str] = []
    for root in SHARED_CALLER_ROOTS:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            shared_parts.append(
                strip_comments(BLOCK_COMMENT_RE.sub("", path.read_text(encoding="utf-8")))
            )
    shared = "\n".join(shared_parts)

    hotkey_only: list[str] = []
    for name in methods:
        # A CALL, not a definition: `.name(` or `::name(`. Matching a bare
        # `name(` would count the method's own `pub fn` as its caller, which
        # makes every row satisfied and the tier vacuous.
        if not re.search(rf"[.:]\s*{name}\s*\(", shared):
            hotkey_only.append(name)

    declared = {**HOTKEY_ONLY_WAIVERS, **HOTKEY_ONLY_BLOCKED}
    for name in hotkey_only:
        reason = declared.get(name)
        if reason is None:
            problems.append(
                f"HOTKEY-ONLY PHASE world.{name}(): the native window's key arms are "
                f"its only caller outside tests - no shared-engine or browser-host "
                f"call site exists, so a player reaches this on neither host. Arm it "
                f"where the entry is shared, or waive it in "
                f"`HOTKEY_ONLY_WAIVERS` with a reason about the ARM."
            )
        elif name in HOTKEY_ONLY_BLOCKED:
            pending.append(f"BLOCKED world.{name}() - {reason}")
        else:
            pending.append(f"waived world.{name}() - {reason}")
    for name, reason in sorted(declared.items()):
        if name not in methods:
            problems.append(
                f"STALE HOTKEY WAIVER world.{name}(): the hotkey arms no longer call "
                f"it. Drop the waiver."
            )
        elif name not in hotkey_only:
            problems.append(
                f"STALE HOTKEY WAIVER world.{name}(): a shared caller exists now, so "
                f"this is not a hotkey-only phase. Drop the waiver."
            )
        if not reason.strip():
            problems.append(f"HOTKEY WAIVER world.{name}(): needs a non-empty reason.")
    return problems, pending, len(methods)


# Control suites for the three tiers above. Each is `(label, ..., want)`, and
# each runs on every invocation for the reason the suites above it do: a
# detector that matches nothing reports every surface clean, which is exactly
# the silence these tiers exist to break.
SELFTEST_OWNERSHIP: list[tuple[str, str, str, bool]] = [
    ("a struct field is ownership",
     "struct App {\n    camera: Camera,\n    n: u8,\n}", "Camera", True),
    ("an Option field is ownership",
     "pub(crate) struct R {\n    pub scene_host: Option<SceneHost>,\n}", "SceneHost", True),
    ("a borrowed parameter on its own line is not ownership",
     "fn frame(\n    cam: &Camera,\n) -> u8 {\n    0\n}", "Camera", False),
    ("a construction is ownership", "let c = Camera::new();", "Camera", True),
    ("a `::default()` construction is ownership",
     "Self { camera: Camera::default() }", "Camera", True),
    ("a `use` line is not ownership", "use legaia_engine_core::camera::Camera;", "Camera", False),
    ("a match arm is not ownership", "match m { Camera => 1, _ => 0 }", "Camera", False),
    ("a by-reference parameter is not ownership",
     "fn frame(cam: &Camera) -> u8 { 0 }", "Camera", False),
    ("a return type is not ownership", "fn make() -> Camera { todo!() }", "Camera", False),
]

SELFTEST_VARIANT_USE: list[tuple[str, str, str, str, bool]] = [
    ("a qualified match arm answers the variant",
     "match m { SceneMode::Fishing => hud(), _ => () }", "SceneMode", "Fishing", True),
    ("an unqualified mention does not",
     "let s: FishingSession = fishing_session();", "SceneMode", "Fishing", False),
    ("a different enum's variant does not",
     "MinigameSubId::Fishing => 972,", "SceneMode", "Fishing", False),
]

SELFTEST_CALL_FORM: list[tuple[str, str, str, bool]] = [
    ("a method call is a caller", "host.world.enter_dance(g);", "enter_dance", True),
    ("a path call is a caller", "World::enter_dance(&mut w, g);", "enter_dance", True),
    ("the definition is not a caller", "pub fn enter_dance(&mut self, g: DanceGame) {", "enter_dance", False),
    ("a same-named free fn is not a method caller", "enter_dance(g);", "enter_dance", False),
]


# --------------------------------------------------------------------------
# Tier 11 - frame-path completeness: does an early exit skip a kernel the
# frame still draws?
# --------------------------------------------------------------------------
#
# Every tier above measures what a host *has*: a builder it reaches, a
# constant it declares, a kernel it names, a type it owns, a variant it
# answers, an entry it arms. This one measures what a host *runs this frame*,
# and the difference is a whole class of defect none of them can see.
#
# The shape. Both hosts drive the engine through one frame path - the native
# window's redraw tick loop, the browser runtime's `tick_frame` - and both
# paths short-circuit. The native loop `continue`s out of several arms (the
# boot UI owns the frame, the name-entry overlay is modal, a prologue
# hand-off swapped scenes, the narration crawl owns the pad, a Start edge
# just opened the pause menu); the browser `return`s out of its own. The
# *draw* does not short-circuit with them: the native draw passes run after
# the loop whatever an iteration did, and `tick_frame` returns to a page that
# draws either way. So an arm that skips a per-frame kernel does not skip the
# draw that reads that kernel's answer - the host paints last frame's
# decision, for as long as the arm is taken.
#
# That is not hypothetical. The native field party HUD's decision kernel
# (`FieldPartyHud::tick`, retail `FUN_801D0D38`) is stepped once per tick in
# the fall-through path, and its suppression predicate names the boot UI
# explicitly - but the boot-UI arm `continue`d before the step, so the kernel
# never saw the state it was written to suppress on and kept its last
# pre-menu `Draw`. The party readout stayed painted under every pause-menu
# frame the native window drew. The browser page has no such arm and never
# showed it, so the two hosts differed on screen with every tier above green:
# no builder was missing (both hosts call the same one), no constant was
# paired, no injection site diverged (both hosts *have* the call), no render
# kernel was absent, no type unowned, no variant unanswered, no entry
# hotkey-only.
#
# Scope, stated as narrowly as the tiers above. This tier reads the two frame
# paths and asks two questions of them:
#
#   1. Completeness. For each early exit, the fall-through kernels that come
#      after it are the ones that arm skips. Each must either be called
#      inside the arm's own block, or be listed in that arm's waiver with a
#      reason - which is how "this arm deliberately freezes the world" gets
#      written down once instead of being re-derived per reader.
#   2. Pairing. Each host's frame path is a list of kernels, and a kernel one
#      host ticks every frame while the other does not is a simulation the
#      two hosts do not share. Names differ across the two crates
#      (`tick_minigame_extras` / `tick_minigame_ui`), so an alias row pairs
#      them and a `host_only` row declares the ones that really are one
#      host's.
#
# What it does not prove: that a kernel an arm runs runs *correctly* there,
# that the draw pass reads what the kernel wrote, or that two paired kernels
# do the same thing (that is tier 3's question, asked of hand-named pairs).
# It proves only that no arm silently drops a step the fall-through path
# takes, and that neither host's per-frame list has grown a member the
# other's has not.
#
# The ratchet is the `skips` list on each waiver row. One that no longer
# matches is stale and fails; a kernel added to the fall-through path lands
# in none of the arms' lists and fails on every arm at once - which is
# exactly the moment to decide, per arm, whether the new step belongs there.

FRAME_PATHS: dict[str, dict] = {
    "native": {
        "path": NATIVE_REDRAW,
        # The window's per-tick body. `handle_redraw` runs it up to four
        # times per rendered frame (the catch-up drain) and then draws once,
        # so every `continue` here returns to a frame that still draws.
        "anchor": "for _ in 0..run_ticks",
        "exit": "continue",
    },
    "web": {
        "path": WEB_RUNTIME,
        # The page's per-animation-frame entry. Its `return`s hand control
        # back to the JS that reads the draw lists, so they are the same
        # shape as the native `continue`s.
        "anchor": "pub fn tick_frame(&mut self)",
        "exit": "return",
    },
}

# `self.<name>(` calls that are not per-frame kernels: input plumbing and
# render-state rebuilds, which are events rather than steps. A named list
# rather than a name-prefix rule, because a prefix rule ("tick_", "poll_")
# silently drops a kernel the moment one is named otherwise - and the
# fall-through paths already carry `advance_ocean_animation`,
# `apply_world_clut_fx`, `rebind_live_npc_models`,
# `check_battle_vram_residency`, `maybe_install_demo_tile_board` and
# `service_cutscene_fmv`, none of which such a rule would find.
FRAME_PATH_NON_KERNELS = {
    "handle_key",
    "rebuild_scene_render_state",
    "rebuild_render_state",
}

SELF_CALL_RE = re.compile(r"self\.([a-z_][a-z_0-9]*)\s*\(")
EXIT_WORD_RE = re.compile(r"\b(continue|return)\b")
# An arm's header starts at the line-start keyword that opens it. Scanning
# back to the nearest `;`/`{`/`}` instead is not enough: `if let
# SceneTickEvent::SceneEntered { name } = event {` carries braces inside its
# own pattern, and the nearest-brace rule reports that arm as `= event`.
ARM_HEAD_RE = re.compile(
    r"(?m)^[ \t]*\}?\s*"
    r"((?:else\s+if\s+let|else\s+if|else|if\s+let|if|while\s+let|while|let|match)\b)"
)
# How far back an arm header may reasonably start. Past this the nearest-
# boundary rule is used instead, so a pathological span cannot swallow the
# statement above it.
ARM_HEAD_WINDOW = 600


def brace_block(text: str, start: int) -> tuple[int, int]:
    """`(body_start, body_end)` of the brace block opening at/after `start`."""
    open_at = text.index("{", start)
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return open_at + 1, i
    raise ValueError("unbalanced braces")


def normalise_condition(src: str) -> str:
    """Collapse an arm header to one line, for matching and for printing."""
    return " ".join(src.split())


def frame_path_scan(host: str) -> tuple[list[tuple[str, int]], list[dict]]:
    """`(kernels, arms)` for one host's frame path.

    `kernels` is the fall-through list: every `self.<name>(` call at the
    body's own nesting depth, in source order. `arms` is one record per early
    exit nested inside it, carrying the arm's header text, the calls made
    within the arm's block, and the fall-through kernels it therefore skips.
    """
    spec = FRAME_PATHS[host]
    text = spec.get("_source")
    if text is None:
        text = strip_comments((REPO / spec["path"]).read_text(encoding="utf-8"))
    body_start, body_end = brace_block(text, text.index(spec["anchor"]))
    body = text[body_start:body_end]
    base_line = text.count("\n", 0, body_start) + 1

    # One pass: nesting depth, the stack of open-brace offsets, every call
    # site and every exit word.
    depth = 0
    stack: list[int] = []
    line = base_line
    kernels: list[tuple[str, int]] = []
    calls: list[tuple[int, str]] = []  # (offset, name)
    exits: list[tuple[int, int]] = []  # (enclosing block `{` offset, line)
    i = 0
    while i < len(body):
        ch = body[i]
        if ch == "\n":
            line += 1
            i += 1
            continue
        if ch == "{":
            depth += 1
            stack.append(i)
            i += 1
            continue
        if ch == "}":
            depth -= 1
            if stack:
                stack.pop()
            i += 1
            continue
        m = SELF_CALL_RE.match(body, i)
        if m:
            name = m.group(1)
            if name not in FRAME_PATH_NON_KERNELS:
                calls.append((i, name))
                if depth == 0:
                    kernels.append((name, line))
            i = m.end() - 1
            continue
        m2 = EXIT_WORD_RE.match(body, i)
        if m2:
            prev = body[i - 1] if i else " "
            if m2.group(1) == spec["exit"] and not (prev.isalnum() or prev == "_"):
                # Only an exit nested in an arm can skip anything; one at the
                # body's own depth is the path's end, not a short circuit.
                if depth >= 1 and stack:
                    exits.append((stack[-1], line))
            i = m2.end()
            continue
        i += 1

    arms: list[dict] = []
    seen_blocks: set[int] = set()
    for block_open, exit_line in exits:
        if block_open in seen_blocks:
            continue
        seen_blocks.add(block_open)
        # The arm header is the source from the end of the previous statement
        # or block up to this block's `{` - `if <cond>`, `if let <pat> = <e>`,
        # `let <pat> = <e> else`, `else`.
        head_from = max(
            body.rfind(";", 0, block_open),
            body.rfind("{", 0, block_open),
            body.rfind("}", 0, block_open),
        )
        window = max(0, block_open - ARM_HEAD_WINDOW)
        # The last line-start keyword whose span up to the arm's `{` is
        # brace-balanced. Balance is what separates a pattern's own braces
        # (`SceneEntered { name }`, balanced) from an enclosing block that
        # merely happens to start with a keyword (`match m {`, unbalanced).
        keyword = None
        for m in ARM_HEAD_RE.finditer(body, window, block_open):
            # From the KEYWORD, not the match: the optional `}` prefix that
            # lets `} else {` be found would otherwise unbalance every span.
            span = body[m.start(1) : block_open]
            if span.count("{") == span.count("}"):
                keyword = m.start(1)
        if keyword is not None:
            head_from = keyword - 1
        header = normalise_condition(body[head_from + 1 : block_open])
        block_end = brace_block(body, block_open)[1]
        inside = {n for off, n in calls if block_open < off < block_end}
        # A kernel the fall-through path runs after this arm's exit, and the
        # arm does not run itself, is skipped for as long as the arm is taken.
        skipped = [n for n, kline in kernels if kline > exit_line and n not in inside]
        arms.append(
            {
                "header": header,
                "line": exit_line,
                "inside": inside,
                "skips": sorted(dict.fromkeys(skipped)),
            }
        )
    return kernels, arms


# The browser host has a SECOND frame path, and it is not Rust. The page's
# `_frame` calls `rt.tick_frame()` inside a guard and calls the overlay draw
# outside it, so a guarded frame runs no kernel at all and still paints -
# every kernel in `tick_frame`, at once, from whatever answer it last held.
# A Rust-only scan is blind to this: `tick_frame` has no such arm, which is
# how "the browser page has no early-out" came to be written down.
WEB_PAGE_FRAME = "site/js/play-app.js"
WEB_PAGE_FRAME_FN = "_frame(skipDraw)"
WEB_PAGE_TICK_CALL = "rt.tick_frame()"
WEB_PAGE_DRAW_CALL = "this._drawOverlay()"


def page_frame_gates() -> tuple[list[str], bool]:
    """`(guards, draw_call_found)` for the page's frame function.

    `guards` is every `if (...)` condition the WASM per-frame call sits
    inside and the overlay draw does not. An empty list means the page ticks
    and draws under the same conditions, which is the shape that needs no
    disclosure.
    """
    text = strip_all_comments((REPO / WEB_PAGE_FRAME).read_text(encoding="utf-8"))
    body_start, body_end = brace_block(text, text.index(WEB_PAGE_FRAME_FN))
    body = text[body_start:body_end]
    tick_at = body.index(WEB_PAGE_TICK_CALL)
    draw_at = body.rfind(WEB_PAGE_DRAW_CALL)

    # The stack of open blocks at a given offset, as (block_open, header).
    def open_blocks(at: int) -> list[tuple[int, str]]:
        stack: list[int] = []
        for i, ch in enumerate(body[:at]):
            if ch == "{":
                stack.append(i)
            elif ch == "}" and stack:
                stack.pop()
        out = []
        for open_at in stack:
            head_from = max(
                body.rfind(";", 0, open_at),
                body.rfind("{", 0, open_at),
                body.rfind("}", 0, open_at),
            )
            out.append((open_at, normalise_condition(body[head_from + 1 : open_at])))
        return out

    tick_stack = open_blocks(tick_at)
    draw_stack = {b for b, _ in open_blocks(draw_at)} if draw_at >= 0 else set()
    guards = [
        head
        for block_open, head in tick_stack
        if block_open not in draw_stack and head.startswith("if")
    ]
    return guards, draw_at >= 0


def load_frame_waivers() -> tuple[list[dict], list[dict]]:
    if not WAIVERS.is_file():
        return [], []
    data = tomllib.loads(WAIVERS.read_text(encoding="utf-8"))
    return data.get("frame_arm", []), data.get("frame_kernel", [])


def check_frame_paths() -> tuple[list[str], list[str], dict[str, int]]:
    problems: list[str] = []
    notes: list[str] = []
    arm_waivers, kernel_waivers = load_frame_waivers()
    scans = {host: frame_path_scan(host) for host in FRAME_PATHS}
    counts = {host: len(scans[host][0]) for host in FRAME_PATHS}
    # The page's JS gate is an arm of the browser host like any other; it
    # skips the WHOLE Rust frame path, so its skip list is that path.
    page_guards, page_draw_found = page_frame_gates()
    if page_guards:
        scans["page"] = (
            [],
            [
                {
                    "header": guard,
                    "line": 0,
                    "inside": set(),
                    "skips": sorted({n for n, _ in scans["web"][0]}),
                }
                for guard in page_guards
            ],
        )
        counts["page (guards over the whole web path)"] = len(page_guards)
    elif not page_draw_found:
        problems.append(
            f"FRAME PAGE {WEB_PAGE_FRAME}: could not locate "
            f"`{WEB_PAGE_DRAW_CALL}` in `{WEB_PAGE_FRAME_FN}` - the gate "
            f"cannot tell whether the page's tick and draw share a guard, "
            f"and a detector that finds nothing reports every host clean."
        )

    # --- half 1: no arm silently drops a fall-through step ----------------
    used_arm_waivers: set[int] = set()
    for host, (_kernels, arms) in scans.items():
        for arm in arms:
            matched = [
                (n, w)
                for n, w in enumerate(arm_waivers)
                if w.get("host") == host and str(w.get("arm", "")) in arm["header"]
            ]
            if len(matched) > 1:
                problems.append(
                    f"FRAME ARM {host} `{arm['header']}`: {len(matched)} waivers "
                    f"match this arm - make each `arm` key name one arm only."
                )
                continue
            waived: set[str] = set()
            if matched:
                idx, entry = matched[0]
                used_arm_waivers.add(idx)
                if not str(entry.get("reason", "")).strip():
                    problems.append(
                        f"FRAME ARM {host} `{arm['header']}`: needs a "
                        f"non-empty `reason`."
                    )
                waived = set(entry.get("skips", []))
                stale = sorted(waived - set(arm["skips"]))
                if stale:
                    problems.append(
                        f"STALE FRAME-ARM WAIVER {host} `{entry.get('arm')}`: "
                        f"{', '.join(stale)} - not skipped by this arm any "
                        f"more (the arm runs it, or it left the frame path). "
                        f"Drop it from `skips`."
                    )
            missing = [k for k in arm["skips"] if k not in waived]
            if missing:
                problems.append(
                    f"FRAME ARM {host} `{arm['header']}` (line {arm['line']}) "
                    f"skips {len(missing)} fall-through kernel(s) the frame "
                    f"still draws after: {', '.join(missing)}. Either call "
                    f"them on this arm, or list them in a `[[frame_arm]]` "
                    f"waiver with a reason."
                )
            else:
                notes.append(
                    f"{host} arm `{arm['header']}`: "
                    f"{len(arm['skips'])} kernel(s) skipped, all disclosed"
                )
    for n, entry in enumerate(arm_waivers):
        if n not in used_arm_waivers:
            problems.append(
                f"STALE FRAME-ARM WAIVER {entry.get('host')} "
                f"`{entry.get('arm')}`: matches no arm of that host's frame "
                f"path (renamed, merged or deleted?). Drop the waiver."
            )

    # --- half 2: neither host's per-frame list has an unpaired member -----
    native = {n for n, _ in scans["native"][0]}
    web = {n for n, _ in scans["web"][0]}
    aliases: dict[str, str] = {}
    solo: dict[tuple[str, str], dict] = {}
    for entry in kernel_waivers:
        if entry.get("host_only"):
            solo[(str(entry.get("host")), str(entry.get("kernel")))] = entry
        else:
            aliases[str(entry.get("native"))] = str(entry.get("web"))
    for host, mine, theirs in (("native", native, web), ("web", web, native)):
        for name in sorted(mine):
            if name in theirs:
                continue
            if host == "native":
                twin = aliases.get(name)
            else:
                twin = next((n for n, w in aliases.items() if w == name), None)
            if twin is not None:
                if twin in theirs:
                    continue
                problems.append(
                    f"STALE FRAME-KERNEL ALIAS `{name}` <-> `{twin}`: the "
                    f"other host's frame path no longer calls its half."
                )
                continue
            entry = solo.get((host, name))
            if entry is None:
                problems.append(
                    f"FRAME KERNEL {host}-only: `{name}` is ticked every frame "
                    f"by the {host} host and by no arm of the other host's "
                    f"frame path. Tick it there too, or declare it with a "
                    f"`[[frame_kernel]]` row (`host_only`, or an alias for a "
                    f"differently-named twin)."
                )
            elif not str(entry.get("reason", "")).strip():
                problems.append(
                    f"FRAME KERNEL {host}-only `{name}`: needs a non-empty "
                    f"`reason`."
                )
            else:
                notes.append(f"{host}-only kernel: {name} - {entry['reason']}")
    for (host, name), entry in solo.items():
        mine = native if host == "native" else web
        theirs = web if host == "native" else native
        if name not in mine:
            problems.append(
                f"STALE FRAME-KERNEL WAIVER {host} `{name}`: not on that "
                f"host's frame path any more. Drop the waiver."
            )
        elif name in theirs:
            problems.append(
                f"STALE FRAME-KERNEL WAIVER {host} `{name}`: both hosts tick "
                f"it now - the gap is closed. Drop the waiver."
            )
    for a, b in aliases.items():
        if a not in native or b not in web:
            problems.append(
                f"STALE FRAME-KERNEL ALIAS `{a}` <-> `{b}`: one side is no "
                f"longer on its host's frame path. Drop or re-point the row."
            )
    return problems, notes, counts


# Control suite for the frame-path scanner, over synthetic frame paths. The
# first pair is the defect this tier was written for and its fix, in shape: an
# arm that `continue`s before a kernel the fall-through path runs, and the
# same arm with the kernel stepped on it. Each case is `(label, source,
# anchor, exit_word, arm_substring, expected_skips)`.
SELFTEST_FRAME: list[tuple[str, str, str, str, str, list[str]]] = [
    (
        "an arm that continues before a kernel skips it",
        "for _ in 0..n {\n"
        "    if self.boot_ui.is_active() {\n"
        "        self.tick_boot_ui();\n"
        "        self.prev_pad = self.pad;\n"
        "        continue;\n"
        "    }\n"
        "    self.tick_field_party_hud();\n"
        "}",
        "for _ in 0..n",
        "continue",
        "self.boot_ui.is_active()",
        ["tick_field_party_hud"],
    ),
    (
        "the same arm stepping the kernel skips nothing",
        "for _ in 0..n {\n"
        "    if self.boot_ui.is_active() {\n"
        "        self.tick_boot_ui();\n"
        "        self.tick_field_party_hud();\n"
        "        continue;\n"
        "    }\n"
        "    self.tick_field_party_hud();\n"
        "}",
        "for _ in 0..n",
        "continue",
        "self.boot_ui.is_active()",
        [],
    ),
    (
        "a kernel ABOVE the exit has already run this iteration",
        "for _ in 0..n {\n"
        "    self.tick_play_clock();\n"
        "    if self.paused {\n"
        "        continue;\n"
        "    }\n"
        "    self.tick_dev_menu();\n"
        "}",
        "for _ in 0..n",
        "continue",
        "self.paused",
        ["tick_dev_menu"],
    ),
    (
        "a `let ... else` return arm is an arm",
        "fn tick_frame(&mut self) {\n"
        "    let Some(h) = self.host.as_mut() else {\n"
        "        return;\n"
        "    };\n"
        "    self.tick_camera();\n"
        "}",
        "fn tick_frame(&mut self)",
        "return",
        "let Some(h) = self.host.as_mut() else",
        ["tick_camera"],
    ),
    (
        "a nested return still names its own arm",
        "fn tick_frame(&mut self) {\n"
        "    self.tick_camera();\n"
        "    if entered {\n"
        "        self.on_scene_change();\n"
        "        return;\n"
        "    }\n"
        "    self.poll_field_shop();\n"
        "    self.drive_npc_clips();\n"
        "}",
        "fn tick_frame(&mut self)",
        "return",
        "if entered",
        ["drive_npc_clips", "poll_field_shop"],
    ),
    (
        "an exit at the path's own depth is the end, not an arm",
        "fn tick_frame(&mut self) {\n"
        "    self.tick_camera();\n"
        "    return;\n"
        "}",
        "fn tick_frame(&mut self)",
        "return",
        "",
        ["<arm not found>"],
    ),
]


def _selftest_frame_case(
    src: str, anchor: str, exit_word: str, arm_sub: str
) -> list[str]:
    """Run the arm scanner over one synthetic frame path."""
    FRAME_PATHS["__selftest__"] = {
        "path": None,
        "anchor": anchor,
        "exit": exit_word,
        "_source": src,
    }
    try:
        _kernels, arms = frame_path_scan("__selftest__")
    finally:
        FRAME_PATHS.pop("__selftest__", None)
    if not arms:
        return ["<arm not found>"]
    for arm in arms:
        if arm_sub and arm_sub in arm["header"]:
            return arm["skips"]
    return ["<arm not found>"]

# --------------------------------------------------------------------------
# Tier 12 - content: do two PAIRED frame kernels call the same engine?
#
# Tier 11 pairs a frame path's kernels by NAME (or by an alias row). That
# answers "does each host take this step", and says nothing about what the
# step does on each side - which is the whole of the question the pairing
# invites a reader to assume it answered. A kernel whose native body is
# `{}` pairs perfectly with a browser twin that drains a cue queue and
# advances every clip player.
#
# This tier asks the next question with the only evidence a source scan can
# carry: for each paired kernel, the set of ENGINE functions each host's body
# reaches. Engine = the four wgpu-free crates both hosts link
# (`engine-core` / `engine-vm` / `engine-ui` / `engine-audio`); a host's own
# helpers are followed transitively, so a step spelled as five private
# methods is compared against a twin that inlines them.
#
# What it proves: two paired bodies reach the same engine surface, or the
# difference is written down with a reason. What it does NOT prove: that they
# call it with the same arguments, in the same order, or under the same
# guard. Those are tier 3's question and the audits' - a difference this tier
# cannot see is not evidence of agreement.
#
# The join is by NAME, which carries one deliberate blind spot: nothing here
# can tell `world.clear()` from `Vec::clear()`. Names that are also ordinary
# std / collection / iterator methods are therefore excluded wholesale
# ([`STD_METHOD_NAMES`]) rather than guessed at, and a genuinely divergent
# engine call that happens to be spelled `insert` is invisible. Stating the
# hole is the point: the alternative is a report where two thirds of every
# row is `len`, which is a report nobody reads.
ENGINE_API_CRATES = ("engine-core", "engine-vm", "engine-ui", "engine-audio")

# Names a `.name(` call cannot be attributed to the engine by name alone.
# Ordinary std / core / collection / iterator methods that an engine type
# also happens to define.
STD_METHOD_NAMES = {
    "abs", "add", "all", "and_then", "any", "append", "as_deref", "as_mut",
    "as_ref", "as_slice", "as_str", "bytes", "chain", "chars", "checked_add",
    "checked_sub", "chunks", "clamp", "clear", "clone", "cloned", "cmp",
    "collect", "contains", "contains_key", "copied", "count", "default",
    "deref", "div", "drain", "encode", "ends_with", "entry", "enumerate",
    "eq", "expect", "extend", "fill", "filter", "filter_map", "find",
    "first", "flat_map", "flatten", "fmt", "format", "from", "from_le_bytes",
    "get", "get_mut", "hash", "index", "index_mut", "insert", "into",
    "into_iter", "is_empty", "is_none", "is_some", "iter", "iter_mut",
    "join", "keys", "last", "len", "lines", "load", "map", "max", "min",
    "mul", "ne", "neg", "new", "next", "not", "ok", "ok_or", "or_else",
    "or_insert", "or_insert_with", "parse", "partial_cmp", "pop", "position",
    "print", "println", "push", "read", "rem", "remove", "replace",
    "reserve", "resize", "retain", "rev", "saturating_add", "saturating_sub",
    "set", "slice", "sort", "sort_by", "sort_by_key", "splice", "split",
    "starts_with", "step_by", "store", "sub", "sum", "swap", "swap_remove",
    "take", "to_le_bytes", "to_lowercase", "to_owned", "to_string",
    "to_uppercase", "to_vec", "trim", "truncate", "unwrap", "unwrap_or",
    "unwrap_or_default", "unwrap_or_else", "values", "values_mut",
    "windows", "with_capacity", "wrapping_add", "wrapping_sub", "write",
    "zip",
}

CONTENT_PUB_FN_RE = re.compile(
    r"\bpub(?:\s*\([^)]*\))?\s+(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+"
    r"([a-z_][a-z_0-9]*)"
)
CONTENT_FN_RE = re.compile(r"\bfn\s+([a-z_][a-z_0-9]*)")
METHOD_CALL_RE = re.compile(r"[.:]\s*([a-z_][a-z_0-9]*)\s*\(")
SELF_METHOD_RE = re.compile(r"\bself\.([a-z_][a-z_0-9]*)\s*\(")

# How far a host helper chain is followed out of a kernel body.
CONTENT_DEPTH = 8


def engine_api_names() -> set[str]:
    """Every `pub fn` name the four shared engine crates define, minus the
    std-collision set."""
    out: set[str] = set()
    for crate in ENGINE_API_CRATES:
        root = REPO / "crates" / crate / "src"
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            text = strip_comments(path.read_text(encoding="utf-8"))
            out |= set(CONTENT_PUB_FN_RE.findall(text))
    return out - STD_METHOD_NAMES


def host_fn_bodies(host: str) -> dict[str, list[str]]:
    """Every `fn` body this host's shipped sources define, by name."""
    out: dict[str, list[str]] = {}
    for root in HOSTS[host]:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            text = strip_comments(path.read_text(encoding="utf-8"))
            for m in CONTENT_FN_RE.finditer(text):
                brace = signature_end(text, m.start())
                if brace < 0:
                    continue
                start, end = brace_block(text, brace)
                out.setdefault(m.group(1), []).append(text[start:end])
    return out


def kernel_engine_calls(
    kernel: str, bodies: dict[str, list[str]], api: set[str]
) -> tuple[set[str], int, bool]:
    """`(engine call names, host fns walked, kernel body found)`.

    Follows `self.<helper>(` edges inside the host's own sources, so a kernel
    that delegates is compared by what its whole subtree reaches.
    """
    own = set(bodies)
    seen: set[str] = set()
    calls: set[str] = set()
    stack = [(kernel, 0)]
    found = False
    while stack:
        name, depth = stack.pop()
        if name in seen or depth > CONTENT_DEPTH:
            continue
        seen.add(name)
        for body in bodies.get(name, []):
            if name == kernel:
                found = True
            for c in METHOD_CALL_RE.findall(body):
                if c in api and c not in own:
                    calls.add(c)
            for c in SELF_METHOD_RE.findall(body):
                if c in own:
                    stack.append((c, depth + 1))
    return calls, len(seen), found


def load_content_waivers() -> list[dict]:
    if not WAIVERS.is_file():
        return []
    return tomllib.loads(WAIVERS.read_text(encoding="utf-8")).get("frame_content", [])


def frame_kernel_pairs() -> list[tuple[str, str]]:
    """Every paired kernel across the two frame paths: same name on both, or
    an alias row. Derived, so a new pair joins by existing."""
    native = {n for n, _ in frame_path_scan("native")[0]}
    web = {n for n, _ in frame_path_scan("web")[0]}
    pairs = [(n, n) for n in sorted(native & web)]
    for entry in load_frame_waivers()[1]:
        if entry.get("host_only"):
            continue
        a, b = str(entry.get("native")), str(entry.get("web"))
        if a in native and b in web:
            pairs.append((a, b))
    return sorted(set(pairs))


def check_frame_content() -> tuple[list[str], list[str], int]:
    """Every paired frame kernel: equal engine call sets, or a waiver."""
    problems: list[str] = []
    notes: list[str] = []
    api = engine_api_names()
    if len(api) < 100:
        problems.append(
            "FRAME CONTENT: the engine API scan found almost no `pub fn` - "
            "the comparison below would report every pair clean for the "
            "wrong reason. Check `ENGINE_API_CRATES`."
        )
        return problems, notes, 0
    bodies = {host: host_fn_bodies(host) for host in FRAME_PATHS}
    waivers = load_content_waivers()
    used: set[int] = set()
    pairs = frame_kernel_pairs()
    for a, b in pairs:
        ca, walked_a, found_a = kernel_engine_calls(a, bodies["native"], api)
        cb, walked_b, found_b = kernel_engine_calls(b, bodies["web"], api)
        for host, name, found in (("native", a, found_a), ("web", b, found_b)):
            if not found:
                problems.append(
                    f"FRAME CONTENT: the {host} frame path calls "
                    f"`{name}` but no body for it was found in that host's "
                    f"sources - the pair below cannot be compared."
                )
        only_a = sorted(ca - cb)
        only_b = sorted(cb - ca)
        if not only_a and not only_b:
            notes.append(
                f"content pair `{a}` <-> `{b}`: identical engine call set "
                f"({len(ca)} calls, {walked_a}/{walked_b} host fns walked)"
            )
            continue
        entry = None
        for n, row in enumerate(waivers):
            if str(row.get("native")) == a and str(row.get("web")) == b:
                entry, index = row, n
                break
        if entry is None:
            problems.append(
                f"FRAME CONTENT `{a}` <-> `{b}`: the two bodies reach "
                f"different engine calls - native-only {only_a}, web-only "
                f"{only_b}. Wire the missing half, or declare the difference "
                f"with a `[[frame_content]]` row naming both lists and a "
                f"reason."
            )
            continue
        used.add(index)
        want_a = sorted(str(x) for x in entry.get("native_only", []))
        want_b = sorted(str(x) for x in entry.get("web_only", []))
        if want_a != only_a or want_b != only_b:
            problems.append(
                f"STALE FRAME-CONTENT WAIVER `{a}` <-> `{b}`: the difference "
                f"moved. Now native-only {only_a}, web-only {only_b}; the row "
                f"says {want_a} / {want_b}. Re-derive it, or close the gap."
            )
        elif not str(entry.get("reason", "")).strip():
            problems.append(
                f"FRAME-CONTENT WAIVER `{a}` <-> `{b}`: needs a non-empty "
                f"`reason`."
            )
        else:
            notes.append(
                f"content pair `{a}` <-> `{b}`: "
                f"{len(only_a)} native-only / {len(only_b)} web-only, "
                f"declared - {entry['reason']}"
            )
    for n, row in enumerate(waivers):
        if n not in used:
            problems.append(
                f"STALE FRAME-CONTENT WAIVER `{row.get('native')}` <-> "
                f"`{row.get('web')}`: the pair is gone or its two bodies now "
                f"reach the same engine calls. Drop the row."
            )
    return problems, notes, len(pairs)


# Control suite for the content scan. Each case is
# `(label, kernel, host source, engine api, expected calls)`; the first pair
# is the shape this tier was written for - an empty body paired with one that
# does the work - and the last two are the two ways the scan can go blind
# (a helper chain not followed, a std-named call counted as engine).
SELFTEST_CONTENT: list[tuple[str, str, str, set[str], set[str]]] = [
    (
        "an empty body reaches nothing",
        "tick_props",
        "fn tick_props(&mut self) {}",
        {"advance_clips"},
        set(),
    ),
    (
        "a body that calls the engine reaches it",
        "tick_props",
        "fn tick_props(&mut self) { self.world.advance_clips(1); }",
        {"advance_clips"},
        {"advance_clips"},
    ),
    (
        "a helper chain is followed",
        "tick_props",
        "fn tick_props(&mut self) { self.inner(); }\n"
        "fn inner(&mut self) { self.world.advance_clips(1); }",
        {"advance_clips"},
        {"advance_clips"},
    ),
    (
        "a std-named call is not credited to the engine",
        "tick_props",
        "fn tick_props(&mut self) { self.rows.clear(); }",
        {"clear"} - STD_METHOD_NAMES,
        set(),
    ),
]


def _selftest_content_case(kernel: str, src: str, api: set[str]) -> set[str]:
    bodies: dict[str, list[str]] = {}
    for m in CONTENT_FN_RE.finditer(src):
        brace = signature_end(src, m.start())
        if brace < 0:
            continue
        start, end = brace_block(src, brace)
        bodies.setdefault(m.group(1), []).append(src[start:end])
    return kernel_engine_calls(kernel, bodies, api)[0]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--quiet", action="store_true", help="findings only")
    ap.add_argument("--list", action="store_true", help="print the full surface table")
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the screen-vs-transform control suite and exit",
    )
    args = ap.parse_args()

    if args.selftest:
        print("check-ui-host-drift self-test")
        return run_selftest()

    # The surface is only meaningful if the classifier demonstrably separates
    # the two shapes. Run the control every time: a "0 orphans" verdict from a
    # classifier that counts everything, or nothing, is not a measurement.
    for _label, text, name, want in SELFTEST_WORDS:
        if (name in word_set(text)) != want:
            print(
                "ERROR: built-in word-set control failed; the reachability pass "
                "cannot tell a whole-name reference from a substring, so every "
                "host label below is unreliable. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _name, sig in SELFTEST_SCREENS:
        if not is_screen_signature(sig):
            print(
                "ERROR: built-in screen control failed; the builder surface is not "
                "trustworthy. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _name, sig in SELFTEST_TRANSFORMS:
        if is_screen_signature(sig):
            print(
                "ERROR: built-in transform control failed; the builder surface is not "
                "trustworthy. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, a, b, want in SELFTEST_CONSTANTS:
        if (normalise_value(a) == normalise_value(b)) != want:
            print(
                "ERROR: built-in constant-pair control failed; a normaliser that "
                "cannot tell a value change from formatting proves nothing about "
                "the pairs below. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) != want:
            print(
                "ERROR: built-in signature control failed; the call graph cannot "
                "find function bodies, so every reachability verdict below is "
                "meaningless. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, mode, a, b, extra, want in SELFTEST_SIM:
        if _selftest_sim_case(mode, a, b, extra) != want:
            print(
                "ERROR: built-in sim-pair control failed; a comparator that cannot "
                "tell agreement from divergence proves nothing about the pairs "
                "below. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, init, want in SELFTEST_DIAG:
        if initialiser_is_off(init) != want:
            print(
                "ERROR: built-in diag-toggle control failed; a detector that "
                "cannot tell `new(false)` from `new(true)` proves nothing "
                "about whether a debug draw is off on the browser. Run "
                "--selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, want in SELFTEST_PAGE_KEYS:
        if _selftest_page_key_case(src) != want:
            print(
                "ERROR: built-in page-key control failed; a detector that cannot "
                "tell a page-side keyboard table from an ordinary object field "
                "proves nothing about the pages below. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, rule, src, want in SELFTEST_RENDER:
        if _selftest_render_case(rule, src) != want:
            print(
                "ERROR: built-in render-kernel control failed; a rule engine "
                "that matches nothing reports every surface clean, which is "
                "exactly the silence this tier exists to break. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, name, want in SELFTEST_OWNERSHIP:
        if owns_type(src, name) != want:
            print(
                "ERROR: built-in ownership control failed; a detector that "
                "cannot tell holding a type from naming it would have passed "
                "the play page's Camera-shaped absence. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, enum_name, variant, want in SELFTEST_VARIANT_USE:
        if bool(re.search(rf"\b{enum_name}::{variant}\b", src)) != want:
            print(
                "ERROR: built-in variant-use control failed; a rule that "
                "accepts an unqualified mention reports every host as "
                "answering every variant. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, kernel, src, api, want in SELFTEST_CONTENT:
        if _selftest_content_case(kernel, src, api) != want:
            print(
                "ERROR: built-in frame-content control failed; a scan that "
                "cannot tell an empty kernel from one that calls the engine "
                "reports every pair identical, which is the silence tier 12 "
                "exists to break. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, name, want in SELFTEST_CALL_FORM:
        if bool(re.search(rf"[.:]\s*{name}\s*\(", src)) != want:
            print(
                "ERROR: built-in call-form control failed; a scan that counts "
                "a `pub fn` as its own caller makes the entry-symmetry tier "
                "vacuous. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    builders = collect_builders()
    if not builders:
        print("[ui-drift] no draw builders found - is crates/engine-ui/src present?", file=sys.stderr)
        return 1
    # Seed and propagate over the whole engine-ui fn graph, then classify only
    # the builders. Non-builder nodes exist so a composition that runs through
    # a method is not mistaken for an unused screen.
    fn_names = collect_fn_names()
    uses = collect_uses(fn_names)
    seed_transitively(uses, collect_call_graph(fn_names))
    waivers = load_waivers()

    drift: list[str] = []
    orphan: list[str] = []
    web_ahead: list[str] = []
    both: list[str] = []
    for name in sorted(builders):
        hosts = uses[name]
        if hosts == {"native", "web"}:
            both.append(name)
        elif hosts == {"native"}:
            drift.append(name)
        elif hosts == {"web"}:
            web_ahead.append(name)
        else:
            orphan.append(name)

    if args.list:
        for name in sorted(builders):
            hosts = uses[name] or {"-"}
            mark = "W" if name in waivers else " "
            print(f"{mark} {name:<40} {','.join(sorted(hosts)):<12} {builders[name]}")

    problems: list[str] = []

    # Unwaived drift / orphans.
    for name in drift:
        if name in waivers:
            if waivers[name].get("kind") != "web_missing":
                problems.append(
                    f"{name}: waiver kind is "
                    f"'{waivers[name].get('kind')}' but the builder is native-only "
                    f"(expected kind = \"web_missing\")"
                )
            continue
        problems.append(
            f"DRIFT {name} ({builders[name]}): wired in the native window, "
            f"not in the browser play page. Wire it into crates/web-viewer, or "
            f"add a waiver with a reason to {WAIVERS.relative_to(REPO)}."
        )
    for name in orphan:
        if name in waivers:
            if waivers[name].get("kind") != "orphan":
                problems.append(
                    f"{name}: waiver kind is '{waivers[name].get('kind')}' but "
                    f"no host calls the builder (expected kind = \"orphan\")"
                )
            continue
        problems.append(
            f"ORPHAN {name} ({builders[name]}): no host calls this builder. "
            f"Wire it, delete it, or waive it in {WAIVERS.relative_to(REPO)}."
        )

    # Stale waivers - the half that stops this file decaying into fiction.
    for name, entry in sorted(waivers.items()):
        if name not in builders:
            problems.append(
                f"STALE WAIVER {name}: no such engine-ui draw builder "
                f"(renamed or deleted?). Drop the waiver."
            )
            continue
        if name in both:
            problems.append(
                f"STALE WAIVER {name}: now wired on BOTH hosts - the gap is "
                f"closed. Drop the waiver."
            )
        if name in web_ahead:
            problems.append(
                f"STALE WAIVER {name}: web calls it and native does not, so "
                f"this is not a web gap. Drop the waiver."
            )
        if not str(entry.get("reason", "")).strip():
            problems.append(f"WAIVER {name}: needs a non-empty `reason`.")

    # The model half: paired host constants must carry equal values.
    problems.extend(check_constant_pairs())

    # The simulation half: paired injection sites must name the same kernels.
    sim_problems, sim_pending = check_sim_pairs()
    problems.extend(sim_problems)

    # The input half: no page may carry its own keyboard table.
    key_problems, key_files = check_page_key_tables()
    problems.extend(key_problems)

    # The diagnostic half: an additive debug draw must be off by default on
    # BOTH hosts, or one host paints it in normal play.
    diag_problems, diag_additive = check_diag_gates()
    problems.extend(diag_problems)

    # The render half: every surface that assembles a given draw list must run
    # the kernel that makes it correct. The surface is derived, so a new one
    # joins the measurement by existing.
    rk_problems, rk_pending, rk_counts = check_render_kernels()
    problems.extend(rk_problems)

    # The ownership half: a host that reaches an engine type's outputs without
    # holding the type re-implements it, invisibly to every tier above.
    shipped = host_shipped_sources()
    own_problems, own_pending = check_owned_types(shipped)
    problems.extend(own_problems)

    # The enum half: a variant both hosts can enter and one can answer.
    enum_problems, enum_pending, enum_checked = check_enum_coverage(shipped)
    problems.extend(enum_problems)

    # The entry half: a World phase whose only caller is a debug key press.
    entry_problems, entry_pending, entry_methods = check_entry_symmetry()
    problems.extend(entry_problems)

    # The frame half: an arm that short-circuits the frame path without
    # short-circuiting the draw that reads what it skipped.
    frame_problems, frame_notes, frame_counts = check_frame_paths()
    problems.extend(frame_problems)

    # The content half: a pair of frame kernels that reach different engine
    # calls is a simulation the two hosts do not share, however well their
    # names pair.
    content_problems, content_notes, content_pairs = check_frame_content()
    problems.extend(content_problems)

    if not args.quiet:
        print(
            f"[ui-drift] engine-ui draw builders: {len(builders)} "
            f"({len(both)} on both hosts, {len(drift)} native-only, "
            f"{len(web_ahead)} web-only, {len(orphan)} unused)"
        )
        print(
            f"[ui-drift] paired host geometry constants: {len(CONSTANT_PAIRS)} "
            f"(value equality only - see the module docstring for what this "
            f"does not prove)"
        )
        print(
            f"[ui-drift] paired simulation injection sites: {len(SIM_PAIRS)} "
            f"({len(sim_pending)} disclosed as blocked)"
        )
        print(
            f"[ui-drift] pad-driving site sources scanned for page-side "
            f"keyboard tables: {key_files}"
        )
        print(
            f"[ui-drift] LEGAIA_DIAG_* gates declared: {len(DIAG_GATES)} "
            f"({diag_additive} additive - i.e. draw something retail does not "
            f"and so need a default-off twin on both hosts)"
        )
        print(
            f"[ui-drift] render kernels checked across "
            f"{len(render_sources())} render-surface sources: "
            f"{len(RENDER_KERNEL_RULES)} rules"
        )
        # Name every row of the kernel x surface matrix. A bare rule count
        # cannot tell "three surfaces assemble this list" from "one does and
        # two were renamed out of the trigger", and the second is how a
        # derived surface quietly shrinks to nothing.
        for kernel, running, blocked_n, exempt_n in rk_counts:
            print(
                f"[ui-drift] render kernel `{kernel}`: {running} surface(s) "
                f"run it, {blocked_n} disclosed as blocked, {exempt_n} exempt"
            )
        for note in rk_pending:
            print(f"[ui-drift] render kernel blocked: {note}")
        print(
            f"[ui-drift] engine types both hosts must own: {len(OWNED_TYPES)} "
            f"({len(own_pending)} disclosed as blocked)"
        )
        for note in own_pending:
            print(f"[ui-drift] unowned type blocked: {note}")
        print(
            f"[ui-drift] shared-enum variant answers checked: {enum_checked} "
            f"across {len(ENUM_COVERAGE)} enum(s) x {len(HOSTS)} hosts "
            f"({len(enum_pending)} waived)"
        )
        for note in enum_pending:
            print(f"[ui-drift] variant waived: {note}")
        print(
            f"[ui-drift] World methods reached from the native hotkey arms: "
            f"{entry_methods} ({len(entry_pending)} reached from nowhere else, "
            f"each disclosed)"
        )
        for note in entry_pending:
            print(f"[ui-drift] hotkey-only: {note}")
        print(
            "[ui-drift] per-frame kernels on each host's frame path: "
            + ", ".join(f"{h} {n}" for h, n in sorted(frame_counts.items()))
        )
        # Name every disclosure, for the reason the native-only builders
        # below are named: a count cannot tell "the same arms as yesterday"
        # from "an arm gained a skip and another lost one".
        for note in frame_notes:
            print(f"[ui-drift] frame path: {note}")
        print(
            f"[ui-drift] paired frame kernels compared by CONTENT: "
            f"{content_pairs} (engine call sets; see the tier-12 note for "
            f"what a name join cannot see)"
        )
        for note in content_notes:
            print(f"[ui-drift] {note}")
        if web_ahead:
            print(f"[ui-drift] web-ahead (informational): {', '.join(web_ahead)}")
        # Name every native-only builder, waived or not, for the same reason
        # the orphans below are named. A waived row still prints nothing but
        # its contribution to a count, so "2 native-only" is indistinguishable
        # from "the same 2 as yesterday plus one that lost its web caller and
        # one that gained one" - the arithmetic is stable while the membership
        # is not. Naming is what makes a waiver auditable from the output.
        for name in drift:
            mark = "waived" if name in waivers else "UNWAIVED"
            print(f"[ui-drift] native-only ({mark}): {name}  {builders[name]}")
        # Name every orphan, waived or not. A bare count cannot distinguish
        # "the same six as yesterday" from "a builder's last caller was
        # deleted this morning", which is exactly how window 25's painter
        # chain lost its consumer without a line of output changing.
        for name in orphan:
            mark = "waived" if name in waivers else "UNWAIVED"
            print(f"[ui-drift] orphan ({mark}): {name}  {builders[name]}")
        for note in sim_pending:
            print(f"[ui-drift] sim pair blocked: {note}")

    if problems:
        print(f"\n[ui-drift] {len(problems)} problem(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1

    if not args.quiet:
        print("[ui-drift] ok - every shared screen reaches both hosts or is waived")
    return 0


if __name__ == "__main__":
    sys.exit(main())
