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
//! * Muscle Dome: the "Welcome to the Muscle Dome!" line (`XA31` channel
//!   `0x0B`) on the hub's first tick, and the arena's battle theme (`2026`,
//!   loaded by `FUN_801CEA6C`) started only when the hub hands the leg over
//!   (`FUN_801D1510`'s `jal 0x80026478`), never under the announcer cards.
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
    // A dome leg the hub has handed over is the ordinary battle.
    let landed = s.host.world.mode == mode
        || (mode == SceneMode::MuscleDome && s.host.world.muscle_leg_in_battle());
    assert!(
        landed,
        "the warp lands in the minigame ({:?})",
        s.host.world.mode
    );
    // The dome's arena is silent until its fight starts
    // (`the_muscle_dome_theme_waits_for_the_fight_start`).
    if mode != SceneMode::MuscleDome || s.host.world.muscle_leg_in_battle() {
        assert!(s.bgm.as_ref().unwrap().is_playing(), "a track is sounding");
    }
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
fn the_muscle_dome_warp_plays_the_welcome_line() {
    let Some(rec) = warp_and_play(MUSCLE_SUB_ID, SceneMode::MuscleDome, 600) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let welcome = legaia_engine_minigames::muscle_ringside::FIRST_VISIT_INTRO_XA;
    eprintln!("[ok] dome commands {:?}", rec.cmds);
    assert!(rec.cmds.contains(&AudioCommand::XaClip(
        u32::from(welcome.clip),
        u32::from(welcome.channel),
        u32::from(welcome.duration_sectors),
    )));
}

#[test]
fn the_muscle_dome_theme_waits_for_the_fight_start() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
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
    s.host.world.request_minigame_warp(MUSCLE_SUB_ID);
    let theme =
        AudioCommand::BgmStartOwned(legaia_engine_system::music_labels::BATTLE_THEME_1_BGM_ID);
    let mut handed_over = None;
    let mut theme_at = None;
    for f in 0..2400u32 {
        step(&mut s, 0, &mut rec);
        let w = &s.host.world;
        let covered = w.minigames.muscle_hub.covers_leg();
        if theme_at.is_none() && rec.cmds.contains(&theme) {
            theme_at = Some(f);
            assert!(
                !covered && w.minigames.muscle_dome.is_some(),
                "frame {f}: the theme starts under a hub screen"
            );
        }
        if handed_over.is_none()
            && w.minigames.muscle_dome.is_some()
            && !w.minigames.muscle_leg_fresh
            && !covered
        {
            handed_over = Some(f);
        }
        if theme_at.is_some() {
            break;
        }
    }
    eprintln!("[ok] leg handed over at {handed_over:?}, theme at {theme_at:?}");
    let welcome = legaia_engine_minigames::muscle_ringside::FIRST_VISIT_INTRO_XA;
    assert!(rec.cmds.contains(&AudioCommand::XaClip(
        u32::from(welcome.clip),
        u32::from(welcome.channel),
        u32::from(welcome.duration_sectors),
    )));
    assert!(handed_over.is_some(), "the first visit hands a leg over");
    // The world's dome tick reads the hub as the previous tick left it, so
    // the theme lands on the hand-off tick or the one after it.
    let (h, t) = (handed_over.unwrap(), theme_at.expect("the theme starts"));
    assert!((h..=h + 1).contains(&t), "theme at {t}, hand-off at {h}");
    assert!(s.bgm.as_ref().unwrap().is_playing());
}
