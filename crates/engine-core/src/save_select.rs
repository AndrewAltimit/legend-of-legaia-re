//! Save-slot select session.
//!
//! PORT: FUN_801DD35C (save-UI dispatcher), FUN_801E08D8 (info-panel renderer), FUN_801E1C1C (slide-in animator)
//!
//! Drives the slot-list UI (read save metadata, browse, Load/Save/Delete
//! confirmations). Renderer-agnostic - engines render the slot list
//! against the existing text overlay; the session emits typed events for
//! engines to react to (cursor blip, confirm chime, etc.).
//!
//! Two operating modes:
//!
//! - [`SaveSelectMode::Load`] - pick a non-empty slot to load.
//! - [`SaveSelectMode::Save`] - pick any slot (empty or full) to write
//!   into. Picking a non-empty slot enters the Overwrite confirm prompt.
//!
//! ## States
//!
//! `Browsing → ConfirmLoad / ConfirmOverwrite / ConfirmDelete → Done`
//!
//! Engines call [`SaveSelectSession::tick`] each frame and react to
//! returned [`SelectEvent`]s. The session never reads the save data
//! itself - engines pre-load slot metadata into [`SlotSnapshot`] entries
//! and feed them through `set_slots`.

/// Per-slot metadata. Engines build these from disc/disk save scans
/// (the `legaia-save` crate provides the parsers). Pure data - the
/// session never touches the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSnapshot {
    pub slot: u8,
    pub present: bool,
    /// Display label engines render. "<empty>" for empty slots.
    pub label: String,
    /// Game time in seconds (for the "Play time: 12:34:56" line).
    pub play_time_seconds: u32,
    /// Party leader's level (for the "Lv. 23" badge).
    pub party_lv: u8,
    /// Map name where the save was written.
    pub location: String,
    /// In-game gold.
    pub money: u32,
    /// Lead character's roster index (0=Vahn, 1=Noa, 2=Gala). Used by
    /// the load-screen slot-preview to pick which 16×16 portrait
    /// sprite to render for this slot. Defaults to 0 (Vahn) which
    /// matches every retail Legaia save (the lead slot is always
    /// Vahn).
    pub leader_char_id: u8,
    /// Lead character's display name ("Vahn" / "Noa" / "Gala"...).
    pub leader_name: String,
    /// Lead character's current/max HP for the slot-preview info
    /// panel.
    pub leader_hp: (u16, u16),
    /// Lead character's current/max MP (a.k.a. WP) for the info
    /// panel.
    pub leader_mp: (u16, u16),
}

impl SlotSnapshot {
    pub fn empty(slot: u8) -> Self {
        Self {
            slot,
            present: false,
            label: format!("Slot {slot}: <empty>"),
            play_time_seconds: 0,
            party_lv: 0,
            location: String::new(),
            money: 0,
            leader_char_id: 0,
            leader_name: String::new(),
            leader_hp: (0, 0),
            leader_mp: (0, 0),
        }
    }

    /// Format play time as `HH:MM:SS`.
    pub fn play_time_string(&self) -> String {
        let secs = self.play_time_seconds;
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{h:02}:{m:02}:{s:02}")
    }
}

/// Save-select operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSelectMode {
    Load,
    Save,
}

/// Phase of the SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectPhase {
    Browsing {
        cursor: u8,
    },
    /// Retail "Now checking. Do not remove MEMORY CARD" dialog frame.
    /// Auto-advances to [`SlotPreview`] when `frames_remaining`
    /// counts down to 0; no input is accepted while in this phase.
    NowChecking {
        slot: u8,
        frames_remaining: u16,
    },
    /// 5×3 portrait grid + bottom info panel preview of `slot`. X
    /// confirms the load (→ `Done(Loaded(slot))`); Circle returns to
    /// `Browsing { cursor: slot }`.
    SlotPreview {
        slot: u8,
    },
    ConfirmOverwrite {
        slot: u8,
        cursor: u8,
    },
    ConfirmDelete {
        slot: u8,
        cursor: u8,
    },
    Done(SelectOutcome),
}

/// Final outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOutcome {
    Loaded(u8),
    Saved(u8),
    Deleted(u8),
    Cancelled,
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub triangle: bool,
}

/// Events emitted per `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectEvent {
    CursorMoved {
        slot: u8,
    },
    EnteredConfirm {
        slot: u8,
        kind: ConfirmKind,
    },
    /// User confirmed a destructive action.
    Confirmed {
        slot: u8,
        kind: ConfirmKind,
    },
    /// User cancelled out of a confirm prompt back to browsing.
    ConfirmCancelled {
        slot: u8,
        kind: ConfirmKind,
    },
    /// User picked an empty slot in Load mode (no-op blip).
    InvalidConfirm,
    /// User pressed X on a non-empty Load slot; "Now checking" dialog
    /// has been entered.
    EnteredNowChecking {
        slot: u8,
    },
    /// "Now checking" timer expired; slot-preview phase entered.
    EnteredSlotPreview {
        slot: u8,
    },
    /// User confirmed the load from the slot-preview screen (X on
    /// SlotPreview).
    LoadConfirmed {
        slot: u8,
    },
    /// User cancelled out of the slot-preview screen back to browsing.
    SlotPreviewCancelled {
        slot: u8,
    },
    /// Whole session cancelled.
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    Overwrite,
    Delete,
}

/// Default duration of the "Now checking" dialog in frames @ 60 Hz.
/// Two seconds of game time mirrors the retail memory-card scan beat.
pub const DEFAULT_NOW_CHECKING_FRAMES: u16 = 120;

/// Slide-in animation timer rate, in 12-bit fixed-point units per tick.
/// Mirrors retail's `DAT_801ef194 += DAT_1f800393 * 0x100` increment in
/// the save-UI dispatcher: 256/frame, clamped at 4096. See Ghidra
/// trace in `overlay_save_ui_select_801dd35c.txt` lines ~4246.
pub const SLIDE_ANIM_RATE: u16 = 256;
/// Fully-arrived sentinel for the slide-in timer (12-bit fixed-point
/// 1.0). Retail uses `0x1000` everywhere as the clamp ceiling and the
/// "at target" marker.
pub const SLIDE_ANIM_FULL: u16 = 0x1000;

/// Off-screen-below y-origin of the bottom info panel (retail
/// `0x18A = 394`). Drives the `t=0` end of the info-panel slide-in
/// interpolation. Pinned from `FUN_801E08D8`'s entry math:
/// `local_34 = (anim_t * -0x100) / 0xfff >> 12 + 0x18A`.
pub const INFO_PANEL_OFFSCREEN_Y: i32 = 394;
/// Parked y-origin of the bottom info panel (retail value, derived as
/// `0x18A - 0x100 = 138` when `anim_t = 0x1000`). The 9-slice chrome's
/// top gold border lands on this y; matches the existing
/// `SLOT_INFO_PANEL_POS.1` chrome scan.
pub const INFO_PANEL_PARKED_Y: i32 = 138;

/// Interpolate between `(start, target)` using a 12-bit fixed-point
/// `t` in `[0, SLIDE_ANIM_FULL]`. Mirrors retail's
/// `pos = start + (target - start) * t / 4096` math from
/// `FUN_801E1C1C`. Free function for callers without a session
/// handle; the [`SaveSelectSession::interpolate`] method forwards
/// here using `self.slide_anim_t()`.
pub fn interpolate_anim(start: (i32, i32), target: (i32, i32), t: u16) -> (i32, i32) {
    let t = t as i32;
    let denom = SLIDE_ANIM_FULL as i32;
    let dx = (target.0 - start.0) * t / denom;
    let dy = (target.1 - start.1) * t / denom;
    (start.0 + dx, start.1 + dy)
}

/// Save-select session state machine.
#[derive(Debug, Clone)]
pub struct SaveSelectSession {
    mode: SaveSelectMode,
    slots: Vec<SlotSnapshot>,
    phase: SelectPhase,
    /// Frames the "Now checking" dialog stays visible after the user
    /// confirms a Load on a non-empty slot. Public knob so tests can
    /// fast-forward through the dialog without ticking 120 times.
    now_checking_frames: u16,
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
    slide_anim_t: u16,
    /// 12-bit fixed-point slide-in timer for the bottom info panel.
    /// Mirrors retail's `DAT_801ef1a0` in `FUN_801E08D8`, which is
    /// distinct from the slot-composite timer above: the info panel
    /// starts hidden (off-screen below the stage at y=394) and slides
    /// up to its parked position (y=138) only AFTER NowChecking
    /// completes. Holds at 0 during Browsing / NowChecking / Done;
    /// ramps during SlotPreview / ConfirmOverwrite / ConfirmDelete.
    info_panel_slide_anim_t: u16,
}

impl SaveSelectSession {
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
        }
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

    fn slot_at(&self, idx: u8) -> Option<&SlotSnapshot> {
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
    fn advance_slide_anim(&mut self) {
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

    fn tick_browsing(&mut self, cursor: u8, input: SelectInput, events: &mut Vec<SelectEvent>) {
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
                    self.phase = SelectPhase::NowChecking {
                        slot: cursor,
                        frames_remaining: self.now_checking_frames,
                    };
                    events.push(SelectEvent::EnteredNowChecking { slot: cursor });
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

    fn tick_now_checking(
        &mut self,
        slot: u8,
        frames_remaining: u16,
        events: &mut Vec<SelectEvent>,
    ) {
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

    fn tick_slot_preview(&mut self, slot: u8, input: SelectInput, events: &mut Vec<SelectEvent>) {
        if input.circle {
            self.phase = SelectPhase::Browsing { cursor: slot };
            events.push(SelectEvent::SlotPreviewCancelled { slot });
            return;
        }
        if input.cross {
            self.phase = SelectPhase::Done(SelectOutcome::Loaded(slot));
            events.push(SelectEvent::LoadConfirmed { slot });
        }
    }

    fn tick_confirm(
        &mut self,
        kind: ConfirmKind,
        slot: u8,
        cursor: u8,
        input: SelectInput,
        events: &mut Vec<SelectEvent>,
    ) {
        if input.circle {
            self.phase = SelectPhase::Browsing { cursor: slot };
            events.push(SelectEvent::ConfirmCancelled { slot, kind });
            return;
        }
        let new_cursor = if input.up || input.down || input.left || input.right {
            cursor ^ 1
        } else {
            cursor
        };
        if new_cursor != cursor {
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
            return;
        }
        if input.cross {
            if cursor == 0 {
                // Yes
                let outcome = match kind {
                    ConfirmKind::Overwrite => SelectOutcome::Saved(slot),
                    ConfirmKind::Delete => SelectOutcome::Deleted(slot),
                };
                self.phase = SelectPhase::Done(outcome);
                events.push(SelectEvent::Confirmed { slot, kind });
            } else {
                // No → back to browsing.
                self.phase = SelectPhase::Browsing { cursor: slot };
                events.push(SelectEvent::ConfirmCancelled { slot, kind });
            }
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

    fn step(&self, from: u8, dir: i8) -> u8 {
        let n = self.slots.len() as i16;
        if n == 0 {
            return from;
        }
        let mut cur = from as i16;
        cur = (cur + dir as i16).rem_euclid(n);
        cur as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(present_set: &[bool]) -> Vec<SlotSnapshot> {
        present_set
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if *p {
                    SlotSnapshot {
                        slot: i as u8,
                        present: true,
                        label: format!("Slot {i}"),
                        play_time_seconds: 1234,
                        party_lv: 5,
                        location: "Town01".into(),
                        money: 100,
                        leader_char_id: 0,
                        leader_name: "Vahn".into(),
                        leader_hp: (100, 100),
                        leader_mp: (20, 20),
                    }
                } else {
                    SlotSnapshot::empty(i as u8)
                }
            })
            .collect()
    }

    #[test]
    fn empty_slots_session_done_immediately() {
        let s = SaveSelectSession::new(SaveSelectMode::Load, vec![]);
        assert!(s.is_done());
        assert_eq!(s.outcome(), Some(SelectOutcome::Cancelled));
    }

    #[test]
    fn load_empty_slot_invalid_confirm() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[false; 3]));
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert!(events.contains(&SelectEvent::InvalidConfirm));
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
    }

    #[test]
    fn load_full_slot_runs_now_checking_then_preview_then_load() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        // Use a short timer so the test doesn't loop 120 times.
        s.set_now_checking_frames(2);
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // Should enter NowChecking with the configured frame count.
        match s.phase() {
            SelectPhase::NowChecking {
                slot: 0,
                frames_remaining: 2,
            } => {}
            other => panic!("expected NowChecking, got {other:?}"),
        }
        assert!(events.contains(&SelectEvent::EnteredNowChecking { slot: 0 }));
        // Tick 3 times: counts down 2→1→0→SlotPreview.
        s.tick(SelectInput::default());
        s.tick(SelectInput::default());
        let events = s.tick(SelectInput::default());
        match s.phase() {
            SelectPhase::SlotPreview { slot: 0 } => {}
            other => panic!("expected SlotPreview, got {other:?}"),
        }
        assert!(events.contains(&SelectEvent::EnteredSlotPreview { slot: 0 }));
        // X on preview confirms load.
        let events = s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
        assert!(events.contains(&SelectEvent::LoadConfirmed { slot: 0 }));
    }

    #[test]
    fn now_checking_ignores_input() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(5);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // Pressing X again while in NowChecking is a no-op (just ticks down).
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::NowChecking {
                frames_remaining: 4,
                ..
            } => {}
            other => panic!("input must not skip NowChecking; got {other:?}"),
        }
    }

    #[test]
    fn slot_preview_circle_returns_to_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // Frames=0 → first NowChecking tick advances to SlotPreview.
        s.tick(SelectInput::default());
        match s.phase() {
            SelectPhase::SlotPreview { slot: 0 } => {}
            other => panic!("expected SlotPreview, got {other:?}"),
        }
        let events = s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::Browsing { cursor: 0 } => {}
            other => panic!("expected Browsing, got {other:?}"),
        }
        assert!(events.contains(&SelectEvent::SlotPreviewCancelled { slot: 0 }));
    }

    #[test]
    fn save_overwrite_default_cursor_is_no() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmOverwrite { cursor: 1, .. } => {}
            _ => panic!("default cursor should be 'No'"),
        }
    }

    #[test]
    fn overwrite_cursor_toggles_with_directions() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // cursor at 1 (No)
        s.tick(SelectInput {
            left: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmOverwrite { cursor: 0, .. } => {}
            _ => panic!(),
        }
        // Confirm Yes.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
    }

    #[test]
    fn save_into_empty_slot_goes_directly_to_saved() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[false, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Saved(0)));
    }

    #[test]
    fn cursor_wraps() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[false, false, false]));
        s.tick(SelectInput {
            up: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::Browsing { cursor: 2 } => {}
            other => panic!("up from 0 should wrap to 2; got {other:?}"),
        }
    }

    #[test]
    fn circle_cancels_session() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Cancelled));
    }

    #[test]
    fn circle_in_save_confirm_returns_to_browse() {
        // Save-mode ConfirmOverwrite still keeps the back-on-Circle
        // behavior (NowChecking only fires in Load mode).
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::Browsing { cursor: 0 } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn delete_shortcut_in_save_mode() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            triangle: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmDelete { .. } => {}
            other => panic!("expected ConfirmDelete, got {other:?}"),
        }
    }

    #[test]
    fn delete_yes_emits_deleted_outcome() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Save, slots(&[true, false, false]));
        s.tick(SelectInput {
            triangle: true,
            ..Default::default()
        });
        // cursor = 1 (No) - switch to Yes.
        s.tick(SelectInput {
            left: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Deleted(0)));
    }

    #[test]
    fn play_time_string_format() {
        let mut snap = SlotSnapshot::empty(0);
        snap.play_time_seconds = 3 * 3600 + 25 * 60 + 7;
        assert_eq!(snap.play_time_string(), "03:25:07");
    }

    #[test]
    fn empty_snapshot_label_includes_empty() {
        let s = SlotSnapshot::empty(2);
        assert!(s.label.contains("empty"));
        assert!(!s.present);
    }

    #[test]
    fn slide_anim_holds_at_zero_while_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        assert_eq!(s.slide_anim_t(), 0);
        for _ in 0..32 {
            s.tick(SelectInput::default());
            assert_eq!(s.slide_anim_t(), 0, "should stay 0 while Browsing");
        }
    }

    #[test]
    fn slide_anim_ramps_in_now_checking_then_clamps() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(120);
        // Enter NowChecking.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        // The cross-press tick already entered NowChecking and
        // advance_slide_anim ran once -> t=256.
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_RATE);
        // Tick 15 more times: t goes 256 -> 512 -> ... -> 4096.
        for i in 2..=16 {
            s.tick(SelectInput::default());
            let expected = (SLIDE_ANIM_RATE as u32 * i).min(SLIDE_ANIM_FULL as u32) as u16;
            assert_eq!(
                s.slide_anim_t(),
                expected,
                "frame {i}: expected {expected}, got {}",
                s.slide_anim_t()
            );
        }
        // Should now be clamped at 4096.
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL);
        s.tick(SelectInput::default());
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL, "stays clamped");
    }

    #[test]
    fn slide_anim_resets_on_cancel_back_to_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        // Enter NowChecking (cross), then NowChecking auto-advances
        // to SlotPreview on the next tick because frames_remaining=0.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
        // Slide should have advanced.
        assert!(s.slide_anim_t() > 0);
        // Cancel back to Browsing.
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
        assert_eq!(s.slide_anim_t(), 0, "must reset on cancel");
    }

    #[test]
    fn interpolate_endpoints_match_start_and_target() {
        let start = (160, 96);
        let target = (48, 40);
        assert_eq!(interpolate_anim(start, target, 0), start);
        assert_eq!(interpolate_anim(start, target, SLIDE_ANIM_FULL), target);
        // Midpoint: half-way between (160, 96) and (48, 40) is
        // (104, 68). Integer division truncates toward zero, so
        // verify the exact rounded value.
        let mid = interpolate_anim(start, target, SLIDE_ANIM_FULL / 2);
        assert_eq!(mid, (104, 68));
    }

    #[test]
    fn info_panel_slide_holds_at_zero_in_browsing_and_now_checking() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(120);
        // Browsing.
        for _ in 0..8 {
            s.tick(SelectInput::default());
            assert_eq!(s.info_panel_slide_anim_t(), 0);
        }
        // Enter NowChecking; the info panel still holds at 0 here so
        // the panel stays hidden while the dialog plays.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..8 {
            s.tick(SelectInput::default());
            assert_eq!(
                s.info_panel_slide_anim_t(),
                0,
                "info panel must stay hidden during NowChecking"
            );
        }
    }

    #[test]
    fn info_panel_slide_ramps_in_slot_preview_then_clamps() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        // Enter NowChecking, auto-advance to SlotPreview because
        // frames_remaining = 0.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
        // First SlotPreview tick already advanced once.
        let mut last = s.info_panel_slide_anim_t();
        assert_eq!(last, SLIDE_ANIM_RATE);
        // Ramp to clamp.
        for _ in 0..32 {
            s.tick(SelectInput::default());
            let now = s.info_panel_slide_anim_t();
            assert!(now >= last, "monotonic non-decreasing");
            assert!(now <= SLIDE_ANIM_FULL, "never exceeds clamp");
            last = now;
        }
        assert_eq!(last, SLIDE_ANIM_FULL);
    }

    #[test]
    fn info_panel_slide_resets_on_cancel_back_to_browsing() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(0);
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        s.tick(SelectInput::default());
        // SlotPreview reached; ramp a few frames.
        for _ in 0..4 {
            s.tick(SelectInput::default());
        }
        assert!(s.info_panel_slide_anim_t() > 0);
        // Cancel back to Browsing.
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
        assert_eq!(s.info_panel_slide_anim_t(), 0);
    }

    #[test]
    fn info_panel_offscreen_to_parked_interpolation() {
        // Endpoint check: anim_t=0 -> off-screen y=394; t=4096 -> parked y=138.
        let off = (0, INFO_PANEL_OFFSCREEN_Y);
        let park = (0, INFO_PANEL_PARKED_Y);
        assert_eq!(interpolate_anim(off, park, 0).1, INFO_PANEL_OFFSCREEN_Y);
        assert_eq!(
            interpolate_anim(off, park, SLIDE_ANIM_FULL).1,
            INFO_PANEL_PARKED_Y
        );
    }

    #[test]
    fn interpolate_method_uses_session_anim_t() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.set_now_checking_frames(120);
        // Browsing: t=0 -> returns start.
        assert_eq!(s.interpolate((100, 50), (200, 80)), (100, 50));
        // Enter NowChecking, then tick 16 frames to reach t=4096.
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..16 {
            s.tick(SelectInput::default());
        }
        assert_eq!(s.slide_anim_t(), SLIDE_ANIM_FULL);
        assert_eq!(s.interpolate((100, 50), (200, 80)), (200, 80));
    }
}
