//! Victory-reward tuning: the EXP multiplier and the Seru catch-rate override.
//!
//! Both knobs are plain data edits to the monster archive (PROT entry 867) -
//! the same decoded-record head the drop randomizer and the difficulty scale
//! edit, re-packed through [`crate::monster::repack_slot`] so every slot keeps
//! its `0x14000`-byte footprint.
//!
//! ## EXP multiplier
//!
//! The base victory-spoils experience is the `u16` at decoded-record `+0x46`
//! ([`EXP_OFFSET`]): the spoils routine `FUN_8004E568` sums it `* 3/4` across
//! dead enemies and splits the total among living party members. Scaling the
//! record field therefore scales every EXP grant - including the flee-EXP
//! hook's, which sums the same field ([`crate::flee_exp`]).
//!
//! The multiplier reuses [`crate::monster_stats::ScalePermille`]
//! (`0.1x..=5.0x`), so the CLI flag and the browser slider share one parser,
//! one range check and one rounding rule with the difficulty scale. Clamps
//! mirror [`crate::monster_stats::scale_stats`]: a zero reward stays zero, a
//! non-zero one lands in `1..=u16::MAX` - so `0.1x` never silently erases a
//! reward and `5x` of a late-game record saturates instead of wrapping.
//!
//! ## Seru catch rate
//!
//! A monster is a capturable Seru when the byte at decoded-record `+0x3E`
//! ([`SERU_ID_OFFSET`]) is nonzero; its catch chance in percent is the byte at
//! `+0x3F` ([`CATCH_RATE_OFFSET`]). On a killing physical blow the battle
//! kernel `FUN_801ec3e4` (overlay 0898, block `0x801ee250..0x801ee2e8`) reads
//! both bytes record-direct through the per-enemy pointer table `0x801C9348`
//! and rolls `rand % 100 < rate` (a party member carrying the Magic Boost
//! passive - Ivory Book - adds a flat `+30` points first). Retail rates span
//! `1..=80` across the 63 capturable records.
//!
//! The override writes one flat percent into every capturable record's
//! `+0x3F` and touches nothing else - non-Seru records (`+0x3E == 0`) keep
//! their zero, so the override can never make a non-Seru monster capturable.
//! `100` still passes through the retail roll (`rand % 100 < 100` always
//! hits), `0` never does.

use crate::monster::repack_slot;
use crate::monster_stats::ScalePermille;
use anyhow::Result;

/// Decoded-record byte offset of the base EXP reward (`u16` LE).
pub const EXP_OFFSET: usize = 0x46;
/// Decoded-record byte offset of the Seru id (`0` = not capturable).
pub const SERU_ID_OFFSET: usize = 0x3E;
/// Decoded-record byte offset of the Seru catch chance in percent.
pub const CATCH_RATE_OFFSET: usize = 0x3F;

/// Scale one EXP value by a multiplier, with the same arithmetic and clamp
/// rule as [`crate::monster_stats::scale_stats`]: round half up, zero stays
/// zero (a no-reward record never gains one), non-zero clamps to
/// `1..=u16::MAX` (a reward is never scaled away entirely, and a big one
/// saturates rather than wrapping). `1x` is exactly the identity.
pub fn scale_exp_value(exp: u16, scale: ScalePermille) -> u16 {
    if exp == 0 {
        return 0;
    }
    // Round half up. `u16::MAX * 5000 + 500` stays well inside u32.
    let scaled = (exp as u32 * scale.permille() + 500) / 1000;
    scaled.clamp(1, u16::MAX as u32) as u16
}

/// Set a monster slot's base EXP reward (`+0x46`), returning the re-packed
/// slot bytes. Convenience wrapper over [`repack_slot`].
pub fn set_exp(slot_bytes: &[u8], exp: u16) -> Result<Vec<u8>> {
    repack_slot(slot_bytes, |block| {
        block[EXP_OFFSET..EXP_OFFSET + 2].copy_from_slice(&exp.to_le_bytes());
    })
}

/// Set a monster slot's Seru catch rate (`+0x3F`, percent), returning the
/// re-packed slot bytes. The caller is responsible for only pointing this at
/// capturable records (`+0x3E != 0`); the byte is meaningless elsewhere.
pub fn set_catch_rate(slot_bytes: &[u8], pct: u8) -> Result<Vec<u8>> {
    repack_slot(slot_bytes, |block| {
        block[CATCH_RATE_OFFSET] = pct;
    })
}

/// Parse a user-facing catch-rate percent: `"55"`, `"55%"`, `"100"`. Rejects
/// anything outside `0..=100`, mirroring [`ScalePermille::parse`]'s
/// refuse-don't-clamp rule. The shared entry point for the CLI flag and the
/// browser slider.
pub fn parse_catch_rate(text: &str) -> Result<u8, String> {
    let t = text.trim().trim_end_matches('%').trim();
    let value: u32 = t
        .parse()
        .map_err(|_| format!("{text:?} is not a whole number (want a percent like 55)"))?;
    if value > 100 {
        return Err(format!(
            "seru catch rate {value}% is out of range (want 0..=100)"
        ));
    }
    Ok(value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::monster_archive::SLOT_STRIDE;

    fn fake_slot(block: &[u8]) -> Vec<u8> {
        let stream = legaia_lzs::compress(block);
        assert!(4 + stream.len() <= SLOT_STRIDE);
        let mut slot = Vec::with_capacity(SLOT_STRIDE);
        slot.extend_from_slice(&(block.len() as u32).to_le_bytes());
        slot.extend_from_slice(&stream);
        slot.resize(SLOT_STRIDE, 0);
        slot
    }

    fn decode_slot(slot: &[u8]) -> Vec<u8> {
        let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
        legaia_lzs::decompress(&slot[4..], declared).unwrap()
    }

    fn scale(text: &str) -> ScalePermille {
        ScalePermille::parse(text).unwrap()
    }

    #[test]
    fn scale_exp_value_clamps_at_both_ends() {
        // Plain scaling, rounding half up like the stat scale.
        assert_eq!(scale_exp_value(100, scale("2")), 200);
        assert_eq!(scale_exp_value(55, scale("0.5")), 28);
        // Zero stays zero even at 5x - no invented rewards.
        assert_eq!(scale_exp_value(0, scale("5")), 0);
        // A non-zero reward floors at 1 rather than vanishing at 0.1x.
        assert_eq!(scale_exp_value(5, scale("0.1")), 1);
        // A big reward saturates at u16::MAX instead of wrapping (42000 * 5
        // = 210000 > 65535 - Gaza's record would wrap to garbage otherwise).
        assert_eq!(scale_exp_value(42000, scale("5")), u16::MAX);
        // Identity really is identity.
        assert_eq!(scale_exp_value(1234, scale("1")), 1234);
    }

    #[test]
    fn set_exp_changes_only_the_exp_halfword() {
        let mut block: Vec<u8> = (0..512u32).map(|i| (i * 7 + 1) as u8).collect();
        block[EXP_OFFSET..EXP_OFFSET + 2].copy_from_slice(&55u16.to_le_bytes());
        let slot = fake_slot(&block);

        let patched = set_exp(&slot, 110).unwrap();
        assert_eq!(patched.len(), SLOT_STRIDE);
        let out = decode_slot(&patched);
        let mut expected = block.clone();
        expected[EXP_OFFSET..EXP_OFFSET + 2].copy_from_slice(&110u16.to_le_bytes());
        assert_eq!(out, expected, "only the two EXP bytes changed");
    }

    #[test]
    fn set_catch_rate_changes_only_the_rate_byte() {
        let mut block: Vec<u8> = (0..512u32).map(|i| (i * 3 + 2) as u8).collect();
        block[SERU_ID_OFFSET] = 1;
        block[CATCH_RATE_OFFSET] = 55;
        let slot = fake_slot(&block);

        let patched = set_catch_rate(&slot, 100).unwrap();
        let out = decode_slot(&patched);
        let mut expected = block.clone();
        expected[CATCH_RATE_OFFSET] = 100;
        assert_eq!(out, expected, "only the rate byte changed");
    }

    #[test]
    fn parse_catch_rate_accepts_percents_rejects_out_of_range() {
        assert_eq!(parse_catch_rate("55"), Ok(55));
        assert_eq!(parse_catch_rate(" 100% "), Ok(100));
        assert_eq!(parse_catch_rate("0"), Ok(0));
        assert!(parse_catch_rate("101").is_err());
        assert!(parse_catch_rate("-1").is_err());
        assert!(parse_catch_rate("half").is_err());
    }
}
