//! Character record schema for Legend of Legaia.
//!
//! Per-character runtime state lives in 0x414-byte records starting at
//! `0x80084708`; record `n` is at `0x80084708 + n * 0x414`. The layout is
//! documented in [`docs/subsystems/battle.md`](../../../docs/subsystems/battle.md#character-record-layout)
//! based on the consumers reversed in `FUN_80042558` / `FUN_80042DBC` /
//! `FUN_800432BC` / `FUN_800431FC` / `FUN_80043264`.
//!
//! This crate provides:
//!
//! - [`CharacterRecord`] — typed accessors for the documented offsets.
//! - [`CharacterRecord::parse`] — read a 0x414-byte buffer into the struct.
//! - [`CharacterRecord::write`] — serialise back to a 0x414-byte buffer.
//! - [`Party`] — convenience wrapper for an N-character roster.
//!
//! Round-trip guarantee: `write(parse(buf)) == buf` for any 0x414 buffer.
//! The struct keeps the full raw bytes alongside typed views; unknown
//! offsets pass through unchanged. This is the foundation for both
//! save/load and any future runtime that needs to mutate party state.
//!
//! ## Clean-room boundary
//!
//! No Sony bytes live in this crate. The struct definitions are derived
//! purely from the consumer disassembly in `ghidra/scripts/funcs/` and
//! the docs page above. Tests use synthesised buffers (zeros + named
//! fields written through the typed setters).
//!
//! ## What this is NOT
//!
//! - **Not** the PSX memory-card `.mcs` save format. That format wraps
//!   one or more character records plus runtime globals (party leader,
//!   inventory, story flags, current scene, frame counter, etc.) in a
//!   block format we haven't reversed yet. Once captured, that wrapper
//!   will live in a sibling module here.
//! - **Not** the inventory layout itself. Inventory is page-banked at
//!   `[0x80085718..0x80085918)` — see
//!   [`docs/subsystems/battle.md`](../../../docs/subsystems/battle.md#inventory)
//!   for the page-of-16-entries × 16-bit layout. This crate covers the
//!   per-character record only.

#![deny(missing_docs)]

pub mod card;
pub mod character;
pub mod ext;

pub use card::{
    BLOCK_SIZE, CARD_MAGIC, CARD_SIZE, DIR_FRAMES, DirEntry, RETAIL_CHAR_RECORD_HEADER_SIZE,
    RETAIL_CHAR_RECORD_STRIDE, RETAIL_GAME_DATA_OFFSET, SAVE_BLOCK_MAGIC, SaveBlock, parse_card,
    read_block, read_retail_char_records, walk_directory, write_block,
};
pub use character::{
    ABILITY_BITS_LEN, CHARACTER_RECORD_SIZE, CharacterRecord, EquipmentSlots, HpMpSp, MAX_SPELLS,
    Party, SpellList,
};
pub use ext::{
    CharSaveExt, SAVE_FILE_EXT_MAGIC, SAVE_FILE_MAGIC, SAVE_FILE_VERSION, SAVE_FILE_VERSION_V1,
    SaveExt, SaveExtV2, SaveFile, SavedChainRecord,
};
