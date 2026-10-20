//! The per-tick battle **presentation** step both play hosts run: the
//! `Field <-> Battle` mode-edge latch and what each edge does to the host's
//! battle HUD model, then the tick's drains (battle events and their audio,
//! the strike FX popups, the per-hit events, the effect-script spawns) and
//! the HUD row fold.
//!
//! The native window (`drain_and_log_battle_events` + `sync_battle_render`)
//! and the browser page (`tick_battle_presentation`) each carried a copy, and
//! the copies had drifted: the window reset the status CLUT state on the
//! battle exit edge and the page on its render build, and the window ran the
//! status CLUT pass a tick before the HUD fold that arms its latch. This is
//! the one copy; each host keeps its render build and teardown, and its own
//! logging of what the step returns.
//!
//! The simulation itself is not here: `World::tick` has already run the
//! battle loop (HP fold, status application, clip advance, effect-script
//! walk) when a host calls in. Everything below is presentation of that
//! state, plus the audio routing the session owns.

use legaia_engine_audio::AudioSink;
use legaia_engine_battle::battle_events::{BattleEvent, BattleHitEvent, BattleHitFx};
use legaia_engine_battle::battle_hud_state::{BattleHud, DamagePopup};
use legaia_engine_core::world::{RoutedEffectSpawn, World};
use legaia_engine_mode::mode::SceneMode;

use crate::session::BootSession;

/// Frames the encounter-transition banner stays on screen after a
/// `Field -> Battle` mode change (~1.5 s at the 60 Hz sim tick).
pub const ENCOUNTER_BANNER_FRAMES: u16 = 90;

/// The encounter banner a host draws: frames left and its label.
pub type EncounterBanner = Option<(u16, String)>;

/// A `Field <-> Battle` mode edge, latched once per transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleModeEdge {
    /// The world entered `SceneMode::Battle` this tick.
    Entered,
    /// The world left `SceneMode::Battle` this tick.
    Left,
}

/// Latch the scene mode against the previous call's and report a battle
/// edge. Every other mode change (and no change) is `None`. Idempotent
/// within a tick, so a host may call it from more than one place.
pub fn battle_mode_edge(prev: &mut Option<SceneMode>, mode: SceneMode) -> Option<BattleModeEdge> {
    let was = prev.replace(mode);
    if was == Some(mode) {
        return None;
    }
    match (was, mode) {
        (_, SceneMode::Battle) => Some(BattleModeEdge::Entered),
        (Some(SceneMode::Battle), _) => Some(BattleModeEdge::Left),
        _ => None,
    }
}

/// The battle-entry edge's presentation: arm the ENCOUNTER! banner for the
/// battle just entered. A host runs its world staging
/// (`battle_stage::stage_battle_world`) and render build after it.
pub fn on_battle_entered(world: &World, banner: &mut EncounterBanner) {
    *banner = Some((
        ENCOUNTER_BANNER_FRAMES,
        legaia_engine_battle_hud::encounter_banner_label(world),
    ));
}

/// The battle-exit edge's presentation: drop the banner, retire this
/// fight's floating numerals and reset the status CLUT's pristine-palette
/// copies and Stone latches.
///
/// Retail rebuilds the battle context - the `+0x220` latches and the
/// per-slot palette copies at `ctx[+0x894]` - per fight
/// (`legaia_engine_battle::battle_status_clut`), and the bands are
/// re-assigned per fight, so neither may survive into the next one. Both
/// are host-side state that outlives the engine object, which is why the
/// boundary is this step's.
pub fn on_battle_left(hud: &mut BattleHud, banner: &mut EncounterBanner) {
    *banner = None;
    hud.clear_popups();
    hud.status_clut.reset();
}

/// What one presentation step drained, for a host's own log.
#[derive(Debug, Default)]
pub struct BattlePresentationStep {
    /// The typed battle events of the tick (observation only: the live loop
    /// owns the gameplay fold).
    pub events: Vec<BattleEvent>,
    /// The strike FX the step turned into damage / heal popups.
    pub hit_fx: Vec<BattleHitFx>,
    /// The per-hit events of the attack band, drained so the world never
    /// accumulates them.
    pub hit_events: Vec<BattleHitEvent>,
    /// The effect-script spawns routed this tick.
    pub routed_effect_spawns: Vec<RoutedEffectSpawn>,
    /// How many battle SFX cues the tick held (`BootSession::route_battle_audio`).
    pub sfx_queued: u32,
}

impl<S: AudioSink> BootSession<S> {
    /// The per-tick battle presentation step, after `World::tick` and the
    /// effect step and before the tick's audio frame (`step_audio_frame`).
    /// Cheap outside battle: every drain is empty and the HUD fold is
    /// skipped.
    ///
    /// In order: the battle events and the audio they carry (the duck
    /// target, the strike / cast cues, the cast-voice prestage, the CD-XA
    /// clips and arts shouts); the strike FX into the popup model; the
    /// per-hit events; the effect-script spawns into the world's spawn paths
    /// (`World::route_battle_effect_spawns`); the HUD rows and status icons
    /// through the shared fold, which also arms the status CLUT latch; the
    /// popup age; the encounter banner's age.
    ///
    /// The status CLUT pass (`battle_vram::step_battle_vram_edits`) reads
    /// the latch, so a host runs it after this step in the same tick: the
    /// latch stands in for retail's applier write at `actor[+0x220]`, which
    /// the world tick has already made by the time the host steps.
    pub fn step_battle_presentation(
        &mut self,
        hud: &mut BattleHud,
        banner: &mut EncounterBanner,
    ) -> BattlePresentationStep {
        let events = self.host.world.drain_battle_events();
        let sfx_queued = self.route_battle_audio(&events);
        let world = &mut self.host.world;
        let hit_fx = world.drain_battle_hit_fx();
        for f in &hit_fx {
            if f.is_heal {
                hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                hud.push_popup(DamagePopup::damage(f.target_slot, f.amount).crit());
            } else {
                hud.push_damage(f.target_slot, f.amount);
            }
        }
        let hit_events = world.drain_battle_hit_events();
        let routed_effect_spawns = world.route_battle_effect_spawns();
        sync_battle_hud(hud, world);
        hud.tick();
        if let Some((frames, _)) = banner {
            *frames = frames.saturating_sub(1);
            if *frames == 0 {
                *banner = None;
            }
        }
        BattlePresentationStep {
            events,
            hit_fx,
            hit_events,
            routed_effect_spawns,
            sfx_queued,
        }
    }
}

/// Refresh the HUD's per-slot rows (and in battle, the status icons and the
/// status CLUT latch) from the live world. A Muscle Dome leg is a battle on
/// the same HUD: the shared fold seats the lead fighter's row off the dome
/// session, and it carries no status tracker.
pub fn sync_battle_hud(hud: &mut BattleHud, world: &World) {
    match world.mode {
        SceneMode::Battle => {
            legaia_engine_battle_hud::sync_battle_hud_rows(hud, world);
            for slot in 0..hud.slots.len() as u8 {
                hud.sync_status(slot, &world.battle.status_effects);
            }
        }
        SceneMode::MuscleDome => legaia_engine_battle_hud::sync_battle_hud_rows(hud, world),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edge_latches_once_per_battle_transition() {
        let mut prev = None;
        assert_eq!(battle_mode_edge(&mut prev, SceneMode::Field), None);
        assert_eq!(
            battle_mode_edge(&mut prev, SceneMode::Battle),
            Some(BattleModeEdge::Entered)
        );
        assert_eq!(battle_mode_edge(&mut prev, SceneMode::Battle), None);
        assert_eq!(
            battle_mode_edge(&mut prev, SceneMode::Field),
            Some(BattleModeEdge::Left)
        );
        assert_eq!(battle_mode_edge(&mut prev, SceneMode::Field), None);
    }

    #[test]
    fn leaving_a_battle_drops_the_banner_and_the_popups() {
        let mut hud = BattleHud::new();
        hud.push_damage(0, 12);
        let mut banner: EncounterBanner = Some((5, "ENCOUNTER!".into()));
        on_battle_left(&mut hud, &mut banner);
        assert!(banner.is_none());
        assert!(hud.popups.is_empty());
        assert!(!hud.status_clut.armed());
    }
}
