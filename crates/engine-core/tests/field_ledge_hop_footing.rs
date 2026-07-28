//! What the ledge classifier measures its rise **from** - and why flat ground
//! at a non-zero elevation tier is not a ledge.
//!
//! Retail's `FUN_801d1878` compares the floor `+32` units ahead against the
//! actor's `+0x16`. That field is not free-floating: `FUN_801d1ba0` glides it
//! toward `FUN_80019278(actor)` - the floor under the actor's *current*
//! position - every field frame (`0x801D1C30..0x801D1C68`) and only then calls
//! the classifier (`0x801D1CB0`). So retail's rise is the local step ahead, and
//! a player standing on flat ground at any tier classifies a rise of zero.
//!
//! The engine leaves `world_y` untouched unless one of its two height
//! controllers is on, so reading it as retail's `+0x16` made every non-zero
//! floor tier read as a step and fired a hop on a wall press - the player flew
//! `96` units through the wall. `World::field_actor_footing` is the repair;
//! these are its pins. The wall-press half is the disc-free twin of
//! `engine-shell/tests/field_collision_discriminator.rs`, whose crate boundary
//! is what hid the regression in the first place.
//!
//! Disc-free: every world here is synthetic.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};

/// `World::field_collision_grid` is a `0x80 x 0x80` byte grid; the crate
/// constants are mirrored here because this is an integration test.
const GRID_STRIDE: usize = 0x80;
const GRID_LEN: usize = GRID_STRIDE * 0x80;

/// Retail's movement-disabled / hop-lock bit on the player actor's `+0x10`.
const MOVE_LOCK: u32 = 0x0008_0000;

/// Elevation tiers used by the fixtures. Tier `1` is the flat ground every
/// world below stands on; tier `2` is the raised band the non-vacuity legs
/// step up onto.
const TIER_GROUND: u8 = 1;
const TIER_RAISED: u8 = 2;

/// A field world whose whole collision grid is flat, wall-free ground on
/// elevation tier `1` at `ground` world units.
///
/// World Y grows downward and the height LUT is loaded from the MAN header's
/// *negated* shorts, so a negative height is a raised floor. `ground` is
/// deliberately non-zero: the whole point is that a floor which is flat but not
/// at `Y = 0` must still classify as flat.
fn flat_world(ground: i16) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_z = 480;
    world.field_collision_grid = vec![TIER_GROUND; GRID_LEN];
    world.field_floor_height_lut = [0i16; 16];
    world.field_floor_height_lut[TIER_GROUND as usize] = ground;
    world
}

/// Raise every tile from row `tz` south onto tier `2` at `height`: an authored
/// step running clean across the fixture, so the classifier's `+32` sample
/// crosses a genuine ledge.
fn raise_from_row(world: &mut World, tz: usize, height: i16) {
    world.field_floor_height_lut[TIER_RAISED as usize] = height;
    for row in tz..GRID_STRIDE {
        for col in 0..GRID_STRIDE {
            world.field_collision_grid[row * GRID_STRIDE + col] = TIER_RAISED;
        }
    }
}

/// Wall off every tile from row `tz` south (all four sub-cell bits).
fn wall_from_row(world: &mut World, tz: usize) {
    for row in tz..GRID_STRIDE {
        for col in 0..GRID_STRIDE {
            world.field_collision_grid[row * GRID_STRIDE + col] |= 0xF0;
        }
    }
}

/// The regression, at its tightest: with neither height controller running,
/// `world_y` is whatever placed the actor (`0`) while the floor underfoot is
/// `-192`. Retail's `+0x16` would be `-192` too, so its rise is `0` - inside
/// the `[-0x60, 0x61)` dead band. Reading the stale `world_y` instead made the
/// rise `-192` and started a hop on level ground.
#[test]
fn flat_ground_at_a_non_zero_tier_is_not_a_ledge() {
    let mut world = flat_world(-192);
    world.field_step_delta = (0, 8);
    assert_eq!(
        world.actors[0].move_state.world_y, 0,
        "the fixture leaves Y where the engine's flat-Y default leaves it"
    );
    assert_eq!(
        world.sample_field_floor_height(320, 480),
        -192,
        "and the floor underfoot is nowhere near it"
    );
    assert!(
        !world.try_field_ledge_hop(0),
        "flat ground is flat however stale the actor's world_y is"
    );
    assert!(world.field_ledge_hop.is_none(), "nothing posted");
}

/// Non-vacuity for the leg above: the same fixture with a real step in front
/// of the actor still classifies, so the fix refuses ledges by measuring them,
/// not by refusing everything.
#[test]
fn a_real_step_ahead_of_flat_ground_still_classifies() {
    let mut world = flat_world(-192);
    // Row 4 starts at z = 512; the actor stands at z = 480 so the `+32` sample
    // lands on the raised tile while the actor's own tile stays on the ground.
    raise_from_row(&mut world, 4, -992);
    world.field_step_delta = (0, 8);
    assert!(
        world.try_field_ledge_hop(0),
        "an authored step is still a ledge"
    );
    let hop = world.field_ledge_hop.expect("posted");
    assert_eq!(hop.kind, 0x18, "a raised tile is retail hop class 0x18");
    assert_eq!(
        hop.target_z,
        480 + 96,
        "the landing is three step-deltas ahead"
    );
}

/// The footing is read off `world_y` whenever the engine maintains it - the
/// `play-window` snap and the ported retail glide both count. A settled actor
/// on flat ground classifies nothing, which is the same verdict by the other
/// branch.
#[test]
fn a_maintained_world_y_is_the_footing() {
    for (settle, follow) in [(true, false), (false, true)] {
        let mut world = flat_world(-192);
        world.field_vertical_settle = settle;
        world.follow_terrain_height = follow;
        // What either controller converges the actor to.
        world.actors[0].move_state.world_y = -192;
        world.field_step_delta = (0, 8);
        assert!(
            !world.try_field_ledge_hop(0),
            "settle={settle} follow={follow}: a settled actor on flat ground has no ledge"
        );
    }
}

/// And it really is `world_y` on that branch, not the sampler: retail's
/// `+0x16` genuinely lags during a glide, and a classifier reading a lagging
/// footing genuinely fires. Pinned so the branch's semantics are explicit
/// rather than incidental - this is why the synthetic arc fixtures in
/// `field_ledge_hop_wired.rs` classify on their first frame.
#[test]
fn an_unconverged_glide_classifies_off_the_lagging_footing() {
    let mut world = flat_world(-192);
    world.field_vertical_settle = true;
    world.actors[0].move_state.world_y = 0; // mid-glide, 192 units to fall
    world.field_step_delta = (0, 8);
    assert!(
        world.try_field_ledge_hop(0),
        "retail measures against +0x16, lag included"
    );
}

/// The disc-free twin of the wall-press oracle: pressed into a wall on flat
/// ground at a non-zero tier, the player rests at the standoff and never
/// leaves it. Before the fix the hop fired here and carried the player `96`
/// units past the wall - the arc runs no collision, so nothing stopped it.
#[test]
fn a_wall_press_on_flat_ground_never_hops_through_the_wall() {
    let mut world = flat_world(-192);
    // Row 6 starts at z = 768. Under the `+2` Z-row bias the wall band reads
    // one tile short of that, which is all this leg needs: the player is
    // walking into a wall well ahead of the grid edge.
    wall_from_row(&mut world, 6);
    world.leading_edge_wall_probes = true;
    let start_z = world.actors[0].move_state.world_z;

    let mut prev_z = start_z;
    let mut rest = start_z;
    // Frames on which the hop's own two forward probes both read clear, so the
    // wall gate was NOT what kept the hop quiet there.
    let mut frames_past_the_wall_probes = 0usize;
    for frame in 0..100 {
        world.set_pad(PadButton::Up.mask());
        let _ = world.tick();
        let ms = &world.actors[0].move_state;
        if !world.field_tile_is_wall(320, ms.world_z + 64)
            && !world.field_tile_is_wall(320, ms.world_z + 96)
        {
            frames_past_the_wall_probes += 1;
        }
        assert!(
            world.field_ledge_hop.is_none(),
            "frame {frame}: a wall press is not a ledge (posted a hop at z={})",
            ms.world_z
        );
        assert_eq!(
            ms.flags & MOVE_LOCK,
            0,
            "frame {frame}: nothing may lock the player here"
        );
        assert!(
            ms.world_z - prev_z <= 16,
            "frame {frame}: the player advanced {} units in one frame - that is a \
             flight, not a walk",
            ms.world_z - prev_z
        );
        prev_z = ms.world_z;
        rest = ms.world_z;
    }
    assert!(
        rest > start_z,
        "non-vacuous: the held pad has to move the player ({start_z} -> {rest})"
    );
    assert!(
        !world.field_tile_is_wall(320, rest),
        "the player rests on open floor, not inside the wall band"
    );
    // The load-bearing non-vacuity: the run really did spend frames where the
    // wall gate passed, so the quiet hop is the height band's doing and not the
    // wall probes'. Retail is the same shape - at `rimelm_wall_press_left` the
    // forward probes read the open sub-cell column from `1892` outward while
    // the walk rests at `1838`.
    assert!(
        frames_past_the_wall_probes > 0,
        "vacuous: every frame had a wall inside the hop's forward probes, so \
         this leg never exercised the height band"
    );
}
