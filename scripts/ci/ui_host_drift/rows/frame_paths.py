"""Tier 11 rows: each host's frame path, the names on it that are not
kernels, and the page-side frame function.

Analysis: `ui_host_drift.tiers.frame_paths`.
"""

from ui_host_drift.rows.host_files import WEB_RUNTIME_FRAME
from ui_host_drift.rows.sim_pairs import NATIVE_REDRAW_STEP_FILES

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
# window's per-tick body `sim_tick`, the browser runtime's `tick_frame` - and
# both paths short-circuit. The native body `return`s out of several arms (the
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
        "path": NATIVE_REDRAW_STEP_FILES[0],
        # The window's per-tick body. `handle_redraw` runs it up to four
        # times per rendered frame (the catch-up drain, `run_frame_ticks`)
        # and then draws once, so every `return` here returns to a frame
        # that still draws. Its step helpers are spliced back in
        # (`splice_native_steps`), so the kernels they call read as the
        # body's own.
        "anchor": "fn sim_tick(&mut self) -> bool",
        "exit": "return",
        "splice": True,
    },
    "web": {
        "path": WEB_RUNTIME_FRAME,
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


# The browser host has a SECOND frame path, and it is not Rust. The page's
# `_frame` runs its ticks (`this._frameTicks`, over the engine's
# `play_frame_ticks`, which calls `tick_frame`) inside a guard and calls the
# overlay draw outside it, so a guarded frame runs no kernel at all and still paints -
# every kernel in `tick_frame`, at once, from whatever answer it last held.
# A Rust-only scan is blind to this: `tick_frame` has no such arm, which is
# how "the browser page has no early-out" came to be written down.
WEB_PAGE_FRAME = "site/js/play-view-frame.js"
WEB_PAGE_FRAME_FN = "_frame(skipDraw)"
WEB_PAGE_TICK_CALL = "this._frameTicks(rt, simSteps)"
WEB_PAGE_DRAW_CALL = "this._drawOverlay()"
