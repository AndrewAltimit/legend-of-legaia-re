//! Field screen-space effects on the play page: the fog sheets, the actor
//! drop shadows and the op `0x34` sub-1 attached lights.
//!
//! Retail draws the field fog pool from the field render pass
//! (`FUN_8003F348` at `0x80026F24`, gated on game mode `3` and
//! `_DAT_8007B854`), two textured semi-transparent quads per live particle
//! through `FUN_8003F86C`. The simulation of that pass is
//! `legaia_engine_field::fog_particles` and its `World::fog_render_step`
//! seam. The step runs once per sim tick on both hosts, through the session's
//! `BootSession::step_field_fog` (here from `tick_frame`, natively from the
//! window's tick tail); this file wraps its quads for the page's screen-prim
//! pass, the twin of the native window's `take_field_fog_prims`. Both wrap the quads through
//! `legaia_render_kernels::screen_prim::fog_puff_prim`, and both resolve the
//! camera with `resolve_field_camera(.., None, centre)` - the follow pose, with
//! the scene-AABB centre as the fallback focus on both hosts - so a
//! scripted camera beat projects the fog the same way on either host.
//!
//! The prims ride the page's existing screen-prim pass (`ScreenPrimPass` in
//! `site/js/play-app.js`), which samples the renderer's uploaded field VRAM -
//! where the effect atlas page `0x27` the fog cells live on is resident from
//! field entry (`upload_effect_textures_into_vram`).

use legaia_engine_world_views::screen_passes;
use legaia_render_kernels::screen_prim::ScreenPrim;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// The follow camera's fallback focus for these screen-space passes: the
    /// loaded scene's AABB centre in X / Z, the value the page's own camera
    /// resolve ([`crate::play_camera`]) and every native field pass hand
    /// `resolve_field_camera`. These four passes used to pin `[0, 0]`, so on
    /// a frame whose camera fell back to the host centre (a cutscene beat
    /// staging no focus with no lead actor, the overworld walk arm) the fog,
    /// shadows, strips and lights projected through a camera aimed at the
    /// world origin while the scene itself drew through one aimed at the map.
    ///
    /// Read off the AABB cache rather than through `scene_aabb()`, because two
    /// of these passes borrow `&self`: the page's per-frame camera resolve
    /// fills the cache before any draw pass runs, and before the first
    /// resolve there is no scene camera to fall back from.
    pub(crate) fn field_camera_centre(&self) -> [f32; 2] {
        screen_passes::scene_centre(self.field_pass_box())
    }

    /// The scene box the field screen passes resolve their camera against:
    /// the AABB cache, or the empty box (centre `[0, 0]`) before the first
    /// camera resolve has filled it.
    pub(crate) fn field_pass_box(&self) -> ([f32; 3], [f32; 3]) {
        self.scene_aabb.unwrap_or_default()
    }

    /// The world, scene camera and `scene_box` this frame's projections read
    /// (`screen_passes::HostView`, the seam the native window reads through
    /// too). `None` before a scene is staged.
    pub(crate) fn host_view_at(
        &self,
        scene_box: ([f32; 3], [f32; 3]),
    ) -> Option<screen_passes::HostView<'_>> {
        let (host, camera) = self.scene_host.host_cam()?;
        Some(screen_passes::HostView::new(&host.world, camera, scene_box))
    }

    /// Run the fog pool's render step for this tick and return its sheets as
    /// screen primitives. Empty outside game mode 3 (a field scene or the
    /// kingdom overworld) or while the script gate is clear.
    pub(crate) fn tick_field_fog_prims(&mut self) -> Vec<ScreenPrim> {
        let centre = self.field_camera_centre();
        let Some(session) = self.scene_host.session_mut() else {
            return Vec::new();
        };
        // The session's per-tick step (`BootSession::step_field_fog`), the
        // one the native window runs at the same point of its tick: the
        // field follow pose, or the overworld walk pose in the field frame -
        // retail's overworld is a game-mode-3 scene and draws the same pool
        // through the same pass.
        let Some(fx) = session.step_field_fog(centre) else {
            return Vec::new();
        };
        // Recorded whether or not the gate let the step run: the drop
        // shadows project through the same field-FX view either way.
        self.fx_depth_affine = Some(fx.depth_affine());
        self.host_view_at(self.field_pass_box())
            .map(|v| v.fog_prims())
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `[a, b]` of `ndc = a + b / w` that this frame's depth-tested field
    /// screen primitives (fog sheets, drop shadows) were projected through,
    /// bit for bit (`FogView::depth_affine`); empty before any field-FX
    /// projection. The page's screen-prim pass inverts a corner's depth
    /// through it to recover the corner's `w`, and writes both its meshes and
    /// those corners on one log-of-`w` scale: the normalised depths sit within
    /// 1e-3 of 1, where a 24-bit buffer cannot hold the quarter-bucket margin
    /// the overworld's fog sheets keep behind the cells of their bucket.
    pub fn play_fx_depth_affine(&self) -> Vec<f32> {
        self.fx_depth_affine
            .map(|(a, b)| vec![a, b])
            .unwrap_or_default()
    }
}

impl LegaiaRuntime {
    /// This tick's actor drop shadows (retail's `FUN_8001C394` blob) as
    /// screen primitives: the shared pass both hosts draw
    /// (`legaia_engine_world_views::screen_passes::HostView::drop_shadow_prims`).
    /// Each cell carries its corners' scene depth, so the page's screen-prim
    /// pass tests it against the field it already drew. The blob texels live
    /// on the menu-glyph atlas page `(960, 256)`, resident in the field VRAM
    /// through the boot system-UI underlay.
    pub(crate) fn field_drop_shadow_prims(&self) -> Vec<ScreenPrim> {
        self.host_view_at(self.field_pass_box())
            .map(|v| v.drop_shadow_prims())
            .unwrap_or_default()
    }
}

impl LegaiaRuntime {
    /// This tick's move-VM strip spans (the extension sub-op `0x2C` callee
    /// `FUN_801D31B0`) as screen primitives - the shared pass both hosts draw
    /// (`legaia_engine_world_views::screen_passes::HostView::move_strip_prims`).
    pub(crate) fn tick_move_strip_prims(&mut self) -> Vec<ScreenPrim> {
        self.host_view_at(self.field_pass_box())
            .map(|v| v.move_strip_prims())
            .unwrap_or_default()
    }

    /// This frame's attached lights (the field VM's op `0x34` sub-1 light
    /// pools) as screen primitives - the shared pass both hosts draw
    /// (`legaia_engine_world_views::screen_passes::HostView::light_prims`).
    pub(crate) fn field_light_prims(&self) -> Vec<ScreenPrim> {
        self.host_view_at(self.field_pass_box())
            .map(|v| v.light_prims())
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The walk-ground heightfield's flat bucket-depth references, flattened
    /// eight floats per drawn vertex (`[x0, z0, x1, z1, y00, y10, y01,
    /// y11]` of its cell) - the shared
    /// [`legaia_engine_scene::field_ground::flat_refs`] over
    /// the same drawn positions `field_ground_positions` returns, so the
    /// page's ground draws each overworld cell at its ordering-table
    /// bucket's depth, and each sloped far-bucket field cell under every
    /// other draw, exactly as the native window's does. Empty with no
    /// ground.
    pub fn field_ground_flat_refs(&self) -> Vec<f32> {
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let Some(hf) = f.ground.as_ref() else {
            return Vec::new();
        };
        // Over the positions currently drawn: re-resolved through the live
        // ladder once `field_ground_live_positions` has moved them.
        let positions = match f.ground_lut_applied.as_ref() {
            Some(lut) => legaia_engine_scene::field_ground::live_render_positions(hf, lut),
            None => legaia_engine_scene::field_ground::render_positions(hf),
        };
        legaia_engine_scene::field_ground::flat_refs(hf, &positions)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Sheets the fog pool emitted on the last tick (two per drawn
    /// particle), and the pool's live count after it - `[quads, live]`.
    /// A page-side probe for the oracle; the draw itself rides the
    /// screen-prim pass.
    pub fn play_fog_stats(&self) -> Vec<u32> {
        let Some(host) = self.scene_host.host() else {
            return vec![0, 0];
        };
        vec![
            host.world.fog_quads().len() as u32,
            u32::from(host.world.fog.live),
        ]
    }
}
