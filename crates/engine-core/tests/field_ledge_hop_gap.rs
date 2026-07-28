//! The ledge hop is **detected and dropped** - pinned as an invariant so the
//! disclosure that says so cannot rot.
//!
//! `World::try_field_ledge_hop` (`FUN_801d1878`) is live off the per-frame
//! vertical controller and classifies an authored ledge correctly. The arc
//! that would carry the player over it (`FUN_801d2404` setup +
//! `FUN_801d2298` advance, ported as `legaia_engine_vm::field_ledge_hop_arc`)
//! is inert, because `World::field_ledge_hop` is a per-frame transient with no
//! reader: `World::step_field_vertical` clears it at the top of the next frame
//! before re-posting.
//!
//! So the port has no ledge hop. That is a player-visible absence, not a
//! cosmetic one, and it is easy to mistake for "the hop works but is not
//! animated" - which is why the two facts below are asserted together rather
//! than left implied by the detection tests in `world::tests::locomotion`.
//!
//! **When the hop is wired, this test is expected to fail.** That is its
//! purpose: the wirer should replace it with one asserting the player arrives
//! at `hop.target_*` over `advance_hop_session`'s frame budget.
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

/// A field world whose collision grid is all-floor, with the 2x2 tile block
/// covering the hop's floor-sample point raised to elevation tier `1` at
/// `height` world units - so walking `+Z` faces a `height`-unit step up.
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

#[test]
fn a_posted_hop_does_not_move_the_player() {
    let mut world = ledge_world(200);
    world.set_pad(PadButton::Up.mask());
    let _ = world.tick();

    // Non-vacuous: the hop really was detected, and its landing point is a
    // real position 96 units ahead - so a wired arc would have somewhere to go.
    let hop = world
        .field_ledge_hop
        .expect("the vertical controller posts the hop");
    assert!(hop.is_up(), "a 200-unit rise is retail hop class 0x10");
    let (x_before, z_before) = {
        let ms = &world.actors[0].move_state;
        (ms.world_x, ms.world_z)
    };
    assert_eq!(hop.target_z, z_before + 96);
    assert_ne!(
        hop.target_y, world.actors[0].move_state.world_y,
        "the landing sits above the player, so an applied hop would be visible"
    );

    // Hold the pad and run further frames. Nothing consumes the record, so the
    // player keeps walking on the flat rather than arcing to the landing.
    for _ in 0..8 {
        let _ = world.tick();
    }
    let ms = &world.actors[0].move_state;
    assert_ne!(
        (ms.world_x, ms.world_z),
        (hop.target_x, hop.target_z),
        "no arc carries the player to the landing point - the hop is inert"
    );
    assert_eq!(
        ms.world_x, x_before,
        "and nothing displaces the player off the walk axis either"
    );
}

#[test]
fn the_hop_record_is_a_per_frame_transient() {
    let mut world = ledge_world(200);
    world.set_pad(PadButton::Up.mask());
    let _ = world.tick();
    assert!(world.field_ledge_hop.is_some(), "posted while walking");

    // Release the pad: with no step delta the trigger declines, and the record
    // from the previous frame is gone rather than latched. This is exactly the
    // storage gap the `field_ledge_hop_arc` disclosure names - there is no
    // cursor/extent pair that survives a frame boundary.
    world.set_pad(0);
    let _ = world.tick();
    assert!(
        world.field_ledge_hop.is_none(),
        "the record does not survive a frame - it is cleared, not stepped"
    );
}
