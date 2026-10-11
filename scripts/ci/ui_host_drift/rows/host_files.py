"""Host source files the declared rows name.

A row cites a file through one of these constants, so a moved or split Rust
file is one edit here. `ui_host_drift.path_check` fails the gate when a path
named in any rows module is not on disk.
"""

# Host source files the paired-constant check reads. Named here rather than
# discovered, because a pair is a claim about two specific declarations.
NATIVE_WINDOW = "crates/engine-shell/src/window.rs"
NATIVE_HUD = "crates/engine-shell/src/window/hud.rs"
NATIVE_DEV_MENU = "crates/engine-shell/src/window/dev_menu.rs"
WEB_PLAY_DEV_MENU = "crates/web-viewer/src/play_dev_menu.rs"
WEB_PLAY_MENU = "crates/web-viewer/src/play_menu.rs"
WEB_PLAY_MENU_INPUT = "crates/web-viewer/src/play_menu/input.rs"
WEB_PLAY_MENU_OPEN_CLOSE = "crates/web-viewer/src/play_menu/open_close.rs"
WEB_PLAY_MENU_SUB_SCREEN_DRAWS = "crates/web-viewer/src/play_menu/sub_screen_draws.rs"
WEB_PLAY_SHOP = "crates/web-viewer/src/play_shop.rs"
WEB_RUNTIME = "crates/web-viewer/src/runtime.rs"
WEB_RUNTIME_DISC_LOAD = "crates/web-viewer/src/runtime/disc_load.rs"
WEB_RUNTIME_FRAME = "crates/web-viewer/src/runtime/frame.rs"
WEB_RUNTIME_SCENE_SETUP = "crates/web-viewer/src/runtime/scene_setup.rs"
# The occlusion-fade tunables are the one paired set whose web half is a
# plain script rather than a wasm crate - the browser play page holds them
# in its GLSL module. `const NAME = <value>;` parses identically either way,
# which is what lets one checker cover both.
NATIVE_OCCL = "crates/engine-render/src/occlusion_fade.rs"
WEB_SHADERS = "site/js/webgl-shaders.js"
NATIVE_PSX_DITHER = "crates/engine-render/src/psx_dither.rs"
NATIVE_RENDER_STATE = "crates/engine-render/src/renderer/state.rs"
# The enhanced-lighting model's constants live in the shared kernel; the
# moods themselves reach the page from the engine (`play_lighting_frame`),
# so only the shading-law constants have a hand-written JS twin.
NATIVE_DYN_LIGHT = "crates/render-kernels/src/scene_lighting/mood.rs"
NATIVE_DYN_WINDOWS = "crates/render-kernels/src/scene_lighting/windows.rs"


NATIVE_BOOT = "crates/engine-session/src/boot.rs"
NATIVE_BOOT_FRAME_TICK = "crates/engine-session/src/boot/frame_tick.rs"
NATIVE_BOOT_SCENE_ENTRY = "crates/engine-session/src/boot/scene_entry.rs"
NATIVE_SAVE_HELPERS = "crates/engine-shell/src/window/save_rack_disk.rs"
NATIVE_ASSETS = "crates/engine-shell/src/window/assets.rs"
# The actor stage of the scene asset upload (`upload_assets`): the player's
# rig and the MAN-placed field NPCs.
NATIVE_ASSETS_ACTORS = "crates/engine-shell/src/window/assets/actor_binding.rs"
# The Muscle Dome session enter / exit / tick methods of the frame tick.
NATIVE_MINIGAME_SESSIONS = (
    "crates/engine-core/src/world/frame_tick/minigame_sessions/muscle_dome.rs"
)
NATIVE_BOOT_CUTSCENE = "crates/engine-shell/src/window/boot_cutscene.rs"
NATIVE_REDRAW = "crates/engine-shell/src/window/event_handler/redraw.rs"
NATIVE_FIELD_RENDER = "crates/engine-shell/src/window/field_render.rs"
NATIVE_GEOMETRY = "crates/engine-shell/src/window/geometry.rs"
WEB_BOOT_TITLE = "crates/web-viewer/src/boot_title.rs"
WEB_MINIGAMES_MUSCLE = "crates/web-viewer/src/minigames_muscle/exports.rs"
WEB_PLAY_BATTLE = "crates/web-viewer/src/play_battle.rs"
WEB_PLAY_BATTLE_SCREEN_PRIMS = "crates/web-viewer/src/play_battle/screen_prims.rs"
WEB_PLAY_BATTLE_SESSION_EXPORTS = "crates/web-viewer/src/play_battle/session_exports.rs"
WEB_PLAY = "crates/web-viewer/src/play.rs"
WEB_PLAY_PLAYER_MESH = "crates/web-viewer/src/play/player_mesh.rs"
WEB_PLAY_TERRAIN_GROUND = "crates/web-viewer/src/play/terrain_ground.rs"
NATIVE_REDRAW_PASSES = (
    "crates/engine-shell/src/window/event_handler/redraw_passes.rs"
)
NATIVE_CAMERA_MOD = "crates/engine-shell/src/window/camera.rs"
WEB_PLAY_CAMERA = "crates/web-viewer/src/play_camera.rs"
NATIVE_KEYBOARD = "crates/engine-shell/src/window/event_handler/keyboard.rs"
# `handle_key`'s arms, one method per key, by family.
NATIVE_KEYBOARD_DEBUG_SPAWN = (
    "crates/engine-shell/src/window/event_handler/keyboard/debug_spawn_keys.rs"
)
NATIVE_KEYBOARD_MINIGAME = (
    "crates/engine-shell/src/window/event_handler/keyboard/minigame_keys.rs"
)
NATIVE_KEYBOARD_OPTION = (
    "crates/engine-shell/src/window/event_handler/keyboard/option_keys.rs"
)
NATIVE_RUN = "crates/engine-shell/src/window/run.rs"
NATIVE_BATTLE = "crates/engine-shell/src/window/battle.rs"
WEB_PLAY_ARENA = "crates/web-viewer/src/play_minigame_arena.rs"
WEB_PLAY_FISHING = "crates/web-viewer/src/play_fishing.rs"
WEB_FIELD_SCENE = "crates/web-viewer/src/field_scene.rs"
NATIVE_MINIGAMES = "crates/engine-shell/src/window/minigames.rs"
WEB_MINIGAMES_DANCE = "crates/web-viewer/src/minigames_dance.rs"
WEB_PLAY_ARENA_SURFACE = "crates/web-viewer/src/play_minigame_arena.rs"
NATIVE_TITLE_SAVE = (
    "crates/engine-shell/src/window/title_save_draws.rs"
)
