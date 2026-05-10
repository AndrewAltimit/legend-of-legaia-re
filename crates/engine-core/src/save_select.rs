//! Save-slot select session.
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
    Browsing { cursor: u8 },
    ConfirmLoad { slot: u8, cursor: u8 },
    ConfirmOverwrite { slot: u8, cursor: u8 },
    ConfirmDelete { slot: u8, cursor: u8 },
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
    /// Whole session cancelled.
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    Load,
    Overwrite,
    Delete,
}

/// Save-select session state machine.
#[derive(Debug, Clone)]
pub struct SaveSelectSession {
    mode: SaveSelectMode,
    slots: Vec<SlotSnapshot>,
    phase: SelectPhase,
}

impl SaveSelectSession {
    pub fn new(mode: SaveSelectMode, slots: Vec<SlotSnapshot>) -> Self {
        let phase = if slots.is_empty() {
            SelectPhase::Done(SelectOutcome::Cancelled)
        } else {
            SelectPhase::Browsing { cursor: 0 }
        };
        Self { mode, slots, phase }
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
            SelectPhase::ConfirmLoad { slot, .. }
            | SelectPhase::ConfirmOverwrite { slot, .. }
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
            SelectPhase::ConfirmLoad { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Load, slot, cursor, input, &mut events);
            }
            SelectPhase::ConfirmOverwrite { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Overwrite, slot, cursor, input, &mut events);
            }
            SelectPhase::ConfirmDelete { slot, cursor } => {
                self.tick_confirm(ConfirmKind::Delete, slot, cursor, input, &mut events);
            }
            SelectPhase::Done(_) => {}
        }
        events
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
                    self.phase = SelectPhase::ConfirmLoad {
                        slot: cursor,
                        cursor: 0, // 0 = Yes
                    };
                    events.push(SelectEvent::EnteredConfirm {
                        slot: cursor,
                        kind: ConfirmKind::Load,
                    });
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
                ConfirmKind::Load => SelectPhase::ConfirmLoad {
                    slot,
                    cursor: new_cursor,
                },
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
                    ConfirmKind::Load => SelectOutcome::Loaded(slot),
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
    fn load_full_slot_enters_confirm_then_yes() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        match s.phase() {
            SelectPhase::ConfirmLoad { cursor: 0, .. } => {}
            other => panic!("expected ConfirmLoad, got {other:?}"),
        }
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(SelectOutcome::Loaded(0)));
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
    fn circle_in_confirm_returns_to_browse() {
        let mut s = SaveSelectSession::new(SaveSelectMode::Load, slots(&[true, false, false]));
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
}
