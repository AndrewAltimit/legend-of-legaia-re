//! Event queue emitted by the battle action state machine through the
//! world's `BattleActionHost` implementation.
//!
//! Mirrors [`crate::field_events`] but for battle. Visual side-effects
//! (pose changes, UI element spawns, spell animations, screen shake,
//! brightness ramps) and gameplay primitives (damage application,
//! capture-archive load, party / monster setup) are pushed onto a
//! [`BattleEvent`] queue on the world; engines drain after
//! [`crate::world::World::tick`] each frame.

use legaia_engine_vm::battle_action::{BattleEndCause, Pose};

/// One side-effect the battle action state machine requested this frame.
/// Variants mirror the `BattleActionHost` callbacks one-to-one — see
/// [`legaia_engine_vm::battle_action::BattleActionHost`] for the per-state
/// citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BattleEvent {
    /// `BattleActionHost::pose` — actor pose change.
    Pose { actor_id: u8, pose: Pose },
    /// `BattleActionHost::ui_element` — battle UI element scheduler.
    /// `mode == 0` spawns / resets, `mode == 1` terminates.
    UiElement { effect_id: u8, mode: u8 },
    /// `BattleActionHost::camera_bounds` — recompute camera framing.
    CameraBounds,
    /// `BattleActionHost::party_setup` — per-party-slot init hook
    /// (`FUN_801EED1C` in retail).
    PartySetup { actor_slot: u8 },
    /// `BattleActionHost::monster_setup` — per-monster-slot init hook
    /// (`FUN_801E7320` in retail).
    MonsterSetup { actor_slot: u8 },
    /// `BattleActionHost::recompute_battle_order` — rebuild the
    /// initiative ordering.
    RecomputeBattleOrder,
    /// `BattleActionHost::load_capture_archive` — load monster-capture
    /// archive (`func_0x8003EAE4(0, idx)` in retail).
    LoadCaptureArchive { idx: u8 },
    /// `BattleActionHost::spell_anim_trigger` — start a one-shot spell
    /// animation.
    SpellAnimTrigger { party_slot: u8, spell_id: u8 },
    /// `BattleActionHost::spell_anim_sustain` — sustained anim during
    /// spell cast / hold.
    SpellAnimSustain { actor_id: u8, anim_id: u8 },
    /// `BattleActionHost::apply_damage` — damage / heal application
    /// primitive. The state machine surfaces this; engines compute and
    /// apply the actual HP/MP delta.
    ApplyDamage {
        icon: u8,
        page: u8,
        target_slot: u8,
        party_slot: u8,
    },
    /// `BattleActionHost::screen_shake` — kick the camera.
    ScreenShake { magnitude: u16 },
    /// `BattleActionHost::ramp_brightness` — ramp brightness toward a
    /// target percentage (used by SummonSustain / MagicCaptureFade).
    RampBrightness { target_pct: u8 },
    /// `BattleActionHost::battle_end` — battle is ending; engines unload
    /// the battle overlay.
    BattleEnd { cause: BattleEndCause },
}

impl BattleEvent {
    /// One-line description for logging / asset-viewer overlays.
    pub fn summary(&self) -> String {
        match self {
            BattleEvent::Pose { actor_id, pose } => format!("Pose(actor={actor_id}, {pose:?})"),
            BattleEvent::UiElement { effect_id, mode } => {
                format!("UiElement(effect={effect_id}, mode={mode})")
            }
            BattleEvent::CameraBounds => "CameraBounds".into(),
            BattleEvent::PartySetup { actor_slot } => format!("PartySetup({actor_slot})"),
            BattleEvent::MonsterSetup { actor_slot } => format!("MonsterSetup({actor_slot})"),
            BattleEvent::RecomputeBattleOrder => "RecomputeBattleOrder".into(),
            BattleEvent::LoadCaptureArchive { idx } => format!("LoadCaptureArchive({idx})"),
            BattleEvent::SpellAnimTrigger {
                party_slot,
                spell_id,
            } => {
                format!("SpellAnimTrigger(party={party_slot}, spell={spell_id})")
            }
            BattleEvent::SpellAnimSustain { actor_id, anim_id } => {
                format!("SpellAnimSustain(actor={actor_id}, anim={anim_id})")
            }
            BattleEvent::ApplyDamage {
                icon,
                page,
                target_slot,
                party_slot,
            } => {
                format!(
                    "ApplyDamage(icon={icon}, page={page}, target={target_slot}, party={party_slot})"
                )
            }
            BattleEvent::ScreenShake { magnitude } => format!("ScreenShake({magnitude})"),
            BattleEvent::RampBrightness { target_pct } => format!("RampBrightness({target_pct}%)"),
            BattleEvent::BattleEnd { cause } => format!("BattleEnd({cause:?})"),
        }
    }
}

/// Damage formula primitive. The retail engine has separate physical /
/// magical / item paths; this is a clean-room minimum-viable formula
/// that engines can replace. Mirrors the JRPG-staple
/// `dmg = base_attack * 2 - target_def`, clamped to `>=1` so attacks
/// never deal zero (battle progress would otherwise stall).
///
/// Inputs are `BattleActor::param` slot reads — engines pass the right
/// stats. Returns the damage to apply via `BattleActor::hp -= dmg`.
pub fn basic_damage(attacker_atk: i32, target_def: i32, variance_rng: u32) -> u16 {
    let raw = (attacker_atk * 2).saturating_sub(target_def);
    // ±12.5% variance — same magnitude as the retail variance roll.
    let var_pct = (variance_rng % 25) as i32 - 12;
    let scaled = raw + (raw * var_pct / 100);
    let clamped = scaled.max(1);
    clamped.min(u16::MAX as i32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_damage_never_below_one() {
        // High def, low atk → would be negative without clamp.
        assert!(basic_damage(1, 999, 0) >= 1);
    }

    #[test]
    fn basic_damage_scales_with_atk() {
        let lo = basic_damage(10, 5, 0);
        let hi = basic_damage(100, 5, 0);
        assert!(hi > lo, "higher atk should produce higher damage");
    }

    #[test]
    fn basic_damage_variance_stays_within_band() {
        // Roll the RNG across all 25 buckets; result should stay within
        // ±13 of the noiseless value (raw = 2*atk - def = 195).
        let center = basic_damage(100, 5, 12) as i32; // variance roll = 0
        for r in 0..25u32 {
            let d = basic_damage(100, 5, r) as i32;
            let drift = (d - center).abs();
            assert!(
                drift <= 30,
                "rng={r} produced d={d}, drift={drift} (center={center})"
            );
        }
    }

    #[test]
    fn summary_is_non_empty_for_each_variant() {
        let events = [
            BattleEvent::Pose {
                actor_id: 0,
                pose: Pose::Idle,
            },
            BattleEvent::UiElement {
                effect_id: 0,
                mode: 0,
            },
            BattleEvent::CameraBounds,
            BattleEvent::PartySetup { actor_slot: 0 },
            BattleEvent::MonsterSetup { actor_slot: 0 },
            BattleEvent::RecomputeBattleOrder,
            BattleEvent::LoadCaptureArchive { idx: 0 },
            BattleEvent::SpellAnimTrigger {
                party_slot: 0,
                spell_id: 0,
            },
            BattleEvent::SpellAnimSustain {
                actor_id: 0,
                anim_id: 0,
            },
            BattleEvent::ApplyDamage {
                icon: 0,
                page: 0,
                target_slot: 0,
                party_slot: 0,
            },
            BattleEvent::ScreenShake { magnitude: 0 },
            BattleEvent::RampBrightness { target_pct: 0 },
            BattleEvent::BattleEnd {
                cause: BattleEndCause::PartyWipe,
            },
        ];
        for e in events {
            assert!(!e.summary().is_empty());
        }
    }
}
