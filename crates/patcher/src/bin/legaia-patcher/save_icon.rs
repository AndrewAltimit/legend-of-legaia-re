//! `save-icon-list` / `save-icon-export` / `save-icon-replace` - swap the
//! save-slot character portraits.
//!
//! These are the 16x16 faces the save UI draws and the memory-card block
//! icon is cut from. They are a distinct command family from `tim-replace`
//! because the sheet has one palette per tile and stores each tile's rows
//! 128 bytes apart - see [`legaia_patcher::save_icon`] for why the generic
//! texture path would repaint every portrait at once.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::ppf;
use legaia_patcher::save_icon::{self, SLOT_COUNT};
use legaia_tim::encode::decode_png_rgba;

use crate::util::{cue_contents, load_image, note_overwrite};

pub(crate) fn cmd_save_icon_list(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let sheet = save_icon::read_sheet(&patcher)?;

    println!(
        "Save-slot portraits - PROT entry {} +0x{:X}, 4bpp 256x16 strip, one 16-colour \
         palette per tile.",
        save_icon::PROT_ENTRY,
        sheet.entry_offset
    );
    println!("slot  save no.  palette offset  first pixel run  colours");
    for slot in 0..SLOT_COUNT {
        let clut = sheet.tile_clut(slot)?;
        let mut distinct: Vec<u16> = clut.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        println!(
            "{:>4}  {:>8}  0x{:08X}      0x{:08X}       {:>2}",
            slot,
            slot + 1,
            sheet.tile_clut_offset(slot),
            sheet.tile_pixel_run_offsets(slot)[0],
            distinct.len(),
        );
    }
    let blank = legaia_asset::save_icon::TILE_COUNT - 1;
    println!(
        "\nTile {blank} exists in the strip but is blank width padding - nothing selects it, \
         so it is not offered as a slot."
    );
    println!("Export one:  legaia-patcher save-icon-export --input DISC.bin --slot 0 -o face.png");
    Ok(())
}

pub(crate) fn cmd_save_icon_export(input: &Path, slot: usize, output: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let sheet = save_icon::read_sheet(&patcher)?;
    let rgba = save_icon::export_slot(&sheet, slot)?;
    let size = legaia_asset::save_icon::TILE_SIZE;
    legaia_tim::write_png(output, size, size, &rgba)?;
    println!(
        "wrote {} - save-icon slot {slot} (save number {}), {size}x{size}, 16-colour palette",
        output.display(),
        slot + 1
    );
    println!(
        "Edit it (still {size}x{size}, at most 16 colours) and feed it back through save-icon-replace."
    );
    Ok(())
}

pub(crate) fn cmd_save_icon_replace(
    input: &Path,
    slot: usize,
    png: &Path,
    quantize: bool,
    output: Option<&Path>,
    patch: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    if !dry_run && output.is_none() && patch.is_none() {
        bail!(
            "pass --output <patched.bin> and/or --patch <out.ppf> (or --dry-run to only validate)"
        );
    }
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    let png_bytes = std::fs::read(png).with_context(|| format!("read {}", png.display()))?;
    let (w, h, rgba) = decode_png_rgba(&png_bytes)?;
    let size = legaia_asset::save_icon::TILE_SIZE;
    if (w, h) != (size, size) {
        bail!(
            "a save-icon portrait must be exactly {size}x{size}; {} is {w}x{h}",
            png.display()
        );
    }

    if dry_run {
        // Validate the encode without touching the image.
        let probe = save_icon::read_sheet(&patcher)?;
        let _ = save_icon::export_slot(&probe, slot)?;
        println!(
            "dry run: slot {slot} (save number {}) validated, nothing written",
            slot + 1
        );
        return Ok(());
    }

    let outcome = save_icon::replace_slot(&mut patcher, slot, &rgba, quantize)?;
    println!(
        "save-icon slot {} (save number {}): {} palette entr{} changed, {} byte run(s) written",
        outcome.slot,
        outcome.slot + 1,
        outcome.palette_entries_changed,
        if outcome.palette_entries_changed == 1 {
            "y"
        } else {
            "ies"
        },
        outcome.touched_offsets.len()
    );
    if outcome.quantized_pixels > 0 {
        println!(
            "  {} pixel(s) quantized to the nearest kept colour",
            outcome.quantized_pixels
        );
    }

    // Verify off the patched image, and prove the edit was surgical: this
    // slot decodes to the requested pixels and every other slot is unchanged.
    let before = save_icon::read_sheet(&DiscPatcher::open(original.clone())?)?;
    let after = save_icon::read_sheet(&patcher).context("re-read patched sheet")?;
    if outcome.quantized_pixels == 0 {
        let got = save_icon::export_slot(&after, slot)?;
        let want: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|p| {
                legaia_tim::bgr555_to_rgba8(legaia_tim::encode::rgba8_to_bgr555(
                    p.try_into().unwrap(),
                ))
            })
            .collect();
        if got != want {
            bail!("verification failed: patched portrait does not decode to the input image");
        }
        println!("verified: slot {slot} decodes pixel-exactly to the input image");
    }
    for other in 0..legaia_asset::save_icon::TILE_COUNT {
        if other == slot {
            continue;
        }
        if before.tile_block_pixels(other)? != after.tile_block_pixels(other)?
            || before.tile_clut(other)? != after.tile_clut(other)?
        {
            bail!("verification failed: replacing slot {slot} also changed tile {other}");
        }
    }
    println!("verified: every other portrait is byte-identical");

    let patched = patcher.into_image();
    if let Some(ppf_path) = patch {
        let runs = ppf::diff_runs(&original, &patched);
        let desc = format!("Legaia save-icon slot {slot}");
        let bytes = ppf::write_ppf3(&desc, &runs);
        note_overwrite(ppf_path);
        std::fs::write(ppf_path, &bytes)
            .with_context(|| format!("write {}", ppf_path.display()))?;
        println!(
            "wrote {} ({} change run(s)) - safe to share, carries only your edit",
            ppf_path.display(),
            runs.len()
        );
    }
    if let Some(out) = output {
        note_overwrite(out);
        std::fs::write(out, &patched).with_context(|| format!("write {}", out.display()))?;
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "patched.bin".into());
        let cue_path = out.with_extension("cue");
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!(
            "wrote {} + {} (contains Sony bytes - local play only, never redistribute)",
            out.display(),
            cue_path.display()
        );
    }
    Ok(())
}
