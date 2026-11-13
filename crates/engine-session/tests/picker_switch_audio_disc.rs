//! A scene-picker switch leaves nothing of the old scene sounding.
//!
//! The picker enters a scene directly (`BootSession::enter_scene_live`), and
//! the engine's load is instant: the outgoing track's notes and the old
//! scene's cue voices used to be keyed off with their envelopes still at
//! level, and the incoming track's bank was uploaded over the BGM region
//! under them on the same frame. Their release tails then read the new
//! bank's bytes - a horn or a chime from the wrong instrument - for as long
//! as each release ran. Retail never reaches that state: the field init
//! ramps the outgoing score down while the map reads (`FUN_801D6704`,
//! `0x801D6AC0`) and keys the cue voices off, and the new bank arrives a
//! read later.
//!
//! Driven over the device-free `TestAudioSink`. `LEGAIA_PICKER_WAV=<path>`
//! writes the rendered switch window as a 16-bit stereo WAV.
//!
//! Disc-gated: skips and passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
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

/// Voices sounding now that were keyed before the switch: `(voice, key-on
/// count)` pairs still unchanged and not off.
fn held_over(s: &Session, before: &[(usize, u32)]) -> Vec<usize> {
    let sink = s.bgm.as_ref().expect("audio").audio().clone();
    legaia_engine_audio::AudioSink::with_spu(&*sink, |spu| {
        before
            .iter()
            .filter(|&&(v, n)| {
                let voice = &spu.voices[v];
                !voice.is_off() && voice.key_on_count() == n
            })
            .map(|&(v, _)| v)
            .collect()
    })
}

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

/// RMS of both channels over `pcm`.
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

/// The sample spans of the director's current bank.
fn bank_spans(s: &Session) -> Vec<(u32, u32)> {
    s.bgm
        .as_ref()
        .and_then(|b| b.bank())
        .map(|bank| {
            bank.samples
                .iter()
                .flatten()
                .map(|v| (v.addr, v.addr + v.size))
                .collect()
        })
        .unwrap_or_default()
}

/// Of `voices`, how many stand on a sample of a bank other than `old`'s -
/// a voice of the old scene reading bytes the switch uploaded.
fn reading_new_bank(s: &Session, voices: &[usize], old: &[(u32, u32)]) -> usize {
    let now = bank_spans(s);
    if now == old {
        return 0;
    }
    let sink = s.bgm.as_ref().expect("audio").audio().clone();
    legaia_engine_audio::AudioSink::with_spu(&*sink, |spu| {
        voices
            .iter()
            .filter(|&&v| {
                now.iter()
                    .any(|&(a, b)| spu.voices_reading(a..b) & (1 << v) != 0)
            })
            .count()
    })
}

/// What one switch measured.
struct Switch {
    /// The director's track before and after.
    before_track: Option<u16>,
    after_track: Option<u16>,
    /// Voices sounding when the switch was made.
    at_switch: usize,
    /// Held-over voices (keyed before the switch, still sounding) per frame.
    held: Vec<usize>,
    /// Of those, the ones reading the incoming bank, per frame.
    on_new_bank: Vec<usize>,
    /// Output RMS per 10 ms window, from 50 ms before the switch.
    levels: Vec<f64>,
}

const WINDOW: usize = SPU_INTERNAL_RATE as usize / 100;

/// How the switch is made.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Via {
    /// The scene picker: a direct entry.
    Picker,
    /// A door: the field VM's named scene change, taken by the next tick.
    Door,
    /// A door behind op-`0x35` sub-op `6` ("music not change").
    KeptDoor,
}

/// Switch `from` -> `to` after `lead` frames of `from`'s music.
fn switch(from: &str, to: &str, lead: usize, via: Via) -> Option<Switch> {
    let mut s = session()?;
    let opts = FieldLiveOpts::default();
    s.enter_scene_live(from, &opts).expect("enter from");
    let mut pcm = Vec::new();
    for _ in 0..lead {
        frame(&mut s, &mut pcm);
    }
    let mut snap = (
        s.bgm.as_ref().and_then(|b| b.last_started),
        sounding(&s),
        bank_spans(&s),
        pcm.len(),
    );
    let mut ticked = Vec::new();
    match via {
        Via::Picker => {
            s.enter_scene_live(to, &opts).expect("enter to");
        }
        Via::Door | Via::KeptDoor => {
            if via == Via::KeptDoor {
                s.host.world.raise_keep_audio_across_field_init();
            }
            // The departure is held behind the streaming actor for a few
            // ticks (`FUN_8001FD44`), the old scene sounding on: the switch
            // is the tick the scene changes on, measured from just before it.
            s.host.world.pending_named_scene_transition = Some((to.to_string(), 10, 10, 0));
            for _ in 0..300 {
                let before = (
                    s.bgm.as_ref().and_then(|b| b.last_started),
                    sounding(&s),
                    bank_spans(&s),
                    pcm.len(),
                );
                frame(&mut s, &mut pcm);
                if s.host.scene.as_ref().is_some_and(|x| x.name == to) {
                    snap = before;
                    ticked.push(());
                    break;
                }
            }
        }
    }
    let (before_track, before, old_bank, at) = snap;
    let (mut held, mut on_new_bank) = (Vec::new(), Vec::new());
    for _ in &ticked {
        let h = held_over(&s, &before);
        on_new_bank.push(reading_new_bank(&s, &h, &old_bank));
        held.push(h.len());
    }
    for _ in ticked.len()..60 {
        frame(&mut s, &mut pcm);
        let h = held_over(&s, &before);
        on_new_bank.push(reading_new_bank(&s, &h, &old_bank));
        held.push(h.len());
    }
    let after_track = s.bgm.as_ref().and_then(|b| b.last_started);
    assert_eq!(
        s.host.scene.as_ref().map(|x| x.name.as_str()),
        Some(to),
        "the switch landed"
    );
    let levels: Vec<f64> = (0..105)
        .map(|i| {
            let a = (at + i * WINDOW).saturating_sub(5 * WINDOW);
            let b = (a + WINDOW).min(pcm.len());
            rms(&pcm[a.min(b)..b])
        })
        .collect();
    eprintln!(
        "[ran] {from} ({before_track:?}) -> {to} ({after_track:?}): {} voices sounding at the switch",
        before.len()
    );
    eprintln!("[ran] held-over voices per frame: {held:?}");
    eprintln!("[ran] of those on the new bank: {on_new_bank:?}");
    eprintln!(
        "[ran] rms per 10 ms (switch at window 5): {:?}",
        levels.iter().map(|l| l.round() as i64).collect::<Vec<_>>()
    );
    if let Some(path) = std::env::var_os("LEGAIA_PICKER_WAV") {
        let kind = match via {
            Via::Picker => "picker",
            Via::Door => "door",
            Via::KeptDoor => "kept_door",
        };
        let path = std::path::PathBuf::from(path).join(format!("{kind}_{from}_to_{to}.wav"));
        write_wav(&path, &pcm[at.saturating_sub(SPU_INTERNAL_RATE as usize)..]);
        eprintln!("[ran] wrote {}", path.display());
    }
    Some(Switch {
        before_track,
        after_track,
        at_switch: before.len(),
        held,
        on_new_bank,
        levels,
    })
}

/// What every switch must hold: nothing of the old scene reads the new
/// bank, nothing of it sounds past the 30 ms fade, and the output falls to
/// silence inside the fade.
fn assert_clean(m: &Switch, label: &str) {
    assert!(m.at_switch > 0, "{label}: the old scene was sounding");
    assert!(
        m.on_new_bank.iter().all(|&n| n == 0),
        "{label}: no old voice reads the new bank: {:?}",
        m.on_new_bank
    );
    // By the third frame (50 ms) nothing of the old scene sounds.
    assert!(
        m.held[2..].iter().all(|&n| n == 0),
        "{label}: old voices still sounding: {:?}",
        m.held
    );
    // The fade never rises above the level it starts at, and the 10 ms
    // window that ends 40 ms after the switch carries nothing of it.
    let pre = m.levels[..5].iter().cloned().fold(0.0, f64::max);
    assert!(
        m.levels[5..8].iter().all(|&l| l <= pre),
        "{label}: the fade only falls: {:?}",
        &m.levels[..8]
    );
    assert!(
        m.levels[8] < pre * 0.05,
        "{label}: silent 30 ms after the switch: {:.0} against {pre:.0}",
        m.levels[8]
    );
}

/// `town01`, `retock` and `rikuroa` start different tracks, each in its own
/// bank, so every switch uploads a new bank over the BGM region; `map01` is
/// the overworld entry. Before the switch fade, `retock -> town01` left all
/// fifteen of `retock`'s voices releasing over `town01`'s bank for up to 22
/// frames, and `town01 -> rikuroa` three voices at full release level for 22
/// frames, louder than the score they came from.
#[test]
fn a_picker_switch_leaves_no_old_voice_sounding() {
    for (from, to) in [
        ("town01", "retock"),
        ("retock", "town01"),
        ("town01", "rikuroa"),
        ("town01", "map01"),
    ] {
        let Some(m) = switch(from, to, 240, Via::Picker) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return;
        };
        assert_clean(&m, &format!("picker {from} -> {to}"));
    }
}

/// A door with sound-flag bit 2 clear runs the field init's close in
/// retail's order: the cue voices `0x10..0x17` take volume `0` before their
/// key-off (`0x801D6BB0..0x801D6BDC`), the score ramps to silence
/// (`0x801D6AC0`) and is closed (`0x801D7164`), and the next scene's banks
/// arrive after. Before, the close was a bare key-off at full level and the
/// next scene's bank landed on the next frame under the release tails.
#[test]
fn a_door_closes_the_old_scene_before_the_new_bank_loads() {
    for (from, to) in [
        ("retock", "town01"),
        ("town01", "rikuroa"),
        ("town01", "retock"),
        ("retock", "map01"),
    ] {
        let Some(m) = switch(from, to, 240, Via::Door) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return;
        };
        assert_clean(&m, &format!("door {from} -> {to}"));
    }
}

/// A door behind sub-op `6` has no close: the init leaves the field-BGM
/// source sounding (`0x801D6A90`, `0x801D6B94`), so the score the door
/// brought plays on into the room with its voices untouched.
#[test]
fn a_kept_door_plays_the_old_score_on() {
    let Some(m) = switch("retock", "town01", 240, Via::KeptDoor) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(m.at_switch > 0);
    assert!(
        m.held[..10].iter().all(|&n| n > 0),
        "the old score sounds on: {:?}",
        m.held
    );
    assert_eq!(m.before_track, m.after_track, "the track the door brought");
}
