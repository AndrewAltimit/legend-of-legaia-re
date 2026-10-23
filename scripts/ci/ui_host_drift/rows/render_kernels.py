"""Tier 7 rows: render-surface roots, surface groups and the kernel rules.

Analysis: `ui_host_drift.tiers.render_kernels`.
"""

from ui_host_drift.roots import REPO

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
    REPO / "crates" / "engine-shell" / "src",
    REPO / "crates" / "engine-render" / "src",
    REPO / "crates" / "engine-world-views" / "src",
    REPO / "crates" / "engine-screens" / "src",
    REPO / "crates" / "web-play" / "src",
    REPO / "crates" / "web-minigames" / "src",
    REPO / "crates" / "web-viewer" / "src",
    REPO / "crates" / "web-patcher" / "src",
    REPO / "site" / "js",
]

# A class declared across several files is ONE render surface. The rules below
# test one file's text at a time, and a rule whose two halves sit in different
# files of the same class - the play page uploads its field ground in
# `play-view-scene-build.js` and drives the battle frame in `play-view-battle.js` - would
# otherwise stop matching and report zero surfaces, which reads as clean. Each
# group is scanned as one source under its first path.
RENDER_SURFACE_GROUPS: list[list[str]] = [
    [
        "site/js/play-app.js",
        "site/js/play-view-scene-build.js",
        "site/js/play-view-input.js",
        "site/js/play-view-overlay.js",
        "site/js/play-view-field-sync.js",
        "site/js/play-view-frame.js",
        "site/js/play-view-battle.js",
    ],
    [
        "site/js/webgl-tmd.js",
        "site/js/webgl-tmd-frame-state.js",
        "site/js/webgl-tmd-assembled-scene.js",
    ],
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
        "exempt": {
            "crates/engine-shell/src/window/field_render/prop_animation.rs":
                "resolves the placed draws only to read each one's "
                "`(res_tmd, anim_id)` bake key (`posed_placement_keys`); it "
                "positions and draws nothing - the native draw list with its "
                "lifts is `field_render/placements.rs`",
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
                "`legaia_scene_formats::field_objects::Placement::rot_x` doc) - the "
                "yaw-only path is not a shortcut there, it is the data",
            "site/js/world-overview-app.js":
                "consumer of the same walk-placement accessors; see above",
        },
    },
    {
        "kernel": "move-VM part rotation through gte::part_rotation",
        "why": "the per-actor dispatcher turns a move-VM node by its angle "
        "trio with `FUN_80026988` (`Rx * Ry * Rz`), and the battle frame's "
        "per-model flip conjugates it. Both hosts once composed "
        "`Ry * Rx * Rz` on unflipped angles, which agrees with retail only "
        "while at most the yaw is non-zero: Gimard's fire tunnel turned "
        "about two axes drew at another angle on both",
        "trigger": r"\b(?:active_summon_part_draws|active_effect_kind4_draws)\b",
        "requires": [r"\bpart_rotation\b"],
    },
    {
        "kernel": "shared value layout -> shared quad emitter",
        "why": "a shared LAYOUT is not a shared DRAW. The battle damage "
        "numerals and the `N HIT` / `TOTAL` counter resolved through one "
        "kernel (`engine-battle-vm::battle_value_readout`) while the native window "
        "sampled retail's 24x24 cells out of VRAM and the browser restyled "
        "the same digits in the dialog font - one model, two letterforms, "
        "and every tier green because both hosts named the kernel. A surface "
        "that resolves the layout must emit through the shared quad builder "
        "(`engine-ui::battle_numerals`), leaving a font path as an explicit "
        "before-the-atlas fallback rather than as the draw. The emit is one "
        "function now (`engine-screens::battle_value_readout::"
        "value_readout_prims`), so a host satisfies the rule by calling it",
        # A `mod battle_value_readout;` line declares the module; it resolves
        # nothing.
        "trigger": r"(?<!mod )\bbattle_value_readout\b",
        "requires": [
            r"\b(?:battle_numerals|value_readout_prims)\b",
            r"\b(?:digit_run_prims|combo_cluster_prims|digit_quad|readout_quad|value_readout_prims)\b",
        ],
    },
    {
        "kernel": "world-map markers through the shared quad kernel",
        "why": "the overworld entity / player markers were native-only for as "
        "long as the native window drew them as world-space lines through a "
        "pipeline the browser has no counterpart for. They are now one-pixel "
        "quads out of `engine-world-views::world_map_markers::marker_quads`, wrapped "
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
    {
        "kernel": "battle screen FX through one placement and projection",
        "why": "the weapon trail, the move-FX streak (and its ribbons) and "
        "the Theeder beam are packets from shared kernels, and for as long as "
        "each host built the camera they project through, the two drifted: "
        "the native window projected the streak without the battle world "
        "scale and drew it as an opaque scene mesh, the page as an additive "
        "screen primitive. The camera, the scale and the prim wrap are one "
        "function now (`engine-screens::battle_fx`); a surface that reads "
        "the raw battle-FX seams must be that function",
        "trigger": r"\b(?:theeder_draw|battle_weapon_trail_draws|move_fx_streak_quads|clip_ribbon_quads)\s*\(",
        "requires": [r"\bbattle_fx_vp\b"],
        "exempt": {
            "crates/engine-world-views/src/screen_passes.rs":
                "the kernels themselves (`weapon_trail_prims`, "
                "`move_fx_streak_quads`) take the view-projection as an "
                "argument; `engine-screens::battle_fx` is their one caller "
                "that builds it",
            "crates/web-play/src/play_battle/test_probes.rs":
                "counts the draws (`debug_battle_*` probes for the battle "
                "ladder) and projects or draws nothing",
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
