//! The STRv2/v3 bitstream decoder - `FUN_801D070C` in the STR/MDEC overlay
//! (PROT 0970, base `0x801CE818`).
//!
//! This is the consumer the [`crate::strv2_table`] unpacker had been missing.
//! The overlay selects it (over the Iki decoder) only for the two dev FMV slots
//! 9/10 (`MV1A.STR` / `MOV15.STR`), whose files are **not** on the released
//! disc, so it is dead in retail; it is ported here for completeness and to
//! give the unpacked VLC table a decoder to feed.
//!
//! ## What the decoder does
//!
//! It converts a demuxed STRv2/v3 frame into the same MDEC command-word list an
//! Iki frame decodes to (`crate::MdecDecoder` consumes that list). The VLC table
//! that [`crate::strv2_table`] unpacks is not a run/level table - it holds the
//! **pre-baked MDEC output codes** directly, so the decode is a bit-prefix
//! lookup that emits one to three ready-made codes per table hit. Only the DC
//! coefficients, the escape codes and the end-of-frame padding are computed.
//!
//! ## Table layout (byte offsets relative to the `param_3` base)
//!
//! `FUN_801D070C` adds `0x800` to its table pointer up front and derives the
//! escape table at `+0x10000` from there, which carves the `0x11000`-byte table
//! into four regions (this is exactly [`crate::strv2_table::STRV2_TABLE_BYTES`]):
//!
//! | Bytes | Region | Record | Index |
//! |---|---|---|---|
//! | `0x00000` | luma DC (`u16 len`, `u16 extra`) | 4 | `acc >> 24` (8 bits) |
//! | `0x00400` | chroma DC (`u16 len`, `u16 extra`) | 4 | `acc >> 24` (8 bits) |
//! | `0x00800` | AC primary (`u32 word0`, `u32 word1`) | 8 | `acc >> 19` (13 bits) |
//! | `0x10800` | AC secondary (`u32`) | 4 | `acc >> 23` (9 bits) |
//!
//! The luma table serves the four Y blocks (`uVar20 >= 3`); the chroma table
//! serves Cr and Cb (`uVar20 < 3`). An AC primary entry packs its output codes
//! into the high halves: `word0 >> 16` is the first code, and `word1`'s two
//! halves are the second and third; the low byte of each `u32` is the number of
//! bitstream bits the entry consumes. A zero `word0` escapes to the secondary
//! table (consume 8 bits, look up a 9-bit index).
//!
//! ## v2 versus v3
//!
//! `frame[3]` (the fourth header word) selects the DC coding: `< 3` is v2, where
//! each block's DC is a raw 10-bit field copied straight into the code; `>= 3` is
//! v3, where the DC is a size-prefixed signed **difference** predicted from a
//! per-channel running accumulator (Cr, Cb, and one luma predictor chained
//! across all four Y blocks). The end-of-frame marker is the top 10 bits reading
//! `0x1FF` (v2) or `0x3FF` (v3).
//!
//! ## Whole-frame decode
//!
//! Retail is a *resumable* routine: it decodes until its output pointer reaches
//! a chunk limit (`t6`), saves nine registers to `DAT_801D0A48..0A68` and returns
//! `1`, resuming on the next call (`param_1 == 0`). The chunk boundary exists
//! only to feed the MDEC in bounded DMA bursts and is checked once per block, at
//! `801cf8cc`. This port drops the chunk harness and decodes the whole frame in
//! one pass - the same computation, uninterrupted - matching how
//! [`crate::MdecDecoder`] takes a whole Iki frame. The one hardware side effect
//! on the end path (`setCopReg(Status | 0x20000)`, re-enabling interrupts) is a
//! no-op here.
//!
//! Every output write in `FUN_801D070C` lands at the current end of the buffer
//! and advances by one slot, with a single quirk: an end-of-block `0xFE00` code
//! is written *without* the cursor advancing, and the next block's DC write
//! pre-increments to compensate. The two cancel, so the port models the whole
//! thing as an in-order [`Vec::push`].
//!
//! See [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md#bitstream-decode--mdec-feed-overlay).
//!
//! # NOT WIRED
//!
//! No engine path can select this decoder, and no retail data would make it
//! do so. The overlay's master dispatch picks a bitstream per `fmv_id` and
//! only slots 9 and 10 - `MV1A.STR` and `MOV15.STR` - clear the Iki flag; both
//! files are dev leftovers that are **not on the released disc**, so
//! `legaia_engine_core::cutscene::fmv_bitstream` returns `Iki` for every
//! reachable id and the STR player's [`Bitstream::Strv2`] arm never comes up.
//! What must exist first is an input: an STRv2/v3 stream to decode, together
//! with the runtime unpack of the VLC table that stream needs
//! ([`crate::strv2_table`], itself only exercised by tests). Both are
//! preservation artefacts of the two cut slots, not gaps in the shipping
//! playback path.
//!
//! There is a second prerequisite, and it outlives the missing input: this
//! function stops at the **MDEC command-word list**, which retail DMAs into
//! the MDEC for the dequantize / IDCT / colour stages. The port has no such
//! stage as a separate entry - [`crate::MdecDecoder::decode_frame`] fuses the
//! Iki bitstream walk and the pixel stage into one pass over the Iki payload,
//! and takes bytes, not codes. So even handed a real STRv2 stream, a caller
//! could not turn this vector into pixels; a code-list-to-RGBA entry point has
//! to exist first.
//!
//! [`Bitstream::Strv2`]: crate::str_player::Bitstream::Strv2

use crate::strv2_table::STRV2_TABLE_U16S;
use anyhow::{Result, bail};

/// MDEC end-of-block code. A table entry carrying it terminates the current
/// block (`xori at,t1,0xfe00` / `beq ...,0x801d07bc`).
pub const MDEC_EOB: u16 = 0xFE00;

/// Escape marker: a table code of this value means the real MDEC code is the
/// next raw 16 bits of the bitstream (`xori at,t1,0x7c1f` / `LAB_801d09bc`).
pub const MDEC_ESCAPE: u16 = 0x7C1F;

/// End-of-frame padding codes `FUN_801D070C` flushes (`addiu v0,zero,0x40`, then
/// a post-decrement loop, so `0x41` writes) - `MDEC_EOB` × 65.
pub const END_PAD_CODES: usize = 0x41;

/// `u16` index of the chroma DC table (byte `0x400`).
const CHROMA_DC_HW: usize = 0x0400 / 2;
/// `u16` index of the AC primary table (byte `0x800`).
const AC_PRIMARY_HW: usize = 0x0800 / 2;
/// `u16` index of the AC secondary table (byte `0x10800`).
const AC_SECONDARY_HW: usize = 0x1_0800 / 2;

/// Safety bound on emitted codes - a well-formed frame stops at its end marker
/// long before this. Retail's chunk limit plays this role; the port needs its
/// own guard against a malformed stream that never terminates.
const MAX_OUTPUT_CODES: usize = 1 << 20;

/// MSB-first bit reader over the frame's `u16` words, mirroring the overlay's
/// 32-bit accumulator with 16-bit refills.
struct BitReader<'a> {
    /// `v0` - the 32-bit accumulator, consumed from the top.
    acc: u32,
    /// `v1` - bits consumed within the current 16-bit window (`0..16`).
    bits: u32,
    words: &'a [u16],
    /// Index of the next word a refill will pull.
    pos: usize,
    /// A refill was attempted past the end of the frame.
    overran: bool,
}

impl<'a> BitReader<'a> {
    /// `FUN_801D070C` init: `acc = (frame[4] << 16) | frame[5]`, next word
    /// `frame[6]`, `bits = 0` (`801d07a4`..`801d07ac`, `801d07a0` `a0 += 0xc`).
    fn new(words: &'a [u16]) -> Self {
        let acc = ((words[4] as u32) << 16) | words[5] as u32;
        Self {
            acc,
            bits: 0,
            words,
            pos: 6,
            overran: false,
        }
    }

    fn take_word(&mut self) -> u32 {
        match self.words.get(self.pos) {
            Some(&w) => {
                self.pos += 1;
                w as u32
            }
            None => {
                self.overran = true;
                0
            }
        }
    }

    /// Consume `n` bits: shift them out of the top, and when the running count
    /// crosses a 16-bit boundary pull one fresh word into the freed low bits
    /// (`sll`/`add`/`andi 0x10`/`sllv`/`or` idiom).
    fn consume(&mut self, n: u32) {
        self.acc = self.acc.wrapping_shl(n);
        self.bits += n;
        if self.bits & 0x10 != 0 {
            self.bits &= 0xf;
            let w = self.take_word();
            self.acc |= w << self.bits;
        }
    }

    /// Consume exactly 16 bits (`801d09d0`): shift 16 and load one word without
    /// touching `bits` (16 bits keeps the window position unchanged).
    fn consume16_raw(&mut self) {
        self.acc <<= 16;
        let w = self.take_word();
        self.acc |= w << self.bits;
    }
}

/// A demuxed STRv2/v3 frame's four `u16` DC-table records live in two 256-entry
/// tables; look one up.
fn dc_entry(table: &[u16], chroma: bool, index8: usize) -> (u32, u32) {
    let base = if chroma { CHROMA_DC_HW } else { 0 };
    let off = base + index8 * 2;
    (table[off] as u32, table[off + 1] as u32)
}

/// AC primary lookup (`801d08d8`): 8-byte record, two packed `u32`s.
fn ac_primary(table: &[u16], index13: usize) -> (u32, u32) {
    let off = AC_PRIMARY_HW + index13 * 4;
    let word0 = table[off] as u32 | ((table[off + 1] as u32) << 16);
    let word1 = table[off + 2] as u32 | ((table[off + 3] as u32) << 16);
    (word0, word1)
}

/// AC secondary lookup (`801d0918`): 4-byte record.
fn ac_secondary(table: &[u16], index9: usize) -> u32 {
    let off = AC_SECONDARY_HW + index9 * 2;
    table[off] as u32 | ((table[off + 1] as u32) << 16)
}

/// Sign-extend a v3 DC difference (`801d0800`..`801d081c`).
///
/// `a` is the accumulator after the VLC prefix has been consumed, so the `extra`
/// difference bits sit at the top. If the top bit is set the value is positive
/// (the raw field); otherwise it is negative and biased by `(1 << extra) - 1`.
pub fn sign_extend_dc(a: u32, extra: u32) -> i32 {
    if extra == 0 {
        return 0;
    }
    let top = (a >> (32 - extra)) as i32;
    if a & 0x8000_0000 != 0 {
        top
    } else {
        top - ((1i32 << extra) - 1)
    }
}

/// Apply a v3 DC difference to a channel predictor and format the MDEC code
/// (`801d086c`..`801d0878`).
///
/// The predicted DC *is* the updated running accumulator (`t1 = pred + dc` and
/// `pred += dc` write the same value), so the returned code carries
/// `pred` after the update: `(qscale << 10) | ((pred << 2) & 0x3FF)`.
pub fn dc_predicted_code(pred: &mut i32, dc: i32, qscale_field: u32) -> u16 {
    *pred += dc;
    (qscale_field | (((*pred << 2) as u32) & 0x3FF)) as u16
}

/// Which channel predictor a v3 block index selects, and which DC table it reads.
/// Block index `uVar20` runs `1..=6` over Cr, Cb, Y0, Y1, Y2, Y3.
fn v3_channel(block: u32) -> (usize, bool) {
    match block {
        1 => (0, true),  // Cr - chroma table, predictor 0
        2 => (1, true),  // Cb - chroma table, predictor 1
        _ => (2, false), // Y0..Y3 - luma table, predictor 2 (chained)
    }
}

/// Decode a demuxed STRv2/v3 frame into its MDEC command-word list.
///
/// `frame` is the frame's `u16` words (`frame[0..4]` the header: code count,
/// `0x3800`, quant scale, version; `frame[4..]` the bitstream). `table` is the
/// `0x8800`-entry VLC table [`crate::strv2_table::unpack_strv2_vlc_table`]
/// produces. The returned vector begins with `frame[0..2]` copied verbatim and
/// ends with [`END_PAD_CODES`] end-of-block codes, exactly as the overlay emits.
// PORT: FUN_801d070c
pub fn decode_frame(frame: &[u16], table: &[u16]) -> Result<Vec<u16>> {
    if frame.len() < 6 {
        bail!("STRv2 frame is {} words, need at least 6", frame.len());
    }
    if table.len() < STRV2_TABLE_U16S {
        bail!(
            "STRv2 table is {} u16 entries, need {}",
            table.len(),
            STRV2_TABLE_U16S
        );
    }

    let qscale_field = (frame[2] as u32) << 10;
    // `uVar20`: 0 pins the v2 DC path forever; v3 seeds it to 1 and cycles 1..=6.
    let is_v3 = frame[3] >= 3;
    let mut block: u32 = if is_v3 { 1 } else { 0 };
    let mut pred = [0i32; 3];

    let mut out: Vec<u16> = Vec::new();
    out.push(frame[0]);
    out.push(frame[1]);

    let mut r = BitReader::new(frame);

    'frame: loop {
        // --- block start (LAB_801d07bc): DC coefficient ---
        let marker = r.acc >> 22; // top 10 bits
        if is_v3 {
            if marker == 0x3FF {
                break 'frame;
            }
            let (channel, chroma) = v3_channel(block);
            let index8 = (r.acc >> 24) as usize;
            let (len, extra) = dc_entry(table, chroma, index8);
            let a = r.acc.wrapping_shl(len); // consume the VLC prefix
            let dc = sign_extend_dc(a, extra);
            // Consume the extra difference bits (already reflected in `a`), then
            // account for both fields with a single refill.
            r.acc = a.wrapping_shl(extra);
            r.bits += extra + len;
            if r.bits & 0x10 != 0 {
                r.bits &= 0xf;
                let w = r.take_word();
                r.acc |= w << r.bits;
            }
            out.push(dc_predicted_code(&mut pred[channel], dc, qscale_field));
            block = if block == 6 { 1 } else { block + 1 };
        } else {
            if marker == 0x1FF {
                break 'frame;
            }
            let dc10 = r.acc >> 22; // raw 10-bit DC
            r.consume(10);
            out.push((qscale_field | dc10) as u16);
        }

        // --- AC coefficients (LAB_801d08d8) ---
        loop {
            let index13 = (r.acc >> 19) as usize;
            let (word0, word1) = ac_primary(table, index13);
            // `word` holds the current code source, `codes` the packed outputs.
            let (len, codes): (u32, [Option<u16>; 3]) = if word0 != 0 {
                // `word1 == 0` stops after the first code (`801d0974`); the
                // third code is present only when its own half is non-zero
                // (`801d0998`), while a zero *second* half is still emitted.
                (
                    word0 & 0xff,
                    [
                        Some((word0 >> 16) as u16),
                        (word1 != 0).then_some((word1 & 0xffff) as u16),
                        ((word1 >> 16) != 0).then_some((word1 >> 16) as u16),
                    ],
                )
            } else {
                r.consume(8);
                let index9 = (r.acc >> 23) as usize;
                let sec = ac_secondary(table, index9);
                (sec & 0xff, [Some((sec >> 16) as u16), None, None])
            };
            r.consume(len);

            // Emit each packed code; an escape replaces it with the raw 16-bit
            // code, an EOB ends the block. `word1 == 0` (secondary, or an empty
            // second half) stops the packed sequence early.
            let mut ended_block = false;
            for code in codes.into_iter().flatten() {
                if code == MDEC_ESCAPE {
                    out.push((r.acc >> 16) as u16);
                    r.consume16_raw();
                    break;
                }
                out.push(code);
                if code == MDEC_EOB {
                    ended_block = true;
                    break;
                }
            }
            if ended_block {
                break;
            }
            if out.len() > MAX_OUTPUT_CODES {
                bail!("STRv2 decode exceeded {MAX_OUTPUT_CODES} codes without an end marker");
            }
            if r.overran {
                bail!("STRv2 bitstream ran past the end of the frame");
            }
        }

        if r.overran {
            bail!("STRv2 bitstream ran past the end of the frame");
        }
        if out.len() > MAX_OUTPUT_CODES {
            bail!("STRv2 decode exceeded {MAX_OUTPUT_CODES} codes without an end marker");
        }
    }

    // `801d09e4`..`801d09f4`: 0x41 copies of the end-of-block code.
    out.resize(out.len() + END_PAD_CODES, MDEC_EOB);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_table() -> Vec<u16> {
        vec![0u16; STRV2_TABLE_U16S]
    }

    /// Pack a big-endian, MSB-first bit list into `u16` words the way the
    /// accumulator consumes them: word 0 first, each word's bit 15 first.
    fn pack_bits(bits: &[u8]) -> Vec<u16> {
        let mut words = Vec::new();
        for chunk in bits.chunks(16) {
            let mut w = 0u16;
            for (i, &b) in chunk.iter().enumerate() {
                if b != 0 {
                    w |= 1 << (15 - i);
                }
            }
            words.push(w);
        }
        words
    }

    fn bits_of(value: u32, n: u32) -> Vec<u8> {
        (0..n).map(|i| ((value >> (n - 1 - i)) & 1) as u8).collect()
    }

    /// A frame header plus a bitstream body, padded with slack words for refills.
    fn frame(header: [u16; 4], body: &[u16]) -> Vec<u16> {
        let mut f = header.to_vec();
        f.extend_from_slice(body);
        f.extend_from_slice(&[0, 0, 0, 0]);
        f
    }

    #[test]
    fn header_passes_through_and_v2_end_marker_pads() {
        // acc top 10 bits == 0x1FF on entry -> immediate frame end.
        let body = pack_bits(&bits_of(0x1FF, 10));
        let f = frame([0xAAAA, 0x3800, 1, 2], &body);
        let out = decode_frame(&f, &empty_table()).unwrap();
        assert_eq!(out[0], 0xAAAA);
        assert_eq!(out[1], 0x3800);
        assert_eq!(out.len(), 2 + END_PAD_CODES);
        assert!(out[2..].iter().all(|&c| c == MDEC_EOB));
    }

    #[test]
    fn v3_end_marker_is_0x3ff() {
        let body = pack_bits(&bits_of(0x3FF, 10));
        let f = frame([0x1234, 0x3800, 0, 3], &body);
        let out = decode_frame(&f, &empty_table()).unwrap();
        assert_eq!(&out[..2], &[0x1234, 0x3800]);
        assert_eq!(out.len(), 2 + END_PAD_CODES);
        // A v2 frame with the same 0x3FF marker would NOT end (needs 0x1FF).
        let f2 = frame([0x1234, 0x3800, 0, 2], &body);
        // It would run off the end instead of ending cleanly.
        assert!(decode_frame(&f2, &empty_table()).is_err());
    }

    #[test]
    fn v2_dc_then_ac_eob_then_end() {
        // Bitstream: DC(10) = 5, AC primary index(13) = 0x1ABC consuming 13 bits
        // to an EOB entry, then DC(10) = 0x1FF end marker.
        let idx: u32 = 0x1ABC;
        let mut bits = Vec::new();
        bits.extend(bits_of(5, 10)); // DC
        bits.extend(bits_of(idx, 13)); // AC prefix (== the 13-bit index)
        bits.extend(bits_of(0x1FF, 10)); // next block: end
        let f = frame([0x1111, 0x2222, 2, 2], &pack_bits(&bits));

        // AC primary entry at `idx`: len = 13, code = EOB, no second word.
        let mut table = empty_table();
        let off = AC_PRIMARY_HW + idx as usize * 4;
        table[off] = 13; // word0 low 16 = length
        table[off + 1] = MDEC_EOB; // word0 high 16 = first code

        let out = decode_frame(&f, &table).unwrap();
        // qscale = 2 -> field 0x800; DC 5 -> code 0x805.
        assert_eq!(out[0], 0x1111);
        assert_eq!(out[1], 0x2222);
        assert_eq!(out[2], 0x0805);
        assert_eq!(out[3], MDEC_EOB); // AC block terminator
        assert_eq!(out.len(), 4 + END_PAD_CODES);
        assert!(out[3..].iter().all(|&c| c == MDEC_EOB));
    }

    #[test]
    fn sign_extend_matches_dc_convention() {
        // Top bit set -> positive, value is the raw field.
        assert_eq!(sign_extend_dc(0b111 << 29, 3), 0b111);
        assert_eq!(sign_extend_dc(0b100 << 29, 3), 0b100);
        // Top bit clear -> negative, biased by (1<<extra)-1.
        assert_eq!(sign_extend_dc(0b011 << 29, 3), 0b011 - 7);
        assert_eq!(sign_extend_dc(0, 3), 0 - 7);
        assert_eq!(sign_extend_dc(0, 0), 0);
    }

    #[test]
    fn dc_prediction_accumulates_on_a_channel() {
        // A luma channel takes four DC differences that chain: 3, 2, -1, 5.
        let q = 1u32 << 10; // 0x400
        let mut pred = 0i32;
        assert_eq!(
            dc_predicted_code(&mut pred, 3, q),
            (q | ((3 << 2) & 0x3ff)) as u16
        );
        assert_eq!(pred, 3);
        assert_eq!(
            dc_predicted_code(&mut pred, 2, q),
            (q | ((5 << 2) & 0x3ff)) as u16
        );
        assert_eq!(pred, 5);
        assert_eq!(
            dc_predicted_code(&mut pred, -1, q),
            (q | ((4 << 2) & 0x3ff)) as u16
        );
        assert_eq!(pred, 4);
        assert_eq!(
            dc_predicted_code(&mut pred, 5, q),
            (q | ((9 << 2) & 0x3ff)) as u16
        );
        assert_eq!(pred, 9);
    }

    #[test]
    fn v3_block_index_maps_channels_and_tables() {
        assert_eq!(v3_channel(1), (0, true)); // Cr
        assert_eq!(v3_channel(2), (1, true)); // Cb
        for y in 3..=6 {
            assert_eq!(v3_channel(y), (2, false)); // Y0..Y3 share the luma predictor
        }
    }

    #[test]
    fn ac_primary_packs_two_codes_before_eob() {
        // One primary entry emitting code A, code B, then a following EOB entry.
        let idx_ab: u32 = 0x0010;
        let idx_eob: u32 = 0x0020;
        let mut bits = Vec::new();
        bits.extend(bits_of(5, 10)); // v2 DC
        bits.extend(bits_of(idx_ab, 13)); // first AC lookup (two codes)
        bits.extend(bits_of(idx_eob, 13)); // second AC lookup (EOB)
        bits.extend(bits_of(0x1FF, 10)); // end
        let f = frame([0, 0x3800, 0, 2], &pack_bits(&bits));

        let mut table = empty_table();
        // Two-code entry: len 13, word0 code = 0x1234, word1 low = 0x5678, high 0.
        let off = AC_PRIMARY_HW + idx_ab as usize * 4;
        table[off] = 13;
        table[off + 1] = 0x1234;
        table[off + 2] = 0x5678; // second code
        table[off + 3] = 0; // no third code
        // EOB entry.
        let off2 = AC_PRIMARY_HW + idx_eob as usize * 4;
        table[off2] = 13;
        table[off2 + 1] = MDEC_EOB;

        let out = decode_frame(&f, &table).unwrap();
        // [.. DC, 0x1234, 0x5678, EOB, pad..]
        assert_eq!(out[2], 0x0005); // qscale 0 -> just the DC
        assert_eq!(out[3], 0x1234);
        assert_eq!(out[4], 0x5678);
        assert_eq!(out[5], MDEC_EOB);
        assert_eq!(out.len(), 6 + END_PAD_CODES);
    }

    #[test]
    fn escape_emits_the_raw_16_bits() {
        // A table code of MDEC_ESCAPE means "the code is the next raw 16 bits".
        let idx_esc: u32 = 0x0040;
        let idx_eob: u32 = 0x0050;
        let raw: u32 = 0xABCD;
        let mut bits = Vec::new();
        bits.extend(bits_of(5, 10)); // v2 DC
        bits.extend(bits_of(idx_esc, 13)); // escape lookup...
        bits.extend(bits_of(raw, 16)); // ...whose real code is these 16 bits
        bits.extend(bits_of(idx_eob, 13)); // then an EOB entry
        bits.extend(bits_of(0x1FF, 10)); // end
        let f = frame([0, 0x3800, 0, 2], &pack_bits(&bits));

        let mut table = empty_table();
        let off = AC_PRIMARY_HW + idx_esc as usize * 4;
        table[off] = 13;
        table[off + 1] = MDEC_ESCAPE;
        let off2 = AC_PRIMARY_HW + idx_eob as usize * 4;
        table[off2] = 13;
        table[off2 + 1] = MDEC_EOB;

        let out = decode_frame(&f, &table).unwrap();
        assert_eq!(out[2], 0x0005); // DC
        assert_eq!(out[3], raw as u16); // the escaped raw code
        assert_eq!(out[4], MDEC_EOB);
    }

    #[test]
    fn ac_secondary_lookup_on_empty_primary() {
        // The peeked primary entry is empty (a zero record) -> consume 8 bits,
        // then use the 9-bit secondary index.
        let sec_idx: u32 = 0x0055;
        let idx_eob: u32 = 0x0060;
        let mut bits = Vec::new();
        bits.extend(bits_of(5, 10)); // v2 DC
        // 13-bit primary index 0 (peeked), then 8 bits consumed, then the 9-bit
        // secondary index. Lay them out so the first 13 peeked bits are zero and
        // the secondary index falls where acc>>23 reads after the 8-bit consume.
        bits.extend(bits_of(0, 8)); // consumed by the empty-primary path
        bits.extend(bits_of(sec_idx, 9)); // secondary index (acc>>23)
        bits.extend(bits_of(idx_eob, 13)); // EOB entry
        bits.extend(bits_of(0x1FF, 10));
        let f = frame([0, 0x3800, 0, 2], &pack_bits(&bits));

        let mut table = empty_table();
        // Secondary entry: len covers the 9 index bits, code 0x0F0F.
        let soff = AC_SECONDARY_HW + sec_idx as usize * 2;
        table[soff] = 9; // low byte = length
        table[soff + 1] = 0x0F0F; // high 16 = code
        let off2 = AC_PRIMARY_HW + idx_eob as usize * 4;
        table[off2] = 13;
        table[off2 + 1] = MDEC_EOB;

        let out = decode_frame(&f, &table).unwrap();
        assert_eq!(out[2], 0x0005);
        assert_eq!(out[3], 0x0F0F);
        assert_eq!(out[4], MDEC_EOB);
    }

    #[test]
    fn rejects_short_frame_and_small_table() {
        assert!(decode_frame(&[0, 0, 0, 0], &empty_table()).is_err());
        let body = pack_bits(&bits_of(0x1FF, 10));
        let f = frame([0, 0x3800, 0, 2], &body);
        assert!(decode_frame(&f, &[0u16; 8]).is_err());
    }
}
