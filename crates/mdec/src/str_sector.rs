//! PSX STR video-sector parser and frame assembler.
//!
//! Each PSX STR file is a sequence of Mode 2 Form 1 CD-ROM sectors. After
//! stripping the Mode 2 subheader (handled by the `legaia-iso` disc reader),
//! each sector's 2048-byte data area has the layout:
//!
//! ```text
//! Offset  Bytes  Field
//! 0x000   2      sector_magic      (0x0160 for video sectors)
//! 0x002   2      chunk_number      (0-indexed position of this sector in the frame)
//! 0x004   2      chunks_per_frame  (total sectors needed for this frame)
//! 0x006   2      frame_number      (frame index, wraps at 0xFFFF)
//! 0x008   4      bs_data_size      (total bitstream bytes across all sectors)
//! 0x00C   2      width             (frame width in pixels)
//! 0x00E   2      height            (frame height in pixels)
//! 0x010   2      bs_version        (2 = PSX BS v2)
//! 0x012   2      quantize_scale    (per-frame quantization scale)
//! 0x014   2028   bs_data           (bitstream payload bytes for this sector)
//! ```
//!
//! `StrFrameAssembler` collects sectors by `frame_number` and calls
//! `on_frame` when a complete frame has arrived.
//!
//! ## Source
//!
//! PSX-SPX §MDEC - "STR Movie Files", plus cross-reference with the
//! Mednafen and PCSX-Redux implementations (clean-room: only the protocol
//! spec, not source bytes, was used).

use anyhow::{Context, Result, bail};

/// Magic value at offset 0 of a video sector.
pub const VIDEO_SECTOR_MAGIC: u16 = 0x0160;

/// Bytes per STR sector data area (Mode 2 Form 1 user data).
pub const SECTOR_DATA_BYTES: usize = 2048;

/// Payload bytes in a single sector (total minus 20-byte header).
pub const SECTOR_PAYLOAD_BYTES: usize = SECTOR_DATA_BYTES - 20;

/// Parsed header from one STR video sector.
#[derive(Debug, Clone)]
pub struct StrSectorHeader {
    /// 0-indexed position of this sector within the current frame.
    pub chunk_number: u16,
    /// Total number of sectors required to complete this frame.
    pub chunks_per_frame: u16,
    /// Frame sequence number.
    pub frame_number: u16,
    /// Total bitstream size in bytes across all sectors for this frame.
    pub bs_data_size: u32,
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Bitstream version. Legaia uses 2.
    pub bs_version: u16,
    /// Per-frame quantization scale.
    pub quantize_scale: u16,
}

/// Parse the header of a 2048-byte video sector data area.
///
/// Returns `None` if the magic doesn't match (audio or non-video sector).
/// Returns an error if the buffer is too short.
pub fn parse_video_sector(sector_data: &[u8]) -> Result<Option<(StrSectorHeader, &[u8])>> {
    if sector_data.len() < 20 {
        bail!("STR sector too short: {} bytes", sector_data.len());
    }
    let magic = u16::from_le_bytes(sector_data[0..2].try_into().unwrap());
    if magic != VIDEO_SECTOR_MAGIC {
        return Ok(None);
    }
    let hdr = StrSectorHeader {
        chunk_number: u16::from_le_bytes(sector_data[2..4].try_into().unwrap()),
        chunks_per_frame: u16::from_le_bytes(sector_data[4..6].try_into().unwrap()),
        frame_number: u16::from_le_bytes(sector_data[6..8].try_into().unwrap()),
        bs_data_size: u32::from_le_bytes(sector_data[8..12].try_into().unwrap()),
        width: u16::from_le_bytes(sector_data[12..14].try_into().unwrap()),
        height: u16::from_le_bytes(sector_data[14..16].try_into().unwrap()),
        bs_version: u16::from_le_bytes(sector_data[16..18].try_into().unwrap()),
        quantize_scale: u16::from_le_bytes(sector_data[18..20].try_into().unwrap()),
    };
    let payload = &sector_data[20..sector_data.len().min(20 + SECTOR_PAYLOAD_BYTES)];
    Ok(Some((hdr, payload)))
}

/// Collects STR sectors and assembles complete frame bitstreams.
///
/// Feed each 2048-byte video sector data area through [`StrFrameAssembler::push_sector`].
/// When a complete frame is assembled the provided callback receives the
/// [`StrSectorHeader`] from the first sector and the concatenated BS payload.
pub struct StrFrameAssembler {
    current_frame: Option<u16>,
    chunks_expected: u16,
    header: Option<StrSectorHeader>,
    payload: Vec<u8>,
}

impl StrFrameAssembler {
    /// Create a new assembler with no in-progress frame.
    pub fn new() -> Self {
        Self {
            current_frame: None,
            chunks_expected: 0,
            header: None,
            payload: Vec::new(),
        }
    }

    /// Push a 2048-byte sector data area. Returns a complete `(header, bs_bytes)`
    /// pair when the current frame finishes, or `Ok(None)` otherwise.
    ///
    /// Audio sectors and non-video sectors are silently skipped (returns `Ok(None)`).
    pub fn push_sector(
        &mut self,
        sector_data: &[u8],
    ) -> Result<Option<(StrSectorHeader, Vec<u8>)>> {
        let Some((hdr, payload)) =
            parse_video_sector(sector_data).context("parse STR video sector")?
        else {
            return Ok(None);
        };

        // Start a new frame if needed
        if self.current_frame != Some(hdr.frame_number) {
            self.current_frame = Some(hdr.frame_number);
            self.chunks_expected = hdr.chunks_per_frame;
            self.header = Some(hdr.clone());
            self.payload.clear();
        }

        // Append payload bytes (limited to bs_data_size remaining)
        let remaining =
            hdr.bs_data_size as usize - self.payload.len().min(hdr.bs_data_size as usize);
        let to_copy = payload.len().min(remaining);
        self.payload.extend_from_slice(&payload[..to_copy]);

        // Check if the frame is complete
        let chunk_no = hdr.chunk_number;
        if chunk_no + 1 >= self.chunks_expected {
            let full_header = self.header.take().unwrap();
            let bs = std::mem::take(&mut self.payload);
            self.current_frame = None;
            return Ok(Some((full_header, bs)));
        }

        Ok(None)
    }

    /// Discard any in-progress frame (e.g. on seek).
    pub fn reset(&mut self) {
        self.current_frame = None;
        self.header = None;
        self.payload.clear();
        self.chunks_expected = 0;
    }

    /// Returns `true` if a frame is currently being assembled.
    pub fn in_progress(&self) -> bool {
        self.current_frame.is_some()
    }
}

impl Default for StrFrameAssembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sector(chunk: u16, total: u16, frame: u16, bs_size: u32) -> Vec<u8> {
        let mut s = vec![0u8; SECTOR_DATA_BYTES];
        s[0..2].copy_from_slice(&VIDEO_SECTOR_MAGIC.to_le_bytes());
        s[2..4].copy_from_slice(&chunk.to_le_bytes());
        s[4..6].copy_from_slice(&total.to_le_bytes());
        s[6..8].copy_from_slice(&frame.to_le_bytes());
        s[8..12].copy_from_slice(&bs_size.to_le_bytes());
        s[12..14].copy_from_slice(&320u16.to_le_bytes()); // width
        s[14..16].copy_from_slice(&240u16.to_le_bytes()); // height
        s[16..18].copy_from_slice(&2u16.to_le_bytes()); // bs_ver
        s[18..20].copy_from_slice(&8u16.to_le_bytes()); // qs
        s
    }

    #[test]
    fn single_sector_frame_completes_immediately() {
        let sector = make_sector(0, 1, 0, 0);
        let mut asm = StrFrameAssembler::new();
        let result = asm.push_sector(&sector).unwrap();
        assert!(result.is_some());
        let (hdr, _bs) = result.unwrap();
        assert_eq!(hdr.frame_number, 0);
        assert_eq!(hdr.width, 320);
        assert_eq!(hdr.height, 240);
    }

    #[test]
    fn multi_sector_frame_assembles_across_pushes() {
        let bs_size = SECTOR_PAYLOAD_BYTES as u32 * 2;
        let s0 = make_sector(0, 2, 5, bs_size);
        let s1 = make_sector(1, 2, 5, bs_size);
        let mut asm = StrFrameAssembler::new();
        assert!(asm.push_sector(&s0).unwrap().is_none()); // not yet complete
        let result = asm.push_sector(&s1).unwrap();
        assert!(result.is_some());
        let (hdr, bs) = result.unwrap();
        assert_eq!(hdr.frame_number, 5);
        assert_eq!(bs.len(), bs_size as usize);
    }

    #[test]
    fn non_video_sector_returns_none() {
        let mut sector = vec![0u8; SECTOR_DATA_BYTES];
        sector[0..2].copy_from_slice(&0x0161u16.to_le_bytes()); // wrong magic
        let mut asm = StrFrameAssembler::new();
        assert!(asm.push_sector(&sector).unwrap().is_none());
    }

    #[test]
    fn new_frame_number_resets_assembler() {
        let s0 = make_sector(0, 2, 0, 100);
        let s1 = make_sector(0, 1, 1, 0); // new frame, single-sector
        let mut asm = StrFrameAssembler::new();
        asm.push_sector(&s0).unwrap(); // frame 0 chunk 0
        let result = asm.push_sector(&s1).unwrap(); // frame 1 completes immediately
        assert!(result.is_some());
        assert_eq!(result.unwrap().0.frame_number, 1);
    }

    #[test]
    fn parse_video_sector_extracts_fields() {
        let s = make_sector(2, 4, 10, 8000);
        let (hdr, payload) = parse_video_sector(&s).unwrap().unwrap();
        assert_eq!(hdr.chunk_number, 2);
        assert_eq!(hdr.chunks_per_frame, 4);
        assert_eq!(hdr.frame_number, 10);
        assert_eq!(hdr.bs_data_size, 8000);
        assert_eq!(payload.len(), SECTOR_PAYLOAD_BYTES);
    }
}
