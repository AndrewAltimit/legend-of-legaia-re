//! The overworld's entity and player markers, and its sky band, on the play
//! page.
//!
//! A port marker, not a retail draw: retail binds each world-map placement to
//! its own actor model, which is still open, so both hosts can draw a
//! kind-coded post + cross at each placement (a debug overlay,
//! `WorldToggles::overworld_marker_overlay`, off by default) and a
//! facing-ticked post for the player when the leader's mesh is missing.
//! The geometry, colours, sizing, projection and the prim wrap all come out
//! of `legaia_engine_world_views::screen_passes`, the pass the native window
//! draws too; this file only answers the one question the page holds alone
//! (is the leader's mesh resident) and hands the prims to the page's
//! screen-prim pass.

use legaia_render_kernels::screen_prim::ScreenPrim;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// This frame's marker primitives. Empty outside the world map.
    pub(crate) fn world_map_marker_prims(&mut self) -> Vec<ScreenPrim> {
        let aabb = self.scene_aabb();
        // The party leader's real mesh draws whenever it resolved; the
        // marker is the stand-in for when it did not (the native window's
        // gate is the same question about its own upload).
        let draw_player = self.player.is_none();
        self.host_view_at(aabb)
            .map(|v| v.world_map_marker_prims(draw_player))
            .unwrap_or_default()
    }

    /// This frame's overworld sky band (`FUN_801F73E4`), the shared pass the
    /// native window draws too. Empty outside the world map.
    pub(crate) fn world_map_sky_prims(&mut self) -> Vec<ScreenPrim> {
        let aabb = self.scene_aabb();
        self.host_view_at(aabb)
            .map(|v| v.world_map_sky_prims())
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// How many marker quads the kernel emitted this frame - a page-side
    /// probe for the oracle; the draw itself rides the screen-prim pass.
    pub fn play_world_map_marker_count(&mut self) -> u32 {
        self.world_map_marker_prims().len() as u32
    }

    /// Debug seat: put the player on raw world `(x, z)` with the floor
    /// sampled under it and re-arm the zone camera's arrival snap - the
    /// frame-pairing aid for comparing the page against a retail save state
    /// at that state's own player position. The native window's twin is
    /// `play-window`'s `LEGAIA_SEAT`.
    pub fn play_debug_seat(&mut self, x: i16, z: i16) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let seated = host.debug_seat_standing(x, z);
        if seated {
            self.scene_host.camera_mut().zone.arm_arrival();
        }
        seated
    }
}
