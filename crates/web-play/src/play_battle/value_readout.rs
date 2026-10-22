//! The battle value readout (damage / heal numerals): layout, atlas probe,
//! screen prims and text draws.

use super::*;

impl LegaiaRuntime {
    /// This frame's value readout, laid out in 320x240 **stage** pixels: the
    /// combo counter cluster and the digit cells of every struck actor's run
    /// (`legaia_engine_screens::battle_value_readout`, the layout the native
    /// window runs too).
    ///
    /// Shared by the retail-art path (`battle_value_readout_prims`) and the
    /// font fallback below so the two cannot seat the same number
    /// differently.
    ///
    /// The projection uses the **stage** aspect (`4:3`), not the canvas
    /// aspect: the cells are authored in the 320x240 stage the host then
    /// letterboxes, which is the aspect `battle_fx_screen_prims` already
    /// projects its trail and streak packets with. The camera is the
    /// engine's (`World::battle_cam_pose`, stepped by `World::tick`), so the
    /// layout needs no render build: a fight whose render did not build still
    /// draws its numbers through the font fallback.
    fn battle_value_readout_layout(
        &self,
    ) -> Option<legaia_engine_screens::battle_value_readout::ValueReadoutLayout> {
        let world = &self.scene_host.host()?.world;
        let vp = legaia_engine_camera::projection::battle_vp(
            &self.battle_cam_pose(),
            BATTLE_WORLD_SCALE_LOCAL,
            4.0 / 3.0,
        );
        legaia_engine_screens::battle_value_readout::battle_value_readout_layout(
            world,
            &self.battle_hud,
            &vp,
            BATTLE_WORLD_SCALE_LOCAL,
        )
    }

    /// Is the battle effect atlas's glyph page resident in the VRAM texture
    /// this page samples? The browser twin of the native window's
    /// `battle_vram.is_some()` gate: the page uploads the atlas into its
    /// battle VRAM copy on the battle-render generation edge, so the render
    /// being live in battle mode *is* the residency.
    fn battle_value_readout_has_atlas(&self) -> bool {
        self.battle_render.is_some()
            && self
                .scene_host
                .host()
                .is_some_and(|h| h.world.mode == legaia_engine_mode::mode::SceneMode::Battle)
    }

    /// The frame's value readout as screen-space PSX primitives - retail's
    /// own 24x24 numeral cells off the resident battle effect atlas.
    ///
    /// The quads come out of `legaia_engine_ui::battle_numerals`, the same
    /// builder the native window emits through, so both hosts sample the same
    /// texels off the same page through the same ordering-table pass. Empty
    /// outside battle and before the battle VRAM exists, which is what leaves
    /// the font fallback in charge for those frames.
    pub(crate) fn battle_value_readout_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        if !self.battle_value_readout_has_atlas() {
            return Vec::new();
        }
        self.battle_value_readout_layout()
            .map(|l| legaia_engine_screens::battle_value_readout::value_readout_prims(&l))
            .unwrap_or_default()
    }

    /// Font-fallback draw list for the value readout, in surface pixels.
    ///
    /// Empty whenever the retail cells are drawable
    /// (`battle_value_readout_prims`) - the two must never both draw, or
    /// every number renders twice. This covers the frames before the battle
    /// VRAM exists, the same bargain `ui::battle_value_readout_draws_for`
    /// documents.
    pub(crate) fn battle_value_readout_draws(
        &self,
        font: &legaia_font::Font,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        if surface_w == 0 || surface_h == 0 || self.battle_value_readout_has_atlas() {
            return Vec::new();
        }
        let Some(layout) = self.battle_value_readout_layout() else {
            return Vec::new();
        };
        // The stage transform the rest of the battle chrome uses.
        let (origin, scale) = crate::play_menu::stage_transform(surface_w, surface_h);
        legaia_engine_screens::battle_value_readout::value_readout_fallback_draws(
            font, &layout, origin, scale,
        )
    }
}
