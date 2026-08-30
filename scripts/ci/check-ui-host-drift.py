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

BUILDER_RE = re.compile(r"^pub fn (?P<name>[a-z0-9_]+)\s*[<(]", re.MULTILINE)
DRAW_RET_RE = re.compile(rf"->[^;{{]*(?:{DRAW_RECORDS})")

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
        "web": (WEB_RUNTIME, "TRANSITION_FADE_IN_SAMPLES"),
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
NATIVE_FRAME_TICK = "crates/engine-core/src/world/frame_tick.rs"
NATIVE_BOOT_CUTSCENE = "crates/engine-shell/src/bin/legaia-engine/window/boot_cutscene.rs"
NATIVE_REDRAW = "crates/engine-shell/src/bin/legaia-engine/window/event_handler/redraw.rs"
NATIVE_FIELD_RENDER = "crates/engine-shell/src/bin/legaia-engine/window/field_render.rs"
NATIVE_GEOMETRY = "crates/engine-shell/src/bin/legaia-engine/window/geometry.rs"
WEB_MINIGAMES_MUSCLE = "crates/web-viewer/src/minigames_muscle.rs"
WEB_PLAY_BATTLE = "crates/web-viewer/src/play_battle.rs"
WEB_PLAY = "crates/web-viewer/src/play.rs"
WEB_FIELD_SCENE = "crates/web-viewer/src/field_scene.rs"

SIM_PAIRS: list[dict[str, object]] = [
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
        "what": "ground-heightfield sink, native vs play page - the walk-ground "
        "grid shares its plane with the env pack's authored floor art (koin6: "
        "both at y=0 with different tessellations), so every render site must "
        "sink it by the shared GROUND_SINK or that host's floors z-fight as "
        "wedge streaks from steep cameras while every other host is clean",
        "sites": {
            "native": (NATIVE_GEOMETRY, "heightfield_to_vram_mesh"),
            "web": (WEB_PLAY, "field_ground_positions"),
        },
        "mode": "symbols_all",
        "symbols": ["GROUND_SINK"],
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
        "symbols": ["GROUND_SINK"],
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
        "what": "save-select model - which rack a host declares decides how "
        "many pills the screen shows and what each one addresses, so the two "
        "hosts must declare the same kind. No host sets the card-slots flag "
        "any more: `SaveSelectSession::for_rack` derives it from the "
        "`SaveRack` variant, and the driver around the second stage is the "
        "shared `save_screen::SaveScreenFlow` - so the assertion is on the "
        "rack kind each host builds, which is the one thing left that a host "
        "still chooses",
        "sites": {
            "native": (NATIVE_SAVE_HELPERS, "disk_save_rack"),
            "web": (WEB_PLAY_MENU, None),
        },
        "mode": "pattern_same",
        "pattern": r"SaveRack::(\w+)",
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
            "web": (WEB_RUNTIME, "play"),
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
        "blocked_on": {
            "crates/web-viewer/src/minigames_dance.rs":
                "the dance-hall venue baker resolves the same two EnvDraw "
                "layers and instances them itself; it needs the lift map "
                "threaded through DanceEnv::append_draw",
            "crates/web-viewer/src/minigames_fishing_scene.rs":
                "same shape in FishingEnv::append_draw",
        },
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
        "blocked_on": {
            "crates/web-viewer/src/minigames_fishing_scene.rs":
                "the fishing venue splices the heightfield into the same "
                "vertex buffer as the env meshes, at its authored height",
        },
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
        "blocked_on": {
            "crates/web-viewer/src/minigames_dance.rs":
                "the dance hall fills both its no-colour-word streams (the "
                "pure-textured env fallback and the ground heightfield) with "
                "white; both want NEUTRAL",
            "crates/web-viewer/src/minigames_fishing_scene.rs":
                "same two streams in the fishing venue baker",
        },
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
            rel = str(path.relative_to(REPO))
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
