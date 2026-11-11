"""Tier 17 rows: GL state and frame order the play page's JavaScript owes.

The Rust tiers cannot see the page's GL state, and the page's frame is one
function over one context. Each row names a thing a real defect left wrong,
found by `scripts/ci/host-pair.mjs`, in the one file that has to hold it.

Analysis: `ui_host_drift.tiers.page_state`.
"""

from ui_host_drift.roots import REPO

# `kind` is one of:
#
# - `contains`: `source` must appear in `path`.
# - `before`: `source` must appear in `path`, ahead of the first `then`.
PAGE_STATE_RULES = [
    {
        "what": "the renderer hands the winding back with the cull",
        "path": REPO / "site/js/webgl-tmd.js",
        "kind": "contains",
        "source": "gl.frontFace(gl.CCW);",
        "why": "the scene pass reads gl_FrontFacing for retail's NCLIP and states "
        "no winding of its own, so a `cullFrontFace: 'cw'` left behind by the "
        "dance hall draws the field with its ground gone for the session",
    },
    {
        "what": "the page declares the intro readback ahead of the frame's ticks",
        "path": REPO / "site/js/play-view-frame.js",
        "kind": "before",
        "source": "rt.play_set_intro_readback(",
        "then": "window.LegaiaPlayMinigames.frame(",
        "why": "the engine leaves the transition layer off the arming frame only "
        "for a host that declared the readback, so a fight armed on the first "
        "frame reads its own strips back as the field",
    },
    {
        "what": "a screen primitive on the PSX canvas takes the tie nudge",
        "path": REPO / "site/js/play-view-frame.js",
        "kind": "contains",
        "source": "f[o + 1] -= 1 / 240 - PSX_TIE_NDC_Y;",
        "why": "the canvas's bottom-up fill rule hands a corner's row tie to the "
        "row under the GPU's: every sprite a row low, its last texel row past "
        "the art, the slot machine's dot matrix missing its top rows",
    },
    {
        "what": "leaving a dance takes the hall's ambient tree off the world",
        "path": REPO / "site/js/play-minigames.js",
        "kind": "before",
        "source": "rt.play_mg_dance_ambient_step();",
        "then": "releaseRenderer(rt, view);\n    S.slotVram",
        "why": "past the last dance frame nothing else asks, and the field keeps "
        "the hall's tree, stagers and frame step",
    },
    {
        "what": "a pause-menu tick composes the frame's screen layers",
        "path": REPO / "crates/web-play/src/runtime/frame_ticks.rs",
        "kind": "before",
        "source": "self.rebuild_screen_geom();",
        "then": "TickOwner::PauseMenuSpawned =>",
        "why": "no world tick composes them on such a frame, and stale layers "
        "lose the wipe on the frame the menu closes: a flash of the lit field "
        "the window, which composes at the draw, does not show",
    },
    {
        "what": "the scene pass stages zeros for a mesh's absent ground refs",
        "path": REPO / "site/js/webgl-tmd-assembled-scene.js",
        "kind": "contains",
        "source": "gl.vertexAttrib4f(this.locGroundRefXz, 0, 0, 0, 0);",
        "why": "a disabled attribute reads the generic value, (0, 0, 0, 1) by "
        "default, and the vertex stage keys 'not a ground cell' on zeros: no "
        "mesh took its link depth, so PSX mode's per-primitive sort was inert "
        "and the sea covered town01's beach off retail's vantage",
    },
    {
        "what": "an untextured screen primitive's tie nudge is coarser than SwiftShader's sub-pixel grid",
        "path": REPO / "site/js/play-view-frame.js",
        "kind": "contains",
        "source": "f[o + 1] -= 1 / 240 - PSX_TIE_FLAT_NDC_Y;",
        "why": "a 64th of a row rounded back onto the tie under SwiftShader, so "
        "every flat band on the PSX canvas (the ending vignettes' letterbox) drew "
        "a row low in the headless pair runs while a real GPU drew it right",
    },
    {
        "what": "the dome hub draws through the screen-primitive pass",
        "path": REPO / "site/js/play-minigames.js",
        "kind": "contains",
        "source": "hub.pass.drawRuns(0, (runs.length / 3) | 0);",
        "why": "a 2D-canvas blit of the hub quads resampled the stretched wall "
        "tiles off the GPU's texel rule (the wall a column left from x = 288) "
        "and drew the course card's text lighter: 3.55% of every hub frame "
        "off the window, on SwiftShader and a real GPU alike",
    },
    {
        "what": "the dome hub draws over the screen primitives, under the text",
        "path": REPO / "site/js/play-view-frame.js",
        "kind": "before",
        "source": "if (!skipDraw) this._drawScreenPrims(rt);",
        "then": "window.LegaiaPlayMinigames.drawHub(rt, this);",
        "why": "the hub's layer sat over the GL frame and under the page's "
        "text, the window's order; drawn inside the minigame frame it would "
        "go under the screen primitives, and on a field frame under the scene",
    },
    {
        "what": "every frame stages the game's dither word on the renderer",
        "path": REPO / "site/js/play-view-frame.js",
        "kind": "contains",
        "source": "this.renderer.setDrawDither(rt.play_draw_dither());",
        "why": "field op `4C ED` and a battle's colour wash clear the word "
        "(`World::frame_dither`), which the window hands its renderer on every "
        "redraw; a page that stops asking dithers those frames in PSX mode "
        "while the window does not",
    },
    {
        "what": "the dither word gates the dither lane of the PSX uniform",
        "path": REPO / "site/js/webgl-tmd-frame-state.js",
        "kind": "contains",
        "source": "on && this.drawDither !== false ? 1 : 0",
        "why": "the word reaches the shader only through `u_psx.w`, the lane "
        "the window's `psx_snap_params` clears; staged and never read, the "
        "page would dither every PSX-mode frame",
    },
    {
        "what": "PSX mode's frame is read out as the 15-bit framebuffer",
        "path": REPO / "site/js/play-app.js",
        "kind": "contains",
        "source": "this._psxTarget.present(!this.ssao && !this.bloom && !this._fogDrew);",
        "why": "the window presents and captures PSX mode through `fs_scanout15` "
        "unless SSAO, bloom or a drawn fog lays a smooth term over it "
        "(`Renderer::scanout_15bit`); a page copying the raw frame shows a "
        "blended pixel's low bits, up to seven levels off the window's",
    },
    {
        "what": "a drawn fog stands the 15-bit readout down",
        "path": REPO / "site/js/play-view-frame.js",
        "kind": "contains",
        "source": "this._fogDrew = this._fogPass.draw(",
        "why": "the window keys its readout on the fog having drawn "
        "(`fog_volume_active`), not on its option; a page that never learns "
        "the fog drew truncates the fog's smooth term to 5 bits",
    },
]
