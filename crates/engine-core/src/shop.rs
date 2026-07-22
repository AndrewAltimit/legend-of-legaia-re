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

/// Max held count of one item id before further buys refuse (retail dims buy
/// attempts past 98 held; enforced by
/// [`crate::world::World::buy_from_shop`], which knows the live inventory).
pub const SHOP_HELD_CAP: u8 = 98;

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

/// Retail gold cap enforced when a sale credits the purse (`0x98967F` at
/// `0x801dbec4..0x801dbeec`).
pub const GOLD_CAP: i32 = 9_999_999;

/// Frame-delta accumulation threshold of the post-sale exit delay
/// (`FUN_801DBD94` phase 2, `slti 0x11` at `0x801dc194`).
pub const SELL_QTY_EXIT_DELAY: i32 = 0x11;

/// The retail sell credit: `(item_record_price * qty) >> 1` - half the
/// item table's price halfword, floored (`mult` + `sra 1` at
/// `0x801dbec0..0x801dbed4`). The price is the *item table's* (`+0x2` of
/// the 12-byte `0x80074368` record), not a per-shop price.
pub fn sell_credit(price: u16, qty: i32) -> i32 {
    (price as i32 * qty) >> 1
}

/// Post-sale purse update: add the credit, clamp to [`GOLD_CAP`].
pub fn apply_sale_gold(gold: i32, credit: i32) -> i32 {
    (gold + credit).min(GOLD_CAP)
}

/// The post-sale sell-list scroll fix-up (`0x801dbef0..0x801dbf4c`):
/// when the sold row is the **last** row and sits alone at the top of
/// the final page (`selected == count-1 && selected == scroll_top &&
/// selected > 0`), the selection steps back one and the scroll steps
/// back one page (retail reads `visible_rows` off live window `0x26`'s
/// list node). Applied on every confirmed sale, whole-stack or not -
/// the condition, not the stack size, is the gate.
pub fn sell_list_fixup(
    sel: &mut crate::menu_list_rows::ListSelection,
    row_count: i32,
    visible_rows: i32,
) {
    if sel.selected == row_count - 1 && sel.selected == sel.scroll_top && sel.selected > 0 {
        sel.selected = row_count - 2;
        sel.scroll_top -= visible_rows;
    }
}

/// The shared quantity-picker pad decode (identical instruction shapes
/// in `FUN_801DBD94` at `0x801dc034..0x801dc178` and `FUN_801DB7F4` at
/// `0x801db980..0x801dbac4`): Right/Left step by one, Down/Up by ten,
/// clamped to `[1, max]`; a step off either end is a silent no-op (the
/// retail gates are `qty < max` for the increments and `qty >= 2` for
/// the decrements, checked before the SFX fires). Returns the new
/// quantity, or `None` when nothing moved.
fn quantity_step(pressed: u16, qty: i32, max: i32) -> Option<i32> {
    use crate::input::PadButton;
    if pressed & PadButton::Right.mask() != 0 && qty < max {
        return Some(qty + 1);
    }
    if pressed & PadButton::Left.mask() != 0 && qty >= 2 {
        return Some(qty - 1);
    }
    if pressed & PadButton::Down.mask() != 0 && qty < max {
        return Some((qty + 10).min(max));
    }
    if pressed & PadButton::Up.mask() != 0 && qty >= 2 {
        return Some((qty - 10).max(1));
    }
    None
}

/// What a [`SellQuantitySession`] frame produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SellQtyEvent {
    None,
    /// Quantity moved (retail cues SFX `0x21`).
    Moved,
    /// Sale confirmed (SFX `0x36`): consume `qty` copies of `item_id`
    /// and credit [`sell_credit`] gold (clamped via [`apply_sale_gold`]).
    /// A whole-stack sale expects a [`SellQuantitySession::finish_whole_stack`]
    /// call with the post-sale bag-rescan result.
    Sold {
        item_id: u8,
        qty: u8,
        credit: i32,
    },
    /// Backed out (SFX `0x37`) - the session is done, back to the sell
    /// list.
    Cancelled,
    /// Session finished: return to the sell list (partial sale, or a
    /// whole-stack sale with items left in the bag).
    ExitToSellList,
    /// Session finished after the exit delay: the sale emptied the bag -
    /// return to the shop root menu.
    ExitToShopRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SellQtyPhase {
    /// Retail phase 0: staging + window script.
    Init,
    /// Retail phase 1: pad-driven quantity edit.
    Interactive,
    /// Post-sale: the next tick returns to the sell list.
    AfterSale,
    /// Retail phase 2: exit-delay accumulator before the shop root.
    ExitDelay,
    Done,
}

/// PORT: FUN_801DBD94 (menu-overlay sub-screen `0x1F` - the shop-sell
/// quantity picker; `see ghidra/scripts/funcs/overlay_menu_801dbd94.txt`).
///
/// Phase 0 seeds quantity 1 and reads the staged bag slot's count as the
/// maximum (`0x80085959 + slot*2` -> `DAT_801E46B8`), hides the list
/// hand (cursor `|= 0x1000`) and parks the kernel. Phase 1 (gated on
/// the window-slide latch) maps the pad edges: Right/Left step the
/// quantity by one, Down/Up by ten, clamped to `[1, max]` - each move
/// gated so a step off either end is a silent no-op (`0x801dc034..
/// 0x801dc178`). Confirm consumes the quantity from the bag
/// (`FUN_80042310`), credits half the item-table price per copy
/// ([`sell_credit`], purse clamp [`GOLD_CAP`]) and applies the
/// [`sell_list_fixup`]; a whole-stack sale rescans the bag (a slot
/// counts only when both id and count bytes are non-zero) and, when
/// empty, runs the ~17-unit exit delay (phase 2) before returning to
/// the shop root instead of the sell list.
///
/// NOT WIRED: the menu runtime's quantity flow
/// ([`ShopSession::set_quantity`]) still drives the hosts; this session
/// is the retail-shaped replacement.
#[derive(Debug, Clone)]
pub struct SellQuantitySession {
    pub item_id: u8,
    /// Item-record price halfword (`0x80074368 + id*0xC + 2`).
    pub price: u16,
    /// Current quantity (`DAT_801E46B4`), seeded 1.
    pub qty: i32,
    /// Staged bag slot's held count (`DAT_801E46B8`).
    pub max: i32,
    phase: SellQtyPhase,
    /// Exit-delay accumulator (`DAT_801E46D0` reuse).
    delay: i32,
}

impl SellQuantitySession {
    pub fn new(item_id: u8, price: u16, held_count: u8) -> Self {
        Self {
            item_id,
            price,
            qty: 1,
            max: held_count as i32,
            phase: SellQtyPhase::Init,
            delay: 0,
        }
    }

    /// Drive one frame. `pressed` is the edge-triggered pad word;
    /// `frame_delta` the scratchpad frame-delta units (1 per vsync at
    /// 60 Hz) consumed by the exit delay.
    pub fn tick(&mut self, pressed: u16, frame_delta: i32) -> SellQtyEvent {
        use crate::input::PadButton;
        match self.phase {
            SellQtyPhase::Init => {
                // Phase 0: staging + window script; interactive next
                // frame.
                self.qty = 1;
                self.phase = SellQtyPhase::Interactive;
                SellQtyEvent::None
            }
            SellQtyPhase::Interactive => {
                if pressed & PadButton::Cross.mask() != 0 {
                    let qty = self.qty.clamp(1, self.max.max(1)) as u8;
                    let credit = sell_credit(self.price, qty as i32);
                    // A whole-stack sale waits for the caller's bag
                    // rescan (`finish_whole_stack`); a partial sale
                    // returns to the sell list next tick.
                    self.phase = SellQtyPhase::AfterSale;
                    self.delay = 0;
                    return SellQtyEvent::Sold {
                        item_id: self.item_id,
                        qty,
                        credit,
                    };
                }
                if pressed & (PadButton::Circle.mask() | PadButton::Triangle.mask()) != 0 {
                    self.phase = SellQtyPhase::Done;
                    return SellQtyEvent::Cancelled;
                }
                if let Some(q) = quantity_step(pressed, self.qty, self.max) {
                    self.qty = q;
                    return SellQtyEvent::Moved;
                }
                SellQtyEvent::None
            }
            SellQtyPhase::AfterSale => {
                self.phase = SellQtyPhase::Done;
                SellQtyEvent::ExitToSellList
            }
            SellQtyPhase::ExitDelay => {
                self.delay += frame_delta;
                if self.delay >= SELL_QTY_EXIT_DELAY {
                    self.phase = SellQtyPhase::Done;
                    SellQtyEvent::ExitToShopRoot
                } else {
                    SellQtyEvent::None
                }
            }
            SellQtyPhase::Done => SellQtyEvent::None,
        }
    }

    /// Route a whole-stack sale by the caller's bag rescan result
    /// (retail: a slot counts only when both its id and count bytes are
    /// non-zero). An empty bag enters the exit-delay phase; otherwise
    /// the next [`Self::tick`] returns to the sell list.
    pub fn finish_whole_stack(&mut self, bag_empty: bool) {
        if bag_empty {
            self.phase = SellQtyPhase::ExitDelay;
            self.delay = 0;
        } else {
            self.phase = SellQtyPhase::AfterSale;
        }
    }

    /// Session left the quantity screen.
    pub fn is_done(&self) -> bool {
        self.phase == SellQtyPhase::Done
    }
}

/// Retail per-stack held cap the buy quantity clamps against (`0x63` at
/// `0x801db8a4..0x801db8ec`).
pub const BUY_QTY_CAP: i32 = 99;

/// The Point Card id whose held-count gates the accrual
/// (`FUN_80042F4C(0xFE)` at `0x801dbac8`).
pub const POINT_CARD_ITEM_ID: u8 = 0xFE;

/// Point Card cap - same `0x98967F` constant as the purse.
pub const POINT_CARD_CAP: i32 = 9_999_999;

/// Point Card accrual for one buy commit: `(price / 20) * qty` (the
/// `0xCCCCCCCD` reciprocal-multiply + `srl 4`, truncated to 16 bits,
/// times the quantity - `0x801dbadc..0x801dbb3c`). Credited **before**
/// the gold debit, only while the party holds item `0xFE`.
pub fn point_card_credit(price: u16, qty: i32) -> i32 {
    (price as i32 / 20) * qty
}

/// Post-buy Point Card counter update, clamped to [`POINT_CARD_CAP`].
pub fn apply_point_card(points: i32, credit: i32) -> i32 {
    (points + credit).min(POINT_CARD_CAP)
}

/// The buy-quantity maximum (`FUN_801DB7F4` phase 0): `gold / price`,
/// clamped to [`BUY_QTY_CAP`], further clamped to `99 - held` when the
/// bag already holds a stack of the item (`FUN_80042EE0` slot scan at
/// `0x801db8ac..0x801db8ec`). The price is the item table's halfword -
/// retail shop buys carry no per-shop price.
pub fn buy_qty_max(gold: i32, price: u16, held: Option<u8>) -> i32 {
    let mut max = (gold / (price as i32).max(1)).min(BUY_QTY_CAP);
    if let Some(held) = held {
        max = max.min(BUY_QTY_CAP - held as i32);
    }
    max
}

/// What a [`BuyQuantitySession`] frame produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyQtyEvent {
    None,
    /// Quantity moved (SFX `0x21`).
    Moved,
    /// Purchase committed (`FUN_800421D4(id, qty)` + gold debit):
    /// consume `cost` gold, add `qty` copies, and - when
    /// `point_credit > 0` - credit the Point Card counter (the credit is
    /// non-zero only while the caller reported the Point Card held).
    Bought {
        item_id: u8,
        qty: u8,
        cost: i32,
        point_credit: i32,
    },
    /// Backed out (SFX `0x37`) - return to the buy list.
    Cancelled,
    /// Session finished - back to the buy list (submenu `0x1B`).
    ExitToBuyList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuyQtyPhase {
    /// Retail phase 0: max derivation + window script.
    Init,
    /// Retail phase 1: pad-driven quantity edit.
    Interactive,
    /// Retail phases 2+3: Point Card accrual + commit (collapsed - the
    /// retail split is a one-frame window-script beat).
    Commit,
    /// Retail phase 4: the Point Card toast waits for a confirm/cancel
    /// press before returning to the list.
    ToastWait,
    /// Exit next tick.
    Exit,
    Done,
}

/// PORT: FUN_801DB7F4 (menu-overlay shop **buy quantity + commit**
/// sub-screen; `see ghidra/scripts/funcs/overlay_menu_801db7f4.txt`).
///
/// Phase 0 derives the quantity maximum ([`buy_qty_max`]) and opens the
/// picker window. Phase 1 maps the pad exactly like the sell picker
/// (shared [`quantity_step`] decode; confirm cues SFX `0x2C`, cancel
/// `0x37`). The commit credits the Point Card **before** the gold debit
/// ([`point_card_credit`], gated on holding item `0xFE`), adds the
/// stack, debits `price * qty`, and - only when the Point Card toast
/// was shown - waits for a button press (SFX `0x20`) before dropping
/// back to the buy list.
///
/// NOT WIRED: the menu runtime's quantity flow
/// ([`ShopSession::set_quantity`] + `World::buy_from_shop`) still
/// drives the hosts; this session is the retail-shaped replacement.
#[derive(Debug, Clone)]
pub struct BuyQuantitySession {
    pub item_id: u8,
    /// Item-record price halfword.
    pub price: u16,
    /// Current quantity (`DAT_801E46B4`), seeded 1.
    pub qty: i32,
    /// Derived maximum (`DAT_801E46B8`).
    pub max: i32,
    /// Party holds the Point Card (item `0xFE`).
    pub point_card_held: bool,
    phase: BuyQtyPhase,
}

impl BuyQuantitySession {
    pub fn new(
        item_id: u8,
        price: u16,
        gold: i32,
        held: Option<u8>,
        point_card_held: bool,
    ) -> Self {
        Self {
            item_id,
            price,
            qty: 1,
            max: buy_qty_max(gold, price, held),
            point_card_held,
            phase: BuyQtyPhase::Init,
        }
    }

    /// Drive one frame from the edge-triggered pad word.
    pub fn tick(&mut self, pressed: u16) -> BuyQtyEvent {
        use crate::input::PadButton;
        match self.phase {
            BuyQtyPhase::Init => {
                self.qty = 1;
                self.phase = BuyQtyPhase::Interactive;
                BuyQtyEvent::None
            }
            BuyQtyPhase::Interactive => {
                if pressed & PadButton::Cross.mask() != 0 {
                    self.phase = BuyQtyPhase::Commit;
                    return BuyQtyEvent::None;
                }
                if pressed & (PadButton::Circle.mask() | PadButton::Triangle.mask()) != 0 {
                    self.phase = BuyQtyPhase::Done;
                    return BuyQtyEvent::Cancelled;
                }
                if let Some(q) = quantity_step(pressed, self.qty, self.max) {
                    self.qty = q;
                    return BuyQtyEvent::Moved;
                }
                BuyQtyEvent::None
            }
            BuyQtyPhase::Commit => {
                let qty = self.qty.clamp(1, self.max.max(1));
                let point_credit = if self.point_card_held {
                    point_card_credit(self.price, qty)
                } else {
                    0
                };
                self.phase = if self.point_card_held {
                    BuyQtyPhase::ToastWait
                } else {
                    BuyQtyPhase::Exit
                };
                BuyQtyEvent::Bought {
                    item_id: self.item_id,
                    qty: qty as u8,
                    cost: self.price as i32 * qty,
                    point_credit,
                }
            }
            BuyQtyPhase::ToastWait => {
                if pressed
                    & (PadButton::Cross.mask()
                        | PadButton::Circle.mask()
                        | PadButton::Triangle.mask())
                    != 0
                {
                    self.phase = BuyQtyPhase::Done;
                    BuyQtyEvent::ExitToBuyList
                } else {
                    BuyQtyEvent::None
                }
            }
            BuyQtyPhase::Exit => {
                self.phase = BuyQtyPhase::Done;
                BuyQtyEvent::ExitToBuyList
            }
            BuyQtyPhase::Done => BuyQtyEvent::None,
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == BuyQtyPhase::Done
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

    use crate::input::PadButton;
    use crate::menu_list_rows::ListSelection;

    #[test]
    fn sell_credit_is_half_price_floored() {
        assert_eq!(sell_credit(100, 2), 100);
        // Odd product floors: 15 * 3 = 45 -> 22.
        assert_eq!(sell_credit(15, 3), 22);
        assert_eq!(sell_credit(1, 1), 0);
    }

    #[test]
    fn apply_sale_gold_clamps_at_cap() {
        assert_eq!(apply_sale_gold(100, 50), 150);
        assert_eq!(apply_sale_gold(GOLD_CAP - 10, 50), GOLD_CAP);
    }

    #[test]
    fn sell_quantity_pad_steps_and_clamps() {
        let mut s = SellQuantitySession::new(0x40, 100, 25);
        assert_eq!(s.tick(0, 1), SellQtyEvent::None); // init frame
        let right = PadButton::Right.mask();
        let left = PadButton::Left.mask();
        let down = PadButton::Down.mask();
        let up = PadButton::Up.mask();
        assert_eq!(s.tick(right, 1), SellQtyEvent::Moved);
        assert_eq!(s.qty, 2);
        assert_eq!(s.tick(down, 1), SellQtyEvent::Moved);
        assert_eq!(s.qty, 12);
        // +10 clamps to max.
        assert_eq!(s.tick(down, 1), SellQtyEvent::Moved);
        assert_eq!(s.tick(down, 1), SellQtyEvent::Moved);
        assert_eq!(s.qty, 25);
        // At max, +1 is a silent no-op.
        assert_eq!(s.tick(right, 1), SellQtyEvent::None);
        // -10 floors at 1.
        assert_eq!(s.tick(up, 1), SellQtyEvent::Moved);
        assert_eq!(s.tick(up, 1), SellQtyEvent::Moved);
        assert_eq!(s.tick(up, 1), SellQtyEvent::Moved);
        assert_eq!(s.qty, 1);
        // At 1, -1 and -10 are silent no-ops (retail gates on qty >= 2).
        assert_eq!(s.tick(left, 1), SellQtyEvent::None);
        assert_eq!(s.tick(up, 1), SellQtyEvent::None);
    }

    #[test]
    fn sell_quantity_partial_sale_returns_to_sell_list() {
        let mut s = SellQuantitySession::new(0x40, 100, 5);
        s.tick(0, 1);
        s.tick(PadButton::Right.mask(), 1); // qty 2
        let ev = s.tick(PadButton::Cross.mask(), 1);
        assert_eq!(
            ev,
            SellQtyEvent::Sold {
                item_id: 0x40,
                qty: 2,
                credit: 100
            }
        );
        assert_eq!(s.tick(0, 1), SellQtyEvent::ExitToSellList);
        assert!(s.is_done());
    }

    #[test]
    fn sell_quantity_whole_stack_empty_bag_delays_to_shop_root() {
        let mut s = SellQuantitySession::new(0x40, 100, 1);
        s.tick(0, 1);
        let ev = s.tick(PadButton::Cross.mask(), 1);
        assert_eq!(
            ev,
            SellQtyEvent::Sold {
                item_id: 0x40,
                qty: 1,
                credit: 50
            }
        );
        s.finish_whole_stack(true);
        // 17 frame-delta units accumulate before the exit fires.
        for _ in 0..(SELL_QTY_EXIT_DELAY - 1) {
            assert_eq!(s.tick(0, 1), SellQtyEvent::None);
        }
        assert_eq!(s.tick(0, 1), SellQtyEvent::ExitToShopRoot);
    }

    #[test]
    fn sell_quantity_whole_stack_with_items_left_returns_to_list() {
        let mut s = SellQuantitySession::new(0x40, 100, 1);
        s.tick(0, 1);
        s.tick(PadButton::Cross.mask(), 1);
        s.finish_whole_stack(false);
        assert_eq!(s.tick(0, 1), SellQtyEvent::ExitToSellList);
    }

    #[test]
    fn sell_quantity_cancel() {
        let mut s = SellQuantitySession::new(0x40, 100, 5);
        s.tick(0, 1);
        assert_eq!(s.tick(PadButton::Circle.mask(), 1), SellQtyEvent::Cancelled);
        assert!(s.is_done());
    }

    #[test]
    fn point_card_credit_is_five_percent_per_unit() {
        // floor(price / 20) per unit, times qty.
        assert_eq!(point_card_credit(100, 3), 15);
        assert_eq!(point_card_credit(19, 5), 0); // < 20 gold earns nothing
        assert_eq!(point_card_credit(59, 2), 4); // floor(59/20) = 2 per unit
        assert_eq!(apply_point_card(POINT_CARD_CAP - 3, 10), POINT_CARD_CAP);
    }

    #[test]
    fn buy_qty_max_laws() {
        // gold / price, capped at 99.
        assert_eq!(buy_qty_max(1000, 100, None), 10);
        assert_eq!(buy_qty_max(100_000, 100, None), 99);
        // Held stack tightens the cap to 99 - held.
        assert_eq!(buy_qty_max(100_000, 100, Some(95)), 4);
        // Poorer than one copy -> 0.
        assert_eq!(buy_qty_max(50, 100, None), 0);
    }

    #[test]
    fn buy_quantity_commit_with_point_card_waits_for_toast() {
        let mut s = BuyQuantitySession::new(0x40, 100, 10_000, None, true);
        assert_eq!(s.tick(0), BuyQtyEvent::None); // init
        assert_eq!(s.tick(PadButton::Right.mask()), BuyQtyEvent::Moved);
        assert_eq!(s.qty, 2);
        assert_eq!(s.tick(PadButton::Cross.mask()), BuyQtyEvent::None);
        assert_eq!(
            s.tick(0),
            BuyQtyEvent::Bought {
                item_id: 0x40,
                qty: 2,
                cost: 200,
                point_credit: 10,
            }
        );
        // Point Card toast holds until a button press.
        assert_eq!(s.tick(0), BuyQtyEvent::None);
        assert_eq!(s.tick(PadButton::Cross.mask()), BuyQtyEvent::ExitToBuyList);
        assert!(s.is_done());
    }

    #[test]
    fn buy_quantity_commit_without_point_card_exits_directly() {
        let mut s = BuyQuantitySession::new(0x40, 100, 10_000, None, false);
        s.tick(0);
        s.tick(PadButton::Cross.mask());
        assert_eq!(
            s.tick(0),
            BuyQtyEvent::Bought {
                item_id: 0x40,
                qty: 1,
                cost: 100,
                point_credit: 0,
            }
        );
        assert_eq!(s.tick(0), BuyQtyEvent::ExitToBuyList);
    }

    #[test]
    fn sell_list_fixup_last_row_alone_on_final_page() {
        // Selling with the hand on the last row, alone at the top of the
        // final page: selection steps back one, scroll steps back a page.
        let mut sel = ListSelection {
            scroll_top: 12,
            selected: 12,
        };
        sell_list_fixup(&mut sel, 13, 12);
        assert_eq!((sel.scroll_top, sel.selected), (0, 11));
        // Not the last row -> untouched.
        let mut sel = ListSelection {
            scroll_top: 12,
            selected: 12,
        };
        sell_list_fixup(&mut sel, 14, 12);
        assert_eq!((sel.scroll_top, sel.selected), (12, 12));
        // Last row but not at the page top -> untouched.
        let mut sel = ListSelection {
            scroll_top: 12,
            selected: 13,
        };
        sell_list_fixup(&mut sel, 14, 12);
        assert_eq!((sel.scroll_top, sel.selected), (12, 13));
        // Row 0 never steps back.
        let mut sel = ListSelection {
            scroll_top: 0,
            selected: 0,
        };
        sell_list_fixup(&mut sel, 1, 12);
        assert_eq!((sel.scroll_top, sel.selected), (0, 0));
    }
}
