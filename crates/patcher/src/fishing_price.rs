//! Fishing point-exchange **price edits**.
//!
//! The fishing prize counters (Buma / Vidna ponds) sell accessories and
//! consumables for **fishing points** rather than gold. Each prize is a 12-byte
//! row `[u32 limit][u32 price][u32 item_id]` in the raw fishing overlay
//! ([`legaia_asset::fishing_exchange`], PROT entry
//! [`legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX`] = 972). The
//! `price` field is both the point cost *and* the "only appears once you can
//! afford it" gate (row 0 is hidden until `price < points`), so lowering a
//! prize's price both cheapens it and makes it show up sooner.
//!
//! This module edits those `price` u32s in place. PROT 972 is a **raw** overlay
//! (no LZS), so an edit is a direct same-size [`crate::disc::DiscPatcher::patch_prot_entry`]
//! write - no recompression. A prize is targeted by its **item id** (the SCUS
//! item-name id space), which is stable across the two venue pages, so
//! e.g. the Buma **Water Egg** (id `0x6F`) is found by id rather than a fixed
//! row index. No Sony bytes: the module only rewrites integer price fields the
//! user's own disc already holds.

use anyhow::{Result, bail};

use legaia_asset::fishing_exchange::{self, EXCHANGE_ROW_STRIDE};
use legaia_asset::fishing_species::{FISHING_OVERLAY_BASE_VA, FISHING_OVERLAY_PROT_INDEX};

/// The fishing overlay's PROT entry (re-exported for callers/tests).
pub const OVERLAY_PROT_INDEX: usize = FISHING_OVERLAY_PROT_INDEX;

/// The two venue-page table VAs, in page order (`[0]` = Buma, `[1]` = Vidna).
const PAGE_TABLE_VAS: [u32; 2] = [
    fishing_exchange::EXCHANGE_TABLE_VA_PAGE0,
    fishing_exchange::EXCHANGE_TABLE_VA_PAGE1,
];

/// Byte offset of a `(page, row)` row's `price` field within the raw overlay
/// entry. `price` is `+0x04` inside the 12-byte row.
pub fn price_field_offset(page: usize, row: usize) -> usize {
    let table = (PAGE_TABLE_VAS[page] - FISHING_OVERLAY_BASE_VA) as usize;
    table + row * EXCHANGE_ROW_STRIDE + 0x04
}

/// One planned price edit: the raw-entry file offset of a `price` u32 and the
/// new little-endian value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceEdit {
    /// Venue page (0 = Buma, 1 = Vidna).
    pub page: usize,
    /// Row within the page (0..6).
    pub row: usize,
    /// The prize's item id (SCUS item-name id space).
    pub item_id: u32,
    /// File offset of the `price` u32 within the PROT 972 entry.
    pub offset: usize,
    /// Prior price (points).
    pub old_price: u32,
    /// New price (points).
    pub new_price: u32,
}

/// Plan the price edits that set **every prize row granting `item_id`** to
/// `new_price`, across both venue pages. Returns one [`PriceEdit`] per matching
/// row whose current price differs from `new_price` (so re-applying is a no-op).
///
/// Fails if the overlay can't be parsed or no row grants `item_id` (a typo /
/// wrong-disc guard rather than a silent no-op).
pub fn plan_set_price(overlay: &[u8], item_id: u32, new_price: u32) -> Result<Vec<PriceEdit>> {
    let exchange = fishing_exchange::parse(overlay).ok_or_else(|| {
        anyhow::anyhow!("fishing overlay (PROT {OVERLAY_PROT_INDEX}) too short to parse")
    })?;
    let mut edits = Vec::new();
    let mut matched = false;
    for (page, rows) in exchange.venues.iter().enumerate() {
        for r in rows {
            if r.item_id != item_id {
                continue;
            }
            matched = true;
            if r.price == new_price {
                continue;
            }
            edits.push(PriceEdit {
                page,
                row: r.row,
                item_id,
                offset: price_field_offset(page, r.row),
                old_price: r.price,
                new_price,
            });
        }
    }
    if !matched {
        bail!("no fishing prize grants item id 0x{item_id:02X} (nothing to reprice)");
    }
    Ok(edits)
}

/// One prize row for listing/UX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrizeInfo {
    /// Venue page (0 = Buma, 1 = Vidna).
    pub page: usize,
    /// Row within the page (0..6).
    pub row: usize,
    /// Granted item id (SCUS item-name id space).
    pub item_id: u32,
    /// Price in fishing points.
    pub price: u32,
    /// One-time prize (latched on purchase) vs. repeatable.
    pub one_time: bool,
}

/// Read the current prize rows across both venue pages, in page/row order.
pub fn list_prizes(overlay: &[u8]) -> Result<Vec<PrizeInfo>> {
    let exchange = fishing_exchange::parse(overlay).ok_or_else(|| {
        anyhow::anyhow!("fishing overlay (PROT {OVERLAY_PROT_INDEX}) too short to parse")
    })?;
    let mut out = Vec::new();
    for (page, rows) in exchange.venues.iter().enumerate() {
        for r in rows {
            out.push(PrizeInfo {
                page,
                row: r.row,
                item_id: r.item_id,
                price: r.price,
                one_time: r.is_one_time(),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::fishing_exchange::EXCHANGE_ROWS;

    /// Build a synthetic raw overlay with a known two-page exchange table.
    fn synthetic() -> Vec<u8> {
        let table0 = (PAGE_TABLE_VAS[0] - FISHING_OVERLAY_BASE_VA) as usize;
        let mut ov = vec![0u8; table0 + 2 * EXCHANGE_ROWS * EXCHANGE_ROW_STRIDE];
        // Page 0 row 0 = item 0x6F (Water Egg) @ 20000; row 1 = item 0xE5 @ 6500.
        // Page 1 row 0 = item 0x6F (also Water Egg) @ 15000 (cross-page match test).
        let put = |ov: &mut [u8], page: usize, row: usize, limit: u32, price: u32, item: u32| {
            let b = (PAGE_TABLE_VAS[page] - FISHING_OVERLAY_BASE_VA) as usize
                + row * EXCHANGE_ROW_STRIDE;
            ov[b..b + 4].copy_from_slice(&limit.to_le_bytes());
            ov[b + 4..b + 8].copy_from_slice(&price.to_le_bytes());
            ov[b + 8..b + 12].copy_from_slice(&item.to_le_bytes());
        };
        put(&mut ov, 0, 0, 1, 20000, 0x6F);
        put(&mut ov, 0, 1, 1, 6500, 0xE5);
        put(&mut ov, 1, 0, 1, 15000, 0x6F);
        ov
    }

    #[test]
    fn price_offset_matches_row_geometry() {
        // Buma page (0) row 0 price field = table base (0x9870) + 4.
        assert_eq!(price_field_offset(0, 0), 0x9874);
        // Vidna page (1) row 0 = page-1 table + 4 = 0x9870 + 6*12 + 4.
        assert_eq!(
            price_field_offset(1, 0),
            0x9870 + EXCHANGE_ROWS * EXCHANGE_ROW_STRIDE + 4
        );
    }

    #[test]
    fn plan_sets_every_matching_row() {
        let ov = synthetic();
        let edits = plan_set_price(&ov, 0x6F, 500).expect("item present");
        // Both the Buma and Vidna Water-Egg rows are repriced.
        assert_eq!(edits.len(), 2);
        assert!(
            edits
                .iter()
                .all(|e| e.new_price == 500 && e.item_id == 0x6F)
        );
        assert_eq!(edits[0].offset, 0x9874);
        assert_eq!(edits[0].old_price, 20000);
    }

    #[test]
    fn plan_is_idempotent_and_skips_unchanged() {
        let ov = synthetic();
        // Setting to the current price yields no edits (but still matches).
        let edits = plan_set_price(&ov, 0xE5, 6500).expect("item present");
        assert!(edits.is_empty());
    }

    #[test]
    fn plan_refuses_absent_item() {
        let ov = synthetic();
        assert!(plan_set_price(&ov, 0x01, 100).is_err());
    }

    #[test]
    fn list_reports_all_rows() {
        let ov = synthetic();
        let rows = list_prizes(&ov).unwrap();
        // 6 rows per page x 2 pages (zero rows are still rows).
        assert_eq!(rows.len(), 12);
        let water = rows.iter().filter(|r| r.item_id == 0x6F).count();
        assert_eq!(water, 2);
    }
}
