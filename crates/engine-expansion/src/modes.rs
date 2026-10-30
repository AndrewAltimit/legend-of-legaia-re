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
//! A **networked** seat ([`PackModeSeat::networked`]) holds a session a
//! netplay driver advances from every player's pad: the seat never ticks
//! it, only filters the local pad for the driver
//! ([`PackModeSeat::local_pad`]) and keeps the quit prompt. A networked
//! session grants nothing; its results screen only ranks the table.
//!
//! What a finished session grants is reported in [`SeatStep::Settled`]; the
//! `World` credits the currency to its purses and keeps the card grants in
//! [`PendingCardReward`]s until the card economy collects them. A forfeited
//! session grants nothing.

use std::collections::BTreeMap;
use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{
    ActiveMode, BuildOptions, ModeEntry, ModeKind, ModeView, Settlement, entries,
};
use legaia_engine_modes::session::{Currency, ModeRng, Placement, Reward};
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::cards::CardSection;
use legaia_expansion_format::map::MapDef;
use legaia_expansion_format::modes::ModeSection;

use crate::mode_stage::{StageFrame, WorldStage};

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
        /// The local player's seat ("You" on the rows).
        local_seat: usize,
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
    /// A netplay driver advances the session; the seat does not tick it.
    pub networked: bool,
    /// The pack's maps, which a golf course or race played on a map is
    /// built from ([`ActiveMode::build_with_maps`]).
    pub maps: Arc<BTreeMap<String, MapDef>>,
    /// The in-world stage's memory (camera ease, the leader's walk,
    /// banners). Presentation only: the session never reads it.
    pub world_stage: WorldStage,
    /// This frame's in-world stage, while a session played on a map runs
    /// on that map; `None` draws the top-down screen.
    pub stage_frame: Option<StageFrame>,
    /// The player's field pose `[x, y, z, facing]` when the seat opened,
    /// restored when it closes (the in-world stage moves the leader).
    pub player_return: Option<[i16; 4]>,
    /// The map NPC slots the in-world stage drives as other seats' bodies,
    /// with the clip each stood in before (restored when the seat closes).
    pub body_idle: Vec<(u8, u8)>,
    /// The headings of the NPCs a duel table turned to the board, restored
    /// when the seat closes.
    pub body_heading: Vec<(u8, i16)>,
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
            maps: Arc::default(),
            world_stage: WorldStage::default(),
            stage_frame: None,
            player_return: None,
            body_idle: Vec::new(),
            body_heading: Vec::new(),
            networked: false,
        }
    }

    /// The seat with the pack's maps to build map instances from.
    pub fn with_maps(mut self, maps: Arc<BTreeMap<String, MapDef>>) -> Self {
        self.maps = maps;
        self
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
            maps: Arc::default(),
            world_stage: WorldStage::default(),
            stage_frame: None,
            player_return: None,
            body_idle: Vec::new(),
            body_heading: Vec::new(),
            networked: false,
        }
    }

    /// A seat playing `mode` for a netplay driver.
    pub fn networked(return_mode: SceneMode, mode: ActiveMode) -> Self {
        Self {
            networked: true,
            ..Self::playing(return_mode, ModeSource::Builtin, mode)
        }
    }

    /// The running session, while playing.
    pub fn active(&self) -> Option<&ActiveMode> {
        match &self.stage {
            ModeStage::Playing { mode, .. } => Some(mode),
            _ => None,
        }
    }

    /// The running session, for a netplay driver.
    pub fn active_mut(&mut self) -> Option<&mut Box<ActiveMode>> {
        match &mut self.stage {
            ModeStage::Playing { mode, .. } => Some(mode),
            _ => None,
        }
    }

    /// The pad word a networked session should see from this machine's
    /// player this frame: nothing while the quit prompt is up, never Start
    /// (it raises the prompt), and no button held over from entry until it
    /// is released.
    pub fn local_pad(&mut self, pad: u16) -> u16 {
        match &self.stage {
            ModeStage::Playing {
                quit_prompt: false, ..
            } => {
                self.held_over &= pad;
                pad & !self.held_over & !PadButton::Start.mask()
            }
            _ => 0,
        }
    }

    /// Append a networked session's event lines to the log.
    pub fn push_log(&mut self, lines: Vec<String>) {
        self.log.extend(lines);
        let excess = self.log.len().saturating_sub(LOG_LINES);
        self.log.drain(..excess);
    }

    /// The running session's view, while playing.
    pub fn view(&self) -> Option<ModeView> {
        self.active().map(ActiveMode::view)
    }

    /// One frame. `pad` / `prev` are this and last frame's raw pad words;
    /// `modes` / `cards` the sections a picker builds from, `opts` the host
    /// choices it builds with (the player's duel deck); `seed` seeds a
    /// session the picker enters; `rng` rolls a win's reward rules.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        pad: u16,
        prev: u16,
        modes: &ModeSection,
        cards: &CardSection,
        opts: &BuildOptions,
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
                    match ActiveMode::build_with_maps(modes, cards, &self.maps, &id, seed, opts) {
                        Ok(mode) => {
                            self.stage = ModeStage::Playing {
                                mode: Box::new(mode),
                                quit_prompt: false,
                            };
                            self.log.clear();
                            self.held_over = u16::MAX;
                            self.world_stage = WorldStage::default();
                            self.stage_frame = None;
                        }
                        Err(e) => *error = Some(e.to_string()),
                    }
                }
                SeatStep::Stay
            }
            ModeStage::Playing { mode, quit_prompt } => {
                if *quit_prompt {
                    if pressed(PadButton::Cross) {
                        self.world_stage.note_final(mode);
                        self.stage = ModeStage::Results {
                            name: mode.name.clone(),
                            kind: mode.kind(),
                            rows: Vec::new(),
                            granted: Vec::new(),
                            completed: false,
                            local_seat: mode.local_seat,
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
                if self.networked {
                    // The driver ticks the session; the seat only notices
                    // its end. Nothing is granted online.
                    if let Some(results) = mode.results() {
                        self.world_stage.note_final(mode);
                        self.stage = ModeStage::Results {
                            name: mode.name.clone(),
                            kind: mode.kind(),
                            rows: results.placements,
                            granted: Vec::new(),
                            completed: true,
                            local_seat: mode.local_seat,
                        };
                    }
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
                self.world_stage.note_final(mode);
                self.stage = ModeStage::Results {
                    name: mode.name.clone(),
                    kind: mode.kind(),
                    rows,
                    granted: granted_lines(&settlement.results.rewards),
                    completed: true,
                    local_seat: mode.local_seat,
                };
                SeatStep::Settled {
                    mode_id,
                    settlement,
                }
            }
            ModeStage::Results { .. } => {
                // A duel table's reaction shot plays out before the card
                // takes the pad.
                if self.world_stage.holds_results() {
                    return SeatStep::Stay;
                }
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
        let opts = BuildOptions::default();
        seat.step(0, 0, modes, cards, &opts, 1, rng);
        seat.step(b.mask(), 0, modes, cards, &opts, 1, rng)
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
