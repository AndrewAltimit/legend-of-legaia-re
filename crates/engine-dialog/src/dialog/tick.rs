//! `OwnedDialogPanel` per-frame step: typewriter pacing, auto-press, the row window, page waits and page advance.

use super::*;

impl OwnedDialogPanel {
    pub fn set_glyphs_per_frame(&mut self, n: u8) {
        self.glyphs_per_frame = n.max(1);
    }

    /// Advance one vsync at `frame_step` (`DAT_1F800393`) and return the new
    /// state: [`Self::frame_step`] is set first, then [`Self::tick`] runs.
    pub fn tick_at(&mut self, frame_step: u8) -> PanelState {
        self.frame_step = frame_step;
        self.tick()
    }

    /// [`Self::tick_at`] plus the pager's **automatic press**: while a box
    /// page waits in state `0x19`, each pager call counts `auto_press`
    /// (retail's `_DAT_80073F00`, which field-VM op `4C 89` writes) down by
    /// the frame step once it is positive, and the call that takes it to zero
    /// or below clears it and presses confirm for the player
    /// (`0x801D8F4C..0x801D8F88`: the pad word is replaced by the confirm
    /// binding `0x800846D0`). The host reads the press through
    /// [`Self::take_auto_press`] and handles it as its own confirm.
    ///
    /// A retail capture (`autorun_dialog_picker_open.lua` with
    /// `LEGAIA_PICKER_AUTO=21` on `retock_innkeeper_talk_open`, frame step 2)
    /// counts `19, 17, .. 1` on the eleven calls after the page waits and
    /// leaves `0x19` on the twelfth with no button down.
    ///
    /// PORT: FUN_801D84D0 (state `0x19`'s `_DAT_80073F00` countdown, `0x801D8F4C..0x801D8F88`)
    pub fn tick_at_auto(&mut self, frame_step: u8, auto_press: &mut i16) -> PanelState {
        let pager_call = self.window.is_some() && self.vsync_phase == 0;
        let in_wait = self.waiting_for_input && !self.menu_active && !self.done;
        let st = self.tick_at(frame_step);
        if pager_call && in_wait && *auto_press > 0 {
            *auto_press = auto_press.saturating_sub(i16::from(frame_step.max(1)));
            if *auto_press <= 0 {
                *auto_press = 0;
                self.auto_pressed = true;
            }
        }
        st
    }

    /// [`Self::tick_at_auto`] plus the pager call's two cursor sprites, run
    /// through the shared primitive state `cursors`
    /// ([`crate::cursor_sprite`]) as retail's draw tail runs them:
    ///
    /// * the **page mark** (kind `1`, mode `1`) at
    ///   `(0x10A, box_y + rows*0xF - 0x13)`, only while the pager sits in state `0x19` **and** the
    ///   automatic-press countdown `_DAT_80073F00` is not running
    ///   (`0x801D9804..0x801D985C`: `bne state,0x19` then `bgtz` on the
    ///   countdown) - a page a script turns by itself never shows the mark;
    /// * the **option hand** (kind `0`, mode `1`) on the selected row once
    ///   the picker's slide rests (`0x801D9BB4..0x801D9BE0`).
    ///
    /// Read the results through [`Self::advance_icon`] and
    /// [`Self::picker_hand_bob`].
    ///
    /// PORT: FUN_801D84D0 (draw tail: page-mark gate `0x801D9804..0x801D985C`, option hand `0x801D9BB4..0x801D9BE0`)
    pub fn tick_at_auto_drawn(
        &mut self,
        frame_step: u8,
        auto_press: &mut i16,
        cursors: &mut crate::cursor_sprite::CursorSprites,
    ) -> PanelState {
        let pager_call = self.window.is_some() && self.vsync_phase == 0;
        let st = self.tick_at_auto(frame_step, auto_press);
        if !pager_call {
            return st;
        }
        self.cursor_driven = true;
        let dt = frame_step.max(1);
        let waits = (self.waiting_for_input || self.done) && !self.menu_active;
        self.advance_icon = if waits && *auto_press <= 0 && !self.auto_pressed {
            let rows = self.box_rows().unwrap_or(0) as i16;
            Some(
                cursors
                    .call(
                        crate::cursor_sprite::KIND_PAGE_MARK,
                        0x10A,
                        0x10 + rows * 0xF - 0x13,
                        dt,
                    )
                    .frame,
            )
        } else {
            None
        };
        self.hand_bob = match self.picker_rect().filter(|_| self.picker_hand_drawn()) {
            Some((x, y, _, _)) => {
                cursors
                    .call(
                        crate::cursor_sprite::KIND_HAND,
                        (x - 6) as i16,
                        (y + self.picker_cursor as i32 * 0xF) as i16,
                        dt,
                    )
                    .dx
            }
            None => 0,
        };
        st
    }

    /// The page mark to draw at the box's lower-right rim: `Some(frame)` of
    /// the two-frame strip ([`crate::cursor_sprite::CURSOR_RECORDS`] kind
    /// `1`), `None` when the pager draws none. A panel no pager call has
    /// animated (the plain-MES paths, a host ticking [`Self::tick`] itself)
    /// answers the still frame `0` while it waits for a press.
    pub fn advance_icon(&self) -> Option<u8> {
        if self.cursor_driven {
            return self.advance_icon;
        }
        ((self.waiting_for_input || self.done) && !self.menu_active).then_some(0)
    }

    /// The option hand's idle bob in stage pixels (X), `0` while no pager
    /// call animates it.
    pub fn picker_hand_bob(&self) -> i32 {
        self.hand_bob
    }

    /// Whether the automatic press fired on the last [`Self::tick_at_auto`];
    /// clears it.
    pub fn take_auto_press(&mut self) -> bool {
        std::mem::take(&mut self.auto_pressed)
    }

    /// Advance one frame (one vsync) and return the new state.
    ///
    /// A pager page (every field-pager path: [`Self::from_inline_dialog`] /
    /// [`Self::at_segment`]) runs the retail row window
    /// ([`crate::dialog_window`]): one pager call every [`Self::frame_step`]
    /// ticks, the last row typed at the [`crate::dialog_pacing`] pace, the
    /// window scrolled a row when a line follows a full one, rows carried
    /// over a page turn scrolled away before the page waits, and a confirm
    /// press ([`Self::confirm_while_typing`]) completing the page. The
    /// plain-MES paths ([`Self::new`] / [`Self::from_scene_mes`]) keep one
    /// event per [`Self::glyphs_per_frame`] ticks.
    pub fn tick(&mut self) -> PanelState {
        if self.window.is_some() {
            return self.tick_window();
        }
        if self.done {
            self.state = PanelState::Done;
            return self.state;
        }
        if self.waiting_for_input {
            self.state = PanelState::PageBreak;
            return self.state;
        }
        self.tick_count += 1;
        if !self.tick_count.is_multiple_of(self.glyphs_per_frame as u64) {
            return self.state;
        }
        let mut interp = Interpreter::new_at(&self.bytes, self.pc);
        let next = interp.next_event();
        self.pc = interp.pc();
        match next {
            Some(MesEvent::EndOfMessage(_)) | None => self.end_row(),
            Some(ev) => {
                self.apply_event(ev);
            }
        }
        self.state
    }

    /// The box path's vsync: the pager's game-tick cadence runs on whether
    /// or not the page waits, as retail's does.
    fn tick_window(&mut self) -> PanelState {
        let dt = self.frame_step.max(1);
        let run = self.vsync_phase == 0;
        self.vsync_phase = (self.vsync_phase + 1) % dt;
        if self.done {
            self.state = PanelState::Done;
            return self.state;
        }
        if self.menu_active {
            // The open menu's pager call: its slide advances a step.
            if run && let Some(slide) = self.picker_slide.as_mut() {
                slide.call(dt);
            }
            self.state = PanelState::PageBreak;
            return self.state;
        }
        self.tick_count += 1;
        if !run {
            return self.state;
        }
        self.refresh_decode();
        let pressed = std::mem::take(&mut self.press_latch);
        let reached_wait = {
            let rows = self
                .page_rows
                .last()
                .map(|(_, r)| Arc::clone(r))
                .unwrap_or_default();
            let Some(w) = self.window.as_mut() else {
                return self.state;
            };
            w.call(dt, pressed, |line| rows.get(line).map_or(0, |r| r.count))
        };
        self.rebuild_page();
        if reached_wait {
            self.page_waits();
        }
        self.state
    }

    /// The page is shown whole and the window waits (pager state `0x19`):
    /// open the menu, page-break on a continuing dispatch, or end.
    fn page_waits(&mut self) {
        if let Some(bx) = self.current_box.as_ref() {
            self.pc = bx.dispatch_at;
        }
        if self.picker.is_some() && !self.menu_active {
            // The prompt's page waits with the advance hand like any other;
            // the menu opens on the press (`0x801D9040..0x801D909C`: the
            // `0x2A` / `0x27` / `0x28` / `0x29` open bytes install their
            // picker states only inside `0x19`'s press arm - no other store
            // of `0x11` / `0x13` / `0x15` / `0x17` exists in the pager).
            if self.menu_opens_at_wait {
                self.open_menu(false);
            } else {
                self.menu_pending = true;
            }
            self.waiting_for_input = true;
            self.state = PanelState::PageBreak;
        } else if self
            .current_box
            .as_ref()
            .is_some_and(|bx| bx.dispatch.continues())
        {
            self.pending_box_advance = true;
            self.waiting_for_input = true;
            self.state = PanelState::PageBreak;
        } else {
            self.done = true;
            self.state = PanelState::Done;
        }
    }

    /// A confirm press while a pager page is still typing or scrolling: the
    /// next pager call sees it - it latches the skip speed `0x25` and
    /// completes the page (state `0x0D`), clears a short-row hold, or speeds a
    /// scroll (see [`crate::dialog_window`]). Returns `false` (and does
    /// nothing) when the page already waits, a menu is up, the panel is done,
    /// or this is a plain-MES panel.
    pub fn confirm_while_typing(&mut self) -> bool {
        if self.done || self.waiting_for_input || self.menu_active {
            return false;
        }
        match self.window.as_ref() {
            Some(w) if !w.waiting() => {
                self.press_latch = true;
                true
            }
            _ => false,
        }
    }

    /// The retail pager's typewriter words for the row being typed (box
    /// path; see [`crate::dialog_pacing`]).
    pub fn pacer(&self) -> &crate::dialog_pacing::TypewriterPacer {
        self.window
            .as_ref()
            .map_or(&crate::dialog_pacing::IDLE_PACER, |w| &w.pacer)
    }

    /// The retail row window, on the box path.
    pub fn window(&self) -> Option<&crate::dialog_window::RowWindow> {
        self.window.as_ref()
    }

    /// The `0x1F` lead of every row in the window, top first - the retail
    /// row table `_DAT_801F3540[]` as buffer offsets. Empty off the box path.
    pub fn window_row_leads(&self) -> Vec<usize> {
        let Some(w) = self.window.as_ref() else {
            return Vec::new();
        };
        w.rows
            .iter()
            .filter_map(|r| {
                self.page_rows
                    .iter()
                    .find(|(s, _)| *s == r.page)
                    .and_then(|(_, rows)| rows.get(r.line))
                    .map(|row| row.lead)
            })
            .collect()
    }

    /// Rows the reading box is tall: the window's capacity
    /// (`_DAT_801F2740`, [`crate::dialog_window::WINDOW_ROWS`] unless op
    /// `4C E0` set it) on the box path (the page buffer can hold one more
    /// while a row scrolls in), `None` on the plain-MES path, whose box
    /// grows with its page.
    pub fn box_rows(&self) -> Option<usize> {
        self.window.as_ref().map(|w| w.capacity)
    }

    /// Apply the field VM's window format to this panel's window: the row
    /// capacity op `4C E0 <n>` set (`_DAT_801F2740`) and, when `instant`,
    /// the `4C E0 00` latch (`_DAT_801F2744`) that loads the next page whole.
    pub fn set_window_format(&mut self, rows: usize, instant: bool) {
        self.window_rows = rows.max(1);
        self.window_instant |= instant;
        if let Some(w) = self.window.as_mut() {
            w.capacity = self.window_rows;
            w.instant |= std::mem::take(&mut self.window_instant);
        }
    }

    /// Whole pixels every row of the page draws above its slot (the pager's
    /// `scroll >> 4`, `<= 0`); `0` off the box path.
    pub fn scroll_px(&self) -> i32 {
        self.window.as_ref().map_or(0, |w| w.scroll_px())
    }

    /// `FUN_80036044` on the line whose `0x1F` lead is at `lead`, the rest of
    /// the buffer after it included (the count's walk overruns the row's
    /// `NUL` by one byte per two-byte unit). Substitutions resolve through
    /// [`Self::substitutions`]; an unresolved one counts nothing, as the
    /// panel then types nothing for it either.
    pub(super) fn line_count(&self, lead: usize) -> u32 {
        let subs = self.substitutions.clone();
        let expand = move |op: u8, arg: u8| -> Option<Vec<u8>> {
            let key = match op {
                0xC1 => 1,
                0xC2 | 0xC4 => 2,
                0xC3 => 3,
                0xC5 => 5,
                0xC7 => 7,
                _ => return None,
            };
            subs.as_ref()?.get(&(key, arg)).cloned()
        };
        let text = self.bytes.get(lead..).unwrap_or(&[]);
        legaia_font::typewriter_glyph_count(text, Some(&expand)).count
    }

    /// Apply one non-terminator event to the page (plain-MES path).
    fn apply_event(&mut self, ev: MesEvent) {
        match ev {
            MesEvent::Glyph(g) | MesEvent::WideGlyph(_, g) | MesEvent::SkipTwo(g) => {
                self.page.push(PanelGlyph {
                    byte: g,
                    clut: self.current_clut,
                });
            }
            MesEvent::Control(_) => {
                self.waiting_for_input = true;
                self.state = PanelState::PageBreak;
            }
            MesEvent::Spacing(arg) => {
                // A number escape: the digits the host resolved at open.
                if let Some(subs) = self.substitutions.as_ref()
                    && let Some(digits) = subs.get(&(SCRIPT_COUNTER_KEY, arg))
                {
                    for &b in digits {
                        self.page.push(PanelGlyph {
                            byte: b,
                            clut: self.current_clut,
                        });
                    }
                }
            }
            MesEvent::Substitute { kind, arg } => {
                // Resolve through the host-installed name table (item /
                // character names). An absent entry emits nothing - the
                // pre-wiring behaviour.
                if let Some(subs) = self.substitutions.as_ref()
                    && let Some(name) = subs.get(&(substitute_kind_key(kind), arg))
                {
                    for &b in name {
                        self.page.push(PanelGlyph {
                            byte: b,
                            clut: self.current_clut,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// The plain-MES stream hit a terminator: open the following menu, or
    /// end.
    fn end_row(&mut self) {
        // A menu stops at the prompt terminator and waits on the option list
        // instead of tearing down: the retail inline-script handler
        // `FUN_80038050` reads the chosen index here and jumps via the
        // picker's relative-offset table.
        if self.picker.is_some() && !self.menu_active {
            self.open_menu(false);
            self.waiting_for_input = true;
            self.state = PanelState::PageBreak;
        } else {
            self.done = true;
            self.state = PanelState::Done;
        }
    }

    /// Resume from a page break. No-op when the panel isn't paused.
    ///
    /// On the box path, a page that ended on a continuing post-page dispatch
    /// turns to the next page: a `0x24` keeps the window (the next page types
    /// beneath the rows already shown, pager state `5`), a `0x48` opens a
    /// fresh box. Retail spends the press call and a load call on the turn,
    /// so the next page's first glyph shows on the second pager call after
    /// the press. On the plain path the page buffer clears. A box page that
    /// ends on a picker open byte opens its menu on this press instead, and
    /// the choice is committed by the next ([`Self::confirm_menu`]).
    pub fn advance_page(&mut self) {
        if !self.waiting_for_input {
            return;
        }
        if std::mem::take(&mut self.menu_pending) {
            // The press on a prompt that ends on a picker open byte opens the
            // menu, which slides in before it takes the choice
            // ([`crate::dialog_picker_slide`]).
            self.open_menu(true);
            self.state = PanelState::PageBreak;
            return;
        }
        self.waiting_for_input = false;
        self.state = PanelState::Typing;
        if self.window.is_none() {
            self.page.clear();
            return;
        }
        if !std::mem::take(&mut self.pending_box_advance) {
            return;
        }
        let Some(cur) = self.current_box.as_ref() else {
            return;
        };
        let keep = matches!(cur.dispatch, legaia_mes::Dispatch::NextPage);
        let next = cur
            .next_box_pc()
            .filter(|&n| self.bytes.get(n) == Some(&0x1F))
            .and_then(|n| legaia_mes::pack_page(&self.bytes, n));
        match next {
            Some(bx) => self.load_page(bx, Some(keep)),
            None => {
                // Continuation byte with no following lead (malformed
                // stream): end rather than re-type the same page.
                self.done = true;
                self.state = PanelState::Done;
            }
        }
    }

    pub fn page_glyphs(&self) -> &[PanelGlyph] {
        &self.page
    }

    pub fn page_bytes(&self) -> Vec<u8> {
        self.page.iter().map(|g| g.byte).collect()
    }

    pub fn state(&self) -> PanelState {
        self.state
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn is_waiting_for_input(&self) -> bool {
        self.waiting_for_input
    }
}
