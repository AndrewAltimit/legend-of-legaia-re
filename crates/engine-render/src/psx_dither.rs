//! CPU mirror of the `PSX_DITHER_WGSL` shader helper, kept byte-for-byte
//! equivalent so the dither algorithm can be unit-tested without a GPU. The
//! PSX GPU dithers when packing a 24-bit shaded colour into the 15-bit
//! (BGR555) framebuffer: a signed 4x4 matrix offset is added to each 8-bit
//! component, the result is clamped, truncated to 5 bits, then expanded back
//! to 8 bits by bit-replication.

/// The PSX GPU's 4x4 ordered-dither offset matrix, row-major by
/// `(y & 3, x & 3)`. Reference: nocash PSX-SPX "GPU - Dithering".
pub const DITHER_MATRIX: [i32; 16] = [
    -4, 0, -3, 1, //
    2, -2, 3, -1, //
    -3, 1, -4, 0, //
    3, -1, 2, -2,
];

/// Dither + quantize one 8-bit colour component at screen pixel
/// `(x, y)`. Returns the 5-bit-truncated value re-expanded to 8 bits
/// (the value the BGR555 framebuffer would read back as).
pub fn dither_component(c8: i32, x: u32, y: u32) -> u8 {
    let d = DITHER_MATRIX[((y & 3) * 4 + (x & 3)) as usize];
    let c = (c8 + d).clamp(0, 255);
    let c5 = c >> 3; // truncate to 5 bits
    ((c5 << 3) | (c5 >> 2)) as u8
}

/// Dither a linear `[0, 1]` RGB triple at pixel `(x, y)`, returning the
/// quantized triple back in `[0, 1]`.
pub fn dither_rgb(rgb: [f32; 3], x: u32, y: u32) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let c8 = (rgb[i] * 255.0).round() as i32;
        out[i] = dither_component(c8, x, y) as f32 / 255.0;
    }
    out
}
