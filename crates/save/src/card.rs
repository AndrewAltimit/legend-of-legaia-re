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
//! +0x1FFC  u32 LE          additive checksum of the words before it
//! ```
//!
//! The trailing word is Legaia's own, not part of the PSX block format:
//! retail stamps it when it composes a save and compares it when it loads
//! one, so an in-place edit that leaves it stale yields a block the game
//! refuses. [`sc_block_checksum`] is the kernel and every writer here
//! restamps.
//!
//! Legaia saves embed the runtime state at fixed offsets within each
//! block. The character record region begins at the offset documented
//! in [`docs/subsystems/battle.md`] - *the exact offset within the
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

/// Offset of the Shift-JIS save title inside an SC block.
pub const RETAIL_TITLE_OFFSET: usize = 0x04;

/// Offsets **within the title** of the two save-number digits' low bytes.
/// The digits are full-width, so only the low byte of each 2-byte character
/// varies.
///
/// PORT: FUN_801e1934 (`0x801E19E4..0x801E19E8`)
pub const RETAIL_TITLE_DIGIT_OFFSETS: [usize; 2] = [0x23, 0x25];

/// Low byte of the Shift-JIS full-width digit zero. A title digit is
/// `SAVE_TITLE_DIGIT_BASE + digit`, which is what makes the BIOS render it
/// as a full-width numeral.
pub const SAVE_TITLE_DIGIT_BASE: u8 = 0x4F;

/// Offset of the 16-entry icon palette (32 bytes, u16 LE BGR555).
pub const RETAIL_ICON_CLUT_OFFSET: usize = 0x60;

/// Offsets of the three 128-byte icon frame slots.
///
/// Retail writes the **same** tile into all three even though the header
/// declares a single frame - three `StoreImage` calls on one rect.
///
/// PORT: FUN_801e1934 (`0x801E1B30..0x801E1B68`)
pub const RETAIL_ICON_FRAME_OFFSETS: [usize; 3] = [0x80, 0x100, 0x180];

/// Bytes in one icon frame (16x16 @ 4bpp).
pub const RETAIL_ICON_FRAME_BYTES: usize = 128;

/// Bytes in the icon palette.
pub const RETAIL_ICON_CLUT_BYTES: usize = 32;

/// Filename prefix retail gives a Legaia save on a USA disc. The **slot
/// number is the suffix**: save number `n` is written as file
/// `BASCUS-94254PRO-<n-1>`, zero-padded to two digits.
///
/// The separator is a hyphen. Verified two ways: the literal in the menu
/// overlay's data segment, and the directory frames of real cards.
pub const LEGAIA_SAVE_FILENAME_PREFIX: &str = "BASCUS-94254PRO-";

/// The directory-frame filename for save slot `slot` (0-based).
pub fn legaia_save_filename(slot: u32) -> String {
    format!("{LEGAIA_SAVE_FILENAME_PREFIX}{:02}", slot.min(99))
}

/// The two title digits for save slot `slot`.
///
/// The slot is displayed **1-based**, so slot `0` writes `"01"`.
///
/// PORT: FUN_801e1934 (`0x801E1974..0x801E19EC`)
pub fn save_title_digits(slot: u32) -> [u8; 2] {
    let n = slot.wrapping_add(1);
    [
        SAVE_TITLE_DIGIT_BASE.wrapping_add((n / 10) as u8),
        SAVE_TITLE_DIGIT_BASE.wrapping_add((n % 10) as u8),
    ]
}

/// The per-slot portrait a save block carries as its memory-card icon.
///
/// Source it from the disc's portrait sheet
/// (`legaia_asset::save_icon::SaveIconSheet::tile_block_pixels` +
/// `tile_clut_bytes` for the same slot); this crate stays disc-free, so the
/// caller supplies the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailBlockIcon {
    /// 16-entry palette, little-endian BGR555.
    pub clut: [u8; RETAIL_ICON_CLUT_BYTES],
    /// 16x16 @ 4bpp pixels in contiguous save-block layout.
    pub pixels: [u8; RETAIL_ICON_FRAME_BYTES],
}

/// Stamp the block-identity fields retail's composer writes and a payload
/// writer cannot derive: the four-byte header, the save-number digits in the
/// title, and the slot's icon.
///
/// `icon = None` leaves the icon region as found - correct when re-saving
/// over a block that already carries one, wrong for a previously-free block,
/// which is why the caller should supply it whenever it has the disc.
///
/// The title digits are only patched when the block already carries a title
/// (a non-zero byte in the title region): stamping two digits into an
/// otherwise-empty title would produce a name that is two numerals and
/// nothing else.
///
/// PORT: FUN_801e1934
pub fn write_retail_block_identity(
    sc_block: &mut [u8],
    slot: u32,
    icon: Option<&RetailBlockIcon>,
) -> Result<()> {
    if sc_block.len() < RETAIL_GAME_DATA_OFFSET {
        bail!(
            "SC block too small for its header: {} bytes, need {RETAIL_GAME_DATA_OFFSET}",
            sc_block.len()
        );
    }
    sc_block[..SAVE_BLOCK_HEADER.len()].copy_from_slice(&SAVE_BLOCK_HEADER);

    let title = &mut sc_block[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET];
    if title.iter().any(|&b| b != 0) {
        let digits = save_title_digits(slot);
        for (off, d) in RETAIL_TITLE_DIGIT_OFFSETS.iter().zip(digits.iter()) {
            if let Some(slot_byte) = title.get_mut(*off) {
                *slot_byte = *d;
            }
        }
    }

    if let Some(icon) = icon {
        sc_block[RETAIL_ICON_CLUT_OFFSET..RETAIL_ICON_CLUT_OFFSET + RETAIL_ICON_CLUT_BYTES]
            .copy_from_slice(&icon.clut);
        for off in RETAIL_ICON_FRAME_OFFSETS {
            sc_block[off..off + RETAIL_ICON_FRAME_BYTES].copy_from_slice(&icon.pixels);
        }
    }
    Ok(())
}

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

/// Byte offset from the start of an SC save block to where game data begins.
///
/// Verified by locating the "Vahn", "Noa", "Gala" names in an actual Legaia
/// retail mednafen `.mcr` save at `~/.mednafen/sav/Legend of Legaia (USA).*.0.mcr`.
/// Block layout: SC magic at +0, icon palette at +0x60, icon pixels at +0x80;
/// game-data region begins at +0x200.
pub const RETAIL_GAME_DATA_OFFSET: usize = 0x200;

/// Byte offset from the game data start to the first character record's base.
///
/// The SC block is a verbatim dump of the resident save-state region (the
/// linear map `block_offset = RETAIL_GAME_DATA_OFFSET + (ram_addr -
/// SAVE_GAME_DATA_RAM_BASE)`, so `game_data` mirrors live RAM from
/// `0x80084340`). Character record `n` lives at live RAM `0x80084708 +
/// n*0x414`, i.e. `game_data + 0x3C8` - confirmed across six in-game RAM
/// captures (mid-game stats at `record+0x104`/`+0x11C` read back the
/// expected per-character HP/MP for all four roster slots).
///
/// The display / global header therefore occupies `game_data 0x000..0x3C7`:
/// location name at `+0x000`, primary character display name at `+0x054`,
/// most-recently-visited CDNAME label at `+0x208`, previous scene CDNAME at
/// `+0x218`, party gold at `+0x25C`.
///
/// NB: a record's **display name** is at internal offset `+0x2A7`
/// ([`crate::character::NAME_OFFSET`]), so the visible "Vahn"/"Noa"/"Gala"/
/// "Terra" strings land at `game_data + 0x66F + n*0x414` (SC `+0x86F` for
/// slot 0). Anchoring the record region at the *name* (`0x66F`) rather than
/// the true base (`0x3C8`) was an earlier off-by-`0x2A7` that made
/// [`crate::character::CharacterRecord`]'s stat offsets read into the wrong
/// fields on a populated save.
pub const RETAIL_CHAR_RECORD_HEADER_SIZE: usize = 0x3C8;

/// Stride between character records in the retail save format (matches
/// `CHARACTER_RECORD_SIZE` = 0x414 used in `crates/save/src/character.rs`).
///
/// Confirmed by the four roster slots at live RAM `0x80084708 + n*0x414`
/// (Vahn / Noa / Gala / Terra) - the names land at `+0x2A7` within each,
/// i.e. `game+0x66F / 0xA83 / 0xE97 / 0x12AB`.
pub const RETAIL_CHAR_RECORD_STRIDE: usize = 0x414;

/// Maximum number of fully non-overlapping character record slots in the
/// retail SC layout.
///
/// The four-slot record array is immediately followed by the global game
/// data, so slot 3 (Terra)'s `0x414`-byte footprint runs from `game+0x1004`
/// to `game+0x1418` and its **tail** (from internal offset `+0x2BC`,
/// i.e. `game+0x12C0`) overlaps the 512-byte story-flag bitmap at
/// [`RETAIL_STORY_FLAGS_OFFSET`] and the inventory at
/// [`RETAIL_INVENTORY_OFFSET`]. Terra's meaningful fields all sit *before*
/// that boundary - her name (`+0x2A7`), live HP/MP (`+0x104`) and
/// RecordStats (`+0x11C`) are in exclusive space - and she is the New Game
/// template's fourth roster entry (HP 400) but never a savable battle-party
/// member, so the tail aliasing is benign: there is no special-case code
/// path; the global region simply begins partway through the fourth slot.
/// Callers that want a strictly non-overlapping record walk cap at 3.
pub const RETAIL_MAX_CHAR_RECORDS: usize = 3;

/// Byte offset from the SC block start to the story-flag bitmap.
///
/// The story-flag bitmap occupies 512 bytes (`0x200`) and mirrors the
/// live-RAM region `0x80085600..0x80085800`.  Derived via the linear
/// address formula `block_offset = RETAIL_GAME_DATA_OFFSET +
/// (ram_addr - SAVE_GAME_DATA_RAM_BASE)`:
/// `0x200 + (0x80085600 - 0x80084340) = 0x14C0`.
///
/// Confirmed by byte-match against mednafen save states cross-referenced
/// against retail Drake and Sebucus MCR save blocks.
pub const RETAIL_STORY_FLAGS_OFFSET: usize = 0x14C0;

/// Size of the story-flag bitmap in the save block (matches the live-RAM
/// window `0x80085600..0x80085800`).
pub const RETAIL_STORY_FLAGS_SIZE: usize = 0x200;

/// Byte offset from the SC block start to the global inventory array.
///
/// The inventory holds 72 slots of `(item_id: u8, count: u8)` pairs
/// (144 bytes total, `0x90`) and mirrors the live-RAM region
/// `0x80085958..0x800859E8`.  Derived via the linear address formula:
/// `0x200 + (0x80085958 - 0x80084340) = 0x1818`.
///
/// Confirmed by 100% byte-match against mednafen save states cross-referenced
/// against retail Drake and Sebucus MCR save blocks.
pub const RETAIL_INVENTORY_OFFSET: usize = 0x1818;

/// Number of inventory slots in the retail save (and in live RAM).
pub const RETAIL_INVENTORY_SLOTS: usize = 72;

/// Size in bytes of the retail inventory array (`RETAIL_INVENTORY_SLOTS × 2`).
pub const RETAIL_INVENTORY_SIZE: usize = RETAIL_INVENTORY_SLOTS * 2; // 0x90

/// Byte offset from the SC block start to the party **gold** (i32 LE).
///
/// Mirrors live RAM `0x8008459C` (`_DAT_8008459C`, the word the battle-victory
/// reward writer `FUN_8004F0E8` credits - see `docs/subsystems/boot.md`) via
/// the linear map: `0x200 + (0x8008459C - 0x80084340) = 0x45C`
/// (= `game_data + 0x25C`, the offset `docs/subsystems/save-screen.md` pins).
pub const RETAIL_GOLD_OFFSET: usize = 0x45C;

/// Byte offset from the SC block start to the casino **coin bank** (u32 LE).
///
/// Mirrors live RAM `0x800845A4` - the global the slot-machine overlay reads
/// at session entry and assigns on cash-out (`docs/subsystems/minigame-slot-machine.md`),
/// and the address the third-party cheat database classifies as `coins_u32`
/// (`crates/cheats/src/classify.rs`). Linear map:
/// `0x200 + (0x800845A4 - 0x80084340) = 0x464`.
///
/// An in-place write here is **not** the end of the edit: the block carries
/// its own additive checksum at [`RETAIL_BLOCK_CHECKSUM_OFFSET`] and every
/// writer in this module restamps it - see [`sc_block_checksum`].
pub const RETAIL_COINS_OFFSET: usize = 0x464;

// ---------------------------------------------------------------------
// Save-block checksum
// ---------------------------------------------------------------------

/// Number of little-endian u32 words in one SC save block.
pub const SC_BLOCK_WORDS: usize = BLOCK_SIZE / 4;

/// Word index the additive block checksum is stored at - the block's last
/// word. The sum covers every word *before* it.
pub const SC_BLOCK_CHECKSUM_WORD: usize = SC_BLOCK_WORDS - 1;

/// Byte offset of the additive block checksum inside an SC save block.
pub const RETAIL_BLOCK_CHECKSUM_OFFSET: usize = SC_BLOCK_CHECKSUM_WORD * 4;

/// The same sum over a block a caller already holds as words.
///
/// A convenience face, not a second model: `checksum_covers_every_word_but_the
/// _last` asserts it agrees with [`sc_block_checksum`] byte for byte. Retail's
/// `a0` is a byte pointer, so the byte form is the one that carries the
/// `PORT:` and the one that runs.
///
/// REF: FUN_801E38D8
pub fn sc_block_checksum_words(words: &[u32]) -> u32 {
    words
        .iter()
        .take(SC_BLOCK_CHECKSUM_WORD)
        .fold(0u32, |sum, &w| sum.wrapping_add(w))
}

/// The additive save-block checksum of an SC block. `None` when the block is
/// too short to hold the checksum word.
///
/// Retail walks `0x7FF` little-endian u32 words from the block base with a
/// wrapping accumulator and returns the total, stopping one word short of
/// the block's final word:
///
/// ```text
/// 801e38d8  clear a1            ; sum = 0
/// 801e38dc  move  v1,a1         ; i   = 0
/// 801e38e0  lw    v0,0x0(a0)    ; w   = block[i]
/// 801e38e4  addiu v1,v1,0x1     ; i  += 1
/// 801e38e8  addu  a1,a1,v0      ; sum = sum +. w   (wrapping)
/// 801e38ec  slti  v0,v1,0x7ff   ; loop while i < 0x7ff
/// 801e38f0  bne   v0,zero,...
/// 801e38f4  _addiu a0,a0,0x4    ; block ptr += 4  (delay slot, always)
/// 801e38f8  jr    ra
/// 801e38fc  _move v0,a1         ; return sum
/// ```
///
/// The base it sums from is the block's **first byte** - the `SC` magic -
/// not the game-data region at [`RETAIL_GAME_DATA_OFFSET`]: the composer
/// `FUN_801E1934` zero-fills a whole `0x2000` staging block, copies
/// `0x1A18` bytes of live state over its front, sums that buffer and stores
/// the result at `+0x1FFC`, and the writer hands the same `0x2000` bytes to
/// the BIOS. The load direction reads the block back whole and compares.
///
/// PORT: FUN_801E38D8 (`ghidra/scripts/funcs/overlay_menu_801e38d8.txt`)
pub fn sc_block_checksum(sc_block: &[u8]) -> Option<u32> {
    if sc_block.len() < BLOCK_SIZE {
        return None;
    }
    Some(
        sc_block[..RETAIL_BLOCK_CHECKSUM_OFFSET]
            .chunks_exact(4)
            .fold(0u32, |sum, c| {
                sum.wrapping_add(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            }),
    )
}

/// Whether a block's stored checksum word matches a fresh
/// [`sc_block_checksum`] - the test retail's load path applies before it
/// accepts a block.
///
/// The compare lives in state `5` of the load/save driver `FUN_801DD35C`
/// (`0x801df880`: `jal FUN_801E38D8` on the read buffer, then
/// `lw v1,0x1ffc(s1); beq v1,v0`). A match advances to sub-state `0x16`
/// and the block is copied into live RAM; a mismatch latches
/// `DAT_801EF140 = 2` and routes to sub-state `0x13`, the "Damaged data."
/// arm. A short block can never be valid.
///
/// REF: FUN_801DD35C (`ghidra/scripts/funcs/overlay_menu_801dd35c.txt`)
pub fn sc_block_checksum_valid(sc_block: &[u8]) -> bool {
    let Some(sum) = sc_block_checksum(sc_block) else {
        return false;
    };
    let stored = &sc_block[RETAIL_BLOCK_CHECKSUM_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET + 4];
    u32::from_le_bytes([stored[0], stored[1], stored[2], stored[3]]) == sum
}

/// Recompute and store a block's checksum word, returning the value stored.
///
/// This is the last thing retail's save composer does (`FUN_801E1934` at
/// `0x801e1be0`: sum the staging block, `sw v0,0x1ffc(v1)`), so it is the
/// last thing an in-place edit of a real block has to do too - otherwise
/// the retail loader captions the save "Damaged data." and refuses it.
/// Every `write_retail_*` writer in this module calls it.
///
/// `None` (and no write) when the block is shorter than [`BLOCK_SIZE`] -
/// a caller editing a bare game-data window rather than a whole card block
/// has no checksum word to restamp.
///
/// REF: FUN_801E1934 (`ghidra/scripts/funcs/overlay_menu_801e1934.txt`)
pub fn restamp_sc_block_checksum(sc_block: &mut [u8]) -> Option<u32> {
    let sum = sc_block_checksum(sc_block)?;
    sc_block[RETAIL_BLOCK_CHECKSUM_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET + 4]
        .copy_from_slice(&sum.to_le_bytes());
    Some(sum)
}

/// Read the party gold (i32 LE at [`RETAIL_GOLD_OFFSET`]) from a retail SC
/// save block. `None` if the block is too small.
pub fn read_retail_gold(sc_block: &[u8]) -> Option<i32> {
    let b = sc_block.get(RETAIL_GOLD_OFFSET..RETAIL_GOLD_OFFSET + 4)?;
    Some(i32::from_le_bytes(b.try_into().unwrap()))
}

/// Write the party gold in place, restamping the block checksum
/// ([`restamp_sc_block_checksum`]). `Err` if the block is too small.
pub fn write_retail_gold(sc_block: &mut [u8], gold: i32) -> Result<()> {
    let end = RETAIL_GOLD_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for retail gold field (need >= {end})");
    }
    sc_block[RETAIL_GOLD_OFFSET..end].copy_from_slice(&gold.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Read the casino coin bank (u32 LE at [`RETAIL_COINS_OFFSET`]) from a
/// retail SC save block. `None` if the block is too small.
pub fn read_retail_coins(sc_block: &[u8]) -> Option<u32> {
    let b = sc_block.get(RETAIL_COINS_OFFSET..RETAIL_COINS_OFFSET + 4)?;
    Some(u32::from_le_bytes(b.try_into().unwrap()))
}

/// Write the casino coin bank in place - the targeted patch the site's
/// minigames use to bank won coins into a player's own retail save.
///
/// Two fields change: the coin slot and the block checksum word
/// ([`restamp_sc_block_checksum`]), which retail's loader compares before it
/// will accept the block. `Err` if the block is too small.
pub fn write_retail_coins(sc_block: &mut [u8], coins: u32) -> Result<()> {
    let end = RETAIL_COINS_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for retail coin field (need >= {end})");
    }
    sc_block[RETAIL_COINS_OFFSET..end].copy_from_slice(&coins.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Byte offset from the SC block start to the most-recently-visited CDNAME
/// scene label (NUL-terminated ASCII) - `game_data + 0x208`, the field the
/// save-select screen reads for its location line's scene join.
pub const RETAIL_SCENE_LABEL_OFFSET: usize = 0x408;

/// Byte offset from the SC block start to the location display name
/// (NUL-terminated ASCII at `game_data + 0x000`).
pub const RETAIL_LOCATION_NAME_OFFSET: usize = RETAIL_GAME_DATA_OFFSET;

/// RAM base address for the save game data block (`game_data[0]` in the SC block).
///
/// The save block's `game_data` region (starting at `RETAIL_GAME_DATA_OFFSET`)
/// is a structured dump whose global fields (gold, story flags, inventory, …)
/// map linearly to live RAM via:
///
/// ```text
/// sc_block_offset = RETAIL_GAME_DATA_OFFSET + (ram_addr - SAVE_GAME_DATA_RAM_BASE)
/// ```
///
/// Derived by anchoring the confirmed inventory address:
/// `0x80085958 - game_data_offset(0x1618) = 0x80084340`.
/// Cross-validated against party gold (`0x8008459C` → `game_data+0x025C`),
/// story-flag bitmap (`0x80085600` → `game_data+0x12C0`), and the active
/// scene slot (`0x80084540` → `game_data+0x0200`).
pub const SAVE_GAME_DATA_RAM_BASE: u32 = 0x80084340;

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
        // Use checked arithmetic: `max_records` is caller-supplied, so a huge
        // value must stop the walk (the `?` short-circuit), never overflow the
        // `offset` / `offset + STRIDE` computation into a slice-index panic.
        let offset = i.checked_mul(RETAIL_CHAR_RECORD_STRIDE)?;
        let end = offset.checked_add(RETAIL_CHAR_RECORD_STRIDE)?;
        let record = records_start.get(offset..end)?;
        if record.iter().all(|&b| b == 0) {
            break; // stop at first empty slot
        }
        out.push(record.to_vec());
    }
    Some(out)
}

/// Extract the 512-byte story-flag bitmap from a retail SC save block.
///
/// Returns a reference to the 512-byte slice at `RETAIL_STORY_FLAGS_OFFSET`, or
/// `None` if the block is too small.  Each bit in the bitmap corresponds to one
/// game-progress flag (visited towns, event triggers, Door of Wind flags, etc.).
///
/// The slice mirrors the live-RAM region `0x80085600..0x80085800`.
pub fn read_retail_story_flags(sc_block: &[u8]) -> Option<&[u8]> {
    sc_block.get(RETAIL_STORY_FLAGS_OFFSET..RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE)
}

/// Extract the 72-slot inventory from a retail SC save block.
///
/// Returns the 144-byte raw inventory array at `RETAIL_INVENTORY_OFFSET`, or
/// `None` if the block is too small.  Each slot is a 2-byte `(item_id: u8,
/// count: u8)` pair in little-endian order.  An `item_id` of `0` with
/// `count` of `0` denotes an empty slot.
///
/// The slice mirrors the live-RAM region `0x80085958..0x800859E8`.
pub fn read_retail_inventory(sc_block: &[u8]) -> Option<&[u8]> {
    sc_block.get(RETAIL_INVENTORY_OFFSET..RETAIL_INVENTORY_OFFSET + RETAIL_INVENTORY_SIZE)
}

/// Write character records into a retail SC save block in place.
///
/// `records` are written sequentially starting at the retail record region
/// (`RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE`). Each record
/// occupies exactly `RETAIL_CHAR_RECORD_STRIDE` (0x414) bytes; shorter slices
/// are zero-padded, longer ones are truncated.
///
/// Returns the number of slots written. Returns `Err` if the SC block is too
/// small to hold even one record. The function does **not** zero trailing
/// slots: the retail SC layout places the story-flag bitmap (`+0x14C0`) and
/// inventory (`+0x1818`) regions in addresses that overlap what would
/// otherwise be the 4th and 5th record slots, so blindly zero-padding
/// trailing slots would clobber later writes. Callers that want a clean
/// block should zero-fill the buffer before writing.
pub fn write_retail_char_records(sc_block: &mut [u8], records: &[Vec<u8>]) -> Result<usize> {
    let records_start = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE;
    if sc_block.len() < records_start + RETAIL_CHAR_RECORD_STRIDE {
        bail!(
            "sc_block too small for retail char record region (need >= {}, got {})",
            records_start + RETAIL_CHAR_RECORD_STRIDE,
            sc_block.len()
        );
    }
    let cap = (sc_block.len() - records_start) / RETAIL_CHAR_RECORD_STRIDE;
    let n = records.len().min(cap);
    for (i, rec) in records.iter().take(n).enumerate() {
        let off = records_start + i * RETAIL_CHAR_RECORD_STRIDE;
        let dst = &mut sc_block[off..off + RETAIL_CHAR_RECORD_STRIDE];
        dst.fill(0);
        let take = rec.len().min(RETAIL_CHAR_RECORD_STRIDE);
        dst[..take].copy_from_slice(&rec[..take]);
    }
    restamp_sc_block_checksum(sc_block);
    Ok(n)
}

/// Write the 512-byte story-flag bitmap into a retail SC save block in place.
///
/// `bits` shorter than [`RETAIL_STORY_FLAGS_SIZE`] is zero-padded on the right;
/// longer slices are truncated. Returns the number of bytes written.
/// Returns `Err` if the SC block is too small to hold the bitmap region.
pub fn write_retail_story_flags(sc_block: &mut [u8], bits: &[u8]) -> Result<usize> {
    let end = RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE;
    if sc_block.len() < end {
        bail!(
            "sc_block too small for retail story-flag region (need >= {}, got {})",
            end,
            sc_block.len()
        );
    }
    let dst = &mut sc_block[RETAIL_STORY_FLAGS_OFFSET..end];
    dst.fill(0);
    let take = bits.len().min(RETAIL_STORY_FLAGS_SIZE);
    dst[..take].copy_from_slice(&bits[..take]);
    restamp_sc_block_checksum(sc_block);
    Ok(take)
}

/// Write the 72-slot inventory into a retail SC save block in place.
///
/// `pairs` is a slice of `(item_id, count)` pairs in destination-slot order.
/// Up to [`RETAIL_INVENTORY_SLOTS`] pairs are written; any extras are dropped.
/// Trailing slots past `pairs.len()` are zeroed. Returns the number of slots
/// written. Returns `Err` if the SC block is too small to hold the inventory
/// region.
pub fn write_retail_inventory(sc_block: &mut [u8], pairs: &[(u8, u8)]) -> Result<usize> {
    let end = RETAIL_INVENTORY_OFFSET + RETAIL_INVENTORY_SIZE;
    if sc_block.len() < end {
        bail!(
            "sc_block too small for retail inventory region (need >= {}, got {})",
            end,
            sc_block.len()
        );
    }
    let dst = &mut sc_block[RETAIL_INVENTORY_OFFSET..end];
    dst.fill(0);
    let n = pairs.len().min(RETAIL_INVENTORY_SLOTS);
    for (i, &(id, count)) in pairs.iter().take(n).enumerate() {
        dst[i * 2] = id;
        dst[i * 2 + 1] = count;
    }
    restamp_sc_block_checksum(sc_block);
    Ok(n)
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
    fn write_block_records_block_aligned_size() {
        let mut card = free_card();
        // An 18-byte payload still occupies a whole 8 KiB block, and that is
        // what the frame must declare - not 18, and not BLOCK_SIZE - 2.
        write_block(&mut card, b"Hello Legaia save!", "BASCUS-94254TEST").unwrap();
        let f = DIR_FRAME_SIZE;
        let size = u32::from_le_bytes(card[f + 4..f + 8].try_into().unwrap());
        assert_eq!(size, BLOCK_SIZE as u32);
        assert_eq!(size % BLOCK_SIZE as u32, 0, "size must be block-aligned");
        // And it reads back through the directory walker.
        assert_eq!(parse_card(&card).unwrap()[0].file_size, BLOCK_SIZE as u32);
    }

    #[test]
    fn write_block_size_covers_every_block_of_a_chain() {
        let mut card = free_card();
        // Two blocks' worth of payload: one byte past what a single block
        // holds, so the chain spans two and the frame must say so.
        let payload = vec![0xABu8; BLOCK_SIZE - 1];
        write_block(&mut card, &payload, "BASCUS-94254TEST").unwrap();

        let saves = parse_card(&card).unwrap();
        assert_eq!(saves[0].block_chain.len(), 2);
        assert_eq!(
            saves[0].file_size,
            (2 * BLOCK_SIZE) as u32,
            "a two-block chain declares two blocks of bytes"
        );
        // The size lives on the first frame only; continuations leave it 0.
        let f2 = DIR_FRAME_SIZE * saves[0].block_chain[1] as usize;
        assert_eq!(
            u32::from_le_bytes(card[f2 + 4..f2 + 8].try_into().unwrap()),
            0
        );
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

    fn fresh_sc_block() -> Vec<u8> {
        let mut block = vec![0u8; BLOCK_SIZE];
        block[..2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        block
    }

    #[test]
    fn write_retail_story_flags_round_trips_through_reader() {
        let mut block = fresh_sc_block();
        let mut bits = vec![0u8; RETAIL_STORY_FLAGS_SIZE];
        for (i, b) in bits.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7) ^ 0x5A;
        }
        let n = write_retail_story_flags(&mut block, &bits).unwrap();
        assert_eq!(n, RETAIL_STORY_FLAGS_SIZE);
        let read = read_retail_story_flags(&block).expect("read back");
        assert_eq!(read, bits.as_slice());
    }

    #[test]
    fn write_retail_story_flags_pads_short_input() {
        let mut block = fresh_sc_block();
        let short = vec![0xAA; 16];
        let n = write_retail_story_flags(&mut block, &short).unwrap();
        assert_eq!(n, 16);
        let read = read_retail_story_flags(&block).unwrap();
        assert!(read[..16].iter().all(|&b| b == 0xAA));
        assert!(read[16..].iter().all(|&b| b == 0));
    }

    #[test]
    fn write_retail_inventory_round_trips() {
        let mut block = fresh_sc_block();
        let pairs: Vec<(u8, u8)> = (0..RETAIL_INVENTORY_SLOTS as u8)
            .map(|i| (i, i.wrapping_mul(3)))
            .collect();
        let n = write_retail_inventory(&mut block, &pairs).unwrap();
        assert_eq!(n, RETAIL_INVENTORY_SLOTS);
        let raw = read_retail_inventory(&block).unwrap();
        for (i, &(id, count)) in pairs.iter().enumerate() {
            assert_eq!(raw[i * 2], id);
            assert_eq!(raw[i * 2 + 1], count);
        }
    }

    #[test]
    fn write_retail_inventory_truncates_overflow() {
        let mut block = fresh_sc_block();
        let pairs: Vec<(u8, u8)> = (0..200u32).map(|i| ((i & 0xFF) as u8, 1)).collect();
        let n = write_retail_inventory(&mut block, &pairs).unwrap();
        assert_eq!(n, RETAIL_INVENTORY_SLOTS);
    }

    #[test]
    fn save_icon_block_identity_stamps_the_full_header() {
        let mut block = vec![0u8; BLOCK_SIZE];
        write_retail_block_identity(&mut block, 0, None).unwrap();
        // All four bytes, not just the magic - `+2` is the icon-frame
        // descriptor and `+3` the block count.
        assert_eq!(&block[..4], &SAVE_BLOCK_HEADER);
    }

    #[test]
    fn save_icon_block_identity_writes_all_three_frames_and_the_palette() {
        let mut block = vec![0u8; BLOCK_SIZE];
        let icon = RetailBlockIcon {
            clut: [0xA5; RETAIL_ICON_CLUT_BYTES],
            pixels: [0x3C; RETAIL_ICON_FRAME_BYTES],
        };
        write_retail_block_identity(&mut block, 0, Some(&icon)).unwrap();
        assert_eq!(
            &block[RETAIL_ICON_CLUT_OFFSET..RETAIL_ICON_CLUT_OFFSET + RETAIL_ICON_CLUT_BYTES],
            &icon.clut
        );
        // Retail writes the same tile into all three frame slots even though
        // the header declares one frame.
        for off in RETAIL_ICON_FRAME_OFFSETS {
            assert_eq!(&block[off..off + RETAIL_ICON_FRAME_BYTES], &icon.pixels);
        }
        // Nothing past the header region moved.
        assert!(block[RETAIL_GAME_DATA_OFFSET..].iter().all(|&b| b == 0));
    }

    #[test]
    fn save_icon_title_digits_are_one_based_and_full_width() {
        assert_eq!(save_title_digits(0), [0x4F, 0x50], "slot 0 shows 01");
        assert_eq!(save_title_digits(8), [0x4F, 0x58]);
        assert_eq!(save_title_digits(9), [0x50, 0x4F], "slot 9 shows 10");
        assert_eq!(save_title_digits(14), [0x50, 0x54], "slot 14 shows 15");
    }

    #[test]
    fn save_icon_identity_patches_digits_only_into_a_real_title() {
        // A block with no title: the digits must not be stamped into
        // emptiness, or the BIOS shows a name that is two numerals.
        let mut empty = vec![0u8; BLOCK_SIZE];
        write_retail_block_identity(&mut empty, 4, None).unwrap();
        assert!(
            empty[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET]
                .iter()
                .all(|&b| b == 0)
        );

        // A block that already carries a title gets its two digits updated.
        let mut titled = vec![0u8; BLOCK_SIZE];
        titled[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET].fill(0x20);
        write_retail_block_identity(&mut titled, 4, None).unwrap();
        let title = &titled[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET];
        assert_eq!(title[RETAIL_TITLE_DIGIT_OFFSETS[0]], 0x4F);
        assert_eq!(
            title[RETAIL_TITLE_DIGIT_OFFSETS[1]], 0x54,
            "slot 4 shows 05"
        );
        // Every other title byte is untouched.
        for (i, &b) in title.iter().enumerate() {
            if !RETAIL_TITLE_DIGIT_OFFSETS.contains(&i) {
                assert_eq!(b, 0x20, "title byte {i} moved");
            }
        }
    }

    #[test]
    fn save_icon_filename_uses_a_hyphen_and_the_slot_suffix() {
        // The separator is a hyphen, verified against the menu overlay's own
        // literal and against real cards' directory frames.
        assert_eq!(legaia_save_filename(0), "BASCUS-94254PRO-00");
        assert_eq!(legaia_save_filename(1), "BASCUS-94254PRO-01");
        assert_eq!(legaia_save_filename(14), "BASCUS-94254PRO-14");
        assert!(legaia_save_filename(3).starts_with(LEGAIA_SAVE_FILENAME_PREFIX));
    }

    #[test]
    fn save_icon_block_identity_rejects_a_short_block() {
        let mut short = vec![0u8; 0x40];
        assert!(write_retail_block_identity(&mut short, 0, None).is_err());
    }

    #[test]
    fn write_retail_char_records_round_trips() {
        let mut block = fresh_sc_block();
        let records: Vec<Vec<u8>> = (0..3u8)
            .map(|n| {
                let mut r = vec![0u8; RETAIL_CHAR_RECORD_STRIDE];
                // Reader stops at the first all-zero slot, so every record
                // here carries at least one nonzero byte to round-trip.
                r[0] = n + 1;
                r[RETAIL_CHAR_RECORD_STRIDE - 1] = (n + 1).wrapping_mul(11);
                r
            })
            .collect();
        let n = write_retail_char_records(&mut block, &records).unwrap();
        assert_eq!(n, 3);
        let read = read_retail_char_records(&block, 4).unwrap();
        assert_eq!(read.len(), 3, "fourth slot is empty so reader stops");
        for (i, rec) in read.iter().enumerate() {
            assert_eq!(rec[0], (i as u8) + 1);
            assert_eq!(
                rec[RETAIL_CHAR_RECORD_STRIDE - 1],
                ((i as u8) + 1).wrapping_mul(11)
            );
        }
    }

    #[test]
    fn read_retail_char_records_huge_max_does_not_panic() {
        // A pathological `max_records` must not overflow the `i * STRIDE`
        // computation into a slice-index panic; the walk just stops at the
        // first empty slot / buffer end.
        let block = fresh_sc_block();
        let _ = read_retail_char_records(&block, usize::MAX);
        // Also a too-small block returns None rather than panicking.
        assert!(read_retail_char_records(&[0u8; 4], usize::MAX).is_none());
    }

    #[test]
    fn write_retail_char_records_does_not_zero_trailing_slots() {
        // The retail SC layout places the story-flag bitmap (+0x14C0) and
        // inventory (+0x1818) regions inside what would otherwise be the
        // trailing record slots. So the writer must leave those bytes
        // untouched - any pre-existing content past the last written record
        // must survive.
        let mut block = fresh_sc_block();
        // Drop a sentinel inside the story-flag region (which lives
        // at +0x14C0, inside the 4th would-be record slot).
        block[RETAIL_STORY_FLAGS_OFFSET + 0x10] = 0xAB;
        let one = vec![{
            let mut r = vec![0u8; RETAIL_CHAR_RECORD_STRIDE];
            r[5] = 99;
            r
        }];
        write_retail_char_records(&mut block, &one).unwrap();
        let read = read_retail_char_records(&block, 4).unwrap();
        assert_eq!(read.len(), 1, "first slot only");
        assert_eq!(read[0][5], 99);
        assert_eq!(
            block[RETAIL_STORY_FLAGS_OFFSET + 0x10],
            0xAB,
            "story-flag region survives the record write"
        );
    }

    #[test]
    fn char_record_region_is_base_anchored_with_name_at_0x2a7() {
        // Regression for the off-by-0x2A7 record anchor. The record region
        // begins at the true record base (game+0x3C8 = live RAM 0x80084708),
        // NOT the visible name field (game+0x66F). A record built with known
        // stats + name must round-trip through write/read + `CharacterRecord`
        // with the stats reading back from their documented offsets, and the
        // name must land in the SC block at game+0x66F (SC +0x86F for slot 0),
        // i.e. record base + `NAME_OFFSET`.
        use crate::character::{CharacterRecord, HpMpSp, NAME_OFFSET};

        let mut block = fresh_sc_block();
        let mut rec = CharacterRecord::zeroed();
        rec.set_hp_mp_sp(HpMpSp {
            hp_cur: 180,
            hp_max: 180,
            mp_cur: 20,
            mp_max: 20,
            sp_cur: 0,
            sp_max: 0,
        });
        rec.set_name("Vahn");
        write_retail_char_records(&mut block, std::slice::from_ref(&rec.raw)).unwrap();

        let read = read_retail_char_records(&block, 4).unwrap();
        assert_eq!(read.len(), 1, "first slot only");
        let parsed = CharacterRecord::parse(&read[0]).unwrap();
        assert_eq!(
            parsed.hp_mp_sp().hp_max,
            180,
            "HP must read back from the +0x104 pair, not a 0x2A7-shifted field"
        );
        assert_eq!(parsed.name(), "Vahn");

        // The name lands at SC +0x86F = game+0x66F = record base + NAME_OFFSET.
        let sc_name = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE + NAME_OFFSET;
        assert_eq!(sc_name, RETAIL_GAME_DATA_OFFSET + 0x66F);
        assert_eq!(&block[sc_name..sc_name + 4], b"Vahn");
    }

    // -- FUN_801E38D8 / FUN_801E1934 / FUN_801DD35C state 5 --------------

    #[test]
    fn checksum_covers_every_word_but_the_last() {
        // 0x7FF words of 1 -> 0x7FF; the stored word itself is excluded.
        let mut block = vec![0u8; BLOCK_SIZE];
        for w in 0..SC_BLOCK_CHECKSUM_WORD {
            block[w * 4] = 1;
        }
        block[RETAIL_BLOCK_CHECKSUM_OFFSET] = 0xFF; // must not be summed
        assert_eq!(
            sc_block_checksum(&block),
            Some(SC_BLOCK_CHECKSUM_WORD as u32)
        );
        // The word-slice kernel and the byte form agree.
        let words: Vec<u32> = block
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(
            sc_block_checksum_words(&words),
            sc_block_checksum(&block).unwrap()
        );
    }

    #[test]
    fn checksum_wraps_and_needs_a_full_block() {
        let mut block = vec![0u8; BLOCK_SIZE];
        block[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        block[4..8].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(sc_block_checksum(&block), Some(2)); // 0xFFFF_FFFF + 3
        // Anything short of a whole block has no checksum word at all.
        assert_eq!(sc_block_checksum(&block[..BLOCK_SIZE - 1]), None);
        assert!(!sc_block_checksum_valid(&block[..BLOCK_SIZE - 1]));
        assert_eq!(restamp_sc_block_checksum(&mut vec![0u8; 0x1A18]), None);
    }

    /// The oracle by contrast: a restamped block validates, a byte poked
    /// afterwards does not, and restamping again repairs it. This is the
    /// whole reason every `write_retail_*` writer restamps.
    #[test]
    fn a_field_edit_invalidates_the_checksum_until_it_is_restamped() {
        let mut block = vec![0u8; BLOCK_SIZE];
        block[..2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        assert!(restamp_sc_block_checksum(&mut block).is_some());
        assert!(sc_block_checksum_valid(&block));

        // A raw poke leaves the word stale.
        block[RETAIL_GOLD_OFFSET] ^= 0x5A;
        assert!(!sc_block_checksum_valid(&block), "stale after a raw poke");
        assert!(restamp_sc_block_checksum(&mut block).is_some());
        assert!(sc_block_checksum_valid(&block));

        // Every writer in this module does the restamp for its caller.
        write_retail_gold(&mut block, 12_345).unwrap();
        assert!(sc_block_checksum_valid(&block), "gold writer restamps");
        write_retail_coins(&mut block, 777).unwrap();
        assert!(sc_block_checksum_valid(&block), "coin writer restamps");
        write_retail_inventory(&mut block, &[(0x10, 3)]).unwrap();
        assert!(sc_block_checksum_valid(&block), "inventory writer restamps");
        write_retail_story_flags(&mut block, &[0xAA; 8]).unwrap();
        assert!(
            sc_block_checksum_valid(&block),
            "story-flag writer restamps"
        );
        write_retail_char_records(&mut block, &[vec![0x11; RETAIL_CHAR_RECORD_STRIDE]]).unwrap();
        assert!(sc_block_checksum_valid(&block), "record writer restamps");

        // And the values survive the restamp.
        assert_eq!(read_retail_gold(&block), Some(12_345));
        assert_eq!(read_retail_coins(&block), Some(777));
    }
}
