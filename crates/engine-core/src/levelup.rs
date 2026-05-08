//! Post-battle level-up tracker.
//!
//! Tracks cumulative XP per party slot and checks against configurable
//! per-level thresholds. On a level-up the tracker returns a [`LevelUpResult`]
//! whose HP / MP gains are applied to the character's [`legaia_save::CharacterRecord`]
//! via typed setters.
//!
//! ## XP table provenance
//!
//! [`retail_xp_table`] contains the 98-entry cumulative XP thresholds extracted
//! from `SCUS_942.54` at address `0x8007123C`. Each entry is a u16 LE per-level
//! increment (50 for L1→2, 56 for L2→3, …, 656 for L98→99). The cumulative
//! totals used here are derived by prefix-summing those increments.
//!
//! Per-slot [`StatGain`] values remain placeholder flat rates (10 HP / 5 MP).
//! Different characters (Vahn / Noa / Gala) have distinct HP / MP growth curves
//! in the retail game; locating those tables requires further overlay binary
//! analysis.

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

/// HP and MP gained per level-up for one party slot.
///
/// The retail game assigns different growth rates to each party member
/// (Vahn / Noa / Gala). The per-slot values live in the overlay DATA segment
/// and remain placeholder until a full binary dump is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatGain {
    pub hp: u16,
    pub mp: u16,
}

impl Default for StatGain {
    fn default() -> Self {
        // Placeholder: 10 HP / 5 MP per level for all slots.
        Self { hp: 10, mp: 5 }
    }
}

/// Cumulative XP thresholds for levels 2..=`MAX_LEVEL` from the retail game.
///
/// `table[i]` = total XP required to reach level `i + 2` (from level 1).
/// Derived by prefix-summing the 98 u16 LE per-level increments stored at
/// `SCUS_942.54` address `0x8007123C` (increments: 50, 56, 62, 69, … 650, 656).
///
/// [`LevelUpTracker::default`] uses this table.
pub fn retail_xp_table() -> Vec<u32> {
    // Per-level increments from SCUS_942.54 0x8007123C (98 u16 values, L1→2 .. L98→99).
    const INCREMENTS: [u16; 98] = [
        50, 56, 62, 69, 75, 81, 87, 94, 100, 106, 113, 119, 125, 131, 138, 144, 150, 157, 163, 169,
        175, 182, 188, 194, 200, 207, 213, 219, 226, 232, 238, 244, 251, 257, 263, 269, 276, 282,
        288, 295, 301, 307, 313, 320, 326, 332, 338, 345, 351, 357, 363, 370, 376, 382, 388, 395,
        401, 407, 413, 420, 426, 432, 438, 445, 451, 457, 463, 470, 476, 482, 488, 495, 501, 507,
        513, 520, 526, 532, 538, 545, 551, 557, 563, 569, 576, 582, 588, 594, 601, 607, 613, 619,
        625, 632, 638, 644, 650, 656,
    ];
    let mut cumulative = Vec::with_capacity(INCREMENTS.len());
    let mut total: u32 = 0;
    for &inc in &INCREMENTS {
        total += u32::from(inc);
        cumulative.push(total);
    }
    cumulative
}

/// Geometric `100 × n²` approximation — used only in unit tests that need
/// fixed threshold values independent of the retail data.
#[cfg(test)]
pub fn placeholder_xp_table() -> Vec<u32> {
    (1u32..MAX_LEVEL as u32).map(|n| 100 * n * n).collect()
}

/// One level-up event returned by [`LevelUpTracker::grant_xp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelUpResult {
    pub char_id: u8,
    pub old_level: u8,
    pub new_level: u8,
    /// XP that was granted in the call that triggered this level-up.
    pub xp_gained: u32,
    /// Total HP max increase (sum across all levels gained).
    pub hp_gained: u16,
    /// Total MP max increase (sum across all levels gained).
    pub mp_gained: u16,
}

/// Per-party XP and level state. Owned by [`crate::world::World`].
///
/// Call [`grant_xp`] after each battle win; call [`apply_to_record`] with the
/// returned result to bump the character record's HP/MP maxima.
///
/// [`grant_xp`]: LevelUpTracker::grant_xp
/// [`apply_to_record`]: LevelUpTracker::apply_to_record
#[derive(Debug, Clone)]
pub struct LevelUpTracker {
    /// Accumulated XP per party slot (index = slot 0..MAX_PARTY).
    pub xp: [u32; MAX_PARTY],
    /// Current level per party slot (1-based, range 1..=MAX_LEVEL).
    pub level: [u8; MAX_PARTY],
    /// Cumulative XP thresholds: `xp_table[current_level - 1]` = XP to reach
    /// `current_level + 1`. Length should be `MAX_LEVEL - 1`.
    pub xp_table: Vec<u32>,
    /// HP / MP increments applied per level gained, indexed by party slot.
    /// Allows different growth rates per character (Vahn / Noa / Gala).
    pub stat_gains: [StatGain; MAX_PARTY],
}

impl Default for LevelUpTracker {
    fn default() -> Self {
        Self {
            xp: [0; MAX_PARTY],
            level: [1; MAX_PARTY],
            xp_table: retail_xp_table(),
            stat_gains: [StatGain::default(); MAX_PARTY],
        }
    }
}

impl LevelUpTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the XP table (e.g. from overlay data once captured).
    pub fn with_xp_table(mut self, table: Vec<u32>) -> Self {
        self.xp_table = table;
        self
    }

    /// Apply the same stat gain to every party slot.
    pub fn with_stat_gain(mut self, gain: StatGain) -> Self {
        self.stat_gains = [gain; MAX_PARTY];
        self
    }

    /// Apply per-slot stat gains (e.g. different growth for each character).
    pub fn with_stat_gains(mut self, gains: [StatGain; MAX_PARTY]) -> Self {
        self.stat_gains = gains;
        self
    }

    /// Grant `xp` to party slot `char_id`. If the accumulated XP crosses one
    /// or more level thresholds the highest level reached is returned.
    /// Multi-level jumps collapse into a single result with the total stat
    /// gains for all levels crossed.
    ///
    /// Returns `None` if:
    /// - `char_id` is out of bounds
    /// - already at `MAX_LEVEL`
    /// - no threshold was crossed
    pub fn grant_xp(&mut self, char_id: u8, xp: u32) -> Option<LevelUpResult> {
        let slot = char_id as usize;
        if slot >= MAX_PARTY {
            return None;
        }
        let old_level = self.level[slot];
        if old_level >= MAX_LEVEL {
            return None;
        }

        self.xp[slot] = self.xp[slot].saturating_add(xp);

        let mut new_level = old_level;
        loop {
            if new_level >= MAX_LEVEL {
                break;
            }
            // xp_table[n - 1] = XP to reach level n + 1.
            match self.xp_table.get(new_level as usize - 1).copied() {
                Some(threshold) if self.xp[slot] >= threshold => new_level += 1,
                _ => break,
            }
        }

        if new_level == old_level {
            return None;
        }

        self.level[slot] = new_level;
        let levels_gained = (new_level - old_level) as u16;
        let gain = self.stat_gains[slot];
        Some(LevelUpResult {
            char_id,
            old_level,
            new_level,
            xp_gained: xp,
            hp_gained: gain.hp * levels_gained,
            mp_gained: gain.mp * levels_gained,
        })
    }

    /// Apply a `LevelUpResult` to a `CharacterRecord` — increases `hp_max`
    /// and `mp_max`, and restores `hp_cur` / `mp_cur` to the new maximums
    /// (Legaia restores HP/MP on level-up).
    pub fn apply_to_record(result: &LevelUpResult, record: &mut CharacterRecord) {
        let mut hms = record.hp_mp_sp();
        hms.hp_max = hms.hp_max.saturating_add(result.hp_gained);
        hms.mp_max = hms.mp_max.saturating_add(result.mp_gained);
        hms.hp_cur = hms.hp_max;
        hms.mp_cur = hms.mp_max;
        record.set_hp_mp_sp(hms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::CharacterRecord;

    #[test]
    fn no_level_up_when_xp_below_threshold() {
        // Use placeholder table for stable threshold values (L2 threshold = 100).
        let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
        assert!(t.grant_xp(0, 99).is_none()); // threshold for level 2 = 100
        assert_eq!(t.level[0], 1);
        assert_eq!(t.xp[0], 99);
    }

    #[test]
    fn level_up_at_exact_threshold() {
        let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
        let r = t.grant_xp(0, 100).expect("should level up");
        assert_eq!(r.old_level, 1);
        assert_eq!(r.new_level, 2);
        assert_eq!(r.hp_gained, 10);
        assert_eq!(r.mp_gained, 5);
        assert_eq!(t.level[0], 2);
    }

    #[test]
    fn multi_level_jump() {
        let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
        // level 1→2 needs 100 XP, 1→3 needs 400 XP total (placeholder: 100×n²)
        let r = t.grant_xp(0, 400).expect("should jump levels");
        assert_eq!(r.old_level, 1);
        assert_eq!(r.new_level, 3);
        assert_eq!(r.hp_gained, 20); // 2 × 10
        assert_eq!(r.mp_gained, 10); // 2 × 5
    }

    #[test]
    fn retail_xp_table_level2_threshold() {
        // Retail: 50 XP to reach L2; 49 is not enough.
        let mut t = LevelUpTracker::new();
        assert!(t.grant_xp(0, 49).is_none());
        let r = t.grant_xp(0, 1).expect("50 total = level 2");
        assert_eq!(r.new_level, 2);
    }

    #[test]
    fn retail_xp_table_cumulative_check() {
        // Table[1] = 50+56 = 106: granting 106 XP at once should reach level 3.
        let mut t = LevelUpTracker::new();
        let r = t.grant_xp(0, 106).expect("106 XP reaches L3");
        assert_eq!(r.new_level, 3);
    }

    #[test]
    fn already_at_max_level_returns_none() {
        let mut t = LevelUpTracker::new();
        t.level[0] = MAX_LEVEL;
        assert!(t.grant_xp(0, u32::MAX).is_none());
    }

    #[test]
    fn out_of_bounds_char_returns_none() {
        let mut t = LevelUpTracker::new();
        assert!(t.grant_xp(MAX_PARTY as u8, 9999).is_none());
    }

    #[test]
    fn accumulated_xp_carries_across_calls() {
        let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
        assert!(t.grant_xp(0, 50).is_none());
        // 50 + 50 = 100 → level up (placeholder threshold for L2 = 100)
        let r = t.grant_xp(0, 50).expect("should level up on second call");
        assert_eq!(r.new_level, 2);
        assert_eq!(t.xp[0], 100);
    }

    #[test]
    fn custom_xp_table() {
        let mut t = LevelUpTracker::new().with_xp_table(vec![50, 150, 300]);
        let r = t.grant_xp(0, 50).expect("table[0] = 50");
        assert_eq!(r.new_level, 2);
    }

    #[test]
    fn apply_to_record_bumps_max_and_restores_cur() {
        let mut rec = CharacterRecord::zeroed();
        let mut hms = rec.hp_mp_sp();
        hms.hp_max = 100;
        hms.hp_cur = 40;
        hms.mp_max = 50;
        hms.mp_cur = 10;
        rec.set_hp_mp_sp(hms);

        let result = LevelUpResult {
            char_id: 0,
            old_level: 1,
            new_level: 2,
            xp_gained: 100,
            hp_gained: 10,
            mp_gained: 5,
        };
        LevelUpTracker::apply_to_record(&result, &mut rec);

        let updated = rec.hp_mp_sp();
        assert_eq!(updated.hp_max, 110);
        assert_eq!(updated.mp_max, 55);
        // HP/MP restored to new max
        assert_eq!(updated.hp_cur, 110);
        assert_eq!(updated.mp_cur, 55);
    }

    #[test]
    fn multiple_party_slots_independent() {
        let mut t = LevelUpTracker::new().with_xp_table(placeholder_xp_table());
        // char 0 levels up (100 XP ≥ threshold 100), char 1 doesn't (50 < 100)
        assert!(t.grant_xp(0, 100).is_some());
        assert!(t.grant_xp(1, 50).is_none());
        assert_eq!(t.level[0], 2);
        assert_eq!(t.level[1], 1);
    }

    #[test]
    fn with_stat_gain_override() {
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_stat_gain(StatGain { hp: 20, mp: 15 });
        let r = t.grant_xp(0, 100).expect("level up");
        assert_eq!(r.hp_gained, 20);
        assert_eq!(r.mp_gained, 15);
    }

    #[test]
    fn per_slot_stat_gains_independent() {
        let gains = [
            StatGain { hp: 30, mp: 5 },
            StatGain { hp: 10, mp: 20 },
            StatGain::default(),
            StatGain::default(),
        ];
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_stat_gains(gains);

        let r0 = t.grant_xp(0, 100).expect("slot 0 levels up");
        assert_eq!(r0.hp_gained, 30);
        assert_eq!(r0.mp_gained, 5);

        let r1 = t.grant_xp(1, 100).expect("slot 1 levels up");
        assert_eq!(r1.hp_gained, 10);
        assert_eq!(r1.mp_gained, 20);
    }
}
