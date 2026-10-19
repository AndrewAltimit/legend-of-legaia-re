//! The dialog panel on the play window: mirroring the world's dialog request
//! into a typed-out panel, committing the text balloon's measurement, and the
//! field message layer the text and chrome passes both read
//! (`legaia_engine_screens::field_messages`).

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
        legaia_engine_screens::field_messages::commit_text_balloon_measure(
            &mut self.session.host.world,
            &self.font,
        );
    }
}

impl PlayWindowApp {
    /// This frame's field message layer
    /// (`legaia_engine_screens::field_messages::field_message_frame`, the
    /// browser page's call too): the balloon, the map labels, the tile-board
    /// prompt, the Incense notice and the dialog reading box, in one order.
    pub(in crate::window) fn field_message_frame(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> legaia_engine_screens::field_messages::FieldMessageFrame {
        use legaia_engine_screens::field_messages as fm;
        fm::field_message_frame(
            &fm::FieldMessageInputs {
                host: &self.session.host,
                host_panel: self.active_dialog.as_ref(),
                font: &self.font,
                chrome: self.save_menu.as_ref().map(|a| &a.rects),
            },
            surface_w,
            surface_h,
        )
    }

    /// The field message layer's chrome sprites (gradient fills, gold
    /// 9-slice frames, hand cursors), sampled from the resident system-UI
    /// atlas and composited in the menu-chrome sprite slot, under the text
    /// layer. Empty without the atlas and under the boot UI.
    pub(in crate::window) fn dialog_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_ui::SpriteDraw> {
        if self.save_menu.is_none() || self.boot_ui.is_active() {
            return Vec::new();
        }
        self.field_message_frame(surface_w, surface_h).sprites
    }
}
