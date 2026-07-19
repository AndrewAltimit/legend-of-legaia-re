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
//! - [`CharacterRecord`] - typed accessors for the documented offsets.
//! - [`CharacterRecord::parse`] - read a 0x414-byte buffer into the struct.
//! - [`CharacterRecord::write`] - serialise back to a 0x414-byte buffer.
//! - [`Party`] - convenience wrapper for an N-character roster.
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
//!   `[0x80085718..0x80085918)` - see
//!   [`docs/subsystems/battle.md`](../../../docs/subsystems/battle.md#inventory)
//!   for the page-of-16-entries × 16-bit layout. This crate covers the
//!   per-character record only.

#![deny(missing_docs)]

pub mod card;
pub mod character;
pub mod emu;
pub mod ext;
pub mod retail_inventory;

pub use card::{
    BLOCK_SIZE, CARD_MAGIC, CARD_SIZE, DIR_FRAMES, DirEntry, RETAIL_CHAR_RECORD_HEADER_SIZE,
    RETAIL_CHAR_RECORD_STRIDE, RETAIL_COINS_OFFSET, RETAIL_GAME_DATA_OFFSET, RETAIL_GOLD_OFFSET,
    RETAIL_INVENTORY_OFFSET, RETAIL_INVENTORY_SIZE, RETAIL_INVENTORY_SLOTS,
    RETAIL_MAX_CHAR_RECORDS, RETAIL_STORY_FLAGS_OFFSET, RETAIL_STORY_FLAGS_SIZE, SAVE_BLOCK_MAGIC,
    SAVE_GAME_DATA_RAM_BASE, SaveBlock, parse_card, read_block, read_retail_char_records,
    read_retail_coins, read_retail_gold, read_retail_inventory, read_retail_story_flags,
    walk_directory, write_block, write_retail_char_records, write_retail_coins, write_retail_gold,
    write_retail_inventory, write_retail_story_flags,
};
pub use character::{
    ABILITY_BITS_LEN, CHARACTER_RECORD_SIZE, CharacterRecord, EquipmentSlots, HpMpSp, MAX_SPELLS,
    NAME_LEN, NAME_OFFSET, Party, SpellList,
};
pub use ext::{
    CharSaveExt, SAVE_FILE_EXT_MAGIC, SAVE_FILE_EXT3_MAGIC, SAVE_FILE_EXT4_MAGIC, SAVE_FILE_MAGIC,
    SAVE_FILE_VERSION, SAVE_FILE_VERSION_V1, SAVE_FILE_VERSION_V2, SAVE_FILE_VERSION_V3, SaveExt,
    SaveExtV2, SaveFile, SavedChainRecord,
};
pub use retail_inventory::{
    AddOutcome, FULL_WINDOW_STORY_FLAG, GENERAL_ITEM_PAGE_SLOTS, ITEM_SLOTS_HALF, ITEM_SLOTS_TOTAL,
    ITEM_WINDOW_BASE, ITEM_WINDOW_SLOTS, ItemWindow, RetailInventory, STACK_CAP,
};

/// Retail cumulative XP thresholds for levels 2..=99 (the **base / slot-0
/// curve**: Vahn and the 4th roster slot; slots 1/2 apply a ± correction, see
/// below). `RETAIL_XP_CUMULATIVE[i]` = total XP required to reach level `i + 2`
/// - `121, 365, 730, 1338, 2190, …, 9_646_483`.
///
/// Fully **derived**, no table bytes copied: the retail level-up applier
/// `FUN_801E9504` (called from the battle reward resolver `FUN_8004E568`)
/// sums the static-SCUS per-level u16 delta table `DAT_80076AF4` - whose 98
/// entries are exactly the closed form `delta(n) = n²/4 + 1` (integer
/// division) - and scales the running sum: `(sum × 9_999_999) / 0x140FE` for
/// `level < 0x11`, else `sum × 0x79`. Validated byte-exact against the record
/// `+0x4` next-level-threshold field across the save-state library (New Game
/// Vahn L1 shows "Next Level 121") and identical to the boot-time disc parse
/// (`legaia_asset::level_up_tables::xp_thresholds_from_scus`).
///
/// Slots 1 (Noa) and 2 (Gala) shift each threshold by `threshold × 0x14 /
/// divisor[level]` (Noa earlier, Gala later; sin-LUT divisor table at
/// `0x80070A2C`, stride `0x28`) - that correction is disc data, handled by
/// `legaia_asset::level_up_tables` + `engine_core::levelup::LevelUpTracker`.
///
/// Engines that don't already pull this from
/// `engine_core::levelup::retail_xp_table` can use this constant directly.
// PORT: FUN_801E9504 (XP-threshold derivation, base curve)
pub const RETAIL_XP_CUMULATIVE: [u32; 98] = build_retail_cumulative();

const fn build_retail_cumulative() -> [u32; 98] {
    // The literal FUN_801E9504 arithmetic over the DAT_80076AF4 closed form
    // (delta(n) = n²/4 + 1). Single source of truth for the workspace -
    // engine_core::levelup::retail_xp_table re-exposes this constant.
    let mut out = [0u32; 98];
    let mut cum: u64 = 0;
    let mut i = 0;
    while i < 98 {
        let n = (i + 1) as u64; // delta index n = 1..=98
        cum += n * n / 4 + 1; // DAT_80076AF4[n - 1]
        // out[i] = XP to reach level i + 2 = threshold at current level i + 1.
        let level = i + 1;
        out[i] = if level < 0x11 {
            (cum * 9_999_999 / 0x140FE) as u32
        } else {
            (cum * 0x79) as u32
        };
        i += 1;
    }
    out
}

/// Cumulative XP needed to reach `level` (1..=99). `xp_for_level(1) == 0`
/// and `xp_for_level(2) == RETAIL_XP_CUMULATIVE[0]`. Levels above 99 saturate
/// at the L99 threshold - the table doesn't go higher.
pub fn xp_for_level(level: u8) -> u32 {
    if level <= 1 {
        return 0;
    }
    let idx = (level as usize - 2).min(RETAIL_XP_CUMULATIVE.len() - 1);
    RETAIL_XP_CUMULATIVE[idx]
}

/// Infer the character level (1..=99) from a cumulative-XP value against the
/// base (slot-0) curve.
///
/// Returns `1` for any `xp < 121` (the L2 threshold), and `99` once `xp`
/// reaches the L99 threshold. Slots 1/2 (Noa/Gala) level slightly earlier /
/// later than this base-curve inference (the ± sin-divisor correction);
/// engines that track the authoritative level byte (record `+0x130`) should
/// prefer it over this inference.
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
        assert_eq!(level_for_cumulative_xp(120), 1);
        assert_eq!(level_for_cumulative_xp(121), 2);
        assert_eq!(level_for_cumulative_xp(365), 3);
        assert_eq!(level_for_cumulative_xp(364), 2);
    }

    #[test]
    fn level_caps_at_99() {
        assert_eq!(level_for_cumulative_xp(u32::MAX), 99);
    }

    #[test]
    fn cumulative_table_matches_retail_captures() {
        // Live record +0x4 across the save-state library: New Game Vahn L1
        // "Next Level 121", L2 -> 365, L3 -> 730 (Status-menu capture +
        // mednafen RAM reads); L37 -> 535_546 (uncorrected slot 0).
        assert_eq!(RETAIL_XP_CUMULATIVE[0], 121);
        assert_eq!(RETAIL_XP_CUMULATIVE[1], 365);
        assert_eq!(RETAIL_XP_CUMULATIVE[2], 730);
        assert_eq!(RETAIL_XP_CUMULATIVE[3], 1338);
        assert_eq!(RETAIL_XP_CUMULATIVE[4], 2190);
        assert_eq!(RETAIL_XP_CUMULATIVE[36], 535_546);
        // L99 total (base curve). A maxed retail Vahn save carries
        // cumulative XP just above this.
        assert_eq!(*RETAIL_XP_CUMULATIVE.last().unwrap(), 9_646_483);
    }

    #[test]
    fn formula_switch_at_level_0x11() {
        // level < 0x11 uses (sum × 9_999_999) / 0x140FE; level >= 0x11 uses
        // sum × 0x79. Both scale factors are ≈ 121.7 vs 121, so the curve is
        // continuous but not identical - pin the boundary values.
        assert_eq!(RETAIL_XP_CUMULATIVE[15], 47_216); // L17 threshold (level 16, scaled form)
        assert_eq!(RETAIL_XP_CUMULATIVE[16], 55_781); // L18 threshold (level 0x11, × 0x79)
    }

    #[test]
    fn xp_for_level_semantics() {
        assert_eq!(xp_for_level(1), 0);
        assert_eq!(xp_for_level(2), 121);
        assert_eq!(xp_for_level(99), 9_646_483);
        // Saturates at the L99 threshold.
        assert_eq!(xp_for_level(200), 9_646_483);
    }
}
