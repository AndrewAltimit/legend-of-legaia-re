//! Status screen session.
//!
//! Per-character stat detail screen: HP / MP / AP / level / XP / equipped
//! slots / element ranks. Engines pre-build a [`StatusSnapshot`] per
//! party member from their record and feed it through the session;
//! cursor input cycles between active party members. The session never
//! reads a record itself — it stays renderer-agnostic and engine-engine
//! decoupled.
//!
//! ## States
//!
//! - [`StatusPhase::Browsing`] — viewing a character's panel; L1/R1 cycle.
//! - [`StatusPhase::Done`] — Circle/Start cancelled out, shell closes.

use crate::input::PadButton;

/// Per-equip-slot view. Plain data so the renderer doesn't have to
/// resolve item ids itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipSlotView {
    pub label: &'static str,
    pub item_name: String,
}

/// Per-element-rank view. Engines feed in the eight retail elemental
/// resistance ranks (0..=99) plus the matching display label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementRankView {
    pub label: &'static str,
    pub rank: u8,
}

/// One character's status panel. Engines populate from the live record;
/// the screen never invents data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub slot: u8,
    pub name: String,
    pub level: u8,
    pub xp: u32,
    pub xp_to_next: u32,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap: u8,
    pub ap_max: u8,
    pub attack: u16,
    pub defense: u16,
    pub stats: [u8; 6],
    pub stat_labels: [&'static str; 6],
    pub equip: Vec<EquipSlotView>,
    pub elements: Vec<ElementRankView>,
}

impl StatusSnapshot {
    /// Convenience constructor for a freshly-rolled record. Engines that
    /// have a [`crate::battle_session::SessionSlotInfo`] handy can `From`
    /// from it; this is the empty-record fallback.
    pub fn placeholder(slot: u8, name: impl Into<String>) -> Self {
        Self {
            slot,
            name: name.into(),
            stat_labels: ["STR", "DEF", "SPI", "AGI", "MAG", "RES"],
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusPhase {
    Browsing { cursor: u8 },
    Done(StatusOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusOutcome {
    /// Player closed the screen.
    Closed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusInput {
    pub left: bool,
    pub right: bool,
    pub l1: bool,
    pub r1: bool,
    pub circle: bool,
    pub start: bool,
}

impl StatusInput {
    pub fn from_pad_edge(pressed: u16) -> Self {
        Self {
            left: pressed & PadButton::Left.mask() != 0,
            right: pressed & PadButton::Right.mask() != 0,
            l1: pressed & PadButton::L1.mask() != 0,
            r1: pressed & PadButton::R1.mask() != 0,
            circle: pressed & PadButton::Circle.mask() != 0,
            start: pressed & PadButton::Start.mask() != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusEvent {
    /// Cursor moved to a different character.
    CharSwitched { cursor: u8 },
    /// Player closed the screen.
    Closed,
}

#[derive(Debug, Clone)]
pub struct StatusScreenSession {
    snapshots: Vec<StatusSnapshot>,
    phase: StatusPhase,
}

impl StatusScreenSession {
    pub fn new(snapshots: Vec<StatusSnapshot>) -> Self {
        Self {
            snapshots,
            phase: StatusPhase::Browsing { cursor: 0 },
        }
    }

    pub fn snapshots(&self) -> &[StatusSnapshot] {
        &self.snapshots
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            StatusPhase::Browsing { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn current(&self) -> Option<&StatusSnapshot> {
        let i = self.cursor() as usize;
        self.snapshots.get(i)
    }

    pub fn phase(&self) -> StatusPhase {
        self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, StatusPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<StatusOutcome> {
        match self.phase {
            StatusPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    fn step_cursor(&self, cursor: u8, dir: i8) -> u8 {
        let n = self.snapshots.len() as i8;
        if n <= 0 {
            return 0;
        }
        let next = (cursor as i8 + dir).rem_euclid(n);
        next as u8
    }

    pub fn tick(&mut self, input: StatusInput) -> Vec<StatusEvent> {
        let mut events = Vec::new();
        if let StatusPhase::Browsing { cursor } = self.phase {
            if input.circle || input.start {
                self.phase = StatusPhase::Done(StatusOutcome::Closed);
                events.push(StatusEvent::Closed);
                return events;
            }
            let mut new_cursor = cursor;
            if input.left || input.l1 {
                new_cursor = self.step_cursor(cursor, -1);
            } else if input.right || input.r1 {
                new_cursor = self.step_cursor(cursor, 1);
            }
            if new_cursor != cursor {
                self.phase = StatusPhase::Browsing { cursor: new_cursor };
                events.push(StatusEvent::CharSwitched { cursor: new_cursor });
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(slot: u8, name: &str) -> StatusSnapshot {
        let mut s = StatusSnapshot::placeholder(slot, name);
        s.level = 5;
        s.hp = 60;
        s.hp_max = 60;
        s.mp = 24;
        s.mp_max = 24;
        s
    }

    #[test]
    fn cursor_starts_at_zero() {
        let s = StatusScreenSession::new(vec![snap(0, "Vahn"), snap(1, "Noa")]);
        assert_eq!(s.cursor(), 0);
        assert_eq!(s.current().unwrap().name, "Vahn");
    }

    #[test]
    fn r1_advances_cursor() {
        let mut s =
            StatusScreenSession::new(vec![snap(0, "Vahn"), snap(1, "Noa"), snap(2, "Gala")]);
        let evs = s.tick(StatusInput {
            r1: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 1);
        assert_eq!(evs, vec![StatusEvent::CharSwitched { cursor: 1 }]);
    }

    #[test]
    fn l1_wraps_to_last() {
        let mut s =
            StatusScreenSession::new(vec![snap(0, "Vahn"), snap(1, "Noa"), snap(2, "Gala")]);
        let _ = s.tick(StatusInput {
            l1: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn circle_closes() {
        let mut s = StatusScreenSession::new(vec![snap(0, "Vahn")]);
        let evs = s.tick(StatusInput {
            circle: true,
            ..Default::default()
        });
        assert!(s.is_done());
        assert_eq!(evs, vec![StatusEvent::Closed]);
    }

    #[test]
    fn left_right_also_cycle() {
        let mut s = StatusScreenSession::new(vec![snap(0, "Vahn"), snap(1, "Noa")]);
        let _ = s.tick(StatusInput {
            right: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 1);
        let _ = s.tick(StatusInput {
            left: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn empty_snapshot_list_does_not_panic() {
        let mut s = StatusScreenSession::new(Vec::new());
        let _ = s.tick(StatusInput {
            r1: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 0);
        assert!(s.current().is_none());
    }

    #[test]
    fn pad_edge_decoder_mirrors_pad_bits() {
        let mask = PadButton::Right.mask() | PadButton::R1.mask();
        let inp = StatusInput::from_pad_edge(mask);
        assert!(inp.right && inp.r1);
        assert!(!inp.left && !inp.circle);
    }

    #[test]
    fn placeholder_default_stat_labels() {
        let s = StatusSnapshot::placeholder(0, "Vahn");
        assert_eq!(s.stat_labels[0], "STR");
        assert_eq!(s.stat_labels[5], "RES");
    }
}
