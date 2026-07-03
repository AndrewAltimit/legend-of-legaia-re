//! `LevelUpResult` + `LevelUpTracker`: the party level-up driver.
//!
//! Extracted verbatim from `levelup.rs`.

use super::*;

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
    /// Slots-1/2 per-level XP-threshold correction divisors
    /// (`legaia_asset::level_up_tables::xp_correction_divisors_from_scus`,
    /// indexed by the character's current level). When installed, slot 1
    /// (Noa) reaches each level `threshold * 0x14 / divisor[level]` XP
    /// *earlier* and slot 2 (Gala) that much *later*, mirroring the retail
    /// threshold builder `FUN_801E9504` (slot 0 / Vahn is uncorrected; the
    /// divisor table pointer `_DAT_8007B81C` is constant `0x80070A2C` across
    /// the whole save corpus, so this is static SCUS data). `None` (the
    /// default) keeps every slot on the uncorrected `xp_table`.
    pub xp_corrections: Option<Vec<i16>>,
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
            xp_corrections: None,
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

    /// Install the slots-1/2 XP-threshold correction divisors (see
    /// [`Self::xp_corrections`]).
    pub fn with_xp_corrections(mut self, divisors: Vec<i16>) -> Self {
        self.xp_corrections = Some(divisors);
        self
    }

    /// The XP threshold for `slot` to advance from `level` to `level + 1`:
    /// the base `xp_table` entry, with the retail slots-1/2 correction
    /// applied when divisors are installed - slot 1 (Noa) subtracts
    /// `threshold * 0x14 / divisor[level]`, slot 2 (Gala) adds it, every
    /// other slot takes the base unchanged (`FUN_801E9504`).
    ///
    /// PORT: FUN_801E9504 (slots-1/2 threshold correction; base formula in
    /// legaia_asset::level_up_tables::xp_threshold_for)
    pub fn threshold_for(&self, slot: usize, level: u8) -> Option<u32> {
        let base = self.xp_table.get(level as usize - 1).copied()?;
        let Some(divs) = self.xp_corrections.as_ref() else {
            return Some(base);
        };
        let divisor = divs.get(level as usize).copied().unwrap_or(0);
        let corr = legaia_asset::level_up_tables::xp_threshold_correction(base, divisor);
        Some(match slot {
            1 => base.saturating_sub(corr),
            2 => base.saturating_add(corr),
            _ => base,
        })
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
    /// LDF, SPD, INT), drawing **one** `rand()` per stat per level - exactly as
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
    /// - one `rand()` per stat per level on the unfloored core, then `max(1, …)`,
    ///   summed across the levels crossed (the `FUN_801E9504` order). Otherwise it
    ///   falls back to the deterministic per-level curve / flat rate, consuming no
    ///   `rand()`.
    fn accumulate_growth(&mut self, slot: usize, old_level: u8, new_level: u8) -> StatGain {
        // Disjoint-field borrow (`growth_tables` shared + `jitter_rng` mut) plus
        // a growth record for this slot (the three playable chars).
        if let (Some(tables), Some(rng)) = (self.growth_tables.as_ref(), self.jitter_rng.as_mut())
            && let Some(cp) = tables.char_params(slot)
        {
            // Zero accumulator - `StatGain::default()` is the flat 10/5
            // placeholder, not zero, so must not seed it here.
            let mut acc = StatGain::hp_mp(0, 0);
            for prev in old_level..new_level {
                let lvl = prev as usize;
                let mut g = [0u16; 8];
                for (i, p) in cp.stats.iter().enumerate() {
                    let raw = tables.level_gain_core_raw(p, lvl).unwrap_or(0) as i32;
                    // Always draw - retail does too, even for jitter == 0.
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
            // xp_table[n - 1] = XP to reach level n + 1, with the retail
            // slots-1/2 correction folded in when divisors are installed.
            match self.threshold_for(slot, new_level) {
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
