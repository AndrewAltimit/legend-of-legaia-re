//! Retail typewriter pacing of the field dialog pager - the reveal counter,
//! its speed accumulator and the short-row hold.
//!
//! REF: FUN_801D84D0 (the pager; the arithmetic below is its row gate)
//! REF: FUN_80036044 (the row's glyph count, [`legaia_font::typewriter_glyph_count`])
//!
//! The pager `FUN_801D84D0` (field overlay, PROT 0897) runs once per game
//! tick, i.e. every `DAT_1F800393` vsyncs, and keeps three words for the row
//! it is typing, read off the disassembly:
//!
//! - **Setup.** The call that opens a box (or re-opens one after a page turn)
//!   stores the reveal counter `_DAT_801F2748 = 1` and the accumulator
//!   `_DAT_801F2758 = 0` (`0x801D8980..0x801D8990`) - one glyph shows on the
//!   opening call, before any accumulation.
//! - **Accumulate.** Each typing call adds `min(DAT_1F800393, 4)` to the
//!   accumulator, moves the counter by as many whole speed units
//!   (`_DAT_801F2754`, `1` on every pager open) as that holds, and caps the
//!   step at three when it reaches four (`0x801D8A08..0x801D8A70`).
//! - **Row gate.** The same call counts the row with `FUN_80036044` (`jal` at
//!   `0x801D8A6C`, argument the row pointer, lead byte included) and finishes
//!   the row once the count is **below** the counter (`slt v1,s0,v1` at
//!   `0x801D8A7C`) - one call after the counter first equals the count. The
//!   finish zeroes the counter and the accumulator and, for a count under
//!   `0x22`, stores the hold `_DAT_801F275C = (0x22 - count) * 4`
//!   (`0x801D8A88..0x801D8AA8`).
//! - **Hold.** A call that finds the hold non-zero runs dispatch case `0x10`
//!   instead of typing and drains it by `32 * min(DAT_1F800393, 4)`, clamping
//!   at zero (`0x801D866C..0x801D8690`); the next row's first typing call is
//!   the one after the hold reaches zero.
//!
//! The draw caps the typed row at the counter (`FUN_80036888`'s third
//! argument), so the counter is how many units of the row are visible.
//!
//! A PCSX-Redux trace of a town01 conversation
//! (`scripts/pcsx-redux/autorun_dialog_typewriter_trace.lua`) shows the
//! arithmetic at the field's `DAT_1F800393 = 2`: the counter runs
//! `1, 3, 5, ...` on the opening row and `2, 4, 6, ...` after a finish, a
//! count-25 row finishes on the call whose counter would be 27 and holds
//! `36` for one call, a count-11 row holds `92` for two. The unit tests below
//! pin that sequence.

/// Rows shorter than this many units hold after they finish.
pub const SHORT_ROW_UNITS: u32 = 0x22;

/// The pager's speed word `_DAT_801F2754` - `1` on every pager open
/// (`0x801D9118` / `0x801D91D0` / `0x801D9268` / `0x801D9CA4`).
pub const PAGER_SPEED: i32 = 1;

/// A pacer with nothing typing (every word zero) - what a panel off the
/// pager path reports.
pub const IDLE_PACER: TypewriterPacer = TypewriterPacer {
    counter: 0,
    acc: 0,
    hold: 0,
    setup: false,
};

/// What one pager call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagerCall {
    /// The counter moved (or was set by the box-opening call).
    Typed,
    /// The short-row hold absorbed the call.
    Held,
    /// The counter passed the row's count: the row is finished and the next
    /// row starts at counter `0` (after any hold).
    RowFinished,
}

/// The pager's per-row typewriter state (`_DAT_801F2748` / `_DAT_801F2758` /
/// `_DAT_801F275C`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypewriterPacer {
    /// Reveal counter `_DAT_801F2748`: units of the current row drawn.
    pub counter: i32,
    /// Speed accumulator `_DAT_801F2758`.
    pub acc: i32,
    /// Short-row hold `_DAT_801F275C`.
    pub hold: i32,
    /// The next call is a box-opening call.
    setup: bool,
}

impl TypewriterPacer {
    /// A pacer whose first call opens a box.
    pub fn new() -> Self {
        Self {
            setup: true,
            ..Self::default()
        }
    }

    /// Arm the box-opening call: the next [`Self::call`] sets the counter to
    /// `1` instead of accumulating (a new box, or a page turned in place).
    pub fn begin_box(&mut self) {
        *self = Self::new();
    }

    /// Units of the current row the draw shows.
    pub fn visible_units(&self) -> u32 {
        self.counter.max(0) as u32
    }

    /// One pager call at frame step `dt` (`DAT_1F800393`) against the current
    /// row's glyph count `row_count` (`FUN_80036044` on the row).
    pub fn call(&mut self, dt: u8, row_count: u32) -> PagerCall {
        if std::mem::take(&mut self.setup) {
            self.counter = 1;
            self.acc = 0;
            self.hold = 0;
            return PagerCall::Typed;
        }
        // `lbu s2, 0x393(0x1F80)`; `slti v0,s2,5` / `li s2,4`.
        let step = i32::from(dt.min(4));
        if self.hold != 0 {
            self.hold -= step << 5;
            if self.hold <= 0 {
                self.hold = 0;
            }
            return PagerCall::Held;
        }
        self.acc += step;
        let mut moved = 0;
        while self.acc >= PAGER_SPEED {
            self.acc -= PAGER_SPEED;
            moved += 1;
        }
        if moved >= 4 {
            moved = 3;
        }
        self.counter += moved;
        let count = row_count as i32;
        if count < self.counter {
            self.acc = 0;
            self.counter = 0;
            if row_count < SHORT_ROW_UNITS {
                self.hold = (SHORT_ROW_UNITS as i32 - count) * 4;
            }
            return PagerCall::RowFinished;
        }
        PagerCall::Typed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-call `(counter, hold)` after each call, for rows of the given
    /// counts typed back to back from a box opening.
    fn run(dt: u8, counts: &[u32]) -> Vec<(i32, i32)> {
        let mut p = TypewriterPacer::new();
        let mut out = Vec::new();
        for &c in counts {
            // A row after the first waits out the previous row's hold (the
            // rows share a box; the last row's hold is the page end's to
            // clear).
            while p.hold != 0 {
                assert_eq!(p.call(dt, c), PagerCall::Held);
                out.push((p.counter, p.hold));
            }
            loop {
                let r = p.call(dt, c);
                out.push((p.counter, p.hold));
                if r == PagerCall::RowFinished {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn town01_capture_first_box_at_frame_step_two() {
        // Retail trace (autorun_dialog_typewriter_trace.lua, town01 P1[16],
        // one row per pager call = every other vsync): row 0 has count 25,
        // row 1 count 30.
        let got = run(2, &[25, 30]);
        let mut want = Vec::new();
        for c in (1..=25).step_by(2) {
            want.push((c, 0));
        }
        want.push((0, 36)); // finish: counter would be 27 > 25
        want.push((0, 0)); // one held call drains 36 by 64
        for c in (2..=30).step_by(2) {
            want.push((c, 0));
        }
        want.push((0, 16)); // finish: 32 > 30; the page ends here
        assert_eq!(got, want);
    }

    #[test]
    fn a_short_row_holds_ceil_of_its_hold_over_the_drain() {
        // Captured: count 11 -> hold 92 -> 28 -> 0 (two held calls);
        // count 16 -> 72 -> 8 -> 0 (two held calls); count 32 -> 8 -> 0.
        for (count, holds) in [
            (11u32, vec![92, 28, 0]),
            (16, vec![72, 8, 0]),
            (32, vec![8, 0]),
        ] {
            let mut p = TypewriterPacer::new();
            while p.call(2, count) != PagerCall::RowFinished {}
            let mut seen = vec![p.hold];
            while p.hold != 0 {
                assert_eq!(p.call(2, count), PagerCall::Held);
                seen.push(p.hold);
            }
            assert_eq!(seen, holds, "count {count}");
        }
    }

    #[test]
    fn a_row_finishes_one_call_after_the_counter_meets_its_count() {
        let mut p = TypewriterPacer::new();
        let mut calls = 0;
        loop {
            calls += 1;
            if p.call(1, 5) == PagerCall::RowFinished {
                break;
            }
        }
        // Setup (1), then 2, 3, 4, 5, then the finishing call (6 > 5).
        assert_eq!(calls, 6);
        assert_eq!(p.hold, (0x22 - 5) * 4);
        // A long row does not hold.
        let mut p = TypewriterPacer::new();
        while p.call(1, 0x22) != PagerCall::RowFinished {}
        assert_eq!(p.hold, 0);
    }

    #[test]
    fn the_step_caps_at_three_units_a_call() {
        let mut p = TypewriterPacer::new();
        p.call(4, 100);
        p.call(4, 100);
        assert_eq!(p.counter, 1 + 3);
        // The frame-step byte itself is capped at 4 before accumulating.
        p.call(9, 100);
        assert_eq!(p.counter, 1 + 3 + 3);
    }
}
