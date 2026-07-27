//! PNG -> TIM encoder for same-size texture replacement.
//!
//! Builds a new TIM whose *structure* is copied verbatim from an original
//! (pixel mode, image / CLUT dimensions, and every `fb_x`/`fb_y` VRAM
//! placement field) and whose *pixels* come from caller-supplied RGBA8 data -
//! typically a decoded PNG. Same dimensions + bpp + CLUT layout means the
//! encoded TIM is byte-for-byte the same size as the original, which is what
//! same-size in-place disc patching requires.
//!
//! ## Alpha -> STP mapping
//!
//! PSX 16-bit texels carry a 1-bit STP (semi-transparency) flag, not an alpha
//! channel. The encoder maps 8-bit alpha onto it as:
//!
//! - `a == 0`   -> `0x0000` (transparent black - the GPU skips the texel; RGB
//!   is ignored),
//! - `0 < a < 255` -> STP set + RGB truncated to 5 bits per channel (draws
//!   semi-transparent when the primitive enables blending),
//! - `a == 255` -> STP clear + RGB truncated, **except** opaque pure black,
//!   which becomes `0x8000` (STP-only black) - a plain `0x0000` would read
//!   back as transparent.
//!
//! ## Original-color reuse
//!
//! Before the alpha rule applies, the encoder reuses the original TIM
//! wherever the new image asks for a color the original already has (compared
//! in decoded-RGBA space): a pixel whose position held the same color keeps
//! its original index / 16-bit texel verbatim, and other palette hits reuse
//! the first entry that decodes to the color. This preserves the original's
//! STP choices for untouched regions and makes `encode(decode(tim))` on an
//! unmodified export reproduce the original TIM byte-for-byte.
//!
//! ## Palettes
//!
//! Indexed modes (4/8 bpp) must fit the palette: at most 16 / 256 distinct
//! 15-bit colors. Colors already in the original palette are free; new colors
//! overwrite palette slots the new image no longer references. Overflow is an
//! error listing offending pixel coordinates, unless
//! [`EncodeOptions::quantize`] maps the least-frequent extra colors to their
//! nearest palette entry. When any palette slot changes, the rebuilt palette
//! is replicated into every CLUT row (multi-palette variants would otherwise
//! recolor the new indices with stale rows); an untouched palette keeps the
//! whole original CLUT block byte-identical.

use std::collections::HashMap;
use std::fmt;

use anyhow::{Context, Result};

use crate::{Clut, PixelMode, Tim, bgr555_to_rgba8};

/// Options for [`encode_replacement`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EncodeOptions {
    /// When the new image holds more distinct colors than the palette fits,
    /// map the least-frequent extras to their nearest palette color instead
    /// of failing. Off by default: overflow is a hard error listing the
    /// offending pixels.
    pub quantize: bool,
}

/// One offending pixel in a [`EncodeError::TooManyColors`] report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorSample {
    pub x: u32,
    pub y: u32,
    /// The pixel's RGBA as supplied by the caller (not the 15-bit rounding).
    pub rgba: [u8; 4],
}

/// Structured encoding failure. `Display` renders the full human-readable
/// report (including per-pixel samples), so callers can surface it directly.
#[derive(Debug)]
pub enum EncodeError {
    /// The replacement image must match the original TIM's pixel dimensions.
    DimensionMismatch {
        expected_w: usize,
        expected_h: usize,
        got_w: usize,
        got_h: usize,
    },
    /// `rgba.len()` disagrees with `w * h * 4`.
    PixelBufferSize { expected: usize, got: usize },
    /// A 4/8 bpp original with no (or a too-short) CLUT block.
    MissingClut { needed: usize, have: usize },
    /// `Mixed` pseudo-mode TIMs are not encodable.
    UnsupportedMode,
    /// More distinct 15-bit colors than the palette holds and quantization is
    /// off. `needed` counts distinct colors in the new image, `capacity` the
    /// palette size; `samples` are the first pixels of colors that got no
    /// slot, `overflow` the total count of slotless colors.
    TooManyColors {
        capacity: usize,
        needed: usize,
        overflow: usize,
        samples: Vec<ColorSample>,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::DimensionMismatch {
                expected_w,
                expected_h,
                got_w,
                got_h,
            } => write!(
                f,
                "replacement image is {got_w}x{got_h} but the original TIM is \
                 {expected_w}x{expected_h}; the replacement must match exactly \
                 (same-size in-place patching)"
            ),
            EncodeError::PixelBufferSize { expected, got } => write!(
                f,
                "RGBA buffer holds {got} bytes, expected {expected} (w*h*4)"
            ),
            EncodeError::MissingClut { needed, have } => write!(
                f,
                "indexed-mode TIM needs a CLUT of at least {needed} entries (found {have})"
            ),
            EncodeError::UnsupportedMode => {
                write!(f, "mixed-mode TIMs cannot be encoded")
            }
            EncodeError::TooManyColors {
                capacity,
                needed,
                overflow,
                samples,
            } => {
                write!(
                    f,
                    "image uses {needed} distinct colors but the palette holds only \
                     {capacity}; {overflow} color(s) have no slot. Reduce the color \
                     count or enable quantization. Offending pixels:"
                )?;
                for s in samples {
                    write!(
                        f,
                        "\n  ({}, {}) rgba({}, {}, {}, {})",
                        s.x, s.y, s.rgba[0], s.rgba[1], s.rgba[2], s.rgba[3]
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Result of a successful encode.
#[derive(Debug, Clone)]
pub struct Encoded {
    /// The full serialized TIM, byte-for-byte the original's size.
    pub bytes: Vec<u8>,
    /// Palette slots overwritten with colors the original palette lacked
    /// (always 0 for 16/24 bpp).
    pub new_palette_entries: usize,
    /// Pixels approximated to a nearest palette color (only ever non-zero
    /// with [`EncodeOptions::quantize`]).
    pub quantized_pixels: usize,
    /// Whether the rebuilt palette was replicated into every CLUT row (true
    /// exactly when `new_palette_entries > 0` on a multi-row CLUT... or any
    /// palette slot changed).
    pub clut_rows_rewritten: bool,
}

/// Map one RGBA8 pixel to a PSX 15-bit texel + STP bit (see module docs).
pub fn rgba8_to_bgr555(px: [u8; 4]) -> u16 {
    let [r, g, b, a] = px;
    if a == 0 {
        return 0x0000;
    }
    let c = ((r as u16) >> 3) | (((g as u16) >> 3) << 5) | (((b as u16) >> 3) << 10);
    if a < 255 {
        c | 0x8000 // semi-transparent: STP set
    } else if c == 0 {
        0x8000 // opaque black: STP-only, else it would read as transparent
    } else {
        c
    }
}

/// Decode a PNG (any color type / bit depth) into `(width, height, RGBA8)`.
pub fn decode_png_rgba(png_bytes: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().context("read PNG header")?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).context("decode PNG frame")?;
    let (w, h) = (info.width as usize, info.height as usize);
    let n = w * h;
    let data = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(n * 4);
            for p in data.chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(n * 4);
            for p in data.chunks_exact(2) {
                out.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(n * 4);
            for &g in data {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        other => anyhow::bail!("unsupported PNG color type {other:?}"),
    };
    if rgba.len() != n * 4 {
        anyhow::bail!("PNG decode size mismatch: {} != {}", rgba.len(), n * 4);
    }
    Ok((w, h, rgba))
}

/// Serialize a [`Tim`] back to bytes with exact block lengths (the inverse of
/// [`crate::parse_strict`]: `serialize(&parse_strict(x)?)? == x` for every
/// strict-valid TIM).
pub fn serialize(tim: &Tim) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(tim.byte_extent());
    out.extend_from_slice(&crate::TIM_MAGIC.to_le_bytes());
    out.extend_from_slice(&tim.flags.to_le_bytes());
    if let Some(c) = &tim.clut {
        let expected = c.w as usize * c.h as usize;
        if c.entries.len() != expected {
            anyhow::bail!(
                "CLUT entries ({}) disagree with w*h ({expected})",
                c.entries.len()
            );
        }
        let bs = 12 + expected * 2;
        out.extend_from_slice(&(bs as u32).to_le_bytes());
        out.extend_from_slice(&c.fb_x.to_le_bytes());
        out.extend_from_slice(&c.fb_y.to_le_bytes());
        out.extend_from_slice(&c.w.to_le_bytes());
        out.extend_from_slice(&c.h.to_le_bytes());
        for &e in &c.entries {
            out.extend_from_slice(&e.to_le_bytes());
        }
    }
    let img = &tim.image;
    let expected = img.fb_w as usize * img.h as usize * 2;
    if img.data.len() != expected {
        anyhow::bail!(
            "image data ({}) disagrees with fb_w*h*2 ({expected})",
            img.data.len()
        );
    }
    let bs = 12 + expected;
    out.extend_from_slice(&(bs as u32).to_le_bytes());
    out.extend_from_slice(&img.fb_x.to_le_bytes());
    out.extend_from_slice(&img.fb_y.to_le_bytes());
    out.extend_from_slice(&img.fb_w.to_le_bytes());
    out.extend_from_slice(&img.h.to_le_bytes());
    out.extend_from_slice(&img.data);
    Ok(out)
}

/// Encode `rgba` (row-major, `w*h*4` bytes) into a TIM structurally identical
/// to `original` (mode, dimensions, VRAM placement, CLUT layout - see module
/// docs). The output is byte-for-byte the same size as the original.
pub fn encode_replacement(
    original: &Tim,
    rgba: &[u8],
    w: usize,
    h: usize,
    opts: &EncodeOptions,
) -> Result<Encoded, EncodeError> {
    let (ew, eh) = (original.pixel_width(), original.pixel_height());
    if (w, h) != (ew, eh) {
        return Err(EncodeError::DimensionMismatch {
            expected_w: ew,
            expected_h: eh,
            got_w: w,
            got_h: h,
        });
    }
    if rgba.len() != w * h * 4 {
        return Err(EncodeError::PixelBufferSize {
            expected: w * h * 4,
            got: rgba.len(),
        });
    }
    let px = |i: usize| -> [u8; 4] { rgba[i * 4..i * 4 + 4].try_into().unwrap() };

    let mut tim = original.clone();
    let mut new_palette_entries = 0usize;
    let mut quantized_pixels = 0usize;
    let mut clut_rows_rewritten = false;

    match original.mode {
        PixelMode::Bpp16 => {
            let mut data = Vec::with_capacity(w * h * 2);
            for i in 0..w * h {
                let o = u16::from_le_bytes([
                    original.image.data[i * 2],
                    original.image.data[i * 2 + 1],
                ]);
                let t = rgba8_to_bgr555(px(i));
                // Positional reuse: keep the original texel (its STP intact)
                // when it already displays the requested color.
                let keep = bgr555_to_rgba8(o) == bgr555_to_rgba8(t);
                data.extend_from_slice(&(if keep { o } else { t }).to_le_bytes());
            }
            tim.image.data = data;
        }
        PixelMode::Bpp24 => {
            // 24bpp has no STP bit; alpha is ignored.
            let mut data = original.image.data.clone();
            for i in 0..w * h {
                let p = px(i);
                data[i * 3..i * 3 + 3].copy_from_slice(&p[..3]);
            }
            tim.image.data = data;
        }
        PixelMode::Bpp4 | PixelMode::Bpp8 => {
            let per = if original.mode == PixelMode::Bpp4 {
                16
            } else {
                256
            };
            let have = original.clut.as_ref().map_or(0, |c| c.entries.len());
            let Some(orig_pal) = original
                .clut
                .as_ref()
                .and_then(|c| c.palette(original.mode, 0))
            else {
                return Err(EncodeError::MissingClut { needed: per, have });
            };
            let (idx, pal, stats) =
                index_against_palette(original, orig_pal, per, rgba, w, h, opts)?;
            new_palette_entries = stats.new_entries;
            quantized_pixels = stats.quantized;

            // Rebuild the CLUT only when a slot changed; otherwise the whole
            // block stays byte-identical (multi-row variants included).
            if pal != orig_pal {
                clut_rows_rewritten = true;
                let clut = tim.clut.as_mut().expect("indexed TIM has a CLUT");
                replicate_palette(clut, &pal, per);
            }

            // Pack indices into the image block (row stride fb_w*2 bytes).
            let stride = original.image.fb_w as usize * 2;
            let mut data = vec![0u8; stride * h];
            match original.mode {
                PixelMode::Bpp4 => {
                    for row in 0..h {
                        for col in 0..w {
                            let v = idx[row * w + col] as u8 & 0x0F;
                            let b = &mut data[row * stride + col / 2];
                            if col & 1 == 0 {
                                *b |= v;
                            } else {
                                *b |= v << 4;
                            }
                        }
                    }
                }
                _ => {
                    for row in 0..h {
                        for col in 0..w {
                            data[row * stride + col] = idx[row * w + col] as u8;
                        }
                    }
                }
            }
            tim.image.data = data;
        }
        PixelMode::Mixed => return Err(EncodeError::UnsupportedMode),
    }

    let bytes = serialize(&tim).expect("structure copied from a parsed TIM serializes");
    debug_assert_eq!(bytes.len(), original.byte_extent());
    Ok(Encoded {
        bytes,
        new_palette_entries,
        quantized_pixels,
        clut_rows_rewritten,
    })
}

struct IndexStats {
    new_entries: usize,
    quantized: usize,
}

/// Core of the indexed-mode encode: assign every pixel a palette index,
/// reusing the original palette / indices where the colors match and
/// allocating freed slots for new colors. Returns `(indices, final palette,
/// stats)`.
fn index_against_palette(
    original: &Tim,
    orig_pal: &[u16],
    per: usize,
    rgba: &[u8],
    w: usize,
    h: usize,
    opts: &EncodeOptions,
) -> Result<(Vec<usize>, Vec<u16>, IndexStats), EncodeError> {
    let mut pal: Vec<u16> = orig_pal.to_vec();
    let decoded: Vec<[u8; 4]> = pal.iter().map(|&e| bgr555_to_rgba8(e)).collect();

    // The original per-pixel index (same dimensions as the replacement).
    let stride = original.image.fb_w as usize * 2;
    let orig_index = |row: usize, col: usize| -> usize {
        match original.mode {
            PixelMode::Bpp4 => {
                let byte = original.image.data[row * stride + col / 2];
                (if col & 1 == 0 { byte & 0x0F } else { byte >> 4 }) as usize
            }
            _ => original.image.data[row * stride + col] as usize,
        }
    };

    // Per-pixel target texel + its canonical decoded color (what the pixel
    // will display). Matching runs in canonical space so 8-bit colors that
    // round to the same 15-bit value share one entry.
    let n = w * h;
    let mut target = Vec::with_capacity(n);
    let mut canon = Vec::with_capacity(n);
    for i in 0..n {
        let t = rgba8_to_bgr555(rgba[i * 4..i * 4 + 4].try_into().unwrap());
        target.push(t);
        canon.push(bgr555_to_rgba8(t));
    }

    // First entry per decoded color (first-match rule).
    let mut first_slot: HashMap<[u8; 4], usize> = HashMap::new();
    for (s, &d) in decoded.iter().enumerate() {
        first_slot.entry(d).or_insert(s);
    }

    const UNSET: usize = usize::MAX;
    let mut idx = vec![UNSET; n];
    let mut used = vec![false; per];

    // Pass A: positional reuse - a pixel already displaying the requested
    // color keeps its original index (and thus the entry's STP bit).
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            let oi = orig_index(row, col);
            if oi < per && decoded[oi] == canon[i] {
                idx[i] = oi;
                used[oi] = true;
            }
        }
    }
    // Pass B: first palette entry that decodes to the color.
    for i in 0..n {
        if idx[i] == UNSET
            && let Some(&s) = first_slot.get(&canon[i])
        {
            idx[i] = s;
            used[s] = true;
        }
    }
    // Pass C: colors the original palette lacks, in first-appearance order.
    struct Pending {
        texel: u16,
        first: (usize, usize), // (x, y) of first occurrence
        first_rgba: [u8; 4],
        pixels: Vec<usize>,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut pending_by_canon: HashMap<[u8; 4], usize> = HashMap::new();
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            if idx[i] != UNSET {
                continue;
            }
            let k = canon[i];
            let p = *pending_by_canon.entry(k).or_insert_with(|| {
                pending.push(Pending {
                    texel: target[i],
                    first: (col, row),
                    first_rgba: rgba[i * 4..i * 4 + 4].try_into().unwrap(),
                    pixels: Vec::new(),
                });
                pending.len() - 1
            });
            pending[p].pixels.push(i);
        }
    }

    let free: Vec<usize> = (0..per).filter(|&s| !used[s]).collect();
    let mut new_entries = 0usize;
    let mut quantized = 0usize;

    if pending.len() > free.len() && !opts.quantize {
        let matched: usize = used.iter().filter(|&&u| u).count();
        let overflow = pending.len() - free.len();
        // Sample the first pixel of each color that would get no slot (the
        // least frequent ones - the same set quantization would fold).
        let mut order: Vec<usize> = (0..pending.len()).collect();
        order.sort_by_key(|&p| std::cmp::Reverse(pending[p].pixels.len()));
        let samples = order[free.len()..]
            .iter()
            .take(8)
            .map(|&p| ColorSample {
                x: pending[p].first.0 as u32,
                y: pending[p].first.1 as u32,
                rgba: pending[p].first_rgba,
            })
            .collect();
        return Err(EncodeError::TooManyColors {
            capacity: per,
            needed: matched + pending.len(),
            overflow,
            samples,
        });
    }

    // Most frequent colors win slots; the remainder (quantize mode only) maps
    // to the nearest live entry.
    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by_key(|&p| std::cmp::Reverse(pending[p].pixels.len()));
    for (rank, &p) in order.iter().enumerate() {
        if rank < free.len() {
            let slot = free[rank];
            pal[slot] = pending[p].texel;
            used[slot] = true;
            new_entries += 1;
            for &i in &pending[p].pixels {
                idx[i] = slot;
            }
        } else {
            // Nearest live entry, transparency class respected when possible.
            let want = bgr555_to_rgba8(pending[p].texel);
            let live: Vec<usize> = (0..per).filter(|&s| used[s]).collect();
            let classed: Vec<usize> = live
                .iter()
                .copied()
                .filter(|&s| (bgr555_to_rgba8(pal[s])[3] == 0) == (want[3] == 0))
                .collect();
            let candidates = if classed.is_empty() { &live } else { &classed };
            let nearest = candidates
                .iter()
                .copied()
                .min_by_key(|&s| {
                    let d = bgr555_to_rgba8(pal[s]);
                    let dr = d[0] as i32 - want[0] as i32;
                    let dg = d[1] as i32 - want[1] as i32;
                    let db = d[2] as i32 - want[2] as i32;
                    dr * dr + dg * dg + db * db
                })
                .expect("at least one live palette slot");
            quantized += pending[p].pixels.len();
            for &i in &pending[p].pixels {
                idx[i] = nearest;
            }
        }
    }

    debug_assert!(idx.iter().all(|&i| i < per));
    Ok((
        idx,
        pal,
        IndexStats {
            new_entries,
            quantized,
        },
    ))
}

/// Write `pal` into every palette-sized chunk of the CLUT (the flat chunk
/// layout [`Clut::palette`] reads). A trailing partial chunk, if the CLUT's
/// `w*h` is not a multiple of the palette size, keeps its original entries.
fn replicate_palette(clut: &mut Clut, pal: &[u16], per: usize) {
    let n_pal = clut.entries.len() / per;
    for p in 0..n_pal {
        clut.entries[p * per..(p + 1) * per].copy_from_slice(pal);
    }
}

/// Convenience: full replacement pipeline over raw bytes. Parses `original
/// TIM bytes` strictly, decodes nothing itself - `rgba` is the caller's -
/// and returns the encoded TIM. Exposed for callers that hold bytes rather
/// than a parsed [`Tim`].
pub fn encode_replacement_bytes(
    original_tim_bytes: &[u8],
    rgba: &[u8],
    w: usize,
    h: usize,
    opts: &EncodeOptions,
) -> Result<Encoded> {
    let original = crate::parse_strict(original_tim_bytes)
        .context("original bytes are not a strict-valid TIM")?;
    let enc = encode_replacement(&original, rgba, w, h, opts)?;
    Ok(enc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_rgba8, parse, parse_strict};

    /// A 4bpp 4x4 TIM with a 16-entry CLUT (2 palette-relevant colors + STP
    /// variants) and a second CLUT row to exercise replication.
    fn tim_4bpp_two_rows() -> Vec<u8> {
        let mut buf = vec![];
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 + CLUT
        // CLUT block: w=16, h=2 -> 12 + 64
        buf.extend_from_slice(&(12u32 + 64).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // fb_x
        buf.extend_from_slice(&479u16.to_le_bytes()); // fb_y (row-479 style)
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        // Row 0: red (STP set), green, opaque black (0x8000), rest zeros.
        let row0: [u16; 16] = [
            0x801F, 0x03E0, 0x8000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        // Row 1: a variant tint.
        let row1: [u16; 16] = [
            0x7C00, 0x001F, 0x8000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        for e in row0.iter().chain(row1.iter()) {
            buf.extend_from_slice(&e.to_le_bytes());
        }
        // Image: 4x4 px @4bpp -> fb_w=1, h=4; indices 0,1,2,0 per row.
        buf.extend_from_slice(&20u32.to_le_bytes());
        buf.extend_from_slice(&64u16.to_le_bytes()); // fb_x
        buf.extend_from_slice(&32u16.to_le_bytes()); // fb_y
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        for _ in 0..4 {
            buf.extend_from_slice(&[0x10, 0x02]); // 0,1,2,0
        }
        buf
    }

    /// 16bpp 2x2 TIM with an STP-set colored pixel.
    fn tim_16bpp() -> Vec<u8> {
        let mut buf = vec![];
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2, no CLUT
        buf.extend_from_slice(&(12u32 + 8).to_le_bytes());
        buf.extend_from_slice(&100u16.to_le_bytes());
        buf.extend_from_slice(&200u16.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        for e in [0x001Fu16, 0x83E0, 0x0000, 0x8000] {
            buf.extend_from_slice(&e.to_le_bytes());
        }
        buf
    }

    #[test]
    fn serialize_inverts_strict_parse() {
        for bytes in [tim_4bpp_two_rows(), tim_16bpp()] {
            let tim = parse_strict(&bytes).unwrap();
            assert_eq!(serialize(&tim).unwrap(), bytes);
        }
    }

    #[test]
    fn reencode_of_own_decode_is_byte_identical() {
        for bytes in [tim_4bpp_two_rows(), tim_16bpp()] {
            let tim = parse_strict(&bytes).unwrap();
            let rgba = decode_rgba8(&tim, 0).unwrap();
            let enc = encode_replacement(
                &tim,
                &rgba,
                tim.pixel_width(),
                tim.pixel_height(),
                &EncodeOptions::default(),
            )
            .unwrap();
            assert_eq!(enc.bytes, bytes, "no-op re-encode must be byte-exact");
            assert_eq!(enc.new_palette_entries, 0);
            assert!(!enc.clut_rows_rewritten);
        }
    }

    #[test]
    fn new_color_lands_in_a_free_slot_and_replicates_rows() {
        let bytes = tim_4bpp_two_rows();
        let tim = parse_strict(&bytes).unwrap();
        let mut rgba = decode_rgba8(&tim, 0).unwrap();
        // Repaint pixel (1,0) (originally green) pure blue - not in the palette.
        rgba[4..8].copy_from_slice(&[0, 0, 255, 255]);
        let enc = encode_replacement(&tim, &rgba, 4, 4, &EncodeOptions::default()).unwrap();
        assert_eq!(enc.bytes.len(), bytes.len());
        assert_eq!(enc.new_palette_entries, 1);
        assert!(enc.clut_rows_rewritten);
        let out = parse(&enc.bytes).unwrap();
        let out_rgba = decode_rgba8(&out, 0).unwrap();
        assert_eq!(&out_rgba[4..8], &[0, 0, 255, 255]);
        // Every untouched pixel still displays its original color.
        let orig_rgba = decode_rgba8(&tim, 0).unwrap();
        for i in 0..16 {
            if i == 1 {
                continue;
            }
            assert_eq!(&out_rgba[i * 4..i * 4 + 4], &orig_rgba[i * 4..i * 4 + 4]);
        }
        // Both CLUT rows now carry the rebuilt palette (replication).
        let clut = out.clut.as_ref().unwrap();
        assert_eq!(&clut.entries[..16], &clut.entries[16..32]);
    }

    /// An 8x8 4bpp TIM (64 pixels - enough to overflow a 16-slot palette).
    fn tim_4bpp_8x8() -> Vec<u8> {
        let mut buf = vec![];
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes());
        buf.extend_from_slice(&(12u32 + 32).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend(std::iter::repeat_n(0u8, 32)); // all-zero palette
        // Image: 8x8 @4bpp -> fb_w=2, h=8; all indices 0.
        buf.extend_from_slice(&(12u32 + 32).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&8u16.to_le_bytes());
        buf.extend(std::iter::repeat_n(0u8, 32));
        buf
    }

    /// 64 visually distinct 15-bit colors (one per pixel of the 8x8 image).
    fn rgba_64_distinct() -> Vec<u8> {
        let mut rgba = Vec::new();
        for i in 0..64u16 {
            let r = ((i % 8) * 32) as u8;
            let g = ((i / 8) * 32) as u8;
            rgba.extend_from_slice(&[r, g, 200, 255]);
        }
        rgba
    }

    #[test]
    fn too_many_colors_lists_offending_pixels() {
        let tim = parse_strict(&tim_4bpp_8x8()).unwrap();
        let err = encode_replacement(&tim, &rgba_64_distinct(), 8, 8, &EncodeOptions::default())
            .unwrap_err();
        match &err {
            EncodeError::TooManyColors {
                capacity, samples, ..
            } => {
                assert_eq!(*capacity, 16);
                assert!(!samples.is_empty());
            }
            other => panic!("expected TooManyColors, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("distinct colors"), "message: {msg}");
        assert!(msg.contains("rgba("), "message lists pixels: {msg}");
    }

    #[test]
    fn quantize_folds_extra_colors_instead_of_failing() {
        let bytes = tim_4bpp_8x8();
        let tim = parse_strict(&bytes).unwrap();
        let enc = encode_replacement(
            &tim,
            &rgba_64_distinct(),
            8,
            8,
            &EncodeOptions { quantize: true },
        )
        .unwrap();
        assert!(enc.quantized_pixels > 0);
        assert_eq!(enc.bytes.len(), bytes.len());
        assert!(parse_strict(&enc.bytes).is_ok());
    }

    #[test]
    fn dimension_mismatch_is_reported() {
        let tim = parse_strict(&tim_4bpp_two_rows()).unwrap();
        let rgba = vec![0u8; 8 * 8 * 4];
        let err = encode_replacement(&tim, &rgba, 8, 8, &EncodeOptions::default()).unwrap_err();
        assert!(matches!(err, EncodeError::DimensionMismatch { .. }));
        assert!(err.to_string().contains("4x4"));
    }

    #[test]
    fn alpha_rules_map_to_stp() {
        // transparent
        assert_eq!(rgba8_to_bgr555([10, 20, 30, 0]), 0x0000);
        // semi-transparent colored -> STP + color
        assert_eq!(rgba8_to_bgr555([255, 0, 0, 128]), 0x801F);
        // opaque colored -> plain color
        assert_eq!(rgba8_to_bgr555([255, 0, 0, 255]), 0x001F);
        // opaque black -> STP-only black (not transparent)
        assert_eq!(rgba8_to_bgr555([0, 0, 0, 255]), 0x8000);
        // semi-transparent black -> STP-only black too
        assert_eq!(rgba8_to_bgr555([0, 0, 0, 100]), 0x8000);
    }

    #[test]
    fn sixteen_bpp_replacement_is_pixel_exact() {
        let bytes = tim_16bpp();
        let tim = parse_strict(&bytes).unwrap();
        let rgba: Vec<u8> = vec![
            0, 255, 0, 255, // green
            0, 0, 0, 0, // transparent
            123, 45, 67, 255, // arbitrary
            0, 0, 0, 255, // opaque black
        ];
        let enc = encode_replacement(&tim, &rgba, 2, 2, &EncodeOptions::default()).unwrap();
        let out = parse(&enc.bytes).unwrap();
        let got = decode_rgba8(&out, 0).unwrap();
        // Each pixel decodes to the 15-bit rounding of what was asked.
        assert_eq!(&got[0..4], &[0, 255, 0, 255]);
        assert_eq!(&got[4..8], &[0, 0, 0, 0]);
        assert_eq!(got[15], 255); // opaque black stayed opaque
    }

    #[test]
    fn png_round_trip_decodes() {
        // Encode a tiny RGBA PNG in memory and decode it back.
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 0, 0, 0, 0])
                .unwrap();
        }
        let (w, h, rgba) = decode_png_rgba(&png_bytes).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(rgba[7], 128);
    }
}
