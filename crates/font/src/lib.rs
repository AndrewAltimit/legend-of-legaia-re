//! Proportional dialog-font loader and layout helper.
//!
//! Consumes `extracted/font/dialog_font_atlas.png` (224×210 RGBA atlas of
//! 14×15-pixel glyph cells, 16 columns × 14 rows) and
//! `dialog_font_widths.csv` (per-character pixel advance). Both artifacts are
//! produced by the extraction pipeline; see
//! [`docs/formats/dialog-font.md`](../../../docs/formats/dialog-font.md) for
//! provenance.
//!
//! No Sony bytes live in this crate — it only knows how to interpret the
//! extracted artifacts.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

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
/// Newline byte in dialog strings — the runtime advances Y by `LINE_HEIGHT`
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
                "no `font/` dir under {} — run `legaia-extract` first",
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
        let (atlas_rgba, atlas_w, atlas_h) = decode_atlas_png(atlas_png)
            .with_context(|| format!("decode {}", atlas_png.display()))?;
        let expected_w = COLS * GLYPH_W;
        let expected_h = ROWS * GLYPH_H;
        if atlas_w != expected_w || atlas_h != expected_h {
            bail!(
                "atlas dimensions {atlas_w}x{atlas_h} don't match expected {expected_w}x{expected_h}",
            );
        }
        Ok(Self {
            widths,
            atlas_rgba,
            atlas_w,
            atlas_h,
        })
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
    /// byte is consumed without rendering — the runtime substitution table
    /// is host-side state we don't model here.
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
                    pen_y += LINE_HEIGHT as i32;
                }
                // 0xCE = inline-escape (variable / string substitution),
                // 0xCF = color-change. Both consume the next byte. The
                // runtime substitution is host-side — we just skip the
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
                    pen_x += self.advance_of(c) as i32;
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
            advance_y: pen_y as u32 + LINE_HEIGHT,
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
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
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
        // char_repr can contain commas (it's quoted in the CSV) — split on the
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
        // 0x05 / 0x10 don't advance the pen — B sits right after A.
        assert_eq!(layout.glyphs[1].dst_x, f.advance_of(b'A') as i32);
    }
}
