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

mod block_identity;
mod directory;
mod sc_block;
mod sc_checksum;
#[cfg(test)]
mod tests;

pub use block_identity::*;
pub use directory::*;
pub use sc_block::*;
pub use sc_checksum::*;
