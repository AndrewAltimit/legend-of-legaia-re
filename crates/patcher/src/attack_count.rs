//! Enemy attack-count multiplier: scale how many strikes an enemy's standard
//! (physical) attack turn lands.
//!
//! ## The retail mechanism
//!
//! An enemy's per-turn hit count is not a stored number. When the monster AI
//! picker `FUN_801E9FD4` (overlay 0898) chooses the physical **Attack**
//! category, it fills the actor's action stream at `actor[+0x1DF..]` by
//! repeatedly rolling a candidate out of the monster record's `+0x4C` action
//! entries - those whose tag byte sits in the `0x0C..=0x1F` command band with a
//! usable AGL cost (entry `+0x74 != 0xFF`) - and appending each pick while the
//! per-round **AGL gauge** (`actor[+0x154]`, reset to the record's AGL `+0x0E`
//! each round by `FUN_801D88CC`'s monster branch) still covers its cost. The
//! fill is bounded at 15 queued actions / 16 failed rolls; the attack-chain
//! strike loop (state `0x1E` of `FUN_801E295C`) then resolves **one hit per
//! queued entry** through `FUN_801EC3E4`. So a monster's standard-attack hit
//! count per turn is `AGL / cost`, stochastically, over its attack entries -
//! retail tunes it exactly this way (a one-hit-per-turn enemy carries
//! `cost == AGL`, e.g. cost and AGL both 88; a three-hit boss carries costs
//! near `AGL / 3`). See `docs/subsystems/battle-action.md` (Enemy AGL
//! action-budget) and `ghidra/scripts/funcs/overlay_battle_action_801e9fd4.txt`
//! (candidate collect + budget fill at `0x801EA244..0x801EA3C8`).
//!
//! ## What the multiplier edits
//!
//! A pure same-size data edit to the monster archive (PROT entry 867), like
//! the difficulty scale and the EXP multiplier: each **retail-affordable**
//! attack entry's AGL-cost byte (`+0x74`) is divided by the multiplier, so the
//! unchanged AGL budget affords proportionally more (or fewer) strikes.
//! `2x` halves every attack's price - a two-hit enemy turns into a four-hit
//! one; `0.5x` doubles it. The record's AGL itself is untouched (it is
//! deliberately outside [`crate::monster_stats::STAT_FIELDS`] too), so the
//! knob composes with `--enemy-stat-scale` and never perturbs whatever else
//! reads the stat.
//!
//! Three kinds of entry are left byte-identical, so the slider changes
//! *counts*, never *movesets*:
//!
//! - **Unavailable entries** (`cost == 0xFF`) - the retail "AI never picks
//!   this" sentinel.
//! - **Retail-unaffordable entries** (`cost > AGL`) - deliberate lockouts
//!   (several records carry a variant priced at `200` against an AGL of `99`);
//!   scaling those *down* would unlock moves the designers priced out.
//! - **Zero-cost entries** - free stays free (the same zero-stays-zero rule as
//!   the stat scale).
//!
//! ## Rounding and clamps
//!
//! The multiplier reuses [`ScalePermille`] (`0.1x..=5x`), so the CLI flag and
//! the browser slider share one parser and one rounding rule with the other
//! dials. A scaled cost is `round-half-up(cost / multiplier)` in integer
//! permille arithmetic, then clamped to `1..=min(AGL, 0xFE)`:
//!
//! - the **floor of 1** keeps a cost byte a real price (and the engine's own
//!   15-action fill bound caps runaway growth);
//! - the **AGL ceiling** guarantees every scaled entry stays affordable, so a
//!   multiplier below `1x` reduces an enemy to a *minimum of one* strike per
//!   attack turn, never zero - an enemy that attacks in retail still attacks;
//! - `0xFE` keeps the byte clear of the `0xFF` sentinel.
//!
//! Like the sibling multipliers this composes on the current disc values: it
//! is deterministic and seedless, and re-applying it compounds (the manifest
//! records the setting, and a `1x` scale writes nothing).
//!
//! Only the unwinnable-by-design Rim Elm sparring partner
//! ([`crate::monster_stats::SCALE_PINNED_MONSTER_IDS`]) is pinned, for the
//! same reason as the difficulty scale: the tutorial fight stays
//! byte-identical at every setting.

use crate::monster::repack_slot;
use crate::monster_stats::ScalePermille;
use anyhow::Result;
use legaia_asset::monster_archive::MonsterRecord;

/// Byte offset of the AGL-cost byte inside a `+0x4C` action entry.
pub const COST_OFFSET: usize = 0x74;
/// The command band an entry's tag byte must sit in to be an AI attack
/// candidate (the same `id - 0x0C < 0x14` test the picker's collect loop
/// runs).
pub const ACTION_BAND: std::ops::RangeInclusive<u8> = 0x0C..=0x1F;
/// The "AI never picks this" cost sentinel.
pub const COST_UNAVAILABLE: u8 = 0xFF;
/// Largest cost the scale may write (stays clear of the sentinel).
pub const COST_MAX: u8 = 0xFE;

/// One planned attack-entry cost edit inside a monster's decoded block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostEdit {
    /// Block-relative byte offset of the action entry (the `+0x4C` array
    /// element).
    pub entry_offset: u32,
    /// The entry's current AGL cost.
    pub old_cost: u8,
    /// The scaled AGL cost to write at `entry_offset + 0x74`.
    pub new_cost: u8,
}

/// Scale one attack entry's AGL cost against the monster's AGL budget.
///
/// Returns `None` when the entry must be left alone: the `0xFF` sentinel, a
/// zero cost, a retail-unaffordable price (`cost > agl`), or a scale that
/// lands on the entry's current value (identity). Otherwise the new cost is
/// `round-half-up(cost / scale)` clamped to `1..=min(agl, 0xFE)`.
pub fn scale_cost(cost: u8, agl: u16, scale: ScalePermille) -> Option<u8> {
    if cost == COST_UNAVAILABLE || cost == 0 || cost as u16 > agl {
        return None;
    }
    // Dividing by the multiplier: cost / (permille/1000) = cost * 1000 /
    // permille, rounded half up. `254 * 1000 + 2500` stays well inside u32.
    let scaled = (cost as u32 * 1000 + scale.permille() / 2) / scale.permille();
    let cap = agl.min(COST_MAX as u16) as u32;
    let new = scaled.clamp(1, cap) as u8;
    (new != cost).then_some(new)
}

/// Plan the cost edits for one monster record: every command-band action entry
/// (`0x0C..=0x1F`) whose scaled cost differs from its current one. An identity
/// scale plans nothing.
pub fn plan_record(rec: &MonsterRecord, scale: ScalePermille) -> Vec<CostEdit> {
    if scale.is_retail() {
        return Vec::new();
    }
    let agl = rec.agility();
    rec.spells
        .iter()
        .filter(|s| ACTION_BAND.contains(&s.id))
        .filter_map(|s| {
            scale_cost(s.agl_cost, agl, scale).map(|new_cost| CostEdit {
                entry_offset: s.offset,
                old_cost: s.agl_cost,
                new_cost,
            })
        })
        .collect()
}

/// Write a set of planned cost edits into a monster slot, returning the
/// re-packed slot bytes. Same-size, in place; errors only on the
/// [`repack_slot`] guards (empty/filler slot, LZS failure, re-packed stream
/// overflows the slot). An edit whose entry falls outside the decoded block is
/// skipped (the plan came from the same block, so this only guards a
/// degenerate record).
pub fn set_costs(slot_bytes: &[u8], edits: &[CostEdit]) -> Result<Vec<u8>> {
    repack_slot(slot_bytes, |block| {
        for e in edits {
            if let Some(b) = block.get_mut(e.entry_offset as usize + COST_OFFSET) {
                *b = e.new_cost;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::monster_archive::SLOT_STRIDE;

    fn scale(text: &str) -> ScalePermille {
        ScalePermille::parse(text).unwrap()
    }

    #[test]
    fn scale_cost_divides_and_rounds_half_up() {
        // 2x halves the price: a 2-hit budget becomes a 4-hit one.
        assert_eq!(scale_cost(28, 60, scale("2")), Some(14));
        // 0.5x doubles it (32 -> 64, still affordable under AGL 84).
        assert_eq!(scale_cost(32, 84, scale("0.5")), Some(64));
        // Round half up: 25 / 2 = 12.5 -> 13; 27 / 2 = 13.5 -> 14.
        assert_eq!(scale_cost(25, 99, scale("2")), Some(13));
        assert_eq!(scale_cost(27, 99, scale("2")), Some(14));
    }

    #[test]
    fn scale_cost_clamps_to_the_agl_budget() {
        // A one-hit enemy (cost == AGL) slowed further stays at one hit: the
        // scaled price caps at the budget instead of pricing the attack out.
        assert_eq!(scale_cost(88, 88, scale("0.5")), None, "already at the cap");
        // A cheaper attack scaled up caps at AGL, keeping one strike affordable.
        assert_eq!(scale_cost(56, 60, scale("0.5")), Some(60));
        // The floor of 1 (and the sentinel guard at the top).
        assert_eq!(
            scale_cost(1, 200, scale("5")),
            None,
            "1 / 5 floors back to 1"
        );
        assert_eq!(scale_cost(3, 200, scale("5")), Some(1));
    }

    #[test]
    fn scale_cost_leaves_sentinels_lockouts_and_zero_alone() {
        // 0xFF = "AI never picks this".
        assert_eq!(scale_cost(0xFF, 99, scale("2")), None);
        // Retail-unaffordable lockout (cost 200 against AGL 99) is not
        // unlocked by a faster setting, nor scaled by a slower one.
        assert_eq!(scale_cost(200, 99, scale("2")), None);
        assert_eq!(scale_cost(200, 99, scale("0.5")), None);
        // Free stays free.
        assert_eq!(scale_cost(0, 99, scale("2")), None);
        // Identity plans nothing.
        assert_eq!(scale_cost(28, 60, scale("1")), None);
    }

    #[test]
    fn scale_cost_stays_clear_of_the_ff_sentinel() {
        // A huge AGL would let the cap reach 0xFF; it must stop at 0xFE.
        assert_eq!(scale_cost(200, 1000, scale("0.5")), Some(254));
    }

    /// A synthetic decoded block shaped like a real record head: name offset,
    /// AGL, a `+0x4A` entry count and a `+0x4C` offset array pointing at
    /// `0x80`-byte-stride action entries.
    fn fake_block(agl: u16, entries: &[(u8, u8)]) -> Vec<u8> {
        let mut block = vec![0u8; 0x400 + entries.len() * 0x100];
        // Name offset -> a printable NUL-terminated string.
        block[0..4].copy_from_slice(&0x60u32.to_le_bytes());
        block[0x60..0x63].copy_from_slice(b"Mob");
        block[0x0E..0x10].copy_from_slice(&agl.to_le_bytes());
        block[0x4A] = entries.len() as u8;
        for (i, (id, cost)) in entries.iter().enumerate() {
            let off = 0x400 + i * 0x100;
            block[0x4C + i * 4..0x4C + i * 4 + 4].copy_from_slice(&(off as u32).to_le_bytes());
            block[off] = *id;
            block[off + COST_OFFSET] = *cost;
        }
        block
    }

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

    fn record_of(block: &[u8]) -> MonsterRecord {
        let slot = fake_slot(block);
        legaia_asset::monster_archive::record(&slot, 1)
            .unwrap()
            .expect("populated record")
    }

    #[test]
    fn plan_record_touches_only_scalable_band_entries() {
        // AGL 60: one plain attack (0x0D, 28), one lockout (0x13, 200), one
        // sentinel (0x0E, 0xFF), one out-of-band reaction entry (0x04, 12).
        let block = fake_block(60, &[(0x0D, 28), (0x13, 200), (0x0E, 0xFF), (0x04, 12)]);
        let rec = record_of(&block);
        let plan = plan_record(&rec, scale("2"));
        assert_eq!(
            plan,
            vec![CostEdit {
                entry_offset: 0x400,
                old_cost: 28,
                new_cost: 14,
            }],
            "only the affordable band entry is planned"
        );
        assert!(
            plan_record(&rec, scale("1")).is_empty(),
            "identity plans nothing"
        );
    }

    #[test]
    fn set_costs_changes_only_the_cost_bytes() {
        let block = fake_block(84, &[(0x0D, 32), (0x0F, 60)]);
        let rec = record_of(&block);
        let plan = plan_record(&rec, scale("0.5"));
        // 32 -> 64, 60 -> 84 (capped at AGL).
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].new_cost, 64);
        assert_eq!(plan[1].new_cost, 84);

        let slot = fake_slot(&block);
        let patched = set_costs(&slot, &plan).unwrap();
        assert_eq!(patched.len(), SLOT_STRIDE);
        let out = decode_slot(&patched);
        let mut expected = block.clone();
        expected[0x400 + COST_OFFSET] = 64;
        expected[0x500 + COST_OFFSET] = 84;
        assert_eq!(out, expected, "only the two cost bytes changed");
    }

    #[test]
    fn min_one_strike_survives_the_slowest_setting() {
        // At 0.1x every price would 10x; the AGL cap keeps at least one
        // attack affordable on any enemy that could attack in retail.
        let block = fake_block(52, &[(0x0D, 27), (0x12, 29)]);
        let rec = record_of(&block);
        let plan = plan_record(&rec, scale("0.1"));
        assert_eq!(plan.len(), 2);
        for e in &plan {
            assert_eq!(e.new_cost as u16, 52, "capped at the AGL budget");
        }
    }
}
