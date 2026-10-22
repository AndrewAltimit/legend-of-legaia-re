//! Engine core primitives: virtual filesystem, asset cache, frame time, and
//! the composite [`world::World`] that wires the per-VM hosts from
//! `legaia-engine-vm` into a single runtime.
//!
//! Engine-agnostic. No wgpu / windowing / audio dependencies - the asset
//! crates talk to this layer, the render and audio crates read from it.

pub mod actor_alloc_host;
pub mod battle_cam_inputs;
pub mod battle_party_form;
pub mod battle_round;
pub mod battle_sideband_textures;
pub mod fishing_exchange_input;
pub mod live_loop;
pub mod move_buffer_host;
pub mod new_game;
pub mod options;
pub mod scus_leaf_kernels;
pub mod world;

pub mod casino_coin_bank;
pub mod field_submode_code_lock;
pub mod field_submode_flag_window;

pub mod field_submode_screen;
pub mod incense_notice;

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
