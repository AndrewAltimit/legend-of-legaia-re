//! The field overlay's three **frame-delta timer** templates - the pool
//! actors whose whole body is a clock and one output.
//!
//! Three consecutive-ish records in the field overlay's own template table
//! share the plain shape (`+0x06 = 0xFFFF`, a tick at `+0x08`, every other
//! word zero), each materialised by one `lui a0, 0x801f` / `jal 0x80020de0`
//! pair against the actor pool `_DAT_8007C34C`, and each spends the frame
//! delta `DAT_1F800393` on something different:
//!
//! | Template | Tick | Spawner | Output |
//! |---|---|---|---|
//! | `0x801F2858` | `FUN_801DD784` | `FUN_801DE754` | two black screen-space quads (the cinematic bars) |
//! | `0x801F2840` | `FUN_801DD4C4` | `FUN_801DE698` | a **second** actor's position triple |
//! | `0x801F27EC` | `FUN_801DA930` | `FUN_801DDE34` | one halfword of the scene floor-height ladder |
//!
//! Each ported entry carries its `PORT` tag on the Rust item that implements
//! it, never at module level, so the liveness audit sees one anchor per port
//! site.
//!
//! REF: FUN_80020DE0 (the allocator every one of the three goes through),
//!      FUN_801DE754, FUN_801DE698, FUN_801DDE34 (the three spawners, whose
//!      operand decode lives in [`crate::field`]),
//!      FUN_8003D2C4 (the OT link the bar emitter ends on)
//!
//! # Provenance
//!
//! All three ticks are in the field overlay image (`field(897)`, slot-A base
//! `0x801CE818`), and the VA ownership is settled from the bytes rather than
//! from a dump filename: `scripts/ghidra-analysis/dump-extent-attribution.csv`
//! classes `801dd784`, `801dd4c4` and `801da930` all `unique / field(897)`.
//! `ghidra/scripts/funcs/overlay_field_0897_801dd784.txt`,
//! `..._801dd4c4.txt` and `..._801da930.txt` are the base-tagged dumps; the
//! spawners were read straight out of
//! `extracted/overlays/overlay_field_0897.bin` with
//! `scripts/ghidra-analysis/disasm-overlay-fn.py --base 0x801CE818`, because
//! the untagged `ghidra/scripts/funcs/overlay_0897_801dde34.txt` and
//! `..._801de698.txt` hold a *different* routine printed at those VAs.
//!
//! # The bar emitter is a blackout, not a 2.35:1 crop
//!
//! `FUN_801DD784`'s envelope tops out at `0x73` = 115 and it emits **two**
//! bars of that height into a `0x140 x 0xE0` (320x224) screen. Twice 115 is
//! 230, which is more than 224: at full envelope the two quads overlap by two
//! scanlines and the frame is entirely black. So the four phases are *close / hold / open /
//! retire* - a shutter wipe to black and back - and the intermediate frames
//! are what read as a letterbox. Anything that treats `0x73` as a fixed bar
//! height for a cinematic crop is reading the ramp and missing the peak.
//!
//! # `0x801F27EC` is the floor-height ladder, not a fade
//!
//! `FUN_801DA930` stores `pos >> 16` into `0x1F800314 + 0x48 + slot * 2` -
//! i.e. `0x1F80035C + slot * 2`, which is the scene's **16-entry `i16`
//! floor-elevation LUT**: `FUN_8003AEB0` fills it from the MAN header at
//! scene entry, `FUN_80019278` bilinearly interpolates it for ground height,
//! `FUN_8003A55C` adds `LUT[cell & 0xF]` to every placed object's Y, and the
//! field VM's own `4C 9E` writes all sixteen entries at once. So the whole
//! `4C 9x` family is the height ladder: sub-`0xE` installs it, sub-`0..2`
//! animates one rung, sub-`0xF` retires the animator. Calling any of them a
//! "fade" is a mislabel that survived because the tick's output was never
//! resolved to its destination.
//!
//! # A retail arm with no exit
//!
//! The spawner writes the caller's sub-op into `+0x6C` and the tick seeds
//! `phase = +0x6C + 1`, so the field VM's sub-`0`/`1`/`2` produce phase
//! `1`/`2`/`3`. Only phase `1` decrements the tick's outer counter
//! (`0x801DAA3C`); phase `>= 2` falls to the loop test at `0x801DAA40` with
//! the counter untouched, which is an unconditional infinite loop. Retail
//! therefore hangs on sub-`1` and sub-`2`, and the port must not: it treats
//! phase `>= 2` as "idle, stop iterating". That is a deliberate, disclosed
//! divergence, and it is only reachable by a script that emits `4C 91` or
//! `4C 92`.

/// Retail's per-frame delta byte at scratchpad `0x1F800393` - how many game
/// ticks this frame spans. Every kernel here advances by it.
pub const FRAME_DELTA_SCRATCH_VA: u32 = 0x1F80_0393;

/// Actor flag bit `+0x10 & 8` - "retire me at the end of this frame". Two of
/// the three ticks below latch it when their clock lands.
pub const ACTOR_FLAG_RETIRE: u32 = 0x8;

// ---------------------------------------------------------------------------
// 0x801F2858 / FUN_801DD784 - the cinematic bar emitter
// ---------------------------------------------------------------------------

/// Spawn-descriptor VA of the bar-emitter template.
pub const LETTERBOX_TEMPLATE_VA: u32 = 0x801F_2858;

/// Full envelope height in scanlines (retail literal `0x73`). Both bars reach
/// it together, which covers a 224-line screen twice over - see the module
/// note.
pub const LETTERBOX_FULL_BAR: i16 = 0x73;

/// Right edge of both quads (retail literal `0x140`).
pub const LETTERBOX_SCREEN_W: i16 = 0x140;

/// Bottom edge retail hardcodes (`0xE0` = 224). The engine's screen-overlay
/// stage is 240 lines, so the prim builder takes the stage height as a
/// parameter and this constant is what a parity check compares against.
pub const LETTERBOX_RETAIL_SCREEN_H: i16 = 0xE0;

/// The top bar's Y bias (retail writes `-4` into both of its top vertices, so
/// the bar starts four lines above the screen and its visible height is
/// `bar - 4`).
pub const LETTERBOX_TOP_BIAS: i16 = -4;

/// The four-phase envelope behind the cinematic bars.
///
/// `durations[phase]` is the tick budget of each phase, exactly as the
/// spawner lays them out: `[close, hold, open, unused]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LetterboxBars {
    /// `+0x54` - phase index.
    pub phase: i16,
    /// `+0x9E` - phase clock, reset at every phase boundary.
    pub clock: u16,
    /// `+0xB8 / +0xBA / +0xBC / +0xBE` - per-phase durations.
    pub durations: [i16; 4],
    /// `+0x10 & 8` - latched by phase 3.
    pub retired: bool,
}

impl LetterboxBars {
    /// Retail's spawn (`FUN_801DE754`): allocate from `0x801F2858`, clear the
    /// phase and clock, and write the three operand bytes into
    /// `+0xB8 / +0xBA / +0xBC`.
    ///
    /// REF: FUN_801DE754 (`0x801DE754..0x801DE7B8`) - the field VM reaches it
    /// from op `0x43` sub-`0xC`, a 5-byte `[43, 0C, close, hold, open]`.
    /// `FUN_801CFF3C` is not a second spawner. Its dump
    /// (`ghidra/scripts/funcs/overlay_0897_xxx_dat_801cff3c.txt`) is the same
    /// 26 instructions with the same `addiu a0, a0, 0x2858` immediate, and
    /// `0x801DE754 - 0x801CFF3C = 0xE818` is exactly the re-key delta
    /// `docs/tooling/phantom-print-index.md` records for the
    /// `overlay_0897_xxx_dat` dump program - one routine printed at two VAs.
    pub fn spawn(close: i16, hold: i16, open: i16) -> Self {
        LetterboxBars {
            phase: 0,
            clock: 0,
            durations: [close, hold, open, 0],
            retired: false,
        }
    }

    /// This phase's duration, `0` past the block retail would read off the
    /// end of.
    fn duration(&self) -> i16 {
        if self.phase < 0 {
            return 0;
        }
        self.durations
            .get(self.phase as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Advance one frame and return the current bar height in scanlines.
    ///
    /// PORT: FUN_801DD784 (`0x801DD784..0x801DD8F8` - the envelope; the tail
    /// from `0x801DD8F8` is the two-quad emit, which is
    /// [`crate::field_actor_timers::letterbox_bar_rects`] plus the host's OT
    /// link)
    ///
    /// The order is retail's and it matters: the clock is advanced *first*,
    /// the phase boundary is tested against the **old** phase's duration, and
    /// only then does the (possibly new) phase pick the height - so a phase
    /// always renders its first frame at clock `0`.
    ///
    /// Retail divides by the phase duration with a hardware `div`, so a zero
    /// duration traps. The port returns `0` for that phase instead, which is
    /// the only place it can differ from a running console.
    pub fn step(&mut self, frame_delta: u8) -> i16 {
        if self.retired {
            return 0;
        }
        self.clock = self.clock.wrapping_add(u16::from(frame_delta));
        if self.clock as i16 >= self.duration() {
            self.phase = self.phase.wrapping_add(1);
            self.clock = 0;
        }
        match self.phase {
            0 => ramp(self.clock, self.durations[0]),
            1 => LETTERBOX_FULL_BAR,
            2 => LETTERBOX_FULL_BAR - ramp(self.clock, self.durations[2]),
            3 => {
                self.retired = true;
                0
            }
            _ => 0,
        }
    }
}

/// `t * 0x73 / d`, retail's `sll`/`subu` chain for `* 115` followed by a
/// truncating `div`. `d <= 0` yields `0` (retail traps).
fn ramp(t: u16, d: i16) -> i16 {
    if d <= 0 {
        return 0;
    }
    ((i32::from(t) * i32::from(LETTERBOX_FULL_BAR)) / i32::from(d)) as i16
}

/// One bar as retail lays its four vertices out: `[x0, y0, x1, y1]` with the
/// quad spanning `x0..x1`, `y0..y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarRect {
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
}

/// The two quads a bar height emits, top first.
///
/// REF: FUN_801DD784 (`0x801DD8F8..0x801DD9B4`) - two `0x05000000`-tagged
/// packets, command byte `0x28` (`POLY_F4`, opaque), with the colour bytes
/// **zeroed after** the `0x28808080` word is stored, so the fill is black and
/// not the mid-grey the immediate suggests.
///
/// `screen_h` is the stage bottom the second bar is flush with. Retail's
/// literal is [`LETTERBOX_RETAIL_SCREEN_H`]; the engine's screen-overlay
/// stage is 240 lines, so the host passes its own display height and the bar
/// stays flush instead of floating 16 lines up.
pub fn letterbox_bar_rects(bar: i16, screen_h: i16) -> [BarRect; 2] {
    [
        BarRect {
            x0: 0,
            y0: LETTERBOX_TOP_BIAS,
            x1: LETTERBOX_SCREEN_W,
            y1: bar + LETTERBOX_TOP_BIAS,
        },
        BarRect {
            x0: 0,
            y0: screen_h - bar,
            x1: LETTERBOX_SCREEN_W,
            y1: screen_h,
        },
    ]
}

// ---------------------------------------------------------------------------
// 0x801F2840 / FUN_801DD4C4 - the three-axis eased move
// ---------------------------------------------------------------------------

/// Spawn-descriptor VA of the eased-move template.
pub const EASED_MOVE_TEMPLATE_VA: u32 = 0x801F_2840;

/// End value that disables an axis (`-1`, the same `0xFFFF` sentinel the
/// immediate branch of op `0x43` sub-9 skips).
pub const EASE_AXIS_DISABLED: i16 = -1;

/// Target flag bit that mirrors the eased Y into the target's `+0x8E`
/// inverted-Y slot. Retail's test is `lui at, 0x2000` - `0x2000_0000`, not
/// `0x2000`.
pub const EASE_TARGET_INVERT_Y: u32 = 0x2000_0000;

/// A quadratic ease of a **second** actor's position triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EasedMove {
    /// `+0x14 / +0x16 / +0x18` - the start triple, copied off the target at
    /// spawn.
    pub start: [i16; 3],
    /// `+0x24 / +0x26 / +0x28` - the end triple. `-1` disables that axis.
    pub end: [i16; 3],
    /// `+0x9E` - the move's length in ticks.
    pub duration: i16,
    /// `+0x50` - elapsed ticks, clamped at `duration`.
    pub clock: u16,
    /// `+0x10 & 8` - latched on arrival.
    pub retired: bool,
}

/// One frame of an [`EasedMove`]: the per-axis writes, `None` for a disabled
/// axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EasedMoveFrame {
    /// `target[+0x14 / +0x16 / +0x18]`.
    pub axis: [Option<i16>; 3],
    /// `target[+0x8E]` - written only when the target carries
    /// [`EASE_TARGET_INVERT_Y`] **and** the Y axis is live.
    pub mirror_y: Option<i16>,
    /// `true` on the frame the clock reached the duration.
    pub retired: bool,
}

impl EasedMove {
    /// Retail's spawn (`FUN_801DE698(target, &start_xyz, &end_xyz, ticks)`):
    /// back-link the target into `+0x90`, clear the clock, latch the
    /// duration, then copy three halfwords from each of the two blocks.
    ///
    /// REF: FUN_801DE698 (`0x801DE698..0x801DE750`). Its one field-VM caller
    /// is op `0x43` sub-9's non-zero-`ticks` branch at `0x801DF874`, which
    /// passes `&target[+0x14]` as the start block - i.e. **the start is the
    /// target's live position**, not a scripted one.
    pub fn spawn(start: [i16; 3], end: [i16; 3], duration: i16) -> Self {
        EasedMove {
            start,
            end,
            duration,
            clock: 0,
            retired: false,
        }
    }

    /// Advance one frame and return this frame's writes.
    ///
    /// PORT: FUN_801DD4C4 (`0x801DD4C4..0x801DD780`)
    ///
    /// Each live axis is `start + (end - start) * t^2 / d^2`, evaluated as
    /// **two** successive `mult`/`div` pairs with an integer truncation in
    /// between - `((delta * t) / d * t) / d` - so the port must not fold them
    /// into one expression. The curve is a plain ease-**in** (it accelerates
    /// the whole way and arrives at full speed), not an ease-in-out.
    ///
    /// `target_flags` is the target actor's `+0x10`; only
    /// [`EASE_TARGET_INVERT_Y`] is read.
    pub fn step(&mut self, frame_delta: u8, target_flags: u32) -> EasedMoveFrame {
        self.clock = self.clock.wrapping_add(u16::from(frame_delta));
        if i32::from(self.clock) >= i32::from(self.duration) {
            self.retired = true;
            self.clock = self.duration as u16;
        }
        let mut frame = EasedMoveFrame {
            retired: self.retired,
            ..Default::default()
        };
        for (axis, out) in frame.axis.iter_mut().enumerate() {
            *out = self.axis_value(axis);
        }
        if target_flags & EASE_TARGET_INVERT_Y != 0
            && let Some(y) = frame.axis[1]
        {
            frame.mirror_y = Some(y.wrapping_neg());
        }
        frame
    }

    /// The current value of one axis, `None` when the axis is disabled.
    fn axis_value(&self, axis: usize) -> Option<i16> {
        let end = self.end[axis];
        if end == EASE_AXIS_DISABLED {
            return None;
        }
        let start = self.start[axis];
        let d = i32::from(self.duration);
        let t = i32::from(self.clock);
        if end == start || t >= d || d <= 0 {
            return Some(end);
        }
        let delta = i32::from(end) - i32::from(start);
        let v = ((delta.wrapping_mul(t) / d).wrapping_mul(t)) / d;
        Some((i32::from(start) + v) as i16)
    }
}

// ---------------------------------------------------------------------------
// 0x801F27EC / FUN_801DA930 - the floor-height-ladder oscillator
// ---------------------------------------------------------------------------

/// Spawn-descriptor VA of the floor-ladder oscillator template.
pub const FLOOR_TIER_TEMPLATE_VA: u32 = 0x801F_27EC;

/// Scratchpad VA of the scene's 16-entry `i16` floor-elevation LUT
/// (`0x1F800314 + 0x48`) - the array this tick writes and the field VM's
/// `4C 9E` installs.
pub const FLOOR_HEIGHT_LUT_VA: u32 = 0x1F80_035C;

/// Number of rungs in that ladder (the low nibble of a collision byte).
pub const FLOOR_HEIGHT_LUT_LEN: usize = 16;

/// `+0x9E` bit that turns the tick into a "run N iterations right now" burst
/// instead of a per-frame step.
pub const FLOOR_TIER_ARM_BIT: u16 = 0x8000;

/// An undamped integer oscillator on one rung of the floor-height ladder.
///
/// Position and velocity are 16.16 fixed point; only the high halfword
/// reaches the LUT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FloorTierBob {
    /// `+0x50` - which of the 16 ladder rungs this actor drives.
    pub slot: u16,
    /// `+0x54` - phase. `0` seeds from [`Self::phase_seed`]; `1` runs;
    /// anything else idles (retail loops forever - see the module note).
    pub phase: i16,
    /// `+0x6C` - the spawner's sub-op; the seeded phase is this plus one.
    pub phase_seed: u8,
    /// `+0x80` - 16.16 position.
    pub pos: i32,
    /// `+0x84` - 16.16 velocity.
    pub vel: i32,
    /// `+0x88` - 16.16 acceleration, applied toward [`Self::target`].
    pub accel: i32,
    /// `+0x8C` - 16.16 rest height. Never rewritten, so the motion is an
    /// oscillation *about* the height the rung had at spawn.
    pub target: i32,
    /// `+0x9E` - burst-arm word.
    pub arm: u16,
}

impl FloorTierBob {
    /// Retail's spawn
    /// (`FUN_801DDE34(slot, sub_op, period, amplitude, arm)`).
    ///
    /// REF: FUN_801DDE34 (`0x801DDE34..0x801DDF44`). Its one field-VM caller
    /// is op `0x4C` outer-nibble-9 sub-`0..2` at `0x801E24E8`, a 9-byte
    /// `[4C, 9N, slot, period:i16, amplitude:i16, arm:i16]`.
    ///
    /// `seed_height` is the rung's current LUT value, which retail reads
    /// straight out of the scratchpad and installs as **both** the position
    /// and the rest height, so the rung starts where it already was.
    pub fn spawn(
        slot: u16,
        sub_op: u8,
        period: i16,
        amplitude: i16,
        arm: i16,
        seed_height: i16,
    ) -> Self {
        let scaled = i32::from(amplitude).wrapping_shl(17);
        let denom = i32::from(period).wrapping_add(1);
        let vel = if denom == 0 { 0 } else { scaled / denom };
        let accel = if period == 0 {
            0
        } else {
            vel.abs() / i32::from(period)
        };
        let pos = i32::from(seed_height).wrapping_shl(16);
        FloorTierBob {
            slot,
            phase: 0,
            phase_seed: sub_op,
            pos,
            vel,
            accel,
            target: pos,
            arm: arm as u16,
        }
    }

    /// Advance the rung and return its new LUT height, `None` when the tick
    /// produced no step this frame.
    ///
    /// PORT: FUN_801DA930 (`0x801DA930..0x801DAA4C`)
    ///
    /// Two nested loops. The outer one runs once per frame normally; when
    /// `+0x9E` carries [`FLOOR_TIER_ARM_BIT`] it instead runs `+0x9E & 0x7FFF`
    /// times with a **one-tick** inner step and clears the word - a scripted
    /// catch-up burst. The inner one runs `frame_delta` times and is the
    /// oscillator: add `accel` to `vel` in whichever direction points at the
    /// rest height, add `vel` to `pos`, publish `pos >> 16`. There is no
    /// damping term and the rest height is never rewritten, so the rung
    /// overshoots and swings back - the ping-pong.
    ///
    /// Phase `>= 2` is where retail's outer loop has no exit; the port stops
    /// iterating instead (module note).
    pub fn step(&mut self, frame_delta: u8) -> Option<i16> {
        let (mut outer, inner) = if self.arm & FLOOR_TIER_ARM_BIT != 0 {
            let n = self.arm & !FLOOR_TIER_ARM_BIT;
            self.arm = 0;
            (u32::from(if n == 0 { 1 } else { n }), 1u32)
        } else {
            (1u32, u32::from(frame_delta))
        };
        let mut out = None;
        while outer != 0 {
            match self.phase {
                1 => {
                    for _ in 0..inner {
                        if self.pos < self.target {
                            self.vel = self.vel.wrapping_add(self.accel);
                        }
                        if self.target < self.pos {
                            self.vel = self.vel.wrapping_sub(self.accel);
                        }
                        self.pos = self.pos.wrapping_add(self.vel);
                        out = Some((self.pos >> 16) as i16);
                    }
                    outer -= 1;
                }
                0 => self.phase = i16::from(self.phase_seed).wrapping_add(1),
                // Retail spins here forever; see the module note.
                _ => break,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- the bar envelope ---------------------------------------------------

    #[test]
    fn the_envelope_closes_holds_opens_and_retires() {
        // The property the disassembly states: phase 0 ramps 0 -> 0x73 over
        // its own duration, phase 1 sits at 0x73, phase 2 ramps back to 0,
        // phase 3 latches the retire bit.
        let mut lb = LetterboxBars::spawn(8, 4, 8);
        let mut heights = Vec::new();
        for _ in 0..24 {
            heights.push(lb.step(1));
            if lb.retired {
                break;
            }
        }
        assert_eq!(heights[0], LETTERBOX_FULL_BAR / 8);
        assert!(
            heights.contains(&LETTERBOX_FULL_BAR),
            "the envelope must reach full height: {heights:?}"
        );
        assert_eq!(*heights.last().unwrap(), 0, "and return: {heights:?}");
        assert!(lb.retired, "phase 3 latches +0x10 & 8");
        // Monotone up then monotone down - no third excursion.
        let peak = heights
            .iter()
            .position(|&h| h == LETTERBOX_FULL_BAR)
            .unwrap();
        assert!(heights[..=peak].windows(2).all(|w| w[0] <= w[1]));
        assert!(heights[peak..].windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn full_envelope_covers_the_whole_retail_screen() {
        // 115 * 2 > 224: the peak is a blackout, not a crop. This is the
        // assertion that keeps the "cinematic letterbox" reading from
        // creeping back.
        let [top, bottom] = letterbox_bar_rects(LETTERBOX_FULL_BAR, LETTERBOX_RETAIL_SCREEN_H);
        assert!(top.y0 <= 0, "top bar starts at or above the screen top");
        assert!(
            bottom.y0 <= top.y1,
            "the two bars must meet or overlap: top ends {}, bottom starts {}",
            top.y1,
            bottom.y0
        );
        assert_eq!(bottom.y1, LETTERBOX_RETAIL_SCREEN_H);
        assert_eq!(top.x1, LETTERBOX_SCREEN_W);
    }

    #[test]
    fn a_zero_duration_phase_does_not_divide_by_zero() {
        let mut lb = LetterboxBars::spawn(0, 0, 0);
        for _ in 0..8 {
            lb.step(1);
        }
        assert!(lb.retired);
    }

    #[test]
    fn the_frame_delta_scales_the_envelope() {
        // Cadence invariance: the same wall-clock envelope at delta 2 in half
        // the frames.
        let mut one = LetterboxBars::spawn(8, 2, 8);
        let mut two = LetterboxBars::spawn(8, 2, 8);
        for _ in 0..4 {
            one.step(1);
            one.step(1);
            two.step(2);
        }
        assert_eq!(one.phase, two.phase);
    }

    // -- the eased move -----------------------------------------------------

    #[test]
    fn the_ease_arrives_exactly_at_the_end_value() {
        let mut m = EasedMove::spawn([0, 0, 0], [100, 200, 300], 10);
        let mut last = EasedMoveFrame::default();
        for _ in 0..10 {
            last = m.step(1, 0);
        }
        assert_eq!(last.axis, [Some(100), Some(200), Some(300)]);
        assert!(last.retired);
    }

    #[test]
    fn the_ease_is_quadratic_not_linear() {
        // t^2/d^2 at the halfway tick is a quarter of the distance, not half.
        let mut m = EasedMove::spawn([0, 0, 0], [400, -1, -1], 8);
        for _ in 0..4 {
            m.step(1, 0);
        }
        assert_eq!(m.step(0, 0).axis[0], Some(100));
    }

    #[test]
    fn a_minus_one_end_disables_only_that_axis() {
        let mut m = EasedMove::spawn([5, 5, 5], [-1, 50, -1], 4);
        let f = m.step(4, 0);
        assert_eq!(f.axis, [None, Some(50), None]);
    }

    #[test]
    fn the_inverted_y_mirror_needs_the_full_word_bit() {
        // The bit is 0x2000_0000. A doc that says 0x2000 is off by 16 bits,
        // and this is the test that says so.
        let mut m = EasedMove::spawn([0, 0, 0], [0, 64, 0], 4);
        assert_eq!(m.step(0, 0x2000).mirror_y, None);
        let mut m = EasedMove::spawn([0, 0, 0], [0, 64, 0], 4);
        assert_eq!(m.step(4, EASE_TARGET_INVERT_Y).mirror_y, Some(-64));
    }

    // -- the floor-ladder oscillator ---------------------------------------

    #[test]
    fn the_rung_swings_past_its_rest_height_and_comes_back() {
        let mut b = FloorTierBob::spawn(3, 0, 16, 40, 0, -100);
        let mut heights = Vec::new();
        for _ in 0..64 {
            if let Some(h) = b.step(1) {
                heights.push(h);
            }
        }
        assert!(!heights.is_empty(), "phase 0 must seed phase 1 and run");
        let hi = *heights.iter().max().unwrap();
        let lo = *heights.iter().min().unwrap();
        assert!(hi > -100, "the rung must leave its rest height: {hi}");
        assert!(
            lo < hi,
            "and reverse: swing spans {lo}..{hi} around the -100 rest"
        );
        // A ping-pong, not a ramp: the sign of the step must change.
        let ups = heights.windows(2).filter(|w| w[1] > w[0]).count();
        let downs = heights.windows(2).filter(|w| w[1] < w[0]).count();
        assert!(ups > 0 && downs > 0, "no reversal in {heights:?}");
    }

    #[test]
    fn the_rest_height_is_the_seed_height() {
        let b = FloorTierBob::spawn(0, 0, 8, 10, 0, -256);
        assert_eq!(b.pos, b.target);
        assert_eq!(b.pos >> 16, -256);
    }

    #[test]
    fn the_arm_bit_runs_a_burst_and_clears_itself() {
        let mut b = FloorTierBob::spawn(0, 0, 8, 10, (FLOOR_TIER_ARM_BIT | 5) as i16, 0);
        b.phase = 1;
        let burst = b.step(0);
        assert!(burst.is_some(), "the burst runs even at frame_delta 0");
        assert_eq!(b.arm, 0, "the arm word is consumed");
    }

    #[test]
    fn a_phase_beyond_one_idles_instead_of_hanging() {
        // The disclosed divergence: retail's outer loop has no exit here.
        let mut b = FloorTierBob::spawn(0, 2, 8, 10, 0, 0);
        assert_eq!(b.step(1), None);
        assert_eq!(b.phase, 3);
        assert_eq!(b.step(1), None);
    }
}
