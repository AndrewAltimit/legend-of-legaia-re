//! Retail cumulative-XP threshold table (and the test placeholder table).
//!
//! Extracted verbatim from `levelup.rs`.

#[cfg(test)]
use super::MAX_LEVEL;

/// Cumulative XP thresholds for levels 2..=`MAX_LEVEL` (the base / slot-0
/// curve).
///
/// `table[i]` = total XP required to reach level `i + 2` (from level 1):
/// `121, 365, 730, 1338, 2190, …, 9_646_483`.
///
/// The values are **derived** from the retail level-up applier `FUN_801E9504`
/// (called from the battle reward resolver `FUN_8004E568` at `0x8004F34C`):
/// the static-SCUS per-level u16 delta table `DAT_80076AF4` is exactly the
/// closed form `delta(n) = n²/4 + 1`, and the running sum is scaled
/// `(sum × 9_999_999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`). The
/// single source of truth is [`legaia_save::RETAIL_XP_CUMULATIVE`]; this
/// helper adapts it to the `Vec<u32>` shape `LevelUpTracker` consumes.
///
/// `BootSession` still installs the disc-parsed curve at boot
/// (`legaia_asset::level_up_tables::xp_thresholds_from_scus`) - byte-identical
/// to this table, kept as a live cross-validation - and the slots-1/2
/// (Noa/Gala) ± sin-divisor threshold corrections via
/// [`LevelUpTracker::with_xp_corrections`]. Validated against the record
/// `+0x4` next-level-threshold field across the save-state library (New Game
/// Vahn L1 = 121 as the Status menu shows; L37 slot 0 = 535_546; L99 caps).
/// See [`docs/subsystems/level-up.md`](https://github.com/altimit-mii/legend-of-legaia-re/blob/main/docs/subsystems/level-up.md#xp-table).
///
/// [`LevelUpTracker::default`]: super::LevelUpTracker
/// [`LevelUpTracker::with_xp_corrections`]: super::LevelUpTracker::with_xp_corrections
// PORT: FUN_801E9504 (XP-threshold derivation, base curve)
pub fn retail_xp_table() -> Vec<u32> {
    legaia_save::RETAIL_XP_CUMULATIVE.to_vec()
}

/// Geometric `100 × n²` approximation - used only in unit tests that need
/// fixed threshold values independent of the retail data.
#[cfg(test)]
pub fn placeholder_xp_table() -> Vec<u32> {
    (1u32..MAX_LEVEL as u32).map(|n| 100 * n * n).collect()
}
