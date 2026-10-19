//! The quantity picker shared by the buy and sell sides, and its pad step.

use super::*;

/// The shared quantity-picker pad decode (identical instruction shapes
/// in `FUN_801DBD94` at `0x801dc034..0x801dc178` and `FUN_801DB7F4` at
/// `0x801db980..0x801dbac4`): Right/Left step by one, Down/Up by ten,
/// clamped to `[1, max]`; a step off either end is a silent no-op (the
/// retail gates are `qty < max` for the increments and `qty >= 2` for
/// the decrements, checked before the SFX fires). Returns the new
/// quantity, or `None` when nothing moved.
pub(super) fn quantity_step(pressed: u16, qty: i32, max: i32) -> Option<i32> {
    use legaia_engine_system::input::PadButton;
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

/// The live quantity screen: whichever of the two retail steppers the shop
/// flow entered.
///
/// Retail runs them as two sibling menu-overlay sub-screens over a parked
/// list; the engine runs them as one `Option` on
/// `legaia_engine_menu_runtime::menu_runtime::MenuRuntime` that takes the pad while it is
/// `Some`, exactly as the buy-recipient picker does. Both hosts read the
/// same accessors for the window they draw, so the screen is one
/// interaction on one bound with one pad decode.
#[derive(Debug, Clone)]
pub enum QuantityPicker {
    Buy(BuyQuantitySession),
    Sell(SellQuantitySession),
}

/// What one [`QuantityPicker`] frame produced, folded to the arms a host
/// cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantityPickerEvent {
    None,
    /// The number stepped (retail cues SFX `0x21`).
    Moved,
    /// The purchase landed: consume `cost` gold, add `qty` copies, and
    /// credit the Point Card by `point_credit` when non-zero.
    Bought {
        item_id: u8,
        qty: u8,
        cost: i32,
        point_credit: i32,
    },
    /// The sale landed: remove `qty` copies of `item_id`, credit `credit`.
    Sold {
        item_id: u8,
        qty: u8,
        credit: i32,
    },
    /// Backed out (SFX `0x37`) - the list takes the pad back.
    Cancelled,
    /// The screen is finished and the flow returns to the list it came from.
    Finished,
}

impl QuantityPicker {
    /// `true` while the picker is sizing a purchase.
    pub fn is_buying(&self) -> bool {
        matches!(self, Self::Buy(_))
    }

    /// The staged item id.
    pub fn item_id(&self) -> u8 {
        match self {
            Self::Buy(s) => s.item_id,
            Self::Sell(s) => s.item_id,
        }
    }

    /// The unit price both panels print the running total from.
    pub fn price(&self) -> u16 {
        match self {
            Self::Buy(s) => s.price,
            Self::Sell(s) => s.price,
        }
    }

    /// The stepped number (`DAT_801E46B4`), clamped into the screen's own
    /// bound for display.
    pub fn quantity(&self) -> u8 {
        let (q, m) = match self {
            Self::Buy(s) => (s.qty, s.max),
            Self::Sell(s) => (s.qty, s.max),
        };
        q.clamp(1, m.max(1)).clamp(0, 255) as u8
    }

    /// The bound (`DAT_801E46B8`) the second number on the value row prints.
    pub fn max(&self) -> u8 {
        let m = match self {
            Self::Buy(s) => s.max,
            Self::Sell(s) => s.max,
        };
        m.clamp(0, 255) as u8
    }

    /// `true` once the screen is done and the caller should drop it.
    pub fn is_done(&self) -> bool {
        match self {
            Self::Buy(s) => s.is_done(),
            Self::Sell(s) => s.is_done(),
        }
    }

    /// Drive one frame from the edge-triggered pad word. `frame_delta` feeds
    /// the sell picker's exit delay (1 per vsync); the buy picker ignores it.
    pub fn tick(&mut self, pressed: u16, frame_delta: i32) -> QuantityPickerEvent {
        match self {
            Self::Buy(s) => match s.tick(pressed) {
                BuyQtyEvent::None => QuantityPickerEvent::None,
                BuyQtyEvent::Moved => QuantityPickerEvent::Moved,
                BuyQtyEvent::Bought {
                    item_id,
                    qty,
                    cost,
                    point_credit,
                } => QuantityPickerEvent::Bought {
                    item_id,
                    qty,
                    cost,
                    point_credit,
                },
                BuyQtyEvent::Cancelled => QuantityPickerEvent::Cancelled,
                BuyQtyEvent::ExitToBuyList => QuantityPickerEvent::Finished,
            },
            Self::Sell(s) => match s.tick(pressed, frame_delta) {
                SellQtyEvent::None => QuantityPickerEvent::None,
                SellQtyEvent::Moved => QuantityPickerEvent::Moved,
                SellQtyEvent::Sold {
                    item_id,
                    qty,
                    credit,
                } => QuantityPickerEvent::Sold {
                    item_id,
                    qty,
                    credit,
                },
                SellQtyEvent::Cancelled => QuantityPickerEvent::Cancelled,
                SellQtyEvent::ExitToSellList | SellQtyEvent::ExitToShopRoot => {
                    QuantityPickerEvent::Finished
                }
            },
        }
    }

    /// Route a whole-stack sale by the caller's post-sale bag rescan. No-op
    /// on the buy side.
    pub fn finish_whole_stack(&mut self, bag_empty: bool) {
        if let Self::Sell(s) = self {
            s.finish_whole_stack(bag_empty);
        }
    }

    /// `true` when the finished sell picker ran its exit delay and the flow
    /// returns to the shop **root** instead of the sell list.
    pub fn exits_to_shop_root(&self) -> bool {
        matches!(self, Self::Sell(s) if s.exits_to_shop_root())
    }
}
