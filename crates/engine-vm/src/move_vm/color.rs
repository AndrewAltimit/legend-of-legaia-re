//! Clean-room RGB<->HSV helpers used by ext sub-ops 0x1F / 0x20.

/// Clean-room RGB→HSV port of `FUN_8001a78c`. Inputs are 0..255; outputs are
/// `(H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255)`. Used by ext sub-ops 0x1F / 0x20
/// to rotate a packed RGB color in HSV space.
///
/// The original SCUS implementation uses signed-integer division with
/// fixed-point scaling by `0x100` and the `0x60 / 0x100 = 60/256` segment
/// multiplier - the result space is effectively degrees in 0..360 (= 0x168).
pub(crate) fn rgb_to_hsv(r: i32, g: i32, b: i32) -> (i32, i32, i32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let diff = max - min;
    let v = max;
    if max == 0 {
        return (0, 0, 0);
    }
    let s = (diff * 0x100) / max;
    if s == 0 {
        return (0, 0, v);
    }
    // Hue computation in segment-based form.
    let mut h = if r == max {
        ((g - b) * 0x100) / diff
    } else if g == max {
        ((b - r) * 0x100) / diff + 0x200
    } else {
        ((r - g) * 0x100) / diff + 0x400
    };
    h = (h * 0x3C) >> 8;
    if h < 0 {
        h += 0x168;
    }
    (h, s, v)
}

/// Clean-room HSV→RGB port of `FUN_8001a8dc`. `H ∈ 0..0x167`, `S, V ∈ 0..256`.
/// Returns `(R, G, B)` each in 0..255 (caller may clamp further; FUN_8001a6c8
/// caps at 0xF8). Used by ext sub-ops 0x1F / 0x20.
pub(crate) fn hsv_to_rgb(h: i32, s: i32, v: i32) -> (i32, i32, i32) {
    let s = s.clamp(0, 0x100);
    let v = v.clamp(0, 0x100);
    let mut h_scaled = (h.rem_euclid(0x168)) * 0x100;
    if h_scaled < 0 {
        h_scaled += 0x16800;
    }
    let f = ((h_scaled / 0x3C) & 0xFF) as u32 as i32;
    let segment = (h_scaled / 0x3C) >> 8;
    let p = (v * (0x100 - s)) >> 8;
    let q = (v * (0x100 - ((s * f) >> 8))) >> 8;
    let t = (v * (0x100 - ((s * (0x100 - f)) >> 8))) >> 8;
    match segment {
        0 | 6 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 => (v, p, q),
        _ => (0, 0, 0),
    }
}
