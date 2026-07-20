//! Core battle arithmetic: PsyQ RNG, spirit damage, MP-cost ability-bit
//! modifiers, accuracy roll, damage caps, art-strike damage, buff ramp.
//! Split out of `battle_formulas.rs`.

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
///
/// `FUN_80035394` is the standalone `SCUS_942.54` form of the same routine -
/// all 19 of its instructions are the record lookup plus this arithmetic:
///
/// ```text
/// 80035398  addiu v1,v1,0x4140    ; 0x80084140
/// 8003539c..800353b0              ; + caster * 0x414
/// 800353b4  lw   v1,0x6bc(v0)     ; 0x80084140 + 0x6BC == char record + 0xF4
/// 800353bc  andi v0,v1,0x20
/// 800353c0  bne  v0,zero,0x800353d4
/// 800353c4  _sra v0,a1,0x1        ; Half: cost >> 1
/// 800353c8  andi v0,v1,0x10
/// 800353cc  beq  v0,zero,0x800353d8
/// 800353d0  _sra v0,a1,0x2        ; Quarter: cost >> 2
/// 800353d4  subu a1,a1,v0
/// ```
///
/// The `0x6BC` displacement off `0x80084140` is what identifies the field: it
/// lands on `0x800847FC`, which is `0x80084708 + 0xF4` - the per-character
/// ability bitfield. The `0x20`-before-`0x10` branch order is the same
/// Half-wins priority [`MpCostModifier::from_ability_flags`] resolves, and it
/// is visible here in the branch structure rather than inferred. Retail's
/// shifts are `sra` (signed); a spell cost is never negative, so the port's
/// unsigned shifts agree over the whole reachable domain.
///
/// PORT: FUN_80035394
/// REF: FUN_801E295C (state `0x28` at `0x801E3D0C` - the inlined copy)
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
