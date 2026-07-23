//! Two small leaf routines from the `SCUS_942.54` battle band.
//!
//! PORT: FUN_80055854, FUN_80046978
//!
//! Both are self-contained arithmetic kernels the surrounding battle code
//! calls: a two-level variable-length word-record copier and a per-channel
//! RGB colour-modulate with saturation. Every claim below is read out of
//! the instruction stream in the reference dumps, not the decompiled C.
//!
//! ## Clean-room boundary
//!
//! No `SCUS_942.54` bytes live in this crate. The reference dumps
//! (`ghidra/scripts/funcs/80055854.txt`, `80046978.txt`) are the *spec*.
//!
//! # NOT WIRED
//!
//! Neither is called by the engine yet. They are ported because each is a
//! faithful, testable computation whose retail edge cases (the do-while
//! floor, the pre-multiply saturation) are observable, and because the
//! engine's battle path is expected to grow a consumer for both - the
//! record copier stages a nested animation/keyframe table, the colour
//! modulate feeds a screen-flash/overlay submit.

// ---------------------------------------------------------------------------
// FUN_80055854 - two-level nested word-record copy
// ---------------------------------------------------------------------------

/// Copy a two-level, variable-length record structure of 32-bit words -
/// `FUN_80055854`.
///
/// PORT: FUN_80055854
///
/// The retail routine takes a source and destination `u32*` and walks a
/// nested table, returning the advanced destination pointer (i.e. the
/// number of words written). The layout, straight off the loads/stores:
///
/// - **Outer header**: three words. Words 0 and 1 are copied verbatim;
///   word 2 is the outer record count (`t0`). If it is `<= 0` the routine
///   stops after the header (`blez t0` at `0x80055884`).
/// - For each of `count` outer records:
///   - **Inner header**: three words. Words 0 and 1 verbatim; word 2 is
///     the inner element count (`v0`).
///   - **Inner body**: `inner_count * 2` words (`sll a2, v0, 1` at
///     `0x800558bc`), copied verbatim. A non-positive doubled count skips
///     the body (`blez a2`).
///
/// Both count tests are signed (`blez` / `slt`), and both loops are
/// `while`-shaped (the guard precedes the body), so a zero count copies
/// only the header - unlike the `do-while` copier in
/// [`scus_core_helpers::copy_blocks_32`](crate::scus_core_helpers::copy_blocks_32).
///
/// This port reads whole words from `src` and writes them into `dst`,
/// returning the count of words written. It stops early - returning what
/// it has copied - if either slice runs short, where retail (which does no
/// bounds check) would stride into adjacent memory.
pub fn copy_nested_records(src: &[u32], dst: &mut [u32]) -> usize {
    let mut si = 0usize;
    let mut di = 0usize;

    // Helper: copy one word src[si] -> dst[di], advancing both. Returns
    // false when either side is exhausted.
    macro_rules! copy_one {
        () => {{
            match (src.get(si), dst.get_mut(di)) {
                (Some(&w), Some(slot)) => {
                    *slot = w;
                    si += 1;
                    di += 1;
                    true
                }
                _ => return di,
            }
        }};
    }

    // Outer header: word0, word1, then the outer count word.
    if !copy_one!() {
        return di;
    }
    if !copy_one!() {
        return di;
    }
    let outer_count = match src.get(si) {
        Some(&w) => w as i32,
        None => return di,
    };
    if !copy_one!() {
        return di;
    }

    if outer_count <= 0 {
        return di;
    }

    for _ in 0..outer_count {
        // Inner header: word0, word1, then the inner count word.
        if !copy_one!() {
            return di;
        }
        if !copy_one!() {
            return di;
        }
        let inner_count = match src.get(si) {
            Some(&w) => w as i32,
            None => return di,
        };
        if !copy_one!() {
            return di;
        }

        // Body: inner_count * 2 words.
        let body = inner_count.wrapping_mul(2);
        if body > 0 {
            for _ in 0..body {
                if !copy_one!() {
                    return di;
                }
            }
        }
    }

    di
}

// ---------------------------------------------------------------------------
// FUN_80046978 - per-channel RGB colour modulate with saturation
// ---------------------------------------------------------------------------

/// Scale each 8-bit channel of a packed `0x00BBGGRR` colour by `scale`,
/// saturating each product at `0xFF` - the arithmetic core of
/// `FUN_80046978`.
///
/// PORT: FUN_80046978
///
/// Retail unpacks the stored colour word into its low three bytes, and for
/// each channel computes `channel * scale` (an 8-bit `* 8-bit` product, at
/// most `0xFE01`), clamping to `0xFF` when the product reaches `0x100`
/// (`slti ..., 0x100`). The three clamped channels are repacked in the
/// original `R | G<<8 | B<<16` order. The top byte is dropped - the routine
/// never reads or reassembles it.
///
/// The full `FUN_80046978` is a triggered submit and is **not** reproduced
/// here: it early-outs unless the trigger word `gp[0x9D4]` is non-zero,
/// clears that word, reads the stored colour from `gp[0x9D0]` and the
/// scale byte from `0x1F800393` (a scratchpad global), and hands the packed
/// result to `FUN_80024EE4(id - 1, 2, packed)` where `id` is the `u16` at
/// `0x1F8003A6`. Those globals and the submit belong to the caller; this
/// function is the colour maths, which is the reusable and testable part.
pub fn scale_rgb24(packed: u32, scale: u8) -> u32 {
    let s = scale as u32;
    let chan = |shift: u32| -> u32 {
        let c = (packed >> shift) & 0xFF;
        let p = c * s;
        if p >= 0x100 { 0xFF } else { p }
    };
    let r = chan(0);
    let g = chan(8);
    let b = chan(16);
    r | (g << 8) | (b << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_copy_header_only_when_outer_count_is_zero() {
        // Outer header [w0, w1, 0] with a non-zero count word absent.
        let src = [0xAAAA_AAAA, 0xBBBB_BBBB, 0, 0xDEAD_BEEF];
        let mut dst = [0u32; 8];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 3, "blez outer count stops after the header");
        assert_eq!(&dst[..3], &[0xAAAA_AAAA, 0xBBBB_BBBB, 0]);
        assert_eq!(dst[3], 0, "trailing word untouched");
    }

    #[test]
    fn nested_copy_one_outer_one_inner_pair() {
        // Outer: [h0, h1, count=1]
        //   Inner: [i0, i1, inner=2] then 2*2 = 4 body words.
        let src = [
            0x0000_00A0, // h0
            0x0000_00A1, // h1
            1,           // outer count
            0x0000_00B0, // inner h0
            0x0000_00B1, // inner h1
            2,           // inner count
            0x10,
            0x11,
            0x12,
            0x13, // 4 body words
        ];
        let mut dst = [0u32; 16];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, src.len());
        assert_eq!(&dst[..src.len()], &src[..]);
    }

    #[test]
    fn nested_copy_inner_body_is_double_the_inner_count() {
        // inner count 3 -> 6 body words.
        let mut src = vec![0xF0, 0xF1, 1, 0xE0, 0xE1, 3];
        src.extend([0x20, 0x21, 0x22, 0x23, 0x24, 0x25]);
        let mut dst = [0u32; 16];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 12);
        assert_eq!(&dst[..12], &src[..]);
    }

    #[test]
    fn nested_copy_zero_inner_count_copies_only_inner_header() {
        let src = [0xA0, 0xA1, 1, 0xB0, 0xB1, 0, 0x99];
        let mut dst = [0u32; 8];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 6, "blez a2 skips the body");
        assert_eq!(&dst[..6], &src[..6]);
        assert_eq!(dst[6], 0);
    }

    #[test]
    fn nested_copy_stops_when_destination_is_short() {
        let src = [0xA0, 0xA1, 1, 0xB0, 0xB1, 1, 0x30, 0x31];
        let mut dst = [0u32; 4];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 4, "no overrun past dst");
        assert_eq!(&dst, &[0xA0, 0xA1, 1, 0xB0]);
    }

    #[test]
    fn scale_rgb_identity_at_scale_one() {
        assert_eq!(scale_rgb24(0x00_12_34_56, 1), 0x00_12_34_56);
    }

    #[test]
    fn scale_rgb_zero_scale_blacks_out() {
        assert_eq!(scale_rgb24(0x00_FF_FF_FF, 0), 0);
    }

    #[test]
    fn scale_rgb_saturates_each_channel_independently() {
        // R=0x08 *4 = 0x20 (no clamp), G=0x40 *4 = 0x100 -> 0xFF,
        // B=0x80 *4 = 0x200 -> 0xFF.
        let packed = 0x00_80_40_08;
        assert_eq!(scale_rgb24(packed, 4), 0x00_FF_FF_20);
    }

    #[test]
    fn scale_rgb_clamp_threshold_is_0x100() {
        // 0x33 * 5 = 0xFF -> exactly under 0x100, kept.
        assert_eq!(scale_rgb24(0x00_00_00_33, 5) & 0xFF, 0xFF);
        // 0x34 * 5 = 0x104 -> clamps to 0xFF.
        assert_eq!(scale_rgb24(0x00_00_00_34, 5) & 0xFF, 0xFF);
        // 0x33 * 4 = 0xCC -> below threshold, kept as product.
        assert_eq!(scale_rgb24(0x00_00_00_33, 4) & 0xFF, 0xCC);
    }

    #[test]
    fn scale_rgb_drops_the_top_byte() {
        // A non-zero alpha/top byte must not appear in the result.
        assert_eq!(scale_rgb24(0xFF_00_00_00, 3) & 0xFF00_0000, 0);
    }
}
