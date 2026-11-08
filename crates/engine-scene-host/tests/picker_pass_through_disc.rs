//! Disc-gated: a scene-picker visit to a **pass-through** scene plays the
//! arrival beat retail plays there, instead of stepping off it into a free
//! roam the disc never stages (`SceneHost::picker_pass_through`).
//!
//! `taiku2` is the disc's one pass-through. Its only inbound `0x3F` (korout's
//! door, entry tile `(0x2F, 0x15)`) lands on a gate-1 walk-on trigger for
//! `P2[0]`, the whole visit: the castle banner, the throne-room scene, then
//! `0x3F` to `MAP03`. The scene reuses `taiku`'s map, but its pack swaps every
//! room mesh that beat never frames for a three-vertex stub, so a picker seat
//! stepped off the trigger left the party walking wall-less floors (the save
//! room at `(76, 12)` above all). The retail `taiku2_field_door_hop` state
//! shows the same beat running from the arrival.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::{PickerSeat, SceneHost};
use legaia_engine_system::input::PadButton;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// korout's `0x3F` into `taiku2`: entry tile `(0x2F, 0x15)` at its centre.
const TAIKU2_ARRIVAL: (i16, i16) = (0x2F * 128 + 64, 0x15 * 128 + 64);

#[test]
fn a_taiku2_picker_visit_plays_its_arrival_beat_out_to_map03() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc host");
    assert!(host.arm_picker_entry_seat("taiku2"), "picker arm refused");
    host.enter_field_scene("taiku2", 0).expect("enter taiku2");

    // Seated on the arrival itself, not rescued off its trigger.
    match host.picker_seat() {
        Some(PickerSeat::Arrival { seat, rejected, .. }) => {
            assert_eq!(*seat, TAIKU2_ARRIVAL, "seat is korout's arrival tile");
            assert_eq!(*rejected, 0);
        }
        other => panic!("taiku2 picker seat is not its arrival: {other:?}"),
    }
    let ms = &host.world.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), TAIKU2_ARRIVAL);

    // The first tick's stale-tile compare spawns P2[0].
    host.tick().expect("tick");
    assert!(
        host.world.cutscene_timeline_active(),
        "the arrival trigger installed taiku2's P2[0]"
    );

    // Paging its dialogue through, the beat ends in its scene change: the
    // visit never hands the party control in taiku2.
    let mut left = None;
    for f in 0..6000u32 {
        let pad = if f % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
        let now = host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if now != "taiku2" {
            left = Some((f, now));
            break;
        }
    }
    let (f, now) = left.expect("taiku2's arrival beat reaches its scene change");
    assert!(
        now.eq_ignore_ascii_case("map03"),
        "the beat leaves for MAP03, not '{now}'"
    );
    println!("taiku2 -> {now} after {f} ticks");
}

/// The rule leaves every other picker seat alone: `taiku` - the playable
/// castle on the same map - still seats off its triggers and stays put.
#[test]
fn taiku_is_not_a_pass_through() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc host");
    assert!(host.arm_picker_entry_seat("taiku"), "picker arm refused");
    host.enter_field_scene("taiku", 0).expect("enter taiku");
    for _ in 0..90 {
        host.tick().expect("tick");
    }
    let now = host
        .scene
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default();
    assert_eq!(now, "taiku", "a taiku picker visit stays in taiku");
}
