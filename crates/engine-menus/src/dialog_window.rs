//! The field dialog pager's **row window** - which rows the reading box shows,
//! how it scrolls, and what a confirm press does while a page types.
//!
//! PORT: FUN_801D84D0 (the reading-box arms: states `0x02`/`0x05`, `0x0B`..`0x0F`, `0x10`, `0x19`)
//!
//! The pager `FUN_801D84D0` (field overlay, PROT 0897) keeps a table of row
//! pointers `_DAT_801F3540[]` and draws every non-null one. Read off the
//! disassembly, the table is a **scrolling window**, not a page buffer:
//!
//! - **Typing (`0x0B`).** The row at the table's last index types at the
//!   [`crate::dialog_pacing`] pace; every row above it draws whole. When a row
//!   finishes and the byte past it is another line (`(b & 0x7F) < 0x20`,
//!   `0x801D8AB4`), the next line takes the next slot - or, once the three
//!   slots (`_DAT_801F2740`) are full, the window first scrolls (`0x0C`). No
//!   button is involved. Any other byte ends the page.
//! - **Scroll-in (`0x0C`).** The scroll word `_DAT_801F2738` falls by
//!   `speed * dt` a call (`speed` = `_DAT_801F2750`, `0x24`); past `-0xEF` the
//!   table shifts up a slot, the next line enters the last one, and the
//!   scroll resets (`0x801D8B5C..0x801D8C60`). The draw adds `scroll >> 4`
//!   to every row's `y`, so a row is `0xF0` scroll units (15 px) tall.
//! - **Page end.** The pager sets `_DAT_801F3534 = rows_on_page - 1 +
//!   (3 - last_index)`; below three, rows carried over from the previous page
//!   still sit above this page's, and state `0x0F` scrolls them away at
//!   `speed - speed/4` a call until they are gone (`0x801D8E3C..0x801D8F48`).
//!   Only then does the page wait (`0x19`) with the advance hand showing.
//! - **Page turn.** A press in `0x19` on a `0x24` byte keeps the table: the
//!   next page's first line goes in the slot below the last row (state `5`,
//!   `0x801D88C8..0x801D89B4`), scrolling first when the window is full. A
//!   `0x48` (fresh box) and the teardown bytes clear it (states `9` / `0`).
//! - **Confirm while typing.** A press in `0x0B` sets `speed = 0x25` and jumps
//!   to `0x0D` (`0x801D89B8..0x801D8A04`), which shows the rest of the page at
//!   once - the typing row whole, then one more line a call - and scrolls the
//!   overflow through (`0x0E`, at `0x25`). A press during a short-row hold
//!   (dispatch case `0x10`, `0x801D86BC`) does the same when `speed` is still
//!   `0x24`, and clears the hold. A press in `0x0C` / `0x0F` only raises the
//!   speed; `0x0C` then hands over to `0x0D`.
//!
//! A PCSX-Redux trace of `town01` placement `P1[16]`
//! (`scripts/pcsx-redux/autorun_dialog_typewriter_trace.lua`, one CSV row per
//! vsync with the scroll word and the row table as record offsets) shows all
//! of it: a two-row page, a page whose third row scrolls in unprompted, the
//! carried rows scrolled away before each wait, and - with confirm taps
//! injected - the `0x25` latch, state `0x0D` and the faster scrolls.
//! `crates/engine-core/tests/dialog_window_disc.rs` replays both traces
//! against the engine.

use crate::dialog_pacing::{PagerCall, TypewriterPacer};

/// Rows the window shows (`_DAT_801F2740`).
pub const WINDOW_ROWS: usize = 3;
/// One row of scroll, in the pager's 1/16-px units (`0xF0` = 15 px).
pub const ROW_SCROLL: i32 = 0xF0;
/// A scroll finishes a row once the word is below this (`slti v0,v0,-0xef`).
pub const SCROLL_DONE_BELOW: i32 = -0xEF;
/// The scroll speed while a page types (`_DAT_801F2750`, `0x801D88CC`).
pub const SPEED_TYPE: i32 = 0x24;
/// The speed a confirm press latches (`0x801D89DC` and siblings).
pub const SPEED_SKIP: i32 = 0x25;

/// Which pager arm the window is in. [`Self::retail_state`] names the
/// `_DAT_801F2734` value it mirrors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPhase {
    /// A page turn's press call: nothing moves (states `3`/`4`).
    PressTurn,
    /// The call that loads the next page's first line (state `5`, or `2` for
    /// a fresh box).
    Load,
    /// `0x0B`: the last row types.
    Typing,
    /// `0x0C`: the window scrolls a row up to make room for the next line.
    ScrollIn,
    /// `0x0D`: a confirm press completes the page, a line a call.
    Complete,
    /// `0x0E`: the completed page's overflow scrolls through.
    CompleteScroll,
    /// `0x0F`: rows carried over from the previous page scroll away.
    ScrollAway,
    /// `0x19`: the page is shown whole and waits for a press.
    Wait,
}

impl WindowPhase {
    /// The `_DAT_801F2734` state this phase mirrors: a page turn reads `0x19`
    /// until the call that sees the press, then `5` until the load call.
    pub fn retail_state(self) -> u8 {
        match self {
            WindowPhase::PressTurn => 0x19,
            WindowPhase::Load => 5,
            WindowPhase::Typing => 0x0B,
            WindowPhase::ScrollIn => 0x0C,
            WindowPhase::Complete => 0x0D,
            WindowPhase::CompleteScroll => 0x0E,
            WindowPhase::ScrollAway => 0x0F,
            WindowPhase::Wait => 0x19,
        }
    }
}

/// One slot of the window: which page and which of its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRow {
    /// Serial of the page the line belongs to (bumped by every page turn).
    pub page: u32,
    /// Line index within that page.
    pub line: usize,
}

/// The pager's row window over a stream of pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowWindow {
    /// Rows in the window, top first (`_DAT_801F3540[]`): at most
    /// [`WINDOW_ROWS`], or one more while `0x0D`/`0x0E` scroll a row in.
    pub rows: Vec<WindowRow>,
    /// Scroll word `_DAT_801F2738` (1/16 px, `<= 0`).
    pub scroll: i32,
    /// Scroll speed / skip latch `_DAT_801F2750`.
    pub speed: i32,
    /// `_DAT_801F3534`.
    pub rows_on_page: i32,
    /// Current arm.
    pub phase: WindowPhase,
    /// Reveal counter / accumulator / hold for the typing row.
    pub pacer: TypewriterPacer,
    page: u32,
    page_len: usize,
    /// Next line of the current page not yet in the window.
    next: usize,
    /// The pending page turn keeps the window (`0x24`) rather than clearing it.
    keep_rows: bool,
}

impl RowWindow {
    /// A fresh box on a page of `page_len` lines: the first line is in the
    /// window and the next call is its opening call (counter `1`).
    pub fn open(page_len: usize) -> Self {
        let mut w = Self {
            rows: Vec::new(),
            scroll: 0,
            speed: SPEED_TYPE,
            rows_on_page: 0,
            phase: WindowPhase::Typing,
            pacer: TypewriterPacer::new(),
            page: 0,
            page_len,
            next: 0,
            keep_rows: false,
        };
        // State 2's load: the first row in slot 0, `_DAT_801F3534 = 1`.
        if w.push_next_line() {
            w.rows_on_page = 1;
        }
        w
    }

    /// Serial of the current page.
    pub fn page(&self) -> u32 {
        self.page
    }

    /// Queue a page turn onto a page of `page_len` lines. `keep_rows` is the
    /// `0x24` continuation (the window stays); `false` opens a fresh box. The
    /// next call is the press call, the one after it loads the first line.
    pub fn turn_page(&mut self, page_len: usize, keep_rows: bool) {
        self.page += 1;
        self.page_len = page_len;
        self.next = 0;
        self.keep_rows = keep_rows;
        self.phase = WindowPhase::PressTurn;
    }

    /// The row being typed and how many of its units show, or `None` when
    /// every row in the window draws whole.
    pub fn typing_row(&self) -> Option<(WindowRow, u32)> {
        (self.phase == WindowPhase::Typing)
            .then(|| self.rows.last().copied())
            .flatten()
            .map(|r| (r, self.pacer.visible_units()))
    }

    /// Whole pixels the draw offsets every row by (`scroll >> 4`, `<= 0`).
    pub fn scroll_px(&self) -> i32 {
        self.scroll >> 4
    }

    /// `true` while the page waits for a press.
    pub fn waiting(&self) -> bool {
        self.phase == WindowPhase::Wait
    }

    fn last_index(&self) -> i32 {
        self.rows.len() as i32 - 1
    }

    fn push_next_line(&mut self) -> bool {
        if self.next >= self.page_len {
            return false;
        }
        self.rows.push(WindowRow {
            page: self.page,
            line: self.next,
        });
        self.next += 1;
        true
    }

    fn has_next_line(&self) -> bool {
        self.next < self.page_len
    }

    /// Drop the top row (the table's shift-up loop).
    fn shift_up(&mut self) {
        if !self.rows.is_empty() {
            self.rows.remove(0);
        }
    }

    /// The page's last line has shown: `rows_on_page - 1 + (3 - last)`, then
    /// scroll the carried rows away or wait.
    fn end_page(&mut self) {
        let max = WINDOW_ROWS as i32;
        self.rows_on_page = self.rows_on_page - 1 + (max - self.last_index());
        self.phase = if self.rows_on_page < max {
            WindowPhase::ScrollAway
        } else {
            WindowPhase::Wait
        };
    }

    /// One pager call at frame step `dt` (`DAT_1F800393`). `pressed` is a
    /// confirm press this call sees; `count(line)` is `FUN_80036044` of a line
    /// of the current page. Returns `true` when this call reached
    /// [`WindowPhase::Wait`].
    pub fn call(&mut self, dt: u8, pressed: bool, mut count: impl FnMut(usize) -> u32) -> bool {
        let step = i32::from(dt.min(4));
        let was_waiting = self.phase == WindowPhase::Wait;
        // `0x801D865C`: state 0x19 clears the hold - the press call of a
        // page turn is a state-0x19 call too.
        if was_waiting || self.phase == WindowPhase::PressTurn {
            self.pacer.hold = 0;
        }
        // `0x801D866C..0x801D8690`, then dispatch case 0x10 (`0x801D86BC`).
        if self.pacer.hold != 0 {
            self.pacer.hold -= step << 5;
            if self.pacer.hold <= 0 {
                self.pacer.hold = 0;
            }
            if pressed && self.speed == SPEED_TYPE {
                self.speed = SPEED_SKIP;
                self.pacer.hold = 0;
            }
            return false;
        }
        let max = WINDOW_ROWS as i32;
        match self.phase {
            WindowPhase::Wait => {}
            WindowPhase::PressTurn => {
                // The 0x19 press handler (`0x801D8FE4`).
                self.rows_on_page = 0;
                self.phase = WindowPhase::Load;
            }
            WindowPhase::Load => {
                // State 5 (`0x801D88C8..0x801D89B4`); a fresh box is state 2.
                self.speed = SPEED_TYPE;
                if !self.keep_rows {
                    self.rows.clear();
                }
                if self.keep_rows && self.last_index() + 1 >= max {
                    // Full window: scroll first; the counter stays at the 0
                    // state 4 left it, so no opening call.
                    self.pacer.counter = 0;
                    self.pacer.acc = 0;
                    self.phase = WindowPhase::ScrollIn;
                } else {
                    self.push_next_line();
                    self.rows_on_page = 1;
                    self.pacer.begin_box();
                    self.pacer.call(dt, 0);
                    self.phase = WindowPhase::Typing;
                }
            }
            WindowPhase::Typing => {
                if pressed {
                    self.speed = SPEED_SKIP;
                }
                if self.speed == SPEED_SKIP {
                    self.phase = WindowPhase::Complete;
                    return false;
                }
                let Some(row) = self.rows.last().copied() else {
                    self.end_page();
                    return self.phase == WindowPhase::Wait;
                };
                if self.pacer.call(dt, count(row.line)) == PagerCall::RowFinished {
                    if !self.has_next_line() {
                        self.end_page();
                    } else if self.last_index() + 1 < max {
                        self.push_next_line();
                        self.rows_on_page += 1;
                    } else {
                        self.phase = WindowPhase::ScrollIn;
                    }
                }
            }
            WindowPhase::ScrollIn => {
                if pressed {
                    self.speed = SPEED_SKIP;
                }
                self.scroll -= step * self.speed;
                if self.scroll < SCROLL_DONE_BELOW {
                    self.shift_up();
                    self.push_next_line();
                    self.rows_on_page += 1;
                    self.phase = if self.speed == SPEED_SKIP {
                        WindowPhase::Complete
                    } else {
                        WindowPhase::Typing
                    };
                    self.scroll = 0;
                }
            }
            WindowPhase::Complete => {
                self.pacer.counter = 0;
                self.pacer.acc = 0;
                if self.last_index() >= max {
                    self.phase = WindowPhase::CompleteScroll;
                    self.complete_scroll(step);
                } else if self.has_next_line() {
                    self.push_next_line();
                    self.rows_on_page += 1;
                } else {
                    self.end_page();
                    if self.phase == WindowPhase::Wait {
                        self.scroll = 0;
                    }
                }
            }
            WindowPhase::CompleteScroll => self.complete_scroll(step),
            WindowPhase::ScrollAway => {
                if pressed {
                    self.speed = SPEED_SKIP;
                }
                self.scroll -= step * (self.speed - (self.speed >> 2));
                if self.scroll < SCROLL_DONE_BELOW {
                    self.shift_up();
                    self.rows_on_page += 1;
                    if self.rows_on_page < max {
                        self.scroll += ROW_SCROLL;
                    } else {
                        self.scroll = 0;
                        self.phase = WindowPhase::Wait;
                    }
                }
            }
        }
        !was_waiting && self.phase == WindowPhase::Wait
    }

    /// State `0x0E` (`0x801D8D28..0x801D8E38`).
    fn complete_scroll(&mut self, step: i32) {
        self.speed = SPEED_SKIP;
        self.scroll -= step * SPEED_SKIP;
        if self.scroll < SCROLL_DONE_BELOW {
            self.shift_up();
            if self.push_next_line() {
                self.rows_on_page += 1;
                self.scroll += ROW_SCROLL;
            } else if self.rows_on_page < WINDOW_ROWS as i32 {
                self.phase = WindowPhase::ScrollAway;
                self.scroll += ROW_SCROLL;
            } else {
                self.scroll = 0;
                self.phase = WindowPhase::Wait;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-call `(state, rows as (page, line), scroll)`.
    type Snap = (u8, Vec<(u32, usize)>, i32);

    fn snap(w: &RowWindow) -> Snap {
        (
            w.phase.retail_state(),
            w.rows.iter().map(|r| (r.page, r.line)).collect(),
            w.scroll,
        )
    }

    /// Run calls at `dt` until the window waits, returning each call's snap.
    fn until_wait(w: &mut RowWindow, dt: u8, counts: &[u32], press_at: &[usize]) -> Vec<Snap> {
        let mut out = Vec::new();
        for i in 0..2000 {
            let pressed = press_at.contains(&i);
            let reached = w.call(dt, pressed, |l| counts[l]);
            out.push(snap(w));
            if reached {
                return out;
            }
        }
        panic!("window never waited");
    }

    #[test]
    fn a_page_turn_keeps_the_rows_and_a_full_window_scrolls_unprompted() {
        // town01 P1[16]'s first two pages at dt = 2: page 0 = rows 25, 30;
        // page 1 = rows 32, 24 (the 32 types beneath the two carried rows,
        // the 24 scrolls in without a press).
        let mut w = RowWindow::open(2);
        until_wait(&mut w, 2, &[25, 30], &[]);
        assert_eq!(w.rows.len(), 2);
        w.turn_page(2, true);
        let calls = until_wait(&mut w, 2, &[32, 24], &[]);
        // Press call, load (row 2 = page 1 line 0 beneath the carried rows).
        assert_eq!(calls[0].0, 5);
        assert_eq!(calls[0].1, vec![(0, 0), (0, 1)]);
        assert_eq!(calls[1], (0x0B, vec![(0, 0), (0, 1), (1, 0)], 0));
        // The trace: the finish holds 8 for one call, then four scroll calls
        // at 72 a call; the fourth (-288) shifts the window.
        let scroll: Vec<i32> = calls.iter().filter(|c| c.0 == 0x0C).map(|c| c.2).collect();
        assert_eq!(scroll, vec![0, 0, -72, -144, -216]);
        let first_typing_after = calls.iter().position(|c| c.0 == 0x0C).unwrap() + 5;
        assert_eq!(
            calls[first_typing_after],
            (0x0B, vec![(0, 1), (1, 0), (1, 1)], 0)
        );
        // The page ends with one carried row: scroll it away at 54 a call.
        let away: Vec<i32> = calls.iter().filter(|c| c.0 == 0x0F).map(|c| c.2).collect();
        assert_eq!(*away.last().unwrap(), -216);
        assert!(away.contains(&-54));
        assert_eq!(calls.last().unwrap(), &(0x19, vec![(1, 0), (1, 1)], 0));
    }

    #[test]
    fn confirm_while_typing_completes_the_page() {
        let mut w = RowWindow::open(2);
        // Setup + a few typing calls, then a press.
        let calls = until_wait(&mut w, 2, &[25, 30], &[5]);
        // The press call jumps to 0x0D with the latch raised; the next adds
        // the second row, the one after ends the page.
        assert_eq!(calls[5].0, 0x0D);
        assert_eq!(w.speed, SPEED_SKIP);
        assert_eq!(calls[6], (0x0D, vec![(0, 0), (0, 1)], 0));
        assert_eq!(calls[7], (0x19, vec![(0, 0), (0, 1)], 0));
        // A page turn resets the speed to the typing speed.
        w.turn_page(1, true);
        until_wait(&mut w, 2, &[10], &[]);
        assert_eq!(w.speed, SPEED_TYPE);
    }

    #[test]
    fn a_press_during_a_hold_latches_the_skip_and_clears_the_hold() {
        let mut w = RowWindow::open(2);
        // Type row 0 (count 11) to its finish: a 92 hold.
        loop {
            w.call(2, false, |_| 11);
            if w.pacer.hold != 0 {
                break;
            }
        }
        assert_eq!(w.pacer.hold, 92);
        w.call(2, true, |_| 11);
        assert_eq!(w.pacer.hold, 0);
        assert_eq!(w.speed, SPEED_SKIP);
        w.call(2, false, |_| 11);
        assert_eq!(w.phase, WindowPhase::Complete);
    }

    #[test]
    fn a_completed_page_scrolls_its_overflow_through_at_the_skip_speed() {
        // A five-line page skipped on its first row: 0x0D fills the window to
        // four rows, then 0x0E scrolls at 74 a call, a line per row.
        let mut w = RowWindow::open(5);
        let calls = until_wait(&mut w, 2, &[20; 5], &[1]);
        let e: Vec<&Snap> = calls.iter().filter(|c| c.0 == 0x0E).collect();
        assert_eq!(e[0].2, -74);
        assert!(calls.iter().any(|c| c.1.len() == 4));
        assert_eq!(calls.last().unwrap().1, vec![(0, 2), (0, 3), (0, 4)]);
        assert_eq!(calls.last().unwrap().2, 0);
    }

    #[test]
    fn a_fresh_box_clears_the_window() {
        let mut w = RowWindow::open(2);
        until_wait(&mut w, 2, &[5, 5], &[]);
        w.turn_page(1, false);
        let calls = until_wait(&mut w, 2, &[5], &[]);
        assert_eq!(calls[1].1, vec![(1, 0)]);
    }
}
