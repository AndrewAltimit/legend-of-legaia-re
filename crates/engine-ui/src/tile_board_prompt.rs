//! The tile board's **quit prompt** panel - the draw half of walk-SM state
//! `5`, shared by both hosts.
//!
//! The state machine and the geometry live in `engine-core`
//! (`legaia_engine_core::tile_board`, `PROMPT_FRAME` / `PROMPT_ROW_Y` /
//! `PROMPT_CURSOR_X`) and arrive here as arguments, as the text balloon's
//! do, so a second copy of the literals cannot drift.
//!
//! ## What retail draws
//!
//! The render tail's state-5 arm (`0x801EFED8..0x801EFFA8`) draws, in this
//! order: the title line at the frame's top-left pen, the two option rows,
//! the pointing hand (`FUN_8002B994` kind 0) beside the selected row, then
//! the frame itself (`FUN_80034B6C(0x44)`, `FUN_8002C69C(100, 92, 120, 40)`).
//! The three lines are strings of the field overlay's own data segment,
//! which the host reads off the disc; this module lays them out.
//!
//! REF: FUN_801EF2B0 (the render tail's state-5 arm), FUN_8002C69C,
//! FUN_80036888, FUN_8002B994

use crate::*;

/// The prompt panel's geometry, in 320x240 stage pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileBoardPromptLayout {
    /// The frame rect `(x, y, w, h)`; the title pen is its top-left.
    pub frame: (i32, i32, i32, i32),
    /// The two option rows' pen `(x, y)`.
    pub rows: [(i32, i32); 2],
    /// The hand cursor's X.
    pub cursor_x: i32,
}

/// Chrome + hand cursor for the prompt with `cursor` on row `0` or `1`.
pub fn tile_board_prompt_sprites_for(
    rects: &SaveMenuAtlasRects,
    layout: &TileBoardPromptLayout,
    cursor: usize,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = dialog_window_chrome_draws_for(rects, layout.frame, stage_origin, stage_scale);
    let scale = stage_scale.max(1) as i32;
    let (_, _, w, h) = rects.cursor;
    let row_y = layout.rows[cursor.min(1)].1;
    out.push(SpriteDraw {
        dst: (
            stage_origin.0 + layout.cursor_x * scale,
            stage_origin.1 + row_y * scale,
            w * stage_scale,
            h * stage_scale,
        ),
        src: rects.cursor,
        color: [1.0, 1.0, 1.0, 1.0],
    });
    out
}

/// The title and the two rows, in stage pixels (the host scales them with
/// [`scale_stage_text_draws`]).
pub fn tile_board_prompt_text_draws_for(
    font: &legaia_font::Font,
    layout: &TileBoardPromptLayout,
    lines: &[Vec<u8>; 3],
) -> Vec<TextDraw> {
    let pens = [
        (layout.frame.0, layout.frame.1),
        layout.rows[0],
        layout.rows[1],
    ];
    let mut out = Vec::new();
    for (line, pen) in lines.iter().zip(pens) {
        if !line.is_empty() {
            out.extend(text_draws_for(&font.layout(line), pen, MENU_TEXT_WHITE));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: TileBoardPromptLayout = TileBoardPromptLayout {
        frame: (100, 92, 120, 40),
        rows: [(152, 105), (152, 118)],
        cursor_x: 132,
    };

    #[test]
    fn the_three_lines_start_at_their_pens() {
        let f = legaia_font::Font::placeholder();
        let lines = [b"ab".to_vec(), b"c".to_vec(), b"d".to_vec()];
        let draws = tile_board_prompt_text_draws_for(&f, &LAYOUT, &lines);
        let ys: std::collections::BTreeSet<i32> = draws.iter().map(|d| d.dst.1).collect();
        assert!(ys.contains(&92) && ys.contains(&105) && ys.contains(&118));
        let empty = [Vec::new(), Vec::new(), Vec::new()];
        assert!(tile_board_prompt_text_draws_for(&f, &LAYOUT, &empty).is_empty());
    }
}
