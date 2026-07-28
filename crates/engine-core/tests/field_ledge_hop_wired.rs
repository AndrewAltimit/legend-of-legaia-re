//! The ledge hop **moves the player** - pinned end to end, from the classify
//! to the landing, so the wire cannot silently regress into the detect-and-drop
//! shape it used to have.
//!
//! `World::try_field_ledge_hop` (`FUN_801d1878`) classifies an authored ledge
//! and starts the retail arc setup (`FUN_801d2404`); `World::step_field_vertical`
//! then ticks the two clips the setup seeded - the arc helper's
//! `FUN_801d5c08` (which evaluates the Bezier and writes the player's
//! transform) and the paired helper's `FUN_801d2298` (the phase machine that
//! holds the movement lock). Both are ported in
//! `legaia_engine_vm::field_ledge_hop_arc`.
//!
//! This file replaces a test that asserted the opposite - that a posted hop
//! moved nothing - which was correct when it was written and is the reason the
//! wiring gap was findable at all. The invariants it pinned (the record is
//! posted with a real landing point 96 units ahead; the record's lifetime is
//! observable) are kept here, restated against the wired behaviour.
//!
//! Disc-free: the world is synthetic, so nothing here is gated on
//! `LEGAIA_DISC_BIN`.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};

/// Collision-grid geometry (`World::field_collision_grid` is a `0x80 x 0x80`
/// byte grid; the crate-private constants are mirrored here because this is an
/// integration test).
const GRID_STRIDE: usize = 0x80;
const GRID_LEN: usize = GRID_STRIDE * 0x80;

/// Retail's movement-disabled / hop-lock bit on the player actor's `+0x10`.
const MOVE_LOCK: u32 = 0x0008_0000;

/// A field world whose collision grid is all-floor, with the 2x2 tile block
/// covering the hop's floor-sample point on elevation tier `1` at `height`
/// world units - so walking `+Z` faces a ledge.
///
/// World Y grows downward and the height LUT is loaded from the MAN header's
/// *negated* shorts, so a negative `height` is a raised tile (a step **up**)
/// and a positive one is a drop.
///
/// Mirrors the `ledge_world` helper the in-crate locomotion tests use.
fn ledge_world(height: i16) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_z = 320;
    world.field_collision_grid = vec![0u8; GRID_LEN];
    world.field_vertical_settle = true;
    world.field_floor_height_lut = [0i16; 16];
    world.field_floor_height_lut[1] = height;
    for (tx, tz) in [(2usize, 2usize), (3, 2), (2, 3), (3, 3)] {
        world.field_collision_grid[tz * GRID_STRIDE + tx] = 0x01;
    }
    world
}

fn locked(world: &World) -> bool {
    world.actors[0].move_state.flags & MOVE_LOCK != 0
}

/// The whole flight, frame by frame: the player leaves the take-off point,
/// arcs above both endpoints, lands exactly on the posted target, and is
/// released six frames later.
#[test]
fn a_posted_hop_carries_the_player_to_the_landing_point() {
    let mut world = ledge_world(-200);
    world.set_pad(PadButton::Up.mask());
    let _ = world.tick();

    // Non-vacuous, as the superseded gap test was: the hop really is detected,
    // and its landing point is a real position 96 units ahead.
    let hop = world
        .field_ledge_hop
        .expect("the vertical controller starts the hop");
    assert!(hop.is_up(), "a raised tile is retail hop class 0x18");
    let (start_x, start_y, start_z) = {
        let ms = &world.actors[0].move_state;
        (ms.world_x, ms.world_y, ms.world_z)
    };
    assert_eq!(hop.target_z, start_z + 96);
    assert_eq!(hop.target_x, start_x);
    assert_ne!(
        hop.target_y, start_y,
        "the landing sits off the take-off height, so the hop is visible"
    );
    assert!(locked(&world), "the setup locks the player for the flight");
    // The clip is seeded but has not stepped: the setup frame only posts.
    assert_eq!(hop.arc.cursor, 0);
    assert_eq!(hop.phase.cursor, 0);

    // Release the pad - the clip is committed, not pad-driven.
    world.set_pad(0);
    let mut peak_y = start_y;
    let mut take_off_cue = None;
    let mut landing_cue = None;
    let mut landed_at = None;
    let mut released_at = None;
    for frame in 1..64 {
        let _ = world.tick();
        let Some(h) = world.field_ledge_hop else {
            break;
        };
        match h.sfx {
            Some(0x2A) => take_off_cue = Some(frame),
            Some(0x29) => landing_cue = Some(frame),
            _ => {}
        }
        let (x, y, z) = {
            let ms = &world.actors[0].move_state;
            (ms.world_x, ms.world_y, ms.world_z)
        };
        peak_y = peak_y.min(y);
        if h.landed && landed_at.is_none() {
            landed_at = Some(frame);
            assert_eq!(
                (x, y, z),
                (hop.target_x, hop.target_y, hop.target_z),
                "the arc snaps the player onto the landing triple verbatim"
            );
        }
        if h.finished {
            released_at = Some(frame);
            assert!(
                !locked(&world),
                "the phase machine's end arm releases the movement lock"
            );
        } else {
            assert!(locked(&world), "and holds it until then");
        }
    }

    // Retail's clip is 16 frames of arc; the phase machine then holds the
    // lock for six more (`extent + 6`, `0x801D2388..0x801D2394`).
    assert_eq!(
        take_off_cue,
        Some(1),
        "take-off cue on the zero-cursor frame"
    );
    assert_eq!(landed_at, Some(0x10), "0x1000 / 0x100 = 16 arc frames");
    assert_eq!(released_at, Some(0x16), "landing + 6 frames of recovery");
    assert_eq!(landing_cue, released_at);

    // World Y grows downward, so clearing both endpoints means dipping below
    // the higher (numerically smaller) of the two.
    assert!(
        peak_y < start_y.min(hop.target_y),
        "the trajectory is an arc, not a lerp: peak {peak_y} vs endpoints \
         {start_y} / {}",
        hop.target_y
    );

    // One more frame and the record is reaped, so the walk controller owns
    // the player again.
    let _ = world.tick();
    assert!(
        world.field_ledge_hop.is_none(),
        "a finished session is reaped on the next tick"
    );
    assert!(!locked(&world), "and the player is free to walk");
}

/// The drop class flies the same clip with the shorter apex.
#[test]
fn the_drop_class_lands_on_its_target_too() {
    let mut world = ledge_world(200);
    world.set_pad(PadButton::Up.mask());
    let _ = world.tick();
    let hop = world.field_ledge_hop.expect("a drop is a ledge too");
    assert!(!hop.is_up(), "a floor below is retail hop class 0x10");
    assert_eq!(hop.kind, 0x10);

    world.set_pad(0);
    for _ in 0..0x10 {
        let _ = world.tick();
    }
    let ms = &world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_y, ms.world_z),
        (hop.target_x, hop.target_y, hop.target_z)
    );
}

/// The session is a committed clip, not a per-frame reading of the world: it
/// survives a released pad and advances the same clip rather than re-posting.
#[test]
fn the_hop_record_outlives_the_frame_that_started_it() {
    let mut world = ledge_world(-200);
    world.set_pad(PadButton::Up.mask());
    let _ = world.tick();
    let first = world.field_ledge_hop.expect("started while walking");

    world.set_pad(0);
    let _ = world.tick();
    let second = world
        .field_ledge_hop
        .expect("the record survives a frame - it is stepped, not cleared");
    assert_eq!(
        (second.target_x, second.target_y, second.target_z),
        (first.target_x, first.target_y, first.target_z),
        "the same clip, advanced - not a fresh post"
    );
    assert!(second.arc.cursor > first.arc.cursor, "and it advanced");
}
