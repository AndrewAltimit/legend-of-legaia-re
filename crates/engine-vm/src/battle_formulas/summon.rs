//! Summon-magic damage roll + scale kernels (`FUN_801dd0ac` summon branch,
//! `FUN_801dd864` modifiers). Split out of `battle_formulas.rs`.

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
