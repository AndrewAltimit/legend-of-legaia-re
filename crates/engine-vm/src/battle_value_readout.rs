//! The multi-cast value readout and its UI teardown.
//!
//! PORT: FUN_801E805C
//!
//! Two jobs in one per-frame body, both keyed on the summon / multi-cast
//! side-band window at `0x801F69xx`:
//!
//! 1. **Teardown.** When the readout flag `DAT_8007B64C` is up, close the UI
//!    elements the finished cast left open - element `0x50` when the shared
//!    buffer `_DAT_8007BD14` is live, then, for each queued entry, the pair
//!    `(id, id - 4)` through `FUN_801D8DE8`.
//! 2. **Render.** For each populated slot in `_DAT_801F6988`, draw a label quad
//!    plus the slot's value as decimal digits, positioned off the slot's HP-bar
//!    widget at `ctx[+0x1074 + ctx[+0x11B6 + slot*0xC] * 4]`.
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801e805c.txt` (1123
//! instructions). See
//! [`docs/reference/functions/battle.md`](../../docs/reference/functions/battle.md)
//! for the entry.
//!
//! ## What this module ports
//!
//! The kernels whose every instruction is accounted for: the digit split, the
//! teardown id pairing, the slot-to-widget indirection, and the label quad's
//! geometry. The per-digit quad placement walks a chain of reciprocal divides
//! interleaved with stores across ~700 instructions and is not reproduced.
//!
//! # NOT WIRED
//!
//! No engine caller, and the reason is neither the site nor the port. Retail
//! calls this unconditionally at the head of every action-SM tick (`jal
//! 0x801e805c` at `0x801E2A70`, ahead of the `ctx[+0x07]` jump), and that head
//! is ported and live - so a caller could be added today and would run every
//! frame. It would have nothing to say. Both halves need a producer the engine
//! does not have: the `0x801F6980` value window and the `0x801F6988` slot list
//! are written by the summon-overlay side band (PROT 0900 / the `readef`
//! streaming slots), which nothing in `engine-core` loads, so every slot reads
//! empty and both the teardown and the render walk return nothing. The output
//! is GPU primitives linked into the ordering table at `_DAT_1F8003A0`;
//! `engine-ui` draws battle damage numbers through its own `TextDraw` path
//! instead. Wiring means the summon side band reaching `engine-core` and the
//! quads reaching `engine-render` - a per-frame call ahead of either is a call
//! that measures as wired and does nothing.

/// Readout slots the value window holds (`_DAT_801F6980..0x801F6987`, four
/// halfwords).
pub const VALUE_SLOTS: usize = 4;
/// Ordering-table tag word the quads carry (`0x09000000`).
pub const OT_TAG: u32 = 0x0900_0000;
/// Command + colour word every quad shares (`0x2C808080` - a textured quad at
/// neutral grey).
pub const CODE_COLOUR: u32 = 0x2C80_8080;
/// Quad size in bytes the primitive cursor advances by (`0x28`).
pub const PRIM_BYTES: usize = 0x28;
/// The UI element closed first when the shared buffer is live.
pub const SHARED_BUFFER_ELEMENT: u8 = 0x50;
/// Offset subtracted to make a queued entry's second teardown id.
pub const TEARDOWN_PAIR_DELTA: u8 = 4;
/// Stride of the per-slot widget-index records at `ctx[+0x11B6]` (`slot * 0xC`).
pub const SLOT_RECORD_STRIDE: usize = 0x0C;
/// Byte offset of the widget index inside the battle context (`+0x11B6`).
pub const SLOT_WIDGET_INDEX: usize = 0x11B6;
/// Byte offset of the widget pointer array inside the battle context
/// (`+0x1074`).
pub const WIDGET_TABLE: usize = 0x1074;

/// Does the teardown pass run?
///
/// Retail's condition is the readout flag up **and** at least one of the four
/// value halfwords non-zero. A flag with an all-zero window skips both the
/// batch and the flag clear, which is what leaves the flag standing for a later
/// frame.
pub fn teardown_runs(readout_flag: u8, values: [u16; VALUE_SLOTS]) -> bool {
    readout_flag != 0 && values.iter().any(|&v| v != 0)
}

/// The `(id, id - 4)` pair each queued entry closes.
///
/// Retail issues `FUN_801D8DE8(id, 0)` and then `FUN_801D8DE8(id - 4, 0)`; the
/// subtraction is `addiu a0, a0, -0x4` on a byte the load zero-extended, so it
/// is a 32-bit subtract that can go negative rather than a byte wrap. The port
/// keeps the signed form.
pub const fn teardown_pair(id: u8) -> (u8, i32) {
    (id, id as i32 - TEARDOWN_PAIR_DELTA as i32)
}

/// The queued entry ids for a batch of `count` entries.
///
/// Retail indexes `_DAT_801F6834 + (count - 1) * 4 + i`, so the *row* is chosen
/// by the count and the entries run along it. A zero count skips the loop.
pub const fn teardown_row_offset(count: u32, index: u32) -> Option<u32> {
    if count == 0 {
        None
    } else {
        Some((count - 1) * 4 + index)
    }
}

/// The widget-pointer index for a readout slot: two indirections, a `0xC`-stride
/// byte record then a word table.
///
/// Returns the byte offset into the battle context that holds the widget
/// pointer, given the byte already read from `ctx[+0x11B6 + slot * 0xC]`.
pub const fn widget_offset(widget_index: u8) -> usize {
    WIDGET_TABLE + widget_index as usize * 4
}

/// The byte offset of a slot's widget-index byte.
pub const fn slot_widget_index_offset(slot: usize) -> usize {
    SLOT_WIDGET_INDEX + slot * SLOT_RECORD_STRIDE
}

/// A decimal split of a readout value, most significant digit first, with
/// leading zeros dropped.
///
/// Retail reaches the digits through reciprocal multiplies rather than `div`:
/// `0xD1B71759` with `mfhi >> 13` is `/ 10000`, and `0xCCCCCCCD` with
/// `mfhi >> 3` is `/ 10`. The ten-thousands digit is tested for zero before
/// anything is drawn, which is what suppresses the leading zero.
pub fn decimal_digits(value: u16) -> Vec<u8> {
    let mut digits = [0u8; 5];
    let mut v = value as u32;
    for slot in digits.iter_mut().rev() {
        *slot = (v % 10) as u8;
        v /= 10;
    }
    let first = digits.iter().position(|&d| d != 0).unwrap_or(4);
    digits[first..].to_vec()
}

/// The two reciprocal divides retail uses, as their own functions, so the
/// magic-constant arithmetic is checkable against plain division.
pub mod reciprocal {
    /// `multu v, 0xD1B71759; mfhi; srl 13` - unsigned divide by 10000.
    pub const fn div10000(v: u32) -> u32 {
        (((v as u64) * 0xD1B7_1759) >> 32) as u32 >> 13
    }

    /// `multu v, 0xCCCCCCCD; mfhi; srl 3` - unsigned divide by 10.
    pub const fn div10(v: u32) -> u32 {
        (((v as u64) * 0xCCCC_CCCD) >> 32) as u32 >> 3
    }
}

/// A textured quad the readout links into the ordering table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadoutQuad {
    /// `+0x00` - ordering-table tag.
    pub tag: u32,
    /// `+0x04` - command and colour.
    pub code_colour: u32,
    /// `+0x08`, `+0x10`, `+0x18`, `+0x20` - screen XY per corner.
    pub xy: [(i16, i16); 4],
    /// `+0x0C`, `+0x14`, `+0x1C`, `+0x24` - texel UV per corner.
    pub uv: [(u8, u8); 4],
    /// `+0x0E` - CLUT word.
    pub clut: u16,
    /// `+0x16` - texture page.
    pub tpage: u16,
}

/// CLUT the label and digit glyphs sample (`0x7703`).
pub const GLYPH_CLUT: u16 = 0x7703;
/// Texture page the label and digit glyphs sample (`0x27`).
pub const GLYPH_TPAGE: u16 = 0x27;

/// The label quad, drawn once per readout pass (only for the first slot).
///
/// Geometry, straight off the stores at `0x801E8208..0x801E82AC`: the quad spans
/// `widget_x + 0x0A .. widget_x + 0x41` by `widget_y - 0x0E .. widget_y + 0x01`,
/// and samples the atlas rectangle `(0, 0xE0) .. (0x37, 0xEF)`. Both extents are
/// `0x37` wide, so the label is drawn 1:1.
pub fn label_quad(widget_x: u16, widget_y: u16) -> ReadoutQuad {
    let x0 = widget_x.wrapping_add(0x0A) as i16;
    let x1 = widget_x.wrapping_add(0x41) as i16;
    let y0 = widget_y.wrapping_sub(0x0E) as i16;
    let y1 = widget_y.wrapping_add(0x01) as i16;
    ReadoutQuad {
        tag: OT_TAG,
        code_colour: CODE_COLOUR,
        xy: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        uv: [(0x00, 0xE0), (0x37, 0xE0), (0x00, 0xEF), (0x37, 0xEF)],
        clut: GLYPH_CLUT,
        tpage: GLYPH_TPAGE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teardown_needs_the_flag_and_a_nonzero_value() {
        assert!(teardown_runs(1, [0, 0, 7, 0]));
        assert!(!teardown_runs(0, [1, 1, 1, 1]), "flag down");
        assert!(!teardown_runs(1, [0; 4]), "window empty");
    }

    #[test]
    fn teardown_pairs_each_id_with_the_one_four_below() {
        assert_eq!(teardown_pair(0x50), (0x50, 0x4C));
        assert_eq!(teardown_pair(0x04), (0x04, 0));
        // Below four the second id goes negative rather than wrapping.
        assert_eq!(teardown_pair(0x02), (0x02, -2));
        assert_eq!(teardown_pair(0), (0, -4));
    }

    #[test]
    fn teardown_row_is_chosen_by_the_count() {
        assert_eq!(teardown_row_offset(0, 0), None);
        assert_eq!(teardown_row_offset(1, 0), Some(0));
        assert_eq!(teardown_row_offset(1, 3), Some(3));
        assert_eq!(teardown_row_offset(2, 0), Some(4));
        assert_eq!(teardown_row_offset(3, 2), Some(10));
    }

    #[test]
    fn widget_lookup_is_a_byte_record_then_a_word_table() {
        assert_eq!(slot_widget_index_offset(0), 0x11B6);
        assert_eq!(slot_widget_index_offset(1), 0x11B6 + 0x0C);
        assert_eq!(slot_widget_index_offset(4), 0x11B6 + 0x30);
        assert_eq!(widget_offset(0), 0x1074);
        assert_eq!(widget_offset(3), 0x1074 + 0x0C);
    }

    #[test]
    fn reciprocals_agree_with_plain_division_across_the_value_range() {
        for v in 0u32..=0xFFFF {
            assert_eq!(reciprocal::div10(v), v / 10, "div10 of {v}");
            assert_eq!(reciprocal::div10000(v), v / 10000, "div10000 of {v}");
        }
    }

    #[test]
    fn digits_drop_leading_zeros_but_keep_a_single_zero() {
        assert_eq!(decimal_digits(0), vec![0]);
        assert_eq!(decimal_digits(7), vec![7]);
        assert_eq!(decimal_digits(70), vec![7, 0]);
        assert_eq!(decimal_digits(1234), vec![1, 2, 3, 4]);
        assert_eq!(decimal_digits(65535), vec![6, 5, 5, 3, 5]);
        assert_eq!(decimal_digits(10000), vec![1, 0, 0, 0, 0]);
    }

    #[test]
    fn digit_count_never_exceeds_five() {
        for v in [0u16, 1, 9, 10, 99, 100, 9999, 10000, 0xFFFF] {
            let d = decimal_digits(v);
            assert!((1..=5).contains(&d.len()), "value {v} -> {d:?}");
            // Reassembling the digits recovers the value.
            let back = d.iter().fold(0u32, |acc, &x| acc * 10 + x as u32);
            assert_eq!(back, v as u32);
        }
    }

    #[test]
    fn label_quad_is_a_one_to_one_blit_of_a_fifty_five_pixel_strip() {
        let q = label_quad(100, 80);
        assert_eq!(q.tag, OT_TAG);
        assert_eq!(q.code_colour, CODE_COLOUR);
        assert_eq!(q.xy[0], (110, 66));
        assert_eq!(q.xy[3], (165, 81));
        // Screen extent equals texel extent in both axes.
        assert_eq!(q.xy[1].0 - q.xy[0].0, (q.uv[1].0 - q.uv[0].0) as i16);
        assert_eq!(q.xy[2].1 - q.xy[0].1, (q.uv[2].1 - q.uv[0].1) as i16);
        assert_eq!(q.clut, GLYPH_CLUT);
        assert_eq!(q.tpage, GLYPH_TPAGE);
    }

    #[test]
    fn label_quad_corners_share_edges() {
        let q = label_quad(0, 32);
        assert_eq!(q.xy[0].1, q.xy[1].1, "top edge");
        assert_eq!(q.xy[2].1, q.xy[3].1, "bottom edge");
        assert_eq!(q.xy[0].0, q.xy[2].0, "left edge");
        assert_eq!(q.xy[1].0, q.xy[3].0, "right edge");
    }

    #[test]
    fn prim_size_matches_a_ten_word_quad() {
        assert_eq!(PRIM_BYTES, 10 * 4);
        // The tag's length field counts the words after it.
        assert_eq!(OT_TAG >> 24, 9);
    }
}
