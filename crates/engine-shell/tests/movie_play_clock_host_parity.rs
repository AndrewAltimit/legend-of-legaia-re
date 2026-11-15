//! Disc-gated: the play clock counts a movie's ticks on both play hosts.
//!
//! Retail's play counter counts vsyncs under a movie (the movie pair's mode
//! word carries the `0x800` bit, `GameMode::play_counter_runs`). The browser
//! page hands every tick of a display frame through `BootSession::claim_tick`
//! whoever owns it, and the claim counts the tick ahead of naming an owner,
//! so a movie's ticks count there. The native window claims no tick while a
//! movie holds the world and counts them in its own hold. A host that only
//! counts the ticks it claims loses the length of every movie from the save's
//! play time.
//!
//! Both hosts trigger the same movie from `town01` and hold under it; the
//! world's frame counter stands still and the play clock runs, by the same
//! count, at each display rate.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_web_play::runtime::LegaiaRuntime;

/// The movie `town01`'s opening runs into (`MOV/MV2.STR`).
const FMV_ID: i16 = 1;
/// Sim ticks spent under the movie.
const HELD_TICKS: u32 = 150;

/// Play clock in ticks, and the world's frame counter.
fn clock(w: &legaia_engine_core::world::World) -> (u64, u64) {
    (
        u64::from(w.clock.play_time_seconds) * 60 + u64::from(w.clock.play_tick_phase),
        w.frame,
    )
}

/// `(play ticks, world frames)` each host spent under the movie.
fn held(disc: &str, cadence: &[u32]) -> ((u64, u64), (u64, u64)) {
    // The window: the script's trigger, the tick that enters the cutscene,
    // then the movie the redraw would stage once its STR decoded.
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(disc), "town01").expect("open");
    w.display_frame(0, 4);
    w.session_mut().host.world.cutscene.pending_fmv_trigger = Some(FMV_ID);
    w.display_frame(0, 1);
    assert_eq!(w.session().host.world.mode, SceneMode::Cutscene);
    w.stage_movie(1);
    let n0 = clock(&w.session().host.world);

    // The page: the same trigger; its tick arms the movie lane, which holds
    // until the page installs the stream and reports the last frame shown.
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).expect("read disc"), String::new())
        .expect("load_disc");
    rt.enter_field("town01").expect("enter_field");
    rt.play_fmv_set_supported(true);
    rt.frame_ticks(4, 0, 0).expect("ticks");
    assert!(rt.play_fmv_trigger(FMV_ID), "page triggers the movie");
    rt.frame_ticks(1, 0, 0).expect("ticks");
    let page_mode = |rt: &LegaiaRuntime| rt.debug_session().expect("session").host.world.mode;
    assert_eq!(page_mode(&rt), SceneMode::Cutscene);
    let p0 = clock(&rt.debug_session().expect("session").host.world);

    let (mut t, mut f) = (0u32, 0usize);
    while t < HELD_TICKS {
        let n = cadence[f % cadence.len()].min(HELD_TICKS - t);
        w.display_frame(0, n);
        rt.frame_ticks(n, 0, 0).expect("ticks");
        t += n;
        f += 1;
    }
    assert_eq!(w.session().host.world.mode, SceneMode::Cutscene);
    assert_eq!(page_mode(&rt), SceneMode::Cutscene);
    let n1 = clock(&w.session().host.world);
    let p1 = clock(&rt.debug_session().expect("session").host.world);
    ((n1.0 - n0.0, n1.1 - n0.1), (p1.0 - p0.0, p1.1 - p0.1))
}

#[test]
fn a_movies_ticks_count_on_both_hosts() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for cadence in [&[1][..], &[2], &[0, 1]] {
        let (native, page) = held(&disc, cadence);
        eprintln!("[movie-clock] cadence {cadence:?}: native {native:?}, page {page:?}");
        assert_eq!(
            native.1, 0,
            "cadence {cadence:?}: the window's world ticked under the movie"
        );
        assert_eq!(
            page.1, 0,
            "cadence {cadence:?}: the page's world ticked under the movie"
        );
        assert_eq!(
            native.0,
            u64::from(HELD_TICKS),
            "cadence {cadence:?}: the window's play clock under the movie"
        );
        assert_eq!(
            page.0,
            u64::from(HELD_TICKS),
            "cadence {cadence:?}: the page's play clock under the movie"
        );
    }
}
