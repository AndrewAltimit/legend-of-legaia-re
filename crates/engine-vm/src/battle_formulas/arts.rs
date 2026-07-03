//! Arts / physical damage-roll branch (`FUN_801dd0ac`, `attacker_slot != 7`).
//! Split out of `battle_formulas.rs`.

use super::*;

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
