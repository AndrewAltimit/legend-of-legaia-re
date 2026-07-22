use crate::*;

/// Retail PSX framebuffer placement of the slot-preview 5×3 grid.
/// GP0-dump-pinned on the live SlotPreview screen: the 32×32 cell
/// quads land at row-0 x = 98, 138, 178, 218, 258 / y = 28, row 1 at
/// x = 102.. / y = 48, row 2 at x = 106.. / y = 68 - each row is
/// shifted **+4 px right** of the row above (the grid slants). With
/// the tile's 6 px transparent margin the visible 20×20 frames start
/// at `(104, 34)`. Pitch X = 40, pitch Y = 20.
pub const SLOT_GRID_ORIGIN: (i32, i32) = (104, 34);
pub const SLOT_GRID_PITCH_X: i32 = 40;
pub const SLOT_GRID_PITCH_Y: i32 = 20;
/// Per-row rightward shift: retail offsets each successive grid row
/// +4 px in x (row 0 at 98, row 1 at 102, row 2 at 106 in cell-quad
/// coords).
pub const SLOT_GRID_ROW_STAGGER_X: i32 = 4;
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

/// Retail park base of a cell quad's slide x: `0x15A` plus the cell
/// drawer's `+8` content inset (`ghidra/scripts/funcs/overlay_menu_801e06c0.txt`
/// at `0x801e0738`).
pub const SLOT_GRID_QUAD_PARK_X: i32 = 0x15A + 8;
/// Retail landed base of a cell quad's slide x: interp target `0x5A`
/// plus the same `+8` inset - row 0 col 0 quad at x = 98.
pub const SLOT_GRID_QUAD_LANDED_X: i32 = 0x5A + 8;
/// Per-slot-index extra park stagger (`sll s5, 6` into the slide base at
/// `0x801e0734`): parked cells fan out 64 px per block-slot on top of the
/// 40 px column pitch - the GP0-visible 104 px stagger between parked
/// row-mates.
pub const SLOT_GRID_PARK_SLOT_STAGGER_X: i32 = 64;

/// A cell quad's x for one frame of the commit slide - the retail grid
/// dispatcher's per-cell placement law.
///
/// Retail computes `base = 0x15A + slot * 64`, interpolates it toward
/// `0x5A` in 12-bit fixed point (`(0x5A - base) * t`, `+0xFFF` bias on a
/// negative product, `>> 12` - the same rounding as the slide-in
/// primitive `FUN_801E1C1C`), then adds the column pitch (`col * 40`)
/// and the row slant (`row * 4`); the cell drawer `FUN_801E0FD0` adds
/// its `+8` content inset. At `t = 0x1000` this lands on the pinned
/// grid (`98 + col*40 + row*4`); at `t = 0` the cells park off-screen
/// right from x = 354 with the 104 px per-slot fan-out.
///
/// PORT: FUN_801e06c0 (per-cell x of the grid loop, `0x801e0734..0x801e0784`)
/// REF: FUN_801e0fd0 (the per-cell drawer whose `+8` inset is folded in)
pub fn slot_grid_quad_x(col: u32, row: u32, slot: u32, t: u16) -> i32 {
    let base = 0x15A + (slot as i32) * SLOT_GRID_PARK_SLOT_STAGGER_X;
    let mut delta = (0x5A - base) * (t.min(0x1000) as i32);
    if delta < 0 {
        delta += 0xFFF;
    }
    let slid = base + (delta >> 12);
    slid + (col as i32) * SLOT_GRID_PITCH_X + (row as i32) * SLOT_GRID_ROW_STAGGER_X + 8
}

/// Build [`SpriteDraw`]s for the 5×3 slot-preview grid. Each cell
/// gets the empty-frame sprite (32×32 with 20×20 visible border).
/// Filled cells additionally get a 16×16 portrait centred in the
/// frame. The cursor sprite sits to the left of the currently
/// selected cell.
///
/// PORT: FUN_801e06c0 (the landed grid: quad x = `98 + col*40 + row*4`,
/// y = `28 + row*20`; focused cell full `0x80` modulation, every other
/// cell dimmed - the `mode 0 / mode 3` split its per-cell calls pass;
/// hand cursor at quad `(-10, +4)` = the `FUN_8002C488(x-2, y-4, 0x4E)`
/// call against the pre-inset pen). The commit slide's per-frame x is
/// [`slot_grid_quad_x`].
/// PORT: FUN_801e0fd0 (the per-cell drawer the loop calls: the 32x32
/// cell quad at the `+8 / -8` content inset, the portrait inside the
/// frame, and the full-vs-dim modulation by the caller's mode - this
/// builder emits the same per-cell draws).
pub fn slot_preview_grid_draws_for(
    rects: &SaveMenuAtlasRects,
    cells: &[SlotGridCell],
    cursor_slot: u8,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    // Retail dims every non-focused cell (and its portrait) to 0x60
    // modulation = 75% of the neutral 0x80; only the focused cell
    // draws at full brightness (GP0 dump of the live SlotPreview).
    let dim = [0.75, 0.75, 0.75, 1.0];
    let mut out = Vec::with_capacity(SLOT_GRID_COLS * SLOT_GRID_ROWS + 2);

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32,
                color: [f32; 4]| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                (sw as u32) * scale as u32,
                (sh as u32) * scale as u32,
            ),
            src,
            color,
        });
    };

    let max_slots = (SLOT_GRID_COLS * SLOT_GRID_ROWS).min(cells.len());
    for (slot, cell) in cells.iter().take(max_slots).enumerate() {
        let col = slot % SLOT_GRID_COLS;
        let row = slot / SLOT_GRID_COLS;
        // Empty-frame sprite top-left in stage pixels. The 32×32
        // sprite has a 6px transparent margin; the visible 20×20
        // frame's top-left should land at (origin.x + col*pitch_x +
        // row*stagger, origin.y + row*pitch_y) - retail slants each
        // row +4 px right of the one above. Sprite origin = grid pos
        // - 6.
        let cell_x = SLOT_GRID_ORIGIN.0
            + (col as i32) * SLOT_GRID_PITCH_X
            + (row as i32) * SLOT_GRID_ROW_STAGGER_X;
        let cell_y = SLOT_GRID_ORIGIN.1 + (row as i32) * SLOT_GRID_PITCH_Y;
        let color = if slot == cursor_slot as usize {
            white
        } else {
            dim
        };
        if let Some(frame) = rects.load_empty_frame {
            // The full 32×32 sprite is drawn with its top-left at
            // (cell_x - 6, cell_y - 6) so the visible 20×20 border
            // sits at the cell position. Engines may instead sample
            // sub-rect (6, 6, 20, 20) and skip the margin - both
            // produce the same on-screen pixels.
            push(&mut out, frame, cell_x - 6, cell_y - 6, 32, 32, color);
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
            push(&mut out, portrait, cell_x + 2, cell_y + 2, 16, 16, color);
        }
    }

    // Cursor sprite to the left of the currently-selected cell.
    // GP0 pin: on the live SlotPreview the pointing-finger sprite sits
    // at (88, 32) against the focused cell quad at (98, 28) - i.e.
    // 10 px left of the cell quad and 4 px below its top, which in
    // visible-cell coords is `(cell_x - 16, cell_y - 2)`.
    let cursor_col = (cursor_slot as usize) % SLOT_GRID_COLS;
    let cursor_row = (cursor_slot as usize) / SLOT_GRID_COLS;
    let cursor_x = SLOT_GRID_ORIGIN.0
        + (cursor_col as i32) * SLOT_GRID_PITCH_X
        + (cursor_row as i32) * SLOT_GRID_ROW_STAGGER_X
        - 16;
    let cursor_y = SLOT_GRID_ORIGIN.1 + (cursor_row as i32) * SLOT_GRID_PITCH_Y - 2;
    push(&mut out, rects.cursor, cursor_x, cursor_y, 16, 16, white);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_grid_quad_x_lands_on_the_gp0_pinned_grid() {
        // t = 0x1000: quad x = 98 + col*40 + row*4 (the landed pin).
        assert_eq!(slot_grid_quad_x(0, 0, 0, 0x1000), 98);
        assert_eq!(slot_grid_quad_x(1, 0, 1, 0x1000), 138);
        assert_eq!(slot_grid_quad_x(4, 2, 14, 0x1000), 98 + 4 * 40 + 2 * 4);
    }

    #[test]
    fn slot_grid_quad_x_parks_offscreen_with_the_104px_fanout() {
        // t = 0: slot 0 parks at 354; row-mates fan out 64 (slot) + 40
        // (column) = 104 px apart.
        assert_eq!(slot_grid_quad_x(0, 0, 0, 0), 354);
        assert_eq!(slot_grid_quad_x(1, 0, 1, 0), 354 + 104);
        assert_eq!(slot_grid_quad_x(4, 0, 4, 0), 354 + 4 * 104);
    }
}
