//! Leaving the fishing hall from an overworld door lands on the overworld.
//!
//! The return warp `FUN_80026018` latches MAIN INIT (mode `2`, `sh v0,-0x47C4(at)`
//! at `0x80026038`) with no scene-kind test of its own, and the initializer
//! brings back whatever the restored label names. `map02`'s and `map03`'s
//! fishing doors leave from the kingdom map, so the round trip ends there. The
//! engine's return warp set `SceneMode::Field` whatever the departure, which
//! left the overworld under a field tick after every fishing trip.
//!
//! Disc-gated: skips and passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_audio::{SPU_INTERNAL_RATE, TestAudioSink};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_engine_system::input::PadButton;

/// The fishing slot's door-warp sub-id.
const FISHING_SUB_ID: u8 = 0;

#[test]
fn the_fishing_hall_returns_to_the_overworld() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host =
        legaia_engine_scene_host::scene_host::SceneHost::open_disc(std::path::Path::new(&disc))
            .expect("open disc");
    let mut s = BootSession::from_host(host, None, &BootConfig::default(), || {
        Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
    })
    .expect("boot");
    s.enter_world_map_live("map02", &FieldLiveOpts::default())
        .expect("enter map02");
    assert_eq!(s.host.world.mode, SceneMode::WorldMap);
    let step = |s: &mut BootSession<TestAudioSink>, pad: u16| {
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
    };
    for _ in 0..30 {
        step(&mut s, 0);
    }
    s.host.world.request_minigame_warp(FISHING_SUB_ID);
    for _ in 0..60 {
        step(&mut s, 0);
    }
    assert_eq!(
        s.host.world.mode,
        SceneMode::Fishing,
        "the door opens the hall"
    );
    step(&mut s, PadButton::Start.mask());
    for _ in 0..30 {
        step(&mut s, 0);
    }
    assert_eq!(s.host.world.active_scene_label, "map02");
    assert_eq!(
        s.host.world.mode,
        SceneMode::WorldMap,
        "the round trip ends on the kingdom map"
    );
}
