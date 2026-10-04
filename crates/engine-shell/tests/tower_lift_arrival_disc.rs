//! Disc-gated regression: **a Sky Gardens Tower lift ride lands where the
//! player can walk off.**
//!
//! `tower`'s lifts are `.MAP` object doors whose bind records teleport the
//! player (`A3 F8 <x> <z>`) onto the partner lift's platform one floor up or
//! down. Retail brackets the arrival: the riding record runs `B1 <partner>
//! 00` (`+0x10 |= 1`, the collision / touch exemption `FUN_801CF754` /
//! `FUN_801CF9F4` honour) before the scripted walk-off and `B2 <partner> 00`
//! after it. Without the bracket the landing - wedged between two walls and
//! the partner platform - re-fires the partner on the first step off it, and
//! the player rides straight back down: the third floor's west room was
//! unreachable on foot.
//!
//! The test rides P0[6] (second-floor west lift, contact `(992, 5408)`) and
//! then presses every direction from the landing, asserting the player stays
//! on the third floor (the ride back would put it near `z = 5568`) and that
//! the bracket lifts once the player is off the platform.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("extracted")
        });
    if d.join("PROT.DAT").is_file() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn player_xz(host: &SceneHost) -> (i16, i16) {
    let w = &host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (a.move_state.world_x, a.move_state.world_z)
}

fn seat(host: &mut SceneHost, (x, z): (i16, i16)) {
    let w = &mut host.world;
    let slot = w.player_actor_slot.expect("player slot") as usize;
    let y = w.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
    let a = &mut w.actors[slot];
    a.move_state.world_x = x;
    a.move_state.world_y = y;
    a.move_state.world_z = z;
}

fn hold(host: &mut SceneHost, mask: u16, frames: usize) {
    for _ in 0..frames {
        host.world.set_pad(mask);
        let _ = host.tick();
    }
    host.world.set_pad(0);
    for _ in 0..4 {
        let _ = host.tick();
    }
}

const DIRS: [PadButton; 4] = [
    PadButton::Up,
    PadButton::Left,
    PadButton::Down,
    PadButton::Right,
];

#[test]
fn tower_lift_arrival_does_not_ride_back() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("tower", 0).expect("enter tower");
    for _ in 0..90 {
        let _ = host.tick();
    }
    let mut up = None;
    for d in DIRS {
        if let Some(at) = ride(&mut host, d) {
            up = Some((d, at));
            break;
        }
    }
    let Some((up, landing)) = up else {
        panic!("no pad direction from {START:?} rode the P0[6] lift up");
    };
    eprintln!(
        "[tower] {up:?} landed {landing:?}, bracket {:?}",
        host.world.props.arrival_exempt
    );
    assert!(
        !host.world.props.arrival_exempt.is_empty(),
        "the ride brackets its partner platform for the arrival"
    );
    // Every way off the landing keeps the player on the third floor; each
    // direction gets a fresh ride, so each starts under the bracket.
    let mut lifted = false;
    for d in DIRS {
        let at = ride(&mut host, up).expect("the same press rides again");
        assert_eq!(at, landing);
        hold(&mut host, d.mask(), 24);
        let at = player_xz(&host);
        eprintln!("[tower] {d:?} from {landing:?} -> {at:?}");
        assert!(
            at.1 < 3000,
            "stepping {d:?} off the landing rode the partner lift back down (now {at:?})"
        );
        hold(&mut host, d.mask(), 60);
        lifted |= host.world.props.arrival_exempt.is_empty() && player_xz(&host).1 < 3000;
    }
    assert!(
        lifted,
        "walking clear of the platform never lifted the bracket"
    );
}

/// Second floor, west room, just short of the P0[6] lift's contact box.
const START: (i16, i16) = (1000, 5216);

/// Seat the player at [`START`] and hold `d`; the landing when that rode the
/// lift up to the third floor.
fn ride(host: &mut SceneHost, d: PadButton) -> Option<(i16, i16)> {
    host.world.set_pad(0);
    for _ in 0..240 {
        let _ = host.tick();
    }
    host.world.props.arrival_exempt.clear();
    host.world.props.active_walk_touch = None;
    seat(host, START);
    for _ in 0..40 {
        host.world.set_pad(d.mask());
        let _ = host.tick();
        let at = player_xz(host);
        if at.1 < 3000 {
            host.world.set_pad(0);
            return Some(at);
        }
    }
    host.world.set_pad(0);
    eprintln!(
        "[tower] {d:?} from {START:?} did not ride: at {:?}, inline {}",
        player_xz(host),
        host.world.dialog.inline.is_some()
    );
    None
}
