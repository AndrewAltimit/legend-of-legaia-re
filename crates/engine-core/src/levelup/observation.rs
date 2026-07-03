//! Captured level-up observations + the `observations` reference dataset.
//!
//! Extracted verbatim from `levelup.rs`.

use super::*;

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
/// This capture-derived observation predates pinning the retail source.
/// The real per-character per-level growth is the static-SCUS tables
/// `DAT_800769CC` (curves) + `DAT_80076918` (param block), read by the
/// victory-path applier `FUN_801E9504` - installed directly via
/// [`LevelUpTracker::with_growth_tables`]. (The earlier "Seru struct
/// +0x74" pointer-dereference path was falsified: the only `+0x74` reads
/// in the captured overlay surface a 32-bit battle-state flag the
/// SCUS-side handler `FUN_800480D8` writes with the constant `0x80808080`.)
/// This observation type stays useful as a flat-curve fallback when a
/// `SCUS_942.54` isn't reachable.
///
/// `stat_deltas` covers the persistent record stat window at
/// `+0x11C..+0x12D` (18 bytes = 9 u16 LE values). The first two values
/// mirror HP_max and MP_max; the third (`+0x120`) is a per-stat cap
/// constant (consistently `100` across all captured characters);
/// `+0x122..+0x12D` are the six u16 record-side stats. Use
/// [`LevelUpObservation::record_stats_u16`] to read the window as nine
/// u16 LE deltas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelUpObservation {
    /// Display name for diagnostics (e.g. `"Vahn 4-level jump"`).
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
    /// Per-stat byte deltas observed at char-record `+0x11C..+0x12D` (18
    /// bytes = 9 u16 LE values). Each u16 entry is the total delta across
    /// `levels_gained` levels. The first two values are HP_max / MP_max
    /// (mirroring `+0x106` / `+0x10A` in the live copy); the third value
    /// at `+0x120` is the per-stat cap constant (`100`); the remaining
    /// six are the record-side stats at `+0x122..+0x12D`.
    pub stat_deltas: [u8; 18],
}

impl LevelUpObservation {
    /// Number of levels the observation spans.
    pub fn levels_gained(&self) -> u16 {
        self.to_level.saturating_sub(self.from_level) as u16
    }

    /// Per-level [`StatGain`] averaged across the observed range. Used
    /// internally by [`Self::to_curve`]. The six battle stats come from the
    /// `+0x122..+0x12D` slice of the captured stat window.
    pub fn average_per_level(&self) -> StatGain {
        let n = self.levels_gained().max(1);
        let w = self.record_stats_u16(); // [HP, MP, cap, AGL, ATK, UDF, LDF, SPD, INT]
        StatGain {
            hp: self.hp_gained / n,
            mp: self.mp_gained / n,
            agl: w[3] / n,
            atk: w[4] / n,
            udf: w[5] / n,
            ldf: w[6] / n,
            spd: w[7] / n,
            int: w[8] / n,
        }
    }

    /// Read the stat-record window at `+0x11C..+0x12D` as 9 u16 LE
    /// deltas. The first two are HP_max / MP_max; index 2 is the per-
    /// stat cap constant (`100` across all captured characters); the
    /// last six are the record-side stats at `+0x122..+0x12D`.
    pub fn record_stats_u16(&self) -> [u16; 9] {
        let mut out = [0u16; 9];
        for (i, slot) in out.iter_mut().enumerate() {
            let lo = self.stat_deltas[i * 2];
            let hi = self.stat_deltas[i * 2 + 1];
            *slot = u16::from_le_bytes([lo, hi]);
        }
        out
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
///
/// Each character's level-up event splits across multiple frames in
/// retail. The save-state corpus pins three phases per character:
///
/// | Phase | Window | Writes |
/// |---|---|---|
/// | Record write | pre → mid | char-record stats `+0x11C..+0x12D`, XP `+0x004..+0x005`, rank `+0x130` |
/// | Live copy | mid → post | live stat window `+0x104..+0x11B` (HP_cur, MP_cur, six u16 stats) |
/// | Settle | post → next | live HP_max / MP_max / SP_max settle at `+0x106 / +0x10A / +0x10E` |
///
/// The Noa and Gala observations exposed below capture the *settled*
/// pre→settled diff (multi-frame collapse) so consumers see the total
/// delta the level-up event grants. The phase split is documented in
/// [`crate::capture_observations::char_level_up`].
pub mod observations {
    use super::LevelUpObservation;

    /// Vahn 4-level-jump observation captured from a pre/post save pair
    /// in the **legacy** corpus (source saves rotated out of the active
    /// save-state corpus when the per-character level-up triplets
    /// shipped). Bytes are kept here as historical fact - engines that
    /// want a Vahn observation should re-capture against the active
    /// corpus once a Vahn-specific triplet lands.
    ///
    /// Bytes mapped to per-stat deltas (from the original capture):
    /// - `+0x11C`: `0xDD → 0x03` (rolled past 0xFF - `+0x26` mod 256)
    /// - `+0x11D`: `0x00 → 0x01` (carry from above; effective u16 LE
    ///   `+0x126` if the field is u16)
    /// - `+0x11E`: `0x1B → 0x23` (+8)
    /// - `+0x122`: `0x67 → 0x6B` (+4)
    /// - `+0x124`: `0x1C → 0x20` (+4)
    /// - `+0x126`: `0x13 → 0x15` (+2)
    /// - `+0x128`: `0x10 → 0x12` (+2)
    /// - `+0x12A`: `0x16 → 0x1A` (+4)
    /// - `+0x12C`: `0x0B → 0x0F` (+4)
    /// - `+0x130`: `0x02 → 0x03` (+1, rank counter - not a stat)
    ///
    /// SP_max byte at `+0x10E`: `0x3A → 0x42` (+8).
    pub fn vahn_4_level_jump() -> LevelUpObservation {
        LevelUpObservation {
            label: "Vahn 4-level jump (legacy)".into(),
            from_level: 6,
            to_level: 10,
            hp_gained: 0, // not surfaced in the diff (record's hp_max stayed steady)
            mp_gained: 0,
            sp_gained: 8, // observed at +0x10E
            stat_deltas: [
                // 18 bytes at +0x11C..+0x12D (9 u16 LE deltas).
                // [+0x11C] HP_max LSB/MSB (rolled +0x26)
                0x26, 0x01, // [+0x11E] MP_max LSB/MSB (+8)
                0x08, 0x00, // [+0x120] cap constant (no change)
                0x00, 0x00, // [+0x122..+0x12D] six record-side stats
                0x04, 0x00, 0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x04, 0x00, 0x04, 0x00,
            ],
        }
    }

    /// Noa 4-level-jump observation captured from a pre / mid / post
    /// save triplet at battle scene `map01`. Spans Noa's cumulative XP
    /// `102 → 336` reward across the early-game thresholds (L2 → L6).
    ///
    /// Settled deltas:
    /// - HP_max: `0x96 → 0xB6` (+32) at `+0x106` (live) and `+0x11C` (record)
    /// - MP_max: `0x0A → 0x10` (+6) at `+0x10A` (live) and `+0x11E` (record)
    /// - SP_max: `0x38 → 0x60` (+40) at `+0x10E` (live only; record at
    ///   `+0x120` is a 100-cap constant, not SP_max)
    /// - Six record-side stats at `+0x122..+0x12D`: +4 / +3 / +3 / +2 / +4 / +3
    /// - Rank counter at `+0x130`: `0x01 → 0x02` (+1)
    /// - XP at `+0x004..+0x005` (u16 LE): 102 → 336 (+234, Noa's share
    ///   of the battle reward)
    ///
    /// The 3-phase write split (record write → live copy → settle) is
    /// documented in [`crate::capture_observations::char_level_up`].
    pub fn noa_4_level_jump() -> LevelUpObservation {
        LevelUpObservation {
            label: "Noa 4-level jump".into(),
            from_level: 2,
            to_level: 6,
            hp_gained: 32,
            mp_gained: 6,
            sp_gained: 40,
            stat_deltas: [
                // [+0x11C] HP_max (+32 = 0x20)
                0x20, 0x00, // [+0x11E] MP_max (+6)
                0x06, 0x00, // [+0x120] cap constant (no change, both saves read 100)
                0x00, 0x00, // [+0x122..+0x12D] six record-side stats
                0x04, 0x00, 0x03, 0x00, 0x03, 0x00, 0x02, 0x00, 0x04, 0x00, 0x03, 0x00,
            ],
        }
    }

    /// Gala 4-level-jump observation captured from a pre / mid / post
    /// save triplet at battle scene `map01`. Spans Gala's cumulative XP
    /// `140 → 394` reward across the early-game thresholds (L3 → L7).
    ///
    /// Settled deltas:
    /// - HP_max: `0xD2 → 0xFE` (+44) at `+0x106` (live) and `+0x11C` (record)
    /// - MP_max: `0x28 → 0x30` (+8) at `+0x10A` (live) and `+0x11E` (record)
    /// - SP_max: **no change** at `+0x10E` (Gala uses physical Tactical
    ///   Arts; level-up grants no SP for him)
    /// - Six record-side stats at `+0x122..+0x12D`: +2 / +4 / +4 / +2 / +2 / +2
    /// - Rank counter at `+0x130`: `0x01 → 0x02` (+1)
    /// - XP at `+0x004..+0x005` (u16 LE): 140 → 394 (+254, Gala's share)
    ///
    /// The 2-phase write split (record write → live copy + settle in
    /// one frame) is documented in
    /// [`crate::capture_observations::char_level_up`].
    pub fn gala_4_level_jump() -> LevelUpObservation {
        LevelUpObservation {
            label: "Gala 4-level jump".into(),
            from_level: 3,
            to_level: 7,
            hp_gained: 44,
            mp_gained: 8,
            sp_gained: 0,
            stat_deltas: [
                // [+0x11C] HP_max (+44 = 0x2C)
                0x2C, 0x00, // [+0x11E] MP_max (+8)
                0x08, 0x00, // [+0x120] cap constant (no change)
                0x00, 0x00, // [+0x122..+0x12D] six record-side stats
                0x02, 0x00, 0x04, 0x00, 0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00,
            ],
        }
    }
}
