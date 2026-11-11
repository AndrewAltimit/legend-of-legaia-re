//! Status screen session.
//!
//! Per-character stat detail screen: HP / MP / AP / level / XP / equipped
//! slots / element ranks. Engines pre-build a [`StatusSnapshot`] per
//! party member from their record and feed it through the session;
//! cursor input cycles between active party members. The session never
//! reads a record itself - it stays renderer-agnostic and engine-engine
//! decoupled.
//!
//! ## States
//!
//! - [`StatusPhase::Browsing`] - viewing a character's panel; Up / Down (and
//!   L1 / R1) walk the party, Left / Right turn the page, a confirm on the
//!   Magic or Moves page hands the pad to that page's list.
//! - [`StatusPhase::Done`] - Circle/Start cancelled out, shell closes.

use legaia_engine_system::input::PadButton;

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
    /// The retail "Next Level" number: the cumulative XP total at which the
    /// next level lands (record `+0x4`, drawn verbatim by `FUN_801D33D8`),
    /// not the remaining difference. 0 at L99.
    pub xp_to_next: u32,
    pub hp: u16,
    pub hp_max: u16,
    /// The record-side maximum HP (record `+0x11C`), which `FUN_801D33D8`
    /// prints in parentheses beside the live maximum (`0x801D3684`).
    pub hp_base: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// The record-side maximum MP (record `+0x11E`, `0x801D3734`).
    pub mp_base: u16,
    pub ap: u8,
    pub ap_max: u8,
    pub attack: u16,
    pub defense: u16,
    /// The six derived stats of the retail status grid, as `(live, growth)`
    /// pairs: live from the record's `+0x110` window, growth from the
    /// `+0x122..+0x12D` record window (the parenthesised second number of
    /// the retail 3x2 grid). Order matches [`RETAIL_STAT_LABELS`].
    pub stats: [(u16, u16); 6],
    pub stat_labels: [&'static str; 6],
    pub equip: Vec<EquipSlotView>,
    pub elements: Vec<ElementRankView>,
    /// The member's packed battle-status halfword - char record `+0x12E`,
    /// the mirror of battle actor `+0x16E` that retail's `FUN_80047430`
    /// re-copies every frame a party actor ticks
    /// (`docs/formats/save-record.md`). The pause menu's HP number inks
    /// gold while this is non-zero: it is the `status` argument of
    /// `FUN_800349EC` (`legaia_engine_ui::menu_hp_ink_with_status`,
    /// which reads it at `0x6f6(0x80084140 + slot*0x414)`).
    pub status_flags: u16,
    /// The worn Ra-Seru's level; `None` with the slot empty, which also
    /// empties the Magic page (`0x801D4124`). The name is the character's
    /// (`0x801E46D4 + char * 4`).
    pub seru_level: Option<u8>,
    /// The Skills page's rows: one per Goods slot whose item grants a passive.
    pub skills: Vec<StatusSkillRow>,
    /// The Magic page's rows, in record order.
    pub spells: Vec<StatusSpellRow>,
    /// The Moves page's rows, in record order.
    pub moves: Vec<StatusMoveRow>,
}

/// One row of the Status screen's Skills page (`0x801D3EBC..0x801D4050`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSkillRow {
    pub item_name: String,
    pub effect_name: String,
    /// `|` is the retail line break.
    pub description: String,
}

/// One row of the Status screen's Magic page (`0x801D4154..0x801D4374`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSpellRow {
    pub spell_id: u8,
    pub name: String,
    /// The `0xCE` element-plate escape the raw name opens with.
    pub icon: Option<u8>,
    pub level: u8,
    pub mp_cost: u16,
    /// The info line under the list (`0x80075DB0[record +4]`), `|` line breaks.
    pub description: String,
}

/// One row of the Status screen's Moves page (`0x801D444C..0x801D4730`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusMoveRow {
    pub id: u8,
    /// The name string as stored, `0xCF` ink and `0xC1` name escapes kept.
    pub name_bytes: Vec<u8>,
    pub ap: u8,
    /// The command string's entries as stored, `0xFF XX` ink markers kept.
    pub glyphs: Vec<[u8; 2]>,
    /// The description string as stored, `|` line breaks.
    pub description: Vec<u8>,
}

/// The retail derived-stat grid labels, in the panel's draw order (left
/// column top-to-bottom, then right column): the same six stats the
/// `FUN_801D33D8` status page prints (docs/subsystems/field-menu.md).
pub const RETAIL_STAT_LABELS: [&str; 6] = ["ATK", "UDF", "LDF", "SPD", "INT", "AGL"];

impl StatusSnapshot {
    /// Convenience constructor for a freshly-rolled record - the
    /// empty-record fallback.
    pub fn placeholder(slot: u8, name: impl Into<String>) -> Self {
        Self {
            slot,
            name: name.into(),
            stat_labels: RETAIL_STAT_LABELS,
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
    /// The raw edge word, for the list page's own picker.
    pub pressed: u16,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub l1: bool,
    pub r1: bool,
    pub circle: bool,
    pub start: bool,
    /// Confirm. Retail's sub-screen `0x15` character picker takes a confirm
    /// and opens the shown page's list.
    pub cross: bool,
}

impl StatusInput {
    pub fn from_pad_edge(pressed: u16) -> Self {
        Self {
            pressed,
            up: pressed & PadButton::Up.mask() != 0,
            down: pressed & PadButton::Down.mask() != 0,
            cross: pressed & PadButton::Cross.mask() != 0,
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
    /// The pager turned to another page.
    PageTurned { page: u8 },
    /// A confirm on a page with no list, or an empty one: retail's `0x23`.
    Rejected,
    /// The page's list took the pad.
    ListOpened,
    /// Player closed the screen.
    Closed,
}

#[derive(Debug, Clone)]
pub struct StatusScreenSession {
    snapshots: Vec<StatusSnapshot>,
    phase: StatusPhase,
    /// The pager's folded page ([`STATUS_PAGE_CONDITION`]..), `DAT_801E46C0`.
    page: u8,
    /// The page's list while it holds the pad (steps `6` / `7`).
    list: Option<crate::list_order::ListOrderSession>,
    /// A closed list whose exchanges the host has yet to replay on the record.
    closed_list: Option<crate::list_order::ListOrderSession>,
}

/// The pager's pages, in the order Right walks them (`FUN_801DA2A0`).
pub const STATUS_PAGE_CONDITION: u8 = 0;
pub const STATUS_PAGE_SKILLS: u8 = 1;
pub const STATUS_PAGE_MAGIC: u8 = 2;
pub const STATUS_PAGE_MOVES: u8 = 3;
const STATUS_PAGES: u8 = 4;

impl StatusScreenSession {
    pub fn new(snapshots: Vec<StatusSnapshot>) -> Self {
        Self {
            snapshots,
            phase: StatusPhase::Browsing { cursor: 0 },
            page: STATUS_PAGE_CONDITION,
            list: None,
            closed_list: None,
        }
    }

    /// The page the pager is on.
    pub fn page(&self) -> u8 {
        self.page
    }

    /// The page's list while it holds the pad.
    pub fn list(&self) -> Option<&crate::list_order::ListOrderSession> {
        self.list.as_ref()
    }

    /// A list that closed since the last call, for the host to replay its
    /// exchanges on the record.
    pub fn take_closed_list(&mut self) -> Option<crate::list_order::ListOrderSession> {
        self.closed_list.take()
    }

    /// The confirm arm of the character picker (`0x801DA3C8..0x801DA448`):
    /// the Magic and Moves pages open their lists, an empty one rejected
    /// before it settles; the Skills page buzzes; the Condition page's hop to
    /// the other column shows the same panel, so it changes nothing here.
    fn confirm_page(&mut self, events: &mut Vec<StatusEvent>) {
        use crate::list_order::{LIST_ORDER_STEP_MAGIC, LIST_ORDER_STEP_MOVES, ListOrderRow};
        let Some(snap) = self.current() else {
            return;
        };
        let slot = snap.slot;
        let opened = match self.page {
            STATUS_PAGE_MAGIC => crate::list_order::ListOrderSession::open(
                LIST_ORDER_STEP_MAGIC,
                slot,
                snap.spells
                    .iter()
                    .map(|r| ListOrderRow {
                        id: r.spell_id,
                        label: r.name.clone(),
                    })
                    .collect(),
            ),
            STATUS_PAGE_MOVES => crate::list_order::ListOrderSession::open(
                LIST_ORDER_STEP_MOVES,
                slot,
                snap.moves
                    .iter()
                    .map(|r| ListOrderRow {
                        id: r.id,
                        label: String::new(),
                    })
                    .collect(),
            ),
            STATUS_PAGE_CONDITION => return,
            _ => None,
        };
        match opened {
            Some(list) => {
                self.list = Some(list);
                events.push(StatusEvent::ListOpened);
            }
            None => events.push(StatusEvent::Rejected),
        }
    }

    /// Fold a closed list's exchanges into the shown rows, as the record will
    /// read after the host replays them.
    fn fold_list_swaps(&mut self, list: &crate::list_order::ListOrderSession) {
        let i = self.cursor() as usize;
        let Some(snap) = self.snapshots.get_mut(i) else {
            return;
        };
        for &(a, b) in list.swaps() {
            if a < snap.spells.len() && b < snap.spells.len() {
                snap.spells.swap(a, b);
            }
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
        if let Some(list) = self.list.as_mut() {
            let _ = list.tick(input.pressed);
            if list.is_done()
                && let Some(list) = self.list.take()
            {
                if list.reorderable() {
                    self.fold_list_swaps(&list);
                }
                self.closed_list = Some(list);
            }
            return events;
        }
        if let StatusPhase::Browsing { cursor } = self.phase {
            if input.circle || input.start {
                self.phase = StatusPhase::Done(StatusOutcome::Closed);
                events.push(StatusEvent::Closed);
                return events;
            }
            if input.cross {
                self.confirm_page(&mut events);
                return events;
            }
            if input.left || input.right {
                self.page = if input.left {
                    (self.page + STATUS_PAGES - 1) % STATUS_PAGES
                } else {
                    (self.page + 1) % STATUS_PAGES
                };
                events.push(StatusEvent::PageTurned { page: self.page });
                return events;
            }
            let mut new_cursor = cursor;
            if input.up || input.l1 {
                new_cursor = self.step_cursor(cursor, -1);
            } else if input.down || input.r1 {
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

    /// `FUN_801DA2A0`: the pad's Left / Right walk the page cursor with a
    /// wrap (`0x801DA480..0x801DA4D8`); `status_page_*` captures walk Skills,
    /// Magic and Moves that way.
    #[test]
    fn left_and_right_turn_the_page() {
        let mut s = StatusScreenSession::new(vec![snap(0, "Vahn"), snap(1, "Noa")]);
        let evs = s.tick(StatusInput::from_pad_edge(PadButton::Right.mask()));
        assert_eq!(s.page(), STATUS_PAGE_SKILLS);
        assert_eq!(evs, vec![StatusEvent::PageTurned { page: 1 }]);
        assert_eq!(s.cursor(), 0, "the page turn leaves the member");
        s.tick(StatusInput::from_pad_edge(PadButton::Left.mask()));
        s.tick(StatusInput::from_pad_edge(PadButton::Left.mask()));
        assert_eq!(s.page(), STATUS_PAGE_MOVES, "Left wraps");
    }

    /// The party picker `FUN_801D688C` runs on Up / Down.
    #[test]
    fn down_walks_the_party() {
        let mut s = StatusScreenSession::new(vec![snap(0, "Vahn"), snap(1, "Noa")]);
        s.tick(StatusInput::from_pad_edge(PadButton::Down.mask()));
        assert_eq!(s.cursor(), 1);
    }

    /// The Skills page has no list (`0x23`); an empty Moves list rejects
    /// before it settles.
    #[test]
    fn a_confirm_with_no_list_is_refused() {
        let mut s = StatusScreenSession::new(vec![snap(0, "Vahn")]);
        s.tick(StatusInput::from_pad_edge(PadButton::Right.mask()));
        let evs = s.tick(StatusInput::from_pad_edge(PadButton::Cross.mask()));
        assert_eq!(evs, vec![StatusEvent::Rejected]);
        assert!(s.list().is_none());
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
        assert_eq!(s.stat_labels[0], "ATK");
        assert_eq!(s.stat_labels[5], "AGL");
    }
}
