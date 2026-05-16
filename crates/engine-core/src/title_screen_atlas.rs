//! Title-screen sprite atlas — RGBA decode + per-frame draw helper.
//!
//! Companion to [`crate::title`] (state machine) and
//! [`crate::publisher_logos`] (boot-phase logo atlas). Decodes the
//! 256×256 8bpp title TIM from PROT 0888 (`sound_data2` per CDNAME,
//! actually carries title art - see
//! [`legaia_asset::title_pak`] for the disc-source pin) into RGBA8
//! pixels the engine layer uploads as a sprite atlas.
//!
//! The title TIM renders to the complete Legend of Legaia title
//! screen: wordmark, orb, "PRESS START BUTTON" prompt, "NEW GAME" /
//! "CONTINUE" menu, copyright lines. Engines blit it as a single
//! quad behind the cursor / blink overlay drawn by the existing
//! [`crate::title::TitleSession`].

use legaia_asset::title_pak;

/// Width of the title atlas in pixels (matches the source 256×256 TIM).
pub const ATLAS_WIDTH: u32 = 256;
/// Height of the title atlas in pixels (matches the source 256×256 TIM).
pub const ATLAS_HEIGHT: u32 = 256;

/// Pre-decoded title atlas — RGBA8 pixels + the source rect to sample
/// when emitting the sprite quad.
///
/// Build once at boot from PROT 0888 bytes via
/// [`build_atlas_from_prot_888`], hand to engine-render's
/// `upload_sprite_atlas`, then sample [`rect`] each frame the title
/// phase is active.
///
/// [`rect`]: TitleScreenAtlas::rect
#[derive(Debug, Clone)]
pub struct TitleScreenAtlas {
    /// RGBA8 pixel data, exactly `4 * width * height` bytes.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Source rect in atlas pixels `(x, y, w, h)` to sample for the
    /// title quad. Always `(0, 0, width, height)` for the single-TIM
    /// layout; kept as a field for symmetry with
    /// [`crate::publisher_logos::LogosAtlas::rects`].
    pub rect: (u32, u32, u32, u32),
}

/// Build a [`TitleScreenAtlas`] from a PROT 0888 byte buffer (or one
/// of its multi-bank duplicates - see
/// [`title_pak::TITLE_TIM_ALTERNATE_SOURCES`]).
///
/// Validates the TIM header at [`title_pak::TITLE_TIM_OFFSET`] (or the
/// caller-supplied alternate `tim_offset`), decodes the 256-colour
/// CLUT against the pixel block, and returns RGBA8 pixels in
/// row-major order.
pub fn build_atlas_from_prot_888(
    prot_bytes: &[u8],
    tim_offset: usize,
) -> anyhow::Result<TitleScreenAtlas> {
    let tim = title_pak::extract_title_tim(prot_bytes, tim_offset)?;
    let parsed = legaia_tim::parse(tim.bytes)?;
    let rgba = legaia_tim::decode_rgba8(&parsed, 0)?;
    let width = parsed.pixel_width() as u32;
    let height = parsed.image.h as u32;
    if rgba.len() != (width * height * 4) as usize {
        anyhow::bail!(
            "title TIM decode size mismatch: rgba={} expected w*h*4={}",
            rgba.len(),
            width * height * 4
        );
    }
    Ok(TitleScreenAtlas {
        rgba,
        width,
        height,
        rect: (0, 0, width, height),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disc-gated: build the real title atlas from extracted PROT 0888.
    /// Skips when `extracted/` is missing (CI runs without disc data).
    #[test]
    fn builds_real_title_atlas_when_disc_extracted() {
        let path = "../../extracted/PROT/0888_sound_data2.BIN";
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skip: extracted/PROT/0888_sound_data2.BIN missing");
                return;
            }
        };
        let atlas = build_atlas_from_prot_888(&bytes, title_pak::TITLE_TIM_OFFSET)
            .expect("build title atlas");
        assert_eq!(atlas.width, ATLAS_WIDTH);
        assert_eq!(atlas.height, ATLAS_HEIGHT);
        assert_eq!(atlas.rgba.len(), (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize);
        assert_eq!(atlas.rect, (0, 0, ATLAS_WIDTH, ATLAS_HEIGHT));
        // Sanity: not all-transparent and not all-opaque-black.
        let any_opaque = atlas.rgba.chunks_exact(4).any(|p| p[3] > 0);
        let any_non_black = atlas.rgba.chunks_exact(4).any(|p| p[0] | p[1] | p[2] != 0);
        assert!(any_opaque, "title atlas is fully transparent");
        assert!(any_non_black, "title atlas is fully black");
    }
}
