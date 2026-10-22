//! Proportional dialog-font loader and layout helper.
//!
//! PORT: FUN_80036888, FUN_80035F04, FUN_8003CC98, FUN_8003CD00
//! REF: FUN_80036044 (typewriter glyph count; ported in `glyph_count`)
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

pub mod accent_font;
pub mod builtin;
pub mod cell_text;
mod disc_source;
pub mod escape_icons;
mod escape_table;
mod extracted_dir;
pub mod glyph_count;
pub mod latin;
mod layout;
pub mod limits;
pub mod measure;
#[cfg(test)]
mod tests;

use disc_source::*;
pub use disc_source::{FONT_SHADOW_ALPHA, FONT_SHADOW_TEXEL};
pub use escape_table::{EscapeEntry, EscapeTable};
use extracted_dir::{decode_atlas_png, parse_widths_csv};
pub use extracted_dir::{export_extracted_font_dir, extracted_font_dir};
pub use glyph_count::{GlyphCount, typewriter_glyph_count};
pub use limits::{TEXT_LIMITS, TextLimit, limit_for};
pub use measure::{MeasureOptions, PenItem, TextMeasure};

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
    /// The `0xCE` escape sprites appended below the glyph cells
    /// ([`Font::with_escape_icons`]), indexed by escape operand. Empty until
    /// attached, in which case an escape lays out as nothing.
    escapes: Vec<Option<EscapeCell>>,
    /// Menu sprite cells appended below the glyphs
    /// ([`Font::with_sprite_cells`]) - the fixed-width numerals and the list
    /// pager pieces retail draws as sprites rather than as font glyphs - by
    /// [`sprite_cell_ids`] id.
    sprite_cells: std::collections::BTreeMap<u16, EscapeCell>,
}

/// Ids of the menu sprite cells a host can attach with
/// [`Font::with_sprite_cells`].
pub mod sprite_cell_ids {
    /// The 8x12 menu numeral for digit `d` is `NUMERAL + d` - the cells the
    /// fixed-width number primitive `FUN_80034B78` blits (menu-glyph page
    /// `(960, 256)`, `uv = (d * 8, 208)`).
    pub const NUMERAL: u16 = 0x100;
    /// The 8x12 separator glyph `g` (`0..14`) is `SEPARATOR + g` - the cells
    /// the separator primitive `FUN_8003C1F8` blits, on the numerals' row
    /// right after the digits (`uv = (0x50 + g * 8, 208)`): `x + - ±`, the
    /// rise / fall arrows (`4` / `5`), `/ ( ) :`.
    pub const SEPARATOR: u16 = 0x110;
    // `SEPARATOR + 13` is the last; `0x11E..` is free.
    /// The list page header's "PAGE" tag (UI-icon `0x76`, 24x8).
    pub const PAGE_TAG: u16 = 0x200;
    /// The page header's 6x8 digit `d` is `PAGE_DIGIT + d` (icons
    /// `0x7A..=0x83`).
    pub const PAGE_DIGIT: u16 = 0x210;
    /// The page header's slash (icon `0x79`).
    pub const PAGE_SLASH: u16 = 0x220;
    /// The list's previous-page triangle (icon `0x27`, 8x8).
    pub const PAGE_PREV: u16 = 0x230;
    /// The list's next-page triangle (icon `0x28`, 8x8).
    pub const PAGE_NEXT: u16 = 0x231;
}

/// One sprite cell's pixels for [`Font::with_sprite_cells`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteCellSrc {
    pub id: u16,
    pub w: u32,
    pub h: u32,
    /// RGBA8, `w * h * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Where one `0xCE` escape's sprite sits in the atlas, and how it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeCell {
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub w: u32,
    pub h: u32,
    /// Offset from the line's pen y (`FUN_80036888` adds it at `0x800369F0`).
    pub y_offset: i8,
    /// Pen advance after the sprite (the table's `+2` byte, added at
    /// `0x80036A10` with no inter-glyph spacing).
    pub advance: u8,
}

/// The escape operand byte a [`LaidGlyph`] for a `0xCE` sprite carries in
/// [`LaidGlyph::byte`]: a sprite is drawn with its own colours, never tinted
/// by the text pen (`FUN_8002C488` emits it at `0x808080`).
pub const ESCAPE_GLYPH_BYTE: u8 = 0xCE;

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
        // The shadow texels stay dark, at [`FONT_SHADOW_TEXEL`]: the grey
        // that lands on retail's fixed `(33,33,33)` shadow under the tier-7
        // ink, whatever tone the extraction baked them in.
        for px in atlas_rgba.as_chunks_mut::<4>().0 {
            if px[3] == 0 {
                continue;
            }
            let v = if px[0] >= 0x80 && px[1] >= 0x80 && px[2] >= 0x80 {
                0xFF
            } else {
                FONT_SHADOW_TEXEL
            };
            px[0] = v;
            px[1] = v;
            px[2] = v;
            if v != 0xFF {
                px[3] = FONT_SHADOW_ALPHA;
            }
        }
        Ok(Self {
            widths,
            atlas_rgba,
            atlas_w,
            atlas_h,
            escapes: Vec::new(),
            sprite_cells: Default::default(),
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
            escapes: Vec::new(),
            sprite_cells: Default::default(),
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
            escapes: Vec::new(),
            sprite_cells: Default::default(),
        })
    }

    /// Append the `0xCE` escape sprites (controller buttons and icons,
    /// [`escape_icons::EscapeIcons`]) to the atlas, so [`Font::layout`]
    /// places them where retail's line renderer `FUN_80036888` draws them:
    /// at the pen, `y_offset` down, advancing the pen by the escape table's
    /// own width. The atlas grows downward; a host reads its size off
    /// [`Font::atlas_dimensions`] as always.
    // PORT: FUN_80036888 (0x80036968..0x80036A10, the sprite-escape arm)
    pub fn with_escape_icons(mut self, icons: &escape_icons::EscapeIcons) -> Self {
        let (aw, base_h) = (self.atlas_w, self.atlas_h);
        let (mut x, mut y, mut row_h) = (0u32, base_h, 0u32);
        let mut cells = Vec::with_capacity(icons.icons.len());
        for icon in &icons.icons {
            let Some(icon) = icon.as_ref().filter(|i| i.w > 0 && i.w <= aw) else {
                cells.push(None);
                continue;
            };
            if x + icon.w > aw {
                x = 0;
                y += row_h;
                row_h = 0;
            }
            cells.push(Some((x, y, icon)));
            x += icon.w;
            row_h = row_h.max(icon.h);
        }
        let new_h = y + row_h;
        self.atlas_rgba.resize((aw * new_h * 4) as usize, 0);
        self.escapes = cells
            .into_iter()
            .map(|c| {
                c.map(|(cx, cy, icon)| {
                    for r in 0..icon.h {
                        let src = (r * icon.w * 4) as usize;
                        let dst = (((cy + r) * aw + cx) * 4) as usize;
                        let n = (icon.w * 4) as usize;
                        self.atlas_rgba[dst..dst + n].copy_from_slice(&icon.rgba[src..src + n]);
                    }
                    EscapeCell {
                        atlas_x: cx,
                        atlas_y: cy,
                        w: icon.w,
                        h: icon.h,
                        y_offset: icon.y_offset,
                        advance: icon.advance,
                    }
                })
            })
            .collect();
        self.atlas_h = new_h;
        self
    }

    /// [`Font::with_escape_icons`] straight off the disc: `prot_head` is
    /// `PROT.DAT` from [`escape_icons::ICON_PROT_DAT_OFFSET`] (at least
    /// [`escape_icons::ICON_PROT_DAT_LEN`] bytes), `scus` the executable. A
    /// source that does not decode leaves the font as it was (escapes lay
    /// out as nothing).
    pub fn with_escape_icons_from_disc(self, prot_head: &[u8], scus: &[u8]) -> Self {
        match escape_icons::EscapeIcons::from_disc(prot_head, scus) {
            Ok(icons) => self.with_escape_icons(&icons),
            Err(_) => self,
        }
    }

    /// Append menu sprite cells ([`sprite_cell_ids`]) below the atlas, the
    /// way [`Font::with_escape_icons`] appends the escape sprites, so a menu
    /// text pass can draw them off the same texture as its glyphs. A cell
    /// wider than the atlas or with short pixels is skipped.
    pub fn with_sprite_cells(mut self, cells: &[SpriteCellSrc]) -> Self {
        let (aw, base_h) = (self.atlas_w, self.atlas_h);
        let (mut x, mut y, mut row_h) = (0u32, base_h, 0u32);
        let mut placed = Vec::new();
        for c in cells {
            if c.w == 0 || c.w > aw || c.rgba.len() < (c.w * c.h * 4) as usize {
                continue;
            }
            if x + c.w > aw {
                x = 0;
                y += row_h;
                row_h = 0;
            }
            placed.push((x, y, c));
            x += c.w;
            row_h = row_h.max(c.h);
        }
        let new_h = y + row_h;
        self.atlas_rgba.resize((aw * new_h * 4) as usize, 0);
        for (cx, cy, c) in placed {
            for r in 0..c.h {
                let src = (r * c.w * 4) as usize;
                let dst = (((cy + r) * aw + cx) * 4) as usize;
                let n = (c.w * 4) as usize;
                self.atlas_rgba[dst..dst + n].copy_from_slice(&c.rgba[src..src + n]);
            }
            self.sprite_cells.insert(
                c.id,
                EscapeCell {
                    atlas_x: cx,
                    atlas_y: cy,
                    w: c.w,
                    h: c.h,
                    y_offset: 0,
                    advance: c.w as u8,
                },
            );
        }
        self.atlas_h = new_h;
        self
    }

    /// The atlas cell of menu sprite `id` ([`sprite_cell_ids`]), when
    /// attached ([`Font::with_sprite_cells`]).
    pub fn sprite_cell(&self, id: u16) -> Option<EscapeCell> {
        self.sprite_cells.get(&id).copied()
    }

    /// The atlas cell of escape operand `index`, when the sprites are
    /// attached ([`Font::with_escape_icons`]).
    pub fn escape_cell(&self, index: u8) -> Option<EscapeCell> {
        self.escapes.get(index as usize).copied().flatten()
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
        escapes: Vec::new(),
        sprite_cells: Default::default(),
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
