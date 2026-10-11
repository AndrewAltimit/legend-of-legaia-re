//! The mask widget: the full-screen subtractive quad.

use super::*;

// ---------------------------------------------------------------------------
// Kind 1 - screen mask (iris) widget
// ---------------------------------------------------------------------------

/// One axis-aligned black border quad of the mask draw. Vertices are packed
/// exactly as the retail GP0 `0x28` (flat opaque quad, colour `0x000000`)
/// packet: `(left,top) (right,top) (left,bottom) (right,bottom)`, linked at
/// OT slot `+0x1c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskQuad {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

/// The screen-mask widget (retail handler `FUN_801F811C`): a rectangle
/// `[L..R] x [T..B]` stays visible while four black quads cover everything
/// outside it. The four edges tween from their value at [`Self::set_rect`]
/// time toward the requested targets.
#[derive(Debug, Clone, Copy)]
pub struct MaskWidget {
    /// Current (latched) edges `[L, T, R, B]` - actor `+0x14/16/18/1a`. These
    /// only move on snap/latch; mid-tween display values are re-interpolated
    /// from here each frame.
    pub cur: [i16; 4],
    /// Target edges - actor `+0x3c/3e/40/42`.
    pub target: [i16; 4],
    /// Tween clock - actor `+0x9c`.
    pub t: i16,
    /// Tween duration - actor `+0x9e` (0 = no active tween, snap to target).
    pub dur: i16,
}

impl MaskWidget {
    /// Fresh-spawn state: fully open (the border quads are degenerate). Port
    /// of the spawn arm of `FUN_801F8D4C`: `cur = [x0, 0, 0x140, height-1]`.
    // PORT: FUN_801F8D4C
    pub fn spawn(screen: &Screen) -> Self {
        let open = [screen.x0, 0, SCREEN_W, screen.height - 1];
        MaskWidget {
            cur: open,
            target: open,
            t: 0,
            dur: 0,
        }
    }

    /// Start a tween of the visible rect toward `(l, t, r, b)` over `dur`
    /// frames - the `FUN_801F8D4C` control API. An edge passed as `-1` takes
    /// its full-open default (`x0` / `0` / `0x140` / `height-1`), so e.g.
    /// `set_rect(-1, -1, -1, -1, d)` opens the mask and `set_rect(160, 120,
    /// 160, 120, d)` irises shut on the screen centre.
    pub fn set_rect(&mut self, l: i16, t: i16, r: i16, b: i16, dur: i16, screen: &Screen) {
        let l = if l == -1 { screen.x0 } else { l };
        let t = if t == -1 { 0 } else { t };
        let r = if r == -1 { SCREEN_W } else { r };
        let b = if b == -1 { screen.height - 1 } else { b };
        self.target = [l, t, r, b];
        self.t = 0;
        self.dur = dur;
    }

    /// Advance the tween one frame and return the four border quads - the
    /// whole of `FUN_801F811C`. `frame_delta` is retail's per-frame byte
    /// `DAT_1F800393`.
    ///
    /// Faithful shape: when `dur == 0` the current edges snap to the targets;
    /// otherwise the clock advances (clamped to `dur`), each *display* edge is
    /// re-interpolated `interp(target, cur, t, dur, Linear)` from the latched
    /// `cur` (which does **not** move mid-tween), and on `t == dur` the edges
    /// latch to the targets and `dur` clears.
    // PORT: FUN_801F811C
    pub fn tick(&mut self, frame_delta: u8, screen: &Screen) -> [MaskQuad; 4] {
        // Locals seed from the targets; axes whose target == cur keep that.
        let mut e = self.target;
        if self.dur == 0 {
            self.cur = self.target;
        } else {
            self.t = self.t.wrapping_add(frame_delta as i16);
            if self.dur < self.t {
                self.t = self.dur;
            }
            for (i, edge) in e.iter_mut().enumerate() {
                if self.target[i] != self.cur[i] {
                    *edge = interp(
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
        let [l, t, r, b] = e;
        [
            // Top band: rows above the rect.
            MaskQuad {
                left: screen.x0,
                top: 0,
                right: SCREEN_W,
                bottom: t,
            },
            // Bottom band: rows below the rect.
            MaskQuad {
                left: screen.x0,
                top: b,
                right: SCREEN_W,
                bottom: screen.height - 1,
            },
            // Left band (note the retail `L - 1` right edge).
            MaskQuad {
                left: screen.x0,
                top: t,
                right: l - 1,
                bottom: b,
            },
            // Right band.
            MaskQuad {
                left: r,
                top: t,
                right: SCREEN_W,
                bottom: b,
            },
        ]
    }
}
