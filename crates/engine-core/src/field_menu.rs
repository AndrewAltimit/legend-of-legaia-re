//! Field menu (pause menu) state machine.
//!
//! The retail "Start in field" pause menu: a vertical list of seven rows
//! in the retail order **Items / Magic / Equip / Status / Options / Load /
//! Save** (the id-50 command-list renderer `FUN_801CFD68` draws exactly
//! these labels at `WY + n*0xe`) plus a return-to-game cancel path. Each
//! row hands off to a sub-session already shipped in this crate (or via
//! the boot-UI dispatch in the shell):
//!
//! - **Items** → [`crate::inventory_use::InventoryUseSession`] in field context.
//! - **Magic** → [`crate::spell_menu::SpellMenuSession`].
//! - **Equip** → [`crate::equip_session::EquipSession`].
//! - **Status** → [`crate::status_screen::StatusScreenSession`].
//! - **Options** → [`crate::options::OptionsSession`].
//! - **Load** → [`crate::save_select::SaveSelectSession`] in Load mode.
//! - **Save** → [`crate::save_select::SaveSelectSession`] in Save mode.
//!
//! The engine's Tactical Arts chain editor
//! ([`crate::tactical_arts_editor::ChainEditor`]) is an engine extension
//! with no retail pause-menu row; it stays reachable through the
//! dedicated arts session commands.
//!
//! Renderer-agnostic. Engines drive [`FieldMenuSession::tick`] each frame
//! with a [`FieldMenuInput`] bundle and consume the returned
//! [`FieldMenuEvent`] stream. The session emits an [`FieldMenuOutcome`] on
//! Done - the shell's job is to push the matching sub-session, then call
//! [`FieldMenuSession::resume`] when control returns.

/// One menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuRow {
    Items,
    Magic,
    Equip,
    Status,
    Options,
    Load,
    Save,
}

impl FieldMenuRow {
    /// Retail row order (`FUN_801CFD68` draw order, top to bottom).
    pub const ALL: [Self; 7] = [
        Self::Items,
        Self::Magic,
        Self::Equip,
        Self::Status,
        Self::Options,
        Self::Load,
        Self::Save,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Items => "Items",
            Self::Magic => "Magic",
            Self::Equip => "Equip",
            Self::Status => "Status",
            Self::Options => "Options",
            Self::Load => "Load",
            Self::Save => "Save",
        }
    }

    pub fn from_index(idx: u8) -> Option<Self> {
        Self::ALL.get(idx as usize).copied()
    }

    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|r| *r == self).unwrap() as u8
    }
}

/// Per-row enable/disable mask. Engines that have a save-blocked overlay
/// (e.g. cutscene playback) flip the matching row off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMenuRowMask(u8);

impl FieldMenuRowMask {
    pub const ALL_ENABLED: Self = Self(0x7F);

    pub fn new() -> Self {
        Self::ALL_ENABLED
    }

    pub fn enable(&mut self, row: FieldMenuRow) {
        self.0 |= 1 << row.index();
    }

    pub fn disable(&mut self, row: FieldMenuRow) {
        self.0 &= !(1 << row.index());
    }

    pub fn is_enabled(&self, row: FieldMenuRow) -> bool {
        (self.0 >> row.index()) & 1 == 1
    }
}

impl Default for FieldMenuRowMask {
    fn default() -> Self {
        Self::ALL_ENABLED
    }
}

/// Phase of the field-menu SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuPhase {
    /// Player is browsing the row list.
    Browsing { cursor: u8 },
    /// Player confirmed a row; shell pushes the sub-session and engines call
    /// [`FieldMenuSession::resume`] when control returns.
    Suspended { row: FieldMenuRow },
    /// Player cancelled out (Circle on Browsing) - shell closes the menu.
    Done(FieldMenuOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuOutcome {
    /// Player closed the menu without picking anything (or after the
    /// pushed sub-session finished). Shell returns to field tick.
    Closed,
    /// A row was confirmed and the shell wants the resolved row.
    Confirmed(FieldMenuRow),
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldMenuInput {
    pub up: bool,
    pub down: bool,
    pub cross: bool,
    pub circle: bool,
    pub start: bool,
}

/// Events emitted on `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuEvent {
    CursorMoved {
        row: u8,
    },
    Confirmed {
        row: FieldMenuRow,
    },
    InvalidConfirm {
        row: FieldMenuRow,
    },
    Cancelled,
    /// Shell is about to push the matching sub-session.
    EnteringSub {
        row: FieldMenuRow,
    },
    /// Sub-session finished and shell handed control back.
    Resumed {
        row: FieldMenuRow,
    },
}

/// Renderer-agnostic field-menu state machine.
#[derive(Debug, Clone)]
pub struct FieldMenuSession {
    phase: FieldMenuPhase,
    mask: FieldMenuRowMask,
    /// Optional gold count to display in the corner. Plain data - engines
    /// pass it through and the renderer view uses it.
    pub money: u32,
    /// Optional play-time-seconds for the corner badge.
    pub play_time_seconds: u32,
}

impl Default for FieldMenuSession {
    fn default() -> Self {
        Self::new()
    }
}

impl FieldMenuSession {
    pub fn new() -> Self {
        Self {
            phase: FieldMenuPhase::Browsing { cursor: 0 },
            mask: FieldMenuRowMask::ALL_ENABLED,
            money: 0,
            play_time_seconds: 0,
        }
    }

    pub fn with_mask(mask: FieldMenuRowMask) -> Self {
        let mut s = Self::new();
        s.mask = mask;
        // Make sure cursor lands on an enabled row.
        let first = s.first_enabled_row().index();
        if let FieldMenuPhase::Browsing { cursor } = &mut s.phase {
            *cursor = first;
        }
        s
    }

    pub fn phase(&self) -> FieldMenuPhase {
        self.phase
    }

    pub fn outcome(&self) -> Option<FieldMenuOutcome> {
        match self.phase {
            FieldMenuPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            FieldMenuPhase::Browsing { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn mask(&self) -> &FieldMenuRowMask {
        &self.mask
    }

    pub fn set_mask(&mut self, mask: FieldMenuRowMask) {
        self.mask = mask;
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, FieldMenuPhase::Done(_))
    }

    pub fn is_suspended(&self) -> bool {
        matches!(self.phase, FieldMenuPhase::Suspended { .. })
    }

    fn first_enabled_row(&self) -> FieldMenuRow {
        FieldMenuRow::ALL
            .iter()
            .copied()
            .find(|r| self.mask.is_enabled(*r))
            .unwrap_or(FieldMenuRow::Items)
    }

    fn next_enabled(&self, from: u8, dir: i8) -> u8 {
        let n = FieldMenuRow::ALL.len() as i8;
        let mut i = from as i8;
        for _ in 0..n {
            i = (i + dir).rem_euclid(n);
            if let Some(r) = FieldMenuRow::from_index(i as u8)
                && self.mask.is_enabled(r)
            {
                return i as u8;
            }
        }
        from
    }

    pub fn tick(&mut self, input: FieldMenuInput) -> Vec<FieldMenuEvent> {
        let mut events = Vec::new();
        match self.phase {
            FieldMenuPhase::Browsing { cursor } => {
                if input.circle || input.start {
                    self.phase = FieldMenuPhase::Done(FieldMenuOutcome::Closed);
                    events.push(FieldMenuEvent::Cancelled);
                    return events;
                }
                let mut new_cursor = cursor;
                if input.up {
                    new_cursor = self.next_enabled(cursor, -1);
                } else if input.down {
                    new_cursor = self.next_enabled(cursor, 1);
                }
                if new_cursor != cursor {
                    self.phase = FieldMenuPhase::Browsing { cursor: new_cursor };
                    events.push(FieldMenuEvent::CursorMoved { row: new_cursor });
                }
                if input.cross
                    && let Some(row) = FieldMenuRow::from_index(new_cursor)
                {
                    if self.mask.is_enabled(row) {
                        self.phase = FieldMenuPhase::Suspended { row };
                        events.push(FieldMenuEvent::Confirmed { row });
                        events.push(FieldMenuEvent::EnteringSub { row });
                    } else {
                        events.push(FieldMenuEvent::InvalidConfirm { row });
                    }
                }
            }
            FieldMenuPhase::Suspended { .. } => {
                // Wait for explicit `resume`/`finish`. Input drained.
            }
            FieldMenuPhase::Done(_) => {}
        }
        events
    }

    /// Sub-session finished and shell hands control back. The caller chooses
    /// whether to drop back into Browsing (the default - most sub-sessions
    /// are "do a thing then return to the menu") or close the menu entirely
    /// (e.g. Save → Continue, where the shell wants the field gameplay back).
    pub fn resume(&mut self, close: bool) -> Vec<FieldMenuEvent> {
        let mut events = Vec::new();
        if let FieldMenuPhase::Suspended { row } = self.phase {
            events.push(FieldMenuEvent::Resumed { row });
            if close {
                self.phase = FieldMenuPhase::Done(FieldMenuOutcome::Confirmed(row));
            } else {
                self.phase = FieldMenuPhase::Browsing {
                    cursor: row.index(),
                };
            }
        }
        events
    }
}

/// Plain-data view for the renderer. Engines call [`FieldMenuSession::view`]
/// once per frame and feed the result into `engine-render::field_menu_draws_for`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMenuView {
    pub rows: [FieldMenuRowView; 7],
    pub cursor: u8,
    pub money: u32,
    pub play_time_seconds: u32,
    pub suspended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMenuRowView {
    pub row: FieldMenuRow,
    pub label: &'static str,
    pub enabled: bool,
}

impl FieldMenuSession {
    pub fn view(&self) -> FieldMenuView {
        let mut rows = [FieldMenuRowView {
            row: FieldMenuRow::Items,
            label: "",
            enabled: false,
        }; 7];
        for (i, r) in FieldMenuRow::ALL.iter().enumerate() {
            rows[i] = FieldMenuRowView {
                row: *r,
                label: r.label(),
                enabled: self.mask.is_enabled(*r),
            };
        }
        FieldMenuView {
            rows,
            cursor: self.cursor(),
            money: self.money,
            play_time_seconds: self.play_time_seconds,
            suspended: self.is_suspended(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> FieldMenuInput {
        FieldMenuInput::default()
    }

    #[test]
    fn cursor_moves_on_down_with_event() {
        let mut s = FieldMenuSession::new();
        let evs = s.tick(FieldMenuInput {
            down: true,
            ..input()
        });
        assert_eq!(s.cursor(), 1);
        assert_eq!(evs, vec![FieldMenuEvent::CursorMoved { row: 1 }]);
    }

    #[test]
    fn up_wraps_to_last_row() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            up: true,
            ..input()
        });
        assert_eq!(s.cursor(), 6);
    }

    #[test]
    fn cross_confirms_to_suspended_with_row() {
        let mut s = FieldMenuSession::new();
        let evs = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        assert!(s.is_suspended());
        assert!(matches!(s.phase, FieldMenuPhase::Suspended { row } if row == FieldMenuRow::Items));
        assert!(evs.contains(&FieldMenuEvent::Confirmed {
            row: FieldMenuRow::Items
        }));
        assert!(evs.contains(&FieldMenuEvent::EnteringSub {
            row: FieldMenuRow::Items
        }));
    }

    #[test]
    fn circle_closes_menu() {
        let mut s = FieldMenuSession::new();
        let evs = s.tick(FieldMenuInput {
            circle: true,
            ..input()
        });
        assert!(s.is_done());
        assert_eq!(s.outcome(), Some(FieldMenuOutcome::Closed));
        assert!(evs.contains(&FieldMenuEvent::Cancelled));
    }

    #[test]
    fn disabled_row_skipped_on_cursor_move() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Magic);
        let mut s = FieldMenuSession::with_mask(mask);
        let _ = s.tick(FieldMenuInput {
            down: true,
            ..input()
        });
        assert_eq!(s.cursor(), FieldMenuRow::Equip.index());
    }

    #[test]
    fn invalid_confirm_on_disabled_does_not_change_phase() {
        // Manually craft an impossible state where cursor sits on a
        // disabled row to make sure InvalidConfirm fires in defence.
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Save);
        let mut s = FieldMenuSession::with_mask(mask);
        s.phase = FieldMenuPhase::Browsing {
            cursor: FieldMenuRow::Save.index(),
        };
        let evs = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        assert!(!s.is_suspended());
        assert!(evs.contains(&FieldMenuEvent::InvalidConfirm {
            row: FieldMenuRow::Save
        }));
    }

    #[test]
    fn resume_returns_to_browsing_by_default() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        let evs = s.resume(false);
        assert!(matches!(s.phase, FieldMenuPhase::Browsing { cursor } if cursor == 0));
        assert!(evs.contains(&FieldMenuEvent::Resumed {
            row: FieldMenuRow::Items
        }));
    }

    #[test]
    fn resume_with_close_closes_menu() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        let _ = s.resume(true);
        assert_eq!(
            s.outcome(),
            Some(FieldMenuOutcome::Confirmed(FieldMenuRow::Items))
        );
    }

    #[test]
    fn view_reflects_mask_and_cursor() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Save);
        let mut s = FieldMenuSession::with_mask(mask);
        s.money = 1234;
        s.play_time_seconds = 60;
        let v = s.view();
        assert_eq!(v.money, 1234);
        assert_eq!(v.play_time_seconds, 60);
        assert!(!v.rows[FieldMenuRow::Save.index() as usize].enabled);
        assert!(v.rows[FieldMenuRow::Items.index() as usize].enabled);
    }

    #[test]
    fn first_enabled_row_for_with_mask() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Items);
        let s = FieldMenuSession::with_mask(mask);
        assert_eq!(s.cursor(), FieldMenuRow::Magic.index());
    }

    #[test]
    fn start_also_closes_like_circle() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            start: true,
            ..input()
        });
        assert!(s.is_done());
    }
}
