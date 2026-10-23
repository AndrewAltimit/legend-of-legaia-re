"""Where the gate reads: the repository root, the shared UI crates, each
host's source roots and the waiver file.
"""

from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
UI_SRC = REPO / "crates" / "engine-ui" / "src"
# engine-ui plus the render kernels it builds on.
UI_SRCS = [UI_SRC, REPO / "crates" / "render-kernels" / "src"]
WAIVERS = Path(__file__).resolve().parent.parent / "ui-host-drift-waivers.toml"

# Source roots per host. `engine-render` counts as native: it wraps some
# engine-ui builders in GPU-resident batches, so a call there is still the
# native window reaching the screen.
HOSTS = {
    "native": [
        REPO / "crates" / "engine-shell" / "src",
        REPO / "crates" / "engine-render" / "src",
    ],
    "web": [
        REPO / "crates" / "web-play" / "src",
        REPO / "crates" / "web-minigames" / "src",
        REPO / "crates" / "web-viewer" / "src",
        REPO / "crates" / "web-patcher" / "src",
    ],
}

# Shared **composition** crates: engine code that projects engine-core state
# into engine-ui builders on both hosts' behalf, so a builder may have its only
# call site there. Each maps its source root to the entry points a host calls.
# A builder named in the crate's (non-test) source is credited to exactly the
# hosts that call one of those entries - so a host that stops calling the
# entry loses every builder behind it, and the reachability claim stays a
# claim about each host rather than about the crate.
SHARED_COMPOSERS: dict[Path, tuple[str, ...]] = {
    REPO / "crates" / "engine-screens" / "src": (
        "shop_overlay_frame",
        "battle_hud_frame_draws",
        "battle_chrome_sprite_draws",
        "field_menu_root_draws",
        "field_menu_sub_draws",
        "battle_text_draws",
        "dialog_box_chrome_draws",
        "dialog_box_text_draws",
        "field_party_hud_draws",
        "save_select_overlay_draws",
        # --- lane B ---
        "battle_value_readout_layout",
        "fishing_hud_items",
        "fishing_line_screen_prims",
        "fishing_exchange_draws",
        "passive_hud_text_draws",
        # --- end lane B ---
    ),
}
