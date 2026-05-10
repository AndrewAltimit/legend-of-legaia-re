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

/// Cumulative XP thresholds for levels 2..=99 derived from the retail
/// SCUS_942.54 increment table at `0x8007123C`. Total XP to reach level
/// `N+2` (from level 1) is `RETAIL_XP_CUMULATIVE[N]`.
///
/// Engines that don't already pull this from
/// `engine_core::levelup::retail_xp_table` can use this constant directly.
pub const RETAIL_XP_CUMULATIVE: [u32; 98] = build_retail_cumulative();

const fn build_retail_cumulative() -> [u32; 98] {
    // Mirrors engine_core::levelup::retail_xp_table — kept here so the
    // legaia-save crate doesn't need to depend on engine-core.
    const INCREMENTS: [u16; 98] = [
        50, 56, 62, 69, 75, 81, 87, 94, 100, 106, 113, 119, 125, 131, 138, 144, 150, 157, 163, 169,
        175, 182, 188, 194, 200, 207, 213, 219, 226, 232, 238, 244, 251, 257, 263, 269, 276, 282,
        288, 295, 301, 307, 313, 320, 326, 332, 338, 345, 351, 357, 363, 370, 376, 382, 388, 395,
        401, 407, 413, 420, 426, 432, 438, 445, 451, 457, 463, 470, 476, 482, 488, 495, 501, 507,
        513, 520, 526, 532, 538, 545, 551, 557, 563, 569, 576, 582, 588, 594, 601, 607, 613, 619,
        625, 632, 638, 644, 650, 656,
    ];
    let mut out = [0u32; 98];
    let mut total: u32 = 0;
    let mut i = 0;
    while i < 98 {
        total += INCREMENTS[i] as u32;
        out[i] = total;
        i += 1;
    }
    out
}

/// Infer the character level (1..=99) from a cumulative-XP value.
///
/// Returns `1` for any `xp < 50` (the L2 threshold), and `99` once `xp`
/// exceeds the L99 threshold. Engines that need finer granularity should
/// override the inference with an authoritative level field once the
/// retail layout is captured.
pub fn level_for_cumulative_xp(xp: u32) -> u8 {
    let mut level: u8 = 1;
    for (i, &thr) in RETAIL_XP_CUMULATIVE.iter().enumerate() {
        if xp >= thr {
            level = (i as u8 + 2).min(99);
        } else {
            break;
        }
    }
    level
}

#[cfg(test)]
mod xp_tests {
    use super::*;

    #[test]
    fn level_inference_thresholds() {
        assert_eq!(level_for_cumulative_xp(0), 1);
        assert_eq!(level_for_cumulative_xp(49), 1);
        assert_eq!(level_for_cumulative_xp(50), 2);
        assert_eq!(level_for_cumulative_xp(106), 3); // 50 + 56
        assert_eq!(level_for_cumulative_xp(105), 2);
    }

    #[test]
    fn level_caps_at_99() {
        assert_eq!(level_for_cumulative_xp(u32::MAX), 99);
    }

    #[test]
    fn cumulative_table_first_and_last() {
        assert_eq!(RETAIL_XP_CUMULATIVE[0], 50);
        // Last entry is the L99 threshold (sum of the 98 increments).
        // The documented retail total ≈ 34_663 (per the SCUS_942.54 0x8007123C
        // increments). Verify the table sums into a sensible range.
        assert!(*RETAIL_XP_CUMULATIVE.last().unwrap() >= 30_000);
        assert!(*RETAIL_XP_CUMULATIVE.last().unwrap() <= 40_000);
    }

    #[test]
    fn vahn_mc8_to_mc9_xp_pin_matches_table() {
        // Pre-grant cumulative 365 reaches level… check.
        let pre = level_for_cumulative_xp(365);
        // 50, 106, 168, 237, 312, 393… so 365 is between L6 (312) and L7 (393).
        assert_eq!(pre, 6);
        // Post-grant cumulative 730 reaches level… check.
        let post = level_for_cumulative_xp(730);
        // 730 should be between L9 and L10.
        assert!((9..=10).contains(&post));
    }
}
