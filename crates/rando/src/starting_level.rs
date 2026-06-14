//! Starting-level randomization: begin a New Game with the lead character
//! already at a chosen level instead of level 1.
//!
//! A vanilla New Game seeds Vahn at level 1: the seed routine `FUN_800560B4`
//! stores a small literal cumulative-XP value (121) into the live record, and
//! his stats come from the starting-party template
//! ([`legaia_asset::new_game::PARTY_TEMPLATE_VA`], level-1 values). The displayed
//! combat level is derived from cumulative XP — the level byte at record `+0x100`
//! is left zero by the new-game memset and never written by retail (verified
//! against early save states) — so there is no static "level" field to edit.
//!
//! This randomizer therefore makes the start coherent at level `N` by editing
//! two places in `SCUS_942.54`, both same-size in place:
//!
//! 1. **XP** — rewrite the `addiu $v0, $zero, imm` literal at
//!    [`legaia_asset::new_game::STARTING_XP_SEED_VA`] to a cumulative XP value
//!    that lands squarely inside level `N`'s XP band (the midpoint between the
//!    real disc thresholds to reach `N` and `N+1`, so it is unambiguously level
//!    `N` regardless of the exact comparison the level display uses). The literal
//!    is a single 16-bit immediate, which caps the level at
//!    [`MAX_STARTING_LEVEL`].
//! 2. **Stats** — overwrite slot 0's eight `u16` template stats with the level-`N`
//!    values, computed by accumulating the deterministic (jitter-free) per-level
//!    growth gains from `FUN_801E9504`'s curves
//!    ([`legaia_asset::level_up_tables`]) on top of the level-1 template, so a
//!    level-10 Vahn has level-10 HP/ATK/… rather than level-1 stats with a
//!    level-10 XP bar.
//!
//! Only party slot 0 (the character actually present at a New Game) is touched.
//! The XP literal is shared with slot 3 (Terra), but she re-scales when she
//! later joins, so the edit has no effect on her. Magic rank (a separate
//! progression that ticks per level-up) is intentionally left at its seeded
//! value of 1.

use anyhow::{Context, Result, bail};
use legaia_asset::level_up_tables::{growth_tables_from_scus, xp_thresholds_from_scus};
use legaia_asset::new_game::{STAT_COUNT, StartingParty};

/// Default starting level the toggle seeds when enabled without an explicit
/// value. A modest head start that skips the earliest grind without
/// trivializing the opening.
pub const DEFAULT_STARTING_LEVEL: u8 = 10;

/// Lowest level the toggle accepts. Level 1 is the vanilla start (a no-op), so
/// the randomizer only acts from level 2 up.
pub const MIN_STARTING_LEVEL: u8 = 2;

/// Highest level the toggle accepts. The XP seed is a single `addiu` immediate
/// (16-bit), and the cumulative-XP midpoint for a level stays within a positive
/// `imm16` (`<= 0x7FFF`) through level 14; beyond that the literal would need a
/// second instruction the surrounding code has no room for.
pub const MAX_STARTING_LEVEL: u8 = 14;

/// A resolved starting-level plan: the XP literal to seed and the level-`N`
/// stats to write into slot 0's template, in template order
/// (`hp, mp, agl, atk, udf, ldf, spd, int`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartingLevelPlan {
    /// The chosen level (`MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL`).
    pub level: u8,
    /// The cumulative-XP value to write into the `addiu` literal (fits a
    /// positive `imm16`).
    pub xp_seed: u16,
    /// The level-`N` stats for slot 0, in template order.
    pub stats: [u16; STAT_COUNT],
}

/// `true` when `level` requests a non-vanilla starting level the randomizer can
/// seed (i.e. `MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL`). Level 0 or 1 is the
/// vanilla start, so callers treat it as "off".
pub fn is_active(level: u8) -> bool {
    (MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL).contains(&level)
}

/// Build the [`StartingLevelPlan`] for `level` from a `SCUS_942.54` image.
///
/// Reads the level-1 starting-party template, the per-level XP thresholds, and
/// the stat-growth curves straight out of the executable (no committed Sony
/// bytes), then computes the level-`N` XP seed and stats for party slot 0.
/// Errors if `level` is out of range or the image is missing the tables.
pub fn plan(scus: &[u8], level: u8) -> Result<StartingLevelPlan> {
    if !is_active(level) {
        bail!(
            "starting level {level} out of range ({}..={})",
            MIN_STARTING_LEVEL,
            MAX_STARTING_LEVEL
        );
    }
    let n = level as usize;

    // Level-N cumulative-XP band from the real disc thresholds. `thresholds[k]`
    // is the total XP to reach level `k + 2`, so the XP to reach level `m` is
    // `thresholds[m - 2]`. Seed the midpoint of `(reach(N), reach(N+1)]` so the
    // value sits unambiguously inside level N's band.
    let thresholds =
        xp_thresholds_from_scus(scus).context("read XP thresholds from SCUS_942.54")?;
    let reach_n = thresholds
        .get(n - 2)
        .copied()
        .with_context(|| format!("XP threshold for level {level} out of range"))?;
    let reach_n1 = thresholds
        .get(n - 1)
        .copied()
        .with_context(|| format!("XP threshold for level {} out of range", level + 1))?;
    let xp = reach_n + (reach_n1 - reach_n) / 2;
    let xp_seed = u16::try_from(xp)
        .ok()
        .filter(|&v| v <= 0x7FFF)
        .with_context(|| {
            format!("level {level} XP seed {xp} does not fit a positive 16-bit immediate")
        })?;

    // Level-N stats: accumulate the deterministic per-level growth gains on top
    // of the level-1 template. Growth slot 0 = Vahn; its 8 sub-records are in the
    // same order as the template stats (hp, mp, agl, atk, udf, ldf, spd, int).
    let template = StartingParty::from_scus(scus)
        .context("decode starting-party template from SCUS_942.54")?;
    let vahn = template
        .member(0)
        .context("starting-party template has no slot 0")?;
    let base = [
        vahn.hp_max,
        vahn.mp_max,
        vahn.agl,
        vahn.atk,
        vahn.udf,
        vahn.ldf,
        vahn.spd,
        vahn.intel,
    ];
    let growth =
        growth_tables_from_scus(scus).context("read stat-growth tables from SCUS_942.54")?;
    let params = growth
        .char_params(0)
        .context("growth tables have no slot 0 (Vahn)")?;

    let mut stats = [0u16; STAT_COUNT];
    for (s, out) in stats.iter_mut().enumerate() {
        let p = &params.stats[s];
        let mut val = base[s] as u32;
        // Gains applied leveling from L to L+1, for L = 1..N-1 (reaching level N).
        for l in 1..n {
            val += growth.level_gain_core(p, l).unwrap_or(1);
        }
        // Never exceed the stat's level-99 ceiling (or the u16 range).
        *out = val.min(p.max as u32).min(u16::MAX as u32) as u16;
    }

    Ok(StartingLevelPlan {
        level,
        xp_seed,
        stats,
    })
}

/// Encode the XP seed into the 4 bytes of the `addiu $v0, $zero, imm` literal.
/// The instruction is `0x2402_0000 | imm16`; only the low halfword changes, but
/// the whole word is returned so the patcher writes a complete instruction.
pub fn xp_seed_instruction(xp_seed: u16) -> [u8; 4] {
    (0x2402_0000u32 | xp_seed as u32).to_le_bytes()
}

/// Encode the level-`N` stats into the 16 bytes of slot 0's template stat block
/// (`STAT_COUNT` little-endian `u16`s, in template order).
pub fn stat_block(stats: &[u16; STAT_COUNT]) -> [u8; STAT_COUNT * 2] {
    let mut out = [0u8; STAT_COUNT * 2];
    for (i, s) in stats.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_predicate() {
        assert!(!is_active(0));
        assert!(!is_active(1));
        assert!(is_active(MIN_STARTING_LEVEL));
        assert!(is_active(DEFAULT_STARTING_LEVEL));
        assert!(is_active(MAX_STARTING_LEVEL));
        assert!(!is_active(MAX_STARTING_LEVEL + 1));
    }

    #[test]
    fn xp_seed_instruction_keeps_the_opcode() {
        // addiu $v0, $zero, 0x258d  (a representative level-10-ish XP)
        assert_eq!(xp_seed_instruction(0x258d), 0x2402_258du32.to_le_bytes());
        // The opcode/rt nibble is preserved for any immediate.
        for imm in [0u16, 1, 0x79, 0x2bbb, 0x7fff] {
            let w = u32::from_le_bytes(xp_seed_instruction(imm));
            assert_eq!(w >> 16, 0x2402, "addiu $v0, $zero stays intact");
            assert_eq!(w & 0xffff, imm as u32);
        }
    }

    #[test]
    fn stat_block_is_little_endian_in_order() {
        let stats = [0x0102u16, 0x0304, 0, 0, 0, 0, 0, 0x0a0b];
        let b = stat_block(&stats);
        assert_eq!(&b[0..2], &[0x02, 0x01]);
        assert_eq!(&b[2..4], &[0x04, 0x03]);
        assert_eq!(&b[14..16], &[0x0b, 0x0a]);
    }

    #[test]
    fn plan_rejects_out_of_range_levels() {
        // No SCUS needed: the range guard fires first.
        assert!(plan(&[], 1).is_err());
        assert!(plan(&[], MAX_STARTING_LEVEL + 1).is_err());
    }
}
