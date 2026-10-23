//! Windowed (winit + wgpu) drivers: the `play-window` / `record` engine
//! viewer (`PlayWindowApp`) and the `play-str` MDEC movie player
//! (`StrPlayerApp`), plus their geometry/asset helpers.

use crate::replay::{PadEvent, ReplayFile, ReplayMeta};
use anyhow::{Context, Result};
use glam::{Mat4, Vec3, Vec4};
use legaia_engine_audio::AudioSink;
use legaia_engine_menu_runtime::menu_runtime::MenuRuntime;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_render::{
    CaptureImage, ColorSceneDraw, RenderTarget, Scene as RenderScene, SceneDraw, TextOverlay,
    UploadedColorMesh, UploadedFontAtlas, UploadedVram, UploadedVramMesh,
    window::{EngineWindow, orbit_camera_mvp},
};
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};
use legaia_engine_session::native::NativeSession;
use legaia_engine_ui::{TextDraw, text_draws_for};
use legaia_font::Font;
use std::path::Path;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

#[path = "window/assets.rs"]
mod assets;
#[path = "window/battle.rs"]
mod battle;
#[path = "window/battle_cam.rs"]
mod battle_cam;
#[path = "window/boot_cutscene.rs"]
mod boot_cutscene;
#[path = "window/camera.rs"]
mod camera;

/// Opt-in developer menu (`LEGAIA_DEV_MENU=1`) - the host screen for the
/// ported retail dev-menu kernels.
#[path = "window/dev_menu.rs"]
mod dev_menu;
#[path = "window/event_handler.rs"]
mod event_handler;
#[path = "window/field_render.rs"]
mod field_render;
#[cfg(test)]
#[path = "window/fog_texture_tests.rs"]
mod fog_texture_tests;
#[path = "window/gamepad.rs"]
mod gamepad;
#[path = "window/geometry.rs"]
mod geometry;
#[path = "window/headless.rs"]
mod headless;
#[path = "window/hud.rs"]
mod hud;
#[path = "window/menu_draws.rs"]
mod menu_draws;
#[path = "window/minigames.rs"]
mod minigames;
#[path = "window/record.rs"]
mod record;
#[path = "window/run.rs"]
mod run;
#[path = "window/save_rack_disk.rs"]
mod save_rack_disk;
#[path = "window/screens.rs"]
mod screens;
#[path = "window/session_steps.rs"]
mod session_steps;
#[path = "window/str_player.rs"]
mod str_player;
#[path = "window/title_save_draws.rs"]
mod title_save_draws;

pub use record::cmd_record;
use record::{RecordLog, RecordTarget};
// Re-export the extracted geometry / driver / save-select / str-player
// items so the sibling window submodules (which `use super::*`) still see
// them at the same effective scope they had before the split.
pub(crate) use geometry::{
    LineGeometry, effect_billboard_mesh, effect_sprite_line_geometry, heightfield_to_vram_mesh,
    scene_viewport_for, world_map_slot4_line_geometry,
};
// The procedural battle ground grid lives in `legaia-scene-formats` so all three
// hosts share one implementation (the native window, the asset-viewer and
// the browser play page) rather than forking the kernel per host.
pub use headless::HeadlessPlayWindow;
pub(crate) use legaia_scene_formats::battle_backdrop::build_ground_grid_rgbc as build_battle_ground_grid;
pub use run::cmd_play_window;
use run::{PlayWindowLaunch, assemble_play_window};
// These two stay window-tree-private (their signatures reference the
// `pub(super)` record types); re-exported only so the sibling submodules
// that `use super::*` still resolve them unqualified.
pub(in crate::window) use run::{build_window_scene_resources, cmd_play_window_with_record};
pub(crate) use save_rack_disk::{
    MountedCard, card_port_blocks, card_port_rack, open_port_one_card, rack_has_save,
};
pub use str_player::cmd_play_str;
pub(crate) use str_player::{play_str_in, resolve_iso_file};

#[path = "window/app_state.rs"]
mod app_state;
#[path = "window/launch_options.rs"]
mod launch_options;

pub(crate) use app_state::*;
pub use launch_options::*;

impl PlayWindowApp {
    /// Maximum number of battle-event log lines kept in the HUD ring.
    const BATTLE_EVENT_LOG_CAP: usize = 6;
}

// --- Field pause-menu window geometry (320x240 boot-UI stage pixels) ---
//
// The field-menu text builders lay glyphs out in stage pixels; the window
// rects come from the menu overlay's **window-descriptor table** (PROT
// 0899 @0x15F20, VA 0x801E4738 - `legaia_game_tables::menu_windows`), parsed
// from the user's disc at boot into `PlayWindowApp::menu_window_table`.
// Both the resolver and its pinned fallback live in
// `legaia_engine_ui::pause_menu` (`MenuRects` / `MENU_WINDOW_FALLBACK`),
// shared with the browser play page: this window used to carry a
// byte-identical private copy of the table, which is a paired constant with
// no gate over it.

/// Options config round-trip file (sibling of `legaia-input.toml`). Holds
/// the retail settings plus the engine-only knobs (BGM / SFX volume,
/// message speed) that the pause-menu options screen doesn't show.
const OPTIONS_CONFIG_FILE: &str = "legaia-options.toml";

/// Keyboard binding round-trip file - the same one
/// `legaia-engine config set --binding` edits and `window/run.rs` loads at
/// startup. The pause menu's Key Config screen writes here too, so the
/// in-game editor and the CLI editor share one store rather than each
/// keeping its own.
pub(super) const INPUT_CONFIG_FILE: &str = "legaia-input.toml";

impl PlayWindowApp {
    /// Advance `World::clock.play_time_seconds` off the window's wall clock,
    /// through the engine's one play-clock law (`World::tick_play_clock`) -
    /// the browser page's `tick_play_clock` calls the same kernel. The origin
    /// and high-water mark live on the world, so New Game restarts them.
    pub(super) fn tick_play_clock(&mut self) {
        let now = f64::from(self.win.elapsed_secs());
        self.session.host.world.tick_play_clock(now);
    }

    /// Apply the live side-effects of [`Self::options_state`] (currently
    /// the Sound Stereo/Monaural downmix on the audio output) and persist
    /// the state to [`OPTIONS_CONFIG_FILE`]. Called when an options
    /// session closes.
    pub(super) fn persist_and_apply_options(&self) {
        self.apply_options_side_effects();
        if self.scripted_run() {
            return;
        }
        let path = std::path::PathBuf::from(OPTIONS_CONFIG_FILE);
        let to_save = self.options_pins.persistable(&self.options_state);
        if let Err(e) = to_save.save(&path) {
            log::warn!("options: save to {} failed: {e:#}", path.display());
        }
    }

    /// A `--screenshot` / `--screenshot-every` / `--key-script` run: a capture,
    /// not a play session, so nothing it toggles is written to the player's
    /// config files. A scripted `F8` used to leave `legaia-options.toml` at
    /// night, and every later launch - scripted or not - started there. The
    /// browser play page's equivalent, a fresh headless context, has no
    /// stored state to leak into.
    pub(super) fn scripted_run(&self) -> bool {
        self.screenshot.is_some() || self.persist_disabled
    }

    /// Push the current options into their live consumers without touching
    /// disk (also called once at startup after the config file loads).
    pub(super) fn apply_options_side_effects(&self) {
        if let Some(audio) = self.session.audio.as_ref() {
            audio.set_mono(matches!(
                self.options_state.audio,
                legaia_engine_system::option_values::AudioMode::Mono
            ));
            // Master mute (engine-only knob; `F2` toggles it live). The
            // mixer gate zeroes the output while the sequencer / SPU keep
            // ticking, so unmuting resumes mid-track in sync.
            audio.set_muted(self.options_state.muted);
            // Engine-only BGM / SFX bus levels (config file only); the
            // default level is unity, the retail mix.
            audio.set_bus_volumes(self.options_state.bgm_volume, self.options_state.sfx_volume);
            // Engine-only full-mono downmix (`downmix_mono`, config file and
            // launcher row), separate from the retail Monaural row above.
            audio.set_downmix(self.options_state.downmix_mono);
        }
        // The two render enhancements that are pure renderer state: lamp
        // shadows (`Y`) and the camera-occlusion fade (`F4`). Persisted
        // options, the same ones the page's boxes edit; the renderer opening
        // later stages them itself (`resumed`).
        if let Some(r) = self.win.renderer.as_ref() {
            r.set_dyn_shadows(self.options_state.light_shadows);
            r.set_occlusion_fade(self.options_state.occlusion_fade);
            r.set_bloom(self.options_state.bloom);
        }
    }
}

#[cfg(test)]
mod battle_camera_tests {
    use super::PlayWindowApp;
    use glam::Vec4;
    use std::f32::consts::TAU;

    /// `retail_battle_mvp` must reproduce the exact retail overworld-battle
    /// projection `screen = H*(Rx(32u)*Ry(yaw)*v + (0,1280,7680))/Ze`, H=256,
    /// PSX +Y down, screen-centre `(OFX, OFY)` over 320x240 - pinned from the
    /// `overworld_battle_bg_angle_*` saves + `FUN_80026988`. Disc-free: pure
    /// math. Guards the glam matrix construction against regression.
    ///
    /// The centre is the GTE control file's, not the geometric one: `OFY` is
    /// `114`. Writing `120` on *both* sides of this comparison is what let the
    /// six-pixel error live - the reference has to name the retail constant,
    /// so that a matrix built on the naive centre fails here.
    #[test]
    fn retail_battle_mvp_matches_psx_projection() {
        // Hand-rolled retail projection (the savestate-verified reference).
        fn handrolled(v: [f32; 3], yaw_u: f32) -> Option<(f32, f32)> {
            let yaw = yaw_u / 4096.0 * TAU;
            let pitch = 32.0 / 4096.0 * TAU;
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            let ry = [cy * v[0] + sy * v[2], v[1], -sy * v[0] + cy * v[2]];
            let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
            let ez = e[2] + 7680.0;
            if ez <= 1.0 {
                return None;
            }
            Some((
                256.0 * e[0] / ez + legaia_engine_camera::projection::GTE_OFX,
                256.0 * (e[1] + 1280.0) / ez + legaia_engine_camera::projection::GTE_OFY,
            ))
        }
        // Sample several world points and orbit angles (4:3 so aspect_fix == 1).
        let mvp_aspect = 4.0 / 3.0;
        for &yaw_u in &[0.0f32, 224.0, 1024.0, 2632.0, 3136.0, 3808.0] {
            let mvp = PlayWindowApp::retail_battle_mvp(yaw_u / 4096.0 * TAU, mvp_aspect);
            for &v in &[
                [1000.0f32, -500.0, 3000.0],
                [-2000.0, 0.0, 6000.0],
                [0.0, -3000.0, -800.0],
                [5000.0, 12.0, 5000.0],
            ] {
                // Engine draws `cam * model` with `model` carrying the Y-flip F,
                // so the dome sample is `cam * F * v_psx` == flip v.y first.
                let clip = mvp * Vec4::new(v[0], -v[1], v[2], 1.0);
                if clip.w <= 1.0 {
                    continue;
                }
                let ndc = (clip.x / clip.w, clip.y / clip.w);
                let sx = 160.0 + ndc.0 * 160.0;
                let sy = 120.0 - ndc.1 * 120.0; // NDC up+ -> PSX screen down+
                if let Some((hx, hy)) = handrolled(v, yaw_u) {
                    let d = ((sx - hx).powi(2) + (sy - hy).powi(2)).sqrt();
                    assert!(
                        d < 0.05,
                        "yaw={yaw_u} v={v:?}: {d}px off ({sx},{sy} vs {hx},{hy})"
                    );
                }
            }
        }
    }
}
