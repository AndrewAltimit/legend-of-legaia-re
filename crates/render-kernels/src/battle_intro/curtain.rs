//! The curtain's no-clear accumulation: mid-pass compose, display trail and the capture blit.

use super::*;
use crate::battle_intro_transition::IntroQuad;
use crate::screen_prim::{ScreenPrim, ScreenQuad};
use crate::vram_capture::{
    FIELD_CAPTURE_PAGE, FIELD_CAPTURE_ROWS, FIELD_CAPTURE_V_OFFSET, PSX_SCREEN_HEIGHT,
    PSX_SCREEN_WIDTH, VramRect,
};
use legaia_tim::Vram;

// ---------------------------------------------------------------------------
// The curtain's no-clear accumulation (FUN_801D1D9C + the per-frame wash)
// ---------------------------------------------------------------------------

/// OT layer `FUN_801D11D0` hands the mid-pass emitter (`0x801D14A4`:
/// `FUN_801D1D9C(0x1EA, 2, 0x808080)`) - between the draw-area install at
/// `0x1F4` and the column strips at `0x1C2`, so the decay lands on the
/// intermediate **before** this frame's columns draw over it.
pub const CURTAIN_MIDPASS_OT: u32 = 0x1EA;
/// The emitter's second argument: ABR mode `2`, `B - F` - the quad subtracts.
pub const CURTAIN_MIDPASS_ABR: u8 = 2;
/// The emitter's third argument: `0x80` per channel, the amount subtracted
/// from the whole intermediate each frame.
pub const CURTAIN_MIDPASS_RGB: u32 = 0x0080_8080;
/// [`CURTAIN_MIDPASS_RGB`] as a 5-bit-channel step (`0x80 >> 3`): what one
/// frame of the mid-pass quad takes off a 15-bpp intermediate pixel.
pub const CURTAIN_MIDPASS_DECAY_5: u16 = 0x10;

/// The per-frame display wash the curtain arms
/// (`crate::battle_intro_styles::CURTAIN_WASH_RGB` = `0x80808`,
/// re-armed unconditionally at `0x801D1228..0x801D1230` on **every** frame),
/// as a 5-bit-channel step: 8 per 8-bit channel is one 5-bit level, so an
/// undrawn display pixel takes ~31 frames to reach black.
pub const CURTAIN_DISPLAY_DECAY_5: u16 = 1;

/// Where the port keeps its CPU model of the transition's display buffer in
/// the cloned VRAM page, so the trail can be drawn as textured quads. Retail
/// needs no such rect - on the console the display buffer *is* VRAM and the
/// no-clear accumulation is free. The rect is otherwise unused during a
/// transition: whatever scene data the clone holds there is invisible for the
/// whole window (the backdrop covers the live scene), and the clone is
/// dropped when the transition ends.
pub const CURTAIN_TRAIL_RECT: VramRect = VramRect::new(640, 256, 320, 240);

/// OT bucket the trail's backdrop quads link at: farther than every on-screen
/// strip (rows at `0x12C`), nearer than the opaque backdrop - the trail *is*
/// the frame the strips draw over.
pub const CURTAIN_TRAIL_OT: u32 = 0x300;

/// Subtract `step` from each 5-bit channel of every 15-bpp pixel, saturating
/// at zero and preserving the mask bit - one application of an ABR-2
/// (`B - F`) full-rect quad with `step` in every channel. A whole-word zero
/// (the overlay shader's hole) stays a hole.
fn decay_15bpp(buf: &mut [u16], step: u16) {
    for p in buf.iter_mut() {
        let v = *p;
        if v == 0 {
            continue;
        }
        *p = (v & 0x1F).saturating_sub(step)
            | ((v >> 5) & 0x1F).saturating_sub(step) << 5
            | ((v >> 10) & 0x1F).saturating_sub(step) << 10
            | (v & 0x8000);
    }
}

impl BattleIntro {
    /// Rasterise this frame's column-pass quads into [`FIELD_CAPTURE_ROWS`],
    /// the intermediate the curtain's row pass samples.
    ///
    /// The port has no render-to-VRAM target for screen-space quads, so the
    /// pass runs on the CPU. Each quad is one texel wide and `h` pixels tall
    /// with its `v` running the full source column, which makes the rasteriser
    /// a per-column vertical resample rather than a general triangle setup -
    /// exactly the shape `build_intro_quad` produced.
    ///
    /// The intermediate is **not cleared between frames**. Retail's mid-pass
    /// emitter `FUN_801D1D9C(0x1EA, 2, 0x808080)` pushes an ABR-2 quad over
    /// the whole rect between the draw-area install and the column strips, so
    /// the previous frame's content decays by [`CURTAIN_MIDPASS_DECAY_5`] per
    /// channel and this frame's columns draw over the ghost - a culled column
    /// fades out over two frames instead of vanishing (see the module docs).
    ///
    /// PORT: FUN_801D1D9C
    pub(super) fn compose_curtain_intermediate(&mut self) {
        let rect = FIELD_CAPTURE_ROWS;
        let (rw, rh) = (rect.w as usize, rect.h as usize);
        if self.capture_src.len() < rw * rh {
            return;
        }
        if self.intermediate.len() != rw * rh {
            // First frame: the rect starts opaque black (nothing has drawn the
            // intermediate yet; the identity-warp columns cover it entirely
            // this same frame).
            self.intermediate = vec![0x8000u16; rw * rh];
        } else {
            decay_15bpp(&mut self.intermediate, CURTAIN_MIDPASS_DECAY_5);
        }
        for q in &self.pending_columns {
            blit_intro_quad(
                q,
                &self.capture_src,
                FIELD_CAPTURE_PAGE,
                rect,
                &mut self.intermediate,
            );
        }
        if let Some(page) = self.captured.as_mut() {
            page.write_block(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                bytemuck::cast_slice(&self.intermediate),
            );
        }
    }

    /// Advance the CPU model of the transition's display buffer and land it in
    /// [`CURTAIN_TRAIL_RECT`].
    ///
    /// Order matters and is retail's: the wash decays what the display already
    /// holds, the row strips then draw over it. So the model is decayed and
    /// **uploaded first** - that snapshot is what the trail quads show under
    /// this frame's live GPU-drawn strips - and this frame's rows are
    /// rasterised into it afterwards, for the next frame's trail.
    ///
    /// Seeded from the field capture: retail's transition init lands the drawn
    /// field frame in *both* display buffers (`DrawSync` + `MoveImage`), so
    /// the first frame's background is the field itself, already one wash step
    /// down (the wash is re-armed on every frame including the first).
    pub(super) fn update_curtain_display_trail(&mut self) {
        let (w, h) = (PSX_SCREEN_WIDTH as usize, PSX_SCREEN_HEIGHT as usize);
        if self.capture_src.len() < (usize::from(FIELD_CAPTURE_V_OFFSET) + h) * w {
            return;
        }
        if self.display_accum.len() != w * h {
            // The display is in drawn rows; the page holds drawn row `d` at
            // `v = d + FIELD_CAPTURE_V_OFFSET`.
            let skip = usize::from(FIELD_CAPTURE_V_OFFSET) * w;
            self.display_accum = self.capture_src[skip..skip + w * h].to_vec();
        }
        decay_15bpp(&mut self.display_accum, CURTAIN_DISPLAY_DECAY_5);
        if let Some(page) = self.captured.as_mut() {
            let r = CURTAIN_TRAIL_RECT;
            page.write_block(
                r.x,
                r.y,
                r.w,
                r.h,
                bytemuck::cast_slice(&self.display_accum),
            );
        }
        let screen = VramRect::new(0, 0, PSX_SCREEN_WIDTH, PSX_SCREEN_HEIGHT);
        let rows = std::mem::take(&mut self.pending_rows);
        for q in &rows {
            blit_intro_quad(
                q,
                &self.intermediate,
                FIELD_CAPTURE_ROWS,
                screen,
                &mut self.display_accum,
            );
        }
        self.pending_rows = rows;
    }

    /// The five textured backdrop quads that put [`CURTAIN_TRAIL_RECT`] on
    /// screen behind the live row strips - the port's stand-in for the display
    /// buffer retail draws over without clearing. Five because a 15-bpp
    /// texture page is 64 texels wide, so the 320 columns tile five pages
    /// (the same scheme the particle capture pages use).
    pub(super) fn curtain_trail_prims(&self, prims: &mut Vec<ScreenPrim>) {
        if self.display_accum.is_empty() {
            return;
        }
        let r = CURTAIN_TRAIL_RECT;
        let base_page = (r.x / 64) | ((r.y / 256) << 4) | (2 << 7);
        for k in 0..(r.w / 64) {
            let x0 = (k * 64) as i16;
            prims.push(ScreenPrim::Textured(ScreenQuad {
                xy: [
                    (x0, 0),
                    (x0 + 64, 0),
                    (x0, r.h as i16),
                    (x0 + 64, r.h as i16),
                ],
                uv: [(0, 0), (64, 0), (0, r.h as u8), (64, r.h as u8)],
                clut: 0,
                tpage: base_page + k,
                color: 0x0080_8080,
                gouraud: None,
                semi_transparent: false,
                ot_index: CURTAIN_TRAIL_OT,
                depth: None,
            }));
        }
    }
}

/// Lift [`FIELD_CAPTURE_PAGE`] - the column rect and the rows below it a
/// texel coordinate can still name - out of `page` as a flat halfword image.
///
/// The curtain's column pass samples that page while writing
/// [`FIELD_CAPTURE_ROWS`] of the same page, which a borrow cannot express - and
/// the source never changes after the one-shot capture, so a copy is also the
/// cheaper answer.
pub(super) fn lift_capture_src(page: &Vram) -> Vec<u16> {
    let src = FIELD_CAPTURE_PAGE;
    (0..src.h as usize)
        .flat_map(|y| (0..src.w as usize).map(move |x| (src.x as usize + x, src.y as usize + y)))
        .map(|(x, y)| page.pixel(x, y))
        .collect()
}

/// PSX texture modulation on one 5-bit channel: `texel * colour / 128`,
/// saturated. `0x80` is the neutral colour, which is why the curtain's `0x7F`
/// (`0xFF` shaded by the pass' `0x80` intensity) reads as "carry the capture".
///
/// Rounded rather than truncated, and that is a deliberate divergence. The
/// hardware truncates, but the port's own row pass runs the same modulation in
/// the fragment shader in floating point and rounds only at the UNORM8 write -
/// so a truncating column pass would drop a whole 5-bit level per channel
/// before the row pass ever saw it, and the two halves of one effect would
/// disagree about a neutral modulation. Rounding makes `0x7F` the no-op it is
/// meant to be on both sides.
fn modulate5(c5: u16, colour: u8) -> u16 {
    ((u32::from(c5) * u32::from(colour) + 64) >> 7).min(31) as u16
}

/// Rasterise one axis-aligned curtain quad from one buffer into another.
///
/// `src` is `src_rect` as a flat halfword image and `out` is `dst` as one;
/// the quad's coordinates are in `dst`'s space (absolute VRAM for the column
/// pass, whose draw offset is zero; screen space for the row pass) and its
/// texture page must resolve inside `src_rect`. Both passes' quads are
/// `POLY_GT4`s with axis-aligned corners - a column is one texel wide with
/// `v` spanning the whole source column, a row one pixel tall with `u`
/// spanning its half of the source scanline - so the mapping is a per-line
/// resample rather than a general triangle setup.
fn blit_intro_quad(q: &IntroQuad, src: &[u16], src_rect: VramRect, dst: VramRect, out: &mut [u16]) {
    let (tx, ty, depth) = (
        ((q.tpage & 0x0F) as i32) * 64,
        if q.tpage & 0x10 != 0 { 256 } else { 0 },
        (q.tpage >> 7) & 0x3,
    );
    // Only a 15-bpp page inside the source rect can be resolved out of `src`.
    if depth != 2 || ty != i32::from(src_rect.y) {
        return;
    }
    let (sw, sh) = (src_rect.w as i32, src_rect.h as i32);
    let (x0, y0) = (i32::from(q.verts[0].x), i32::from(q.verts[0].y));
    let (x1, y1) = (i32::from(q.verts[3].x), i32::from(q.verts[3].y));
    let (w, h) = (x1 - x0, y1 - y0);
    if w <= 0 || h <= 0 {
        return;
    }
    let (u0, v0) = (i32::from(q.verts[0].u), i32::from(q.verts[0].v));
    let (u1, v1) = (i32::from(q.verts[3].u), i32::from(q.verts[3].v));
    let (rx, ry) = (i32::from(dst.x), i32::from(dst.y));
    let (rw, rh) = (i32::from(dst.w), i32::from(dst.h));
    for dy in y0.max(ry)..y1.min(ry + rh) {
        let v = v0 + (dy - y0) * (v1 - v0) / h;
        // The gradient is per-vertex; the two edges are equal on the disc
        // records, so this is a no-op there and correct if they ever differ.
        let t = (dy - y0) as u32 * 256 / h as u32;
        let shade: [u8; 3] = std::array::from_fn(|i| {
            let (a, b) = (u32::from(q.verts[0].rgb[i]), u32::from(q.verts[3].rgb[i]));
            ((a * (256 - t) + b * t) >> 8) as u8
        });
        for dx in x0.max(rx)..x1.min(rx + rw) {
            let u = u0 + (dx - x0) * (u1 - u0) / w;
            let (sx, sy) = (tx + u - i32::from(src_rect.x), v);
            if !(0..sw).contains(&sx) || !(0..sh).contains(&sy) {
                continue;
            }
            // The overlay shader's transparency rule, applied to the same
            // texels: a whole-word zero is a hole. The capture sets the mask
            // bit, so a black *captured* pixel is `0x8000` and still draws.
            let texel = src[(sy * sw + sx) as usize];
            if texel == 0 {
                continue;
            }
            let r = modulate5(texel & 0x1F, shade[0]);
            let g = modulate5((texel >> 5) & 0x1F, shade[1]);
            let b = modulate5((texel >> 10) & 0x1F, shade[2]);
            out[((dy - ry) * rw + (dx - rx)) as usize] = r | (g << 5) | (b << 10) | 0x8000;
        }
    }
}
