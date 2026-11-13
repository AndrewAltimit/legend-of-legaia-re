//! A direct scene switch (the scene picker, a card load, a New Game) on the
//! director: the outgoing scene's sound fades to silence before the
//! incoming scene's bank is uploaded.
//!
//! The engine's scene load is instant. Without this the old score's notes
//! and the old scene's cue voices were keyed off at full level, and the
//! entry script's track start uploaded the new bank over the BGM region on
//! the next frame - under those voices, whose release tails then read the
//! new bank's bytes and sounded as a stray horn or chime for as long as
//! each release ran. Retail never reaches that state: the field init ramps
//! the field score down while the next map reads (`FUN_801D6704`,
//! `0x801D6AC0`, the ramp `FUN_80062004(.., 0x78)`) and keys the cue voices
//! off (`0x801D6B88`), and the new bank arrives a CD read later.
//!
//! So a direct entry starts a short fade of everything sounding
//! ([`legaia_engine_audio::AudioSink::fade_to_silence`]) and the director
//! holds the BGM commands the new scene issues until the fade has cut what
//! was sounding, then replays them in order. A door does not come through
//! here: its field init keeps the retail order the parity captures pin.

use super::*;

/// The switch fade: 30 ms at the SPU rate. Long enough that the cut is not
/// a click, short enough that the next scene's track is not noticeably
/// late.
pub const SCENE_SWITCH_FADE_SAMPLES: u32 = 1_323;

/// The most audio frames a held command waits for the fade. A host whose
/// output is not pulling samples (no device) ends the fade here instead of
/// holding the next scene's music for ever.
pub const SCENE_SWITCH_HOLD_MAX_FRAMES: u16 = 6;

/// A BGM command issued while a switch fade runs, replayed once it ends.
#[derive(Debug)]
pub(super) enum HeldCommand {
    Start(u16, Vec<u8>),
    StartOwned(u16, Vec<u8>),
    StartOwnedFaded(u16, Vec<u8>, u16),
    Stop,
    Pause,
    Resume,
    UnhaltPause,
}

/// The director's switch-fade state.
#[derive(Debug, Default)]
pub(super) struct SceneSwitchHold {
    /// A switch fade is running and commands are being held.
    active: bool,
    /// Audio frames left before the hold ends regardless.
    frames_left: u16,
    /// The held commands, in issue order.
    queued: Vec<HeldCommand>,
}

impl<S: AudioSink> AudioDirector<S> {
    /// Start a direct scene entry's audio: drop the cues queued against the
    /// old scene, clear the dedupe latch (so the entry's own start is
    /// honoured even for the track already playing, re-staging its bank),
    /// and fade out what is sounding ([`SCENE_SWITCH_FADE_SAMPLES`]). The
    /// BGM commands the new scene issues before the fade ends are held and
    /// replayed after it. Nothing sounding: no fade and no hold.
    pub fn begin_scene_switch(&mut self) {
        self.clear_sfx();
        self.last_started = None;
        self.release_pack_music();
        self.switch.queued.clear();
        self.audio.fade_to_silence(SCENE_SWITCH_FADE_SAMPLES);
        self.switch.active = self.audio.silence_pending();
        self.switch.frames_left = SCENE_SWITCH_HOLD_MAX_FRAMES;
    }

    /// Whether a switch fade is holding the BGM commands.
    pub fn scene_switch_holding(&self) -> bool {
        self.switch.active
    }

    /// Hold `cmd` if a switch fade is running; `false` (and `cmd` unused)
    /// otherwise.
    pub(super) fn hold_for_switch(&mut self, cmd: impl FnOnce() -> HeldCommand) -> bool {
        if !self.switch.active {
            return false;
        }
        self.switch.queued.push(cmd());
        true
    }

    /// One audio frame of the hold: once the fade has cut what was sounding
    /// (or the frame cap runs out, which cuts it now), replay the held
    /// commands.
    pub(super) fn tick_scene_switch(&mut self) {
        if !self.switch.active {
            return;
        }
        self.switch.frames_left = self.switch.frames_left.saturating_sub(1);
        if self.audio.silence_pending() && self.switch.frames_left > 0 {
            return;
        }
        self.audio.finish_silence();
        self.switch.active = false;
        for cmd in std::mem::take(&mut self.switch.queued) {
            match cmd {
                HeldCommand::Start(id, seq) => BgmDirector::start(self, id, &seq),
                HeldCommand::StartOwned(id, entry) => {
                    BgmDirector::start_owned_vab(self, id, &entry)
                }
                HeldCommand::StartOwnedFaded(id, entry, frames) => {
                    BgmDirector::start_owned_vab_faded(self, id, &entry, frames)
                }
                HeldCommand::Stop => BgmDirector::stop(self),
                HeldCommand::Pause => BgmDirector::pause(self),
                HeldCommand::Resume => BgmDirector::resume(self),
                HeldCommand::UnhaltPause => BgmDirector::unhalt_pause(self),
            }
        }
    }
}
