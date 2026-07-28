//! Battle-texture replacement - swap the character art inside the player
//! battle files (`data\battle\PLAYER1..4`, PROT entries 863..866).
//!
//! This is the party's in-battle skin: body, face, weapon and armour art,
//! one block per equipment variant. Roughly 1.3 MiB of 4bpp pixels that no
//! texture tool on this disc could reach before, because of one fact:
//!
//! ## Why this is not `tim-replace` on entry 864
//!
//! **The blocks are not TIMs.** There is no `0x10` magic, no flag word and
//! no per-half block header - the geometry comes from the *loader's* static
//! rect table, not from the bytes:
//!
//! ```text
//!   [u16 clut_x][u16 clut_n][clut_n x u16 BGR555][w*h halfwords of 4bpp]
//! ```
//!
//! So the raw tier's magic scan cannot see them, the LZS tier's magic scan
//! cannot see them either (they *are* LZS-compressed - the magic is simply
//! not there to find), and `legaia_tim::parse_strict` has nothing to parse.
//! Format detail in [`legaia_asset::battle_texture_catalog`] and
//! [`docs/formats/battle-data-pack.md`](../../../docs/formats/battle-data-pack.md).
//!
//! ## The fit budget is a slot allocation, not a stream length
//!
//! Unlike the LZS texture tier, a player file's records are addressed by a
//! descriptor table whose chain invariant `offset[i+1] == offset[i] +
//! size[i]` pins every later record in place. So a replacement must
//! recompress into **this record's own slot footprint** (`size - 4`, past
//! the `dec_size` prefix) - nothing downstream may move. Retail leaves a
//! median of a few hundred spare bytes per slot and as little as 2, so a
//! detailed repaint can genuinely fail to fit; [`replace_block`] says by
//! how much rather than corrupting the next record.
//!
//! ## One palette at a time
//!
//! Most blocks ship 2 or 3 sixteen-colour palettes in a single CLUT run,
//! and a mesh primitive's CBA column picks which one it samples - the same
//! index data rendered in alternate colours. A replacement therefore
//! rewrites the pixels plus **only the palette it was exported through**,
//! leaving the sibling palettes byte-identical so their variants keep
//! working.

use anyhow::{Context, Result, bail};

use crate::disc::DiscPatcher;
use legaia_asset::battle_char_assembly::CLUT_ENTRIES_PER_PALETTE;
use legaia_asset::battle_texture_catalog::{
    self as catalog, BattleTextureBlock, BattleTextureSlot, ResolvedBlock,
};
use legaia_asset::item_names::ItemNameTable;

/// The PROT entries that carry a player battle file.
pub const PLAYER_FILE_ENTRIES: [u32; 4] = catalog::PLAYER_FILE_ENTRIES;

/// Where the target block lives: a player-file PROT entry plus the slot
/// selector [`catalog`] rows print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleTextureTarget {
    /// Owning PROT entry (863..866 in retail).
    pub entry: u32,
    /// Which block of that file.
    pub slot: BattleTextureSlot,
}

impl std::fmt::Display for BattleTextureTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "entry {} battle-slot {}", self.entry, self.slot)
    }
}

/// Build the battle-texture catalog from the patcher's current image.
///
/// The equipment ids in a player file are item ids, so the disc's own
/// `SCUS_942.54` name table is read alongside to label each row with the
/// equipment its art belongs to ("Noa - Ra-Seru Terra $8"). A disc without
/// a readable executable still catalogs; the labels just fall back to ids.
pub fn catalog(patcher: &DiscPatcher) -> Result<Vec<BattleTextureBlock>> {
    let prot = patcher
        .read_named_file("PROT.DAT")
        .context("PROT.DAT not found in disc image")?;
    let spans = patcher.entry_spans();
    let names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| ItemNameTable::from_scus(&scus));
    Ok(catalog::build_from_spans_with_names(
        &prot,
        &spans,
        names.as_ref(),
    ))
}

/// Read one block off the current (possibly already patched) image.
pub fn read_block(patcher: &DiscPatcher, target: &BattleTextureTarget) -> Result<ResolvedBlock> {
    let file = patcher
        .read_entry(target.entry as usize)
        .with_context(|| format!("read PROT entry {}", target.entry))?;
    catalog::resolve_block(&file, target.slot, 0).with_context(|| format!("resolve {target}"))
}

/// Which palette a decode used, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteSource {
    /// One of the block's own palettes, by index into its CLUT run.
    Block(usize),
    /// A 16-entry window of the CLUT row the whole player file assembles.
    /// The only option for a block that ships `clut_n = 0`.
    AssembledRow(usize),
}

impl std::fmt::Display for PaletteSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block(i) => write!(f, "block palette {i}"),
            Self::AssembledRow(i) => write!(f, "assembled-row palette {i}"),
        }
    }
}

/// A decoded block, ready to write as a PNG.
#[derive(Debug, Clone)]
pub struct ExportedBlock {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    /// Palettes the block carries of its own (0 for a pixel-only block).
    pub palette_count: usize,
    pub palette: PaletteSource,
}

/// Resolve which 16 entries to decode a block with.
///
/// A block that ships palettes of its own is decoded block-locally, which
/// is the frame its CBA column addresses. A block with `clut_n = 0` has
/// nothing local, so `palette` indexes the CLUT row the file assembles
/// instead - and the returned [`PaletteSource`] says which happened.
fn palette_for(
    patcher: &DiscPatcher,
    target: &BattleTextureTarget,
    block: &ResolvedBlock,
    palette: usize,
) -> Result<(Vec<u16>, PaletteSource)> {
    if block.upload.palette_count() > 0 {
        let pal = block.upload.palette(palette)?.to_vec();
        return Ok((pal, PaletteSource::Block(palette)));
    }
    let file = patcher
        .read_entry(target.entry as usize)
        .with_context(|| format!("read PROT entry {}", target.entry))?;
    let row = catalog::assemble_clut_row(&file)?;
    let start = palette * CLUT_ENTRIES_PER_PALETTE;
    let window = row
        .get(start..start + CLUT_ENTRIES_PER_PALETTE)
        .with_context(|| {
            format!(
                "assembled-row palette {palette} out of range (the row holds {})",
                row.len() / CLUT_ENTRIES_PER_PALETTE
            )
        })?
        .to_vec();
    Ok((window, PaletteSource::AssembledRow(palette)))
}

/// Decode one block to RGBA8 through `palette`.
pub fn export_block(
    patcher: &DiscPatcher,
    target: &BattleTextureTarget,
    palette: usize,
) -> Result<ExportedBlock> {
    let block = read_block(patcher, target)?;
    let (pal, source) = palette_for(patcher, target, &block, palette)?;
    let rgba = block
        .upload
        .rgba_with_palette(&pal)
        .with_context(|| format!("decode {target} through {source}"))?;
    Ok(ExportedBlock {
        width: block.upload.pixel_width(),
        height: block.upload.pixel_height(),
        rgba,
        palette_count: block.upload.palette_count(),
        palette: source,
    })
}

/// Normalise a CLUT entry into the frame the runtime actually samples:
/// `FUN_80053B9C` ORs the STP bit onto every non-zero entry as it uploads,
/// so `0x1234` and `0x9234` are the same colour and only `0x0000` is
/// transparent. Comparing in this frame is what stops a re-encode from
/// reporting sixteen changed entries when nothing changed.
fn visible(entry: u16) -> u16 {
    if entry == 0 { 0 } else { entry | 0x8000 }
}

/// The form retail stores an entry in: STP cleared, because the runtime
/// re-applies it. (Measured across all four player files: of the non-zero
/// stored entries the overwhelming majority have bit 15 clear.) Opaque
/// black is the one exception - it must keep `0x8000`, or it would store
/// as `0x0000` and read back transparent.
fn stored_form(visible_entry: u16) -> u16 {
    if visible_entry == 0x8000 {
        0x8000
    } else {
        visible_entry & 0x7FFF
    }
}

/// Encode `rgba` into `palette`'s 16 slots plus packed 4bpp indices,
/// starting from the palette already on disc. Both `original_palette` and
/// the returned palette are in the [`visible`] frame.
///
/// Keeping the existing slot assignments is what makes an unedited export
/// re-encode to the bytes already there: a round-trip is a zero-run patch,
/// and a real edit recompresses about as well as retail did (which matters
/// a great deal here - see the fit budget in the module docs).
fn encode_pixels(
    rgba: &[u8],
    width: usize,
    height: usize,
    original_palette: &[u16],
    quantize: bool,
) -> Result<([u16; CLUT_ENTRIES_PER_PALETTE], Vec<u8>, usize)> {
    let want = width * height * 4;
    if rgba.len() != want {
        bail!(
            "image must be {width}x{height} RGBA ({want} bytes), got {}",
            rgba.len()
        );
    }
    let texels: Vec<u16> = rgba
        .chunks_exact(4)
        .map(|c| {
            // `rgba8_to_bgr555` already sends alpha 0 to 0x0000 and opaque
            // black to 0x8000; the normalisation puts everything else in
            // the same frame the runtime uploads into.
            visible(legaia_tim::encode::rgba8_to_bgr555([
                c[0], c[1], c[2], c[3],
            ]))
        })
        .collect();

    let mut counts: Vec<(u16, usize)> = Vec::new();
    for &t in &texels {
        match counts.iter_mut().find(|(c, _)| *c == t) {
            Some((_, n)) => *n += 1,
            None => counts.push((t, 1)),
        }
    }
    counts.sort_by_key(|&(_, n)| std::cmp::Reverse(n));

    let mut palette = [0u16; CLUT_ENTRIES_PER_PALETTE];
    palette.copy_from_slice(&original_palette[..CLUT_ENTRIES_PER_PALETTE]);
    let mut assigned: Vec<(u16, usize)> = Vec::new();
    let mut slot_taken = [false; CLUT_ENTRIES_PER_PALETTE];
    let mut unplaced: Vec<u16> = Vec::new();
    for &(colour, _) in &counts {
        match palette
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
    let mut overflow = 0usize;
    for colour in unplaced {
        match (0..CLUT_ENTRIES_PER_PALETTE).find(|&i| !slot_taken[i]) {
            Some(i) => {
                slot_taken[i] = true;
                palette[i] = colour;
                assigned.push((colour, i));
            }
            None => overflow += 1,
        }
    }
    if overflow > 0 && !quantize {
        bail!(
            "image holds {} distinct 15-bit colours; a battle-texture palette has room for \
             {CLUT_ENTRIES_PER_PALETTE}. Reduce the palette, or pass --quantize to map the \
             least-used colours to their nearest kept neighbour.",
            counts.len()
        );
    }

    let nearest = |t: u16| -> usize {
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

    // Row stride is the rect's halfword width, i.e. exactly width/2 bytes.
    let mut pixels = vec![0u8; width * height / 2];
    let mut quantized = 0usize;
    for (i, &t) in texels.iter().enumerate() {
        let idx = match assigned.iter().find(|(c, _)| *c == t) {
            Some(&(_, idx)) => idx,
            None => {
                quantized += 1;
                nearest(t)
            }
        };
        if i % 2 == 0 {
            pixels[i / 2] = idx as u8;
        } else {
            pixels[i / 2] |= (idx as u8) << 4;
        }
    }
    Ok((palette, pixels, quantized))
}

/// Decode a palette-plus-indices pair the way the runtime samples it: low
/// nibble first, row-major, `0x0000` transparent.
fn pixels_to_rgba(pixels: &[u8], palette: &[u16; CLUT_ENTRIES_PER_PALETTE]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 8);
    for byte in pixels {
        for nib in [byte & 0x0F, byte >> 4] {
            out.extend_from_slice(&legaia_tim::bgr555_to_rgba8(palette[nib as usize]));
        }
    }
    out
}

/// Recompression fit numbers for a battle-texture replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleFit {
    /// Bytes the record's slot allocates for its LZS stream (`size - 4`).
    pub capacity: usize,
    /// Bytes the retail stream occupies inside that allocation.
    pub retail: usize,
    /// Bytes the edited record recompressed to.
    pub recompressed: usize,
}

/// What a replacement did (or, for a dry run, would do).
#[derive(Debug, Clone)]
pub struct BattleReplaceOutcome {
    pub width: usize,
    pub height: usize,
    /// Palette the pixels were encoded against.
    pub palette: PaletteSource,
    /// Palette slots whose colour changed.
    pub palette_entries_changed: usize,
    /// Pixels folded onto a nearest kept colour.
    pub quantized_pixels: usize,
    /// The edited record came out byte-identical to the one on disc, so
    /// nothing was written. Worth reporting rather than silently
    /// recompressing: our LZS encoder is not retail's, so re-emitting an
    /// unchanged record would still move disc bytes and bloat a PPF for no
    /// visible effect.
    pub unchanged: bool,
    pub fit: BattleFit,
}

/// An edit resolved against the disc but not yet committed: the record with
/// the new block spliced in, plus everything both the preview and the write
/// need to report. Shared so a preview cannot disagree with the write it
/// previews - they are the same encode.
struct Edited {
    block: ResolvedBlock,
    source: PaletteSource,
    new_palette: [u16; CLUT_ENTRIES_PER_PALETTE],
    pixels: Vec<u8>,
    quantized_pixels: usize,
    palette_entries_changed: usize,
    /// The whole decoded record with the edited block spliced in.
    decoded: Vec<u8>,
    /// The spliced record came out identical to the one on disc.
    unchanged: bool,
}

fn edit_block(
    patcher: &DiscPatcher,
    target: &BattleTextureTarget,
    rgba: &[u8],
    width: usize,
    height: usize,
    palette: usize,
    quantize: bool,
) -> Result<Edited> {
    let block = read_block(patcher, target)?;
    if (width, height) != (block.upload.pixel_width(), block.upload.pixel_height()) {
        bail!(
            "{target} is {}x{}; the replacement image is {width}x{height}",
            block.upload.pixel_width(),
            block.upload.pixel_height()
        );
    }
    let (original_palette, source) = palette_for(patcher, target, &block, palette)?;
    let (new_palette, pixels, quantized_pixels) =
        encode_pixels(rgba, width, height, &original_palette, quantize)?;
    let palette_entries_changed = new_palette
        .iter()
        .zip(original_palette.iter())
        .filter(|(a, b)| a != b)
        .count();

    // Splice the edited block back into its decoded record: the palette
    // half (only when the block owns one - a borrowed row palette lives in
    // a different record and is left alone) then the pixel half.
    let mut decoded = block.decoded.clone();
    let clut_bytes = block.upload.clut.len() * 2;
    if let PaletteSource::Block(idx) = source {
        let off = block.pool_offset + 4 + idx * CLUT_ENTRIES_PER_PALETTE * 2;
        for (i, e) in new_palette.iter().enumerate() {
            // A slot whose colour did not change keeps its bytes verbatim,
            // so an unedited round-trip writes nothing: the stored form is
            // not unique (the runtime ORs STP back on), and re-deriving it
            // would rewrite every entry retail happened to store the other
            // way.
            if *e == original_palette[i] {
                continue;
            }
            decoded[off + i * 2..off + i * 2 + 2].copy_from_slice(&stored_form(*e).to_le_bytes());
        }
    } else if palette_entries_changed > 0 && !quantize {
        bail!(
            "{target} carries no palette of its own - it samples {source}, which lives in \
             another record - so a replacement may only use colours already on that row. \
             Re-export, keep the palette, or pass --quantize."
        );
    }
    let pix_off = block.pool_offset + 4 + clut_bytes;
    decoded[pix_off..pix_off + pixels.len()].copy_from_slice(&pixels);

    let unchanged = decoded == block.decoded;
    Ok(Edited {
        block,
        source,
        new_palette,
        pixels,
        quantized_pixels,
        palette_entries_changed,
        decoded,
        unchanged,
    })
}

/// Recompress an edited record and measure it against its slot allocation.
/// `Ok` only when it fits - the descriptor chain forbids growing into the
/// next slot, so an overage is an error carrying its own size.
fn recompress(e: &Edited) -> Result<(Vec<u8>, BattleFit)> {
    let block = &e.block;
    // An unedited round-trip stops here. Recompressing would still move
    // disc bytes (our encoder is not retail's) for no visible change.
    if e.unchanged {
        return Ok((
            Vec::new(),
            BattleFit {
                capacity: block.stream_capacity,
                retail: block.stream_consumed,
                recompressed: block.stream_consumed,
            },
        ));
    }

    // Retail's own encoder is not ours, so try both and keep the smaller -
    // the greedy pass sometimes beats the optimal one's token choices on
    // this data, and every byte counts against the slot footprint.
    let greedy = legaia_lzs::compress(&e.decoded);
    let optimal = legaia_lzs::compress_optimal(&e.decoded);
    let stream = if greedy.len() <= optimal.len() {
        greedy
    } else {
        optimal
    };
    let fit = BattleFit {
        capacity: block.stream_capacity,
        retail: block.stream_consumed,
        recompressed: stream.len(),
    };
    if stream.len() > block.stream_capacity {
        bail!(
            "the edited record recompresses to {} bytes but its slot allocates only {} \
             ({} over). The descriptor chain pins every later record, so this cannot grow. \
             Try an image with more flat regions / fewer distinct colours.",
            stream.len(),
            block.stream_capacity,
            stream.len() - block.stream_capacity
        );
    }
    Ok((stream, fit))
}

/// A validated-but-unwritten replacement, with the art as it will display.
#[derive(Debug, Clone)]
pub struct BattlePreview {
    pub width: usize,
    pub height: usize,
    /// The replacement as the runtime will sample it: 15-bit rounding and
    /// any colour folding already applied, decoded through the palette the
    /// write would install.
    pub rgba: Vec<u8>,
    /// Palettes the block carries of its own (0 for a pixel-only block).
    pub palette_count: usize,
    pub palette: PaletteSource,
    pub palette_entries_changed: usize,
    pub quantized_pixels: usize,
    pub unchanged: bool,
    pub fit: BattleFit,
}

/// Validate a replacement and render it as it will display, without writing.
///
/// Same encode and same fit measurement [`replace_block`] performs, so a
/// preview that says "valid, recompresses to N bytes" is not a second
/// opinion about the write - it is the write, stopped before the patch.
pub fn preview_block(
    patcher: &DiscPatcher,
    target: &BattleTextureTarget,
    rgba: &[u8],
    width: usize,
    height: usize,
    palette: usize,
    quantize: bool,
) -> Result<BattlePreview> {
    let e = edit_block(patcher, target, rgba, width, height, palette, quantize)?;
    let (_, fit) = recompress(&e)?;
    Ok(BattlePreview {
        width,
        height,
        rgba: pixels_to_rgba(&e.pixels, &e.new_palette),
        palette_count: e.block.upload.palette_count(),
        palette: e.source,
        palette_entries_changed: e.palette_entries_changed,
        quantized_pixels: e.quantized_pixels,
        unchanged: e.unchanged,
        fit,
    })
}

/// Replace one block's art with `rgba`, in place.
///
/// The whole record is re-LZS'd (the block is its tail, and the record is
/// one stream), then written back into the record's own slot footprint. A
/// record that no longer fits errors with the byte overage and writes
/// nothing - the descriptor chain forbids growing into the next slot.
#[allow(clippy::too_many_arguments)]
pub fn replace_block(
    patcher: &mut DiscPatcher,
    target: &BattleTextureTarget,
    rgba: &[u8],
    width: usize,
    height: usize,
    palette: usize,
    quantize: bool,
    dry_run: bool,
) -> Result<BattleReplaceOutcome> {
    let e = edit_block(patcher, target, rgba, width, height, palette, quantize)?;
    let (stream, fit) = recompress(&e)?;
    let Edited {
        block,
        source,
        quantized_pixels,
        palette_entries_changed,
        unchanged,
        ..
    } = e;

    if unchanged {
        return Ok(BattleReplaceOutcome {
            width,
            height,
            palette: source,
            palette_entries_changed,
            quantized_pixels,
            unchanged,
            fit,
        });
    }

    if !dry_run {
        // Zero-fill up to the retail stream's extent so no stale tail bytes
        // survive a shorter re-encode; the decoder stops on output count,
        // so the fill is inert either way.
        let mut padded = stream;
        padded.resize(padded.len().max(block.stream_consumed), 0);
        patcher
            .patch_prot_entry(target.entry as usize, block.stream_offset as u64, &padded)
            .with_context(|| format!("write {target}"))?;
    }

    Ok(BattleReplaceOutcome {
        width,
        height,
        palette: source,
        palette_entries_changed,
        quantized_pixels,
        unchanged,
        fit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in palette in the [`visible`] frame - i.e. as
    /// `parse_upload_block` hands it over, with STP already forced onto
    /// every non-zero entry.
    fn base_palette() -> Vec<u16> {
        (0..CLUT_ENTRIES_PER_PALETTE as u16)
            .map(|i| visible((i << 5) | u16::from(i > 0)))
            .collect()
    }

    fn solid(colour: [u8; 4], w: usize, h: usize) -> Vec<u8> {
        colour.iter().copied().cycle().take(w * h * 4).collect()
    }

    #[test]
    fn battle_encode_reuses_the_slot_a_colour_already_has() {
        let pal = base_palette();
        let rgba = solid(legaia_tim::bgr555_to_rgba8(pal[7]), 8, 2);
        let (palette, pixels, q) = encode_pixels(&rgba, 8, 2, &pal, false).unwrap();
        assert_eq!(q, 0);
        assert_eq!(
            &palette[..],
            &pal[..],
            "an unedited image is a zero-run patch"
        );
        assert!(pixels.iter().all(|&b| b == 0x77));
    }

    #[test]
    fn battle_encode_compares_in_the_post_stp_frame() {
        // A palette whose entries came off disc with STP set and one whose
        // entries came off with it clear are the same palette to the
        // runtime, so re-encoding an unedited image must report no change
        // either way - the trap that made a no-op round-trip rewrite all
        // sixteen entries.
        let pal = base_palette();
        assert!(pal.iter().skip(1).all(|e| e & 0x8000 != 0));
        let rgba: Vec<u8> = (0..8 * 2)
            .flat_map(|i| legaia_tim::bgr555_to_rgba8(pal[i % CLUT_ENTRIES_PER_PALETTE]))
            .collect();
        let (palette, _, _) = encode_pixels(&rgba, 8, 2, &pal, false).unwrap();
        assert_eq!(&palette[..], &pal[..]);
        // And the stored form drops STP again, except for opaque black.
        assert_eq!(stored_form(0x9234), 0x1234);
        assert_eq!(stored_form(0x8000), 0x8000);
        assert_eq!(stored_form(0), 0);
        assert_eq!(visible(0x1234), 0x9234);
        assert_eq!(visible(0), 0);
    }

    #[test]
    fn battle_encode_maps_alpha_zero_onto_the_transparent_entry() {
        // Slot 0 is stored 0x0000 = transparent; a fully transparent pixel
        // must land there, and an opaque black must not.
        let pal = base_palette();
        let mut rgba = solid([0, 0, 0, 0], 8, 2);
        rgba[0..4].copy_from_slice(&[0, 0, 0, 255]);
        let (palette, pixels, _) = encode_pixels(&rgba, 8, 2, &pal, false).unwrap();
        let idx: Vec<u8> = pixels.iter().flat_map(|&b| [b & 0x0F, b >> 4]).collect();
        assert_eq!(idx[1], 0, "transparent pixel -> the 0x0000 slot");
        assert_ne!(idx[0], 0, "opaque black must not collapse onto it");
        assert_eq!(palette[idx[0] as usize], 0x8000);
    }

    #[test]
    fn battle_encode_rejects_palette_overflow_without_quantize() {
        let pal = base_palette();
        // 17 distinct colours, none in the base palette.
        let mut rgba = solid([0, 0, 0, 255], 32, 2);
        for i in 0..17 {
            rgba[i * 4] = (i as u8 + 1) << 3;
        }
        let err = encode_pixels(&rgba, 32, 2, &pal, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("distinct 15-bit colours"), "{err}");
        let (_, _, q) = encode_pixels(&rgba, 32, 2, &pal, true).unwrap();
        assert!(q > 0);
    }

    #[test]
    fn battle_encode_checks_dimensions() {
        assert!(encode_pixels(&[0u8; 16], 8, 2, &base_palette(), false).is_err());
    }

    #[test]
    fn target_display_names_both_slot_shapes() {
        let sec = BattleTextureTarget {
            entry: 864,
            slot: BattleTextureSlot::Section(14),
        };
        assert_eq!(sec.to_string(), "entry 864 battle-slot 14");
        let hdr = BattleTextureTarget {
            entry: 863,
            slot: BattleTextureSlot::Record0(1),
        };
        assert_eq!(hdr.to_string(), "entry 863 battle-slot header1");
    }

    #[test]
    fn palette_source_names_where_the_colours_came_from() {
        assert_eq!(PaletteSource::Block(1).to_string(), "block palette 1");
        assert_eq!(
            PaletteSource::AssembledRow(13).to_string(),
            "assembled-row palette 13"
        );
    }
}
