//! The battle value readout (damage / heal numerals): layout, atlas probe,
//! screen prims and text draws.

use super::*;

impl LegaiaRuntime {
    /// The live sparring-tutorial prompt's box rect in 320x240 **stage**
    /// pixels, or `None` when no box is up.
    ///
    /// The width is measured in this host's font (retail measures it with
    /// `FUN_80035F04`) and `engine-core` applies the emitter's placement +
    /// sizing arithmetic (`FUN_801F747C`). Shared by the text and chrome
    /// layers so the frame and the rows cannot disagree - the native window's
    /// `battle_tutorial_stage_rect` twin.
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
    ) -> Option<(
        Option<legaia_engine_battle_vm::battle_value_readout::ComboCluster>,
        Vec<legaia_engine_battle_vm::battle_value_readout::ValueCell>,
    )> {
        let world = &self.scene_host.host()?.world;
        let vp = legaia_engine_camera::projection::battle_vp(
            &self.battle_cam_pose(),
            BATTLE_WORLD_SCALE_LOCAL,
            4.0 / 3.0,
        );
        let (cluster, runs) =
            legaia_engine_screens::battle_value_readout::battle_value_readout_layout(
                world,
                &self.battle_hud,
                &vp,
                BATTLE_WORLD_SCALE_LOCAL,
            )?;
        Some((cluster, runs.into_iter().flatten().collect()))
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
        use legaia_engine_ui::battle_numerals as bn;
        if !self.battle_value_readout_has_atlas() {
            return Vec::new();
        }
        let Some((cluster, cells)) = self.battle_value_readout_layout() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(c) = cluster.as_ref() {
            out.extend(bn::combo_cluster_prims(c, bn::VALUE_READOUT_OT));
        }
        out.extend(bn::digit_run_prims(&cells, bn::VALUE_READOUT_OT));
        out
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
        let Some((cluster, cells)) = self.battle_value_readout_layout() else {
            return Vec::new();
        };
        // The stage transform the rest of the battle chrome uses.
        let (origin, scale) = crate::play_menu::stage_transform(surface_w, surface_h);
        let view =
            |k: &legaia_engine_battle_vm::battle_value_readout::ValueCell| ui::ValueCellView {
                digit: k.digit,
                x: k.x,
                y: k.y,
                w: k.w,
                h: k.h,
            };
        let mut out = Vec::new();
        if let Some(c) = cluster.as_ref() {
            let labels: Vec<ui::ComboLabelView<'_>> = c
                .labels
                .iter()
                .map(|l| ui::ComboLabelView {
                    word: l.word,
                    x: l.x,
                    y: l.y,
                })
                .collect();
            let cluster_cells: Vec<ui::ValueCellView> = c.cells.iter().map(view).collect();
            out.extend(ui::battle_combo_cluster_draws_for(
                font,
                &labels,
                &cluster_cells,
                origin,
                scale,
            ));
        }
        let popup_cells: Vec<ui::ValueCellView> = cells.iter().map(view).collect();
        out.extend(ui::battle_value_readout_draws_for(
            font,
            &popup_cells,
            ui::VALUE_READOUT_FALLBACK_COLOR,
            origin,
            scale,
        ));
        out
    }
}
