//! Driving the fishing point-exchange sub-screen - the one input kernel both
//! play hosts route through.
//!
//! The sub-screen itself is state on the world
//! ([`crate::world::MinigameState::fishing_exchange`]) and a draw
//! (`legaia_engine_ui::ui_fishing_exchange`). What was host-side was *holding
//! it open*: the native window opened it on a key and kept it across frames,
//! so its HUD composed the screen every frame, while the browser play page's
//! only entry opened it, bought one row and closed it inside one call - so
//! the page's HUD compose, which reads the same world field, always found it
//! closed and the screen never drew there.
//!
//! [`World::fishing_exchange_input`] is that host-side logic moved into the
//! engine: toggle (banking the live session's points first, as retail's
//! running pool has them), cursor moves floored at the first visible row,
//! venue switch, and a buy at the cursor. Hosts map their own input onto
//! [`ExchangeInput`] and hand over the two decoded venue pages.

use crate::fishing::{PrizeExchange, PrizePurchase};
use crate::world::World;

/// One input to the open (or closed) point-exchange sub-screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeInput {
    /// Open the sub-screen on the first venue, or close it when open.
    Toggle,
    /// Move the cursor up one row (never above the first visible row).
    Up,
    /// Move the cursor down one row (clamped at the last row).
    Down,
    /// Switch to the other venue's page.
    SwitchVenue,
    /// Buy one of the row under the cursor.
    Buy,
}

impl ExchangeInput {
    /// Decode the small integer code a browser host passes across the wasm
    /// boundary: `0` toggle, `1` up, `2` down, `3` venue, `4` buy.
    pub fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            0 => Self::Toggle,
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::SwitchVenue,
            4 => Self::Buy,
            _ => return None,
        })
    }
}

/// What one [`World::fishing_exchange_input`] call did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeOutcome {
    /// The sub-screen opened (on the venue named).
    Opened(usize),
    /// The sub-screen closed.
    Closed,
    /// The cursor or the venue moved.
    Moved,
    /// A row was bought.
    Bought(PrizePurchase),
    /// The input did nothing: the screen was closed for a non-toggle input,
    /// or the buy did not validate (points, stack cap, one-time latch).
    Refused,
}

impl World {
    /// Apply one [`ExchangeInput`] to the fishing point-exchange sub-screen,
    /// with `venues` the two decoded venue pages (Buma, Vidna).
    ///
    /// Opening banks the live session's point total into
    /// [`crate::world::MinigameState::fishing_points`] first: retail credits
    /// its pool (`_DAT_8008444C`) as each catch lands, while the port credits
    /// the pool only in [`World::exit_fishing`], and the counter is reachable
    /// only while fishing is still active - without the bank it reads the
    /// previous session's total. [`World::fishing_exchange_buy`] pushes the
    /// spent total back into the session record.
    pub fn fishing_exchange_input(
        &mut self,
        venues: &[PrizeExchange; 2],
        input: ExchangeInput,
    ) -> ExchangeOutcome {
        let open = self.minigames.fishing_exchange.is_some();
        match input {
            ExchangeInput::Toggle if open => {
                self.close_fishing_exchange();
                ExchangeOutcome::Closed
            }
            ExchangeInput::Toggle => {
                if let Some(points) = self.minigames.fishing.as_ref().map(|s| s.record.points) {
                    self.minigames.fishing_points = points;
                }
                self.open_fishing_exchange(venues[0].clone());
                ExchangeOutcome::Opened(0)
            }
            _ if !open => ExchangeOutcome::Refused,
            ExchangeInput::Up | ExchangeInput::Down => {
                let points = self.minigames.fishing_points;
                if let Some(ex) = &mut self.minigames.fishing_exchange {
                    let floor = ex.first_visible(points);
                    let last = ex.rows.len().saturating_sub(1);
                    ex.cursor = if input == ExchangeInput::Up {
                        ex.cursor.saturating_sub(1).max(floor)
                    } else {
                        (ex.cursor + 1).min(last)
                    };
                }
                ExchangeOutcome::Moved
            }
            ExchangeInput::SwitchVenue => {
                let current = self
                    .minigames
                    .fishing_exchange
                    .as_ref()
                    .map_or(0, |e| e.venue.min(1));
                self.open_fishing_exchange(venues[1 - current].clone());
                ExchangeOutcome::Moved
            }
            ExchangeInput::Buy => {
                let row = self.minigames.fishing_exchange.as_ref().map(|e| e.cursor);
                match row.and_then(|r| self.fishing_exchange_buy(r, 1)) {
                    Some(p) => ExchangeOutcome::Bought(p),
                    None => ExchangeOutcome::Refused,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::fishing_exchange::ExchangeRow;

    fn venue(v: usize) -> PrizeExchange {
        let rows: Vec<ExchangeRow> = (0..6)
            .map(|row| ExchangeRow {
                row,
                limit: 99,
                price: 10 * (row as u32 + 1),
                item_id: 0x10 + row as u32 + 8 * v as u32,
            })
            .collect();
        PrizeExchange::from_asset(v, &rows, None)
    }

    #[test]
    fn toggle_holds_the_screen_open_across_calls_until_toggled_again() {
        let venues = [venue(0), venue(1)];
        let mut w = World::new();
        assert_eq!(
            w.fishing_exchange_input(&venues, ExchangeInput::Toggle),
            ExchangeOutcome::Opened(0)
        );
        assert!(w.minigames.fishing_exchange.is_some());
        assert_eq!(
            w.fishing_exchange_input(&venues, ExchangeInput::Down),
            ExchangeOutcome::Moved
        );
        assert!(
            w.minigames.fishing_exchange.is_some(),
            "still open after a move"
        );
        assert_eq!(
            w.fishing_exchange_input(&venues, ExchangeInput::Toggle),
            ExchangeOutcome::Closed
        );
        assert!(w.minigames.fishing_exchange.is_none());
    }

    #[test]
    fn inputs_other_than_toggle_are_refused_while_closed() {
        let venues = [venue(0), venue(1)];
        let mut w = World::new();
        for i in [
            ExchangeInput::Up,
            ExchangeInput::Down,
            ExchangeInput::SwitchVenue,
            ExchangeInput::Buy,
        ] {
            assert_eq!(
                w.fishing_exchange_input(&venues, i),
                ExchangeOutcome::Refused
            );
            assert!(w.minigames.fishing_exchange.is_none());
        }
    }

    #[test]
    fn venue_switch_flips_the_page_and_a_buy_spends_points_without_closing() {
        let venues = [venue(0), venue(1)];
        let mut w = World::new();
        w.minigames.fishing_points = 1000;
        w.fishing_exchange_input(&venues, ExchangeInput::Toggle);
        w.fishing_exchange_input(&venues, ExchangeInput::SwitchVenue);
        assert_eq!(
            w.minigames.fishing_exchange.as_ref().map(|e| e.venue),
            Some(1)
        );
        let before = w.minigames.fishing_points;
        match w.fishing_exchange_input(&venues, ExchangeInput::Buy) {
            ExchangeOutcome::Bought(p) => {
                assert_eq!(before - w.minigames.fishing_points, p.cost as i32)
            }
            other => panic!("expected a buy, got {other:?}"),
        }
        assert!(
            w.minigames.fishing_exchange.is_some(),
            "a buy keeps the screen open"
        );
    }

    #[test]
    fn wasm_codes_round_trip() {
        for (code, want) in [
            (0, ExchangeInput::Toggle),
            (1, ExchangeInput::Up),
            (2, ExchangeInput::Down),
            (3, ExchangeInput::SwitchVenue),
            (4, ExchangeInput::Buy),
        ] {
            assert_eq!(ExchangeInput::from_code(code), Some(want));
        }
        assert_eq!(ExchangeInput::from_code(5), None);
    }
}
