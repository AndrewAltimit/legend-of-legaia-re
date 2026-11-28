//! A title NEW GAME fades the title theme under the launch's black.
//!
//! Retail's card manager runs `LaunchFade` (sub-mode `0x16` of
//! `FUN_801DD35C`) between the NEW GAME confirm and master mode `2`, and with
//! the title's entry word up it stores the live audio level every frame: a
//! PCSX-Redux write watch from `title_menu_idle` (Up, Cross;
//! `autorun_continue_level_watch.lua`) reads `254` through the `0x5A` hold -
//! the theme at sequence volume `127` - then sixteen steps down to `0` over
//! the ramp, and `LaunchGame` stops the title's source on the launch frame.
//! The port entered the opening scene and stopped the theme at the confirm.
//!
//! Driven over the device-free `TestAudioSink`. Disc-gated: skips and passes
//! when `LEGAIA_DISC_BIN` is unset.

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

#[test]
fn the_new_game_black_swells_then_fades_the_title_theme() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(s.start_title_bgm(), "the title theme starts");
    let title = legaia_engine_system::music_labels::TITLE_THEME_BGM_ID;
    for _ in 0..30 {
        s.step_audio_frame();
    }
    assert!(s.start_new_game(&FieldLiveOpts::default()).is_some());
    assert!(
        s.host.world.launch_fade.is_some(),
        "the launch's black runs"
    );
    let mut applied = Vec::new();
    let mut frames = 0;
    loop {
        s.tick().expect("tick");
        s.step_audio_frame();
        let d = s.bgm.as_ref().expect("audio");
        if s.host.world.launch_fade.is_none() {
            // The launch frame: the hold ends and the opening's commands run.
            break;
        }
        assert_eq!(d.last_started, Some(title), "the title theme plays on");
        assert!(d.is_playing(), "nothing stops it under the black");
        applied.push(d.duck_applied());
        frames += 1;
        assert!(frames < 400, "the launch never handed over");
    }
    let d = s.bgm.as_ref().expect("audio");
    let hold = applied.iter().filter(|&&l| l == 254).count();
    eprintln!(
        "[new-game-launch] {frames} black frames, {hold} at 254, tail {:?}",
        &applied[applied.len().saturating_sub(20)..]
    );
    assert!(
        (88..=90).contains(&hold),
        "the hold swells the theme for its 0x5A frames"
    );
    assert_eq!(applied.last(), Some(&0), "the ramp fades it to silence");
    assert!(!d.launch_holding(), "the launch frame ends the hold");
    assert_ne!(d.duck_level(), 0, "the scene load rests the level");
}
