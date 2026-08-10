//! Victory-reward tuning: EXP multiplier + Seru catch-rate override.

use super::*;
use crate::monster_stats::ScalePermille;
use crate::rewards;

/// Outcome of a reward-tuning pass ([`scale_monster_exp`] /
/// [`set_seru_catch_rate`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RewardReport {
    /// Monster slots actually rewritten.
    pub monsters_changed: usize,
    /// Monster ids whose re-packed slot would overflow the `0x14000` footprint,
    /// so the edit was skipped (the original value is kept). Same rare LZS
    /// re-pack guard as the drop / stat passes (see [`crate::monster`]).
    pub skipped: Vec<u16>,
}

/// Scale every populated monster's base EXP reward (decoded-record `+0x46`) by
/// a multiplier (`0.1x..=5x`; see [`crate::rewards`]). Seedless - the result
/// depends only on the disc and the scale. A retail (`1x`) scale writes
/// nothing; a record whose scaled value equals its current one is left alone,
/// so the pass is idempotent. Slot handling (same-size re-pack,
/// skip-on-overflow) matches the stat scale.
pub fn scale_monster_exp(patcher: &mut DiscPatcher, scale: ScalePermille) -> Result<RewardReport> {
    let mut report = RewardReport::default();
    if scale.is_retail() {
        return Ok(report);
    }
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    for rec in &records {
        let new_exp = rewards::scale_exp_value(rec.exp, scale);
        if new_exp == rec.exp {
            continue;
        }
        let slot = patcher
            .monster_slot(rec.id)
            .with_context(|| format!("read monster {} slot", rec.id))?;
        let new_slot = match rewards::set_exp(&slot, new_exp) {
            Ok(s) => s,
            // Only the slot-overflow guard can fire here; a malformed slot
            // would already have failed decoding above.
            Err(_) => {
                report.skipped.push(rec.id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(rec.id, &new_slot)
                .with_context(|| format!("write monster {} slot", rec.id))?;
            report.monsters_changed += 1;
        }
    }
    Ok(report)
}

/// Override every capturable Seru monster's catch rate (decoded-record
/// `+0x3F`) with one flat percent (`0..=100`; see [`crate::rewards`]).
/// Only records with a nonzero Seru id (`+0x3E`) are touched - the override
/// can never make a non-Seru monster capturable. Records already at the target
/// rate are left alone, so the pass is idempotent.
pub fn set_seru_catch_rate(patcher: &mut DiscPatcher, pct: u8) -> Result<RewardReport> {
    let mut report = RewardReport::default();
    let entry = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    for rec in &records {
        if rec.seru_id == 0 || rec.catch_rate_pct == pct {
            continue;
        }
        let slot = patcher
            .monster_slot(rec.id)
            .with_context(|| format!("read monster {} slot", rec.id))?;
        let new_slot = match rewards::set_catch_rate(&slot, pct) {
            Ok(s) => s,
            Err(_) => {
                report.skipped.push(rec.id);
                continue;
            }
        };
        if new_slot != slot {
            patcher
                .patch_monster_slot(rec.id, &new_slot)
                .with_context(|| format!("write monster {} slot", rec.id))?;
            report.monsters_changed += 1;
        }
    }
    Ok(report)
}
