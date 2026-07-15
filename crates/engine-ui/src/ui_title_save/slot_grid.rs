use crate::*;

/// Retail PSX framebuffer placement of the slot-preview 5×3 grid.
/// Mirror of `legaia_asset::title_pak::OVERLAY_LOAD_SLOT_GRID_*` -
/// retail-pinned via per-row/per-column blue-outline scan on
/// `slot_info_fb.png`: cell visible top-left corners at fb-y rows
/// 35 (row 0), 55 (row 1), 75 (row 2) and fb-x columns 104, 144,
/// 184, 224, 264 (col 0..4). Pitch X = 40, pitch Y = 20.
pub const SLOT_GRID_ORIGIN: (i32, i32) = (104, 35);
pub const SLOT_GRID_PITCH_X: i32 = 40;
pub const SLOT_GRID_PITCH_Y: i32 = 20;
pub const SLOT_GRID_COLS: usize = 5;
pub const SLOT_GRID_ROWS: usize = 3;

/// Per-cell view passed into [`slot_preview_grid_draws_for`]. Each
/// memory-card block becomes one cell; `present=false` cells render
/// as plain empty frames. When a save is present, `portrait_char_id`
/// (= lead party member's char_id) selects which 16×16 portrait
/// (0=Vahn, 1=Noa, 2=Gala) is drawn inside the frame; `None` falls
/// back to the empty frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotGridCell {
    pub present: bool,
    pub portrait_char_id: Option<u8>,
}

/// Build [`SpriteDraw`]s for the 5×3 slot-preview grid. Each cell
/// gets the empty-frame sprite (32×32 with 20×20 visible border).
/// Filled cells additionally get a 16×16 portrait centred in the
/// frame. The cursor sprite sits to the left of the currently
/// selected cell.
pub fn slot_preview_grid_draws_for(
    rects: &SaveMenuAtlasRects,
    cells: &[SlotGridCell],
    cursor_slot: u8,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut out = Vec::with_capacity(SLOT_GRID_COLS * SLOT_GRID_ROWS + 2);

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                (sw as u32) * scale as u32,
                (sh as u32) * scale as u32,
            ),
            src,
            color: white,
        });
    };

    let max_slots = (SLOT_GRID_COLS * SLOT_GRID_ROWS).min(cells.len());
    for (slot, cell) in cells.iter().take(max_slots).enumerate() {
        let col = slot % SLOT_GRID_COLS;
        let row = slot / SLOT_GRID_COLS;
        // Empty-frame sprite top-left in stage pixels. The 32×32
        // sprite has a 6px transparent margin; the visible 20×20
        // frame's top-left should land at (origin.x + col*pitch_x,
        // origin.y + row*pitch_y). So sprite origin = grid pos - 6.
        let cell_x = SLOT_GRID_ORIGIN.0 + (col as i32) * SLOT_GRID_PITCH_X;
        let cell_y = SLOT_GRID_ORIGIN.1 + (row as i32) * SLOT_GRID_PITCH_Y;
        if let Some(frame) = rects.load_empty_frame {
            // The full 32×32 sprite is drawn with its top-left at
            // (cell_x - 6, cell_y - 6) so the visible 20×20 border
            // sits at the cell position. Engines may instead sample
            // sub-rect (6, 6, 20, 20) and skip the margin - both
            // produce the same on-screen pixels.
            push(&mut out, frame, cell_x - 6, cell_y - 6, 32, 32);
        }
        if cell.present
            && let Some(char_id) = cell.portrait_char_id
            && let Some(portrait) = rects
                .load_portrait_by_char
                .get(char_id as usize)
                .copied()
                .flatten()
        {
            // Portrait centred inside the 20×20 visible frame
            // (16×16 portrait + 2px margin each side).
            push(&mut out, portrait, cell_x + 2, cell_y + 2, 16, 16);
        }
    }

    // Cursor sprite to the left of the currently-selected cell.
    // Retail pin: in `slot_info_fb.png` the pointing-finger cursor
    // bbox sits at fb-x 90..105 (16 wide) pointing at cell (0, 0) at
    // fb-x 104. That puts the cursor's right edge 1 px shy of the
    // cell's left edge - i.e. `cursor_x = cell_x - 14` (not -16).
    let cursor_col = (cursor_slot as usize) % SLOT_GRID_COLS;
    let cursor_row = (cursor_slot as usize) / SLOT_GRID_COLS;
    let cursor_x = SLOT_GRID_ORIGIN.0 + (cursor_col as i32) * SLOT_GRID_PITCH_X - 14;
    let cursor_y = SLOT_GRID_ORIGIN.1 + (cursor_row as i32) * SLOT_GRID_PITCH_Y;
    push(&mut out, rects.cursor, cursor_x, cursor_y, 16, 16);

    out
}
