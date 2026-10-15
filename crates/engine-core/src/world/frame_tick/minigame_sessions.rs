use super::*;

mod baka_fighter;
mod dance;
mod fishing;
mod muscle_dome;
mod round_trip;
mod slot_machine;

impl World {
    /// The live minigame's status caption
    /// ([`legaia_engine_minigame_scenes::minigame_status::caption`]), or `None`
    /// outside a minigame. The one call both play hosts make: the world
    /// answers the mode and the two Muscle Dome readings that sit on it (the
    /// contest's flag gates, the turn play-out tally); the host answers only
    /// `host`.
    pub fn minigame_caption(
        &self,
        host: &legaia_engine_minigame_scenes::minigame_status::StatusHost<'_>,
    ) -> Option<legaia_engine_minigame_scenes::minigame_status::MinigameCaption> {
        use legaia_engine_minigame_scenes::minigame_status::{MuscleWorld, caption};
        let muscle = MuscleWorld {
            contest_flags: self.muscle_contest_flags(),
            playback_tally: self.muscle_playback_tally(),
        };
        caption(self.mode, &self.minigames, &muscle, host)
    }
}
