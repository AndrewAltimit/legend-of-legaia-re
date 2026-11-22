//! The fishing venue's hub screen: the rules half (`FishingHub`, its text
//! tables and the `PondSession` hub tick) lives in
//! `legaia_engine_fishing::fishing_hub`; this file
//! keeps the `World` methods that route the hub's exits into the world's
//! exchange sub-screen and wallet.

use legaia_engine_fishing::fishing_hub::*;

impl crate::world::World {
    /// The fishing hub's frame on the world: the step every world host (the
    /// native window, the browser play page) runs before the pond.
    ///
    /// Returns `true` when the hub owns the frame, so the pond must not tick.
    /// Row 3 opens the world's exchange sub-screen through
    /// [`crate::world::World::fishing_exchange_input`] (the same kernel both
    /// hosts' exchange keys drive), and the hub waits under it; row 4 starts
    /// the venue's exit fade, after which the venue leaves as each host's own
    /// quit key does ([`crate::world::World::exit_fishing`] then the minigame
    /// round-trip close).
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
            // Row 4 starts the venue's exit fade (state `200`); the session
            // asks to leave once it has run out
            // (`PondSession::take_exit_request`, read in `tick_fishing`).
            Some(HubExit::Leave) => return true,
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

    /// The exchange's own pad step, fed the packed pad edge: the list
    /// (state `0x78`, `FUN_801D0C3C(1)`), the quantity picker (`0x79`,
    /// `FUN_801D092C(1)`) and the confirm (`0x7A`, `FUN_801D06C8(1)`), as
    /// [`legaia_engine_fishing::fishing::PrizeExchange::pad_step`] runs them.
    /// The list's cancel closes it back to the hub menu; the confirm's Yes
    /// grants through [`crate::world::World::fishing_exchange_buy`] and
    /// stores runtime cue `0x206` into ring slot 0 (`0x801D089C`). Retail
    /// reads the cursor keys off the screen's auto-repeat word; this reads
    /// the edge. On the list, Left / Right switch the venue page - a port
    /// affordance retail has no key for (each pond shows its own page).
    ///
    /// The hosts' own exchange keys ([`crate::fishing_exchange_input`]) land
    /// here too, so a key buys only through the picker and the confirm.
    pub fn fishing_exchange_pad(
        &mut self,
        edge: u32,
    ) -> crate::fishing_exchange_input::ExchangeOutcome {
        use crate::fishing_exchange_input::{ExchangeInput, ExchangeOutcome};
        use legaia_engine_fishing::fishing::{
            PRIZE_BOUGHT_SFX, PRIZE_CANCEL, PRIZE_CONFIRM, PRIZE_LEFT, PRIZE_RIGHT, PrizeScreen,
        };
        let Some(venues) = self.minigames.fishing_prize_venues.clone() else {
            // No pages decoded: nothing the list could show, so any cancel
            // or confirm closes it rather than holding the pad.
            if edge & (PRIZE_CONFIRM | PRIZE_CANCEL) != 0 {
                self.close_fishing_exchange();
                return ExchangeOutcome::Closed;
            }
            return ExchangeOutcome::Refused;
        };
        let on_list = self.minigames.fishing_exchange_dialog.screen == PrizeScreen::List;
        if on_list && edge & (PRIZE_LEFT | PRIZE_RIGHT) != 0 {
            self.fishing_exchange_input(&venues, ExchangeInput::SwitchVenue);
            self.minigames.pending_sfx.push(0x21);
            return ExchangeOutcome::Moved;
        }
        let points = self.minigames.fishing_points;
        let mask = self.minigames.fishing_prizes_purchased;
        let bag = &self.party.inventory;
        let mut dlg = self.minigames.fishing_exchange_dialog;
        let Some(ex) = self.minigames.fishing_exchange.as_mut() else {
            return ExchangeOutcome::Refused;
        };
        let step = ex.pad_step(
            &mut dlg,
            edge,
            edge,
            points,
            |id| u32::from(bag.get(&id).copied().unwrap_or(0)),
            mask,
        );
        self.minigames.fishing_exchange_dialog = dlg;
        let mut outcome = match step.sfx {
            Some(0x22) if dlg.screen == PrizeScreen::List => ExchangeOutcome::Refused,
            _ => ExchangeOutcome::Moved,
        };
        if let Some(p) = step.purchase {
            if let Some(bought) = self.fishing_exchange_buy(dlg.row, p.qty) {
                outcome = ExchangeOutcome::Bought(bought);
            }
            // The next frame's list opens on its entry clamp.
            let points = self.minigames.fishing_points;
            if let Some(ex) = self.minigames.fishing_exchange.as_mut() {
                ex.cursor = ex.step_cursor(ex.cursor, points, false, false);
            }
        }
        match step.sfx {
            Some(PRIZE_BOUGHT_SFX) => self.audio.sfx_ring_ops.push(
                legaia_engine_system::sound_state::SfxRingOp::WriteSlot(0, PRIZE_BOUGHT_SFX as i16),
            ),
            Some(cue) => self.minigames.pending_sfx.push(cue),
            None => {}
        }
        if step.closed {
            self.fishing_exchange_input(&venues, ExchangeInput::Toggle);
            outcome = ExchangeOutcome::Closed;
        }
        outcome
    }

    /// The hub's draw lines on the world: the session's layout with the
    /// tackle rows named off the SCUS item table and counted off the bag.
    /// Empty while no hub is up (or no hub text decoded). The one layout
    /// both world hosts draw through `legaia_engine_screens::fishing::fishing_venue_screen`.
    pub fn fishing_hub_lines(&self) -> Vec<HubLine> {
        self.fishing_hub_layers()
            .into_iter()
            .flat_map(|l| l.lines)
            .collect()
    }

    /// [`Self::fishing_hub_lines`] with the panels they sit on, back to
    /// front (`PondSession::hub_layers`).
    pub fn fishing_hub_layers(&self) -> Vec<HubLayer> {
        let (Some(s), Some(text)) = (
            self.minigames.fishing.as_ref(),
            self.minigames.fishing_hub_text.as_ref(),
        ) else {
            return Vec::new();
        };
        let names = self.menu.text.as_ref();
        let bag = &self.party.inventory;
        s.hub_layers(text, |id| {
            let name = names
                .and_then(|t| t.item_name(id as u8))
                .map(|n| n.as_bytes().to_vec())
                .unwrap_or_else(|| format!("item {id:#04x}").into_bytes());
            (name, i32::from(bag.get(&(id as u8)).copied().unwrap_or(0)))
        })
    }
}
