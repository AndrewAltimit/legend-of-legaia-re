//! Post-battle level-up tracker.
//!
//! Tracks cumulative XP per party slot and checks against configurable
//! per-level thresholds. On a level-up the tracker returns a [`LevelUpResult`]
//! whose HP / MP gains are applied to the character's [`legaia_save::CharacterRecord`]
//! via typed setters.
//!
//! ## XP table provenance
//!
//! [`retail_xp_table`] ships a 98-entry placeholder slice (`50, 56, 62, …`) that
//! an earlier pass mis-extracted from `0x8007123C` — that address is doubly
//! wrong (an off-by-`0x800` file/virtual confusion, then a slice of the GTE sin
//! LUT at `0x80070A2C`), so these numbers are **fabricated XP**, not retail.
//!
//! The real retail curve is the static-SCUS per-level u16 delta table
//! `DAT_80076AF4`, read by the overlay level-up applier `FUN_801E9504` (called
//! from the reward resolver `FUN_8004E568` at `0x8004F34C`): the running sum to
//! the current level is scaled `(sum × 9_999_999) / 0x140FE` for `level < 0x11`
//! (else `sum × 0x79`) and compared `≤ record cumulative XP`. That curve is
//! parsed by `legaia_asset::level_up_tables::xp_thresholds_from_scus` and
//! **installed at boot** by `legaia_engine_shell::BootSession` (which reads the
//! user's `SCUS_942.54`) over [`LevelUpTracker::xp_table`]. The placeholder
//! below is only used when no executable is reachable (disc-less tests). See
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
//! `rand()` jitter is the one piece not modeled (it needs the RNG stream for
//! replay determinism) — see `docs/subsystems/level-up.md` § Stat gains.

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

/// Stats gained per level-up for one party slot — the eight `FUN_801E9504`
/// grows, in template / applier order (HP, MP, then the six battle stats
/// AGL / ATK / UDF / LDF / SPD / INT).
///
/// Per-character growth comes from the static-SCUS tables (installed via
/// [`LevelUpTracker::with_growth_tables`]); the [`Default`] is the flat
/// disc-less placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatGain {
    pub hp: u16,
    pub mp: u16,
    pub agl: u16,
    pub atk: u16,
    pub udf: u16,
    pub ldf: u16,
    pub spd: u16,
    pub int: u16,
}

impl Default for StatGain {
    fn default() -> Self {
        // Placeholder: 10 HP / 5 MP per level, no battle-stat growth.
        Self::hp_mp(10, 5)
    }
}

impl StatGain {
    /// Construct from HP/MP only, with zero battle-stat growth. The common
    /// case for the flat placeholder and HP/MP-only callers.
    pub const fn hp_mp(hp: u16, mp: u16) -> Self {
        Self {
            hp,
            mp,
            agl: 0,
            atk: 0,
            udf: 0,
            ldf: 0,
            spd: 0,
            int: 0,
        }
    }

    /// The six battle stats in applier / template order
    /// `[AGL, ATK, UDF, LDF, SPD, INT]` (= applier stat indices 2..=7).
    pub const fn battle(&self) -> [u16; 6] {
        [self.agl, self.atk, self.udf, self.ldf, self.spd, self.int]
    }

    /// Field-wise saturating sum.
    pub fn saturating_add(self, o: Self) -> Self {
        Self {
            hp: self.hp.saturating_add(o.hp),
            mp: self.mp.saturating_add(o.mp),
            agl: self.agl.saturating_add(o.agl),
            atk: self.atk.saturating_add(o.atk),
            udf: self.udf.saturating_add(o.udf),
            ldf: self.ldf.saturating_add(o.ldf),
            spd: self.spd.saturating_add(o.spd),
            int: self.int.saturating_add(o.int),
        }
    }

    /// Scale every field by `n` (saturating). Used for flat-rate `× levels`.
    pub fn saturating_mul(self, n: u16) -> Self {
        Self {
            hp: self.hp.saturating_mul(n),
            mp: self.mp.saturating_mul(n),
            agl: self.agl.saturating_mul(n),
            atk: self.atk.saturating_mul(n),
            udf: self.udf.saturating_mul(n),
            ldf: self.ldf.saturating_mul(n),
            spd: self.spd.saturating_mul(n),
            int: self.int.saturating_mul(n),
        }
    }
}

/// Per-level stat growth curve.
///
/// The retail game stores per-character HP/MP growth tables in overlay DATA
/// (the `level_up` cluster - see overlay capture). This enum lets the engine
/// hold both the captured-from-retail level-indexed arrays and the simple
/// flat-rate fallback the engine ships with today.
///
/// The retail per-character source is now pinned: the static-SCUS curves at
/// `DAT_800769CC` + parameter block at `DAT_80076918`, read by `FUN_801E9504`
/// (the falsified "Seru struct `+0x74`" hypothesis is dead). Engines install
/// the [`PerLevel`](Self::PerLevel) form via
/// [`LevelUpTracker::with_growth_tables`]; [`with_stat_curves`] remains for
/// callers supplying their own curves.
///
/// [`with_stat_curves`]: LevelUpTracker::with_stat_curves
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatGrowthCurve {
    /// Constant growth - same HP/MP gain for every level. Default.
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
        let mut total = StatGain::hp_mp(0, 0);
        if to_level <= from_level {
            return total;
        }
        for prev in from_level..to_level {
            total = total.saturating_add(self.gain_for(prev));
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

/// Faithful PSX BIOS `rand()` (BIOS call `A(0x2F)`) — a 32-bit LCG.
///
/// `seed = seed × 0x41C6_4E6D + 0x3039; return (seed >> 16) & 0x7FFF`. This is
/// the generator the retail level-up applier `FUN_801E9504` draws from for the
/// per-level stat-growth jitter (`rand() % (2×jitter+1) − jitter`). The
/// *algorithm* is faithful; the seed at level-up time is runtime BIOS state the
/// engine can't recover from disc, so a bit-exact roll requires seeding from a
/// capture. Installed (opt-in) via [`LevelUpTracker::with_level_up_jitter`].
///
/// PORT: BIOS `rand`/`srand` (A-table 0x2F/0x30); consumed by FUN_801E9504.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiosRand {
    seed: u32,
}

impl BiosRand {
    /// Seed the generator. Mirrors BIOS `srand(seed)`.
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// Advance and return the next 15-bit value (`0..=0x7FFF`), as BIOS `rand()`.
    pub fn next_u15(&mut self) -> u16 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        ((self.seed >> 16) & 0x7FFF) as u16
    }
}

/// Cumulative XP thresholds for levels 2..=`MAX_LEVEL`.
///
/// `table[i]` = total XP required to reach level `i + 2` (from level 1).
/// Prefix-summed from the 98 u16 values below.
///
/// **These values are FABRICATED, not retail XP.** They are a 98-entry slice
/// of the GTE sin LUT (`sin[0x408..0x46A]`) that an earlier pass mis-read as an
/// XP table after an off-by-`0x800` confusion (file `0x6123C` vs virtual
/// `0x80070A3C`); the bytes are rotation-LUT data the `RotMatrixX/Y/Z`
/// (`0x800461A4 / 0x8004629C / 0x8004638C`) and cutscene-camera (`FUN_8001CF50`)
/// builders consume, not levelling data.
///
/// The real retail curve is the static-SCUS per-level u16 delta table
/// `DAT_80076AF4`, read by the overlay level-up applier `FUN_801E9504` (called
/// from `FUN_8004E568` at `0x8004F34C`): the running sum to the current level is
/// scaled `(sum × 9_999_999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`)
/// and compared `≤ record cumulative XP`. The engine installs that real curve at
/// boot (`legaia_asset::level_up_tables::xp_thresholds_from_scus` →
/// `BootSession`); this placeholder is the fallback when no `SCUS_942.54` is
/// reachable (disc-less tests), retained only so the tracker has a curve shape.
///
/// [`LevelUpTracker::default`] uses this table. See
/// [`docs/subsystems/level-up.md`](https://github.com/altimit-mii/legend-of-legaia-re/blob/main/docs/subsystems/level-up.md#xp-table)
/// for the full write-up.
pub fn retail_xp_table() -> Vec<u32> {
    // 98 u16 values that are a sin-LUT slice (SCUS file offset 0x61A3C / virtual
    // 0x80070A3C = sin LUT base 0x80070A2C + 0x10), NOT retail XP. Placeholder
    // only - the real curve is DAT_80076AF4 + formula (see docstring).
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

/// Geometric `100 × n²` approximation - used only in unit tests that need
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
/// This capture-derived observation predates pinning the retail source.
/// The real per-character per-level growth is the static-SCUS tables
/// `DAT_800769CC` (curves) + `DAT_80076918` (param block), read by the
/// victory-path applier `FUN_801E9504` — installed directly via
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
    /// Total increase to the six battle stats, in applier order
    /// `[AGL, ATK, UDF, LDF, SPD, INT]` (sum across all levels gained).
    pub battle_gained: [u16; 6],
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
    /// is `[StatGrowthCurve::default(); MAX_PARTY]` - flat rate equal to
    /// `StatGain::default()`.
    pub stat_curves: [StatGrowthCurve; MAX_PARTY],
    /// The parsed static-SCUS growth tables, retained by
    /// [`with_growth_tables`](Self::with_growth_tables). Needed (alongside
    /// [`jitter_rng`](Self::jitter_rng)) to apply the per-level jitter spread to
    /// the *unfloored* core in the exact applier order. `None` until installed.
    pub growth_tables: Option<legaia_asset::level_up_tables::GrowthTables>,
    /// Opt-in PSX BIOS-rand stream for the per-level stat-growth jitter. `None`
    /// (the default) means **no jitter**: the tracker applies only the
    /// deterministic core and draws zero `rand()`, so every replay/determinism
    /// oracle stays bit-identical. Enable via
    /// [`with_level_up_jitter`](Self::with_level_up_jitter).
    pub jitter_rng: Option<BiosRand>,
}

impl Default for LevelUpTracker {
    fn default() -> Self {
        Self {
            xp: [0; MAX_PARTY],
            level: [1; MAX_PARTY],
            xp_table: retail_xp_table(),
            stat_gains: [StatGain::default(); MAX_PARTY],
            stat_curves: std::array::from_fn(|_| StatGrowthCurve::default()),
            growth_tables: None,
            jitter_rng: None,
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

    /// Install a flat per-level curve derived from a [`crate::seru_stats::SeruStatTable`]
    /// summed against `roster`. Convenience wrapper around
    /// [`crate::seru_stats::SeruStatTable::to_flat_curve`] that targets a
    /// specific party slot.
    ///
    /// For the three playable characters, prefer
    /// [`with_growth_tables`](Self::with_growth_tables): the retail per-level
    /// growth is the static-SCUS `DAT_800769CC` / `DAT_80076918` tables (the
    /// "Seru struct `+0x74`" grant path was falsified). This Seru-roster curve
    /// remains for engines modelling Seru-sourced gains directly.
    pub fn with_seru_roster(
        mut self,
        char_slot: u8,
        table: &crate::seru_stats::SeruStatTable,
        roster: &[u16],
    ) -> Self {
        let slot = char_slot as usize;
        if slot < MAX_PARTY {
            self.stat_curves[slot] = table.to_flat_curve(roster);
        }
        self
    }

    /// Install per-character deterministic HP/MP growth curves from the parsed
    /// static-SCUS growth tables (`legaia_asset::level_up_tables`).
    ///
    /// Uses the **jitter-free core** of the retail applier `FUN_801E9504`
    /// ([`legaia_asset::level_up_tables::GrowthTables::level_gain_core`]) for the
    /// HP (stat 0) and MP (stat 1) growth records: per level `prev → prev+1`,
    /// `gain = max(1, (max-start) × curve[row][prev-1] / 0x24C0)`. This is the
    /// validated retail per-character growth (checked byte-exact against the Noa
    /// L2→L3 capture); it replaces the flat 10 HP / 5 MP placeholder for the
    /// three playable slots (Vahn / Noa / Gala). The 4th slot keeps its existing
    /// curve.
    ///
    /// Retail additionally adds a per-level `rand() % (2×jitter+1) − jitter`
    /// spread on top of this core. That jitter is applied only when a caller
    /// opts in via [`with_level_up_jitter`](Self::with_level_up_jitter) (it draws
    /// `rand()` and so is off by default to keep replays bit-identical); without
    /// it the tracker uses the jitter-free core (the jitter mean is 0, so totals
    /// are unbiased). All six battle stats are grown alongside HP/MP.
    pub fn with_growth_tables(
        mut self,
        tables: &legaia_asset::level_up_tables::GrowthTables,
    ) -> Self {
        use legaia_asset::level_up_tables::GROWTH_CHAR_COUNT;
        // Retain the raw tables so an opt-in jitter pass can apply the spread to
        // the unfloored core in applier order (see `with_level_up_jitter`).
        self.growth_tables = Some(tables.clone());
        for slot in 0..GROWTH_CHAR_COUNT.min(MAX_PARTY) {
            let Some(cp) = tables.char_params(slot) else {
                continue;
            };
            let mut table = Vec::with_capacity((MAX_LEVEL - 1) as usize);
            // table[prev-1] is the gain for the level-up prev → prev+1, matching
            // `gain_for(prev)`. `level_gain_core(_, prev)` reads curve[row][prev-1].
            // Applier stat order: 0=HP, 1=MP, 2..=7 = AGL/ATK/UDF/LDF/SPD/INT.
            for prev in 1u8..MAX_LEVEL {
                let lvl = prev as usize;
                let g = |i: usize| tables.level_gain_core(&cp.stats[i], lvl).unwrap_or(0) as u16;
                table.push(StatGain {
                    hp: g(0),
                    mp: g(1),
                    agl: g(2),
                    atk: g(3),
                    udf: g(4),
                    ldf: g(5),
                    spd: g(6),
                    int: g(7),
                });
            }
            self.stat_curves[slot] = StatGrowthCurve::PerLevel(table);
        }
        self
    }

    /// Enable the retail per-level stat-growth **jitter** spread, seeding the
    /// PSX BIOS-rand stream ([`BiosRand`]) with `seed`.
    ///
    /// **Off by default.** With no jitter RNG installed the tracker applies only
    /// the deterministic jitter-free core and draws **zero** `rand()`, so every
    /// existing replay/determinism oracle stays bit-identical. Once enabled (and
    /// once growth tables are installed via
    /// [`with_growth_tables`](Self::with_growth_tables)), each level-up adds
    /// `rand() % (2×jitter+1) − jitter` to each stat's **unfloored** core before
    /// the `max(1, …)` floor, in the applier's stat order (HP, MP, AGL, ATK, UDF,
    /// LDF, SPD, INT), drawing **one** `rand()` per stat per level — exactly as
    /// `FUN_801E9504` does, including the draw when `jitter == 0` (`rand() % 1 ==
    /// 0`). Faithful in algorithm; a bit-exact reproduction of a *specific*
    /// retail level-up additionally requires seeding from the BIOS-rand state at
    /// that moment (runtime, not recoverable from disc).
    ///
    /// Order matters: install the growth tables first, then enable jitter.
    pub fn with_level_up_jitter(mut self, seed: u32) -> Self {
        self.jitter_rng = Some(BiosRand::new(seed));
        self
    }

    /// Accumulate the stat growth for `old_level → new_level`.
    ///
    /// When a jitter RNG is installed *and* this slot has parsed growth params
    /// (the three playable characters), this applies the full retail jitter pass
    /// — one `rand()` per stat per level on the unfloored core, then `max(1, …)`,
    /// summed across the levels crossed (the `FUN_801E9504` order). Otherwise it
    /// falls back to the deterministic per-level curve / flat rate, consuming no
    /// `rand()`.
    fn accumulate_growth(&mut self, slot: usize, old_level: u8, new_level: u8) -> StatGain {
        // Disjoint-field borrow (`growth_tables` shared + `jitter_rng` mut) plus
        // a growth record for this slot (the three playable chars).
        if let (Some(tables), Some(rng)) = (self.growth_tables.as_ref(), self.jitter_rng.as_mut())
            && let Some(cp) = tables.char_params(slot)
        {
            // Zero accumulator — `StatGain::default()` is the flat 10/5
            // placeholder, not zero, so must not seed it here.
            let mut acc = StatGain::hp_mp(0, 0);
            for prev in old_level..new_level {
                let lvl = prev as usize;
                let mut g = [0u16; 8];
                for (i, p) in cp.stats.iter().enumerate() {
                    let raw = tables.level_gain_core_raw(p, lvl).unwrap_or(0) as i32;
                    // Always draw — retail does too, even for jitter == 0.
                    let roll = i32::from(rng.next_u15());
                    let span = 2 * i32::from(p.jitter) + 1;
                    let jit = roll % span - i32::from(p.jitter);
                    g[i] = (raw + jit).max(1) as u16;
                }
                acc = acc.saturating_add(StatGain {
                    hp: g[0],
                    mp: g[1],
                    agl: g[2],
                    atk: g[3],
                    udf: g[4],
                    ldf: g[5],
                    spd: g[6],
                    int: g[7],
                });
            }
            return acc;
        }
        // Deterministic fallback (no rand draws).
        match &self.stat_curves[slot] {
            StatGrowthCurve::PerLevel(_) => self.stat_curves[slot].sum_range(old_level, new_level),
            StatGrowthCurve::Flat(_) => {
                let levels_gained = (new_level - old_level) as u16;
                self.stat_gains[slot].saturating_mul(levels_gained)
            }
        }
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
        // table - preserves backward compat for callers that haven't
        // moved to `with_stat_curves`. If the caller installed a flat
        // curve, prefer the explicit `stat_gains` (set via
        // `with_stat_gain` / `with_stat_gains`) since it's the more
        // intentional configuration.
        let total = self.accumulate_growth(slot, old_level, new_level);

        Some(LevelUpResult {
            char_id,
            old_level,
            new_level,
            xp_gained: xp,
            hp_gained: total.hp,
            mp_gained: total.mp,
            battle_gained: total.battle(),
        })
    }

    /// Apply a `LevelUpResult` to a `CharacterRecord` - increases `hp_max` /
    /// `mp_max`, restores `hp_cur` / `mp_cur` to the new maximums (Legaia
    /// restores HP/MP on level-up), adds the six battle-stat gains to both the
    /// record-side window (`+0x11C..+0x12D`) and the live window
    /// (`+0x110..+0x11B`) - matching `FUN_801E9504`'s write-then-mirror - and
    /// writes the new level back to the record's `+0x100` byte.
    pub fn apply_to_record(result: &LevelUpResult, record: &mut CharacterRecord) {
        let mut hms = record.hp_mp_sp();
        hms.hp_max = hms.hp_max.saturating_add(result.hp_gained);
        hms.mp_max = hms.mp_max.saturating_add(result.mp_gained);
        hms.hp_cur = hms.hp_max;
        hms.mp_cur = hms.mp_max;
        record.set_hp_mp_sp(hms);

        // Six battle stats: AGL / ATK / UDF / LDF / SPD / INT. Retail grows the
        // record-side window (+0x11C..+0x12D) then MIRRORS it into the live
        // window (+0x110..+0x11B), so the two stay consistent.
        let [d_agl, d_atk, d_udf, d_ldf, d_spd, d_int] = result.battle_gained;

        let mut rs = record.record_stats();
        rs.hp_max = hms.hp_max; // keep the record-side HP/MP copy in sync
        rs.mp_max = hms.mp_max;
        rs.agl = rs.agl.saturating_add(d_agl);
        rs.atk = rs.atk.saturating_add(d_atk);
        rs.udf = rs.udf.saturating_add(d_udf);
        rs.ldf = rs.ldf.saturating_add(d_ldf);
        rs.spd = rs.spd.saturating_add(d_spd);
        rs.int = rs.int.saturating_add(d_int);
        record.set_record_stats(rs);

        // Mirror the grown record-side battle stats into the live window the
        // battle reads each frame.
        let mut ls = record.live_stats();
        ls.agl = rs.agl;
        ls.atk = rs.atk;
        ls.udf = rs.udf;
        ls.ldf = rs.ldf;
        ls.spd = rs.spd;
        ls.int = rs.int;
        record.set_live_stats(ls);

        record.set_level(result.new_level);
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
    fn growth_tables_drive_deterministic_per_level_hp_mp() {
        use legaia_asset::level_up_tables::{
            GROWTH_PARAM_LEN, GROWTH_ROW_COUNT, GROWTH_ROW_STRIDE, GrowthTables,
        };
        // A curve of all 0x60 sums to 98 × 96 = 9408 = 0x24C0, so the divide is
        // exact and gain = (max-start) × 0x60 / 0x24C0 every level.
        let curves = vec![vec![0x60u8; GROWTH_ROW_STRIDE]; GROWTH_ROW_COUNT];
        let mut param = vec![0u8; GROWTH_PARAM_LEN];
        // slot 0 HP: start=100, max=4900 → (4800 × 96) / 9408 = 48; jitter 0, row 0.
        param[0..6].copy_from_slice(&[100, 0, 0x24, 0x13, 0, 0]); // 0x1324 = 4900
        // slot 0 MP: start=10, max=970 → (960 × 96) / 9408 = 9.
        param[6..12].copy_from_slice(&[10, 0, 0xCA, 0x03, 0, 0]); // 0x03CA = 970
        let g = GrowthTables { curves, param };

        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_growth_tables(&g);

        // Curve replaced the flat placeholder for slot 0.
        assert!(matches!(t.stat_curves[0], StatGrowthCurve::PerLevel(_)));
        // L1 → L2: deterministic core, not the 10/5 flat rate.
        let r = t.grant_xp(0, 100).expect("level up");
        assert_eq!(r.new_level, 2);
        assert_eq!(r.hp_gained, 48);
        assert_eq!(r.mp_gained, 9);
        // Slot 3 (no growth record) keeps the flat default.
        assert!(matches!(t.stat_curves[3], StatGrowthCurve::Flat(_)));
    }

    /// Build a synthetic `GrowthTables` whose every curve is the flat `0x60`
    /// ramp (sums to `0x24C0`, so the divide is exact), with slot-0 HP/MP params
    /// `start/max/jitter` configurable. HP is stat 0, MP stat 1.
    fn synth_growth(hp_jitter: u8, mp_jitter: u8) -> legaia_asset::level_up_tables::GrowthTables {
        use legaia_asset::level_up_tables::{
            GROWTH_PARAM_LEN, GROWTH_ROW_COUNT, GROWTH_ROW_STRIDE, GrowthTables,
        };
        let curves = vec![vec![0x60u8; GROWTH_ROW_STRIDE]; GROWTH_ROW_COUNT];
        let mut param = vec![0u8; GROWTH_PARAM_LEN];
        // HP: start=100, max=4900 → raw core 48 every level.
        param[0..6].copy_from_slice(&[100, 0, 0x24, 0x13, hp_jitter, 0]);
        // MP: start=10, max=970 → raw core 9 every level.
        param[6..12].copy_from_slice(&[10, 0, 0xCA, 0x03, mp_jitter, 0]);
        GrowthTables { curves, param }
    }

    #[test]
    fn bios_rand_is_the_psx_lcg() {
        // seed=1: 1×0x41C64E6D + 0x3039 = 0x41C67EA6; (>>16)&0x7FFF = 0x41C6.
        let mut r = BiosRand::new(1);
        assert_eq!(r.next_u15(), 0x41C6);
        // Deterministic for a given seed.
        let mut a = BiosRand::new(0xDEAD_BEEF);
        let mut b = BiosRand::new(0xDEAD_BEEF);
        for _ in 0..256 {
            assert_eq!(a.next_u15(), b.next_u15());
        }
        // 15-bit range.
        let mut c = BiosRand::new(7);
        for _ in 0..10_000 {
            assert!(c.next_u15() <= 0x7FFF);
        }
    }

    #[test]
    fn level_up_jitter_off_draws_nothing_and_equals_core() {
        let g = synth_growth(4, 2);
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_growth_tables(&g);
        // No jitter RNG installed → deterministic core, no rand consumed.
        assert!(t.jitter_rng.is_none());
        let r = t.grant_xp(0, 100).expect("level up");
        assert_eq!((r.hp_gained, r.mp_gained), (48, 9));
        assert!(t.jitter_rng.is_none()); // still none — nothing was drawn
    }

    #[test]
    fn level_up_jitter_stays_in_band_and_is_seed_deterministic() {
        let g = synth_growth(4, 0); // HP jitters ±4, MP has zero jitter
        let build = |seed| {
            LevelUpTracker::new()
                .with_xp_table(placeholder_xp_table())
                .with_growth_tables(&g)
                .with_level_up_jitter(seed)
        };
        // Same seed → identical roll.
        let mut t1 = build(0x1234_5678);
        let mut t2 = build(0x1234_5678);
        let r1 = t1.grant_xp(0, 100).unwrap();
        let r2 = t2.grant_xp(0, 100).unwrap();
        assert_eq!((r1.hp_gained, r1.mp_gained), (r2.hp_gained, r2.mp_gained));

        // Across seeds: HP lands in [44, 52] (raw 48 ± 4, floored at 1); MP is
        // exactly 9 every time (jitter 0 ⇒ spread 0, even though a draw is made).
        let mut saw_low = false;
        let mut saw_high = false;
        for seed in 1u32..400 {
            let mut t = build(seed);
            let r = t.grant_xp(0, 100).unwrap();
            assert!(
                (44..=52).contains(&r.hp_gained),
                "hp {} out of band",
                r.hp_gained
            );
            assert_eq!(r.mp_gained, 9, "mp must be unaffected by jitter==0");
            saw_low |= r.hp_gained < 48;
            saw_high |= r.hp_gained > 48;
        }
        // The spread actually moves the value both ways (not a stuck constant).
        assert!(
            saw_low && saw_high,
            "jitter should vary above and below the core"
        );
    }

    #[test]
    fn level_up_jitter_multilevel_matches_per_level_draw_order() {
        // A 2-level jump must draw rand per stat per level (HP then MP, level
        // L1→L2 then L2→L3) — so the total equals the sum of two independently
        // jittered single levels off the same stream.
        let g = synth_growth(4, 0);
        let seed = 0xABCD_1234;
        // One tracker that jumps two levels at once.
        let mut jump = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_growth_tables(&g)
            .with_level_up_jitter(seed);
        let rj = jump.grant_xp(0, 400).expect("two-level jump"); // placeholder L3 = 400
        assert_eq!(rj.new_level, 3);

        // A tracker that levels one at a time off the same seed stream.
        let mut step = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_growth_tables(&g)
            .with_level_up_jitter(seed);
        let s1 = step.grant_xp(0, 100).unwrap();
        let s2 = step.grant_xp(0, 300).unwrap(); // cumulative 400 → L3
        assert_eq!(
            (rj.hp_gained, rj.mp_gained),
            (s1.hp_gained + s2.hp_gained, s1.mp_gained + s2.mp_gained)
        );
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
        // Seed the six battle stats in both windows (a real record keeps them
        // equal) so the level-up grows the record side then mirrors to live.
        let seed = legaia_save::character::LiveStats {
            agl: 20,
            atk: 24,
            udf: 16,
            ldf: 12,
            spd: 19,
            int: 9,
        };
        rec.set_live_stats(seed);
        let mut rs = rec.record_stats();
        rs.agl = 20;
        rs.atk = 24;
        rs.udf = 16;
        rs.ldf = 12;
        rs.spd = 19;
        rs.int = 9;
        rec.set_record_stats(rs);

        let result = LevelUpResult {
            char_id: 0,
            old_level: 1,
            new_level: 2,
            xp_gained: 100,
            hp_gained: 10,
            mp_gained: 5,
            battle_gained: [2, 4, 4, 3, 4, 3], // AGL/ATK/UDF/LDF/SPD/INT
        };
        LevelUpTracker::apply_to_record(&result, &mut rec);

        let updated = rec.hp_mp_sp();
        assert_eq!(updated.hp_max, 110);
        assert_eq!(updated.mp_max, 55);
        // HP/MP restored to new max
        assert_eq!(updated.hp_cur, 110);
        assert_eq!(updated.mp_cur, 55);

        // The six battle stats grew in both the live and record-side windows.
        let ls = rec.live_stats();
        assert_eq!(
            [ls.agl, ls.atk, ls.udf, ls.ldf, ls.spd, ls.int],
            [22, 28, 20, 15, 23, 12]
        );
        let rs = rec.record_stats();
        assert_eq!(
            [rs.agl, rs.atk, rs.udf, rs.ldf, rs.spd, rs.int],
            [22, 28, 20, 15, 23, 12]
        );
        assert_eq!((rs.hp_max, rs.mp_max), (110, 55));
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
            .with_stat_gain(StatGain::hp_mp(20, 15));
        let r = t.grant_xp(0, 100).expect("level up");
        assert_eq!(r.hp_gained, 20);
        assert_eq!(r.mp_gained, 15);
    }

    #[test]
    fn per_slot_stat_gains_independent() {
        let gains = [
            StatGain::hp_mp(30, 5),
            StatGain::hp_mp(10, 20),
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
        let curve = StatGrowthCurve::Flat(StatGain::hp_mp(7, 3));
        // Per-level lookup is the flat value regardless of level.
        for prev in 1u8..10 {
            assert_eq!(curve.gain_for(prev), StatGain::hp_mp(7, 3));
        }
        // Sum across 5 levels = 5×.
        let total = curve.sum_range(1, 6);
        assert_eq!(total, StatGain::hp_mp(35, 15));
    }

    #[test]
    fn stat_growth_curve_per_level_lookup() {
        let curve = StatGrowthCurve::PerLevel(vec![
            StatGain::hp_mp(10, 2), // L1→2
            StatGain::hp_mp(12, 3), // L2→3
            StatGain::hp_mp(15, 4), // L3→4
            StatGain::hp_mp(18, 5), // L4→5
        ]);
        assert_eq!(curve.gain_for(1), StatGain::hp_mp(10, 2));
        assert_eq!(curve.gain_for(4), StatGain::hp_mp(18, 5));
        // Past-table indices fall back to default.
        assert_eq!(curve.gain_for(10), StatGain::default());
        // Sum across 1..=4: 10+12+15+18 = 55, 2+3+4+5 = 14.
        assert_eq!(curve.sum_range(1, 5), StatGain::hp_mp(55, 14));
    }

    #[test]
    fn level_up_uses_per_level_curve_when_installed() {
        // Multi-level jump (L1 → L3 with 400 XP under placeholder table).
        // Curve gives 7 HP for L1→2 and 13 HP for L2→3 (total 20).
        let curve = StatGrowthCurve::PerLevel(vec![
            StatGain::hp_mp(7, 1),
            StatGain::hp_mp(13, 2),
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
            stat_deltas: [0; 18],
        };
        let avg = obs.average_per_level();
        assert_eq!(avg.hp, 2);
        assert_eq!(avg.mp, 1);
        let curve = obs.to_curve();
        // Inside the observed range each level emits the average.
        assert_eq!(curve.gain_for(6), StatGain::hp_mp(2, 1));
        assert_eq!(curve.gain_for(9), StatGain::hp_mp(2, 1));
        // Outside the range falls back to default.
        assert_eq!(curve.gain_for(1), StatGain::default());
        assert_eq!(curve.gain_for(50), StatGain::default());
        // Sum across the observed range == hp_gained / mp_gained.
        let total = curve.sum_range(6, 10);
        assert_eq!(total, StatGain::hp_mp(8, 4));
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
            stat_deltas: [0; 18],
        };
        assert_eq!(obs.levels_gained(), 0);
        assert_eq!(obs.average_per_level(), StatGain::hp_mp(0, 0));
    }

    #[test]
    fn vahn_legacy_observation_matches_capture() {
        let obs = observations::vahn_4_level_jump();
        assert_eq!(obs.from_level, 6);
        assert_eq!(obs.to_level, 10);
        assert_eq!(obs.levels_gained(), 4);
        // Spirit-max gain captured at +0x10E (single-byte +8).
        assert_eq!(obs.sp_gained, 8);
        // First stat delta byte is the wrap-around 0xDD->0x03 = +0x26.
        assert_eq!(obs.stat_deltas[0], 0x26);
        // u16 LE projection: HP_max delta = 0x0126 (rolled past 0xFF).
        let stats = obs.record_stats_u16();
        assert_eq!(stats[0], 0x0126);
        // [+0x120] cap constant unchanged.
        assert_eq!(stats[2], 0);
    }

    #[test]
    fn noa_observation_pins_settled_deltas() {
        let obs = observations::noa_4_level_jump();
        assert_eq!(obs.from_level, 2);
        assert_eq!(obs.to_level, 6);
        assert_eq!(obs.levels_gained(), 4);
        assert_eq!(obs.hp_gained, 32);
        assert_eq!(obs.mp_gained, 6);
        // Noa is a Seru-magic user; level-up grants Spirit at +0x10E.
        assert_eq!(obs.sp_gained, 40);
        let stats = obs.record_stats_u16();
        // HP_max delta at +0x11C.
        assert_eq!(stats[0], 32);
        // MP_max delta at +0x11E.
        assert_eq!(stats[1], 6);
        // [+0x120] per-stat cap constant unchanged.
        assert_eq!(stats[2], 0);
        // Six record-side stat deltas at +0x122..+0x12D.
        assert_eq!(&stats[3..9], &[4, 3, 3, 2, 4, 3]);
    }

    #[test]
    fn gala_observation_pins_settled_deltas() {
        let obs = observations::gala_4_level_jump();
        assert_eq!(obs.from_level, 3);
        assert_eq!(obs.to_level, 7);
        assert_eq!(obs.levels_gained(), 4);
        assert_eq!(obs.hp_gained, 44);
        assert_eq!(obs.mp_gained, 8);
        // Gala uses physical Tactical Arts; level-up grants no SP.
        assert_eq!(obs.sp_gained, 0);
        let stats = obs.record_stats_u16();
        assert_eq!(stats[0], 44);
        assert_eq!(stats[1], 8);
        assert_eq!(stats[2], 0);
        assert_eq!(&stats[3..9], &[2, 4, 4, 2, 2, 2]);
    }

    #[test]
    fn record_stats_u16_lifts_18_byte_window() {
        let mut obs = LevelUpObservation {
            label: "round-trip".into(),
            from_level: 1,
            to_level: 2,
            hp_gained: 0,
            mp_gained: 0,
            sp_gained: 0,
            stat_deltas: [0; 18],
        };
        // Set the second u16 (at +0x11E) to 0x1234 LE.
        obs.stat_deltas[2] = 0x34;
        obs.stat_deltas[3] = 0x12;
        let stats = obs.record_stats_u16();
        assert_eq!(stats[1], 0x1234);
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
            stat_deltas: [0; 18],
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
    fn with_seru_roster_installs_flat_curve_summed_from_table() {
        use crate::seru_stats::{SeruStatGrant, SeruStatTable};
        let mut table = SeruStatTable::new();
        table.insert(0, SeruStatGrant::hp_mp(8, 3));
        table.insert(1, SeruStatGrant::hp_mp(4, 2));
        // Roster sum: hp 12, mp 5.
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_seru_roster(0, &table, &[0, 1]);
        let r = t.grant_xp(0, 100).expect("level up");
        assert_eq!(r.hp_gained, 12);
        assert_eq!(r.mp_gained, 5);
    }

    #[test]
    fn level_up_default_flat_still_uses_stat_gains_field() {
        // No curve installed (default = Flat(default)). The legacy
        // `with_stat_gain` path should still drive the result.
        let mut t = LevelUpTracker::new()
            .with_xp_table(placeholder_xp_table())
            .with_stat_gain(StatGain::hp_mp(25, 11));
        let r = t.grant_xp(0, 400).expect("multi-level");
        assert_eq!(r.new_level, 3);
        assert_eq!(r.hp_gained, 50); // 2 levels × 25
        assert_eq!(r.mp_gained, 22); // 2 levels × 11
    }
}
