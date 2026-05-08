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
/// `0x00..0x7E`, stored at `0x7F`). Returns the first block index written.
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

    let total_size = save_data.len() as u32;
    let mut pc = [0u8; 20];
    {
        let src = product_code.as_bytes();
        let n = src.len().min(20);
        pc[..n].copy_from_slice(&src[..n]);
    }

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
        let frame = &mut card_buf[frame_off..frame_off + DIR_FRAME_SIZE];
        frame.fill(0);
        frame[..4].copy_from_slice(&blk_state.to_le_bytes());
        if idx == 0 {
            frame[4..8].copy_from_slice(&total_size.to_le_bytes());
        }
        frame[8..10].copy_from_slice(&next.to_le_bytes());
        frame[10..30].copy_from_slice(&pc);
        let checksum = frame[..0x7F].iter().fold(0u8, |acc, &b| acc ^ b);
        frame[0x7F] = checksum;

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

/// Byte offset from the start of an SC save block to where game data begins.
///
/// Verified by locating the "Vahn", "Noa", "Gala" names in an actual Legaia
/// retail mednafen `.mcr` save at `~/.mednafen/sav/Legend of Legaia (USA).*.0.mcr`.
/// Block layout: SC magic at +0, icon palette at +0x60, icon pixels at +0x80;
/// game-data region begins at +0x200.
pub const RETAIL_GAME_DATA_OFFSET: usize = 0x200;

/// Byte offset from the game data start to the first character record.
///
/// The display / global header occupies `0x000..0x66E`; character records begin
/// at `0x66F`. Known header fields: location name at `+0x000`, primary character
/// display name at `+0x054`, most-recently-visited CDNAME label at `+0x208`,
/// previous scene CDNAME label at `+0x218`. Verified against a real Legaia save.
pub const RETAIL_CHAR_RECORD_HEADER_SIZE: usize = 0x66F;

/// Stride between character records in the retail save format (matches
/// `CHARACTER_RECORD_SIZE` = 0x414 used in `crates/save/src/character.rs`).
///
/// Confirmed by observing Vahn at `game+0x66F`, Noa at `game+0xA83`,
/// Gala at `game+0xE97`, Terra at `game+0x12AB` — all at 0x414-byte intervals.
pub const RETAIL_CHAR_RECORD_STRIDE: usize = 0x414;

/// Extract raw character record bytes from a retail SC save block.
///
/// `sc_block` is the full 8192-byte save block starting with the `SC` magic.
/// Returns at most `max_records` records. Stops early at the first all-zero
/// record (unused / empty slot). Returns `None` if the block is too small to
/// hold even the header region.
///
/// Each returned `Vec<u8>` is exactly `RETAIL_CHAR_RECORD_STRIDE` (0x414) bytes
/// and can be parsed by `legaia_save::CharacterRecord::parse`.
///
/// # Example
///
/// ```
/// # use legaia_save::card::{read_retail_char_records, RETAIL_GAME_DATA_OFFSET};
/// let sc_block = vec![0u8; 8192];
/// // An all-zero block yields zero records (first slot is empty).
/// assert!(read_retail_char_records(&sc_block, 4).map_or(true, |v| v.is_empty()));
/// ```
pub fn read_retail_char_records(sc_block: &[u8], max_records: usize) -> Option<Vec<Vec<u8>>> {
    let game_data = sc_block.get(RETAIL_GAME_DATA_OFFSET..)?;
    let records_start = game_data.get(RETAIL_CHAR_RECORD_HEADER_SIZE..)?;
    let mut out = Vec::new();
    for i in 0..max_records {
        let offset = i * RETAIL_CHAR_RECORD_STRIDE;
        let record = records_start.get(offset..offset + RETAIL_CHAR_RECORD_STRIDE)?;
        if record.iter().all(|&b| b == 0) {
            break; // stop at first empty slot
        }
        out.push(record.to_vec());
    }
    Some(out)
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

    fn free_card() -> Vec<u8> {
        let mut buf = vec![0u8; CARD_SIZE];
        buf[..2].copy_from_slice(&CARD_MAGIC);
        // Mark all blocks free.
        for i in 1..=DIR_FRAMES {
            let frame_off = DIR_FRAME_SIZE * i;
            buf[frame_off..frame_off + 4].copy_from_slice(&state::FREE.to_le_bytes());
            let checksum = buf[frame_off..frame_off + 0x7F]
                .iter()
                .fold(0u8, |acc, &b| acc ^ b);
            buf[frame_off + 0x7F] = checksum;
        }
        buf
    }

    #[test]
    fn write_block_single_block() {
        let mut card = free_card();
        let payload = b"Hello Legaia save!";
        let block = write_block(&mut card, payload, "BASCUS-94254TEST").unwrap();
        assert_eq!(block, 1, "first free block should be 1");

        // Directory frame 1: state = FIRST_BLOCK, next = 0xFFFF.
        let frame_off = DIR_FRAME_SIZE;
        let blk_state = u32::from_le_bytes(card[frame_off..frame_off + 4].try_into().unwrap());
        assert_eq!(blk_state, state::FIRST_BLOCK);
        let next = u16::from_le_bytes(card[frame_off + 8..frame_off + 10].try_into().unwrap());
        assert_eq!(next, 0xFFFF);

        // Block 1 data: SC magic + payload.
        let blk_off = BLOCK_SIZE;
        assert_eq!(&card[blk_off..blk_off + 2], &SAVE_BLOCK_MAGIC);
        assert_eq!(&card[blk_off + 2..blk_off + 2 + payload.len()], payload);
    }

    #[test]
    fn write_block_checksum_is_correct() {
        let mut card = free_card();
        write_block(&mut card, b"checksum test payload", "BASCUS-94254TEST").unwrap();
        let frame_off = DIR_FRAME_SIZE;
        let expected = card[frame_off..frame_off + 0x7F]
            .iter()
            .fold(0u8, |acc, &b| acc ^ b);
        assert_eq!(card[frame_off + 0x7F], expected, "XOR checksum mismatch");
    }

    #[test]
    fn write_block_product_code_stored() {
        let mut card = free_card();
        write_block(&mut card, b"data", "BASCUS-94254LEGAIA").unwrap();
        let frame_off = DIR_FRAME_SIZE;
        let pc_bytes = &card[frame_off + 10..frame_off + 30];
        assert!(pc_bytes.starts_with(b"BASCUS-94254LEGAIA"));
    }

    #[test]
    fn write_block_rejects_full_card() {
        let mut card = synth_card_with_one_save();
        // Fill remaining blocks with state FIRST_BLOCK so none are free.
        for i in 2..=DIR_FRAMES {
            let frame_off = DIR_FRAME_SIZE * i;
            card[frame_off..frame_off + 4].copy_from_slice(&state::FIRST_BLOCK.to_le_bytes());
        }
        assert!(write_block(&mut card, b"data", "BASCUS-94254TEST").is_err());
    }
}
