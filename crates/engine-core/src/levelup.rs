//! Post-battle level-up tracker.
//!
//! Tracks cumulative XP per party slot and checks against configurable
//! per-level thresholds. On a level-up the tracker returns a [`LevelUpResult`]
//! whose HP / MP gains are applied to the character's [`legaia_save::CharacterRecord`]
//! via typed setters.
//!
//! ## XP table provenance
//!
//! [`retail_xp_table`] ships the **derived retail base curve** (`121, 365,
//! 730, …, 9_646_483`): the static-SCUS per-level u16 delta table
//! `DAT_80076AF4` is the closed form `delta(n) = n²/4 + 1`, read by the
//! overlay level-up applier `FUN_801E9504` (called from the reward resolver
//! `FUN_8004E568` at `0x8004F34C`) - the running sum to the current level is
//! scaled `(sum × 9_999_999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`)
//! and compared `≤ record cumulative XP` (`+0x0`). The same curve is parsed
//! off the disc by `legaia_asset::level_up_tables::xp_thresholds_from_scus`
//! and **installed at boot** by `legaia_engine_shell::BootSession` (which
//! reads the user's `SCUS_942.54`) over [`LevelUpTracker::xp_table`] - a
//! byte-identical live cross-validation of the derived constants. Slots 1/2
//! (Noa/Gala) shift each threshold by the ± sin-divisor correction
//! ([`LevelUpTracker::with_xp_corrections`]). An earlier pass shipped a GTE
//! sin-LUT slice (`50, 56, 62, …`) mis-extracted from "`0x8007123C`" as the
//! curve - refuted by the retail Status-menu capture (New Game "Next Level
//! 121") and library-wide record `+0x4` sampling. See
//! `docs/subsystems/level-up.md` § XP table.
//!
//! Per-character HP/MP growth is wired from the same static-SCUS tables:
//! per-stat curves at `DAT_800769CC` (stride `0x62`) + the per-character
//! parameter block at `DAT_80076918` (`{u16 start, u16 max, u8 jitter, u8 row}`
//! sub-records), read by the same `FUN_801E9504` (not the falsified Seru
//! `+0x74` path). [`LevelUpTracker::with_growth_tables`] installs the validated
//! jitter-free per-level core (`(max-start) × curve[row][level-1] / 0x24C0`,
//! min 1) for all eight stats (HP, MP, AGL, ATK, UDF, LDF, SPD, INT);
//! `BootSession` calls it from the user's `SCUS_942.54`. The flat 10 HP / 5 MP
//! [`StatGain`] default is only the disc-less fallback. Retail's per-level
//! `rand()` jitter is modeled as an **opt-in** layer
//! ([`LevelUpTracker::with_level_up_jitter`] + the [`BiosRand`] LCG); it is off
//! by default so it draws no `rand()` and replays stay bit-identical - see
//! `docs/subsystems/level-up.md` § Stat gains.

use legaia_save::CharacterRecord;

/// Maximum party size tracked by this module.
pub const MAX_PARTY: usize = 4;

/// HUD banner shown after a level-up.
///
/// Engines draw this via the dialog font overlay. `frames_remaining` counts
/// down each [`crate::world::World::tick`]; when it reaches zero the banner
/// is cleared by the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelUpBanner {
    pub char_id: u8,
    pub new_level: u8,
    pub hp_gained: u16,
    pub mp_gained: u16,
    /// Remaining display frames. Decremented by the world tick.
    pub frames_remaining: u16,
}

impl LevelUpBanner {
    /// Default display duration: 180 frames (3 s at 60 Hz).
    pub const DEFAULT_FRAMES: u16 = 180;
}

/// Maximum character level.
pub const MAX_LEVEL: u8 = 99;

mod observation;
mod stats;
mod tracker;
mod xp_table;

pub use observation::*;
pub use stats::*;
pub use tracker::*;
pub use xp_table::*;

#[cfg(test)]
mod tests;
