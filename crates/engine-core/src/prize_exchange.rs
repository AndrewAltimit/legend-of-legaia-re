//! Casino prize-exchange session - the menu-overlay sub-screen `0x20`.
//!
//! PORT: FUN_801DC1CC (menu overlay, entered through the save/menu outer
//! dispatcher `FUN_801DC6B4` when the entry-context pointer `_DAT_8007B450`
//! targets an `0x07` byte; the byte at `ptr+1` selects the prize **block** -
//! see `ghidra/scripts/funcs/overlay_menu_801dc1cc.txt`).
//!
//! Retail drives a 4-state machine on the shared sub-screen step counter
//! `DAT_801E46AC`:
//!
//! - **State 0 (build)**: sets system flag `8` (`FUN_8003CE08(8)`), then
//!   walks the active `0x60`-byte block of the prize table at `0x801E4518`
//!   (8-byte records `[u16 item_id][u16 gate][u32 price]` - the same table
//!   `legaia_rando::casino` edits), collecting the **visible** row indices
//!   into `0x801F00E0` (count at `0x801F00D0`): the walk stops at the first
//!   record whose id halfword is `<= 0` (`lh`/`blez` at `0x801dc264..`), and
//!   a row is skipped when its gate is non-zero **and** the system flag it
//!   names is already set (`FUN_8003CE64` at `0x801dc2bc` - the gate marks a
//!   one-shot prize as already redeemed). Runs the list window script
//!   `&DAT_801E4F18`, zeroes the list cursor, advances to state 1.
//! - **State 1 (browse)**: `FUN_801D688C(&_DAT_8007BB98, count, 1)`. On
//!   confirm the selected record is gated: coin bank `0x800845A4` short of
//!   the price (`slt` at `0x801dc3cc`) or held count not `< 0x63`
//!   (`FUN_80042F4C(id)` + `slti v0,v0,0x63` at `0x801dc3ec`) buzzes SFX
//!   `0x23` and stays; otherwise the Yes/No script `&DAT_801E4F2C` opens
//!   with the confirm cursor **defaulting to row 1 = No**
//!   (`DAT_801E46D0 = 1` at `0x801dc414`) and the browse cursor's editing
//!   bit `0x1000` raised. On cancel the session exits to sub-screen `0`.
//! - **State 2 (confirm)**: `FUN_801D688C(&DAT_801E46D0, 2, 1)`. Confirm on
//!   row 0 (Yes) advances to state 3; row 1 (No) and the cancel button both
//!   re-run the list script `&DAT_801E4F34` and return to state 1.
//! - **State 3 (commit)**: plays the coin jingle SFX `0x25`
//!   (`FUN_80035B50(0x25)` at `0x801dc4c0`), grants one copy
//!   (`FUN_800421D4(id, 1)`), debits the **casino coin bank**
//!   `0x800845A4 -= price` (`0x801dc540..0x801dc550` - coins, not gold),
//!   sets the record's gate flag when non-zero (`FUN_8003CE08(gate)` - the
//!   one-shot prize disappears from the rebuilt list), rebuilds the visible
//!   rows and returns to state 1.
//!
//! The engine port keeps the session renderer-free: the SM emits
//! [`PrizeEvent`]s (SFX cues included as values) and the host applies the
//! coin/inventory/flag deltas through [`apply_redeem`], then calls
//! [`PrizeExchangeSession::rebuild`] - mirroring how the retail commit
//! rebuilds the row list in place.

use crate::menu_input::{CursorNav, NavButtons, menu_cursor_nav};
use std::collections::HashMap;

/// One 8-byte prize record of the `0x801E4518` table: `[u16 item_id]
/// [u16 gate][u32 price]`. Ids share the 256-entry item-id space; a `0` id
/// terminates the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrizeRecord {
    pub item_id: u8,
    /// System-flag index (bank `DAT_80085758`); `0` = always available,
    /// non-zero marks a one-shot prize hidden once the flag is set.
    pub gate: u16,
    /// Price in casino coins (the `0x800845A4` bank, not gold).
    pub price: u32,
}

/// Retail per-stack held cap the redeem gate tests against
/// (`slti v0,v0,0x63` at `0x801dc3ec` - the same `0x63` = 99 literal as the
/// gold-shop buy clamp).
pub const PRIZE_HELD_CAP: u8 = 99;

/// System flag the exchange raises on entry (`FUN_8003CE08(8)` at
/// `0x801dc230`).
pub const PRIZE_EXCHANGE_VISITED_FLAG: u16 = 8;

/// SFX cue for a refused redeem (coin-short or held-cap buzz).
pub const SFX_BUZZ: u8 = 0x23;
/// SFX cue for the commit's coin jingle (`FUN_80035B50(0x25)`).
pub const SFX_COIN_JINGLE: u8 = 0x25;

/// Why a redeem attempt buzzed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedeemRefusal {
    /// Coin bank short of the price (`0x800845A4 < price`).
    NotEnoughCoins,
    /// Held count of the prize item is already at [`PRIZE_HELD_CAP`].
    HeldCap,
}

/// The retail state-1 confirm gate: `Ok` when the redeem can proceed to the
/// Yes/No prompt (coin test first, then the held-cap test - the retail
/// order at `0x801dc3c0..0x801dc3f0`).
pub fn redeem_gate(coins: u32, price: u32, held: u8) -> Result<(), RedeemRefusal> {
    if coins < price {
        return Err(RedeemRefusal::NotEnoughCoins);
    }
    if held >= PRIZE_HELD_CAP {
        return Err(RedeemRefusal::HeldCap);
    }
    Ok(())
}

/// The state-0 visible-row walk: record indices of the active block, in
/// table order, stopping at the first zero id, skipping rows whose non-zero
/// gate flag is already set.
pub fn visible_rows(block: &[PrizeRecord], mut flag_set: impl FnMut(u16) -> bool) -> Vec<usize> {
    let mut rows = Vec::new();
    for (i, rec) in block.iter().enumerate() {
        if rec.item_id == 0 {
            break;
        }
        if rec.gate != 0 && flag_set(rec.gate) {
            continue;
        }
        rows.push(i);
    }
    rows
}

/// What one [`PrizeExchangeSession::tick`] produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrizeEvent {
    None,
    /// Cursor moved (SFX `0x21` via the shared navigator).
    Moved,
    /// Redeem attempt buzzed (SFX [`SFX_BUZZ`]); session stays browsing.
    Refused(RedeemRefusal),
    /// Yes/No prompt opened (confirm cursor seeded to No).
    OpenedConfirm,
    /// Confirm-prompt cursor moved.
    ConfirmMoved,
    /// Backed out of the Yes/No prompt (No row or cancel button).
    BackToList,
    /// Yes committed: the host applies [`apply_redeem`] with these values,
    /// then calls [`PrizeExchangeSession::rebuild`]. Carries the commit
    /// jingle [`SFX_COIN_JINGLE`].
    Redeemed {
        item_id: u8,
        price: u32,
        gate: u16,
    },
    /// Browse cancelled - session over (retail exits to sub-screen `0`).
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Browse,
    Confirm,
    Done,
}

/// Renderer-free prize-exchange session over one prize block.
#[derive(Debug, Clone)]
pub struct PrizeExchangeSession {
    block: Vec<PrizeRecord>,
    /// Visible row -> record index (the retail `0x801F00E0` list).
    rows: Vec<usize>,
    /// Packed browse cursor (`_DAT_8007BB98` low 12 bits).
    cursor: u32,
    /// Yes/No cursor (`DAT_801E46D0`): 0 = Yes, 1 = No.
    confirm_cursor: u32,
    phase: Phase,
}

impl PrizeExchangeSession {
    /// Open the session on a prize block (state 0). The host should also
    /// raise [`PRIZE_EXCHANGE_VISITED_FLAG`], which retail sets on entry.
    pub fn new(block: Vec<PrizeRecord>, flag_set: impl FnMut(u16) -> bool) -> Self {
        let rows = visible_rows(&block, flag_set);
        Self {
            block,
            rows,
            cursor: 0,
            confirm_cursor: 1,
            phase: Phase::Browse,
        }
    }

    /// Re-run the state-0 visible-row walk (the retail commit path does
    /// this in place after every redeem).
    pub fn rebuild(&mut self, flag_set: impl FnMut(u16) -> bool) {
        self.rows = visible_rows(&self.block, flag_set);
        self.cursor = (self.cursor & crate::menu_input::CURSOR_INDEX_MASK)
            .min(self.rows.len().saturating_sub(1) as u32);
    }

    /// Visible rows as records, in list order.
    pub fn rows(&self) -> impl Iterator<Item = &PrizeRecord> {
        self.rows.iter().map(|&i| &self.block[i])
    }

    /// Browse-cursor row index.
    pub fn cursor(&self) -> usize {
        (self.cursor & crate::menu_input::CURSOR_INDEX_MASK) as usize
    }

    /// Yes/No cursor (0 = Yes, 1 = No).
    pub fn confirm_cursor(&self) -> u8 {
        (self.confirm_cursor & crate::menu_input::CURSOR_INDEX_MASK) as u8
    }

    /// Selected visible row's record, if any.
    pub fn selected(&self) -> Option<&PrizeRecord> {
        self.rows.get(self.cursor()).map(|&i| &self.block[i])
    }

    /// `true` once the browse cancel exited the session.
    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Drive one frame. `coins` is the live coin bank and `held` the
    /// party's held count of an item id (the two redeem-gate inputs).
    pub fn tick(&mut self, nav: NavButtons, coins: u32, held: impl Fn(u8) -> u8) -> PrizeEvent {
        match self.phase {
            Phase::Done => PrizeEvent::None,
            Phase::Browse => {
                match menu_cursor_nav(&mut self.cursor, self.rows.len() as u32, true, nav) {
                    CursorNav::Confirm => {
                        let Some(rec) = self.selected().copied() else {
                            return PrizeEvent::None;
                        };
                        match redeem_gate(coins, rec.price, held(rec.item_id)) {
                            Err(refusal) => PrizeEvent::Refused(refusal),
                            Ok(()) => {
                                // Retail seeds the Yes/No cursor to 1 (No) and
                                // raises the browse cursor's editing bit.
                                self.confirm_cursor = 1;
                                self.cursor |= 0x1000;
                                self.phase = Phase::Confirm;
                                PrizeEvent::OpenedConfirm
                            }
                        }
                    }
                    CursorNav::Cancel => {
                        self.phase = Phase::Done;
                        PrizeEvent::Exit
                    }
                    CursorNav::Moved => PrizeEvent::Moved,
                    CursorNav::None => PrizeEvent::None,
                }
            }
            Phase::Confirm => match menu_cursor_nav(&mut self.confirm_cursor, 2, true, nav) {
                CursorNav::Confirm => {
                    self.cursor &= crate::menu_input::CURSOR_INDEX_MASK;
                    self.phase = Phase::Browse;
                    if self.confirm_cursor() == 1 {
                        return PrizeEvent::BackToList;
                    }
                    let Some(rec) = self.selected().copied() else {
                        return PrizeEvent::BackToList;
                    };
                    PrizeEvent::Redeemed {
                        item_id: rec.item_id,
                        price: rec.price,
                        gate: rec.gate,
                    }
                }
                CursorNav::Cancel => {
                    self.cursor &= crate::menu_input::CURSOR_INDEX_MASK;
                    self.phase = Phase::Browse;
                    PrizeEvent::BackToList
                }
                CursorNav::Moved => PrizeEvent::ConfirmMoved,
                CursorNav::None => PrizeEvent::None,
            },
        }
    }
}

/// The state-3 commit deltas: grant one copy, debit the coin bank, set the
/// one-shot gate flag. Returns `false` (no mutation) when the gate would
/// refuse - retail cannot reach the commit in that state, so a `false`
/// here marks a host driving the session with stale inputs.
pub fn apply_redeem(
    coins: &mut u32,
    inventory: &mut HashMap<u8, u8>,
    flags: &mut impl FnMut(u16),
    item_id: u8,
    price: u32,
    gate: u16,
) -> bool {
    let held = *inventory.get(&item_id).unwrap_or(&0);
    if redeem_gate(*coins, price, held).is_err() {
        return false;
    }
    *coins -= price;
    let slot = inventory.entry(item_id).or_insert(0);
    *slot = slot.saturating_add(1);
    if gate != 0 {
        flags(gate);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(confirm: bool, cancel: bool, left: bool, right: bool) -> NavButtons {
        NavButtons::new(confirm, cancel, left, right)
    }

    fn block() -> Vec<PrizeRecord> {
        vec![
            PrizeRecord {
                item_id: 0x10,
                gate: 0,
                price: 100,
            },
            PrizeRecord {
                item_id: 0x20,
                gate: 0x36,
                price: 5000,
            },
            PrizeRecord {
                item_id: 0x30,
                gate: 0,
                price: 20,
            },
            // Terminator: the walk must stop here even though a live
            // record follows (retail's `blez` guard).
            PrizeRecord {
                item_id: 0,
                gate: 0,
                price: 0,
            },
            PrizeRecord {
                item_id: 0x40,
                gate: 0,
                price: 1,
            },
        ]
    }

    #[test]
    fn visible_rows_stop_at_terminator_and_hide_redeemed_gates() {
        // Gate clear: all three live rows visible, nothing past the 0 id.
        assert_eq!(visible_rows(&block(), |_| false), vec![0, 1, 2]);
        // Gate 0x36 set: the one-shot prize disappears, order kept.
        assert_eq!(visible_rows(&block(), |g| g == 0x36), vec![0, 2]);
    }

    #[test]
    fn redeem_gate_orders_coin_test_before_held_cap() {
        assert_eq!(redeem_gate(99, 100, 0), Err(RedeemRefusal::NotEnoughCoins));
        // Both failing: retail tests coins first (`0x801dc3cc` before the
        // `FUN_80042F4C` call).
        assert_eq!(redeem_gate(0, 1, 99), Err(RedeemRefusal::NotEnoughCoins));
        assert_eq!(redeem_gate(100, 100, 99), Err(RedeemRefusal::HeldCap));
        // 98 held still passes - the cap refuses at 99, same 0x63 law as
        // the gold shop.
        assert_eq!(redeem_gate(100, 100, 98), Ok(()));
    }

    #[test]
    fn confirm_defaults_to_no_and_no_returns_to_list() {
        let mut s = PrizeExchangeSession::new(block(), |_| false);
        assert_eq!(
            s.tick(nav(true, false, false, false), 1000, |_| 0),
            PrizeEvent::OpenedConfirm
        );
        assert_eq!(s.confirm_cursor(), 1, "retail seeds DAT_801E46D0 = 1 (No)");
        // Confirming the default lands on No -> back to the list, no grant.
        assert_eq!(
            s.tick(nav(true, false, false, false), 1000, |_| 0),
            PrizeEvent::BackToList
        );
        assert!(!s.is_done());
    }

    #[test]
    fn yes_commit_emits_redeem_and_apply_sets_the_one_shot_gate() {
        let mut s = PrizeExchangeSession::new(block(), |_| false);
        // Move to the gated 5000-coin prize (row 1).
        assert_eq!(
            s.tick(nav(false, false, false, true), 9000, |_| 0),
            PrizeEvent::Moved
        );
        assert_eq!(
            s.tick(nav(true, false, false, false), 9000, |_| 0),
            PrizeEvent::OpenedConfirm
        );
        // Left from No wraps/steps to Yes.
        assert_eq!(
            s.tick(nav(false, false, true, false), 9000, |_| 0),
            PrizeEvent::ConfirmMoved
        );
        let ev = s.tick(nav(true, false, false, false), 9000, |_| 0);
        assert_eq!(
            ev,
            PrizeEvent::Redeemed {
                item_id: 0x20,
                price: 5000,
                gate: 0x36
            }
        );

        // Host applies + rebuilds: coins debited, item granted, one-shot
        // row gone from the rebuilt list.
        let mut coins = 9000u32;
        let mut inv = HashMap::new();
        let mut set = std::collections::HashSet::new();
        assert!(apply_redeem(
            &mut coins,
            &mut inv,
            &mut |g| {
                set.insert(g);
            },
            0x20,
            5000,
            0x36
        ));
        assert_eq!(coins, 4000);
        assert_eq!(inv.get(&0x20), Some(&1));
        assert!(set.contains(&0x36));
        s.rebuild(|g| set.contains(&g));
        assert_eq!(s.rows().count(), 2);
    }

    #[test]
    fn refusals_buzz_and_browse_cancel_exits() {
        let mut s = PrizeExchangeSession::new(block(), |_| false);
        assert_eq!(
            s.tick(nav(true, false, false, false), 0, |_| 0),
            PrizeEvent::Refused(RedeemRefusal::NotEnoughCoins)
        );
        assert_eq!(
            s.tick(nav(true, false, false, false), 1000, |_| 99),
            PrizeEvent::Refused(RedeemRefusal::HeldCap)
        );
        assert_eq!(
            s.tick(nav(false, true, false, false), 1000, |_| 0),
            PrizeEvent::Exit
        );
        assert!(s.is_done());
    }

    #[test]
    fn apply_redeem_refuses_on_stale_inputs() {
        let mut coins = 10u32;
        let mut inv = HashMap::new();
        assert!(!apply_redeem(
            &mut coins,
            &mut inv,
            &mut |_| {},
            0x10,
            100,
            0
        ));
        assert_eq!(coins, 10);
        assert!(inv.is_empty());
    }
}
