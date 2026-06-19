//! Proportional dialog-font loader and layout helper.
//!
//! PORT: FUN_80036888, FUN_80036044, FUN_80035F04, FUN_8003CC98, FUN_8003CD00
//!
//! Consumes `extracted/font/dialog_font_atlas.png` (224×210 RGBA atlas of
//! 14×15-pixel glyph cells, 16 columns × 14 rows) and
//! `dialog_font_widths.csv` (per-character pixel advance). Both artifacts are
//! produced by the extraction pipeline; see
//! [`docs/formats/dialog-font.md`](../../../docs/formats/dialog-font.md) for
//! provenance.
//!
//! No Sony bytes live in this crate - it only knows how to interpret the
//! extracted artifacts.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub mod builtin;

/// Drawn region within each atlas cell. Source cells are 16×16 with one row
/// and two columns of inter-glyph guard space; the actual glyph occupies the
/// upper-left 14×15 pixels.
pub const GLYPH_W: u32 = 14;
pub const GLYPH_H: u32 = 15;
/// Column count of the atlas grid. Columns map to `char & 0x0F`.
pub const COLS: u32 = 16;
/// Row count of the atlas grid. Rows map to `(char - 0x20) >> 4`.
pub const ROWS: u32 = 14;
/// First character in the atlas. Bytes below this are control/escape codes
/// (`0x7C` newline, `0xCE` escape, `0xCF` color change, etc.).
pub const FIRST_CHAR: u8 = 0x20;
/// Inter-character spacing added by the runtime, see SCUS docs.
pub const INTER_GLYPH_PAD: u8 = 1;
/// Newline byte in dialog strings - the runtime advances Y by `LINE_HEIGHT`
/// and resets X to the start of the line.
pub const NEWLINE: u8 = 0x7C;
/// Y advance per newline in the runtime renderer (mirrors the dialog
/// renderer's hard-coded line spacing).
pub const LINE_HEIGHT: u32 = 14;

/// One glyph in a laid-out string. Coordinates are pixel-space relative to
/// the layout origin; atlas coordinates are pixel-space inside the source
/// PNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaidGlyph {
    /// Original byte from the input string (informational).
    pub byte: u8,
    /// Pixel X offset relative to the layout origin (left edge of glyph).
    pub dst_x: i32,
    /// Pixel Y offset relative to the layout origin (top edge of glyph).
    pub dst_y: i32,
    /// Glyph rectangle width in pixels (always [`GLYPH_W`] for printable
    /// chars; this lets renderers treat unprintables as zero-rect skips).
    pub width: u32,
    /// Glyph rectangle height in pixels (always [`GLYPH_H`] for printable
    /// chars).
    pub height: u32,
    /// Source X in the atlas PNG (pixels).
    pub atlas_x: u32,
    /// Source Y in the atlas PNG (pixels).
    pub atlas_y: u32,
}

/// A laid-out string. Independent of any renderer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// One [`LaidGlyph`] per printable character. Newlines and unprintable
    /// bytes do not produce a glyph but do advance the layout cursor.
    pub glyphs: Vec<LaidGlyph>,
    /// Total horizontal advance of the longest line, in pixels.
    pub advance_x: u32,
    /// Total vertical advance, in pixels (one line height + N extra lines
    /// per newline).
    pub advance_y: u32,
}

/// Loaded font: per-byte width table + atlas pixels.
#[derive(Debug, Clone)]
pub struct Font {
    /// Pixel advance for each character byte. Bytes outside `0x20..=0xFF`
    /// have undefined widths in the source data; we always treat them as
    /// zero advance unless overridden.
    widths: [u8; 256],
    /// RGBA8 atlas pixels, row-major, `atlas_w * atlas_h * 4` bytes.
    atlas_rgba: Vec<u8>,
    atlas_w: u32,
    atlas_h: u32,
}

impl Font {
    /// Load the font from an `extracted/` root containing `font/` artifacts.
    pub fn load_from_extracted(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let font_dir = root.join("font");
        if !font_dir.is_dir() {
            bail!(
                "no `font/` dir under {} - run `legaia-extract` first",
                root.display()
            );
        }
        let atlas_path = font_dir.join("dialog_font_atlas.png");
        let widths_path = font_dir.join("dialog_font_widths.csv");
        Self::load_paths(&atlas_path, &widths_path)
    }

    /// Lower-level loader: explicit paths to the atlas PNG and widths CSV.
    pub fn load_paths(atlas_png: &Path, widths_csv: &Path) -> Result<Self> {
        let widths = parse_widths_csv(widths_csv)
            .with_context(|| format!("parse widths CSV {}", widths_csv.display()))?;
        let (mut atlas_rgba, atlas_w, atlas_h) = decode_atlas_png(atlas_png)
            .with_context(|| format!("decode {}", atlas_png.display()))?;
        let expected_w = COLS * GLYPH_W;
        let expected_h = ROWS * GLYPH_H;
        if atlas_w != expected_w || atlas_h != expected_h {
            bail!(
                "atlas dimensions {atlas_w}x{atlas_h} don't match expected {expected_w}x{expected_h}",
            );
        }
        // Whitewash the fill pixels only. The extracted atlas bakes two
        // CLUT-row colours into the texels: `(131,131,131)` for the
        // glyph fill and `(32,32,32)` for a drop-shadow offset +1,+1
        // from each fill pixel (retail PSX-era font convention; see
        // `extracted/font/dialog_font_atlas.png` - the shadow is the
        // visible second tone in every glyph cell).
        //
        // Retail's renderer picks the fill colour at draw time via
        // per-context CLUT swaps. Our `texel.rgb * color.rgb` shader
        // mirrors that, but only if the fill texel is pure white -
        // otherwise the tint can never reach retail's brightness (e.g.
        // the load-screen `(206,206,206)` requires a `>1.0` multiplier
        // against the baked `(131,131,131)`, which clamps).
        //
        // The shadow texels are left at `(32,32,32)`: tinted by any
        // typical foreground colour they fade to near-black, which
        // blends into dark UI panels the same way retail's dim-CLUT-
        // entry shadows do. Keeping shadow texels stops a one-pixel
        // bold-outline halo when the tint goes bright (~white).
        for px in atlas_rgba.chunks_exact_mut(4) {
            if px[3] != 0 && px[0] >= 0x80 && px[1] >= 0x80 && px[2] >= 0x80 {
                px[0] = 0xFF;
                px[1] = 0xFF;
                px[2] = 0xFF;
            }
        }
        Ok(Self {
            widths,
            atlas_rgba,
            atlas_w,
            atlas_h,
        })
    }

    /// Build a placeholder font with no Sony bytes - every glyph cell is a
    /// solid white rect, every printable char has a fixed advance. Useful
    /// for engines that don't have the extracted atlas yet (e.g. CI smoke
    /// runs, or end-users who haven't run `font-extract`); HUD text renders
    /// as visible white blocks instead of crashing.
    ///
    /// Engines should prefer [`Font::load_from_extracted`] when the atlas
    /// is available - the placeholder is purely a fallback.
    pub fn placeholder() -> Self {
        let atlas_w = COLS * GLYPH_W;
        let atlas_h = ROWS * GLYPH_H;
        let mut atlas_rgba = vec![0u8; (atlas_w * atlas_h * 4) as usize];
        // Built-in 5×7 ASCII bitmap font centred in each 14×15 cell. Bytes
        // outside the printable ASCII range fall through to the unknown
        // glyph (a hollow box) so non-ASCII text is still distinguishable
        // from missing glyphs.
        const GLYPH_PX_W: u32 = 5;
        const GLYPH_PX_H: u32 = 7;
        const PAD_X: u32 = (GLYPH_W - GLYPH_PX_W) / 2;
        const PAD_Y: u32 = (GLYPH_H - GLYPH_PX_H) / 2;
        for c in FIRST_CHAR..=0xFFu8 {
            let Some((ox, oy)) = Self::glyph_origin(c) else {
                continue;
            };
            let g = builtin::glyph(c);
            for row in 0..GLYPH_PX_H {
                let bits = g[row as usize];
                for col in 0..GLYPH_PX_W {
                    let bit = bits >> (GLYPH_PX_W - 1 - col) & 1;
                    if bit == 0 {
                        continue;
                    }
                    let x = ox + PAD_X + col;
                    let y = oy + PAD_Y + row;
                    let off = ((y * atlas_w + x) * 4) as usize;
                    atlas_rgba[off] = 255;
                    atlas_rgba[off + 1] = 255;
                    atlas_rgba[off + 2] = 255;
                    atlas_rgba[off + 3] = 255;
                }
            }
        }
        // Fixed-width 8 px advance for every printable char.
        let mut widths = [0u8; 256];
        for w in widths.iter_mut().take(256).skip(FIRST_CHAR as usize) {
            *w = 8;
        }
        Self {
            widths,
            atlas_rgba,
            atlas_w,
            atlas_h,
        }
    }

    /// Try to load from `extracted/`, falling back to a placeholder font
    /// when the artifacts aren't present. Logs a warning so the player
    /// knows text will render as white blocks.
    pub fn load_or_placeholder(root: impl AsRef<Path>) -> Self {
        match Self::load_from_extracted(root) {
            Ok(f) => f,
            Err(e) => {
                log::warn!(
                    "dialog font not loaded ({:#}); falling back to placeholder",
                    e
                );
                Self::placeholder()
            }
        }
    }

    /// Atlas dimensions (pixels).
    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_w, self.atlas_h)
    }

    /// Atlas RGBA8 pixels (row-major, `atlas_w * atlas_h * 4` bytes).
    pub fn atlas_rgba(&self) -> &[u8] {
        &self.atlas_rgba
    }

    /// Pixel advance for character `c`. Returns `0` for characters outside
    /// the printable range (e.g. `\0`, control bytes).
    pub fn advance_of(&self, c: u8) -> u32 {
        if c < FIRST_CHAR {
            return 0;
        }
        self.widths[c as usize] as u32 + INTER_GLYPH_PAD as u32
    }

    /// Try to load the runtime escape table from
    /// `<extracted>/font/dialog_font_metadata.json`. Returns `None` when
    /// the metadata file is missing or doesn't contain an `escape_table`
    /// field - engines that don't need substitution can ignore this.
    pub fn load_escape_table(extracted_root: impl AsRef<Path>) -> Result<Option<EscapeTable>> {
        let path = extracted_root
            .as_ref()
            .join("font")
            .join("dialog_font_metadata.json");
        if !path.is_file() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let table =
            EscapeTable::from_json(&text).with_context(|| format!("parse {}", path.display()))?;
        Ok(Some(table))
    }

    /// Atlas top-left pixel coordinate for character `c`. Returns `None` for
    /// characters that don't have a glyph.
    pub fn glyph_origin(c: u8) -> Option<(u32, u32)> {
        if c < FIRST_CHAR {
            return None;
        }
        let col = (c & 0x0F) as u32;
        let row = ((c - FIRST_CHAR) >> 4) as u32;
        if row >= ROWS {
            return None;
        }
        Some((col * GLYPH_W, row * GLYPH_H))
    }

    /// Lay out `text` starting at `(0, 0)`. The caller translates by the
    /// pen position when uploading to the GPU.
    ///
    /// Newline byte (`0x7C`) advances the cursor down one line and resets X.
    /// Escape sequences (`0xCE`, `0xCF`) are recognized but their argument
    /// byte is consumed without rendering - the runtime substitution table
    /// is host-side state we don't model here.
    /// PORT: FUN_80035F04
    ///
    /// `FUN_80035F04` (SCUS `0x80035f04`) is retail's MES line-width measurement
    /// pass: it walks an MES message, handles `0x7C` newline by tracking the
    /// max-so-far pen advance, consumes the `0xCE` / `0xCF` 2-byte inline
    /// escapes without advancing, accumulates per-glyph widths from the font
    /// width table at `DAT_80073F1C` (the same proportional table this loader
    /// reads from `dialog_font_widths.csv`), and returns the max line width
    /// across all newline-separated lines. [`Layout::advance_x`] is the
    /// engine-side equivalent return value; this `layout` method is a superset
    /// that also emits the per-glyph positions for the renderer.
    pub fn layout(&self, text: &[u8]) -> Layout {
        let mut glyphs = Vec::new();
        let mut pen_x: i32 = 0;
        let mut pen_y: i32 = 0;
        let mut max_x: i32 = 0;
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            i += 1;
            match c {
                0 => break,
                NEWLINE => {
                    if pen_x > max_x {
                        max_x = pen_x;
                    }
                    pen_x = 0;
                    // Saturating: a pathologically long all-newline input must
                    // not overflow the i32 pen cursor (panic in debug builds).
                    pen_y = pen_y.saturating_add(LINE_HEIGHT as i32);
                }
                // 0xCE = inline-escape (variable / string substitution),
                // 0xCF = color-change. Both consume the next byte. The
                // runtime substitution is host-side - we just skip the
                // operand here so the pen doesn't pretend to render it.
                0xCE | 0xCF if i < text.len() => {
                    i += 1;
                }
                0xCE | 0xCF => {}
                FIRST_CHAR..=0xFF => {
                    if let Some((ax, ay)) = Self::glyph_origin(c) {
                        glyphs.push(LaidGlyph {
                            byte: c,
                            dst_x: pen_x,
                            dst_y: pen_y,
                            width: GLYPH_W,
                            height: GLYPH_H,
                            atlas_x: ax,
                            atlas_y: ay,
                        });
                    }
                    pen_x = pen_x.saturating_add(self.advance_of(c) as i32);
                }
                _ => {
                    // Unprintable / out-of-range bytes (0x01..0x1F minus
                    // 0x7C). No glyph, no advance.
                }
            }
        }
        if pen_x > max_x {
            max_x = pen_x;
        }
        Layout {
            glyphs,
            advance_x: max_x.max(0) as u32,
            advance_y: (pen_y.max(0) as u32).saturating_add(LINE_HEIGHT),
        }
    }

    /// Convenience: lay out a UTF-8 string, lossily decoded to the printable
    /// ASCII subset of the dialog font. Bytes outside `0x20..=0xFF` are
    /// dropped. Use [`Font::layout`] directly when you have a pre-encoded
    /// dialog string.
    pub fn layout_ascii(&self, text: &str) -> Layout {
        let bytes: Vec<u8> = text.bytes().collect();
        self.layout(&bytes)
    }

    /// Lay out `text` with word-wrap at `box_width_px` pixels. Words
    /// (runs of non-space bytes) that would overflow the current line are
    /// pushed to the next line. Existing newlines (`0x7C`) are honoured.
    /// Single words longer than the box width are emitted as-is on their
    /// own line - no mid-word breaking.
    ///
    /// Mirrors the field VM's pre-layout pass at SCUS `FUN_80036044` -
    /// engines that drive the dialog renderer's per-line measure step use
    /// this to feed pre-wrapped glyph streams to [`Font::layout`].
    pub fn layout_wrapped(&self, text: &[u8], box_width_px: u32) -> Layout {
        let wrapped = self.wrap_bytes(text, box_width_px);
        self.layout(&wrapped)
    }

    /// Insert `0x7C` newlines into `text` at word boundaries so each line
    /// fits within `box_width_px` when laid out. Returns a new byte buffer
    /// suitable for passing to [`Font::layout`].
    pub fn wrap_bytes(&self, text: &[u8], box_width_px: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() + 8);
        let mut line_w: u32 = 0;
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            if c == 0 {
                break;
            }
            if c == NEWLINE {
                out.push(c);
                line_w = 0;
                i += 1;
                continue;
            }
            // Inline-escape opcodes consume their operand byte without
            // emitting a glyph; mirror Font::layout's behaviour so the
            // wrapped width tracks rendering width.
            if (c == 0xCE || c == 0xCF) && i + 1 < text.len() {
                out.push(c);
                out.push(text[i + 1]);
                i += 2;
                continue;
            }
            if c == b' ' {
                if line_w.saturating_add(self.advance_of(c)) > box_width_px {
                    // Soft-break instead of emitting a trailing space.
                    out.push(NEWLINE);
                    line_w = 0;
                    i += 1;
                    continue;
                }
                out.push(c);
                line_w = line_w.saturating_add(self.advance_of(c));
                i += 1;
                continue;
            }
            // Find the end of the current word.
            let mut j = i;
            let mut word_w: u32 = 0;
            while j < text.len() {
                let cj = text[j];
                if cj == 0 || cj == b' ' || cj == NEWLINE {
                    break;
                }
                if (cj == 0xCE || cj == 0xCF) && j + 1 < text.len() {
                    j += 2;
                    continue;
                }
                word_w = word_w.saturating_add(self.advance_of(cj));
                j += 1;
            }
            if line_w > 0 && line_w.saturating_add(word_w) > box_width_px {
                // Trim trailing space if the previous emit was one.
                if matches!(out.last(), Some(&b' ')) {
                    out.pop();
                }
                out.push(NEWLINE);
                line_w = 0;
            }
            out.extend_from_slice(&text[i..j]);
            line_w = line_w.saturating_add(word_w);
            i = j;
        }
        out
    }
}

fn decode_atlas_png(path: &Path) -> Result<(Vec<u8>, u32, u32)> {
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
            for px in buf.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            (out, info.width, info.height)
        }
        other => bail!("unexpected PNG color type {:?}", other),
    };
    Ok((rgba, w, h))
}

fn parse_widths_csv(path: &Path) -> Result<[u8; 256]> {
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

/// Helper for tests + tooling: synthesize a deterministic font with a fixed
/// width table and an all-white atlas. Lets crates that depend on this one
/// run unit tests without an `extracted/font/` tree.
pub fn synthetic_for_tests() -> Font {
    let mut widths = [0u8; 256];
    for (b, w) in widths.iter_mut().enumerate().take(0x100).skip(0x20) {
        *w = ((b as u8) % 9 + 4).min(9);
    }
    let atlas_w = COLS * GLYPH_W;
    let atlas_h = ROWS * GLYPH_H;
    let atlas_rgba = vec![0xFFu8; (atlas_w * atlas_h * 4) as usize];
    Font {
        widths,
        atlas_rgba,
        atlas_w,
        atlas_h,
    }
}

/// `extracted/font/` path under a given extracted root. Public so callers
/// can probe for the artifact set before constructing a [`Font`].
pub fn extracted_font_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("font")
}

/// Runtime escape table at SCUS `0x80074050`.
///
/// The dialog renderer dispatches the `0xCE` (inline-escape) byte by
/// indexing this 38-entry table with the next byte. Each entry encodes a
/// substitution: a `string_id` referencing a runtime string, a horizontal
/// `advance_px` (how many pixels to move the pen after the substitution),
/// and a `y_offset` for the substitution's baseline.
///
/// Format on disc: 38 × 4-byte entries - `(i16 string_id, u8 advance_px,
/// i8 y_offset)`. Loaded from
/// `extracted/font/dialog_font_metadata.json` produced by `font-extract`.
#[derive(Debug, Clone)]
pub struct EscapeTable {
    pub entries: Vec<EscapeEntry>,
}

/// One escape-table entry. See [`EscapeTable`] for the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeEntry {
    pub string_id: i16,
    pub advance_px: u8,
    pub y_offset: i8,
}

impl EscapeTable {
    /// Parse a `dialog_font_metadata.json` file's `escape_table.entries`
    /// array.
    pub fn from_json(json: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json).context("parse escape_table JSON")?;
        let arr = v
            .pointer("/escape_table/entries")
            .and_then(|x| x.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing /escape_table/entries"))?;
        let mut entries = Vec::with_capacity(arr.len());
        for (i, e) in arr.iter().enumerate() {
            let string_id = e
                .get("string_id")
                .and_then(|x| x.as_i64())
                .ok_or_else(|| anyhow::anyhow!("entry {i}: missing string_id"))?
                as i16;
            let advance_px = e
                .get("advance_px")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| anyhow::anyhow!("entry {i}: missing advance_px"))?
                as u8;
            let y_offset = e
                .get("y_offset")
                .and_then(|x| x.as_i64())
                .ok_or_else(|| anyhow::anyhow!("entry {i}: missing y_offset"))?
                as i8;
            entries.push(EscapeEntry {
                string_id,
                advance_px,
                y_offset,
            });
        }
        Ok(Self { entries })
    }

    /// Look up an entry by index - `byte` is the operand of `0xCE`.
    pub fn entry(&self, byte: u8) -> Option<&EscapeEntry> {
        self.entries.get(byte as usize)
    }
}

#[cfg(test)]
mod placeholder_tests {
    use super::*;

    #[test]
    fn placeholder_has_expected_dimensions() {
        let f = Font::placeholder();
        let (w, h) = f.atlas_dimensions();
        assert_eq!(w, COLS * GLYPH_W);
        assert_eq!(h, ROWS * GLYPH_H);
    }

    #[test]
    fn placeholder_widths_are_fixed_8_for_printables() {
        let f = Font::placeholder();
        for c in FIRST_CHAR..=0x7E {
            assert!(f.advance_of(c) >= 8, "advance for 0x{c:02x} should be >= 8");
        }
        // Non-printable still 0.
        assert_eq!(f.advance_of(0x05), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_origin_first_char() {
        let (x, y) = Font::glyph_origin(0x20).unwrap();
        assert_eq!((x, y), (0, 0));
    }

    #[test]
    fn glyph_origin_a() {
        // 'A' = 0x41. Column 1, row 2.
        let (x, y) = Font::glyph_origin(b'A').unwrap();
        assert_eq!((x, y), (GLYPH_W, GLYPH_H * 2));
    }

    #[test]
    fn glyph_origin_below_first_char_returns_none() {
        assert!(Font::glyph_origin(0x10).is_none());
        assert!(Font::glyph_origin(0x1F).is_none());
    }

    #[test]
    fn glyph_origin_last_char() {
        // 0xFF = column 0xF, row 0xD.
        let (x, y) = Font::glyph_origin(0xFF).unwrap();
        assert_eq!((x, y), (15 * GLYPH_W, 13 * GLYPH_H));
    }

    #[test]
    fn synthetic_font_layout_advances_per_char() {
        let f = synthetic_for_tests();
        // Three chars with the synthetic width formula; advances accumulate.
        let layout = f.layout(b"AAA");
        assert_eq!(layout.glyphs.len(), 3);
        let advance0 = f.advance_of(b'A');
        assert_eq!(layout.glyphs[0].dst_x, 0);
        assert_eq!(layout.glyphs[1].dst_x, advance0 as i32);
        assert_eq!(layout.glyphs[2].dst_x, (advance0 * 2) as i32);
        assert!(layout.advance_x >= advance0 * 3);
    }

    #[test]
    fn newline_advances_y_resets_x() {
        let f = synthetic_for_tests();
        let layout = f.layout(&[b'A', b'B', NEWLINE, b'C', b'D']);
        // Two glyphs on line 0, two on line 1.
        assert_eq!(layout.glyphs.len(), 4);
        assert_eq!(layout.glyphs[2].dst_x, 0);
        assert_eq!(layout.glyphs[2].dst_y, LINE_HEIGHT as i32);
        assert_eq!(layout.glyphs[3].dst_y, LINE_HEIGHT as i32);
    }

    #[test]
    fn escape_byte_consumes_one_operand() {
        let f = synthetic_for_tests();
        // A, 0xCE, 0x05 (operand), B → only A and B emit glyphs.
        let layout = f.layout(&[b'A', 0xCE, 0x05, b'B']);
        assert_eq!(layout.glyphs.len(), 2);
        assert_eq!(layout.glyphs[1].byte, b'B');
    }

    #[test]
    fn wrap_breaks_at_word_boundary() {
        let f = synthetic_for_tests();
        // "AA AAAA" - synthetic widths give A,B,...,Z width 6 (b - 0x20 + 1)
        // ... actually let's use a wide letter to force wrap.
        // The synthetic font widths are deterministic but varied; just
        // assert the wrapped output contains a NEWLINE that wasn't in
        // the input when the box width is small enough.
        let input = b"hello world there";
        let wrapped = f.wrap_bytes(input, 50);
        assert!(
            wrapped.contains(&NEWLINE),
            "narrow box width should force a wrap: {:?}",
            wrapped
        );
    }

    #[test]
    fn wrap_passes_through_when_fits() {
        let f = synthetic_for_tests();
        let wrapped = f.wrap_bytes(b"hi", 1000);
        assert!(
            !wrapped.contains(&NEWLINE),
            "wide box shouldn't insert wraps: {:?}",
            wrapped
        );
    }

    #[test]
    fn wrap_preserves_explicit_newlines() {
        let f = synthetic_for_tests();
        let input = &[b'A', NEWLINE, b'B'];
        let wrapped = f.wrap_bytes(input, 1000);
        assert_eq!(wrapped, input);
    }

    #[test]
    fn null_terminator_stops_layout() {
        let f = synthetic_for_tests();
        let layout = f.layout(&[b'A', 0, b'B']);
        assert_eq!(layout.glyphs.len(), 1);
        assert_eq!(layout.glyphs[0].byte, b'A');
    }

    #[test]
    fn unprintable_bytes_are_skipped() {
        let f = synthetic_for_tests();
        let layout = f.layout(&[b'A', 0x05, 0x10, b'B']);
        assert_eq!(layout.glyphs.len(), 2);
        // 0x05 / 0x10 don't advance the pen - B sits right after A.
        assert_eq!(layout.glyphs[1].dst_x, f.advance_of(b'A') as i32);
    }

    #[test]
    fn escape_table_parses_real_metadata_shape() {
        // Synthesize a metadata JSON with two entries and verify both parse.
        let json = r#"{
            "escape_table": {
                "entries": [
                    {"idx": 0, "string_id": 55, "advance_px": 16, "y_offset": -2},
                    {"idx": 1, "string_id": 33, "advance_px": 8, "y_offset": 0}
                ]
            }
        }"#;
        let table = EscapeTable::from_json(json).expect("parse");
        assert_eq!(table.entries.len(), 2);
        assert_eq!(table.entry(0).unwrap().string_id, 55);
        assert_eq!(table.entry(1).unwrap().advance_px, 8);
        assert_eq!(table.entry(0).unwrap().y_offset, -2);
        assert!(table.entry(2).is_none());
    }

    #[test]
    fn escape_table_returns_error_when_missing_field() {
        let json = r#"{"unrelated": 42}"#;
        assert!(EscapeTable::from_json(json).is_err());
    }

    #[test]
    fn escape_table_rejects_non_json_garbage() {
        // Truncated / non-JSON input must Err, not panic.
        assert!(EscapeTable::from_json("").is_err());
        assert!(EscapeTable::from_json("{not json").is_err());
        assert!(EscapeTable::from_json("\u{0}\u{1}\u{2}").is_err());
    }

    #[test]
    fn layout_empty_input_yields_empty_layout() {
        let f = synthetic_for_tests();
        let layout = f.layout(&[]);
        assert!(layout.glyphs.is_empty());
        assert_eq!(layout.advance_x, 0);
        assert_eq!(layout.advance_y, LINE_HEIGHT);
    }

    #[test]
    fn layout_all_bytes_does_not_panic() {
        // Every possible byte value in one string: control bytes, escape
        // opcodes (with and without trailing operand), printables. Must not
        // panic and must produce a bounded layout.
        let f = synthetic_for_tests();
        let all: Vec<u8> = (1u16..=255).map(|b| b as u8).collect(); // skip 0 (terminator)
        let layout = f.layout(&all);
        // Bounded glyph count: at most one per byte.
        assert!(layout.glyphs.len() <= all.len());
    }

    #[test]
    fn layout_trailing_escape_with_no_operand_is_safe() {
        // 0xCE at the very end has no operand byte to consume.
        let f = synthetic_for_tests();
        let layout = f.layout(&[b'A', 0xCE]);
        assert_eq!(layout.glyphs.len(), 1);
    }

    #[test]
    fn wrap_bytes_on_garbage_does_not_panic() {
        let f = synthetic_for_tests();
        let junk: Vec<u8> = (0..512).map(|i| (i * 7 + 1) as u8).collect();
        // Tiny box width forces many wrap decisions on arbitrary bytes.
        let wrapped = f.wrap_bytes(&junk, 4);
        // Output is bounded (input + inserted newlines, no infinite growth).
        assert!(wrapped.len() <= junk.len() * 2 + 16);
    }

    #[test]
    fn wrap_bytes_zero_box_width_terminates() {
        // A zero-width box is a degenerate input; wrapping must still
        // terminate and not loop forever.
        let f = synthetic_for_tests();
        let wrapped = f.wrap_bytes(b"hello world", 0);
        assert!(!wrapped.is_empty());
    }

    #[test]
    fn parse_widths_csv_rejects_malformed_lines() {
        let dir = std::env::temp_dir().join("legaia-font-fuzz");
        let _ = std::fs::create_dir_all(&dir);

        // A line with no comma after the header row → Err.
        let bad = dir.join("bad_no_comma.csv");
        std::fs::write(&bad, "header\nnocommahere\n").unwrap();
        assert!(parse_widths_csv(&bad).is_err());

        // A line with a non-hex char field → Err.
        let bad2 = dir.join("bad_hex.csv");
        std::fs::write(&bad2, "header\nZZ,1,x,8\n").unwrap();
        assert!(parse_widths_csv(&bad2).is_err());

        // A line with a non-numeric width → Err.
        let bad3 = dir.join("bad_width.csv");
        std::fs::write(&bad3, "header\n0x41,65,A,notanumber\n").unwrap();
        assert!(parse_widths_csv(&bad3).is_err());

        let _ = std::fs::remove_file(&bad);
        let _ = std::fs::remove_file(&bad2);
        let _ = std::fs::remove_file(&bad3);
    }

    #[test]
    fn parse_widths_csv_accepts_well_formed_synthetic() {
        let dir = std::env::temp_dir().join("legaia-font-fuzz");
        let _ = std::fs::create_dir_all(&dir);
        let good = dir.join("good.csv");
        // header row skipped; one valid data row for 'A' (0x41) width 12.
        std::fs::write(
            &good,
            "char_hex,char_dec,char_repr,width_px\n0x41,65,\"A\",12\n",
        )
        .unwrap();
        let widths = parse_widths_csv(&good).unwrap();
        assert_eq!(widths[0x41], 12);
        let _ = std::fs::remove_file(&good);
    }
}
