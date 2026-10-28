//! Disc-gated: what a door does to the field BGM, by sound-flag bit 2.
//!
//! Op-`0x35` sub-op `6` ("music not change") raises bit 2 of
//! `_DAT_8007B750`. The next field init (`FUN_801D6704`) then leaves the
//! field-BGM source sounding, and the swap poller `FUN_800243F0` latches the
//! entry script's start onto the loaded index without loading it
//! (`0x80024518..0x8002456C`), so the music the door brought plays on under
//! the new scene's track word. Without the bit the init closes the source
//! (`0x801D7154..0x801D7170`) and the poller loads the scene's track afresh.
//!
//! Sol's floors are the measured case: `kor4` starts `2045`, its rooms
//! `koin6` / `koin4` start `2041`, and every `kor4` door into them carries
//! sub-op `6`. The retail route walks `koin6_field_route_walk` /
//! `koin4_field_route_walk` hold `2045` sounding under the word `2041`.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Start(u16),
    Stop,
}

#[derive(Default)]
struct Calls(Vec<Call>);

impl BgmDirector for Calls {
    fn start(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push(Call::Start(id));
    }
    fn start_owned_vab(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push(Call::Start(id));
    }
    fn stop(&mut self) {
        self.0.push(Call::Stop);
    }
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return None;
    };
    Some(SceneHost::open_disc(&disc).expect("open disc"))
}

/// Run op `0x35` sub-op `6` through the field VM, as a door record does
/// before its scene change.
fn raise_music_not_change(host: &mut SceneHost) {
    host.world
        .load_field_script_at(vec![0x35, 0x00, 0x00, 0x06, 0x21], 0);
    host.world.step_field();
}

/// Enter `scene` through a door and route its first `frames` ticks.
fn enter(host: &mut SceneHost, scene: &str, frames: usize) -> Calls {
    host.set_entry_seat_tile(10, 10);
    host.enter_field_scene(scene, 0).expect("enter");
    let mut calls = Calls::default();
    for _ in 0..frames {
        let _ = host.tick();
        host.route_bgm_events(&mut calls).expect("route");
    }
    calls
}

#[test]
fn a_kept_door_plays_on_under_the_rooms_word() {
    let Some(mut host) = host() else { return };
    let calls = enter(&mut host, "kor4", 30);
    eprintln!("kor4: {:?}", calls.0);
    assert!(calls.0.contains(&Call::Start(2045)), "kor4 starts 2045");

    raise_music_not_change(&mut host);
    let calls = enter(&mut host, "koin6", 30);
    eprintln!("koin6 behind sub-op 6: {:?}", calls.0);
    assert!(
        calls.0.is_empty(),
        "bit 2: no stop, no load - 2045 plays on: {:?}",
        calls.0
    );
    assert_eq!(
        host.bgm_track_word,
        Some(2041),
        "the word takes the room's id"
    );
}

#[test]
fn a_plain_door_closes_the_score_and_loads_afresh() {
    let Some(mut host) = host() else { return };
    let _ = enter(&mut host, "kor4", 30);
    // Back into the same floor without sub-op 6: the same track restarts.
    let calls = enter(&mut host, "kor4", 30);
    eprintln!("kor4 again: {:?}", calls.0);
    assert_eq!(
        calls.0.first(),
        Some(&Call::Stop),
        "the init closes the source"
    );
    assert!(
        calls.0.contains(&Call::Start(2045)),
        "and loads 2045 afresh"
    );
}

#[test]
fn a_second_kept_door_latches_again() {
    let Some(mut host) = host() else { return };
    let _ = enter(&mut host, "kor4", 30);
    raise_music_not_change(&mut host);
    let _ = enter(&mut host, "koin6", 30);
    // koin6 -> kor4 behind sub-op 6 again: kor4's 2045 is latched too.
    raise_music_not_change(&mut host);
    let calls = enter(&mut host, "kor4", 30);
    assert!(calls.0.is_empty(), "still 2045, untouched: {:?}", calls.0);
    assert_eq!(host.bgm_track_word, Some(2045));
}

/// The ending theme (`0x814`) sounds from sound source 10, which the field
/// init's close (`0x8007052C` only) never touches: the credits vignettes'
/// inits leave it playing.
#[test]
fn a_field_init_leaves_the_ending_theme_alone() {
    let Some(mut host) = host() else { return };
    let _ = enter(&mut host, "edteien", 5);
    host.world
        .load_field_script_at(vec![0x35, 0x14, 0x08, 0x01, 0x21], 0);
    host.world.step_field();
    let mut calls = Calls::default();
    host.route_bgm_events(&mut calls).expect("route");
    assert_eq!(calls.0, vec![Call::Start(0x814)], "the ending theme starts");
    let calls = enter(&mut host, "edbylon", 5);
    assert!(
        !calls.0.contains(&Call::Stop),
        "no close of the ending source: {:?}",
        calls.0
    );
}

/// The real door, not a synthesized op: `kor4`'s `P2[13]` walk-on door into
/// `koin6` raises sub-op 6 itself, and the room plays on under `2045`.
#[test]
fn kor4s_own_door_keeps_its_music_in_koin6() {
    let Some(mut host) = host() else { return };
    let _ = enter(&mut host, "kor4", 30);
    let taken = host
        .take_door_to("koin6", Some(13))
        .expect("kor4 has the door");
    eprintln!("taken {taken:?}");
    let mut calls = Calls::default();
    for _ in 0..600 {
        let _ = host.tick();
        host.route_bgm_events(&mut calls).expect("route");
        if host.scene.as_ref().is_some_and(|s| s.name == "koin6") {
            break;
        }
    }
    assert_eq!(host.scene.as_ref().map(|s| s.name.as_str()), Some("koin6"));
    for _ in 0..30 {
        let _ = host.tick();
        host.route_bgm_events(&mut calls).expect("route");
    }
    assert!(
        !calls
            .0
            .iter()
            .any(|c| matches!(c, Call::Stop | Call::Start(_))),
        "the door kept 2045: {:?}",
        calls.0
    );
    assert_eq!(host.bgm_track_word, Some(2041));
}
