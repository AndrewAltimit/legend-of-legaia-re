"""Tier 5 rows: which site sources drive the pad, and the DOM codes that are
not pad input.

Analysis: `ui_host_drift.tiers.page_keys`.
"""

from ui_host_drift.roots import REPO

# --------------------------------------------------------------------------
# Tier 5 - no page-side keyboard table
# --------------------------------------------------------------------------
#
# The three hosts share one keyboard layout, served out of the engine by
# `pad_bindings_json` (`legaia_engine_system::input::Mapping::web_default`).
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


# The one site source that may read the Gamepad API. A controller binds
# buttons to buttons through the engine's layout
# (`legaia_engine_system::gamepad`, served by `gamepad_layout_json`, the table
# the native window folds gilrs through); this file adopts it, and every page
# reads controllers through its `legaiaPollGamepad` / `legaiaGamepadEdges`.
GAMEPAD_ADOPTER = SITE_ROOT / "js" / "pad-bindings.js"


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
