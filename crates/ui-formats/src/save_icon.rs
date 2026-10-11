//! The **save-slot portrait sheet** - the 16x16 character portraits the save
//! UI draws and the memory-card block icon is cut from.
//!
//! One 4bpp TIM in the **menu overlay** (PROT entry 899) holds sixteen 16x16
//! tiles side by side as a single 256x16 strip, plus a 256-entry CLUT block
//! that is sixteen 16-colour palettes - **one palette per tile**. The strip
//! uploads to VRAM at `(960, 224)` and the CLUT row to `(0, 1)`, both by the
//! per-TIM uploader `FUN_800198E0` called twice from the save-screen driver
//! `FUN_801DD35C` (`0x801DD4CC` uploads the UI sheet, `0x801DD4D8` this
//! strip).
//!
//! Fifteen tiles carry portraits; **tile 15 is blank** - one flat pixel index
//! and an all-zero palette. Fifteen is the number of save blocks a PSX
//! memory card holds, and every consumer indexes the sheet the same way:
//! `u = index * 16`, `clut_id = 0x40 + index` (CLUT row 1, x = `index * 16`).
//!
//! ## Two index meanings, one sheet
//!
//! The tile space is a **character** space, but its consumers do not all
//! index it by character:
//!
//! * The memory-card block icon (`FUN_801E1934`) indexes by **card slot** -
//!   tile `slot`, where the displayed save number is `slot + 1`. See
//!   [`tile_for_slot`].
//! * The save-screen info panel indexes by **party id** (`0` Vahn, `1` Noa,
//!   `2` Gala) read from the per-slot buffer at `+0x2C + i`
//!   (`FUN_801E08D8` at `0x801E0C6C`).
//!
//! ## Layout: strip rows vs. save-block tile
//!
//! The TIM stores the tiles **row-interleaved** across one 256-pixel-wide
//! strip: a tile's 16 rows are 8-byte runs at stride 128. A PSX save block
//! instead stores its icon as a **contiguous** 16x16 tile - 128 bytes at
//! block `+0x80`. Retail never converts in software: `FUN_801E1934` reads the
//! tile back out of VRAM with `StoreImage` on the rect
//! `(0x3C0 + slot*4, 0xE0, 4, 16)`, and a rectangular VRAM read is contiguous
//! by construction. Offline we do the same conversion arithmetically -
//! [`SaveIconSheet::tile_block_pixels`].
//!
//! A byte search for a save block's icon therefore finds the **CLUT**
//! verbatim in the overlay but **not** the pixels.
//!
//! ## The pre-de-interleaved copy is real, and it is only three tiles
//!
//! Tiles 0, 1 and 2 also exist as three standalone 16x16 TIMs in the
//! boot-resident system-UI TIM-pack at **raw PROT TOC entry 0** (members
//! 16, 17, 18 - `PROT.DAT` `0x1AC90` / `0x1AD50` / `0x1AE10`, stride `0xC0`),
//! already in contiguous tile layout because each is its own image rect.
//! Raw TOC entries 0 and 1 are the head region the extraction index space
//! skips, which is why no `NNNN_*.BIN` extraction file carries those bytes.
//! See [`crate::system_ui_bundle`]. Those three upload to `(976/980/984,
//! 256)` with CLUTs on rows `304..=306` - a different VRAM residency from
//! this strip, and not the source the block-icon writer reads.
//!
//! Parser scope: this module owns the strip in PROT entry 899.

use anyhow::{Context, Result, bail};
use legaia_tim::{PixelMode, Tim};

/// PROT extraction entry that carries the sheet (the menu overlay).
pub const PROT_ENTRY: usize = 899;

/// Tiles in the strip, including the blank one.
pub const TILE_COUNT: usize = 16;

/// Tiles a running game can actually select. A PSX memory card holds 15
/// save blocks, so the block-icon writer's `slot * 4` halfword step reaches
/// tiles `0..=14` and never tile 15.
pub const USABLE_TILE_COUNT: usize = 15;

/// Tile edge in pixels.
pub const TILE_SIZE: usize = 16;

/// Palette entries per tile.
pub const CLUT_ENTRIES_PER_TILE: usize = 16;

/// Bytes one tile occupies in a PSX save block (16x16 @ 4bpp).
pub const TILE_BLOCK_BYTES: usize = TILE_SIZE * TILE_SIZE / 2;

/// Bytes one tile's palette occupies in a save block (16 x u16 LE).
pub const TILE_CLUT_BYTES: usize = CLUT_ENTRIES_PER_TILE * 2;

/// Declared CLUT rect of the sheet: 256 entries on VRAM row 1.
pub const CLUT_RECT: (u16, u16, u16, u16) = (0, 1, 256, 1);

/// Declared image rect: 64 VRAM halfwords (256 4bpp texels) x 16 rows at
/// `(960, 224)`.
pub const IMAGE_RECT: (u16, u16, u16, u16) = (960, 224, 64, 16);

/// Byte offset of the sheet TIM inside PROT entry 899 on the retail NA disc.
///
/// [`find_in_entry`] locates the sheet by its rect fingerprint and does not
/// use this; it is exported so a patcher can address the bytes without
/// re-scanning, and so tests can pin that the scan agrees with it.
/// Provenance: the single materialisation of the TIM's VA `0x801EE120`
/// (overlay base `0x801CE818`) is the `lui`/`addiu` pair at `0x801DD4D4`
/// feeding the `FUN_800198E0` upload call.
pub const PROT_ENTRY_OFFSET: usize = 0x1F908;

/// Offset of the CLUT **data** (first palette entry) within the entry.
pub const PROT_ENTRY_CLUT_DATA_OFFSET: usize = PROT_ENTRY_OFFSET + 8 + 12;

/// Offset of the pixel **data** (row 0, pixel 0) within the entry.
pub const PROT_ENTRY_PIXEL_DATA_OFFSET: usize =
    PROT_ENTRY_CLUT_DATA_OFFSET + (CLUT_RECT.2 as usize) * 2 + 12;

/// Bytes per strip row: 256 pixels at 4bpp.
pub const STRIP_ROW_BYTES: usize = 128;

/// The parsed sheet.
#[derive(Debug, Clone)]
pub struct SaveIconSheet {
    /// Byte offset the sheet was found at inside the PROT entry.
    pub entry_offset: usize,
    /// The TIM exactly as it ships (declared rects preserved).
    pub tim: Tim,
}

impl SaveIconSheet {
    /// Parse a sheet from bytes that start at the TIM header.
    ///
    /// Uses the **lenient** TIM reader: both menu-overlay TIMs ship with
    /// flag word `0x00010008`, and bit 16 is a reserved bit
    /// [`legaia_tim::parse_strict`] rejects. The rect fingerprint that
    /// [`Self::validate`] applies is the stricter check here anyway.
    pub fn parse(bytes: &[u8], entry_offset: usize) -> Result<Self> {
        let tim = legaia_tim::parse(bytes).context("save-icon sheet is not a valid TIM")?;
        let sheet = Self { entry_offset, tim };
        sheet.validate()?;
        Ok(sheet)
    }

    fn validate(&self) -> Result<()> {
        if self.tim.mode != PixelMode::Bpp4 {
            bail!("save-icon sheet must be 4bpp, got {:?}", self.tim.mode);
        }
        let img = &self.tim.image;
        let got = (img.fb_x, img.fb_y, img.fb_w, img.h);
        if got != IMAGE_RECT {
            bail!("save-icon sheet image rect {got:?}, expected {IMAGE_RECT:?}");
        }
        let clut = self
            .tim
            .clut
            .as_ref()
            .context("save-icon sheet has no CLUT block")?;
        let got = (clut.fb_x, clut.fb_y, clut.w, clut.h);
        if got != CLUT_RECT {
            bail!("save-icon sheet CLUT rect {got:?}, expected {CLUT_RECT:?}");
        }
        if img.data.len() < TILE_COUNT * TILE_SIZE * TILE_SIZE / 2 {
            bail!("save-icon sheet pixel block truncated");
        }
        if clut.entries.len() < TILE_COUNT * CLUT_ENTRIES_PER_TILE {
            bail!("save-icon sheet CLUT block truncated");
        }
        Ok(())
    }

    /// The tile's 128 icon bytes in **PSX save-block layout** - 16 rows of
    /// 8 bytes, contiguous. This is the de-interleave: the strip stores the
    /// tile's rows 128 bytes apart, the block stores them back to back.
    ///
    /// Byte-identical to what retail's `StoreImage` on
    /// `(0x3C0 + tile*4, 0xE0, 4, 16)` deposits at block `+0x80`.
    pub fn tile_block_pixels(&self, tile: usize) -> Result<[u8; TILE_BLOCK_BYTES]> {
        if tile >= TILE_COUNT {
            bail!("tile {tile} out of range (0..{TILE_COUNT})");
        }
        let data = &self.tim.image.data;
        let mut out = [0u8; TILE_BLOCK_BYTES];
        let row_run = TILE_SIZE / 2; // 8 bytes per tile row
        for row in 0..TILE_SIZE {
            let src = row * STRIP_ROW_BYTES + tile * row_run;
            let Some(seg) = data.get(src..src + row_run) else {
                bail!("save-icon sheet pixel data truncated at row {row}");
            };
            out[row * row_run..(row + 1) * row_run].copy_from_slice(seg);
        }
        Ok(out)
    }

    /// Write a tile's 128 icon bytes back into the strip, re-interleaving
    /// them across the 16 rows. Inverse of [`Self::tile_block_pixels`].
    pub fn set_tile_block_pixels(
        &mut self,
        tile: usize,
        pixels: &[u8; TILE_BLOCK_BYTES],
    ) -> Result<()> {
        if tile >= TILE_COUNT {
            bail!("tile {tile} out of range (0..{TILE_COUNT})");
        }
        let row_run = TILE_SIZE / 2;
        for row in 0..TILE_SIZE {
            let dst = row * STRIP_ROW_BYTES + tile * row_run;
            let Some(seg) = self.tim.image.data.get_mut(dst..dst + row_run) else {
                bail!("save-icon sheet pixel data truncated at row {row}");
            };
            seg.copy_from_slice(&pixels[row * row_run..(row + 1) * row_run]);
        }
        Ok(())
    }

    /// The tile's 16-entry palette, raw BGR555 halfwords.
    pub fn tile_clut(&self, tile: usize) -> Result<[u16; CLUT_ENTRIES_PER_TILE]> {
        if tile >= TILE_COUNT {
            bail!("tile {tile} out of range (0..{TILE_COUNT})");
        }
        let clut = self.tim.clut.as_ref().context("sheet has no CLUT")?;
        let base = tile * CLUT_ENTRIES_PER_TILE;
        let Some(seg) = clut.entries.get(base..base + CLUT_ENTRIES_PER_TILE) else {
            bail!("save-icon CLUT truncated at tile {tile}");
        };
        let mut out = [0u16; CLUT_ENTRIES_PER_TILE];
        out.copy_from_slice(seg);
        Ok(out)
    }

    /// The tile's palette as the 32 little-endian bytes a save block carries
    /// at `+0x60`.
    pub fn tile_clut_bytes(&self, tile: usize) -> Result<[u8; TILE_CLUT_BYTES]> {
        let entries = self.tile_clut(tile)?;
        let mut out = [0u8; TILE_CLUT_BYTES];
        for (i, e) in entries.iter().enumerate() {
            out[i * 2..i * 2 + 2].copy_from_slice(&e.to_le_bytes());
        }
        Ok(out)
    }

    /// Replace a tile's palette.
    pub fn set_tile_clut(
        &mut self,
        tile: usize,
        entries: &[u16; CLUT_ENTRIES_PER_TILE],
    ) -> Result<()> {
        if tile >= TILE_COUNT {
            bail!("tile {tile} out of range (0..{TILE_COUNT})");
        }
        let clut = self.tim.clut.as_mut().context("sheet has no CLUT")?;
        let base = tile * CLUT_ENTRIES_PER_TILE;
        let Some(seg) = clut.entries.get_mut(base..base + CLUT_ENTRIES_PER_TILE) else {
            bail!("save-icon CLUT truncated at tile {tile}");
        };
        seg.copy_from_slice(entries);
        Ok(())
    }

    /// Decode a tile to RGBA8, row-major, 16x16.
    pub fn tile_rgba(&self, tile: usize) -> Result<Vec<u8>> {
        let px = self.tile_block_pixels(tile)?;
        let clut = self.tile_clut(tile)?;
        let mut out = Vec::with_capacity(TILE_SIZE * TILE_SIZE * 4);
        for byte in px.iter() {
            for nib in [byte & 0x0F, byte >> 4] {
                out.extend_from_slice(&legaia_tim::bgr555_to_rgba8(clut[nib as usize]));
            }
        }
        Ok(out)
    }

    /// Whether a tile is the blank padding one: a single pixel index across
    /// all 256 texels **and** an all-zero palette. Measured, not assumed -
    /// retail's tile 15 satisfies both.
    pub fn tile_is_blank(&self, tile: usize) -> Result<bool> {
        let px = self.tile_block_pixels(tile)?;
        let flat = px.iter().all(|&b| b == px[0]) && (px[0] & 0x0F) == (px[0] >> 4);
        let clut = self.tile_clut(tile)?;
        Ok(flat && clut.iter().all(|&e| e == 0))
    }

    /// Byte offset of a tile's palette inside the PROT entry.
    pub fn tile_clut_offset(&self, tile: usize) -> usize {
        self.entry_offset + 8 + 12 + tile * TILE_CLUT_BYTES
    }

    /// Byte offsets of a tile's 16 pixel runs inside the PROT entry, one per
    /// strip row. Each run is `TILE_SIZE / 2` = 8 bytes; the runs are
    /// [`STRIP_ROW_BYTES`] apart, which is what makes the tile scattered on
    /// disc even though it is contiguous in VRAM.
    pub fn tile_pixel_run_offsets(&self, tile: usize) -> [usize; TILE_SIZE] {
        let clut_bytes = self
            .tim
            .clut
            .as_ref()
            .map_or(0, |c| (c.w as usize) * (c.h as usize) * 2);
        let pix_base = self.entry_offset + 8 + 12 + clut_bytes + 12;
        let row_run = TILE_SIZE / 2;
        std::array::from_fn(|row| pix_base + row * STRIP_ROW_BYTES + tile * row_run)
    }
}

/// Which tile a memory-card slot's block icon is cut from.
///
/// `slot` is the **0-based** card slot; the save number the game displays is
/// `slot + 1`. Retail computes the VRAM x as `0x3C0 + slot * 4` halfwords,
/// which is the strip's own x plus one 16-pixel tile per slot - so the tile
/// index *is* the slot index, with no modulo and no bound check.
///
/// PORT: FUN_801e1934 (`0x801E1B1C..0x801E1B34`)
pub fn tile_for_slot(slot: usize) -> usize {
    slot
}

/// The VRAM rect retail's `StoreImage` reads for `slot`, in halfwords:
/// `(x, y, w, h)`.
///
/// PORT: FUN_801e1934 (`0x801E1B1C..0x801E1B34`)
pub fn slot_vram_rect(slot: usize) -> (u16, u16, u16, u16) {
    (
        IMAGE_RECT.0 + (slot as u16) * 4,
        IMAGE_RECT.1,
        4,
        TILE_SIZE as u16,
    )
}

/// Locate the sheet inside a PROT entry by its rect fingerprint.
///
/// Scans for a TIM header whose CLUT rect is [`CLUT_RECT`] and image rect is
/// [`IMAGE_RECT`], so the location comes from the bytes rather than from a
/// constant. Returns the byte offset within `entry`.
pub fn find_in_entry(entry: &[u8]) -> Option<usize> {
    let mut off = 0usize;
    while off + 8 <= entry.len() {
        // TIM magic is a 32-bit 0x10 with the low byte first.
        if entry[off] == 0x10
            && entry[off + 1] == 0
            && entry[off + 2] == 0
            && entry[off + 3] == 0
            && let Ok(tim) = legaia_tim::parse(&entry[off..])
            && tim.mode == PixelMode::Bpp4
            && (tim.image.fb_x, tim.image.fb_y, tim.image.fb_w, tim.image.h) == IMAGE_RECT
            && tim
                .clut
                .as_ref()
                .is_some_and(|c| (c.fb_x, c.fb_y, c.w, c.h) == CLUT_RECT)
        {
            return Some(off);
        }
        off += 4;
    }
    None
}

/// Parse the sheet out of a whole PROT entry 899 image.
pub fn parse_entry(entry: &[u8]) -> Result<SaveIconSheet> {
    let off = find_in_entry(entry).context(
        "no save-icon sheet in this entry (no 4bpp TIM with image (960,224) 64x16 + CLUT (0,1) 256x1)",
    )?;
    SaveIconSheet::parse(&entry[off..], off)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic sheet with the retail rects: distinct pixel data per
    /// tile so the de-interleave is observable, and a blank final tile.
    fn synthetic() -> Vec<u8> {
        let clut_len = 12 + (CLUT_RECT.2 as usize) * 2;
        let img_len = 12 + (IMAGE_RECT.2 as usize) * (IMAGE_RECT.3 as usize) * 2;
        let mut out = Vec::with_capacity(8 + clut_len + img_len);
        out.extend_from_slice(&0x10u32.to_le_bytes());
        // Retail's flag word: 4bpp + has-CLUT, plus the reserved bit 16 both
        // menu-overlay TIMs set.
        out.extend_from_slice(&0x0001_0008u32.to_le_bytes());
        out.extend_from_slice(&(clut_len as u32).to_le_bytes());
        for v in [CLUT_RECT.0, CLUT_RECT.1, CLUT_RECT.2, CLUT_RECT.3] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for tile in 0..TILE_COUNT {
            for i in 0..CLUT_ENTRIES_PER_TILE {
                // Tile 15 keeps an all-zero palette, like retail's padding.
                let e = if tile == TILE_COUNT - 1 {
                    0u16
                } else {
                    ((tile * 16 + i) as u16) | 0x0001
                };
                out.extend_from_slice(&e.to_le_bytes());
            }
        }
        out.extend_from_slice(&(img_len as u32).to_le_bytes());
        for v in [IMAGE_RECT.0, IMAGE_RECT.1, IMAGE_RECT.2, IMAGE_RECT.3] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for row in 0..TILE_SIZE {
            for tile in 0..TILE_COUNT {
                for b in 0..(TILE_SIZE / 2) {
                    // Tile 15 is a flat fill; the rest encode (tile, row, b).
                    out.push(if tile == TILE_COUNT - 1 {
                        0
                    } else {
                        ((tile as u8) << 4) | ((row as u8 + b as u8) & 0x0F)
                    });
                }
            }
        }
        out
    }

    #[test]
    fn save_icon_parses_synthetic_sheet() {
        let sheet = SaveIconSheet::parse(&synthetic(), 0).unwrap();
        assert_eq!(sheet.tim.mode, PixelMode::Bpp4);
        assert_eq!(sheet.tim.pixel_width(), TILE_COUNT * TILE_SIZE);
    }

    #[test]
    fn save_icon_deinterleave_is_contiguous_and_reversible() {
        let sheet = SaveIconSheet::parse(&synthetic(), 0).unwrap();
        for tile in 0..TILE_COUNT - 1 {
            let px = sheet.tile_block_pixels(tile).unwrap();
            // Every byte of the tile carries that tile's tag in the high
            // nibble - proof the runs came from the right strip columns.
            assert!(px.iter().all(|b| (b >> 4) as usize == tile), "tile {tile}");
        }
        // Round-trip a tile through the setter.
        let mut sheet = sheet;
        let original = sheet.tile_block_pixels(3).unwrap();
        let replacement = [0xABu8; TILE_BLOCK_BYTES];
        sheet.set_tile_block_pixels(3, &replacement).unwrap();
        assert_eq!(sheet.tile_block_pixels(3).unwrap(), replacement);
        // Neighbours are untouched by the scattered write.
        assert!(
            sheet
                .tile_block_pixels(2)
                .unwrap()
                .iter()
                .all(|b| b >> 4 == 2)
        );
        assert!(
            sheet
                .tile_block_pixels(4)
                .unwrap()
                .iter()
                .all(|b| b >> 4 == 4)
        );
        sheet.set_tile_block_pixels(3, &original).unwrap();
        assert_eq!(sheet.tile_block_pixels(3).unwrap(), original);
    }

    #[test]
    fn save_icon_blank_tile_detection() {
        let sheet = SaveIconSheet::parse(&synthetic(), 0).unwrap();
        assert!(sheet.tile_is_blank(TILE_COUNT - 1).unwrap());
        for tile in 0..TILE_COUNT - 1 {
            assert!(!sheet.tile_is_blank(tile).unwrap(), "tile {tile}");
        }
    }

    #[test]
    fn save_icon_slot_maps_to_tile_and_vram_rect() {
        // Displayed save number is slot + 1, so save 1 uses tile 0.
        assert_eq!(tile_for_slot(0), 0);
        assert_eq!(tile_for_slot(14), 14);
        assert_eq!(slot_vram_rect(0), (960, 224, 4, 16));
        assert_eq!(slot_vram_rect(14), (960 + 56, 224, 4, 16));
        // The blank tile sits one step past the last reachable slot.
        assert_eq!(slot_vram_rect(USABLE_TILE_COUNT).0, 960 + 60);
    }

    #[test]
    fn save_icon_clut_bytes_are_little_endian_pairs() {
        let sheet = SaveIconSheet::parse(&synthetic(), 0).unwrap();
        let entries = sheet.tile_clut(2).unwrap();
        let bytes = sheet.tile_clut_bytes(2).unwrap();
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(&bytes[i * 2..i * 2 + 2], &e.to_le_bytes());
        }
    }

    #[test]
    fn save_icon_find_in_entry_locates_by_fingerprint() {
        let mut entry = vec![0u8; 0x400];
        entry.extend_from_slice(&synthetic());
        let off = find_in_entry(&entry).expect("fingerprint scan finds the sheet");
        assert_eq!(off, 0x400);
        let sheet = parse_entry(&entry).unwrap();
        assert_eq!(sheet.entry_offset, 0x400);
        // The derived offsets track the located header, not the constant.
        assert_eq!(sheet.tile_clut_offset(0), 0x400 + 20);
        assert_eq!(sheet.tile_pixel_run_offsets(0)[0], 0x400 + 20 + 512 + 12);
    }

    #[test]
    fn save_icon_rejects_wrong_rects() {
        let mut bad = synthetic();
        // Move the image rect off (960,224).
        bad[8 + 12 + 512 + 4..8 + 12 + 512 + 6].copy_from_slice(&0u16.to_le_bytes());
        assert!(SaveIconSheet::parse(&bad, 0).is_err());
        assert!(find_in_entry(&bad).is_none());
    }

    #[test]
    fn save_icon_tile_index_bounds_are_checked() {
        let mut sheet = SaveIconSheet::parse(&synthetic(), 0).unwrap();
        assert!(sheet.tile_block_pixels(TILE_COUNT).is_err());
        assert!(sheet.tile_clut(TILE_COUNT).is_err());
        assert!(sheet.tile_rgba(TILE_COUNT).is_err());
        assert!(
            sheet
                .set_tile_block_pixels(TILE_COUNT, &[0; TILE_BLOCK_BYTES])
                .is_err()
        );
        assert!(
            sheet
                .set_tile_clut(TILE_COUNT, &[0; CLUT_ENTRIES_PER_TILE])
                .is_err()
        );
    }

    #[test]
    fn save_icon_constant_offsets_agree_with_block_arithmetic() {
        // The exported constants must describe the same layout the parser
        // derives, so a patcher addressing bytes by constant and a viewer
        // addressing them by parse cannot drift.
        assert_eq!(PROT_ENTRY_CLUT_DATA_OFFSET, PROT_ENTRY_OFFSET + 20);
        assert_eq!(
            PROT_ENTRY_PIXEL_DATA_OFFSET,
            PROT_ENTRY_CLUT_DATA_OFFSET + 512 + 12
        );
        let sheet = SaveIconSheet::parse(&synthetic(), PROT_ENTRY_OFFSET).unwrap();
        assert_eq!(sheet.tile_clut_offset(0), PROT_ENTRY_CLUT_DATA_OFFSET);
        assert_eq!(
            sheet.tile_pixel_run_offsets(0)[0],
            PROT_ENTRY_PIXEL_DATA_OFFSET
        );
    }
}
