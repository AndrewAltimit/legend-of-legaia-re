//! PSX MDEC clean-room decoder for Legend of Legaia FMV.
//!
//! Legaia's `MV*.STR` movies use the PSX **"Iki"** bitstream variant (the same
//! family jPSXdec decodes for a handful of titles), *not* the common STRv2
//! layout. The two differ in how the per-block DC/quantization data is carried:
//!
//! - **STRv2**: a per-frame quantization scale in the header, then each block's
//!   DC is a 10-bit value inside the entropy bitstream.
//! - **Iki** (Legaia): a separate **LZSS-compressed lookup table** (right after
//!   the frame header) holds the `(qscale, DC)` pair for *every* block; the
//!   entropy bitstream then carries only the AC coefficients.
//!
//! The frame header is therefore 10 bytes:
//! `[u16 mdec_code_count][u16 0x3800][u16 width][u16 height][u16 lzss_size]`
//! (little-endian). The compressed qscale/DC table occupies the next
//! `lzss_size` bytes; the AC bitstream begins immediately after it.
//!
//! ## Algorithm
//!
//! 1. Parse the 10-byte Iki header (magic `0x3800` at offset 2; `lzss_size` at
//!    offset 8).
//! 2. LZSS-decompress `lzss_size` bytes into a `block_count * 2`-byte table.
//!    For block `i` the packed word is `(table[i] << 8) | table[i + block_count]`;
//!    its top 6 bits are the quant scale and its low 10 bits the signed DC.
//! 3. Read the AC bitstream as 16-bit little-endian words, MSB-first within each
//!    word. Per block: AC run/level codes from the PSX VLC table (`AC_CODES`),
//!    an escape (`000001` + 16-bit raw `run<<10 | signed10 level`), terminated
//!    by the End-of-Block code `10`.
//! 4. Dequantize (`DC * Q_MAT[0]`; `AC = (level * Q_MAT[zz] * qscale + 4) / 8`),
//!    de-zigzag, run an 8x8 IDCT, and convert YCbCr -> RGBA (PSX MDEC output is
//!    signed/zero-centred, so `+128` is added to the final RGB).
//! 5. Blocks within a macroblock are ordered Cr, Cb, Y0, Y1, Y2, Y3.
//!    **Macroblocks are laid out column-major** (top-to-bottom down a column,
//!    then the next column to the right).
//!
//! ## STR sector format
//!
//! The `str_sector` module parses PSX STR video-sector headers and assembles
//! multi-sector frames. Feed each 2048-byte sector data area to
//! [`str_sector::StrFrameAssembler`]; call [`MdecDecoder::decode_frame`] on
//! the assembled frame payload (header + LZSS table + AC bitstream).

#![deny(missing_docs)]

pub mod str_sector;

// ── Tables ─────────────────────────────────────────────────────────────────

/// PSX quantization matrix (luma and chroma share one matrix).
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

/// 8x8 IDCT cosine table, scaled by 2048 (`round(2048 * w(k) * cos(...))`,
/// `w(0)=1/sqrt(2)`). Combined with the `>> 24` final shift in [`idct_8x8`]
/// this gives the standard normalisation where a DC-only block resolves to
/// `coef[0] / 8` (`1448^2 / 2^24 ~= 0.125`).
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

/// PSX AC run/level VLC table: `(nbits, code, run, level)`. Every entry is
/// followed by a sign bit (`0` = positive). End-of-Block (`10`) and the escape
/// (`000001`) are handled separately. The packed MDEC value for an entry is
/// `run << 10 | level`. Derived from the PSX-SPX "Huffman codes for AC values"
/// table and jPSXdec's format documentation.
#[rustfmt::skip]
const AC_CODES: &[(u32, u32, u8, i16)] = &[
    (2, 0b11, 0, 1),
    (3, 0b011, 1, 1),
    (4, 0b0100, 0, 2),
    (4, 0b0101, 2, 1),
    (5, 0b00110, 4, 1),
    (5, 0b00111, 3, 1),
    (5, 0b00101, 0, 3),
    (6, 0b000100, 7, 1),
    (6, 0b000101, 6, 1),
    (6, 0b000110, 1, 2),
    (6, 0b000111, 5, 1),
    (7, 0b0000100, 2, 2),
    (7, 0b0000101, 9, 1),
    (7, 0b0000110, 0, 4),
    (7, 0b0000111, 8, 1),
    (8, 0b00100000, 13, 1),
    (8, 0b00100001, 0, 6),
    (8, 0b00100010, 12, 1),
    (8, 0b00100011, 11, 1),
    (8, 0b00100100, 3, 2),
    (8, 0b00100101, 1, 3),
    (8, 0b00100110, 0, 5),
    (8, 0b00100111, 10, 1),
    (10, 0b0000001000, 16, 1),
    (10, 0b0000001001, 5, 2),
    (10, 0b0000001010, 0, 7),
    (10, 0b0000001011, 2, 3),
    (10, 0b0000001100, 1, 4),
    (10, 0b0000001101, 15, 1),
    (10, 0b0000001110, 14, 1),
    (10, 0b0000001111, 4, 2),
    (12, 0b000000010000, 0, 11),
    (12, 0b000000010001, 8, 2),
    (12, 0b000000010010, 4, 3),
    (12, 0b000000010011, 0, 10),
    (12, 0b000000010100, 2, 4),
    (12, 0b000000010101, 7, 2),
    (12, 0b000000010110, 21, 1),
    (12, 0b000000010111, 20, 1),
    (12, 0b000000011000, 0, 9),
    (12, 0b000000011001, 19, 1),
    (12, 0b000000011010, 18, 1),
    (12, 0b000000011011, 1, 5),
    (12, 0b000000011100, 3, 3),
    (12, 0b000000011101, 0, 8),
    (12, 0b000000011110, 6, 2),
    (12, 0b000000011111, 17, 1),
    (13, 0b0000000010000, 10, 2),
    (13, 0b0000000010001, 9, 2),
    (13, 0b0000000010010, 5, 3),
    (13, 0b0000000010011, 3, 4),
    (13, 0b0000000010100, 2, 5),
    (13, 0b0000000010101, 1, 7),
    (13, 0b0000000010110, 1, 6),
    (13, 0b0000000010111, 0, 15),
    (13, 0b0000000011000, 0, 14),
    (13, 0b0000000011001, 0, 13),
    (13, 0b0000000011010, 0, 12),
    (13, 0b0000000011011, 26, 1),
    (13, 0b0000000011100, 25, 1),
    (13, 0b0000000011101, 24, 1),
    (13, 0b0000000011110, 23, 1),
    (13, 0b0000000011111, 22, 1),
    (14, 0b00000000010000, 0, 31),
    (14, 0b00000000010001, 0, 30),
    (14, 0b00000000010010, 0, 29),
    (14, 0b00000000010011, 0, 28),
    (14, 0b00000000010100, 0, 27),
    (14, 0b00000000010101, 0, 26),
    (14, 0b00000000010110, 0, 25),
    (14, 0b00000000010111, 0, 24),
    (14, 0b00000000011000, 0, 23),
    (14, 0b00000000011001, 0, 22),
    (14, 0b00000000011010, 0, 21),
    (14, 0b00000000011011, 0, 20),
    (14, 0b00000000011100, 0, 19),
    (14, 0b00000000011101, 0, 18),
    (14, 0b00000000011110, 0, 17),
    (14, 0b00000000011111, 0, 16),
    (15, 0b000000000010000, 0, 40),
    (15, 0b000000000010001, 0, 39),
    (15, 0b000000000010010, 0, 38),
    (15, 0b000000000010011, 0, 37),
    (15, 0b000000000010100, 0, 36),
    (15, 0b000000000010101, 0, 35),
    (15, 0b000000000010110, 0, 34),
    (15, 0b000000000010111, 0, 33),
    (15, 0b000000000011000, 0, 32),
    (15, 0b000000000011001, 1, 14),
    (15, 0b000000000011010, 1, 13),
    (15, 0b000000000011011, 1, 12),
    (15, 0b000000000011100, 1, 11),
    (15, 0b000000000011101, 1, 10),
    (15, 0b000000000011110, 1, 9),
    (15, 0b000000000011111, 1, 8),
    (16, 0b0000000000010000, 1, 18),
    (16, 0b0000000000010001, 1, 17),
    (16, 0b0000000000010010, 1, 16),
    (16, 0b0000000000010011, 1, 15),
    (16, 0b0000000000010100, 6, 3),
    (16, 0b0000000000010101, 16, 2),
    (16, 0b0000000000010110, 15, 2),
    (16, 0b0000000000010111, 14, 2),
    (16, 0b0000000000011000, 13, 2),
    (16, 0b0000000000011001, 12, 2),
    (16, 0b0000000000011010, 11, 2),
    (16, 0b0000000000011011, 31, 1),
    (16, 0b0000000000011100, 30, 1),
    (16, 0b0000000000011101, 29, 1),
    (16, 0b0000000000011110, 28, 1),
    (16, 0b0000000000011111, 27, 1),
];

/// End-of-Block VLC: `10` (2 bits).
const EOB_CODE: u32 = 0b10;
/// Escape prefix VLC: `000001` (6 bits), followed by a 16-bit raw MDEC value.
const ESCAPE_CODE: u32 = 0b000001;

// ── Iki LZSS (qscale/DC table) ───────────────────────────────────────────────

/// Decompress the Iki per-block qscale/DC table.
///
/// Control byte, bits tested LSB-first: a `0` bit copies one literal byte; a
/// `1` bit is a back-reference - a length byte (`+3`, range 3..=258) then a
/// 1- or 2-byte offset (high bit of the first byte selects 2-byte form; offset
/// is `+1`, relative to the current output position). Overlapping copies are
/// allowed (the window is the output produced so far). Stops once `out_len`
/// bytes have been produced.
fn iki_lzss_decompress(src: &[u8], out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let mut sp = 0usize;
    'outer: while out.len() < out_len && sp < src.len() {
        let mut flags = src[sp];
        sp += 1;
        for _ in 0..8 {
            if out.len() >= out_len {
                break 'outer;
            }
            if flags & 1 != 0 {
                // Back-reference.
                if sp >= src.len() {
                    break 'outer;
                }
                let size = src[sp] as usize + 3;
                sp += 1;
                if sp >= src.len() {
                    break 'outer;
                }
                let mut off = src[sp] as usize;
                sp += 1;
                if off & 0x80 != 0 {
                    if sp >= src.len() {
                        break 'outer;
                    }
                    off = ((off & 0x7f) << 8) | src[sp] as usize;
                    sp += 1;
                }
                off += 1;
                for _ in 0..size {
                    if out.len() >= out_len {
                        break 'outer;
                    }
                    if off == 0 || off > out.len() {
                        break 'outer; // malformed; bail rather than panic
                    }
                    let b = out[out.len() - off];
                    out.push(b);
                }
            } else {
                if sp >= src.len() {
                    break 'outer;
                }
                out.push(src[sp]);
                sp += 1;
            }
            flags >>= 1;
        }
    }
    out
}

// ── Bit reader (16-bit little-endian words, MSB-first) ───────────────────────

struct BitReader<'a> {
    data: &'a [u8],
    /// Absolute bit index into the logical MSB-first stream.
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Read one bit. The stream is a sequence of 16-bit little-endian words,
    /// each consumed MSB-first (i.e. byte order 1,0,3,2,... MSB-first).
    fn read_bit(&mut self) -> Option<u32> {
        let word = self.pos / 16;
        let within = self.pos % 16;
        let byte_index = word * 2 + (1 - within / 8);
        if byte_index >= self.data.len() {
            return None;
        }
        let bit_in_byte = 7 - (self.pos % 8);
        let b = (self.data[byte_index] >> bit_in_byte) & 1;
        self.pos += 1;
        Some(b as u32)
    }

    fn read_bits(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.read_bit()?;
        }
        Some(v)
    }
}

/// One decoded AC entry.
enum Ac {
    EndOfBlock,
    Code { run: u8, level: i16 },
    Err,
}

/// Decode one AC run/level entry from the bitstream.
fn decode_ac(br: &mut BitReader<'_>) -> Ac {
    let mut acc = 0u32;
    let mut n = 0u32;
    loop {
        match br.read_bit() {
            Some(b) => {
                acc = (acc << 1) | b;
                n += 1;
            }
            None => return Ac::Err,
        }
        if n == 2 && acc == EOB_CODE {
            return Ac::EndOfBlock;
        }
        if n == 6 && acc == ESCAPE_CODE {
            // Escape: 16-bit raw MDEC value = run<<10 | level (10-bit signed).
            let raw = match br.read_bits(16) {
                Some(v) => v,
                None => return Ac::Err,
            };
            let run = (raw >> 10) as u8;
            let lvl10 = raw & 0x3FF;
            let level = if lvl10 & 0x200 != 0 {
                lvl10 as i32 - 1024
            } else {
                lvl10 as i32
            };
            return Ac::Code {
                run,
                level: level as i16,
            };
        }
        for &(nbits, code, run, level) in AC_CODES {
            if nbits == n && code == acc {
                let s = match br.read_bit() {
                    Some(v) => v,
                    None => return Ac::Err,
                };
                return Ac::Code {
                    run,
                    level: if s == 0 { level } else { -level },
                };
            }
        }
        if n > 16 {
            return Ac::Err;
        }
    }
}

// ── 8x8 IDCT ──────────────────────────────────────────────────────────────

/// Separable 8x8 IDCT using [`IDCT_C`] (scaled by 2048 per pass). The row pass
/// keeps full precision in an `i64` intermediate; the single final `>> 24` after
/// the column pass yields the standard normalisation where a DC-only block
/// resolves to `coef[0] / 8` (1448^2 / 2^24 = 0.125). Deferring the rounding to
/// one shift avoids the AC-precision loss a per-pass shift would introduce.
fn idct_8x8(block: &mut [i32; 64]) {
    let mut tmp = [0i64; 64];
    // Row pass (no shift - keep full precision).
    for row in 0..8 {
        for n in 0..8 {
            let mut s = 0i64;
            for k in 0..8 {
                s += IDCT_C[k][n] as i64 * block[row * 8 + k] as i64;
            }
            tmp[row * 8 + n] = s;
        }
    }
    // Column pass + final rounding shift.
    for col in 0..8 {
        for n in 0..8 {
            let mut s = 0i64;
            for k in 0..8 {
                s += IDCT_C[k][n] as i64 * tmp[k * 8 + col];
            }
            block[n * 8 + col] = ((s + (1 << 23)) >> 24) as i32;
        }
    }
}

// ── Block decode ────────────────────────────────────────────────────────────

/// Decode one 8x8 block: dequantize the table-supplied DC plus the bitstream AC
/// coefficients, then run the IDCT. Returns the spatial-domain samples.
fn decode_block(br: &mut BitReader<'_>, qscale: i32, dc: i32) -> [i32; 64] {
    let mut coeffs = [0i32; 64];
    coeffs[0] = dc * Q_MAT[0];

    let mut pos = 1usize;
    // Read AC codes until End-of-Block. A block that fills all 63 AC positions
    // is still terminated by an explicit EOB in the bitstream, so the loop must
    // always read the next code rather than stopping the moment `pos` saturates
    // (otherwise the trailing EOB is left unconsumed and the stream desyncs).
    loop {
        match decode_ac(br) {
            Ac::EndOfBlock | Ac::Err => break,
            Ac::Code { run, level } => {
                pos += run as usize;
                if pos > 63 {
                    break;
                }
                // Dequantize. Arithmetic shift (floor) matches the PSX rounding;
                // the coefficient is intentionally not range-clamped here (escape
                // codes carry large levels that the IDCT consumes directly).
                let q = Q_MAT[pos];
                coeffs[ZIGZAG[pos]] = (level as i32 * q * qscale + 4) >> 3;
                pos += 1;
            }
        }
    }

    idct_8x8(&mut coeffs);
    coeffs
}

// ── YCbCr -> RGBA ────────────────────────────────────────────────────────────

/// BT.601 YCbCr -> RGB. The MDEC produces zero-centred (signed) samples, so the
/// luma is offset by `+128` to recover the display range.
fn ycbcr_to_rgba(y: i32, cb: i32, cr: i32) -> [u8; 4] {
    let y = y + 128;
    let r = (y + ((91881 * cr) >> 16)).clamp(0, 255) as u8;
    let g = (y - ((22554 * cb + 46802 * cr) >> 16)).clamp(0, 255) as u8;
    let b = (y + ((116130 * cb) >> 16)).clamp(0, 255) as u8;
    [r, g, b, 255]
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Stateless PSX "Iki" MDEC frame decoder for Legaia FMV.
///
/// Call [`MdecDecoder::decode_frame`] with a complete frame payload (the Iki
/// header + LZSS qscale/DC table + AC bitstream, as assembled by
/// [`str_sector::StrFrameAssembler`]). Output is a row-major RGBA8 buffer of
/// `width x height x 4` bytes.
pub struct MdecDecoder {
    /// Frame width in pixels (must be a non-zero multiple of 16).
    pub width: u32,
    /// Frame height in pixels (must be a non-zero multiple of 16).
    pub height: u32,
}

/// Iki frame-header magic at byte offset 2 (`u16` little-endian).
const IKI_MAGIC: u16 = 0x3800;
/// Iki frame-header size in bytes.
const IKI_HEADER_LEN: usize = 10;

impl MdecDecoder {
    /// Create a decoder for the given frame dimensions.
    ///
    /// Both `width` and `height` must be non-zero multiples of 16.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Decode a complete Iki frame payload into an RGBA8 pixel buffer.
    ///
    /// Returns `width x height x 4` bytes. Regions the bitstream doesn't reach
    /// (truncated/garbled input) are left zero-filled rather than panicking.
    pub fn decode_frame(&self, frame: &[u8]) -> anyhow::Result<Vec<u8>> {
        use anyhow::bail;

        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 || !w.is_multiple_of(16) || !h.is_multiple_of(16) {
            bail!("MDEC: frame dimensions must be non-zero multiples of 16 (got {w}x{h})");
        }
        // PSX MDEC frames never exceed 640x480-class dimensions; cap generously
        // so a junk header can't request a multi-gigabyte allocation.
        const MAX_DIM: usize = 4096;
        if w > MAX_DIM || h > MAX_DIM {
            bail!("MDEC: frame dimensions {w}x{h} exceed the {MAX_DIM}px-per-axis limit");
        }

        let mut rgba = vec![0u8; w * h * 4];
        let mb_cols = w / 16;
        let mb_rows = h / 16;
        let block_count = mb_cols * mb_rows * 6;

        // Parse the 10-byte Iki header.
        if frame.len() < IKI_HEADER_LEN {
            return Ok(rgba); // too short to decode; blank frame
        }
        let magic = u16::from_le_bytes([frame[2], frame[3]]);
        let lzss_size = u16::from_le_bytes([frame[8], frame[9]]) as usize;
        if magic != IKI_MAGIC || IKI_HEADER_LEN + lzss_size > frame.len() {
            return Ok(rgba); // not a recognisable Iki frame; blank
        }

        // LZSS-decompress the per-block qscale/DC table.
        let table = iki_lzss_decompress(
            &frame[IKI_HEADER_LEN..IKI_HEADER_LEN + lzss_size],
            block_count * 2,
        );
        let qscale_dc = |block: usize| -> (i32, i32) {
            if block * 2 >= table.len() || block + block_count >= table.len() {
                return (1, 0);
            }
            let v = ((table[block] as u32) << 8) | table[block + block_count] as u32;
            let qscale = (v >> 10) as i32;
            let dc10 = v & 0x3FF;
            let dc = if dc10 & 0x200 != 0 {
                dc10 as i32 - 1024
            } else {
                dc10 as i32
            };
            (qscale.max(1), dc)
        };

        // AC bitstream follows the compressed table.
        let mut br = BitReader::new(&frame[IKI_HEADER_LEN + lzss_size..]);

        // Macroblocks are laid out column-major: down each column, then the
        // next column to the right.
        let mut block_idx = 0usize;
        for mb_col in 0..mb_cols {
            for mb_row in 0..mb_rows {
                let decode_one = |idx: usize, br: &mut BitReader<'_>| {
                    let (q, dc) = qscale_dc(idx);
                    decode_block(br, q, dc)
                };
                let cr = decode_one(block_idx, &mut br);
                let cb = decode_one(block_idx + 1, &mut br);
                let y_blocks = [
                    decode_one(block_idx + 2, &mut br),
                    decode_one(block_idx + 3, &mut br),
                    decode_one(block_idx + 4, &mut br),
                    decode_one(block_idx + 5, &mut br),
                ];
                block_idx += 6;

                // Y0=top-left, Y1=top-right, Y2=bottom-left, Y3=bottom-right.
                for sub_y in 0..2usize {
                    for sub_x in 0..2usize {
                        let y_block = &y_blocks[sub_y * 2 + sub_x];
                        for py in 0..8usize {
                            for px in 0..8usize {
                                let luma = y_block[py * 8 + px];
                                let cx = sub_x * 4 + px / 2;
                                let cy = sub_y * 4 + py / 2;
                                let cr_v = cr[cy * 8 + cx];
                                let cb_v = cb[cy * 8 + cx];

                                let pixel_x = mb_col * 16 + sub_x * 8 + px;
                                let pixel_y = mb_row * 16 + sub_y * 8 + py;
                                if pixel_x < w && pixel_y < h {
                                    let idx = (pixel_y * w + pixel_x) * 4;
                                    rgba[idx..idx + 4]
                                        .copy_from_slice(&ycbcr_to_rgba(luma, cb_v, cr_v));
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

    /// Build a minimal valid Iki frame: 10-byte header + an LZSS-compressed
    /// table whose entries are all `qscale=1, DC=0`, and an empty AC bitstream
    /// (so every block is solid neutral). `lzss` is the pre-built compressed
    /// table bytes.
    fn iki_frame(w: u16, h: u16, lzss: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&0u16.to_le_bytes()); // mdec code count
        v.extend_from_slice(&IKI_MAGIC.to_le_bytes());
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(&(lzss.len() as u16).to_le_bytes());
        v.extend_from_slice(lzss);
        v
    }

    #[test]
    fn lzss_literals_roundtrip() {
        // 1 control byte (all-literal flags = 0x00) then 8 literal bytes.
        let src = [0x00u8, 1, 2, 3, 4, 5, 6, 7, 8];
        let out = iki_lzss_decompress(&src, 8);
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn lzss_back_reference_repeats() {
        // flags: bit0=0 (literal 0xAA), bit1=1 (copy). copy: size byte 0 (=>3),
        // offset byte 0 (=> off 1) -> repeat last byte 3 times.
        let src = [0b0000_0010u8, 0xAA, 0x00, 0x00];
        let out = iki_lzss_decompress(&src, 4);
        assert_eq!(out, vec![0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn lzss_stops_at_out_len() {
        let src = [0x00u8, 1, 2, 3, 4, 5, 6, 7, 8];
        let out = iki_lzss_decompress(&src, 3);
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn lzss_malformed_offset_bails_without_panic() {
        // copy with offset pointing before the output start must not panic.
        let src = [0b0000_0001u8, 0x00, 0x05];
        let out = iki_lzss_decompress(&src, 16);
        assert!(out.len() <= 16);
    }

    #[test]
    fn decode_frame_rejects_non_multiple_of_16() {
        assert!(MdecDecoder::new(15, 16).decode_frame(&[]).is_err());
        assert!(MdecDecoder::new(16, 0).decode_frame(&[]).is_err());
    }

    #[test]
    fn decode_frame_rejects_oversized_dimensions() {
        // 65520 = largest multiple of 16 below u16::MAX.
        assert!(
            MdecDecoder::new(65520, 65520)
                .decode_frame(&[0u8; 10])
                .is_err()
        );
    }

    #[test]
    fn decode_frame_blank_on_short_or_bad_header() {
        let dec = MdecDecoder::new(16, 16);
        // Too short for a header.
        assert_eq!(dec.decode_frame(&[0u8; 4]).unwrap().len(), 16 * 16 * 4);
        // Wrong magic -> blank but correctly sized.
        let mut bad = vec![0u8; 10];
        bad[2] = 0xFF;
        assert_eq!(dec.decode_frame(&bad).unwrap().len(), 16 * 16 * 4);
    }

    #[test]
    fn decode_frame_neutral_table_is_mid_gray() {
        // One 16x16 macroblock: 6 blocks, all qscale=1 DC=0. The packed table
        // is block_count*2 = 12 bytes of 0x00 (qscale top-6 bits = 0 -> clamped
        // to 1, DC = 0). Compress as literals: 12 bytes needs 2 control bytes.
        let table = [0u8; 12];
        let mut lzss = Vec::new();
        // 8 literals
        lzss.push(0x00);
        lzss.extend_from_slice(&table[..8]);
        // 4 literals
        lzss.push(0x00);
        lzss.extend_from_slice(&table[8..]);
        let frame = iki_frame(16, 16, &lzss);
        let out = MdecDecoder::new(16, 16).decode_frame(&frame).unwrap();
        assert_eq!(out.len(), 16 * 16 * 4);
        // DC=0 + no AC -> luma 0 -> +128 -> neutral gray, fully opaque.
        for px in out.chunks_exact(4) {
            assert!((px[0] as i32 - 128).abs() <= 4, "r={}", px[0]);
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn ac_table_is_prefix_free_and_decodes_common_codes() {
        // "11" + sign 0 -> (run 0, level +1); then "10" EOB.
        // Bitstream is read as 16-bit LE words MSB-first; pack bits into a word.
        // bits: 1 1 0 (=(0,+1)) then 1 0 (EOB) = 11010xxxxxxxxxxx
        let word: u16 = 0b1101_0000_0000_0000;
        let bytes = word.to_le_bytes();
        let mut br = BitReader::new(&bytes);
        match decode_ac(&mut br) {
            Ac::Code { run, level } => {
                assert_eq!(run, 0);
                assert_eq!(level, 1);
            }
            _ => panic!("expected (0,1)"),
        }
        assert!(matches!(decode_ac(&mut br), Ac::EndOfBlock));
    }

    #[test]
    fn bitreader_reads_words_msb_first() {
        // word 0x1234 LE = bytes [0x34, 0x12]; MSB-first bits = 0001 0010 0011 0100
        let bytes = 0x1234u16.to_le_bytes();
        let mut br = BitReader::new(&bytes);
        assert_eq!(br.read_bits(16), Some(0x1234));
    }

    #[test]
    fn idct_dc_only_is_constant_eighth() {
        // DC-only block: coef[0]=D should resolve to ~D/8 in every cell.
        let mut block = [0i32; 64];
        block[0] = 1024;
        idct_8x8(&mut block);
        let expect = 1024 / 8;
        for &v in block.iter() {
            assert!((v - expect).abs() <= 2, "v={v} expect={expect}");
        }
    }

    #[test]
    fn ycbcr_neutral_chroma_passes_luma() {
        let [r, g, b, a] = ycbcr_to_rgba(0, 0, 0);
        assert_eq!(a, 255);
        // Y=0 (signed) -> +128 -> ~gray.
        for c in [r, g, b] {
            assert!((c as i32 - 128).abs() < 4);
        }
    }
}
