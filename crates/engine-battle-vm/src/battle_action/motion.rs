//! Battle-actor motion kernels: the retail range law, the animation tick's
//! root-motion step, and the arrival shove.
//!
//! Three disassembly-grounded pieces:
//!
//! 1. **The range law** ([`range_metric`]) - `FUN_8004E2F0`
//!    (`ghidra/scripts/funcs/8004e2f0.txt`). Not a table lookup: a computed
//!    distance metric over the attacker's **live** position pair
//!    (`+0x34`/`+0x38`) and the target's **seat** pair (`+0x3C`/`+0x40`),
//!    plus two small `SCUS_942.54` data tables ([`RANGE_THRESHOLDS`] at
//!    `DAT_80078870`, [`PARTY_REACH_OFFSETS`] at `DAT_80078878`).
//! 2. **The root-motion step** ([`root_motion_step`]) - the approach drive.
//!    It lives in the battle anim-node tick `FUN_80047430`
//!    (`0x80047D20..0x80047E18`, `ghidra/scripts/funcs/80047430.txt`), NOT in
//!    the action state machine: while a clip whose entry `+0xC` speed is
//!    non-zero plays, the actor's live pair steps along its facing by
//!    `trig * speed * frame_dt * actor[+0x21D] >> 15` per tick. A positive
//!    speed is gated on `FUN_8004E2F0` still returning non-zero (the walk
//!    stops itself exactly on arrival); a negative speed (retreat clips) is
//!    ungated. Both are gated on `actor[+0x1DC] & 8` clear.
//! 3. **The arrival shove** ([`arrival_shove_step`]) - the one movement the
//!    SM itself performs. State `0x16`'s in-range arm
//!    (`overlay_battle_action_801e295c.txt`, `0x801E33EC..0x801E3490`) steps
//!    the **target's** live *and* seat pairs along the attacker's facing by
//!    `trig >> 9` per iteration, looping while the range check still returns
//!    0 - the pair is pushed back out to the range boundary before the
//!    close-in clip runs.
//!
//! PORT: FUN_8004E2F0 - battle range / reach metric
//! REF: FUN_80047430 - the anim tick whose root-motion term this mirrors
//! REF: FUN_801E295C - state 0x16's arrival shove
//! REF: FUN_80019B28 - the bearing helper ([`crate::battle_action::bearing_12bit`])

use crate::battle_approach::projected_separation;

/// Sine table sample of a 12-bit angle, `trunc(sin(a * 2pi / 4096) * 4096)`.
///
/// This is the formula of the SCUS-resident table at `0x80070A2C`
/// (`_DAT_8007B81C`; disc-verified byte-exact by the engine's
/// `action_effect_script::RetailRotationLut`, truncation toward zero). The
/// cosine read is the same table at `+0x800` bytes (`_DAT_8007B7F8`), i.e.
/// the angle shifted a quarter turn.
pub fn sin12(angle: u16) -> i16 {
    let a = (angle & 0xFFF) as f64;
    (f64::sin(a * std::f64::consts::TAU / 4096.0) * 4096.0).trunc() as i16
}

/// `(sin, cos)` LUT pair of a 12-bit angle - the `_DAT_8007B81C` /
/// `_DAT_8007B7F8` read pair every battle motion consumer performs.
pub fn trig12(angle: u16) -> (i16, i16) {
    (sin12(angle), sin12(angle.wrapping_add(0x400)))
}

/// Size-class thresholds `DAT_80078870` (`SCUS_942.54`): the in-range bound
/// used when the resolved size class is `< 3`. Indexed by size class.
pub const RANGE_THRESHOLDS: [i16; 3] = [256, 384, 1024];

/// Per-character reach offsets `DAT_80078878` (`SCUS_942.54`), indexed by
/// `roster_char_id - 1` (`DAT_8007BD10[slot]`): Vahn `+43`, Noa `0`, Gala
/// `-53`, id 4 `-100`. The offset is **added to the projected distance**, so
/// a positive value means the character has to close further before its
/// swing connects (a shorter reach).
pub const PARTY_REACH_OFFSETS: [i16; 4] = [43, 0, -53, -100];

/// Reach offset for a playable character ([`PARTY_REACH_OFFSETS`] keyed by
/// the retail roster char id `1..=3`).
pub fn party_reach_offset(character: legaia_art::Character) -> i16 {
    use legaia_art::Character::*;
    match character {
        Vahn => PARTY_REACH_OFFSETS[0],
        Noa => PARTY_REACH_OFFSETS[1],
        Gala => PARTY_REACH_OFFSETS[2],
    }
}

/// Inputs of one [`range_metric`] evaluation, named by the retail sources.
#[derive(Debug, Clone, Copy, Default)]
pub struct RangeInputs {
    /// Attacker slot `< 3` in retail (engine: `< party_count`).
    pub attacker_party: bool,
    /// Target slot `< 3`.
    pub target_party: bool,
    /// [`party_reach_offset`] of the attacking character (`DAT_80078878`
    /// keyed by `DAT_8007BD10[slot] - 1`); ignored for a monster attacker.
    pub attacker_reach: i16,
    /// Monster record `+0x1F` size class of the attacker (0 for party).
    pub attacker_size: u8,
    /// Monster record `+0x1F` size class of the target (0 for party).
    pub target_size: u8,
    /// Attacker **live** position (`+0x34`, `+0x38`).
    pub attacker_pos: (i16, i16),
    /// Target **seat** position (`+0x3C`, `+0x40`).
    pub target_ref: (i16, i16),
}

/// PORT: FUN_8004E2F0 - the battle range / reach metric.
///
/// `sin` / `cos` are the LUT samples at the angle
/// `(bearing_12bit(target_ref.z, target_ref.x, attacker.z, attacker.x)
/// + 0x800) & 0xFFF` - the caller derives it with
/// [`crate::battle_action::bearing_12bit`] (or the approx variant) plus
/// [`crate::battle_approach::approach_angle`], then samples [`trig12`].
///
/// Faithful to the disassembly:
///
/// - Party attacker: `base = attacker_reach`; monster attacker:
///   `size = attacker_size`.
/// - Monster target: `size = target_size` when the attacker was party, else
///   `size = ((attacker_size + target_size) * 3) / 5` (unsigned divide).
/// - Party-vs-party halves the base (`sra 1`).
/// - `d = base + |(|dx| * sin) >> 12| + |(|dz| * cos) >> 12|` (the same
///   double-abs projection as [`projected_separation`]).
/// - In-range compare: size `< 3` uses the **signed** `slt` against
///   [`RANGE_THRESHOLDS`]`[size]`; size `>= 3` uses the **unsigned** `sltu`
///   against `size << 4`.
///
/// Returns `0` when in range, else the metric truncated to `i16` and
/// reinterpreted (`(s3 << 16) >> 16`) - the shape every SM caller tests
/// against zero.
pub fn range_metric(inp: &RangeInputs, sin: i16, cos: i16) -> u16 {
    let mut base: i32 = if inp.attacker_party {
        i32::from(inp.attacker_reach)
    } else {
        0
    };
    let mut size: u32 = if inp.attacker_party {
        0
    } else {
        u32::from(inp.attacker_size)
    };
    if !inp.target_party {
        size = if size == 0 {
            u32::from(inp.target_size)
        } else {
            ((size + u32::from(inp.target_size)) * 3) / 5
        };
    }
    if inp.attacker_party && inp.target_party {
        base >>= 1;
    }
    let proj = projected_separation(
        inp.attacker_pos.0,
        inp.attacker_pos.1,
        inp.target_ref.0,
        inp.target_ref.1,
        sin,
        cos,
    );
    let d = base.wrapping_add(proj);
    let in_range = if size < 3 {
        d < i32::from(RANGE_THRESHOLDS[size as usize])
    } else {
        (d as u32) < (size << 4)
    };
    if in_range { 0 } else { d as i16 as u16 }
}

/// PORT: FUN_80047430 (root-motion term, `0x80047D20..0x80047E18`) - one
/// tick of the playing clip's approach/retreat drive.
///
/// `(dx, dz) = (sin * speed * frame_dt * scale >> 15,
///              cos * speed * frame_dt * scale >> 15)` with the exact retail
/// multiply order and arithmetic shift. `speed` is the clip entry's `+0xC`
/// halfword, `frame_dt` the scratchpad frame-time byte `DAT_1F800393`,
/// `scale` the actor's `+0x21D` byte (retail normal `4`).
pub fn root_motion_step(sin: i16, cos: i16, speed: i16, frame_dt: u8, scale: u8) -> (i32, i32) {
    let (rx, rz) = root_motion_raw(sin, cos, speed, frame_dt, scale);
    (rx >> 15, rz >> 15)
}

/// The un-shifted product [`root_motion_step`] shifts:
/// `trig * speed * frame_dt * scale` per axis, in the same multiply order.
pub fn root_motion_raw(sin: i16, cos: i16, speed: i16, frame_dt: u8, scale: u8) -> (i32, i32) {
    let raw = |trig: i16| -> i32 {
        i32::from(trig)
            .wrapping_mul(i32::from(speed))
            .wrapping_mul(i32::from(frame_dt))
            .wrapping_mul(i32::from(scale))
    };
    (raw(sin), raw(cos))
}

/// Battle frames the engine's one-vsync ticks are grouped into for the
/// root-motion truncation: retail's frame step `DAT_1F800393` at the battle
/// tick's usual cadence, the step the battle camera already assumes.
pub const ROOT_MOTION_FRAME_VSYNCS: u64 = 2;

/// Spreads retail's **per-frame** root-motion truncation over the engine's
/// per-vsync ticks.
///
/// Retail moves an actor `trig * speed * frame_dt * scale >> 15` once a
/// battle frame (`sra` - a floor), so a heading's small axis keeps whatever
/// a whole frame of it amounts to. The engine ticks once a vsync with
/// `frame_dt = 1`; shifting each tick's product on its own floors that axis
/// to nothing whenever it is under a unit a vsync, and Vahn's strikes at
/// heading `142` lost their whole sideways drift. This sums the raw products
/// over a frame of [`ROOT_MOTION_FRAME_VSYNCS`] ticks, moves each tick by
/// what the running floor gained, and drops the remainder at the frame's
/// end the way retail does - so every frame totals exactly retail's
/// `floor(frame_raw >> 15)`, spread smoothly across its vsyncs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RootMotionCarry {
    frame: u64,
    acc: [i64; 2],
    moved: [i32; 2],
}

impl RootMotionCarry {
    /// One tick's displacement for the tick's raw product `raw`
    /// ([`root_motion_raw`] at `frame_dt = 1`) on display frame `tick`.
    pub fn step(&mut self, tick: u64, raw: (i32, i32)) -> (i32, i32) {
        self.step_in_frame(tick / ROOT_MOTION_FRAME_VSYNCS, raw)
    }

    /// [`Self::step`] with the battle frame the tick falls in named by the
    /// caller - a host whose frames are not a fixed
    /// [`ROOT_MOTION_FRAME_VSYNCS`] long (a replayed retail frame step,
    /// `BattleActionHost::battle_frame`).
    pub fn step_in_frame(&mut self, frame: u64, raw: (i32, i32)) -> (i32, i32) {
        if frame != self.frame {
            *self = Self {
                frame,
                ..Self::default()
            };
        }
        let mut out = [0i32; 2];
        for (k, r) in [raw.0, raw.1].into_iter().enumerate() {
            self.acc[k] += i64::from(r);
            let total = (self.acc[k] >> 15) as i32;
            out[k] = total - self.moved[k];
            self.moved[k] = total;
        }
        (out[0], out[1])
    }
}

/// The anim tick's **end-of-clip displacement**: at the natural end the actor
/// moves along its facing by the committed entry's `+0x0E`,
/// `(sin * step >> 12, cos * step >> 12)` into the live `(+0x34, +0x38)`
/// pair (`mult; mflo; sra 0xc` per axis).
///
/// PORT: FUN_80047430 (`0x80047A68..0x80047B2C`)
pub fn end_root_step(sin: i16, cos: i16, step: i16) -> (i32, i32) {
    let d = |trig: i16| (i32::from(trig) * i32::from(step)) >> 12;
    (d(sin), d(cos))
}

/// The same displacement taken on the **event-path** commit, which cuts the
/// clip short: pro-rated by the cursor's whole frame over the stream's frame
/// count, `(trig * step * frame / frames) >> 12` with retail's truncating
/// signed `div` (`0x80047970..0x80047A28`). A zero frame count moves nothing.
///
/// PORT: FUN_80047430 (`0x80047950..0x80047A28`)
pub fn event_cut_root_step(sin: i16, cos: i16, step: i16, frame: i16, frames: u8) -> (i32, i32) {
    if frames == 0 {
        return (0, 0);
    }
    let d = |trig: i16| {
        let v = (i32::from(trig) * i32::from(step)).wrapping_mul(i32::from(frame));
        (v / i32::from(frames)) >> 12
    };
    (d(sin), d(cos))
}

/// PORT: FUN_801E295C state `0x16` (`0x801E33EC..0x801E3490`) - one
/// iteration of the arrival shove: the per-axis displacement the target's
/// live and seat pairs both take, `(sin >> 9, cos >> 9)` (arithmetic shifts
/// of the signed samples; retail `sll 0x10; sra 0x19`).
pub fn arrival_shove_step(sin: i16, cos: i16) -> (i16, i16) {
    ((sin >> 9), (cos >> 9))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_carry_totals_each_frame_to_retails_frame_step_floor() {
        // Heading 142 at Vahn's strike drift: the small axis is under a unit
        // a vsync, so a per-tick shift would never move it.
        let (sin, cos) = trig12(142);
        // The per-tick shift drops the whole small axis of a forward drift.
        assert_eq!(root_motion_step(sin, cos, 3, 1, 8).0, 0);
        for speed in [-3i16, 3, 7] {
            let tick_raw = root_motion_raw(sin, cos, speed, 1, 8);
            let (fx, fz) = root_motion_step(sin, cos, speed, 2, 8);
            let mut carry = RootMotionCarry::default();
            let mut total = (0, 0);
            for t in 10..20u64 {
                let (dx, dz) = carry.step(t, tick_raw);
                total = (total.0 + dx, total.1 + dz);
                if t % ROOT_MOTION_FRAME_VSYNCS == ROOT_MOTION_FRAME_VSYNCS - 1 {
                    assert_eq!(total, (fx, fz), "speed {speed} frame ending {t}");
                    total = (0, 0);
                }
            }
        }
    }

    #[test]
    fn sin12_matches_the_retail_table_anchors() {
        // Quarter-turn anchors of the 0x80070A2C table.
        assert_eq!(sin12(0), 0);
        assert_eq!(sin12(0x400), 4096);
        assert_eq!(sin12(0x800), 0);
        assert_eq!(sin12(0xC00), -4096);
        // Truncation toward zero (not rounding): sin(1/4096 turn).
        assert_eq!(sin12(1), 6); // 4096*sin(2pi/4096) = 6.283.. -> 6
        // cos = the same table a quarter turn on.
        assert_eq!(trig12(0), (0, 4096));
        assert_eq!(trig12(0x400), (4096, 0));
    }

    #[test]
    fn end_of_clip_displacement_is_the_entry_step_along_the_facing() {
        // Facing 0: all on Z. A negative step walks back.
        assert_eq!(end_root_step(0, 4096, -826), (0, -826));
        // A quarter turn: all on X.
        assert_eq!(end_root_step(4096, 0, 300), (300, 0));
        // `sra 12` floors: a -1 step against a small positive sample is -1.
        assert_eq!(end_root_step(100, -100, -1), (-1, 0));
        // The event-path cut pro-rates by frame / frames (truncating div).
        assert_eq!(event_cut_root_step(0, 4096, -800, 3, 8), (0, -300));
        assert_eq!(event_cut_root_step(0, 4096, -800, 3, 0), (0, 0));
    }

    #[test]
    fn party_attacker_uses_reach_plus_projection_against_small_class() {
        // Colocated pair: proj = 0, d = reach. Vahn's +43 < threshold 256
        // for a size-0 target -> in range.
        let inp = RangeInputs {
            attacker_party: true,
            attacker_reach: 43,
            target_size: 0,
            ..Default::default()
        };
        assert_eq!(range_metric(&inp, 0x1000, 0x1000), 0);
        // 300 units away on Z with unit cos: d = 43 + 300 = 343 >= 256.
        let far = RangeInputs {
            attacker_pos: (0, 300),
            ..inp
        };
        assert_eq!(range_metric(&far, 0x1000, 0x1000), 343);
    }

    #[test]
    fn monster_size_class_threshold_is_size_shl_4_unsigned() {
        // Gaza-shaped: monster attacker size 0x1A (26) on a party target ->
        // threshold 26 << 4 = 416 (matches the captured ~416 reach).
        let inp = RangeInputs {
            attacker_size: 0x1A,
            target_party: true,
            attacker_pos: (0, 556),
            ..Default::default()
        };
        // d = 556 >= 416 -> the captured 554-557 out-of-range metric band.
        assert_eq!(range_metric(&inp, 0x1000, 0x1000), 556);
        let near = RangeInputs {
            attacker_pos: (0, 415),
            ..inp
        };
        assert_eq!(range_metric(&near, 0x1000, 0x1000), 0);
    }

    #[test]
    fn monster_pair_folds_sizes_by_three_fifths() {
        // Monster on monster: size = ((a + t) * 3) / 5, unsigned divide.
        let inp = RangeInputs {
            attacker_size: 20,
            target_size: 30,
            attacker_pos: (0, 500),
            ..Default::default()
        };
        // size = (50*3)/5 = 30 -> threshold 480; d = 500 out of range.
        assert_eq!(range_metric(&inp, 0x1000, 0x1000), 500);
        let near = RangeInputs {
            attacker_pos: (0, 479),
            ..inp
        };
        assert_eq!(range_metric(&near, 0x1000, 0x1000), 0);
    }

    #[test]
    fn party_on_party_halves_the_reach_base() {
        // Gala's -53 halves (sra) to -27; size stays 0 -> threshold 256.
        let inp = RangeInputs {
            attacker_party: true,
            target_party: true,
            attacker_reach: -53,
            attacker_pos: (0, 282),
            ..Default::default()
        };
        // d = -27 + 282 = 255 < 256 -> in range.
        assert_eq!(range_metric(&inp, 0x1000, 0x1000), 0);
        let far = RangeInputs {
            attacker_pos: (0, 283),
            ..inp
        };
        assert_eq!(range_metric(&far, 0x1000, 0x1000), 256);
    }

    #[test]
    fn root_motion_reproduces_the_measured_approach_rate() {
        // Gaza's Move clip: entry speed +20, actor scale 8, dt 1, facing on
        // an axis (trig 0x1000) -> the measured ~20 units/vsync.
        let (dx, dz) = root_motion_step(0, 4096, 20, 1, 8);
        assert_eq!((dx, dz), (0, 20));
        // The retail-normal scale 4 halves it.
        assert_eq!(root_motion_step(0, 4096, 20, 1, 4), (0, 10));
        // Negative speed (retreat clip) steps backward along facing.
        assert_eq!(root_motion_step(0, 4096, -20, 1, 8), (0, -20));
        // The shift is arithmetic (rounds toward -inf on a ragged product).
        assert_eq!(root_motion_step(0, 4096, -1, 1, 1), (0, -1));
    }

    #[test]
    fn shove_step_is_the_arithmetic_ninth_shift() {
        assert_eq!(arrival_shove_step(4096, 4096), (8, 8));
        assert_eq!(arrival_shove_step(-4096, 512), (-8, 1));
        // Sub-0x200 samples shift to zero on that axis.
        assert_eq!(arrival_shove_step(0x1FF, -0x200), (0, -1));
    }

    #[test]
    fn range_metric_is_deterministic_over_the_trig_pair() {
        let inp = RangeInputs {
            attacker_party: true,
            attacker_reach: 43,
            target_size: 5,
            attacker_pos: (-321, 777),
            target_ref: (140, -260),
            ..Default::default()
        };
        let bearing = crate::battle_action::bearing_12bit_approx(
            inp.target_ref.1,
            inp.target_ref.0,
            inp.attacker_pos.1,
            inp.attacker_pos.0,
        );
        let angle = crate::battle_approach::approach_angle(bearing) as u16;
        let (sin, cos) = trig12(angle);
        assert_eq!(range_metric(&inp, sin, cos), range_metric(&inp, sin, cos));
    }
}
