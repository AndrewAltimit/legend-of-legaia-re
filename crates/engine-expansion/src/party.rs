//! A party night played alone: the player against the night's AI rivals,
//! in the world's mode seat.
//!
//! The night is the netplay one ([`PartyNight`]); here every seat but the
//! player's is an AI rival, so the world directs it itself
//! (`World::enter_pack_mode` with a night's id, ticked under
//! `SceneMode::Expansion`). Between rounds the seat is empty and the screen
//! is the night's party card; a round plays on the seat like any other
//! instance, except that it settles nothing of its own - the night scores
//! it, and a completed night grants the player its prize.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::party::{PartyNight, PlayerKey};

/// One night played alone.
#[derive(Debug, Clone)]
pub struct LocalNight {
    pub night: PartyNight,
    /// The mode the world returns to when the night closes.
    pub return_mode: SceneMode,
    /// The round on the seat's mode seat -> night player.
    pub seat_keys: Vec<PlayerKey>,
    /// Frames the finished round's results have stood.
    pub finished_for: u32,
    /// The player's prize lines, once granted (empty: no prize).
    pub granted: Option<Vec<String>>,
    /// Frames since the night opened (the party card's clock).
    pub frame: u32,
}

impl LocalNight {
    pub fn new(night: PartyNight, return_mode: SceneMode) -> Self {
        Self {
            night,
            return_mode,
            seat_keys: Vec::new(),
            finished_for: 0,
            granted: None,
            frame: 0,
        }
    }
}
