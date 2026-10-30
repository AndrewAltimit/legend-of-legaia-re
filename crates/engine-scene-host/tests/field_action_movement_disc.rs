//! Disc-gated: the action-movement enhancement
//! (`WorldToggles::action_movement`) in a real scene, Rim Elm (`town01`).
//!
//! With the switch on, the player jumps off the real floor and lands back
//! on it, a run at the scene's walls stops on walkable ground, and walking
//! into Mei's house door still fires the door's walk-touch warp - the door
//! reads the player's position and pushed direction, which the action model
//! keeps publishing.
//!
//! Structural assertions only (positions, flags) - no Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_core::world::ACTION_JUMP_BUTTON;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use std::path::PathBuf;

/// Mei's house door object's contact centre and its interior landing, as
/// `rim_elm_door_roundtrip_disc.rs` decodes them.
const MEI_DOOR: (i16, i16) = (2240, 3728);
const MEI_INSIDE: (i16, i16) = (12480, 6976);

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn town01() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    host.world.toggles.action_movement = true;
    Some(host)
}

fn slot(host: &SceneHost) -> usize {
    host.world.player_actor_slot.expect("player installed") as usize
}

fn pos(host: &SceneHost) -> (i16, i16, i16) {
    let ms = &host.world.actors[slot(host)].move_state;
    (ms.world_x, ms.world_y, ms.world_z)
}

fn seat(host: &mut SceneHost, x: i16, z: i16) {
    let s = slot(host);
    host.world.actors[s].move_state.world_x = x;
    host.world.actors[s].move_state.world_z = z;
}

/// Point the left stick so the camera-relative model walks world
/// `(wx, wz)`, whatever the follow camera's azimuth is this frame.
fn stick_toward(host: &mut SceneHost, wx: f64, wz: f64) {
    let az = f64::from(host.world.locomotion.camera_azimuth) / 4096.0 * std::f64::consts::TAU;
    let (s, c) = az.sin_cos();
    let len = (wx * wx + wz * wz).sqrt();
    let (wx, wz) = (wx / len, wz / len);
    // Inverse of the kernel's screen-to-world rotation; stick +Y is down.
    let sx = wx * c - wz * s;
    let sy = wx * s + wz * c;
    host.world
        .input
        .set_lstick(((sx * 127.0) as i8, (-sy * 127.0) as i8));
}

fn floor_under(host: &SceneHost) -> i32 {
    let (x, _, z) = pos(host);
    host.world
        .sample_field_floor_height(i32::from(x), i32::from(z))
}

#[test]
fn the_player_jumps_and_lands_on_the_real_floor() {
    let Some(mut host) = town01() else {
        return;
    };
    seat(&mut host, MEI_DOOR.0, MEI_DOOR.1 - 2 * 128);
    // The seat moves X / Z only: the body drops from the old height onto
    // the floor here.
    for _ in 0..30 {
        host.world.set_pad(0);
        host.tick().expect("tick");
    }
    assert!(
        host.world.action_movement_drove(),
        "the model takes the player"
    );
    let ground = pos(&host).1;
    assert_eq!(
        i32::from(ground),
        floor_under(&host),
        "standing on the floor"
    );
    let mut apex = ground;
    let mut landed = false;
    for f in 0..60 {
        host.world
            .set_pad(if f < 15 { ACTION_JUMP_BUTTON.mask() } else { 0 });
        host.tick().expect("tick");
        apex = apex.min(pos(&host).1);
        landed |= host
            .world
            .action_movement_last_step()
            .is_some_and(|s| s.landed);
    }
    assert!(apex < ground - 100, "rose to {apex} from {ground}");
    assert!(landed, "touched down");
    assert_eq!(i32::from(pos(&host).1), floor_under(&host));
}

#[test]
fn a_run_at_the_walls_stops_on_walkable_ground() {
    let Some(mut host) = town01() else {
        return;
    };
    for (wx, wz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, -1.0)] {
        seat(&mut host, MEI_DOOR.0, MEI_DOOR.1 - 2 * 128);
        let mut last = pos(&host);
        let mut still = 0;
        for _ in 0..400 {
            stick_toward(&mut host, wx, wz);
            host.world.set_pad(0);
            host.tick().expect("tick");
            let p = pos(&host);
            still = if (p.0, p.2) == (last.0, last.2) {
                still + 1
            } else {
                0
            };
            last = p;
            if still > 20 {
                break;
            }
        }
        host.world.input.set_lstick((0, 0));
        assert!(still > 20, "({wx}, {wz}): never came to rest, at {last:?}");
        assert!(
            !host.world.field_tile_is_wall(last.0, last.2),
            "({wx}, {wz}): rests inside a wall at {last:?}"
        );
    }
}

#[test]
fn walking_into_meis_door_still_warps_inside() {
    let Some(mut host) = town01() else {
        return;
    };
    seat(&mut host, MEI_DOOR.0, MEI_DOOR.1 - 2 * 128);
    let mut prev = pos(&host);
    let mut warped = None;
    for f in 0..240u32 {
        if host.world.cutscene_timeline_active() {
            host.world.input.set_lstick((0, 0));
            host.world.set_pad(if f.is_multiple_of(2) {
                PadButton::Cross.mask()
            } else {
                0
            });
        } else {
            stick_toward(&mut host, 0.0, 1.0);
            host.world.set_pad(0);
        }
        host.tick().expect("tick");
        let p = pos(&host);
        if (p.0 - prev.0).abs() > 128 || (p.2 - prev.2).abs() > 128 {
            warped = Some(p);
            break;
        }
        prev = p;
    }
    host.world.input.set_lstick((0, 0));
    let p = warped.expect("walking into the door never warped");
    assert_eq!((p.0, p.2), MEI_INSIDE, "the door lands the player inside");
}
