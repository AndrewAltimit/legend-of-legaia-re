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

/// Per-level stat growth curve.
///
/// The retail game stores per-character HP/MP growth tables in overlay DATA
/// (the `level_up` cluster — see overlay capture). This enum lets the engine
/// hold both the captured-from-retail level-indexed arrays and the simple
/// flat-rate fallback the engine ships with today.
///
/// Once a watchpoint trace pins down the source of the per-level increments
/// (suspected to live at the `Seru struct +0x74` slot per the level_up
/// overlay analysis), engines populate one of these per character slot via
/// [`LevelUpTracker::with_stat_curves`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatGrowthCurve {
    /// Constant growth — same HP/MP gain for every level. Default.
    Flat(StatGain),
    /// Per-level growth indexed by `target_level - 2` (entry for L1→2 at
    /// index 0, L98→99 at index 96). Length should be `MAX_LEVEL - 1`.
    PerLevel(Vec<StatGain>),
}

impl StatGrowthCurve {
    /// Resolve the gain applied for the level-up `prev_level → prev_level +
    /// 1`. `prev_level` is the level **before** the level-up (1..=98). Out-of-
    /// range or empty curves fall back to [`StatGain::default`].
    pub fn gain_for(&self, prev_level: u8) -> StatGain {
        match self {
            StatGrowthCurve::Flat(g) => *g,
            StatGrowthCurve::PerLevel(table) => {
                if prev_level < 1 {
                    return StatGain::default();
                }
                let idx = (prev_level - 1) as usize;
                table.get(idx).copied().unwrap_or_default()
            }
        }
    }

    /// Sum the stat gains for `from_level → to_level` (inclusive of every
    /// level-up between).
    pub fn sum_range(&self, from_level: u8, to_level: u8) -> StatGain {
        let mut total = StatGain { hp: 0, mp: 0 };
        if to_level <= from_level {
            return total;
        }
        for prev in from_level..to_level {
            let g = self.gain_for(prev);
            total.hp = total.hp.saturating_add(g.hp);
            total.mp = total.mp.saturating_add(g.mp);
        }
        total
    }
}

impl Default for StatGrowthCurve {
    fn default() -> Self {
        StatGrowthCurve::Flat(StatGain::default())
    }
}

impl From<StatGain> for StatGrowthCurve {
    fn from(g: StatGain) -> Self {
        StatGrowthCurve::Flat(g)
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

/// One observed level-up delta from a save-state capture pair.
///
/// Captured via the `mednafen-state diff` toolkit (`scripts/mednafen/`):
/// engines get a "before" save (pre-level-up) and an "after" save
/// (post-level-up), diff the character-record window across them, and
/// translate the byte-level deltas into this struct.
///
/// The observation is an *average* across `levels_gained`, so engines
/// using [`LevelUpObservation::to_curve`] get a flat curve where every
/// level inside the observed range yields `(hp_total / levels_gained,
/// mp_total / levels_gained)`. Outside the observed range the curve
/// falls back to [`StatGain::default`].
///
/// The retail per-character per-level table — the writer-search target
/// for the next round of overlay analysis — would let the engine emit
/// a true [`StatGrowthCurve::PerLevel`] vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelUpObservation {
    /// Display name for diagnostics (e.g. `"Vahn mc8→mc9"`).
    pub label: String,
    /// Pre-event level (1-based).
    pub from_level: u8,
    /// Post-event level (1-based).
    pub to_level: u8,
    /// Total HP_max gained across `from_level → to_level`.
    pub hp_gained: u16,
    /// Total MP_max gained across `from_level → to_level`.
    pub mp_gained: u16,
    /// Spirit-max (SP_max at char-record `+0x10E`) gain across the same
    /// range. Engines that mirror the retail "Spirit gain on level-up"
    /// behavior fold this into their gauge cap.
    pub sp_gained: u16,
    /// Per-stat byte deltas observed at char-record `+0x11C..+0x12D`.
    /// Each entry is the total delta across `levels_gained` levels.
    /// The retail layout for these bytes is not yet pinned (the typed
    /// `CharacterRecord` accessors don't surface them yet).
    pub stat_deltas: [u8; 12],
}

impl LevelUpObservation {
    /// Number of levels the observation spans.
    pub fn levels_gained(&self) -> u16 {
        self.to_level.saturating_sub(self.from_level) as u16
    }

    /// Per-level [`StatGain`] averaged across the observed range. Used
    /// internally by [`Self::to_curve`].
    pub fn average_per_level(&self) -> StatGain {
        let n = self.levels_gained().max(1);
        StatGain {
            hp: self.hp_gained / n,
            mp: self.mp_gained / n,
        }
    }

    /// Build a [`StatGrowthCurve::PerLevel`] vector that emits the
    /// per-level average inside the observed `from_level..to_level`
    /// range and falls back to [`StatGain::default`] outside it.
    pub fn to_curve(&self) -> StatGrowthCurve {
        let avg = self.average_per_level();
        let mut table: Vec<StatGain> = Vec::with_capacity((MAX_LEVEL - 1) as usize);
        for prev in 1u8..MAX_LEVEL {
            let inside = prev >= self.from_level && prev < self.to_level;
            table.push(if inside { avg } else { StatGain::default() });
        }
        StatGrowthCurve::PerLevel(table)
    }
}

/// Captured observations indexed by party slot. Engines read this and
/// install per-slot curves at boot via
/// [`LevelUpTracker::with_observed_curve`].
///
/// The slot indices match the retail party layout (Vahn = 0, Noa = 1,
/// Gala = 2). Slot 3 is reserved for whichever character occupies the
/// fourth party slot (Maya / Songi in the late game).
pub mod observations {
    use super::LevelUpObservation;

    /// Vahn level-up captured from `~/.mednafen/mcs/.mc8 → .mc9` (post-#26
    /// session 13). Pre: cumulative XP 365 → post 730 (delta +365). The
    /// captured `+0x00` byte changed `0x4F → 0x73` (a multi-level jump).
    /// Per-stat byte deltas come from the diff at `+0x11C..+0x12D`.
    ///
    /// Bytes mapped to per-stat deltas:
    /// - `+0x11C`: `0xDD → 0x03` (rolled past 0xFF — `+0x26` mod 256)
    /// - `+0x11D`: `0x00 → 0x01` (carry from above; effective u16 LE
    ///   `+0x126` if the field is u16)
    /// - `+0x11E`: `0x1B → 0x23` (+8)
    /// - `+0x122`: `0x67 → 0x6B` (+4)
    /// - `+0x124`: `0x1C → 0x20` (+4)
    /// - `+0x126`: `0x13 → 0x15` (+2)
    /// - `+0x128`: `0x10 → 0x12` (+2)
    /// - `+0x12A`: `0x16 → 0x1A` (+4)
    /// - `+0x12C`: `0x0B → 0x0F` (+4)
    /// - `+0x130`: `0x02 → 0x03` (+1, rank counter — not a stat)
    ///
    /// SP_max byte at `+0x10E`: `0x3A → 0x42` (+8).
    ///
    /// The XP table gap of 365 → 730 spans the L1→2..L9→10 cumulative
    /// range, but the level field semantics are unconfirmed (the byte
    /// at `+0x00` jumped by +0x24 = 36 which doesn't match a single-
    /// level granularity). Treating the observation as a 4-level jump
    /// from L6 → L10 matches the cumulative thresholds.
    pub fn vahn_mc8_to_mc9() -> LevelUpObservation {
        LevelUpObservation {
            label: "Vahn mc8->mc9 (4-level jump)".into(),
            from_level: 6,
            to_level: 10,
            hp_gained: 0, // not surfaced in the diff (record's hp_max stayed steady)
            mp_gained: 0,
            sp_gained: 8, // observed at +0x10E
            stat_deltas: [
                // 12 stat bytes at +0x11C..+0x127 (interpreted as 12 u8 stats).
                // Engines that decode these as 6 u16s should fold pairs.
                0x26, 0x01, 0x08, 0x00, 0x00, 0x00, 0x04, 0x00, 0x04, 0x00, 0x02, 0x00,
            ],
        }
    }
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
    /// Per-level growth curves, indexed by party slot. When populated, the
    /// engine prefers `stat_curves[slot]` over `stat_gains[slot]`. Default
    /// is `[StatGrowthCurve::default(); MAX_PARTY]` — flat rate equal to
    /// `StatGain::default()`.
    pub stat_curves: [StatGrowthCurve; MAX_PARTY],
}

impl Default for LevelUpTracker {
    fn default() -> Self {
        Self {
            xp: [0; MAX_PARTY],
            level: [1; MAX_PARTY],
            xp_table: retail_xp_table(),
            stat_gains: [StatGain::default(); MAX_PARTY],
            stat_curves: std::array::from_fn(|_| StatGrowthCurve::default()),
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

    /// Install per-slot per-level growth curves. When set, these override
    /// the flat-rate `stat_gains` for the matching slot. Use this once the
    /// retail per-character growth tables have been captured from the
    /// level-up overlay.
    pub fn with_stat_curves(mut self, curves: [StatGrowthCurve; MAX_PARTY]) -> Self {
        self.stat_curves = curves;
        self
    }

    /// Convenience: install the same curve into every party slot.
    pub fn with_stat_curve(mut self, curve: StatGrowthCurve) -> Self {
        self.stat_curves = std::array::from_fn(|_| curve.clone());
        self
    }

    /// Install a curve derived from a captured `LevelUpObservation`.
    /// Engines call this when they have one or more recorded delta samples
    /// from real save-state captures and want the tracker to reproduce
    /// the same average-per-level gain inside the observed range. Outside
    /// that range the curve falls back to [`StatGain::default`].
    pub fn with_observed_curve(mut self, char_slot: u8, obs: &LevelUpObservation) -> Self {
        let slot = char_slot as usize;
        if slot < MAX_PARTY {
            self.stat_curves[slot] = obs.to_curve();
        }
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

        // Curve takes precedence over the flat-rate stat_gains. A
        // `Flat(default())` curve produces the same value as the flat
        // table — preserves backward compat for callers that haven't
        // moved to `with_stat_curves`. If the caller installed a flat
        // curve, prefer the explicit `stat_gains` (set via
        // `with_stat_gain` / `with_stat_gains`) since it's the more
        // intentional configuration.
        let (hp_gained, mp_gained) = match &self.stat_curves[slot] {
            StatGrowthCurve::PerLevel(_) => {
                let summed = self.stat_curves[slot].sum_range(old_level, new_level);
                (summed.hp, summed.mp)
            }
            StatGrowthCurve::Flat(_) => {
                let levels_gained = (new_level - old_level) as u16;
                let gain = self.stat_gains[slot];
                (gain.hp * levels_gained, gain.mp * levels_gained)
            }
        };

        Some(LevelUpResult {
            char_id,
            old_level,
            new_level,
            xp_gained: xp,
            hp_gained,
            mp_gained,
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

    #[test]
    fn stat_growth_curve_flat_matches_legacy_behavior() {
        let curve = StatGrowthCurve::Flat(StatGain { hp: 7, mp: 3 });
        // Per-level lookup is the flat value regardless of level.
        for prev in 1u8..10 {
            assert_eq!(curve.gain_for(prev), StatGain { hp: 7, mp: 3 });
        }
        // Sum across 5 levels = 5×.
        let total = curve.sum_range(1, 6);
        assert_eq!(total, StatGain { hp: 35, mp: 15 });
    }

    #[test]
    fn stat_growth_curve_per_level_lookup() {
        let curve = StatGrowthCurve::PerLevel(vec![
            StatGain { hp: 10, mp: 2 }, // L1→2
            StatGain { hp: 12, mp: 3 }, // L2→3
            StatGain { hp: 15, mp: 4 }, // L3→4
            StatGain { hp: 18, mp: 5 }, // L4→5
        ]);
        assert_eq!(curve.gain_for(1), StatGain { hp: 10, mp: 2 });
        assert_eq!(curve.gain_for(4), StatGain { hp: 18, mp: 5 });
        // Past-table indices fall back to default.
        assert_eq!(curve.gain_for(10), StatGain::default());
        // Sum across 1..=4: 10+12+15+18 = 55, 2+3+4+5 = 14.
        assert_eq!(curve.sum_range(1, 5), StatGain { hp: 55, mp: 14 });
    }

    #[test]
    fn level_up_uses_per_level_curve_when_installed() {
        // Multi-level jump (L1 → L3 with 400 XP under placeholder table).
        // Curve gives 7 HP for L1→2 and 13 HP for L2→3 (total 20).
        let curve = StatGrowthCurve::PerLevel(vec![
            StatGain { hp: 7, mp: 1 },
            StatGain { hp: 13, mp: 2 },
            // … rest unused for this test
        ]);
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_stat_curve(curve);
        let r = t.grant_xp(0, 400).expect("level up");
        assert_eq!(r.old_level, 1);
        assert_eq!(r.new_level, 3);
        assert_eq!(r.hp_gained, 20); // 7 + 13
        assert_eq!(r.mp_gained, 3); // 1 + 2
    }

    #[test]
    fn observation_to_curve_yields_per_level_average_inside_range() {
        let obs = LevelUpObservation {
            label: "test 4-level jump".into(),
            from_level: 6,
            to_level: 10,
            hp_gained: 8,
            mp_gained: 4,
            sp_gained: 8,
            stat_deltas: [0; 12],
        };
        let avg = obs.average_per_level();
        assert_eq!(avg.hp, 2);
        assert_eq!(avg.mp, 1);
        let curve = obs.to_curve();
        // Inside the observed range each level emits the average.
        assert_eq!(curve.gain_for(6), StatGain { hp: 2, mp: 1 });
        assert_eq!(curve.gain_for(9), StatGain { hp: 2, mp: 1 });
        // Outside the range falls back to default.
        assert_eq!(curve.gain_for(1), StatGain::default());
        assert_eq!(curve.gain_for(50), StatGain::default());
        // Sum across the observed range == hp_gained / mp_gained.
        let total = curve.sum_range(6, 10);
        assert_eq!(total, StatGain { hp: 8, mp: 4 });
    }

    #[test]
    fn observation_with_zero_levels_gained_is_zero_avg() {
        let obs = LevelUpObservation {
            label: "no-op".into(),
            from_level: 5,
            to_level: 5,
            hp_gained: 0,
            mp_gained: 0,
            sp_gained: 0,
            stat_deltas: [0; 12],
        };
        assert_eq!(obs.levels_gained(), 0);
        assert_eq!(obs.average_per_level(), StatGain { hp: 0, mp: 0 });
    }

    #[test]
    fn vahn_mc8_to_mc9_observation_matches_capture() {
        let obs = observations::vahn_mc8_to_mc9();
        assert_eq!(obs.from_level, 6);
        assert_eq!(obs.to_level, 10);
        assert_eq!(obs.levels_gained(), 4);
        // Spirit-max gain captured at +0x10E (single-byte +8).
        assert_eq!(obs.sp_gained, 8);
        // First stat delta byte is the wrap-around 0xDD->0x03 = +0x26.
        assert_eq!(obs.stat_deltas[0], 0x26);
    }

    #[test]
    fn with_observed_curve_installs_per_slot() {
        let obs = LevelUpObservation {
            label: "synthetic".into(),
            from_level: 1,
            to_level: 3,
            hp_gained: 20,
            mp_gained: 4,
            sp_gained: 0,
            stat_deltas: [0; 12],
        };
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_observed_curve(0, &obs);
        let r = t.grant_xp(0, 400).expect("level up");
        // Each level inside [1, 3) yields avg(20/2) = 10 HP, avg(4/2) = 2 MP.
        assert_eq!(r.new_level, 3);
        assert_eq!(r.hp_gained, 20);
        assert_eq!(r.mp_gained, 4);
    }

    #[test]
    fn level_up_default_flat_still_uses_stat_gains_field() {
        // No curve installed (default = Flat(default)). The legacy
        // `with_stat_gain` path should still drive the result.
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_stat_gain(StatGain { hp: 25, mp: 11 });
        let r = t.grant_xp(0, 400).expect("multi-level");
        assert_eq!(r.new_level, 3);
        assert_eq!(r.hp_gained, 50); // 2 levels × 25
        assert_eq!(r.mp_gained, 22); // 2 levels × 11
    }
}
