//! Cutscene camera mover - the per-frame glide behind field-VM op `0x45`.
//!
//! PORT: FUN_801DC0BC, FUN_801DD310
//! REF: FUN_801DE084, FUN_8002519C
//!
//! # Where the mover sits
//!
//! Op `0x45` CONFIGURE merges its masked params into the persistent camera
//! staging struct (`0x801C6EA8`) and commits with
//! `FUN_801DE084(struct, apply, curve)`, where `apply` is the u16 at operand
//! `+2` and `curve = op0 >> 2 & 0xF` (`overlay_0897_801de840.txt`, case `0x45`
//! sub-`0x00`).
//!
//! * `apply == 0` - **snap**. `FUN_801DE084` writes the ten params straight
//!   into the camera globals and marks every live mover actor dead (flag bit
//!   `8`), cancelling any glide in flight.
//! * `apply != 0` - **glide**. `FUN_801DD310` finds (or allocates) the ONE
//!   camera-mover actor - the node in list `_DAT_8007C34C` whose tick fn is
//!   `FUN_801DC0BC` - and hands it a 40-byte block of ten `(start, end)` u16
//!   pairs: `start` read from the LIVE camera globals, `end` from the staging
//!   struct. It then sets `actor[+0x9C] = 0` (progress), `actor[+0x9E] = apply`
//!   (duration) and `actor[+0x50] = curve`.
//!
//! Two consequences the engine has to mirror:
//!
//! * The mover is a **separate actor**, ticked by the per-frame actor-list
//!   walk (`FUN_8002519C`) like any other. The script that staged the beat
//!   does **not** block on it - choreography and glide run in parallel, and a
//!   record whose `WaitFrames` run out before the glide does simply moves on
//!   while the camera keeps travelling (the `opurud` `apply 2300` beat is
//!   interrupted about a quarter of the way through its travel).
//! * A new glide beat landing mid-tween **re-seeds every axis' `start` from
//!   the current interpolated globals and resets the shared progress to `0`**.
//!   There is no per-axis progress and no carry-over: one counter, one
//!   duration, one curve for all ten axes.
//!
//! # Per-frame law (`FUN_801DC0BC`, body `0x801DC104..0x801DD220`)
//!
//! ```text
//! t = min(t + DAT_1F800393, d)          // progress, clamped to the duration
//! for each of the ten axes (start s, end e):
//!     if e == s: value = s              // untouched axis holds
//!     elif t >= d: value = e            // arrived: exact target, no overshoot
//!     else: value = s + curve_num(e - s, t, d, curve)
//! ```
//!
//! `DAT_1F800393` is the adaptive frame-skip factor (the logic tick's `dt` in
//! display frames), so `t` counts **display frames** and `apply` is a duration
//! in display frames 1:1 - live-confirmed on the `opurud` `apply 2300` beat,
//! whose progress advances exactly 30 per 30 display frames while the factor
//! reads `3`.
//!
//! Every axis - the three angles included - uses the same curve. There is no
//! per-axis curve split and no shortest-arc handling: the angle axes are
//! lerped as plain integers over their raw 12-bit-space values.
//!
//! On `t >= d` the mover actor sets its own dead bit and frees the pair block,
//! so the glide is one-shot.
//!
//! The globals the ten axes drive, in order: pitch / yaw / roll
//! (`0x8007B790/92/94`), the eye-space translation trio
//! (`0x800840B8/BC/C0`), the negated focus trio (`0x80089118/1C/20`), and the
//! GTE `H` projection register (`0x8007B6F4`). Camera shake
//! (`_DAT_8007B630`) is added to the two eye axes *after* the tween and is
//! not part of the mover law.

/// Number of camera axes an op-`0x45` beat can stage.
pub const AXIS_COUNT: usize = 10;

/// Truncating-toward-zero integer division, matching MIPS `div`.
///
/// Rust's `/` on integers already truncates toward zero, but the mover's
/// arithmetic is expressed on `i32` intermediates that can legitimately be
/// negative, so this is spelled out to keep the port readable next to the
/// decompiled `iVar / iVar` sequences it mirrors.
#[inline]
fn tdiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a / b }
}

/// The curve numerator: the offset from `start` at progress `t` of `d` for a
/// span of `delta = end - start`.
///
/// Mirrors the per-axis block that `FUN_801DC0BC` repeats ten times. The
/// double truncating divisions are load-bearing - `(k*t/d)*t/d` is **not**
/// `k*t*t/(d*d)` in integer arithmetic, and retail computes the former.
///
/// | `curve` | shape | expression |
/// |---|---|---|
/// | `2` | quadratic ease-**out** | `n = k*t; (n + (n/d)*(d - t)) / d` |
/// | `3` | quadratic ease-**in** | `((k*t)/d * t) / d` |
/// | `4` | ease-in-out (two halves) | ease-in over `d/2` to the midpoint `k/2`, then curve `2` from there |
/// | any other (incl. `1`) | linear | `(k*t) / d` |
///
/// `curve 4`'s halves both run over `h = d >> 1`, so an odd `d` spends its
/// final frame at the exact target via the `t >= d` arrival branch.
pub fn curve_offset(delta: i32, t: i32, d: i32, curve: i16) -> i32 {
    if d <= 0 {
        return delta;
    }
    match curve {
        4 => {
            let h = d >> 1;
            let half = delta >> 1;
            if h < t {
                // Second half: quad-out from the midpoint to the end.
                let t2 = t - h;
                let mid = half;
                let k2 = delta - mid;
                let n = k2 * t2;
                half + quad_out_num(n, h, t2)
            } else {
                // First half: quad-in toward the midpoint.
                tdiv(tdiv(half * t, h) * t, h)
            }
        }
        3 => tdiv(tdiv(delta * t, d) * t, d),
        2 => quad_out_num(delta * t, d, t),
        _ => tdiv(delta * t, d),
    }
}

/// `curve 2`'s body: `(n + (n/d)*(d - t)) / d` for `n = delta * t`.
#[inline]
fn quad_out_num(n: i32, d: i32, t: i32) -> i32 {
    if d == 0 {
        return 0;
    }
    tdiv(n + tdiv(n, d) * (d - t), d)
}

/// Value of one axis at progress `t`.
///
/// `start == end` holds the axis (retail's `if (e != s)` guard), and
/// `t >= d` lands exactly on `end` - a glide never overshoots.
pub fn axis_value(start: i32, end: i32, t: i32, d: i32, curve: i16) -> i32 {
    if end == start {
        return start;
    }
    if t >= d {
        return end;
    }
    start + curve_offset(end - start, t, d, curve)
}

/// The retail curve as a normalized `f32` shape: the fraction of the span
/// covered at normalized progress `u ∈ [0, 1]`.
///
/// This is [`curve_offset`] with the integer truncation removed, for consumers
/// that carry the camera pose in floats (the renderer's cutscene view works in
/// world-space `f32` / radians, not the retail integer globals). The shapes
/// are identical; only the rounding differs.
///
/// | `curve` | `f(u)` |
/// |---|---|
/// | `2` | `2u − u²` (ease-out) |
/// | `3` | `u²` (ease-in) |
/// | `4` | `2u²` for `u ≤ ½`; `½ + ½·(2w − w²)` with `w = 2u − 1` otherwise |
/// | any other (incl. `1`) | `u` (linear) |
pub fn curve_unit(u: f32, curve: u8) -> f32 {
    let u = u.clamp(0.0, 1.0);
    match curve & 0x0F {
        2 => 2.0 * u - u * u,
        3 => u * u,
        4 => {
            if u <= 0.5 {
                2.0 * u * u
            } else {
                let w = 2.0 * u - 1.0;
                0.5 + 0.5 * (2.0 * w - w * w)
            }
        }
        _ => u,
    }
}

/// The single camera-mover actor: ten `(start, end)` pairs advanced by one
/// shared progress counter.
///
/// A [`CameraMover`] is one-shot - [`Self::tick`] reports arrival on the frame
/// the progress reaches the duration, mirroring the retail actor marking
/// itself dead and freeing its pair block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CameraMover {
    /// Per-axis `(start, end)`.
    pub pairs: [(i32, i32); AXIS_COUNT],
    /// Progress in display frames.
    pub t: i32,
    /// Duration in display frames (the beat's `apply_trigger`).
    pub d: i32,
    /// Ease curve (`op0 >> 2 & 0xF`), shared by all ten axes.
    pub curve: i16,
}

impl CameraMover {
    /// Arm a glide from `current` toward `targets` over `apply` frames.
    ///
    /// Mirrors `FUN_801DD310`: every axis is re-seeded from the live pose, the
    /// shared progress resets to `0`, and the duration / curve are replaced.
    /// Calling this on a mover already in flight is exactly what retail does
    /// when a beat lands mid-tween.
    pub fn arm(
        &mut self,
        current: [i32; AXIS_COUNT],
        targets: [i32; AXIS_COUNT],
        apply: u16,
        curve: u8,
    ) {
        for (p, (&c, &t)) in self
            .pairs
            .iter_mut()
            .zip(current.iter().zip(targets.iter()))
        {
            *p = (c, t);
        }
        self.t = 0;
        self.d = i32::from(apply);
        self.curve = i16::from(curve & 0x0F);
    }

    /// Advance the shared progress by `dt` display frames (retail's
    /// `DAT_1F800393`) and return `true` once the glide has arrived.
    pub fn tick(&mut self, dt: i32) -> bool {
        self.t = (self.t + dt).min(self.d);
        self.t >= self.d
    }

    /// The current pose.
    pub fn values(&self) -> [i32; AXIS_COUNT] {
        let mut out = [0i32; AXIS_COUNT];
        for (o, &(s, e)) in out.iter_mut().zip(self.pairs.iter()) {
            *o = axis_value(s, e, self.t, self.d, self.curve);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear (`curve 1`) is a plain truncating lerp with exact arrival.
    #[test]
    fn linear_curve_is_a_truncating_lerp() {
        assert_eq!(axis_value(0, 1000, 0, 100, 1), 0);
        assert_eq!(axis_value(0, 1000, 25, 100, 1), 250);
        assert_eq!(axis_value(0, 1000, 50, 100, 1), 500);
        assert_eq!(axis_value(0, 1000, 99, 100, 1), 990);
        assert_eq!(axis_value(0, 1000, 100, 100, 1), 1000);
        // Truncation, not rounding: 1000*33/100 = 330 exactly, 1000*7/100 = 70.
        assert_eq!(axis_value(0, 7, 50, 100, 1), 3);
    }

    /// `curve 2` = quadratic ease-OUT: fast first, decelerating.
    #[test]
    fn curve_two_eases_out() {
        // n = k*t; (n + (n/d)*(d - t)) / d, with k = 1000, d = 100.
        // t = 50: n = 50000; n/d = 500; 50000 + 500*50 = 75000; /100 = 750.
        assert_eq!(axis_value(0, 1000, 50, 100, 2), 750);
        // t = 25: n = 25000; n/d = 250; 25000 + 250*75 = 43750; /100 = 437.
        assert_eq!(axis_value(0, 1000, 25, 100, 2), 437);
        assert_eq!(axis_value(0, 1000, 100, 100, 2), 1000);
        // Past the halfway point of TIME it is past halfway in DISTANCE.
        assert!(axis_value(0, 1000, 50, 100, 2) > 500);
    }

    /// `curve 3` = quadratic ease-IN: slow first, accelerating.
    #[test]
    fn curve_three_eases_in() {
        // ((k*t)/d * t)/d, k = 1000, d = 100. t = 50: (50000/100)*50/100 = 250.
        assert_eq!(axis_value(0, 1000, 50, 100, 3), 250);
        // t = 25: (25000/100)*25/100 = 62.
        assert_eq!(axis_value(0, 1000, 25, 100, 3), 62);
        assert_eq!(axis_value(0, 1000, 100, 100, 3), 1000);
        assert!(axis_value(0, 1000, 50, 100, 3) < 500);
    }

    /// `curve 4` = ease-in-out: quad-in to the midpoint, quad-out from it.
    #[test]
    fn curve_four_eases_in_then_out() {
        // Midpoint at t = d/2 lands on delta/2 (the halves meet).
        assert_eq!(axis_value(0, 1000, 50, 100, 4), 500);
        // First half is ease-IN, so below the linear line.
        assert!(axis_value(0, 1000, 25, 100, 4) < 250);
        // Second half is ease-OUT, so above it.
        assert!(axis_value(0, 1000, 75, 100, 4) > 750);
        assert_eq!(axis_value(0, 1000, 100, 100, 4), 1000);
        // Symmetric about the midpoint (integer truncation aside).
        let a = axis_value(0, 1000, 25, 100, 4);
        let b = axis_value(0, 1000, 75, 100, 4);
        assert!((1000 - b - a).abs() <= 2, "a={a} b={b}");
    }

    /// Every axis uses the SAME curve - there is no angle/eye split.
    #[test]
    fn all_axes_share_one_curve() {
        let mut mv = CameraMover::default();
        mv.arm([0; 10], [1000; 10], 100, 2);
        mv.tick(50);
        let v = mv.values();
        assert!(v.iter().all(|&x| x == v[0]), "{v:?}");
        assert_eq!(v[0], 750);
    }

    /// An untouched axis (start == end) holds through the whole glide.
    #[test]
    fn untouched_axis_holds() {
        let mut mv = CameraMover::default();
        let cur = [5, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut tgt = [0i32; 10];
        tgt[0] = 5;
        tgt[1] = 400;
        mv.arm(cur, tgt, 80, 1);
        mv.tick(40);
        let v = mv.values();
        assert_eq!(v[0], 5, "axis 0 was not restaged");
        assert_eq!(v[1], 200);
    }

    /// Progress counts display frames, advancing by the frame-skip `dt`, and
    /// clamps at the duration.
    #[test]
    fn progress_advances_by_dt_and_clamps() {
        let mut mv = CameraMover::default();
        mv.arm([0; 10], [900; 10], 90, 1);
        assert!(!mv.tick(3));
        assert_eq!(mv.t, 3);
        assert!(!mv.tick(3));
        assert_eq!(mv.t, 6);
        // 60Hz counting: 30 frames of dt=3 logic ticks == 90 display frames.
        for _ in 0..28 {
            mv.tick(3);
        }
        assert_eq!(mv.t, 90);
        assert!(mv.tick(3), "arrived");
        assert_eq!(mv.t, 90, "clamped, never past the duration");
        assert_eq!(mv.values()[0], 900);
    }

    /// Re-arming mid-tween re-seeds every start from the live pose and resets
    /// the shared progress - retail's `FUN_801DD310` on an in-flight mover.
    #[test]
    fn rearm_midtween_reseeds_from_the_live_pose() {
        let mut mv = CameraMover::default();
        mv.arm([0; 10], [1000; 10], 100, 1);
        mv.tick(40);
        let mid = mv.values();
        assert_eq!(mid[0], 400);
        // A new beat re-stages only axis 1; axis 0 keeps the same target but
        // still restarts from where it currently sits.
        let mut tgt = [1000i32; 10];
        tgt[1] = -500;
        mv.arm(mid, tgt, 50, 1);
        assert_eq!(mv.t, 0);
        assert_eq!(
            mv.pairs[0],
            (400, 1000),
            "start re-seeded, no discontinuity"
        );
        assert_eq!(mv.values()[0], 400, "continuous across the re-stage");
        mv.tick(25);
        assert_eq!(mv.values()[0], 700);
    }

    /// A zero-length glide is a snap.
    #[test]
    fn zero_duration_snaps() {
        assert_eq!(axis_value(10, 900, 0, 0, 1), 900);
        let mut mv = CameraMover::default();
        mv.arm([10; 10], [900; 10], 0, 2);
        assert!(mv.tick(1), "arrives immediately");
        assert_eq!(mv.values()[0], 900);
    }

    /// Angles are plain integer lerps in their raw 12-bit space - no
    /// shortest-arc wrap. A beat staging 4000 -> 100 travels DOWN through
    /// 2000, not up through the 4096 wrap.
    #[test]
    fn angle_axes_do_not_take_the_short_arc() {
        assert_eq!(axis_value(4000, 100, 50, 100, 1), 2050);
    }

    /// The `f32` shape tracks the integer law within the truncation error.
    #[test]
    fn curve_unit_tracks_the_integer_law() {
        for &c in &[1u8, 2, 3, 4] {
            for t in 0..=100i32 {
                let want = axis_value(0, 10_000, t, 100, i16::from(c)) as f32;
                let got = curve_unit(t as f32 / 100.0, c) * 10_000.0;
                assert!(
                    (want - got).abs() <= 3.0,
                    "curve {c} t {t}: {want} vs {got}"
                );
            }
        }
    }

    /// Negative spans truncate toward zero, like MIPS `div`.
    #[test]
    fn negative_spans_truncate_toward_zero() {
        assert_eq!(axis_value(0, -7, 50, 100, 1), -3);
        assert_eq!(tdiv(-7, 2), -3);
    }
}
