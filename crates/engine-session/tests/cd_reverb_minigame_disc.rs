//! The CD input's reverb send (`SPUCNT` bit 2) reaches the output on the
//! session both play hosts run: the Baka Fighter door warp raises it on the
//! sink's SPU, and the duel's exit drops it.
//!
//! Retail: the duel overlay's init `FUN_801CF00C` writes `cd.reverb = 1`
//! through `SpuSetCommonAttr` (mask `0x300`) and the shared exit writes `0`,
//! so every announcer line rings through Studio C
//! (`World::cd_input_reverb`). This pins the wiring from the world through
//! the director's audio frame to the mix core, not the predicate.
//!
//! Driven over the device-free `TestAudioSink`. Disc-gated: skips and passes
//! when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{AudioSink, SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};

const BAKA_SUB_ID: u8 = 4;

fn session() -> Option<BootSession<TestAudioSink>> {
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

/// One tick and its audio frame, as the hosts run them.
fn step(s: &mut BootSession<TestAudioSink>) {
    s.host.world.set_pad(0);
    s.tick().expect("tick");
    s.sound_baka_announcer();
    let _ = s.step_audio_frame();
}

fn sink_cd_reverb(s: &BootSession<TestAudioSink>) -> bool {
    s.bgm
        .as_ref()
        .expect("audio enabled")
        .audio()
        .with_spu(|spu| spu.cd_reverb)
}

#[test]
fn the_duel_raises_the_cd_reverb_send_on_the_mix_core() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    s.enter_scene_live("koin3", &FieldLiveOpts::default())
        .expect("enter koin3");
    step(&mut s);
    assert!(!sink_cd_reverb(&s), "the casino floor leaves the bit clear");

    s.host.world.request_minigame_warp(BAKA_SUB_ID);
    for _ in 0..120 {
        step(&mut s);
        if s.host.world.mode == SceneMode::BakaFighter {
            break;
        }
    }
    assert_eq!(
        s.host.world.mode,
        SceneMode::BakaFighter,
        "the door warp opens the duel"
    );
    step(&mut s);
    assert!(
        sink_cd_reverb(&s),
        "the duel routes the CD input into the send"
    );
    assert!(s.bgm.as_ref().unwrap().cd_reverb());

    s.host.world.exit_baka_fighter();
    step(&mut s);
    assert_ne!(s.host.world.mode, SceneMode::BakaFighter);
    assert!(!sink_cd_reverb(&s), "the exit drops it");
}
