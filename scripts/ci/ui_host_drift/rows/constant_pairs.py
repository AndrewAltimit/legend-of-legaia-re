"""Tier 2 rows: geometry constants each host declares once and that must agree.

Analysis: `ui_host_drift.tiers.constant_pairs`.
"""

from ui_host_drift.rows.host_files import (
    NATIVE_DEV_MENU,
    NATIVE_DYN_LIGHT,
    NATIVE_DYN_WINDOWS,
    NATIVE_OCCL,
    NATIVE_PSX_DITHER,
    WEB_PLAY_DEV_MENU,
    WEB_SHADERS,
)

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
    # The dev-menu pens (`DEV_MENU_PEN`, `DEV_RECORDS_PEN`) and the Records page
# headings (`RECORDS_LABELS`) were pairs here too; they are one copy in
# `legaia_engine_screens::dev_menu` now, which both hosts import.
# `legaia_engine_ui::pause_menu` (`MENU_WINDOW_FALLBACK` /
    # `MENU_SUBWINDOW_CONTENT`), read by the shared composition both hosts
    # call. A pair proves two copies agree; one copy needs no proof. The
    # engine-ui rect table is exercised by `tests/pause_menu_compose.rs`; the
    # disc side is pinned separately by the disc-gated `menu_windows_real`
    # test, which asserts the same rects against its own literal list rather
    # than against this constant.
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
    {
        "what": "occlusion-fade feet-line ramp, as a fraction of the feet -> "
        "body-centre screen span - nothing at or below the player's feet on "
        "screen fades, so the floor in front of them never dissolves "
        "(folded into each host's lift axis, lift_axis / occlLiftAxis)",
        "native": (NATIVE_OCCL, "OCCL_LIFT_FEATHER_FRAC"),
        "web": (WEB_SHADERS, "OCCL_LIFT_FEATHER_FRAC"),
    },
    # The two opt-in render toggles (PSX rasterisation, dynamic lighting) are
    # hand-written twin shaders too: the page interpolates these JS constants
    # into its GLSL, so pairing them pairs the shader model.
    {
        "what": "PSX ordered-dither matrix - the 15-bit dither each host's "
        "PSX-rasterisation toggle adds before truncating to 5 bits",
        "native": (NATIVE_PSX_DITHER, "DITHER_MATRIX"),
        "web": (WEB_SHADERS, "PSX_DITHER_MATRIX"),
    },
] + [
    {
        "what": f"dynamic-light model constant {native}",
        "native": (path, native),
        "web": (WEB_SHADERS, web),
    }
    for path, native, web in (
        (NATIVE_DYN_LIGHT, "DIFFUSE", "DYN_DIFFUSE"),
        (NATIVE_DYN_LIGHT, "TOTAL_MAX_GAIN", "DYN_TOTAL_MAX_GAIN"),
        (NATIVE_DYN_LIGHT, "MAX_GAIN", "DYN_MAX_GAIN"),
        (NATIVE_DYN_LIGHT, "LAMBERT_FALLBACK", "DYN_LAMBERT_FALLBACK"),
        (NATIVE_DYN_LIGHT, "POOL_CENTER", "DYN_POOL_CENTER"),
        (NATIVE_DYN_LIGHT, "POOL_INNER", "DYN_POOL_INNER"),
        (NATIVE_DYN_LIGHT, "POOL_OUTER", "DYN_POOL_OUTER"),
        # Lit windows: the glass test and the pane colour of `shade_window`.
        (NATIVE_DYN_WINDOWS, "WINDOW_GLASS_MIN_BLUE", "DYN_WIN_GLASS_MIN_BLUE"),
        (NATIVE_DYN_WINDOWS, "WINDOW_RGB", "DYN_WIN_RGB"),
        (NATIVE_DYN_WINDOWS, "WINDOW_FLOOR", "DYN_WIN_FLOOR"),
        (NATIVE_DYN_WINDOWS, "WINDOW_GLASS_BLACK_MAX", "DYN_WIN_GLASS_BLACK_MAX"),
    )
] + [
    # The point lights' shadow maps: the page's depth-array pass
    # (webgl-tmd.js `_renderLightShadows`) rebuilds the native cone and
    # compare from these literals, so each is the shadow model.
    {
        "what": f"point-light shadow constant {name}",
        "native": (path, name),
        "web": ("site/js/webgl-tmd.js", name),
    }
    for path, name in (
        ("crates/engine-render/src/renderer/depth.rs", "SHADOW_MAP_DIM"),
        ("crates/engine-render/src/renderer/depth.rs", "SHADOW_COMPARE_BIAS"),
        ("crates/engine-render/src/scene_lights.rs", "SHADOW_FOV"),
        ("crates/engine-render/src/scene_lights.rs", "SHADOW_NEAR_FRAC"),
        ("crates/engine-render/src/scene_lights.rs", "SHADOW_NEAR_MIN"),
    )
]
