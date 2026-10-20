//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;
use legaia_render_kernels::battle_intro::BattleIntro;
use legaia_render_kernels::screen_prim::ScreenPrim;

// The battle-HUD row fold is shared with the browser play page: it lives in
// `legaia_engine_battle_hud`, and each host projects the resulting
// `BattleHud` into its own renderer's view types. Re-exported here so the
// HUD wiring tests keep their `battle::` path.
#[cfg(test)]
pub(super) use legaia_engine_battle_hud::sync_battle_hud_rows;

#[path = "battle/intro.rs"]
mod intro;
#[path = "battle/overlay_draws.rs"]
mod overlay_draws;
#[path = "battle/presentation_ticks.rs"]
mod presentation_ticks;
#[path = "battle/scene_entry.rs"]
mod scene_entry;
#[path = "battle/summon_spawn.rs"]
mod summon_spawn;

pub(super) use scene_entry::*;

impl PlayWindowApp {
    /// Drain world battle events and append a one-line summary to the HUD
    /// ring. Called once per simulation tick from the redraw handler.
    ///
    /// **Observation only - do not fold here.** `World::live_battle_tick`
    /// owns the gameplay fold and re-publishes the stream afterwards, so a
    /// second `fold_battle_event` would apply an art strike's HP twice.
    pub(super) fn drain_and_log_battle_events(&mut self) {
        // The presentation step both hosts run
        // (`BootSession::step_battle_presentation`): the tick's battle audio
        // (duck target, strike / cast cues, cast-voice prestage, CD-XA clips,
        // arts shouts), the strike FX into the popup model, the per-hit
        // events, the effect-script spawn routing, the HUD row and status
        // fold, the popup and banner ages. The tick's audio frame
        // (`tick_sfx`, the next step of the frame path) ramps the duck and
        // fires whatever matured. What stays here is this window's log ring.
        let step = self
            .session
            .step_battle_presentation(&mut self.battle_hud, &mut self.encounter_banner);
        for ev in &step.events {
            self.push_battle_log(ev.summary());
        }
        for f in &step.hit_fx {
            let sign = if f.is_heal { '+' } else { '-' };
            self.push_battle_log(format!("slot {} {}{} HP", f.target_slot, sign, f.amount));
        }
        // The per-hit events surface as diag lines (the ring only draws
        // under `LEGAIA_DIAG_HUD` / `F1`, so this is off by default).
        for h in &step.hit_events {
            self.push_battle_log(format!(
                "hit {} {}->{} pb {:#04x} dmg {} total {}{}{}",
                h.hit_index,
                h.attacker_slot,
                h.target_slot,
                h.power_byte,
                h.damage,
                h.running_total,
                if h.applied { " APPLIED" } else { "" },
                if h.is_art { " art" } else { "" },
            ));
        }
        for r in &step.routed_effect_spawns {
            log::debug!(
                "effect-script spawn effect {:#04x} actor {} at {:?} direct={} staged={:?}",
                r.spawn.effect,
                r.spawn.actor_slot,
                r.spawn.at,
                r.spawn.direct,
                r.table_staged
            );
        }
        // The battle VRAM re-stamps read the status CLUT latch the HUD fold
        // just armed, so they run after it in the same tick, as on the page.
        if self.session.host.world.mode == SceneMode::Battle {
            self.tick_battle_vram_restamps();
            self.tick_battle_stage_shell();
        }
    }

    /// Append one line to the HUD's battle event ring, dropping the oldest
    /// past [`Self::BATTLE_EVENT_LOG_CAP`].
    fn push_battle_log(&mut self, line: String) {
        if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
            self.battle_event_log.pop_front();
        }
        self.battle_event_log.push_back(line);
    }

    /// React to a `Field <-> Battle` scene-mode change once per transition:
    /// on entering battle, decode each enemy's mesh and inject it; on leaving,
    /// restore the clean field VRAM and drop the battle meshes. Called each
    /// frame before the render borrows `uploaded_vram`.
    pub(super) fn sync_battle_render(&mut self) {
        use legaia_engine_session::battle_presentation::{
            BattleModeEdge, battle_mode_edge, on_battle_entered, on_battle_left,
        };
        let mode = self.session.host.world.mode;
        match battle_mode_edge(&mut self.prev_scene_mode, mode) {
            Some(BattleModeEdge::Entered) => {
                on_battle_entered(&self.session.host.world, &mut self.encounter_banner);
                // The world half first, renderer or not: the stage class,
                // the fog's stage cap, each monster's texture slot and idle
                // (the page runs the same step on its own edge).
                let scus = self.scus_bytes();
                let archive = self.monster_archive_bytes();
                let stage = legaia_engine_session::battle_stage::stage_battle_world(
                    &mut self.session.host,
                    scus.as_deref(),
                    archive.as_deref().map(Vec::as_slice),
                );
                self.enter_battle_render(stage);
            }
            Some(BattleModeEdge::Left) => {
                // The banner, this fight's numerals and the status CLUT's
                // palette copies and latches, shared with the page.
                on_battle_left(&mut self.battle_hud, &mut self.encounter_banner);
                self.exit_battle_render();
            }
            None => {}
        }
    }

    /// Bridge the decoded monster meshes for the current battle into the draw
    /// list: inject each enemy's texture pool into a clone of the field VRAM
    /// at the loader's per-slot coords, upload the relocated mesh, and bind it
    /// to the enemy actor. Re-uploads the edited VRAM so the injected texture
    /// pages resolve.
    /// Build the current scene's battle-stage backdrop, if it has one. Returns
    /// the battle VRAM (scene + stage-dome textures resident) and the stage
    /// dome's `(Tmd, raw)`. The faithful battle backdrop is the scene's
    /// `scene_tmd_stream` half-dome (sky + mountain ring + ground); building the
    /// scene in `SceneLoadKind::Battle` makes that dome TMD + its textures
    /// resident (the Field build excludes them). `None` when the scene has no
    /// stage entry.
    /// Read `SCUS_942.54` from whichever boot source this window opened, if it
    /// is reachable. Used to resolve the backdrop's second-copy transform;
    /// `None` just means the port falls back to the default half turn.
    fn scus_bytes(&self) -> Option<Vec<u8>> {
        use legaia_engine_scene::vfs::Vfs;
        if let Some(root) = self.extracted_root.as_deref() {
            legaia_engine_scene::vfs::DirVfs::new(root)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else if let Some(disc) = self.disc_path.as_deref() {
            legaia_engine_scene::vfs::DiscVfs::open(disc)
                .ok()
                .and_then(|v| v.read("SCUS_942.54").ok())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod encounter_banner_tests {
    use legaia_engine_battle_hud::encounter_banner_label;
    use legaia_engine_core::world::World;

    #[test]
    fn label_names_unresolved_monsters_positionally() {
        let mut world = World::new();
        world.enter_battle(1, 2);
        // `enter_battle` seats actors but leaves monster HP unseeded; seed it
        // so the label counts the slots as live formation members. With no
        // catalog resolution the label falls back to positional M<n> names.
        for a in world.actors.iter_mut().skip(1).take(2) {
            a.battle.max_hp = 10;
            a.battle.hp = 10;
        }
        assert_eq!(encounter_banner_label(&world), "M1  M2");
    }

    #[test]
    fn unseeded_formation_yields_empty_label() {
        let mut world = World::new();
        world.enter_battle(1, 2);
        assert_eq!(encounter_banner_label(&world), "");
    }
}
