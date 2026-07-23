//! Small arithmetic kernels lifted from the `SCUS_942.54` battle band.
//!
//! PORT: FUN_80055854, FUN_80046978
//!
//! Two self-contained whole routines: a two-level variable-length word-record
//! copier ([`copy_nested_records`]) and a per-channel RGB colour-modulate with
//! saturation ([`scale_rgb24`]).
//!
//! On top of those, this module carries the reusable *arithmetic cores* of
//! several larger battle-actor routines whose bodies are otherwise
//! render-track (they call the GTE, write GPU primitive packets through the
//! scratchpad OT pointer `_DAT_1f8003a0`, or drive dozens of battle globals)
//! and therefore are **not** ported whole in the clean-room engine. Each core
//! is the faithful, testable computation the retail routine repeats inline:
//!
//! - [`bgr555_to_grey`] - the desaturate step of the stone/petrify CLUT-fade
//!   builder `FUN_8004ce2c`.
//! - [`depth_cue_scale_channel`] - the per-channel depth-brightness ramp of the
//!   actor colour/OTZ setup `FUN_8004a908`.
//! - [`invert_bgr24`] - the "negative colour" status recolour, also from
//!   `FUN_8004a908`.
//!
//! Every claim below is read out of the instruction stream in the reference
//! dumps, not the decompiled C.
//!
//! ## Clean-room boundary
//!
//! No `SCUS_942.54` bytes live in this crate. The reference dumps
//! (`ghidra/scripts/funcs/80055854.txt`, `80046978.txt`, `8004ce2c.txt`,
//! `8004a908.txt`) are the *spec*. The render-track parents of the extracted
//! cores are documented, with provenance, in `docs/subsystems/battle.md`.
//!
//! # NOT WIRED
//!
//! None of these is called by the engine yet. They are ported because each is
//! a faithful, testable computation whose retail edge cases (the do-while
//! floor, the pre-multiply saturation, the luminance clamp, the min-4 dim
//! floor) are observable, and because the engine's battle path is expected to
//! grow a consumer for each - the record copier stages a nested
//! animation/keyframe table, the colour maths feed screen-flash / status /
//! depth-cue submits.

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

// ---------------------------------------------------------------------------
// FUN_8004ce2c - stone/petrify grey-out CLUT-fade luminance
// ---------------------------------------------------------------------------

/// Desaturate one 15-bit `BGR555` pixel to grey - the arithmetic core of the
/// petrify CLUT-fade builder `FUN_8004ce2c`.
///
/// PORT: FUN_8004ce2c
///
/// The retail routine walks a 240-entry captured framebuffer strip, and for
/// each pixel computes a single luminance value and writes it into all three
/// 5-bit channels, producing the grey CLUT the stone-status overlay fades to.
/// Straight off the disassembly (`0x8004d700`):
///
/// - `r = pixel & 0x1F`, `g = (pixel >> 5) & 0x1F`, `b = (pixel >> 10) & 0x1F`.
/// - `lum = (r + g + b) >> 2` (an arithmetic shift; the sum is non-negative
///   so it is a plain floor-divide by four - note the divisor is 4, not 3, so
///   this is a deliberately-dimmed average, max `0x17`).
/// - `if lum > 0x1F { lum = 0x1F }` (`sltiu ..., 0x20`).
/// - repack `lum | (lum << 5) | (lum << 10)`.
///
/// The top `STP` bit (`0x8000`) is **not** set by this variant - it is added
/// by the sibling brightened path (`lum * 3 / 2`), which is a separate ramp
/// and not reproduced here. The surrounding routine (packet build via
/// `_DAT_1f8003a0`, `FUN_800583c8` submit) is render-track; see
/// `docs/subsystems/battle.md`.
pub fn bgr555_to_grey(pixel: u16) -> u16 {
    let r = pixel & 0x1F;
    let g = (pixel >> 5) & 0x1F;
    let b = (pixel >> 10) & 0x1F;
    let lum = ((r + g + b) >> 2).min(0x1F);
    lum | (lum << 5) | (lum << 10)
}

// ---------------------------------------------------------------------------
// FUN_8004a908 - actor depth-cue brightness + negative-colour recolour
// ---------------------------------------------------------------------------

/// Scale one 10-bit colour channel by a depth ratio `num/den`, clamped so a
/// near actor never brightens past its base and a far one never fades to
/// black - the per-channel core of the actor colour/OTZ setup `FUN_8004a908`.
///
/// PORT: FUN_8004a908
///
/// The retail routine computes, per channel, exactly (`0x8004aac8`):
///
/// - `p = (raw * num) / den` - an unsigned 10-bit `*` 16-bit product then
///   `divu` (`den` is the transformed depth `>> 4`; `num` is the mesh's
///   half-range `mesh[+0x58]`).
/// - `if raw < p { p = raw }` - clamp to the base channel, so an actor closer
///   than the half-range stays at full brightness rather than over-driving.
/// - `if p == 0 { p = 4 }` - a dim floor; a fully-faded channel still shows a
///   4/1024 ember rather than pure black.
///
/// Retail guarantees `den >= 1` (the caller replaces a zero depth with 1
/// before this runs); this port maps `den == 0` to `1` to match. The result
/// is the scaled 10-bit value; retail then quantises it to 8 bits per channel
/// (`>> 2` / `& 0x3FC`) when packing the GPU colour word - that packing, and
/// the `FUN_8003d344` GTE transform the depth comes from, are render-track and
/// live in `docs/subsystems/battle.md`.
pub fn depth_cue_scale_channel(raw10: u16, num: u16, den: u16) -> u16 {
    let raw = (raw10 & 0x3FF) as u32;
    let den = den.max(1) as u32;
    let mut p = (raw * num as u32) / den;
    if raw < p {
        p = raw;
    }
    if p == 0 {
        p = 4;
    }
    p as u16
}

/// Invert the low 24 bits (`0x00BBGGRR`) of a packed actor colour word while
/// preserving the top byte - the "negative colour" status recolour from
/// `FUN_8004a908`.
///
/// PORT: FUN_8004a908
///
/// Off the disassembly (`0x8004abf4`): `out = (0xFFFFFF - (c & 0xFFFFFF)) |
/// (c & 0xFF000000)`. Retail applies it only when the three channels are
/// already equal (a greyscale word) - that guard belongs to the caller; this
/// function is the recolour itself, which is a plain complement of the colour
/// bits with the GPU code/attribute byte in bits 24..31 left intact.
pub fn invert_bgr24(color: u32) -> u32 {
    (0x00FF_FFFF - (color & 0x00FF_FFFF)) | (color & 0xFF00_0000)
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

    #[test]
    fn grey_replicates_luminance_into_all_channels() {
        // White (all 0x1F): sum 0x5D >> 2 = 0x17, clamp keeps 0x17.
        let g = bgr555_to_grey(0x7FFF);
        let lum = g & 0x1F;
        assert_eq!(lum, 0x17);
        assert_eq!((g >> 5) & 0x1F, lum);
        assert_eq!((g >> 10) & 0x1F, lum);
        assert_eq!(g & 0x8000, 0, "STP bit is never set by this variant");
    }

    #[test]
    fn grey_black_stays_black() {
        assert_eq!(bgr555_to_grey(0x0000), 0);
    }

    #[test]
    fn grey_averages_the_three_5bit_channels_floor_divided_by_four() {
        // r=0x1F, g=0, b=0 -> sum 0x1F >> 2 = 7.
        let g = bgr555_to_grey(0x001F);
        assert_eq!(g & 0x1F, 7);
        assert_eq!(g, 7 | (7 << 5) | (7 << 10));
    }

    #[test]
    fn grey_output_is_a_valid_15bit_word_with_equal_channels() {
        // A single 555 pixel can never exceed lum 0x17, so the top bit stays
        // clear and all three channels always match for every input.
        for p in [0x7FFFu16, 0x03FF, 0x7C1F, 0x0000] {
            let g = bgr555_to_grey(p);
            let lum = g & 0x1F;
            assert_eq!((g >> 5) & 0x1F, lum);
            assert_eq!((g >> 10) & 0x1F, lum);
            assert!(g < 0x8000, "no STP bit, so top bit clear");
        }
    }

    #[test]
    fn depth_cue_full_brightness_when_closer_than_half_range() {
        // den (depth) < num (half-range) -> (raw*num)/den >= raw -> clamp to raw.
        assert_eq!(depth_cue_scale_channel(0x200, 0x40, 0x10), 0x200);
    }

    #[test]
    fn depth_cue_dims_when_farther_than_half_range() {
        // raw=0x100, num=8, den=0x20 -> (0x100*8)/0x20 = 0x40.
        assert_eq!(depth_cue_scale_channel(0x100, 8, 0x20), 0x40);
    }

    #[test]
    fn depth_cue_floor_is_four_not_zero() {
        // Product rounds to 0 -> dim floor of 4.
        assert_eq!(depth_cue_scale_channel(1, 1, 0x40), 4);
    }

    #[test]
    fn depth_cue_masks_input_to_ten_bits_and_guards_zero_den() {
        // Bits above bit 9 in raw are dropped before scaling.
        assert_eq!(
            depth_cue_scale_channel(0xFC00 | 0x100, 1, 1),
            depth_cue_scale_channel(0x100, 1, 1)
        );
        // den == 0 is treated as 1 (retail guarantees >= 1 upstream).
        assert_eq!(depth_cue_scale_channel(0x080, 1, 0), 0x080);
    }

    #[test]
    fn invert_complements_colour_bits_keeps_top_byte() {
        assert_eq!(invert_bgr24(0x00_00_00_00), 0x00FF_FFFF);
        assert_eq!(invert_bgr24(0x00_FF_FF_FF), 0x0000_0000);
        // Top byte (GPU code/attr) survives untouched.
        assert_eq!(invert_bgr24(0xC5_10_20_30), 0xC5_EF_DF_CF);
    }

    #[test]
    fn invert_is_self_inverse_on_the_colour_bits() {
        for c in [0x00_12_34_56u32, 0x81_00_80_FF, 0x00_7F_7F_7F] {
            assert_eq!(invert_bgr24(invert_bgr24(c)), c);
        }
    }
}
