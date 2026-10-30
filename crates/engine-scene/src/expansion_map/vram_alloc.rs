//! Free-VRAM allocation for the textures a pack map brings in: another
//! scene's texture rectangles and palettes, and a model asset's quantised
//! texture.
//!
//! The map's VRAM is its base scene's. [`VramAlloc`] starts from what that
//! build leaves written (every TIM the scene uploads, the boot-resident
//! system UI, the effect-texture pool and the field character atlas the hosts
//! layer under or over it) plus [`RESERVED`], the rectangles the runtime
//! writes after the build, and hands out rectangles nothing else claims. A
//! texture rectangle stays inside one texture page, which is what a
//! primitive's UVs can address; a palette starts on a 16-halfword boundary,
//! which is what a CLUT word can address.

use legaia_tim::Vram;
use legaia_tim::vram::{VRAM_HEIGHT, VRAM_WIDTH};

/// VRAM the runtime writes after the scene build, so never handed out:
/// `(x, y, w, h)` in halfwords.
///
/// - `(320, 0)-(640, 512)`: the field capture pages the battle transition
///   renders into and samples (`render-kernels::vram_capture`,
///   `FIELD_CAPTURE_ROWS` / `FIELD_CAPTURE_COLS`), and the effect pool's
///   pages beside them.
/// - `(896, 0)-(960, 256)`: the dialog font's glyph page
///   (`docs/formats/dialog-font.md`).
/// - `(0, 256)-(208, 472)`: the Grid Duel's card page, written while a duel
///   is seated (`legaia_engine_modes::cards::table_art::PAGE_RECT`).
///
/// Retail's display buffers (`x < 320`) are otherwise free here: the port
/// presents from its own render target and never draws a frame into VRAM.
pub const RESERVED: &[(u16, u16, u16, u16)] =
    &[(320, 0, 320, 512), (896, 0, 64, 256), (0, 256, 208, 216)];

/// A rectangle of halfwords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// A halfword-granular occupancy map with a summed-area table for O(1)
/// "is this rectangle free" queries.
pub struct VramAlloc {
    used: Vec<bool>,
    sat: Vec<u32>,
    dirty: bool,
}

impl VramAlloc {
    /// Everything `vram` has written, plus [`RESERVED`].
    pub fn from_vram(vram: &Vram) -> Self {
        let mut used = vec![false; VRAM_WIDTH * VRAM_HEIGHT];
        for y in 0..VRAM_HEIGHT {
            for x in 0..VRAM_WIDTH {
                used[y * VRAM_WIDTH + x] = vram.is_written(x, y);
            }
        }
        let mut a = Self {
            used,
            sat: Vec::new(),
            dirty: true,
        };
        for &(x, y, w, h) in RESERVED {
            a.mark(Rect { x, y, w, h });
        }
        a
    }

    /// An empty map with only [`RESERVED`] taken (tests).
    pub fn empty() -> Self {
        Self::from_vram(&Vram::new())
    }

    /// Claim `r`.
    pub fn mark(&mut self, r: Rect) {
        for y in r.y..(r.y + r.h).min(VRAM_HEIGHT as u16) {
            for x in r.x..(r.x + r.w).min(VRAM_WIDTH as u16) {
                self.used[usize::from(y) * VRAM_WIDTH + usize::from(x)] = true;
            }
        }
        self.dirty = true;
    }

    fn rebuild(&mut self) {
        let w = VRAM_WIDTH + 1;
        self.sat = vec![0; w * (VRAM_HEIGHT + 1)];
        for y in 0..VRAM_HEIGHT {
            let mut row = 0u32;
            for x in 0..VRAM_WIDTH {
                row += u32::from(self.used[y * VRAM_WIDTH + x]);
                self.sat[(y + 1) * w + x + 1] = self.sat[y * w + x + 1] + row;
            }
        }
        self.dirty = false;
    }

    /// Whether every halfword of `r` is free.
    pub fn is_free(&mut self, r: Rect) -> bool {
        let (x0, y0) = (usize::from(r.x), usize::from(r.y));
        let (x1, y1) = (x0 + usize::from(r.w), y0 + usize::from(r.h));
        if x1 > VRAM_WIDTH || y1 > VRAM_HEIGHT {
            return false;
        }
        if self.dirty {
            self.rebuild();
        }
        let w = VRAM_WIDTH + 1;
        let s = |x: usize, y: usize| self.sat[y * w + x];
        s(x1, y1) + s(x0, y0) == s(x0, y1) + s(x1, y0)
    }

    /// A free `w x h` texture rectangle for a page of `depth` bits per texel
    /// (4, 8 or 16), inside one texture page, at a halfword column that is a
    /// multiple of `align`. Claims it. Returns the rectangle and its page's
    /// origin.
    pub fn alloc_texture(
        &mut self,
        w: u16,
        h: u16,
        depth: u8,
        align: u16,
    ) -> Option<(Rect, (u16, u16))> {
        let span: u16 = match depth {
            4 => 64,
            8 => 128,
            _ => 256,
        };
        if w == 0 || h == 0 || w > span || h > 256 {
            return None;
        }
        let align = align.max(1);
        // Highest pages first: the scene's own TIMs crowd the low pages.
        for py in [256u16, 0] {
            for pxi in (0..16u16).rev() {
                let px = pxi * 64;
                if px + span > VRAM_WIDTH as u16 {
                    continue;
                }
                for oy in (0..=256 - h).step_by(1) {
                    let mut ox = 0;
                    while ox + w <= span {
                        let r = Rect {
                            x: px + ox,
                            y: py + oy,
                            w,
                            h,
                        };
                        if self.is_free(r) {
                            self.mark(r);
                            return Some((r, (px, py)));
                        }
                        ox += align;
                    }
                }
            }
        }
        None
    }

    /// A free palette of `entries` halfwords (16 or 256) on a 16-halfword
    /// boundary. Claims it.
    pub fn alloc_clut(&mut self, entries: u16) -> Option<Rect> {
        for y in (0..VRAM_HEIGHT as u16).rev() {
            let mut x = 0u16;
            while x + entries <= VRAM_WIDTH as u16 {
                let r = Rect {
                    x,
                    y,
                    w: entries,
                    h: 1,
                };
                if self.is_free(r) {
                    self.mark(r);
                    return Some(r);
                }
                x += 16;
            }
        }
        None
    }
}

/// A CLUT word for the palette at `(x, y)`: `y << 6 | x / 16`.
pub fn cba_word(x: u16, y: u16) -> u16 {
    (y << 6) | (x / 16)
}

/// The `(x, y)` a CLUT word names.
pub fn cba_xy(cba: u16) -> (u16, u16) {
    ((cba & 0x3F) * 16, (cba >> 6) & 0x1FF)
}

/// A texture page's `(x, y, depth bits)` from a TSB word.
pub fn tsb_page(tsb: u16) -> (u16, u16, u8) {
    let depth = match (tsb >> 7) & 3 {
        0 => 4,
        1 => 8,
        _ => 16,
    };
    ((tsb & 0xF) * 64, ((tsb >> 4) & 1) * 256, depth)
}

/// `tsb` moved to the page at `(px, py)`, its depth and blend bits kept.
pub fn tsb_with_page(tsb: u16, px: u16, py: u16) -> u16 {
    (tsb & !0x1F) | (px / 64) | ((py / 256) << 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_and_written_vram_is_never_handed_out() {
        let mut vram = Vram::new();
        // A scene TIM filling the top-right page.
        vram.write_block(960, 0, 64, 256, &vec![1u8; 64 * 256 * 2]);
        let mut a = VramAlloc::from_vram(&vram);
        let (r, (px, py)) = a.alloc_texture(16, 32, 4, 1).unwrap();
        assert!(!(320..640).contains(&r.x), "outside the capture pages");
        assert!(!(r.x >= 960 && r.y < 256), "not over the written page");
        assert!(r.x >= px && r.x + r.w <= px + 64 && r.y >= py && r.y + r.h <= py + 256);
        // A second rectangle does not overlap the first.
        let (r2, _) = a.alloc_texture(16, 32, 4, 1).unwrap();
        let overlap =
            r.x < r2.x + r2.w && r2.x < r.x + r.w && r.y < r2.y + r2.h && r2.y < r.y + r.h;
        assert!(!overlap);
    }

    #[test]
    fn palettes_land_on_a_clut_boundary() {
        let mut a = VramAlloc::empty();
        let c = a.alloc_clut(16).unwrap();
        assert_eq!(c.x % 16, 0);
        let (x, y) = cba_xy(cba_word(c.x, c.y));
        assert_eq!((x, y), (c.x, c.y));
    }

    #[test]
    fn page_words_round_trip() {
        let tsb = 0x009C; // 8bpp, page (768, 256), blend bits clear
        assert_eq!(tsb_page(tsb), (768, 256, 8));
        let moved = tsb_with_page(tsb | 0x20, 640, 0);
        assert_eq!(tsb_page(moved), (640, 0, 8));
        assert_eq!(moved & 0x60, 0x20, "blend mode kept");
    }
}
