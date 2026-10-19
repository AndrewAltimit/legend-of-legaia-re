//! Browser **live battle** host: encounter arming + battle overlay draws.
//!
//! The simulation half is entirely [`legaia_engine_core`]: with
//! `World::toggles.live_gameplay_loop` armed, `World::tick` rolls step-driven random
//! encounters off the scene MAN's own encounter table, flips
//! `Field -> Battle`, runs the battle-action state machine (player-driven
//! command menus included - `battle_player_driven`), and returns to the field
//! with loot on victory. Nothing here re-implements a rule; this module is
//! the browser twin of the native window's battle *presentation*:
//!
//! * [`LegaiaRuntime::arm_live_battles`] mirrors
//!   `BootSession::enter_field_live`'s live-loop arming (MAN encounter table
//!   first, synthetic vanilla registry as the fallback the native boot uses).
//! * [`LegaiaRuntime::tick_battle_presentation`] mirrors the native
//!   `sync_battle_render` mode-edge latch + `drain_and_log_battle_events`:
//!   arm the ENCOUNTER! banner on the `Field -> Battle` edge, fold battle
//!   events, feed strike FX into the damage-popup model, refresh the
//!   per-slot rows through the **shared**
//!   [`legaia_engine_battle_hud::sync_battle_hud_rows`] fold.
//! * [`LegaiaRuntime::battle_overlay_draws`] mirrors the native window's
//!   battle HUD block (`window/hud.rs`): the shared
//!   [`legaia_engine_ui::battle_hud_draws_for`] rows (carrying the retail
//!   HP / MP colour law, FUN_800349EC / FUN_80035EA8), the centred
//!   [`legaia_engine_ui::encounter_banner_draws_for`] transition banner, and
//!   the player-driven command / arts / magic / item submenus.
//!
//! Draws are emitted in **surface pixels** (not the 320x240 menu stage): the
//! native window draws its battle HUD in surface space, and the HUD's
//! measured column offsets (name / HP / MP / AP / K.O.) span wider than the
//! 320-px stage, so stage-scaling would push the status strip off screen.
//!
//! The battle's 3D layer under this overlay is no longer missing: the stage
//! dome, ground grid, monster meshes and assembled party battle forms build in
//! [`crate::play_battle_render`], and the effect layer on top of them - the
//! effect-script spawn drain, the effect-pool billboards, the `etmd` / move-VM
//! FX models, the summon creature and the target-select cursor tint - in
//! [`crate::play_battle_fx`].

use crate::runtime::LegaiaRuntime;
use legaia_engine_battle::battle_hud_state::DamagePopup;
use legaia_engine_battle_hud::{encounter_banner_label, sync_battle_hud_rows};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};
use legaia_render_kernels::battle_intro::BattleIntro;
use wasm_bindgen::prelude::*;

mod overlay_draws;
mod screen_prims;
mod session_exports;
#[cfg(not(target_arch = "wasm32"))]
mod test_probes;
mod tutorial;
mod value_readout;

/// Frames the encounter-transition banner stays on screen after a
/// `Field -> Battle` mode change (~1.5 s at the 60 Hz sim tick; the native
/// window's `ENCOUNTER_BANNER_FRAMES`).
const ENCOUNTER_BANNER_FRAMES: u16 = 90;
/// The retail 4x battle world scale (`play_battle_render::BATTLE_WORLD_SCALE`,
/// private to that module) - the actor stage the battle view-projection is
/// built for, so an actor origin has to be scaled before it is projected.
const BATTLE_WORLD_SCALE_LOCAL: f32 = 4.0;

impl LegaiaRuntime {
    /// Arm the live gameplay loop on the freshly-entered scene, the browser
    /// twin of `BootSession::enter_field_live`'s live-loop block:
    /// `enter_field_scene` already installed the scene MAN's own encounter
    /// table (with its formations' real archive stats), so the synthetic
    /// vanilla registry is only the fallback for scenes whose MAN carries no
    /// rollable encounter (towns resolve to a 0% trigger rate there - no
    /// invented town fights). Battles are player-driven, as the native
    /// `--player-battle` flag makes them.
    pub(crate) fn arm_live_battles(&mut self, scene: &str) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // One shared kernel with the native host (`BootSession::enter_field_live`
        // calls the same `World::arm_live_loop`). This used to be a
        // hand-maintained copy of that block and had already lost the scene
        // label and the Battle<->Field BGM swap - which is why battle music
        // was silent in the browser while it played natively.
        let mut opts = legaia_engine_core::live_loop::LiveLoopOpts::playable();
        // `None` keeps the shipped default battle theme; an explicit page-side
        // override of `0` disables the swap, any other id replaces the track.
        if let Some(id) = self.battle_bgm {
            opts.battle_bgm = (id != 0).then_some(id);
        }
        host.world.arm_live_loop(scene, &opts);
    }

    /// Per-tick battle presentation, called from [`LegaiaRuntime::tick_frame`]:
    /// the browser twin of the native `sync_battle_render` mode-edge latch +
    /// `drain_and_log_battle_events`. Cheap no-op while no scene is up.
    pub(crate) fn tick_battle_presentation(&mut self) {
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // Mode-edge latch: arm the ENCOUNTER! banner + build the battle 3D
        // render entering battle; drop both (and any stale popups) leaving -
        // the browser twin of the native `sync_battle_render` edge.
        let mode = host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev != Some(mode) {
            match (prev, mode) {
                (_, SceneMode::Battle) => {
                    self.encounter_banner =
                        Some((ENCOUNTER_BANNER_FRAMES, encounter_banner_label(&host.world)));
                    // The world half first: the stage class, the fog's stage
                    // cap, each monster's texture slot and idle - the step
                    // the native window runs on its own edge.
                    let archive = host
                        .index
                        .entry_bytes_extended(
                            legaia_engine_session::battle_stage::MONSTER_ARCHIVE_PROT_INDEX,
                        )
                        .ok();
                    let stage = legaia_engine_session::battle_stage::stage_battle_world(
                        host,
                        self.scus.as_deref(),
                        archive.as_deref(),
                    );
                    // 3D layer: battle VRAM + backdrop / grid / monster /
                    // party meshes ([`crate::play_battle_render`]).
                    self.enter_battle_render(stage, archive);
                }
                (Some(SceneMode::Battle), _) => {
                    self.encounter_banner = None;
                    self.battle_hud.clear_popups();
                    self.exit_battle_render();
                }
                _ => {}
            }
        }
        // The battle camera is stepped inside `World::tick` for every host
        // (`World::tick_battle_camera`), independent of this page's render
        // build.
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // Floating damage / heal numbers: the live loop resolves HP itself
        // and queues a presentation-only FX per strike.
        let fx = host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud
                    .push_popup(DamagePopup::damage(f.target_slot, f.amount).crit());
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
        }
        // Per-hit events of the attack band (`World::tick_battle_hit_events`):
        // the channel the impact-FX and HIT / TOTAL counter layers read.
        // Drained so the world never accumulates it; nothing on this host
        // consumes it yet (the popups above already carry each hit's number).
        let _hits = host.world.drain_battle_hit_events();
        // Battle effect-script spawn requests (one per effect record the
        // per-actor effect-script walk consumed this tick). Routed into the
        // world's own spawn paths so the FX render layers
        // ([`crate::play_battle_fx`]) have something to draw - the browser
        // used to leave this queue undrained, which is why every cast, art
        // impact and enemy special was visually a no-op here.
        self.drain_battle_effect_spawns_web();
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // Refresh per-slot rows + status icons through the shared fold, then
        // age the popups one frame.
        if host.world.mode == SceneMode::Battle {
            sync_battle_hud_rows(&mut self.battle_hud, &host.world);
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &host.world.battle.status_effects);
            }
        } else if host.world.mode == SceneMode::MuscleDome {
            // A dome leg is a battle on the same HUD: the shared fold seats
            // the lead fighter's row off the dome session, as the native
            // window's does.
            sync_battle_hud_rows(&mut self.battle_hud, &host.world);
        }
        self.battle_hud.tick();
        // Age the encounter-transition banner one frame; drop it at zero.
        if let Some((frames, _)) = &mut self.encounter_banner {
            *frames = frames.saturating_sub(1);
            if *frames == 0 {
                self.encounter_banner = None;
            }
        }
        // Audio side of the battle tick: typed battle events (audio duck),
        // strike / cast SFX cues, arts-voice shouts, XA clip requests. Lives
        // in [`crate::play_battle_audio`]; every queue is drained there so
        // the world never accumulates one.
        self.drain_battle_audio_cues();
        // Mid-battle VRAM re-stamps (facial animation, status-effect actor
        // recolour, effect CLUT stage) - [`crate::play_battle_vram`].
        self.tick_battle_vram_channel();
    }
}

impl LegaiaRuntime {
    /// Raise the return-to-title hand-off on the `World::game_over` edge:
    /// the session's probe (`BootSession::take_party_wipe`, the native
    /// window's `poll_game_over` too), which consumes the flag and hands back
    /// the hold this page then steps from `game_over_input`. Nothing is
    /// scanned off the card rack: the hand-off has one destination whether or
    /// not a save exists, because retail's wipe arm has one exit store.
    pub(crate) fn poll_game_over(&mut self) {
        let frame_owned = self.game_over.is_some();
        if let Some(hold) = self
            .scene_host
            .session_mut()
            .and_then(|s| s.take_party_wipe(frame_owned))
        {
            self.game_over = Some(hold);
        }
    }
}

/// Disc-gated runtime oracle for the drawn battle-HUD surface + the enemy
/// target strip, driven through a REAL scripted battle off the scene MAN.
/// Skips + passes without `LEGAIA_DISC_BIN`. Set `LEGAIA_HUD_DUMP=<path>` to
/// also write the raw overlay JSON for external inspection.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod live_hud_tests;

#[cfg(test)]
mod tests {
    use legaia_engine_battle::battle_tutorial::BoxStyle;

    /// The tutorial box rect is in 320x240 stage space, so the page's own
    /// stage transform must carry it - the same law the native window applies
    /// via `save_select_stage`. Pinned at the browser play canvas's real
    /// shape (4:3, not an integer multiple of the stage) because that is
    /// exactly where reading the rect as surface pixels went wrong.
    #[test]
    fn tutorial_rect_rides_the_page_stage_transform() {
        let (x, y, w, h) = BoxStyle::from_raw(0).unwrap().box_rect(177, 2);
        assert_eq!((x, y, w, h), (0x10, 0x0E, 177, 24));

        let ((ox, oy), scale) = crate::play_menu::stage_transform(862, 646);
        assert_eq!((ox, oy, scale), (111, 83, 2));
        // Placed on the stage the box is inset by the letterbox plus the
        // scaled margin; read as surface pixels it would sit at (16, 14) -
        // hard against the corner.
        assert_eq!(ox + x as i32 * scale as i32, 143);
        assert_eq!(oy + y as i32 * scale as i32, 111);

        // And it stays inside the stage at every supported scale.
        for (sw, sh) in [(320u32, 240u32), (960, 720), (1920, 1080)] {
            let ((ox, oy), scale) = crate::play_menu::stage_transform(sw, sh);
            let right = ox + (x as i32 + w as i32) * scale as i32;
            let bottom = oy + (y as i32 + h as i32) * scale as i32;
            assert!(right <= sw as i32, "{sw}x{sh}: right {right}");
            assert!(bottom <= sh as i32, "{sw}x{sh}: bottom {bottom}");
        }
    }
}
