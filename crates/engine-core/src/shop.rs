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
//! price table) and parked on [`crate::world::World::scene_shops`]; the field-VM
//! op-`0x49` merchant trigger opens one ([`crate::world::World::try_arm_field_shop`]).
//! This module owns only the buy/sell session state. (Disc-free builds leave the
//! stock host-supplied.)

/// One item a shop stocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopItem {
    pub item_id: u8,
    /// Buy price in gold.
    pub price: u32,
}

/// The items a shop offers for purchase. Built from per-scene data; the sell
/// side uses the player's own inventory (at half buy-price or 1 gold minimum).
#[derive(Debug, Clone, Default)]
pub struct ShopInventory {
    /// Opaque identifier tying this stock list to a CDNAME scene block.
    pub shop_id: u8,
    /// Items the shop sells.
    pub items: Vec<ShopItem>,
}

impl ShopInventory {
    pub fn new(shop_id: u8, items: Vec<ShopItem>) -> Self {
        Self { shop_id, items }
    }

    /// Find an item by ID.
    pub fn find(&self, item_id: u8) -> Option<&ShopItem> {
        self.items.iter().find(|i| i.item_id == item_id)
    }

    /// Sell price for an item: half the buy price, minimum 1. Returns 1 for
    /// items not in the shop's buy list (players can always sell any item).
    pub fn sell_price(&self, item_id: u8) -> u32 {
        self.find(item_id)
            .map(|i| (i.price / 2).max(1))
            .unwrap_or(1)
    }
}

/// Mutable session state for an open shop interaction. Installed on
/// [`crate::menu_runtime::MenuRuntime`] by `open_shop` before the menu VM
/// enters `ShopBuy`.
#[derive(Debug, Clone)]
pub struct ShopSession {
    /// The shop's stock list.
    pub inventory: ShopInventory,
    /// Item selected during the current sub-flow.
    pub pending_item_id: Option<u8>,
    /// Quantity chosen at `ShopQuantity` before `ShopConfirm`.
    pub pending_quantity: u8,
    /// `true` = buy (from shop), `false` = sell (from player inventory).
    pub pending_is_buying: bool,
    /// Stable identity of the vendor running this shop (the seru-trade offer
    /// generator keys on it so each vendor trades independently). Derived from
    /// the shop record at field-shop arm time via
    /// [`legaia_asset::seru_trade::vendor_id_from_shop`]; `0` for sessions built
    /// directly (tests / host-supplied stock).
    pub vendor_id: u16,
}

impl ShopSession {
    pub fn new(inventory: ShopInventory) -> Self {
        Self {
            inventory,
            pending_item_id: None,
            pending_quantity: 1,
            pending_is_buying: true,
            vendor_id: 0,
        }
    }

    /// Called when the player confirms an item row in the buy list.
    /// `cursor` indexes into `inventory.items`.
    pub fn select_buy_item(&mut self, cursor: usize) {
        if let Some(item) = self.inventory.items.get(cursor) {
            self.pending_item_id = Some(item.item_id);
            self.pending_quantity = 1;
            self.pending_is_buying = true;
        }
    }

    /// Called when the player confirms an item row in the sell list.
    /// `sell_items` is the sorted `(item_id, count)` slice the menu rendered.
    pub fn select_sell_item(&mut self, cursor: usize, sell_items: &[(u8, u8)]) {
        if let Some(&(item_id, _)) = sell_items.get(cursor) {
            self.pending_item_id = Some(item_id);
            self.pending_quantity = 1;
            self.pending_is_buying = false;
        }
    }

    /// Called when the player picks a quantity at `ShopQuantity`.
    /// `slot` is the cursor value (0-based); quantity = `slot + 1`.
    pub fn set_quantity(&mut self, slot: u8) {
        self.pending_quantity = slot.saturating_add(1);
    }

    /// Attempt to execute the pending buy. Returns `(item_id, qty, gold_delta)`
    /// where `gold_delta` is negative (the cost to deduct). Returns `None` if
    /// the player cannot afford it, no item is pending, or we're in sell mode.
    pub fn try_buy(&self, world_money: i32) -> Option<(u8, u8, i32)> {
        if !self.pending_is_buying {
            return None;
        }
        let item_id = self.pending_item_id?;
        let item = self.inventory.find(item_id)?;
        let cost = (item.price as i64) * (self.pending_quantity as i64);
        if world_money < cost as i32 {
            return None;
        }
        Some((item_id, self.pending_quantity, -(cost as i32)))
    }

    /// Attempt to execute the pending sell. `held_count` is how many of the
    /// pending item the player currently holds. Returns `(item_id, qty,
    /// gold_delta)` where `gold_delta` is positive (proceeds to add). Returns
    /// `None` if in buy mode, no item is pending, or the player holds none.
    pub fn try_sell(&self, held_count: u8) -> Option<(u8, u8, i32)> {
        if self.pending_is_buying {
            return None;
        }
        let item_id = self.pending_item_id?;
        let qty = self.pending_quantity.min(held_count);
        if qty == 0 {
            return None;
        }
        let unit = self.inventory.sell_price(item_id) as i32;
        Some((item_id, qty, unit * qty as i32))
    }

    /// Number of items the buy list contains (caps at 255).
    pub fn buy_item_count(&self) -> u8 {
        self.inventory.items.len().min(u8::MAX as usize) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_items() -> ShopSession {
        ShopSession::new(ShopInventory::new(
            1,
            vec![
                ShopItem {
                    item_id: 10,
                    price: 100,
                },
                ShopItem {
                    item_id: 20,
                    price: 200,
                },
            ],
        ))
    }

    #[test]
    fn sell_price_half_buy_rounded_down() {
        let inv = ShopInventory::new(
            0,
            vec![ShopItem {
                item_id: 1,
                price: 101,
            }],
        );
        assert_eq!(inv.sell_price(1), 50);
    }

    #[test]
    fn sell_price_unknown_item_is_one() {
        let inv = ShopInventory::new(0, vec![]);
        assert_eq!(inv.sell_price(99), 1);
    }

    #[test]
    fn select_buy_item_sets_pending() {
        let mut s = session_with_items();
        s.select_buy_item(1);
        assert_eq!(s.pending_item_id, Some(20));
        assert!(s.pending_is_buying);
    }

    #[test]
    fn select_buy_item_out_of_range_is_noop() {
        let mut s = session_with_items();
        s.select_buy_item(99);
        assert!(s.pending_item_id.is_none());
    }

    #[test]
    fn select_sell_item_sets_pending() {
        let mut s = session_with_items();
        let inventory = vec![(10u8, 3u8), (20, 1)];
        s.select_sell_item(0, &inventory);
        assert_eq!(s.pending_item_id, Some(10));
        assert!(!s.pending_is_buying);
    }

    #[test]
    fn set_quantity_slot_plus_one() {
        let mut s = session_with_items();
        s.set_quantity(2); // cursor 2 → 3 items
        assert_eq!(s.pending_quantity, 3);
    }

    #[test]
    fn try_buy_deducts_correct_amount() {
        let mut s = session_with_items();
        s.select_buy_item(0); // item 10, price 100
        s.set_quantity(2); // 3 items
        let result = s.try_buy(500);
        assert_eq!(result, Some((10, 3, -300)));
    }

    #[test]
    fn try_buy_fails_when_insufficient_gold() {
        let mut s = session_with_items();
        s.select_buy_item(0); // price 100
        s.set_quantity(4); // 5 × 100 = 500
        assert!(s.try_buy(499).is_none());
    }

    #[test]
    fn try_buy_fails_when_in_sell_mode() {
        let mut s = session_with_items();
        s.select_sell_item(0, &[(10, 5)]);
        assert!(s.try_buy(9999).is_none());
    }

    #[test]
    fn try_sell_returns_proceeds() {
        let mut s = session_with_items();
        s.select_sell_item(0, &[(10, 5)]);
        s.set_quantity(1); // 2 items (slot 1 → qty 2)
        let result = s.try_sell(5);
        // sell price of item 10 (buy 100) = 50; qty 2 → 100
        assert_eq!(result, Some((10, 2, 100)));
    }

    #[test]
    fn try_sell_clamps_to_held_count() {
        let mut s = session_with_items();
        s.select_sell_item(0, &[(10, 1)]);
        s.set_quantity(8); // want 9 but only hold 1
        let result = s.try_sell(1);
        assert_eq!(result.map(|(_, q, _)| q), Some(1));
    }

    #[test]
    fn try_sell_fails_when_hold_zero() {
        let mut s = session_with_items();
        s.select_sell_item(0, &[(10, 1)]);
        assert!(s.try_sell(0).is_none());
    }

    #[test]
    fn buy_item_count_reflects_inventory_length() {
        let s = session_with_items();
        assert_eq!(s.buy_item_count(), 2);
    }
}
