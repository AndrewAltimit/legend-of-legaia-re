//! The fishing overlay's **HUD sprite table** - the 20-byte records the
//! shared quad emitter `FUN_801D63B0` draws every HUD glyph, digit, gauge cap
//! and banner from.
//!
//! ## Provenance
//!
//! `FUN_801D63B0` (PROT 0972 at slot-A base `0x801CE818`,
//! `0x801D63B0..0x801D67BC`) takes `(anchor, x, y, id, brightness, scale_x,
//! scale_y)`, indexes this table at `0x801D8590 + (id & 0x3FF) * 20`
//! (`lui 0x801E` / `addiu -0x7A70` at `0x801D63E8`), and builds one `POLY_GT4`
//! (`0x0C000000` tag, GP0 `0x3C` or `0x3E`) from the record. The digit
//! emitters `FUN_801D7DD8` / `FUN_801D7D44` patch records `6` / `0x18`'s `u`
//! byte (`0x801D8610` / `0x801D8778`) before each call.
//!
//! ## Record layout (stride [`FISHING_SPRITE_STRIDE`])
//!
//! | Off | Field | Read at |
//! |---|---|---|
//! | `+0x00` | `scale`: i32, `0x1000` = 1.0, multiplies the cell size | `0x801D65F4` |
//! | `+0x04` | `tpage`: GP0 texpage word (the ABR rate is added on top) | `0x801D6760` |
//! | `+0x06` | `clut`: GP0 CLUT word | `0x801D677C` |
//! | `+0x08` | `u`, `v` | `0x801D66E0` |
//! | `+0x0A` | `w`, `h`: the cell in texels | `0x801D65F8` |
//! | `+0x0C` | top `r, g, b` (vertices 0 / 1) | `0x801D644C` |
//! | `+0x0F` | `semi`: semi-transparency bit for mode-0 ids | `0x801D6404` |
//! | `+0x10` | bottom `r, g, b` (vertices 2 / 3) | `0x801D651C` |
//! | `+0x13` | `abr`: blend rate for mode-0 ids | `0x801D6408` |
//!
//! The table holds [`FISHING_SPRITE_COUNT`] records; the words after the last
//! one are other data (record 29's `scale` would be `4`). Every record names
//! the 4bpp page at `(832, 0)` and a palette of the CLUT strip at `(0, 503)`,
//! which is the first TIM of the fishing venue bundle's (`other1`) texture
//! list - so the art the records cut is disc data the venue upload already
//! places.
//!
//! ## No Sony bytes
//!
//! Layout and addresses only; the records are read from the user's disc.

use crate::fishing_species::FISHING_OVERLAY_BASE_VA;

/// VA of record 0 (`0x801D63E8..0x801D63EC`).
pub const FISHING_SPRITE_TABLE_VA: u32 = 0x801D_8590;
/// Bytes per record.
pub const FISHING_SPRITE_STRIDE: usize = 20;
/// Records in the table: ids `0..=0x1C`, the highest any caller passes.
pub const FISHING_SPRITE_COUNT: usize = 29;
/// The `scale` value every live record carries (1.0 in 20.12).
pub const FISHING_SPRITE_UNIT_SCALE: i32 = 0x1000;

/// One HUD sprite record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingSprite {
    pub scale: i32,
    pub tpage: u16,
    pub clut: u16,
    pub u: u8,
    pub v: u8,
    pub w: u8,
    pub h: u8,
    pub rgb_top: [u8; 3],
    pub semi: bool,
    pub rgb_bottom: [u8; 3],
    pub abr: u8,
}

impl FishingSprite {
    /// Decode one 20-byte record.
    pub fn from_bytes(r: &[u8]) -> Option<Self> {
        let r = r.get(..FISHING_SPRITE_STRIDE)?;
        Some(Self {
            scale: i32::from_le_bytes([r[0], r[1], r[2], r[3]]),
            tpage: u16::from_le_bytes([r[4], r[5]]),
            clut: u16::from_le_bytes([r[6], r[7]]),
            u: r[8],
            v: r[9],
            w: r[10],
            h: r[11],
            rgb_top: [r[12], r[13], r[14]],
            semi: r[15] != 0,
            rgb_bottom: [r[16], r[17], r[18]],
            abr: r[19],
        })
    }
}

/// Read the table out of the fishing overlay image (PROT 0972, loaded at its
/// slot-A base). `None` when the image is too short or a record does not look
/// like one (a `scale` other than 1.0 - the shape every live record has), so
/// a host keeps its fallback rather than drawing garbage.
pub fn parse(overlay: &[u8]) -> Option<Vec<FishingSprite>> {
    let off = (FISHING_SPRITE_TABLE_VA - FISHING_OVERLAY_BASE_VA) as usize;
    let table = overlay.get(off..off + FISHING_SPRITE_COUNT * FISHING_SPRITE_STRIDE)?;
    let recs: Vec<FishingSprite> = table
        .as_chunks::<FISHING_SPRITE_STRIDE>()
        .0
        .iter()
        .filter_map(|r| FishingSprite::from_bytes(r))
        .collect();
    recs.iter()
        .all(|r| r.scale == FISHING_SPRITE_UNIT_SCALE && r.w != 0 && r.h != 0)
        .then_some(recs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_with(records: &[[u8; FISHING_SPRITE_STRIDE]]) -> Vec<u8> {
        let off = (FISHING_SPRITE_TABLE_VA - FISHING_OVERLAY_BASE_VA) as usize;
        let mut img = vec![0u8; off + FISHING_SPRITE_COUNT * FISHING_SPRITE_STRIDE];
        for (i, r) in records.iter().enumerate() {
            img[off + i * FISHING_SPRITE_STRIDE..][..FISHING_SPRITE_STRIDE].copy_from_slice(r);
        }
        img
    }

    fn rec(u: u8) -> [u8; FISHING_SPRITE_STRIDE] {
        [
            0, 0x10, 0, 0, 0x0D, 0, 0xC1, 0x7D, u, 2, 16, 8, 1, 2, 3, 1, 4, 5, 6, 1,
        ]
    }

    #[test]
    fn decodes_every_field_in_place() {
        let r = FishingSprite::from_bytes(&rec(7)).unwrap();
        assert_eq!(r.scale, 0x1000);
        assert_eq!((r.tpage, r.clut), (0x0D, 0x7DC1));
        assert_eq!((r.u, r.v, r.w, r.h), (7, 2, 16, 8));
        assert_eq!(
            (r.rgb_top, r.semi, r.rgb_bottom, r.abr),
            ([1, 2, 3], true, [4, 5, 6], 1)
        );
    }

    #[test]
    fn a_table_with_a_foreign_record_does_not_parse() {
        let mut recs = vec![rec(0); FISHING_SPRITE_COUNT];
        assert_eq!(
            parse(&image_with(&recs)).unwrap().len(),
            FISHING_SPRITE_COUNT
        );
        recs[28][0] = 4;
        recs[28][1] = 0;
        assert!(parse(&image_with(&recs)).is_none());
        assert!(parse(&[0u8; 16]).is_none());
    }
}
