//! The save-select session state machine (`SaveSelectSession`).
//! Split out of `save_select.rs`.

use super::*;

/// Save-select session state machine.
#[derive(Debug, Clone)]
pub struct SaveSelectSession {
    pub(super) mode: SaveSelectMode,
    pub(super) slots: Vec<SlotSnapshot>,
    pub(super) phase: SelectPhase,
    /// Frames the "Now checking" dialog stays visible after the user
    /// confirms a Load on a non-empty slot. Public knob so tests can
    /// fast-forward through the dialog without ticking 120 times.
    pub(super) now_checking_frames: u16,
    /// 12-bit fixed-point slide-in animation timer (0..=4096). Holds
    /// at 0 while Browsing/Done. On entry to `NowChecking` it resets
    /// to 0 and ramps `+SLIDE_ANIM_RATE` per tick, clamped at
    /// `SLIDE_ANIM_FULL`. Drives the linear interpolation
    /// `pos = start + (target - start) * t / 4096` used by the
    /// renderer to slide the slot composite + NowChecking dialog
    /// into place. Mirrors retail's per-element `DAT_801ef194` /
    /// `DAT_801ef160` (collapsed into a single timer here since the
    /// engine doesn't currently break the slide into independent
    /// elements).
    pub(super) slide_anim_t: u16,
    /// 12-bit fixed-point slide-in timer for the bottom info panel.
    /// Mirrors retail's `DAT_801ef1a0` in `FUN_801E08D8`, which is
    /// distinct from the slot-composite timer above: the info panel
    /// starts hidden (off-screen below the stage at y=394) and slides
    /// up to its parked position (y=138) only AFTER NowChecking
    /// completes. Holds at 0 during Browsing / NowChecking / Done;
    /// ramps during SlotPreview / ConfirmOverwrite / ConfirmDelete.
    pub(super) info_panel_slide_anim_t: u16,
    /// Opt-in retail two-stage flow: the slot list is the console's two
    /// **memory-card slots** rather than the save blocks themselves, so
    /// Save mode must cross the same `NowChecking` card-read beat Load
    /// mode does before the host can show the card's block grid. See the
    /// module docs. Default `false` = the flat block-list model.
    pub(super) card_slots_mode: bool,
    /// The four memory-card kernel events [`card_status_poll`] tests on
    /// every frame of the `NowChecking` beat. A host with a real card
    /// reader latches them through [`Self::set_card_events`]; the
    /// default all-`false` means "no card hardware is reporting", which
    /// leaves the beat to run its full [`Self::now_checking_frames`]
    /// count exactly as a disk-backed host expects.
    pub(super) card_events: [bool; CARD_STATUS_EVENTS],
    /// Retail's `DAT_801EF17C`. Cleared on entry to `NowChecking`
    /// (retail clears it in `FUN_801E3294`'s state 0) and advanced by
    /// [`card_status_poll`] on every frame of the beat.
    pub(super) card_poll_counter: u16,
}

/// What a save screen's pill row addresses - and the one thing that decides
/// whether the session runs retail's two-stage card flow.
///
/// Hosts do not set [`SaveSelectSession::set_card_slots_mode`]; they declare
/// a rack and [`SaveSelectSession::for_rack`] derives the flag from it. That
/// is deliberate: the mode is a property of *what the slot list is*, not a
/// switch a host may flip independently, and having each host decide
/// separately is the exact shape `scripts/ci/check-ui-host-drift.py` calls a
/// simulation divergence.
///
/// The driver around a card-ports session lives in
/// [`crate::save_screen::SaveScreenFlow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRack {
    /// Flat: the pill row **is** the block list. A pick off the pills is a
    /// pick of the save itself. Kept for headless drivers that own a plain
    /// list of saves and never show a second stage.
    Blocks(Vec<SlotSnapshot>),
    /// Retail: the pills are the console's memory-card **ports**, and the
    /// 5x3 preview grid is the chosen port's fifteen blocks. `present` on a
    /// port means "something is mounted here", not "this holds a save".
    CardPorts(Vec<SlotSnapshot>),
}

impl SaveRack {
    /// The pill row's entries.
    pub fn slots(&self) -> &[SlotSnapshot] {
        match self {
            Self::Blocks(s) | Self::CardPorts(s) => s,
        }
    }

    /// `true` for the two-stage card flow.
    pub fn is_card_ports(&self) -> bool {
        matches!(self, Self::CardPorts(_))
    }
}

impl SaveSelectSession {
    /// Build a session against a [`SaveRack`], taking the card-slots mode
    /// from the rack's kind. This is the constructor hosts use; `new` is the
    /// flat-list shorthand behind it.
    pub fn for_rack(mode: SaveSelectMode, rack: &SaveRack) -> Self {
        let mut s = Self::new(mode, rack.slots().to_vec());
        s.card_slots_mode = rack.is_card_ports();
        s
    }

    pub fn new(mode: SaveSelectMode, slots: Vec<SlotSnapshot>) -> Self {
        let phase = if slots.is_empty() {
            SelectPhase::Done(SelectOutcome::Cancelled)
        } else {
            SelectPhase::Browsing { cursor: 0 }
        };
        Self {
            mode,
            slots,
            phase,
            now_checking_frames: DEFAULT_NOW_CHECKING_FRAMES,
            slide_anim_t: 0,
            info_panel_slide_anim_t: 0,
            card_slots_mode: false,
            card_events: [false; CARD_STATUS_EVENTS],
            card_poll_counter: 0,
        }
    }

    /// Build a session whose slot list comes straight off a memory
    /// card's directory.
    ///
    /// Chains the three retail pieces the way retail chains them:
    /// [`card_directory_scan`] fills the fifteen-entry table and counts
    /// the files, [`card_free_blocks`] turns the summed file sizes into
    /// a free-block budget, and [`classify_card_directory`] spends that
    /// budget deciding which unmatched blocks are genuinely free rather
    /// than merely unreadable. The negative free-block count retail can
    /// return for an over-full card is floored at zero here, since it is
    /// consumed as a budget.
    pub fn from_card_directory(mode: SaveSelectMode, entries: &[CardDirEntry]) -> Self {
        let (table, count) = card_directory_scan(entries);
        let avail = card_free_blocks(&table, count).max(0) as u32;
        let names: Vec<&[u8]> = table[..count].iter().map(|e| e.name.as_slice()).collect();
        Self::new(mode, card_directory_slots(&names, avail))
    }

    /// Latch this frame's four memory-card kernel events.
    ///
    /// A host driving real card hardware sets these before each
    /// [`Self::tick`]; the `NowChecking` beat then ends as soon as the
    /// card reports rather than sitting out its full frame count, and a
    /// missing card fails the beat instead of silently succeeding. Hosts
    /// with no card reader leave them alone.
    pub fn set_card_events(&mut self, events: [bool; CARD_STATUS_EVENTS]) {
        self.card_events = events;
    }

    /// The events last latched by [`Self::set_card_events`].
    pub fn card_events(&self) -> [bool; CARD_STATUS_EVENTS] {
        self.card_events
    }

    /// The retail two-stage memory-card flow: Save mode crosses the
    /// `NowChecking` card-read beat and lands in
    /// [`SelectPhase::SlotPreview`], where the host renders the chosen
    /// card's block grid, and the overwrite prompt fires from the preview
    /// rather than from the pill row.
    ///
    /// **Hosts do not call this** - they build a [`SaveRack`] and let
    /// [`Self::for_rack`] derive the flag, so the two hosts cannot make the
    /// call differently. It stays public for tests that want a session in a
    /// given mode without constructing a rack around it.
    pub fn set_card_slots_mode(&mut self, on: bool) {
        self.card_slots_mode = on;
    }

    /// `true` when the two-stage memory-card flow is enabled.
    pub fn card_slots_mode(&self) -> bool {
        self.card_slots_mode
    }

    /// Current slide-in animation t (12-bit fixed-point 0..=4096).
    /// Render code interpolates `pos = start + (target - start) * t /
    /// 4096`. Returns 0 outside the Load-active phases (Browsing /
    /// Done). See [`SLIDE_ANIM_RATE`] for the ramp rate and
    /// [`SLIDE_ANIM_FULL`] for the fully-arrived sentinel.
    pub fn slide_anim_t(&self) -> u16 {
        self.slide_anim_t
    }

    /// Current info-panel slide-in animation t (12-bit fixed-point
    /// 0..=4096). Renderer uses this to interpolate the panel's
    /// y-origin between [`INFO_PANEL_OFFSCREEN_Y`] (off-screen below
    /// stage, t=0) and [`INFO_PANEL_PARKED_Y`] (parked under load
    /// chrome, t=4096) via [`interpolate_anim`]. Mirrors retail's
    /// `DAT_801ef1a0` which is held to 0 by `FUN_801DD35C` until the
    /// NowChecking dialog completes, then ramps during SlotPreview /
    /// Confirm phases.
    pub fn info_panel_slide_anim_t(&self) -> u16 {
        self.info_panel_slide_anim_t
    }

    /// Override the "Now checking" dialog duration (frames @ 60 Hz).
    /// Tests use this to bypass the 2-second beat.
    pub fn set_now_checking_frames(&mut self, frames: u16) {
        self.now_checking_frames = frames;
    }

    /// Read-only accessor used by render code to compute the dialog's
    /// dwell percentage.
    pub fn now_checking_frames(&self) -> u16 {
        self.now_checking_frames
    }

    pub fn mode(&self) -> SaveSelectMode {
        self.mode
    }

    pub fn slots(&self) -> &[SlotSnapshot] {
        &self.slots
    }

    pub fn phase(&self) -> SelectPhase {
        self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, SelectPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<SelectOutcome> {
        match self.phase {
            SelectPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    /// Index of the slot the cursor is pointing at, regardless of phase.
    pub fn current_slot(&self) -> u8 {
        match self.phase {
            SelectPhase::Browsing { cursor } => cursor,
            SelectPhase::NowChecking { slot, .. } | SelectPhase::SlotPreview { slot } => slot,
            SelectPhase::ConfirmOverwrite { slot, .. }
            | SelectPhase::ConfirmDelete { slot, .. } => slot,
            SelectPhase::Done(_) => 0,
        }
    }

    pub(super) fn slot_at(&self, idx: u8) -> Option<&SlotSnapshot> {
        self.slots.get(idx as usize)
    }

    pub fn tick(&mut self, input: SelectInput) -> Vec<SelectEvent> {
        let mut events = Vec::new();
        match self.phase {
            SelectPhase::Browsing { cursor } => self.tick_browsing(cursor, input, &mut events),
            SelectPhase::NowChecking {
                slot,
                frames_remaining,
            } => {
                self.tick_now_checking(slot, frames_remaining, &mut events);
            }
            SelectPhase::SlotPreview { slot } => {
                self.tick_slot_preview(slot, input, &mut events);
            }
            SelectPhase::ConfirmOverwrite { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Overwrite, slot, cursor, input, &mut events);
            }
            SelectPhase::ConfirmDelete { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Delete, slot, cursor, input, &mut events);
            }
            SelectPhase::Done(_) => {}
        }
        self.advance_slide_anim();
        events
    }

    /// Ramps `slide_anim_t` + `info_panel_slide_anim_t` based on the
    /// current phase.
    ///
    /// * `slide_anim_t` (slot composite pill + NowChecking dialog):
    ///   holds at 0 during Browsing/Done; ramps during NowChecking /
    ///   SlotPreview / Confirm.
    /// * `info_panel_slide_anim_t` (bottom info panel): holds at 0
    ///   during Browsing / NowChecking / Done so the panel stays
    ///   off-screen while the "checking" beat runs; ramps during
    ///   SlotPreview / Confirm so the panel slides up only after the
    ///   dialog dismisses. Mirrors retail's two-stage flow where
    ///   `DAT_801ef1a0` only starts incrementing once `DAT_801ef160`
    ///   (NowChecking) has retracted.
    pub(super) fn advance_slide_anim(&mut self) {
        match self.phase {
            SelectPhase::Browsing { .. } | SelectPhase::Done(_) => {
                self.slide_anim_t = 0;
                self.info_panel_slide_anim_t = 0;
            }
            SelectPhase::NowChecking { .. } => {
                self.slide_anim_t = self.slide_anim_t.saturating_add(SLIDE_ANIM_RATE);
                if self.slide_anim_t > SLIDE_ANIM_FULL {
                    self.slide_anim_t = SLIDE_ANIM_FULL;
                }
                self.info_panel_slide_anim_t = 0;
            }
            SelectPhase::SlotPreview { .. }
            | SelectPhase::ConfirmOverwrite { .. }
            | SelectPhase::ConfirmDelete { .. } => {
                self.slide_anim_t = self.slide_anim_t.saturating_add(SLIDE_ANIM_RATE);
                if self.slide_anim_t > SLIDE_ANIM_FULL {
                    self.slide_anim_t = SLIDE_ANIM_FULL;
                }
                self.info_panel_slide_anim_t =
                    self.info_panel_slide_anim_t.saturating_add(SLIDE_ANIM_RATE);
                if self.info_panel_slide_anim_t > SLIDE_ANIM_FULL {
                    self.info_panel_slide_anim_t = SLIDE_ANIM_FULL;
                }
            }
        }
    }

    pub(super) fn tick_browsing(
        &mut self,
        cursor: u8,
        input: SelectInput,
        events: &mut Vec<SelectEvent>,
    ) {
        if input.circle {
            self.phase = SelectPhase::Done(SelectOutcome::Cancelled);
            events.push(SelectEvent::Cancelled);
            return;
        }
        if input.up {
            let new = self.step(cursor, -1);
            if new != cursor {
                self.phase = SelectPhase::Browsing { cursor: new };
                events.push(SelectEvent::CursorMoved { slot: new });
            }
            return;
        }
        if input.down {
            let new = self.step(cursor, 1);
            if new != cursor {
                self.phase = SelectPhase::Browsing { cursor: new };
                events.push(SelectEvent::CursorMoved { slot: new });
            }
            return;
        }
        if input.cross {
            let snap = match self.slot_at(cursor) {
                Some(s) => s.clone(),
                None => return,
            };
            match (self.mode, snap.present) {
                (SaveSelectMode::Load, false) => {
                    events.push(SelectEvent::InvalidConfirm);
                }
                (SaveSelectMode::Load, true) => {
                    // Retail: pressing X on a non-empty slot opens the
                    // "Now checking. Do not remove MEMORY CARD" dialog
                    // for ~2 seconds, then transitions to the slot
                    // preview (portrait grid + info panel).
                    // Retail clears `DAT_801EF17C` in `FUN_801E3294`'s
                    // state 0, on the way into the poll.
                    self.card_poll_counter = 0;
                    self.phase = SelectPhase::NowChecking {
                        slot: cursor,
                        frames_remaining: self.now_checking_frames,
                    };
                    events.push(SelectEvent::EnteredNowChecking { slot: cursor });
                }
                // Card-slots mode: a Save picks a *card*, not a block, so
                // it crosses the same "Now checking" card-read beat Load
                // does and lands in SlotPreview for the host to draw the
                // card's block grid. `present` here means "a card is in
                // this slot" - an empty slot is nothing to save into.
                (SaveSelectMode::Save, true) if self.card_slots_mode => {
                    // Retail clears `DAT_801EF17C` in `FUN_801E3294`'s
                    // state 0, on the way into the poll.
                    self.card_poll_counter = 0;
                    self.phase = SelectPhase::NowChecking {
                        slot: cursor,
                        frames_remaining: self.now_checking_frames,
                    };
                    events.push(SelectEvent::EnteredNowChecking { slot: cursor });
                }
                (SaveSelectMode::Save, false) if self.card_slots_mode => {
                    events.push(SelectEvent::InvalidConfirm);
                }
                (SaveSelectMode::Save, true) => {
                    self.phase = SelectPhase::ConfirmOverwrite {
                        slot: cursor,
                        cursor: 1, // default to "No" for safety
                    };
                    events.push(SelectEvent::EnteredConfirm {
                        slot: cursor,
                        kind: ConfirmKind::Overwrite,
                    });
                }
                (SaveSelectMode::Save, false) => {
                    // Empty slot - go straight to "Saved" outcome (no
                    // destructive prompt needed).
                    self.phase = SelectPhase::Done(SelectOutcome::Saved(cursor));
                    events.push(SelectEvent::Confirmed {
                        slot: cursor,
                        kind: ConfirmKind::Overwrite,
                    });
                }
            }
            return;
        }
        if input.triangle && self.mode == SaveSelectMode::Save {
            // Triangle = delete shortcut on the save screen.
            if let Some(s) = self.slot_at(cursor)
                && s.present
            {
                self.phase = SelectPhase::ConfirmDelete {
                    slot: cursor,
                    cursor: 1,
                };
                events.push(SelectEvent::EnteredConfirm {
                    slot: cursor,
                    kind: ConfirmKind::Delete,
                });
            }
        }
    }

    /// Run one frame of the "Now checking" card-read beat.
    ///
    /// Retail does not sit on a fixed timer here: `FUN_801E3294`'s state
    /// 1 polls the card every frame through `FUN_801E3900` and branches
    /// on what comes back. That poll is [`card_status_poll`], and it runs
    /// on every frame of this phase.
    ///
    /// * [`CardStatus::Ready`] / [`CardStatus::Complete`] - the card
    ///   answered, so the beat ends early and the slot preview opens.
    /// * [`CardStatus::NoCard`] - retail prints "NOT CARD" and fails the
    ///   read; the session drops back to browsing with
    ///   [`SelectEvent::CardReadFailed`].
    /// * [`CardStatus::Pending`] / [`CardStatus::Aborted`] - nothing
    ///   conclusive, so the frame countdown decides. With no card events
    ///   latched (the default) this is the only path taken, and the beat
    ///   lasts exactly [`Self::now_checking_frames`] frames.
    ///
    /// The poll models retail's four `TestEvent` calls, and `TestEvent`
    /// *consumes* the event it tests - so the latched flags are cleared
    /// through [`card_events_drain`] once the poll has read them. Without
    /// that, a host that latches an event once would keep re-reporting it
    /// on every later frame of every later beat.
    pub(super) fn tick_now_checking(
        &mut self,
        slot: u8,
        frames_remaining: u16,
        events: &mut Vec<SelectEvent>,
    ) {
        let mut counter = self.card_poll_counter;
        let status = card_status_poll(self.card_events, &mut counter);
        self.card_poll_counter = counter;
        card_events_drain(&mut self.card_events);

        match status {
            CardStatus::Ready | CardStatus::Complete => {
                self.phase = SelectPhase::SlotPreview { slot };
                events.push(SelectEvent::EnteredSlotPreview { slot });
                return;
            }
            CardStatus::NoCard => {
                self.phase = SelectPhase::Browsing { cursor: slot };
                events.push(SelectEvent::CardReadFailed { slot });
                return;
            }
            CardStatus::Pending | CardStatus::Aborted => {}
        }

        if frames_remaining == 0 {
            self.phase = SelectPhase::SlotPreview { slot };
            events.push(SelectEvent::EnteredSlotPreview { slot });
        } else {
            self.phase = SelectPhase::NowChecking {
                slot,
                frames_remaining: frames_remaining - 1,
            };
        }
    }

    pub(super) fn tick_slot_preview(
        &mut self,
        slot: u8,
        input: SelectInput,
        events: &mut Vec<SelectEvent>,
    ) {
        if input.circle {
            self.phase = SelectPhase::Browsing { cursor: slot };
            events.push(SelectEvent::SlotPreviewCancelled { slot });
            return;
        }
        if input.cross {
            // Save mode reaches the preview only in card-slots mode (the
            // host is showing the card's block grid). Confirming there is
            // a destructive write, so it lands on the overwrite prompt
            // rather than committing - retail's "Do you wish to save?".
            if self.mode == SaveSelectMode::Save {
                self.phase = SelectPhase::ConfirmOverwrite {
                    slot,
                    cursor: 1, // default to "No" for safety
                };
                events.push(SelectEvent::EnteredConfirm {
                    slot,
                    kind: ConfirmKind::Overwrite,
                });
                return;
            }
            self.phase = SelectPhase::Done(SelectOutcome::Loaded(slot));
            events.push(SelectEvent::LoadConfirmed { slot });
        }
    }

    // REF: FUN_801d688c (the Yes/No confirm cursor - retail sub-screen 0x03
    // drives it with `FUN_801D688C(&DAT_801E46D0, 2, 1)`). A cross-reference,
    // not a port: the routine itself is `crate::menu_input` (`//! PORT:
    // FUN_801d688c`), whose `menu_cursor_nav` this calls. Here it advances the
    // 2-item horizontal cursor and reports confirm / cancel / move, and the
    // Yes/No branch is decided from the resulting cursor (retail return `1` =
    // the caller inspects the cursor to pick Yes vs No).
    pub(super) fn tick_confirm(
        &mut self,
        kind: ConfirmKind,
        slot: u8,
        cursor: u8,
        input: SelectInput,
        events: &mut Vec<SelectEvent>,
    ) {
        let mut cell = cursor as u32;
        let buttons = NavButtons {
            confirm: input.cross,
            cancel: input.circle,
            // The Yes/No prompt is horizontal; accept the vertical d-pad too
            // so the toggle works with either axis (equivalent to a 2-item
            // wrap either way).
            left: input.left || input.up,
            right: input.right || input.down,
        };
        // Backing out of the prompt returns where it was opened from: the
        // pill row in the flat model, but the card's block grid in
        // card-slots mode (the overwrite prompt is raised from the
        // preview there, so "No" must not eject the player to the pills).
        let back = if self.card_slots_mode && kind == ConfirmKind::Overwrite {
            SelectPhase::SlotPreview { slot }
        } else {
            SelectPhase::Browsing { cursor: slot }
        };
        match menu_cursor_nav(&mut cell, 2, true, buttons) {
            CursorNav::Cancel => {
                self.phase = back;
                events.push(SelectEvent::ConfirmCancelled { slot, kind });
            }
            CursorNav::Moved => {
                let new_cursor = (cell & CURSOR_INDEX_MASK) as u8;
                self.phase = match kind {
                    ConfirmKind::Overwrite => SelectPhase::ConfirmOverwrite {
                        slot,
                        cursor: new_cursor,
                    },
                    ConfirmKind::Delete => SelectPhase::ConfirmDelete {
                        slot,
                        cursor: new_cursor,
                    },
                };
                events.push(SelectEvent::CursorMoved { slot });
            }
            CursorNav::Confirm => {
                if cursor == 0 {
                    // Yes
                    let outcome = match kind {
                        ConfirmKind::Overwrite => SelectOutcome::Saved(slot),
                        ConfirmKind::Delete => SelectOutcome::Deleted(slot),
                    };
                    self.phase = SelectPhase::Done(outcome);
                    events.push(SelectEvent::Confirmed { slot, kind });
                } else {
                    // No → back where the prompt was opened from.
                    self.phase = back;
                    events.push(SelectEvent::ConfirmCancelled { slot, kind });
                }
            }
            CursorNav::None => {}
        }
    }

    /// Interpolate between `(start, target)` using `slide_anim_t()` as
    /// the 12-bit fixed-point t. Mirrors retail's
    /// `pos = start + (target - start) * t / 4096` math from
    /// `FUN_801E1C1C`. At t=0 returns `start`; at t=4096 returns
    /// `target`. Render code uses this to slide UI elements into
    /// place.
    pub fn interpolate(&self, start: (i32, i32), target: (i32, i32)) -> (i32, i32) {
        interpolate_anim(start, target, self.slide_anim_t)
    }

    pub(super) fn step(&self, from: u8, dir: i8) -> u8 {
        let n = self.slots.len() as i16;
        if n == 0 {
            return from;
        }
        let mut cur = from as i16;
        cur = (cur + dir as i16).rem_euclid(n);
        cur as u8
    }
}
