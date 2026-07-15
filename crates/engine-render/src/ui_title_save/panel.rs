use crate::*;

// -----------------------------------------------------------------------
// Generic 9-slice panel composition + "Now checking" dialog + slot-preview
// grid + slot-info panel rendering.
//
// All positions are stage pixels (32x240 boot-UI stage); `stage_scale`
// upscales to surface pixels. Pinned against the captured retail
// framebuffer in `captures/slot_info_dump/.../now_checking_fb.png`
// (sstate9 → CROSS → ~30 vsyncs) and `slot_info_fb.png` (~170 vsyncs).
// -----------------------------------------------------------------------

/// Compose a 9-slice panel at arbitrary `(dst_x, dst_y, dst_w, dst_h)`
/// stage pixels into `out`. Tiles the top/bottom 24-wide edges with a
/// remainder, and tiles the left/right 4×21 edges vertically with a
/// remainder. Used by both [`save_select_chrome_draws_for`] (which has
/// its own legacy code path that retains byte-exact behaviour) and
/// [`now_checking_panel_draws_for`].
///
/// Interior fill: a horizontal tiling of `rects.panel_interior` with
/// the per-tile width narrowed on the last (partial) column. The
/// retail engine emits the interior FIRST (3 gouraud-shaded quads
/// covering 32+32+17 of the 81-wide panel), then the border on top.
pub(crate) fn nine_slice_panel_into(
    out: &mut Vec<SpriteDraw>,
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32), // (x, y, w, h)
    stage_origin: (i32, i32),
    stage_scale: u32,
    // `true` = tile the raw filigree in 2D (the pause-menu windows, which
    // fill with navy damask). `false` = the save-screen behaviour: a single
    // gradient-baked interior tile stretched to the panel height (byte-pinned
    // against `now_checking_fb.png` / `slot_info_fb.png`; those panels are not
    // filigree-filled - the empty preview panel shows dark title-art bleed).
    tile_filigree: bool,
) {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let (px, py, pw, ph) = dst_stage;

    let push_c = |out: &mut Vec<SpriteDraw>,
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
    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32| { push_c(out, src, sx, sy, sw, sh, white) };

    if tile_filigree {
        // Pause-menu interior: the marbled navy filigree tiled in BOTH axes.
        // (The old path stretched one 29-tall tile to the full window height,
        // smearing the pattern into vertical streaks; 2D tiling keeps it
        // crisp.) `FILIGREE_TINT` darkens the raw tile to retail's dark navy
        // (retail modulates it with a gouraud gradient; a flat multiply is a
        // close, non-streaking approximation).
        // Tuned so the tiled raw filigree lands on retail's grayer navy
        // (~RGB 33,40,107) rather than an over-saturated blue: keep red,
        // pull green down a little, damp blue most (desaturate).
        const FILIGREE_TINT: [f32; 4] = [0.98, 0.84, 0.60, 1.0];
        let (fx, fy, fw, fh) = rects.panel_filigree;
        let (fw, fh) = (fw as i32, fh as i32);
        let mut y_int = py;
        while y_int < py + ph {
            let row_h = (py + ph - y_int).min(fh);
            let mut x_int = px;
            while x_int < px + pw {
                let col_w = (px + pw - x_int).min(fw);
                let src = (fx, fy, col_w as u32, row_h as u32);
                push_c(out, src, x_int, y_int, col_w, row_h, FILIGREE_TINT);
                x_int += col_w;
            }
            y_int += row_h;
        }
    } else {
        // Save-screen interior: a single gradient-baked tile, tiled
        // horizontally and stretched to the panel height (byte-pinned).
        let int_w = rects.panel_interior.2 as i32;
        let int_h = rects.panel_interior.3 as i32;
        let interior_h = ph.min(int_h.max(ph));
        let mut x_int = px;
        while x_int < px + pw {
            let remaining = px + pw - x_int;
            let this_w = remaining.min(int_w);
            let (sx, sy, _, sh) = rects.panel_interior;
            let actual_sh = sh.min(interior_h as u32);
            let src = (sx, sy, this_w as u32, actual_sh);
            push(out, src, x_int, py, this_w, interior_h);
            x_int += this_w;
        }
    }

    nine_slice_border_into(out, rects, (px, py, pw, ph), stage_origin, stage_scale);
}

/// Emit only the 9-slice **border** (4 corners + tiled edges) for a
/// stage rect - the shared frame pass under both the filigree-filled
/// pause-menu windows and the gradient-filled dialog boxes.
pub(crate) fn nine_slice_border_into(
    out: &mut Vec<SpriteDraw>,
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32), // (x, y, w, h)
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let (px, py, pw, ph) = dst_stage;

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

    let cw = rects.panel_tl.2 as i32;
    let ch = rects.panel_tl.3 as i32;
    let edge_w = rects.panel_top.2 as i32;
    let edge_h = rects.panel_top.3 as i32;
    let v_edge_h = rects.panel_left.3 as i32;

    // Four corners.
    push(out, rects.panel_tl, px, py, cw, ch);
    push(out, rects.panel_tr, px + pw - cw, py, cw, ch);
    push(out, rects.panel_bl, px, py + ph - ch, cw, ch);
    push(out, rects.panel_br, px + pw - cw, py + ph - ch, cw, ch);

    // Top + bottom edges with remainder.
    let edge_span = pw - 2 * cw;
    let full_tiles = edge_span / edge_w;
    let remainder = edge_span - full_tiles * edge_w;
    let edge_y_top = py;
    let edge_y_bot = py + ph - edge_h;
    let mut x = px + cw;
    for _ in 0..full_tiles {
        push(out, rects.panel_top, x, edge_y_top, edge_w, edge_h);
        push(out, rects.panel_bot, x, edge_y_bot, edge_w, edge_h);
        x += edge_w;
    }
    if remainder > 0 {
        let (ux, uy, _, uh) = rects.panel_top;
        let top_rem = (ux, uy, remainder as u32, uh);
        let (bx, by, _, bh) = rects.panel_bot;
        let bot_rem = (bx, by, remainder as u32, bh);
        push(out, top_rem, x, edge_y_top, remainder, edge_h);
        push(out, bot_rem, x, edge_y_bot, remainder, edge_h);
    }

    // Left + right edges. The source tile is 4x21; tile vertically
    // with a remainder for taller-than-21 interiors.
    let vert_span = ph - 2 * ch;
    let v_full = vert_span / v_edge_h;
    let v_rem = vert_span - v_full * v_edge_h;
    let mut y = py + ch;
    for _ in 0..v_full {
        push(out, rects.panel_left, px, y, cw, v_edge_h);
        push(out, rects.panel_right, px + pw - cw, y, cw, v_edge_h);
        y += v_edge_h;
    }
    if v_rem > 0 {
        let (lx, ly, lw, _) = rects.panel_left;
        let left_rem = (lx, ly, lw, v_rem as u32);
        let (rx, ry, rw, _) = rects.panel_right;
        let right_rem = (rx, ry, rw, v_rem as u32);
        push(out, left_rem, px, y, cw, v_rem);
        push(out, right_rem, px + pw - cw, y, cw, v_rem);
    }
}

/// Compose the retail 9-slice window chrome (interior fill + border) for
/// **any** menu window at an arbitrary `(x, y, w, h)` stage rect.
///
/// This is the reusable primitive shared by every faithful menu panel.
/// All of Legaia's pause-menu windows (field menu, item list, status,
/// equipment, spell list) are framed by the same bordered-window sprite:
/// the 9-slice tiles of the system-UI sprite sheet at `PROT.DAT[0x018E0]`
/// (CLUT row 2). They all pull chrome from the same [`SaveMenuAtlasRects`]
/// the save screen already builds; the tile composition (4 corners +
/// tiled edges + tiled interior) is identical and only the destination
/// rect changes per window.
///
/// `dst_stage` is `(x, y, w, h)` in stage pixels; `stage_origin` +
/// `stage_scale` place and upscale the stage into the surface exactly as
/// [`save_select_chrome_draws_for`] does. Returns the border/interior
/// [`SpriteDraw`]s only - text, cursors, and portraits are layered on top
/// by the caller.
pub fn menu_window_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(24);
    nine_slice_panel_into(&mut out, rects, dst_stage, stage_origin, stage_scale, true);
    out
}

/// Compose the retail **dialog window** chrome (NPC dialogue box,
/// dialogue option picker, item-received box) at an arbitrary
/// `(x, y, w, h)` stage rect.
///
/// PORT: FUN_8002C69C - the window/box emitter the dialog pager
/// (`FUN_801D84D0`) calls per frame with `(x, y, 0xF4, lines*0xF - 3)`
/// after staging the box skin via `FUN_80034B6C`. Two layers:
///
/// - **Interior**: retail fills the body with two stacked
///   semi-transparent (mode 0, `B/2 + F/2`) gouraud `POLY_G4` quads,
///   top vertices RGB `(0x18,0x18,0x28)`, bottom RGB `(0x40,0x40,0xA0)`;
///   the two passes compose to `0.25*back + 0.75*gradient`. The
///   engine draws one alpha-blended sprite stretching the pre-baked
///   alpha-191 gradient column (`rects.dialog_fill`) over the rect,
///   which reproduces the same arithmetic.
/// - **Border**: the same system-UI 9-slice sprite family as the menu
///   windows (4 corners + tiled edges from the skin's tile records),
///   drawn on top of the fill exactly like retail.
///
/// Returns [`SpriteDraw`]s only - text and hand cursors layer on top.
pub fn dialog_window_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let (px, py, pw, ph) = dst_stage;
    let mut out = Vec::with_capacity(24);
    // Translucent gradient fill first (retail's G4 pair spans the raw
    // box rect; the border sprites overlap its rim).
    out.push(SpriteDraw {
        dst: (
            stage_origin.0 + px * scale,
            stage_origin.1 + py * scale,
            (pw.max(0) as u32) * scale as u32,
            (ph.max(0) as u32) * scale as u32,
        ),
        src: rects.dialog_fill,
        color: [1.0, 1.0, 1.0, 1.0],
    });
    nine_slice_border_into(&mut out, rects, dst_stage, stage_origin, stage_scale);
    out
}

/// The dialog **option-picker hand cursor**: retail draws the
/// pointing-finger sprite (`FUN_8002B994` kind 0) at `(box_x - 6,
/// box_y + cursor*0xF)` on the selected option row of a `0x27..0x29`
/// picker box (`FUN_801D84D0` picker arm). Same 16x16 CLUT-row-7
/// sprite the save-select / options screens use.
pub fn dialog_option_hand_sprite(
    rects: &SaveMenuAtlasRects,
    box_pos: (i32, i32),
    cursor_row: usize,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let scale = stage_scale.max(1) as i32;
    let (_, _, w, h) = rects.cursor;
    SpriteDraw {
        dst: (
            stage_origin.0 + (box_pos.0 - 6) * scale,
            stage_origin.1 + (box_pos.1 + cursor_row as i32 * 0xF) * scale,
            w * stage_scale,
            h * stage_scale,
        ),
        src: rects.cursor,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

/// The dialog **page-advance hand**: retail draws a hand sprite
/// (`FUN_8002B994` kind 1) at `(0x10A, box_y + lines*0xF - 0x13)` -
/// the lower-right rim of the standard 244-wide box - while the pager
/// waits for confirm on a full page (state `0x19`). The engine keeps
/// the same "1 tile inside the right edge, near the bottom" anchor
/// relative to the caller's box rect so non-standard widths stay
/// attached. Tinted gold: the retail kind-1 sprite decodes with the
/// gold ramp CLUT rather than the silver cursor row.
pub fn dialog_advance_hand_sprite(
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let scale = stage_scale.max(1) as i32;
    let (px, py, pw, ph) = dst_stage;
    let (_, _, w, h) = rects.cursor;
    SpriteDraw {
        dst: (
            stage_origin.0 + (px + pw - 4) * scale,
            // Retail: y = box_y + lines*0xF - 0x13 with box h =
            // lines*0xF - 3, i.e. 0x10 above the box bottom.
            stage_origin.1 + (py + ph - 0x10) * scale,
            w * stage_scale,
            h * stage_scale,
        ),
        src: rects.cursor,
        color: [1.0, 0.82, 0.35, 1.0],
    }
}
