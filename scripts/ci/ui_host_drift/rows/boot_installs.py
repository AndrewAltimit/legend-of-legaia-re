"""Tier 13 rows: the native boot source and the page's boot root.

Analysis: `ui_host_drift.tiers.boot_installs`.
"""

from ui_host_drift.roots import REPO

# ---------------------------------------------------------------------------
# Tier 13 - boot installs: does the page's disc load install every World
# table the native boot does?
#
# The native boot (`crates/engine-session/src/session.rs`) installs disc tables
# onto the world one call at a time, and the page's `load_disc` repeats the
# list by hand. Nothing failed when the page's copy fell behind: every
# consumer has a disc-free fallback, so six progression tables (XP curve +
# divisors, stat growth, victory pose, XA cue durations, magic-XP thresholds,
# accessory passives) were missing on the page for as long as nobody read the
# two lists side by side - placeholder growth, silent melee grunts, summons
# that never level, and an RNG stream that diverged after every victory. None
# of tiers 1-12 look at a boot. This one takes every `world.install_*` /
# `world.set_*` call in the native boot source and requires the page's
# shipped sources to call the same method, or a `[[boot_install]]` waiver
# with a reason. The cure for the six was one engine entry both boots call
# (`World::install_retail_progression_tables`), which is also what keeps this
# list short.
#
# The page now builds the native session type over its host
# (`BootSession::from_host`), so every install inside `from_host` runs on the
# page too and counts as the page's: a page source that calls `from_host(`
# inherits that body's installs. The ones outside it (`enter_field_live`'s
# catalogs, ...) still need a page call of their own.
# ---------------------------------------------------------------------------

BOOT_NATIVE = REPO / "crates/engine-session/src/session.rs"
BOOT_WEB_ROOTS = (REPO / "crates/web-play/src", REPO / "crates/web-minigames/src")
