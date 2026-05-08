//! Post-battle level-up tracker.
//!
//! Tracks cumulative XP per party slot and checks against configurable
//! per-level thresholds. On a level-up the tracker returns a [`LevelUpResult`]
//! whose HP / MP gains are applied to the character's [`legaia_save::CharacterRecord`]
//! via typed setters.
//!
//! ## Placeholder values
//!
//! [`placeholder_xp_table`] is a geometric approximation (`100 × n²`). The
//! actual XP thresholds are stored in the DATA segment of the
//! `overlay_magic_level_up.bin` binary (not in any function code) and cannot
//! be extracted from Ghidra function dumps alone. Capturing a full overlay
//! binary dump and locating the table at a known static address is required to
//! replace this.
//!
//! Per-slot [`StatGain`] values in [`LevelUpTracker::stat_gains`] are also
//! placeholder flat rates. Different party members (Vahn / Noa / Gala) have
//! different HP / MP growth curves in the retail game; those values live in the
//! same overlay DATA segment alongside the XP table.

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

/// Cumulative XP thresholds for levels 2..=`MAX_LEVEL`.
/// `table[i]` = total XP required to reach level `i + 2` (from level 1).
///
/// This is a placeholder `100 × n²` curve. Replace from overlay dump.
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
            xp_table: placeholder_xp_table(),
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
        let mut t = LevelUpTracker::new();
        assert!(t.grant_xp(0, 99).is_none()); // threshold for level 2 = 100
        assert_eq!(t.level[0], 1);
        assert_eq!(t.xp[0], 99);
    }

    #[test]
    fn level_up_at_exact_threshold() {
        let mut t = LevelUpTracker::new();
        let r = t.grant_xp(0, 100).expect("should level up");
        assert_eq!(r.old_level, 1);
        assert_eq!(r.new_level, 2);
        assert_eq!(r.hp_gained, 10);
        assert_eq!(r.mp_gained, 5);
        assert_eq!(t.level[0], 2);
    }

    #[test]
    fn multi_level_jump() {
        let mut t = LevelUpTracker::new();
        // level 1→2 needs 100 XP, 1→3 needs 400 XP total
        let r = t.grant_xp(0, 400).expect("should jump levels");
        assert_eq!(r.old_level, 1);
        assert_eq!(r.new_level, 3);
        assert_eq!(r.hp_gained, 20); // 2 × 10
        assert_eq!(r.mp_gained, 10); // 2 × 5
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
        let mut t = LevelUpTracker::new();
        assert!(t.grant_xp(0, 50).is_none());
        // 50 + 50 = 100 → level up
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
        let mut t = LevelUpTracker::new();
        // char 0 levels up, char 1 doesn't
        assert!(t.grant_xp(0, 100).is_some());
        assert!(t.grant_xp(1, 50).is_none());
        assert_eq!(t.level[0], 2);
        assert_eq!(t.level[1], 1);
    }

    #[test]
    fn with_stat_gain_override() {
        let mut t = LevelUpTracker::new().with_stat_gain(StatGain { hp: 20, mp: 15 });
        let r = t.grant_xp(0, 100).expect("level up");
        assert_eq!(r.hp_gained, 20);
        assert_eq!(r.mp_gained, 15);
    }

    #[test]
    fn per_slot_stat_gains_independent() {
        // Slot 0: high HP growth, slot 1: high MP growth
        let gains = [
            StatGain { hp: 30, mp: 5 },
            StatGain { hp: 10, mp: 20 },
            StatGain::default(),
            StatGain::default(),
        ];
        let mut t = LevelUpTracker::new().with_stat_gains(gains);

        let r0 = t.grant_xp(0, 100).expect("slot 0 levels up");
        assert_eq!(r0.hp_gained, 30);
        assert_eq!(r0.mp_gained, 5);

        let r1 = t.grant_xp(1, 100).expect("slot 1 levels up");
        assert_eq!(r1.hp_gained, 10);
        assert_eq!(r1.mp_gained, 20);
    }
}
