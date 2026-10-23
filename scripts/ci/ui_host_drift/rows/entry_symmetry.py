"""Tier 10 rows: the native hotkey sources, the roots that count as a shared
caller, and the hotkey-only waivers.

Analysis: `ui_host_drift.tiers.entry_symmetry`.
"""

from ui_host_drift.roots import REPO

# --------------------------------------------------------------------------
# Tier 10 - entry symmetry: is a World phase armed only from a debug hotkey?
# --------------------------------------------------------------------------
#
# The dance minigame's pre-song count-in and the Disco King how-to actor were
# both complete shared kernels, and the native window drove them - from a
# count-in driver of its own, reached only by the `K` / `U` hotkeys. The
# player-reachable entry is the mode-24 door warp, which the shared scene host
# drains into `World::enter_dance`, so NEITHER host counted in when a player
# walked into the hall.
#
# A gap that reads as "one host has it" can turn out to be "one host's debug
# path has it", and no tier above can tell those apart, because both end at a
# live call site. This one asks a different question: for every `World`
# method the native window's keyboard arms call, is there a call site anywhere
# outside the native window - the shared engine, or the browser hosts?
# If not, the phase exists in the engine and only a key press reaches it.
#
# Derived over the hotkey sources, so a new hotkey arm joins the measurement
# by existing. A debug-only spawner is a legitimate answer and takes a waiver;
# what the waiver may not say is "not wired yet".
#
# Scope:
#
# * it DOES prove every `world.<method>()` the hotkey arms call has (or does
#   not have) a call site in `SHARED_CALLER_ROOTS` (outside the native window);
# * it does NOT prove that site is player-reachable, that it passes the same
#   arguments, or that a phase with no hotkey arm at all is wired.

# The native window's key-driven arms. `--key-script` delivers scripted keys
# through these same handlers, which is why they are the tier's denominator.
HOTKEY_SOURCES = [
    "crates/engine-shell/src/window/event_handler/keyboard.rs",
    "crates/engine-shell/src/window/event_handler/keyboard/debug_spawn_keys.rs",
    "crates/engine-shell/src/window/event_handler/keyboard/minigame_keys.rs",
    "crates/engine-shell/src/window/event_handler/keyboard/option_keys.rs",
    "crates/engine-shell/src/window/minigames.rs",
    "crates/engine-shell/src/window/minigames/baka_fighter.rs",
    "crates/engine-shell/src/window/minigames/dance.rs",
    "crates/engine-shell/src/window/minigames/fishing.rs",
    "crates/engine-shell/src/window/minigames/muscle_dome.rs",
    "crates/engine-shell/src/window/minigames/slot_machine.rs",
]

# Where a shared (i.e. not hotkey-only) caller may live: the engine crates the
# browser hosts share, and the browser hosts themselves.
SHARED_CALLER_ROOTS = [
    REPO / "crates" / "engine-core" / "src",
    REPO / "crates" / "engine-menu-runtime" / "src",
    REPO / "crates" / "engine-scene-host" / "src",
    REPO / "crates" / "engine-world-views" / "src",
    REPO / "crates" / "engine-minigames" / "src",
    REPO / "crates" / "engine-fishing" / "src",
    REPO / "crates" / "engine-effects" / "src",
    REPO / "crates" / "engine-field" / "src",
    REPO / "crates" / "engine-scene" / "src",
    REPO / "crates" / "engine-menus" / "src",
    REPO / "crates" / "engine-title" / "src",
    REPO / "crates" / "engine-battle-menus" / "src",
    REPO / "crates" / "engine-dialog" / "src",
    REPO / "crates" / "engine-system" / "src",
    REPO / "crates" / "engine-vm" / "src",
    REPO / "crates" / "engine-battle-vm" / "src",
    REPO / "crates" / "engine-cast" / "src",
    REPO / "crates" / "engine-motion-vm" / "src",
    REPO / "crates" / "engine-ui" / "src",
    REPO / "crates" / "render-kernels" / "src",
    REPO / "crates" / "web-play" / "src",
    REPO / "crates" / "web-minigames" / "src",
    REPO / "crates" / "web-viewer" / "src",
    REPO / "crates" / "web-patcher" / "src",
]

# A hotkey-only `World` method, with the reason it is one. Each entry is
# validated in both directions: a name that is no longer hotkey-called, or has
# gained a shared caller, fails as stale.
HOTKEY_ONLY_WAIVERS: dict[str, str] = {
    "spawn_debug_effect": "an effect-pool probe with no retail counterpart - "
    "it seats a marker billboard at the player so the pool's ageing can be "
    "watched. Retail spawns effects from the move VM; nothing about this arm "
    "belongs on a player-reachable entry, on either host.",
    "spawn_debug_effect_model": "the model-emitting twin of the probe above, "
    "same reasoning: it exists to prove the pool emits a model rather than a "
    "billboard for a chosen TMD index.",
    "active_field_fx_render_nodes": "a read accessor the hotkey uses for its "
    "own on-screen readout of what the pool currently holds; it arms no phase "
    "and changes no world state.",
    "spawn_field_stager": "superseded, not unwired: the production op-0x34 "
    "sub-3 route is `World::spawn_ambient_record_at` (the full `FUN_80021B04` "
    "port with the op-0x25 fan-out, the CLUT-cell integrator and the VDF "
    "morph envelope), reached from `vm_hosts::effect_anim_trigger` on both "
    "hosts. This entry stages the same record id into the older "
    "`SummonScene` pool and is kept as the native window's probe over it.",
    "spawn_summon": "superseded, not unwired: the production producer of "
    "`World::casting.active_summon` is the battle cast band "
    "(`world/battle/cast_band.rs`, `SummonScene::spawn_parts`), and the "
    "faithful player-summon render is the battle-actor TRS-keyframe path in "
    "`legaia_engine_vm::anim_vm`. This entry parses a chosen stager overlay "
    "instead and exists for the native window's summon probe.",
}

# The other half: a phase that SHOULD have a shared entry and does not yet.
# Validated exactly as `blocked_on` is elsewhere - an entry that goes clean
# FAILS, demanding the marker be deleted - so a pending wire cannot rot into a
# permanent exemption. What an entry may not say is "not wired yet": it names
# the shared caller that is missing.
HOTKEY_ONLY_BLOCKED: dict[str, str] = {
    # Empty today. Two rows were drafted here and moved to the waivers above
    # once their production route was read: both entries are superseded
    # stand-ins with a live replacement, not phases awaiting a wire. That is
    # the distinction this dict exists to keep - "the shared caller is
    # missing" is a different claim from "the shared caller is elsewhere",
    # and only the first one is pending work.
}
