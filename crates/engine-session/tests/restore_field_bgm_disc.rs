//! A card load restores the save's field track through
//! `BootSession::restore_field_bgm`, and a scene-local id there loads retail's
//! fallback track (`legaia_engine_core::scene::bgm_bank_id`) exactly as the
//! op-`0x35` route does. Driven over the device-free `TestAudioSink`, so the
//! session's real director runs with no audio device.
//!
//! Disc-gated: skips and passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_core::scene::{GLOBAL_BGM_BASE, SceneHost};
use legaia_engine_session::{BootConfig, BootSession};

fn session() -> Option<BootSession<TestAudioSink>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = SceneHost::open_disc(std::path::Path::new(&disc)).expect("open disc");
    let cfg = BootConfig::default();
    Some(
        BootSession::from_host(host, None, &cfg, || {
            Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
        })
        .expect("boot session"),
    )
}

#[test]
fn a_scene_local_track_restores_the_fallback_not_silence() {
    let Some(mut s) = session() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let local = 5u16;
    assert!(local < GLOBAL_BGM_BASE);
    s.host.world.audio.current_bgm = Some(local);
    s.restore_field_bgm();
    let bgm = s.bgm.as_ref().expect("audio enabled");
    assert_eq!(bgm.last_started, Some(local), "the scene-local id starts");
    assert!(bgm.is_attached(), "a sequencer is attached");
}

#[test]
fn a_global_track_restores_itself() {
    let Some(mut s) = session() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let global = GLOBAL_BGM_BASE + 2;
    s.host.world.audio.current_bgm = Some(global);
    s.restore_field_bgm();
    assert_eq!(s.bgm.as_ref().expect("audio").last_started, Some(global));
}
