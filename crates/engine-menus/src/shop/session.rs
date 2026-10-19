//! Shop stock and the buy / sell session state.

use super::*;

/// Max held count of one item id - a stack fills at **99**. Two retail
/// gates both carry the `0x63` literal: the buy-list row builder dims a
/// row once the held count stops being `< 0x63` (`sltiu v0,v0,0x63` at
/// `0x80030f0c` / `ori s0,s0,0x800` at `0x80030f18`, shop-row case of
/// `FUN_80030628`; recomp-corroborated), and the buy-quantity maximum
/// clamps to `0x63 - held` (`li a0,0x63; subu a0,a0,v1` at
/// `0x801db8d0..0x801db8dc` in `FUN_801DB7F4`) so a buy can top the
/// stack off at exactly 99. Enforced by
/// `legaia_engine_core::world::World::buy_from_shop`, which knows the live inventory.
pub const SHOP_HELD_CAP: u8 = 99;

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
    /// Items the shop sells, in the retail **row** order.
    pub items: Vec<ShopItem>,
    /// How many leading rows of [`Self::items`] are the builder's hoisted
    /// band - the record's last entries, emitted first and tagged
    /// [`crate::menu_list_rows::CLASS_SHOP_ALT`], which the list kernel draws
    /// in ink [`SHOP_INK_FEATURED`] (see [`shop_buy_row_ink`]). `0` for a
    /// hand-built inventory.
    pub featured_rows: usize,
}

impl ShopInventory {
    pub fn new(shop_id: u8, items: Vec<ShopItem>) -> Self {
        Self {
            shop_id,
            items,
            featured_rows: 0,
        }
    }

    /// Build the buy list the way the list builder does, from the stock
    /// record's own id run (`[+3 ..]`, padding included, record order).
    ///
    /// Runs [`crate::menu_list_rows::build_shop_buy_rows`] for the row order,
    /// the conditional tail and the hoisted band's class, and prices each row
    /// from `price_of`. The dim bit the builder also computes is left to the
    /// draw ([`shop_buy_row_ink`]): it is a function of the live purse and
    /// held counts, which a buy changes while the list stays open, and retail
    /// rebuilds the rows on each content refresh.
    ///
    /// REF: FUN_80030628 (case `0x0B`; the port is `build_shop_buy_rows`)
    pub fn from_stock_record(
        shop_id: u8,
        record_ids: &[u8],
        tail_rows_allowed: bool,
        price_of: impl Fn(u8) -> u16,
    ) -> Self {
        let words = crate::menu_list_rows::build_shop_buy_rows(
            record_ids,
            tail_rows_allowed,
            u32::MAX,
            &price_of,
            |_| 0,
        );
        let featured_rows = words
            .iter()
            .take_while(|&&w| {
                w & crate::menu_list_rows::ROW_CLASS_MASK == crate::menu_list_rows::CLASS_SHOP_ALT
            })
            .count();
        let items = words
            .iter()
            .map(|&w| {
                let item_id = (w & 0xFF) as u8;
                ShopItem {
                    item_id,
                    price: u32::from(price_of(item_id)),
                }
            })
            .collect();
        Self {
            shop_id,
            items,
            featured_rows,
        }
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
/// `legaia_engine_menu_runtime::menu_runtime::MenuRuntime` by `open_shop` before the menu VM
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
    /// Physical **bag slot** the staged sell row named, when the host built the
    /// row list from the bag's slots.
    ///
    /// Retail's list rows carry a slot, not an id (`_DAT_8007BB88` is a bag
    /// slot index), and the bag is not compacted when a menu opens, so the row
    /// ordinal and the slot diverge as soon as a hole sits above the selection.
    /// `None` for a host that staged the row by id alone, which then removes
    /// from whichever slot the window scan reaches first.
    pub pending_bag_slot: Option<u8>,
    /// Stable identity of the vendor running this shop. Derived from
    /// the shop record at field-shop arm time via
    /// [`legaia_game_tables::seru_trade::vendor_id_from_shop`]; `0` for sessions built
    /// directly (tests / host-supplied stock).
    pub vendor_id: u16,
    /// The vendor's phase offset into the seru-trade bucket schedule
    /// ([`legaia_game_tables::seru_trade::vendor_bucket_offset`], the same
    /// count+ids+name sum the retail handler takes from the armed op-`0x49`
    /// record) - each trader shows its own offer. `0` for sessions built
    /// directly (tests / host-supplied stock).
    pub vendor_bucket_offset: u8,
}

impl ShopSession {
    pub fn new(inventory: ShopInventory) -> Self {
        Self {
            inventory,
            pending_item_id: None,
            pending_quantity: 1,
            pending_is_buying: true,
            pending_bag_slot: None,
            vendor_id: 0,
            vendor_bucket_offset: 0,
        }
    }

    /// How many quantities the `ShopQuantity` screen offers for whatever this
    /// session has staged (cursor `n` = quantity `n + 1`).
    ///
    /// `held` is the bag's count of the staged item - the buy side's
    /// `99 - held` clamp and the sell side's whole bound. One kernel so the
    /// menu runtime's row count and both hosts' row lists cannot disagree
    /// about how many copies retail would let the player take.
    pub fn quantity_rows(&self, gold: i32, held: Option<u8>) -> u8 {
        let Some(id) = self.pending_item_id else {
            return 1;
        };
        let price = self
            .inventory
            .find(id)
            .map(|i| u16::try_from(i.price).unwrap_or(u16::MAX))
            .unwrap_or(0);
        quantity_row_count(self.pending_is_buying, gold, price, held, held)
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
    /// `sell_items` is the `(item_id, count)` slice the menu rendered, in the
    /// order it drew.
    pub fn select_sell_item(&mut self, cursor: usize, sell_items: &[(u8, u8)]) {
        if let Some(&(item_id, _)) = sell_items.get(cursor) {
            self.pending_item_id = Some(item_id);
            self.pending_quantity = 1;
            self.pending_is_buying = false;
            self.pending_bag_slot = None;
        }
    }

    /// [`Self::select_sell_item`] for a host whose rows carry their bag slot -
    /// `rows` is the drawn list as `(bag slot, item id)`.
    ///
    /// Staging the slot is what makes the sale take the stack the player
    /// pointed at. Without it the commit removes by id, which is the same slot
    /// only while no hole sits above the selection and no second stack of that
    /// id exists.
    pub fn select_sell_row(&mut self, cursor: usize, rows: &[(u8, u8)]) {
        if let Some(&(slot, item_id)) = rows.get(cursor) {
            self.pending_item_id = Some(item_id);
            self.pending_quantity = 1;
            self.pending_is_buying = false;
            self.pending_bag_slot = Some(slot);
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
