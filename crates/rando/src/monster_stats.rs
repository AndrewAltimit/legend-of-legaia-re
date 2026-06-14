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
//! A set of scripted enemies ([`PROTECTED_MONSTER_IDS`]) is left untouched so
//! their fights stay coherent. Two kinds qualify. **Early tutorial enemies** —
//! the scripted Rim Elm sparring partner fights the player in a teaching battle
//! the game never expects the player to lose (there is no game-over branch out
//! of it), so giving it a different monster's attack can let it one-shot the
//! party and soft-lock a brand-new game; the first wild enemies are similarly
//! fragile by design, and a late-game monster's stats can wall a fresh save.
//! **Story bosses** — set-piece fights tuned around scripted HP/phase triggers
//! and a specific difficulty; scrambling their stats can make a mandatory fight
//! unwinnable (or trivial), and donating a boss's extreme stats to a random
//! trash mob is its own kind of soft-lock. Every version of each protected boss
//! is pinned. Their combat stats are always kept as the disc ships them, both as
//! a randomization source and target. The encounter randomizer already keeps
//! scripted formations fixed (`crate::encounter`); this is the matching guard on
//! the stat side.
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

/// 1-based monster ids the stat randomizer must never modify.
///
/// 1-based monster ids pinned to their disc stats — never modified, and never a
/// donor into another monster's stats. Two groups (see the module docs):
/// the early **tutorial enemies** (the Piura and the scripted Tetsu sparring
/// partner) that must stay beatable on a fresh save, and the **story bosses**
/// whose set-piece fights randomized stats could break (or whose extreme stats
/// would wreck balance if leaked to a trash mob). Every version of each named
/// boss is listed.
pub const PROTECTED_MONSTER_IDS: &[u16] = &[
    // Early tutorial enemies.
    19, 20, 21, // Red / Black / Blue Piura — the first wild enemies, deliberately weak.
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
    // Story bosses (all versions of each).
    73, 171, 172, // Caruban
    75,  // Zeto
    76, 136, 179, // Songi
    77, 173, 174, // Berserker
    175, // Tetsu (boss form; 79 above is the tutorial form)
    138, // Dohati
    139, // Xain
    162, 163, 164, // Gi / Che / Lu Delilas
    165, 166, // Gaza
    169, // Zora
    170, // Jette
    180, 181, 183, 184, 185, 186, // Cort
];

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
///
/// Monsters in [`PROTECTED_MONSTER_IDS`] (the scripted tutorial fight) are passed
/// through unchanged and excluded from every column pool, so they keep their own
/// stats and never donate them to another monster. Under `Shuffle` this still
/// preserves each column's full multiset: a protected monster contributes the
/// same value before and after, and the rest are a permutation among themselves.
pub fn plan_stats(current: &[StatAssignment], seed: u64, mode: StatMode) -> Vec<StatAssignment> {
    let mut out = current.to_vec();
    // Indices of the monsters eligible for randomization (everything but the
    // protected scripted-fight ids). Protected entries stay byte-identical.
    let free: Vec<usize> = (0..current.len())
        .filter(|&i| !PROTECTED_MONSTER_IDS.contains(&current[i].monster_id))
        .collect();
    if free.is_empty() {
        return out;
    }
    let mut rng = SplitMix64::new(seed);
    for field in 0..FIELD_COUNT {
        let column: Vec<u16> = free.iter().map(|&i| current[i].stats[field]).collect();
        match mode {
            StatMode::Shuffle => {
                let mut bag = column;
                rng.shuffle(&mut bag);
                for (&i, value) in free.iter().zip(bag) {
                    out[i].stats[field] = value;
                }
            }
            StatMode::Random => {
                for &i in &free {
                    out[i].stats[field] = column[rng.below(column.len())];
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
        // Base ids at 100 so the synthetic roster never overlaps the real
        // PROTECTED_MONSTER_IDS (the tutorial enemies) — a test that wants a
        // protected monster sets one id explicitly.
        (0..n)
            .map(|i| StatAssignment {
                monster_id: i as u16 + 100,
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

    /// A protected monster keeps its exact stats and never leaks them into the
    /// pool, while the rest of the roster is still randomized.
    #[test]
    fn protected_monster_is_pinned() {
        let protected = PROTECTED_MONSTER_IDS[0];
        // A roster that includes the protected id, with a recognisable, unique
        // stat block on the protected monster.
        let mut current = sample(24);
        let pidx = 5;
        current[pidx].monster_id = protected;
        let pinned = [4242u16, 4243, 4244, 4245, 4246, 4247, 4248];
        current[pidx].stats = pinned;

        for mode in [StatMode::Shuffle, StatMode::Random] {
            let plan = plan_stats(&current, 0x1234_5678, mode);
            let p = plan.iter().find(|a| a.monster_id == protected).unwrap();
            assert_eq!(
                p.stats, pinned,
                "{mode:?}: protected monster must be pinned"
            );
            // Its unique values never appear on any other monster.
            for a in &plan {
                if a.monster_id == protected {
                    continue;
                }
                for (field, &p) in pinned.iter().enumerate() {
                    assert_ne!(
                        a.stats[field], p,
                        "{mode:?}: protected monster's stats leaked to id {}",
                        a.monster_id
                    );
                }
            }
            // The rest of the roster is actually randomized (not a no-op).
            let moved = current
                .iter()
                .zip(&plan)
                .filter(|(c, p)| c.monster_id != protected && c.stats != p.stats)
                .count();
            assert!(moved > 0, "{mode:?}: non-protected monsters should change");
        }
    }

    /// Shuffle still preserves each column's full multiset even with a protected
    /// monster in the roster (the protected value is conserved in place).
    #[test]
    fn shuffle_with_protected_preserves_full_multiset() {
        let mut current = sample(24);
        current[3].monster_id = PROTECTED_MONSTER_IDS[0];
        let plan = plan_stats(&current, 0xFEED_BEEF, StatMode::Shuffle);
        for field in 0..FIELD_COUNT {
            let mut before: Vec<u16> = current.iter().map(|a| a.stats[field]).collect();
            let mut after: Vec<u16> = plan.iter().map(|a| a.stats[field]).collect();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "column {field} multiset must be preserved");
        }
    }
}
