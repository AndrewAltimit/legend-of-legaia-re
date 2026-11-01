//! Disc-gated: Rebirth's races across Drake's overworld (`scene = "map01"`).
//!
//! - Every course lies on walkable ground: each centerline, sampled every
//!   sixteen world units, and every power-up spot stand on `map01` tiles
//!   with no wall sub-cell in the scene's collision grid. The racers keep
//!   to the session's corridor, not to the overworld's walls, so this is
//!   what keeps a course off the cliffs and the sea.
//! - A sprint runs end to end through the scene host: entered from Rim Elm
//!   it takes the party onto `map01` under the mode seat, stands the leader
//!   on the overworld's heightfield, runs every gate to the finish, and the
//!   closed seat takes the party back to Rim Elm at the pose it left.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::session::{AiTier, SeatKind};
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::modes::RaceCourse;

fn rebirth() -> ExpansionPack {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    ExpansionPack::from_files(&files).expect("Rebirth parses")
}

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    host.activate_expansion_pack(Arc::new(rebirth()))
        .expect("Rebirth activates");
    Some(host)
}

/// A tile's wall sub-cell bits in a `0x80 x 0x80` collision grid.
fn walls(grid: &[u8], x: f64, z: f64) -> u8 {
    let (tx, tz) = (x.floor() as usize, z.floor() as usize);
    grid[tz.min(127) * 128 + tx.min(127)] >> 4
}

#[test]
fn rebirths_overworld_courses_lie_on_walkable_ground() {
    let Some(mut host) = host() else {
        return;
    };
    let pack = rebirth();
    let races: Vec<_> = pack
        .modes
        .races
        .iter()
        .filter(|r| r.scene.is_some())
        .collect();
    assert!(races.len() >= 2, "Rebirth runs its races on the overworld");
    for r in races {
        let scene = r.scene.as_deref().unwrap();
        host.enter_world_map_scene(scene)
            .expect("enter the overworld");
        let grid = host
            .scene
            .as_ref()
            .unwrap()
            .field_collision_grid(&host.index)
            .unwrap()
            .expect("the overworld's collision grid");
        assert_eq!(r.course, RaceCourse::Sprint, "{}", r.id);
        // Tile centres, in tiles.
        let c = |w: [u16; 2]| (f64::from(w[0]) + 0.5, f64::from(w[1]) + 0.5);
        for (k, pair) in r.waypoints.windows(2).enumerate() {
            let (a, b) = (c(pair[0]), c(pair[1]));
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let steps = (len * 8.0).ceil() as usize;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let (x, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                assert_eq!(
                    walls(&grid, x, z),
                    0,
                    "{}: segment {k} crosses a wall at tile ({x:.1}, {z:.1})",
                    r.id
                );
            }
        }
        for (k, p) in r.pickups.iter().enumerate() {
            let at = (f64::from(p.at[0]) + 0.5, f64::from(p.at[1]) + 0.5);
            assert_eq!(
                walls(&grid, at.0, at.1),
                0,
                "{}: pickup {k} on a wall",
                r.id
            );
        }
    }
}

#[test]
fn a_sprint_across_map01_runs_from_rim_elm_to_the_finish_and_back() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.world.set_pad(0);
        host.tick().expect("tick");
    }
    let slot = usize::from(host.world.player_actor_slot.expect("the player"));
    let ms = &host.world.actors[slot].move_state;
    let before = (ms.world_x, ms.world_z);

    host.world
        .enter_pack_mode("drake-circuit")
        .expect("the race builds");
    assert!(host.world.pack_mode_wants_map().is_none(), "no pack map");
    assert_eq!(host.world.pack_mode_wants_scene().as_deref(), Some("map01"));
    let mut entered = false;
    for _ in 0..4 {
        host.world.set_pad(0);
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            assert_eq!(name, "map01");
            entered = true;
        }
    }
    assert!(entered, "the overworld entered under the seat");
    assert_eq!(host.world.mode, SceneMode::Expansion, "the seat survives");
    assert_eq!(host.world.active_scene_label, "map01");
    assert!(host.world.pack_mode_wants_scene().is_none());
    assert!(host.world.shows_overworld());

    let stage = host
        .world
        .pack_mode_stage()
        .expect("the stage runs")
        .clone();
    let track = stage.track.as_ref().expect("the track");
    assert!(!track.closed);
    assert_eq!(track.next_gate, Some(1));
    assert_eq!(track.pickups.len(), 6);
    // The leader stands on the overworld's heightfield, not a flat floor.
    let slot = usize::from(host.world.player_actor_slot.expect("the player"));
    let a = &host.world.actors[slot].move_state;
    let floor = host
        .world
        .sample_field_floor_height(a.world_x.into(), a.world_z.into());
    assert_eq!(i32::from(a.world_y), floor);
    assert!(floor < 0, "the plain under Rim Elm stands above sea level");

    // Hand the player's seat to the AI and run the sprint out.
    let seat = host.world.expansion.mode_seat.as_deref_mut().unwrap();
    seat.active_mut()
        .unwrap()
        .set_seat_kind(0, SeatKind::Ai(AiTier::Expert));
    let mut frames = 0;
    let mut gates = 0;
    let mut last_gate = Some(1);
    while host
        .world
        .expansion
        .mode_seat
        .as_deref()
        .and_then(|s| s.active())
        .is_some_and(|m| !m.is_finished())
    {
        host.world.set_pad(0);
        host.tick().expect("tick");
        let g = host
            .world
            .pack_mode_stage()
            .and_then(|s| s.track.as_ref())
            .and_then(|t| t.next_gate);
        if g != last_gate {
            gates += 1;
            last_gate = g;
        }
        frames += 1;
        assert!(frames < 60 * 60 * 4, "the sprint never finished");
    }
    assert_eq!(gates, 9, "the leader passed every gate in order");

    // Leave the results card: the party is back in Rim Elm where it stood.
    let mut back = false;
    for i in 0..240 {
        host.world.set_pad(if i % 2 == 0 {
            legaia_engine_system::pad::PadButton::Cross.mask()
        } else {
            0
        });
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            assert_eq!(name, "town01");
            back = true;
            break;
        }
    }
    assert!(back, "the closed seat took the party back");
    assert_eq!(host.world.mode, SceneMode::Field);
    let slot = usize::from(host.world.player_actor_slot.expect("the player"));
    let ms = &host.world.actors[slot].move_state;
    assert_eq!((ms.world_x, ms.world_z), before);
}
