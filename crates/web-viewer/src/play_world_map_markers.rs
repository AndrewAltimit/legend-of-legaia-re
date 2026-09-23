//! The overworld's entity and player markers on the play page.
//!
//! A port marker, not a retail draw: retail binds each world-map placement to
//! its own actor model, which is still open, so both hosts draw a kind-coded
//! post + cross at each placement and a facing-ticked post for the player.
//! The geometry, colours, sizing and projection all come out of
//! `legaia_engine_core::world_map_markers`; this file only wraps its quads
//! through `legaia_engine_ui::screen_prim::world_map_marker_prim` onto the
//! page's existing screen-prim pass - the page has no line primitive, which
//! is why the kernel emits one-pixel quads rather than lines. The native
//! window's twin is `world_map_marker_prims`.
//!
//! The camera is the resolver's non-scripted frame (`resolve_field_camera`
//! with no cutscene view), the same call the native window makes, so the
//! draw step never advances the cutscene glide.

use legaia_engine_core::camera_view::resolve_field_camera;
use legaia_engine_core::world::SceneMode;
use legaia_engine_core::world_map_markers::marker_quads;
use legaia_engine_ui::screen_prim::{ScreenPrim, world_map_marker_prim};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// This frame's marker primitives. Empty outside the world map.
    pub(crate) fn world_map_marker_prims(&mut self) -> Vec<ScreenPrim> {
        let in_world_map = self
            .scene_host
            .as_ref()
            .is_some_and(|h| h.world.mode == SceneMode::WorldMap);
        if !in_world_map {
            return Vec::new();
        }
        let aabb = self.scene_aabb();
        // The party leader's real mesh draws whenever it resolved; the
        // marker is the stand-in for when it did not (the native window's
        // gate is the same question about its own upload).
        let draw_player = self.player.is_none();
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let world = &host.world;
        let centre = [(aabb.0[0] + aabb.1[0]) * 0.5, (aabb.0[2] + aabb.1[2]) * 0.5];
        let frame = resolve_field_camera(world, &self.camera, None, centre);
        marker_quads(world, &frame, aabb, draw_player)
            .iter()
            .map(|q| world_map_marker_prim(q.xy, q.rgba))
            .collect()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// How many marker quads the kernel emitted this frame - a page-side
    /// probe for the oracle; the draw itself rides the screen-prim pass.
    pub fn play_world_map_marker_count(&mut self) -> u32 {
        self.world_map_marker_prims().len() as u32
    }
}
