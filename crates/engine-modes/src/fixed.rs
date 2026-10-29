//! Integer-only math the mode kernels share: 20.12 fixed point, a 12-bit
//! angle space, table sine / cosine, integer `atan2` and square root.
//!
//! Everything here is integer arithmetic on purpose. A lockstep session
//! must compute the same state on every peer, and `f64::sin` / `sqrt`
//! results are allowed to differ between libm builds, CPUs and wasm. The
//! sine table below is a fixed literal, so `sin12` is the same function on
//! every target. (`legaia_engine_system::trig::sin12` evaluates `f64::sin`
//! at run time, which is fine for retail parity but not for lockstep.)

/// A 20.12 fixed-point scalar: `FX_ONE` is `1.0`.
pub type Fx = i32;
/// `1.0` in [`Fx`] - the GTE's 12 fractional bits.
pub const FX_ONE: Fx = 1 << 12;
/// One full turn in the 12-bit angle space.
pub const ANGLE_TURN: u16 = 4096;
/// A quarter turn.
pub const ANGLE_QUARTER: u16 = 1024;

/// `sin` over one quarter turn sampled every 4 angle units,
/// `round(sin(i * pi / 512) * 4096)` for `i in 0..=256`.
const QUARTER_SINE: [i16; 257] = [
    0, 25, 50, 75, 101, 126, 151, 176, 201, 226, 251, 276, 301, 326, 351, 376, 401, 426, 451, 476,
    501, 526, 551, 576, 601, 626, 651, 675, 700, 725, 750, 774, 799, 824, 848, 873, 897, 922, 946,
    971, 995, 1020, 1044, 1068, 1092, 1117, 1141, 1165, 1189, 1213, 1237, 1261, 1285, 1309, 1332,
    1356, 1380, 1404, 1427, 1451, 1474, 1498, 1521, 1544, 1567, 1591, 1614, 1637, 1660, 1683, 1706,
    1729, 1751, 1774, 1797, 1819, 1842, 1864, 1886, 1909, 1931, 1953, 1975, 1997, 2019, 2041, 2062,
    2084, 2106, 2127, 2149, 2170, 2191, 2213, 2234, 2255, 2276, 2296, 2317, 2338, 2359, 2379, 2399,
    2420, 2440, 2460, 2480, 2500, 2520, 2540, 2559, 2579, 2598, 2618, 2637, 2656, 2675, 2694, 2713,
    2732, 2751, 2769, 2788, 2806, 2824, 2843, 2861, 2878, 2896, 2914, 2932, 2949, 2967, 2984, 3001,
    3018, 3035, 3052, 3068, 3085, 3102, 3118, 3134, 3150, 3166, 3182, 3198, 3214, 3229, 3244, 3260,
    3275, 3290, 3305, 3320, 3334, 3349, 3363, 3378, 3392, 3406, 3420, 3433, 3447, 3461, 3474, 3487,
    3500, 3513, 3526, 3539, 3551, 3564, 3576, 3588, 3600, 3612, 3624, 3636, 3647, 3659, 3670, 3681,
    3692, 3703, 3713, 3724, 3734, 3745, 3755, 3765, 3775, 3784, 3794, 3803, 3812, 3822, 3831, 3839,
    3848, 3857, 3865, 3873, 3881, 3889, 3897, 3905, 3912, 3920, 3927, 3934, 3941, 3948, 3954, 3961,
    3967, 3973, 3979, 3985, 3991, 3996, 4002, 4007, 4012, 4017, 4022, 4027, 4031, 4036, 4040, 4044,
    4048, 4052, 4055, 4059, 4062, 4065, 4068, 4071, 4074, 4076, 4079, 4081, 4083, 4085, 4087, 4088,
    4090, 4091, 4092, 4093, 4094, 4095, 4095, 4096, 4096, 4096,
];

/// `sin` of a 12-bit angle in [`Fx`] (`-4096..=4096`), linearly
/// interpolated between the table's 4-unit samples.
pub fn sin12(angle: u16) -> Fx {
    let a = angle & 0xFFF;
    let quadrant = a >> 10;
    let within = (a & 0x3FF) as i32;
    let t = if quadrant & 1 == 0 {
        within
    } else {
        1024 - within
    };
    let i = (t >> 2) as usize;
    let frac = t & 3;
    let lo = QUARTER_SINE[i] as i32;
    let hi = QUARTER_SINE[(i + 1).min(256)] as i32;
    let v = lo + (hi - lo) * frac / 4;
    if quadrant >= 2 { -v } else { v }
}

/// `cos` of a 12-bit angle in [`Fx`].
pub fn cos12(angle: u16) -> Fx {
    sin12(angle.wrapping_add(ANGLE_QUARTER))
}

/// The angle of the vector `(x, y)` in the 12-bit space (`0` = `+x`, a
/// quarter turn = `+y`). `(0, 0)` is angle `0`.
///
/// Integer only: the first-quadrant angle is the binary-searched root of
/// `y * cos(a) - x * sin(a)` over `0..=1024`, then folded by quadrant.
pub fn atan2_12(y: i64, x: i64) -> u16 {
    if x == 0 && y == 0 {
        return 0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (mut lo, mut hi) = (0i64, 1024i64);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let s = sin12(mid as u16) as i64;
        let c = cos12(mid as u16) as i64;
        // The ray at `mid` is still below (ax, ay) while ay*c > ax*s.
        if ay * c > ax * s {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let q = lo as u16;
    match (x >= 0, y >= 0) {
        (true, true) => q & 0xFFF,
        (false, true) => (2048 - q) & 0xFFF,
        (false, false) => (2048 + q) & 0xFFF,
        (true, false) => (4096 - q) & 0xFFF,
    }
}

/// Integer square root, `floor(sqrt(v))`.
pub fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    // Newton from an over-estimate converges monotonically downward.
    let mut x = 1u64 << ((64 - v.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + v / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Fixed-point multiply.
pub fn fx_mul(a: Fx, b: Fx) -> Fx {
    ((a as i64 * b as i64) >> 12) as Fx
}

/// A 2D fixed-point vector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Vec2 {
    pub x: Fx,
    pub y: Fx,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0, y: 0 };

    pub const fn new(x: Fx, y: Fx) -> Self {
        Self { x, y }
    }

    /// A vector of length `len` pointing along `angle`.
    pub fn from_angle(angle: u16, len: Fx) -> Self {
        Self::new(fx_mul(cos12(angle), len), fx_mul(sin12(angle), len))
    }

    /// Squared length in raw units (no fixed-point shift), as `i64`.
    pub fn len_sq(self) -> i64 {
        self.x as i64 * self.x as i64 + self.y as i64 * self.y as i64
    }

    /// Length in [`Fx`].
    pub fn length(self) -> Fx {
        isqrt(self.len_sq() as u64) as Fx
    }

    /// The vector's angle.
    pub fn angle(self) -> u16 {
        atan2_12(self.y as i64, self.x as i64)
    }

    /// Scale by `num / den` (integer, truncating toward zero).
    pub fn scale(self, num: i32, den: i32) -> Vec2 {
        Vec2::new(
            (self.x as i64 * num as i64 / den as i64) as Fx,
            (self.y as i64 * num as i64 / den as i64) as Fx,
        )
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.wrapping_add(o.x), self.y.wrapping_add(o.y))
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.wrapping_sub(o.x), self.y.wrapping_sub(o.y))
    }
}

/// Signed shortest difference `to - from` in the angle space,
/// `-2048..2048`.
pub fn angle_delta(from: u16, to: u16) -> i32 {
    let d = (to as i32 - from as i32).rem_euclid(4096);
    if d >= 2048 { d - 4096 } else { d }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_matches_float_within_two_units() {
        for a in 0..4096u16 {
            let want = (a as f64 * std::f64::consts::TAU / 4096.0).sin() * 4096.0;
            let got = sin12(a) as f64;
            assert!((want - got).abs() <= 2.0, "angle {a}: {got} vs {want}");
        }
        assert_eq!(sin12(0), 0);
        assert_eq!(sin12(1024), 4096);
        assert_eq!(sin12(2048), 0);
        assert_eq!(sin12(3072), -4096);
        assert_eq!(cos12(0), 4096);
    }

    #[test]
    fn atan2_round_trips_every_angle_closely() {
        for a in (0..4096u16).step_by(7) {
            let v = Vec2::from_angle(a, 100 * FX_ONE);
            let back = v.angle();
            assert!(angle_delta(a, back).abs() <= 2, "angle {a} -> {back}");
        }
        assert_eq!(atan2_12(0, 5), 0);
        assert_eq!(atan2_12(5, 0), 1024);
        assert_eq!(atan2_12(0, -5), 2048);
        assert_eq!(atan2_12(-5, 0), 3072);
    }

    #[test]
    fn isqrt_is_floor_sqrt() {
        for v in 0..10_000u64 {
            let r = isqrt(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "{v}");
        }
        assert_eq!(isqrt(u64::MAX), 0xFFFF_FFFF);
    }

    #[test]
    fn angle_delta_wraps_short_way() {
        assert_eq!(angle_delta(4000, 100), 196);
        assert_eq!(angle_delta(100, 4000), -196);
        assert_eq!(angle_delta(0, 2048), -2048);
    }
}
