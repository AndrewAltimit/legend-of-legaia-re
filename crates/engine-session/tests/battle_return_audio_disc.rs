//! The battle's score is silent before the field's track starts again.
//!
//! Retail's return from a fight (`docs/subsystems/audio.md`, "The battle
//! music timeline"): the battle sequence plays on past the exit fade, the
//! field init ramps it down over `0xB4` ticks while the map reads
//! (`FUN_80062004` at `0x801D6B7C`, `129` vsyncs measured), the poller's
//! stage 4 closes the battle source (`0x80024750..0x80024760`), and only then
//! does the field's track start from its head. Nothing of the battle score sounds under the
//! field track's first notes. The engine's load is instant, so the return
//! takes the same compressed close a door takes
//! (`BgmDirector::close_field_source`): a short fade, a cut, then the start.
//!
//! The re-run of the scene-entry script a return makes is the MAN install's
//! slice, inside the init's BGM window: its starts load with the field
//! (`0x801D72D4..0x801D7320`), so the field's track sounds from the field's
//! first frames rather than a swap walk's settle and read later.
//!
//! Driven over the device-free `TestAudioSink`. `LEGAIA_BATTLE_RETURN_WAV=<dir>`
//! writes the rendered return window as a 16-bit stereo WAV.
//!
//! Disc-gated: skips and passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};

type Session = BootSession<TestAudioSink>;

fn session() -> Option<Session> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host =
        legaia_engine_scene_host::scene_host::SceneHost::open_disc(std::path::Path::new(&disc))
            .expect("open disc");
    Some(
        BootSession::from_host(host, None, &BootConfig::default(), || {
            Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
        })
        .expect("boot session"),
    )
}

/// One game frame: the session tick, the audio frame, then one 60 Hz video
/// frame of output pulled sample by sample into `pcm`.
fn frame(s: &mut Session, pcm: &mut Vec<(i16, i16)>) {
    s.tick().expect("tick");
    s.step_audio_frame();
    let sink = s.bgm.as_ref().expect("audio").audio().clone();
    for _ in 0..sink.frames_per_video_frame() {
        pcm.push(sink.next_frame());
    }
}

/// `(voice, key-on count)` of every voice sounding now.
fn sounding(s: &Session) -> Vec<(usize, u32)> {
    let sink = s.bgm.as_ref().expect("audio").audio().clone();
    legaia_engine_audio::AudioSink::with_spu(&*sink, |spu| {
        spu.voices
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_off())
            .map(|(i, v)| (i, v.key_on_count()))
            .collect()
    })
}

/// Of `before`, the voices still sounding on the same key-on.
fn held_over(s: &Session, before: &[(usize, u32)]) -> usize {
    let now = sounding(s);
    before.iter().filter(|b| now.contains(b)).count()
}

fn rms(pcm: &[(i16, i16)]) -> f64 {
    if pcm.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm
        .iter()
        .map(|&(l, r)| (l as f64).powi(2) + (r as f64).powi(2))
        .sum();
    (sum / (pcm.len() as f64 * 2.0)).sqrt()
}

fn write_wav(path: &std::path::Path, pcm: &[(i16, i16)]) {
    let mut out = Vec::with_capacity(44 + pcm.len() * 4);
    let data = (pcm.len() * 4) as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&SPU_INTERNAL_RATE.to_le_bytes());
    out.extend_from_slice(&(SPU_INTERNAL_RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for &(l, r) in pcm {
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    std::fs::write(path, out).expect("write wav");
}

const WINDOW: usize = SPU_INTERNAL_RATE as usize / 100;

/// A forced `teien` fight, its battle theme playing, ended by the results
/// sequence, back to the field.
#[test]
fn the_battle_score_is_silent_before_the_field_track_starts() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    s.enter_scene_live("teien", &opts).expect("enter teien");
    let mut pcm = Vec::new();
    for _ in 0..120 {
        frame(&mut s, &mut pcm);
    }
    let field_track = s.bgm.as_ref().and_then(|b| b.last_started);
    let first_sound = pcm.iter().position(|&(l, r)| l != 0 || r != 0);
    eprintln!(
        "[ran] cold entry: first sample at {:?} ms",
        first_sound.map(|i| i * 1000 / SPU_INTERNAL_RATE as usize)
    );
    let row = s
        .host
        .world
        .registered_formation_ids()
        .into_iter()
        .next()
        .expect("teien has formation rows");
    assert!(s.host.world.force_encounter(row));
    let mut entered = false;
    for _ in 0..600 {
        frame(&mut s, &mut pcm);
        if s.host.world.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    assert!(entered, "the fight starts");
    // The battle theme plays a while.
    for _ in 0..240 {
        frame(&mut s, &mut pcm);
    }
    let battle_track = s.bgm.as_ref().and_then(|b| b.last_started);
    assert_ne!(battle_track, field_track, "the fight has its own theme");
    s.host.world.battle.end =
        Some(legaia_engine_battle_vm::battle_action::BattleEndCause::MonsterWipe);
    s.host.world.begin_battle_end_sequence();
    let mut snap = (Vec::new(), 0usize);
    let mut returned = false;
    for n in 0..2000 {
        let before = (sounding(&s), pcm.len());
        frame(&mut s, &mut pcm);
        if s.host.world.mode == SceneMode::Field {
            eprintln!("[ran] back on the field {n} frames after the end");
            snap = before;
            returned = true;
            break;
        }
    }
    assert!(returned, "back on the field");
    let (before, at) = snap;
    let mut held = vec![held_over(&s, &before)];
    let mut field_start = (s.bgm.as_ref().and_then(|b| b.last_started) == field_track).then_some(0);
    for i in 1..=150 {
        frame(&mut s, &mut pcm);
        if i <= 60 {
            held.push(held_over(&s, &before));
        }
        if field_start.is_none() && s.bgm.as_ref().and_then(|b| b.last_started) == field_track {
            field_start = Some(i);
        }
    }
    eprintln!("[ran] field track started {field_start:?} frames after the return");
    let after_track = s.bgm.as_ref().and_then(|b| b.last_started);
    let levels: Vec<f64> = (0..105)
        .map(|i| {
            let a = (at + i * WINDOW).saturating_sub(5 * WINDOW);
            let b = (a + WINDOW).min(pcm.len());
            rms(&pcm[a.min(b)..b])
        })
        .collect();
    eprintln!(
        "[ran] field {field_track:?}, battle {battle_track:?}, after {after_track:?}: {} voices sounding at the return",
        before.len()
    );
    eprintln!("[ran] held-over battle voices per frame: {held:?}");
    eprintln!(
        "[ran] rms per 10 ms (return at window 5): {:?}",
        levels.iter().map(|l| l.round() as i64).collect::<Vec<_>>()
    );
    if let Some(dir) = std::env::var_os("LEGAIA_BATTLE_RETURN_WAV") {
        let path = std::path::PathBuf::from(dir).join("battle_return_teien.wav");
        write_wav(
            &path,
            &pcm[at.saturating_sub(2 * SPU_INTERNAL_RATE as usize)..],
        );
        eprintln!("[ran] wrote {}", path.display());
    }
    assert_eq!(after_track, field_track, "the field's track is back");
    // The re-run entry script's starts load with the field (the init's
    // poller loop), not a settle and a read later: before, the field played
    // silent for 66 frames after the fight.
    assert!(
        field_start.is_some_and(|f| f <= 4),
        "the field's track starts with the field: {field_start:?}"
    );
    assert!(!before.is_empty(), "the battle score was sounding");
    assert!(
        held[2..].iter().all(|&n| n == 0),
        "no battle voice sounds past the fade: {held:?}"
    );
    let pre = levels[..5].iter().cloned().fold(0.0, f64::max);
    assert!(
        levels[8] < pre * 0.05,
        "silent 30 ms after the return: {:.0} against {pre:.0}",
        levels[8]
    );
}
