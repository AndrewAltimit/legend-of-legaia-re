//! The letterbox: the two bars and their slide.

use super::*;

// ---------------------------------------------------------------------------
// Kind 3 - letterbox-band widget
// ---------------------------------------------------------------------------

/// The letterbox widget (retail handler `FUN_801F8A34`): two solid black
/// bands with gradient "feather" strips on their inner edges. No tween - the
/// `FUN_801F8F28` config writes the band edges directly.
///
/// Draw shape (all between `x_left..x_right`, OT slot `+0x4`):
/// * solid black `(x_left, -y_off) .. (x_right, y0)` and `(x_left, y3) ..
///   (x_right, height)` (GP0 `0x28`);
/// * gradient `y0` (white) → `y1` (black) and `y2` (black) → `y3` (white)
///   (GP0 `0x3B` shaded semi-transparent quads, preceded by a draw-mode
///   packet selecting **subtractive** blending - `FUN_80059010(.., 0x55, ..)`,
///   texpage blend bits mode 2), so the white edge subtracts to black.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Letterbox {
    /// Horizontal extent - actor `+0xa0` / `+0xa2`.
    pub x_left: i16,
    pub x_right: i16,
    /// Band edges top..bottom - actor `+0xa4/a6/a8/aa`.
    pub y0: i16,
    pub y1: i16,
    pub y2: i16,
    pub y3: i16,
}

impl Letterbox {
    /// Decode the 6-`i16` config block the field VM hands `FUN_801F8F28`:
    /// `[x_left][x_right][y0][y1][y2][y3]` (the spawn writes `y3` into both
    /// `+0xa8`'s pair slots).
    // PORT: FUN_801F8F28
    pub fn from_config(block: &[u8]) -> Option<Self> {
        if block.len() < 12 {
            return None;
        }
        Some(Letterbox {
            x_left: rs16(block, 0),
            x_right: rs16(block, 2),
            y0: rs16(block, 4),
            y1: rs16(block, 6),
            y2: rs16(block, 8),
            y3: rs16(block, 10),
        })
    }

    /// The two solid bands: `(left, top, right, bottom)` rects. Port of the
    /// `FUN_801F8A34` flat-quad pair (top band starts at `-y_off`; bottom band
    /// ends at `height`, **not** `height - 1` - retail's literal).
    // PORT: FUN_801F8A34
    pub fn solid_bands(&self, screen: &Screen) -> [MaskQuad; 2] {
        [
            MaskQuad {
                left: self.x_left,
                top: -screen.y_off,
                right: self.x_right,
                bottom: self.y0,
            },
            MaskQuad {
                left: self.x_left,
                top: self.y3,
                right: self.x_right,
                bottom: screen.height,
            },
        ]
    }

    /// The two gradient feather strips as `(rect, top_is_white)`: the first
    /// fades white at `y0` → black at `y1`, the second black at `y2` → white
    /// at `y3`. Drawn subtractively, so the white edge darkens fully.
    pub fn gradient_bands(&self) -> [(MaskQuad, bool); 2] {
        [
            (
                MaskQuad {
                    left: self.x_left,
                    top: self.y0,
                    right: self.x_right,
                    bottom: self.y1,
                },
                true,
            ),
            (
                MaskQuad {
                    left: self.x_left,
                    top: self.y2,
                    right: self.x_right,
                    bottom: self.y3,
                },
                false,
            ),
        ]
    }
}
