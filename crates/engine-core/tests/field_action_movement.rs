//! The action-movement enhancement on a synthetic field
//! (`WorldToggles::action_movement`, the kernel in
//! `legaia_engine_field::action_movement`).
//!
//! Off, the retail walk and its settle run exactly as they always have: a
//! scripted pad stream - the jump and dash buttons included - drives a world
//! whose trajectory digest is pinned. On, the player jumps and lands, a wall
//! still blocks, a ledge the walk refuses is climbed by jumping, and every
//! pad lock still holds the player.
//!
//! Disc-free.

use legaia_engine_core::world::{ACTION_DASH_BUTTON, ACTION_JUMP_BUTTON, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

const GRID_STRIDE: usize = 0x80;
const GRID_LEN: usize = GRID_STRIDE * 0x80;

/// An all-floor field with a raised 2x2 tile block (tier 1 at `ledge` units,
/// Y down) at tiles (4..6, 4..6) and a wall tile column at x = 9.
fn field_world(ledge: i16) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_z = 320;
    world.terrain.collision_grid = vec![0u8; GRID_LEN];
    world.terrain.floor_height_lut = [0i16; 16];
    world.terrain.floor_height_lut[1] = ledge;
    for (tx, tz) in [(4usize, 4usize), (5, 4), (4, 5), (5, 5)] {
        world.terrain.collision_grid[tz * GRID_STRIDE + tx] = 0x01;
    }
    for tz in 0..GRID_STRIDE {
        world.terrain.collision_grid[tz * GRID_STRIDE + 9] = 0xF0;
    }
    world
}

fn pos(world: &World) -> (i16, i16, i16) {
    let ms = &world.actors[0].move_state;
    (ms.world_x, ms.world_y, ms.world_z)
}

/// A fixed pad stream: walks, runs, diagonals, and the action buttons, so
/// the off-path digest covers every input the switch would read.
fn scripted_pad(frame: u32) -> u16 {
    let up = PadButton::Up.mask();
    let right = PadButton::Right.mask();
    let run = PadButton::R1.mask();
    // The action model's jump and dash buttons (pinned below).
    let jump = PadButton::Square.mask();
    let dash = PadButton::R2.mask();
    match frame {
        0..=29 => up,
        30..=59 => up | right | run,
        60..=69 => right | jump,
        70..=79 => right | dash,
        80..=99 => 0,
        100..=159 => right | (if frame.is_multiple_of(9) { jump } else { 0 }),
        160..=199 => PadButton::Down.mask() | PadButton::Left.mask(),
        _ => up | jump,
    }
}

/// FNV-1a over the per-frame position, facing, clip base and step delta.
fn digest(world: &mut World, frames: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: i64| {
        for b in v.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for f in 0..frames {
        world.set_pad(scripted_pad(f));
        let _ = world.tick();
        let ms = &world.actors[0].move_state;
        mix(i64::from(ms.world_x));
        mix(i64::from(ms.world_y));
        mix(i64::from(ms.world_z));
        mix(i64::from(ms.render_26));
        mix(i64::from(world.locomotion.step_delta.0));
        mix(i64::from(world.locomotion.step_delta.1));
    }
    h
}

/// The retail walk's digests over [`scripted_pad`] - without and with the
/// opt-in vertical settle (whose ledge hop the raised block triggers) -
/// measured on the controller before the action model existed. A change here
/// is a change to the faithful walk.
const RETAIL_WALK_DIGEST: [u64; 2] = [0x0ac5_703f_d86c_ecb1, 0x12d3_69c6_935c_b18b];

#[test]
fn switched_off_the_walk_is_the_retail_walk() {
    let mut got = [0u64; 2];
    for (i, settle) in [false, true].into_iter().enumerate() {
        let mut world = field_world(-200);
        world.locomotion.vertical_settle = settle;
        assert!(!world.toggles.action_movement, "off by default");
        got[i] = digest(&mut world, 240);
    }
    eprintln!("digests {:#018x} {:#018x}", got[0], got[1]);
    assert_eq!(got, RETAIL_WALK_DIGEST);
}

#[test]
fn the_action_buttons_are_square_and_r2() {
    assert_eq!(ACTION_JUMP_BUTTON, PadButton::Square);
    assert_eq!(ACTION_DASH_BUTTON, PadButton::R2);
}

/// The same stream with the switch on moves the player differently - the
/// pin above is not vacuous - and the switch reports as a retail deviation.
#[test]
fn switched_on_the_stream_takes_the_action_model() {
    let mut world = field_world(-200);
    world.toggles.action_movement = true;
    assert!(
        world
            .toggles
            .retail_deviations()
            .contains(&"action_movement")
    );
    assert_ne!(digest(&mut world, 240), RETAIL_WALK_DIGEST[0]);
    let mut pinned = field_world(-200);
    pinned.toggles.action_movement = true;
    pinned.toggles.pin_retail();
    assert!(
        !pinned.toggles.action_movement,
        "retail-faithful pins it off"
    );
}

fn hold(world: &mut World, pad: u16, frames: u32) {
    for _ in 0..frames {
        world.set_pad(pad);
        let _ = world.tick();
    }
}

#[test]
fn the_player_jumps_and_lands_on_the_floor() {
    let mut world = field_world(-200);
    world.toggles.action_movement = true;
    hold(&mut world, 0, 2);
    let ground = pos(&world).1;
    let mut apex = ground;
    let mut landed = false;
    for f in 0..60 {
        let pad = if f < 20 { ACTION_JUMP_BUTTON.mask() } else { 0 };
        world.set_pad(pad);
        let _ = world.tick();
        assert!(world.action_movement_drove(), "frame {f}: the model drives");
        apex = apex.min(pos(&world).1);
        landed |= f > 0 && world.action_movement_last_step().is_some_and(|s| s.landed);
    }
    assert!(apex < ground - 100, "rose to {apex} from {ground}");
    assert!(landed);
    assert_eq!(pos(&world).1, ground, "back on the floor");
    assert!(
        world.locomotion.ledge_hop.is_none(),
        "the retail hop never starts under the model"
    );
}

#[test]
fn a_wall_still_blocks_the_player() {
    // The wall column is tile x = 9; run at it along z = 320. The model
    // stops where the retail walk stops.
    let mut retail = field_world(-200);
    hold(&mut retail, PadButton::Right.mask(), 200);
    let wall_stop = pos(&retail).0;
    assert!(wall_stop <= 1152);
    let mut world = field_world(-200);
    world.toggles.action_movement = true;
    hold(&mut world, PadButton::Right.mask(), 120);
    assert_eq!(pos(&world).0, wall_stop, "stopped at the wall");
    assert!(world.action_movement_last_step().unwrap().blocked.0);
    // Jumping at it does not get over it either.
    for f in 0..60 {
        let jump = if f % 20 == 0 {
            ACTION_JUMP_BUTTON.mask()
        } else {
            0
        };
        world.set_pad(PadButton::Right.mask() | jump);
        let _ = world.tick();
        assert!(pos(&world).0 <= wall_stop);
    }
}

/// With every tile on the flat-mean model (the object-grid `0x800` bit, the
/// shape authored stairs and ledges take) the raised block rises in two
/// 60-unit steps - each taller than a stair - so the walk is refused at its
/// foot and a jump lands the player on top.
#[test]
fn a_jump_climbs_the_stepped_block_the_walk_refuses() {
    let mut world = field_world(-120);
    world.terrain.object_cells =
        vec![legaia_engine_field::field_elevation::CELL_ELEVATION_OVERRIDE; GRID_LEN];
    world.toggles.action_movement = true;
    // Walk +Z into the block from the south, along x = 576 (tile 4).
    world.actors[0].move_state.world_x = 576;
    hold(&mut world, PadButton::Up.mask(), 40);
    let walked = pos(&world);
    assert_eq!(
        walked.2, 383,
        "refused at the first 60-unit step (tile z 3)"
    );
    assert_eq!(walked.1, 0);
    // Jump up the two steps: hold forward with a press at the start of
    // each 40-frame leg.
    for f in 0..80 {
        let jump = if f % 40 == 0 {
            ACTION_JUMP_BUTTON.mask()
        } else {
            0
        };
        let up = if pos(&world).2 < 600 {
            PadButton::Up.mask()
        } else {
            0
        };
        world.set_pad(up | jump);
        let _ = world.tick();
    }
    let (_, y, z) = pos(&world);
    assert!(y < -50, "climbed onto the block: y {y} at z {z}");
    let floor = world.sample_field_floor_height(576, i32::from(z));
    assert_eq!(i32::from(y), floor, "standing on the floor under it");
}

#[test]
fn every_pad_lock_still_holds_the_player() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut world = field_world(-200);
    world.toggles.action_movement = true;
    world.cutscene.timeline = Some(CutsceneTimeline::new(vec![0x21, 0x2E, 0x1A], 0));
    assert!(world.cutscene_timeline_active());
    let start = pos(&world);
    for _ in 0..10 {
        world.set_pad(PadButton::Up.mask() | ACTION_JUMP_BUTTON.mask());
        world.step_field_locomotion();
    }
    assert_eq!(pos(&world), start, "a cutscene timeline owns the player");
    assert!(!world.action_movement_drove());
    world.cutscene.timeline = None;
    // The movement-disabled bit.
    world.actors[0].move_state.flags |= 0x0008_0000;
    hold(&mut world, PadButton::Up.mask(), 10);
    assert_eq!(pos(&world), start);
    world.actors[0].move_state.flags &= !0x0008_0000;
    hold(&mut world, PadButton::Up.mask(), 10);
    assert!(pos(&world).2 > start.2, "released, the model walks again");
}

/// A body that is airborne when a lock takes the player is set down, not
/// left hanging.
#[test]
fn a_lock_mid_jump_sets_the_player_down() {
    let mut world = field_world(-200);
    world.toggles.action_movement = true;
    hold(&mut world, 0, 2);
    let ground = pos(&world).1;
    hold(&mut world, ACTION_JUMP_BUTTON.mask(), 6);
    assert!(pos(&world).1 < ground - 40);
    world.actors[0].move_state.flags |= 0x0008_0000;
    world.set_pad(0);
    world.step_field_locomotion();
    world.step_field_vertical(0);
    assert_eq!(pos(&world).1, ground);
    assert!(world.action_movement_body().is_none());
}
