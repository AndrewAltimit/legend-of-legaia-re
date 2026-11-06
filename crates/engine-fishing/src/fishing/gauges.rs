//! Reel input, cast power, the tension gauge and the persistent catch record.

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
/// locks it, setting the cast distance. The step is [`CAST_POWER_STEP`] per
/// frame-step (`dir << 5` times `DAT_1F800393`, `0x801CFB3C..0x801CFB5C`).
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
    /// A fresh oscillator on the wind-up's exit: state `0x13` zeroes the meter
    /// as it enters state `0x14` (`0x801CFA60`). The direction word stands
    /// across casts in retail and cannot matter here, since the first step from
    /// zero lands on the floor either way.
    pub fn new() -> Self {
        Self {
            power: 0,
            dir: 1,
            locked: false,
        }
    }

    /// The meter as the run-loop init leaves it for a cast: state `10` stores
    /// `0x40` to `DAT_801D9274` and `1` to its direction `DAT_801D9278`. The
    /// wind-up (state `0xD`) falls through into the meter body, so the meter
    /// runs from here until the wind-up's exit zeroes it.
    pub fn seeded() -> Self {
        Self {
            power: CAST_POWER_SEED,
            dir: 1,
            locked: false,
        }
    }

    /// Zero the meter, as the wind-up's exit does (`sw zero` at `0x801CFA60`).
    pub fn zero(&mut self) {
        self.power = 0;
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
        if p > CAST_POWER_MAX {
            p = CAST_POWER_MAX;
            self.dir = -1;
        } else if p < CAST_POWER_MIN {
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

    /// Apply one frame of the reel arms against the fish's reel `load`
    /// ([`FishFrame::load`]), scaled by the frame step `frame_step`
    /// (`DAT_1f800393`), then clamp.
    ///
    /// `reel_mask` is the held pad word `_DAT_8007b850`. Each reel bit runs
    /// its own arm, so with both held both add (`0x801D44D4` / `0x801D4548`):
    ///
    /// ```text
    /// 0x40 held:  tension += (load / 3) / (rod * 9 + 0x23) * step
    /// 0x80 held:  tension += (load / 3) / (rod * 6 + 0x19) * step
    /// neither:    tension -= (rod * 0x40 + 0x4a) * step
    /// ```
    ///
    /// `/ 3` is the reciprocal `0x55555556` (`hi - sign`, toward zero), the
    /// rod division a `divu`, the step a `mult` / `mflo`
    /// (`0x801D44E0..0x801D4540`). One clamp to `[0, 0x1000]` follows both
    /// arms (`0x801D4604..0x801D462C`).
    // PORT: FUN_801d4004 (tension-gauge integration, reel held / released)
    pub fn apply_load(&mut self, reel_mask: u32, load: i32, frame_step: i32) {
        let fs = frame_step.max(1);
        let third = (load / 3) as u32;
        let mut t = self.tension;
        if reel_mask & REEL_A_PAD_BIT != 0 {
            let div = (self.rod_stat * REEL_A_DIV_MUL + REEL_A_DIV_ADD) as u32;
            t = t.wrapping_add(((third / div) as i32).wrapping_mul(fs));
        }
        if reel_mask & REEL_B_PAD_BIT != 0 {
            let div = (self.rod_stat * REEL_B_DIV_MUL + REEL_B_DIV_ADD) as u32;
            t = t.wrapping_add(((third / div) as i32).wrapping_mul(fs));
        }
        if reel_mask & (REEL_A_PAD_BIT | REEL_B_PAD_BIT) == 0 {
            t -= (self.rod_stat * REEL_RELEASE_MUL + REEL_RELEASE_ADD) * fs;
        }
        self.tension = t.clamp(TENSION_MIN, TENSION_MAX);
    }

    /// The lure tick's far-record strain: a hooked fish whose line record
    /// (`DAT_801D927C`) is at or past [`FAR_RECORD_STRAIN_FROM`] adds
    /// [`FAR_RECORD_STRAIN`] per frame step to the gauge, after the reel
    /// arms and ahead of the ceiling test (`0x801D3B34..0x801D3B8C`), which
    /// clamps to [`TENSION_MAX`] (`0x801D3B9C..0x801D3BAC`).
    ///
    /// Retail gates it on `DAT_801D91B4` (hooked) and `DAT_801D91B0 == 0`.
    /// The only non-zero store to `DAT_801D91B0` is the line actor's
    /// landed-lift arm (`FUN_801D4948` state `4`, `0x801D4AEC`), which runs
    /// after the reel-in has ended the fight, so the gate is open for the
    /// whole hooked phase. Returns whether the strain applied.
    // PORT: FUN_801d26cc (the far-record tension strain, `0x801D3B34..0x801D3B8C`)
    pub fn far_record_strain(&mut self, line_record: i32, frame_step: i32) -> bool {
        if line_record < FAR_RECORD_STRAIN_FROM {
            return false;
        }
        let fs = frame_step.max(1);
        self.tension = (self.tension + FAR_RECORD_STRAIN * fs).min(TENSION_MAX);
        true
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
