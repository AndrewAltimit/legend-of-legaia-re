//! Disc-gated: Rim Elm's intra-scene doorways work in **both** directions.
//!
//! Entering a Rim Elm house is not a scene change - the scene stays `town01`
//! and the player's world position jumps to an interior sub-area of the same
//! collision grid. The mechanism is a `.MAP` **object** whose key tile resolves
//! a kind-1 trigger to a MAN record (`FUN_8003A55C`), and that record's script
//! cross-context teleports the *player* channel (`0xA3 0xF8 <xb> <zb>` - op
//! `0x23 | 0x80` into channel `0xF8`), paired with the arrival heading its
//! preceding `0xB8 0xF8 <dir> 00` (op `0x38 | 0x80`) writes.
//!
//! The records pair by fullwidth-name convention - ＩＮ / ＯＵＴ - and the
//! ＯＵＴ record is what makes the return trip work. Rim Elm ships two such
//! pairs (`恋人` = Mei's house, `木` = the tree), identically in all three of
//! its story-state scenes (`town01` / `town0b` / `town0c` share one
//! partition-0 table and one `.MAP` trigger table).
//!
//! Asserted here, per scene:
//!
//! 1. both pair members install as walk-touch binds with their disc-decoded
//!    target **and facing**;
//! 2. each pair is **reciprocal** - the ＯＵＴ target sits next to the ＩＮ
//!    door and vice versa, so neither landing re-fires the door it just came
//!    through (no ping-pong);
//! 3. pad-walking into the ＩＮ door lands inside with the record's facing,
//!    and pad-walking into the ＯＵＴ door from that landing comes back out
//!    with the ＯＵＴ record's facing.
//!
//! The bind sits at the door **object's** contact-box centre (`FUN_801CFC40`),
//! resolved through the `.MAP` object layer (`FUN_8003A55C`) - the kind-1
//! trigger tile is only the record lookup key and is usually a wall.
//!
//! Structural assertions only (tiles, coords, headings) - no Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! convention).

use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::WalkTouchEvent;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::World;
use std::path::PathBuf;

/// The three Rim Elm story-state scenes (CDNAME blocks at PROT 3 / 12 / 21).
const RIM_ELM_SCENES: [&str; 3] = ["town01", "town0b", "town0c"];

/// One doorway pair, in **contact-box-centre** / landing-world coordinates, as
/// decoded from the ＩＮ / ＯＵＴ records and the `.MAP` object layer.
///
/// The bind position is the object's contact centre (`FUN_801CFC40`), NOT the
/// kind-1 trigger tile: the trigger tile is only the record lookup key
/// (`FUN_8003A55C` reads it at `object_tile + descriptor (dx,dz)`) and is
/// routinely a collision wall the player can never stand on - Rim Elm's own
/// house-door key tile `(38,25)` is inside the house wall.
struct Doorway {
    name: &'static str,
    /// The ＩＮ object's contact centre (outdoors) and its interior landing.
    in_contact: (i16, i16),
    in_target: (i16, i16),
    /// The ＯＵＴ object's contact centre (interior) and its outdoor landing.
    /// Only one representative centre is listed; the tree exit has three
    /// objects side by side.
    out_contact: (i16, i16),
    out_target: (i16, i16),
    /// Engine render headings the two records write (`0` = Z+, `0x800` = Z-).
    in_facing: i16,
    out_facing: i16,
}

/// Rim Elm's two reciprocal doorways. Every field is decoded from the disc by
/// the test itself before it is used - the literals are the pin.
const DOORWAYS: [Doorway; 2] = [
    // 恋人ＩＮ / 恋人ＯＵＴ - Mei's house.
    Doorway {
        name: "mei",
        in_contact: (2240, 3728),
        in_target: (12480, 6976),
        out_contact: (12480, 6784),
        out_target: (2240, 3456),
        in_facing: 0,
        out_facing: 0x800,
    },
    // 木ＩＮ / 木ＯＵＴ - the tree.
    Doorway {
        name: "tree",
        in_contact: (3264, 3728),
        in_target: (4160, 10624),
        out_contact: (4160, 10432),
        out_target: (3264, 3520),
        in_facing: 0,
        out_facing: 0x800,
    },
];

/// Half-width of the walk-touch contact box (`World`'s `FIELD_PROP_BOX_HALF`).
/// A landing this close to a door bind would re-fire it on arrival.
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

fn player(host: &SceneHost) -> (i16, i16, i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    let ms = &host.world.actors[s].move_state;
    (ms.world_x, ms.world_z, ms.render_26)
}

fn seat(host: &mut SceneHost, wx: i16, wz: i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[s].move_state.world_x = wx;
    host.world.actors[s].move_state.world_z = wz;
}

/// The `.MAP`-object door bind whose contact box is centred at `contact`.
fn bind_at(host: &SceneHost, contact: (i16, i16)) -> Option<(i16, i16, Option<i16>)> {
    host.world
        .field_walk_touch
        .iter()
        .filter(|&(&slot, _)| slot >= World::TRIGGER_WALK_TOUCH_SLOT_BASE)
        .find_map(|(_, &(pos, event))| match event {
            WalkTouchEvent::PlayerMoveTo {
                world_x,
                world_z,
                facing,
            } if pos == contact => Some((world_x, world_z, facing)),
            _ => None,
        })
}

/// Hold `pad` until the player's position jumps by more than a tile (the warp)
/// or `frames` elapse. A landing record that opens an inline dialog box parks
/// the timeline, so confirm is mashed on alternate frames to dismiss it.
fn walk_until_warp(host: &mut SceneHost, pad: u16, frames: u32) -> Option<(i16, i16, i16)> {
    let mut prev = player(host);
    for f in 0..frames {
        // A parked landing record owns the frame: dismiss its box, don't walk.
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

/// After a warp the landing record (a gate-1 ambience / dialog beat sitting on
/// the landing tile) may hold the frame; drain it so locomotion resumes.
fn settle(host: &mut SceneHost) {
    for f in 0..80u32 {
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

/// Each doorway's ＩＮ and ＯＵＴ records both install, with the disc-decoded
/// target and arrival facing, in every Rim Elm story-state scene.
#[test]
fn both_pair_members_install_in_every_rim_elm_scene() {
    let Some(mut host) = open_host() else {
        return;
    };
    for scene in RIM_ELM_SCENES {
        host.enter_field_scene(scene, 0).expect("enter scene");
        for _ in 0..3 {
            host.tick().expect("tick");
        }
        for d in &DOORWAYS {
            let inb = bind_at(&host, d.in_contact).unwrap_or_else(|| {
                panic!("{scene}: {} IN bind missing at {:?}", d.name, d.in_contact)
            });
            assert_eq!(
                (inb.0, inb.1),
                d.in_target,
                "{scene}: {} IN lands at the record's decoded tile",
                d.name
            );
            assert_eq!(
                inb.2,
                Some(d.in_facing),
                "{scene}: {} IN carries the record's arrival facing",
                d.name
            );
            let out = bind_at(&host, d.out_contact).unwrap_or_else(|| {
                panic!(
                    "{scene}: {} OUT bind missing at {:?}",
                    d.name, d.out_contact
                )
            });
            assert_eq!(
                (out.0, out.1),
                d.out_target,
                "{scene}: {} OUT lands at the record's decoded tile",
                d.name
            );
            assert_eq!(
                out.2,
                Some(d.out_facing),
                "{scene}: {} OUT carries the record's arrival facing",
                d.name
            );
        }
    }
}

/// The pairs are reciprocal, and neither landing sits inside the contact box
/// of the door it just came through - so a warp cannot immediately re-fire
/// (the ping-pong the naive box model would produce).
#[test]
fn pairs_are_reciprocal_and_do_not_re_fire() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    for d in &DOORWAYS {
        let (in_trig, out_trig) = (d.in_contact, d.out_contact);
        // Reciprocity: the OUT landing is next to the IN trigger, and the IN
        // landing is next to the OUT trigger (same doorway, both faces).
        let near = |a: (i16, i16), b: (i16, i16)| {
            (i32::from(a.0) - i32::from(b.0)).abs() <= 512
                && (i32::from(a.1) - i32::from(b.1)).abs() <= 512
        };
        assert!(
            near(d.out_target, in_trig),
            "{}: the OUT landing returns to the IN doorstep",
            d.name
        );
        assert!(
            near(d.in_target, out_trig),
            "{}: the IN landing arrives at the OUT doorstep",
            d.name
        );
        // Non-re-fire: each landing is outside the *other* record's contact
        // box, so the arrival tick cannot bounce the player straight back.
        for (landing, other) in [(d.in_target, out_trig), (d.out_target, in_trig)] {
            let dx = (i32::from(landing.0) - i32::from(other.0)).abs();
            let dz = (i32::from(landing.1) - i32::from(other.1)).abs();
            assert!(
                dx >= CONTACT_HALF || dz >= CONTACT_HALF,
                "{}: landing {landing:?} must sit outside the paired door's contact box at {other:?}",
                d.name
            );
        }
    }
}

/// The whole round trip through the locomotion: pad-walk into each door, land
/// inside with the ＩＮ record's facing; pad-walk into the interior's exit,
/// come back out at the ＯＵＴ record's tile with its facing.
#[test]
fn every_doorway_round_trips_under_the_locomotion() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    for d in &DOORWAYS {
        // Approach the doorstep from two tiles short of the door object's
        // contact centre and hold toward it (+Z). The doorway tile the object
        // keys off is a wall - retail's contact box is what fires, not standing
        // on the tile.
        let (tx, tz) = d.in_contact;
        seat(&mut host, tx, tz - 2 * 128);
        let landed = walk_until_warp(&mut host, PadButton::Up.mask(), 240)
            .unwrap_or_else(|| panic!("{}: walking into the door never warped", d.name));
        assert_eq!(
            (landed.0, landed.1),
            d.in_target,
            "{}: the door lands the player at the record's interior tile",
            d.name
        );
        assert_eq!(
            landed.2, d.in_facing,
            "{}: arrival faces the record's heading, not the walk-in heading",
            d.name
        );
        settle(&mut host);

        // Now the return leg: from the landing, walk back toward the exit
        // trigger row (-Z) until the OUT record fires.
        let back = walk_until_warp(&mut host, PadButton::Down.mask(), 240)
            .unwrap_or_else(|| panic!("{}: the interior exit never fired", d.name));
        assert_eq!(
            (back.0, back.1),
            d.out_target,
            "{}: the exit returns the player to the record's outdoor tile",
            d.name
        );
        assert_eq!(
            back.2, d.out_facing,
            "{}: the return faces the OUT record's heading",
            d.name
        );
        settle(&mut host);
    }
}
