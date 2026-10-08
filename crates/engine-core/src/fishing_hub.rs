//! The fishing venue's hub screen: the rules half (`FishingHub`, its text
//! tables and the `PondSession` hub tick) lives in
//! `legaia_engine_minigames::fishing_hub` and is re-exported here; this file
//! keeps the `World` methods that route the hub's exits into the world's
//! exchange sub-screen and wallet.

pub use legaia_engine_minigames::fishing_hub::*;

impl crate::world::World {
    /// The fishing hub's frame on the world: the step every world host (the
    /// native window, the browser play page) runs before the pond.
    ///
    /// Returns `true` when the hub owns the frame, so the pond must not tick.
    /// Row 3 opens the world's exchange sub-screen through
    /// [`crate::world::World::fishing_exchange_input`] (the same kernel both
    /// hosts' exchange keys drive), and the hub waits under it; row 4 leaves
    /// the venue exactly as each host's own quit key does
    /// ([`crate::world::World::exit_fishing`] then the minigame round-trip
    /// close).
    pub fn tick_fishing_hub(&mut self) -> bool {
        use crate::fishing_exchange_input::ExchangeInput;
        let exchange_open = self.minigames.fishing_exchange.is_some();
        let edge = (self.input.pad() & !self.input.pad_prev()).rotate_right(8) as u32;
        let bag = &self.party.inventory;
        let Some(session) = self.minigames.fishing.as_mut() else {
            return false;
        };
        if !exchange_open {
            session.hub_exchange_closed();
        }
        if exchange_open {
            let hub_up = session.hub().is_some();
            self.fishing_exchange_pad(edge);
            return hub_up;
        }
        let step = session.hub_step(edge, |id| {
            i32::from(bag.get(&(id as u8)).copied().unwrap_or(0))
        });
        let open = session.hub().is_some();
        if let Some(sfx) = step.sfx {
            self.minigames.pending_sfx.push(sfx);
        }
        match step.exit {
            Some(HubExit::Leave) => {
                self.exit_fishing();
                self.close_minigame_round_trip();
                return true;
            }
            Some(HubExit::OpenExchange) => {
                match self.minigames.fishing_prize_venues.clone() {
                    Some(venues) => {
                        self.fishing_exchange_input(&venues, ExchangeInput::Toggle);
                    }
                    // No pages decoded: nothing to open, so the menu stays.
                    None => {
                        if let Some(s) = self.minigames.fishing.as_mut() {
                            s.hub_exchange_closed();
                        }
                    }
                }
            }
            _ => {}
        }
        open || step.exit.is_some()
    }

    /// The prize list's own pad step (state `0x78`, `FUN_801D0C3C(1)`), fed
    /// the packed pad edge. Without it the list opened from the hub's row 3
    /// answered only host keys (the native window's `P` family, the browser
    /// page's panel buttons), so a pad-only player - a gamepad, the play page
    /// - was left on a screen nothing could close.
    ///
    /// Read off the disassembly (`0x801D0CB8..0x801D0DB0`): Up (`0x1000`) and
    /// Down (`0x4000`) move the cursor with SFX `0x21`, clamped to the list
    /// floor; `& 0x44` (Cross / L1) buys at the cursor through the
    /// availability test `FUN_801D6F90` - SFX `0x20` when it passes, `0x22`
    /// when refused; `& 0x21` (Circle / L2) cancels with SFX `0x37` back to
    /// the hub menu (state `0x64`). Retail reads the cursor keys off the
    /// auto-repeat word; this reads the edge. Left / Right switch the venue
    /// page - a port affordance retail has no key for (each pond shows its
    /// own page).
    ///
    /// The buy is one unit: retail's quantity picker (`0x7A`) and confirm
    /// (`0x79`) screens are not modelled; the grant, the spend and the
    /// one-time latch are [`crate::world::World::fishing_exchange_buy`]'s.
    pub(crate) fn fishing_exchange_pad(&mut self, edge: u32) {
        use crate::fishing_exchange_input::{ExchangeInput, ExchangeOutcome};
        const UP: u32 = 0x1000;
        const RIGHT: u32 = 0x2000;
        const DOWN: u32 = 0x4000;
        const LEFT: u32 = 0x8000;
        const CONFIRM: u32 = 0x44;
        const CANCEL: u32 = 0x21;
        let Some(venues) = self.minigames.fishing_prize_venues.clone() else {
            // No pages decoded: nothing the list could show, so any cancel
            // or confirm closes it rather than holding the pad.
            if edge & (CONFIRM | CANCEL) != 0 {
                self.close_fishing_exchange();
            }
            return;
        };
        let mut sfx = Vec::new();
        for (mask, input) in [
            (UP, ExchangeInput::Up),
            (DOWN, ExchangeInput::Down),
            (LEFT | RIGHT, ExchangeInput::SwitchVenue),
        ] {
            if edge & mask != 0
                && self.fishing_exchange_input(&venues, input) != ExchangeOutcome::Refused
            {
                sfx.push(0x21);
            }
        }
        if edge & CONFIRM != 0 {
            match self.fishing_exchange_input(&venues, ExchangeInput::Buy) {
                // The purchase itself: retail's confirm screen `FUN_801d06c8`
                // stores runtime cue `0x206` straight into ring slot 0 on its
                // Yes arm (`0x801D089C`), after the list's and the quantity
                // screen's `0x20` selects this one press stands in for.
                ExchangeOutcome::Bought(_) => self
                    .audio
                    .sfx_ring_ops
                    .push(crate::world::SfxRingOp::WriteSlot(0, 0x206)),
                _ => sfx.push(0x22),
            }
        }
        if edge & CANCEL != 0 && self.minigames.fishing_exchange.is_some() {
            self.fishing_exchange_input(&venues, ExchangeInput::Toggle);
            sfx.push(0x37);
        }
        self.minigames.pending_sfx.extend(sfx);
    }

    /// The hub's draw lines on the world: the session's layout with the
    /// tackle rows named off the SCUS item table and counted off the bag.
    /// Empty while no hub is up (or no hub text decoded). The one layout
    /// both world hosts hand to `legaia_engine_ui::ui_fishing_hub`.
    pub fn fishing_hub_lines(&self) -> Vec<HubLine> {
        let (Some(s), Some(text)) = (
            self.minigames.fishing.as_ref(),
            self.minigames.fishing_hub_text.as_ref(),
        ) else {
            return Vec::new();
        };
        let names = self.menu.text.as_ref();
        let bag = &self.party.inventory;
        s.hub_lines(text, |id| {
            let name = names
                .and_then(|t| t.item_name(id as u8))
                .map(|n| n.as_bytes().to_vec())
                .unwrap_or_else(|| format!("item {id:#04x}").into_bytes());
            (name, i32::from(bag.get(&(id as u8)).copied().unwrap_or(0)))
        })
    }
}
