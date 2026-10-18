//! The post-FMV control transfer: `FmvHandoffOutcome` and `SceneHost::apply_pending_fmv_handoff`.

use super::*;

/// What [`SceneHost::apply_pending_fmv_handoff`] did with control once an FMV
/// finished playing.
///
/// One value per outcome so a host only has to format it: every host prints
/// the same set of cases, and the set is the retail dispatch's, not a
/// per-host choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmvHandoffOutcome {
    /// The [`FmvHandoff::Field`] arm ran: the world is now in `scene`.
    Entered {
        /// The FMV that just finished.
        fmv_id: i16,
        /// CDNAME label the dispatch wrote to `0x80084548`.
        scene: &'static str,
        /// Spawn/door word written to `0x80084540`. Reported, not applied -
        /// the engine seats a cold field entry from its own resolver and
        /// carries no door-word -> arrival-seat table.
        door: u16,
    },
    /// The [`FmvHandoff::Field`] arm's scene failed to load; the trigger
    /// scene stays live.
    Failed {
        /// The FMV that just finished.
        fmv_id: i16,
        /// The scene the dispatch asked for.
        scene: &'static str,
        /// Why the load failed, already formatted.
        error: String,
    },
    /// A non-field arm, reported rather than run. `ResumeField` needs no
    /// transfer (the trigger scene continues); `CardInit` and `ModeZero` hand
    /// off to game modes 22 and 0, neither of which exists in the engine's
    /// [`SceneMode`] set, so there is nothing to transfer control *to*.
    /// `None` is a dev slot that encodes no hand-off at all.
    NotApplied {
        /// The FMV that just finished.
        fmv_id: i16,
        /// The arm the dispatch selected.
        handoff: FmvHandoff,
    },
}

impl std::fmt::Display for FmvHandoffOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Entered {
                fmv_id,
                scene,
                door,
            } => write!(
                f,
                "fmv {fmv_id} hands off to field scene '{scene}' (door {door:#05x})"
            ),
            Self::Failed {
                fmv_id,
                scene,
                error,
            } => write!(
                f,
                "fmv {fmv_id} hand-off to field scene '{scene}' failed: {error}"
            ),
            Self::NotApplied { fmv_id, handoff } => match handoff {
                FmvHandoff::ResumeField => write!(
                    f,
                    "fmv {fmv_id} resumes the trigger scene (no scene name written)"
                ),
                FmvHandoff::CardInit { card_arg } => write!(
                    f,
                    "fmv {fmv_id} hands off to game mode 22 (card init, arg {card_arg}) - mode not in the engine"
                ),
                FmvHandoff::ModeZero => write!(
                    f,
                    "fmv {fmv_id} hands off to game mode 0 - mode not in the engine"
                ),
                FmvHandoff::None => write!(
                    f,
                    "fmv {fmv_id} encodes no hand-off (dev slot); trigger scene continues"
                ),
                FmvHandoff::Field { .. } => write!(f, "fmv {fmv_id} field hand-off"),
            },
        }
    }
}

impl SceneHost {
    /// Run retail's post-FMV control transfer for the FMV that just finished.
    ///
    /// The master dispatch `FUN_801CEA3C` ends every playback with a second
    /// `switch` that writes the mode/scene globals: the next-scene CDNAME
    /// label (`0x80084548`), the spawn/door word (`0x80084540`) and the
    /// next-game-mode byte (`0x8007B83C`). Mid-game FMVs therefore do **not**
    /// resume the trigger scene - `town01` triggers `fmv_id 1` and lands in
    /// `town0b`. The per-id map is
    /// [`legaia_engine_system::movie::cutscene::fmv_post_play_handoff`].
    ///
    /// This is the single host of that map. Every host calls it after
    /// [`World::finish_cutscene`] and formats the returned outcome; the
    /// source edge is [`World::take_finished_fmv`], so calling it from two
    /// hosts, or twice from one, cannot transfer control twice. Returns
    /// `None` when no FMV has finished since the last call.
    // PORT: FUN_801cea3c
    pub fn apply_pending_fmv_handoff(&mut self) -> Option<FmvHandoffOutcome> {
        let fmv_id = self.world.take_finished_fmv()?;
        let handoff = legaia_engine_system::movie::cutscene::fmv_post_play_handoff(fmv_id);
        Some(match handoff {
            FmvHandoff::Field { scene, door } => match self.enter_field_scene(scene, 0) {
                Ok(()) => FmvHandoffOutcome::Entered {
                    fmv_id,
                    scene,
                    door,
                },
                Err(e) => FmvHandoffOutcome::Failed {
                    fmv_id,
                    scene,
                    error: format!("{e:#}"),
                },
            },
            FmvHandoff::ResumeField => {
                // The resume arm (`0x801CEF18`) leaves the field-entry word
                // at 2, so the field comes back through MAIN INIT's warp arm,
                // which takes twenty seconds off a running escape clock
                // (`0x8003B62C..0x8003B648`).
                self.world.battle.escape_timer.field_return_penalty();
                FmvHandoffOutcome::NotApplied { fmv_id, handoff }
            }
            other => FmvHandoffOutcome::NotApplied {
                fmv_id,
                handoff: other,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::escape_timer::{EscapeTimer, FIELD_RETURN_PENALTY};

    /// The resume arm comes back through MAIN INIT's warp arm, which takes
    /// twenty seconds off a running escape clock (`0x8003B62C..0x8003B648`).
    /// Disc-gated: `SceneHost` needs the disc's index.
    #[test]
    fn the_fmv_resume_arm_costs_a_running_escape_clock_twenty_seconds() {
        let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(std::path::PathBuf::from) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let mut host = SceneHost::open_disc(&path).expect("open disc");
        assert_eq!(
            legaia_engine_system::movie::cutscene::fmv_post_play_handoff(5),
            FmvHandoff::ResumeField
        );
        host.world.battle.escape_timer = EscapeTimer {
            remaining: 21600,
            warn_threshold: 3600,
            armed: true,
        };
        host.world.cutscene.finished_fmv = Some(5);
        let out = host.apply_pending_fmv_handoff();
        assert!(matches!(
            out,
            Some(FmvHandoffOutcome::NotApplied {
                handoff: FmvHandoff::ResumeField,
                ..
            })
        ));
        assert_eq!(
            host.world.battle.escape_timer.remaining,
            21600 - FIELD_RETURN_PENALTY
        );
    }
}
