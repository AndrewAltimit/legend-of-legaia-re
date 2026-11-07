//! Disc-gated: the play page's overworld entrance crossing follows retail's
//! timeline - the player stops on the first seat inside the entrance tile,
//! the entrance record's exit fade is pushed from the crossing on, and the
//! destination loads only after the streaming actor's hold.
//!
//! The retail film of the same walk (`map01` -> `town0c`, the
//! `overworld_into_town_man_load` state, Down held;
//! `scripts/pcsx-redux/autorun_entrance_film.lua`) has the player at
//! `z = 3198` from the crossing vsync to MAIN INIT, the colour tween's actor
//! installed on that vsync and the frame black 33 vsyncs later
//! (`docs/subsystems/world-map.md#the-crossing-frame`). The native session
//! is pinned by `scene_round_trip_disc.rs`; this is the page's half, through
//! the runtime the browser runs.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

/// The retail state's seat, one step north of `map01`'s Rim Elm entrance
/// tile `(96, 24)`.
const SEAT: (i16, i16) = (12362, 3218);
const DOWN: u16 = 0x0040;

#[test]
fn play_page_entrance_crossing_stops_the_player_and_fades() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).expect("read disc"), String::new())
        .expect("load disc");
    rt.enter_field("map01").expect("enter map01");
    for _ in 0..4 {
        let _ = rt.tick_frame();
    }
    assert!(rt.debug_seat_player(SEAT.0, SEAT.1), "a player to seat");
    let player_z = |rt: &LegaiaRuntime| {
        let w = &rt.debug_session().expect("session").host.world;
        w.actors[usize::from(w.player_actor_slot.expect("player"))]
            .move_state
            .world_z
    };
    let in_flight = |rt: &LegaiaRuntime| {
        rt.debug_session()
            .expect("session")
            .host
            .world
            .scene_change_in_flight()
    };
    let scene = |rt: &LegaiaRuntime| {
        rt.debug_session()
            .expect("session")
            .host
            .world
            .active_scene_label
            .clone()
    };

    let mut crossing = None;
    for f in 0..120u32 {
        rt.set_pad(DOWN);
        let _ = rt.tick_frame();
        if in_flight(&rt) {
            crossing = Some(f);
            break;
        }
    }
    assert!(crossing.is_some(), "the walk reaches the entrance tile");
    let seat_z = player_z(&rt);
    assert_eq!(
        seat_z >> 7,
        24,
        "the crossing lands inside the entrance tile"
    );

    let mut fade_seen = None;
    let mut frames = 0u32;
    while scene(&rt) == "map01" {
        rt.set_pad(DOWN);
        let _ = rt.tick_frame();
        frames += 1;
        assert!(frames < 200, "the parked scene change commits");
        if scene(&rt) != "map01" {
            break;
        }
        assert_eq!(player_z(&rt), seat_z, "the held d-pad walks no further");
        if fade_seen.is_none() && rt.play_screen_effect_push_count() > 0 {
            fade_seen = Some(frames);
        }
    }
    rt.set_pad(0);
    assert_eq!(scene(&rt), "town0c", "the entrance loads Rim Elm");
    let fade = fade_seen.expect("the exit fade is pushed on the page");
    eprintln!("page crossing: seat z {seat_z}, first wash +{fade}, commit +{frames}");
    assert!(
        fade <= 4,
        "the exit fade starts at the crossing (first push {fade} frames after it)"
    );
    assert!(
        frames >= 60,
        "the destination waits out the streaming actor's hold ({frames} frames)"
    );
}
