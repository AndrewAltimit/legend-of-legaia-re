//! The mode seat: the expansion game modes (poker, Grid Duel, golf,
//! racing, prop hunt) as one screen the `World` hands the frame to.
//!
//! The seat is in one of three stages:
//!
//! - **Picker** - the instance list (the active pack's, or the built-in
//!   practice set without one). Up / Down choose, Cross enters, Circle or
//!   Start leaves.
//! - **Playing** - one [`ActiveMode`] ticked with the pad. Start raises a
//!   quit prompt (Cross quits and forfeits, Circle or Start resumes); the
//!   session never sees the frames the prompt is up.
//! - **Results** - the finished session's placements and what was granted;
//!   any of Cross, Circle or Start leaves.
//!
//! What a finished session grants is reported in [`SeatStep::Settled`]; the
//! `World` credits the currency to its purses and keeps the card grants in
//! [`PendingCardReward`]s until the card economy collects them. A forfeited
//! session grants nothing.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{
    ActiveMode, BuildOptions, ModeEntry, ModeKind, ModeView, Settlement, entries,
};
use legaia_engine_modes::session::{Currency, ModeRng, Placement, Reward};
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::cards::CardSection;
use legaia_expansion_format::modes::ModeSection;

/// Log lines the playing stage keeps.
pub const LOG_LINES: usize = 4;

/// Which instance list the seat offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeSource {
    /// The active pack's `modes.toml`.
    Pack,
    /// The built-in practice set.
    Builtin,
}

/// Where the seat is.
#[derive(Debug, Clone)]
pub enum ModeStage {
    Picker {
        entries: Vec<ModeEntry>,
        cursor: usize,
        /// Why the last entry attempt failed.
        error: Option<String>,
    },
    Playing {
        mode: Box<ActiveMode>,
        quit_prompt: bool,
    },
    Results {
        name: String,
        kind: ModeKind,
        /// Every seat, best first; empty when the player quit.
        rows: Vec<Placement>,
        /// What was granted, one line each ("+120 coins").
        granted: Vec<String>,
        /// The session ran to its end (`false`: the player quit).
        completed: bool,
    },
}

/// The seat on `ExpansionState`.
#[derive(Debug, Clone)]
pub struct PackModeSeat {
    /// The mode the world returns to when the seat closes.
    pub return_mode: SceneMode,
    pub source: ModeSource,
    pub stage: ModeStage,
    /// The playing session's recent event lines, newest last.
    pub log: Vec<String>,
    /// Buttons held since the session last gained the pad (entry, or the
    /// quit prompt closing), withheld from it until released - so the Cross
    /// that picked a mode is not also its first move.
    pub held_over: u16,
}

/// A card grant waiting for the card economy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingCardReward {
    Booster { id: String, from_mode: String },
    Card { id: String, from_mode: String },
}

/// What one [`PackModeSeat::step`] asks of the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatStep {
    /// Nothing beyond the seat itself.
    Stay,
    /// Close the seat and restore [`PackModeSeat::return_mode`].
    Leave,
    /// Session `mode_id` finished: grant seat 0's rewards. The seat has
    /// moved to its results stage.
    Settled {
        mode_id: String,
        settlement: Settlement,
    },
}

impl PackModeSeat {
    /// A seat on the instance picker.
    pub fn picker(return_mode: SceneMode, source: ModeSource, modes: &ModeSection) -> Self {
        Self {
            return_mode,
            source,
            stage: ModeStage::Picker {
                entries: entries(modes),
                cursor: 0,
                error: None,
            },
            log: Vec::new(),
            held_over: u16::MAX,
        }
    }

    /// A seat already playing `mode`.
    pub fn playing(return_mode: SceneMode, source: ModeSource, mode: ActiveMode) -> Self {
        Self {
            return_mode,
            source,
            stage: ModeStage::Playing {
                mode: Box::new(mode),
                quit_prompt: false,
            },
            log: Vec::new(),
            held_over: u16::MAX,
        }
    }

    /// The running session, while playing.
    pub fn active(&self) -> Option<&ActiveMode> {
        match &self.stage {
            ModeStage::Playing { mode, .. } => Some(mode),
            _ => None,
        }
    }

    /// The running session's view, while playing.
    pub fn view(&self) -> Option<ModeView> {
        self.active().map(ActiveMode::view)
    }

    /// One frame. `pad` / `prev` are this and last frame's raw pad words;
    /// `modes` / `cards` the sections a picker builds from; `seed` seeds a
    /// session the picker enters; `rng` rolls a win's reward rules.
    pub fn step(
        &mut self,
        pad: u16,
        prev: u16,
        modes: &ModeSection,
        cards: &CardSection,
        seed: u64,
        rng: &mut ModeRng,
    ) -> SeatStep {
        let pressed = |b: PadButton| pad & !prev & b.mask() != 0;
        match &mut self.stage {
            ModeStage::Picker {
                entries,
                cursor,
                error,
            } => {
                if pressed(PadButton::Circle) || pressed(PadButton::Start) {
                    return SeatStep::Leave;
                }
                let n = entries.len();
                if n == 0 {
                    return SeatStep::Stay;
                }
                if pressed(PadButton::Up) {
                    *cursor = (*cursor + n - 1) % n;
                }
                if pressed(PadButton::Down) {
                    *cursor = (*cursor + 1) % n;
                }
                if pressed(PadButton::Cross) {
                    let id = entries[*cursor].id.clone();
                    match ActiveMode::build(modes, cards, &id, seed, &BuildOptions::default()) {
                        Ok(mode) => {
                            self.stage = ModeStage::Playing {
                                mode: Box::new(mode),
                                quit_prompt: false,
                            };
                            self.log.clear();
                            self.held_over = u16::MAX;
                        }
                        Err(e) => *error = Some(e.to_string()),
                    }
                }
                SeatStep::Stay
            }
            ModeStage::Playing { mode, quit_prompt } => {
                if *quit_prompt {
                    if pressed(PadButton::Cross) {
                        self.stage = ModeStage::Results {
                            name: mode.name.clone(),
                            kind: mode.kind(),
                            rows: Vec::new(),
                            granted: Vec::new(),
                            completed: false,
                        };
                    } else if pressed(PadButton::Circle) || pressed(PadButton::Start) {
                        *quit_prompt = false;
                        self.held_over = u16::MAX;
                    }
                    return SeatStep::Stay;
                }
                if pressed(PadButton::Start) {
                    *quit_prompt = true;
                    return SeatStep::Stay;
                }
                self.held_over &= pad;
                let lines = mode.tick(&[pad & !self.held_over]);
                self.log.extend(lines);
                let excess = self.log.len().saturating_sub(LOG_LINES);
                self.log.drain(..excess);
                let Some(settlement) = mode.settle(rng) else {
                    return SeatStep::Stay;
                };
                let rows = settlement.results.placements.clone();
                let mode_id = mode.id.clone();
                self.stage = ModeStage::Results {
                    name: mode.name.clone(),
                    kind: mode.kind(),
                    rows,
                    granted: granted_lines(&settlement.results.rewards),
                    completed: true,
                };
                SeatStep::Settled {
                    mode_id,
                    settlement,
                }
            }
            ModeStage::Results { .. } => {
                if pressed(PadButton::Cross)
                    || pressed(PadButton::Circle)
                    || pressed(PadButton::Start)
                {
                    SeatStep::Leave
                } else {
                    SeatStep::Stay
                }
            }
        }
    }
}

/// Seat 0's rewards as result-screen lines.
pub fn granted_lines(rewards: &[Reward]) -> Vec<String> {
    rewards
        .iter()
        .filter_map(|r| match r {
            Reward::Currency {
                seat: 0,
                currency,
                amount,
            } => {
                let unit = match currency {
                    Currency::Gold => "gold",
                    Currency::Coins => "coins",
                };
                Some(format!("{amount:+} {unit}"))
            }
            Reward::Booster { seat: 0, booster } => Some(format!("Booster: {booster}")),
            Reward::Card { seat: 0, card } => Some(format!("Card: {card}")),
            _ => None,
        })
        .collect()
}

/// Seat 0's card grants as pending rewards.
pub fn pending_cards(mode_id: &str, rewards: &[Reward]) -> Vec<PendingCardReward> {
    rewards
        .iter()
        .filter_map(|r| match r {
            Reward::Booster { seat: 0, booster } => Some(PendingCardReward::Booster {
                id: booster.clone(),
                from_mode: mode_id.to_string(),
            }),
            Reward::Card { seat: 0, card } => Some(PendingCardReward::Card {
                id: card.clone(),
                from_mode: mode_id.to_string(),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_modes::active::builtin_sections;

    fn press(seat: &mut PackModeSeat, b: PadButton, rng: &mut ModeRng) -> SeatStep {
        let (modes, cards) = builtin_sections();
        seat.step(0, 0, modes, cards, 1, rng);
        seat.step(b.mask(), 0, modes, cards, 1, rng)
    }

    #[test]
    fn picker_enters_and_quit_prompt_forfeits() {
        let (modes, _) = builtin_sections();
        let mut rng = ModeRng::new(0);
        let mut seat = PackModeSeat::picker(SceneMode::Field, ModeSource::Builtin, modes);
        press(&mut seat, PadButton::Down, &mut rng);
        press(&mut seat, PadButton::Cross, &mut rng);
        assert_eq!(
            seat.active().map(|m| m.kind()),
            Some(legaia_engine_modes::active::ModeKind::CardDuel)
        );
        press(&mut seat, PadButton::Start, &mut rng);
        assert!(matches!(
            seat.stage,
            ModeStage::Playing {
                quit_prompt: true,
                ..
            }
        ));
        press(&mut seat, PadButton::Cross, &mut rng);
        assert!(matches!(
            seat.stage,
            ModeStage::Results {
                completed: false,
                ..
            }
        ));
        assert_eq!(
            press(&mut seat, PadButton::Cross, &mut rng),
            SeatStep::Leave
        );
    }

    #[test]
    fn granted_lines_name_only_seat_zero() {
        let r = vec![
            Reward::Currency {
                seat: 0,
                currency: Currency::Coins,
                amount: 120,
            },
            Reward::Currency {
                seat: 1,
                currency: Currency::Coins,
                amount: -120,
            },
            Reward::Booster {
                seat: 0,
                booster: "b".into(),
            },
        ];
        assert_eq!(granted_lines(&r), vec!["+120 coins", "Booster: b"]);
        assert_eq!(pending_cards("m", &r).len(), 1);
    }
}
