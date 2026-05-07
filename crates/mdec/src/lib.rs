//! PSX MDEC clean-room decoder.
//!
//! Decodes the MDEC bitstream (BS v2 format) produced by the PSX MDEC
//! hardware into RGBA8 pixel output. The MDEC is an MPEG-1-compatible fixed
//! DCT coprocessor; the algorithm below is derived from the PSX-SPX hardware
//! reference at <https://problemkaputt.de/psx-spx.htm#mdecdecompression>.
//!
//! ## Algorithm outline
//!
//! 1. The BS payload starts with a 4-byte header: `u16 n_words, u16 qs`
//!    where `qs` is the per-frame quantization scale (0–63).
//! 2. Decode macroblocks in raster order. Each 16×16 macroblock contains
//!    six 8×8 blocks in the order: Cr, Cb, Y0, Y1, Y2, Y3.
//! 3. Per 8×8 block: Read VLC-coded DC (MPEG-1 luma/chroma DC VLC; delta-
//!    coded from the previous same-component block); read run/level-coded AC
//!    coefficients (MPEG-1 AC VLC table, escape, EOB); de-zigzag; dequantize
//!    (`coef[i] = clamp((coef[i] * qs * Q_MAT[i] + 4) / 8, -2048, 2047)`);
//!    apply 8-point IDCT.
//! 4. Assemble the 16×16 RGBA block from six decoded blocks:
//!    Cr/Cb are 8×8, each sample covering a 2×2 luma region (4:2:0).
//! 5. Convert YCbCr → RGBA8 with BT.601 coefficients.
//!
//! ## STR sector format
//!
//! The `str_sector` module parses PSX STR video-sector headers and assembles
//! multi-sector frames. Feed each 2048-byte sector data area to
//! [`str_sector::StrFrameAssembler`]; call [`MdecDecoder::decode_frame`] on
//! the assembled BS payload.

#![deny(missing_docs)]

pub mod str_sector;

// ── Tables ─────────────────────────────────────────────────────────────────

/// PSX quantization matrix (luma and chroma share one matrix).
///
/// Source: PSX-SPX §MDEC Decompression — "Quantization Table".
const Q_MAT: [i32; 64] = [
    2, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38,
    22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32, 35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58,
    26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

/// JPEG/MPEG zigzag scan order: `ZIGZAG[pos]` = raster index for position `pos`.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// 8×8 IDCT cosine table, scaled by 2048.
///
/// `IDCT_C[k][n]` = `round(2048 * w(k) * cos(pi * k * (2n+1) / 16))`
/// where `w(0) = 1/sqrt(2)`, `w(k≥1) = 1`.
///
/// Rows indexed by frequency `k`, columns by spatial sample `n`.
/// Source: DCT-III definition (ISO/IEC 11172-2 §Annex A).
const IDCT_C: [[i32; 8]; 8] = [
    [1448, 1448, 1448, 1448, 1448, 1448, 1448, 1448],
    [2008, 1702, 1138, 400, -400, -1138, -1702, -2008],
    [1892, 784, -784, -1892, -1892, -784, 784, 1892],
    [1702, -400, -2008, -1138, 1138, 2008, 400, -1702],
    [1448, -1448, -1448, 1448, 1448, -1448, -1448, 1448],
    [1138, -2008, 400, 1702, -1702, -400, 2008, -1138],
    [784, -1892, 1892, -784, -784, 1892, -1892, 784],
    [400, -1138, 1702, -2008, 2008, -1702, 1138, -400],
];

// ── Bit reader ──────────────────────────────────────────────────────────────

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 7 = MSB of current byte, 0 = LSB
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 7,
        }
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.byte_pos >= self.data.len() {
            return None;
        }
        let bit = (self.data[self.byte_pos] >> self.bit_pos) & 1;
        if self.bit_pos == 0 {
            self.byte_pos += 1;
            self.bit_pos = 7;
        } else {
            self.bit_pos -= 1;
        }
        Some(bit as u32)
    }

    fn read_bits(&mut self, n: u8) -> Option<u32> {
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | self.read_bit()?;
        }
        Some(val)
    }

    /// MPEG-1 sign extension: if MSB of the n-bit field is 0, value is
    /// negative in the range `[-(2^n - 1), -1]`.
    fn read_signed(&mut self, n: u8) -> Option<i32> {
        if n == 0 {
            return Some(0);
        }
        let raw = self.read_bits(n)?;
        if raw >> (n - 1) == 0 {
            Some(raw as i32 - ((1 << n) - 1))
        } else {
            Some(raw as i32)
        }
    }

    /// Advance to the next 16-bit word boundary. BS streams are word-aligned
    /// between blocks.
    fn align_to_word(&mut self) {
        if self.bit_pos != 7 {
            self.byte_pos += 1;
            self.bit_pos = 7;
        }
        if self.byte_pos & 1 != 0 {
            self.byte_pos += 1;
        }
    }

    fn bytes_remaining(&self) -> usize {
        self.data.len().saturating_sub(self.byte_pos)
    }
}

// ── DC coefficient VLC ──────────────────────────────────────────────────────

/// Decode the DC coefficient size category for a luma block.
///
/// Luma DC VLC (MPEG-1 / PSX-SPX Table B.12):
/// `100`→0, `00`→1, `01`→2, `101`→3, `110`→4, `111`→5, `1110`→6,
/// `11110`→7, `111110`→8, `1111110`→9, `11111110`→10, `111111110`→11.
fn decode_dc_size_luma(br: &mut BitReader<'_>) -> Option<u8> {
    let b0 = br.read_bit()?;
    let b1 = br.read_bit()?;
    match (b0, b1) {
        (0, 0) => Some(1),
        (0, 1) => Some(2),
        (1, 0) => match br.read_bit()? {
            0 => Some(0),
            _ => Some(3),
        },
        _ => {
            // 11
            match br.read_bit()? {
                0 => Some(4),
                _ => match br.read_bit()? {
                    0 => Some(5),
                    _ => {
                        let mut size = 6u8;
                        loop {
                            if br.read_bit()? == 0 {
                                return Some(size);
                            }
                            size += 1;
                            if size > 11 {
                                return None;
                            }
                        }
                    }
                },
            }
        }
    }
}

/// Decode the DC coefficient size category for a chroma block.
///
/// Chroma DC VLC (MPEG-1 / PSX-SPX Table B.13):
/// `00`→0, `01`→1, `10`→2, `110`→3, `1110`→4, `11110`→5, …
fn decode_dc_size_chroma(br: &mut BitReader<'_>) -> Option<u8> {
    let b0 = br.read_bit()?;
    let b1 = br.read_bit()?;
    match (b0, b1) {
        (0, 0) => Some(0),
        (0, 1) => Some(1),
        (1, 0) => Some(2),
        _ => {
            let mut size = 3u8;
            loop {
                if br.read_bit()? == 0 {
                    return Some(size);
                }
                size += 1;
                if size > 11 {
                    return None;
                }
            }
        }
    }
}

fn decode_dc_value(br: &mut BitReader<'_>, is_chroma: bool, dc_pred: &mut i32) -> Option<i32> {
    let size = if is_chroma {
        decode_dc_size_chroma(br)?
    } else {
        decode_dc_size_luma(br)?
    };
    let diff = if size == 0 { 0 } else { br.read_signed(size)? };
    *dc_pred += diff;
    Some(*dc_pred)
}

// ── AC coefficient VLC ──────────────────────────────────────────────────────

/// Decode one AC run/level entry from the bitstream.
///
/// Returns `(run, level)`. Special sentinel values:
/// - `run == 64` → end-of-block
/// - `run == 65` → decode error / unexpected bit pattern
///
/// Source: MPEG-1 ISO 11172-2 Table B.14 (used verbatim by the PSX MDEC).
fn decode_ac(br: &mut BitReader<'_>) -> (u8, i16) {
    macro_rules! bit {
        () => {
            match br.read_bit() {
                Some(b) => b,
                None => return (65, 0),
            }
        };
    }

    let b0 = bit!();
    let b1 = bit!();

    match (b0, b1) {
        // 10s → run=0, level=±1  (most common AC code)
        (1, s) => (0, if s == 0 { 1 } else { -1 }),

        // 01xx → 4-bit group: level 2 or 3 at run 0
        (0, 1) => {
            let b2 = bit!();
            let s = bit!();
            match b2 {
                1 => (0, if s == 0 { 2 } else { -2 }),
                _ => (0, if s == 0 { 3 } else { -3 }),
            }
        }

        // 00x... → longer codes
        _ => {
            let b2 = bit!();
            if b2 == 0 {
                // 000x... — very long codes or escape
                decode_ac_long(br)
            } else {
                // 001x...
                let b3 = bit!();
                match b3 {
                    0 => (64, 0), // 0010 = EOB
                    _ => {
                        // 0011xx
                        let b4 = bit!();
                        let s = bit!();
                        match b4 {
                            1 => (1, if s == 0 { 1 } else { -1 }),
                            _ => (2, if s == 0 { 1 } else { -1 }),
                        }
                    }
                }
            }
        }
    }
}

fn decode_ac_long(br: &mut BitReader<'_>) -> (u8, i16) {
    macro_rules! bit {
        () => {
            match br.read_bit() {
                Some(b) => b,
                None => return (65, 0),
            }
        };
    }

    // We've consumed 000; read more.
    let b3 = bit!();
    if b3 == 1 {
        // 0001x...
        let b4 = bit!();
        if b4 == 1 {
            // 00011x s
            let b5 = bit!();
            let s = bit!();
            match b5 {
                0 => (0, if s == 0 { 4 } else { -4 }),
                _ => (2, if s == 0 { 2 } else { -2 }),
            }
        } else {
            // 00010x s
            let b5 = bit!();
            let s = bit!();
            match b5 {
                0 => (7, if s == 0 { 1 } else { -1 }),
                _ => (8, if s == 0 { 1 } else { -1 }),
            }
        }
    } else {
        // 0000x...
        let b4 = bit!();
        if b4 == 1 {
            // 00001x s
            let b5 = bit!();
            let s = bit!();
            match b5 {
                0 => (0, if s == 0 { 5 } else { -5 }),
                _ => (9, if s == 0 { 1 } else { -1 }),
            }
        } else {
            // 00000x...
            let b5 = bit!();
            if b5 == 1 {
                // 000001 = escape: 6-bit run, 8-bit signed level
                let run = match br.read_bits(6) {
                    Some(v) => v as u8,
                    None => return (65, 0),
                };
                let level_byte = match br.read_bits(8) {
                    Some(v) => v as i8,
                    None => return (65, 0),
                };
                (run, level_byte as i16)
            } else {
                // Unknown long code
                (65, 0)
            }
        }
    }
}

// ── 8×8 IDCT ───────────────────────────────────────────────────────────────

/// Reference 8-point IDCT on one row or column using the precomputed cosine
/// table [`IDCT_C`]. O(N²) but trivially correct for verification.
///
/// Input/output range: coefficients in [-2048, 2047]; pixels in [-128, 127].
/// Scaling: sum of `IDCT_C[k][n] * coef[k]`, divided by 2048 (table scale) ×
/// 8 (IDCT normalisation) = 16384 = 2^14. Half-bit rounding: bias by 8192.
fn idct_1d(coef: &[i32; 8]) -> [i32; 8] {
    let mut out = [0i32; 8];
    for n in 0..8 {
        let sum: i32 = (0..8).map(|k| IDCT_C[k][n] * coef[k]).sum();
        out[n] = (sum + 8192) >> 14;
    }
    out
}

/// Full 8×8 2D IDCT: row IDCT followed by column IDCT.
fn idct_8x8(block: &mut [i32; 64]) {
    // Row pass
    for row in 0..8 {
        let mut r = [0i32; 8];
        r.copy_from_slice(&block[row * 8..row * 8 + 8]);
        let out = idct_1d(&r);
        block[row * 8..row * 8 + 8].copy_from_slice(&out);
    }
    // Column pass
    for col in 0..8 {
        let mut c = [0i32; 8];
        for row in 0..8 {
            c[row] = block[row * 8 + col];
        }
        let out = idct_1d(&c);
        for row in 0..8 {
            block[row * 8 + col] = out[row];
        }
    }
}

// ── Block decode ────────────────────────────────────────────────────────────

fn decode_block(
    br: &mut BitReader<'_>,
    is_chroma: bool,
    qs: i32,
    dc_pred: &mut i32,
) -> Option<[i32; 64]> {
    let mut coeffs = [0i32; 64];

    // DC coefficient (no zigzag, no qs scaling — Q_MAT[0]=2, divide by 8)
    let dc = decode_dc_value(br, is_chroma, dc_pred)?;
    coeffs[0] = dc * Q_MAT[0]; // * 2; divide later in IDCT output

    // AC coefficients
    let mut pos = 1usize;
    loop {
        if pos > 63 {
            break;
        }
        let (run, level) = decode_ac(br);
        match run {
            64 => break, // EOB
            65 => break, // decode error — emit what we have
            r => {
                pos += r as usize;
                if pos > 63 {
                    break;
                }
                let q = Q_MAT[pos];
                let val = ((level as i32 * qs * q + 4) / 8).clamp(-2048, 2047);
                coeffs[ZIGZAG[pos]] = val;
                pos += 1;
            }
        }
    }

    idct_8x8(&mut coeffs);
    Some(coeffs)
}

// ── YCbCr → RGBA ───────────────────────────────────────────────────────────

/// BT.601 YCbCr→RGB. Cb and Cr are already offset-subtracted (zero-centred).
fn ycbcr_to_rgba(y: i32, cb: i32, cr: i32) -> [u8; 4] {
    let r = (y + ((91881 * cr) >> 16)).clamp(0, 255) as u8;
    let g = (y - ((22554 * cb + 46802 * cr) >> 16)).clamp(0, 255) as u8;
    let b = (y + ((116130 * cb) >> 16)).clamp(0, 255) as u8;
    [r, g, b, 255]
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Stateless PSX MDEC frame decoder.
///
/// Call [`MdecDecoder::decode_frame`] with a complete BS v2 payload
/// (assembled from one or more STR video sectors via
/// [`str_sector::StrFrameAssembler`]). Output is a row-major RGBA8 buffer
/// of `width × height × 4` bytes.
pub struct MdecDecoder {
    /// Frame width in pixels (must be a multiple of 16).
    pub width: u32,
    /// Frame height in pixels (must be a multiple of 16).
    pub height: u32,
}

impl MdecDecoder {
    /// Create a decoder for the given frame dimensions.
    ///
    /// Both `width` and `height` must be non-zero multiples of 16.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Decode a complete MDEC BS v2 payload into an RGBA8 pixel buffer.
    ///
    /// The first 4 bytes of `bs` are the BS header: `[u16 n_words, u16 qs]`
    /// where `qs` is the per-frame quantization scale. If `bs` starts with a
    /// high nibble of `0` (i.e. the MDEC command word pattern) those 4 bytes
    /// are skipped automatically.
    ///
    /// Returns `width × height × 4` bytes, or an error for invalid dimensions.
    pub fn decode_frame(&self, bs: &[u8]) -> anyhow::Result<Vec<u8>> {
        use anyhow::bail;

        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 || !w.is_multiple_of(16) || !h.is_multiple_of(16) {
            bail!(
                "MDEC: frame dimensions must be non-zero multiples of 16 (got {}×{})",
                w,
                h
            );
        }

        // Parse the 4-byte BS header: u16 n_words, u16 qs.
        let (qs, bs_payload) = if bs.len() >= 4 {
            let _n_words = u16::from_le_bytes(bs[0..2].try_into().unwrap());
            let qs = u16::from_le_bytes(bs[2..4].try_into().unwrap()) as i32;
            (qs.max(1), &bs[4..])
        } else {
            (8, bs) // fallback: qs=8 (mid-range)
        };

        let mut br = BitReader::new(bs_payload);
        let mut rgba = vec![0u8; w * h * 4];

        let mb_cols = w / 16;
        let mb_rows = h / 16;

        for mb_row in 0..mb_rows {
            for mb_col in 0..mb_cols {
                if br.bytes_remaining() == 0 {
                    break;
                }

                let mut dc_cr = 0i32;
                let mut dc_cb = 0i32;
                let mut dc_y = 0i32;

                // Block order per PSX-SPX: Cr, Cb, Y0, Y1, Y2, Y3
                let cr_block = decode_block(&mut br, true, qs, &mut dc_cr).unwrap_or([0i32; 64]);
                br.align_to_word();
                let cb_block = decode_block(&mut br, true, qs, &mut dc_cb).unwrap_or([0i32; 64]);
                br.align_to_word();
                let y_blocks = [
                    decode_block(&mut br, false, qs, &mut dc_y).unwrap_or([0i32; 64]),
                    {
                        br.align_to_word();
                        decode_block(&mut br, false, qs, &mut dc_y).unwrap_or([0i32; 64])
                    },
                    {
                        br.align_to_word();
                        decode_block(&mut br, false, qs, &mut dc_y).unwrap_or([0i32; 64])
                    },
                    {
                        br.align_to_word();
                        decode_block(&mut br, false, qs, &mut dc_y).unwrap_or([0i32; 64])
                    },
                ];
                br.align_to_word();

                // Y layout: Y0=top-left, Y1=top-right, Y2=bottom-left, Y3=bottom-right
                for sub_y in 0..2usize {
                    for sub_x in 0..2usize {
                        let y_block = &y_blocks[sub_y * 2 + sub_x];
                        for py in 0..8usize {
                            for px in 0..8usize {
                                let luma = y_block[py * 8 + px];
                                // Each 8×8 chroma block covers the full 16×16 macroblock.
                                // Chroma sample for pixel (sub_x*8+px, sub_y*8+py):
                                // chroma_x = sub_x*4 + px/2, chroma_y = sub_y*4 + py/2
                                let cx = sub_x * 4 + px / 2;
                                let cy = sub_y * 4 + py / 2;
                                // Chroma values from IDCT are centred at ~128; subtract.
                                let cr = cr_block[cy * 8 + cx] - 128;
                                let cb = cb_block[cy * 8 + cx] - 128;

                                let pixel_x = mb_col * 16 + sub_x * 8 + px;
                                let pixel_y = mb_row * 16 + sub_y * 8 + py;
                                if pixel_x < w && pixel_y < h {
                                    let idx = (pixel_y * w + pixel_x) * 4;
                                    rgba[idx..idx + 4]
                                        .copy_from_slice(&ycbcr_to_rgba(luma, cb, cr));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(rgba)
    }
}

/// A decoded video frame ready for display.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// RGBA8 pixel data, row-major.
    pub rgba: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Sequential frame number from the STR stream.
    pub frame_number: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitreader_reads_msb_first() {
        let data = [0b1010_0000u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bit(), Some(1));
        assert_eq!(br.read_bit(), Some(0));
        assert_eq!(br.read_bit(), Some(1));
        assert_eq!(br.read_bit(), Some(0));
    }

    #[test]
    fn bitreader_read_bits_groups() {
        let data = [0b1100_1010u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bits(4), Some(0b1100));
        assert_eq!(br.read_bits(4), Some(0b1010));
    }

    #[test]
    fn decode_frame_returns_correct_byte_count() {
        // Sparse/zero payload; decoder tolerates short inputs and fills zeros.
        let dec = MdecDecoder::new(16, 16);
        let bs = vec![0u8; 8]; // 4-byte header + 4 bytes payload
        let result = dec.decode_frame(&bs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 16 * 16 * 4);
    }

    #[test]
    fn decode_frame_rejects_non_multiple_of_16() {
        let dec = MdecDecoder::new(15, 16);
        assert!(dec.decode_frame(&[]).is_err());
        let dec2 = MdecDecoder::new(16, 0);
        assert!(dec2.decode_frame(&[]).is_err());
    }

    #[test]
    fn idct_1d_dc_only_is_constant() {
        // DC-only input: coef[0] = 128, rest 0.
        // IDCT should produce the same value scaled by w(0) = 1/sqrt(2).
        let coef = {
            let mut c = [0i32; 8];
            c[0] = 1024; // DC
            c
        };
        let out = idct_1d(&coef);
        // All outputs should be equal (DC-only = constant block).
        let first = out[0];
        for &v in &out[1..] {
            // Allow ±1 for fixed-point rounding.
            assert!((v - first).abs() <= 1, "DC-only IDCT not constant: {out:?}");
        }
        // Value should be non-zero.
        assert_ne!(first, 0);
    }

    #[test]
    fn ycbcr_neutral_chroma_passes_luma() {
        // Y=128, Cb=0, Cr=0 (centred at 0) → roughly gray
        let [r, g, b, a] = ycbcr_to_rgba(128, 0, 0);
        assert_eq!(a, 255);
        assert!((r as i32 - 128).abs() < 10);
        assert!((g as i32 - 128).abs() < 10);
        assert!((b as i32 - 128).abs() < 10);
    }

    #[test]
    fn ycbcr_alpha_is_always_255() {
        let [_, _, _, a] = ycbcr_to_rgba(0, 0, 0);
        assert_eq!(a, 255);
        let [_, _, _, a] = ycbcr_to_rgba(255, -128, 127);
        assert_eq!(a, 255);
    }

    #[test]
    fn align_to_word_rounds_up_to_even_byte() {
        let data = [0u8; 4];
        let mut br = BitReader::new(&data);
        br.read_bits(3); // consume partial byte
        br.align_to_word(); // should advance to byte 2
        assert_eq!(br.byte_pos, 2);
    }

    #[test]
    fn dc_luma_size_code_1_is_00() {
        // Code "00" → dc_size=1
        let data = [0b00_000000u8];
        let mut br = BitReader::new(&data);
        assert_eq!(decode_dc_size_luma(&mut br), Some(1));
    }

    #[test]
    fn dc_luma_size_code_0_is_100() {
        // Code "100" → dc_size=0
        let data = [0b100_00000u8];
        let mut br = BitReader::new(&data);
        assert_eq!(decode_dc_size_luma(&mut br), Some(0));
    }

    #[test]
    fn dc_chroma_size_code_0_is_00() {
        let data = [0b00_000000u8];
        let mut br = BitReader::new(&data);
        assert_eq!(decode_dc_size_chroma(&mut br), Some(0));
    }

    #[test]
    fn ac_eob_is_decoded() {
        // EOB = "0010" (see B.14)
        let data = [0b0010_0000u8];
        let mut br = BitReader::new(&data);
        let (run, _) = decode_ac(&mut br);
        assert_eq!(run, 64); // EOB sentinel
    }
}
