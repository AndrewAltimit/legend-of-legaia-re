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

/// Max held count of one item id - a stack fills at **99**. Two retail
/// gates both carry the `0x63` literal: the buy-list row builder dims a
/// row once the held count stops being `< 0x63` (`sltiu v0,v0,0x63` at
/// `0x80030f0c` / `ori s0,s0,0x800` at `0x80030f18`, shop-row case of
/// `FUN_80030628`; recomp-corroborated), and the buy-quantity maximum
/// clamps to `0x63 - held` (`li a0,0x63; subu a0,a0,v1` at
/// `0x801db8d0..0x801db8dc` in `FUN_801DB7F4`) so a buy can top the
/// stack off at exactly 99. Enforced by
/// [`crate::world::World::buy_from_shop`], which knows the live inventory.
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

/// Where a confirmed buy-list row routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyListRoute {
    /// Gold short of the item price: buzz SFX `0x23` and stay on the
    /// list (retail re-arms list mode 1).
    Refused,
    /// Equipment (item kind `1`): the buy-recipient picker (retail
    /// sub-screen `0x1C` = `FUN_801DB380`).
    RecipientPicker,
    /// Stackable (item kind `2`): the buy-quantity picker (retail
    /// sub-screen `0x1D` = `FUN_801DB7F4`).
    QuantityPicker,
    /// Any other kind byte falls back to the Buy/Sell/Quit mode select
    /// (retail sub-screen `0x1A` - the default store the kind tests
    /// overwrite).
    ModeSelect,
}

/// Route a confirmed buy-list row - the shop buy-list sub-screen's
/// state-2 confirm dispatch. The affordability test reads the item
/// record's price halfword against the purse (`0x8008459C`); the kind
/// byte (item record `+0`) picks the follow-up screen.
///
/// PORT: FUN_801DB21C (menu-overlay sub-screen `0x1B`, the shop buy
/// list; see `ghidra/scripts/funcs/overlay_menu_801db21c.txt` -
/// `slt gold, price` + buzz at `0x801db314..0x801db328`, the kind
/// dispatch at `0x801db334..0x801db364`)
pub fn buy_list_confirm_route(kind: u8, gold: i32, price: u16) -> BuyListRoute {
    if gold < price as i32 {
        return BuyListRoute::Refused;
    }
    match kind {
        1 => BuyListRoute::RecipientPicker,
        2 => BuyListRoute::QuantityPicker,
        _ => BuyListRoute::ModeSelect,
    }
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

/// What a [`BuyRecipientSession`] frame produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyRecipientEvent {
    None,
    /// Cursor moved between the rows (SFX `0x21`).
    Moved,
    /// Row 0 confirmed: a plain single-unit buy into the bag (SFX
    /// `0x2C`) - add one copy, debit `cost`, credit `point_credit`.
    BoughtToBag {
        item_id: u8,
        cost: i32,
        point_credit: i32,
    },
    /// A party row confirmed (SFX `0x24`): buy **and equip now**. The
    /// previously equipped piece in the target slot (if any) returns to
    /// the bag (`FUN_800421D4(old, 1)` at `0x801db6a4`), the purchase
    /// equips directly - it never enters the bag - then the gold debit
    /// and Point Card accrual run and the ability bits rebuild
    /// (`FUN_80042558`).
    BoughtAndEquipped {
        /// 0-based party index (`cursor - 1`).
        party_index: u8,
        item_id: u8,
        cost: i32,
        point_credit: i32,
    },
    /// Confirmed a party member who cannot equip the item (SFX `0x23`).
    Buzz,
    /// Backed out (SFX `0x37`) - back to the buy list.
    Cancelled,
    /// Session finished (post-toast) - back to the buy list.
    ExitToBuyList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuyRecipientPhase {
    Init,
    Interactive,
    /// Point Card toast (script `0x801E4EA8`): waits for a press.
    ToastWait,
    Exit,
    Done,
}

/// PORT: FUN_801DB380 (menu-overlay shop **buy recipient picker**
/// sub-screen; `see ghidra/scripts/funcs/overlay_menu_801db380.txt`).
///
/// Navigates `party_count + 1` rows through the shared cursor
/// primitive (`FUN_801D688C`, wrap - port
/// [`crate::menu_input::menu_cursor_nav`]): row 0 buys one copy into
/// the bag, a party row buys **and equips immediately** after an
/// equippability check (item subtype -> equip-record `+6` mask vs the
/// per-character mask byte `0x801E43F0[char]`; a mismatch buzzes and
/// stays). Both purchase paths debit the item-table price, accrue the
/// Point Card ([`point_card_credit`], gated on holding item `0xFE`)
/// and - only when the accrual ran - hold a toast for a button press
/// (SFX `0x20`) before returning to the buy list.
///
/// NOT WIRED: the hosts' shop flow buys into the bag only; equip-now
/// needs the equip-session bridge.
#[derive(Debug, Clone)]
pub struct BuyRecipientSession {
    pub item_id: u8,
    /// Item-record price halfword.
    pub price: u16,
    pub point_card_held: bool,
    /// Cursor row: 0 = the bag, `1..=party` = party members.
    pub cursor: u32,
    /// Per-party-member equippability (index 0 = party member 0 = row 1).
    pub can_equip: Vec<bool>,
    phase: BuyRecipientPhase,
}

impl BuyRecipientSession {
    pub fn new(item_id: u8, price: u16, can_equip: Vec<bool>, point_card_held: bool) -> Self {
        Self {
            item_id,
            price,
            point_card_held,
            cursor: 0,
            can_equip,
            phase: BuyRecipientPhase::Init,
        }
    }

    /// Drive one frame from the menu nav-button edges.
    pub fn tick(&mut self, buttons: crate::menu_input::NavButtons) -> BuyRecipientEvent {
        use crate::menu_input::CursorNav;
        match self.phase {
            BuyRecipientPhase::Init => {
                self.cursor = 0;
                self.phase = BuyRecipientPhase::Interactive;
                BuyRecipientEvent::None
            }
            BuyRecipientPhase::Interactive => {
                let rows = self.can_equip.len() as u32 + 1;
                match crate::menu_input::menu_cursor_nav(&mut self.cursor, rows, true, buttons) {
                    CursorNav::Confirm => {
                        let row = self.cursor & 0xFFF;
                        let point_credit = if self.point_card_held {
                            point_card_credit(self.price, 1)
                        } else {
                            0
                        };
                        if row == 0 {
                            self.phase = if self.point_card_held {
                                BuyRecipientPhase::ToastWait
                            } else {
                                BuyRecipientPhase::Exit
                            };
                            BuyRecipientEvent::BoughtToBag {
                                item_id: self.item_id,
                                cost: self.price as i32,
                                point_credit,
                            }
                        } else if self.can_equip.get(row as usize - 1) == Some(&true) {
                            self.phase = if self.point_card_held {
                                BuyRecipientPhase::ToastWait
                            } else {
                                BuyRecipientPhase::Exit
                            };
                            BuyRecipientEvent::BoughtAndEquipped {
                                party_index: (row - 1) as u8,
                                item_id: self.item_id,
                                cost: self.price as i32,
                                point_credit,
                            }
                        } else {
                            // Retail buzzes and stays on this sub-screen.
                            BuyRecipientEvent::Buzz
                        }
                    }
                    CursorNav::Cancel => {
                        self.phase = BuyRecipientPhase::Done;
                        BuyRecipientEvent::Cancelled
                    }
                    CursorNav::Moved => BuyRecipientEvent::Moved,
                    CursorNav::None => BuyRecipientEvent::None,
                }
            }
            BuyRecipientPhase::ToastWait => {
                if buttons.confirm || buttons.cancel {
                    self.phase = BuyRecipientPhase::Done;
                    BuyRecipientEvent::ExitToBuyList
                } else {
                    BuyRecipientEvent::None
                }
            }
            BuyRecipientPhase::Exit => {
                self.phase = BuyRecipientPhase::Done;
                BuyRecipientEvent::ExitToBuyList
            }
            BuyRecipientPhase::Done => BuyRecipientEvent::None,
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == BuyRecipientPhase::Done
    }
}

// ---------------------------------------------------------------------------
// Shop window *content* renderers (menu overlay).
//
// The shop panels are descriptor-table windows: the 9-slice frame is drawn by
// the window host, and each window's renderer VA draws content only, reading
// its content origin from the live window record's `+0xa` / `+0xc` (`WX`/`WY`).
// The kernels below are the data-derived halves of three of those renderers -
// ink selection, row geometry, cursor gating and the digit-field width law -
// with the retail glyph pushes left to the host's own text layer.
// ---------------------------------------------------------------------------

/// Normal white text ink staged into `_DAT_8007B454` before a string draw.
pub const SHOP_INK_NORMAL: u8 = 7;
/// Greyed / unavailable text ink.
pub const SHOP_INK_GREY: u8 = 0;
/// Accent ink a stock row takes when its record carries the non-zero
/// "already owned / restricted" marker at `+2`.
pub const SHOP_INK_MARKED: u8 = 6;

/// Vertical pitch between shop rows (retail `0xE`).
pub const SHOP_ROW_PITCH: i16 = 0x0E;
/// Text indent from the window content origin (retail `0x14`).
pub const SHOP_TEXT_INDENT: i16 = 0x14;
/// Price-field indent from the window content origin (retail `0x14 + 0x5C`).
pub const SHOP_PRICE_INDENT: i16 = 0x70;
/// Digit count of a stock row's price field.
pub const SHOP_PRICE_DIGITS: u8 = 6;

/// Decode a shop picker's cursor-state word into the hand-sprite mode for
/// `row`, or `None` when no hand draws on that row.
///
/// The word (`DAT_801E46BC` for the root picker, `_DAT_8007BB98` for the
/// stock list) packs the selection in its low 12 bits plus three flags, and
/// every shop renderer re-runs the same four-way decode per row:
///
/// * bit `0x4000` - cursor suppressed entirely (no row draws a hand);
/// * bit `0x2000` - parked/unfocused: the row-index gate is **bypassed**, so
///   every row draws, mode `4` or `0` by the blink bit;
/// * otherwise the low 12 bits must equal `row`, and the mode is the
///   inverted blink bit (`1` animated / `0` static).
///
/// PORT: FUN_801d4868 (per-row cursor gate, `0x801D48B4..0x801D4910`)
/// PORT: FUN_801d5de0 (same decode, `0x801D5E40..0x801D5E9C`)
pub fn shop_cursor_mode(word: u32, row: u16) -> Option<u8> {
    if word & 0x4000 != 0 {
        return None;
    }
    let blink_off = (word & 0x1000) == 0;
    if word & 0x2000 != 0 {
        return Some(if blink_off { 4 } else { 0 });
    }
    if (word & 0xFFF) as u16 != row {
        return None;
    }
    Some(u8::from(blink_off))
}

/// One row of the shop root command window as retail lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShopRootRow {
    /// Row index (`0` Buy, `1` Sell, `2` Quit).
    pub row: u16,
    /// Text pen, content-origin relative: `(WX + 0x14, WY + row * 0xE)`.
    pub text: (i16, i16),
    /// Text ink staged into `_DAT_8007B454` before the string draw.
    pub ink: u8,
    /// Hand-sprite mode + pen when the cursor draws on this row. The hand
    /// sits at the window origin X, not the text indent.
    pub cursor: Option<(u8, (i16, i16))>,
}

/// Build the three rows of the shop **root command window** (menu-overlay
/// window `0x2A`): Buy / Sell / Quit.
///
/// `bag_has_sellable` is the outcome of retail's inventory scan over the
/// `[id, count]` pair array at `0x80085958` across the active slot window
/// `_DAT_8007B5EA.._DAT_8007B5EC` - true when some slot has **both** bytes
/// non-zero. The subtlety the disassembly settles: the ink global is staged
/// to `7` once on entry and cleared to `0` *after* the Buy row has already
/// been drawn, so an empty bag greys **Sell and Quit together** - Buy always
/// renders white.
///
/// PORT: FUN_801d4868 (menu-overlay shop root command window content renderer)
pub fn shop_root_command_rows(
    window: (i16, i16),
    cursor_word: u32,
    bag_has_sellable: bool,
) -> [ShopRootRow; 3] {
    let (wx, wy) = window;
    let mut rows = [ShopRootRow {
        row: 0,
        text: (0, 0),
        ink: SHOP_INK_NORMAL,
        cursor: None,
    }; 3];
    for (i, out) in rows.iter_mut().enumerate() {
        let row = i as u16;
        let y = wy + row as i16 * SHOP_ROW_PITCH;
        out.row = row;
        out.text = (wx + SHOP_TEXT_INDENT, y);
        out.ink = if row == 0 || bag_has_sellable {
            SHOP_INK_NORMAL
        } else {
            SHOP_INK_GREY
        };
        out.cursor = shop_cursor_mode(cursor_word, row).map(|mode| (mode, (wx, y)));
    }
    rows
}

/// Text ink for one row of the shop **stock list** (menu-overlay window
/// renderer `FUN_801D5DE0`).
///
/// `held` is the party's held count of the row's item (retail's bag scan
/// `FUN_80042F4C`), `marker` the stock record's `+2` halfword, `gold` the
/// party purse `_DAT_800845A4` and `price` the record's `+4` word.
///
/// The three tests are applied in a fixed order and each **overwrites** the
/// previous verdict, so the precedence is not the "first rule wins" reading
/// the bullet list of colours suggests: a full stack greys, a non-zero marker
/// then re-inks it to `6` *even though the stack is full*, and an
/// unaffordable price finally greys it again regardless of the marker.
///
/// PORT: FUN_801d5de0 (stock-row ink selection, `0x801D5EA0..0x801D5F6C`)
pub fn shop_stock_row_ink(held: i16, marker: i16, gold: i32, price: i32) -> u8 {
    let mut ink = SHOP_INK_NORMAL;
    if held >= SHOP_HELD_CAP as i16 {
        ink = SHOP_INK_GREY;
    }
    if marker != 0 {
        ink = SHOP_INK_MARKED;
    }
    if gold < price {
        ink = SHOP_INK_GREY;
    }
    ink
}

/// Digit-field width the buy-quantity panel prints the running total
/// `qty * price` with.
///
/// The width is chosen from the magnitude of the **unit price**, not of the
/// total, through three cascading compares against `99` / `999` / `9999`, so
/// the number stays right-aligned in the box as the quantity climbs.
///
/// PORT: FUN_801d5510 (total digit-field width, `0x801D5654..0x801D56A4`)
pub fn shop_total_digit_field(price: u16) -> u8 {
    let mut n: u8 = if price > 9999 { 5 } else { 4 };
    if price > 999 {
        n += 1;
    }
    if price > 99 {
        n += 1;
    }
    n
}

/// The buy-quantity prompt panel's data-derived content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuyQuantityPanel {
    /// `Some(count)` when the party already holds the highlighted item -
    /// retail prints the "Have" label plus this 2-digit count. `None` is the
    /// bag-scan sentinel `0x100` (nothing held) and prints the "None" string
    /// at the window origin instead.
    pub have: Option<u8>,
    /// Pen of the held-count digits, `(WX + 0x20, WY)`. Unused when `have`
    /// is `None`.
    pub have_count_pen: (i16, i16),
    /// Pen of the label that follows the count, `(WX + 0x30, WY)` when a
    /// count printed and `(WX, WY)` when it did not.
    pub have_tail_pen: (i16, i16),
    /// Pen of the "How many will you buy?" prompt, `(WX, WY + 0xE)`.
    pub prompt_pen: (i16, i16),
    /// Baseline of the quantity row, `WY + 0x22`.
    pub value_row_y: i16,
    /// Chosen quantity + its 2-digit pen `(WX + 0x18, value_row_y)`.
    pub quantity: (u8, (i16, i16)),
    /// Unit price + its 2-digit pen `(WX + 0x30, value_row_y)`.
    pub unit: (u16, (i16, i16)),
    /// Running total `qty * price`, its digit-field width and its pen
    /// `(WX + 0x62, value_row_y)`.
    pub total: (u32, u8, (i16, i16)),
    /// Hand-sprite pen `(WX + 4, value_row_y)`; the mode is always `1`.
    pub cursor_pen: (i16, i16),
}

/// Build the **buy-quantity prompt panel** content (menu-overlay window
/// renderer `FUN_801D5510`).
///
/// `held` is the bag-scan result for the highlighted item id: `Some(slot
/// count)` or `None` for retail's `0x100` "not held" sentinel.
///
/// PORT: FUN_801d5510 (menu-overlay buy-quantity prompt window content renderer)
pub fn shop_buy_quantity_panel(
    window: (i16, i16),
    held: Option<u8>,
    quantity: u8,
    unit_price: u16,
) -> BuyQuantityPanel {
    let (wx, wy) = window;
    let value_row_y = wy + 0x22;
    BuyQuantityPanel {
        have: held,
        have_count_pen: (wx + 0x20, wy),
        have_tail_pen: (if held.is_some() { wx + 0x30 } else { wx }, wy),
        prompt_pen: (wx, wy + SHOP_ROW_PITCH),
        value_row_y,
        quantity: (quantity, (wx + 0x18, value_row_y)),
        unit: (unit_price, (wx + 0x30, value_row_y)),
        total: (
            quantity as u32 * unit_price as u32,
            shop_total_digit_field(unit_price),
            (wx + 0x62, value_row_y),
        ),
        cursor_pen: (wx + 4, value_row_y),
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

    use crate::menu_input::NavButtons;

    fn nav(confirm: bool, cancel: bool, left: bool, right: bool) -> NavButtons {
        NavButtons {
            confirm,
            cancel,
            left,
            right,
        }
    }

    #[test]
    fn buy_recipient_row0_buys_into_bag() {
        let mut s = BuyRecipientSession::new(0x30, 200, vec![true, false], false);
        assert_eq!(
            s.tick(nav(false, false, false, false)),
            BuyRecipientEvent::None
        );
        assert_eq!(
            s.tick(nav(true, false, false, false)),
            BuyRecipientEvent::BoughtToBag {
                item_id: 0x30,
                cost: 200,
                point_credit: 0,
            }
        );
        assert_eq!(
            s.tick(nav(false, false, false, false)),
            BuyRecipientEvent::ExitToBuyList
        );
        assert!(s.is_done());
    }

    #[test]
    fn buy_recipient_party_row_equips_or_buzzes() {
        let mut s = BuyRecipientSession::new(0x30, 200, vec![true, false], true);
        s.tick(nav(false, false, false, false));
        assert_eq!(
            s.tick(nav(false, false, false, true)),
            BuyRecipientEvent::Moved
        );
        assert_eq!(
            s.tick(nav(false, false, false, true)),
            BuyRecipientEvent::Moved
        );
        // Row 2 = party member 1, cannot equip -> buzz, stays.
        assert_eq!(
            s.tick(nav(true, false, false, false)),
            BuyRecipientEvent::Buzz
        );
        assert!(!s.is_done());
        // Back to row 1 = party member 0 (can equip).
        s.tick(nav(false, false, true, false));
        assert_eq!(
            s.tick(nav(true, false, false, false)),
            BuyRecipientEvent::BoughtAndEquipped {
                party_index: 0,
                item_id: 0x30,
                cost: 200,
                point_credit: 10,
            }
        );
        // Point Card toast holds for a press.
        assert_eq!(
            s.tick(nav(false, false, false, false)),
            BuyRecipientEvent::None
        );
        assert_eq!(
            s.tick(nav(true, false, false, false)),
            BuyRecipientEvent::ExitToBuyList
        );
    }

    #[test]
    fn buy_recipient_cancel_returns_to_list() {
        let mut s = BuyRecipientSession::new(0x30, 200, vec![true], false);
        s.tick(nav(false, false, false, false));
        assert_eq!(
            s.tick(nav(false, true, false, false)),
            BuyRecipientEvent::Cancelled
        );
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
    fn buy_list_confirm_routes_by_item_kind() {
        // FUN_801DB21C state-2 dispatch: affordability first (buzz +
        // stay), then kind 1 -> recipient picker, kind 2 -> quantity
        // picker, anything else -> the mode-select fallback.
        assert_eq!(buy_list_confirm_route(2, 99, 100), BuyListRoute::Refused);
        assert_eq!(
            buy_list_confirm_route(1, 100, 100),
            BuyListRoute::RecipientPicker
        );
        assert_eq!(
            buy_list_confirm_route(2, 100, 100),
            BuyListRoute::QuantityPicker
        );
        assert_eq!(
            buy_list_confirm_route(0, 100, 100),
            BuyListRoute::ModeSelect
        );
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

    #[test]
    fn cursor_word_suppress_park_and_select() {
        // bit 0x4000 kills the hand on every row.
        assert_eq!(shop_cursor_mode(0x4000 | 1, 1), None);
        // bit 0x2000 parks it: every row draws, mode by the blink bit.
        assert_eq!(shop_cursor_mode(0x2000, 0), Some(4));
        assert_eq!(shop_cursor_mode(0x2000, 2), Some(4));
        assert_eq!(shop_cursor_mode(0x2000 | 0x1000, 2), Some(0));
        // Focused: only the selected row, mode = inverted blink bit.
        assert_eq!(shop_cursor_mode(1, 0), None);
        assert_eq!(shop_cursor_mode(1, 1), Some(1));
        assert_eq!(shop_cursor_mode(0x1000 | 1, 1), Some(0));
    }

    #[test]
    fn root_command_rows_grey_sell_and_quit_together() {
        let rows = shop_root_command_rows((40, 50), 0, false);
        assert_eq!(rows[0].text, (40 + 0x14, 50));
        assert_eq!(rows[1].text, (40 + 0x14, 50 + 0x0E));
        assert_eq!(rows[2].text, (40 + 0x14, 50 + 0x1C));
        // The ink is cleared after the Buy row draws, so Buy stays white and
        // both later rows grey.
        assert_eq!(rows[0].ink, SHOP_INK_NORMAL);
        assert_eq!(rows[1].ink, SHOP_INK_GREY);
        assert_eq!(rows[2].ink, SHOP_INK_GREY);

        let rows = shop_root_command_rows((40, 50), 0, true);
        assert!(rows.iter().all(|r| r.ink == SHOP_INK_NORMAL));
    }

    #[test]
    fn root_command_cursor_sits_at_the_window_origin() {
        let rows = shop_root_command_rows((40, 50), 1, true);
        assert_eq!(rows[0].cursor, None);
        assert_eq!(rows[1].cursor, Some((1, (40, 50 + 0x0E))));
        assert_eq!(rows[2].cursor, None);
    }

    #[test]
    fn stock_row_ink_precedence_is_last_rule_wins() {
        // Plain affordable row.
        assert_eq!(shop_stock_row_ink(0, 0, 1000, 100), SHOP_INK_NORMAL);
        // Full stack greys.
        assert_eq!(shop_stock_row_ink(99, 0, 1000, 100), SHOP_INK_GREY);
        // ...but a non-zero marker re-inks it even at a full stack.
        assert_eq!(shop_stock_row_ink(99, 1, 1000, 100), SHOP_INK_MARKED);
        // ...and an unaffordable price greys it again, marker or not.
        assert_eq!(shop_stock_row_ink(0, 1, 50, 100), SHOP_INK_GREY);
        assert_eq!(shop_stock_row_ink(0, 0, 50, 100), SHOP_INK_GREY);
        // Exactly affordable is affordable (`gold < price` is strict).
        assert_eq!(shop_stock_row_ink(0, 0, 100, 100), SHOP_INK_NORMAL);
        // 98 held is still under the cap.
        assert_eq!(shop_stock_row_ink(98, 0, 1000, 100), SHOP_INK_NORMAL);
    }

    #[test]
    fn total_digit_field_steps_on_the_unit_price() {
        assert_eq!(shop_total_digit_field(0), 4);
        assert_eq!(shop_total_digit_field(99), 4);
        assert_eq!(shop_total_digit_field(100), 5);
        assert_eq!(shop_total_digit_field(999), 5);
        assert_eq!(shop_total_digit_field(1000), 6);
        assert_eq!(shop_total_digit_field(9999), 6);
        assert_eq!(shop_total_digit_field(10000), 7);
    }

    #[test]
    fn buy_quantity_panel_geometry_and_total() {
        let p = shop_buy_quantity_panel((10, 20), Some(3), 5, 250);
        assert_eq!(p.have, Some(3));
        assert_eq!(p.have_count_pen, (10 + 0x20, 20));
        assert_eq!(p.have_tail_pen, (10 + 0x30, 20));
        assert_eq!(p.prompt_pen, (10, 20 + 0x0E));
        assert_eq!(p.value_row_y, 20 + 0x22);
        assert_eq!(p.quantity, (5, (10 + 0x18, 20 + 0x22)));
        assert_eq!(p.unit, (250, (10 + 0x30, 20 + 0x22)));
        assert_eq!(p.total, (1250, 5, (10 + 0x62, 20 + 0x22)));
        assert_eq!(p.cursor_pen, (10 + 4, 20 + 0x22));

        // Nothing held: the "None" string takes the window origin and no
        // count row prints.
        let p = shop_buy_quantity_panel((10, 20), None, 1, 10);
        assert_eq!(p.have, None);
        assert_eq!(p.have_tail_pen, (10, 20));
    }
}
