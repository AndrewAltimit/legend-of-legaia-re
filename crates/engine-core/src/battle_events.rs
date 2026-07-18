//! Event queue emitted by the battle action state machine through the
//! world's `BattleActionHost` implementation.
//!
//! Mirrors [`crate::field_events`] but for battle. Visual side-effects
//! (pose changes, UI element spawns, spell animations, screen shake,
//! brightness ramps) and gameplay primitives (damage application,
//! capture-archive load, party / monster setup) are pushed onto a
//! [`BattleEvent`] queue on the world; engines drain after
//! [`crate::world::World::tick`] each frame.

use crate::art_strike::ArtStrikeOutcome;
use legaia_engine_vm::battle_action::{BattleEndCause, Pose};

/// One side-effect the battle action state machine requested this frame.
/// Variants mirror the `BattleActionHost` callbacks one-to-one - see
/// [`legaia_engine_vm::battle_action::BattleActionHost`] for the per-state
/// citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BattleEvent {
    /// `BattleActionHost::pose` - actor pose change.
    Pose { actor_id: u8, pose: Pose },
    /// `BattleActionHost::ui_element` - battle UI element scheduler.
    /// `mode == 0` spawns / resets, `mode == 1` terminates.
    UiElement { effect_id: u8, mode: u8 },
    /// `BattleActionHost::camera_bounds` - recompute camera framing.
    CameraBounds,
    /// `BattleActionHost::party_setup` - per-party-slot init hook
    /// (`FUN_801EED1C` in retail).
    PartySetup { actor_slot: u8 },
    /// `BattleActionHost::monster_setup` - per-monster-slot init hook
    /// (`FUN_801E7320` in retail).
    MonsterSetup { actor_slot: u8 },
    /// `BattleActionHost::recompute_battle_order` - rebuild the
    /// initiative ordering.
    RecomputeBattleOrder,
    /// `BattleActionHost::load_capture_archive` - load monster-capture
    /// archive (`func_0x8003EAE4(0, idx)` in retail).
    LoadCaptureArchive { idx: u8 },
    /// `BattleActionHost::spell_anim_trigger` - start a one-shot spell
    /// animation.
    SpellAnimTrigger { party_slot: u8, spell_id: u8 },
    /// `BattleActionHost::spell_anim_sustain` - sustained anim during
    /// spell cast / hold.
    SpellAnimSustain { actor_id: u8, anim_id: u8 },
    /// `BattleActionHost::apply_damage` - damage / heal application
    /// primitive. The state machine surfaces this; engines compute and
    /// apply the actual HP/MP delta.
    ApplyDamage {
        icon: u8,
        page: u8,
        target_slot: u8,
        party_slot: u8,
    },
    /// `BattleActionHost::apply_art_strike` - Tactical-Art strike outcome.
    /// The state machine resolved the per-strike values from the active
    /// art record; the world's host impl converted them into the concrete
    /// HP delta + status flag + SFX schedule via
    /// [`crate::art_strike::apply_art_strike`]. Engines fold the outcome
    /// into HP / status / SFX through whatever runtime path they have.
    ApplyArtStrike {
        actor_slot: u8,
        target_slot: u8,
        strike_index: u8,
        outcome: ArtStrikeOutcome,
    },
    /// `BattleActionHost::screen_shake` - kick the camera.
    ScreenShake { magnitude: u16 },
    /// `BattleActionHost::ramp_brightness` - ramp brightness toward a
    /// target percentage (used by SummonSustain / MagicCaptureFade).
    RampBrightness { target_pct: u8 },
    /// `BattleActionHost::battle_end` - battle is ending; engines unload
    /// the battle overlay.
    BattleEnd { cause: BattleEndCause },
    /// `World::notify_art_used` - a character's Tactical Art use count
    /// crossed the learn threshold for the first time. Engines display a
    /// HUD banner; the art is already marked learned in
    /// `World::tactical_arts` when this fires.
    TacticalArtLearned { char_id: u8, art_id: u8 },
    /// `World::apply_battle_xp` - a character's XP crossed a level threshold.
    /// HP/MP maxima have already been bumped in the roster record and the live
    /// `BattleActor` mirror when this fires.
    LevelUp {
        char_id: u8,
        new_level: u8,
        hp_gained: u16,
        mp_gained: u16,
    },
}

/// One resolved per-strike HP delta, surfaced purely for HUD presentation
/// (floating damage / heal numbers).
///
/// This is a *cosmetic* signal: the gameplay-state HP mutation has already
/// been applied by the battle loop before an entry is queued, so engines
/// feed these straight into a damage-popup model and never re-subtract HP.
/// Distinct from [`BattleEvent`], which carries state-machine side effects
/// the engine still has to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleHitFx {
    /// Actor table index the number floats over (0..=2 party, 3.. monsters).
    pub target_slot: u8,
    /// Magnitude of the HP delta (always positive; `is_heal` gives sign).
    pub amount: u16,
    /// `true` when the delta restored HP (rendered green / `+N`).
    pub is_heal: bool,
    /// `true` for a critical / all-stars hit (rendered with emphasis).
    pub is_crit: bool,
}

/// One battle sound-effect cue surfaced for the host to play through its SFX
/// bank. Mirrors [`BattleHitFx`]'s "drain and present" pattern, but for audio:
/// it carries no gameplay-state side effect - the host just fires the sound.
///
/// `kind` is the [`crate::art_strike::ScheduledCue`] kind = the art-record
/// `HitCue::kind` (e.g. `0x1A` generic hit). It is the **SfxBank cue id
/// directly**, NOT a `FUN_8004fcc8` dispatch id, so the host plays it through
/// `SfxBank::play_one_shot` without `classify_cue`. (The move-power `+0x0d` cue
/// is the separate `FUN_8004fcc8` namespace, surfaced via
/// [`crate::world::World::take_pending_move_fx_cue`].) `timing_frames` is the
/// delay after strike-start, for the host's frame-timed scheduler.
/// One Tactical-Arts **shout** cue - the per-character CD-XA voice clip a
/// party art fires as it begins (distinct from the VAB one-shot
/// [`BattleSfxCue`]s the art's strikes schedule). Emitted once per executed
/// art, on the art's animation-start frame, only when the art carries a real
/// action constant (synthetic/demo arts stay silent, matching the retail
/// degradation for arts with no cue-table entry).
///
/// The host resolves it against the arts-voice cue tables + the demuxed
/// XA2/XA4/XA6 clip banks (`legaia_engine_audio::ArtsShoutBank`) and plays it
/// through the XA mixing path with the modeled CD-response start delay, so
/// the shout **trails** the animation start rather than leading it.
/// REF: FUN_8004C140 (cue select), FUN_8003D53C (CD-XA clip play).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleShoutCue {
    /// 0-based character slot (0 Vahn / 1 Noa / 2 Gala) - the XA clip-file
    /// key (`XA2`/`XA4`/`XA6`).
    pub cslot: u8,
    /// The art's action constant - the cue-table key that selects the
    /// candidate-channel pool.
    pub action: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleSfxCue {
    /// SfxBank cue id (the art `HitCue::kind`).
    pub kind: u16,
    /// Frames after strike-start when the cue should fire.
    pub timing_frames: u16,
    /// Attacker actor slot (HUD / 3D-pan context).
    pub actor_slot: u8,
    /// Target actor slot the strike landed on.
    pub target_slot: u8,
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
            BattleEvent::ApplyArtStrike {
                actor_slot,
                target_slot,
                strike_index,
                outcome,
            } => {
                let dmg = outcome
                    .damage
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "-".into());
                format!(
                    "ApplyArtStrike(actor={actor_slot}, target={target_slot}, strike={strike_index}, dmg={dmg})"
                )
            }
            BattleEvent::ScreenShake { magnitude } => format!("ScreenShake({magnitude})"),
            BattleEvent::RampBrightness { target_pct } => format!("RampBrightness({target_pct}%)"),
            BattleEvent::BattleEnd { cause } => format!("BattleEnd({cause:?})"),
            BattleEvent::TacticalArtLearned { char_id, art_id } => {
                format!("TacticalArtLearned(char={char_id}, art={art_id})")
            }
            BattleEvent::LevelUp {
                char_id,
                new_level,
                hp_gained,
                mp_gained,
            } => {
                format!("LevelUp(char={char_id}, lv={new_level}, +{hp_gained}hp, +{mp_gained}mp)")
            }
        }
    }
}

/// Damage formula primitive. The retail engine has separate physical /
/// magical / item paths; this is a clean-room minimum-viable formula
/// that engines can replace. Mirrors the JRPG-staple
/// `dmg = base_attack * 2 - target_def`, clamped to `>=1` so attacks
/// never deal zero (battle progress would otherwise stall).
///
/// Inputs are `BattleActor::param` slot reads - engines pass the right
/// stats. Returns the damage to apply via `BattleActor::hp -= dmg`.
pub fn basic_damage(attacker_atk: i32, target_def: i32, variance_rng: u32) -> u16 {
    let raw = (attacker_atk * 2).saturating_sub(target_def);
    // ±12.5% variance - same magnitude as the retail variance roll.
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
            BattleEvent::TacticalArtLearned {
                char_id: 0,
                art_id: 1,
            },
            BattleEvent::LevelUp {
                char_id: 0,
                new_level: 2,
                hp_gained: 10,
                mp_gained: 5,
            },
            BattleEvent::ApplyArtStrike {
                actor_slot: 0,
                target_slot: 3,
                strike_index: 1,
                outcome: ArtStrikeOutcome {
                    damage: Some(102),
                    enemy_effect: legaia_art::record::EnemyEffect::Toxic,
                    cues: Vec::new(),
                    alt_range: false,
                    power_target: Some(legaia_art::power::PowerTarget::Udf),
                },
            },
        ];
        for e in events {
            assert!(!e.summary().is_empty());
        }
    }
}
