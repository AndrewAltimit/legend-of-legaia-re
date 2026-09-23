//! Field screen-space effects on the play page: the fog sheets.
//!
//! Retail draws the field fog pool from the field render pass
//! (`FUN_8003F348` at `0x80026F24`, gated on game mode `3` and
//! `_DAT_8007B854`), two textured semi-transparent quads per live particle
//! through `FUN_8003F86C`. The simulation of that pass is
//! `legaia_engine_core::fog_particles` and its `World::fog_render_step`
//! seam; this file is the page's draw-path call into it, the twin of the
//! native window's `take_field_fog_prims`. Both wrap the quads through
//! `legaia_engine_ui::screen_prim::fog_puff_prim`, and both resolve the
//! camera with `resolve_field_camera(.., None, ..)` - the follow pose - so a
//! scripted camera beat projects the fog the same way on either host.
//!
//! The prims ride the page's existing screen-prim pass (`ScreenPrimPass` in
//! `site/js/play-app.js`), which samples the renderer's uploaded field VRAM -
//! where the effect atlas page `0x27` the fog cells live on is resident from
//! field entry (`upload_effect_textures_into_vram`).

use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::screen_prim::{ScreenPrim, fog_puff_prim};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// Run the fog pool's render step for this tick and return its sheets as
    /// screen primitives. Empty outside a field scene or while the script
    /// gate is clear.
    pub(crate) fn tick_field_fog_prims(&mut self) -> Vec<ScreenPrim> {
        let Some(host) = self.scene_host.as_mut() else {
            return Vec::new();
        };
        let world = &mut host.world;
        if world.mode != SceneMode::Field || !world.fog.gate {
            return Vec::new();
        }
        let frame = resolve_field_camera(world, &self.camera, None, [0.0, 0.0]);
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) = frame else {
            return Vec::new();
        };
        world
            .fog_render_step(&view)
            .iter()
            .map(|q| fog_puff_prim(q.xy, q.uv, q.clut, q.tpage, q.rgb, q.ot_index))
            .collect()
    }
}

impl LegaiaRuntime {
    /// This tick's move-VM strip spans (the extension sub-op `0x2C` callee
    /// `FUN_801D31B0`) as screen primitives - the native window's
    /// `take_move_strip_prims` twin, through the same shared kernel and the
    /// same follow camera as the fog sheets. The captured requests drain on
    /// every call; only a field scene draws them.
    pub(crate) fn tick_move_strip_prims(&mut self) -> Vec<ScreenPrim> {
        let Some(host) = self.scene_host.as_mut() else {
            return Vec::new();
        };
        let world = &mut host.world;
        let requests = world.move_vm.take_strip_requests();
        if requests.is_empty() || world.mode != SceneMode::Field {
            return Vec::new();
        }
        let frame = resolve_field_camera(world, &self.camera, None, [0.0, 0.0]);
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) = frame else {
            return Vec::new();
        };
        legaia_engine_ui::move_strip::move_strip_prims(&requests, &view)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Sheets the fog pool emitted on the last tick (two per drawn
    /// particle), and the pool's live count after it - `[quads, live]`.
    /// A page-side probe for the oracle; the draw itself rides the
    /// screen-prim pass.
    pub fn play_fog_stats(&self) -> Vec<u32> {
        let Some(host) = self.scene_host.as_ref() else {
            return vec![0, 0];
        };
        vec![
            host.world.fog_quads().len() as u32,
            u32::from(host.world.fog.live),
        ]
    }
}
