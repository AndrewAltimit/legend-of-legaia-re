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
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};
use wasm_bindgen::prelude::*;

mod overlay_draws;
mod screen_prims;
mod session_exports;
#[cfg(not(target_arch = "wasm32"))]
mod test_probes;
mod tutorial;
mod value_readout;

/// The retail 4x battle world scale (`play_battle_render::BATTLE_WORLD_SCALE`,
/// private to that module) - the actor stage the battle view-projection is
/// built for, so an actor origin has to be scaled before it is projected.
const BATTLE_WORLD_SCALE_LOCAL: f32 = legaia_engine_camera::projection::BATTLE_WORLD_SCALE;

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
    /// the mode-edge latch and the presentation step both hosts run
    /// ([`legaia_engine_session::battle_presentation`]), then this page's
    /// battle VRAM re-stamps. Cheap no-op while no scene is up.
    pub(crate) fn tick_battle_presentation(&mut self) {
        use legaia_engine_session::battle_presentation::{
            BattleModeEdge, battle_mode_edge, on_battle_entered, on_battle_left,
        };
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        // Mode-edge latch: arm the ENCOUNTER! banner + build the battle 3D
        // render entering battle; drop both (and the popups and status CLUT
        // state) leaving - the same edge the native window latches.
        match battle_mode_edge(&mut self.prev_scene_mode, host.world.mode) {
            Some(BattleModeEdge::Entered) => {
                on_battle_entered(&host.world, &mut self.encounter_banner);
                // The world half first: the stage class, the fog's stage
                // cap, each monster's texture slot and idle - the step the
                // native window runs on its own edge.
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
            Some(BattleModeEdge::Left) => {
                on_battle_left(&mut self.battle_hud, &mut self.encounter_banner);
                self.exit_battle_render();
            }
            None => {}
        }
        // The battle camera is stepped inside `World::tick` for every host
        // (`World::tick_battle_camera`), independent of this page's render
        // build.
        let Some(session) = self.scene_host.session_mut() else {
            return;
        };
        // The step both hosts run: battle audio, strike popups, the per-hit
        // drain, the effect-script spawn routing (so the FX render layers in
        // [`crate::play_battle_fx`] have something to draw), the HUD row and
        // status fold, the popup and banner ages.
        let step =
            session.step_battle_presentation(&mut self.battle_hud, &mut self.encounter_banner);
        // Counted whether or not a director exists to hear them.
        self.sfx.queued += step.sfx_queued;
        for r in &step.routed_effect_spawns {
            crate::play_battle_render::web_log(&format!(
                "play battle FX: effect {:#04x} actor {} at {:?} direct={} staged={:?}",
                r.spawn.effect, r.spawn.actor_slot, r.spawn.at, r.spawn.direct, r.table_staged
            ));
        }
        // Mid-battle VRAM re-stamps (facial animation, status-effect actor
        // recolour, effect CLUT stage) - [`crate::play_battle_vram`]. After
        // the HUD fold, which arms the status CLUT latch they read.
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
