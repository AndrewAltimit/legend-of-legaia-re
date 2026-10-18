//! Shop menu rows, root labels and the UI cue each shop input raises.

use crate::menu::{MenuInput, MenuState};

/// The action each row of the [`MenuState::ShopMenu`] top picker commits to, in
/// row order. The Trade row only exists when seru trading is enabled, so this is
/// the single source of truth the cursor count, render, route, and commit all
/// read (keeping the dynamic layout consistent).
pub fn shop_menu_rows(trading: bool) -> &'static [MenuState] {
    if trading {
        &[
            MenuState::ShopBuy,
            MenuState::ShopSell,
            MenuState::ShopTrade,
            MenuState::ShopExit,
        ]
    } else {
        &[MenuState::ShopBuy, MenuState::ShopSell, MenuState::ShopExit]
    }
}

/// The [`MenuState::ShopMenu`] top picker's rows as both play hosts print
/// them: one `(label, ink)` per [`shop_menu_rows`] entry.
///
/// The labels are retail's own, read out of the menu overlay
/// ([`crate::shop::ShopLabels::picker`], `0x801CEB94` / `0x801CEB9C` /
/// `0x801CEBA4`, drawn by `FUN_801D4868`); with no overlay installed they
/// are empty. The ink is retail's root command window
/// (`shop_root_command_rows`): Buy always white, and an empty bag greys
/// **Sell and Quit together** - the ink global is cleared after the Buy row
/// draws, so every row below it takes the grey. The engine-only Trade row
/// (the randomizer's Seru trade, no retail string) sits between Sell and
/// Quit and takes the same rule.
pub fn shop_root_labels(
    labels: &crate::shop::ShopLabels,
    trading: bool,
    bag_has_sellable: bool,
) -> Vec<(String, u8)> {
    let ink = crate::shop::shop_root_command_rows((0, 0), 0x4000, bag_has_sellable);
    shop_menu_rows(trading)
        .iter()
        .map(|s| match s {
            MenuState::ShopBuy => (labels.picker[0].plain(), ink[0].ink),
            MenuState::ShopSell => (labels.picker[1].plain(), ink[1].ink),
            MenuState::ShopTrade => ("Trade Seru".to_string(), ink[1].ink),
            _ => (labels.picker[2].plain(), ink[2].ink),
        })
        .collect()
}

/// The UI cue a shop **list** screen keys for one tick of input - the
/// kind-4 list kernel `FUN_80032A44`'s ring writes, which every shop list
/// (the root picker, the buy list built by `FUN_80030628` case `0x0B`, the
/// sell list) is paged by: a cursor step `0x21` (`li a2,0x21` at
/// `0x80032B9C`) only when the cursor actually moved, a confirm `0x20` on an
/// enabled row (`0x80032D24`) or the dim-row buzz `0x23` (`0x80032D0C`), and
/// the cancel `0x37` (`0x80032D74`). `None` outside the shop's list states
/// (the inn prompt and the pause-menu screens key their own).
// REF: FUN_80032A44
pub fn shop_ui_cue(state: MenuState, input: MenuInput, moved: bool, refused: bool) -> Option<u8> {
    use crate::menu_cues::{MENU_CANCEL_CUE, MENU_CONFIRM_CUE, MENU_CURSOR_CUE};
    if !matches!(
        state,
        MenuState::ShopMenu
            | MenuState::ShopBuy
            | MenuState::ShopSell
            | MenuState::ShopConfirm
            | MenuState::ShopTrade
    ) {
        return None;
    }
    if input.cross {
        return Some(if refused {
            SHOP_REFUSAL_CUE
        } else {
            MENU_CONFIRM_CUE
        });
    }
    if input.circle || input.triangle {
        return Some(MENU_CANCEL_CUE);
    }
    moved.then_some(MENU_CURSOR_CUE)
}

/// The list kernel's dim-row buzz (`li a1,0x23` at `0x80032D0C`), which the
/// recipient picker also keys for a member who cannot equip the piece.
pub const SHOP_REFUSAL_CUE: u8 = 0x23;

/// The quantity steppers' cues. Buying (`FUN_801DB7F4`): a step `0x21` only
/// when the number moved (`0x801DB9B0` and siblings, each behind its bound
/// test), the purchase `0x2C` (`FUN_80035BD0(0x2C)` at `0x801DB940`, the
/// overwrite producer, which replaces the confirm blip of the same frame),
/// cancel `0x37` (`0x801DB970`). Selling (`FUN_801DBD94`): the same steps
/// and cancel (`0x801DC064..`, `0x801DC020`) and the sale `0x36`
/// (`0x801DBE78`).
// REF: FUN_801DB7F4, FUN_801DBD94
pub fn quantity_ui_cue(event: &crate::shop::QuantityPickerEvent) -> Option<u8> {
    use crate::shop::QuantityPickerEvent as E;
    match event {
        E::Moved => Some(crate::menu_cues::MENU_CURSOR_CUE),
        E::Bought { .. } => Some(0x2C),
        E::Sold { .. } => Some(0x36),
        E::Cancelled => Some(crate::menu_cues::MENU_CANCEL_CUE),
        E::None | E::Finished => None,
    }
}

/// The buy-recipient picker's cues (`FUN_801DB380`, a list-kernel list): a
/// row step `0x21`, cancel `0x37` (the kernel's), and the confirm the overlay
/// overwrites through `FUN_80035BD0` - `0x2C` into the bag (`0x801DB480`),
/// `0x24` buy-and-equip (`0x801DB5C8`), `0x23` a member who cannot equip it
/// (`0x801DB5AC`).
// REF: FUN_801DB380
pub fn recipient_ui_cue(event: &crate::shop::BuyRecipientEvent) -> Option<u8> {
    use crate::shop::BuyRecipientEvent as E;
    match event {
        E::Moved => Some(crate::menu_cues::MENU_CURSOR_CUE),
        E::BoughtToBag { .. } => Some(0x2C),
        E::BoughtAndEquipped { .. } => Some(0x24),
        E::Buzz => Some(SHOP_REFUSAL_CUE),
        E::Cancelled => Some(crate::menu_cues::MENU_CANCEL_CUE),
        E::None | E::ExitToBuyList => None,
    }
}

/// Rows on one page of the shop list a menu state browses, or `None` for a
/// state that is not a shop list. The kind-4 kernel's
/// `(content_h - 4) / 0xE` over the two descriptor rects: window 40 (the buy
/// list, `h = 104`) pages 7, window 38 (the sell list, `h = 158`) pages 11 -
/// the page lengths the Retock capture shows.
pub fn shop_list_page_rows(state: MenuState) -> Option<usize> {
    match state {
        MenuState::ShopBuy => Some(7),
        MenuState::ShopSell => Some(11),
        _ => None,
    }
}
