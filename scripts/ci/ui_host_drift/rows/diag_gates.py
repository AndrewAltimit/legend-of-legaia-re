"""Tier 6 rows: every `LEGAIA_DIAG_*` gate and the roots scanned for them.

Analysis: `ui_host_drift.tiers.diag_gates`.
"""

from ui_host_drift.roots import REPO

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
            "the gate lives in the shared engine-ui leaf (`battle_hud::"
            "diag_hud_enabled`), so it is one implementation both hosts call "
            "rather than a per-host twin. `std::env::var` answers Err under "
            "wasm32, which makes the browser default-off by construction."
        ),
        "note": "battle-event log + pose/HP diagnostic text over the frame",
    },
    # --- subtractive / logging only ----------------------------------------
    {"env": "LEGAIA_DIAG_CAST_PRES", "additive": False, "note": "print one stderr line per cast-module arm that fires presentation calls; draws nothing"},
    {"env": "LEGAIA_DIAG_DRAWS", "additive": False, "note": "write the frame's draw census + CPU VRAM to a file; draws nothing"},
    {"env": "LEGAIA_DIAG_DRAW_TRIS", "additive": False, "note": "beside LEGAIA_DIAG_DRAWS, write one census family per triangle; draws nothing"},
    {"env": "LEGAIA_DIAG_CAPTURE_TICK", "additive": False, "note": "take a gated --screenshot capture at a chosen world tick instead of on its gate; draws nothing"},
    {"env": "LEGAIA_DIAG_FOLLOW_KNOBS", "additive": False, "note": "pose the follow camera's orbit / tilt / zoom knobs as a mouse drag would, for screenshot runs; draws nothing retail-foreign"},
    # --- lane B2 ---
    {"env": "LEGAIA_DIAG_INTRO_CAPTURE", "additive": False, "note": "write the field frame the battle intro captured as a PNG; draws nothing"},
    {"env": "LEGAIA_DIAG_CAST_PRES", "additive": False, "note": "print one stderr line per cast-module tick whose arm has presentation sites; draws nothing"},
    {"env": "LEGAIA_DIAG_CAPTURE_STATE", "additive": False, "note": "print the battle state on the --screenshot frame; draws nothing"},
    {"env": "LEGAIA_DIAG_CAST_SEQ", "additive": False, "note": "record a cast's frames + per-tick metadata for the cast-sequence oracle; draws nothing"},
    {"env": "LEGAIA_DIAG_CAST_SEQ_EVERY", "additive": False, "note": "LEGAIA_DIAG_CAST_SEQ frame cadence; draws nothing"},
    {"env": "LEGAIA_DIAG_CAST_SEQ_MAX", "additive": False, "note": "LEGAIA_DIAG_CAST_SEQ frame cap; draws nothing"},
    {"env": "LEGAIA_DIAG_CAST_SEQ_SEAT", "additive": False, "note": "LEGAIA_DIAG_CAST_SEQ caster seat filter; draws nothing"},
    {"env": "LEGAIA_DIAG_CAST_SEQ_TAIL", "additive": False, "note": "LEGAIA_DIAG_CAST_SEQ ticks past the Done band; draws nothing"},
    {"env": "LEGAIA_DIAG_CAPTURE_VRAM", "additive": False, "note": "write the battle VRAM on the --screenshot frame; draws nothing"},
    {"env": "LEGAIA_DIAG_FX_ALL", "additive": False, "note": "log every effect sprite's projected centre each frame; draws nothing"},
    {"env": "LEGAIA_DIAG_NOFX", "additive": False, "note": "suppress the effect layer"},
    {"env": "LEGAIA_DIAG_NO_GHOSTS", "additive": False, "note": "suppress the battle after-image ghost pass (A/B attribution)"},
    {"env": "LEGAIA_DIAG_NO_PRIM_NEAR", "additive": False, "note": "disarm the per-primitive near reject (A/B attribution; the browser never disarms it)"},
    {"env": "LEGAIA_DIAG_NOSEMI", "additive": False, "note": "semi-transparent blend off"},
    {"env": "LEGAIA_DIAG_NO_LIT_ROWS", "additive": False, "note": "leave the light-source rows at the neutral texel (A/B attribution; the browser always shades them)"},
    {"env": "LEGAIA_DIAG_NO_DECO_CUE", "additive": False, "note": "drop the overworld decoration depth cue (before/after frames)"},
    {"env": "LEGAIA_DIAG_LAYERS", "additive": False, "note": "draw only the named layers"},
    {"env": "LEGAIA_DIAG_PLACE_RANGE", "additive": False, "note": "draw only placements [a,b)"},
    {"env": "LEGAIA_DIAG_MESHTEX", "additive": False, "note": "mesh/texture bind log"},
    {"env": "LEGAIA_DIAG_PLACE", "additive": False, "note": "placement-resolve log"},
    {"env": "LEGAIA_DIAG_POSE", "additive": False, "note": "per-actor pose/AABB log"},
    {"env": "LEGAIA_DIAG_CREATURE_ANIM", "additive": False, "note": "per-tick summon creature clip / rate / keyframe log; draws nothing"},
    {"env": "LEGAIA_DIAG_CAMERA", "additive": False, "note": "camera-solve log"},
    {"env": "LEGAIA_DIAG_CUTCAM", "additive": False, "note": "cutscene-camera log"},
    {"env": "LEGAIA_DIAG_BATCAM", "additive": False, "note": "battle-camera log"},
    {"env": "LEGAIA_DIAG_BATDRAW", "additive": False, "note": "battle draw-list log"},
    {"env": "LEGAIA_DIAG_TIMELINE", "additive": False, "note": "narration-timeline log"},
    {"env": "LEGAIA_DIAG_MEI", "additive": False, "note": "test-only NPC-entry log"},
]


# Roots swept for gate *declarations*. Tests are included deliberately: a gate
# introduced in a test still has to be declared, because the next wave will
# reach for it from the engine.
DIAG_ROOTS = [
    REPO / "crates" / "engine-shell",
    REPO / "crates" / "engine-render",
    REPO / "crates" / "engine-core",
    REPO / "crates" / "engine-menu-runtime",
    REPO / "crates" / "engine-scene-host",
    REPO / "crates" / "engine-world-views",
    REPO / "crates" / "engine-minigames",
    REPO / "crates" / "engine-fishing",
    REPO / "crates" / "engine-effects",
    REPO / "crates" / "engine-field",
    REPO / "crates" / "engine-scene",
    REPO / "crates" / "engine-menus",
    REPO / "crates" / "engine-title",
    REPO / "crates" / "engine-battle-menus",
    REPO / "crates" / "engine-dialog",
    REPO / "crates" / "engine-system",
    REPO / "crates" / "engine-ui",
    REPO / "crates" / "render-kernels",
    REPO / "crates" / "engine-vm",
    REPO / "crates" / "engine-battle-vm",
    REPO / "crates" / "engine-cast",
    REPO / "crates" / "engine-motion-vm",
    REPO / "crates" / "web-play",
    REPO / "crates" / "web-minigames",
    REPO / "crates" / "web-viewer",
    REPO / "crates" / "web-patcher",
    REPO / "crates" / "parity",
]
