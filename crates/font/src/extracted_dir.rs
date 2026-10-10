//! The `extracted/font/` artifact directory: the atlas PNG and width CSV
//! readers, and the disc-only exporter that writes the whole set.

use super::*;

pub(super) fn decode_atlas_png(path: &Path) -> Result<(Vec<u8>, u32, u32)> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let decoder = png::Decoder::new(BufReader::new(f));
    let mut reader = decoder.read_info().context("read PNG header")?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).context("read PNG frame")?;
    buf.truncate(info.buffer_size());
    let (rgba, w, h) = match info.color_type {
        png::ColorType::Rgba => (buf, info.width, info.height),
        png::ColorType::Rgb => {
            // `info.width * info.height * 4` is u32 arithmetic that could
            // overflow on a malicious header; compute the reserve as u64 and
            // saturate to usize so we never panic on the capacity hint. The
            // actual data is bounded by the already-decoded `buf`.
            let cap = (info.width as u64)
                .saturating_mul(info.height as u64)
                .saturating_mul(4);
            let mut out =
                Vec::with_capacity(usize::try_from(cap).unwrap_or(0).min(buf.len() / 3 * 4));
            for px in buf.as_chunks::<3>().0 {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            (out, info.width, info.height)
        }
        other => bail!("unexpected PNG color type {:?}", other),
    };
    Ok((rgba, w, h))
}

pub(super) fn parse_widths_csv(path: &Path) -> Result<[u8; 256]> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut widths = [0u8; 256];
    for (line_no, line) in text.lines().enumerate() {
        if line_no == 0 || line.is_empty() {
            continue;
        }
        // CSV columns: char_hex, char_dec, char_repr, width_px.
        // char_repr can contain commas (it's quoted in the CSV) - split on the
        // last comma to get the width and use the first comma to get char_hex.
        let first_comma = line
            .find(',')
            .ok_or_else(|| anyhow::anyhow!("malformed widths line {}: {:?}", line_no + 1, line))?;
        let last_comma = line
            .rfind(',')
            .ok_or_else(|| anyhow::anyhow!("malformed widths line {}: {:?}", line_no + 1, line))?;
        let char_hex = &line[..first_comma];
        let width_str = &line[last_comma + 1..];
        let stripped = char_hex.trim_start_matches("0x").trim_start_matches("0X");
        let byte = u8::from_str_radix(stripped, 16)
            .with_context(|| format!("parse char hex {char_hex}"))?;
        let width: u8 = width_str
            .trim()
            .parse()
            .with_context(|| format!("parse width on line {}: {:?}", line_no + 1, width_str))?;
        widths[byte as usize] = width;
    }
    Ok(widths)
}

/// `extracted/font/` path under a given extracted root. Public so callers
/// can probe for the artifact set before constructing a [`Font`].
pub fn extracted_font_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("font")
}

/// Encode an RGBA8 buffer as a PNG file.
fn write_rgba_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let f = std::io::BufWriter::new(
        File::create(path).with_context(|| format!("create {}", path.display()))?,
    );
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}

/// Write the full `extracted/font/` artifact set from disc bytes alone - the
/// same five file names `font-extract` produces from a mednafen save state,
/// so every existing consumer ([`Font::load_from_extracted`],
/// [`Font::load_escape_table`], the asset-viewer, the engine) loads the
/// result unchanged:
///
/// - `dialog_font_atlas.png` - 224x210 glyph atlas. Written as the
///   whitewashed stencil [`Font::from_disc_tim_and_scus`] builds (white
///   fill / dark shadow / transparent), which [`Font::load_paths`]
///   normalises to the identical in-memory atlas as the save-state PNG.
/// - `dialog_font_sheet.png` - the full 256x256 tile page, same stencil
///   colours (the disc TIM's own CLUT is mastering scratch, not the runtime
///   dialog palette, so rendering through it would be misleading).
/// - `dialog_font_widths.csv` - per-character advance table from SCUS;
///   byte-identical to the save-state extraction.
/// - `dialog_font_metadata.json` - same schema as `font-extract`; the
///   escape table (from SCUS) is byte-equivalent, the `clut` section carries
///   the disc TIM's own CLUT with a note on how it differs from the runtime
///   dialog CLUT.
/// - `dialog_font_vram_4bpp.bin` - the raw 4bpp page bytes; byte-identical
///   to the save-state VRAM dump.
///
/// `font_tim` is the slice at [`FONT_TIM_PROT_DAT_OFFSET`] (at least
/// [`FONT_TIM_LEN`] bytes); `scus` is the whole `SCUS_942.54` image;
/// `out_dir` is created if missing.
pub fn export_extracted_font_dir(font_tim: &[u8], scus: &[u8], out_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let indexed = decode_font_tim(font_tim).context("decode dialog-font TIM")?;
    let widths = read_scus_widths(scus).context("read dialog-font width table")?;
    let escape_bytes = read_scus_ram(scus, ESCAPE_TABLE_RAM, ESCAPE_TABLE_ENTRIES * 4)
        .context("read dialog escape table")?;
    let (clut, raw_4bpp) = font_tim_clut_and_pixels(font_tim)?;

    // Atlas (stencil - identical after `load_paths` whitewash to the
    // save-state atlas).
    let atlas = pack_stencil_atlas(&indexed);
    write_rgba_png(
        &out_dir.join("dialog_font_atlas.png"),
        &atlas,
        COLS * GLYPH_W,
        ROWS * GLYPH_H,
    )
    .context("write atlas PNG")?;

    // Sheet (full page, stencil colours).
    let mut sheet = Vec::with_capacity(256 * 256 * 4);
    for &idx in &indexed {
        sheet.extend_from_slice(&stencil_rgba(idx));
    }
    write_rgba_png(&out_dir.join("dialog_font_sheet.png"), &sheet, 256, 256)
        .context("write sheet PNG")?;

    // Widths CSV (byte-identical to font-extract's writer).
    let mut csv = String::from("char_hex,char_dec,char_repr,width_px\n");
    for (i, &w) in widths.iter().enumerate() {
        let c = i as u8;
        let repr = if c.is_ascii_graphic() {
            format!("\"{}\"", c as char)
        } else {
            format!("\"\\x{c:02X}\"")
        };
        csv.push_str(&format!("0x{c:02X},{c},{repr},{w}\n"));
    }
    let csv_path = out_dir.join("dialog_font_widths.csv");
    std::fs::write(&csv_path, csv).with_context(|| format!("write {}", csv_path.display()))?;

    // Metadata JSON (same schema as font-extract).
    let escape_entries: Vec<serde_json::Value> = escape_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let string_id = i16::from_le_bytes([c[0], c[1]]);
            serde_json::json!({
                "idx": i,
                "string_id": string_id,
                "kind": if string_id == 0 { "variable" } else { "string" },
                "advance_px": c[2],
                "y_offset": c[3] as i8,
            })
        })
        .collect();
    let palette: Vec<String> = clut.iter().map(|c| format!("0x{c:04X}")).collect();
    let meta = serde_json::json!({
        "format": "legend-of-legaia-dialog-font",
        "version": 1,
        "description": "Proportional dialog font, extracted from the disc alone \
            (PROT.DAT dialog-font TIM + SCUS_942.54). Width table and escape table \
            come from the SCUS executable; glyph pixel data comes from the on-disc \
            font TIM. Glyph PNGs are whitewashed stencils (white fill, dark drop \
            shadow) - the runtime picks ink colours per draw via CLUT swaps.",
        "vram_source": {
            "x_pixels_16bit": 896,
            "y_pixels": 0,
            "width_16bit_pixels": 64,
            "height_pixels": 256,
            "pixel_format": "4bpp_indexed",
            "tpage_4bpp_x": 14,
            "tpage_4bpp_y": 0,
            "note": "Font lives in VRAM tile-page 14 row 0 (4bpp). 64 VRAM 16-bit \
                pixels wide x 256 tall = 256x256 source 4bpp pixels. Sourced here \
                from the on-disc TIM whose upload target is that framebuffer rect."
        },
        "clut": {
            "vram_x_pixels_16bit": 0,
            "vram_y_pixels": 510,
            "colors": 16,
            "index_for_dialog": 0,
            "note": "The disc TIM's own 16-color BGR555 CLUT (destination (0,510)). \
                This is mastering scratch, NOT the runtime dialog palette the \
                renderer swaps in at (96,510); the stencil PNGs deliberately ignore \
                it. Index 0 = transparent background, index 14 = drop shadow.",
            "palette_bgr555": palette
        },
        "glyph_layout": {
            "cell_width_px": 16,
            "cell_height_px": 16,
            "drawn_width_px": GLYPH_W,
            "drawn_height_px": GLYPH_H,
            "columns": COLS,
            "rows": ROWS,
            "first_char": FIRST_CHAR,
            "last_char": 0xFF,
            "u_formula": "(char & 0x0F) * 16",
            "v_formula": "(char & 0xF0) - 0x20"
        },
        "widths": widths.to_vec(),
        "escape_table": {
            "ram_address": format!("0x{ESCAPE_TABLE_RAM:08X}"),
            "entries": escape_entries
        },
        "rendering_pipeline": {
            "dialog_renderer": "FUN_80036888",
            "draw_and_glyph_count": "FUN_8003CC98",
            "preprocessor": "FUN_80036514",
            "gpu_primitive": "GP0 0x64 (variable-size textured rectangle)",
            "newline_byte": "0x7C",
            "color_change_byte": "0xCF (operand: u8 clut_index)",
            "escape_byte": "0xCE (operand: u8 escape_idx)",
            "string_terminator": "0x00"
        }
    });
    let meta_path = out_dir.join("dialog_font_metadata.json");
    let mut meta_text = serde_json::to_string_pretty(&meta)?;
    meta_text.push('\n');
    std::fs::write(&meta_path, meta_text)
        .with_context(|| format!("write {}", meta_path.display()))?;

    // Raw 4bpp page bytes.
    let raw_path = out_dir.join("dialog_font_vram_4bpp.bin");
    std::fs::write(&raw_path, raw_4bpp).with_context(|| format!("write {}", raw_path.display()))?;

    Ok(())
}
