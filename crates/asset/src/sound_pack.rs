//! Sound-pack (`sound_data2` / `.dpk`) decoder - the VAB+SEQ bundle.
//!
//! ### What it is
//!
//! A per-scene sound pack is a **type-`0x02`-terminated streaming-chunk
//! container** (the `FUN_8001FE70` walker; see [`crate::parse_streaming_with`]
//! with [`StreamTerminator::TypeTwo`]) whose chunks are a **VAB + SEQ bundle**:
//!
//! | Chunk type | Role |
//! |---|---|
//! | `0` | VAB **header** section (magic `pBAV` = `0x5641_4270` LE). |
//! | `1` | VAB **sample** section (the SPU-ADPCM / VAG waveform pool). |
//! | `2` | **SEQ** (magic `pQES`), which is also the stream terminator. |
//!
//! ### The decisive invariant
//!
//! The type-0 and type-1 chunks reconstitute **one contiguous VAB**: the VAB
//! header's declared `total_size` (at header `+0x0C`) equals
//! `chunk[0].size + chunk[1].size`. Byte-verified across the `sound_data2`
//! corpus (`0877`..=`0885`: `total_size == c0+c1` exactly, e.g. `0877`
//! `0x2820 + 0x1BA90 == 0x1E2B0`).
//!
//! ### Why this resolves the open question
//!
//! `FUN_8001FE70`'s **type-1** chunk handler is the graphics-side TIM/CLUT
//! upload (`FUN_800198E0`) on the *battle-init* walk, so the sound side's
//! type-1 payload could not be a TIM. It isn't: on the sound side type-1 is
//! the **VAB sample pool**. The pack is not a novel `.MAP`/`.PCH`/`.spk`
//! layout - it is a [`VAB`](../vab/index.html) (header chunk + sample chunk)
//! plus a trailing [`SEQ`](../seq/index.html), the same content shape as the
//! [`scene_vab_stream`](crate::scene_vab_stream) BGM wrapper but carried in
//! the type-2-terminated streaming container instead of the chunk0-prefixed
//! one. See `docs/formats/sound-driver.md`.

use serde::Serialize;

use crate::{StreamTerminator, parse_streaming_with};

/// VAB header magic as a little-endian `u32` (`pBAV` on disk - PsyQ's `VABp`).
pub const VAB_MAGIC_LE: u32 = 0x5641_4270;

/// SEQ file magic, in source byte order (`pQES` on disk).
pub const SEQ_MAGIC: [u8; 4] = *b"pQES";

/// Byte offset of the `total_size` field inside a VAB header.
const VAB_TOTAL_SIZE_OFFSET: usize = 0x0C;

/// Max chunks to walk - a sound pack is tiny (header + samples + SEQ).
const MAX_CHUNKS: usize = 16;

/// A decoded sound pack: the reconstituted VAB plus the trailing SEQ.
#[derive(Debug, Clone, Serialize)]
pub struct SoundPack {
    /// The reconstituted VAB bytes (type-0 header chunk + type-1 sample chunk
    /// payloads concatenated, truncated to the header's `total_size`). Parses
    /// directly with `legaia_vab`.
    pub vab: Vec<u8>,
    /// `total_size` declared by the VAB header (`+0x0C`).
    pub vab_total_size: u32,
    /// `true` when the concatenated pre-terminator payloads reach
    /// `vab_total_size` exactly (the clean `[header][samples]` split). `false`
    /// means the pack carried fewer sample bytes than the header declares
    /// (an outlier layout); `vab` then holds whatever was present.
    pub vab_complete: bool,
    /// The type-2 terminator chunk payload, when it is a SEQ (`pQES`). Parses
    /// with `legaia_seq`.
    pub seq: Option<Vec<u8>>,
}

/// Returns `true` when `buf` leads with a sound-pack VAB header chunk: a
/// type-0 streaming chunk whose payload starts with the VAB magic.
pub fn detect(buf: &[u8]) -> bool {
    // type-0 chunk header at +0, VAB magic at the payload (+4).
    if buf.len() < 8 {
        return false;
    }
    let header = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if (header >> 24) & 0xFF != 0 {
        return false;
    }
    let magic = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    magic == VAB_MAGIC_LE
}

/// Decode a sound pack into its VAB + SEQ parts. Returns `None` when the
/// buffer does not lead with a VAB header chunk or the streaming walk doesn't
/// reach a type-2 terminator.
pub fn extract(buf: &[u8]) -> Option<SoundPack> {
    if !detect(buf) {
        return None;
    }
    let report = parse_streaming_with(buf, MAX_CHUNKS, StreamTerminator::TypeTwo).ok()?;
    if !report.terminated {
        return None;
    }

    // VAB total_size from the leading header chunk's payload.
    let head = report.chunks.first()?;
    let head_payload = head.header_offset + 4;
    let total_size = u32::from_le_bytes(
        buf.get(head_payload + VAB_TOTAL_SIZE_OFFSET..head_payload + VAB_TOTAL_SIZE_OFFSET + 4)?
            .try_into()
            .ok()?,
    );

    // Concatenate every pre-terminator chunk payload - these are the VAB
    // header section (type 0) followed by the sample section (type 1).
    let mut vab = Vec::with_capacity(total_size as usize);
    let last = report.chunks.len().saturating_sub(1);
    for c in &report.chunks[..last] {
        let start = c.header_offset + 4;
        let end = start + c.size as usize;
        vab.extend_from_slice(buf.get(start..end)?);
    }

    let vab_complete = vab.len() >= total_size as usize;
    if vab_complete {
        vab.truncate(total_size as usize);
    }

    // The terminator chunk is the SEQ (when it carries the SEQ magic).
    let term = report.chunks.get(last)?;
    let term_payload = term.header_offset + 4;
    let seq = buf
        .get(term_payload..term_payload + term.size as usize)
        .filter(|b| b.len() >= 4 && b[0..4] == SEQ_MAGIC)
        .map(|b| b.to_vec());

    Some(SoundPack {
        vab,
        vab_total_size: total_size,
        vab_complete,
        seq,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `(type << 24) | size` chunk header + padded body.
    fn chunk(type_byte: u8, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let header = ((type_byte as u32) << 24) | (body.len() as u32 & 0x00FF_FFFF);
        out.extend_from_slice(&header.to_le_bytes());
        out.extend_from_slice(body);
        // 4-byte align the body (matches the walker's `(size & ~3) + 4`).
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    /// Synthesize a minimal `[VAB header][VAB samples][SEQ]` sound pack with a
    /// header `total_size` equal to header-body + sample-body.
    fn synth() -> Vec<u8> {
        let header_body_len = 0x20usize; // a tiny stand-in VAB header
        let sample_body_len = 0x40usize;
        let total = (header_body_len + sample_body_len) as u32;

        let mut header_body = vec![0u8; header_body_len];
        header_body[0..4].copy_from_slice(&VAB_MAGIC_LE.to_le_bytes());
        header_body[VAB_TOTAL_SIZE_OFFSET..VAB_TOTAL_SIZE_OFFSET + 4]
            .copy_from_slice(&total.to_le_bytes());

        let sample_body = vec![0xABu8; sample_body_len];
        let mut seq_body = vec![0u8; 0x10];
        seq_body[0..4].copy_from_slice(&SEQ_MAGIC);

        let mut buf = Vec::new();
        buf.extend_from_slice(&chunk(0, &header_body));
        buf.extend_from_slice(&chunk(1, &sample_body));
        buf.extend_from_slice(&chunk(2, &seq_body));
        buf
    }

    #[test]
    fn detects_vab_header_lead() {
        assert!(detect(&synth()));
        // A non-VAB lead is rejected.
        let mut bad = synth();
        bad[4] = 0xFF;
        assert!(!detect(&bad));
    }

    #[test]
    fn extracts_vab_and_seq() {
        let buf = synth();
        let p = extract(&buf).expect("sound pack should decode");
        assert_eq!(p.vab_total_size, 0x60);
        assert!(p.vab_complete);
        // Reconstituted VAB == header body + sample body, truncated to total.
        assert_eq!(p.vab.len(), 0x60);
        assert_eq!(
            u32::from_le_bytes(p.vab[0..4].try_into().unwrap()),
            VAB_MAGIC_LE
        );
        // Sample bytes follow the header section.
        assert_eq!(p.vab[0x20], 0xAB);
        // SEQ surfaced from the terminator.
        let seq = p.seq.expect("terminator is a SEQ");
        assert_eq!(seq[0..4], SEQ_MAGIC);
    }

    #[test]
    fn rejects_non_sound_pack() {
        // A bare DATA_FIELD-ish buffer with no VAB lead.
        let buf = chunk(0, &[0u8; 0x20]);
        assert!(extract(&buf).is_none());
    }
}
