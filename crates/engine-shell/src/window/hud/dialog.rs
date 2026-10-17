//! The dialog panel on the play window: mirroring the world's dialog and
//! text-balloon requests into typed-out panels, the plain-string snapshot the
//! text and chrome layers share, the stage layout and the chrome sprites.

use super::*;

impl PlayWindowApp {
    /// Keep the rendered dialog panel ([`Self::active_dialog`]) in sync with
    /// the world's pending dialog request.
    ///
    /// The world owns dismissal: the field VM's op-`0x4C` dialog-advance hook
    /// and the overworld talk-to handler both clear `World::dialog.current` on
    /// a confirm/cancel press. This method only mirrors that state into a
    /// visible, typed-out box - it opens a panel from the scene's MES the frame
    /// a request appears, ticks its typewriter reveal, and drops the panel the
    /// frame the world clears the request. It never clears `current_dialog`
    /// itself, so it can't race the world's dismiss.
    pub(in crate::window) fn sync_dialog_panel(&mut self) {
        // When the inline-script field-VM runner owns dialogue, it manages its
        // own box (rendered from `world.dialog.inline`); don't also open the
        // simplified panel.
        if self.session.host.world.toggles.use_vm_dialogue {
            self.active_dialog = None;
            return;
        }
        if self.session.host.world.dialog.current.is_none() {
            self.active_dialog = None;
            return;
        }
        if self.active_dialog.is_none()
            && let Some(mut panel) = self.session.host.open_pending_dialog()
        {
            panel.set_glyphs_per_frame(2);
            self.active_dialog = Some(panel);
        }
        if let Some(panel) = self.active_dialog.as_mut() {
            panel.tick();
        }
    }

    /// Commit the `4C E1` text balloon's font measurement while the app is
    /// still `&mut` - the tick-phase half of the balloon's measure/commit
    /// round-trip (`World::commit_text_balloon_width`).
    ///
    /// Retail measures the line inside the spawner (`FUN_8003C764` via
    /// `FUN_80035F04`) because its font metrics share its address space; the
    /// engine's font atlas is host-side, so the record's `x` stays `None`
    /// until a host measures. Doing it here, in the same mutation phase as
    /// [`Self::sync_dialog_panel`], is what lets [`Self::build_hud`] and
    /// [`Self::dialog_chrome_sprite_draws`] stay `&self`: by the time the
    /// draw passes run, the committed geometry is already on the record
    /// (`TextBalloon::pen` / `frame_rect`). The commit is idempotent, so the
    /// cheap `x.is_none()` guard is an optimisation, not a correctness gate.
    pub(in crate::window) fn sync_text_balloon(&mut self) {
        let world = &mut self.session.host.world;
        if world
            .cutscene
            .text_balloon
            .as_ref()
            .is_none_or(|b| b.x.is_some() || b.killed)
        {
            return;
        }
        let width = match world.cutscene.text_balloon.as_ref() {
            Some(b) => legaia_engine_ui::text_balloon_text_width(&self.font, &b.text),
            None => return,
        };
        world.commit_text_balloon_width(width);
    }
}

impl PlayWindowApp {
    /// The live dialog source as a plain-string snapshot
    /// (`legaia_engine_screens::dialog_screens::dialog_snapshot`, the browser
    /// page's call too): this window's own typed-out panel first, then the
    /// script / timeline panel, then the inline field-VM runner.
    pub(in crate::window) fn dialog_snapshot(&self) -> Option<DialogSnapshot> {
        legaia_engine_screens::dialog_screens::dialog_snapshot(
            &self.session.host.world,
            self.active_dialog.as_ref(),
        )
    }
}

impl PlayWindowApp {
    /// Build the dialog-window chrome sprites (gradient fill + gold
    /// 9-slice frame + hand cursors) for the active dialog box, if
    /// any. Sampled from the resident system-UI atlas; composited in
    /// the same sprite slot as the menu chrome, under the text layer.
    pub(in crate::window) fn dialog_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        // The `4C E1` balloon's frame: the fixed `0x58 x 0x90` window
        // (`FUN_8002C69C(0x58, y, 0x90, 0xB)`), in the same chrome skin as
        // the reading box. Drawn whether or not a reading box is also up -
        // retail's balloon is its own actor and outlives any dialog
        // engagement that spawned it.
        let mut out: Vec<legaia_engine_ui::SpriteDraw> = Vec::new();
        if self.session.host.world.text_balloon_drawing().is_some()
            && let Some(rect) = self
                .session
                .host
                .world
                .cutscene
                .text_balloon
                .as_ref()
                .map(|b| b.frame_rect())
        {
            out.extend(legaia_engine_ui::text_balloon_chrome_draws_for(
                &assets.rects,
                rect,
                stage_origin,
                stage_scale,
            ));
        }
        // The overworld map display's place labels: one dialog-skin box per
        // discovered place, at its projected map position.
        let labels =
            legaia_engine_core::world_map_labels::map_display_labels_now(&self.session.host.world);
        if !labels.is_empty() {
            let draws: Vec<legaia_engine_ui::MapLabelDraw<'_>> = labels
                .iter()
                .map(|l| (l.name.as_slice(), l.sx, l.sy))
                .collect();
            out.extend(legaia_engine_ui::map_label_chrome_draws_for(
                &assets.rects,
                &self.font,
                &draws,
                stage_origin,
                stage_scale,
            ));
        }
        if let Some(lay) = self.tile_board_prompt_layout()
            && let Some(cursor) = self.session.host.world.tile_board_prompt_cursor()
        {
            out.extend(legaia_engine_ui::tile_board_prompt_sprites_for(
                &assets.rects,
                &lay,
                cursor,
                stage_origin,
                stage_scale,
            ));
        }
        if self.session.host.world.incense_notice_shown() {
            out.extend(legaia_engine_ui::incense_notice_sprites_for(
                &assets.rects,
                legaia_engine_core::incense_notice::notice_frame_rect(),
                stage_origin,
                stage_scale,
            ));
        }
        let Some(snap) = self.dialog_snapshot() else {
            return out;
        };
        out.extend(
            legaia_engine_screens::dialog_screens::dialog_box_chrome_draws(
                &assets.rects,
                &snap,
                stage_origin,
                stage_scale,
            ),
        );
        out
    }
}

/// Plain-string view of the live dialog panel shared by the text and
/// chrome layers - the shared `engine-screens` type.
pub(in crate::window) use legaia_engine_screens::dialog_screens::DialogSnapshot;
