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
//! PORT: FUN_801DD0AC (damage roll - both branches. Summon branch
//! (`attacker_slot == 7`): `summon_attacker_roll` / `summon_defender_roll` /
//! `summon_bonus_roll` / `summon_predamage`. Arts/physical branch
//! (`attacker_slot != 7`, seeded by the `0x801F4F5C` move-power table):
//! `arts_attacker_roll` / `arts_bonus_roll` / `arts_physical_predamage`
//! (defender roll shared with the summon branch). The live `FUN_801DDB30`
//! mitigation/finisher glue is not reproduced here - see the REF below.)
//! PORT: FUN_801DD864 (summon-roll scale stage - `apply_element_affinity` /
//! `apply_status_weaken` / `apply_magic_power`).
//! PORT: FUN_8004E568 (victory-spoils gold + EXP scaling - `victory_gold_*` /
//! `victory_exp_per_member`. The reward resolver's drop roll + level-up
//! application live in engine-core `apply_battle_loot` / `apply_battle_xp`.)
//! PORT: FUN_801DDB30 (damage finisher - the closed-form damage-finalisation
//! arithmetic (`damage_finish`: equipment elemental-resistance halving, the
//! guard halve, the no-damage `rand%9+8` floor, the summon power-percent scale,
//! the 9999 cap) + the spirit-gauge fill (`spirit_gauge_fill`). The finisher's
//! state-mutating tail - damage-popup accumulator, AI revenge table, MP drain,
//! and the per-element stat-debuff switch - reads/writes ~20 battle globals and
//! stays in the live battle context; see the REF below + `damage_finish` docs.)
//! REF: FUN_801E295C, FUN_801EED1C (the action-SM glue that drives the kernels
//! and applies the finisher's coupled global side effects).

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

// ---------------------------------------------------------------------------
// Summon-magic damage roll (FUN_801dd0ac summon branch + FUN_801dd864 modifiers)
// ---------------------------------------------------------------------------
//
// A player Seru-magic *damage* summon (PROT 0904 / 0912 / 0914) applies its HP
// delta through the shared battle kernel `FUN_801dd0ac`
// (`overlay_battle_action_801dd0ac.txt`) with `attacker_slot == 7` (the summon
// body's actor slot). There is **no static per-spell power scalar** for summons
// (see [`crate::battle_action`] / `docs/formats/spell-table.md`); the magnitude
// is built from live battle stats in three stages:
//
//   1. **Roll** (`FUN_801dd0ac`): an attacker roll and a defender roll, each a
//      `rand % stat_term + stat_terms` sum ([`summon_attacker_roll`],
//      [`summon_defender_roll`]).
//   2. **Scale** (`FUN_801dd864`): the attacker roll is scaled by the
//      element-affinity percent, the attacker/defender status-weaken bits, and
//      (summon only) the caster's per-character magic-power byte
//      ([`apply_element_affinity`], [`apply_status_weaken`], [`apply_magic_power`]).
//      A conditional re-roll bonus then fires back in `FUN_801dd0ac`
//      ([`summon_bonus_roll`]).
//   3. **Finish** (`FUN_801ddb30`): elemental-resistance bits, the crit term,
//      the 9999 cap, the spirit-gauge fill, the damage-popup accumulator, MP
//      drain, and per-element stat-debuff application. That stage reads ~20
//      battle globals + both actors' full records and mutates live battle state,
//      so it is the deeply-coupled tail of the live battle context, **not** a
//      pure kernel - it is intentionally not reproduced here. The pieces below
//      are the bounded, state-free arithmetic the roll and scale stages are made
//      of; the engine supplies the live stats / RNG / affinity / power-byte.
//
// Returned damage is `attacker_roll - defender_roll` after all three stages.
// [`summon_predamage`] composes stages 1 + 2 (everything that is closed-form
// given its inputs); the finisher is applied by the caller's battle context.

/// One battle actor's stats as read by the summon-damage roll. Field offsets
/// cite the [battle-actor record](../subsystems/battle.md): `hp` = `+0x14c`,
/// `agl` = `+0x168`, `stat_a`/`stat_b` = `+0x15c`/`+0x160` (defender-only
/// defense terms), `status` = the `+0x16e` status bitfield, `guard` = the
/// `+0x1de` guard byte.
#[derive(Debug, Clone, Copy, Default)]
pub struct SummonRollActor {
    /// Current HP (`+0x14c`).
    pub hp: u16,
    /// Agility (`+0x168`).
    pub agl: u16,
    /// Defense term A (`+0x15c`); only the defender's is read.
    pub stat_a: u16,
    /// Defense term B (`+0x160`); only the defender's is read.
    pub stat_b: u16,
    /// Status bitfield (`+0x16e`): bit `0x1` weakens the roll to 9/10, bit
    /// `0x2` to 7/10 (applied in that order).
    pub status: u16,
    /// Guard byte (`+0x1de`); `== 4` doubles the defender roll.
    pub guard: u8,
}

/// Attacker (summon-body) roll, `FUN_801dd0ac` summon branch (`attacker_slot ==
/// 7`): `rand % (summon_agl + 1) + summon_hp + caster_agl * 2`.
///
/// `rand` is one `rand()` draw (`0..=0x7FFF`); the summon body's AGL/HP are the
/// slot-7 actor's, `caster_agl` is the *party caster's* AGL (`DAT_801C9370[ctx
/// + 0x13]`), which contributes doubled.
pub fn summon_attacker_roll(summon_hp: u16, summon_agl: u16, caster_agl: u16, rand: u16) -> u32 {
    let modulus = summon_agl as u32 + 1; // u16 + 1 is never zero
    (rand as u32) % modulus + summon_hp as u32 + caster_agl as u32 * 2
}

/// Defender roll, `FUN_801dd0ac` (always computed): `rand % ((agl >> 1) + 1) +
/// (hp >> 8) + (stat_a >> 4) + (stat_b >> 4) + agl * 2`.
pub fn summon_defender_roll(defender: &SummonRollActor, rand: u16) -> u32 {
    let modulus = (defender.agl as u32 >> 1) + 1; // never zero
    (rand as u32) % modulus
        + (defender.hp as u32 >> 8)
        + (defender.stat_a as u32 >> 4)
        + (defender.stat_b as u32 >> 4)
        + defender.agl as u32 * 2
}

/// Element-affinity scale, `FUN_801dd864`: `roll * affinity_pct / 100`. The
/// percent is one byte from the 8x8 element-affinity matrix at `0x801F53E8`,
/// indexed `matrix[attacker_element][defender_element]` (the disasm computes
/// `def_elem + atk_elem*8`, so the **row is the attacker**, the column the
/// defender). The matrix is parsed off the disc by
/// [`legaia_asset::element_affinity`]; retail values are a small nudge -
/// 100 = neutral, 96 = same-element self-resist, 104 = opposite-element bonus.
pub fn apply_element_affinity(roll: u32, affinity_pct: u8) -> u32 {
    roll.saturating_mul(affinity_pct as u32) / 100
}

/// Status-weaken bits, `FUN_801dd864`: bit `0x1` of the status field scales the
/// roll to `9/10`, then bit `0x2` scales (the result) to `7/10`. Both can apply
/// (bit `0x1` first). Used for both the attacker and defender rolls.
pub fn apply_status_weaken(roll: u32, status: u16) -> u32 {
    let mut r = roll;
    if status & 0x1 != 0 {
        r = r.saturating_mul(9) / 10;
    }
    if status & 0x2 != 0 {
        r = r.saturating_mul(7) / 10;
    }
    r
}

/// Per-character magic-power scale, `FUN_801dd864` summon arm: `roll + roll *
/// (power_byte - 1) >> 3` (i.e. `roll * (7 + power_byte) / 8`). `power_byte` is
/// the caster's recovery/magic-power stat from the SC-block table at
/// `0x80084140 + 0x729`, matched against the cast spell-id at `+0x705`. A
/// `power_byte` of 0 or 1 leaves the roll unchanged.
pub fn apply_magic_power(roll: u32, power_byte: u8) -> u32 {
    let extra = roll.saturating_mul(power_byte.saturating_sub(1) as u32) >> 3;
    roll + extra
}

/// Conditional re-roll bonus, `FUN_801dd0ac` summon branch second arm. After
/// the scale stage, when `defender_roll + summon_hp > attacker_roll` (the
/// attacker has not already overwhelmed the defender), the attacker roll is
/// rebuilt as `defender_roll + rand % ((summon_agl >> 1) + 1) + summon_hp`.
pub fn summon_bonus_roll(defender_roll: u32, summon_hp: u16, summon_agl: u16, rand: u16) -> u32 {
    let modulus = (summon_agl as u32 >> 1) + 1; // never zero
    defender_roll + (rand as u32) % modulus + summon_hp as u32
}

/// All inputs to [`summon_predamage`] (stages 1 + 2 of the summon-damage roll).
#[derive(Debug, Clone, Copy)]
pub struct SummonPredamage {
    /// Summon-body (attacker slot 7) stats.
    pub summon: SummonRollActor,
    /// The party caster's AGL (`DAT_801C9370[ctx + 0x13]`), doubled into the roll.
    pub caster_agl: u16,
    /// The target (defender) stats.
    pub target: SummonRollActor,
    /// Element-affinity percent (`0x801F53E8[atk_elem][def_elem]`).
    pub element_affinity_pct: u8,
    /// Caster magic-power byte (`SC + 0x729`).
    pub magic_power_byte: u8,
    /// Three `rand()` draws, in call order: attacker roll, defender roll, bonus.
    pub rng: [u16; 3],
}

/// Compose the closed-form stages of the summon-damage roll: the attacker +
/// defender rolls ([`summon_attacker_roll`] / [`summon_defender_roll`]), the
/// `FUN_801dd864` scale stage (affinity → status → magic-power on the attacker,
/// guard-double → status on the defender), and the conditional bonus re-roll.
///
/// Returns `(attacker_roll, defender_roll)` *before* the `FUN_801ddb30`
/// finisher. The pre-finisher damage is `attacker_roll.saturating_sub(
/// defender_roll)`; the engine's live battle context then applies the finisher
/// (resistance bits / crit / 9999 cap / spirit-gauge / popup / MP drain).
pub fn summon_predamage(i: &SummonPredamage) -> (u32, u32) {
    let bonus = i.rng[2];
    summon_predamage_lazy(
        &i.summon,
        i.caster_agl,
        &i.target,
        i.element_affinity_pct,
        i.magic_power_byte,
        [i.rng[0], i.rng[1]],
        move || bonus,
    )
}

/// As [`summon_predamage`], but the bonus-arm draw is produced lazily by
/// `bonus_rng`, which is invoked **only when the conditional bonus re-roll
/// actually fires**.
///
/// Retail (`FUN_801dd0ac` summon branch) draws that `rand()` *inside* the bonus
/// arm - `func_0x80056798(7)` after the `local_24 + summon_hp <= local_28`
/// check fails - so a caller pulling from a shared RNG cursor (e.g.
/// `World::player_summon_predamage`) advances the cursor by exactly **two**
/// draws on the no-bonus path and **three** on the bonus path, matching the
/// retail call order. The eager [`summon_predamage`] wrapper passes a closure
/// that just returns its pre-drawn value, so its observable behaviour is
/// unchanged.
pub fn summon_predamage_lazy(
    summon: &SummonRollActor,
    caster_agl: u16,
    target: &SummonRollActor,
    element_affinity_pct: u8,
    magic_power_byte: u8,
    rng2: [u16; 2],
    bonus_rng: impl FnOnce() -> u16,
) -> (u32, u32) {
    // Stage 1: rolls.
    let mut attacker = summon_attacker_roll(summon.hp, summon.agl, caster_agl, rng2[0]);
    let mut defender = summon_defender_roll(target, rng2[1]);

    // Stage 2a: FUN_801dd864 scales the attacker roll.
    attacker = apply_element_affinity(attacker, element_affinity_pct);
    attacker = apply_status_weaken(attacker, summon.status);
    attacker = apply_magic_power(attacker, magic_power_byte);

    // Stage 2b: FUN_801dd864 scales the defender roll (guard-double, then status).
    if target.guard == 4 {
        defender = defender.saturating_mul(2);
    }
    defender = apply_status_weaken(defender, target.status);

    // Stage 2c: conditional bonus re-roll (FUN_801dd0ac second arm). The draw
    // is pulled here, lazily, exactly as retail does.
    if defender + summon.hp as u32 > attacker {
        attacker = summon_bonus_roll(defender, summon.hp, summon.agl, bonus_rng());
    }

    (attacker, defender)
}

/// Recovery-summon healing amount, applied inline by the heal stagers (PROT
/// 0903 / 0905 / 0910 / 0911 / 0913): `(power_byte << 5) + 0xE0` = `power_byte *
/// 32 + 224`, clamped by the caller to `maxHP - curHP`. `power_byte` is the
/// caster's magic-power stat (`SC + 0x729`, the same byte [`apply_magic_power`]
/// reads). There is no roll and no RNG on the heal path.
pub fn heal_summon_amount(power_byte: u8) -> u16 {
    ((power_byte as u32) << 5) as u16 + 0xE0
}

// ---------------------------------------------------------------------------
// FUN_801dd0ac - arts / physical branch (attacker_slot != 7)
// ---------------------------------------------------------------------------
//
// The same shared kernel `FUN_801dd0ac` also resolves every melee / Tactical-Art
// / enemy-special-attack hit (the `attacker_slot != 7` branch), and it is the
// twin of the summon branch above with two differences:
//
//   * the attacker roll is seeded by a **static per-move power scalar** from the
//     26-byte-stride move-power table at `0x801F4F5C` (now parsed off the disc as
//     [`legaia_asset::move_power`], PROT 0898 file `0x26744`) - `move_type * 0x1a
//     + 0x801F4F5C`, the i16 at `+0`. This is the one true per-move power scalar
//     in the battle system; summons have no such scalar (see the summon block
//     above + `docs/formats/move-power.md`).
//   * it draws **two** `rand()`s for the attacker roll (vs the summon branch's
//     one) and **two** for the bonus re-roll, so the arts path consumes up to
//     five draws total (attacker ×2, defender ×1, bonus ×2) against the summon
//     path's three.
//
// The `FUN_801dd864` scale stage and the `FUN_801ddb30` finisher are shared with
// the summon branch; the scale's per-character magic-power arm is summon-only
// (`param_1 == 7` in `FUN_801dd864`), so arts hits scale by element affinity +
// status weaken only - exactly [`apply_element_affinity`] + [`apply_status_weaken`].
//
// `power` is the **sign-extended i16** read of the move-power record's `+0`, so
// the `power >> 1/2/3` folds are arithmetic shifts. Real records carry
// non-negative powers; the kernels mirror the signed shifts the retail code uses.

/// Attacker roll, `FUN_801dd0ac` arts/physical branch (`attacker_slot != 7`):
/// `rand0 % ((power >> 2) + 1) + rand1 % ((agl >> 1) + 1) + (hp >> 8) + power +
/// agl * 2`.
///
/// `power` is the move-power record's `+0` i16 (sign-extended); `hp`/`agl` are the
/// attacking actor's `+0x14c`/`+0x168`. Two `rand()` draws in call order.
pub fn arts_attacker_roll(power: i32, attacker_hp: u16, attacker_agl: u16, rng: [u16; 2]) -> u32 {
    let modulus_power = (power >> 2) + 1; // >= 1 for power >= 0
    let modulus_agl = (attacker_agl as i32 >> 1) + 1; // never zero
    let roll = rng[0] as i32 % modulus_power
        + rng[1] as i32 % modulus_agl
        + (attacker_hp as i32 >> 8)
        + power
        + attacker_agl as i32 * 2;
    roll as u32
}

/// Conditional bonus re-roll, `FUN_801dd0ac` arts/physical second arm. After the
/// scale stage, when `attacker_roll < defender_roll + (power >> 1) + (agl >> 1)`
/// (the attacker has not overwhelmed the defender's mitigation), the attacker
/// roll is rebuilt as `defender_roll + (power >> 1) + rand0 % ((power >> 3) + 1) +
/// (agl >> 1) + rand1 % ((agl >> 3) + 1)`. Two `rand()` draws in call order.
pub fn arts_bonus_roll(defender_roll: u32, power: i32, attacker_agl: u16, rng: [u16; 2]) -> u32 {
    let modulus_power = (power >> 3) + 1; // >= 1 for power >= 0
    let modulus_agl = (attacker_agl as i32 >> 3) + 1; // never zero
    let bonus = defender_roll as i32
        + (power >> 1)
        + rng[0] as i32 % modulus_power
        + (attacker_agl as i32 >> 1)
        + rng[1] as i32 % modulus_agl;
    bonus as u32
}

/// All inputs to [`arts_physical_predamage`] (stages 1 + 2 of the arts/physical
/// damage roll).
#[derive(Debug, Clone, Copy)]
pub struct ArtsPredamage {
    /// The move-power record's `+0` power scalar (sign-extended i16), from
    /// [`legaia_asset::move_power`] indexed via the `0x801F4E63` id→index map.
    pub power: i32,
    /// Attacking actor stats (slot != 7).
    pub attacker: SummonRollActor,
    /// Target (defender) stats.
    pub target: SummonRollActor,
    /// Element-affinity percent (`0x801F53E8[atk_elem][def_elem]`).
    pub element_affinity_pct: u8,
    /// Five `rand()` draws, in call order: attacker ×2, defender ×1, bonus ×2.
    /// The bonus pair is consumed only when the bonus arm fires; a caller that
    /// must match retail's RNG-cursor advance should use
    /// [`arts_physical_predamage_lazy`] (which draws the bonus pair lazily)
    /// rather than pre-drawing all five.
    pub rng: [u16; 5],
}

/// Compose the closed-form stages of the arts/physical damage roll: the attacker
/// roll ([`arts_attacker_roll`], two draws) + defender roll
/// ([`summon_defender_roll`], shared with the summon branch), the
/// `FUN_801dd864` scale stage (affinity → status on the attacker, guard-double →
/// status on the defender; **no** magic-power arm on this branch), and the
/// conditional bonus re-roll ([`arts_bonus_roll`], two draws).
///
/// Returns `(attacker_roll, defender_roll)` *before* the `FUN_801ddb30`
/// finisher, exactly as [`summon_predamage`] does for the summon branch. The
/// pre-finisher damage is `attacker_roll.saturating_sub(defender_roll)`.
pub fn arts_physical_predamage(i: &ArtsPredamage) -> (u32, u32) {
    let bonus = [i.rng[3], i.rng[4]];
    arts_physical_predamage_lazy(
        i.power,
        &i.attacker,
        &i.target,
        i.element_affinity_pct,
        [i.rng[0], i.rng[1], i.rng[2]],
        move || bonus,
    )
}

/// As [`arts_physical_predamage`], but the bonus-arm draws are produced lazily by
/// `bonus_rng`, which is invoked **only when the conditional bonus re-roll
/// actually fires**.
///
/// Retail (`FUN_801dd0ac`) draws those two `rand()` values *inside* the bonus
/// arm, so a caller that pulls from a shared RNG cursor (e.g.
/// `World::enemy_move_predamage`) advances the cursor by exactly **three** draws
/// on the no-bonus path and **five** on the bonus path - matching the retail call
/// order. The eager [`arts_physical_predamage`] wrapper passes a closure that
/// just returns its pre-drawn pair, so its observable draw count is unchanged.
pub fn arts_physical_predamage_lazy(
    power: i32,
    attacker: &SummonRollActor,
    target: &SummonRollActor,
    element_affinity_pct: u8,
    rng3: [u16; 3],
    bonus_rng: impl FnOnce() -> [u16; 2],
) -> (u32, u32) {
    // Stage 1: rolls. Attacker uses rng3[0..2]; defender uses rng3[2].
    let mut attacker_roll =
        arts_attacker_roll(power, attacker.hp, attacker.agl, [rng3[0], rng3[1]]);
    let mut defender = summon_defender_roll(target, rng3[2]);

    // Stage 2a: FUN_801dd864 scales the attacker roll (affinity, status; the
    // magic-power arm is summon-only and does not apply here).
    attacker_roll = apply_element_affinity(attacker_roll, element_affinity_pct);
    attacker_roll = apply_status_weaken(attacker_roll, attacker.status);

    // Stage 2b: FUN_801dd864 scales the defender roll (guard-double, then status).
    if target.guard == 4 {
        defender = defender.saturating_mul(2);
    }
    defender = apply_status_weaken(defender, target.status);

    // Stage 2c: conditional bonus re-roll (FUN_801dd0ac second arm). Threshold =
    // defender + (power >> 1) + (attacker_agl >> 1); fires when attacker < it. The
    // two bonus draws are pulled here, lazily, exactly as retail does.
    let threshold = defender as i32 + (power >> 1) + (attacker.agl as i32 >> 1);
    if (attacker_roll as i32) < threshold {
        attacker_roll = arts_bonus_roll(defender, power, attacker.agl, bonus_rng());
    }

    (attacker_roll, defender)
}

// ---------------------------------------------------------------------------
// FUN_801ddb30 - damage finisher (post-roll finalisation)
// ---------------------------------------------------------------------------
//
// The shared finisher `FUN_801ddb30` (`overlay_battle_action_801ddb30.txt`) takes
// the pre-finisher damage produced by the roll + scale stages above and turns it
// into the final HP loss + the defender's spirit-gauge fill. It works on
// `over = *param_3 - *param_4` (the damage *above* the base `*param_4`); every
// stage rewrites `over` in place. The closed-form arithmetic splits cleanly into
// two pure kernels:
//
//   * [`damage_finish`] - equipment elemental-resistance halving (one element
//     bit per attacker element; the absorb-bit `0x10` gate routes to a 3/4 scale
//     instead), the defender-guard halve (`actor+0x1de == 4`), the no-damage
//     `rand()%9 + 8` floor, the summon power-percent scale (`attacker_slot == 7`),
//     and the `9999` cap. Returns the final `over` (HP loss).
//   * [`spirit_gauge_fill`] - the defender's spirit-gauge accrual from the same
//     `over`, plus the two "spirit gain up" equipment bits, clamped to 100.
//
// The finisher's remaining tail is genuinely coupled to live battle state and is
// **not** reproduced here: the damage-popup accumulator (`_DAT_8007bd14`), the
// `DAT_801f6980` AI revenge / counter-aggro table, the MP-drain + the
// per-element stat-debuff `switch` keyed on the attacker's element
// (`DAT_801c9358+0x1d`), and the `+0x16e` "nullify" status that zeroes the hit
// after the spirit accrual. The action SM applies those; see the REF in the
// module header.

/// The defender's equipment-derived elemental-resistance + spirit flags, read by
/// [`damage_finish`] / [`spirit_gauge_fill`] from the live character record's two
/// words at `+0xF4` (`lo`) and `+0xF8` (`hi`) (runtime `0x800847FC`/`0x80084800`
/// for member 0, `0x414` stride). Only a party defender (slot `< 3`) carries
/// these; enemy defenders pass [`Default`] (no resistance).
///
/// The two words are the first half of the accessory-passive **ability
/// bitfield** (record `+0xF4..+0x103`, aggregator `FUN_80042558`), and every
/// flag here is simply passive index `0x1D + element` read through the word
/// boundary: the elemental-guard passives are contiguous at `0x1D..=0x23`
/// (Earth, Water, Fire, Wind, Thunder, Light, Dark - the element-id order),
/// All Guard is `0x24`, and the "spirit gain up" pair is AP Boost 1/2 at
/// `0x28`/`0x29`. So the bit layout (mirroring the disassembly's per-element
/// `if` ladder in `FUN_801ddb30`):
///
/// | element | guard passive | bit | word |
/// |---|---|---|---|
/// | 0 Earth | `0x1D` | `0x20000000` | `lo` (`+0xF4`) |
/// | 1 Water | `0x1E` | `0x40000000` | `lo` |
/// | 2 Fire | `0x1F` | `0x80000000` | `lo` |
/// | 3 Wind | `0x20` | `0x1` | `hi` (`+0xF8`) |
/// | 4 Thunder | `0x21` | `0x2` | `hi` |
/// | 5 Light | `0x22` | `0x4` | `hi` |
/// | 6 Dark | `0x23` | `0x8` | `hi` |
///
/// `hi & 0x10` is the All-Guard gate (passive `0x24`, Rainbow Jewel);
/// `hi & 0x100` / `hi & 0x200` are AP Boost 1/2 (`0x28`/`0x29`), the two
/// spirit-gain-up flags (see [`spirit_gauge_fill`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefenderResist {
    /// Record word `+0xF4` (ability-bitfield word 0, passive indices
    /// `0x00..=0x1F`): elements 0..=2 in the top three bits.
    pub lo: u32,
    /// Record word `+0xF8` (ability-bitfield word 1, passive indices
    /// `0x20..=0x3F`): elements 3..=6 in the low nibble, the All-Guard gate
    /// (`0x10`) and the spirit-gain-up bits (`0x100`/`0x200`).
    pub hi: u32,
}

impl DefenderResist {
    /// Build from the first two words of a character's accessory-passive
    /// ability bitfield (record `+0xF4` / `+0xF8`), the exact words retail's
    /// finisher indexes.
    pub fn from_ability_words(word0: u32, word1: u32) -> Self {
        Self {
            lo: word0,
            hi: word1,
        }
    }

    /// `true` if the defender resists `element` (0..=6) - the per-element bit set.
    fn resists(&self, element: u8) -> bool {
        match element {
            0 => self.lo & 0x2000_0000 != 0,
            1 => self.lo & 0x4000_0000 != 0,
            2 => self.lo & 0x8000_0000 != 0,
            3 => self.hi & 0x1 != 0,
            4 => self.hi & 0x2 != 0,
            5 => self.hi & 0x4 != 0,
            6 => self.hi & 0x8 != 0,
            _ => false,
        }
    }
}

/// All inputs to [`damage_finish`] (the closed-form stages of `FUN_801ddb30`).
#[derive(Debug, Clone, Copy)]
pub struct DamageFinish {
    /// Pre-finisher damage above base (`attacker_roll - defender_roll`, the
    /// `over` the roll/scale stages produce - already saturated to `>= 0`).
    pub predamage: u32,
    /// Attacker actor slot (`param_1`); `7` is the summon body, `>= 3` an enemy.
    pub attacker_slot: u8,
    /// Defender actor slot (`param_2`); `< 3` a party member.
    pub defender_slot: u8,
    /// Attacker element (0..=6); `7` = non-elemental, which bypasses the absorb
    /// gate so the per-element halve ladder still runs.
    pub attacker_element: u8,
    /// The party defender's equipment resistance flags. Ignored for an enemy
    /// defender (`defender_slot >= 3`).
    pub defender_resist: DefenderResist,
    /// Defender is in the guard/defend state (`actor+0x1de == 4`) - halves `over`.
    pub defender_guarding: bool,
    /// The `_DAT_8007bd84` global, consulted only for an enemy defender
    /// (`defender_slot >= 3`): when set, the enemy takes half damage.
    pub enemy_defender_halve: bool,
    /// The `param_5` flag: when non-zero the party-defender resistance block is
    /// skipped entirely (the retail caller passes it for certain fixed hits).
    pub bypass_party_resist: bool,
    /// Summon power-percent (`attacker_slot == 7` only): `over` is scaled
    /// `over * pct / 100`. From the per-caster element table at `0x801F5468`.
    pub summon_power_pct: u8,
    /// One `rand()` draw (`0..=0x7FFF`), consumed **only** when `over` has been
    /// reduced to `0` by mitigation (the `rand()%9 + 8` floor). Pass any value
    /// when the caller knows mitigation can't zero the hit.
    pub floor_rand: u16,
}

/// Apply the closed-form finalisation stages of `FUN_801ddb30` to the
/// pre-finisher damage and return the final HP loss (`over`).
///
/// Order mirrors the disassembly exactly:
///
/// 1. **Party-defender elemental resistance** (`defender_slot < 3`, attacker is
///    an enemy `>= 3`, and `!bypass_party_resist`): if the defender's absorb bit
///    (`hi & 0x10`) is clear *or* the attacker is non-elemental (element 7), the
///    per-element halve ladder runs - `over >>= 1` when the defender resists the
///    attacker's element. Otherwise `over = over * 3 >> 2` (3/4).
/// 2. **Enemy-defender halve** (`defender_slot >= 3`): `over >>= 1` when
///    `enemy_defender_halve`.
/// 3. **Guard halve**: `over >>= 1` when the defender is guarding.
/// 4. **No-damage floor**: when `over == 0`, `over = rand()%9 + 8`.
/// 5. **Summon power scale** (`attacker_slot == 7`): `over = over * pct / 100`.
/// 6. **9999 cap**.
///
/// The multi-hit pointer bump (`if *param_3 == *param_4 param_3++`) and the
/// `+0x16e` nullify status are not part of this value - they are caller concerns
/// (see the module section comment).
pub fn damage_finish(i: &DamageFinish) -> u32 {
    damage_finish_lazy(i, || i.floor_rand)
}

/// As [`damage_finish`], but the stage-4 floor `rand()` is produced lazily by
/// `floor_rand`, invoked **only when mitigation has reduced `over` to zero** -
/// the single point retail's `FUN_801ddb30` draws RNG. A caller pulling from a
/// shared RNG cursor advances it by zero or one draw, exactly as retail does;
/// [`DamageFinish::floor_rand`] is ignored on this path.
pub fn damage_finish_lazy(i: &DamageFinish, floor_rand: impl FnOnce() -> u16) -> u32 {
    let mut over = i.predamage;

    // Stage 1: party-defender elemental resistance.
    if (i.defender_slot as u32) < 3 {
        if i.attacker_slot >= 3 && !i.bypass_party_resist {
            let absorb_gate = i.defender_resist.hi & 0x10 != 0;
            if !absorb_gate || i.attacker_element == 7 {
                if i.attacker_element <= 6 && i.defender_resist.resists(i.attacker_element) {
                    over >>= 1;
                }
            } else {
                // Absorb bit set + elemental attacker: 3/4 scale.
                over = (over * 3) >> 2;
            }
        }
    } else if i.enemy_defender_halve {
        // Stage 2: enemy-defender global halve.
        over >>= 1;
    }

    // Stage 3: defender guard halve.
    if i.defender_guarding {
        over >>= 1;
    }

    // Stage 4: no-damage floor (the only RNG draw the finisher consumes).
    if over == 0 {
        over = (floor_rand() as u32) % 9 + 8;
    }

    // Stage 5: summon power-percent scale.
    if i.attacker_slot == 7 {
        over = over.saturating_mul(i.summon_power_pct as u32) / 100;
    }

    // Stage 6: 9999 cap.
    if over > 9999 {
        over = 9999;
    }
    over
}

/// The defender's spirit-gauge fill from a finished hit, `FUN_801ddb30`'s spirit
/// stage. Mirrors the disassembly:
///
/// ```text
/// pct = max(1, over * 100 / defender_maxhp)
/// if defender_is_party:
///     if (resist.hi & 0x200): spirit += pct >> 2     // "spirit gain up" ×1
///     if (resist.hi & 0x100): spirit += pct / 10     // "spirit gain up" ×2
/// spirit = min(100, spirit + pct)
/// ```
///
/// `over` is the **pre-nullify** damage (spirit still accrues when a `+0x16e`
/// nullify status later zeroes the HP loss). `defender_maxhp` is `actor+0x14e`;
/// retail `trap`s on a zero max-HP - the kernel instead returns the gauge
/// unchanged (the caller guarantees a living defender). Returns the new gauge
/// value (already clamped to `100`).
///
/// The live battle loop drives this on the defender of every damaging hit
/// (physical and magic) into [`BattleActor::spirit_gauge`] (`actor+0x170`); see
/// `World::accrue_spirit_gauge`. The engine passes [`DefenderResist::default`]
/// (the per-character resist/spirit-gain-up words aren't modelled yet), so only
/// the unconditional base `pct` term contributes today.
pub fn spirit_gauge_fill(
    over: u32,
    defender_maxhp: u16,
    current_spirit: u16,
    resist: DefenderResist,
    defender_is_party: bool,
) -> u16 {
    if defender_maxhp == 0 {
        return current_spirit.min(100);
    }
    let pct = (over * 100) / defender_maxhp as u32;
    let pct = if pct == 0 { 1 } else { pct };
    let mut spirit = current_spirit as u32;
    if defender_is_party {
        if resist.hi & 0x200 != 0 {
            spirit += pct >> 2;
        }
        if resist.hi & 0x100 != 0 {
            spirit += pct / 10;
        }
    }
    spirit += pct;
    spirit.min(100) as u16
}

// ---------------------------------------------------------------------------
// FUN_8004E568 - victory spoils (gold + EXP reward arithmetic)
// ---------------------------------------------------------------------------
//
// The post-battle reward resolver `FUN_8004E568`
// (`ghidra/scripts/funcs/8004e568.txt`) builds the gold and EXP awards from the
// dead enemies' record fields (`+0x44` gold, `+0x46` EXP). Both are scaled - the
// engine must not credit the raw record sums. Pinned arithmetic (decompiled
// block at `8004e568.txt:411..461`):
//
//   gold: acc = Σ (enemy_gold >> 1) over dead enemies;
//         if a living party member carries ability bit 0x10000: acc += acc >> 2;  // +25%
//         credited = acc - (acc >> 1);                                            // halve
//   exp:  per_member = ceil((Σ enemy_exp - (Σ enemy_exp >> 2)) / alive_count);    // ×3/4
//
// The gold path is runtime-confirmed: the lone-enemy Gimard fight (record gold
// 60) credited exactly +15 (`60>>1 = 30`, `30 - (30>>1) = 15`) via a
// write-watchpoint on the party purse `0x8008459C`.

/// One dead enemy's contribution to the victory gold accumulator
/// (`FUN_8004E568`, `8004e568.txt:413`): `enemy_gold >> 1` (record `+0x44`).
/// Sum this over every dead enemy, then pass the total to
/// [`victory_gold_finalize`].
pub fn victory_gold_per_monster(enemy_gold: u16) -> u32 {
    (enemy_gold >> 1) as u32
}

/// Finalize the accumulated victory gold (`FUN_8004E568`, `8004e568.txt:435/440`):
/// apply the optional +25% "extra gold" bonus when a living party member carries
/// ability bit `0x10000` (`acc += acc >> 2`), then halve the total (`acc - (acc
/// >> 1)`). With `more_gold == false` and a lone enemy this is the
/// runtime-confirmed Gimard chain `60 -> 30 -> 15` (`floor((gold >> 1) / 2)`).
/// The party-purse cap (`99,999,999`) is applied by the caller, not here.
pub fn victory_gold_finalize(accumulated: u32, more_gold: bool) -> u32 {
    let acc = if more_gold {
        accumulated.saturating_add(accumulated >> 2)
    } else {
        accumulated
    };
    acc - (acc >> 1)
}

/// Per-member EXP from a won battle (`FUN_8004E568`, `8004e568.txt:461`): the
/// summed enemy EXP (record `+0x46`) is scaled by 3/4 (`v - (v >> 2)`) then
/// **ceiling**-divided among the `alive` living, EXP-eligible party members
/// (`(scaled + alive - 1) / alive`). Returns 0 when `alive == 0`.
pub fn victory_exp_per_member(exp_sum: u32, alive: u32) -> u32 {
    if alive == 0 {
        return 0;
    }
    let scaled = exp_sum - (exp_sum >> 2);
    scaled.div_ceil(alive)
}

// ---------------------------------------------------------------------------
// Summon-magic spell XP + level-up (FUN_801DDB30 tail / FUN_801E70BC)
// ---------------------------------------------------------------------------
//
// Casting Seru magic trains the spell itself. Two coupled retail pieces:
//
// - The damage finisher `FUN_801ddb30` ends with a spell-XP accrual tail that
//   only runs for the summon attacker (`param_1 == 7`): it finds the cast
//   spell id (`caster_actor + 0x1DF`) in the caster's character-record
//   spell-id list (record `+0x13D`, search bound `0x20`) and adds a
//   damage-proportional gain into the parallel per-spell u32 XP array at
//   record `+0x8` (`overlay_battle_action_801ddb30.txt:1037..1084`).
// - After the summon returns (state `0x36` of `FUN_801E295C`), `FUN_801e70bc`
//   re-finds the slot, reads the spell-level byte (`+0x161` array) and the
//   accrued XP, and levels the spell up when the XP clears a threshold from
//   the static SCUS table at `0x8007656C`
//   (`overlay_battle_action_801e70bc.txt`).
//
// The leveled byte is the **magic-power** stage input of the next cast
// (`FUN_801dd864` reads the same `+0x161` byte - see [`apply_magic_power`]),
// so the loop is: cast → XP → level → stronger cast.
//
// Unmodelled retail gates (documented, intentionally not reproduced): the
// per-battle no-reward flag `_DAT_8007BAC0` (scripted fights skip the accrual,
// same flag battle-formulas.md notes as the unmodelled gold gate) and the
// unidentified accrual skip `_DAT_8007BDB8`.

/// One target's spell-XP gain from a summon hit - PORT: FUN_801ddb30
/// (spell-XP accrual tail, `attacker_slot == 7` only; decompiled block
/// `overlay_battle_action_801ddb30.txt:1049..1084`).
///
/// `damage` is the finisher's final damage delta (`*param_3 - *param_4`),
/// `target_hp`/`target_max_hp` are the defender's live and max HP (actor
/// `+0x14C`/`+0x14E`), `group_target` mirrors the summon actor's target byte
/// (`+0x1DD`): `false` for a single-target cast (`< 8`), `true` for a
/// group-target cast (`8`/`9`).
///
/// Retail arithmetic, exactly:
///
/// - a target with fewer than 2 HP grants nothing (both branches gate on
///   `target_hp >= 2`);
/// - non-killing hit (`damage < target_hp`): gain = `damage * 12 /
///   target_max_hp` single-target, `damage * 4 / target_max_hp` group;
/// - killing hit (`damage >= target_hp`): flat `12` single-target, `4` group.
///
/// A zero `target_max_hp` divides-by-zero in retail (`trap(0x1c00)`); the
/// engine returns 0 instead.
pub fn summon_spell_xp_gain(
    damage: u32,
    target_hp: u16,
    target_max_hp: u16,
    group_target: bool,
) -> u32 {
    if target_hp < 2 {
        return 0;
    }
    let unit: u32 = if group_target { 4 } else { 12 };
    if damage < target_hp as u32 {
        if target_max_hp == 0 {
            return 0;
        }
        (damage * unit) / target_max_hp as u32
    } else {
        unit
    }
}

/// The six spell ids whose level-up threshold is scaled ×1.5 - the explicit
/// `switch` cases of `FUN_801e70bc` (`iVar1 = 3` instead of `2`, halved into
/// `(threshold * mult) >> 1`).
pub const SUMMON_XP_TRIPLE_THRESHOLD_IDS: [u8; 6] = [0x86, 0x88, 0x8D, 0x99, 0x9B, 0xA0];

/// The spell-XP total a spell at `level` must **exceed** to level up -
/// PORT: FUN_801e70bc (battle overlay 0898,
/// `overlay_battle_action_801e70bc.txt`).
///
/// `table` is the static SCUS u16 threshold table at `0x8007656C`, indexed
/// `[level - 1]` (8 ascending entries for levels 1..=8; level 9 is the cap).
/// The retail comparison is `((table[level-1] * mult) >> 1) < xp` with
/// `mult = 3` for the [`SUMMON_XP_TRIPLE_THRESHOLD_IDS`] and `2` otherwise
/// (so the default multiplier is the raw table value - the same compare the
/// heal-spell inline copy in `FUN_800402F4` case-0 tier-4 uses).
///
/// Returns `None` when no level-up is possible: level already at the cap
/// (`level >= 9`, the retail pre-increment guard), level `0` (retail would
/// read `table[-1]`; the engine guards), or `table` too short.
pub fn summon_magic_level_threshold(spell_id: u8, level: u8, table: &[u16]) -> Option<u32> {
    if level == 0 || level >= 9 {
        return None;
    }
    let base = *table.get((level - 1) as usize)? as u32;
    let mult: u32 = if SUMMON_XP_TRIPLE_THRESHOLD_IDS.contains(&spell_id) {
        3
    } else {
        2
    };
    Some((base * mult) >> 1)
}

/// `true` when a spell at `level` with accrued `xp` levels up - the
/// strict-greater compare of `FUN_801e70bc` (`threshold < xp`). The caller
/// applies the level increment (`level += 1`, cap 9) and the UI banner.
/// REF: FUN_801e70bc
pub fn summon_magic_levels_up(spell_id: u8, level: u8, xp: u32, table: &[u16]) -> bool {
    match summon_magic_level_threshold(spell_id, level, table) {
        Some(threshold) => threshold < xp,
        None => false,
    }
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

    #[test]
    fn summon_attacker_roll_matches_disasm() {
        // rand % (agl+1) + hp + caster_agl*2.
        // 5 % 21 = 5; + 200 + 16*2(=32) = 237.
        assert_eq!(summon_attacker_roll(200, 20, 16, 5), 237);
        // 25 % 21 = 4; + 200 + 32 = 236.
        assert_eq!(summon_attacker_roll(200, 20, 16, 25), 236);
        // agl = 0 -> modulus 1, rand contributes nothing (no div-by-zero).
        assert_eq!(summon_attacker_roll(50, 0, 0, 12345), 50);
    }

    #[test]
    fn summon_defender_roll_matches_disasm() {
        let d = SummonRollActor {
            hp: 1000,
            agl: 18,
            stat_a: 64,
            stat_b: 48,
            ..Default::default()
        };
        // modulus = (18>>1)+1 = 10; 7 % 10 = 7.
        // (1000>>8)=3 + (64>>4)=4 + (48>>4)=3 + 18*2=36 = 53.
        assert_eq!(summon_defender_roll(&d, 7), 53);
    }

    #[test]
    fn element_affinity_scales_by_percent() {
        assert_eq!(apply_element_affinity(100, 100), 100); // neutral
        assert_eq!(apply_element_affinity(100, 200), 200); // weakness (double)
        assert_eq!(apply_element_affinity(100, 50), 50); // resist
        assert_eq!(apply_element_affinity(100, 0), 0); // immune
    }

    #[test]
    fn status_weaken_applies_bits_in_order() {
        assert_eq!(apply_status_weaken(100, 0), 100); // no bits
        assert_eq!(apply_status_weaken(100, 0x1), 90); // 9/10
        assert_eq!(apply_status_weaken(100, 0x2), 70); // 7/10
        // Both: 9/10 first (90), then 7/10 (63).
        assert_eq!(apply_status_weaken(100, 0x3), 63);
    }

    #[test]
    fn magic_power_scales_roll() {
        assert_eq!(apply_magic_power(80, 1), 80); // power 1 = no change
        assert_eq!(apply_magic_power(80, 0), 80); // power 0 guarded to no change
        // power 9: 80 + (80*8 >> 3) = 80 + 80 = 160.
        assert_eq!(apply_magic_power(80, 9), 160);
    }

    #[test]
    fn heal_summon_amount_matches_disasm() {
        // (power<<5) + 0xE0.
        assert_eq!(heal_summon_amount(0), 0xE0); // 224 floor
        assert_eq!(heal_summon_amount(10), 544); // 320 + 224
        assert_eq!(heal_summon_amount(255), 8384); // 8160 + 224
    }

    #[test]
    fn summon_predamage_takes_bonus_when_attacker_is_weak() {
        let i = SummonPredamage {
            summon: SummonRollActor {
                hp: 200,
                agl: 20,
                ..Default::default()
            },
            caster_agl: 16,
            target: SummonRollActor {
                hp: 1000,
                agl: 18,
                stat_a: 64,
                stat_b: 48,
                ..Default::default()
            },
            element_affinity_pct: 100,
            magic_power_byte: 1,
            rng: [5, 7, 5],
        };
        // attacker initial 237, neutral affinity/status/power -> 237.
        // defender 53. 53 + 200 = 253 > 237 -> bonus re-roll:
        // 53 + (5 % 11 = 5) + 200 = 258.
        assert_eq!(summon_predamage(&i), (258, 53));
    }

    #[test]
    fn summon_predamage_skips_bonus_on_elemental_weakness() {
        let i = SummonPredamage {
            summon: SummonRollActor {
                hp: 200,
                agl: 20,
                ..Default::default()
            },
            caster_agl: 16,
            target: SummonRollActor {
                hp: 1000,
                agl: 18,
                stat_a: 64,
                stat_b: 48,
                ..Default::default()
            },
            element_affinity_pct: 200, // double damage -> attacker dominates
            magic_power_byte: 1,
            rng: [5, 7, 5],
        };
        // attacker 237 * 200/100 = 474. defender 53. 53 + 200 = 253 <= 474 -> no bonus.
        assert_eq!(summon_predamage(&i), (474, 53));
    }

    #[test]
    fn summon_predamage_doubles_defender_on_guard() {
        let i = SummonPredamage {
            summon: SummonRollActor {
                hp: 200,
                agl: 20,
                ..Default::default()
            },
            caster_agl: 16,
            target: SummonRollActor {
                hp: 1000,
                agl: 18,
                stat_a: 64,
                stat_b: 48,
                guard: 4, // doubles defender roll
                ..Default::default()
            },
            element_affinity_pct: 100,
            magic_power_byte: 1,
            rng: [5, 7, 5],
        };
        // defender 53 * 2 = 106. 106 + 200 = 306 > 237 -> bonus:
        // 106 + (5 % 11 = 5) + 200 = 311.
        assert_eq!(summon_predamage(&i), (311, 106));
    }

    #[test]
    fn summon_predamage_lazy_draws_bonus_only_when_arm_fires() {
        use std::cell::Cell;

        // (a) reproduce the eager result exactly and (b) invoke the bonus
        // closure exactly when the bonus arm fires - zero times on the
        // no-bonus path (shared RNG cursor advances by two, not three) and
        // once on the bonus path, mirroring FUN_801dd0ac's in-arm rand().
        let strong_summon = SummonRollActor {
            hp: 500,
            agl: 40,
            ..Default::default()
        };
        let weak_target = SummonRollActor {
            hp: 100,
            agl: 4,
            ..Default::default()
        };
        // attacker = 0%41 + 500 + 32 = 532; defender = 0%3 + 0 + 8 = 8.
        // 8 + 500 = 508 <= 532 -> no bonus.
        let calls = Cell::new(0u32);
        let out = summon_predamage_lazy(&strong_summon, 16, &weak_target, 100, 1, [0, 0], || {
            calls.set(calls.get() + 1);
            0
        });
        assert_eq!(calls.get(), 0, "no-bonus path must not draw");
        assert_eq!(
            out,
            summon_predamage(&SummonPredamage {
                summon: strong_summon,
                caster_agl: 16,
                target: weak_target,
                element_affinity_pct: 100,
                magic_power_byte: 1,
                rng: [0, 0, 0],
            }),
            "matches the eager result"
        );

        // Bonus path: the guard-doubled defender overwhelms the attacker
        // (the summon_predamage_doubles_defender_on_guard setup).
        let summon = SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        };
        let target = SummonRollActor {
            hp: 1000,
            agl: 18,
            stat_a: 64,
            stat_b: 48,
            guard: 4,
            ..Default::default()
        };
        let calls = Cell::new(0u32);
        let out = summon_predamage_lazy(&summon, 16, &target, 100, 1, [5, 7], || {
            calls.set(calls.get() + 1);
            5
        });
        assert_eq!(calls.get(), 1, "bonus path draws exactly once");
        assert_eq!(out, (311, 106), "matches the eager bonus result");
    }

    #[test]
    fn damage_finish_lazy_draws_floor_rand_only_when_zeroed() {
        use std::cell::Cell;

        // Non-zero damage: the floor closure must never run (retail draws no
        // RNG in FUN_801ddb30 unless mitigation zeroed the hit).
        let calls = Cell::new(0u32);
        let i = DamageFinish {
            predamage: 100,
            attacker_slot: 3,
            defender_slot: 4,
            attacker_element: 7,
            defender_resist: DefenderResist::default(),
            defender_guarding: false,
            enemy_defender_halve: false,
            bypass_party_resist: false,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let out = damage_finish_lazy(&i, || {
            calls.set(calls.get() + 1);
            4
        });
        assert_eq!(out, 100);
        assert_eq!(calls.get(), 0, "non-zero damage must not draw");

        // Zeroed damage: exactly one draw, rand%9 + 8.
        let calls = Cell::new(0u32);
        let z = DamageFinish { predamage: 0, ..i };
        let out = damage_finish_lazy(&z, || {
            calls.set(calls.get() + 1);
            13
        });
        assert_eq!(out, 13 % 9 + 8);
        assert_eq!(calls.get(), 1, "zeroed damage draws exactly once");
    }

    #[test]
    fn arts_attacker_roll_matches_disasm() {
        // power 20, attacker hp 200 / agl 20, rng [0, 0]:
        //   rand0 % ((20>>2)+1 = 6) = 0
        // + rand1 % ((20>>1)+1 = 11) = 0
        // + (200 >> 8 = 0) + power 20 + agl*2 (40) = 60.
        assert_eq!(arts_attacker_roll(20, 200, 20, [0, 0]), 60);
        // rng [5, 7]: 5%6 (5) + 7%11 (7) + 0 + 20 + 40 = 72.
        assert_eq!(arts_attacker_roll(20, 200, 20, [5, 7]), 72);
        // power 0 -> modulus_power (0>>2)+1 = 1, rand%1 = 0; hp 0xFFFF>>8 = 0xFF.
        assert_eq!(arts_attacker_roll(0, 0xFFFF, 0, [123, 0]), 0xFF);
    }

    #[test]
    fn arts_bonus_roll_matches_disasm() {
        // defender 30, power 10, agl 0, rng [0, 0]:
        //   30 + (10>>1 = 5) + 0%((10>>3)+1=2) + (0>>1 = 0) + 0%((0>>3)+1=1) = 35.
        assert_eq!(arts_bonus_roll(30, 10, 0, [0, 0]), 35);
        // rng [1, 0]: 30 + 5 + 1%2 (1) + 0 + 0 = 36.
        assert_eq!(arts_bonus_roll(30, 10, 0, [1, 0]), 36);
    }

    #[test]
    fn arts_physical_predamage_skips_bonus_when_attacker_dominates() {
        let i = ArtsPredamage {
            power: 20,
            attacker: SummonRollActor {
                hp: 200,
                agl: 20,
                ..Default::default()
            },
            target: SummonRollActor {
                hp: 100,
                agl: 16,
                stat_a: 32,
                stat_b: 48,
                ..Default::default()
            },
            element_affinity_pct: 100,
            rng: [0, 0, 0, 0, 0],
        };
        // attacker 60 (see arts_attacker_roll test), defender 0%9 + 0 + 2 + 3 + 32 = 37.
        // threshold = 37 + (20>>1=10) + (20>>1=10) = 57; attacker 60 >= 57 -> no bonus.
        assert_eq!(arts_physical_predamage(&i), (60, 37));
    }

    #[test]
    fn arts_physical_predamage_takes_bonus_when_attacker_is_weak() {
        let i = ArtsPredamage {
            power: 10,
            attacker: SummonRollActor {
                hp: 0,
                agl: 0,
                ..Default::default()
            },
            target: SummonRollActor {
                hp: 0,
                agl: 0,
                stat_a: 0xFF,
                stat_b: 0xFF,
                ..Default::default()
            },
            element_affinity_pct: 100,
            rng: [0, 0, 0, 0, 0],
        };
        // attacker = 0%3 + 0%1 + 0 + 10 + 0 = 10. defender = 0 + 0 + 15 + 15 + 0 = 30.
        // threshold = 30 + (10>>1=5) + (0>>1=0) = 35; attacker 10 < 35 -> bonus:
        //   30 + 5 + 0%2 + 0 + 0%1 = 35.
        assert_eq!(arts_physical_predamage(&i), (35, 30));
    }

    #[test]
    fn arts_physical_predamage_lazy_draws_bonus_only_when_arm_fires() {
        use std::cell::Cell;

        // Reuses the eager tests' two setups. The lazy variant must (a) reproduce
        // the eager result exactly and (b) invoke the bonus closure exactly when
        // the bonus arm fires - zero times on the no-bonus path (so a shared RNG
        // cursor advances by three, not five) and once on the bonus path.
        let dominant_attacker = SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        };
        let dominant_target = SummonRollActor {
            hp: 100,
            agl: 16,
            stat_a: 32,
            stat_b: 48,
            ..Default::default()
        };
        let weak_attacker = SummonRollActor::default();
        let weak_target = SummonRollActor {
            stat_a: 0xFF,
            stat_b: 0xFF,
            ..Default::default()
        };

        // No-bonus path: attacker dominates, closure never runs.
        let calls = Cell::new(0u32);
        let out = arts_physical_predamage_lazy(
            20,
            &dominant_attacker,
            &dominant_target,
            100,
            [0; 3],
            || {
                calls.set(calls.get() + 1);
                [0, 0]
            },
        );
        assert_eq!(out, (60, 37), "matches the eager no-bonus result");
        assert_eq!(calls.get(), 0, "bonus pair not drawn on the no-bonus path");

        // Bonus path: weak attacker, closure runs exactly once.
        let calls = Cell::new(0u32);
        let out =
            arts_physical_predamage_lazy(10, &weak_attacker, &weak_target, 100, [0; 3], || {
                calls.set(calls.get() + 1);
                [0, 0]
            });
        assert_eq!(out, (35, 30), "matches the eager bonus result");
        assert_eq!(
            calls.get(),
            1,
            "bonus pair drawn exactly once when the arm fires"
        );
    }

    #[test]
    fn arts_physical_predamage_scales_by_affinity() {
        let i = ArtsPredamage {
            power: 20,
            attacker: SummonRollActor {
                hp: 200,
                agl: 20,
                ..Default::default()
            },
            target: SummonRollActor {
                hp: 100,
                agl: 16,
                stat_a: 32,
                stat_b: 48,
                ..Default::default()
            },
            element_affinity_pct: 200, // weakness -> double the attacker roll
            rng: [0, 0, 0, 0, 0],
        };
        // attacker 60 * 200/100 = 120. defender 37. threshold 57; 120 >= 57 -> no bonus.
        assert_eq!(arts_physical_predamage(&i), (120, 37));
    }

    #[test]
    fn victory_gold_matches_runtime_gimard() {
        // Lone Gimard: record gold 60 -> 60>>1 = 30 accumulated -> 30 - (30>>1) = 15.
        let acc = victory_gold_per_monster(60);
        assert_eq!(acc, 30);
        assert_eq!(victory_gold_finalize(acc, false), 15);
        // +25% "extra gold" bonus: 30 + (30>>2 = 7) = 37 -> 37 - (37>>1 = 18) = 19.
        assert_eq!(victory_gold_finalize(acc, true), 19);
        // Multi-enemy accumulation rounds per monster: gold 60 + 61 -> 30 + 30 = 60.
        let two = victory_gold_per_monster(60) + victory_gold_per_monster(61);
        assert_eq!(two, 60);
        assert_eq!(victory_gold_finalize(two, false), 30);
    }

    #[test]
    fn victory_exp_per_member_scales_and_ceils() {
        // 100 exp, 3 alive: 100 - (100>>2 = 25) = 75; ceil(75/3) = 25.
        assert_eq!(victory_exp_per_member(100, 3), 25);
        // 100 exp, 1 alive: 75 -> ceil(75/1) = 75.
        assert_eq!(victory_exp_per_member(100, 1), 75);
        // Ceiling: 10 exp, 3 alive: 10 - 2 = 8; ceil(8/3) = 3 (floor would give 2).
        assert_eq!(victory_exp_per_member(10, 3), 3);
        // alive 0 -> 0 (guard).
        assert_eq!(victory_exp_per_member(100, 0), 0);
    }

    // -- FUN_801ddb30 finisher -------------------------------------------------

    fn finish(predamage: u32) -> DamageFinish {
        DamageFinish {
            predamage,
            attacker_slot: 3, // enemy attacker
            defender_slot: 0, // party defender
            attacker_element: 0,
            defender_resist: DefenderResist::default(),
            defender_guarding: false,
            enemy_defender_halve: false,
            bypass_party_resist: false,
            summon_power_pct: 0,
            floor_rand: 0,
        }
    }

    #[test]
    fn defender_resist_passive_index_layout() {
        // The elemental-guard passives are contiguous at ability-bit index
        // `0x1D + element` (Earth..Dark); words 0/1 of the bitfield split
        // them across the +0xF4/+0xF8 boundary exactly as FUN_801ddb30's
        // ladder reads them. All Guard (0x24) is the absorb gate.
        for element in 0..=6u8 {
            let index = 0x1D + element as u32;
            let (w0, w1) = if index < 32 {
                (1u32 << index, 0)
            } else {
                (0, 1u32 << (index - 32))
            };
            let r = DefenderResist::from_ability_words(w0, w1);
            for probe in 0..=6u8 {
                assert_eq!(
                    r.resists(probe),
                    probe == element,
                    "guard passive 0x{index:02X} must resist element {element} only"
                );
            }
        }
        let all_guard = DefenderResist::from_ability_words(0, 1 << (0x24 - 32));
        assert_eq!(all_guard.hi & 0x10, 0x10, "All Guard = the absorb gate bit");
    }

    #[test]
    fn damage_finish_passthrough_when_no_mitigation() {
        // No resistance, no guard, no cap: over passes through unchanged.
        assert_eq!(damage_finish(&finish(500)), 500);
    }

    #[test]
    fn damage_finish_halves_on_matching_element_resist() {
        let mut i = finish(500);
        // Defender resists element 0 / Earth Guard (word-0 bit 0x20000000);
        // attacker is element 0.
        i.defender_resist = DefenderResist {
            lo: 0x2000_0000,
            hi: 0,
        };
        i.attacker_element = 0;
        assert_eq!(damage_finish(&i), 250);
        // A non-matching attacker element (1) is not resisted -> full damage.
        i.attacker_element = 1;
        assert_eq!(damage_finish(&i), 500);
    }

    #[test]
    fn damage_finish_low_word_elements_and_absorb_gate() {
        let mut i = finish(800);
        // Element 3 / Wind Guard lives in the +0xF8 word bit 0x1.
        i.defender_resist = DefenderResist { lo: 0, hi: 0x1 };
        i.attacker_element = 3;
        assert_eq!(damage_finish(&i), 400);
        // All-Guard gate (hi & 0x10) set + elemental attacker -> 3/4 scale,
        // ignoring the per-element ladder: 800 * 3 >> 2 = 600.
        i.defender_resist = DefenderResist {
            lo: 0,
            hi: 0x1 | 0x10,
        };
        assert_eq!(damage_finish(&i), 600);
        // ...but a non-elemental attacker (7) bypasses the gate -> ladder runs,
        // and element 7 resists nothing -> full damage.
        i.attacker_element = 7;
        assert_eq!(damage_finish(&i), 800);
    }

    #[test]
    fn damage_finish_guard_and_resist_stack() {
        let mut i = finish(800);
        i.defender_resist = DefenderResist {
            lo: 0x2000_0000,
            hi: 0,
        }; // element 0 resist -> /2
        i.defender_guarding = true; // -> /2 again
        // 800 -> 400 -> 200.
        assert_eq!(damage_finish(&i), 200);
    }

    #[test]
    fn damage_finish_enemy_defender_halve() {
        let mut i = finish(500);
        i.defender_slot = 3; // enemy defender -> party-resist block skipped
        i.attacker_slot = 0; // party attacker
        i.defender_resist = DefenderResist {
            lo: 0xFFFF_FFFF,
            hi: 0xFFFF_FFFF,
        }; // ignored for enemy defender
        assert_eq!(damage_finish(&i), 500, "no halve flag -> full");
        i.enemy_defender_halve = true;
        assert_eq!(damage_finish(&i), 250);
    }

    #[test]
    fn damage_finish_bypass_party_resist() {
        let mut i = finish(500);
        i.defender_resist = DefenderResist {
            lo: 0x2000_0000,
            hi: 0,
        };
        i.attacker_element = 0;
        i.bypass_party_resist = true; // resistance block skipped entirely
        assert_eq!(damage_finish(&i), 500);
    }

    #[test]
    fn damage_finish_no_damage_floor_uses_rand() {
        // Mitigation reduces over to 0 -> floor rand()%9 + 8.
        let mut i = finish(1);
        i.defender_resist = DefenderResist {
            lo: 0x2000_0000,
            hi: 0,
        };
        i.attacker_element = 0; // 1 >> 1 = 0 -> floor fires
        i.floor_rand = 0; // 0 % 9 + 8 = 8
        assert_eq!(damage_finish(&i), 8);
        i.floor_rand = 17; // 17 % 9 = 8 -> 8 + 8 = 16
        assert_eq!(damage_finish(&i), 16);
        // A predamage of 0 also triggers the floor.
        assert_eq!(damage_finish(&finish(0)), 8);
    }

    #[test]
    fn damage_finish_summon_power_scale() {
        let mut i = finish(400);
        i.attacker_slot = 7; // summon body
        i.summon_power_pct = 150; // 400 * 150 / 100 = 600
        assert_eq!(damage_finish(&i), 600);
        i.summon_power_pct = 50; // 400 * 50 / 100 = 200
        assert_eq!(damage_finish(&i), 200);
    }

    #[test]
    fn damage_finish_caps_at_9999() {
        assert_eq!(damage_finish(&finish(50_000)), 9999);
        // Exactly 9999 passes; 10000 caps.
        assert_eq!(damage_finish(&finish(9999)), 9999);
        assert_eq!(damage_finish(&finish(10_000)), 9999);
    }

    #[test]
    fn spirit_gauge_fill_basic_accrual() {
        // over 50 of maxhp 500 -> pct = 10; gauge 0 -> 10.
        assert_eq!(
            spirit_gauge_fill(50, 500, 0, DefenderResist::default(), true),
            10
        );
        // pct floors at 1 even for tiny hits.
        assert_eq!(
            spirit_gauge_fill(1, 500, 0, DefenderResist::default(), true),
            1
        );
        // Clamps to 100.
        assert_eq!(
            spirit_gauge_fill(500, 500, 50, DefenderResist::default(), true),
            100
        );
    }

    #[test]
    fn spirit_gauge_fill_gain_up_bits_party_only() {
        // pct = 40 (over 200 / max 500). hi & 0x200 -> +pct>>2 (=10); base +pct.
        let resist = DefenderResist { lo: 0, hi: 0x200 };
        // 0 + 10 + 40 = 50.
        assert_eq!(spirit_gauge_fill(200, 500, 0, resist, true), 50);
        // hi & 0x100 -> +pct/10 (=4); 0 + 4 + 40 = 44.
        let resist = DefenderResist { lo: 0, hi: 0x100 };
        assert_eq!(spirit_gauge_fill(200, 500, 0, resist, true), 44);
        // Both bits: +10 +4 +40 = 54.
        let resist = DefenderResist { lo: 0, hi: 0x300 };
        assert_eq!(spirit_gauge_fill(200, 500, 0, resist, true), 54);
        // Enemy defender (not party): gain-up bits ignored -> just +pct.
        assert_eq!(spirit_gauge_fill(200, 500, 0, resist, false), 40);
    }

    #[test]
    fn spirit_gauge_fill_zero_maxhp_guard() {
        // Retail traps; the kernel returns the (clamped) gauge unchanged.
        assert_eq!(
            spirit_gauge_fill(100, 0, 73, DefenderResist::default(), true),
            73
        );
    }

    // --- summon spell XP + level-up (FUN_801ddb30 tail / FUN_801e70bc) -----

    #[test]
    fn summon_spell_xp_gain_non_kill_is_damage_proportional() {
        // damage * 12 / max_hp single-target; * 4 group-target.
        assert_eq!(summon_spell_xp_gain(100, 500, 600, false), 2); // 1200/600
        assert_eq!(summon_spell_xp_gain(100, 500, 600, true), 0); // 400/600
        assert_eq!(summon_spell_xp_gain(300, 500, 600, true), 2); // 1200/600
        // Integer floor, exactly as the MIPS divide.
        assert_eq!(summon_spell_xp_gain(149, 500, 600, false), 2); // 1788/600
    }

    #[test]
    fn summon_spell_xp_gain_kill_is_flat_unit() {
        // damage >= target_hp -> flat 12 (single) / 4 (group), no division.
        assert_eq!(summon_spell_xp_gain(500, 500, 600, false), 12);
        assert_eq!(summon_spell_xp_gain(9999, 2, 600, true), 4);
    }

    #[test]
    fn summon_spell_xp_gain_low_hp_target_grants_nothing() {
        // Both retail branches gate on target_hp >= 2.
        assert_eq!(summon_spell_xp_gain(1, 1, 600, false), 0);
        assert_eq!(summon_spell_xp_gain(9999, 1, 600, false), 0);
        assert_eq!(summon_spell_xp_gain(9999, 0, 600, true), 0);
        // Zero max HP on a non-kill: retail traps, engine returns 0.
        assert_eq!(summon_spell_xp_gain(1, 500, 0, false), 0);
    }

    #[test]
    fn summon_magic_level_threshold_default_mult_is_raw_table() {
        // mult = 2 -> (t * 2) >> 1 == t.
        let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
        assert_eq!(summon_magic_level_threshold(0x81, 1, &table), Some(17));
        assert_eq!(summon_magic_level_threshold(0x81, 8, &table), Some(536));
    }

    #[test]
    fn summon_magic_level_threshold_triple_ids_scale_1_5x() {
        // mult = 3 -> (t * 3) >> 1.
        let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
        for id in SUMMON_XP_TRIPLE_THRESHOLD_IDS {
            assert_eq!(summon_magic_level_threshold(id, 1, &table), Some(25)); // 51 >> 1
            assert_eq!(summon_magic_level_threshold(id, 2, &table), Some(75)); // 150 >> 1
        }
    }

    #[test]
    fn summon_magic_level_up_is_strict_greater_and_caps_at_9() {
        let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
        // Strict: xp == threshold does NOT level.
        assert!(!summon_magic_levels_up(0x81, 1, 17, &table));
        assert!(summon_magic_levels_up(0x81, 1, 18, &table));
        // Level cap: 9 never levels (retail pre-increment `< 9` guard).
        assert!(!summon_magic_levels_up(0x81, 9, 99999, &table));
        // Level 0 guarded (retail would read table[-1]).
        assert!(!summon_magic_levels_up(0x81, 0, 99999, &table));
        // Short table: level 8 needs table[7].
        assert!(!summon_magic_levels_up(0x81, 8, 99999, &table[..7]));
    }
}
