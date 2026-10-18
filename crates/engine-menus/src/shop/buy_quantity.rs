//! The buy side: Point Card credit, the buy-list confirm route and the buy quantity picker session.

use super::*;

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
///
/// Wired: `legaia_engine_menu_runtime::menu_runtime`'s `ShopBuy` commit + route override run
/// this dispatch on every confirmed buy row (kind byte off the on-disc
/// item-effect tables, `World::tables.item_effects`). `Refused` is the retail
/// list-level refusal beat (the hand stays on the row); `RecipientPicker`
/// opens [`BuyRecipientSession`] while the host opted into
/// `MenuRuntime::retail_equipment_buy`; `ModeSelect` returns to the
/// Buy/Sell/Quit picker; `QuantityPicker` keeps the quantity route. A
/// PROT.DAT-only load has no kind byte and falls back to the stackable arm.
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

/// How many quantities the `ShopQuantity` screen may offer for the staged
/// item - the shared row count both hosts draw and the menu VM navigates.
///
/// Retail does not pick a quantity off a list at all: `FUN_801DB7F4` and
/// `FUN_801DBD94` are **steppers** on a scalar at `DAT_801E46B4`
/// (`andi 0x2000` +1, `andi 0x8000` -1, `andi 0x4000` +10, `andi 0x1000` -10,
/// each clamped), and neither calls the cursor-nav primitive `FUN_801D688C`
/// the recipient picker uses. What retail *does* own is the **bound**, and
/// that is what this returns: the buy side's
/// `min(gold / price, 99, 99 - held)` ([`buy_qty_max`],
/// `0x801db868..0x801db8ec`) and the sell side's staged bag count.
///
/// The screen used to offer a flat nine rows, which capped every purchase at
/// nine copies no matter what the purse or the stack allowed.
///
/// Always at least `1`: a zero-row list has no confirmable row, and retail's
/// own steppers clamp their low end to `1`.
pub fn quantity_row_count(
    is_buying: bool,
    gold: i32,
    price: u16,
    held: Option<u8>,
    staged_count: Option<u8>,
) -> u8 {
    let n = match is_buying {
        true => buy_qty_max(gold, price, held),
        false => i32::from(staged_count.unwrap_or(1)),
    };
    n.clamp(1, BUY_QTY_CAP) as u8
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
/// Wired through [`QuantityPicker`] on both hosts, the same way as
/// [`SellQuantitySession`]: the picker owns the pad for the screen, the
/// commit runs straight out of the stepper (retail has no Yes/No screen
/// between them), and window 35 (`engine-ui`'s `buy_quantity_draws_for`) draws the
/// stepped number.
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
        use legaia_engine_system::input::PadButton;
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
