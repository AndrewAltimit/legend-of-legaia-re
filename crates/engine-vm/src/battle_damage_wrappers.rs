//! The two per-move damage-roll wrappers `FUN_801DD4B0` / `FUN_801DD6B4`.
//!
//! These are the sibling entry points the battle-action path calls to resolve
//! one hit whose damage is *not* routed through the shared move-power kernel
//! `FUN_801DD0AC` (ported in [`crate::battle_formulas`]). Each one:
//!
//! 1. reads the attacker and defender battle-actor records out of the actor
//!    pointer table `DAT_801C9370`,
//! 2. builds an attacker roll and a defender roll from a caller-supplied power
//!    scalar plus a handful of actor stat fields and two/three `rand()` draws,
//! 3. runs the shared scale stage `FUN_801DD864` (element affinity + status
//!    weaken on the attacker, guard-double + status weaken on the defender),
//! 4. re-rolls the attacker when it failed to clear the defender's mitigation,
//! 5. calls the shared finisher `FUN_801DDB30`, and
//! 6. returns `attacker_roll - defender_roll` as the net damage.
//!
//! The two differ in exactly two places, and the second one is the interesting
//! one:
//!
//! | | `FUN_801DD4B0` | `FUN_801DD6B4` |
//! |---|---|---|
//! | attacker stat mixed into the roll | AGL `+0x168` (also `* 2`) | spell power `+0x158` |
//! | defender mitigation terms | AGL `+0x168` (`>> 1` modulus, `* 2` flat) plus `+0x15C`/`+0x160` `>> 4` | `+0x15C`/`+0x160` only (`>> 3` modulus, `>> 1` flat) |
//! | finisher `param_5` | `0` - the equipment resist ladder runs | `1` - **the whole party-defender resist block is skipped** |
//!
//! The `param_5 = 1` path is the resist-**bypass** wrapper: a hit routed
//! through it takes no Earth/Luminous-Jewel or All-Guard reduction even when
//! the defender is elementally warded. That is the mechanism behind the
//! non-elemental capture-class boss casts. The affinity scale still reads the
//! caster's slot element either way, so only the *defender's* jewel stage is
//! dropped.
//!
//! Both kernels are closed-form given their inputs: they take the actor stat
//! fields and the `rand()` draws as parameters rather than reaching into a
//! global actor pool, so they are directly testable and reusable by any host.
//! The finisher itself is [`crate::battle_formulas::damage_finish`]; the
//! `bypass_party_resist` flag each wrapper must pass is exposed here as
//! [`PHYSICAL_BYPASSES_PARTY_RESIST`] / [`SPELL_BYPASSES_PARTY_RESIST`].
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801dd4b0.txt`
//! and `_801dd6b4.txt`; behaviour summary in
//! `docs/subsystems/battle-action.md` and `docs/subsystems/battle-formulas.md`.

use crate::battle_formulas::{apply_element_affinity, apply_status_weaken};

/// The `param_5` the physical wrapper `FUN_801DD4B0` passes to the finisher:
/// `0`, so the party-defender equipment resist ladder runs.
pub const PHYSICAL_BYPASSES_PARTY_RESIST: bool = false;

/// The `param_5` the spell wrapper `FUN_801DD6B4` passes to the finisher: `1`,
/// so the party-defender resist block is skipped entirely.
pub const SPELL_BYPASSES_PARTY_RESIST: bool = true;

/// The attacker-side actor fields both wrappers read.
#[derive(Debug, Clone, Copy, Default)]
pub struct WrapperAttacker {
    /// Current HP (`+0x14C`). Contributes `hp >> 8` to the roll.
    pub hp: u16,
    /// Agility (`+0x168`). Read by `FUN_801DD4B0` only.
    pub agl: u16,
    /// Spell power (`+0x158`). Read by `FUN_801DD6B4` only.
    pub spell_power: u16,
    /// Status bitfield (`+0x16E`): bit `0x1` scales the roll to 9/10, bit
    /// `0x2` to 7/10, applied in that order (the `FUN_801DD864` stage).
    pub status: u16,
}

/// The defender-side actor fields both wrappers read.
#[derive(Debug, Clone, Copy, Default)]
pub struct WrapperDefender {
    /// Current HP (`+0x14C`). Contributes `hp >> 8` to both wrappers' rolls.
    pub hp: u16,
    /// Agility (`+0x168`). Read by `FUN_801DD4B0` only.
    pub agl: u16,
    /// Defence term A (`+0x15C`).
    pub stat_a: u16,
    /// Defence term B (`+0x160`).
    pub stat_b: u16,
    /// Status bitfield (`+0x16E`), scaled the same way as the attacker's.
    pub status: u16,
    /// Guard byte (`+0x1DE`); `== 4` doubles the defender roll in the
    /// `FUN_801DD864` stage.
    pub guard: u8,
}

/// Attacker roll of `FUN_801DD4B0`:
/// `rand0 % ((power >> 2) + 1) + rand1 % ((agl >> 1) + 1) + (hp >> 8) + power +
/// agl * 2`.
///
/// Two `rand()` draws, in call order. `power` is unsigned - the retail shift is
/// `srl` and the first modulus is taken with `divu`.
///
/// PORT: FUN_801DD4B0 (attacker-roll stage)
pub fn physical_attacker_roll(power: u32, a: &WrapperAttacker, rng: [u16; 2]) -> u32 {
    let modulus_power = (power >> 2) + 1;
    let modulus_agl = (a.agl as u32 >> 1) + 1;
    (rng[0] as u32) % modulus_power
        + (rng[1] as u32) % modulus_agl
        + (a.hp as u32 >> 8)
        + power
        + a.agl as u32 * 2
}

/// Defender roll of `FUN_801DD4B0`:
/// `rand % ((agl >> 1) + 1) + (hp >> 8) + (stat_a >> 4) + (stat_b >> 4) +
/// agl * 2`.
///
/// Identical arithmetic to the shared kernel's defender roll
/// (`FUN_801DD0AC`), which is why the physical wrapper reuses the same stat
/// set.
///
/// PORT: FUN_801DD4B0 (defender-roll stage)
pub fn physical_defender_roll(d: &WrapperDefender, rand: u16) -> u32 {
    let modulus = (d.agl as u32 >> 1) + 1;
    (rand as u32) % modulus
        + (d.hp as u32 >> 8)
        + (d.stat_a as u32 >> 4)
        + (d.stat_b as u32 >> 4)
        + d.agl as u32 * 2
}

/// Attacker roll of `FUN_801DD6B4`:
/// `rand % ((power >> 2) + 1) + (hp >> 8) + power + spell_power`.
///
/// One `rand()` draw. The AGL term the physical wrapper carries is replaced by
/// the flat spell-power stat `+0x158`.
///
/// PORT: FUN_801DD6B4 (attacker-roll stage)
pub fn spell_attacker_roll(power: u32, a: &WrapperAttacker, rand: u16) -> u32 {
    let modulus_power = (power >> 2) + 1;
    (rand as u32) % modulus_power + (a.hp as u32 >> 8) + power + a.spell_power as u32
}

/// Defender roll of `FUN_801DD6B4`:
/// `rand % (((stat_a + stat_b) >> 3) + 1) + (hp >> 8) + (stat_a >> 1) +
/// (stat_b >> 1)`.
///
/// The defender's AGL is not read at all on this path; the two defence terms
/// carry the whole mitigation, and they carry it eight times as heavily as in
/// the physical wrapper (`>> 1` instead of `>> 4`).
///
/// PORT: FUN_801DD6B4 (defender-roll stage)
pub fn spell_defender_roll(d: &WrapperDefender, rand: u16) -> u32 {
    let sum = d.stat_a as u32 + d.stat_b as u32;
    let modulus = (sum >> 3) + 1;
    (rand as u32) % modulus + (d.hp as u32 >> 8) + (d.stat_a as u32 >> 1) + (d.stat_b as u32 >> 1)
}

/// The bonus re-roll both wrappers share: when the scaled attacker roll is
/// below `defender_roll + power`, the attacker roll is rebuilt as
/// `defender_roll + power + rand % ((power >> 2) + 1)`.
///
/// One `rand()` draw, and the modulus is the *same* one the attacker roll used.
/// The comparison retail makes is unsigned (`sltu`).
///
/// PORT: FUN_801DD4B0 (bonus arm; `FUN_801DD6B4`'s is instruction-identical)
pub fn wrapper_bonus_roll(defender_roll: u32, power: u32, rand: u16) -> u32 {
    let modulus_power = (power >> 2) + 1;
    defender_roll + power + (rand as u32) % modulus_power
}

/// Both wrappers' scale stage, `FUN_801DD864` as reached from here: element
/// affinity then status weaken on the attacker; guard-double then status weaken
/// on the defender.
fn scale(
    attacker_roll: u32,
    defender_roll: u32,
    a: &WrapperAttacker,
    d: &WrapperDefender,
    element_affinity_pct: u8,
) -> (u32, u32) {
    let mut atk = apply_element_affinity(attacker_roll, element_affinity_pct);
    atk = apply_status_weaken(atk, a.status);

    let mut def = defender_roll;
    if d.guard == 4 {
        def = def.saturating_mul(2);
    }
    def = apply_status_weaken(def, d.status);

    (atk, def)
}

/// The physical wrapper `FUN_801DD4B0`, up to but not including the finisher.
///
/// Returns `(attacker_roll, defender_roll)` after the scale stage and the
/// conditional bonus arm. The pre-finisher damage retail hands
/// `FUN_801DDB30` is `attacker_roll - defender_roll`; feed it to
/// [`crate::battle_formulas::damage_finish`] with
/// `bypass_party_resist = `[`PHYSICAL_BYPASSES_PARTY_RESIST`].
///
/// `rng` is three draws in retail's call order: attacker `x2`, defender `x1`.
/// `bonus_rng` supplies the fourth draw and is invoked **only when the bonus
/// arm fires**, matching the point retail draws it - a caller pulling from a
/// shared RNG cursor therefore advances it by three or four draws exactly as
/// retail does.
///
/// PORT: FUN_801DD4B0
pub fn physical_wrapper_predamage(
    power: u32,
    a: &WrapperAttacker,
    d: &WrapperDefender,
    element_affinity_pct: u8,
    rng: [u16; 3],
    bonus_rng: impl FnOnce() -> u16,
) -> (u32, u32) {
    let atk = physical_attacker_roll(power, a, [rng[0], rng[1]]);
    let def = physical_defender_roll(d, rng[2]);
    let (mut atk, def) = scale(atk, def, a, d, element_affinity_pct);
    if atk < def + power {
        atk = wrapper_bonus_roll(def, power, bonus_rng());
    }
    (atk, def)
}

/// The spell / capture-class wrapper `FUN_801DD6B4`, up to but not including
/// the finisher. Same contract as [`physical_wrapper_predamage`], except that
/// `rng` is two draws (attacker `x1`, defender `x1`) and the finisher must be
/// called with `bypass_party_resist = `[`SPELL_BYPASSES_PARTY_RESIST`].
///
/// PORT: FUN_801DD6B4
pub fn spell_wrapper_predamage(
    power: u32,
    a: &WrapperAttacker,
    d: &WrapperDefender,
    element_affinity_pct: u8,
    rng: [u16; 2],
    bonus_rng: impl FnOnce() -> u16,
) -> (u32, u32) {
    let atk = spell_attacker_roll(power, a, rng[0]);
    let def = spell_defender_roll(d, rng[1]);
    let (mut atk, def) = scale(atk, def, a, d, element_affinity_pct);
    if atk < def + power {
        atk = wrapper_bonus_roll(def, power, bonus_rng());
    }
    (atk, def)
}

/// The net damage either wrapper returns: `attacker_roll - defender_roll`.
///
/// Retail computes this with `subu` and returns it as a signed word, so a roll
/// that failed to clear the defender yields a negative value. The bonus arm
/// makes that rare but not impossible (it fires only on the *unsigned*
/// comparison, and the scale stage runs before it). Callers that want the HP
/// loss should saturate at zero before the finisher, which is what
/// [`crate::battle_formulas::damage_finish`] expects.
pub fn wrapper_net_damage(attacker_roll: u32, defender_roll: u32) -> i32 {
    attacker_roll.wrapping_sub(defender_roll) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atk() -> WrapperAttacker {
        WrapperAttacker {
            hp: 0x300,
            agl: 40,
            spell_power: 55,
            status: 0,
        }
    }

    fn def() -> WrapperDefender {
        WrapperDefender {
            hp: 0x500,
            agl: 32,
            stat_a: 64,
            stat_b: 48,
            status: 0,
            guard: 0,
        }
    }

    #[test]
    fn physical_attacker_roll_matches_hand_evaluation() {
        // power 100: modulus_power = 26, modulus_agl = 21.
        // 7 % 26 + 5 % 21 + (0x300 >> 8) + 100 + 80 = 7 + 5 + 3 + 100 + 80
        assert_eq!(physical_attacker_roll(100, &atk(), [7, 5]), 195);
    }

    #[test]
    fn physical_defender_roll_matches_hand_evaluation() {
        // modulus = 17; 9 % 17 + (0x500 >> 8) + (64 >> 4) + (48 >> 4) + 64
        assert_eq!(physical_defender_roll(&def(), 9), 9 + 5 + 4 + 3 + 64);
    }

    #[test]
    fn spell_attacker_roll_uses_spell_power_not_agl() {
        // modulus_power = 26; 3 % 26 + 3 + 100 + 55. AGL never read.
        let mut a = atk();
        assert_eq!(spell_attacker_roll(100, &a, 3), 3 + 3 + 100 + 55);
        a.agl = 999;
        assert_eq!(spell_attacker_roll(100, &a, 3), 3 + 3 + 100 + 55);
    }

    #[test]
    fn spell_defender_roll_ignores_agility_and_weights_defence_terms() {
        // sum = 112, modulus = 15; 4 % 15 + 5 + 32 + 24
        let mut d = def();
        assert_eq!(spell_defender_roll(&d, 4), 4 + 5 + 32 + 24);
        d.agl = 999;
        assert_eq!(spell_defender_roll(&d, 4), 4 + 5 + 32 + 24);
    }

    #[test]
    fn bonus_arm_fires_only_when_the_attacker_falls_short() {
        // A huge attacker roll clears `def + power`, so the bonus closure must
        // never run; retail draws that rand() inside the arm.
        let mut drew = false;
        let (a1, _) = physical_wrapper_predamage(
            10,
            &WrapperAttacker {
                hp: 0xFFFF,
                agl: 400,
                spell_power: 0,
                status: 0,
            },
            &WrapperDefender::default(),
            100,
            [0, 0, 0],
            || {
                drew = true;
                0
            },
        );
        assert!(!drew, "bonus rand drawn on a clearing roll");
        assert!(a1 > 0);

        // A zeroed attacker against a defended target must take the arm.
        let mut drew = false;
        let (a2, d2) = physical_wrapper_predamage(
            50,
            &WrapperAttacker::default(),
            &def(),
            100,
            [0, 0, 0],
            || {
                drew = true;
                3
            },
        );
        assert!(drew, "bonus rand not drawn on a short roll");
        // rebuilt as def + power + 3 % ((50 >> 2) + 1 = 13)
        assert_eq!(a2, d2 + 50 + 3);
    }

    #[test]
    fn guard_doubles_the_defender_roll_in_the_scale_stage() {
        let mut d = def();
        let (_, plain) = physical_wrapper_predamage(100, &atk(), &d, 100, [1, 1, 1], || 0);
        d.guard = 4;
        let (_, guarded) = physical_wrapper_predamage(100, &atk(), &d, 100, [1, 1, 1], || 0);
        assert_eq!(guarded, plain * 2);
    }

    #[test]
    fn element_affinity_scales_only_the_attacker_side() {
        // A defenceless target and a small power keep the bonus arm shut, so
        // the affinity scale is the only thing that moves between the two runs.
        let d = WrapperDefender::default();
        let (a_full, d_full) = spell_wrapper_predamage(4, &atk(), &d, 100, [1, 1], || 0);
        let (a_half, d_half) = spell_wrapper_predamage(4, &atk(), &d, 50, [1, 1], || 0);
        assert_eq!(d_full, d_half, "affinity must not touch the defender roll");
        assert_eq!(a_half, a_full / 2);
    }

    #[test]
    fn affinity_can_drop_the_attacker_under_the_bonus_threshold() {
        // A halved attacker roll that no longer clears `defender + power` takes
        // the bonus arm, which rebuilds it from the defender roll - so the
        // affinity scale is *not* observable in the returned value there. This
        // is retail's ordering (scale, then the conditional re-roll), and it is
        // why the previous test has to disarm the bonus.
        let mut drew = false;
        let (a, d) = spell_wrapper_predamage(200, &atk(), &def(), 50, [1, 1], || {
            drew = true;
            0
        });
        assert!(drew);
        assert_eq!(a, d + 200);
    }

    #[test]
    fn the_two_wrappers_disagree_on_the_finisher_flag() {
        // The whole point of the pair: same shape, opposite resist policy.
        assert_ne!(
            PHYSICAL_BYPASSES_PARTY_RESIST, SPELL_BYPASSES_PARTY_RESIST,
            "the wrappers must pass opposite param_5 values"
        );
    }

    #[test]
    fn net_damage_is_a_signed_difference() {
        assert_eq!(wrapper_net_damage(100, 40), 60);
        assert_eq!(wrapper_net_damage(40, 100), -60);
    }

    #[test]
    fn zero_power_does_not_divide_by_zero() {
        // (0 >> 2) + 1 == 1, so the modulus is never zero even at power 0 -
        // the retail `divu` break vector is unreachable from this path.
        assert_eq!(
            physical_attacker_roll(0, &WrapperAttacker::default(), [7, 0]),
            0
        );
        assert_eq!(spell_attacker_roll(0, &WrapperAttacker::default(), 7), 0);
        // The spell defender modulus is `((0 + 0) >> 3) + 1 == 1` likewise.
        assert_eq!(spell_defender_roll(&WrapperDefender::default(), 7), 0);
    }
}
