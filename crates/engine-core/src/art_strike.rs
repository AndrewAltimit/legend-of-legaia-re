//! Tactical-Art strike applier.
//!
//! Translates an
//! [`legaia_engine_vm::battle_action::ArtStrikeInfo`] (resolved per-strike
//! values from the actor's chosen art) into the concrete side-effects
//! engines apply: HP deduction on the target, optional status flag, and a
//! list of audio / visual cues scheduled for the right anim frame.
//!
//! The state machine in `engine-vm` only resolves the values from the art
//! record; this module is the engine-side pure function that turns them
//! into outcomes. The world drains the resulting [`ArtStrikeOutcome`] into
//! its battle event queue.
//!
//! ## Inputs
//!
//! - `attacker_atk` / `target_def`: the actor's weapon / target's defense.
//!   Engines look these up from the per-actor stat record (battle actor
//!   `+0x9C` / `+0x9E` regions).
//! - `info`: the [`ArtStrikeInfo`] the SM produced for this strike.
//!
//! ## Outputs
//!
//! [`ArtStrikeOutcome`] is plain data — engines apply each field through
//! whatever runtime path they have for HP/status/SFX.

use legaia_art::power::{PowerByte, PowerTarget};
use legaia_art::record::{EnemyEffect, HitCue};
use legaia_engine_vm::battle_action::ArtStrikeInfo;
use legaia_engine_vm::battle_formulas::art_strike_damage_default;

/// Concrete side-effects an [`ArtStrikeInfo`] produces. Plain data —
/// engines fold each field into their runtime in whatever order they like.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArtStrikeOutcome {
    /// HP delta to deduct from the target. `None` when the strike's power
    /// byte was outside the damage encoding range (the art's power vec
    /// terminator slot, or an alt-range hit that missed). Engines that
    /// wire the power table fall back to default damage in that case.
    pub damage: Option<u16>,
    /// Status to inflict on the target on hit, if non-`EnemyEffect::None`.
    pub enemy_effect: EnemyEffect,
    /// Audio / visual cues scheduled for this strike. Each entry tells the
    /// engine "play `kind` at `timing_frames` after strike-start." The
    /// retail engine fires these inline during the strike anim — engines
    /// that compute their own anim timing can ignore the timing field.
    pub cues: Vec<ScheduledCue>,
    /// Whether the art's `power` byte indicated an "alt-range" hit (misses
    /// floating enemies on LDF, short enemies on UDF). Engines apply their
    /// own range-class check and zero out [`Self::damage`] when the target
    /// is in the missing class.
    pub alt_range: bool,
    /// Defense category the multiplier targets. `None` when the strike has
    /// no power byte (engines should treat this as "no damage").
    pub power_target: Option<PowerTarget>,
}

/// One scheduled audio or visual cue produced by an art strike. Mirrors
/// [`HitCue`] but with the `timing_frames` field already shifted so the
/// engine can compute "fire on frame N after strike start".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledCue {
    /// Frame within the strike anim when the cue fires.
    pub timing_frames: u16,
    /// Cue kind. `0x1A` = sound, `0x4C` = hit visual; other values are
    /// game-specific.
    pub kind: u16,
}

impl ScheduledCue {
    /// `true` if the cue is the SFX-trigger kind (`0x1A`).
    pub fn is_sound(&self) -> bool {
        self.kind == 0x1A
    }

    /// `true` if the cue is a hit-effect (visible flash / damage popup) —
    /// kind `0x4C`.
    pub fn is_hit_effect(&self) -> bool {
        self.kind == 0x4C
    }
}

impl From<HitCue> for ScheduledCue {
    fn from(cue: HitCue) -> Self {
        Self {
            timing_frames: cue.timing_frames,
            kind: cue.kind,
        }
    }
}

/// Compute the [`ArtStrikeOutcome`] for one strike.
///
/// The damage formula is
/// [`legaia_engine_vm::battle_formulas::art_strike_damage_default`] —
/// `damage = max(1, attack × multiplier / 16 - defense)`. The multiplier
/// is read from `info.power.multiplier` after [`PowerByte::from_byte`]
/// decoding; "no damage" power bytes (terminator slots) yield
/// `damage = None`.
///
/// The `cues` list pulls from `info.hit_cue` when present — at most one
/// cue per strike. The retail art format reserves up to four hit cues per
/// art, with the cue index implicitly matching the strike index. If the
/// caller needs all cues across multiple strikes, run [`apply_art_strike`]
/// per strike and concatenate.
pub fn apply_art_strike(
    attacker_atk: u16,
    target_def: u16,
    info: &ArtStrikeInfo,
) -> ArtStrikeOutcome {
    let (damage, alt_range, power_target) = match info.power {
        Some(PowerByte::Damage(p)) => (
            Some(art_strike_damage_default(
                attacker_atk,
                target_def,
                p.multiplier,
            )),
            p.alt_range,
            Some(p.target),
        ),
        Some(PowerByte::NoDamage) | None => (None, false, None),
    };
    let cues: Vec<ScheduledCue> = info.hit_cue.map(|c| vec![c.into()]).unwrap_or_default();
    ArtStrikeOutcome {
        damage,
        enemy_effect: info.enemy_effect,
        cues,
        alt_range,
        power_target,
    }
}

/// Variant that takes a custom `divisor` (the fixed-point base for the
/// power multiplier). Useful for engines reproducing a captured retail
/// state where `divisor != 16`.
pub fn apply_art_strike_with_divisor(
    attacker_atk: u16,
    target_def: u16,
    power_divisor: u8,
    min_floor: u16,
    info: &ArtStrikeInfo,
) -> ArtStrikeOutcome {
    use legaia_engine_vm::battle_formulas::art_strike_damage;
    let (damage, alt_range, power_target) = match info.power {
        Some(PowerByte::Damage(p)) => (
            Some(art_strike_damage(
                attacker_atk,
                target_def,
                p.multiplier,
                power_divisor,
                min_floor,
            )),
            p.alt_range,
            Some(p.target),
        ),
        Some(PowerByte::NoDamage) | None => (None, false, None),
    };
    let cues: Vec<ScheduledCue> = info.hit_cue.map(|c| vec![c.into()]).unwrap_or_default();
    ArtStrikeOutcome {
        damage,
        enemy_effect: info.enemy_effect,
        cues,
        alt_range,
        power_target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_art::Character;
    use legaia_art::queue::ActionConstant;

    fn synthetic_info(power: Option<PowerByte>) -> ArtStrikeInfo {
        ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power,
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::None,
            hit_cue: None,
        }
    }

    #[test]
    fn strike_with_no_power_byte_yields_no_damage() {
        let info = synthetic_info(None);
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.damage, None);
        assert_eq!(outcome.power_target, None);
        assert!(!outcome.alt_range);
        assert!(outcome.cues.is_empty());
        assert_eq!(outcome.enemy_effect, EnemyEffect::None);
    }

    #[test]
    fn strike_with_no_damage_byte_yields_no_damage() {
        let info = synthetic_info(Some(PowerByte::NoDamage));
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.damage, None);
        assert_eq!(outcome.power_target, None);
    }

    #[test]
    fn strike_udf_28_dishes_max_tier_damage() {
        // 0x1A → UDF × 28; attack=64 def=10 → (64*28)/16 - 10 = 112 - 10 = 102.
        let info = synthetic_info(Some(PowerByte::from_byte(0x1A)));
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.damage, Some(102));
        assert_eq!(outcome.power_target, Some(PowerTarget::Udf));
        assert!(!outcome.alt_range);
    }

    #[test]
    fn strike_ldf_12_lowest_tier_damage() {
        // 0x1B → LDF × 12; attack=64 def=10 → (64*12)/16 - 10 = 48 - 10 = 38.
        let info = synthetic_info(Some(PowerByte::from_byte(0x1B)));
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.damage, Some(38));
        assert_eq!(outcome.power_target, Some(PowerTarget::Ldf));
    }

    #[test]
    fn strike_alt_range_marks_outcome_flag() {
        // 0x0C → alt-range UDF × 12.
        let info = synthetic_info(Some(PowerByte::from_byte(0x0C)));
        let outcome = apply_art_strike(64, 10, &info);
        assert!(outcome.alt_range);
        assert_eq!(outcome.power_target, Some(PowerTarget::Udf));
    }

    #[test]
    fn strike_emits_cue_when_hit_cue_present() {
        let mut info = synthetic_info(Some(PowerByte::from_byte(0x1A)));
        info.hit_cue = Some(HitCue::from_word(0x0010_001A)); // sound on frame 16
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.cues.len(), 1);
        assert!(outcome.cues[0].is_sound());
        assert_eq!(outcome.cues[0].timing_frames, 0x10);
    }

    #[test]
    fn strike_emits_visual_cue_kind_4c() {
        let mut info = synthetic_info(Some(PowerByte::from_byte(0x1A)));
        info.hit_cue = Some(HitCue::from_word(0x0020_004C));
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.cues.len(), 1);
        assert!(outcome.cues[0].is_hit_effect());
        assert_eq!(outcome.cues[0].timing_frames, 0x20);
    }

    #[test]
    fn strike_carries_enemy_effect() {
        let mut info = synthetic_info(Some(PowerByte::from_byte(0x1A)));
        info.enemy_effect = EnemyEffect::Burned;
        let outcome = apply_art_strike(64, 10, &info);
        assert_eq!(outcome.enemy_effect, EnemyEffect::Burned);
    }

    #[test]
    fn strike_floors_at_one_when_def_overwhelms() {
        // attack=10 def=200 mult=12 → ~(120-200) saturates → floor 1.
        let info = synthetic_info(Some(PowerByte::from_byte(0x16))); // UDF × 12
        let outcome = apply_art_strike(10, 200, &info);
        assert_eq!(outcome.damage, Some(1));
    }

    #[test]
    fn strike_with_divisor_8_doubles_damage() {
        // Same input as `strike_udf_28_dishes_max_tier_damage` but div=8 instead
        // of 16: (64*28)/8 - 10 = 224 - 10 = 214.
        let info = synthetic_info(Some(PowerByte::from_byte(0x1A)));
        let outcome = apply_art_strike_with_divisor(64, 10, 8, 1, &info);
        assert_eq!(outcome.damage, Some(214));
    }

    #[test]
    fn strike_with_higher_floor_clamps_low_damage() {
        // Default floor=1 returns 1; floor=10 returns 10.
        let info = synthetic_info(Some(PowerByte::from_byte(0x16))); // UDF × 12
        let outcome = apply_art_strike_with_divisor(10, 200, 16, 10, &info);
        assert_eq!(outcome.damage, Some(10));
    }

    #[test]
    fn apply_art_strike_default_calls_div_16_floor_1() {
        // Default and explicit div=16 floor=1 must produce identical outcomes.
        let info = synthetic_info(Some(PowerByte::from_byte(0x1A)));
        let a = apply_art_strike(80, 30, &info);
        let b = apply_art_strike_with_divisor(80, 30, 16, 1, &info);
        assert_eq!(a, b);
    }
}
