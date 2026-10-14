//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;
use legaia_render_kernels::battle_intro::BattleIntro;
use legaia_render_kernels::screen_prim::ScreenPrim;

// The battle-HUD row fold and the encounter-banner label are shared with the
// browser play page: both live in `legaia_engine_core::battle_hud` and each
// host projects the resulting `BattleHud` into its own renderer's view types.
// Re-exported here so the window modules (and their wiring tests) keep their
// `battle::` paths.
pub(super) use legaia_engine_core::battle_hud::{encounter_banner_label, sync_battle_hud_rows};

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
        let events = self.session.host.world.drain_battle_events();
        // The tick's battle audio - the duck target, the strike / cast cues
        // at their strike-relative timing, the round's cast-voice prestage,
        // the one-shot CD-XA clips and the arts shouts - is one session step
        // on both hosts (`BootSession::route_battle_audio`). The tick's audio
        // frame (`tick_sfx`, the next step of the frame path) ramps the duck
        // and fires whatever matured.
        let _ = self.session.route_battle_audio(&events);
        for ev in events {
            // Surface in the HUD ring.
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(ev.summary());
        }

        // Floating damage / heal numbers: the live battle loop resolves and
        // applies HP itself, then queues a presentation-only FX per strike.
        // Feed those into the popup model and log the magnitude (the typed
        // events above are consumed inside the live loop, so this is the
        // only place per-strike damage surfaces while live).
        let fx = self.session.host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud.push_popup(
                    legaia_engine_battle::battle_hud_state::DamagePopup::damage(
                        f.target_slot,
                        f.amount,
                    )
                    .crit(),
                );
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            let sign = if f.is_heal { '+' } else { '-' };
            self.battle_event_log
                .push_back(format!("slot {} {}{} HP", f.target_slot, sign, f.amount));
        }

        // Per-hit events of the attack band (`World::tick_battle_hit_events`,
        // one per damage-kernel resolution): the channel the impact-FX and
        // HIT / TOTAL counter layers read. Drained here so the world never
        // accumulates it; each surfaces as a diag line (the ring only draws
        // under `LEGAIA_DIAG_HUD` / `F1`, so this is off by default).
        let hits = self.session.host.world.drain_battle_hit_events();
        for h in hits {
            if self.battle_event_log.len() >= Self::BATTLE_EVENT_LOG_CAP {
                self.battle_event_log.pop_front();
            }
            self.battle_event_log.push_back(format!(
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

        // Battle effect-script spawn requests: one per effect record the
        // per-actor effect-script walk consumed this tick (the FUN_801DEA50
        // port driven from the animation tick). Route each into the world's
        // matching spawn path - the render layers this window already
        // composes then draw them: the direct (0x80-flagged) form lands in
        // the 2D effect pool (effect billboards / outline markers), the
        // table form stages a 0x801F6324 prototype scene whose parts ride
        // the move-FX part-draw seam.
        // The routing is the engine's (`World::route_battle_effect_spawns`),
        // shared with the browser play page.
        for r in self.session.host.world.route_battle_effect_spawns() {
            log::debug!(
                "effect-script spawn effect {:#04x} actor {} at {:?} direct={} staged={:?}",
                r.spawn.effect,
                r.spawn.actor_slot,
                r.spawn.at,
                r.spawn.direct,
                r.table_staged
            );
        }

        // Refresh per-slot rows + status icons, then age the popups one frame.
        if self.session.host.world.mode == SceneMode::Battle {
            self.sync_battle_hud_rows();
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &self.session.host.world.battle.status_effects);
            }
        } else if self.session.host.world.mode == SceneMode::MuscleDome {
            // A dome leg is a battle on the same HUD: the shared fold seats
            // the lead fighter's row off the dome session.
            self.sync_battle_hud_rows();
        }
        self.battle_hud.tick();

        // Age the encounter-transition banner one frame; drop it at zero.
        if let Some((frames, _)) = &mut self.encounter_banner {
            *frames = frames.saturating_sub(1);
            if *frames == 0 {
                self.encounter_banner = None;
            }
        }
    }

    /// Fold the live battle-actor table into [`Self::battle_hud`]'s per-slot
    /// rows. Thin wrapper over [`sync_battle_hud_rows`], which is a free
    /// function so it can be exercised against a bare `World`.
    pub(super) fn sync_battle_hud_rows(&mut self) {
        sync_battle_hud_rows(&mut self.battle_hud, &self.session.host.world);
    }

    /// React to a `Field <-> Battle` scene-mode change once per transition:
    /// on entering battle, decode each enemy's mesh and inject it; on leaving,
    /// restore the clean field VRAM and drop the battle meshes. Called each
    /// frame before the render borrows `uploaded_vram`.
    pub(super) fn sync_battle_render(&mut self) {
        let mode = self.session.host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev == Some(mode) {
            return;
        }
        match (prev, mode) {
            (_, SceneMode::Battle) => {
                self.arm_encounter_banner();
                self.enter_battle_render();
            }
            (Some(SceneMode::Battle), _) => {
                self.encounter_banner = None;
                // Retire this fight's floating numerals and reset the status
                // CLUT's pristine-palette copy + Stone latch at the battle
                // boundary, where retail rebuilds its context. Both are
                // host-side state that outlives the engine object; without
                // them a numeral could survive the mode edge and a petrified
                // member in the next fight restaged the previous fight's
                // palette. The browser play page already did both.
                self.battle_hud.clear_popups();
                self.battle_hud.status_clut.reset();
                self.exit_battle_render();
            }
            _ => {}
        }
    }

    /// Frames the encounter-transition banner stays on screen after a
    /// `Field -> Battle` mode change (~1.5 s at the 60 Hz sim tick).
    const ENCOUNTER_BANNER_FRAMES: u16 = 90;

    /// Arm the HUD's "ENCOUNTER!" banner for the battle just entered.
    fn arm_encounter_banner(&mut self) {
        let label = encounter_banner_label(&self.session.host.world);
        self.encounter_banner = Some((Self::ENCOUNTER_BANNER_FRAMES, label));
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
    use super::encounter_banner_label;
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
