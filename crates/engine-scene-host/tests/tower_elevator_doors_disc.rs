//! Disc-gated regression: **every Sky Gardens Tower elevator opens its doors.**
//!
//! `tower`'s elevators are `.MAP` placed objects whose partition-0 bind
//! records are the whole ride: on contact the record swings its own door open
//! (`2C 07`, `2C 01`, `2B 03`, `31 00`), walks the player in, closes the door,
//! hides the player, fades, jumps with `A3 F8 x z`, then opens the **partner**
//! door one floor away with cross-context ops on the partner object (`AC <id>
//! 07`, `AC <id> 01`, `AB <id> 03`, `B1 <id> 00`, end latch `AD <id> 08`),
//! walks the player out and closes it.
//!
//! Two defects hid the doors. The walk-touch dispatch applied the record's
//! decoded `MoveTo` on the first contact frame, so the player was on the far
//! floor before the near door moved; and the prop runner stepped the
//! cross-context partner ops on the record's own context, so the partner door
//! never moved and the near door swung a second time, out of view.
//!
//! For each elevator the test presses into the door and asserts that the near
//! door opens while the player still stands at it, that the player then
//! lands on another floor, and that the door the player lands at opens
//! afterwards. The rapid elevator (`P0[4]` / `P0[5]`) runs with its switch
//! `0x1C6` up.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("extracted");
    if root.join("PROT.DAT").exists() {
        Some(root)
    } else {
        eprintln!("[skip] extracted/ missing");
        None
    }
}

fn player_xz(host: &SceneHost) -> (i32, i32) {
    let w = &host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        i32::from(a.move_state.world_x),
        i32::from(a.move_state.world_z),
    )
}

fn dist((ax, az): (i32, i32), (bx, bz): (i32, i32)) -> i32 {
    (ax - bx).abs() + (az - bz).abs()
}

fn open_tower(extracted: &std::path::Path) -> SceneHost {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.enter_field_scene("tower", 0).expect("enter tower");
    for _ in 0..90 {
        let _ = host.tick();
    }
    host
}

type Door = ((u8, u8), usize, (i32, i32));

/// The elevator doors: every bank prop bound to the 30-frame door clip.
fn doors(host: &SceneHost) -> Vec<Door> {
    host.world
        .props
        .bank
        .props
        .iter()
        .filter(|(_, p)| p.anim.anim_id == 1 && p.anim.frames == 30)
        .map(|(&a, p)| (a, p.record, p.collider))
        .collect()
}

struct Ride {
    /// The near door's clip moved while the player still stood at it.
    near_opened_before_jump: bool,
    /// The player left for another floor.
    jumped: bool,
    /// The door the player came out of (nearest to where the ride ended).
    landing_door: Option<((u8, u8), usize)>,
    /// That door's clip moved after the jump.
    landing_opened: bool,
}

/// Seat the player just south of `door` and hold Up into it.
fn ride(extracted: &std::path::Path, door: Door) -> Ride {
    let (anchor, _, (cx, cz)) = door;
    let mut host = open_tower(extracted);
    host.world.system_flag_set(0x1C6);
    let start = (cx, cz - 176);
    {
        let w = &mut host.world;
        let slot = w.player_actor_slot.expect("player slot") as usize;
        let y = w.sample_field_floor_height(start.0, start.1) as i16;
        let ms = &mut w.actors[slot].move_state;
        ms.world_x = start.0 as i16;
        ms.world_z = start.1 as i16;
        ms.world_y = y;
    }
    let all = doors(&host);
    let mut near_opened_before_jump = false;
    let mut jumped = false;
    let mut moved_after_jump = std::collections::BTreeSet::new();
    for f in 0..900 {
        host.world
            .set_pad(if f < 40 { PadButton::Up.mask() } else { 0 });
        let _ = host.tick();
        jumped |= dist(player_xz(&host), start) > 1200;
        for &(a, _, _) in &all {
            if host.world.props.bank.props[&a].anim.cursor <= 0 {
                continue;
            }
            if a == anchor && !jumped {
                near_opened_before_jump = true;
            }
            if jumped {
                moved_after_jump.insert(a);
            }
        }
    }
    let end = player_xz(&host);
    let landing_door = all
        .iter()
        .filter(|&&(a, _, _)| a != anchor)
        .min_by_key(|&&(_, _, c)| dist(c, end))
        .filter(|&&(_, _, c)| dist(c, end) < 600)
        .map(|&(a, r, _)| (a, r));
    Ride {
        near_opened_before_jump,
        jumped,
        landing_opened: landing_door.is_some_and(|(a, _)| moved_after_jump.contains(&a)),
        landing_door,
    }
}

#[test]
fn every_tower_elevator_opens_both_doors() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let all = doors(&open_tower(&extracted));
    assert!(
        all.len() >= 16,
        "tower carries its elevator doors ({} found)",
        all.len()
    );
    let mut failures = Vec::new();
    for door in all {
        let r = ride(&extracted, door);
        eprintln!(
            "[tower] P0[{}] at {:?}: near-open {} jumped {} landing {:?} landing-open {}",
            door.1, door.0, r.near_opened_before_jump, r.jumped, r.landing_door, r.landing_opened
        );
        if !(r.near_opened_before_jump && r.jumped && r.landing_opened) {
            failures.push(door.1);
        }
    }
    assert!(
        failures.is_empty(),
        "elevator records whose doors do not both open: {failures:?}"
    );
}
