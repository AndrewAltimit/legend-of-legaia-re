//! Engine core primitives: virtual filesystem, asset cache, frame time, and
//! the composite [`world::World`] that wires the per-VM hosts from
//! `legaia-engine-vm` into a single runtime.
//!
//! Engine-agnostic. No wgpu / windowing / audio dependencies - the asset
//! crates talk to this layer, the render and audio crates read from it.

pub mod actor_alloc_host;
pub mod battle_cam_inputs;
pub mod battle_flow;
pub mod battle_hud;
pub mod battle_open;
pub mod battle_party_form;
pub mod battle_round;
pub mod battle_sideband_textures;
pub mod battle_tutorial;
pub mod camera;
pub mod camera_view;
pub mod card_write;
pub mod cheat_applier;
pub mod cheats;
pub mod cutscene_caption;
pub mod dance;
pub mod dance_cast_scene;
pub mod dance_venue;
pub mod dev_menu;
pub mod dev_menu_host;
pub mod drop_shadow;
pub mod field_channels;
pub mod field_ground;
pub mod field_menu_dispatch;
pub mod field_occlusion;
pub mod field_view_window;
pub mod fishing_actors;
pub mod fishing_exchange_input;
pub mod fishing_hub;
pub mod fishing_scene;
pub mod fishing_venue;
pub mod fog_particles;
pub mod fog_volume;
pub mod frame_step;
pub mod glb_export;
pub mod live_loop;
pub mod menu_runtime;
pub mod menu_validator;
pub mod minigame_status;
pub mod mode;
pub mod move_buffer_host;
pub mod muscle_dome;
pub mod muscle_ringside;
pub mod new_game;
pub mod npc_catalog;
pub mod options;
pub mod overworld_curvature;
pub mod pause_screens;
pub mod place_name_banner;
pub mod resume;
pub mod scene_host;
pub mod scene_live;
pub mod scene_staging;
pub mod scus_leaf_kernels;
pub mod shop;
pub mod tile_board;
pub mod timed_fight;
pub mod world;
pub mod world_map_markers;
pub mod world_map_sky;

pub mod casino_coin_bank;
pub mod field_submode_code_lock;
pub mod field_submode_flag_window;

pub mod field_submode_screen;
pub mod incense_notice;
pub mod world_map_panel_host;

use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// Wall-clock + delta accumulator. Used by the frame loop to drive
/// fixed-timestep gameplay updates while letting render run uncapped.
///
/// On `wasm32-unknown-unknown` `std::time::Instant` is not implemented, so
/// this type becomes a zero-size stub - callers (JS `requestAnimationFrame`
/// loop) supply their own delta timing.
pub struct FrameTime {
    #[cfg(not(target_arch = "wasm32"))]
    started_at: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_frame: Instant,
}

impl FrameTime {
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            Self {
                started_at: now,
                last_frame: now,
            }
        }
        #[cfg(target_arch = "wasm32")]
        Self {}
    }

    pub fn tick(&mut self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            let dt = now - self.last_frame;
            self.last_frame = now;
            dt
        }
        #[cfg(target_arch = "wasm32")]
        Duration::ZERO
    }

    pub fn elapsed(&self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Instant::now() - self.started_at
        }
        #[cfg(target_arch = "wasm32")]
        Duration::ZERO
    }
}

impl Default for FrameTime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_time_starts_at_zero() {
        let ft = FrameTime::new();
        assert!(ft.elapsed() < Duration::from_millis(50));
    }
}
