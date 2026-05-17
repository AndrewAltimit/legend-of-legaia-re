//! Menu-glyph sprite atlas — RGBA decode + per-character draw helpers.
//!
//! Pairs with [`legaia_asset::menu_glyph_atlas`] (TIM source pin) and
//! [`crate::title_screen_atlas`] (title-art bands). Decodes the 256×256
//! 4bpp atlas at `PROT.DAT` offset `0x11218` into a tint-friendly
//! "stencil" RGBA8 buffer the renderer uploads as a sprite atlas:
//!
//! - **Pixel index 0** (transparent in every CLUT row) → `RGBA(0,0,0,0)`.
//! - **Pixel index 1..15** (the glyph fill / highlight indices) → opaque
//!   white, with alpha derived from the source CLUT's per-index value.
//!
//! Why white instead of the source-CLUT colour? The on-disc CLUT row 0
//! renders the alphabet glyphs in solid red (with magenta highlights).
//! Retail switches CLUT rows per-context so the same glyphs read white
//! / gold / dim. Rather than mirror the CLUT-switching logic, we decode
//! once to a stencil and let the per-row `SpriteDraw::color` tint do
//! the colour change. That keeps the engine layer trivially correct
//! across all three retail row colours (white, gold cursor, dim
//! "disabled" Continue) without re-decoding the atlas.
//!
//! Build once at boot via [`build_atlas_from_prot_dat`]; engines pin
//! the resulting [`MenuGlyphAtlas`] in `play-window`-style state and
//! emit one [`legaia_engine_render::SpriteRequest`] per character per
//! frame the title menu is visible.

use anyhow::{Context, Result};
use legaia_asset::menu_glyph_atlas as src;

/// Atlas width in pixels.
pub const ATLAS_WIDTH: u32 = src::ATLAS_WIDTH;
/// Atlas height in pixels.
pub const ATLAS_HEIGHT: u32 = src::ATLAS_HEIGHT;
/// Per-character cell width (alphabet + digits share this pitch).
pub const GLYPH_W: u32 = src::GLYPH_W;
/// Alphabet-row glyph height.
pub const ALPHABET_GLYPH_H: u32 = src::ALPHABET_GLYPH_H;

/// Pre-decoded menu-glyph atlas — RGBA8 stencil pixels + the source
/// rects engines sample to render text.
///
/// See module docs for the stencil rationale. Engines upload `rgba`
/// directly via `engine-render::upload_sprite_atlas` and look up per-
/// character source rects via [`Self::glyph_rect`].
#[derive(Debug, Clone)]
pub struct MenuGlyphAtlas {
    /// RGBA8 stencil pixels, exactly `4 * ATLAS_WIDTH * ATLAS_HEIGHT` bytes.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl MenuGlyphAtlas {
    /// Source rect (atlas pixels, `(x, y, w, h)`) for a single character.
    ///
    /// Returns `None` for characters not present in the atlas (space,
    /// punctuation outside `A..Z` / `0..9`). Case-insensitive — the
    /// atlas only carries uppercase letterforms.
    pub fn glyph_rect(&self, c: char) -> Option<(u32, u32, u32, u32)> {
        src::glyph_rect(c)
    }

    /// Width (atlas pixels) of a single character cell. Useful for
    /// reserving horizontal space for un-drawn characters (space).
    pub fn cell_w(&self) -> u32 {
        GLYPH_W
    }

    /// Per-character glyph height for alphabet-row text (the menu rows
    /// "NEW GAME" / "CONTINUE" / "OPTIONS").
    pub fn cell_h(&self) -> u32 {
        ALPHABET_GLYPH_H
    }

    /// Pre-measure a string's rendered width in atlas pixels using the
    /// shared alphabet/digits cell pitch. Includes blanks for chars
    /// without a glyph rect (so spaces are spaced like letters).
    pub fn measure_width(&self, s: &str) -> u32 {
        s.chars().count() as u32 * GLYPH_W
    }
}

/// Build a [`MenuGlyphAtlas`] from raw `PROT.DAT` bytes.
///
/// Validates the TIM header at [`legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET`],
/// decodes the 4bpp pixel block against CLUT row 0 to identify
/// transparent vs. fill pixels, and produces an RGBA8 stencil
/// (opaque white over transparent background). See the module-level
/// docs for the stencil-vs-CLUT rationale.
pub fn build_atlas_from_prot_dat(prot_dat: &[u8]) -> Result<MenuGlyphAtlas> {
    let view = src::extract_from_prot_dat(prot_dat).context("extract menu-glyph TIM")?;
    build_from_view(&view)
}

/// Build a [`MenuGlyphAtlas`] from a buffer that already contains the
/// menu-glyph TIM bytes starting at offset 0. Use when the caller
/// fetched only the TIM slice from PROT.DAT (see
/// [`legaia_engine_core::scene::ProtIndex::prot_dat_raw_bytes`] +
/// [`legaia_asset::menu_glyph_atlas::TIM_SIZE`]) rather than the full
/// PROT.DAT image.
pub fn build_atlas_from_prot_dat_slice(tim_bytes: &[u8]) -> Result<MenuGlyphAtlas> {
    let view = src::extract_from_tim_slice(tim_bytes).context("extract menu-glyph TIM slice")?;
    build_from_view(&view)
}

fn build_from_view(view: &src::MenuGlyphTim<'_>) -> Result<MenuGlyphAtlas> {
    let parsed = legaia_tim::parse(view.bytes).context("parse menu-glyph TIM")?;
    let width = parsed.pixel_width() as u32;
    let height = parsed.image.h as u32;
    if width != ATLAS_WIDTH || height != ATLAS_HEIGHT {
        anyhow::bail!(
            "menu-glyph atlas size mismatch: got {}×{}, expected {}×{}",
            width,
            height,
            ATLAS_WIDTH,
            ATLAS_HEIGHT
        );
    }

    // Walk the 4bpp pixel block directly and emit a stencil: index 0
    // -> (0,0,0,0); index != 0 -> (255,255,255,255). We deliberately
    // ignore the CLUT — see module docs.
    let fb_w = parsed.image.fb_w as usize; // halfwords per row
    let row_bytes = fb_w * 2; // = width / 2 for 4bpp
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let byte = parsed.image.data[y * row_bytes + x / 2];
            let idx = if x & 1 == 0 {
                byte & 0x0F
            } else {
                (byte >> 4) & 0x0F
            };
            let dst = (y * width as usize + x) * 4;
            if idx == 0 {
                // transparent in every CLUT row
                rgba[dst..dst + 4].copy_from_slice(&[0, 0, 0, 0]);
            } else {
                rgba[dst..dst + 4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
            }
        }
    }

    Ok(MenuGlyphAtlas {
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_rect_delegates_to_asset_layer() {
        // Build an empty stub atlas so `glyph_rect` is callable.
        let atlas = MenuGlyphAtlas {
            rgba: vec![0; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize],
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
        };
        // A → first alphabet cell at (8, 224)
        assert_eq!(atlas.glyph_rect('A'), Some((8, 224, 8, 14)));
        // Z → 26th cell.
        assert_eq!(atlas.glyph_rect('Z'), Some((8 + 25 * 8, 224, 8, 14)));
        // 0 → first digit cell.
        assert_eq!(atlas.glyph_rect('0'), Some((8, 209, 8, 11)));
        // Space is unmapped.
        assert_eq!(atlas.glyph_rect(' '), None);
    }

    #[test]
    fn measure_width_uses_cell_pitch() {
        let atlas = MenuGlyphAtlas {
            rgba: vec![0; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize],
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
        };
        // "NEW GAME" = 8 chars × 8 px pitch (incl. the space).
        assert_eq!(atlas.measure_width("NEW GAME"), 8 * 8);
    }

    /// Disc-gated: build the real menu-glyph atlas from a `PROT.DAT`.
    /// Skips when `extracted/PROT.DAT` is missing (CI runs without disc data).
    #[test]
    fn builds_real_menu_glyph_atlas_when_disc_extracted() {
        let path = "../../extracted/PROT.DAT";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT.DAT missing");
                return;
            }
        };
        let atlas = build_atlas_from_prot_dat(&bytes).expect("build menu-glyph atlas");
        assert_eq!(atlas.width, ATLAS_WIDTH);
        assert_eq!(atlas.height, ATLAS_HEIGHT);
        assert_eq!(atlas.rgba.len(), (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize);
        // Sanity: not all-transparent and not all-opaque.
        let opaque = atlas.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        let transparent = atlas.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
        assert!(opaque > 1000, "atlas has too few opaque pixels: {}", opaque);
        assert!(
            transparent > 1000,
            "atlas has too few transparent pixels: {}",
            transparent
        );
        // Verify the 'A' cell at (8, 224) contains at least one
        // opaque pixel (the glyph itself).
        let (ax, ay, aw, ah) = atlas.glyph_rect('A').unwrap();
        let mut a_opaque = 0;
        for y in ay..ay + ah {
            for x in ax..ax + aw {
                let off = ((y * ATLAS_WIDTH + x) * 4 + 3) as usize;
                if atlas.rgba[off] > 0 {
                    a_opaque += 1;
                }
            }
        }
        assert!(
            a_opaque > 0,
            "no opaque pixels in 'A' cell at ({},{})",
            ax,
            ay
        );
    }
}
