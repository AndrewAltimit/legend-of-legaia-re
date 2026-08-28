//! Save-slot portrait replacement - swap one of the memory-card / save-UI
//! character portraits for user art.
//!
//! The portraits live in a single 4bpp TIM in the menu overlay (PROT entry
//! 899): sixteen 16x16 tiles row-interleaved across one 256x16 strip, with a
//! 256-entry CLUT block that is **one 16-colour palette per tile**. Format
//! detail in [`legaia_asset::save_icon`] and
//! [`docs/formats/save-icon.md`](../../../docs/formats/save-icon.md).
//!
//! ## Why this is not just `tim-replace` on entry 899
//!
//! Two properties of the sheet defeat the generic texture path:
//!
//! * **The palette is per tile.** [`legaia_tim::encode::encode_replacement`]
//!   replicates a rebuilt palette into *every* CLUT row, which is correct for
//!   a normal multi-palette texture and catastrophic here - editing one
//!   portrait would repaint the other fifteen.
//! * **A tile is scattered on disc.** Its 16 rows are 8-byte runs 128 bytes
//!   apart, so replacing one tile is 16 small writes, not one.
//!
//! So this module owns its own encoder: it quantises a 16x16 RGBA image
//! against that tile's own 16 slots and writes the tile's runs plus its 32
//! palette bytes, leaving every other tile byte-identical.
//!
//! ## Fifteen slots, not sixteen
//!
//! [`SLOT_COUNT`] is 15. Tile 15 is blank width padding and no code path
//! selects it: the block-icon writer `FUN_801E1934` reads the VRAM rect
//! `(0x3C0 + slot*4, 0xE0, 4, 16)` and a PSX memory card holds 15 blocks, so
//! `slot` never reaches 15. Art painted there would silently never appear,
//! which is why [`replace_slot`] refuses it.

use anyhow::{Context, Result, bail};

use crate::disc::DiscPatcher;
use legaia_asset::save_icon::{
    self, CLUT_ENTRIES_PER_TILE, SaveIconSheet, TILE_BLOCK_BYTES, TILE_CLUT_BYTES, TILE_SIZE,
};
use legaia_asset::title_pak;

/// PROT entry that carries the sheet.
pub const PROT_ENTRY: u32 = save_icon::PROT_ENTRY as u32;

/// Replaceable slots. Save number `n` (as the game displays it) is slot
/// `n - 1`.
pub const SLOT_COUNT: usize = save_icon::USABLE_TILE_COUNT;

/// What a replacement changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotReplaceOutcome {
    /// Slot replaced (0-based); the displayed save number is `slot + 1`.
    pub slot: usize,
    /// Palette slots whose colour changed.
    pub palette_entries_changed: usize,
    /// Pixels mapped to a nearest palette colour because the image held more
    /// than 16 distinct colours (only non-zero with `quantize`).
    pub quantized_pixels: usize,
    /// Byte offsets inside entry 899 the patch touched.
    pub touched_offsets: Vec<usize>,
}

/// Read the sheet off a disc image.
pub fn read_sheet(patcher: &DiscPatcher) -> Result<SaveIconSheet> {
    let entry = patcher
        .read_entry(PROT_ENTRY as usize)
        .with_context(|| format!("read PROT entry {PROT_ENTRY}"))?;
    save_icon::parse_entry(&entry).context("locate the save-icon sheet in PROT entry 899")
}

/// Decode one slot's portrait to RGBA8 (16x16, row-major).
pub fn export_slot(sheet: &SaveIconSheet, slot: usize) -> Result<Vec<u8>> {
    check_slot(slot)?;
    sheet.tile_rgba(slot)
}

fn check_slot(slot: usize) -> Result<()> {
    if slot >= SLOT_COUNT {
        bail!(
            "save-icon slot {slot} is out of range: only slots 0..{SLOT_COUNT} \
             (save numbers 1..={SLOT_COUNT}) are reachable in game. Tile 15 exists \
             in the strip but nothing ever selects it, so art written there would \
             never be displayed."
        );
    }
    Ok(())
}

/// Encode `rgba` (16x16, row-major) into this tile's 16-entry palette + 4bpp
/// indices, **starting from the tile's existing palette**.
///
/// Keeping the original slot assignments matters for more than tidiness: an
/// unedited portrait re-encodes to the exact bytes already on disc, so a
/// round-trip is a zero-run patch and a real edit's PPF carries only the
/// pixels the user actually changed.
///
/// Colours are matched in the PSX 15-bit space, so two RGBA8 values that
/// round to the same texel share a slot. Colours the original palette lacks
/// take unused slots; with `quantize` off, running out of slots is a hard
/// error naming the count.
fn encode_tile(
    rgba: &[u8],
    original_palette: &[u16; CLUT_ENTRIES_PER_TILE],
    quantize: bool,
) -> Result<([u16; CLUT_ENTRIES_PER_TILE], [u8; TILE_BLOCK_BYTES], usize)> {
    let want = TILE_SIZE * TILE_SIZE * 4;
    if rgba.len() != want {
        bail!(
            "portrait must be {TILE_SIZE}x{TILE_SIZE} RGBA ({want} bytes), got {}",
            rgba.len()
        );
    }
    let texels: Vec<u16> = rgba
        .chunks_exact(4)
        .map(|c| legaia_tim::encode::rgba8_to_bgr555([c[0], c[1], c[2], c[3]]))
        .collect();

    // Distinct colours, most frequent first - frequency decides who keeps a
    // slot when the image needs more than the palette holds.
    let mut counts: Vec<(u16, usize)> = Vec::new();
    for &t in &texels {
        match counts.iter_mut().find(|(c, _)| *c == t) {
            Some((_, n)) => *n += 1,
            None => counts.push((t, 1)),
        }
    }
    counts.sort_by_key(|&(_, n)| std::cmp::Reverse(n));

    let mut palette = *original_palette;
    // Slot assignment: reuse the original index for any colour already in
    // the palette, then hand out slots no reused colour claimed.
    let mut assigned: Vec<(u16, usize)> = Vec::new();
    let mut slot_taken = [false; CLUT_ENTRIES_PER_TILE];
    let mut unplaced: Vec<u16> = Vec::new();
    for &(colour, _) in &counts {
        match original_palette
            .iter()
            .position(|&p| p == colour)
            .filter(|&i| !slot_taken[i])
        {
            Some(i) => {
                slot_taken[i] = true;
                assigned.push((colour, i));
            }
            None => unplaced.push(colour),
        }
    }
    let mut overflow: Vec<u16> = Vec::new();
    for colour in unplaced {
        match (0..CLUT_ENTRIES_PER_TILE).find(|&i| !slot_taken[i]) {
            Some(i) => {
                slot_taken[i] = true;
                palette[i] = colour;
                assigned.push((colour, i));
            }
            None => overflow.push(colour),
        }
    }
    if !overflow.is_empty() && !quantize {
        bail!(
            "portrait holds {} distinct 15-bit colours; a save-icon tile has room for \
             {CLUT_ENTRIES_PER_TILE}. Reduce the palette, or pass --quantize to map the \
             least-used colours to their nearest kept neighbour.",
            counts.len()
        );
    }

    let nearest = |t: u16| -> usize {
        // Distance in the 5-5-5 channel space; exact matches were resolved
        // above, so this only runs for colours with no slot of their own.
        let (tr, tg, tb) = (t & 31, (t >> 5) & 31, (t >> 10) & 31);
        let mut best = 0usize;
        let mut best_d = i32::MAX;
        for &(colour, idx) in &assigned {
            let (kr, kg, kb) = (colour & 31, (colour >> 5) & 31, (colour >> 10) & 31);
            let d = (tr as i32 - kr as i32).pow(2)
                + (tg as i32 - kg as i32).pow(2)
                + (tb as i32 - kb as i32).pow(2);
            if d < best_d {
                best_d = d;
                best = idx;
            }
        }
        best
    };

    let mut pixels = [0u8; TILE_BLOCK_BYTES];
    let mut quantized = 0usize;
    for (i, &t) in texels.iter().enumerate() {
        let idx = match assigned.iter().find(|(c, _)| *c == t) {
            Some(&(_, idx)) => idx,
            None => {
                quantized += 1;
                nearest(t)
            }
        };
        // 4bpp packs two texels per byte, low nibble first.
        if i % 2 == 0 {
            pixels[i / 2] = idx as u8;
        } else {
            pixels[i / 2] |= (idx as u8) << 4;
        }
    }
    Ok((palette, pixels, quantized))
}

/// What a replacement *would* do, without touching the disc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPreview {
    /// The portrait as it will display in game: 16x16 RGBA8 after the PSX
    /// 15-bit rounding and any quantization. What the user sees here is what
    /// the game gets.
    pub rgba: Vec<u8>,
    /// Palette slots whose colour would change.
    pub palette_entries_changed: usize,
    /// Pixels that would be mapped to a nearest palette colour.
    pub quantized_pixels: usize,
}

/// Encode a replacement for `slot` and render it back, writing nothing.
///
/// The browser patcher's side-by-side preview runs on this, so the preview
/// and the write share one encoder and cannot disagree.
pub fn preview_slot(
    sheet: &SaveIconSheet,
    slot: usize,
    rgba: &[u8],
    quantize: bool,
) -> Result<SlotPreview> {
    check_slot(slot)?;
    let before = sheet.tile_clut(slot)?;
    let (palette, pixels, quantized_pixels) = encode_tile(rgba, &before, quantize)?;
    let palette_entries_changed = palette
        .iter()
        .zip(before.iter())
        .filter(|(a, b)| a != b)
        .count();
    let mut out = Vec::with_capacity(TILE_SIZE * TILE_SIZE * 4);
    for byte in pixels.iter() {
        for nib in [byte & 0x0F, byte >> 4] {
            out.extend_from_slice(&legaia_tim::bgr555_to_rgba8(palette[nib as usize]));
        }
    }
    Ok(SlotPreview {
        rgba: out,
        palette_entries_changed,
        quantized_pixels,
    })
}

/// Replace one slot's portrait in place on the disc.
///
/// `rgba` is 16x16 row-major RGBA8. Every write is same-size and goes through
/// [`DiscPatcher`], so touched sectors get EDC/ECC re-encoded and no LBA
/// moves. Only this tile's runs and its 32 palette bytes change.
pub fn replace_slot(
    patcher: &mut DiscPatcher,
    slot: usize,
    rgba: &[u8],
    quantize: bool,
) -> Result<SlotReplaceOutcome> {
    check_slot(slot)?;
    let sheet = read_sheet(patcher)?;
    let before = sheet.tile_clut(slot)?;
    let (palette, pixels, quantized_pixels) = encode_tile(rgba, &before, quantize)?;

    let palette_entries_changed = palette
        .iter()
        .zip(before.iter())
        .filter(|(a, b)| a != b)
        .count();

    let mut touched_offsets = Vec::with_capacity(TILE_SIZE + 1);

    // Palette: 32 contiguous bytes.
    let clut_off = sheet.tile_clut_offset(slot);
    let mut clut_bytes = [0u8; TILE_CLUT_BYTES];
    for (i, e) in palette.iter().enumerate() {
        clut_bytes[i * 2..i * 2 + 2].copy_from_slice(&e.to_le_bytes());
    }
    patcher
        .patch_prot_entry(PROT_ENTRY as usize, clut_off as u64, &clut_bytes)
        .with_context(|| format!("write save-icon slot {slot} palette"))?;
    touched_offsets.push(clut_off);

    // Pixels: 16 scattered 8-byte runs, one per strip row.
    let run = TILE_SIZE / 2;
    for (row, &off) in sheet.tile_pixel_run_offsets(slot).iter().enumerate() {
        patcher
            .patch_prot_entry(
                PROT_ENTRY as usize,
                off as u64,
                &pixels[row * run..(row + 1) * run],
            )
            .with_context(|| format!("write save-icon slot {slot} row {row}"))?;
        touched_offsets.push(off);
    }

    Ok(SlotReplaceOutcome {
        slot,
        palette_entries_changed,
        quantized_pixels,
        touched_offsets,
    })
}

/// Copy one portrait tile onto another, byte-exact - palette and pixels
/// together, so the destination becomes pixel-identical to the source.
///
/// Two disc surfaces carry a hero portrait and both are patched:
///
/// - the strip in PROT entry 899 (the save-UI face draw *and* the VRAM
///   rect the card-block icon writer `FUN_801E1934` grabs), and
/// - for `dst` 0..3, the boot load-screen's standalone portrait TIM in
///   the pre-`init_data` head of `PROT.DAT`
///   ([`legaia_asset::title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET`],
///   indexed by char_id), whose pixels retail keeps byte-identical to
///   strip tiles 0..2.
///
/// Compare-before-write throughout, so re-copying an already-copied
/// tile is a no-op; returns whether anything changed.
pub fn copy_slot_portrait(patcher: &mut DiscPatcher, src: usize, dst: usize) -> Result<bool> {
    check_slot(src)?;
    check_slot(dst)?;
    if src == dst {
        return Ok(false);
    }
    let sheet = read_sheet(patcher)?;
    let pixels = sheet.tile_block_pixels(src)?;
    let clut = sheet.tile_clut_bytes(src)?;
    let mut changed = false;

    if sheet.tile_clut_bytes(dst)? != clut {
        patcher
            .patch_prot_entry(
                PROT_ENTRY as usize,
                sheet.tile_clut_offset(dst) as u64,
                &clut,
            )
            .with_context(|| format!("copy save-icon palette {src} -> {dst}"))?;
        changed = true;
    }
    if sheet.tile_block_pixels(dst)? != pixels {
        let run = TILE_SIZE / 2;
        for (row, &off) in sheet.tile_pixel_run_offsets(dst).iter().enumerate() {
            patcher
                .patch_prot_entry(
                    PROT_ENTRY as usize,
                    off as u64,
                    &pixels[row * run..(row + 1) * run],
                )
                .with_context(|| format!("copy save-icon pixels {src} -> {dst} row {row}"))?;
        }
        changed = true;
    }

    if dst < title_pak::OVERLAY_LOAD_PORTRAIT_COUNT {
        // Standalone TIM: 8-byte header, 12-byte CLUT block header + 32
        // CLUT bytes, 12-byte image block header + 128 pixel bytes.
        const TIM_CLUT_OFF: usize = 8 + 12;
        const TIM_PIX_OFF: usize = TIM_CLUT_OFF + TILE_CLUT_BYTES + 12;
        let base = title_pak::OVERLAY_LOAD_PORTRAIT_TIM_OFFSET
            + dst * title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE;
        let tim = patcher.read_prot_bytes(base as u64, title_pak::OVERLAY_LOAD_PORTRAIT_STRIDE)?;
        anyhow::ensure!(
            tim[..8] == [0x10, 0, 0, 0, 0x08, 0, 0, 0],
            "standalone portrait TIM {dst}: unexpected header {:02X?}",
            &tim[..8]
        );
        if tim[TIM_CLUT_OFF..TIM_CLUT_OFF + TILE_CLUT_BYTES] != clut {
            patcher
                .patch_named_file("PROT.DAT", (base + TIM_CLUT_OFF) as u64, &clut)
                .with_context(|| format!("copy standalone portrait palette -> char {dst}"))?;
            changed = true;
        }
        if tim[TIM_PIX_OFF..TIM_PIX_OFF + TILE_BLOCK_BYTES] != pixels {
            patcher
                .patch_named_file("PROT.DAT", (base + TIM_PIX_OFF) as u64, &pixels)
                .with_context(|| format!("copy standalone portrait pixels -> char {dst}"))?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(rgba: [u8; 4]) -> Vec<u8> {
        rgba.iter()
            .copied()
            .cycle()
            .take(TILE_SIZE * TILE_SIZE * 4)
            .collect()
    }

    #[test]
    fn save_icon_slot_bounds_reject_the_blank_tile() {
        assert!(check_slot(0).is_ok());
        assert!(check_slot(SLOT_COUNT - 1).is_ok());
        let err = check_slot(SLOT_COUNT).unwrap_err().to_string();
        assert!(err.contains("never be displayed"), "{err}");
        assert!(check_slot(99).is_err());
    }

    /// A stand-in for a tile's existing palette: 16 distinct colours.
    fn base_palette() -> [u16; CLUT_ENTRIES_PER_TILE] {
        std::array::from_fn(|i| ((i as u16 + 1) << 5) | 0x0001)
    }

    /// Decode a packed 4bpp tile back to its per-pixel indices.
    fn indices(pixels: &[u8; TILE_BLOCK_BYTES]) -> Vec<u8> {
        pixels.iter().flat_map(|&b| [b & 0x0F, b >> 4]).collect()
    }

    #[test]
    fn save_icon_encode_tile_reuses_the_slot_a_colour_already_has() {
        let pal = base_palette();
        // Paint the whole tile in the colour that already sits in slot 7.
        let rgba: Vec<u8> = legaia_tim::bgr555_to_rgba8(pal[7])
            .iter()
            .copied()
            .cycle()
            .take(TILE_SIZE * TILE_SIZE * 4)
            .collect();
        let (palette, pixels, q) = encode_tile(&rgba, &pal, false).unwrap();
        assert_eq!(q, 0);
        // The palette is untouched, and every pixel points at slot 7 - so an
        // unedited portrait re-encodes to the bytes already on disc.
        assert_eq!(palette, pal);
        assert!(indices(&pixels).iter().all(|&i| i == 7));
    }

    #[test]
    fn save_icon_encode_tile_places_new_colours_in_free_slots_only() {
        let pal = base_palette();
        // Two colours: one already in slot 3, one brand new.
        let known = legaia_tim::bgr555_to_rgba8(pal[3]);
        let mut rgba = solid(known);
        for i in 0..4 {
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        let (palette, pixels, q) = encode_tile(&rgba, &pal, false).unwrap();
        assert_eq!(q, 0);
        assert_eq!(palette[3], pal[3], "the reused slot keeps its colour");
        let idx = indices(&pixels);
        assert_eq!(idx[4], 3, "known colour keeps slot 3");
        let new_slot = idx[0] as usize;
        assert_ne!(new_slot, 3);
        assert_eq!(
            palette[new_slot],
            legaia_tim::encode::rgba8_to_bgr555([255, 255, 255, 255])
        );
        // Exactly one palette slot moved.
        let changed = palette
            .iter()
            .zip(pal.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(changed, 1);
    }

    #[test]
    fn save_icon_encode_tile_rejects_palette_overflow_without_quantize() {
        // 17 distinct 15-bit colours, none of them in the base palette.
        let mut rgba = solid([0, 0, 0, 255]);
        for i in 0..17 {
            rgba[i * 4] = (i as u8 + 1) << 3;
        }
        let pal = base_palette();
        let err = encode_tile(&rgba, &pal, false).unwrap_err().to_string();
        assert!(err.contains("distinct 15-bit colours"), "{err}");
        // With quantize the extras collapse onto their nearest neighbours.
        let (_, _, q) = encode_tile(&rgba, &pal, true).unwrap();
        assert!(q > 0, "quantize should report approximated pixels");
    }

    #[test]
    fn save_icon_encode_tile_checks_dimensions() {
        assert!(encode_tile(&[0u8; 16], &base_palette(), false).is_err());
    }
}
