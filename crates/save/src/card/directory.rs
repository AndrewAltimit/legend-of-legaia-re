//! The card container: block and directory-frame geometry, the directory
//! walk, block reads, a formatted blank card and the block writer.

use anyhow::{Result, bail};
use serde::Serialize;

/// Memory-card block size in bytes (8 KB).
pub const BLOCK_SIZE: usize = 0x2000;

/// Total memory-card size in bytes (128 KB).
pub const CARD_SIZE: usize = BLOCK_SIZE * 16;

/// Number of directory frames (one per block).
pub const DIR_FRAMES: usize = 15;

/// Size of one directory frame in bytes.
pub const DIR_FRAME_SIZE: usize = 0x80;

/// `MC` magic at offset 0.
pub const CARD_MAGIC: [u8; 2] = *b"MC";

/// `SC` magic at the start of each save block.
pub const SAVE_BLOCK_MAGIC: [u8; 2] = *b"SC";

/// The **full** four-byte header retail stamps at block `+0`: the `SC` magic,
/// the icon-frame descriptor `0x11` (one frame, 16 colours), and the block
/// count `1`.
///
/// Writing only [`SAVE_BLOCK_MAGIC`] leaves `+2` and `+3` as found, and in a
/// previously-free block that is zero - which the BIOS card browser reads as
/// a malformed entry however correct the payload behind it is.
///
/// PORT: FUN_801e1934 (`0x801E19FC..0x801E1A14`)
pub const SAVE_BLOCK_HEADER: [u8; 4] = [b'S', b'C', 0x11, 0x01];

/// Directory-frame state codes.
pub mod state {
    /// Block holds the first frame of a save (and possibly the only one).
    pub const FIRST_BLOCK: u32 = 0x51;
    /// Block continues a multi-block save.
    pub const MID_BLOCK: u32 = 0x52;
    /// Block holds the final frame of a save.
    pub const LAST_BLOCK: u32 = 0x53;
    /// Block is unused / available.
    pub const FREE: u32 = 0xA0;
}

/// One directory entry walked from the card's directory frames.
#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    /// Block index (1..=15).
    pub block: u8,
    /// Raw state byte (`0x51` = first, `0x52` = mid, `0x53` = last,
    /// `0xA0` = free).
    pub state: u32,
    /// File size in bytes the directory frame declares. Block-aligned on a
    /// real card (`blocks * BLOCK_SIZE`), so it is the save's footprint, not
    /// its payload length.
    pub file_size: u32,
    /// Next block index (`0xFFFF` for terminal).
    pub next_block: u16,
    /// Region/product code (typically `BASCUSXXXXX...`).
    pub product_code: String,
    /// Game-specific name region (variable, may include shift-JIS).
    pub name: Vec<u8>,
}

impl DirEntry {
    /// `true` if this directory entry marks the start of an active save.
    pub fn is_active_first(&self) -> bool {
        self.state == state::FIRST_BLOCK
    }
}

/// One discovered save block - start frame + every chained continuation.
#[derive(Debug, Clone, Serialize)]
pub struct SaveBlock {
    /// First block index (1..=15).
    pub block: u8,
    /// File size as declared by the directory frame.
    pub file_size: u32,
    /// Product code (e.g. `BASCUS-94254...`).
    pub product_code: String,
    /// Block indices that make up this save (always at least one).
    pub block_chain: Vec<u8>,
}

/// Open a memory-card image and surface every active save block.
pub fn parse_card(buf: &[u8]) -> Result<Vec<SaveBlock>> {
    if buf.len() < CARD_SIZE {
        bail!(
            "card buffer too small: {} bytes (need >= {})",
            buf.len(),
            CARD_SIZE
        );
    }
    if buf[..2] != CARD_MAGIC {
        bail!(
            "missing MC magic at offset 0: {:02X?}",
            &buf[..2.min(buf.len())]
        );
    }
    let dir = walk_directory(buf)?;
    let mut saves = Vec::new();
    for entry in &dir {
        if !entry.is_active_first() {
            continue;
        }
        let mut chain = vec![entry.block];
        let mut cur = entry.next_block;
        let mut visited = 0;
        while cur != 0xFFFF && visited < DIR_FRAMES {
            visited += 1;
            // Validate the block index BEFORE recording it (block 1..=15 → frame
            // index 0..=14), so a malformed `next_block` aborts the walk without
            // leaving a bogus index in the reported chain.
            let frame_idx = cur as usize;
            if frame_idx == 0 || frame_idx > DIR_FRAMES {
                break;
            }
            let frame_off = DIR_FRAME_SIZE * frame_idx;
            if frame_off + DIR_FRAME_SIZE > buf.len() {
                break;
            }
            chain.push(cur as u8);
            let frame = &buf[frame_off..frame_off + DIR_FRAME_SIZE];
            cur = u16::from_le_bytes([frame[8], frame[9]]);
        }
        let product_code = entry.product_code.clone();
        saves.push(SaveBlock {
            block: entry.block,
            file_size: entry.file_size,
            product_code,
            block_chain: chain,
        });
    }
    Ok(saves)
}

/// Walk every directory frame in `buf` (frames 0..15) and return them as
/// typed entries. Includes free blocks too (so callers can audit
/// fragmentation).
pub fn walk_directory(buf: &[u8]) -> Result<Vec<DirEntry>> {
    if buf.len() < DIR_FRAME_SIZE * 16 {
        bail!("buffer too small for directory: {} bytes", buf.len());
    }
    let mut out = Vec::with_capacity(DIR_FRAMES);
    for i in 1..=DIR_FRAMES {
        let off = DIR_FRAME_SIZE * i;
        let frame = &buf[off..off + DIR_FRAME_SIZE];
        let state = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
        let file_size = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let next_block = u16::from_le_bytes([frame[8], frame[9]]);
        let product_code = bytes_to_ascii(&frame[10..0x1E]);
        let name = frame[0x1E..0x7F].to_vec();
        out.push(DirEntry {
            block: i as u8,
            state,
            file_size,
            next_block,
            product_code,
            name,
        });
    }
    Ok(out)
}

/// Read a save block's bytes from `buf` (a memory-card image).
pub fn read_block(buf: &[u8], block: u8) -> Option<&[u8]> {
    let i = block as usize;
    if i == 0 || i > DIR_FRAMES {
        return None;
    }
    let off = BLOCK_SIZE * i;
    let end = off + BLOCK_SIZE;
    if end > buf.len() {
        return None;
    }
    Some(&buf[off..end])
}

/// Encode one directory frame in place.
///
/// The single place this crate knows a frame's layout: state at `+0`, total
/// save size at `+4` (first frame of a chain only, hence `total_size:
/// Option`), next-block pointer at `+8` (`0xFFFF` ends the chain), 20-byte
/// product code at `+10`, and the XOR checksum of bytes `0x00..0x7E` at
/// `0x7F`. That XOR covers the **frame**; the block it describes carries a
/// second, independent checksum of its own at
/// [`RETAIL_BLOCK_CHECKSUM_OFFSET`], and both have to hold for retail to
/// load the save (see `docs/subsystems/save-screen.md`).
///
/// `total_size` is **block-aligned** - `blocks * BLOCK_SIZE`, the byte count
/// a real card records, never the payload length.
///
/// `frame` must be exactly [`DIR_FRAME_SIZE`] bytes. Shared by
/// [`write_block`] and [`crate::emu::CardView::claim_block`] so the two
/// cannot drift.
pub(crate) fn encode_dir_frame(
    frame: &mut [u8],
    block_state: u32,
    total_size: Option<u32>,
    next_block: u16,
    product_code: &str,
) {
    debug_assert_eq!(frame.len(), DIR_FRAME_SIZE);
    frame.fill(0);
    frame[..4].copy_from_slice(&block_state.to_le_bytes());
    if let Some(size) = total_size {
        frame[4..8].copy_from_slice(&size.to_le_bytes());
    }
    frame[8..10].copy_from_slice(&next_block.to_le_bytes());
    let src = product_code.as_bytes();
    let n = src.len().min(20);
    frame[10..10 + n].copy_from_slice(&src[..n]);
    frame[0x7F] = frame[..0x7F].iter().fold(0u8, |acc, &b| acc ^ b);
}

/// Directory frames (in block 0) that hold the broken-sector list - frames
/// `16..=35`, after the fifteen per-block frames.
const BROKEN_SECTOR_FRAMES: std::ops::RangeInclusive<usize> = 16..=35;

/// Frame 63, the last frame of block 0: the BIOS's write-test frame, which a
/// freshly formatted card carries as a copy of the `MC` header frame.
const WRITE_TEST_FRAME: usize = 63;

/// A freshly **formatted** 128 KiB memory-card image: the `MC` header frame,
/// fifteen free directory frames, an empty broken-sector list and the
/// write-test frame, every frame carrying its XOR checksum - the card a
/// console's memory-card manager produces, with no saves on it.
///
/// This is the card a host mounts when the player has none of their own: the
/// browser play page keeps one in the page's storage and mounts it in port 1,
/// the twin of the native window's save directory, so the retail Save / Load
/// screens always have a card to read and write.
pub fn formatted_card_image() -> Vec<u8> {
    fn stamp_checksum(frame: &mut [u8]) {
        frame[0x7F] = frame[..0x7F].iter().fold(0u8, |acc, &b| acc ^ b);
    }
    let mut buf = vec![0u8; CARD_SIZE];
    buf[..2].copy_from_slice(&CARD_MAGIC);
    stamp_checksum(&mut buf[..DIR_FRAME_SIZE]);
    for i in 1..=DIR_FRAMES {
        let off = DIR_FRAME_SIZE * i;
        encode_dir_frame(
            &mut buf[off..off + DIR_FRAME_SIZE],
            state::FREE,
            None,
            0xFFFF,
            "",
        );
    }
    for i in BROKEN_SECTOR_FRAMES {
        let off = DIR_FRAME_SIZE * i;
        let frame = &mut buf[off..off + DIR_FRAME_SIZE];
        frame[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        frame[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        stamp_checksum(frame);
    }
    let (head, rest) = buf.split_at_mut(DIR_FRAME_SIZE);
    let off = DIR_FRAME_SIZE * (WRITE_TEST_FRAME - 1);
    rest[off..off + DIR_FRAME_SIZE].copy_from_slice(head);
    buf
}

/// Write `save_data` into a free block chain on a PSX memory-card image.
///
/// Finds enough free blocks (state `0xA0`) starting from the lowest-indexed
/// available slot. Each block is `BLOCK_SIZE` (8 KB); the usable payload
/// per block is `BLOCK_SIZE - 2` (the first 2 bytes are the `SC` magic).
/// Multi-block chains are written with the `FIRST → MID* → LAST` state
/// encoding and `next_block` chain pointers; single-block saves use
/// `FIRST_BLOCK` with `next_block = 0xFFFF`.
///
/// Directory frames are rewritten with XOR checksums (XOR of bytes
/// `0x00..0x7E`, stored at `0x7F`), and the first frame records the save's
/// **block-aligned** size (`blocks * BLOCK_SIZE`), matching what a real card
/// declares. Returns the first block index written.
///
/// # Errors
///
/// Fails if the buffer is too small, no `MC` magic is present, or there
/// are not enough free blocks to hold `save_data`.
pub fn write_block(card_buf: &mut [u8], save_data: &[u8], product_code: &str) -> Result<u8> {
    if card_buf.len() < CARD_SIZE {
        bail!(
            "card buffer too small: {} bytes (need {})",
            card_buf.len(),
            CARD_SIZE
        );
    }
    if card_buf[..2] != CARD_MAGIC {
        bail!(
            "missing MC magic at offset 0: {:02X?}",
            &card_buf[..2.min(card_buf.len())]
        );
    }

    const DATA_PER_BLOCK: usize = BLOCK_SIZE - 2;
    let n_needed = if save_data.is_empty() {
        1
    } else {
        save_data.len().div_ceil(DATA_PER_BLOCK)
    };

    let dir = walk_directory(card_buf)?;
    let free: Vec<u8> = dir
        .iter()
        .filter(|e| e.state == state::FREE)
        .map(|e| e.block)
        .take(n_needed)
        .collect();

    if free.len() < n_needed {
        bail!(
            "not enough free blocks: need {n_needed}, found {} free",
            free.len()
        );
    }

    // The frame's size field is a byte count of the blocks the save occupies,
    // not of its payload: a real card always records a multiple of BLOCK_SIZE
    // (a one-block save reads 8192, never 8190). Card browsers derive the
    // block count from it, so an unaligned value misreports the save.
    let total_size = (n_needed * BLOCK_SIZE) as u32;

    for (idx, &blk) in free.iter().enumerate() {
        let blk_state = if idx == 0 {
            state::FIRST_BLOCK
        } else if idx + 1 == n_needed {
            state::LAST_BLOCK
        } else {
            state::MID_BLOCK
        };
        let next: u16 = if idx + 1 < n_needed {
            free[idx + 1] as u16
        } else {
            0xFFFF
        };

        // Rewrite directory frame
        let frame_off = DIR_FRAME_SIZE * blk as usize;
        encode_dir_frame(
            &mut card_buf[frame_off..frame_off + DIR_FRAME_SIZE],
            blk_state,
            (idx == 0).then_some(total_size),
            next,
            product_code,
        );

        // Write block: SC magic + payload chunk
        let chunk_start = idx * DATA_PER_BLOCK;
        let chunk_end = (chunk_start + DATA_PER_BLOCK).min(save_data.len());
        let block_off = BLOCK_SIZE * blk as usize;
        card_buf[block_off..block_off + 2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        if chunk_start < save_data.len() {
            let chunk = &save_data[chunk_start..chunk_end];
            card_buf[block_off + 2..block_off + 2 + chunk.len()].copy_from_slice(chunk);
        }
    }

    Ok(free[0])
}

fn bytes_to_ascii(b: &[u8]) -> String {
    b.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            if (0x20..=0x7E).contains(&c) {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}
