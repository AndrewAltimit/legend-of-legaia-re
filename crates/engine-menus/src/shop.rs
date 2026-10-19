//! Shop UI state - item list, buy / sell cursor, quantity selection, and
//! gold / inventory delta application.
//!
//! Owns only the mutable state needed to drive
//! `MenuState::ShopBuy / ShopSell / ShopQuantity / ShopConfirm / ShopExit`.
//! Gold and inventory mutations are applied to the caller's `World` by
//! [`ShopSession::try_buy`] and [`ShopSession::try_sell`].
//!
//! Per-scene shop **stock + prices** are decoded from the disc by
//! [`crate::shop_catalog`] (the scene MAN's op-`0x49` records + the SCUS item
//! price table) and parked on `crate::shop_state::ShopState::scene_shops`; the field-VM
//! op-`0x49` merchant trigger opens one (`legaia_engine_core::world::World::try_arm_field_shop`).
//! This module owns only the buy/sell session state. (Disc-free builds leave the
//! stock host-supplied.)

mod buy_quantity;
mod buy_recipient;
mod item_detail;
mod labels;
mod quantity_picker;
mod row_style;
mod screen_windows;
mod sell_quantity;
mod session;

pub use buy_quantity::*;
pub use buy_recipient::*;
pub use item_detail::*;
pub use labels::*;
pub use quantity_picker::*;
pub use row_style::*;
pub use screen_windows::*;
pub use sell_quantity::*;
pub use session::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod buy_row_tests {
    use super::*;

    fn prices(id: u8) -> u16 {
        if id < 0x1A { 0 } else { u16::from(id) * 10 }
    }

    #[test]
    fn the_hoisted_band_leads_and_is_counted() {
        // Ten sellable ids, no padding: the last three hoist.
        let rec: Vec<u8> = (0x30..0x3A).collect();
        let inv = ShopInventory::from_stock_record(1, &rec, true, prices);
        let ids: Vec<u8> = inv.items.iter().map(|i| i.item_id).collect();
        assert_eq!(&ids[..3], &[0x37, 0x38, 0x39]);
        assert_eq!(&ids[3..], &[0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36]);
        assert_eq!(inv.featured_rows, 3);
        assert_eq!(inv.items[0].price, 0x37 * 10);
    }

    #[test]
    fn padding_narrows_the_band_and_no_card_drops_it() {
        // Eight sellable ids plus one template id: band width 2.
        let mut rec: Vec<u8> = (0x30..0x38).collect();
        rec.push(0x03);
        let inv = ShopInventory::from_stock_record(1, &rec, true, prices);
        assert_eq!(inv.featured_rows, 2);
        assert_eq!(inv.items.len(), 8);
        // Without the Platinum Card the last three record entries go.
        let inv = ShopInventory::from_stock_record(1, &rec, false, prices);
        assert_eq!(inv.featured_rows, 0);
        let ids: Vec<u8> = inv.items.iter().map(|i| i.item_id).collect();
        assert_eq!(ids, (0x30..0x36).collect::<Vec<u8>>());
    }

    #[test]
    fn buy_row_ink_is_last_rule_wins() {
        assert_eq!(shop_buy_row_ink(false, 0, 100, 50, false), SHOP_INK_NORMAL);
        assert_eq!(shop_buy_row_ink(false, 0, 10, 50, false), SHOP_INK_GREY);
        assert_eq!(shop_buy_row_ink(false, 99, 100, 50, false), SHOP_INK_GREY);
        // Featured overwrites the dim verdict.
        assert_eq!(shop_buy_row_ink(true, 0, 10, 50, false), SHOP_INK_FEATURED);
        // A parked list skips the dim test.
        assert_eq!(shop_buy_row_ink(false, 0, 10, 50, true), SHOP_INK_NORMAL);
    }
}
