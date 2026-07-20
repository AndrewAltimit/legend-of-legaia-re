//! Game-over screen.
//!
//! **This screen is an engine invention, and it is currently unreachable
//! on purpose.** Retail's game over is not a menu: the content lives in
//! PROT 0902, whose only readable string is `GAME OVER` and whose single
//! unconditional exit writes `game_mode = 0`. There is no Continue /
//! Retry / Quit vocabulary anywhere in it, and one exit store cannot
//! express three outcomes - so the rows below are ours, not the game's.
//!
//! The trigger is unpinned too. The battle action SM's `0x5A` gate does
//! detect a party wipe (`DAT_8007BD71 = 0xFE`, cause `_DAT_8007BD2C = 5`),
//! but the battle-exit mode selector never reads that cause, and no
//! static writer of the game-over mode exists anywhere on the disc. Until
//! a runtime probe settles where retail actually goes on a wipe, nothing
//! constructs [`GameOverSession`] outside tests - wiring it would commit
//! the port to behaviour nobody has observed. See
//! `docs/subsystems/battle.md` § party wipe + the game-over overlay.
//!
//! Nominally: the player is offered Continue (drop into save-select),
//! Retry (re-roll the same encounter), or Quit (return to title).
//!
//! Renderer-agnostic state machine. Engines drive [`GameOverSession::tick`]
//! each frame and react to the [`GameOverEvent`] stream.

use crate::input::PadButton;

/// Outcome rows on the game-over panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverRow {
    Continue,
    Retry,
    Quit,
}

impl GameOverRow {
    pub const ALL: [Self; 3] = [Self::Continue, Self::Retry, Self::Quit];

    pub fn label(self) -> &'static str {
        match self {
            Self::Continue => "Continue",
            Self::Retry => "Retry",
            Self::Quit => "Quit",
        }
    }

    pub fn from_index(idx: u8) -> Option<Self> {
        Self::ALL.get(idx as usize).copied()
    }

    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|r| *r == self).unwrap() as u8
    }
}

/// Phase of the SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverPhase {
    /// Opening fade-to-red. No input until this counter drains.
    FadeIn { frames_remaining: u16 },
    /// Player picks an outcome.
    Choosing { cursor: u8 },
    /// Player committed; engine inspects [`GameOverSession::outcome`].
    Done(GameOverOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverOutcome {
    Continue,
    Retry,
    Quit,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameOverInput {
    pub up: bool,
    pub down: bool,
    pub cross: bool,
}

impl GameOverInput {
    pub fn from_pad_edge(pressed: u16) -> Self {
        Self {
            up: pressed & PadButton::Up.mask() != 0,
            down: pressed & PadButton::Down.mask() != 0,
            cross: pressed & PadButton::Cross.mask() != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverEvent {
    FadeInDone,
    CursorMoved { row: u8 },
    Confirmed { row: GameOverRow },
}

#[derive(Debug, Clone)]
pub struct GameOverSession {
    pub phase: GameOverPhase,
    pub fade_in_frames: u16,
    /// Disable Continue when no save data is present (engines pass false).
    pub continue_enabled: bool,
}

impl GameOverSession {
    pub fn new() -> Self {
        Self {
            phase: GameOverPhase::FadeIn {
                frames_remaining: 60,
            },
            fade_in_frames: 60,
            continue_enabled: true,
        }
    }

    pub fn with_no_save() -> Self {
        let mut s = Self::new();
        s.continue_enabled = false;
        s
    }

    pub fn phase(&self) -> GameOverPhase {
        self.phase
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            GameOverPhase::Choosing { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, GameOverPhase::Done(_))
    }

    pub fn outcome(&self) -> Option<GameOverOutcome> {
        match self.phase {
            GameOverPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    fn first_admissible(&self) -> u8 {
        if !self.continue_enabled {
            GameOverRow::Retry.index()
        } else {
            0
        }
    }

    fn step(&self, cursor: u8, dir: i8) -> u8 {
        let n = GameOverRow::ALL.len() as i8;
        let mut i = cursor as i8;
        for _ in 0..n {
            i = (i + dir).rem_euclid(n);
            let row = GameOverRow::from_index(i as u8).unwrap();
            if row == GameOverRow::Continue && !self.continue_enabled {
                continue;
            }
            return i as u8;
        }
        cursor
    }

    pub fn tick(&mut self, input: GameOverInput) -> Vec<GameOverEvent> {
        let mut events = Vec::new();
        match self.phase {
            GameOverPhase::FadeIn { frames_remaining } => {
                if frames_remaining == 0 {
                    self.phase = GameOverPhase::Choosing {
                        cursor: self.first_admissible(),
                    };
                    events.push(GameOverEvent::FadeInDone);
                } else {
                    self.phase = GameOverPhase::FadeIn {
                        frames_remaining: frames_remaining.saturating_sub(1),
                    };
                    if frames_remaining == 1 {
                        self.phase = GameOverPhase::Choosing {
                            cursor: self.first_admissible(),
                        };
                        events.push(GameOverEvent::FadeInDone);
                    }
                }
            }
            GameOverPhase::Choosing { cursor } => {
                let mut new_cursor = cursor;
                if input.up {
                    new_cursor = self.step(cursor, -1);
                } else if input.down {
                    new_cursor = self.step(cursor, 1);
                }
                if new_cursor != cursor {
                    self.phase = GameOverPhase::Choosing { cursor: new_cursor };
                    events.push(GameOverEvent::CursorMoved { row: new_cursor });
                }
                if input.cross {
                    let row = GameOverRow::from_index(new_cursor).unwrap_or(GameOverRow::Quit);
                    let outcome = match row {
                        GameOverRow::Continue => GameOverOutcome::Continue,
                        GameOverRow::Retry => GameOverOutcome::Retry,
                        GameOverRow::Quit => GameOverOutcome::Quit,
                    };
                    self.phase = GameOverPhase::Done(outcome);
                    events.push(GameOverEvent::Confirmed { row });
                }
            }
            GameOverPhase::Done(_) => {}
        }
        events
    }
}

impl Default for GameOverSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_in_drains_to_choosing() {
        let mut s = GameOverSession::new();
        s.fade_in_frames = 2;
        s.phase = GameOverPhase::FadeIn {
            frames_remaining: 2,
        };
        let _ = s.tick(GameOverInput::default());
        let _ = s.tick(GameOverInput::default());
        assert!(matches!(s.phase, GameOverPhase::Choosing { cursor: 0 }));
    }

    #[test]
    fn choosing_cursor_moves() {
        let mut s = GameOverSession::new();
        s.phase = GameOverPhase::Choosing { cursor: 0 };
        let evs = s.tick(GameOverInput {
            down: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), 1);
        assert_eq!(evs, vec![GameOverEvent::CursorMoved { row: 1 }]);
    }

    #[test]
    fn cross_commits_outcome() {
        let mut s = GameOverSession::new();
        s.phase = GameOverPhase::Choosing { cursor: 1 };
        let _ = s.tick(GameOverInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(GameOverOutcome::Retry));
    }

    #[test]
    fn no_save_skips_continue_row() {
        let mut s = GameOverSession::with_no_save();
        s.phase = GameOverPhase::Choosing {
            cursor: GameOverRow::Retry.index(),
        };
        let _ = s.tick(GameOverInput {
            up: true,
            ..Default::default()
        });
        assert_eq!(s.cursor(), GameOverRow::Quit.index());
    }

    #[test]
    fn pad_edge_decoder() {
        let m = PadButton::Cross.mask();
        let i = GameOverInput::from_pad_edge(m);
        assert!(i.cross && !i.up && !i.down);
    }
}
