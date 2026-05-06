//! PSX memory-card layout walker.
//!
//! A PSX memory card holds 16 × 8 KB blocks (block 0 is reserved for the
//! directory; blocks 1..15 are user data). Each block is `BLOCK_SIZE`
//! bytes; saves longer than 8 KB chain across multiple blocks via the
//! directory frame's `next_block` field.
//!
//! For Legaia we only care about locating active save blocks; the
//! per-block payload format is documented separately (the block's
//! per-character record region maps to [`crate::CharacterRecord`]).
//!
//! ## Memory-card frame layout (block 0)
//!
//! ```text
//! +0x0000  u8[2]  = 'MC'    file magic
//! +0x0080  16 × 128 B       directory frames, one per block
//! +0x0880  20 × 128 B       broken-frame table
//! +0x1F80  128 B            test write frame
//! ```
//!
//! Each directory frame:
//! ```text
//! +0x00  u32 LE  block state (0x51 = first, 0x52 = mid, 0x53 = last)
//! +0x04  u32 LE  file size (bytes)
//! +0x08  u16 LE  next block (or 0xFFFF for terminal)
//! +0x0A  u8[20]  region/product code (e.g. `BASCUSXXXXXNAME`)
//! +0x1E  ...     game-specific name
//! +0x7F  u8      XOR checksum of bytes 0..0x7F
//! ```
//!
//! ## Block layout
//!
//! ```text
//! +0x0000  u8[2]   = 'SC'   save block magic
//! +0x0002  ...             game-specific payload
//! ```
//!
//! Legaia saves embed the runtime state at fixed offsets within each
//! block. The character record region begins at the offset documented
//! in [`docs/subsystems/battle.md`] — *the exact offset within the
//! save block hasn't been pinned yet*; this module surfaces the block
//! boundaries and lets callers slice as documentation evolves.

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
    /// File size in bytes the directory frame declares.
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

/// One discovered save block — start frame + every chained continuation.
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
            chain.push(cur as u8);
            // Read the next directory frame (block index 1..=15 → frame
            // index 0..=14).
            let frame_idx = cur as usize;
            if frame_idx == 0 || frame_idx > DIR_FRAMES {
                break;
            }
            let frame_off = DIR_FRAME_SIZE * frame_idx;
            if frame_off + DIR_FRAME_SIZE > buf.len() {
                break;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_card_with_one_save() -> Vec<u8> {
        let mut buf = vec![0u8; CARD_SIZE];
        buf[..2].copy_from_slice(&CARD_MAGIC);
        // Directory frame 1: state=FIRST_BLOCK, size=8192, next=0xFFFF.
        let f = DIR_FRAME_SIZE;
        buf[f..f + 4].copy_from_slice(&state::FIRST_BLOCK.to_le_bytes());
        buf[f + 4..f + 8].copy_from_slice(&8192u32.to_le_bytes());
        buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let pc = b"BASCUS-94254LEGAIA";
        buf[f + 10..f + 10 + pc.len()].copy_from_slice(pc);
        // Save block 1 starts with SC magic.
        let b = BLOCK_SIZE;
        buf[b..b + 2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        buf
    }

    #[test]
    fn detects_one_save_block() {
        let card = synth_card_with_one_save();
        let saves = parse_card(&card).unwrap();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].block, 1);
        assert_eq!(saves[0].file_size, 8192);
        assert!(saves[0].product_code.starts_with("BASCUS-94254"));
        assert_eq!(saves[0].block_chain, vec![1]);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut buf = vec![0u8; CARD_SIZE];
        buf[0] = b'X';
        assert!(parse_card(&buf).is_err());
    }

    #[test]
    fn read_block_returns_8kb_slice() {
        let card = synth_card_with_one_save();
        let block = read_block(&card, 1).unwrap();
        assert_eq!(block.len(), BLOCK_SIZE);
        assert_eq!(&block[..2], &SAVE_BLOCK_MAGIC);
    }

    #[test]
    fn read_block_rejects_out_of_range() {
        let card = vec![0u8; CARD_SIZE];
        assert!(read_block(&card, 0).is_none());
        assert!(read_block(&card, 16).is_none());
    }

    #[test]
    fn walk_directory_returns_15_entries() {
        let card = synth_card_with_one_save();
        let dir = walk_directory(&card).unwrap();
        assert_eq!(dir.len(), DIR_FRAMES);
        assert_eq!(dir[0].state, state::FIRST_BLOCK);
        assert_eq!(dir[0].block, 1);
    }
}
