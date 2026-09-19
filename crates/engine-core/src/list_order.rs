//! The **list-reorder page** of the per-character record screen
//! (`FUN_801DA2A0`, menu-overlay sub-screen `0x15`).
//!
//! Retail runs one body over three lists - the 64-slot ability bitfield, the
//! learned-spell list and a third per-character list - selected by the
//! screen's own step counter rather than by three screens
//! ([`crate::save_subscreen::sub15_list_source`]). One of the three, the
//! spell list, is also **reorderable**: a confirm latches a row, the next
//! confirm exchanges the latched row with the hovered one across the three
//! parallel arrays the record keeps in step
//! ([`crate::save_subscreen::sub15_swap_rows`]), and a cancel drops the
//! latch before it drops the screen.
//!
//! This module is the session the two hosts drive. It owns the page window
//! and the latch; the record arithmetic stays in `save_subscreen`, where it
//! was ported, and runs against the live record in
//! [`crate::field_menu_dispatch::apply_list_order_outcome`].

use crate::menu_input::{CursorNav, NavButtons, menu_cursor_nav};
use crate::save_subscreen::{Sub15Frame, Sub15ListSource, sub15_frame, sub15_list_source};

/// Rows the page shows at once.
///
/// Retail's paging arithmetic is written in terms of `scroll + 7` (the
/// forward page test and the post-move clamp `scroll = selection - 6` both
/// name it), so the window is seven rows and a page step is seven.
pub const LIST_ORDER_PAGE_ROWS: usize = 7;

/// The reorder step of the screen: the running spell list (step `6`).
///
/// Steps `2`/`3`/`4` are the settle frames of the three lists and `5`/`6`/`7`
/// their running twins; only `6` carries the swap arm, which is why a
/// session built for another source browses without ever latching.
pub const LIST_ORDER_STEP_MAGIC: u8 = 6;

/// One row of the page: the id the record holds and the label to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListOrderRow {
    pub id: u8,
    pub label: String,
}

/// What a frame of the page produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOrderEvent {
    None,
    /// The hand moved (retail cues SFX `0x21`).
    Moved,
    /// A row was latched for exchange (SFX `0x20`).
    Latched(usize),
    /// Two rows were exchanged (SFX `0x20`).
    Swapped(usize, usize),
    /// A latch was dropped without exchanging anything.
    Unlatched,
    /// The page closed - the caller returns to the screen it came from.
    Closed,
}

/// The per-character list page, with the spell list's reorder.
#[derive(Debug, Clone)]
pub struct ListOrderSession {
    source: Sub15ListSource,
    /// Party slot whose record the outcome applies to.
    char_slot: u8,
    rows: Vec<ListOrderRow>,
    cursor: usize,
    scroll_top: usize,
    /// The row a confirm latched, waiting for its partner. Retail keeps this
    /// in the second cursor word's index bits and marks "nothing latched"
    /// with bit `0x1000`.
    latched: Option<usize>,
    /// Exchanges performed, in order, for the caller to replay against the
    /// live record.
    swaps: Vec<(usize, usize)>,
    done: bool,
}

impl ListOrderSession {
    /// Open the page for `char_slot`'s rows at retail's `step`.
    ///
    /// Returns `None` on the reject arm: retail tests the row count **before**
    /// the settle test, so an empty list buzzes and drops straight back to the
    /// character picker rather than showing an empty page
    /// ([`sub15_frame`]).
    pub fn open(step: u8, char_slot: u8, rows: Vec<ListOrderRow>) -> Option<Self> {
        let len = u8::try_from(rows.len()).unwrap_or(u8::MAX);
        match sub15_frame(step, len) {
            Sub15Frame::Reject => None,
            _ => Some(Self {
                source: sub15_list_source(step),
                char_slot,
                rows,
                cursor: 0,
                scroll_top: 0,
                latched: None,
                swaps: Vec::new(),
                done: false,
            }),
        }
    }

    /// Which of the three lists this page is showing.
    pub fn source(&self) -> Sub15ListSource {
        self.source
    }

    /// Party slot the outcome applies to.
    pub fn char_slot(&self) -> u8 {
        self.char_slot
    }

    pub fn rows(&self) -> &[ListOrderRow] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// First row of the visible page.
    pub fn scroll_top(&self) -> usize {
        self.scroll_top
    }

    /// The latched row, if a confirm is waiting for its exchange partner.
    pub fn latched(&self) -> Option<usize> {
        self.latched
    }

    /// The exchanges this session performed, oldest first.
    pub fn swaps(&self) -> &[(usize, usize)] {
        &self.swaps
    }

    /// `true` while this page can exchange rows - only the spell list's
    /// running step carries the swap arm.
    pub fn reorderable(&self) -> bool {
        self.source == Sub15ListSource::Magic
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Drive one frame from the edge-triggered pad word.
    ///
    /// PORT: FUN_801DA2A0 (`0x801DA6D0..0x801DA9CC` - the browse half: the
    /// clamping row picker, the page steps, the latch/exchange confirm and
    /// the two-stage cancel; `see ghidra/scripts/funcs/overlay_save_ui_801da2a0.txt`)
    pub fn tick(&mut self, pressed: u16) -> ListOrderEvent {
        use crate::input::PadButton;
        if self.done || self.rows.is_empty() {
            return ListOrderEvent::None;
        }
        let len = self.rows.len();
        let buttons = NavButtons::new(
            pressed & PadButton::Cross.mask() != 0,
            pressed & (PadButton::Circle.mask() | PadButton::Triangle.mask()) != 0,
            pressed & PadButton::Up.mask() != 0,
            pressed & PadButton::Down.mask() != 0,
        );
        // The row picker is the only call site in this overlay that passes
        // the clamping mode: every other one wraps.
        let mut cursor = self.cursor as u32;
        let nav = menu_cursor_nav(&mut cursor, len as u32, false, buttons);
        let event = match nav {
            CursorNav::Confirm => self.confirm(),
            CursorNav::Cancel => self.cancel(),
            CursorNav::Moved => {
                self.cursor = cursor as usize;
                ListOrderEvent::Moved
            }
            CursorNav::None => {
                if pressed & PadButton::Right.mask() != 0 {
                    self.page_forward()
                } else if pressed & PadButton::Left.mask() != 0 {
                    self.page_back()
                } else {
                    ListOrderEvent::None
                }
            }
        };
        self.clamp_window();
        event
    }

    /// Retail's forward page step: advance by a whole page while one fits,
    /// by the remainder while a partial one does, and otherwise jump the
    /// hand to the last row without moving the window.
    fn page_forward(&mut self) -> ListOrderEvent {
        let len = self.rows.len();
        if self.scroll_top + LIST_ORDER_PAGE_ROWS < len {
            let step = (len - self.scroll_top - LIST_ORDER_PAGE_ROWS).min(LIST_ORDER_PAGE_ROWS);
            self.cursor += step;
            self.scroll_top += step;
        } else {
            self.cursor = len - 1;
        }
        ListOrderEvent::Moved
    }

    /// The mirror: a page back, or the top when less than a page remains
    /// above.
    fn page_back(&mut self) -> ListOrderEvent {
        if self.scroll_top == 0 {
            self.cursor = 0;
        } else {
            let step = self.scroll_top.min(LIST_ORDER_PAGE_ROWS);
            self.scroll_top -= step;
            self.cursor = self.cursor.saturating_sub(step);
        }
        ListOrderEvent::Moved
    }

    /// Keep the hand inside the window (retail runs both clamps after every
    /// move, not only after a page step).
    fn clamp_window(&mut self) {
        let len = self.rows.len();
        if len == 0 {
            return;
        }
        self.cursor = self.cursor.min(len - 1);
        if self.cursor < self.scroll_top {
            self.scroll_top = self.cursor;
        }
        if self.scroll_top + LIST_ORDER_PAGE_ROWS <= self.cursor {
            self.scroll_top = self.cursor + 1 - LIST_ORDER_PAGE_ROWS;
        }
    }

    /// Confirm: latch a row, or exchange it with the latched one. A page
    /// that cannot reorder consumes the press without changing anything -
    /// retail cues a sound and returns.
    fn confirm(&mut self) -> ListOrderEvent {
        if !self.reorderable() {
            return ListOrderEvent::None;
        }
        match self.latched.take() {
            None => {
                self.latched = Some(self.cursor);
                ListOrderEvent::Latched(self.cursor)
            }
            Some(first) if first == self.cursor => {
                // Retail exchanges a row with itself happily; the swap is a
                // no-op, so record nothing and drop the latch.
                ListOrderEvent::Unlatched
            }
            Some(first) => {
                self.rows.swap(first, self.cursor);
                self.swaps.push((first, self.cursor));
                ListOrderEvent::Swapped(first, self.cursor)
            }
        }
    }

    /// Cancel: the first press drops a pending latch, the second closes the
    /// page.
    fn cancel(&mut self) -> ListOrderEvent {
        if self.latched.take().is_some() {
            return ListOrderEvent::Unlatched;
        }
        self.done = true;
        ListOrderEvent::Closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<ListOrderRow> {
        (0..n)
            .map(|i| ListOrderRow {
                id: i as u8 + 1,
                label: format!("row {i}"),
            })
            .collect()
    }

    fn pad(b: crate::input::PadButton) -> u16 {
        b.mask()
    }

    #[test]
    fn an_empty_list_never_opens() {
        assert!(ListOrderSession::open(LIST_ORDER_STEP_MAGIC, 0, Vec::new()).is_none());
    }

    #[test]
    fn the_hand_clamps_instead_of_wrapping() {
        let mut s = ListOrderSession::open(LIST_ORDER_STEP_MAGIC, 0, rows(3)).unwrap();
        s.tick(pad(crate::input::PadButton::Up));
        assert_eq!(s.cursor(), 0, "up at the top stays put");
        for _ in 0..5 {
            s.tick(pad(crate::input::PadButton::Down));
        }
        assert_eq!(s.cursor(), 2, "down past the end stays put");
    }

    #[test]
    fn a_page_step_moves_the_window_and_the_last_page_moves_only_the_hand() {
        let mut s = ListOrderSession::open(LIST_ORDER_STEP_MAGIC, 0, rows(20)).unwrap();
        s.tick(pad(crate::input::PadButton::Right));
        assert_eq!((s.scroll_top(), s.cursor()), (7, 7));
        s.tick(pad(crate::input::PadButton::Right));
        assert_eq!(
            (s.scroll_top(), s.cursor()),
            (13, 13),
            "the partial page advances by the remainder"
        );
        s.tick(pad(crate::input::PadButton::Right));
        assert_eq!(
            (s.scroll_top(), s.cursor()),
            (13, 19),
            "with no page left the hand jumps to the last row"
        );
        s.tick(pad(crate::input::PadButton::Left));
        assert_eq!((s.scroll_top(), s.cursor()), (6, 12));
    }

    #[test]
    fn confirm_latches_then_exchanges() {
        let mut s = ListOrderSession::open(LIST_ORDER_STEP_MAGIC, 0, rows(4)).unwrap();
        assert_eq!(
            s.tick(pad(crate::input::PadButton::Cross)),
            ListOrderEvent::Latched(0)
        );
        assert_eq!(s.latched(), Some(0));
        s.tick(pad(crate::input::PadButton::Down));
        s.tick(pad(crate::input::PadButton::Down));
        assert_eq!(
            s.tick(pad(crate::input::PadButton::Cross)),
            ListOrderEvent::Swapped(0, 2)
        );
        assert_eq!(s.swaps(), &[(0, 2)]);
        assert_eq!(s.rows()[0].id, 3, "the page shows the exchange");
        assert_eq!(s.rows()[2].id, 1);
        assert!(s.latched().is_none(), "the exchange drops the latch");
    }

    #[test]
    fn cancel_drops_the_latch_before_it_drops_the_page() {
        let mut s = ListOrderSession::open(LIST_ORDER_STEP_MAGIC, 0, rows(4)).unwrap();
        s.tick(pad(crate::input::PadButton::Cross));
        assert_eq!(
            s.tick(pad(crate::input::PadButton::Circle)),
            ListOrderEvent::Unlatched
        );
        assert!(!s.is_done());
        assert_eq!(
            s.tick(pad(crate::input::PadButton::Circle)),
            ListOrderEvent::Closed
        );
        assert!(s.is_done());
    }

    #[test]
    fn a_non_reorderable_page_browses_without_latching() {
        // Step 5 is the abilities list's running twin - same body, no swap.
        let mut s = ListOrderSession::open(5, 0, rows(4)).unwrap();
        assert!(!s.reorderable());
        assert_eq!(
            s.tick(pad(crate::input::PadButton::Cross)),
            ListOrderEvent::None
        );
        assert!(s.latched().is_none());
        assert!(s.swaps().is_empty());
    }
}
