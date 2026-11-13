//! Disc-gated: the post-Caruban beat's shatter cue plays out and stops.
//!
//! With story flag `0x289` up (set by `rikuroa` P1[3] before the Caruban
//! fight), the scene-entry script P1[0] takes its `0x07E6` arm: side-band
//! request `2012` (`36 01 80 DC 07`), the scene's track `2025`
//! (`35 E9 07 01`), then the beat P2[50] (`44 5C`). The beat shatters
//! Caruban and rings cue `0x20D` (`36 00 80 0D 02`): a three-voice one-shot
//! of the side-band bank's program 0, whose sample ends on an end flag with
//! no repeat. Its voices land on 23 and 22.
//!
//! The port parks the side-band bank behind the BGM. The track loads a few
//! frames after the cue and its samples overrun the bank, which is re-staged
//! higher up; a voice left on the old span then reads the track's
//! instruments - a looped one held for good (player issue 591: a horn
//! looping over the rest of the scene). The director moves those voices onto
//! the re-staged copy, so the cue plays out to its own end flag.
//!
//! The SPU is clocked one video frame per tick (735 samples) so the voices
//! advance as they would under an output.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset.

use legaia_engine_audio::{AudioSink, Phase};
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_engine_system::pad::PadButton;

type Sink = legaia_engine_audio::AudioOut;

/// The post-fight flag P1[3] raises before the battle (`52 89`).
const CARUBAN_FOUGHT: u16 = 0x289;
/// The shatter cue P2[50] rings.
const SHATTER_CUE: u16 = 0x20D;

#[test]
fn the_shatter_cue_plays_out_after_the_track_loads_over_its_bank() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut s = BootSession::<Sink>::open_disc_detached_audio(
        std::path::Path::new(&disc),
        &BootConfig::default(),
    )
    .expect("boot session");
    s.enter_scene_live("rikuroa", &FieldLiveOpts::default())
        .expect("enter rikuroa");
    for _ in 0..30 {
        s.host.world.set_pad(0);
        s.tick().expect("tick");
        s.step_audio_frame();
    }
    // A cold entry clears the story; raise the flag and replay the entry
    // script, as the field init after the fight runs it.
    s.host.world.system_flag_set(CARUBAN_FOUGHT);
    assert!(s.play_scene_record(1, 0), "replay rikuroa P1[0]");
    let audio = s.audio.clone().expect("audio enabled");
    let mut cue_at = None;
    let mut track_loaded_after_cue = false;
    let mut last_track = None;
    let mut silent_at = None;
    // Frames a sounding cue voice read from outside the parked side-band
    // bank - the track's samples.
    let mut stray_frames = 0u32;
    for f in 0..3000u32 {
        // The beat's dialogue waits for Cross: press 2, release 14.
        let pad = if f % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        let report = s.step_audio_frame().unwrap_or_default();
        audio.with_spu(|spu| spu.render_into(&mut [0i16; 735 * 2]));
        if cue_at.is_none() && report.fired.iter().any(|&(id, _)| id == SHATTER_CUE) {
            cue_at = Some(f);
        }
        let track = s.bgm.as_ref().and_then(|b| b.last_started);
        if cue_at.is_some() && track != last_track && track.is_some() {
            track_loaded_after_cue = true;
        }
        last_track = track;
        let sounding =
            audio.with_spu(|spu| [22, 23].map(|v| spu.voices[v].adsr.phase != Phase::Off));
        let parked = s
            .bgm
            .as_ref()
            .and_then(|b| b.bgm_tail().side_band())
            .map(|(_, b)| b.base..b.end);
        if cue_at.is_some() {
            let stray = audio.with_spu(|spu| {
                [22, 23].iter().any(|&v| {
                    let voice = &spu.voices[v];
                    !voice.is_off() && parked.clone().is_none_or(|p| !voice.reads_from(p))
                })
            });
            stray_frames += u32::from(stray);
        }
        if cue_at.is_some() && silent_at.is_none() && !sounding.contains(&true) {
            silent_at = Some(f);
        }
        if silent_at.is_some() {
            break;
        }
    }
    eprintln!(
        "[ran] cue {SHATTER_CUE:#x} at {cue_at:?}, track after it {track_loaded_after_cue}, \
         voices 22..=23 silent at {silent_at:?}, {stray_frames} frames off the bank"
    );
    let cue_at = cue_at.expect("the beat rings the shatter cue");
    assert!(
        track_loaded_after_cue,
        "the scene's track loads after the cue - the case this test pins"
    );
    assert_eq!(
        stray_frames, 0,
        "a cue voice read the track's samples instead of its own"
    );
    // The lowest of the cue's tones (centre 92 under note 60) plays its
    // sample out in about eleven seconds; a voice reading a looped track
    // instrument holds until something else keys it off.
    let silent_at = silent_at.expect("the cue's voices fall silent");
    assert!(
        silent_at - cue_at < 900,
        "the cue held {} frames",
        silent_at - cue_at
    );
}
