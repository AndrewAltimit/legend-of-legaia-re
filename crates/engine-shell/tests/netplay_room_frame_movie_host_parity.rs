//! Disc-gated: the multiplayer room's frame runs under a movie on both play
//! hosts.
//!
//! The room's frame pumps the connection and steps the room's own clocks
//! (reactions, a party night's timers). The browser page hands every tick of
//! a display frame through `BootSession::claim_tick` whoever owns it and
//! steps the room on each, a movie's ticks among them. The native window
//! claims no tick while a movie holds the world, so a host that only steps
//! the room on the ticks it claims leaves the connection unread for the
//! length of the movie and its room clocks behind the other players'.
//!
//! Both hosts join a room over an in-memory link, trigger the same movie
//! from `town01` and hold under it; the world's frame counter stands still
//! and the room's frame counter runs, by the same count, at each display
//! rate.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_netplay::protocol::RoomCode;
use legaia_netplay::transport::loopback::{LinkConditions, Loopback, SimClock, link};
use legaia_web_play::runtime::LegaiaRuntime;

/// The movie `town01`'s opening runs into (`MOV/MV2.STR`).
const FMV_ID: i16 = 1;
/// Sim ticks spent under the movie.
const HELD_TICKS: u32 = 150;

/// One end of an in-memory link for a host, and the far end to keep open.
fn room_link(clock: &SimClock, seed: u64) -> (Box<Loopback>, Loopback) {
    let (near, far) = link(
        clock,
        LinkConditions::default(),
        LinkConditions::default(),
        seed,
    );
    (Box::new(near), far)
}

/// `(room frames, world frames)` each host spent under the movie.
fn held(disc: &str, cadence: &[u32]) -> ((u32, u64), (u32, u64)) {
    let clock = SimClock::new();
    let code = RoomCode::parse("ABC234").expect("room code");

    let mut w = HeadlessPlayWindow::open(std::path::Path::new(disc), "town01").expect("open");
    let (near, _window_far) = room_link(&clock, 1);
    w.join_room(near, code);
    w.display_frame(0, 4);
    w.session_mut().host.world.cutscene.pending_fmv_trigger = Some(FMV_ID);
    w.display_frame(0, 1);
    assert_eq!(w.session().host.world.mode, SceneMode::Cutscene);
    w.stage_movie(1);
    let n0 = (
        w.room_frames().expect("window in a room"),
        w.session().host.world.frame,
    );

    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).expect("read disc"), String::new())
        .expect("load_disc");
    rt.enter_field("town01").expect("enter_field");
    let (near, _page_far) = room_link(&clock, 2);
    rt.debug_join_room(near, code);
    rt.play_fmv_set_supported(true);
    rt.frame_ticks(4, 0, 0).expect("ticks");
    assert!(rt.play_fmv_trigger(FMV_ID), "page triggers the movie");
    rt.frame_ticks(1, 0, 0).expect("ticks");
    let page = |rt: &LegaiaRuntime| {
        let world = &rt.debug_session().expect("session").host.world;
        (world.mode, world.frame)
    };
    assert_eq!(page(&rt).0, SceneMode::Cutscene);
    let p0 = (rt.debug_room_frames().expect("page in a room"), page(&rt).1);
    assert_eq!(
        n0.0, p0.0,
        "both hosts stepped the room on every tick ahead of the movie"
    );

    let (mut t, mut f) = (0u32, 0usize);
    while t < HELD_TICKS {
        let n = cadence[f % cadence.len()].min(HELD_TICKS - t);
        w.display_frame(0, n);
        rt.frame_ticks(n, 0, 0).expect("ticks");
        t += n;
        f += 1;
    }
    assert_eq!(w.session().host.world.mode, SceneMode::Cutscene);
    assert_eq!(page(&rt).0, SceneMode::Cutscene);
    let n1 = (
        w.room_frames().expect("window in a room"),
        w.session().host.world.frame,
    );
    let p1 = (rt.debug_room_frames().expect("page in a room"), page(&rt).1);
    ((n1.0 - n0.0, n1.1 - n0.1), (p1.0 - p0.0, p1.1 - p0.1))
}

#[test]
fn the_room_frame_runs_under_a_movie_on_both_hosts() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for cadence in [&[1][..], &[2], &[0, 1]] {
        let (native, page) = held(&disc, cadence);
        eprintln!("[movie-room] cadence {cadence:?}: native {native:?}, page {page:?}");
        assert_eq!(
            native.1, 0,
            "cadence {cadence:?}: the window's world ticked under the movie"
        );
        assert_eq!(
            page.1, 0,
            "cadence {cadence:?}: the page's world ticked under the movie"
        );
        assert_eq!(
            page.0, HELD_TICKS,
            "cadence {cadence:?}: the page's room frames under the movie"
        );
        assert_eq!(
            native.0, HELD_TICKS,
            "cadence {cadence:?}: the window's room frames under the movie"
        );
    }
}
