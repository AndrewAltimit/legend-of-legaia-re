//! Fishing-minigame **point-exchange (prize shop) tables** - the two per-venue
//! 6-row price tables the exchange branch of the fishing overlay sells from.
//!
//! ## Provenance
//!
//! The exchange sub-screens of the fishing mode driver (`FUN_801cf3bc` states
//! `0x64..0x7a`) read a 12-byte-stride record table through the pointer cell
//! `PTR_DAT_801d90b8`:
//!
//! * `FUN_801d0c3c` (`ghidra/scripts/funcs/overlay_fishing_801d0c3c.txt`) -
//!   the 6-row prize list screen. Each row prints the item name via the MES
//!   `0xC2` item-name token fed with `record[+8]` and the price from
//!   `record[+4]`; the running point total `_DAT_8008444C` renders capped at
//!   `999999`. Row 0 is *hidden* until it is strictly affordable
//!   (`record0[+4] < points`) - the list cursor's minimum index is
//!   `(price0 < points) ^ 1`.
//! * `FUN_801d092c` (`overlay_fishing_801d092c.txt`) - the "Trade how many?"
//!   quantity picker. Max quantity = `min(points / price, limit - owned)`
//!   where `owned` = the live inventory count of `record[+8]`
//!   (`func_0x80042f4c`) and `limit` = `record[+0]`; a one-time row
//!   (`limit == 1`) that has not been purchased yet treats `owned` as 0.
//! * `FUN_801d06c8` (`overlay_fishing_801d06c8.txt`) - the "Are you sure?"
//!   confirm. On Yes it grants `func_0x800421d4(record[+8], qty)`, deducts
//!   `record[+4] * qty` from `_DAT_8008444C`, and for `limit == 1` rows
//!   latches bit `(row + venue * 8)` of the persistent purchased bitmask
//!   `_DAT_8008446C`.
//! * `FUN_801d6f90` (`overlay_fishing_801d6f90.txt`) - row availability
//!   (drawn white vs grey): `price <= points`, inventory count `!= 99`, and
//!   the row's one-time bit not latched.
//!
//! ## Record layout (stride [`EXCHANGE_ROW_STRIDE`])
//!
//! | Off | Field | Meaning |
//! |---|---|---|
//! | `+0x00` | `limit` | Max obtainable count: `1` = one-time prize (latched in `_DAT_8008446C`), `99` = repeatable up to the inventory cap |
//! | `+0x04` | `price` | Cost in fishing points (`_DAT_8008444C`) per unit |
//! | `+0x08` | `item_id` | Granted item id (the SCUS item-name-table id space) |
//!
//! ## Venue pages
//!
//! Two consecutive 6-row tables live in the overlay `.rodata`
//! (`FUN_801cf3bc` case 1 selects the page into `PTR_DAT_801d90b8` from the
//! venue global `_DAT_8007BAC4`):
//!
//! * page 0 @ VA [`EXCHANGE_TABLE_VA_PAGE0`] - selected when
//!   `_DAT_8007BAC4 == 0x187` (the **Buma** pond; `0x187` = 391 = the Karisto
//!   kingdom-bundle extraction index).
//! * page 1 @ VA [`EXCHANGE_TABLE_VA_PAGE1`] (= page 0 + `6 * 12`) - selected
//!   when `_DAT_8007BAC4 == 0xF4` (the **Vidna** pond; `0xF4` = 244 = the
//!   Sebucus kingdom-bundle extraction index).
//!
//! The same case also pages the venue's **species-spawn table** into
//! `PTR_DAT_801d9114` (see [`crate::fishing_species`] - the spawn tables sit
//! directly after the species table).
//!
//! Both venues spend and latch against the same globals - the point pool and
//! the purchased bitmask are shared, with venue 1's one-time rows occupying
//! bits `8..` (the `row + venue * 8` bit index).
//!
//! ## No Sony bytes
//!
//! The literal row values (prices, item ids) stay on the user's disc; this
//! module decodes them from the as-loaded PROT 0972 overlay image at runtime
//! (same pipeline as [`crate::fishing_species`]). The disc-gated
//! `fishing_exchange_real` test pins the structural invariants.

use crate::fishing_species::FISHING_OVERLAY_BASE_VA;

/// Runtime VA of the page-0 (Buma) exchange table (`&DAT_801d8088`).
pub const EXCHANGE_TABLE_VA_PAGE0: u32 = 0x801D_8088;

/// Runtime VA of the page-1 (Vidna) exchange table (`&DAT_801d80d0`).
pub const EXCHANGE_TABLE_VA_PAGE1: u32 = 0x801D_80D0;

/// Per-row stride (`DAT_801d90d4 * 0xc` index math).
pub const EXCHANGE_ROW_STRIDE: usize = 0xC;

/// Rows per venue page (the list screen draws exactly 6).
pub const EXCHANGE_ROWS: usize = 6;

/// Venue-selector value for page 0 (`_DAT_8007BAC4 == 0x187`, Buma).
pub const VENUE_SELECTOR_PAGE0: u32 = 0x187;

/// Venue-selector value for page 1 (`_DAT_8007BAC4 == 0xF4`, Vidna).
pub const VENUE_SELECTOR_PAGE1: u32 = 0xF4;

/// Retail RAM VA of the shared fishing-point pool the exchange spends from.
pub const FISHING_POINTS_VA: u32 = 0x8008_444C;

/// Retail RAM VA of the persistent one-time-purchase bitmask
/// (bit `row + venue * 8`).
pub const PURCHASED_MASK_VA: u32 = 0x8008_446C;

/// Inventory count at which a row greys out (`FUN_801d6f90`'s `== 99` check).
pub const OWNED_CAP: u32 = 99;

/// One decoded exchange row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeRow {
    /// Row index within the venue page (0..6; also the one-time bit's
    /// low index).
    pub row: usize,
    /// `+0x00` - max obtainable count (1 = one-time, 99 = repeatable).
    pub limit: u32,
    /// `+0x04` - price in fishing points per unit.
    pub price: u32,
    /// `+0x08` - granted item id (SCUS item-name-table id space).
    pub item_id: u32,
}

impl ExchangeRow {
    /// Whether this is a one-time prize row (latched in the purchased
    /// bitmask on buy).
    pub fn is_one_time(&self) -> bool {
        self.limit == 1
    }
}

/// The two venue pages, in page order (`[0]` = Buma, `[1]` = Vidna).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FishingExchange {
    /// `venues[page][row]`.
    pub venues: [Vec<ExchangeRow>; 2],
}

impl FishingExchange {
    /// The one-time-purchase bit index for `(page, row)` - retail's
    /// `row + venue * 8` into `_DAT_8008446C`.
    pub fn purchase_bit(page: usize, row: usize) -> u32 {
        (row + page * 8) as u32 & 0x1F
    }
}

/// Parse both venue pages out of the as-loaded fishing overlay image
/// (PROT entry [`crate::fishing_species::FISHING_OVERLAY_PROT_INDEX`]).
/// Returns `None` if the image is too short.
pub fn parse(overlay: &[u8]) -> Option<FishingExchange> {
    let page0 = parse_page(overlay, EXCHANGE_TABLE_VA_PAGE0)?;
    let page1 = parse_page(overlay, EXCHANGE_TABLE_VA_PAGE1)?;
    Some(FishingExchange {
        venues: [page0, page1],
    })
}

/// Parse one 6-row page at overlay VA `table_va`.
pub fn parse_page(overlay: &[u8], table_va: u32) -> Option<Vec<ExchangeRow>> {
    let off = table_va.checked_sub(FISHING_OVERLAY_BASE_VA)? as usize;
    let need = off + EXCHANGE_ROWS * EXCHANGE_ROW_STRIDE;
    if overlay.len() < need {
        return None;
    }
    let rd = |p: usize| -> u32 {
        u32::from_le_bytes([overlay[p], overlay[p + 1], overlay[p + 2], overlay[p + 3]])
    };
    Some(
        (0..EXCHANGE_ROWS)
            .map(|row| {
                let b = off + row * EXCHANGE_ROW_STRIDE;
                ExchangeRow {
                    row,
                    limit: rd(b),
                    price: rd(b + 4),
                    item_id: rd(b + 8),
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_geometry() {
        // Page 1 sits directly after page 0 (6 rows x 12 bytes).
        assert_eq!(
            EXCHANGE_TABLE_VA_PAGE1,
            EXCHANGE_TABLE_VA_PAGE0 + (EXCHANGE_ROWS * EXCHANGE_ROW_STRIDE) as u32
        );
        // File offsets are in the overlay's rodata band, before the species
        // table at 0x998C.
        assert_eq!(
            (EXCHANGE_TABLE_VA_PAGE0 - FISHING_OVERLAY_BASE_VA) as usize,
            0x9870
        );
    }

    #[test]
    fn purchase_bits_split_by_venue() {
        assert_eq!(FishingExchange::purchase_bit(0, 0), 0);
        assert_eq!(FishingExchange::purchase_bit(0, 5), 5);
        assert_eq!(FishingExchange::purchase_bit(1, 0), 8);
        assert_eq!(FishingExchange::purchase_bit(1, 5), 13);
    }

    #[test]
    fn parse_page_decodes_rows() {
        // Synthetic overlay: place a page-0 table with recognisable values.
        let off = (EXCHANGE_TABLE_VA_PAGE0 - FISHING_OVERLAY_BASE_VA) as usize;
        let mut ov = vec![0u8; off + EXCHANGE_ROWS * EXCHANGE_ROW_STRIDE];
        for row in 0..EXCHANGE_ROWS {
            let b = off + row * EXCHANGE_ROW_STRIDE;
            let limit: u32 = if row == 0 { 1 } else { 99 };
            ov[b..b + 4].copy_from_slice(&limit.to_le_bytes());
            ov[b + 4..b + 8].copy_from_slice(&(100 * (row as u32 + 1)).to_le_bytes());
            ov[b + 8..b + 12].copy_from_slice(&(0x70 + row as u32).to_le_bytes());
        }
        let rows = parse_page(&ov, EXCHANGE_TABLE_VA_PAGE0).expect("parses");
        assert_eq!(rows.len(), EXCHANGE_ROWS);
        assert!(rows[0].is_one_time());
        assert_eq!(rows[2].price, 300);
        assert_eq!(rows[5].item_id, 0x75);
        // Too-short image refuses.
        assert!(parse_page(&ov[..off], EXCHANGE_TABLE_VA_PAGE0).is_none());
    }
}
