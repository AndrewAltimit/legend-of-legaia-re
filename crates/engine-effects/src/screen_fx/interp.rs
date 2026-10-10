//! The interpolation kernel the screen-effect widgets ease with, scalar and RGB.

// REF: FUN_80020DE0, FUN_8003CF04, FUN_801DE840, FUN_80021B04, FUN_801F7088
// REF: FUN_8003CE9C, FUN_8003CEB8, FUN_8003CE64, FUN_80059010

/// Interpolation modes of the shared tween helper `FUN_801DE4C8`.
///
/// Mode 1 is plain linear; 2/3 are quadratic ease-out / ease-in; 4 is the
/// two-segment ease-in-out (first half mode-3 toward the midpoint over `D/2`,
/// second half mode-2 from the midpoint). All arithmetic is integer with
/// truncating division, exactly as retail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpMode {
    /// `(a-b)*t/D + b`
    Linear,
    /// `e = (a-b)*t; (e + (e/D)*(D-t))/D + b` - quadratic ease-out.
    EaseOut,
    /// `((a-b)*t/D)*t/D + b` - quadratic ease-in.
    EaseIn,
    /// Ease-in to the midpoint over `D/2`, then ease-out to the target.
    EaseInOut,
}

impl InterpMode {
    /// Decode a script mode byte (`FUN_801DE4C8`'s `param_5`). Any value other
    /// than 2/3/4 takes the linear arm (retail's `default`).
    pub fn from_byte(b: u8) -> Self {
        match b {
            2 => InterpMode::EaseOut,
            3 => InterpMode::EaseIn,
            4 => InterpMode::EaseInOut,
            _ => InterpMode::Linear,
        }
    }
}

/// Multi-mode integer interpolator - full port of `FUN_801DE4C8(a, b, t, D,
/// mode)`: interpolate from `start` (`b`) toward `target` (`a`) at time `t` of
/// duration `dur`. Returns `target` exactly when `target == start` or
/// `t >= dur`. Division truncates toward zero (MIPS `div`), which is Rust `/`
/// on `i32`.
// PORT: FUN_801DE4C8 (PROT 0897)
pub fn interp(target: i32, start: i32, t: i32, dur: i32, mode: InterpMode) -> i32 {
    if target == start || dur <= t {
        return target;
    }
    match mode {
        InterpMode::Linear => (target - start) * t / dur + start,
        InterpMode::EaseOut => {
            let e = (target - start) * t;
            (e + (e / dur) * (dur - t)) / dur + start
        }
        InterpMode::EaseIn => ((target - start) * t / dur) * t / dur + start,
        InterpMode::EaseInOut => {
            let half = dur >> 1;
            if half < t {
                // Second half: ease-out from the midpoint over the remainder.
                let mid = start + ((target - start) >> 1);
                interp(target, mid, t - half, half, InterpMode::EaseOut)
            } else {
                // First half: ease-in covering half the distance.
                let span = (target - start) >> 1;
                (span * t / half) * t / half + start
            }
        }
    }
}

/// Per-byte tween of an RGB triple (the sprite handler's colour channels).
pub(super) fn interp_rgb(
    target: [u8; 3],
    start: [u8; 3],
    t: i32,
    d: i32,
    mode: InterpMode,
) -> [u8; 3] {
    let mut out = [0u8; 3];
    for ((o, &tv), &sv) in out.iter_mut().zip(target.iter()).zip(start.iter()) {
        *o = interp(tv as i32, sv as i32, t, d, mode) as u8;
    }
    out
}
