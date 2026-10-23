//! The live minigame's status caption over a running `World`: the one call
//! both play hosts make for the line above a minigame
//! ([`legaia_engine_minigame_scenes::minigame_status::caption`]). The world
//! answers the mode and the two Muscle Dome readings that sit on it; the host
//! answers only its [`StatusHost`]. Only the hosts read it, so it sits above
//! `engine-core` with the other host reads.

use legaia_engine_core::world::World;
use legaia_engine_minigame_scenes::minigame_status::{MinigameCaption, StatusHost};

/// The minigame status caption, as a method on [`World`]. Bring the trait
/// into scope to call it.
pub trait WorldMinigameCaption {
    /// The live minigame's status caption, or `None` outside a minigame.
    fn minigame_caption(&self, host: &StatusHost<'_>) -> Option<MinigameCaption>;
}

impl WorldMinigameCaption for World {
    /// The live minigame's status caption
    /// ([`legaia_engine_minigame_scenes::minigame_status::caption`]), or `None`
    /// outside a minigame. The one call both play hosts make: the world
    /// answers the mode and the two Muscle Dome readings that sit on it (the
    /// contest's flag gates, the turn play-out tally); the host answers only
    /// `host`.
    fn minigame_caption(&self, host: &StatusHost<'_>) -> Option<MinigameCaption> {
        use legaia_engine_minigame_scenes::minigame_status::{MuscleWorld, caption};
        let muscle = MuscleWorld {
            contest_flags: self.muscle_contest_flags(),
            playback_tally: self.muscle_playback_tally(),
        };
        caption(self.mode, &self.minigames, &muscle, host)
    }
}
