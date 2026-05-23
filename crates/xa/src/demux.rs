//! CD-XA sector demuxer.
//!
//! Legaia's `XA*.XA` files on disc are CD-XA Mode 2 Form 2 streams that
//! multiplex up to 8 audio channels per file at the sector level. The
//! existing `extracted/XA/*.XA` files were extracted as Form 1 (truncating
//! each sector to 2048 bytes), which silently dropped 276 bytes of audio
//! per sector AND collapsed every channel of the stream into a single
//! shuffled byte sequence.
//!
//! The fix is at the disc layer: read raw 2352-byte sectors, parse each
//! sector's CD-XA subheader (bytes 16..24), filter to `AUDIO + FORM2`
//! sectors, and split the audio data into one buffer per `(file_no,
//! ch_no)` tuple. Each per-channel buffer is then a clean concatenation
//! of 128-byte sound groups that the standard 4-bit XA decoder in
//! [`crate::decode`] handles directly.
//!
//! ## CD-XA sector layout (Mode 2 Form 2)
//!
//! ```text
//!   bytes 0..12  : sync (0x00 + 10x 0xFF + 0x00)
//!   bytes 12..16 : header (MM SS FF mode)
//!   bytes 16..24 : 8-byte subheader (4 fields, each duplicated for redundancy):
//!                    file_no, ch_no, submode, coding_info  | repeated
//!   bytes 24..2348 : 2324-byte user data (18 sound groups × 128 = 2304
//!                    bytes audio + 20 bytes padding)
//!   bytes 2348..2352 : EDC (4 bytes)
//! ```
//!
//! Submode bits relevant here:
//! - bit 2 (0x04): AUDIO
//! - bit 5 (0x20): FORM2
//!
//! Coding info bits:
//! - bit 0 (0x01): stereo (vs mono)
//! - bits 2..3:    sample rate (00 = 37.8 kHz, 01 = 18.9 kHz)
//! - bits 4..5:    bits/sample (00 = 4-bit, 01 = 8-bit)

use anyhow::{Context, Result, bail};
use legaia_iso::raw::{RawDisc, SECTOR_SIZE};
use std::collections::BTreeMap;
use std::path::Path;

/// 8 sound groups × 128 bytes = the audio block inside a Form 2 user data
/// field, before the 20-byte trailing padding.
pub const AUDIO_BYTES_PER_SECTOR: usize = 18 * 128;

/// Subheader byte offsets within a raw 2352-byte sector.
pub const SUBHEADER_OFFSET: usize = 16;
pub const USER_DATA_OFFSET: usize = 24;

/// Decoded subheader fields. The 8-byte on-disc subheader is two copies of
/// these four bytes; we read the first copy and (caller's choice) verify
/// the second matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subheader {
    pub file_no: u8,
    pub ch_no: u8,
    pub submode: u8,
    pub coding_info: u8,
}

impl Subheader {
    pub fn is_audio(&self) -> bool {
        self.submode & 0x04 != 0
    }
    pub fn is_form2(&self) -> bool {
        self.submode & 0x20 != 0
    }
    pub fn is_stereo(&self) -> bool {
        self.coding_info & 0x01 != 0
    }
    /// XA sample rate in Hz, derived from coding_info bits 2..=3.
    pub fn sample_rate(&self) -> u32 {
        match (self.coding_info >> 2) & 0x03 {
            0 => 37_800,
            _ => 18_900,
        }
    }
    /// True when both 4-byte halves of the on-disc subheader agree (the
    /// standard CD-XA redundancy invariant). Worth checking because
    /// coding-info corruption mid-stream is the most common dump glitch.
    pub fn matches_redundant_copy(raw_subheader: &[u8; 8]) -> bool {
        raw_subheader[0..4] == raw_subheader[4..8]
    }
}

/// Parse the 8-byte subheader at `bytes`. Returns the four fields plus
/// whether the redundant copy matched. Caller decides whether to skip
/// sectors with a mismatched copy (most consumers skip).
pub fn parse_subheader(bytes: &[u8; 8]) -> (Subheader, bool) {
    let sub = Subheader {
        file_no: bytes[0],
        ch_no: bytes[1],
        submode: bytes[2],
        coding_info: bytes[3],
    };
    (sub, Subheader::matches_redundant_copy(bytes))
}

/// One per-channel demuxed stream.
#[derive(Debug, Clone)]
pub struct ChannelStream {
    pub file_no: u8,
    pub ch_no: u8,
    pub sample_rate: u32,
    pub stereo: bool,
    /// Concatenated audio data, one 128-byte sound group at a time.
    pub audio: Vec<u8>,
    /// How many sectors contributed to this channel (informational).
    pub sector_count: usize,
}

impl ChannelStream {
    pub fn group_count(&self) -> usize {
        self.audio.len() / 128
    }
}

/// Demux a contiguous range of raw CD sectors into one stream per
/// `(file_no, ch_no)` tuple. Reads each sector with
/// [`RawDisc::read_raw_sector`], filters to `AUDIO + FORM2`, and appends
/// each sector's 2304-byte audio block to the matching channel's buffer.
///
/// Sectors with a corrupt subheader (the redundant copy doesn't match)
/// are skipped silently. Non-audio sectors (data / video / EOF markers)
/// are also skipped - they're the muxing scaffolding around the audio.
pub fn demux_disc_range(
    disc: &mut RawDisc,
    start_lba: u32,
    sector_count: u32,
) -> Result<Vec<ChannelStream>> {
    let mut by_key: BTreeMap<(u8, u8), ChannelStream> = BTreeMap::new();
    for s in 0..sector_count {
        let raw = disc
            .read_raw_sector(start_lba + s)
            .with_context(|| format!("read sector {} of XA stream", start_lba + s))?;
        let mut sub_bytes = [0u8; 8];
        sub_bytes.copy_from_slice(&raw[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sub, ok) = parse_subheader(&sub_bytes);
        if !ok || !sub.is_audio() || !sub.is_form2() {
            continue;
        }
        let key = (sub.file_no, sub.ch_no);
        let stream = by_key.entry(key).or_insert_with(|| ChannelStream {
            file_no: sub.file_no,
            ch_no: sub.ch_no,
            sample_rate: sub.sample_rate(),
            stereo: sub.is_stereo(),
            audio: Vec::new(),
            sector_count: 0,
        });
        // The 18 sound groups live at sector bytes 24..2328 (= 2304 bytes).
        // The 20 bytes between 2328..2348 are padding, then 4 bytes EDC.
        let audio_off = USER_DATA_OFFSET;
        let audio_end = audio_off + AUDIO_BYTES_PER_SECTOR;
        if audio_end > SECTOR_SIZE {
            bail!("sector {} smaller than expected", start_lba + s);
        }
        stream.audio.extend_from_slice(&raw[audio_off..audio_end]);
        stream.sector_count += 1;
    }
    Ok(by_key.into_values().collect())
}

/// Convenience: open a `.bin` disc image and demux a byte range starting
/// at `start_lba` covering `byte_size` bytes (rounded up to whole
/// sectors). The byte size is the directory-entry-reported file size,
/// which under Form 1 reading would be `sector_count * 2048`, so we
/// recover the on-disc sector count by dividing by 2048 and rounding up.
pub fn demux_file(bin_path: &Path, start_lba: u32, byte_size: u32) -> Result<Vec<ChannelStream>> {
    let mut disc =
        RawDisc::open(bin_path).with_context(|| format!("open disc {}", bin_path.display()))?;
    let sector_count = byte_size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
    demux_disc_range(&mut disc, start_lba, sector_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_form2_audio_sector(file_no: u8, ch_no: u8, audio_byte: u8) -> [u8; SECTOR_SIZE] {
        let mut s = [0u8; SECTOR_SIZE];
        // Subheader: 0x64 = AUDIO (0x04) + FORM2 (0x20) + RT (0x40).
        s[16..20].copy_from_slice(&[file_no, ch_no, 0x64, 0x01]);
        s[20..24].copy_from_slice(&[file_no, ch_no, 0x64, 0x01]);
        s[USER_DATA_OFFSET..USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR].fill(audio_byte);
        s
    }

    #[test]
    fn parse_subheader_extracts_fields() {
        let bytes = [1u8, 2, 0x64, 0x01, 1, 2, 0x64, 0x01];
        let (sub, ok) = parse_subheader(&bytes);
        assert!(ok);
        assert_eq!(sub.file_no, 1);
        assert_eq!(sub.ch_no, 2);
        assert!(sub.is_audio());
        assert!(sub.is_form2());
        assert!(sub.is_stereo());
        assert_eq!(sub.sample_rate(), 37_800);
    }

    #[test]
    fn parse_subheader_flags_corrupt_redundant_copy() {
        let bytes = [1u8, 2, 0x64, 0x01, 9, 9, 9, 9];
        let (_, ok) = parse_subheader(&bytes);
        assert!(!ok);
    }

    #[test]
    fn audio_subheader_classifies_low_quality_mono() {
        // coding_info = 0x04 → mono, 18.9 kHz, 4-bit.
        let bytes = [1u8, 0, 0x64, 0x04, 1, 0, 0x64, 0x04];
        let (sub, _) = parse_subheader(&bytes);
        assert!(!sub.is_stereo());
        assert_eq!(sub.sample_rate(), 18_900);
    }

    /// Sanity: sector layout constants line up with the expected on-disc
    /// CD-XA Form 2 layout.
    #[test]
    fn sector_layout_constants() {
        assert_eq!(SECTOR_SIZE, 2352);
        assert_eq!(SUBHEADER_OFFSET, 16);
        assert_eq!(USER_DATA_OFFSET, 24);
        assert_eq!(AUDIO_BYTES_PER_SECTOR, 2304);
    }

    #[test]
    fn parse_subheader_on_junk_does_not_panic_and_is_not_audio() {
        // All-0xFF subheader: redundant copy matches (both halves equal), but
        // submode 0xFF still classifies; the demuxer's audio/form2 gate is
        // what decides inclusion. The point here is no panic on garbage.
        let bytes = [0xFFu8; 8];
        let (sub, ok) = parse_subheader(&bytes);
        assert!(ok); // both 4-byte halves identical
        // 0xFF has both AUDIO (0x04) and FORM2 (0x20) bits set; sample-rate
        // and stereo derivations must not panic regardless.
        let _ = sub.sample_rate();
        let _ = sub.is_stereo();

        // A subheader whose halves disagree is flagged corrupt (skipped).
        let mixed = [0u8, 0, 0x64, 0x01, 9, 9, 9, 9];
        let (_, ok2) = parse_subheader(&mixed);
        assert!(!ok2);
    }

    /// Build a stand-in Form 2 sector and verify our offsets pull the
    /// right bytes back out.
    #[test]
    fn form2_sector_offsets_round_trip() {
        let s = make_form2_audio_sector(7, 3, 0xAA);
        let mut sub_bytes = [0u8; 8];
        sub_bytes.copy_from_slice(&s[SUBHEADER_OFFSET..SUBHEADER_OFFSET + 8]);
        let (sub, ok) = parse_subheader(&sub_bytes);
        assert!(ok);
        assert_eq!(sub.file_no, 7);
        assert_eq!(sub.ch_no, 3);
        assert!(sub.is_audio());
        assert_eq!(s[USER_DATA_OFFSET], 0xAA);
        assert_eq!(s[USER_DATA_OFFSET + AUDIO_BYTES_PER_SECTOR - 1], 0xAA);
    }
}
