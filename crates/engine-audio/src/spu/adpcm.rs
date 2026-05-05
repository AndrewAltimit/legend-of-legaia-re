//! Streaming PSX SPU-ADPCM decoder, one block at a time.
//!
//! Block layout (16 bytes each):
//!
//! ```text
//!   byte 0:  (filter << 4) | shift
//!   byte 1:  flag bits (bit0=end, bit1=repeat, bit2=loop-start)
//!   bytes 2..15: 14 bytes = 28 nibble-pair samples (low nibble first)
//! ```
//!
//! Identical algorithm to the one in `legaia-vab` (and to the XA decoder's
//! filter constants), but kept stateful here so a long-playing voice can be
//! advanced one block at a time without re-walking the body.
//!
//! No Sony bytes — the algorithm is the standard PSX SPU formula
//! (see `docs/formats/vab.md` and the `legaia-xa` filter table).

const F0: [i32; 5] = legaia_xa::F0;
const F1: [i32; 5] = legaia_xa::F1;

/// Bytes in one ADPCM block.
pub const BLOCK_BYTES: usize = 16;
/// PCM samples produced by one block.
pub const SAMPLES_PER_BLOCK: usize = 28;

/// What a block decoder learned about the block header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFlags {
    /// `flag & 0x01` — voice should jump to its loop point (or stop, if no
    /// loop set) after the last sample of this block plays out.
    pub end: bool,
    /// `flag & 0x02` — *repeat* bit. Used together with the loop-start bit
    /// to indicate this block is the loop tail; the SPU will jump back to
    /// the loop-start address.
    pub repeat: bool,
    /// `flag & 0x04` — *loop-start* bit. The SPU latches this block's
    /// address as the loop point.
    pub loop_start: bool,
    /// True if the header byte has `filter > 4`. Real banks sometimes mark
    /// the trailing terminator block with garbage filter; treat as EOS.
    pub bad_header: bool,
}

impl BlockFlags {
    pub fn from_bytes(header: u8, flag: u8) -> Self {
        Self {
            end: flag & 0x01 != 0,
            repeat: flag & 0x02 != 0,
            loop_start: flag & 0x04 != 0,
            bad_header: ((header >> 4) & 0x0F) > 4,
        }
    }
}

/// Stateful single-block decoder. Reused across calls so `prev1`/`prev2`
/// carry the inter-block continuation samples.
#[derive(Debug, Clone, Default)]
pub struct AdpcmDecoder {
    pub prev1: i32,
    pub prev2: i32,
}

impl AdpcmDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset history. Call when a voice key-on starts a fresh playback so
    /// the new voice doesn't inherit the tail of whatever played before.
    pub fn reset(&mut self) {
        self.prev1 = 0;
        self.prev2 = 0;
    }

    /// Decode one block. Returns the 28 PCM samples plus the parsed flags.
    /// `block` must be exactly [`BLOCK_BYTES`] long.
    pub fn decode_block(&mut self, block: &[u8]) -> ([i16; SAMPLES_PER_BLOCK], BlockFlags) {
        debug_assert_eq!(block.len(), BLOCK_BYTES);
        let header = block[0];
        let flag = block[1];
        let flags = BlockFlags::from_bytes(header, flag);

        let mut out = [0i16; SAMPLES_PER_BLOCK];
        if flags.bad_header {
            // Treat as silence; the engine should observe `flags.bad_header`
            // and stop the voice.
            return (out, flags);
        }
        let filter = ((header >> 4) & 0x0F) as usize;
        let shift = (header & 0x0F) as i32;
        let f0 = F0[filter];
        let f1 = F1[filter];

        let mut idx = 0usize;
        for &byte in &block[2..16] {
            for nibble_idx in 0..2 {
                let nibble = if nibble_idx == 0 {
                    byte & 0x0F
                } else {
                    (byte >> 4) & 0x0F
                };
                // Sign-extend 4-bit signed nibble.
                let s = ((nibble as i8) << 4) >> 4;
                let mut sample = (s as i32) << (12 - shift);
                sample += (self.prev1 * f0 + self.prev2 * f1 + 32) >> 6;
                let clamped = sample.clamp(i16::MIN as i32, i16::MAX as i32);
                out[idx] = clamped as i16;
                idx += 1;
                self.prev2 = self.prev1;
                self.prev1 = clamped;
            }
        }
        (out, flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All-zero filter=0 shift=0 block decodes to exactly 28 zero samples
    /// when history is zero, and leaves history at zero.
    #[test]
    fn silence_block_yields_zeros() {
        let mut d = AdpcmDecoder::new();
        let block = [0u8; BLOCK_BYTES];
        let (pcm, flags) = d.decode_block(&block);
        assert_eq!(pcm, [0i16; SAMPLES_PER_BLOCK]);
        assert!(!flags.end);
        assert!(!flags.bad_header);
        assert_eq!(d.prev1, 0);
        assert_eq!(d.prev2, 0);
    }

    /// Bad filter (>4) is reported via `flags.bad_header` and produces
    /// silence; the caller should react by stopping the voice.
    #[test]
    fn bad_filter_returns_silence_and_flag() {
        let mut d = AdpcmDecoder::new();
        let mut block = [0u8; BLOCK_BYTES];
        block[0] = 0x70; // filter=7, garbage
        let (pcm, flags) = d.decode_block(&block);
        assert!(flags.bad_header);
        assert_eq!(pcm, [0i16; SAMPLES_PER_BLOCK]);
    }

    /// flag-byte bits are correctly decoded.
    #[test]
    fn flag_bits_decode() {
        let bf = BlockFlags::from_bytes(0x00, 0x07);
        assert!(bf.end && bf.repeat && bf.loop_start);
        let bf = BlockFlags::from_bytes(0x00, 0x04);
        assert!(!bf.end && !bf.repeat && bf.loop_start);
        let bf = BlockFlags::from_bytes(0x00, 0x01);
        assert!(bf.end && !bf.repeat && !bf.loop_start);
    }

    /// History is preserved across blocks. Filter=0 (`f0=f1=0`) plus a
    /// non-zero leading nibble pair gives a known first two output samples;
    /// the rest of the body is zeros. After the block, history reflects
    /// the LAST sample (zero), not the first.
    #[test]
    fn history_carries_across_blocks() {
        let mut d = AdpcmDecoder::new();
        let mut b1 = [0u8; BLOCK_BYTES];
        b1[2] = 0x11; // both nibbles = 1 -> sample = 1<<12 = 4096
        let (out1, _) = d.decode_block(&b1);
        // First two samples come from the 0x11 byte.
        assert_eq!(out1[0], 4096);
        assert_eq!(out1[1], 4096);
        // Remaining 26 samples are zero (the rest of the body is 0x00).
        for &s in &out1[2..] {
            assert_eq!(s, 0);
        }
        // History at end of block: last sample was 0, previous was 0.
        assert_eq!(d.prev1, 0);
        assert_eq!(d.prev2, 0);
    }
}
