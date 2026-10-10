//! The panel widget: the eased colour quad.

use super::*;

// ---------------------------------------------------------------------------
// Kind 2 - image panel widget
// ---------------------------------------------------------------------------

/// One textured panel quad: GP0 `0x2C` (textured blended quad, colour
/// `0x888888`) over a **15bpp direct-colour** texpage (the spawn ORs `0x100`
/// into the page selector - depth bits = 15bpp, so no CLUT). OT slot `+0x10`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelQuad {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
    pub u0: u8,
    pub v0: u8,
    pub u1: u8,
    pub v1: u8,
    pub texpage: u16,
}

/// The image-panel widget (retail handler `FUN_801F849C`): a full-screen-art
/// panel (e.g. an event cut-in image) that can move and scale. Five channels
/// tween together: x, y, width, height, and the first-page width (a panel
/// wider than 256px splits across two 15bpp texpages).
#[derive(Debug, Clone, Copy)]
pub struct PanelWidget {
    /// Current `[x, y, w, h, w_page0]` - actor `+0x14/16/18/1a` and `+0x24`.
    pub cur: [i16; 5],
    /// Targets - actor `+0x3c/3e/40/42` and `+0x26`.
    pub target: [i16; 5],
    /// Unscaled base `[w, h, w_page0]` - actor `+0xb8/ba/bc` (the `FUN_801F8E6C`
    /// scale reference).
    pub base: [i16; 3],
    /// Tween clock / duration - actor `+0x9c` / `+0x9e`.
    pub t: i16,
    pub dur: i16,
    /// Spawn-time pixel size - actor `+0xaa/+0xac` (drives the UV extents).
    pub w0: i16,
    pub h0: i16,
    /// Texel origin - actor `+0xa4` (`u`) / `+0xa8` (`v`).
    pub u: u8,
    pub v: u8,
    /// First / second 15bpp texpage - actor `+0xa0` / `+0xa2` (0 = no second
    /// page; set when the panel is wider than 256px).
    pub texpage: u16,
    pub texpage2: u16,
}

impl PanelWidget {
    /// Spawn from the on-script record `[x:i16@1][y:i16@3][w:i16@5][h:i16@7]
    /// [tex_x:i16@9][tex_y:i16@0xb]` (offsets are relative to the field-VM
    /// operand byte, matching the retail reader which is handed `operand`
    /// and reads from `+1`).
    // PORT: FUN_801F88FC
    pub fn spawn(rec: &[u8]) -> Option<Self> {
        if rec.len() < 0xd {
            return None;
        }
        let x = rs16(rec, 1);
        let y = rs16(rec, 3);
        let w = rs16(rec, 5);
        let h = rs16(rec, 7);
        let tex_x = rs16(rec, 9);
        let tex_y = rs16(rec, 0xb) as u16;
        let u = ((tex_x as i32 & 0x3f) << 2) as u8;
        let page_hi = ((tex_y & 0xff80) >> 4) as i16;
        let texpage = ((tex_x >> 6) + page_hi) as u16 | 0x100;
        let v = (tex_y & 0x7f) as u8;
        let mut w_page0 = w;
        let mut texpage2 = 0u16;
        if w as u16 > 0x100 {
            texpage2 = (((tex_x as i32 + 0x100) >> 6) as i16 + page_hi) as u16 | 0x100;
            w_page0 = 0x100;
        }
        Some(PanelWidget {
            cur: [x, y, w, h, w_page0],
            target: [x, y, w, h, w_page0],
            base: [w, h, w_page0],
            t: 0,
            dur: 0,
            w0: w,
            h0: h,
            u,
            v,
            texpage,
            texpage2,
        })
    }

    /// Start a move/scale tween - the `FUN_801F8E6C` control API. `scale` is
    /// 4.12 fixed point (`0x1000` = 1.0): the width / height / first-page
    /// width targets become `(base * scale) >> 12`.
    // PORT: FUN_801F8E6C
    pub fn move_scale(&mut self, x: i16, y: i16, scale: i32, dur: i16) {
        self.target[0] = x;
        self.target[1] = y;
        self.target[2] = ((self.base[0] as i32 * scale) >> 12) as i16;
        self.target[3] = ((self.base[1] as i32 * scale) >> 12) as i16;
        self.target[4] = ((self.base[2] as i32 * scale) >> 12) as i16;
        self.t = 0;
        self.dur = dur;
    }

    /// Advance the 5-channel tween one frame and return the 1–2 textured
    /// quads - the whole of `FUN_801F849C`. Unlike the mask handler this one
    /// has no `dur == 0` snap arm: with no active tween the latched `cur`
    /// values draw as-is.
    // PORT: FUN_801F849C
    pub fn tick(&mut self, frame_delta: u8, screen: &Screen) -> (PanelQuad, Option<PanelQuad>) {
        let mut e = self.cur;
        if self.dur != 0 {
            self.t = self.t.wrapping_add(frame_delta as i16);
            if self.dur < self.t {
                self.t = self.dur;
            }
            for (i, ch) in e.iter_mut().enumerate() {
                if self.target[i] != self.cur[i] {
                    *ch = interp(
                        self.target[i] as i32,
                        self.cur[i] as i32,
                        self.t as i32,
                        self.dur as i32,
                        InterpMode::Linear,
                    ) as i16;
                }
            }
            if self.t == self.dur {
                self.dur = 0;
                self.cur = self.target;
                e = self.target;
            }
        }
        let [x, y, _w, h, w0] = e;
        let y = y - screen.y_off;
        // Retail truncates the spawn width to a byte for the UV extent and
        // zeroes it when the panel needs a second page.
        let cw: u8 = if (self.w0 as u16) < 0x101 {
            self.w0 as u8
        } else {
            0
        };
        let q0 = PanelQuad {
            left: x,
            top: y,
            right: x + w0 - 1,
            bottom: y + h - 1,
            u0: self.u,
            v0: self.v,
            u1: self.u.wrapping_add(cw).wrapping_sub(1),
            v1: self.v.wrapping_add(self.h0 as u8).wrapping_sub(1),
            texpage: self.texpage,
        };
        let q1 = (self.texpage2 != 0).then(|| PanelQuad {
            left: x + w0 - 2,
            top: y,
            right: x + e[2],
            bottom: y + h - 1,
            u0: self.u.wrapping_add(cw).wrapping_add(0xe),
            v0: self.v,
            u1: self.u.wrapping_add(self.w0 as u8).wrapping_add(0x10),
            v1: self.v.wrapping_add(self.h0 as u8).wrapping_sub(1),
            texpage: self.texpage2,
        });
        (q0, q1)
    }
}
