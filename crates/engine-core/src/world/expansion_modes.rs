//! The expansion game modes on the `World`: entering the mode seat
//! ([`legaia_engine_expansion::modes::PackModeSeat`]), ticking it under
//! [`SceneMode::Expansion`], and granting a finished session's rewards.
//!
//! The instance list is the active pack's `modes.toml` (with its
//! `cards.toml` as the duel's card book); with no pack active it is the
//! built-in practice set (`legaia_engine_modes::active::builtin_sections`),
//! so every mode can be played without one. Nothing here runs unless a
//! host enters the seat: a world that never does is untouched.
//!
//! Rewards are seat 0's: a currency change lands in the party gold purse or
//! the casino coin bank (each clamped at its retail ceiling, floored at
//! zero); booster and card grants are kept in
//! [`legaia_engine_expansion::ExpansionState::pending_card_rewards`] for the
//! card economy.

use legaia_engine_expansion::modes::{ModeSource, PackModeSeat, SeatStep, pending_cards};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{ActiveMode, BuildError, BuildOptions, ModeEntry, entries};
use legaia_engine_modes::session::{Currency, ModeRng, Reward};
use legaia_expansion_format::cards::CardSection;
use legaia_expansion_format::modes::ModeSection;

use super::World;
use crate::casino_coin_bank::COIN_BANK_CEILING;

/// The party gold purse's ceiling (the op `0x3A` clamp).
pub(in crate::world) const GOLD_CEILING: i64 = 9_999_999;

impl World {
    /// The sections the mode seat builds from, and where they came from:
    /// the active pack's when it defines any mode, else the built-in set.
    fn mode_sections(&self) -> (ModeSource, &ModeSection, &CardSection) {
        if let Some(p) = self.expansion.pack.as_deref()
            && !entries(&p.modes).is_empty()
        {
            return (ModeSource::Pack, &p.modes, &p.cards);
        }
        let (modes, cards) = legaia_engine_modes::active::builtin_sections();
        (ModeSource::Builtin, modes, cards)
    }

    /// The mode instances the seat offers right now.
    pub fn pack_mode_entries(&self) -> Vec<ModeEntry> {
        entries(self.mode_sections().1)
    }

    /// A session seed: the frame counter and the world RNG mixed with the
    /// instance id, so two entries in one frame still differ by instance.
    fn pack_mode_seed(&self, id: &str) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for b in id.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
        }
        h ^ self.frame.rotate_left(17) ^ u64::from(self.rng_state)
    }

    /// The mode the seat returns to: the one running now, or the one a seat
    /// already open returns to.
    fn pack_mode_return(&self) -> SceneMode {
        match &self.expansion.mode_seat {
            Some(seat) if self.mode == SceneMode::Expansion => seat.return_mode,
            _ if self.mode == SceneMode::Expansion => SceneMode::Field,
            _ => self.mode,
        }
    }

    /// Open the mode picker, suspending the current mode.
    pub fn open_pack_mode_picker(&mut self) {
        let return_mode = self.pack_mode_return();
        let (source, modes, _) = self.mode_sections();
        let seat = PackModeSeat::picker(return_mode, source, modes);
        self.expansion.mode_seat = Some(Box::new(seat));
        self.mode = SceneMode::Expansion;
    }

    /// Enter mode instance `id` directly, suspending the current mode.
    pub fn enter_pack_mode(&mut self, id: &str) -> Result<(), BuildError> {
        let return_mode = self.pack_mode_return();
        let seed = self.pack_mode_seed(id);
        let opts = self.pack_mode_build_options();
        let (source, modes, cards) = self.mode_sections();
        let mode = ActiveMode::build(modes, cards, id, seed, &opts)?;
        let seat = PackModeSeat::playing(return_mode, source, mode);
        self.expansion.mode_seat = Some(Box::new(seat));
        self.mode = SceneMode::Expansion;
        Ok(())
    }

    /// Close the seat and restore the suspended mode. A session still
    /// running is forfeited (no rewards). No-op while no seat is open.
    pub fn exit_pack_mode(&mut self) {
        if self.expansion.cards.seat.is_some() {
            self.close_card_screen();
            return;
        }
        let Some(seat) = self.expansion.mode_seat.take() else {
            return;
        };
        if self.mode == SceneMode::Expansion {
            self.mode = seat.return_mode;
        }
    }

    /// One frame of the seat, off this frame's pad (seat 0).
    pub(crate) fn tick_pack_mode(&mut self) {
        if self.expansion.cards.seat.is_some() {
            self.tick_card_seat();
            return;
        }
        let Some(mut seat) = self.expansion.mode_seat.take() else {
            // Mode is Expansion but no seat (a pack swap dropped it).
            self.mode = SceneMode::Field;
            return;
        };
        let (pad, prev) = (self.input.pad(), self.input.pad_prev());
        let id = match &seat.stage {
            legaia_engine_expansion::modes::ModeStage::Picker {
                entries, cursor, ..
            } => entries
                .get(*cursor)
                .map(|e| e.id.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let seed = self.pack_mode_seed(&id);
        let mut rng = ModeRng::new(seed ^ 0x5EED);
        let opts = match &seat.stage {
            legaia_engine_expansion::modes::ModeStage::Picker { .. } => {
                self.pack_mode_build_options()
            }
            _ => BuildOptions::default(),
        };
        let step = {
            let (_, modes, cards) = self.mode_sections();
            seat.step(pad, prev, modes, cards, &opts, seed, &mut rng)
        };
        match step {
            SeatStep::Stay => self.expansion.mode_seat = Some(seat),
            SeatStep::Leave => self.mode = seat.return_mode,
            SeatStep::Settled {
                mode_id,
                settlement,
            } => {
                self.grant_pack_mode_rewards(&mode_id, &settlement.results.rewards);
                self.expansion.mode_seat = Some(seat);
            }
        }
    }

    /// Credit seat 0's rewards: currency to its purse, cards to the pending
    /// list.
    pub fn grant_pack_mode_rewards(&mut self, mode_id: &str, rewards: &[Reward]) {
        for r in rewards {
            if let Reward::Currency {
                seat: 0,
                currency,
                amount,
            } = r
            {
                match currency {
                    Currency::Gold => {
                        let v = (i64::from(self.party.money) + amount).clamp(0, GOLD_CEILING);
                        self.party.money = v as i32;
                    }
                    Currency::Coins => {
                        let v = (i64::from(self.minigames.casino_coins) + amount)
                            .clamp(0, i64::from(COIN_BANK_CEILING));
                        self.minigames.casino_coins = v as u32;
                    }
                }
            }
        }
        self.expansion
            .pending_card_rewards
            .extend(pending_cards(mode_id, rewards));
        // The card economy: a lost stake leaves the collection, the
        // winnings join it.
        self.settle_card_stakes(rewards);
        self.collect_card_grants();
    }
}
