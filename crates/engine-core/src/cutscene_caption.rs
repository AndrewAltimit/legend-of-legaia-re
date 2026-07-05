//! Opening-cutscene "It was the Seru." caption image.
//!
//! The dramatic caption shown centered over the villager tableau in the gap
//! between `opdeene`'s two narration crawl blocks is **not** rendered text: it
//! is a pre-rendered **112x32 4bpp TIM** baked into the `opdeene` geometry pack
//! (PROT entry 0749, LZS-decoded offset `0x01EC30`), which retail's scene
//! renderer draws as a screen-space textured quad. This was pinned by cold-boot
//! probes showing every UI text/image draw path fires zero times in the caption
//! window and the string is in RAM in no encoding - see
//! [`docs/subsystems/cutscene.md`](../../../docs/subsystems/cutscene.md).
//!
//! This module decodes that one TIM to RGBA8 (its background palette entry is
//! `0x0000`, so [`legaia_tim::decode_rgba8`] gives it alpha 0 and only the white
//! text is opaque) so the host can upload it as a sprite atlas and blit it,
//! faded, over the gap. No Sony bytes live here: the pixels are decoded from the
//! user's disc at runtime.

use crate::scene::Scene;

/// PROT entry carrying the `opdeene` geometry/texture pack (72 TMDs + 51 TIMs),
/// loaded at its extended footprint. The caption TIM lives among that pack's
/// scene textures.
const OPDEENE_GEOMETRY_ENTRY: u32 = 749;
/// The caption TIM's pixel dimensions. Unique among entry 0749's 51 TIMs, so
/// they identify it without a fragile byte-offset dependency.
const CAPTION_W: u32 = 112;
const CAPTION_H: u32 = 32;

/// A decoded RGBA8 caption image (renderer-agnostic; the host uploads it to a
/// sprite atlas and draws one textured quad against it).
#[derive(Debug, Clone)]
pub struct CaptionImage {
    /// Row-major RGBA8, `width * height * 4` bytes. Background pixels are
    /// alpha 0; the caption glyphs are opaque white.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decode the `opdeene` "It was the Seru." caption from a loaded scene.
///
/// Finds the 112x32 4bpp TIM inside PROT entry 0749's LZS-decompressed sections
/// and decodes it to RGBA8 through its first CLUT palette. Returns `None` for
/// any scene without that TIM, so only `opdeene` ever produces a caption.
pub fn decode_opdeene_caption(scene: &Scene) -> Option<CaptionImage> {
    let entry = scene
        .entries
        .iter()
        .find(|e| e.idx == OPDEENE_GEOMETRY_ENTRY)?;
    let scan = legaia_asset::tim_scan::scan_entry(&entry.bytes);
    for (source, hit) in &scan.hits {
        if hit.width != CAPTION_W || hit.height != CAPTION_H || hit.bpp != 4 {
            continue;
        }
        let section: &[u8] = match source {
            legaia_asset::tim_scan::Source::Lzs(i) => scan.lzs_sections.get(*i)?.as_slice(),
            legaia_asset::tim_scan::Source::Raw => &entry.bytes,
        };
        let payload = section.get(hit.offset..hit.offset + hit.byte_len)?;
        let tim = legaia_tim::parse(payload).ok()?;
        let rgba = legaia_tim::decode_rgba8(&tim, 0).ok()?;
        return Some(CaptionImage {
            rgba,
            width: hit.width,
            height: hit.height,
        });
    }
    None
}
