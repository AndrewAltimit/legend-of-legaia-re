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
//! ## The sheet, read out of a retail frame
//!
//! [`GLYPH_TPAGE`] / [`GLYPH_CLUT`] are not just this routine's - they are the
//! whole battle value readout's, per-hit numerals included, and the sheet they
//! name is legible. A mednafen battle save state carries VRAM verbatim, so
//! decoding page `(448, 0)` at 4bpp through the CLUT at `(48, 476)` shows:
//!
//! | texels | content |
//! |---|---|
//! | `v = 64..=87` | ten 24x24 digit cells, in strip order **`1234567890`** |
//! | `u = 0..=55, v = 224..=239` | the `DAMAGE` label - [`label_quad`]'s rect |
//! | `u = 0..=31, v = 240..=255` | the `HIT` label |
//! | `u = 32..=79, v = 240..=255` | the `TOTAL` label |
//!
//! The strip starts at `1`, not `0`, so a digit's cell is
//! [`digit_cell_u`]: `((d + 9) % 10) * 24`. The page is inside the battle
//! effect atlas (PROT 870), which the battle loader already makes resident -
//! the digits need no separate asset.
//!
//! ## The per-hit floating numeral
//!
//! The same sheet carries the numeral a landed hit throws, and the display
//! list pins its geometry directly. In `battle_melee_hit_spark` both frame
//! arenas are live in one RAM image, so the pair reads as an animation:
//!
//! | arena | cells | screen |
//! |---|---|---|
//! | earlier | `u = 0` and `u = 96` (= `15`) | `(118, 49)` and `(137, 49)`, each 18x19 |
//! | later | same cells | `(114, 32)` and `(137, 32)`, each 22x22 |
//!
//! Three laws fall out and are ported below: the run's horizontal **centre**
//! holds (136.5 in both), the cell **grows** toward its 1:1 24-px size, and
//! the run **rises** to a fixed screen row - `y = 32`, which is also where the
//! `battle_gimard_tail_fire` pair sits with its growth already finished
//! (cells 19, 20, 22 and 23 px at that same row, over a different monster at
//! a different `x`). Cell pitch is the cell width plus one in every sample.
//! The quads are `0x2C` at colour `0x808080` - opaque, unmodulated - so retail
//! does **not** fade the numeral out; it ends by no longer being emitted.
//!
//! [`value_cells`] is that layout. What is *not* pinned is the frame count the
//! growth and the rise take, or the numeral's total lifetime: no capture
//! carries a frame index. [`POP_FRAMES`] and [`POP_START_CELL`] are therefore
//! engine-chosen, constrained at both ends by the measured sizes.
//!
//! # Wiring
//!
//! [`value_cells`] is live: the native window lays a landed hit's damage out
//! with it and draws the cells as screen-space VRAM quads off the resident
//! effect atlas, so the numerals are retail's own art at retail's own
//! geometry.
//!
//! The multi-cast half above it - the teardown pass and the
//! `DAMAGE`/`HIT`/`TOTAL` combo cluster - stays unwired, and for a reason that
//! is neither the site nor the port. Retail calls this unconditionally at the
//! head of every action-SM tick (`jal 0x801e805c` at `0x801E2A70`, ahead of
//! the `ctx[+0x07]` jump), and that head is ported and live. It would have
//! nothing to say: the `0x801F6980` value window and the `0x801F6988` slot
//! list are written by the summon-overlay side band (PROT 0900 / the `readef`
//! streaming slots), which nothing in `engine-core` loads, so every slot reads
//! empty and both the teardown and the combo walk return nothing.

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

/// CLUT the label and digit glyphs sample (`0x7703` - VRAM `(48, 476)`).
pub const GLYPH_CLUT: u16 = 0x7703;
/// Texture page the label and digit glyphs sample (`0x27` - VRAM page
/// `(448, 0)` at 4bpp, inside the battle effect atlas).
pub const GLYPH_TPAGE: u16 = 0x27;

/// Texel row the ten digit cells start on.
pub const DIGIT_ROW_V: u8 = 64;
/// One digit cell is square, this many texels on a side.
pub const DIGIT_CELL: u8 = 24;
/// Screen pitch between adjacent cells is the drawn cell width plus this.
pub const DIGIT_GAP: i32 = 1;

/// Screen row the floating numeral rises to and rests on.
pub const RESTING_TOP_Y: i32 = 32;
/// Frames the pop-in growth and the rise take. **Engine-chosen** - the
/// captures pin the endpoints, not the rate.
pub const POP_FRAMES: u16 = 8;
/// Drawn cell size the pop starts at, in screen pixels. The smallest measured
/// cell is 18; retail's own start frame is not captured.
pub const POP_START_CELL: u32 = 16;

/// Inclusive texel rect of the `DAMAGE` label (the one [`label_quad`] draws).
pub const LABEL_DAMAGE: (u8, u8, u8, u8) = (0x00, 0xE0, 0x37, 0xEF);
/// Inclusive texel rect of the `HIT` label.
pub const LABEL_HIT: (u8, u8, u8, u8) = (0x00, 0xF0, 0x1F, 0xFF);
/// Inclusive texel rect of the `TOTAL` label.
pub const LABEL_TOTAL: (u8, u8, u8, u8) = (0x20, 0xF0, 0x4F, 0xFF);

/// Left texel of a decimal digit's cell.
///
/// The strip runs `1234567890`, so `1` is the first cell and `0` the last -
/// `((d + 9) % 10) * 24`. Read straight off the decoded sheet, and
/// corroborated by the two-cell `15` in `battle_melee_hit_spark` landing on
/// `u = 0` and `u = 96`.
pub const fn digit_cell_u(digit: u8) -> u8 {
    ((digit + 9) % 10) * DIGIT_CELL
}

/// One drawn digit of a floating value readout.
///
/// Screen fields are stage pixels on retail's 320x240 stage; texel fields
/// address [`GLYPH_TPAGE`] through [`GLYPH_CLUT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueCell {
    /// The decimal digit this cell shows.
    pub digit: u8,
    /// Stage-pixel top-left corner.
    pub x: i32,
    pub y: i32,
    /// Stage-pixel extent (square while the pop is running).
    pub w: u32,
    pub h: u32,
    /// Texel top-left corner on the glyph page.
    pub u: u8,
    pub v: u8,
    /// Texel extent - always [`DIGIT_CELL`]; the screen extent is what scales.
    pub cell: u8,
}

/// Drawn cell size `age` frames into the pop, in screen pixels.
///
/// Grows linearly from [`POP_START_CELL`] to the 1:1 [`DIGIT_CELL`] over
/// [`POP_FRAMES`] and then holds. The endpoints are measured; the ramp
/// between them is engine-chosen.
pub fn pop_cell_px(age: u16) -> u32 {
    let end = u32::from(DIGIT_CELL);
    if age >= POP_FRAMES {
        return end;
    }
    let span = end - POP_START_CELL;
    POP_START_CELL + span * u32::from(age) / u32::from(POP_FRAMES)
}

/// Top edge `age` frames into the pop: rises from `start_y` to
/// [`RESTING_TOP_Y`] over [`POP_FRAMES`], then holds. A numeral that starts at
/// or above the resting row never moves.
pub fn pop_top_y(start_y: i32, age: u16) -> i32 {
    if start_y <= RESTING_TOP_Y {
        // Already at or above the resting row - the rise never pushes down.
        return start_y;
    }
    if age >= POP_FRAMES {
        return RESTING_TOP_Y;
    }
    let travel = start_y - RESTING_TOP_Y;
    start_y - travel * i32::from(age) / i32::from(POP_FRAMES)
}

/// Lay a value out as floating digit cells.
///
/// `centre_x` is the stage column the run is centred on - retail holds the
/// centre fixed while the cells grow - and `start_y` the top edge the run pops
/// in at, which the rise walks up to [`RESTING_TOP_Y`]. `age` is frames since
/// the hit landed.
///
/// Leading zeros are dropped ([`decimal_digits`]), so a zero value still draws
/// a single `0` cell.
pub fn value_cells(value: u16, centre_x: i32, start_y: i32, age: u16) -> Vec<ValueCell> {
    let digits = decimal_digits(value);
    let size = pop_cell_px(age);
    let pitch = size as i32 + DIGIT_GAP;
    let run = pitch * digits.len() as i32 - DIGIT_GAP;
    let left = centre_x - run / 2;
    let top = pop_top_y(start_y, age);
    digits
        .iter()
        .enumerate()
        .map(|(i, &d)| ValueCell {
            digit: d,
            x: left + i as i32 * pitch,
            y: top,
            w: size,
            h: size,
            u: digit_cell_u(d),
            v: DIGIT_ROW_V,
            cell: DIGIT_CELL,
        })
        .collect()
}

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
    fn digit_strip_starts_at_one_and_ends_at_zero() {
        // Read off the decoded sheet: the row is "1234567890".
        assert_eq!(digit_cell_u(1), 0);
        assert_eq!(digit_cell_u(2), 24);
        assert_eq!(digit_cell_u(9), 192);
        assert_eq!(digit_cell_u(0), 216);
        // Every cell is inside the 256-texel page row.
        for d in 0u8..=9 {
            assert!(u16::from(digit_cell_u(d)) + u16::from(DIGIT_CELL) <= 256);
        }
    }

    #[test]
    fn label_rects_are_the_three_the_sheet_carries() {
        // `label_quad`'s own rect is the DAMAGE label.
        let q = label_quad(0, 0);
        assert_eq!(
            (q.uv[0].0, q.uv[0].1, q.uv[3].0, q.uv[3].1),
            LABEL_DAMAGE,
            "label_quad draws DAMAGE"
        );
        // HIT and TOTAL share the row below it and do not overlap.
        assert_eq!(LABEL_HIT.1, LABEL_TOTAL.1);
        assert!(LABEL_HIT.2 < LABEL_TOTAL.0);
        // Measured drawn widths: 55 / 31 / 47 px for inclusive corner spans.
        assert_eq!(LABEL_DAMAGE.2 - LABEL_DAMAGE.0, 55);
        assert_eq!(LABEL_HIT.2 - LABEL_HIT.0, 31);
        assert_eq!(LABEL_TOTAL.2 - LABEL_TOTAL.0, 47);
    }

    #[test]
    fn the_pop_grows_to_a_one_to_one_cell_and_holds() {
        assert_eq!(pop_cell_px(0), POP_START_CELL);
        assert_eq!(pop_cell_px(POP_FRAMES), u32::from(DIGIT_CELL));
        assert_eq!(pop_cell_px(POP_FRAMES * 4), u32::from(DIGIT_CELL));
        // Monotonic.
        for age in 0..POP_FRAMES {
            assert!(pop_cell_px(age) <= pop_cell_px(age + 1));
        }
    }

    #[test]
    fn the_run_rises_to_the_resting_row_and_stops() {
        assert_eq!(pop_top_y(49, 0), 49);
        assert_eq!(pop_top_y(49, POP_FRAMES), RESTING_TOP_Y);
        assert_eq!(pop_top_y(49, 240), RESTING_TOP_Y);
        // A seat already above the row is left alone rather than pushed down.
        assert_eq!(pop_top_y(10, 0), 10);
        assert_eq!(pop_top_y(10, POP_FRAMES), 10);
    }

    #[test]
    fn the_run_grows_about_a_fixed_centre() {
        // `battle_melee_hit_spark`: `15` at cells 18 and 22 px, centre 136.5
        // in both arenas (118..155 then 114..159).
        let narrow = value_cells(15, 136, 49, 0);
        let wide = value_cells(15, 136, 49, POP_FRAMES);
        let span = |c: &[ValueCell]| {
            let l = c.first().unwrap().x;
            let r = c.last().unwrap().x + c.last().unwrap().w as i32;
            (l + r) / 2
        };
        assert_eq!(
            span(&narrow),
            span(&wide),
            "centre holds while the cells grow"
        );
        assert!(wide[0].w > narrow[0].w);
        // Pitch is the drawn width plus one.
        assert_eq!(wide[1].x - wide[0].x, wide[0].w as i32 + DIGIT_GAP);
    }

    #[test]
    fn value_cells_address_the_right_digits() {
        let cells = value_cells(15, 100, 40, POP_FRAMES);
        assert_eq!(cells.len(), 2);
        assert_eq!((cells[0].digit, cells[0].u), (1, 0));
        assert_eq!((cells[1].digit, cells[1].u), (5, 96));
        assert!(
            cells
                .iter()
                .all(|c| c.v == DIGIT_ROW_V && c.cell == DIGIT_CELL)
        );
        // Zero still draws one cell rather than none.
        assert_eq!(value_cells(0, 0, 0, 0).len(), 1);
    }

    #[test]
    fn prim_size_matches_a_ten_word_quad() {
        assert_eq!(PRIM_BYTES, 10 * 4);
        // The tag's length field counts the words after it.
        assert_eq!(OT_TAG >> 24, 9);
    }
}
