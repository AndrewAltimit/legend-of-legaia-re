//! The buy recipient picker: bag row or a party member who equips the purchase.

use super::*;

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
/// [`legaia_engine_system::menu_input::menu_cursor_nav`]): row 0 buys one copy into
/// the bag, a party row buys **and equips immediately** after an
/// equippability check (item subtype -> equip-record `+6` mask vs the
/// per-character mask byte `0x801E43F0[char]`; a mismatch buzzes and
/// stays). Both purchase paths debit the item-table price, accrue the
/// Point Card ([`point_card_credit`], gated on holding item `0xFE`)
/// and - only when the accrual ran - hold a toast for a button press
/// (SFX `0x20`) before returning to the buy list.
///
/// Wired: `legaia_engine_menu_runtime::menu_runtime::MenuRuntime` opens this session from the
/// buy list's `RecipientPicker` route and drives it in `tick` (the browser
/// play page enables the flow via `retail_equipment_buy` and draws windows
/// 36 / 25 / 41 over the parked list). The Point Card gate stays closed -
/// `World` keeps no Point Card counter, so the session runs with
/// `point_card_held = false` and the toast beat never arms.
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
    pub fn tick(
        &mut self,
        buttons: legaia_engine_system::menu_input::NavButtons,
    ) -> BuyRecipientEvent {
        use legaia_engine_system::menu_input::CursorNav;
        match self.phase {
            BuyRecipientPhase::Init => {
                self.cursor = 0;
                self.phase = BuyRecipientPhase::Interactive;
                BuyRecipientEvent::None
            }
            BuyRecipientPhase::Interactive => {
                let rows = self.can_equip.len() as u32 + 1;
                match legaia_engine_system::menu_input::menu_cursor_nav(
                    &mut self.cursor,
                    rows,
                    true,
                    buttons,
                ) {
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
