//! Reel input, cast power, the tension gauge and the persistent catch record.
//! Split out of `fishing.rs`.

use super::*;

/// The reel-input state this frame. The retail held mask is `_DAT_8007b850`
/// bits `0x40` / `0x80`, which are now pinned to physical buttons via the pad
/// packer `FUN_8001822C`: `0x40` = Cross, `0x80` = Square (reel B is Square,
/// NOT Circle; Circle `0x20` is the cast/hook input). See the fishing doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReelInput {
    /// Neither reel button held - tension bleeds off.
    Idle,
    /// The `0x40` reel button (Cross; the `rod*9 + 0x23`-divisor path).
    ReelA,
    /// The `0x80` reel button (Square; the `rod*6 + 0x19`-divisor path).
    ReelB,
}

impl ReelInput {
    /// Decode the pad held-mask (`_DAT_8007b850`) into a reel input.
    ///
    /// Cross (`0x40`) takes priority and selects reel A; Square (`0x80`)
    /// without Cross selects reel B; anything else is idle. This mirrors the
    /// retail decoder's three-way branch exactly - holding both reel buttons
    /// resolves to reel A, not a blend - so `0x40 -> ReelA`, `0x80 -> ReelB`,
    /// else `Idle`. The retail body is `if (m & 0x40) return 1; else return
    /// (m >> 6) & 2;`, whose `1` / `2` / `0` results map onto these variants.
    ///
    /// Wired: `World::tick_fishing` assembles the two reel bits out of this
    /// frame's held pad and decodes them here, so the priority rule is the
    /// ported one rather than a host `if` chain.
    // PORT: FUN_801d7450 (reel-button decoder)
    pub fn from_pad_mask(mask: u32) -> Self {
        if mask & 0x40 != 0 {
            ReelInput::ReelA
        } else if (mask >> 6) & 2 != 0 {
            ReelInput::ReelB
        } else {
            ReelInput::Idle
        }
    }
}

/// The casting-power oscillator (`FUN_801cf3bc` state `0x14`): a value that
/// bounces between [`CAST_POWER_MIN`] and [`CAST_POWER_MAX`] until the player
/// locks it, setting the cast distance. The per-frame `step` magnitude is not
/// byte-pinned in the dumps, so it is a caller parameter (the retail meter
/// visibly sweeps the full range in well under a second).
#[derive(Debug, Clone, Copy)]
pub struct CastPower {
    pub(super) power: i32,
    /// Oscillation direction (`DAT_801d9278`, `+1` / `-1`).
    pub(super) dir: i32,
    pub(super) locked: bool,
}

impl Default for CastPower {
    fn default() -> Self {
        Self::new()
    }
}

impl CastPower {
    /// A fresh oscillator seeded at [`CAST_POWER_SEED`], sweeping upward.
    pub fn new() -> Self {
        Self {
            power: CAST_POWER_SEED,
            dir: 1,
            locked: false,
        }
    }

    /// Current meter value.
    pub fn value(&self) -> i32 {
        self.power
    }

    /// `true` once [`Self::lock`] has fixed the meter.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Advance the meter by `step`, bouncing off the `[0x20, 0x1000]` bounds and
    /// flipping direction. No-op once locked.
    // PORT: FUN_801cf3bc state 0x14 (casting-power oscillator + direction flip)
    pub fn advance(&mut self, step: i32) {
        if self.locked {
            return;
        }
        let step = step.max(1);
        let mut p = self.power + self.dir * step;
        if p >= CAST_POWER_MAX {
            p = CAST_POWER_MAX;
            self.dir = -1;
        } else if p <= CAST_POWER_MIN {
            p = CAST_POWER_MIN;
            self.dir = 1;
        }
        self.power = p;
    }

    /// Lock the meter at its current value and return it (the cast distance).
    pub fn lock(&mut self) -> i32 {
        self.locked = true;
        self.power
    }
}

/// The tension gauge (`DAT_801d9168`): a `[0, 0x1000]` tug-of-war raised by
/// reeling and bled off when the reel is released. `rod_stat` is the persistent
/// rod / upgrade stat (`_DAT_80084454`); a higher value softens both the
/// reel-in spike and the bleed-off.
#[derive(Debug, Clone, Copy)]
pub struct TensionGauge {
    pub(super) tension: i32,
    pub(super) rod_stat: i32,
}

impl TensionGauge {
    /// A slack gauge for a rod of the given persistent stat.
    pub fn new(rod_stat: i32) -> Self {
        Self {
            tension: 0,
            rod_stat: rod_stat.max(0),
        }
    }

    /// Current tension, `0..=0x1000`.
    pub fn tension(&self) -> i32 {
        self.tension
    }

    /// `true` when tension is pinned at [`TENSION_MAX`] (the line-snap edge).
    pub fn at_max(&self) -> bool {
        self.tension >= TENSION_MAX
    }

    /// Apply one frame of reel input against a fish pulling with `base_pull`,
    /// scaled by the frame step `frame_step` (`DAT_1f800393`), then clamp.
    ///
    /// Confirmed (`FUN_801d4004` tail): the reel-held divisors
    /// (`rod*9 + 0x23` / `rod*6 + 0x19`) and the reel-released decrement
    /// `(rod*0x40 + 0x4a) * frame_step`, and the `[0, 0x1000]` clamp. The
    /// held-path grouping `base_pull * frame_step / divisor` is the natural
    /// integer reading (a stronger fish pull spikes tension faster); the exact
    /// MIPS operand order of the held term is not separately pinned.
    // PORT: FUN_801d4004 (tension-gauge integration, reel held / released)
    pub fn apply_reel(&mut self, input: ReelInput, base_pull: i32, frame_step: i32) {
        let fs = frame_step.max(1);
        let delta = match input {
            ReelInput::ReelA => {
                let div = (self.rod_stat * REEL_A_DIV_MUL + REEL_A_DIV_ADD).max(1);
                base_pull.max(0) * fs / div
            }
            ReelInput::ReelB => {
                let div = (self.rod_stat * REEL_B_DIV_MUL + REEL_B_DIV_ADD).max(1);
                base_pull.max(0) * fs / div
            }
            ReelInput::Idle => -((self.rod_stat * REEL_RELEASE_MUL + REEL_RELEASE_ADD) * fs),
        };
        self.tension = (self.tension + delta).clamp(TENSION_MIN, TENSION_MAX);
    }
}

/// The persistent fishing record (`_DAT_8008444c` / `_DAT_80084458` /
/// `_DAT_8008445c`): the running point total and the best single catch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishingRecord {
    /// Accumulated fishing points, capped at [`FISH_POINTS_CAP`].
    pub points: i32,
    /// Best single-catch point value seen.
    pub best_points: i32,
    /// Fish id of the best catch.
    pub best_fish: usize,
}

impl FishingRecord {
    /// Credit a landed catch worth `award` points from species `fish_id`
    /// (`FUN_801d5298`): add to the capped point total and, if it beats the
    /// current best, update the best value + fish id. Returns the awarded
    /// points (post-cap contribution is not clamped away from the return - the
    /// caller sees the raw award).
    // PORT: FUN_801d5298 (persistent point credit + best-catch update)
    pub fn credit(&mut self, fish_id: usize, award: i32) {
        let award = award.max(0);
        self.points = (self.points + award).min(FISH_POINTS_CAP);
        if award > self.best_points {
            self.best_points = award;
            self.best_fish = fish_id;
        }
    }
}
