//! Disc-gated: a scene-picker entry into a scene whose entry script **parks**
//! the BGM slot plays the track the scene's arrival inherits.
//!
//! Op `0x35` with the park sentinel `0x1000` starts nothing: retail's
//! resolver leaves the slot holding whatever the previous scene was playing,
//! which on every retail path is the music the door brought (`bylon`'s entry
//! parks first and starts `2019` only inside its monastery box). A picked
//! scene has no previous scene, so the park left it silent - or still
//! playing the previously picked scene's track. The picker's seat is a door
//! of some other scene, and that scene's entry track is what the door would
//! have brought; the field host plays it in place of the park start.
//!
//! A door entry (an armed entry operand) runs the park as retail does.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_scene_host::scene_host::{BgmDirector, PickerSeat, SceneHost};

#[derive(Default)]
struct Starts(Vec<u16>);

impl BgmDirector for Starts {
    fn start(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push(id);
    }
    fn start_owned_vab(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push(id);
    }
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return None;
    };
    Some(SceneHost::open_disc(&disc).expect("open disc"))
}

/// Enter `scene` (a picker entry when `door_seat` is `None`) and route the
/// first `frames` ticks' BGM events.
fn enter_and_route(
    host: &mut SceneHost,
    scene: &str,
    door_seat: Option<(i16, i16)>,
    frames: usize,
) -> Starts {
    match door_seat {
        None => {
            host.world.stage_picker_entry(scene, false);
            host.arm_picker_entry_seat(scene);
        }
        Some((x, z)) => host.set_entry_seat(x, z),
    }
    host.enter_field_scene(scene, 0).expect("enter");
    let mut starts = Starts::default();
    for _ in 0..frames {
        let _ = host.tick();
        host.route_bgm_events(&mut starts).expect("route");
    }
    starts
}

#[test]
fn a_picked_parking_scene_plays_its_arrivals_track() {
    let Some(mut host) = host() else { return };
    for scene in ["bylon", "retockin"] {
        let starts = enter_and_route(&mut host, scene, None, 120);
        let Some(PickerSeat::Arrival { arrival, .. }) = host.picker_seat().cloned() else {
            panic!("{scene}: the picker seats the party at a door");
        };
        let inherited = host
            .arrival_catalog()
            .home_bgm(&arrival.from_scene)
            .expect("the arrival's scene starts a track");
        eprintln!(
            "[ok] {scene}: seated off {} -> starts {:?}",
            arrival.from_scene, starts.0
        );
        assert_eq!(
            starts.0.first(),
            Some(&inherited),
            "{scene}: the park start plays {}'s track",
            arrival.from_scene
        );
        assert_eq!(host.world.audio.current_bgm, Some(inherited));
    }
}

#[test]
fn a_door_entry_into_a_parking_scene_keeps_retails_park() {
    let Some(mut host) = host() else { return };
    // bylon's entry parks, then starts its own track only inside the
    // monastery box (tiles x 10..70, z 30..86); seat the party outside it.
    let seat = (5 * 128 + 0x40, 5 * 128 + 0x40);
    let starts = enter_and_route(&mut host, "bylon", Some(seat), 30);
    assert!(
        host.picker_seat().is_none(),
        "a door entry is no picker entry"
    );
    let picked = host
        .arrival_catalog()
        .ranked("bylon")
        .first()
        .and_then(|a| host.arrival_catalog().home_bgm(&a.from_scene))
        .expect("bylon's best arrival names a track");
    eprintln!("[ok] door entry starts {:?}", starts.0);
    assert!(
        !starts.0.contains(&picked),
        "nothing replaces the park on a door entry: {:?}",
        starts.0
    );
    assert!(host.world.field_vm.picker_bgm_inherit.is_none());
}

/// The entry-track scan names the first pool start of record 0, past a
/// leading park: `bylon`'s entry parks, then starts `2019`.
#[test]
fn the_entry_track_skips_a_leading_park() {
    let Some(mut host) = host() else { return };
    let catalog = host.arrival_catalog();
    assert_eq!(catalog.home_bgm("bylon"), Some(2019));
    assert_eq!(catalog.home_bgm("town01"), Some(2016));
}
