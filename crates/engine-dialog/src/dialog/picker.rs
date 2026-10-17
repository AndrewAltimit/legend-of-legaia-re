//! `OwnedDialogPanel` choice picker: confirm, labels, cursor, slide and rect.

use super::*;

/// SFX cue the pager raises when the option cursor moves: `jal
/// FUN_80035B50` with `a0 = 0x21` at `0x801D9510`, inside the shared cursor
/// handler of `FUN_801D84D0`.
pub const PICKER_MOVE_CUE: i16 = 0x21;
/// SFX cue the pager raises on the confirm edge that commits a choice: `jal
/// FUN_80035B50` with `a0 = 0x20` at `0x801D9530`, behind the test of the
/// edge word against the confirm mask `0x800846D0`.
pub const PICKER_CONFIRM_CUE: i16 = 0x20;

/// Raw PSX pad bits an open picker reads: Up / Down step the cursor, Cross
/// and Circle commit the choice.
pub const PICKER_PAD_UP: u16 = 0x0010;
/// See [`PICKER_PAD_UP`].
pub const PICKER_PAD_DOWN: u16 = 0x0040;
/// See [`PICKER_PAD_UP`].
pub const PICKER_PAD_CONFIRM: u16 = 0x4000 | 0x2000;
/// Every bit an open picker owns while it is up.
pub const PICKER_PAD_MASK: u16 = PICKER_PAD_UP | PICKER_PAD_DOWN | PICKER_PAD_CONFIRM;

/// What one tick's pad edges did to an open picker
/// ([`OwnedDialogPanel::step_picker_pad`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PickerPadStep {
    /// Bits the picker owns this tick: the caller hides them from whatever
    /// else reads the pad (the world's dialog dismiss), so the confirm that
    /// commits a choice does not also close the box.
    pub consumed: u16,
    /// The cursor moved ([`PICKER_MOVE_CUE`]).
    pub moved: bool,
    /// The option a confirm committed ([`PICKER_CONFIRM_CUE`]).
    pub chosen: Option<usize>,
}

impl OwnedDialogPanel {
    /// Drive an open picker from one tick's **raw** pad words (`pressed` =
    /// this tick's press edges). The bound pad word is the input, not a key:
    /// a rebound key or a controller reaches the picker exactly as a default
    /// key does. Up and Down step the cursor, Cross / Circle commit. With no
    /// menu up nothing is consumed.
    pub fn step_picker_pad(&mut self, pressed: u16) -> PickerPadStep {
        if !self.menu_active() {
            return PickerPadStep::default();
        }
        let mut step = PickerPadStep {
            consumed: PICKER_PAD_MASK,
            ..PickerPadStep::default()
        };
        if pressed & PICKER_PAD_UP != 0 {
            step.moved |= self.move_picker_cursor(-1);
        }
        if pressed & PICKER_PAD_DOWN != 0 {
            step.moved |= self.move_picker_cursor(1);
        }
        if pressed & PICKER_PAD_CONFIRM != 0 {
            step.chosen = self.confirm_menu();
        }
        step
    }

    /// Confirm the highlighted menu option: apply its relative jump and resume
    /// the dialogue at the chosen branch's reply (mirrors the retail inline-
    /// script control handler `FUN_80038050`, which sets the script PC to
    /// `(open + 1 + index*2) + i16_LE(entry[index])`). The panel jumps to the
    /// branch, skips any leading non-text bytecode to the next `0x1F` reply
    /// segment, types it, and re-attaches a nested menu if one follows. With no
    /// reply segment at/after the target the conversation ends (`Done`).
    ///
    /// Returns the chosen option index, or `None` if no menu is active.
    // PORT: FUN_80038050
    pub fn confirm_menu(&mut self) -> Option<usize> {
        if !self.picker_takes_input() {
            return None;
        }
        let picker = self.picker.as_ref()?;
        let cursor = self.picker_cursor;
        let target = picker.jump_target(cursor)?;
        // Resume at the chosen branch: find its first reply text segment.
        let next_lead = self.bytes[target.min(self.bytes.len())..]
            .iter()
            .position(|&b| b == 0x1F)
            .map(|rel| target + rel);
        self.page.clear();
        self.menu_active = false;
        self.picker_slide = None;
        self.waiting_for_input = false;
        self.picker_cursor = 0;
        match next_lead {
            Some(lead) => {
                self.seed_box_at_lead(lead);
                self.state = PanelState::Typing;
            }
            None => {
                self.picker = None;
                self.done = true;
                self.state = PanelState::Done;
            }
        }
        Some(cursor)
    }

    /// The decoded option-picker following the prompt, if this box is a
    /// multiple-choice menu. `None` for plain dialogue.
    pub fn picker(&self) -> Option<&legaia_mes::Picker> {
        self.picker.as_ref()
    }

    /// The open picker's option labels as drawn: each label re-decoded from
    /// its `0x1F` lead with the `0xC1..=0xC7` name escapes resolved through
    /// [`Self::substitutions`], as the box text is. Empty without a picker.
    ///
    /// [`legaia_mes::PickerOption::label`] carries no names (that crate has
    /// no tables), and both hosts printed it, so a choice such as dolk2's
    /// "Be careful, `C1 01`!" read "Be careful, !" on either.
    pub fn picker_labels(&self) -> Vec<Vec<u8>> {
        let Some(p) = self.picker.as_ref() else {
            return Vec::new();
        };
        p.options
            .iter()
            .map(|o| {
                let mut out = Vec::new();
                let mut interp = Interpreter::new_at(&self.bytes, o.lead + 1);
                loop {
                    match interp.next_event() {
                        Some(MesEvent::Glyph(g))
                        | Some(MesEvent::SkipTwo(g))
                        | Some(MesEvent::WideGlyph(_, g)) => out.push(g),
                        Some(MesEvent::Substitute { kind, arg }) => {
                            if let Some(name) = self
                                .substitutions
                                .as_ref()
                                .and_then(|s| s.get(&(substitute_kind_key(kind), arg)))
                            {
                                out.extend_from_slice(name);
                            }
                        }
                        Some(MesEvent::EndOfMessage(_)) | None => break,
                        Some(_) => {}
                    }
                }
                out
            })
            .collect()
    }

    /// `true` once the prompt has finished typing and a menu is awaiting a
    /// choice (the host should draw the options + cursor and route Up/Down).
    pub fn menu_active(&self) -> bool {
        self.menu_active
    }

    /// Highlighted option index while the menu is active (`0` otherwise).
    pub fn picker_cursor(&self) -> usize {
        self.picker_cursor
    }

    /// The open menu's slide state, if a menu is open.
    pub fn picker_slide(&self) -> Option<&crate::dialog_picker_slide::PickerSlide> {
        self.picker_slide.as_ref().filter(|_| self.menu_active)
    }

    /// The picker box's centre rect `(x, y, w, h)` in 320x240 stage pixels
    /// this frame - somewhere on its slide, or at rest - or `None` when no
    /// menu is open or the slide has not started (the press's sentinel,
    /// which retail draws as no box). Both hosts draw the box and its labels
    /// here; `0x2A` rests at the top right, the N-option lists at the bottom.
    pub fn picker_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.picker_slide()?.rect()
    }

    /// The option hand is drawn: the slide has come to rest (count 0).
    pub fn picker_hand_drawn(&self) -> bool {
        self.picker_slide().is_some_and(|s| s.hand_drawn())
    }

    /// The open menu takes Up/Down and confirm: the slide came to rest on an
    /// earlier pager call. `false` while it slides, and when no menu is open.
    pub fn picker_takes_input(&self) -> bool {
        self.menu_active && self.picker_slide.as_ref().is_none_or(|s| s.takes_input())
    }

    /// Move the menu cursor by `delta` - wrapping within `0..n`, or clamping
    /// at the ends for a `0x2A` menu (the shared cursor handler at
    /// `0x801D941C` tests for its state `0x12`). No-op when the box isn't a
    /// menu, and while an open menu still slides in.
    ///
    /// Returns whether the cursor moved - retail's test for the move cue
    /// ([`PICKER_MOVE_CUE`]): the handler compares the cursor it loaded on
    /// entry against the stored one (`beq s0,v0` at `0x801D9508`), so a
    /// clamped press at either end of a `0x2A` menu is silent.
    pub fn move_picker_cursor(&mut self, delta: i32) -> bool {
        if self.menu_active && !self.picker_takes_input() {
            return false;
        }
        let before = self.picker_cursor;
        if let Some(p) = &self.picker
            && p.n > 0
        {
            let n = p.n as i32;
            let next = self.picker_cursor as i32 + delta;
            self.picker_cursor = if legaia_mes::picker_cursor_clamps(p.open_byte) {
                next.clamp(0, n - 1) as usize
            } else {
                ((next % n + n) % n) as usize
            };
        }
        self.picker_cursor != before
    }

    /// Open the menu: set it active with its slide state - the press's
    /// sentinel on the pager path, at rest where no press opens it.
    pub(super) fn open_menu(&mut self, pressed: bool) {
        self.menu_active = true;
        let open_byte = self.picker.as_ref().map_or(0x27, |p| p.open_byte);
        self.picker_slide = Some(if pressed && self.window.is_some() {
            crate::dialog_picker_slide::PickerSlide::pressed(open_byte)
        } else {
            crate::dialog_picker_slide::PickerSlide::at_rest(open_byte)
        });
    }
}
