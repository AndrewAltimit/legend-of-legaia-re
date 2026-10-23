"""The declared rows: paths and accepted exceptions a contributor edits.

`SPLIT_MODULES` names Rust directories that are one module split across
files, and `DEFINING_DOC_ROOTS` the doc trees whose rows define a function.
A moved directory is one edit here; `port_provenance.path_check` stops the
sweep when a path named here is not on disk.
"""

from __future__ import annotations

# The three doc roots that say what a routine *is*. Everything else under
# `docs/` answers a different question about it: `docs/tooling/` describes the
# instruments, `docs/guides/` the tasks, and the thread ledgers
# (`re-settled-threads.md`, `re-do-not-re-walk.md`, `open-rev-eng-threads.md`)
# record which questions are answered and which readings are falsified. A
# falsification titled "X is *not* Y" names an address without claiming it, so
# it is not a second label - `dual-label` only compares defining pages.
DEFINING_DOC_ROOTS = (
    "docs/subsystems/",
    "docs/formats/",
    "docs/reference/functions/",
)


# ---------------------------------------------------------------------------
# Port tags
# ---------------------------------------------------------------------------


# Modules split into a directory for size (`foo.rs` + `foo/*.rs`) after their
# findings and waivers were keyed. A split is a move, not a new module: every
# file of one keeps one sibling set and one finding key, the parent file's, so
# `module-orphan` sees the siblings it saw before the move and a waiver keyed on
# the old path keeps matching. Only the displayed `file:line` names the child.
SPLIT_MODULES = frozenset({
    "crates/disc-audit/src/byte_account",
    "crates/engine-battle-vm/src/battle_cam_script",
    "crates/engine-minigames/src/muscle_dome",
    "crates/engine-minigames/src/dance",
    "crates/engine-menus/src/save_select",
    "crates/delilas-party/src/delilas_party",
    "crates/engine-cast/src/cast_module_ticks",
    "crates/engine-fishing/src/fishing",
    "crates/engine-title/src/title_overlay",
    "crates/web-minigames/src/muscle_web",
    "crates/engine-core/src/world/vm_hosts",
    "crates/engine-core/src/world/narration",
    "crates/engine-core/src/world/field_movement",
    "crates/engine-core/src/world/battle/cast_band",
    "crates/engine-core/src/world/frame_tick",
    "crates/engine-battle-hud/src",
    "crates/engine-minigames/src/baka_fighter",
    "crates/parity/src/retail_compare_battle",
    "crates/engine-menus/src/pause_screens",
    "crates/engine-menus/src/shop",
    "crates/engine-menus/src/save_menu_atlas",
    "crates/engine-menus/src/equip_session",
    "crates/engine-menus/src/save_subscreen",
    "crates/engine-dialog/src/dialog",
    "crates/engine-mode/src/mode",
    "crates/engine-field/src/camera",
    "crates/engine-field/src/world_map_panel_host",
    "crates/engine-effects/src/summon",
    "crates/engine-effects/src/screen_fx",
    "crates/engine-cast/src/cast_arm_ticks",
    "crates/engine-cast/src/seru_magic_ticks",
    "crates/engine-battle-vm/src/battle_attack_camera",
    "crates/engine-motion-vm/src/ambient_motion",
    "crates/engine-motion-vm/src/motion_vm",
    "crates/engine-field/src/submode_hub_actors",
    "crates/engine-vm/src/actor_tick",
    "crates/engine-field/src/world_map_panel_actors",
    "crates/engine-minigames/src/slot_machine",
    "crates/engine-minigames/src/baka_fighter_chrome",
    "crates/engine-minigames/src/baka_cabinet",
    "crates/engine-fishing/src/fishing_actors",
    "crates/engine-minigame-scenes/src/muscle_dome_scene",
    "crates/render-kernels/src/scene_lighting",
    "crates/render-kernels/src/battle_intro",
    "crates/engine-ui/src/battle_hud",
    "crates/engine-ui/src/fishing/hud",
    "crates/engine-ui/src/menu/window_painters",
    "crates/engine-audio/src/sequencer",
    "crates/engine-render/src/shaders",
    "crates/engine-render/src/renderer/render",
})


# Every citation this test rescued, for the report's own accounting: a
# silent acceptance is indistinguishable from a signal that stopped
# working, which is the failure mode a checker in this repo has already
# had (`check-port-tags.py`'s "0 warnings across 0 files").
SITE_ACCEPTED: list[tuple[str, str, str]] = []


# Tags the `non-entry` test let through because they name an image whose own
# bytes open a routine at the VA - reported by count, for the same reason
# SITE_ACCEPTED is: a silent acceptance reads like a signal that stopped.
IMAGE_ENTRY_ACCEPTED: list[tuple[str, str, str]] = []
