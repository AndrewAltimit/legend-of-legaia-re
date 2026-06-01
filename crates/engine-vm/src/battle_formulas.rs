//! Battle damage / cost / RNG formulas.
//!
//! Clean-room Rust port of the in-game battle math. Each function is keyed
//! to a citation in `docs/subsystems/battle-formulas.md` so the provenance
//! stays traceable. None of these functions touch `FUN_800402F4`'s full
//! selector-dispatch - that lives next to the state machine in
//! [`crate::battle_action`]. This module is the **arithmetic kernel** that
//! every selector eventually feeds into.
//!
//! PORT: FUN_80056798 (PsyQ rand; full per-formula attribution lives on
//! the individual `pub fn` docs below).
//! PORT: FUN_800402F4 (selector-dispatch lives in battle_action; this
//! module ports the arithmetic kernel the dispatch feeds into).
//! REF: FUN_801E295C, FUN_801EED1C

#![allow(clippy::too_many_arguments)]

/// PsyQ-shape 32-bit linear congruential RNG. Returned value is the high-15
/// bits, in the range `0..=0x7FFF`. The seed is mutated in place.
///
/// Identical to PSX libc `rand()`, which is what the game uses
/// (`FUN_80056798`, `ghidra/scripts/funcs/80056798.txt`). For deterministic
/// replay the engine must seed this from the same boot-time source the
/// retail game uses; the precise source is currently the SPU master clock
/// at boot, captured in `_DAT_8007AE5C`.
pub fn psyq_rand_step(seed: &mut u32) -> u16 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    ((*seed >> 16) & 0x7FFF) as u16
}

/// Spirit super-art damage. Hard-coded per battle-action state 0x3E / 0x46:
/// `damage = ((target_hp * 7) / 5) + 8`, capped.
///
/// `cap` is the per-spell ceiling - battle-action.md observes 288 (`0x120`)
/// for the larger spirit arts and 100 for the smaller ones.
pub fn spirit_damage(target_hp: u16, cap: u16) -> u16 {
    // saturating math: target_hp * 7 fits in u32 since target_hp <= 0xFFFF
    let raw = (target_hp as u32 * 7) / 5 + 8;
    raw.min(cap as u32) as u16
}

/// Modifier classes for [`mp_cost_after_ability_bits`]. The bit checks the
/// retail engine performs are `0x10` and `0x20` against the character
/// record at `+0xF4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpCostModifier {
    /// No ability-bit modifier - pay full cost.
    Full,
    /// `+0xF4 & 0x20` set - cost reduced *by* half: `cost - (cost >> 1)`.
    Half,
    /// `+0xF4 & 0x10` set - cost reduced *by* a quarter (pay 3/4):
    /// `cost - (cost >> 2)`. NOT "cost becomes a quarter" - the bit shaves
    /// 25% off, where `0x20` (Half) shaves 50% off.
    Quarter,
}

impl MpCostModifier {
    /// Resolve the modifier from a 32-bit ability-flag word, reading `+0xF4`
    /// and testing `0x20` (Half) before `0x10` (Quarter).
    ///
    /// PRIORITY (dump-confirmed): when both bits are set, **Half (`0x20`) wins**.
    /// The retail state-`0x28` block (`FUN_801E295C` at `0x801E3D0C`) is
    /// `andi 0x20; bne <half>` then `andi 0x10; beq <none>` - i.e.
    /// `if (bits & 0x20) { half } else if (bits & 0x10) { quarter }`, with Half
    /// short-circuiting the `0x10` test. This `Half`-first order matches the
    /// docs; the earlier engine SM port / live cast path that applied Quarter
    /// first were a guess and are now corrected.
    pub fn from_ability_flags(flags: u32) -> Self {
        if flags & 0x20 != 0 {
            MpCostModifier::Half
        } else if flags & 0x10 != 0 {
            MpCostModifier::Quarter
        } else {
            MpCostModifier::Full
        }
    }
}

/// Apply the [`MpCostModifier`] to a base spell MP cost. Mirrors the
/// state-`0x28` body of `FUN_801E295C` (`0x801E3D0C`): the modifier subtracts a
/// right-shifted copy of the cost (`cost -= cost >> 1` for Half, `cost -= cost
/// >> 2` for Quarter), NOT a floor-divide - so Half rounds *up* on odd costs
/// (`7 -> 4`, not `3`) and Quarter shaves only 25% off (`40 -> 30`, not `10`).
pub fn mp_cost_after_ability_bits(base_cost: u16, modifier: MpCostModifier) -> u16 {
    match modifier {
        MpCostModifier::Full => base_cost,
        MpCostModifier::Half => base_cost - (base_cost >> 1),
        MpCostModifier::Quarter => base_cost - (base_cost >> 2),
    }
}

/// Hit / evasion roll, selector 9 of `FUN_800402F4`.
///
/// Returns `true` if the attack lands. Probability:
///
/// ```text
/// p_hit = caster_acc / (caster_acc + target_eva)
/// ```
///
/// Computed in the retail engine as `roll = rand() % (caster + target);
/// hit = (target < roll)`, which is equivalent.
///
/// If both stats are zero the roll modulus is undefined - we treat that as
/// an automatic hit (matches retail behavior, which would have crashed on
/// `% 0` but never sees both stats simultaneously zero in practice).
pub fn accuracy_roll(caster_acc: u16, target_eva: u16, rng_seed: &mut u32) -> bool {
    let denom = caster_acc as u32 + target_eva as u32;
    if denom == 0 {
        return true;
    }
    let r = psyq_rand_step(rng_seed) as u32;
    let roll = r % denom;
    (target_eva as u32) < roll
}

/// Stat cap table for party slots 0..2 - cap halfwords at `DAT_8007655C`.
/// The table is six halfwords; party slots index it directly.
///
/// Engines that load the cap table from a real `extracted/SCUS_942.54` byte
/// pool can pass it here as the `caps` slice; the unit tests embed a
/// reasonable default (10000 / 9999 / 999 - generous, matches the
/// per-actor shipping caps the game enforces in stat-up animations) so
/// callers without disc data still get monotonic damage scaling.
pub fn damage_cap_for_party_slot(caps: &[u16; 6], party_slot: u8) -> u16 {
    let idx = party_slot.min(5) as usize;
    caps[idx]
}

/// Art-strike damage. One per-strike call into the HP-deduction kernel
/// (`FUN_801EED1C` in the battle overlay, dispatched from
/// `BattleActionHost::apply_art_strike`).
///
/// Formula:
///
/// ```text
/// raw      = attack × power_multiplier / power_divisor
/// damage   = max(min_floor, raw.saturating_sub(defense))
/// ```
///
/// `power_divisor` is the fixed-point base for the multiplier table.
/// The retail engine appears to use `divisor = 16`, giving multipliers in
/// `12..=28` the fractional range `0.75..=1.75` against the target defense.
/// `min_floor` is the in-game minimum-damage floor (1 in vanilla - the
/// retail engine never deals zero damage on a successful strike unless the
/// target is invulnerable).
///
/// Saturating arithmetic is used end-to-end so absurd inputs (e.g.
/// captured trace replay where a stat overflowed) don't panic.
pub fn art_strike_damage(
    attack: u16,
    defense: u16,
    power_multiplier: u8,
    power_divisor: u8,
    min_floor: u16,
) -> u16 {
    if power_divisor == 0 {
        return min_floor;
    }
    let raw = (attack as u32 * power_multiplier as u32) / power_divisor as u32;
    let after_def = raw.saturating_sub(defense as u32);
    after_def.max(min_floor as u32).min(0xFFFF) as u16
}

/// Convenience wrapper using the documented `divisor = 16, min_floor = 1`.
pub fn art_strike_damage_default(attack: u16, defense: u16, power_multiplier: u8) -> u16 {
    art_strike_damage(attack, defense, power_multiplier, 16, 1)
}

/// Standard "stat-up by 20%" ramp from selectors 1..7.
///
/// Mirrors the retail check `value * (6/5)` if `value * 6/5 < 0xFFFF`,
/// else clamps to `0xFFFF`. The retail dump uses the magic constant
/// `0x4cccccccd >> 0x22` for the comparison; that's just the "is the
/// post-ramp value still under `0xFFFF`?" check expressed as a multiply +
/// shift to avoid the cost of a divide.
pub fn buff_ramp(value: u16) -> u16 {
    let next = value as u32 + (value as u32 / 5);
    if next >= 0xFFFF { 0xFFFF } else { next as u16 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the spirit-damage formula on a few hand-checked points. The
    /// `cap = 0x120 = 288` cap is the larger of the two ceilings observed
    /// in the state machine.
    #[test]
    fn spirit_damage_matches_doc() {
        assert_eq!(spirit_damage(0, 288), 8); // floor at +8
        assert_eq!(spirit_damage(100, 288), 148); // 100*7/5 + 8 = 148
        assert_eq!(spirit_damage(200, 288), 288); // 200*7/5+8 = 288 = cap
        assert_eq!(spirit_damage(500, 288), 288); // overflow → cap
        assert_eq!(spirit_damage(50, 100), 78); // smaller cap (100), under
        assert_eq!(spirit_damage(150, 100), 100); // smaller cap, clipped
    }

    #[test]
    fn mp_cost_modifier_resolves() {
        assert_eq!(
            MpCostModifier::from_ability_flags(0x00),
            MpCostModifier::Full
        );
        assert_eq!(
            MpCostModifier::from_ability_flags(0x10),
            MpCostModifier::Quarter
        );
        assert_eq!(
            MpCostModifier::from_ability_flags(0x20),
            MpCostModifier::Half
        );
        // Half wins when both bits set (matches the `if/else if` chain).
        assert_eq!(
            MpCostModifier::from_ability_flags(0x30),
            MpCostModifier::Half
        );
    }

    #[test]
    fn mp_cost_arithmetic() {
        assert_eq!(mp_cost_after_ability_bits(40, MpCostModifier::Full), 40);
        // Half = cost - cost>>1; Quarter = cost - cost>>2 (shave 25%, not "/4").
        assert_eq!(mp_cost_after_ability_bits(40, MpCostModifier::Half), 20);
        assert_eq!(mp_cost_after_ability_bits(40, MpCostModifier::Quarter), 30);
        // Odd cost: Half rounds UP (7 - 7>>1 = 7 - 3 = 4); Quarter 7 - 1 = 6.
        assert_eq!(mp_cost_after_ability_bits(7, MpCostModifier::Half), 4);
        assert_eq!(mp_cost_after_ability_bits(7, MpCostModifier::Quarter), 6);
    }

    #[test]
    fn psyq_rand_is_deterministic_from_seed() {
        let mut a = 0x12345678;
        let mut b = 0x12345678;
        // Ten draws from identical seeds produce identical values.
        for _ in 0..10 {
            assert_eq!(psyq_rand_step(&mut a), psyq_rand_step(&mut b));
        }
        // ...but two draws are not equal in general.
        let mut s = 0x12345678;
        let r1 = psyq_rand_step(&mut s);
        let r2 = psyq_rand_step(&mut s);
        assert_ne!(r1, r2);
    }

    #[test]
    fn psyq_rand_in_range() {
        let mut seed = 1;
        for _ in 0..1000 {
            let r = psyq_rand_step(&mut seed);
            assert!(r <= 0x7FFF);
        }
    }

    #[test]
    fn accuracy_roll_zero_stats_auto_hits() {
        let mut s = 0;
        assert!(accuracy_roll(0, 0, &mut s));
    }

    #[test]
    fn accuracy_roll_high_caster_hits_more() {
        // 100 vs 1: the roll is `rand % 101`; we need `target < roll`,
        // i.e. `1 < roll`, which is true except when `roll = 0` or `1`.
        // Two failures in 101 outcomes - over many seeds we should land
        // close to >=98% hit rate.
        let mut hits = 0;
        let mut s = 1;
        for _ in 0..1000 {
            if accuracy_roll(100, 1, &mut s) {
                hits += 1;
            }
        }
        assert!(hits > 950, "expected >95% hit rate, got {}", hits / 10);
    }

    #[test]
    fn buff_ramp_increments_by_20pct_then_clamps() {
        assert_eq!(buff_ramp(0), 0);
        assert_eq!(buff_ramp(100), 120);
        assert_eq!(buff_ramp(50_000), 60_000);
        // Just under the clamp threshold.
        assert_eq!(buff_ramp(54_613), 65_535);
        // Over → clamp at 0xFFFF.
        assert_eq!(buff_ramp(60_000), 0xFFFF);
        assert_eq!(buff_ramp(0xFFFF), 0xFFFF);
    }

    #[test]
    fn art_strike_damage_basic_arithmetic() {
        // attack=64, def=10, mult=16, div=16 → 64 - 10 = 54.
        assert_eq!(art_strike_damage(64, 10, 16, 16, 1), 54);
        // attack=64, def=10, mult=12 → (64 * 12) / 16 = 48; 48 - 10 = 38.
        assert_eq!(art_strike_damage(64, 10, 12, 16, 1), 38);
        // attack=64, def=10, mult=28 → (64 * 28)/16 = 112; 112-10 = 102.
        assert_eq!(art_strike_damage(64, 10, 28, 16, 1), 102);
    }

    #[test]
    fn art_strike_damage_floor_when_def_exceeds_attack() {
        // High defense should clamp to floor, not underflow.
        assert_eq!(art_strike_damage(10, 100, 16, 16, 1), 1);
        // Custom floor.
        assert_eq!(art_strike_damage(10, 100, 16, 16, 5), 5);
    }

    #[test]
    fn art_strike_damage_zero_divisor_returns_floor() {
        assert_eq!(art_strike_damage(64, 10, 16, 0, 1), 1);
    }

    #[test]
    fn art_strike_damage_saturates_at_u16_max() {
        // attack=0xFFFF, mult=28, div=1 -> raw overflows u16 -> clamps.
        assert_eq!(art_strike_damage(0xFFFF, 0, 28, 1, 1), 0xFFFF);
    }

    #[test]
    fn art_strike_damage_default_uses_div_16_floor_1() {
        assert_eq!(
            art_strike_damage_default(64, 10, 16),
            art_strike_damage(64, 10, 16, 16, 1)
        );
    }

    #[test]
    fn damage_cap_clamps_to_party_slots() {
        let caps = [200, 250, 300, 999, 999, 999];
        assert_eq!(damage_cap_for_party_slot(&caps, 0), 200);
        assert_eq!(damage_cap_for_party_slot(&caps, 2), 300);
        // Out-of-range falls back to the last entry.
        assert_eq!(damage_cap_for_party_slot(&caps, 10), 999);
    }
}
