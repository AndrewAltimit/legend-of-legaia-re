//! Ocean tile texture + CLUT animation extraction from kingdom bundles.
//!
//! The world-map ocean surface is a static 4bpp 64×256 (halfword) texture
//! at VRAM `(768, 256)` whose first 16 CLUT entries at VRAM `(0, 506)`
//! are overwritten each frame by one of 13 precomputed BGR555 frames -
//! that's the rolling-wave animation. The texture, base CLUT, and
//! animation table all live inside slot 0 (TIM_LIST) of each world-map
//! kingdom's 7-asset bundle (PROT 0085 / 0244 / 0391) and are
//! byte-identical across the three kingdoms (shared global asset baked
//! into each TIM_LIST).
//!
//! See `docs/subsystems/world-map.md` § "Ocean / coastline source" for
//! the reverse-engineering provenance, including the 532-byte
//! "CLUT-only TIM" record layout the disc uses to wrap each frame.

/// Per-kingdom ocean-tile assets recovered from slot 0 of the kingdom
/// bundle.
pub struct OceanAssets {
    /// 4bpp indexed pixel data, 64 halfwords × 256 rows = 32 768 bytes.
    /// Each halfword holds 4 pixels (low nibble first); CLUT index
    /// lookups go through the base CLUT + per-frame overrides.
    pub texture: Vec<u8>,
    /// Static 256-entry CLUT row uploaded at world-map entry. Encoded
    /// as 512 bytes (256 BGR555 entries, little-endian). The first 16
    /// entries are the ones the runtime overrides per frame; entries
    /// 16..255 stay fixed and belong to other tiles sharing the row.
    pub base_clut: Vec<u8>,
    /// 13 animation frames × 32 bytes (16 BGR555 entries each). Frame N's
    /// bytes overwrite the first 16 entries of `base_clut` in VRAM,
    /// creating the rolling-wave appearance. Frame 13 (the 14th slot in
    /// the disc table) is all zeros and unused, so it's not surfaced
    /// here.
    pub animation_frames: Vec<u8>,
}

/// Frame-0 signature of the ocean CLUT animation table. 16 BGR555 entries
/// little-endian: index 0 = transparent black, indices 1..15 = the first
/// frame of the wave-peak cycle starting at `(0x3083, 0x2C83, 0x3083,
/// 0x34C4, 0x38E4, 0x3D05, ...)`. Identical across PROT 0085 / 0244 /
/// 0391 (verified by SHA-256 cross-check).
pub const OCEAN_ANIM_FRAME0_HEAD: [u8; 16] = [
    0x00, 0x00, 0x83, 0x30, 0x83, 0x2C, 0x83, 0x30, 0xC4, 0x34, 0xE4, 0x38, 0x05, 0x3D, 0xE4, 0x38,
];

/// Frame count baked into the disc-side animation table. The 14th slot
/// (`0x000 ...` zeros) is unused padding; the runtime only cycles
/// through frames 0..12.
pub const OCEAN_ANIM_FRAME_COUNT: usize = 13;

/// Extract the ocean tile texture + base CLUT + 13-frame CLUT animation
/// table out of a kingdom bundle's decompressed slot 0 (TIM_LIST). The
/// caller decompresses slot 0 via [`legaia_lzs::decompress`] and passes
/// the resulting buffer here.
///
/// Returns `None` when the buffer doesn't contain the ocean TIM (any
/// non-world-map kingdom, or a corrupt bundle). The ocean TIM is keyed
/// by its VRAM upload coordinates - CLUT at `(0, 506)` and image at
/// `(768, 256)` 64×256 halfwords in 4bpp mode - which match across all
/// three retail world-map kingdoms (Drake 0085, Sebucus 0244, Karisto
/// 0391) and no other PROT entry in the disc.
///
/// The animation table is located by signature-scanning the slot-0
/// buffer for [`OCEAN_ANIM_FRAME0_HEAD`]; the disc layout uses 532-byte
/// "CLUT-only TIM" records to wrap each frame, but the scan is robust
/// against future layout drift since the frame-0 bytes are unique
/// across the slot.
pub fn find_ocean_assets(slot0: &[u8]) -> Option<OceanAssets> {
    if slot0.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes(slot0[0..4].try_into().ok()?) as usize;
    let table_end = 4usize.checked_add(count.checked_mul(4)?)?;
    if slot0.len() < table_end {
        return None;
    }
    let mut texture: Vec<u8> = Vec::new();
    let mut base_clut: Vec<u8> = Vec::new();
    for k in 0..count {
        let woff = u32::from_le_bytes(slot0[4 + k * 4..8 + k * 4].try_into().ok()?) as usize;
        let bo = woff.saturating_mul(4);
        if bo >= slot0.len() {
            continue;
        }
        let Ok(tim) = legaia_tim::parse(&slot0[bo..]) else {
            continue;
        };
        let img_ok = tim.image.fb_x == 768
            && tim.image.fb_y == 256
            && tim.image.fb_w == 64
            && tim.image.h == 256;
        let clut_ok = tim
            .clut
            .as_ref()
            .is_some_and(|c| c.fb_x == 0 && c.fb_y == 506);
        if img_ok && clut_ok {
            texture = tim.image.data.clone();
            base_clut = tim
                .clut
                .as_ref()
                .map(|c| {
                    let mut b = Vec::with_capacity(c.entries.len() * 2);
                    for e in &c.entries {
                        b.extend_from_slice(&e.to_le_bytes());
                    }
                    b
                })
                .unwrap_or_default();
            break;
        }
    }
    if texture.is_empty() || base_clut.is_empty() {
        return None;
    }
    let frame_bytes = OCEAN_ANIM_FRAME_COUNT * 32;
    let mut animation_frames = Vec::new();
    let scan_end = slot0.len().saturating_sub(frame_bytes);
    for i in 0..=scan_end {
        if slot0[i..i + OCEAN_ANIM_FRAME0_HEAD.len()] == OCEAN_ANIM_FRAME0_HEAD {
            animation_frames = slot0[i..i + frame_bytes].to_vec();
            break;
        }
    }
    Some(OceanAssets {
        texture,
        base_clut,
        animation_frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slot_returns_none() {
        assert!(find_ocean_assets(&[]).is_none());
        assert!(find_ocean_assets(&[0, 0, 0, 0]).is_none());
    }

    #[test]
    fn missing_ocean_tim_returns_none() {
        // Tiny TIM_LIST with one TIM whose VRAM coords don't match the
        // ocean signature. The function should bail out before
        // signature-scanning the animation table.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&1u32.to_le_bytes()); // word_offsets[0] = 1 word = 4 bytes
        // (TIM body at byte offset 4 is invalid; legaia_tim::parse rejects)
        buf.extend_from_slice(&[0u8; 16]);
        assert!(find_ocean_assets(&buf).is_none());
    }

    #[test]
    fn anim_frame0_head_constants_match_documented_values() {
        // First 16 entries of frame 0 from the disc-side capture; if the
        // constants drift, the find function will silently miss the
        // animation table, leaving placement_frames empty.
        assert_eq!(OCEAN_ANIM_FRAME_COUNT, 13);
        assert_eq!(OCEAN_ANIM_FRAME0_HEAD[0], 0x00);
        assert_eq!(OCEAN_ANIM_FRAME0_HEAD[1], 0x00);
        // entry 1 = 0x3083 little-endian = (R=3, G=4, B=12) dark blue
        assert_eq!(OCEAN_ANIM_FRAME0_HEAD[2], 0x83);
        assert_eq!(OCEAN_ANIM_FRAME0_HEAD[3], 0x30);
        // entry 5 = 0x3D05 LE = (R=5, G=8, B=15) brightest wave peak
        assert_eq!(OCEAN_ANIM_FRAME0_HEAD[12], 0x05);
        assert_eq!(OCEAN_ANIM_FRAME0_HEAD[13], 0x3D);
    }
}
