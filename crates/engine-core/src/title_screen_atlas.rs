//! Title-screen sprite atlas - RGBA decode + per-frame draw helper.
//!
//! Companion to [`crate::title`] (state machine) and
//! [`crate::publisher_logos`] (boot-phase logo atlas). Decodes the
//! 256×256 8bpp title TIM from PROT 0890 (the multi-bank sound-data
//! cluster's trailing pool - see [`legaia_asset::title_pak`] for the
//! disc-source pin, and for why 0888 / 0889 used to look like
//! duplicate sources) into RGBA8 pixels the engine layer uploads as a
//! sprite atlas.
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

/// Pre-decoded title atlas - RGBA8 pixels + the source rects engines
/// sample to compose the retail title screen.
///
/// Build once at boot from PROT 0888 bytes via
/// [`build_atlas_from_prot_888`], hand to engine-render's
/// `upload_sprite_atlas`, then emit one sprite quad per active band
/// each frame the title phase is active.
///
/// The 256×256 source TIM is a sprite sheet that bundles every band
/// retail might draw plus one demo-build leftover (`<DEMO>`). Retail
/// composes the screen by drawing the wordmark + copyright bands
/// always, the press-start band only during the PressStart phase, and
/// skipping the `<DEMO>` band entirely. The engine port mirrors that
/// composition - see [`band_wordmark`], [`band_press_start`],
/// [`band_tm_copyright`], [`band_c_copyright`].
///
/// [`band_wordmark`]: TitleScreenAtlas::band_wordmark
/// [`band_press_start`]: TitleScreenAtlas::band_press_start
/// [`band_tm_copyright`]: TitleScreenAtlas::band_tm_copyright
/// [`band_c_copyright`]: TitleScreenAtlas::band_c_copyright
#[derive(Debug, Clone)]
pub struct TitleScreenAtlas {
    /// RGBA8 pixel data, exactly `4 * width * height` bytes.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Source rect in atlas pixels `(x, y, w, h)` covering the full
    /// 256×256 TIM. Engines that want the legacy "single fullscreen
    /// quad" behaviour can sample this; retail-faithful renderers
    /// use the per-band rects below.
    pub rect: (u32, u32, u32, u32),
}

impl TitleScreenAtlas {
    /// Orb + "Legend of Legaia" wordmark band - drawn in every
    /// post-fade phase.
    pub fn band_wordmark(&self) -> (u32, u32, u32, u32) {
        title_pak::TITLE_BAND_WORDMARK
    }
    /// "PRESS START BUTTON" prompt label - drawn only during the
    /// PressStart phase. Engines should suppress their font-rendered
    /// "PRESS START" text when this band is drawn, to avoid the
    /// duplicate text the early no-disc fallback emits.
    pub fn band_press_start(&self) -> (u32, u32, u32, u32) {
        title_pak::TITLE_BAND_PRESS_START
    }
    /// "TM of Sony..." copyright line - drawn in every post-fade phase.
    pub fn band_tm_copyright(&self) -> (u32, u32, u32, u32) {
        title_pak::TITLE_BAND_TM_COPYRIGHT
    }
    /// "© 1998,1999..." copyright line - drawn in every post-fade phase.
    pub fn band_c_copyright(&self) -> (u32, u32, u32, u32) {
        title_pak::TITLE_BAND_C_COPYRIGHT
    }
}

/// Build a [`TitleScreenAtlas`] from the PROT 0890 byte buffer
/// ([`title_pak::PROT_INDEX_PRIMARY`]; the name keeps the historical
/// `888` for call-site stability).
///
/// Validates the TIM header at [`title_pak::TITLE_TIM_OFFSET`] (or the
/// caller-supplied `tim_offset`), decodes the 256-colour
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

    /// Disc-gated: build the real title atlas from PROT 0890, read at the
    /// entry's own sector span. Skips when `extracted/PROT.DAT` is missing
    /// (CI runs without disc data).
    #[test]
    fn builds_real_title_atlas_when_disc_extracted() {
        let mut archive = match legaia_prot::archive::Archive::open(std::path::Path::new(
            "../../extracted/PROT.DAT",
        )) {
            Ok(a) => a,
            Err(_) => {
                eprintln!("skip: extracted/PROT.DAT missing");
                return;
            }
        };
        let entry = archive
            .entries
            .iter()
            .find(|e| e.index == title_pak::PROT_INDEX_PRIMARY as u32)
            .expect("PROT 0890 in the TOC")
            .clone();
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).expect("read entry");
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
