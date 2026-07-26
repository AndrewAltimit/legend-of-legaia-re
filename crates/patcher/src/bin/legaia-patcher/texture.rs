//! `tim-list` / `tim-export` / `tim-replace` - texture replacement.
//!
//! The modding loop: `tim-list` catalogs every TIM on the disc with its
//! replacement coordinates, `tim-export` decodes one to a PNG for editing,
//! and `tim-replace` encodes an edited PNG back into a same-size in-place
//! disc patch (EDC/ECC re-encoded per touched sector).

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::ppf;
use legaia_patcher::texture::{TextureTarget, read_texture, replace_texture, texture_catalogs};
use legaia_tim::encode::{EncodeOptions, decode_png_rgba};

use crate::cli::TimTierArg;
use crate::util::{cue_contents, load_image, note_overwrite};

pub(crate) fn target(entry: Option<u32>, offset: u64, lzs_section: Option<u32>) -> TextureTarget {
    TextureTarget {
        entry,
        lzs_section,
        offset,
    }
}

pub(crate) fn cmd_tim_list(input: &Path, entry: Option<u32>, tier: TimTierArg) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let (raw, deep) = texture_catalogs(&patcher)?;

    println!("tier  entry  section  offset      size       bpp  cluts  bytes    label");
    let mut raw_n = 0usize;
    if matches!(tier, TimTierArg::Raw | TimTierArg::All) {
        for t in &raw {
            if let Some(e) = entry
                && t.entry_index != Some(e)
            {
                continue;
            }
            raw_n += 1;
            println!(
                "raw   {:>5}  {:>7}  0x{:08X}  {:>4}x{:<4}  {:>3}  {:>5}  {:>7}  {}",
                t.entry_index
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "gap".into()),
                "-",
                t.offset_in_entry,
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.label.unwrap_or(""),
            );
        }
    }
    let mut deep_n = 0usize;
    if matches!(tier, TimTierArg::Lzs | TimTierArg::All) {
        for t in &deep {
            if let Some(e) = entry
                && t.entry_index != e
            {
                continue;
            }
            deep_n += 1;
            println!(
                "lzs   {:>5}  {:>7}  0x{:08X}  {:>4}x{:<4}  {:>3}  {:>5}  {:>7}  {}",
                t.entry_index,
                t.lzs_section,
                t.offset_in_section,
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.label.unwrap_or(""),
            );
        }
    }
    println!(
        "\n{raw_n} raw texture(s) (always replaceable in place) + {deep_n} LZS-compressed \
         (replaceable when the edited section recompresses into its footprint)."
    );
    println!(
        "Export one:  legaia-patcher tim-export --input DISC.bin --entry N --offset 0xHEX \
         [--lzs-section S] -o out.png"
    );
    println!(
        "Replace it:  legaia-patcher tim-replace --input DISC.bin --entry N --offset 0xHEX \
         [--lzs-section S] --png edited.png --patch out.ppf"
    );
    Ok(())
}

pub(crate) fn cmd_tim_export(
    input: &Path,
    entry: Option<u32>,
    offset: u64,
    lzs_section: Option<u32>,
    clut: usize,
    output: &Path,
) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let t = target(entry, offset, lzs_section);
    let orig = read_texture(&patcher, &t)?;
    let rgba = legaia_tim::decode_rgba8(&orig.tim, clut)
        .with_context(|| format!("decode {} (clut {clut})", t))?;
    let (w, h) = (orig.tim.pixel_width(), orig.tim.pixel_height());
    legaia_tim::write_png(output, w, h, &rgba)?;
    println!(
        "wrote {} - {} ({}x{}, {} bpp, {} palette(s), {} bytes on disc)",
        output.display(),
        t,
        w,
        h,
        match orig.tim.mode {
            legaia_tim::PixelMode::Bpp4 => 4,
            legaia_tim::PixelMode::Bpp8 => 8,
            legaia_tim::PixelMode::Bpp16 => 16,
            legaia_tim::PixelMode::Bpp24 => 24,
            legaia_tim::PixelMode::Mixed => 0,
        },
        orig.tim.palette_count(),
        orig.tim_bytes.len(),
    );
    println!("Edit it (same dimensions!) and feed it back through tim-replace.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_tim_replace(
    input: &Path,
    entry: Option<u32>,
    offset: u64,
    lzs_section: Option<u32>,
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
    let t = target(entry, offset, lzs_section);

    let png_bytes = std::fs::read(png).with_context(|| format!("read {}", png.display()))?;
    let (w, h, rgba) = decode_png_rgba(&png_bytes)?;

    let opts = EncodeOptions { quantize };
    let outcome = replace_texture(&mut patcher, &t, &rgba, w, h, &opts, dry_run)?;

    println!(
        "{}: {}x{} {} bpp, {} palette(s), {} bytes - encoded in place",
        t, outcome.width, outcome.height, outcome.bpp, outcome.clut_count, outcome.byte_len
    );
    if outcome.new_palette_entries > 0 {
        println!(
            "  {} new palette color(s) written{}",
            outcome.new_palette_entries,
            if outcome.clut_rows_rewritten {
                " (palette replicated into every CLUT row)"
            } else {
                ""
            }
        );
    }
    if outcome.quantized_pixels > 0 {
        println!(
            "  {} pixel(s) quantized to the nearest palette color",
            outcome.quantized_pixels
        );
    }
    if let Some(fit) = outcome.lzs {
        println!(
            "  LZS fit: recompressed {} bytes into the {}-byte retail stream",
            fit.recompressed, fit.capacity
        );
    }
    if dry_run {
        println!("dry run: validated only, nothing written");
        return Ok(());
    }

    // Verify off the patched image: the texture must read back strict-valid
    // and (when nothing was quantized) display exactly the requested pixels.
    let after = read_texture(&patcher, &t).context("re-read patched texture")?;
    if outcome.quantized_pixels == 0 {
        let got = legaia_tim::decode_rgba8(&after.tim, 0)?;
        let want: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|p| {
                legaia_tim::bgr555_to_rgba8(legaia_tim::encode::rgba8_to_bgr555(
                    p.try_into().unwrap(),
                ))
            })
            .collect();
        if got != want {
            bail!("verification failed: patched texture does not decode to the input image");
        }
        println!("verified: patched texture decodes pixel-exactly to the input image");
    }

    let patched = patcher.into_image();
    if let Some(ppf_path) = patch {
        let runs = ppf::diff_runs(&original, &patched);
        let desc = format!("Legaia texture replace {t}");
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
