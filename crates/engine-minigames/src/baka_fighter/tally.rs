//! The end-of-match score tally and its drain cadence.

use super::*;

// --- End-of-match score tally ----------------------------------------------

/// Remainder above which the tally drains at [`TALLY_DIVISOR_FAST`] per step.
pub const TALLY_FAST_THRESHOLD: i32 = 5;
/// Remainder below which the tally drains one unit per step.
pub const TALLY_SLOW_THRESHOLD: i32 = 3;
/// Divisor applied to a large remainder (`> TALLY_FAST_THRESHOLD`).
pub const TALLY_DIVISOR_FAST: i32 = 5;
/// Divisor applied to a mid-sized remainder.
pub const TALLY_DIVISOR_MID: i32 = 2;

/// How much the end-of-match tally moves out of a counter this frame, given
/// the amount still to drain.
///
/// The tally screen animates four score counters emptying into the running
/// total and the player's gold. The step is proportional, not linear, so a big
/// remainder empties fast and the last few units tick over one at a time:
/// `> 5` drains a fifth per frame, `3..=5` a half, and `< 3` exactly one - so
/// the counter always reaches zero rather than asymptotically approaching it.
///
/// `skip` is the tally's fast-forward flag (`DAT_801dbf00`): when set the whole
/// remainder moves in one step, which is what makes holding the button snap
/// the tally to its end state.
// PORT: FUN_801d6710 (tally drain step; the doc's "digit drawer" reading is
// wrong - this function draws nothing, it is the per-frame drain rate)
// REF: FUN_801d14b0 (the same routine linked into the PROT 0977 hub overlay)
// Wired: [`BakaTally::tick`] (the port of `FUN_801d239c`) calls this for
// every counter step, and [`BakaFight`] runs a tally once the match is
// decided, so the drain rate paces the prize actually reaching party gold.
//
// `FUN_801D6710` and the hub overlay's `FUN_801D14B0` are **one routine linked
// twice**: the two dumps are 24 instructions each and agree opcode for opcode
// and register for register, differing only in the `lui`/`lw` pair that loads
// the bypass flag (`DAT_801DBF00` here, `DAT_801D1AB4` there) and in the
// relocated branch targets. So the port holds one implementation and this entry
// delegates to it - the shape the `sin_4096` incident argues for, where two
// reproductions of one table disagreed and nothing compared them.
pub fn tally_drain_step(remaining: i32, skip: bool) -> i32 {
    crate::other_game_overlay::step_scale(remaining, skip)
}

/// Run one counter of the tally to empty, returning the per-frame steps it
/// takes. Each step is [`tally_drain_step`] of what is left; the sum is the
/// original `amount`.
///
/// Retail drains a negative counter by the same rule, which would run away
/// from zero - no call site produces one, and the port treats it as empty.
pub fn tally_drain_sequence(amount: i32) -> Vec<i32> {
    let mut left = amount.max(0);
    let mut steps = Vec::new();
    while left > 0 {
        let step = tally_drain_step(left, false).clamp(1, left);
        steps.push(step);
        left -= step;
    }
    steps
}

/// Frame-steps a tally row must have been on screen before its counter is
/// allowed to drain (`fade < 0x11` stalls the row in `FUN_801d239c`).
pub const TALLY_FADE_GATE: i32 = 0x11;

/// Number of counters the end-of-match tally drains.
pub const TALLY_COUNTERS: usize = 4;

/// Index of the tally counter that pays into the player's gold rather than
/// into the on-screen score total (`DAT_801dbee8` → `_DAT_80084440`).
pub const TALLY_GOLD_COUNTER: usize = 3;

/// The end-of-match **score tally**: four counters draining, strictly in
/// order, into the running total and the player's gold.
///
/// PORT: FUN_801d239c (end-of-match score tally). The retail screen holds
/// four counters (`DAT_801dbee0` / `DAT_801dbed8` / `DAT_801dbedc` for the
/// score rows and `DAT_801dbee8` for the coin prize). Each row has its own
/// fade counter that advances by the frame step only once every *earlier*
/// row has emptied; a row starts draining when its fade reaches
/// [`TALLY_FADE_GATE`], moves [`tally_drain_step`] out per frame and queues
/// the tick blip ([`BAKA_CUE_CURSOR`]) on every step. The first three rows
/// feed the score total (`DAT_801dbee4`); the fourth feeds party gold.
///
/// The fast-forward latch is the retail one: `FUN_801d239c` opens by testing
/// the edge-triggered pad word `_DAT_8007b874 & 0xf0` (any face button) and
/// setting `DAT_801dbf00`, which makes [`tally_drain_step`] move each whole
/// remainder in a single step - so holding a button snaps the tally to its
/// end state. The latch is never cleared inside the tally, matching retail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakaTally {
    counters: [i32; TALLY_COUNTERS],
    fade: [i32; TALLY_COUNTERS],
    total: i32,
    gold_drained: i32,
    gold_pending: i32,
    fast_forward: bool,
    cues: Vec<u8>,
}

impl BakaTally {
    /// Start a tally over the four counters, in retail row order (the three
    /// score rows first, the coin prize last).
    pub fn new(counters: [i32; TALLY_COUNTERS]) -> Self {
        Self {
            counters: counters.map(|c| c.max(0)),
            fade: [0; TALLY_COUNTERS],
            total: 0,
            gold_drained: 0,
            gold_pending: 0,
            fast_forward: false,
            cues: Vec::new(),
        }
    }

    /// Advance the tally one frame. `frame_step` is the global frame-rate
    /// step; `face_button` is this frame's edge-triggered face-button mask
    /// test (`_DAT_8007b874 & 0xf0`), which latches the fast-forward.
    pub fn tick(&mut self, frame_step: i32, face_button: bool) {
        if face_button {
            self.fast_forward = true;
        }
        for i in 0..TALLY_COUNTERS {
            // A row is only reached once every earlier row has emptied.
            if self.counters[..i].iter().any(|&c| c != 0) {
                break;
            }
            self.fade[i] += frame_step;
            if self.counters[i] == 0 {
                continue;
            }
            if self.fade[i] < TALLY_FADE_GATE {
                break;
            }
            let step =
                tally_drain_step(self.counters[i], self.fast_forward).clamp(1, self.counters[i]);
            self.cues.push(BAKA_CUE_CURSOR);
            self.counters[i] -= step;
            if i == TALLY_GOLD_COUNTER {
                self.gold_drained += step;
                self.gold_pending += step;
            } else {
                self.total += step;
            }
            // No break: retail falls straight through into the next row's
            // section, so the frame that empties a row also advances the
            // following row's fade. That row cannot drain on the same frame
            // (its fade is still under the gate), but it does start a frame
            // earlier than a break here would allow. If this row did *not*
            // empty, the next iteration's own guard stops the sweep.
        }
    }

    /// `true` once every counter has emptied.
    pub fn done(&self) -> bool {
        self.counters.iter().all(|&c| c == 0)
    }

    /// The counters still to drain.
    pub fn counters(&self) -> [i32; TALLY_COUNTERS] {
        self.counters
    }

    /// The on-screen score total accumulated so far (`DAT_801dbee4`).
    pub fn total(&self) -> i32 {
        self.total
    }

    /// Coins moved out of the prize counter so far.
    pub fn gold_drained(&self) -> i32 {
        self.gold_drained
    }

    /// Coins the prize counter has not paid out yet.
    pub fn gold_remaining(&self) -> i32 {
        self.counters[TALLY_GOLD_COUNTER]
    }

    /// Take the coins drained since the last call, for the host to add to
    /// party gold (retail adds each step straight into `_DAT_80084440`).
    pub fn take_gold(&mut self) -> i32 {
        std::mem::take(&mut self.gold_pending)
    }

    /// Drain the tick blips queued since the last call.
    pub fn take_cues(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.cues)
    }
}
