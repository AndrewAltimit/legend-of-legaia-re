//! Disc-gated: the Baka Fighter and Muscle Dome door warps sound on the
//! session both play hosts run - music, cues and announcer lines.
//!
//! Driven the way a host drives a frame (the session tick, the minigame and
//! duel cue drains, the two announcer steps, the audio frame) over a
//! device-less output, from the Sol casino floor through the mode-24 warp.
//!
//! * Baka Fighter: the overlay's own track (`2055`, `FUN_801CF00C`), the
//!   cabinet's confirm blip `0x20` and the exchange hit `0x09` **keyed** on a
//!   voice (not only queued), and the chrome's announcer lines requested.
//! * Muscle Dome: the arena's battle theme (`2026`, `FUN_801CEA6C`) and the
//!   "Welcome to the Muscle Dome!" line (`XA31` channel `0x0B`) on the hub's
//!   first tick.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::audio_director::AudioCommand;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_engine_system::pad::PadButton;

type Sink = legaia_engine_audio::AudioOut;

const BAKA_SUB_ID: u8 = 4;
const MUSCLE_SUB_ID: u8 = 5;

fn session() -> Option<BootSession<Sink>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    Some(
        BootSession::open_disc_detached_audio(std::path::Path::new(&disc), &BootConfig::default())
            .expect("boot session"),
    )
}

/// What the director was asked for and what keyed, frame by frame.
#[derive(Default)]
struct Rec {
    cmds: Vec<AudioCommand>,
    keyed: Vec<u16>,
    seen: u64,
}

/// One host frame.
fn step(s: &mut BootSession<Sink>, pad: u16, rec: &mut Rec) {
    s.host.world.set_pad(pad);
    s.tick().expect("tick");
    s.enqueue_minigame_sfx_cues();
    s.enqueue_baka_sfx_cues();
    s.sound_baka_announcer();
    s.sound_muscle_hub();
    if let Some(r) = s.step_audio_frame() {
        rec.keyed.extend(r.fired.iter().map(|&(id, _)| id));
    }
    let log = s.bgm.as_ref().expect("audio enabled").commands();
    let new = (log.count() - rec.seen) as usize;
    let recent: Vec<_> = log.recent().copied().collect();
    rec.cmds
        .extend(recent.iter().skip(recent.len().saturating_sub(new)));
    rec.seen = log.count();
}

/// Warp from the casino floor into minigame `sub` and play `frames` frames,
/// pressing Cross on a slow cadence (a confirm on every menu the cabinet
/// shows) and a throw button in between.
fn warp_and_play(sub: u8, mode: SceneMode, frames: u32) -> Option<Rec> {
    let mut s = session()?;
    s.enter_scene_live("koin1", &FieldLiveOpts::default())
        .expect("enter koin1");
    let mut rec = Rec::default();
    for _ in 0..30 {
        step(&mut s, 0, &mut rec);
    }
    rec = Rec {
        seen: rec.seen,
        ..Rec::default()
    };
    s.host.world.request_minigame_warp(sub);
    for f in 0..frames {
        let pad = match f {
            f if f % 90 == 0 => PadButton::Cross as u16,
            f if f % 61 == 0 => PadButton::Square as u16,
            _ => 0,
        };
        step(&mut s, pad, &mut rec);
    }
    assert_eq!(s.host.world.mode, mode, "the warp lands in the minigame");
    assert!(s.bgm.as_ref().unwrap().is_playing(), "a track is sounding");
    Some(rec)
}

#[test]
fn the_baka_fighter_warp_plays_its_track_cues_and_announcer() {
    let Some(rec) = warp_and_play(BAKA_SUB_ID, SceneMode::BakaFighter, 1500) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let xa: Vec<_> = rec
        .cmds
        .iter()
        .filter(|c| matches!(c, AudioCommand::XaClip(..)))
        .collect();
    eprintln!("[ok] keyed {} cues; announcer {xa:?}", rec.keyed.len());
    assert!(rec.cmds.contains(&AudioCommand::BgmStartOwned(2055)));
    let hit = u16::from(legaia_engine_minigames::baka_fighter::BAKA_CUE_HIT);
    let confirm = u16::from(legaia_engine_minigames::baka_fighter::BAKA_CUE_CONFIRM);
    assert!(
        rec.keyed.contains(&confirm),
        "the confirm blip keys a voice"
    );
    assert!(rec.keyed.contains(&hit), "the exchange hit keys a voice");
    assert!(!xa.is_empty(), "the chrome's announcer lines are requested");
}

#[test]
fn the_muscle_dome_warp_plays_its_theme_and_the_welcome_line() {
    let Some(rec) = warp_and_play(MUSCLE_SUB_ID, SceneMode::MuscleDome, 600) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let welcome = legaia_engine_minigames::muscle_ringside::FIRST_VISIT_INTRO_XA;
    eprintln!("[ok] dome commands {:?}", rec.cmds);
    assert!(rec.cmds.contains(&AudioCommand::BgmStartOwned(
        legaia_engine_system::music_labels::BATTLE_THEME_1_BGM_ID
    )));
    assert!(rec.cmds.contains(&AudioCommand::XaClip(
        u32::from(welcome.clip),
        u32::from(welcome.channel),
        u32::from(welcome.duration_sectors),
    )));
}
