//! Monster combat-stat randomizer: redistribute HP / MP / ATK / DEF / INT / SPD
//! across the enemy roster.
//!
//! Each monster's combat stats live as `u16` halfwords at fixed offsets in the
//! decoded `battle_data` block (PROT entry 867; offsets pinned in
//! [`legaia_asset::monster_archive`]). [`plan_stats`] collects each field's
//! value across the whole populated roster into a *column*, then either permutes
//! the column ([`StatMode::Shuffle`] - a 1:1 reassignment, so the multiset of
//! each stat is preserved) or draws each cell from the column pool with
//! replacement ([`StatMode::Random`]). Because every value that lands came from
//! a real monster, the global stat budget is preserved and no field is ever
//! pushed outside the game's own range - a tanky enemy may inherit a weakling's
//! HP while keeping its own attack, scrambling difficulty without producing
//! impossible records.
//!
//! AGL (`+0x0E`, the agility / action gauge) is **deliberately left alone** - it
//! gates the enemy AI's action economy (how many actions / casts it can afford
//! per round) rather than player-facing difficulty, and shuffling it would
//! mostly perturb how often enemies act, not how hard the fight is.
//!
//! A set of scripted enemies ([`PROTECTED_MONSTER_IDS`]) is left untouched so
//! their fights stay coherent. Two kinds qualify. **Early tutorial enemies** -
//! the scripted Rim Elm sparring partner fights the player in a teaching battle
//! the game never expects the player to lose (there is no game-over branch out
//! of it), so giving it a different monster's attack can let it one-shot the
//! party and soft-lock a brand-new game; the first wild enemies are similarly
//! fragile by design, and a late-game monster's stats can wall a fresh save.
//! **Story bosses** - set-piece fights tuned around scripted HP/phase triggers
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
//! `0x14000`-byte footprint and every other monster's slot offset is fixed - a
//! same-size, in-place byte edit, exactly like the drop randomizer.
//!
//! ## Difficulty scale (the multiplier)
//!
//! [`plan_scale`] is the module's second, *seedless* pass: instead of moving
//! values between monsters it multiplies every monster's stats by one global
//! factor ([`ScalePermille`], `0.1x..=5x`), so the whole roster gets uniformly
//! weaker or stronger while every monster keeps its own relative profile. It
//! composes with the randomizer - run the shuffle first and the scale multiplies
//! the *shuffled* values.
//!
//! Two things about the scale differ from the randomizer above, and both are
//! deliberate:
//!
//! - **Bosses are scaled.** A difficulty knob that skipped the set-piece fights
//!   would leave the hardest fights in the game untouched, which is the opposite
//!   of what it is for. The shuffle's [`PROTECTED_MONSTER_IDS`] guard exists
//!   because *reassigning* a boss's stats breaks a scripted fight; multiplying
//!   them keeps every fight's shape and only moves its difficulty. The one
//!   carve-out is [`SCALE_PINNED_MONSTER_IDS`].
//! - **AGL is still untouched.** `+0x0E` is the action gauge, not a difficulty
//!   stat: scaling it multiplies how many actions an enemy gets per round, which
//!   turns a 5x run into a slideshow of enemy turns rather than a harder fight.
//!   [`STAT_FIELDS`] already excludes it, so the scale inherits the exclusion.
//!
//! Rewards (EXP `+0x46` / gold `+0x44` / the drop slot) are outside
//! [`STAT_FIELDS`] and never move, so a 5x run does not also pay out 5x.
//!
//! The scale lands on the **record**, and the battle loader applies its own
//! fixed boost on top when it copies a record into a live actor (`FUN_80054cb0`
//! multiplies ATK / DEF / INT - see [`legaia_asset::monster_archive`]). The two
//! compose multiplicatively, so an `Nx` record really is an `Nx` fight, up to
//! the loader's own integer rounding.

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
    ("intelligence", 0x18),
    ("speed", 0x1A),
];

/// Number of stat fields a [`StatAssignment`] carries.
pub const FIELD_COUNT: usize = STAT_FIELDS.len();

/// 1-based monster ids the stat randomizer must never modify.
///
/// 1-based monster ids pinned to their disc stats - never modified, and never a
/// donor into another monster's stats. Two groups (see the module docs):
/// the early **tutorial enemies** (the Piura and the scripted Tetsu sparring
/// partner) that must stay beatable on a fresh save, and the **story bosses**
/// whose set-piece fights randomized stats could break (or whose extreme stats
/// would wreck balance if leaked to a trash mob). Every version of each named
/// boss is listed.
pub const PROTECTED_MONSTER_IDS: &[u16] = &[
    // Early tutorial enemies.
    19, 20, 21, // Red / Black / Blue Piura - the first wild enemies, deliberately weak.
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
    // Story bosses (all versions of each).
    10, // Gimard - the early scripted Seru-boss fight (also guarded on the encounter side).
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
    /// (`[hp, mp, attack, defense_high, defense_low, intelligence, speed]`).
    pub stats: [u16; FIELD_COUNT],
}

/// A monster's overall **combat power**: the sum of its damage-relevant combat
/// stats - every [`STAT_FIELDS`] entry **except MP**
/// (`hp + attack + defense_high + defense_low + agility + speed`). A single
/// scalar standing in for a monster's whole stat budget, so a swapped-in monster
/// can be compared against an area's native average (the
/// solo-strong-encounter option). MP is excluded for the same reason the AGL
/// gauge is left out of the stat shuffle: it gates the enemy's action economy,
/// not raw danger. Saturating, so a degenerate record can never overflow.
pub fn combat_power(stats: &[u16; FIELD_COUNT]) -> u32 {
    // STAT_FIELDS order: hp, mp, attack, defense_high, defense_low, intelligence, speed.
    let [hp, _mp, atk, dhi, dlo, intl, spd] = *stats;
    [hp, atk, dhi, dlo, intl, spd]
        .into_iter()
        .fold(0u32, |acc, v| acc.saturating_add(v as u32))
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

/// 1-based monster ids the **difficulty scale** must never touch.
///
/// Much shorter than [`PROTECTED_MONSTER_IDS`], because a uniform multiplier
/// keeps every fight's shape (see the module docs) - story bosses are scaled on
/// purpose. What a multiplier *can* break is a fight whose script depends on the
/// player being unable to end it: the Rim Elm sparring partner is unwinnable by
/// design and has no branch for the player winning, so scaling it *down* turns
/// the tutorial into a soft-lock. Its stats are pinned in both directions rather
/// than only below `1x`, so the fight is byte-identical at every setting.
pub const SCALE_PINNED_MONSTER_IDS: &[u16] = &[
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
];

/// A monster-stat difficulty multiplier, held as **permille** (thousandths) so
/// the plan is exact integer arithmetic and a given setting always reproduces
/// byte-identically - no float ever reaches the disc.
///
/// Range [`MIN`](Self::MIN)`..=`[`MAX`](Self::MAX) (`0.1x..=5x`);
/// [`RETAIL`](Self::RETAIL) (`1x`) is the identity and applies no edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScalePermille(u32);

impl ScalePermille {
    /// Weakest setting: `0.1x`.
    pub const MIN: u32 = 100;
    /// Strongest setting: `5x`.
    pub const MAX: u32 = 5000;
    /// The identity multiplier - retail stats, no edit.
    pub const RETAIL: u32 = 1000;

    /// Build from a raw permille value, rejecting anything outside
    /// [`MIN`](Self::MIN)`..=`[`MAX`](Self::MAX).
    pub fn from_permille(permille: u32) -> Result<Self, String> {
        if !(Self::MIN..=Self::MAX).contains(&permille) {
            return Err(format!(
                "enemy stat scale {} is out of range (want {}..={})",
                Self(permille),
                Self(Self::MIN),
                Self(Self::MAX),
            ));
        }
        Ok(Self(permille))
    }

    /// Parse a user-facing multiplier: `"2.5"`, `"2.5x"`, `"0.1"`, `"1"`.
    /// Rounded to the nearest permille, then range-checked. The shared entry
    /// point for the CLI flag and the browser slider, so both accept exactly the
    /// same values and produce the same bytes.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim().trim_end_matches(['x', 'X']).trim();
        let value: f64 = t
            .parse()
            .map_err(|_| format!("{text:?} is not a number (want a multiplier like 2.5)"))?;
        if !value.is_finite() || value < 0.0 {
            return Err(format!("{text:?} is not a usable multiplier"));
        }
        Self::from_permille((value * 1000.0).round() as u32)
    }

    /// The multiplier in thousandths (`2500` = `2.5x`).
    pub fn permille(self) -> u32 {
        self.0
    }

    /// Whether this is the identity multiplier (retail stats).
    pub fn is_retail(self) -> bool {
        self.0 == Self::RETAIL
    }
}

impl std::fmt::Display for ScalePermille {
    /// `2500` -> `2.5x`, `1000` -> `1x` (trailing zeros trimmed).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (whole, frac) = (self.0 / 1000, self.0 % 1000);
        if frac == 0 {
            write!(f, "{whole}x")
        } else {
            write!(f, "{whole}.{}x", format!("{frac:03}").trim_end_matches('0'))
        }
    }
}

/// Multiply one monster's stats by `scale`.
///
/// Each [`STAT_FIELDS`] halfword is scaled independently with
/// round-half-up integer arithmetic, then held inside the record's own `u16`
/// range. Two clamps matter:
///
/// - **A zero stays zero.** A monster with no MP has no MP at any multiplier;
///   scaling would otherwise invent a resource it never had.
/// - **A non-zero never reaches zero.** At `0.1x` a 5-HP enemy floors at `1`
///   rather than becoming a zero-HP actor the battle code never expects.
///
/// The top end saturates at [`u16::MAX`], so a `5x` run on a boss already past
/// `13107` HP simply pins that stat at the record's ceiling.
pub fn scale_stats(stats: &[u16; FIELD_COUNT], scale: ScalePermille) -> [u16; FIELD_COUNT] {
    let mut out = *stats;
    for v in &mut out {
        if *v == 0 {
            continue;
        }
        // Round half up. `u16::MAX * 5000 + 500` stays well inside u32.
        let scaled = (*v as u32 * scale.permille() + 500) / 1000;
        *v = scaled.clamp(1, u16::MAX as u32) as u16;
    }
    out
}

/// Plan a uniform difficulty scale over the roster. `current` holds
/// `(id, stats)` for every populated monster; the returned plan is the same
/// monsters with each stat multiplied by `scale`.
///
/// Seedless and total: the result depends only on `(current, scale)`.
/// Monsters in [`SCALE_PINNED_MONSTER_IDS`] pass through untouched; everything
/// else - story bosses included - is scaled. A `1x` scale is the identity, so
/// the caller writes nothing.
pub fn plan_scale(current: &[StatAssignment], scale: ScalePermille) -> Vec<StatAssignment> {
    current
        .iter()
        .map(|a| {
            if SCALE_PINNED_MONSTER_IDS.contains(&a.monster_id) {
                *a
            } else {
                StatAssignment {
                    monster_id: a.monster_id,
                    stats: scale_stats(&a.stats, scale),
                }
            }
        })
        .collect()
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
        // PROTECTED_MONSTER_IDS (the tutorial enemies) - a test that wants a
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

    fn scale(text: &str) -> ScalePermille {
        ScalePermille::parse(text).expect("valid scale")
    }

    /// The user-facing multiplier round-trips through permille and back to a
    /// display string, and the accepted spellings agree.
    #[test]
    fn scale_parses_and_displays() {
        assert_eq!(scale("1").permille(), 1000);
        assert_eq!(scale("2.5").permille(), 2500);
        assert_eq!(scale("2.5x").permille(), 2500);
        assert_eq!(scale(" 0.1 ").permille(), 100);
        assert_eq!(scale("5").permille(), 5000);
        assert_eq!(scale("1").to_string(), "1x");
        assert_eq!(scale("2.5").to_string(), "2.5x");
        assert_eq!(scale("0.1").to_string(), "0.1x");
        assert!(scale("1").is_retail());
        assert!(!scale("1.1").is_retail());
    }

    /// Out-of-range and non-numeric settings are refused, not clamped - a typo
    /// must not silently become a different difficulty.
    #[test]
    fn scale_rejects_out_of_range() {
        for bad in ["0", "0.05", "5.1", "10", "-2", "abc", ""] {
            assert!(
                ScalePermille::parse(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    /// A multiplier scales every stat field, rounding half up.
    #[test]
    fn scale_multiplies_every_field() {
        let stats = [100u16, 40, 25, 30, 35, 45, 55];
        assert_eq!(scale_stats(&stats, scale("1")), stats, "1x is the identity");
        assert_eq!(
            scale_stats(&stats, scale("2")),
            [200, 80, 50, 60, 70, 90, 110]
        );
        // 25 * 0.5 = 12.5 -> 13 (half up); 35 * 0.5 = 17.5 -> 18.
        assert_eq!(
            scale_stats(&stats, scale("0.5")),
            [50, 20, 13, 15, 18, 23, 28]
        );
    }

    /// The two clamps: a zero stat stays zero (no invented MP), a non-zero one
    /// never rounds down to zero, and the top saturates inside `u16`.
    #[test]
    fn scale_clamps_at_both_ends() {
        let sparse = [5u16, 0, 1, 0, 0, 2, 0];
        let down = scale_stats(&sparse, scale("0.1"));
        assert_eq!(
            down,
            [1, 0, 1, 0, 0, 1, 0],
            "zeros stay zero, non-zeros floor at 1"
        );

        let huge = [60000u16, 60000, 60000, 60000, 60000, 60000, 60000];
        assert_eq!(
            scale_stats(&huge, scale("5")),
            [u16::MAX; FIELD_COUNT],
            "the record's own u16 ceiling holds"
        );
    }

    /// The scale is seedless, total, and hits bosses - only the pinned tutorial
    /// fight passes through untouched.
    #[test]
    fn plan_scale_covers_bosses_but_pins_the_tutorial() {
        let mut current = sample(24);
        // A story boss (protected against the *shuffle*) and the pinned
        // tutorial partner, side by side.
        let boss = 138; // Dohati
        current[2].monster_id = boss;
        current[7].monster_id = SCALE_PINNED_MONSTER_IDS[0];
        let pinned_stats = current[7].stats;

        let plan = plan_scale(&current, scale("2"));
        assert_eq!(plan.len(), current.len());
        assert!(
            PROTECTED_MONSTER_IDS.contains(&boss),
            "the chosen id must be shuffle-protected, to prove the scale differs"
        );
        let b = plan.iter().find(|a| a.monster_id == boss).unwrap();
        let c = current.iter().find(|a| a.monster_id == boss).unwrap();
        assert_eq!(
            b.stats,
            scale_stats(&c.stats, scale("2")),
            "story bosses are scaled"
        );
        let p = plan
            .iter()
            .find(|a| a.monster_id == SCALE_PINNED_MONSTER_IDS[0])
            .unwrap();
        assert_eq!(p.stats, pinned_stats, "the tutorial fight is pinned");

        // Identity at 1x, and deterministic without a seed.
        assert_eq!(plan_scale(&current, scale("1")), current, "1x is a no-op");
        assert_eq!(
            plan_scale(&current, scale("0.4")),
            plan_scale(&current, scale("0.4"))
        );
        assert!(plan_scale(&[], scale("3")).is_empty());
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
