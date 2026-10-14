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
    /// combo counter cluster and one run of digit cells per live damage
    /// popup, seated over the struck actor.
    ///
    /// Shared by the retail-art path (`battle_value_readout_prims`) and the
    /// font fallback below so the two cannot seat the same number
    /// differently. Layout is `legaia_engine_battle_vm::battle_value_readout`, the
    /// same kernel the native window runs.
    ///
    /// Only the newest popup per actor contributes: retail's readout is a
    /// per-slot value window, and two runs centred on one point interleave
    /// unreadably.
    ///
    /// The projection uses the **stage** aspect (`4:3`), not the canvas
    /// aspect: the cells are authored in the 320x240 stage the host then
    /// letterboxes, which is the aspect `battle_fx_screen_prims` already
    /// projects its trail and streak packets with.
    fn battle_value_readout_layout(
        &self,
    ) -> Option<(
        Option<legaia_engine_battle_vm::battle_value_readout::ComboCluster>,
        Vec<legaia_engine_battle_vm::battle_value_readout::ValueCell>,
    )> {
        use legaia_engine_battle_vm::battle_value_readout as vr;
        if self.battle_hud.popups.is_empty() && self.battle_hud.combo.is_none() {
            return None;
        }
        let world = &self.scene_host.host()?.world;
        // The camera is the engine's (`World::battle_cam_pose`, stepped by
        // `World::tick`), so the layout needs no render build. It used to
        // read `play_battle_camera_vp`, which answers empty without a battle
        // render - the very frames the font fallback exists for - so a fight
        // whose render did not build drew no numbers at all.
        if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
            return None;
        }
        let vp = legaia_engine_camera::projection::battle_vp(
            &self.battle_cam_pose(),
            BATTLE_WORLD_SCALE_LOCAL,
            4.0 / 3.0,
        );
        let cluster = self
            .battle_hud
            .combo
            .as_ref()
            .map(|c| vr::combo_cluster(c.style, c.hits, c.total, c.slide()));
        let mut newest: Vec<&legaia_engine_battle::battle_hud_state::DamagePopup> = Vec::new();
        for p in &self.battle_hud.popups {
            if p.status.is_some() {
                continue;
            }
            match newest.iter_mut().find(|q| q.slot == p.slot) {
                Some(q) if q.frames_remaining >= p.frames_remaining => {}
                Some(q) => *q = p,
                None => newest.push(p),
            }
        }
        // Seated the way retail's renderer `FUN_801DF6B8` seats them: a
        // view-space square over the struck actor's display trio, rising and
        // growing with the ring timer (`battle_numerals::popup_value_cells`,
        // the kernel the native window seats through too).
        let mut cells = Vec::new();
        for p in newest {
            let Some(trio) = world.battle_display_trio(usize::from(p.slot)) else {
                continue;
            };
            let age = p.frames_total.saturating_sub(p.frames_remaining);
            cells.extend(legaia_render_kernels::battle_numerals::popup_value_cells(
                &vp,
                BATTLE_WORLD_SCALE_LOCAL,
                trio,
                p.amount,
                age,
            ));
        }
        Some((cluster, cells))
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
    /// The quads come out of `legaia_render_kernels::battle_numerals`, the same
    /// builder the native window emits through, so both hosts sample the same
    /// texels off the same page through the same ordering-table pass. Empty
    /// outside battle and before the battle VRAM exists, which is what leaves
    /// the font fallback in charge for those frames.
    pub(crate) fn battle_value_readout_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        use legaia_render_kernels::battle_numerals as bn;
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
