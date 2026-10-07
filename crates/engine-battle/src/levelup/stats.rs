//! Per-stat gain, growth-curve classification, and the BIOS RNG used by level-ups.
//!
//! Extracted verbatim from `levelup.rs`.

/// Stats gained per level-up for one party slot - the eight `FUN_801E9504`
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

/// Faithful PSX BIOS `rand()` (BIOS call `A(0x2F)`) - a 32-bit LCG.
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
