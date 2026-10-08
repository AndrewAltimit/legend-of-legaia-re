//! `"ME"` keyframe-stream archive - the battle **art-animation stream
//! source** the anim commit pulls a player art's packed TRS keyframes from.
//!
//! The retail reader is `FUN_8002B28C` (`ghidra/scripts/funcs/8002b28c.txt`):
//! `FUN_8004AD80` calls it as `FUN_8002B28C(_DAT_8007BD74, scratch, n)` to
//! materialize a dynamic art slot (`docs/formats/battle-data-pack.md`
//! § Art-animation bank). `_DAT_8007BD74` is the battle side-band
//! **streaming buffer**: `FUN_801F17F8` (battle overlay) fills it with one
//! `0x10800`-byte slot of `data\battle\summon.dat` / `readef.DAT`
//! (`docs/formats/summon-readef.md`), and the player art archives live at
//! the head of the `readef.DAT` slots `3*char + 1` / `3*char + 2`
//! (disc-pinned - see `battle_char_assembly::art_me_archive`).
//!
//! ## Layout
//!
//! ```text
//! +0x00  'M' 'E'                  ; magic
//! +0x02  u8  count                ; entry count
//! +0x03  u16 entry_sizes[count]   ; bit 15 = compressed, low 15 bits = body size
//! +0x03 + 2*count                 ; concatenated bodies, in entry order
//! ```
//!
//! Entry `n`'s body starts at `3 + 2*count + sum(size[i] & 0x7FFF for i < n)`.
//! A clear bit 15 means the body is the decoded payload verbatim; a set
//! bit 15 means it's compressed with the channel-delta codec
//! [`decode_channel_delta`] (`FUN_8002A9CC`). Either way the decoded output
//! is a standard packed keyframe stream
//! `[u8 parts][u8 frames][parts*frames x 9-byte TRS records]`
//! (`docs/formats/monster-animation.md`; parser
//! `crate::monster_archive::parse_animation_stream`).
//!
//! On the retail disc **every** entry of every player art archive has
//! bit 15 set, so the codec is required (no raw entry ships in the art
//! corpus; raw support is kept because the reader handles it).

use anyhow::{Context, Result, bail};

/// Archive magic.
pub const MAGIC: [u8; 2] = *b"ME";

/// Parsed view of one `"ME"` archive (borrows the source bytes).
#[derive(Debug, Clone)]
pub struct MeArchive<'a> {
    bytes: &'a [u8],
    /// Raw size words (bit 15 = compressed).
    sizes: Vec<u16>,
    /// Absolute body offset per entry.
    offsets: Vec<usize>,
}

/// Parse an archive header at the start of `bytes` (trailing bytes after
/// the last body - e.g. the rest of a `0x10800` streaming slot - are
/// ignored, matching the retail reader, which only walks the size chain).
// PORT: FUN_8002b28c - the archive walk: magic check, `u8 count`,
// `u16 entry_sizes[count]` (bit 15 = compressed), body offset = header end
// + sum of the preceding entries' low-15-bit sizes.
pub fn parse(bytes: &[u8]) -> Result<MeArchive<'_>> {
    if bytes.len() < 3 || bytes[..2] != MAGIC {
        bail!("not an ME archive (magic mismatch)");
    }
    let count = bytes[2] as usize;
    let table_end = 3 + 2 * count;
    if bytes.len() < table_end {
        bail!("ME size table truncated ({count} entries)");
    }
    let mut sizes = Vec::with_capacity(count);
    let mut offsets = Vec::with_capacity(count);
    let mut off = table_end;
    for n in 0..count {
        let sz = u16::from_le_bytes([bytes[3 + n * 2], bytes[4 + n * 2]]);
        offsets.push(off);
        off += (sz & 0x7FFF) as usize;
        if off > bytes.len() {
            bail!("ME entry {n} body past archive end");
        }
        sizes.push(sz);
    }
    Ok(MeArchive {
        bytes,
        sizes,
        offsets,
    })
}

impl<'a> MeArchive<'a> {
    /// Entry count.
    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    /// True when the archive holds no entries.
    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }

    /// Whether entry `n`'s body is compressed (size-word bit 15).
    pub fn is_compressed(&self, n: usize) -> Option<bool> {
        self.sizes.get(n).map(|s| s & 0x8000 != 0)
    }

    /// Entry `n`'s body bytes as stored (compressed or raw).
    pub fn raw_body(&self, n: usize) -> Option<&'a [u8]> {
        let off = *self.offsets.get(n)?;
        let len = (self.sizes[n] & 0x7FFF) as usize;
        self.bytes.get(off..off + len)
    }

    /// Decode entry `n` to its packed keyframe stream
    /// (`[u8 parts][u8 frames][9-byte TRS records]`): a verbatim copy for a
    /// raw entry, the [`decode_channel_delta`] codec for a compressed one.
    pub fn entry(&self, n: usize) -> Result<Vec<u8>> {
        let body = self
            .raw_body(n)
            .ok_or_else(|| anyhow::anyhow!("ME entry {n} out of range (count {})", self.len()))?;
        if self.is_compressed(n) == Some(true) {
            decode_channel_delta(body).with_context(|| format!("ME entry {n} codec"))
        } else {
            Ok(body.to_vec())
        }
    }
}

/// Bit-stream reader over the codec's MSB-first bit section.
struct BitCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl BitCursor<'_> {
    fn bit(&mut self) -> Result<bool> {
        let byte = self
            .bytes
            .get(self.pos >> 3)
            .ok_or_else(|| anyhow::anyhow!("codec bit stream exhausted"))?;
        let b = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Ok(b != 0)
    }
}

/// Decompress one channel-delta-coded keyframe stream into the packed
/// `[u8 parts][u8 frames][parts*frames x 9-byte TRS]` layout.
///
/// Source layout (5-byte header, all offsets relative to `src + 5`):
///
/// ```text
/// +0x00  u8  flags        ; (flags & 0xC0) must be 0x40
/// +0x01  u16 nibble_off   ; -> 4-bit operand stream (high nibble first)
/// +0x03  u16 byte_off     ; -> byte stream: [parts][frames][literal lows...]
/// +0x05  bit stream       ; MSB-first selector bits
/// ```
///
/// Per value (six 12-bit channels per part - `tx ty tz rx ry rz` - in
/// part-major order within each frame), the selector bits choose:
///
/// - `1`: 12-bit literal - `nibble << 8 | next byte`;
/// - `01`: previous part's same channel **delta** + nibble (`0..15`);
/// - `001`: previous-part delta + (nibble | 0xFF0) (negative nibble);
/// - `0001`: literal nibble (`0..15`);
/// - `0000`: literal nibble | 0xFF0 (negative nibble).
///
/// "Previous part" wraps to the row tail for the first part, i.e. the
/// channel 6 slots back in the per-frame delta row, which persists across
/// frames. Frame 0's deltas accumulate **spatially** (each part adds onto
/// the previous part's accumulated channel); later frames accumulate
/// **temporally** (each channel adds its delta onto its own running value).
/// Each accumulated frame row is emitted as `parts` 9-byte packed records
/// (low bytes + packed high nibbles - the `FUN_8004998C` unpack layout in
/// reverse).
// PORT: FUN_8002a9cc - the channel-delta codec (scratchpad table builder);
// the 0x1F800000 scratchpad delta/accumulator tables become the in-memory
// `delta` / `acc` rows here.
pub fn decode_channel_delta(src: &[u8]) -> Result<Vec<u8>> {
    if src.len() < 7 {
        bail!("codec source shorter than its header");
    }
    if src[0] & 0xC0 != 0x40 {
        bail!("codec header flags {:#04x} (want 0x4X)", src[0]);
    }
    let nibble_off = 5 + u16::from_le_bytes([src[1], src[2]]) as usize;
    let byte_off = 5 + u16::from_le_bytes([src[3], src[4]]) as usize;
    let nibbles = src
        .get(nibble_off..)
        .ok_or_else(|| anyhow::anyhow!("codec nibble stream offset past end"))?;
    let bytes = src
        .get(byte_off..)
        .ok_or_else(|| anyhow::anyhow!("codec byte stream offset past end"))?;
    if bytes.len() < 2 {
        bail!("codec byte stream shorter than its parts/frames head");
    }
    let parts = bytes[0] as usize;
    let frames = bytes[1] as usize;

    let mut bits = BitCursor {
        bytes: &src[5..],
        pos: 0,
    };
    let mut nib_pos = 0usize;
    let nibble = |pos: &mut usize| -> Result<u16> {
        let byte = nibbles
            .get(*pos >> 1)
            .ok_or_else(|| anyhow::anyhow!("codec nibble stream exhausted"))?;
        let v = if *pos & 1 == 0 { byte >> 4 } else { byte & 0xF };
        *pos += 1;
        Ok(v as u16)
    };
    let mut byte_pos = 2usize;
    let next_byte = |pos: &mut usize| -> Result<u16> {
        let v = bytes
            .get(*pos)
            .ok_or_else(|| anyhow::anyhow!("codec byte stream exhausted"))?;
        *pos += 1;
        Ok(*v as u16)
    };

    let channels = parts * 6;
    let mut delta = vec![0u16; channels];
    let mut acc = vec![0u16; channels];
    let mut out = Vec::with_capacity(2 + parts * frames * 9);
    out.push(parts as u8);
    out.push(frames as u8);

    for frame in 0..frames {
        for c in 0..channels {
            let v = if bits.bit()? {
                // 12-bit literal: nibble high, byte low.
                let n = nibble(&mut nib_pos)?;
                next_byte(&mut byte_pos)? | (n << 8)
            } else {
                let prev = || delta[if c < 6 { c + channels - 6 } else { c - 6 }];
                if bits.bit()? {
                    prev().wrapping_add(nibble(&mut nib_pos)?)
                } else if bits.bit()? {
                    prev().wrapping_add(nibble(&mut nib_pos)? | 0xFF0)
                } else if bits.bit()? {
                    nibble(&mut nib_pos)?
                } else {
                    nibble(&mut nib_pos)? | 0xFF0
                }
            };
            delta[c] = v;
        }
        if frame == 0 {
            acc[..6].copy_from_slice(&delta[..6]);
            for c in 6..channels {
                acc[c] = delta[c].wrapping_add(acc[c - 6]);
            }
        } else {
            for c in 0..channels {
                acc[c] = acc[c].wrapping_add(delta[c]);
            }
        }
        for p in 0..parts {
            let ch = &acc[p * 6..p * 6 + 6];
            for pair in 0..3 {
                let (a, b) = (ch[pair * 2], ch[pair * 2 + 1]);
                out.push(a as u8);
                out.push(b as u8);
                out.push(((a >> 8) as u8 & 0x0F) | ((b >> 4) as u8 & 0xF0));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid archive: one raw entry + one compressed entry.
    fn synth_archive(raw: &[u8], compressed: &[u8]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&MAGIC);
        a.push(2);
        a.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        a.extend_from_slice(&((compressed.len() as u16) | 0x8000).to_le_bytes());
        a.extend_from_slice(raw);
        a.extend_from_slice(compressed);
        a
    }

    /// Hand-built codec stream: parts = 1, frames = 2.
    ///
    /// Frame 0: six 12-bit literals (selector `1`) - values
    /// `0x123 0x456 0x789 0xABC 0x0DE 0xF01`.
    /// Frame 1: six small positive nibble literals (selector `0001`) -
    /// deltas `1 2 3 4 5 6` accumulated onto frame 0.
    fn synth_codec_stream() -> (Vec<u8>, Vec<u8>) {
        let f0 = [0x123u16, 0x456, 0x789, 0xABC, 0x0DE, 0xF01];
        // Bit stream: frame 0 = six `1` bits; frame 1 = six `0001` groups.
        // 6 + 24 = 30 bits, MSB-first.
        let mut bitstring: Vec<u8> = Vec::new();
        bitstring.extend(std::iter::repeat_n(1u8, 6));
        for _ in 0..6 {
            bitstring.extend_from_slice(&[0, 0, 0, 1]);
        }
        let mut bits = vec![0u8; bitstring.len().div_ceil(8)];
        for (i, b) in bitstring.iter().enumerate() {
            bits[i >> 3] |= b << (7 - (i & 7));
        }
        // Nibble stream: frame 0 high nibbles, then frame 1 deltas.
        let nibblestring: Vec<u8> = f0.iter().map(|v| (v >> 8) as u8).chain(1..=6u8).collect();
        let mut nibs = vec![0u8; nibblestring.len().div_ceil(2)];
        for (i, n) in nibblestring.iter().enumerate() {
            nibs[i >> 1] |= if i & 1 == 0 { n << 4 } else { *n };
        }
        // Byte stream: [parts][frames][frame-0 low bytes].
        let mut bytes = vec![1u8, 2u8];
        bytes.extend(f0.iter().map(|v| *v as u8));

        let mut src = vec![0x40u8];
        let nib_off = bits.len() as u16;
        let byte_off = (bits.len() + nibs.len()) as u16;
        src.extend_from_slice(&nib_off.to_le_bytes());
        src.extend_from_slice(&byte_off.to_le_bytes());
        src.extend_from_slice(&bits);
        src.extend_from_slice(&nibs);
        src.extend_from_slice(&bytes);

        // Expected packed output.
        let pack = |ch: [u16; 6]| -> Vec<u8> {
            let mut o = Vec::new();
            for pair in 0..3 {
                let (a, b) = (ch[pair * 2], ch[pair * 2 + 1]);
                o.push(a as u8);
                o.push(b as u8);
                o.push(((a >> 8) as u8 & 0x0F) | ((b >> 4) as u8 & 0xF0));
            }
            o
        };
        let f1: [u16; 6] = std::array::from_fn(|i| f0[i] + (i as u16 + 1));
        let mut expect = vec![1u8, 2u8];
        expect.extend(pack(f0));
        expect.extend(pack(f1));
        (src, expect)
    }

    #[test]
    fn codec_decodes_literals_and_nibble_deltas() {
        let (src, expect) = synth_codec_stream();
        let out = decode_channel_delta(&src).expect("codec ok");
        assert_eq!(out, expect);
    }

    #[test]
    fn codec_rejects_bad_header() {
        let (mut src, _) = synth_codec_stream();
        src[0] = 0x80;
        assert!(decode_channel_delta(&src).is_err());
        assert!(decode_channel_delta(&[0x40, 0, 0]).is_err());
    }

    #[test]
    fn archive_walks_raw_and_compressed_entries() {
        let (codec_src, expect) = synth_codec_stream();
        let raw = [2u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let bytes = synth_archive(&raw, &codec_src);
        let a = parse(&bytes).expect("parse");
        assert_eq!(a.len(), 2);
        assert_eq!(a.is_compressed(0), Some(false));
        assert_eq!(a.is_compressed(1), Some(true));
        assert_eq!(a.entry(0).unwrap(), raw.to_vec());
        assert_eq!(a.entry(1).unwrap(), expect);
        assert!(a.entry(2).is_err());
    }

    #[test]
    fn archive_rejects_bad_magic_and_truncation() {
        assert!(parse(b"NO").is_err());
        assert!(parse(b"ME").is_err());
        // Count claims 2 entries but the table is cut short.
        assert!(parse(&[b'M', b'E', 2, 4, 0]).is_err());
        // Body extends past the end.
        assert!(parse(&[b'M', b'E', 1, 8, 0, 1, 2]).is_err());
    }
}
