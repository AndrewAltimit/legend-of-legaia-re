//! The sell side: sale credit, the sell-list fixup and the sell quantity picker session.

use super::*;

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
///
/// The engine reaches the same rule without this helper: its shop lists run
/// on one flat cursor whose page is derived from it, and
/// `legaia_engine_menu_runtime::menu_runtime::MenuRuntime` returns the hand to its row after a
/// sale clamped to the rebuilt list - a lone last-page row that is sold away
/// leaves the cursor on the new last row, one page back, which is exactly the
/// `(selected, scroll_top)` step this function makes on the paged pair.
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
/// Wired: [`QuantityPicker`] installs this session the moment the sell list
/// stages a stack, and `legaia_engine_menu_runtime::menu_runtime::MenuRuntime` hands it the pad
/// for the whole screen - so both hosts step one number in place and draw
/// window 37 (`engine-ui`'s `sell_quantity_draws_for`) over the parked list, with no
/// Yes/No screen between the stepper and the sale.
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
    /// The whole-stack sale emptied the bag, so the exit routes to the shop
    /// root rather than the sell list.
    root_exit: bool,
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
            root_exit: false,
        }
    }

    /// Drive one frame. `pressed` is the edge-triggered pad word;
    /// `frame_delta` the scratchpad frame-delta units (1 per vsync at
    /// 60 Hz) consumed by the exit delay.
    pub fn tick(&mut self, pressed: u16, frame_delta: i32) -> SellQtyEvent {
        use legaia_engine_system::input::PadButton;
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
        self.root_exit = bag_empty;
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

    /// `true` while the session is in (or has just left) the exit-delay
    /// phase a whole-stack sale that emptied the bag enters - the route
    /// retail takes back to the shop root instead of the sell list.
    pub fn exits_to_shop_root(&self) -> bool {
        self.root_exit
    }
}
