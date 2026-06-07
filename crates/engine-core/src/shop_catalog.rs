//! Per-scene **gold-shop stock**, decoded from the disc.
//!
//! A town merchant's stock list is not an overlay data table - it lives **inline
//! in the scene's field-VM script** (the MAN), as field-VM op `0x49` (`STATE_RESUME`)
//! sub-op `0` carrying `[count][item_ids][ASCII name]`. The shared scanner
//! [`legaia_asset::shop_stock`] locates these records robustly (a byte-scan that
//! survives the dialogue-picker jump tables a linear walk desyncs on); this
//! module pairs it with item prices to populate the engine's
//! [`crate::shop::ShopInventory`] with real per-scene stock.
//!
//! Buy **prices** come from the static `SCUS_942.54` item table (the `u16` at
//! record `+2`, [`legaia_asset::item_names::item_price`]) - the same field the
//! gold-debiting buy handler reads. A price of `0` marks a quest / key /
//! found-only / internal item the game never sells, so the price table doubles
//! as a **sellable mask** (price `> 0`) that guards the shop-record scan. This
//! is a stronger guard than the randomizer's "names a real item" mask: several
//! internal placeholder ids (e.g. the `Ra-Seru Meta $N` slots `0x01..=0x03`)
//! *are* named but priced `0`, so a stray `0x49` run made only of those passes
//! the name mask yet is correctly rejected here - the engine never surfaces a
//! phantom shop or a free item.
//!
//! Nothing here is a Sony byte: the stock ids + prices are decoded from the
//! user's own disc at runtime, exactly like the level-up growth tables and the
//! move-power table. Disc-free builds leave [`crate::world::World::scene_shops`]
//! empty and fall back to host-supplied stock, so determinism oracles are
//! unaffected.

use crate::shop::{ShopInventory, ShopItem};

/// Item buy-price table from `SCUS_942.54`, the gold-shop path's source of both
/// prices and the **sellable mask** (price `> 0`). Built once at boot
/// ([`Self::from_scus`]) and parked on [`crate::world::World::item_shop_data`].
#[derive(Debug, Clone)]
pub struct ShopItemData {
    /// Buy price in gold for each id (`0` = quest / found-only / internal /
    /// not for sale).
    prices: [u16; 256],
}

impl ShopItemData {
    /// Parse the item buy-price table from `SCUS_942.54` bytes. `None` if the
    /// executable / its item table is absent (the id-1 price slot must resolve).
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        // Require a parseable item table (id 1's price slot resolves), matching
        // the randomizer's `sellable_pool` precondition.
        legaia_asset::item_names::price_slot(scus, 1)?;
        let mut prices = [0u16; 256];
        for id in 0u16..=255 {
            prices[id as usize] = legaia_asset::item_names::item_price(scus, id as u8).unwrap_or(0);
        }
        Some(Self { prices })
    }

    /// Buy price in gold for `id` (`0` = not for sale).
    pub fn price(&self, id: u8) -> u16 {
        self.prices[id as usize]
    }

    /// The 256-entry **sellable mask** (id is priced `> 0`) the shop-record scan
    /// uses to reject a non-shop `0x49` payload and any phantom record made of
    /// internal price-`0` ids. Every id in an accepted record is therefore a
    /// real, priced, sellable item.
    pub fn sellable_mask(&self) -> [bool; 256] {
        std::array::from_fn(|id| self.prices[id] > 0)
    }
}

/// One gold shop located in a scene bundle: its on-screen name and the priced
/// stock list the buy UI offers.
#[derive(Debug, Clone)]
pub struct SceneShop {
    /// On-screen shop title (e.g. "Variety Store", "Weapon Shop").
    pub name: String,
    /// The buy list (item id + gold price), in display order.
    pub inventory: ShopInventory,
}

/// Decode every gold shop in one scene-bundle PROT entry.
///
/// `entry_bytes` is the raw PROT entry (the same footprint
/// [`legaia_asset::scene_asset_table`] expects); `entry_idx` is its PROT index,
/// used only as the [`ShopInventory::shop_id`] tag. When `item_data` is supplied
/// the scan is restricted to records whose every id is a **sellable** (priced
/// `> 0`) item - the strongest false-positive guard - and the stock is priced;
/// without it the scan is structural-only and every price is `0`.
///
/// Returns an empty vec when the entry isn't a scene bundle, has no MAN, or has
/// no shop record.
pub fn scene_shops(
    entry_bytes: &[u8],
    entry_idx: usize,
    item_data: Option<&ShopItemData>,
) -> Vec<SceneShop> {
    let mask = item_data.map(|d| d.sellable_mask());
    let Some(sc) = legaia_asset::shop_stock::locate(entry_bytes, mask.as_ref()) else {
        return Vec::new();
    };
    sc.records
        .iter()
        .map(|shop| {
            let items = shop
                .id_offsets
                .iter()
                .map(|&off| {
                    let id = sc.decoded[off];
                    let price = item_data.map(|d| d.price(id) as u32).unwrap_or(0);
                    ShopItem { item_id: id, price }
                })
                .collect();
            SceneShop {
                name: shop.name.clone(),
                inventory: ShopInventory::new(entry_idx as u8, items),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic scene-bundle entry exercises the scan + pricing without a
    /// disc: a MAN body with one op-`0x49` shop record, scanned with a
    /// sellable mask, and the prices flow through to the inventory.
    #[test]
    fn sellable_mask_keeps_priced_record_and_prices_flow() {
        // ids 0x22/0x34 priced (sellable); 0x05 priced 0 (internal).
        let mut prices = [0u16; 256];
        prices[0x22] = 50;
        prices[0x34] = 120;
        let data = ShopItemData { prices };
        let mask = data.sellable_mask();
        assert!(mask[0x22] && mask[0x34] && !mask[0x05]);

        // A minimal MAN: op 0x49, sub-op 0, length 0, count 2, ids, name.
        let mut man = Vec::new();
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x02, 0x22, 0x34]);
        man.extend_from_slice(b"Variety Store\0");
        let records = legaia_asset::shop_stock::scan(&man, Some(&mask));
        assert_eq!(records.len(), 1, "all-priced record is kept");
        assert_eq!(data.price(0x22), 50);
        assert_eq!(data.price(0x34), 120);
    }

    /// A record that contains a price-`0` (internal / non-sellable) id is
    /// rejected by the sellable mask - this is what filters the phantom
    /// `Ra-Seru Meta $N` "shops" out of the engine's stock.
    #[test]
    fn sellable_mask_rejects_record_with_unpriced_id() {
        let mut prices = [0u16; 256];
        prices[0x22] = 50;
        // 0x03 left at price 0 (an internal placeholder id).
        let data = ShopItemData { prices };
        let mut man = Vec::new();
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x02, 0x22, 0x03]);
        man.extend_from_slice(b"Variety Store\0");
        let records = legaia_asset::shop_stock::scan(&man, Some(&data.sellable_mask()));
        assert!(
            records.is_empty(),
            "a record with an unpriced id is rejected"
        );
        // Structural-only (no mask) still finds it - the price gate is the guard.
        assert_eq!(legaia_asset::shop_stock::scan(&man, None).len(), 1);
    }
}
