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

/// `PROT.DAT` file offset of the proportional dialog-font TIM: a 4bpp
/// 256×256 tile-page whose image framebuffer is `(896, 0)` (the runtime
/// font VRAM page) with its 16-entry CLUT at `(0, 510)`. Pinned by a
/// PROT.DAT-wide TIM scan for that framebuffer - it is the on-disc carrier
/// of the glyph bitmaps [`crate::Font`] otherwise sources from a live VRAM
/// capture, so a disc-only consumer can build the real font without a save
/// state. See [`docs/formats/dialog-font.md`](../../../docs/formats/dialog-font.md).
pub const FONT_TIM_PROT_DAT_OFFSET: u64 = 0x7F40;

/// Byte length to slice from [`FONT_TIM_PROT_DAT_OFFSET`] to cover the whole
/// font TIM (8-byte header + 44-byte CLUT block + 12-byte image header +
/// 32768-byte 4bpp page = 32832; rounded up).
pub const FONT_TIM_LEN: usize = 0x8100;

/// SCUS RAM address of the 256-byte per-character advance table.
const WIDTH_TABLE_RAM: u32 = 0x8007_3F1C;
/// PSX-EXE `t_addr` header field offset + header size + the default load
/// address used when the executable header is absent/unparseable.
const PSX_EXE_T_ADDR_OFFSET: usize = 0x18;
const PSX_EXE_HEADER: u32 = 0x800;
const SCUS_LOAD_ADDR_FALLBACK: u32 = 0x8001_0000;
/// Font-page CLUT indices: 0 = transparent background, 14 = drop-shadow,
/// everything else = glyph fill (whitewashed to pure white so the engine's
/// `texel.rgb * tint` shader can reach any retail ink colour).
const FONT_SHADOW_INDEX: u8 = 14;

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
                "no `font/` dir under {} - run `legaia-extract` (writes extracted/font/) or `font-extract --disc <bin>`",
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

    /// Build the real proportional dialog font straight from a disc, no
    /// mednafen save state required: the 4bpp font TIM carried in `PROT.DAT`
    /// (at [`FONT_TIM_PROT_DAT_OFFSET`]) supplies the glyph bitmaps, and
    /// `SCUS_942.54` the per-character advance table (VA `0x80073F1C`).
    ///
    /// Produces the identical whitewashed atlas [`Font::load_from_extracted`]
    /// yields from `extracted/font/` (fill texels → pure white, the
    /// `(32,32,32)` drop-shadow texels preserved), so a disc-only consumer -
    /// the WASM site - renders text byte-for-byte like native without the
    /// extracted artifacts.
    ///
    /// `font_tim` is the TIM slice starting at [`FONT_TIM_PROT_DAT_OFFSET`]
    /// (read at least [`FONT_TIM_LEN`] bytes); `scus` is the whole executable.
    pub fn from_disc_tim_and_scus(font_tim: &[u8], scus: &[u8]) -> Result<Self> {
        let indexed = decode_font_tim(font_tim).context("decode dialog-font TIM")?;
        let atlas_rgba = pack_stencil_atlas(&indexed);
        let widths = read_scus_widths(scus).context("read dialog-font width table")?;
        Ok(Self {
            widths,
            atlas_rgba,
            atlas_w: COLS * GLYPH_W,
            atlas_h: ROWS * GLYPH_H,
        })
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

/// Decode the 4bpp dialog-font TIM (`font_tim` starting at the TIM magic) into
/// a 256×256 row-major indexed buffer (one byte per pixel, 0..=15). Validates
/// the header + the framebuffer `(896, 0)` so a wrong slice fails loudly rather
/// than producing garbage glyphs.
fn decode_font_tim(font_tim: &[u8]) -> Result<Vec<u8>> {
    let rd_u32 = |o: usize| -> Result<u32> {
        font_tim
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    let rd_u16 = |o: usize| -> Result<u16> {
        font_tim
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    if rd_u32(0)? != 0x10 {
        bail!("not a TIM (bad magic)");
    }
    let flags = rd_u32(4)?;
    let bpp = flags & 0x7;
    if bpp != 0 {
        bail!("font TIM is not 4bpp (bpp mode {bpp})");
    }
    let mut p = 8usize;
    // Skip the CLUT block if present (bit 3): `[u32 block_len][block_len bytes]`.
    if flags & 0x8 != 0 {
        let clut_len = rd_u32(p)? as usize;
        p = p
            .checked_add(clut_len)
            .filter(|&e| e <= font_tim.len())
            .ok_or_else(|| anyhow::anyhow!("font TIM CLUT block overruns"))?;
    }
    // Image block: `[u32 len][u16 dx][u16 dy][u16 w16][u16 h][pixels]`.
    let _img_len = rd_u32(p)?;
    let (dx, dy, w16, h) = (
        rd_u16(p + 4)?,
        rd_u16(p + 6)?,
        rd_u16(p + 8)?,
        rd_u16(p + 10)?,
    );
    if (dx, dy) != (896, 0) || w16 != 64 || h != 256 {
        bail!("font TIM framebuffer ({dx},{dy}) {w16}x{h} != expected (896,0) 64x256");
    }
    let pixels = &font_tim[p + 12..];
    let stride = (w16 as usize) * 2; // bytes per row (2 px per byte)
    let width = w16 as usize * 4; // = 256 pixels
    let height = h as usize;
    let need = stride * height;
    if pixels.len() < need {
        bail!("font TIM pixel data truncated ({} < {need})", pixels.len());
    }
    let mut indexed = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let byte = pixels[y * stride + x / 2];
            // Low nibble = even pixel, high nibble = odd (PSX 4bpp order).
            indexed[y * width + x] = if x & 1 == 0 { byte & 0xF } else { byte >> 4 };
        }
    }
    Ok(indexed)
}

/// Pack the 256×256 indexed font page into the 224×210 (16×14 cells of 14×15)
/// atlas [`Font`] expects, as a whitewashed stencil: cell `c` lives in the page
/// at `U = (c & 0x0F) * 16`, `V = (c & 0xF0) - 0x20` (`docs/formats/dialog-font.md`),
/// and its upper-left 14×15 pixels copy to `glyph_origin(c)`. Index 0 → fully
/// transparent, [`FONT_SHADOW_INDEX`] → the `(32,32,32)` drop shadow, every
/// other index → pure white fill.
fn pack_stencil_atlas(indexed: &[u8]) -> Vec<u8> {
    const PAGE: usize = 256;
    let aw = (COLS * GLYPH_W) as usize;
    let ah = (ROWS * GLYPH_H) as usize;
    let mut atlas = vec![0u8; aw * ah * 4];
    for c in FIRST_CHAR..=0xFFu8 {
        let Some((ox, oy)) = Font::glyph_origin(c) else {
            continue;
        };
        let src_u = ((c & 0x0F) as usize) * 16;
        let src_v = ((c as usize) & 0xF0).wrapping_sub(0x20);
        if src_v + GLYPH_H as usize > PAGE {
            continue;
        }
        for gy in 0..GLYPH_H as usize {
            for gx in 0..GLYPH_W as usize {
                let idx = indexed[(src_v + gy) * PAGE + (src_u + gx)];
                let px = match idx {
                    0 => [0, 0, 0, 0],
                    FONT_SHADOW_INDEX => [32, 32, 32, 255],
                    _ => [255, 255, 255, 255],
                };
                let dx = ox as usize + gx;
                let dy = oy as usize + gy;
                let off = (dy * aw + dx) * 4;
                atlas[off..off + 4].copy_from_slice(&px);
            }
        }
    }
    atlas
}

/// Read the 256-byte per-character advance table from `SCUS_942.54` (RAM
/// [`WIDTH_TABLE_RAM`]). Resolves the file offset through the PSX-EXE `t_addr`
/// header, falling back to the retail load address when the header is absent.
fn read_scus_widths(scus: &[u8]) -> Result<[u8; 256]> {
    let t_addr = if scus.len() >= 0x40 && &scus[0..8] == b"PS-X EXE" {
        u32::from_le_bytes(
            scus[PSX_EXE_T_ADDR_OFFSET..PSX_EXE_T_ADDR_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    } else {
        SCUS_LOAD_ADDR_FALLBACK
    };
    let ram_off = WIDTH_TABLE_RAM
        .checked_sub(t_addr)
        .ok_or_else(|| anyhow::anyhow!("width table below t_addr"))?;
    let file_off = ram_off
        .checked_add(PSX_EXE_HEADER)
        .map(|v| v as usize)
        .ok_or_else(|| anyhow::anyhow!("width table offset overflow"))?;
    let slice = scus
        .get(file_off..file_off + 256)
        .ok_or_else(|| anyhow::anyhow!("width table past SCUS end"))?;
    let mut widths = [0u8; 256];
    widths.copy_from_slice(slice);
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

    /// Build a minimal 4bpp font TIM (only cell 'A' painted) + a PSX-EXE with a
    /// one-entry width table and assert `from_disc_tim_and_scus` places the
    /// stencilled glyph + advance where the atlas expects them.
    #[test]
    fn from_disc_tim_and_scus_stencils_and_reads_widths() {
        // 256x256 4bpp page: 64 halfwords * 2 = 128 bytes per row.
        let stride = 64 * 2;
        let mut page = vec![0u8; stride * 256];
        // 'A' (0x41) cell: U = 1*16 = 16, V = (0x40)-0x20 = 32. Paint the
        // top-left glyph pixel fill (index 15) and the one to its right shadow
        // (index 14).
        let put = |page: &mut [u8], x: usize, y: usize, idx: u8| {
            let b = &mut page[y * stride + x / 2];
            if x & 1 == 0 {
                *b = (*b & 0xF0) | idx;
            } else {
                *b = (*b & 0x0F) | (idx << 4);
            }
        };
        put(&mut page, 16, 32, 15); // fill -> white
        put(&mut page, 17, 32, 14); // shadow -> (32,32,32)

        // Wrap the page in a TIM: header + CLUT block (16 entries) + image block.
        let mut tim = Vec::new();
        tim.extend_from_slice(&0x10u32.to_le_bytes()); // magic
        tim.extend_from_slice(&0x8u32.to_le_bytes()); // flags: 4bpp + CLUT
        let clut_body = 4 + 8 + 16 * 2; // len(4) + dst rect(8) + 16 entries(32) = 44
        tim.extend_from_slice(&(clut_body as u32).to_le_bytes());
        tim.extend_from_slice(&0u16.to_le_bytes()); // clut dx
        tim.extend_from_slice(&510u16.to_le_bytes()); // clut dy
        tim.extend_from_slice(&16u16.to_le_bytes()); // clut w
        tim.extend_from_slice(&1u16.to_le_bytes()); // clut h
        tim.extend_from_slice(&[0u8; 32]); // 16 CLUT entries (ignored)
        let img_len = 4 + 8 + page.len();
        tim.extend_from_slice(&(img_len as u32).to_le_bytes());
        tim.extend_from_slice(&896u16.to_le_bytes()); // img dx
        tim.extend_from_slice(&0u16.to_le_bytes()); // img dy
        tim.extend_from_slice(&64u16.to_le_bytes()); // img w16
        tim.extend_from_slice(&256u16.to_le_bytes()); // img h
        tim.extend_from_slice(&page);

        // Minimal PSX-EXE: magic + t_addr at 0x18, width table at file offset
        // (WIDTH_TABLE_RAM - t_addr) + 0x800.
        let t_addr = SCUS_LOAD_ADDR_FALLBACK;
        let file_off = (WIDTH_TABLE_RAM - t_addr) as usize + PSX_EXE_HEADER as usize;
        let mut scus = vec![0u8; file_off + 256];
        scus[0..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        scus[file_off + 0x41] = 12; // width['A'] = 12

        let font = Font::from_disc_tim_and_scus(&tim, &scus).unwrap();
        assert_eq!(font.atlas_dimensions(), (COLS * GLYPH_W, ROWS * GLYPH_H));
        // 'A' advance = width + inter-glyph pad.
        assert_eq!(font.advance_of(b'A'), 12 + INTER_GLYPH_PAD as u32);
        // The painted cell's top-left is fill (white, opaque); the pixel to its
        // right is the dark shadow.
        let (ox, oy) = Font::glyph_origin(b'A').unwrap();
        let aw = (COLS * GLYPH_W) as usize;
        let at = |x: u32, y: u32| -> [u8; 4] {
            let o = ((y as usize) * aw + x as usize) * 4;
            font.atlas_rgba()[o..o + 4].try_into().unwrap()
        };
        assert_eq!(at(ox, oy), [255, 255, 255, 255]);
        assert_eq!(at(ox + 1, oy), [32, 32, 32, 255]);
    }

    /// Byte-exact parity oracle: the font decoded straight from the disc
    /// (`PROT.DAT` TIM + SCUS width table) must equal the save-state extraction
    /// the native engine loads from `extracted/font/`. Skips (passes) when the
    /// extracted artifacts aren't present. Proves the WASM site's disc-only
    /// font is identical to native's.
    #[test]
    fn disc_font_matches_extracted_artifacts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
        let prot = root.join("PROT.DAT");
        let scus = root.join("SCUS_942.54");
        let atlas_png = root.join("font/dialog_font_atlas.png");
        let widths_csv = root.join("font/dialog_font_widths.csv");
        if !(prot.exists() && scus.exists() && atlas_png.exists() && widths_csv.exists()) {
            eprintln!("[skip] extracted/ font artifacts absent - disc-font parity");
            return;
        }
        let prot_bytes = std::fs::read(&prot).unwrap();
        let scus_bytes = std::fs::read(&scus).unwrap();
        let off = FONT_TIM_PROT_DAT_OFFSET as usize;
        let tim = &prot_bytes[off..off + FONT_TIM_LEN];
        let disc = Font::from_disc_tim_and_scus(tim, &scus_bytes).unwrap();
        let extracted = Font::load_paths(&atlas_png, &widths_csv).unwrap();
        assert_eq!(
            disc.atlas_dimensions(),
            extracted.atlas_dimensions(),
            "atlas dims diverge"
        );
        assert!(
            disc.atlas_rgba() == extracted.atlas_rgba(),
            "disc-decoded font atlas differs from the extracted (save-state) atlas"
        );
        for c in 0u16..256 {
            assert_eq!(
                disc.advance_of(c as u8),
                extracted.advance_of(c as u8),
                "advance for byte 0x{c:02X} diverges"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Disc-only `extracted/font/` export
// ---------------------------------------------------------------------------

/// SCUS RAM address of the 38-entry runtime escape table (the `0xCE`
/// substitution table [`EscapeTable`] models). Same table `font-extract`
/// reads from a save state's companion SCUS; here it feeds the disc-only
/// export of `dialog_font_metadata.json`.
const ESCAPE_TABLE_RAM: u32 = 0x8007_4050;
/// Entry count of the escape table (4 bytes each).
const ESCAPE_TABLE_ENTRIES: usize = 38;

/// Resolve an SCUS RAM address to a file slice through the PSX-EXE `t_addr`
/// header (falling back to the retail load address when the header is
/// absent), mirroring [`read_scus_widths`] for arbitrary tables.
fn read_scus_ram(scus: &[u8], ram_addr: u32, len: usize) -> Result<&[u8]> {
    let t_addr = if scus.len() >= 0x40 && &scus[0..8] == b"PS-X EXE" {
        u32::from_le_bytes(
            scus[PSX_EXE_T_ADDR_OFFSET..PSX_EXE_T_ADDR_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    } else {
        SCUS_LOAD_ADDR_FALLBACK
    };
    let ram_off = ram_addr
        .checked_sub(t_addr)
        .ok_or_else(|| anyhow::anyhow!("RAM 0x{ram_addr:08X} below t_addr 0x{t_addr:08X}"))?;
    let file_off = ram_off
        .checked_add(PSX_EXE_HEADER)
        .map(|v| v as usize)
        .ok_or_else(|| anyhow::anyhow!("RAM 0x{ram_addr:08X} offset overflows"))?;
    scus.get(file_off..file_off + len)
        .ok_or_else(|| anyhow::anyhow!("RAM 0x{ram_addr:08X} + {len} past SCUS end"))
}

/// Walk the dialog-font TIM header and return `(clut, pixel_bytes)`: the
/// TIM's own 16-entry BGR555 CLUT and the raw 4bpp page bytes (32768 bytes =
/// 256x256 pixels, two per byte). The pixel bytes are exactly what the
/// runtime uploads to VRAM `(896, 0)`, so they byte-match the
/// `dialog_font_vram_4bpp.bin` a save-state extraction produces.
fn font_tim_clut_and_pixels(font_tim: &[u8]) -> Result<([u16; 16], &[u8])> {
    let rd_u32 = |o: usize| -> Result<u32> {
        font_tim
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow::anyhow!("font TIM truncated at 0x{o:X}"))
    };
    if rd_u32(0)? != 0x10 {
        bail!("not a TIM (bad magic)");
    }
    let flags = rd_u32(4)?;
    if flags & 0x7 != 0 {
        bail!("font TIM is not 4bpp");
    }
    let mut clut = [0u16; 16];
    let mut p = 8usize;
    if flags & 0x8 != 0 {
        let clut_len = rd_u32(p)? as usize;
        // Entries start after len(4) + destination rect(8).
        for (i, slot) in clut.iter_mut().enumerate() {
            let off = p + 12 + i * 2;
            *slot = font_tim
                .get(off..off + 2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .unwrap_or(0);
        }
        p = p
            .checked_add(clut_len)
            .filter(|&e| e <= font_tim.len())
            .ok_or_else(|| anyhow::anyhow!("font TIM CLUT block overruns"))?;
    }
    // Image block: `[u32 len][u16 dx][u16 dy][u16 w16][u16 h][pixels]`.
    let pixels = font_tim
        .get(p + 12..p + 12 + 256 * 128)
        .ok_or_else(|| anyhow::anyhow!("font TIM pixel data truncated"))?;
    Ok((clut, pixels))
}

/// Stencil colour for a 4bpp palette index: transparent background, the
/// `(32,32,32)` drop shadow, or pure-white fill (see [`pack_stencil_atlas`]).
fn stencil_rgba(idx: u8) -> [u8; 4] {
    match idx {
        0 => [0, 0, 0, 0],
        FONT_SHADOW_INDEX => [32, 32, 32, 255],
        _ => [255, 255, 255, 255],
    }
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
        .chunks_exact(4)
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
            "wrapper_with_word_wrap": "FUN_8003CC98",
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

#[cfg(test)]
mod export_tests {
    use super::*;

    /// Build the same synthetic TIM + SCUS as
    /// `from_disc_tim_and_scus_stencils_and_reads_widths`, export a font dir,
    /// and assert the artifact set exists and round-trips through the
    /// standard extracted-artifact loaders.
    #[test]
    fn export_extracted_font_dir_roundtrips_through_loaders() {
        let stride = 64 * 2;
        let mut page = vec![0u8; stride * 256];
        // Paint 'A' fill + shadow like the from_disc test does.
        page[32 * stride + 16 / 2] = 15; // (16,32) low nibble = fill
        page[32 * stride + 17 / 2] |= 14 << 4; // (17,32) high nibble = shadow

        let mut tim = Vec::new();
        tim.extend_from_slice(&0x10u32.to_le_bytes());
        tim.extend_from_slice(&0x8u32.to_le_bytes());
        tim.extend_from_slice(&44u32.to_le_bytes());
        tim.extend_from_slice(&0u16.to_le_bytes());
        tim.extend_from_slice(&510u16.to_le_bytes());
        tim.extend_from_slice(&16u16.to_le_bytes());
        tim.extend_from_slice(&1u16.to_le_bytes());
        tim.extend_from_slice(&[0u8; 32]);
        tim.extend_from_slice(&((4 + 8 + page.len()) as u32).to_le_bytes());
        tim.extend_from_slice(&896u16.to_le_bytes());
        tim.extend_from_slice(&0u16.to_le_bytes());
        tim.extend_from_slice(&64u16.to_le_bytes());
        tim.extend_from_slice(&256u16.to_le_bytes());
        tim.extend_from_slice(&page);

        // Minimal PSX-EXE covering both the width table and the escape table.
        let t_addr = SCUS_LOAD_ADDR_FALLBACK;
        let escape_off = (ESCAPE_TABLE_RAM - t_addr) as usize + PSX_EXE_HEADER as usize;
        let widths_off = (WIDTH_TABLE_RAM - t_addr) as usize + PSX_EXE_HEADER as usize;
        let mut scus = vec![0u8; escape_off + ESCAPE_TABLE_ENTRIES * 4];
        scus[0..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        scus[widths_off + 0x41] = 12;
        scus[escape_off] = 55; // entry 0 string_id lo byte
        scus[escape_off + 2] = 16; // entry 0 advance_px

        let out = std::env::temp_dir().join("legaia-font-export-test");
        let _ = std::fs::remove_dir_all(&out);
        export_extracted_font_dir(&tim, &scus, &out).unwrap();

        for name in [
            "dialog_font_atlas.png",
            "dialog_font_sheet.png",
            "dialog_font_widths.csv",
            "dialog_font_metadata.json",
            "dialog_font_vram_4bpp.bin",
        ] {
            assert!(out.join(name).exists(), "missing artifact {name}");
        }

        // The exported artifacts must load into the same Font the direct
        // disc path builds.
        let direct = Font::from_disc_tim_and_scus(&tim, &scus).unwrap();
        let loaded = Font::load_paths(
            &out.join("dialog_font_atlas.png"),
            &out.join("dialog_font_widths.csv"),
        )
        .unwrap();
        assert_eq!(direct.atlas_rgba(), loaded.atlas_rgba());
        assert_eq!(direct.advance_of(b'A'), loaded.advance_of(b'A'));

        // Escape table loads through the standard metadata reader.
        let root = out.parent().unwrap();
        // load_escape_table expects `<root>/font/...`; check via from_json.
        let _ = root;
        let meta = std::fs::read_to_string(out.join("dialog_font_metadata.json")).unwrap();
        let table = EscapeTable::from_json(&meta).unwrap();
        assert_eq!(table.entries.len(), ESCAPE_TABLE_ENTRIES);
        assert_eq!(table.entries[0].string_id, 55);
        assert_eq!(table.entries[0].advance_px, 16);

        // Raw page bytes round-trip exactly.
        let raw = std::fs::read(out.join("dialog_font_vram_4bpp.bin")).unwrap();
        assert_eq!(raw, page);

        let _ = std::fs::remove_dir_all(&out);
    }
}
