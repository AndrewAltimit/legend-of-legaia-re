//! PSX TIM (texture image) parser and PNG exporter.
//!
//! TIM is Sony's PSX texture format. A file is:
//!
//!   magic   u32  0x00000010
//!   flags   u32  bit0..2 = pmode (0=4bpp, 1=8bpp, 2=16bpp, 3=24bpp), bit3 = has CLUT
//!   [clut block, if bit3 set]
//!     bs_len  u32  byte length of this block, including itself
//!     fb_x    u16  framebuffer X (CLUT load position)
//!     fb_y    u16  framebuffer Y
//!     w       u16  CLUT width  in 16-bit entries
//!     h       u16  CLUT height in rows
//!     data    w*h*2 bytes (rows of 16-bit BGR555 + STP)
//!   image block:
//!     bs_len  u32
//!     fb_x    u16  framebuffer X (image load position, in 16-bit words)
//!     fb_y    u16
//!     w       u16  image width in 16-bit words (NOT pixels for 4/8 bpp)
//!     h       u16  image height in rows
//!     data    w*h*2 bytes (raw pixel data)
//!
//! Pixel widths in real pixels:
//!   4bpp:  fb_w * 4
//!   8bpp:  fb_w * 2
//!   16bpp: fb_w
//!   24bpp: fb_w * 2 / 3 (24-bit packed; 3 bytes per pixel)
//!
//! 16-bit pixels are stored STP|B|G|R (1+5+5+5 bits, little-endian).

use anyhow::{Context, Result, bail};

pub mod vram;
pub use vram::{VRAM_HEIGHT, VRAM_PIXELS, VRAM_WIDTH, Vram};

pub const TIM_MAGIC: u32 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelMode {
    Bpp4,
    Bpp8,
    Bpp16,
    Bpp24,
    Mixed,
}

impl PixelMode {
    fn from_pmode(p: u32) -> Result<Self> {
        match p {
            0 => Ok(PixelMode::Bpp4),
            1 => Ok(PixelMode::Bpp8),
            2 => Ok(PixelMode::Bpp16),
            3 => Ok(PixelMode::Bpp24),
            4 => Ok(PixelMode::Mixed),
            other => bail!("unknown TIM pmode {}", other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clut {
    pub fb_x: u16,
    pub fb_y: u16,
    pub w: u16,
    pub h: u16,
    /// raw 16-bit BGR555+STP entries, row-major
    pub entries: Vec<u16>,
}

impl Clut {
    /// Number of distinct palettes available, given the image's pixel mode.
    ///
    /// PSX convention: a CLUT block stores `w` 16-bit entries per row × `h`
    /// rows. For 4bpp images, palettes are 16 entries each; for 8bpp, 256.
    /// Multiple palettes may be packed into a single wide row.
    pub fn n_palettes(&self, mode: PixelMode) -> usize {
        let per = entries_per_palette(mode);
        if per == 0 {
            return 0;
        }
        (self.w as usize / per) * (self.h as usize)
    }

    /// Get palette `idx` as a slice, given the image's pixel mode.
    pub fn palette(&self, mode: PixelMode, idx: usize) -> Option<&[u16]> {
        let per = entries_per_palette(mode);
        if per == 0 {
            return None;
        }
        let start = idx.checked_mul(per)?;
        let end = start.checked_add(per)?;
        self.entries.get(start..end)
    }
}

fn entries_per_palette(mode: PixelMode) -> usize {
    match mode {
        PixelMode::Bpp4 => 16,
        PixelMode::Bpp8 => 256,
        _ => 0,
    }
}

#[derive(Debug, Clone)]
pub struct Image {
    pub fb_x: u16,
    pub fb_y: u16,
    /// Image width in 16-bit framebuffer units (NOT real pixels for 4/8 bpp).
    pub fb_w: u16,
    pub h: u16,
    pub data: Vec<u8>,
}

impl Image {
    pub fn pixel_width(&self, mode: PixelMode) -> usize {
        match mode {
            PixelMode::Bpp4 => self.fb_w as usize * 4,
            PixelMode::Bpp8 => self.fb_w as usize * 2,
            PixelMode::Bpp16 => self.fb_w as usize,
            PixelMode::Bpp24 => self.fb_w as usize * 2 / 3,
            PixelMode::Mixed => self.fb_w as usize,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tim {
    pub flags: u32,
    pub mode: PixelMode,
    pub clut: Option<Clut>,
    pub image: Image,
}

impl Tim {
    pub fn pixel_width(&self) -> usize {
        self.image.pixel_width(self.mode)
    }

    pub fn pixel_height(&self) -> usize {
        self.image.h as usize
    }

    /// Total bytes the TIM occupies: 8-byte header + optional CLUT block +
    /// image block (each block is `12 + w*h*2` bytes). For a strict-parsed
    /// TIM (exact block lengths) this is the precise on-disc footprint.
    pub fn byte_extent(&self) -> usize {
        let mut end = 8;
        if let Some(c) = &self.clut {
            end += 12 + (c.w as usize) * (c.h as usize) * 2;
        }
        end += 12 + (self.image.fb_w as usize) * (self.image.h as usize) * 2;
        end
    }

    /// Number of CLUT palettes available for this TIM's pixel mode (0 for
    /// 16/24-bpp images, which carry no CLUT).
    pub fn palette_count(&self) -> usize {
        self.clut.as_ref().map_or(0, |c| c.n_palettes(self.mode))
    }
}

/// Convert a raw 16-bit PSX color (BGR555 + STP) to RGBA8888.
/// STP is treated as opaque-by-default; black pixels with STP=0 are transparent.
pub fn bgr555_to_rgba8(c: u16) -> [u8; 4] {
    let r5 = (c & 0x1F) as u8;
    let g5 = ((c >> 5) & 0x1F) as u8;
    let b5 = ((c >> 10) & 0x1F) as u8;
    let stp = (c >> 15) & 1;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g5 << 3) | (g5 >> 2);
    let b = (b5 << 3) | (b5 >> 2);
    let a = if c == 0x0000 && stp == 0 { 0 } else { 255 };
    [r, g, b, a]
}

pub fn parse(buf: &[u8]) -> Result<Tim> {
    parse_with(buf, false)
}

/// Strict TIM parse that reproduces jPSXdec's TIM detector exactly.
///
/// On top of the lenient [`parse`] checks it additionally requires:
/// - **No reserved flag bits.** Only bits 0..3 (pmode + has-CLUT) may be set.
/// - **A real pixel mode.** `pmode` must be 0..=3; the `Mixed` (pmode 4)
///   pseudo-mode is rejected.
/// - **Exact block lengths.** Each block's `bs_len` must equal
///   `12 + w*h*2` precisely (the lenient parser tolerates trailing padding).
/// - **In-bounds VRAM rectangles.** The image (and CLUT, if present) must fit
///   inside the 1024x512 16-bit framebuffer at its declared load position.
///
/// This is the validation level used to build the PROT.DAT TIM catalog: a
/// flat scan of PROT.DAT with `parse_strict` recovers byte-for-byte the same
/// item set (offsets, dimensions, palette counts) that jPSXdec reports. The
/// lenient [`parse`] is retained for callers that decode bytes already known
/// to be a TIM (web-viewer thumbnails, sub-asset extraction) where the extra
/// rejections would only get in the way.
pub fn parse_strict(buf: &[u8]) -> Result<Tim> {
    parse_with(buf, true)
}

/// VRAM is 1024 16-bit pixels wide by 512 rows; every TIM rectangle (image
/// and CLUT) loads somewhere inside it.
pub const VRAM_FB_WIDTH: u16 = 1024;
pub const VRAM_FB_HEIGHT: u16 = 512;

fn parse_with(buf: &[u8], strict: bool) -> Result<Tim> {
    if buf.len() < 8 {
        bail!("buffer too small for TIM header");
    }
    let magic = u32_le(buf, 0);
    if magic != TIM_MAGIC {
        bail!(
            "not a TIM (magic = 0x{:08x}, expected 0x{:08x})",
            magic,
            TIM_MAGIC
        );
    }
    let flags = u32_le(buf, 4);
    if strict && flags & !0xF != 0 {
        bail!("reserved TIM flag bits set: 0x{:08x}", flags);
    }
    let pmode = flags & 0x7;
    if strict && pmode > 3 {
        bail!("non-standard TIM pmode {} (strict allows 0..=3)", pmode);
    }
    let has_clut = (flags >> 3) & 1 == 1;
    let mode = PixelMode::from_pmode(pmode)?;

    let mut cur = 8;

    let clut = if has_clut {
        if buf.len() < cur + 12 {
            bail!("buffer too small for CLUT block header");
        }
        let bs_len = u32_le(buf, cur) as usize;
        if bs_len < 12 || bs_len > buf.len() - cur {
            bail!(
                "invalid CLUT block length {} (cur={}, buf={})",
                bs_len,
                cur,
                buf.len()
            );
        }
        let fb_x = u16_le(buf, cur + 4);
        let fb_y = u16_le(buf, cur + 6);
        let w = u16_le(buf, cur + 8);
        let h = u16_le(buf, cur + 10);
        let data_bytes = bs_len - 12;
        let expected = (w as usize) * (h as usize) * 2;
        if expected > data_bytes {
            bail!(
                "CLUT data length mismatch: w*h*2={}, bs_len-12={}",
                expected,
                data_bytes
            );
        }
        if strict {
            if expected != data_bytes {
                bail!(
                    "CLUT block length not exact: w*h*2={}, bs_len-12={}",
                    expected,
                    data_bytes
                );
            }
            if w == 0 || h == 0 {
                bail!("zero-dimension CLUT block ({}x{})", w, h);
            }
            // NOTE: the CLUT rectangle is deliberately NOT VRAM-bounds-checked.
            // Legaia stores many NPC palettes at fb_y 510..511 (the row-479+
            // CLUT band) with h up to 16, so the block legitimately extends a
            // few rows past the 512-row framebuffer edge. jPSXdec accepts
            // these; only the image rectangle is bounds-checked.
        }
        let mut entries = Vec::with_capacity(expected / 2);
        for i in 0..(expected / 2) {
            entries.push(u16_le(buf, cur + 12 + i * 2));
        }
        cur += bs_len;
        Some(Clut {
            fb_x,
            fb_y,
            w,
            h,
            entries,
        })
    } else {
        None
    };

    if buf.len() < cur + 12 {
        bail!("buffer too small for image block header");
    }
    let bs_len = u32_le(buf, cur) as usize;
    if bs_len < 12 || bs_len > buf.len() - cur {
        bail!(
            "invalid image block length {} (cur={}, buf={})",
            bs_len,
            cur,
            buf.len()
        );
    }
    let fb_x = u16_le(buf, cur + 4);
    let fb_y = u16_le(buf, cur + 6);
    let fb_w = u16_le(buf, cur + 8);
    let h = u16_le(buf, cur + 10);
    let data_bytes = bs_len - 12;
    let expected = (fb_w as usize) * (h as usize) * 2;
    if expected > data_bytes {
        bail!(
            "image data length mismatch: w*h*2={}, bs_len-12={}",
            expected,
            data_bytes
        );
    }
    if strict {
        if expected != data_bytes {
            bail!(
                "image block length not exact: w*h*2={}, bs_len-12={}",
                expected,
                data_bytes
            );
        }
        if fb_w == 0 || h == 0 {
            bail!("zero-dimension image block ({}x{})", fb_w, h);
        }
        check_vram_bounds("image", fb_x, fb_y, fb_w, h)?;
    }
    let data = buf[cur + 12..cur + 12 + expected].to_vec();
    let image = Image {
        fb_x,
        fb_y,
        fb_w,
        h,
        data,
    };

    Ok(Tim {
        flags,
        mode,
        clut,
        image,
    })
}

/// A TIM rectangle (in 16-bit framebuffer units) must load fully inside VRAM.
fn check_vram_bounds(which: &str, fb_x: u16, fb_y: u16, w: u16, h: u16) -> Result<()> {
    let right = (fb_x as u32) + (w as u32);
    let bottom = (fb_y as u32) + (h as u32);
    if right > VRAM_FB_WIDTH as u32 || bottom > VRAM_FB_HEIGHT as u32 {
        bail!(
            "{} rect ({},{})+{}x{} exceeds {}x{} VRAM",
            which,
            fb_x,
            fb_y,
            w,
            h,
            VRAM_FB_WIDTH,
            VRAM_FB_HEIGHT
        );
    }
    Ok(())
}

/// Decode a TIM into row-major RGBA8 pixel data.
///
/// `clut_idx` selects which CLUT row to use for indexed modes. If the TIM has
/// no CLUT (16/24 bpp), `clut_idx` is ignored.
pub fn decode_rgba8(tim: &Tim, clut_idx: usize) -> Result<Vec<u8>> {
    let w = tim.pixel_width();
    let h = tim.pixel_height();
    // `w`/`h` derive from the public `fb_w`/`h` header fields, which a caller
    // can set to arbitrary values when constructing a `Tim` by hand (the
    // struct's fields are `pub`, and the web-viewer software rasteriser builds
    // `Tim`/`Image` directly). Validate the output footprint with checked
    // arithmetic before reserving so a bogus dimension can't capacity-overflow
    // panic; surface a graceful `Err` instead.
    let out_len = w
        .checked_mul(h)
        .and_then(|p| p.checked_mul(4))
        .context("TIM output dimensions overflow")?;
    let mut out = Vec::with_capacity(out_len);
    match tim.mode {
        PixelMode::Bpp4 => {
            let clut = tim.clut.as_ref().context("4bpp TIM requires a CLUT")?;
            let palette = clut.palette(tim.mode, clut_idx).with_context(|| {
                format!(
                    "palette {} out of range (entries={})",
                    clut_idx,
                    clut.entries.len()
                )
            })?;
            for row in 0..h {
                for col in 0..w {
                    let byte_off = row * (tim.image.fb_w as usize * 2) + col / 2;
                    let byte = *tim.image.data.get(byte_off).with_context(|| {
                        format!(
                            "4bpp pixel ({},{}) byte offset {} past image data ({})",
                            row,
                            col,
                            byte_off,
                            tim.image.data.len()
                        )
                    })?;
                    let nibble = if col & 1 == 0 {
                        byte & 0x0F
                    } else {
                        (byte >> 4) & 0x0F
                    };
                    let entry = *palette.get(nibble as usize).with_context(|| {
                        format!("nibble {} >= palette len {}", nibble, palette.len())
                    })?;
                    out.extend_from_slice(&bgr555_to_rgba8(entry));
                }
            }
        }
        PixelMode::Bpp8 => {
            let clut = tim.clut.as_ref().context("8bpp TIM requires a CLUT")?;
            let palette = clut.palette(tim.mode, clut_idx).with_context(|| {
                format!(
                    "palette {} out of range (entries={})",
                    clut_idx,
                    clut.entries.len()
                )
            })?;
            for row in 0..h {
                for col in 0..w {
                    let byte_off = row * (tim.image.fb_w as usize * 2) + col;
                    let idx = *tim.image.data.get(byte_off).with_context(|| {
                        format!(
                            "8bpp pixel ({},{}) byte offset {} past image data ({})",
                            row,
                            col,
                            byte_off,
                            tim.image.data.len()
                        )
                    })? as usize;
                    let entry = *palette.get(idx).with_context(|| {
                        format!("index {} >= palette len {}", idx, palette.len())
                    })?;
                    out.extend_from_slice(&bgr555_to_rgba8(entry));
                }
            }
        }
        PixelMode::Bpp16 => {
            for row in 0..h {
                for col in 0..w {
                    let byte_off = (row * w + col) * 2;
                    if byte_off + 2 > tim.image.data.len() {
                        bail!("16bpp pixel ({},{}) out of range", row, col);
                    }
                    let entry = u16_le(&tim.image.data, byte_off);
                    out.extend_from_slice(&bgr555_to_rgba8(entry));
                }
            }
        }
        PixelMode::Bpp24 => {
            for row in 0..h {
                for col in 0..w {
                    let byte_off = (row * w + col) * 3;
                    if byte_off + 3 > tim.image.data.len() {
                        bail!("24bpp pixel ({},{}) out of range", row, col);
                    }
                    let r = tim.image.data[byte_off];
                    let g = tim.image.data[byte_off + 1];
                    let b = tim.image.data[byte_off + 2];
                    out.extend_from_slice(&[r, g, b, 255]);
                }
            }
        }
        PixelMode::Mixed => bail!("mixed-mode TIM decoding not implemented"),
    }
    Ok(out)
}

/// Encode RGBA8 pixel data to a PNG file.
pub fn write_png(path: &std::path::Path, width: usize, height: usize, rgba: &[u8]) -> Result<()> {
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let w = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().context("writing PNG header")?;
    writer
        .write_image_data(rgba)
        .context("writing PNG image data")?;
    Ok(())
}

#[inline]
fn u32_le(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

#[inline]
fn u16_le(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_tim_4bpp() -> Vec<u8> {
        let mut buf = vec![];
        // header
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 + has CLUT
        // clut block: 12 + 16*2 = 44
        buf.extend_from_slice(&44u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // fb_x
        buf.extend_from_slice(&0u16.to_le_bytes()); // fb_y
        buf.extend_from_slice(&16u16.to_le_bytes()); // w (16 entries)
        buf.extend_from_slice(&1u16.to_le_bytes()); // h (1 row)
        for i in 0..16u16 {
            // store distinct red intensities (i * 2 in 5-bit red field)
            let r5 = (i * 2) & 0x1F;
            let entry = r5; // green=blue=0
            buf.extend_from_slice(&entry.to_le_bytes());
        }
        // image block: 4x4 pixels @ 4bpp = 4 bytes per row × 4 rows = 16 bytes
        // fb_w in 16-bit words: 4 pixels at 4bpp = 2 bytes = 1 word per row
        // bs_len = 12 + 1 * 4 * 2 = 12 + 8 = 20
        buf.extend_from_slice(&20u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // fb_w = 1 (4 pixels)
        buf.extend_from_slice(&4u16.to_le_bytes()); // h = 4
        // 4 rows × 2 bytes per row of pixel indices
        // row 0: 0,1,2,3
        // row 1: 4,5,6,7
        // row 2: 8,9,A,B
        // row 3: C,D,E,F
        buf.extend_from_slice(&[0x10, 0x32]);
        buf.extend_from_slice(&[0x54, 0x76]);
        buf.extend_from_slice(&[0x98, 0xBA]);
        buf.extend_from_slice(&[0xDC, 0xFE]);
        buf
    }

    #[test]
    fn parses_4bpp() {
        let buf = build_tim_4bpp();
        let tim = parse(&buf).unwrap();
        assert_eq!(tim.mode, PixelMode::Bpp4);
        assert_eq!(tim.pixel_width(), 4);
        assert_eq!(tim.pixel_height(), 4);
        let clut = tim.clut.as_ref().unwrap();
        assert_eq!(clut.entries.len(), 16);
    }

    #[test]
    fn decodes_4bpp_rgba() {
        let buf = build_tim_4bpp();
        let tim = parse(&buf).unwrap();
        let rgba = decode_rgba8(&tim, 0).unwrap();
        assert_eq!(rgba.len(), 4 * 4 * 4);
        // pixel (0,0) = palette index 0 = entry r5=0 → black, alpha 0 (since u16=0 and STP=0)
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        // pixel (0,1) = palette index 1 = r5=2 → r = (2<<3)|(2>>2) = 16
        assert_eq!(&rgba[4..8], &[16, 0, 0, 255]);
        // pixel (3,3) = palette index 15 = r5=30 → r = (30<<3)|(30>>2) = 247
        assert_eq!(&rgba[(15 * 4)..(16 * 4)], &[247, 0, 0, 255]);
    }

    #[test]
    fn n_palettes_packs_4bpp() {
        // CLUT with w=256, h=1: real-world Legaia layout, 16 palettes of 16 colors.
        let mut buf = vec![];
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 + has CLUT
        let clut_data_bytes = 256 * 2;
        buf.extend_from_slice(&((12 + clut_data_bytes) as u32).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&256u16.to_le_bytes()); // w=256
        buf.extend_from_slice(&1u16.to_le_bytes()); // h=1
        buf.extend(std::iter::repeat_n(0u8, clut_data_bytes));
        // minimal image block (1x1 pixels at 4bpp, fb_w=1)
        buf.extend_from_slice(&14u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&[0u8, 0u8]);
        let tim = parse(&buf).unwrap();
        let clut = tim.clut.as_ref().unwrap();
        assert_eq!(clut.n_palettes(PixelMode::Bpp4), 16);
        assert_eq!(clut.n_palettes(PixelMode::Bpp8), 1);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = build_tim_4bpp();
        buf[0] = 0xFF;
        assert!(parse(&buf).is_err());
    }

    // --- Strict-parse (jPSXdec-parity) regression ------------------------

    #[test]
    fn strict_accepts_well_formed_tim() {
        let buf = build_tim_4bpp();
        assert!(parse_strict(&buf).is_ok());
        let tim = parse_strict(&buf).unwrap();
        assert_eq!(tim.byte_extent(), buf.len());
    }

    #[test]
    fn strict_rejects_reserved_flag_bits() {
        // The PROT.DAT init_data "TIM" at +0x10 carries flags 0x00010008:
        // pmode 0 + has-CLUT, but with reserved bit 16 set. jPSXdec rejects
        // it (verified by feeding the carved slab to jPSXdec); strict must too.
        let mut buf = build_tim_4bpp();
        buf[4..8].copy_from_slice(&0x0001_0008u32.to_le_bytes());
        assert!(parse(&buf).is_ok(), "lenient parse still accepts it");
        assert!(parse_strict(&buf).is_err(), "strict rejects reserved bits");
    }

    #[test]
    fn strict_rejects_trailing_padding_in_block() {
        // Pad the image block length by 16 bytes: lenient parse tolerates the
        // padding, strict requires an exact 12 + w*h*2 block.
        let mut buf = build_tim_4bpp();
        // image block bs_len is the last 4-byte LE word before the image header
        // fields; locate it by re-deriving the layout.
        let img_bs_off = 8 + 44; // header + CLUT block
        let bs = u32_le(&buf, img_bs_off);
        buf[img_bs_off..img_bs_off + 4].copy_from_slice(&(bs + 16).to_le_bytes());
        buf.extend(std::iter::repeat_n(0u8, 16));
        assert!(parse(&buf).is_ok());
        assert!(parse_strict(&buf).is_err());
    }

    #[test]
    fn strict_rejects_out_of_vram_image() {
        let mut buf = build_tim_4bpp();
        // image fb_x sits at +8+44+4 (after CLUT block + image bs_len). Push it
        // past the right VRAM edge.
        let img_fbx_off = 8 + 44 + 4;
        buf[img_fbx_off..img_fbx_off + 2].copy_from_slice(&1024u16.to_le_bytes());
        assert!(parse_strict(&buf).is_err());
    }

    // --- Panic-hardening regression tests ---------------------------------
    //
    // The web viewer and bulk scanners feed ARBITRARY PROT-entry / LZS-section
    // bytes into `parse`, and the software rasteriser constructs `Tim`/`Image`
    // values directly (the fields are `pub`). A junk magic match or a
    // hand-built `Tim` with inconsistent dimensions must return `Err`, never
    // panic (OOB slice, capacity overflow, etc.).

    #[test]
    fn empty_input_is_err_not_panic() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn one_byte_input_is_err_not_panic() {
        assert!(parse(&[0x10]).is_err());
    }

    #[test]
    fn truncated_header_is_err_not_panic() {
        // Valid magic but only 4 of the 8 header bytes present.
        assert!(parse(&0x10u32.to_le_bytes()).is_err());
    }

    #[test]
    fn truncated_after_flags_is_err_not_panic() {
        // magic + flags (claims has-CLUT) but no CLUT block at all.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 + has CLUT
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn bogus_huge_clut_block_len_is_err_not_panic() {
        // Valid magic + has-CLUT flag, then a CLUT block length far past EOF.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes());
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // bs_len
        buf.extend_from_slice(&0u16.to_le_bytes()); // fb_x
        buf.extend_from_slice(&0u16.to_le_bytes()); // fb_y
        buf.extend_from_slice(&0xFFFFu16.to_le_bytes()); // w
        buf.extend_from_slice(&0xFFFFu16.to_le_bytes()); // h
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn bogus_huge_image_block_len_is_err_not_panic() {
        // No CLUT; image block claims an enormous bs_len.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2 = 16bpp, no CLUT
        buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // bs_len
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
        buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn valid_magic_garbage_body_is_err_not_panic() {
        // Valid TIM magic followed by random-looking bytes for the rest.
        let mut buf = vec![0x10u8, 0, 0, 0];
        buf.extend(std::iter::repeat_n(0xA5u8, 60));
        // Whatever the flags byte decodes to, the result must be Ok or Err -
        // never a panic. (pmode 5..7 -> Err; has-CLUT with junk lengths -> Err.)
        let _ = parse(&buf);
    }

    #[test]
    fn decode_rejects_handbuilt_tim_with_short_data() {
        // A caller builds a 16bpp Tim claiming 64x64 but supplies no pixels.
        let tim = Tim {
            flags: 0x02,
            mode: PixelMode::Bpp16,
            clut: None,
            image: Image {
                fb_x: 0,
                fb_y: 0,
                fb_w: 64,
                h: 64,
                data: Vec::new(),
            },
        };
        assert!(decode_rgba8(&tim, 0).is_err());
    }

    #[test]
    fn decode_rejects_handbuilt_8bpp_short_data() {
        let clut = Clut {
            fb_x: 0,
            fb_y: 0,
            w: 256,
            h: 1,
            entries: vec![0u16; 256],
        };
        let tim = Tim {
            flags: 0x09,
            mode: PixelMode::Bpp8,
            clut: Some(clut),
            image: Image {
                fb_x: 0,
                fb_y: 0,
                fb_w: 128,
                h: 64,
                data: Vec::new(), // claims 128*2*64 px but has none
            },
        };
        assert!(decode_rgba8(&tim, 0).is_err());
    }

    #[test]
    fn decode_rejects_handbuilt_dimension_overflow() {
        // fb_w/h chosen so pixel_width()*h*4 overflows usize on 64-bit.
        let tim = Tim {
            flags: 0x02,
            mode: PixelMode::Bpp16,
            clut: None,
            image: Image {
                fb_x: 0,
                fb_y: 0,
                fb_w: u16::MAX,
                h: u16::MAX,
                data: Vec::new(),
            },
        };
        // 16bpp: w*h*4 = 65535*65535*4 fits usize on 64-bit, so this returns the
        // short-data Err; the point is no panic on the reserve.
        assert!(decode_rgba8(&tim, 0).is_err());
    }

    #[test]
    fn bgr555_basics() {
        // pure red: r5=31, others 0 → r=(31<<3)|7 = 255
        assert_eq!(bgr555_to_rgba8(0x001F), [255, 0, 0, 255]);
        // pure green: g5=31 in bits 5..9
        assert_eq!(bgr555_to_rgba8(0x03E0), [0, 255, 0, 255]);
        // pure blue: b5=31 in bits 10..14
        assert_eq!(bgr555_to_rgba8(0x7C00), [0, 0, 255, 255]);
        // STP-only set, otherwise 0 → opaque black
        assert_eq!(bgr555_to_rgba8(0x8000), [0, 0, 0, 255]);
        // all zeros → transparent
        assert_eq!(bgr555_to_rgba8(0x0000), [0, 0, 0, 0]);
    }
}
