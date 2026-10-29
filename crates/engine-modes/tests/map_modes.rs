//! Golf and racing laid on pack maps: the `links` fixture pack's course and
//! track build from their maps, the map's tiles become the hole, and the
//! frame places mode units on the field.

use std::path::PathBuf;

use legaia_engine_modes::active::{ActiveMode, BuildOptions, ModeRunner, instance_map};
use legaia_engine_modes::fixed::{FX_ONE, Vec2};
use legaia_engine_modes::golf::course::{RollEnd, Surface, TILE};
use legaia_engine_modes::map_frame::{GOLF_UNIT, MAP_TILE};
use legaia_engine_modes::session::ModeSession;
use legaia_expansion_format::ExpansionPack;

pub fn links_pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/links")
}

fn links() -> ExpansionPack {
    ExpansionPack::from_dir(&links_pack_dir()).expect("the links pack parses")
}

fn build(pack: &ExpansionPack, id: &str) -> ActiveMode {
    ActiveMode::build_with_maps(
        &pack.modes,
        &pack.cards,
        &pack.maps,
        id,
        7,
        &BuildOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn map_instances_name_their_maps() {
    let p = links();
    assert_eq!(
        instance_map(&p.modes, "links-golf"),
        Some(("links_green".into(), String::new()))
    );
    assert_eq!(
        instance_map(&p.modes, "links-race").map(|m| m.0),
        Some("links_ring".into())
    );
    // Without the maps the instances are refused, not played on nothing.
    assert!(
        ActiveMode::build(
            &p.modes,
            &p.cards,
            "links-golf",
            1,
            &BuildOptions::default()
        )
        .is_err()
    );
}

#[test]
fn the_green_is_the_maps_quarter_grid() {
    let p = links();
    let m = build(&p, "links-golf");
    let course = m.golf_course().unwrap();
    let frame = m.map_frame().unwrap().clone();
    // 14 x 10 map tiles, one golf tile each.
    assert_eq!((course.width, course.height), (14, 10));
    assert_eq!(frame.map_id, "links_green");
    assert_eq!(frame.origin, [16 * MAP_TILE, 16 * MAP_TILE]);
    // The tee and the cup sit on their map tiles' centres in the world.
    let centre = |c: i32| (16 + c) * MAP_TILE + MAP_TILE / 2;
    assert_eq!(frame.to_world(course.tee), [centre(2), centre(2)]);
    assert_eq!(frame.to_world(course.cup), [centre(11), centre(2)]);
    // Surfaces: the border is wall, `,` rough by the table, `s` sand and
    // `w` water by their labels.
    let at = |c: i32, r: i32| {
        let q = Vec2::new((c * TILE + 4) * FX_ONE, (r * TILE + 4) * FX_ONE);
        course.tile_at(q)
    };
    assert_eq!(at(0, 0).surface, Surface::Wall);
    assert_eq!(at(1, 1).surface, Surface::Fairway);
    assert_eq!(at(9, 3).surface, Surface::Rough);
    assert_eq!(at(9, 5).surface, Surface::Sand);
    assert_eq!(at(5, 4).surface, Surface::Water);
    // The plateau is the higher rank, doubled.
    assert_eq!(at(6, 7).height, 2);
    assert_eq!(at(1, 7).height, 0);
    // A golf tile is one map tile.
    assert_eq!(TILE * GOLF_UNIT, MAP_TILE);
}

#[test]
fn a_putt_holes_inside_the_cup_tile() {
    let p = links();
    let m = build(&p, "links-golf");
    let course = m.golf_course().unwrap().clone();
    let frame = m.map_frame().unwrap().clone();
    // Straight along the row toward the cup at some power.
    let angle = (course.cup - course.tee).angle();
    let (end, at) = (1..=100)
        .map(|power| course.simulate(course.tee, angle, power))
        .find(|(e, _)| *e == RollEnd::Holed)
        .expect("some power holes the straight putt");
    assert_eq!(end, RollEnd::Holed);
    let [x, z] = frame.to_world(at);
    let def = &p.maps["links_green"];
    let grid = def.tile_grid().unwrap();
    assert_eq!(def.tile_at_world(&grid, x, z), Some((11, 2)));
}

#[test]
fn the_second_hole_uses_its_area() {
    let p = links();
    let mut m = build(&p, "links-golf");
    let ModeRunner::Golf(s) = &mut m.runner else {
        panic!("golf");
    };
    // Finish hole 1 by script: the player holes out, the AI seat too.
    while s.view().hole == 0 {
        s.tick(&[]);
        if let legaia_engine_modes::golf::GolfPhase::Aim { seat: 0 } = s.phase() {
            let to = (s.course().cup - s.view().balls[0].pos).angle();
            s.set_aim(0, to);
            s.strike(0, 30);
        }
    }
    let f = s.frame().unwrap();
    assert_eq!(f.origin, [17 * MAP_TILE, 20 * MAP_TILE]);
    assert_eq!((s.course().width, s.course().height), (12, 5));
}

#[test]
fn the_ring_runs_on_its_waypoints() {
    let p = links();
    let m = build(&p, "links-race");
    let track = m.race_track().unwrap();
    let frame = m.map_frame().unwrap();
    assert_eq!(track.points.len(), 4);
    let centre = |c: i32| (16 + c) * MAP_TILE + MAP_TILE / 2;
    assert_eq!(frame.to_world(track.points[1]), [centre(15), centre(2)]);
    assert_eq!(track.half_width, 90 * FX_ONE);
}
