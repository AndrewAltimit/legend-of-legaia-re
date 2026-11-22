//! Driving the fishing point-exchange sub-screen - the one input kernel both
//! play hosts route through.
//!
//! The sub-screen itself is state on the world
//! ([`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_exchange`]) and a draw
//! (`legaia_engine_ui::fishing::exchange`). What was host-side was *holding
//! it open*: the native window opened it on a key and kept it across frames,
//! so its HUD composed the screen every frame, while the browser play page's
//! only entry opened it, bought one row and closed it inside one call - so
//! the page's HUD compose, which reads the same world field, always found it
//! closed and the screen never drew there.
//!
//! [`World::fishing_exchange_input`] is that host-side logic moved into the
//! engine: toggle (banking the live session's points first, as retail's
//! running pool has them, and opening on the pond's own page), venue switch,
//! and the pad keys - the cursor keys, confirm and cancel - which run the
//! list, the quantity picker and the confirm exactly as the pad does
//! ([`World::fishing_exchange_pad`]). A confirm on the list opens the picker;
//! a buy is the confirm's Yes. Hosts map their own input onto
//! [`ExchangeInput`] and hand over the two decoded venue pages.

use crate::world::World;
use legaia_engine_fishing::fishing::{PrizeExchange, PrizePurchase};

/// One input to the open (or closed) point-exchange sub-screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeInput {
    /// Open the sub-screen on the pond's own page, or close it when open.
    Toggle,
    /// Up: the list's cursor (wrapping from the first visible row to the
    /// last), the confirm's row.
    Up,
    /// Down: the list's cursor (wrapping from the last row to the first
    /// visible one), the confirm's row.
    Down,
    /// Switch to the other venue's page.
    SwitchVenue,
    /// Confirm (`& 0x44`): the list opens the quantity picker on an
    /// available row, the picker opens the confirm, the confirm's Yes buys.
    Buy,
    /// Left: the picker's quantity down; on the list, the venue switch.
    Left,
    /// Right: the picker's quantity up; on the list, the venue switch.
    Right,
    /// Cancel (`& 0x21`): back one screen, and from the list closed.
    Cancel,
}

impl ExchangeInput {
    /// Decode the small integer code a browser host passes across the wasm
    /// boundary: `0` toggle, `1` up, `2` down, `3` venue, `4` confirm,
    /// `5` left, `6` right, `7` cancel.
    pub fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            0 => Self::Toggle,
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::SwitchVenue,
            4 => Self::Buy,
            5 => Self::Left,
            6 => Self::Right,
            7 => Self::Cancel,
            _ => return None,
        })
    }

    /// The packed pad edge the input stands for, for the inputs the screens
    /// read off the pad.
    pub fn pad_edge(self) -> Option<u32> {
        use legaia_engine_fishing::fishing::{
            PRIZE_CANCEL, PRIZE_CONFIRM, PRIZE_DOWN, PRIZE_LEFT, PRIZE_RIGHT, PRIZE_UP,
        };
        Some(match self {
            Self::Up => PRIZE_UP,
            Self::Down => PRIZE_DOWN,
            Self::Left => PRIZE_LEFT,
            Self::Right => PRIZE_RIGHT,
            // Cross (`0x40`) and Circle (`0x20`), one bit of each mask.
            Self::Buy => PRIZE_CONFIRM & 0x40,
            Self::Cancel => PRIZE_CANCEL & 0x20,
            Self::Toggle | Self::SwitchVenue => return None,
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
    /// The cursor, the quantity, the screen or the venue moved.
    Moved,
    /// A row was bought.
    Bought(PrizePurchase),
    /// The input did nothing: the screen was closed for a non-toggle input,
    /// or the list refused to open the picker on an unavailable row (points,
    /// stack cap, one-time latch; cue `0x22`).
    Refused,
}

impl World {
    /// Apply one [`ExchangeInput`] to the fishing point-exchange sub-screen,
    /// with `venues` the two decoded venue pages (Buma, Vidna).
    ///
    /// Opening banks the live session's point total into
    /// [`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_points`] first: retail credits
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
                // The pond's own page: the venue select points the list's
                // table pointer `PTR_DAT_801D90B8` at Buma's or Vidna's
                // (`0x801CF5F8..0x801CF63C`), as it picks the spawn page.
                let venue = self
                    .minigames
                    .fishing
                    .as_ref()
                    .map_or(0, |s| s.venue.min(1));
                self.open_fishing_exchange(venues[venue].clone());
                ExchangeOutcome::Opened(venue)
            }
            _ if !open => ExchangeOutcome::Refused,
            ExchangeInput::SwitchVenue => {
                let current = self
                    .minigames
                    .fishing_exchange
                    .as_ref()
                    .map_or(0, |e| e.venue.min(1));
                self.open_fishing_exchange(venues[1 - current].clone());
                ExchangeOutcome::Moved
            }
            _ => match input.pad_edge() {
                Some(edge) => self.fishing_exchange_pad(edge),
                None => ExchangeOutcome::Refused,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_minigame_formats::fishing_exchange::ExchangeRow;

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
        w.minigames.fishing_prize_venues = Some(venues.clone());
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
        w.minigames.fishing_prize_venues = Some(venues.clone());
        w.minigames.fishing_points = 1000;
        w.fishing_exchange_input(&venues, ExchangeInput::Toggle);
        w.fishing_exchange_input(&venues, ExchangeInput::SwitchVenue);
        assert_eq!(
            w.minigames.fishing_exchange.as_ref().map(|e| e.venue),
            Some(1)
        );
        let before = w.minigames.fishing_points;
        // A confirm on the list opens the picker, a second its confirm on
        // No; nothing is spent until Up and a confirm on Yes.
        for _ in 0..2 {
            assert_eq!(
                w.fishing_exchange_input(&venues, ExchangeInput::Buy),
                ExchangeOutcome::Moved
            );
        }
        assert_eq!(w.minigames.fishing_points, before);
        w.fishing_exchange_input(&venues, ExchangeInput::Up);
        match w.fishing_exchange_input(&venues, ExchangeInput::Buy) {
            ExchangeOutcome::Bought(p) => {
                assert_eq!(p.qty, 1);
                assert_eq!(before - w.minigames.fishing_points, p.cost as i32)
            }
            other => panic!("expected a buy, got {other:?}"),
        }
        assert!(
            w.minigames.fishing_exchange.is_some(),
            "a buy keeps the screen open"
        );
    }

    /// The list a pad opened must close on the pad: the soak harness found a
    /// fishing run parked on it for good, since only host keys drove it.
    #[test]
    fn the_pad_moves_buys_and_closes_the_list() {
        let mut w = World::new();
        w.minigames.fishing_prize_venues = Some([venue(0), venue(1)]);
        w.minigames.fishing_points = 1000;
        let venues = [venue(0), venue(1)];
        w.fishing_exchange_input(&venues, ExchangeInput::Toggle);
        let cursor = |w: &World| w.minigames.fishing_exchange.as_ref().map(|e| e.cursor);
        let start = cursor(&w).unwrap();
        w.fishing_exchange_pad(0x4000); // Down
        assert_eq!(cursor(&w), Some(start + 1));
        w.fishing_exchange_pad(0x1000); // Up
        assert_eq!(cursor(&w), Some(start));
        let before = w.minigames.fishing_points;
        use legaia_engine_fishing::fishing::PrizeScreen;
        let screen = |w: &World| w.minigames.fishing_exchange_dialog.screen;
        w.fishing_exchange_pad(0x40); // Cross: the quantity picker
        assert_eq!(screen(&w), PrizeScreen::Quantity);
        assert_eq!(
            w.minigames.fishing_points, before,
            "the picker spends nothing"
        );
        w.fishing_exchange_pad(0x2000); // Right: two
        assert_eq!(w.minigames.fishing_exchange_dialog.qty, 2);
        w.fishing_exchange_pad(0x40); // Cross: the confirm, on No
        assert_eq!(screen(&w), PrizeScreen::Confirm);
        w.fishing_exchange_pad(0x40); // Cross on No: back to the picker
        assert_eq!(screen(&w), PrizeScreen::Quantity);
        w.fishing_exchange_pad(0x40);
        w.fishing_exchange_pad(0x1000); // Up: Yes
        w.fishing_exchange_pad(0x40); // Cross on Yes buys two
        assert_eq!(screen(&w), PrizeScreen::List);
        let price = venue(0).rows[start].price as i32;
        assert_eq!(before - w.minigames.fishing_points, 2 * price, "two bought");
        assert!(w.minigames.fishing_exchange.is_some());
        w.fishing_exchange_pad(0x20); // Circle
        assert!(w.minigames.fishing_exchange.is_none(), "Circle closes");
        assert!(w.minigames.pending_sfx.contains(&0x37));
    }

    /// The list opens on the pond's own page (`PTR_DAT_801D90B8`, set by the
    /// venue select) and on the cursor it was left on (`DAT_801D90A8`), which
    /// starts at `0` and goes to the last row through the list's entry clamp.
    #[test]
    fn the_list_opens_on_the_ponds_page_and_its_last_cursor() {
        let venues = [venue(0), venue(1)];
        let mut w = World::new();
        w.minigames.fishing_prize_venues = Some(venues.clone());
        w.minigames.fishing_points = 1000;
        let tables = legaia_engine_fishing::fishing::FishingTables {
            species: Vec::new(),
            spawn: [Vec::new(), Vec::new()],
            cadence: Vec::new(),
        };
        w.enter_fishing_session(&tables, 1, None);
        w.minigames.fishing_points = 1000;
        assert_eq!(
            w.fishing_exchange_input(&venues, ExchangeInput::Toggle),
            ExchangeOutcome::Opened(1)
        );
        let cursor = |w: &World| w.minigames.fishing_exchange.as_ref().map(|e| e.cursor);
        assert_eq!(cursor(&w), Some(0), "the overlay's zeroed word");
        // Row 0 (10 points) is visible at 1000, so the first Up wraps to 5.
        w.fishing_exchange_input(&venues, ExchangeInput::Up);
        assert_eq!(cursor(&w), Some(5));
        w.fishing_exchange_input(&venues, ExchangeInput::Up);
        w.fishing_exchange_input(&venues, ExchangeInput::Toggle);
        w.fishing_exchange_input(&venues, ExchangeInput::Toggle);
        assert_eq!(cursor(&w), Some(4), "kept across a close and reopen");
    }

    #[test]
    fn wasm_codes_round_trip() {
        for (code, want) in [
            (0, ExchangeInput::Toggle),
            (1, ExchangeInput::Up),
            (2, ExchangeInput::Down),
            (3, ExchangeInput::SwitchVenue),
            (4, ExchangeInput::Buy),
            (5, ExchangeInput::Left),
            (6, ExchangeInput::Right),
            (7, ExchangeInput::Cancel),
        ] {
            assert_eq!(ExchangeInput::from_code(code), Some(want));
        }
        assert_eq!(ExchangeInput::from_code(8), None);
    }
}
