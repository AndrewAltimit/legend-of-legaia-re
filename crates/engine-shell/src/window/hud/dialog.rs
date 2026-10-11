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
    pub(in crate::window) fn dialog_snapshot(&self) -> Option<DialogSnapshot> {
        let to_ascii = |bytes: &[u8]| -> String {
            bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) {
                        b as char
                    } else {
                        '?'
                    }
                })
                .collect()
        };
        let from_panel = |panel: &legaia_engine_dialog::dialog::OwnedDialogPanel,
                          require_text: bool|
         -> Option<DialogSnapshot> {
            let page = legaia_engine_ui::dialog_page_string(&panel.page_bytes());
            if require_text && page.is_empty() {
                return None;
            }
            let (options, cursor) = if panel.menu_active() {
                match panel.picker() {
                    Some(_) => (
                        panel.picker_labels().iter().map(|l| to_ascii(l)).collect(),
                        panel.picker_cursor(),
                    ),
                    None => (Vec::new(), 0),
                }
            } else {
                (Vec::new(), 0)
            };
            Some(DialogSnapshot {
                page,
                scroll_px: panel.scroll_px(),
                box_rows: panel.box_rows(),
                options,
                cursor,
                picker_rect: panel.picker_rect(),
                picker_hand: panel.picker_hand_drawn(),
                hand_bob: panel.picker_hand_bob(),
                advance_icon: self.session.host.world.page_mark_frame(panel),
            })
        };
        if let Some(panel) = self.active_dialog.as_ref() {
            return from_panel(panel, false);
        }
        if let Some(panel) = self.session.host.world.script_dialog_panel()
            && let Some(snap) = from_panel(panel, true)
        {
            return Some(snap);
        }
        if let Some(id) = self.session.host.world.dialog.inline.as_ref()
            && let Some(panel) = id.panel.as_ref()
        {
            return from_panel(panel, true);
        }
        None
    }

    /// Compute the stage-pixel box rects for a dialog snapshot,
    /// mirroring the pager's traced geometry (`FUN_801D84D0`):
    ///
    /// - Main (reading) box: `(0x26, 0x10, 0xF4, lines*0xF - 3)` - the
    ///   per-frame `FUN_8002C69C` call passes `(ctx+0x12, ctx+0x14,
    ///   0xF4, lines*0xF + 5 - 8)`, and the live context in the
    ///   `v0_1_tetsu_dialogue_accept` capture holds `ctx+0x12 = 0x26`,
    ///   `ctx+0x14 = 0x10` (framebuffer cross-checked: drawn footprint
    ///   `x 30..289, y 8..65` = this rect inflated by the skin border).
    ///   Retail anchors the reading box at the TOP of the stage - with
    ///   or without an option picker.
    /// - Picker box: the engine's slide rect
    ///   ([`legaia_engine_dialog::dialog::OwnedDialogPanel::picker_rect`]) -
    ///   the box on its way in from off screen, then at rest: `0x2A` at the
    ///   top right `(0xD8, 0x4A, 0x58, 0x1A)`, the N-option lists at
    ///   `(0x26, 0x94 + ((4-n)*0xF)/2, 0xF4, 0x38 - (4-n)*0xF)`. `None`
    ///   (no box) on the press's sentinel call.
    ///
    /// Rects are the retail centre rects; the border skin the chrome
    /// pass draws extends ~8 px beyond them on every side
    /// (`dialog_window_chrome_draws_for`).
    pub(in crate::window) fn dialog_stage_layout(snap: &DialogSnapshot) -> DialogStageLayout {
        // Retail's standard reading box is ALWAYS 3 rows tall
        // (`_DAT_801F2740 = 3` in both box-init arms) regardless of how
        // much text has typed in; only over-long simplified pages grow
        // it to a 4th row.
        let lines = legaia_engine_ui::dialog_reading_box_lines(&snap.page, snap.box_rows);
        let main_w = 0xF4;
        let main_h = lines * 0xF - 3;
        let picker = if snap.options.is_empty() {
            None
        } else {
            snap.picker_rect
        };
        DialogStageLayout {
            main: (0x26, 0x10, main_w, main_h),
            picker,
        }
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
        let lay = Self::dialog_stage_layout(&snap);
        out.extend(legaia_engine_ui::dialog_window_chrome_draws_for(
            &assets.rects,
            lay.main,
            stage_origin,
            stage_scale,
        ));
        if let Some(prect) = lay.picker {
            out.extend(legaia_engine_ui::dialog_window_chrome_draws_for(
                &assets.rects,
                prect,
                stage_origin,
                stage_scale,
            ));
            // Pointing-hand cursor on the selected option row
            // (FUN_8002B994 kind 0 at box_x-6, box_y + cursor*0xF), drawn
            // only once the slide rests (count 0).
            if snap.picker_hand {
                out.push(legaia_engine_ui::dialog_option_hand_sprite(
                    &assets.rects,
                    (prect.0 + snap.hand_bob, prect.1),
                    snap.cursor,
                    stage_origin,
                    stage_scale,
                ));
            }
        } else if let Some(frame) = snap.advance_icon {
            // The two-frame page mark at the lower-right rim while the
            // pager waits for confirm (FUN_8002B994 kind 1).
            out.push(legaia_engine_ui::dialog_page_mark_sprite(
                legaia_engine_menus::save_menu_atlas::ATLAS_RECT_ADVANCE_ICON
                    [usize::from(frame) & 1],
                lay.main,
                stage_origin,
                stage_scale,
            ));
        }
        out
    }
}

/// Plain-string view of the live dialog panel shared by the text and
/// chrome layers (see `PlayWindowApp::dialog_snapshot`).
pub(in crate::window) struct DialogSnapshot {
    /// Current typed-out page, `|` (0x7C) separating rows.
    pub page: String,
    /// Whole pixels the rows draw above their slots (the pager window's
    /// scroll, `<= 0`).
    pub scroll_px: i32,
    /// The pager window's height in rows (`None` for the plain-MES panel,
    /// whose box grows with its page); also the text clip band.
    pub box_rows: Option<usize>,
    /// Decoded option labels when a picker menu is open (empty
    /// otherwise).
    pub options: Vec<String>,
    /// Selected option row.
    pub cursor: usize,
    /// The picker box's rect this frame, from the engine's slide
    /// (`OwnedDialogPanel::picker_rect`); `None` before it starts.
    pub picker_rect: Option<(i32, i32, i32, i32)>,
    /// The option hand is drawn (the slide rests).
    pub picker_hand: bool,
    /// The option hand's idle bob (X, stage pixels).
    pub hand_bob: i32,
    /// The page mark's strip frame, `None` when the pager draws none (no
    /// wait, or an automatic press counting down).
    pub advance_icon: Option<u8>,
}

/// Stage-pixel dialog box layout (see
/// `PlayWindowApp::dialog_stage_layout`).
pub(in crate::window) struct DialogStageLayout {
    /// Main reading-box rect `(x, y, w, h)`.
    pub main: (i32, i32, i32, i32),
    /// Option-picker box rect when a menu is open.
    pub picker: Option<(i32, i32, i32, i32)>,
}
