//! Monster texture-page replacement - repaint an enemy's battle skin inside
//! its `battle_data` archive slot (PROT entry 867).
//!
//! Every enemy, boss and their Ra-Seru wears one 4bpp page out of its own
//! LZS-compressed monster block. No TIM tool on this disc could reach one:
//! the pool is bare `[15 x 16 BGR555][w*h/2 bytes of 4bpp]` with no magic
//! word and no geometry (the loader's `StoreImage` rect supplies that), so
//! the raw TIM catalog finds nothing in entry 867 and the compressed catalog
//! finds a single 64x64 effect texture. Layout and provenance in
//! [`legaia_asset::monster_archive`].
//!
//! ## The palettes are not ours to rewrite
//!
//! A monster's CLUT region is uploaded to VRAM **verbatim**, so the `0x8000`
//! bit stored in an entry is live semi-transparency state that the GPU
//! samples - not, as in the player battle art, a bit the loader re-derives.
//! Measured across the retail archive: of 27,097 non-zero entries only 996
//! carry bit 15, and 151 of those are exactly `0x8000` (opaque black). An
//! RGBA image cannot express that bit, so re-deriving a palette from one
//! would silently change which texels blend.
//!
//! This writer therefore **never touches a palette**. It re-indexes each
//! texel within the colours its own palette already holds, exactly or - with
//! `quantize` - through the nearest kept colour. A repaint is an arrangement
//! of the page's existing colours, and it can never break the colouring of a
//! region it did not touch.
//!
//! ## One page, many palettes
//!
//! Which palette a texel is read through is a property of the primitive that
//! samples it ([`MonsterPage::ownership`]), so this module encodes per texel
//! against its owner rather than against one page-wide palette. Texels no
//! primitive samples are dead bytes: they are left exactly as retail wrote
//! them, and edits to them are reported rather than applied.
//!
//! ## The fit budget is the archive slot
//!
//! A block is one LZS stream inside a fixed `0x14000`-byte slot, so an edit
//! must recompress into `SLOT_STRIDE - 4`. [`replace_page`] measures that
//! before writing anything and fails with the overage rather than truncating.

use anyhow::{Context, Result, bail};

use legaia_asset::monster_archive::{
    self as archive, CLUT_COUNT, MonsterPage, PALETTE_COLOURS, SLOT_STRIDE,
};

use crate::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

/// Bytes a monster slot allocates for its LZS stream: the slot past the
/// `u32 dec_size` prefix.
pub const SLOT_STREAM_CAPACITY: usize = SLOT_STRIDE - 4;

/// Which monster's page to read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterTextureTarget {
    /// 1-based monster id, the archive slot index + 1.
    pub id: u16,
}

impl std::fmt::Display for MonsterTextureTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "monster id {}", self.id)
    }
}

/// The whole archive entry, read at its true on-disc footprint (a retail LZS
/// stream may run past its own slot, and the decoder stops on output count).
fn archive_entry(patcher: &DiscPatcher) -> Result<Vec<u8>> {
    patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .context("read the monster archive (PROT entry 867)")
}

/// Every populated monster's texture page, in id order.
pub fn catalog(patcher: &DiscPatcher) -> Result<Vec<MonsterPage>> {
    let entry = archive_entry(patcher)?;
    archive::pages(&entry)
}

/// Read one monster's page off the current (possibly already patched) image.
pub fn read_page(patcher: &DiscPatcher, target: &MonsterTextureTarget) -> Result<MonsterPage> {
    let entry = archive_entry(patcher)?;
    archive::page(&entry, target.id)?.with_context(|| {
        format!(
            "{target} has no texture page (empty / filler slot - monster-stats lists the \
             populated ids)"
        )
    })
}

/// A decoded page, ready to write as a PNG.
#[derive(Debug, Clone)]
pub struct ExportedPage {
    pub id: u16,
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// Composite decode: every texel through the palette of the primitive
    /// that samples it, dead texels transparent.
    pub rgba: Vec<u8>,
    /// Palettes the pool carries colours in.
    pub palettes_populated: usize,
}

/// Decode one monster's page as the game colours it.
pub fn export_page(patcher: &DiscPatcher, target: &MonsterTextureTarget) -> Result<ExportedPage> {
    let page = read_page(patcher, target)?;
    let owner = page.ownership();
    Ok(ExportedPage {
        id: page.id,
        name: page.name.clone(),
        width: page.width(),
        height: page.height(),
        rgba: page.rgba(&owner),
        palettes_populated: page.palettes_populated(),
    })
}

/// Recompression fit numbers for a monster-page replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonsterFit {
    /// Bytes the archive slot allocates for the block's LZS stream.
    pub capacity: usize,
    /// Bytes the retail stream consumes inside that allocation.
    pub retail: usize,
    /// Bytes the edited block recompressed to.
    pub recompressed: usize,
}

/// What a replacement did (or, for a preview, would do).
#[derive(Debug, Clone)]
pub struct MonsterReplaceOutcome {
    pub id: u16,
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// Texels whose index changed.
    pub texels_changed: usize,
    /// Texels folded onto the nearest colour their own palette holds.
    pub quantized_texels: usize,
    /// Texels no primitive samples. Edits to these are not written - nothing
    /// on the model reads them, and leaving retail's bytes in place keeps the
    /// block compressing the way retail's did.
    pub dead_texels_ignored: usize,
    /// The edited block came out byte-identical, so nothing was written.
    pub unchanged: bool,
    pub fit: MonsterFit,
}

/// A validated-but-unwritten replacement, with the page as it will display.
#[derive(Debug, Clone)]
pub struct MonsterPreview {
    pub width: usize,
    pub height: usize,
    /// The replacement as the runtime will sample it - every texel decoded
    /// through the palette that reads it, dead texels transparent.
    pub rgba: Vec<u8>,
    pub palettes_populated: usize,
    pub texels_changed: usize,
    pub quantized_texels: usize,
    pub dead_texels_ignored: usize,
    pub unchanged: bool,
    pub fit: MonsterFit,
}

/// An edit resolved against the disc but not yet committed. Shared by the
/// preview and the write so a preview cannot disagree with what it previews.
struct Edited {
    page: MonsterPage,
    /// The new 4bpp index per texel.
    indices: Vec<u8>,
    texels_changed: usize,
    quantized_texels: usize,
    dead_texels_ignored: usize,
    /// The whole decoded block with the new pixels spliced in.
    block: Vec<u8>,
    unchanged: bool,
    /// Per-texel palette owner, kept for the preview's decode.
    owner: Vec<Option<u8>>,
}

/// A palette's colours in the frame an RGBA image can express: the stored
/// entry with its semi-transparency bit masked off, and `None` for the
/// transparent entry.
fn matchable(entry: u16) -> Option<u16> {
    (entry != 0).then_some(entry & 0x7FFF)
}

/// Choose the index in `palette` that best represents `texel`, given the
/// index that texel already holds.
///
/// The incumbent wins whenever it already produces the requested colour, and
/// that is not a micro-optimisation: a retail palette repeats colours across
/// slots, so "first slot holding this colour" would re-index hundreds of
/// unedited texels and turn a round-trip into a real patch. Songi #179 alone
/// moved 962 texels that way before this rule.
///
/// Otherwise: an exact match, or - only when the caller asked for it - the
/// nearest colour by squared distance in 5-bit space.
fn index_for(
    palette: &[u16; PALETTE_COLOURS],
    current: u8,
    texel: [u8; 4],
    quantize: bool,
) -> Result<(usize, bool), ()> {
    let incumbent = palette.get(current as usize).copied();
    if texel[3] == 0 {
        // Fully transparent: the entry that IS transparent, if this palette
        // has one. Every retail palette does.
        if incumbent == Some(0) {
            return Ok((current as usize, false));
        }
        if let Some(i) = palette.iter().position(|&e| e == 0) {
            return Ok((i, false));
        }
    }
    let want = legaia_tim::encode::rgba8_to_bgr555(texel) & 0x7FFF;
    if texel[3] != 0 && incumbent.and_then(matchable) == Some(want) {
        return Ok((current as usize, false));
    }
    if let Some(i) = palette
        .iter()
        .position(|&e| matchable(e) == Some(want) && texel[3] != 0)
    {
        return Ok((i, false));
    }
    if !quantize {
        return Err(());
    }
    let (wr, wg, wb) = (want & 31, (want >> 5) & 31, (want >> 10) & 31);
    let mut best = None::<(usize, i32)>;
    for (i, &e) in palette.iter().enumerate() {
        let Some(c) = matchable(e) else { continue };
        let (r, g, b) = (c & 31, (c >> 5) & 31, (c >> 10) & 31);
        let d = (wr as i32 - r as i32).pow(2)
            + (wg as i32 - g as i32).pow(2)
            + (wb as i32 - b as i32).pow(2);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    match best {
        Some((i, _)) => Ok((i, true)),
        // A palette of nothing but transparent entries: index 0 is as good
        // an answer as exists, and it is what retail stores there.
        None => Ok((0, true)),
    }
}

fn edit_page(
    page: MonsterPage,
    rgba: &[u8],
    width: usize,
    height: usize,
    quantize: bool,
) -> Result<Edited> {
    if (width, height) != (page.width(), page.height()) {
        bail!(
            "monster id {} is {}x{}; the replacement image is {width}x{height}",
            page.id,
            page.width(),
            page.height()
        );
    }
    let want = width * height * 4;
    if rgba.len() != want {
        bail!(
            "image must be {width}x{height} RGBA ({want} bytes), got {}",
            rgba.len()
        );
    }

    let owner = page.ownership();
    let palettes: Vec<[u16; PALETTE_COLOURS]> = (0..CLUT_COUNT)
        .map(|p| page.palette_raw(p).unwrap_or([0u16; PALETTE_COLOURS]))
        .collect();

    let mut indices = page.texture.indices.clone();
    let mut texels_changed = 0usize;
    let mut quantized_texels = 0usize;
    let mut dead_texels_ignored = 0usize;
    let mut unmatched = 0usize;
    let mut first_unmatched = None::<(usize, usize)>;

    for (i, slot) in indices.iter_mut().enumerate() {
        let Some(p) = owner.get(i).copied().flatten() else {
            // A dead texel. Report an edit to it, never apply one.
            if rgba[i * 4 + 3] != 0 {
                dead_texels_ignored += 1;
            }
            continue;
        };
        let texel = [
            rgba[i * 4],
            rgba[i * 4 + 1],
            rgba[i * 4 + 2],
            rgba[i * 4 + 3],
        ];
        match index_for(&palettes[p as usize], *slot, texel, quantize) {
            Ok((idx, folded)) => {
                if folded {
                    quantized_texels += 1;
                }
                if *slot != idx as u8 {
                    texels_changed += 1;
                    *slot = idx as u8;
                }
            }
            Err(()) => {
                unmatched += 1;
                if first_unmatched.is_none() {
                    first_unmatched = Some((i % width, i / width));
                }
            }
        }
    }

    if unmatched > 0 {
        let (x, y) = first_unmatched.unwrap_or((0, 0));
        bail!(
            "{unmatched} pixel(s) use a colour the monster's own palettes do not hold (first at \
             {x},{y}). This family never rewrites a palette - a monster's CLUTs upload to VRAM \
             verbatim, so their semi-transparency bits are live state an RGBA image cannot \
             express. Repaint using the colours already in the exported page, or ask for \
             quantize to fold each stray colour onto the nearest one its own region holds."
        );
    }

    // Splice the packed 4bpp page back into the decoded block. Low nibble
    // first, exactly the order the decode read it.
    let mut block = page.block.clone();
    let pix = page.pixels_offset();
    for (i, chunk) in indices.chunks_exact(2).enumerate() {
        let byte = (chunk[0] & 0x0F) | (chunk[1] << 4);
        let at = pix + i;
        if at >= block.len() {
            bail!("monster id {} page runs past its block", page.id);
        }
        block[at] = byte;
    }
    let unchanged = block == page.block;

    Ok(Edited {
        page,
        indices,
        texels_changed,
        quantized_texels,
        dead_texels_ignored,
        block,
        unchanged,
        owner,
    })
}

/// Bytes the retail stream consumes inside the slot.
fn retail_stream_len(page: &MonsterPage, patcher: &DiscPatcher) -> Result<usize> {
    let slot = patcher.monster_slot(page.id)?;
    let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
    let (_, consumed) = legaia_lzs::decompress_tracked(&slot[4..], declared)
        .with_context(|| format!("measure monster id {}'s retail stream", page.id))?;
    Ok(consumed)
}

/// Recompress an edited block and measure it against the slot allocation.
/// `Ok` only when it fits - a monster slot is fixed stride, so an overage is
/// an error carrying its own size and nothing is written.
fn recompress(e: &Edited, retail: usize) -> Result<(Vec<u8>, MonsterFit)> {
    if e.unchanged {
        return Ok((
            Vec::new(),
            MonsterFit {
                capacity: SLOT_STREAM_CAPACITY,
                retail,
                recompressed: retail,
            },
        ));
    }
    // Retail's encoder is not ours, so try both and keep the smaller.
    let greedy = legaia_lzs::compress(&e.block);
    let optimal = legaia_lzs::compress_optimal(&e.block);
    let stream = if greedy.len() <= optimal.len() {
        greedy
    } else {
        optimal
    };
    let fit = MonsterFit {
        capacity: SLOT_STREAM_CAPACITY,
        retail,
        recompressed: stream.len(),
    };
    if stream.len() > SLOT_STREAM_CAPACITY {
        bail!(
            "the edited monster block recompresses to {} bytes but its archive slot allocates \
             only {} ({} over). Every monster sits at a fixed {SLOT_STRIDE:#X} stride, so this \
             cannot grow. Try an image with more flat regions.",
            stream.len(),
            SLOT_STREAM_CAPACITY,
            stream.len() - SLOT_STREAM_CAPACITY
        );
    }
    Ok((stream, fit))
}

/// Validate a replacement and render it as it will display, without writing.
///
/// The same encode and the same fit measurement [`replace_page`] performs,
/// stopped before the patch.
pub fn preview_page(
    patcher: &DiscPatcher,
    target: &MonsterTextureTarget,
    rgba: &[u8],
    width: usize,
    height: usize,
    quantize: bool,
) -> Result<MonsterPreview> {
    let page = read_page(patcher, target)?;
    let retail = retail_stream_len(&page, patcher)?;
    let e = edit_page(page, rgba, width, height, quantize)?;
    let (_, fit) = recompress(&e, retail)?;
    // Decode what was just encoded, through the same ownership map - so the
    // preview is the write's own result, not a second opinion about it.
    let mut shown = e.page.clone();
    shown.texture.indices = e.indices.clone();
    Ok(MonsterPreview {
        width,
        height,
        rgba: shown.rgba(&e.owner),
        palettes_populated: e.page.palettes_populated(),
        texels_changed: e.texels_changed,
        quantized_texels: e.quantized_texels,
        dead_texels_ignored: e.dead_texels_ignored,
        unchanged: e.unchanged,
        fit,
    })
}

/// Replace one monster's texture page, in place.
///
/// The whole block is re-LZS'd and written back into its own `0x14000` slot.
/// A block that no longer fits errors with the overage and writes nothing.
pub fn replace_page(
    patcher: &mut DiscPatcher,
    target: &MonsterTextureTarget,
    rgba: &[u8],
    width: usize,
    height: usize,
    quantize: bool,
    dry_run: bool,
) -> Result<MonsterReplaceOutcome> {
    let page = read_page(patcher, target)?;
    let retail = retail_stream_len(&page, patcher)?;
    let e = edit_page(page, rgba, width, height, quantize)?;
    let (stream, fit) = recompress(&e, retail)?;

    if !e.unchanged && !dry_run {
        let mut slot = Vec::with_capacity(SLOT_STRIDE);
        slot.extend_from_slice(&(e.block.len() as u32).to_le_bytes());
        slot.extend_from_slice(&stream);
        slot.resize(SLOT_STRIDE, 0);
        patcher
            .patch_monster_slot(target.id, &slot)
            .with_context(|| format!("write {target}"))?;
    }

    Ok(MonsterReplaceOutcome {
        id: e.page.id,
        name: e.page.name.clone(),
        width,
        height,
        texels_changed: e.texels_changed,
        quantized_texels: e.quantized_texels,
        dead_texels_ignored: e.dead_texels_ignored,
        unchanged: e.unchanged,
        fit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(colours: &[u16]) -> [u16; PALETTE_COLOURS] {
        let mut p = [0u16; PALETTE_COLOURS];
        p[..colours.len()].copy_from_slice(colours);
        p
    }

    #[test]
    fn an_exact_colour_keeps_the_slot_it_already_has() {
        // 0 transparent, then red, green, blue in BGR555.
        let p = palette(&[0, 0x001F, 0x03E0, 0x7C00]);
        let red = legaia_tim::bgr555_to_rgba8(0x001F);
        assert_eq!(index_for(&p, 0, red, false), Ok((1, false)));
        let green = legaia_tim::bgr555_to_rgba8(0x03E0);
        assert_eq!(index_for(&p, 0, green, false), Ok((2, false)));
    }

    #[test]
    fn transparent_takes_the_transparent_entry_not_a_colour() {
        let p = palette(&[0, 0x001F]);
        assert_eq!(index_for(&p, 1, [0, 0, 0, 0], false), Ok((0, false)));
        // Opaque black is a colour, and this palette does not hold it.
        assert!(index_for(&p, 0, [0, 0, 0, 255], false).is_err());
    }

    #[test]
    fn a_stray_colour_is_refused_unless_quantize_was_asked_for() {
        let p = palette(&[0, 0x001F, 0x7C00]);
        let orange = [255u8, 128, 0, 255];
        assert!(index_for(&p, 0, orange, false).is_err());
        let (idx, folded) = index_for(&p, 0, orange, true).unwrap();
        assert!(folded);
        assert_eq!(idx, 1, "orange folds onto red, not blue");
    }

    #[test]
    fn the_semi_transparency_bit_does_not_hide_a_colour() {
        // A palette entry stored with bit 15 set is the same colour to an
        // RGBA image; it must still match, because this writer may not
        // rewrite the entry to clear the bit.
        let p = palette(&[0, 0x801F]);
        let red = legaia_tim::bgr555_to_rgba8(0x001F);
        assert_eq!(index_for(&p, 0, red, false), Ok((1, false)));
    }

    #[test]
    fn target_names_the_monster_id() {
        assert_eq!(
            MonsterTextureTarget { id: 179 }.to_string(),
            "monster id 179"
        );
    }
}
