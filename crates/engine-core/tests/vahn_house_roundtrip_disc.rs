//! Disc-gated: Vahn's house in Rim Elm (`town01`) round-trips - you can walk
//! in **and back out** - with no story flags set.
//!
//! This is the user-reported bug ("you go into Vahn's house and can't come back
//! out") and the door class it exposed.
//!
//! Rim Elm's doors use **two different** mechanisms, and Vahn's house uses one
//! of each:
//!
//! - the **way in** is a `.MAP` object bind (kind-1 trigger, gate 0) whose MAN
//!   partition-0 record cross-context-teleports the player channel
//!   (`0xA3 0xF8` - the ＩＮ/ＯＵＴ family Mei's house and the tree use for both
//!   directions);
//! - the **way out** is a `.MAP` **kind-0** intra-scene-teleport record - a
//!   plain tile just inside the doorway whose destination is map data, with no
//!   object, no script and no record name.
//!
//! There is no ＯＵＴ record for Vahn's house anywhere in the MAN, and no story
//! flag gates the exit: it was simply a door class the engine never dispatched
//! (`FUN_801D1EC4`'s kind-0 arm). A MAN-only door census cannot see this class
//! at all, which is why the earlier "Vahn's house is a story-entry warp with no
//! exit" reading looked true.
//!
//! Driven through `SceneHost::tick` with pad input - the path `play-window`
//! runs - not by calling the resolvers directly.
//!
//! Structural assertions only (tiles, coords) - no Sony bytes. Skip-passes
//! without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

/// Vahn's-house door object's contact-box centre (outdoors, on the doorstep).
/// `.MAP` object `315` at tile `(37,25)`, key tile `(38,25)`.
const DOOR_CONTACT: (i16, i16) = (4880, 3216);

/// The one walkable approach to that contact box. The door is recessed: the
/// collision grid walls the box off on three sides, leaving a narrow channel
/// due north of it.
const APPROACH: (i16, i16) = (4824, 3110);

/// The ＩＮ record's landing: `0xA3 0xF8` to tile `(97,10)` - the interior
/// sub-area of the same collision grid.
const INTERIOR: (i16, i16) = (12480, 1344);

/// The kind-0 exit record sits at interior tile `(97,9)`, one tile back toward
/// the door, and lands the player at half-tile `(72,46)`:
/// `world = (72*64 + 64, (46+1)*64)` = tile `(36,23)` centre - the doorstep.
const EXIT_LANDING: (i16, i16) = (4672, 3008);

/// Half-width of the walk-touch contact box (`World`'s `FIELD_PROP_BOX_HALF`).
const CONTACT_HALF: i32 = 80;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

fn player(host: &SceneHost) -> (i16, i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    let ms = &host.world.actors[s].move_state;
    (ms.world_x, ms.world_z)
}

fn seat(host: &mut SceneHost, wx: i16, wz: i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[s].move_state.world_x = wx;
    host.world.actors[s].move_state.world_z = wz;
}

/// Hold `pad` until the player's position jumps by more than a tile (the warp)
/// or `frames` elapse. A landing record that opens an inline dialog box parks
/// the timeline, so confirm is mashed on alternate frames to dismiss it.
fn walk_until_warp(host: &mut SceneHost, pad: u16, frames: u32) -> Option<(i16, i16)> {
    let mut prev = player(host);
    for f in 0..frames {
        let pad = if host.world.cutscene_timeline_active() {
            if f.is_multiple_of(2) {
                PadButton::Cross.mask()
            } else {
                0
            }
        } else {
            pad
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
        let p = player(host);
        if (p.0 - prev.0).abs() > 128 || (p.1 - prev.1).abs() > 128 {
            host.world.set_pad(0);
            return Some(p);
        }
        prev = p;
    }
    host.world.set_pad(0);
    None
}

/// Drain a landing record that owns the frame so locomotion resumes.
fn settle(host: &mut SceneHost) {
    for f in 0..120u32 {
        if !host.world.cutscene_timeline_active() {
            break;
        }
        host.world.set_pad(if f.is_multiple_of(2) {
            PadButton::Cross.mask()
        } else {
            0
        });
        host.tick().expect("tick");
    }
    host.world.set_pad(0);
    assert!(
        !host.world.cutscene_timeline_active(),
        "the landing record must not lock the player"
    );
}

/// The disc side: `town01` carries a kind-0 record at interior tile `(97,9)`
/// whose landing is the doorstep - the exit the MAN has no record for.
#[test]
fn vahn_house_exit_is_a_kind0_map_record() {
    let Some(host) = open_host() else {
        return;
    };
    let scene = legaia_engine_core::scene::Scene::load(&host.index, "town01").expect("load town01");
    let (primary, fallback) = scene
        .field_intra_scene_teleports(&host.index)
        .expect("kind-0 table");
    let tp =
        legaia_engine_core::field_regions::lookup_intra_scene_teleport(&primary, &fallback, 97, 9)
            .expect("town01 carries a kind-0 teleport at interior tile (97,9)");
    assert_eq!(
        tp.dest_world(),
        EXIT_LANDING,
        "the exit lands on Vahn's doorstep (dest_x*64+64, (dest_z+1)*64)"
    );
    assert_eq!(tp.dest_tile(), (36, 23), "landing tile = dest >> 1");
    // The landing must sit clear of the ＩＮ door's contact box, or arriving
    // would immediately walk back in.
    let dx = (i32::from(EXIT_LANDING.0) - i32::from(DOOR_CONTACT.0)).abs();
    let dz = (i32::from(EXIT_LANDING.1) - i32::from(DOOR_CONTACT.1)).abs();
    assert!(
        dx >= CONTACT_HALF || dz >= CONTACT_HALF,
        "the exit landing must not sit inside the door's contact box"
    );
}

/// The whole round trip through the locomotion, with **no story flags set** -
/// the state a fresh `play-window --scene town01` boots into. Walk into the
/// door, land inside; walk back at the doorway, land on the doorstep.
#[test]
fn vahn_house_round_trips_under_the_locomotion() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    // No story flags: this is the boot state the user hits.
    assert!(
        !host.world.system_flag_test(0x226),
        "flag 0x226 is clear on a cold town01 entry"
    );
    assert!(
        !host.world.system_flag_test(0x227),
        "flag 0x227 is clear on a cold town01 entry"
    );

    // In: Vahn's door sits in a recessed alcove - the only walkable approach to
    // the object's contact box is the narrow channel due north of it. Seat in
    // that channel and hold toward the door (+Z).
    seat(&mut host, APPROACH.0, APPROACH.1);
    let landed = walk_until_warp(&mut host, PadButton::Up.mask(), 240)
        .expect("walking into Vahn's door never warped");
    assert_eq!(
        landed, INTERIOR,
        "the door lands the player inside at the record's tile (97,10)"
    );
    settle(&mut host);

    // Out: the kind-0 exit tile (97,9) is one tile back toward the door (-Z).
    let back = walk_until_warp(&mut host, PadButton::Down.mask(), 240)
        .expect("the interior exit never fired - Vahn's house has no way out");
    assert_eq!(
        back, EXIT_LANDING,
        "the kind-0 exit lands the player back on the doorstep"
    );

    // And the arrival is outside the door's contact box, so it does not bounce
    // straight back in: keep ticking with no pad and confirm we stay outside.
    settle(&mut host);
    for _ in 0..10 {
        host.world.set_pad(0);
        host.tick().expect("tick");
    }
    let (x, z) = player(&host);
    assert!(
        (i32::from(x) - i32::from(INTERIOR.0)).abs() > 1024,
        "the player stays outside (no ping-pong back into the house)"
    );
    assert_eq!(
        (x >> 7, z >> 7),
        (36, 23),
        "the player rests on the doorstep tile"
    );
}
