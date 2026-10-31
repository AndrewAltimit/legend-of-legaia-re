//! Golf and racing played **in the world**: the `links` fixture pack's
//! map-laid course and track run through `World::tick` with the pack map
//! standing under them, and the in-world stage maps the session onto the
//! field - the leader at the ball on the floor, the ball in the cup's tile,
//! heights from the field image - without changing what the session does.
//!
//! Synthetic world: the map's field image is the one the scene host would
//! seat (`legaia_engine_scene::expansion_map::build_field_map_image`), laid
//! straight onto the world's terrain. No disc, no Sony bytes.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::world::World;
use legaia_engine_core::world::expansion_stage::mode_angle_to_heading;
use legaia_engine_expansion::map::ActiveMap;
use legaia_engine_expansion::mode_stage::LeaderClip;
use legaia_engine_expansion::modes::ModeStage;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::ModeView;
use legaia_engine_modes::golf::GolfPhase;
use legaia_engine_modes::map_frame::MAP_TILE;
use legaia_engine_scene::expansion_map::{BaseInputs, build_field_map_image, floor_lut};
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::Ground;

const CROSS: u16 = PadButton::Cross as u16;

fn links_pack() -> Arc<ExpansionPack> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine-modes/tests/fixtures/links");
    Arc::new(ExpansionPack::from_dir(&dir).expect("the links pack parses"))
}

/// A field world with the links pack active and the player standing on map
/// `map_id`, its field image seated on the terrain. `on_map = false` leaves
/// the player off every map (the mode then draws top-down).
fn links_world(map_id: &str, on_map: bool) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.spawn_actor(0).active = true;
    w.install_field_player(0);
    let pack = links_pack();
    w.activate_expansion(pack.clone())
        .expect("the pack activates");
    if on_map {
        let def = &pack.maps[map_id];
        let grid = def.tile_grid().unwrap();
        // The `base_cell` grounds borrow from the disc's base map and the
        // wall boxes append to its geometry pack; without a disc neither
        // draws. Heights and collision walls are the map's own.
        let mut bare = def.clone();
        for style in bare.tiles.legend.values_mut() {
            if matches!(style.ground, Some(Ground::BaseCell { .. })) {
                style.ground = None;
            }
            style.wall = None;
        }
        let image = build_field_map_image(&bare, &grid, BaseInputs::default()).unwrap();
        w.terrain.collision_grid = image[0x4000..0x8000].to_vec();
        w.terrain.floor_height_lut = floor_lut(def, None).map(i16::wrapping_neg);
        w.expansion.map = Some(ActiveMap::new(
            map_id,
            Arc::new(def.clone()),
            Arc::new(grid),
            (2, 2),
            |_| false,
        ));
    }
    w
}

fn frame(w: &mut World, pad: u16) {
    w.input.set_pad(pad);
    w.tick();
}

fn in_results(w: &World) -> bool {
    matches!(
        w.expansion.mode_seat.as_deref().map(|s| &s.stage),
        Some(ModeStage::Results { .. })
    )
}

fn view(w: &World) -> Option<ModeView> {
    w.expansion.mode_seat.as_deref()?.view()
}

fn hash(w: &World) -> Option<u64> {
    w.expansion
        .mode_seat
        .as_deref()?
        .active()
        .map(|m| m.state_hash())
}

/// The map tile under a world position on `map_id`.
fn tile_at(map_id: &str, x: i32, z: i32) -> Option<(u16, u16)> {
    let pack = links_pack();
    let def = &pack.maps[map_id];
    def.tile_at_world(&def.tile_grid().unwrap(), x, z)
}

/// The golf player's pad: aim straight at the cup, then a short tap on the
/// power meter scaled to the distance.
fn golf_pad(w: &World, n: u32) -> u16 {
    let Some(ModeView::Golf(v)) = view(w) else {
        return 0;
    };
    match v.phase {
        GolfPhase::Aim { seat: 0 } => {
            let want = (v.cup - v.balls[0].pos).angle();
            let d = legaia_engine_modes::fixed::angle_delta(v.balls[0].aim, want);
            if d.abs() > 24 {
                if d > 0 {
                    PadButton::Right as u16
                } else {
                    PadButton::Left as u16
                }
            } else if n.is_multiple_of(2) {
                CROSS
            } else {
                0
            }
        }
        GolfPhase::Power { seat: 0, meter, .. } => {
            let dist = (v.cup - v.balls[0].pos).length() / legaia_engine_modes::fixed::FX_ONE;
            let want = (dist as u32 * 100 / 400).clamp(8, 100);
            if meter >= want && n.is_multiple_of(2) {
                CROSS
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[test]
fn an_in_world_golf_hole_stands_the_leader_at_the_ball() {
    let mut w = links_world("links_green", true);
    w.enter_pack_mode("links-golf").expect("enters");
    assert!(w.pack_mode_wants_map().is_none(), "already on the map");
    frame(&mut w, 0);
    let stage = w
        .pack_mode_stage()
        .expect("the stage runs on its map")
        .clone();
    // The ball sits on the tee tile, the cup on the cup tile.
    let ball = stage.balls.iter().find(|b| b.seat == 0).unwrap();
    assert_eq!(
        tile_at("links_green", ball.pos[0] as i32, ball.pos[1] as i32),
        Some((2, 2))
    );
    let cup = stage.cup.unwrap();
    assert_eq!(
        tile_at("links_green", cup[0] as i32, cup[1] as i32),
        Some((11, 2))
    );
    // The leader stands beside the ball, on the floor, facing the aim.
    let leader = stage.leader.unwrap();
    let a = &w.actors[0].move_state;
    let (dx, dz) = (
        f32::from(a.world_x) - ball.pos[0],
        f32::from(a.world_z) - ball.pos[1],
    );
    assert!(
        (dx * dx + dz * dz).sqrt() < 64.0,
        "the leader is at the ball"
    );
    assert_eq!(
        i32::from(a.world_y),
        w.sample_field_floor_height(a.world_x.into(), a.world_z.into())
    );
    assert_eq!(a.render_26, mode_angle_to_heading(leader.facing));
    assert_eq!(leader.clip, LeaderClip::Idle);
    // The aim arrow starts at the ball.
    assert_eq!(stage.aim.unwrap().from, ball.pos);
    // The hole is laid on the ground for the hosts to tint: its playable
    // tiles are map tiles inside the hole, the ball's and the cup's among
    // them, and its tee is where the ball starts.
    let course = stage.course.as_ref().expect("the hole is laid out");
    assert!(
        (course.tile - 128.0).abs() < 1e-3,
        "a golf tile is a map tile"
    );
    let tile_of = |p: [f32; 2]| tile_at("links_green", p[0] as i32, p[1] as i32);
    let played: Vec<_> = course
        .tiles
        .iter()
        .map(|t| tile_of([t.at[0] + course.tile * 0.5, t.at[1] + course.tile * 0.5]))
        .collect();
    assert!(played.contains(&Some((2, 2))) && played.contains(&Some((11, 2))));
    assert_eq!(tile_of(course.tee), Some((2, 2)));
    assert!(stage.track.is_none());
}

#[test]
fn a_scripted_round_plays_to_completion_in_the_world() {
    let mut w = links_world("links_green", true);
    w.enter_pack_mode("links-golf").expect("enters");
    let (mut swung, mut followed, mut holed_in_cup) = (false, false, false);
    let mut n = 0;
    while !in_results(&w) {
        assert!(n < 60 * 60 * 20, "the round ends");
        let pad = golf_pad(&w, n);
        frame(&mut w, pad);
        n += 1;
        let Some(stage) = w.pack_mode_stage().cloned() else {
            continue;
        };
        swung |= stage.leader.is_some_and(|l| l.clip == LeaderClip::Swing);
        if let Some(ModeView::Golf(v)) = view(&w) {
            // While a ball rolls the camera follows it.
            if let (GolfPhase::Rolling { .. }, Some(r)) = (v.phase, v.rolling) {
                let at = stage.frame.to_world_f(r.pos);
                let b = stage.balls.iter().find(|b| b.rolling).unwrap();
                assert_eq!(b.pos, at, "the drawn ball is the session's ball");
                let d = (stage.shot.focus[0] - at[0]).hypot(stage.shot.focus[1] - at[1]);
                followed |= d < 48.0;
            }
            // A holed ball lies in the cup's tile in the world.
            if v.hole == 0 && v.balls[0].done {
                let [x, z] = stage.frame.to_world(v.balls[0].pos);
                holed_in_cup |= tile_at("links_green", x, z) == Some((11, 2));
            }
            // The leader is on the floor wherever it stands - the plateau
            // included.
            let a = &w.actors[0].move_state;
            assert_eq!(
                i32::from(a.world_y),
                w.sample_field_floor_height(a.world_x.into(), a.world_z.into())
            );
        }
    }
    assert!(swung, "the leader swung");
    assert!(followed, "the camera followed a rolling ball");
    assert!(holed_in_cup, "the player holed out in the cup tile");
    // The plateau stands above the green: the field image's heights.
    let centre = |c: i32| (16 + c) * MAP_TILE + MAP_TILE / 2;
    assert_eq!(w.sample_field_floor_height(centre(6), centre(7)), -48);
    assert_eq!(w.sample_field_floor_height(centre(2), centre(2)), 0);
    // Leaving restores the field and the player's pose.
    let before = w
        .expansion
        .mode_seat
        .as_ref()
        .unwrap()
        .player_return
        .unwrap();
    frame(&mut w, 0);
    frame(&mut w, CROSS);
    assert_eq!(w.mode, SceneMode::Field);
    let a = &w.actors[0].move_state;
    assert_eq!([a.world_x, a.world_y, a.world_z, a.render_26], before);
}

#[test]
fn a_scripted_race_plays_to_completion_in_the_world() {
    let mut w = links_world("links_ring", true);
    w.enter_pack_mode("links-race").expect("enters");
    let (mut countdown, mut chased, mut ran) = (false, false, false);
    let mut n = 0;
    while !in_results(&w) {
        assert!(n < 60 * 60 * 10, "the race ends");
        // Steer at the next checkpoint with the throttle held.
        let mut pad = CROSS;
        if let Some(ModeView::Race(v)) = view(&w) {
            let me = &v.racers[0];
            let m = w.expansion.mode_seat.as_deref().unwrap().active().unwrap();
            let track = m.race_track().unwrap();
            let target = track.points[me.next_cp];
            let want = (target - me.pos).angle();
            let d = legaia_engine_modes::fixed::angle_delta(me.heading, want);
            if d > 40 {
                pad |= PadButton::Right as u16;
            } else if d < -40 {
                pad |= PadButton::Left as u16;
            }
        }
        frame(&mut w, pad);
        n += 1;
        let Some(stage) = w.pack_mode_stage().cloned() else {
            continue;
        };
        countdown |= stage.banner.as_deref() == Some("3");
        // The track is the session's corridor, in world units.
        let m = w.expansion.mode_seat.as_deref().unwrap().active().unwrap();
        let session = m.race_track().unwrap();
        let track = stage.track.as_ref().expect("the track is laid out");
        assert_eq!(track.points.len(), session.points.len());
        assert_eq!(track.points[0], stage.frame.to_world_f(session.points[0]));
        assert_eq!(
            track.half_width,
            stage.frame.len_to_world(session.half_width)
        );
        assert_eq!(track.boost_segments, session.boost_segments);
        assert!(stage.course.is_none());
        ran |= stage.leader.is_some_and(|l| l.clip == LeaderClip::Run);
        assert_eq!(stage.bodies.len(), 2, "two AI racers");
        // Each AI racer is a map NPC (Noa, Gala), its anchor the racer's.
        for b in &stage.bodies {
            let slot = b.actor.expect("the body is a map NPC");
            let (x, z) = w.npcs.positions[&slot];
            assert!((f32::from(x) - b.pos[0]).abs() <= 1.0);
            assert!((f32::from(z) - b.pos[1]).abs() <= 1.0);
        }
        if let Some(ModeView::Race(v)) = view(&w) {
            let me = stage.frame.to_world_f(v.racers[0].pos);
            let a = &w.actors[0].move_state;
            assert!((f32::from(a.world_x) - me[0]).abs() <= 1.0);
            assert!((f32::from(a.world_z) - me[1]).abs() <= 1.0);
            chased |= (stage.shot.yaw - f32::from(v.racers[0].heading)).abs() < 64.0
                && v.racers[0].speed > 0;
        }
    }
    assert!(countdown && chased && ran);
    let ModeStage::Results { rows, .. } = &w.expansion.mode_seat.as_ref().unwrap().stage else {
        unreachable!()
    };
    assert_eq!(rows.len(), 3);
}

#[test]
fn the_stage_does_not_change_the_session() {
    // The same entry and pads with the map under the session (in-world)
    // and without it (the top-down screen): the session state hashes match
    // frame for frame.
    for id in ["links-golf", "links-race"] {
        let map = if id == "links-golf" {
            "links_green"
        } else {
            "links_ring"
        };
        let mut a = links_world(map, true);
        let mut b = links_world(map, false);
        a.enter_pack_mode(id).unwrap();
        b.enter_pack_mode(id).unwrap();
        let mut staged = 0;
        for n in 0..3000u32 {
            let pad = if id == "links-golf" {
                golf_pad(&a, n)
            } else {
                CROSS
                    | if n % 90 < 30 {
                        PadButton::Right as u16
                    } else {
                        0
                    }
            };
            frame(&mut a, pad);
            frame(&mut b, pad);
            staged += usize::from(a.pack_mode_stage().is_some());
            assert!(b.pack_mode_stage().is_none());
            assert_eq!(hash(&a), hash(&b), "{id}: frame {n}");
        }
        assert!(staged > 0, "{id}: the in-world side drew");
    }
}

#[test]
fn dash_boosts_the_racer_under_action_movement() {
    for action in [false, true] {
        let mut w = links_world("links_ring", true);
        w.toggles.action_movement = action;
        w.enter_pack_mode("links-race").unwrap();
        // Past the countdown with the throttle held, then the dash button
        // once the meter is full. A spent meter is the button's boost (a
        // boost pad's costs nothing).
        let (mut boosted, mut meter) = (false, 0);
        for n in 0..900u32 {
            let dash = if n > 700 && n % 4 == 0 {
                legaia_engine_core::world::ACTION_DASH_BUTTON.mask()
            } else {
                0
            };
            frame(&mut w, CROSS | dash);
            if let Some(ModeView::Race(v)) = view(&w) {
                boosted |= v.racers[0].boost_meter < meter;
                meter = v.racers[0].boost_meter;
            }
        }
        assert_eq!(boosted, action, "action movement {action}");
    }
}
