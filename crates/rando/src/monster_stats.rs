//! Monster combat-stat randomizer: redistribute HP / MP / ATK / DEF / AGL / SPD
//! across the enemy roster.
//!
//! Each monster's combat stats live as `u16` halfwords at fixed offsets in the
//! decoded `battle_data` block (PROT entry 867; offsets pinned in
//! [`legaia_asset::monster_archive`]). [`plan_stats`] collects each field's
//! value across the whole populated roster into a *column*, then either permutes
//! the column ([`StatMode::Shuffle`] — a 1:1 reassignment, so the multiset of
//! each stat is preserved) or draws each cell from the column pool with
//! replacement ([`StatMode::Random`]). Because every value that lands came from
//! a real monster, the global stat budget is preserved and no field is ever
//! pushed outside the game's own range — a tanky enemy may inherit a weakling's
//! HP while keeping its own attack, scrambling difficulty without producing
//! impossible records.
//!
//! Spirit (`+0x0E`, the SP / action gauge) is **deliberately left alone** — it
//! gates the enemy AI's spell economy rather than player-facing difficulty, and
//! shuffling it would mostly perturb how often enemies cast, not how hard the
//! fight is.
//!
//! Each edit re-packs the monster's slot through [`crate::monster::repack_slot`]:
//! the decoded block length is unchanged, so the slot stays its original
//! `0x14000`-byte footprint and every other monster's slot offset is fixed — a
//! same-size, in-place byte edit, exactly like the drop randomizer.

use crate::monster::repack_slot;
use crate::rng::SplitMix64;
use anyhow::Result;

/// How a randomizer reassigns values. Re-exported [`crate::drops::DropMode`] so
/// the monster-stat pass shares the CLI's `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as StatMode;

/// The combat-stat fields the randomizer touches, as `(label, decoded-record
/// byte offset)`. Each is a little-endian `u16` halfword in the monster's
/// decoded block. Order matches [`StatAssignment::stats`].
pub const STAT_FIELDS: [(&str, usize); 7] = [
    ("hp", 0x0C),
    ("mp", 0x10),
    ("attack", 0x12),
    ("defense_high", 0x14),
    ("defense_low", 0x16),
    ("agility", 0x18),
    ("speed", 0x1A),
];

/// Number of stat fields a [`StatAssignment`] carries.
pub const FIELD_COUNT: usize = STAT_FIELDS.len();

/// One monster's stat values, in [`STAT_FIELDS`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatAssignment {
    /// 1-based monster id (the `battle_data` archive slot index + 1).
    pub monster_id: u16,
    /// The randomized stat halfwords, in [`STAT_FIELDS`] order
    /// (`[hp, mp, attack, defense_high, defense_low, agility, speed]`).
    pub stats: [u16; FIELD_COUNT],
}

/// Read the [`STAT_FIELDS`] halfwords out of a decoded monster block. Returns
/// `None` if the block is too short to hold the last field.
pub fn read_stats(block: &[u8]) -> Option<[u16; FIELD_COUNT]> {
    let mut out = [0u16; FIELD_COUNT];
    for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
        let b = block.get(*off..*off + 2)?;
        out[i] = u16::from_le_bytes([b[0], b[1]]);
    }
    Some(out)
}

/// Re-pack a monster slot with new stat values. Writes each [`STAT_FIELDS`]
/// halfword into the decoded block and recompresses into a fresh `0x14000`-byte
/// slot. Same-size, in place; errors only on the [`repack_slot`] guards
/// (empty/filler slot, LZS failure, re-packed stream overflows the slot).
pub fn set_stats(slot_bytes: &[u8], stats: &[u16; FIELD_COUNT]) -> Result<Vec<u8>> {
    repack_slot(slot_bytes, |block| {
        for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
            if let Some(dst) = block.get_mut(*off..*off + 2) {
                dst.copy_from_slice(&stats[i].to_le_bytes());
            }
        }
    })
}

/// Plan a column-wise stat randomization. `current` holds `(id, stats)` for
/// every populated monster, in roster order; the returned plan is the same
/// monsters with reassigned stats. Deterministic in `(current, seed, mode)`.
///
/// Each field is randomized independently across the roster: [`StatMode::Shuffle`]
/// permutes the column (so the multiset of, say, every monster's HP is exactly
/// preserved); [`StatMode::Random`] draws each cell from the column pool with
/// replacement (the multiset is no longer preserved, but every value is still a
/// real in-game stat).
pub fn plan_stats(current: &[StatAssignment], seed: u64, mode: StatMode) -> Vec<StatAssignment> {
    let mut out = current.to_vec();
    if out.is_empty() {
        return out;
    }
    let mut rng = SplitMix64::new(seed);
    for field in 0..FIELD_COUNT {
        let column: Vec<u16> = current.iter().map(|a| a.stats[field]).collect();
        match mode {
            StatMode::Shuffle => {
                let mut bag = column;
                rng.shuffle(&mut bag);
                for (slot, value) in out.iter_mut().zip(bag) {
                    slot.stats[field] = value;
                }
            }
            StatMode::Random => {
                for slot in out.iter_mut() {
                    slot.stats[field] = column[rng.below(column.len())];
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::monster_archive::SLOT_STRIDE;

    /// `[u32 size][LZS]` slot padded to `SLOT_STRIDE`, like a real archive slot.
    fn fake_slot(block: &[u8]) -> Vec<u8> {
        let stream = legaia_lzs::compress(block);
        let mut slot = Vec::with_capacity(SLOT_STRIDE);
        slot.extend_from_slice(&(block.len() as u32).to_le_bytes());
        slot.extend_from_slice(&stream);
        slot.resize(SLOT_STRIDE, 0);
        slot
    }

    fn decode_block(slot: &[u8]) -> Vec<u8> {
        let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
        legaia_lzs::decompress(&slot[4..], declared).unwrap()
    }

    #[test]
    fn set_stats_is_surgical() {
        // Recognisable, non-zero content at every byte.
        let mut block: Vec<u8> = (0..256u32).map(|i| (i * 5 + 3) as u8).collect();
        // Known starting stats.
        let start = [10u16, 20, 30, 40, 50, 60, 70];
        for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
            block[*off..*off + 2].copy_from_slice(&start[i].to_le_bytes());
        }
        let slot = fake_slot(&block);

        let new = [111u16, 222, 333, 444, 555, 666, 777];
        let patched = set_stats(&slot, &new).expect("re-pack");
        assert_eq!(patched.len(), SLOT_STRIDE, "slot size preserved");

        let out = decode_block(&patched);
        assert_eq!(out.len(), block.len(), "decoded length preserved");
        assert_eq!(read_stats(&out).unwrap(), new, "stats applied");

        // Every byte outside the seven stat halfwords is untouched.
        let mut expected = block.clone();
        for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
            expected[*off..*off + 2].copy_from_slice(&new[i].to_le_bytes());
        }
        assert_eq!(out, expected, "only the stat halfwords changed");
    }

    fn sample(n: usize) -> Vec<StatAssignment> {
        (0..n)
            .map(|i| StatAssignment {
                monster_id: i as u16 + 1,
                stats: [
                    i as u16,
                    i as u16 + 100,
                    i as u16 + 200,
                    i as u16 + 300,
                    i as u16 + 400,
                    i as u16 + 500,
                    i as u16 + 600,
                ],
            })
            .collect()
    }

    /// Shuffle preserves each column's multiset and is a 1:1 reassignment.
    #[test]
    fn shuffle_preserves_each_column_multiset() {
        let current = sample(24);
        let plan = plan_stats(&current, 0xABCD_1234, StatMode::Shuffle);
        assert_eq!(plan.len(), current.len());
        for field in 0..FIELD_COUNT {
            let mut before: Vec<u16> = current.iter().map(|a| a.stats[field]).collect();
            let mut after: Vec<u16> = plan.iter().map(|a| a.stats[field]).collect();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "column {field} multiset must be preserved");
        }
        // ids are unchanged (the plan re-skins monsters in place).
        for (c, p) in current.iter().zip(&plan) {
            assert_eq!(c.monster_id, p.monster_id);
        }
    }

    /// Random draws stay within the column's value set (no invented stats).
    #[test]
    fn random_draws_from_column_pool() {
        let current = sample(16);
        let plan = plan_stats(&current, 7, StatMode::Random);
        for field in 0..FIELD_COUNT {
            let pool: std::collections::HashSet<u16> =
                current.iter().map(|a| a.stats[field]).collect();
            for a in &plan {
                assert!(pool.contains(&a.stats[field]), "drew an out-of-pool value");
            }
        }
    }

    #[test]
    fn plan_is_deterministic() {
        let current = sample(20);
        let a = plan_stats(&current, 99, StatMode::Shuffle);
        let b = plan_stats(&current, 99, StatMode::Shuffle);
        assert_eq!(a, b, "same seed must reproduce the plan");
    }

    #[test]
    fn empty_roster_is_noop() {
        assert!(plan_stats(&[], 1, StatMode::Shuffle).is_empty());
    }
}
