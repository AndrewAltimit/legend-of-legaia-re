//! The overworld walk's **axis convention**, pinned end to end and disc-free.
//!
//! Nothing pinned this before, and the gap is the reason a rung-3 failure of
//! the pad-driven critical-path replay was first read as "Z moves the wrong
//! way on the overworld". Two disc-free tests existed and neither could have
//! caught a sign error in the walk:
//!
//! - `world_map_camera_relative_bits_rotates_with_azimuth` (unit) asserts the
//!   remap's *output bits*, so it re-states the mapping rather than checking
//!   it against anything.
//! - `world_map_camera_remap` (engine-shell) projects the chosen world
//!   direction through the real camera matrix, so it pins remap-vs-camera
//!   agreement - and stays green if remap and camera are rotated **together**.
//!
//! What was missing is the pad-to-`move_state` leg: press a direction, tick,
//! and look at where the actor went. That is what this file asserts, plus the
//! retail contract underneath it.
//!
//! ## Retail's contract (disassembly)
//!
//! `FUN_801D01B0` - the locomotion integrator the world-map-walk overlay and
//! the field overlay share - consumes the **post-remap** pad word masked to
//! `0xF000` and steps one axis per set bit (each arm gated on
//! `FUN_801CFE4C(dir)`):
//!
//! | bit | probe `dir` | store |
//! |---|---|---|
//! | `0x1000` | `2` | `actor[+0x18] += 2` (world **Z+**) |
//! | `0x4000` | `0` | `actor[+0x18] -= 2` (world **Z-**) |
//! | `0x2000` | `3` | `actor[+0x14] += 2` (world **X+**) |
//! | `0x8000` | `1` | `actor[+0x14] -= 2` (world **X-**) |
//!
//! `advance_with_collision` is the port of exactly that, and the first test
//! below pins the four arms against it. The bits are also the raw PSX d-pad
//! layout (Up `0x1000`, Right `0x2000`, Down `0x4000`, Left `0x8000`), so with
//! retail's rotation global clear the overworld d-pad is the identity onto the
//! world axes: Up walks `+Z`, Right walks `+X`.
//!
//! ## The port's frame is offset from that, on purpose
//!
//! The port's overworld camera (`world_map_camera_mvp`) frames azimuth `0`
//! with the eye on `+X`, where retail's yaw-`0` GTE camera looks down `+Z`. So
//! `world_map_camera_relative_bits` carries a compensating rotation and the
//! port's azimuth-`0` Up walks `-X`, not `+Z`. That is a whole-frame offset,
//! which leaves what the player sees identical; a **single-axis** sign flip
//! would not. The third test separates those two: it pins the frame's
//! handedness - the one property the whole-frame offset preserves and a
//! single-axis flip inverts - without pinning which axis `Up` happens to pick.
//!
//! See `docs/subsystems/world-map.md#overworld-axis-convention`.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World, world_map_camera_relative_bits};

/// Unit world-XZ step for a post-remap direction-bit set.
fn bits_to_axis(bits: u16) -> (i32, i32) {
    let z = i32::from(bits & 0x1000 != 0) - i32::from(bits & 0x4000 != 0);
    let x = i32::from(bits & 0x2000 != 0) - i32::from(bits & 0x8000 != 0);
    (x, z)
}

/// A clear overworld with the player parked mid-map.
fn clear_overworld() -> World {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.reset_field_collision_grid(); // present but all-walkable
    world.actors[0].move_state.world_x = 4096;
    world.actors[0].move_state.world_z = 4096;
    world
}

/// Press `pad` for one tick at `azimuth` and report the world-XZ delta,
/// normalised to signs.
fn walk_one_tick(azimuth: i32, pad: u16) -> (i32, i32) {
    let mut world = clear_overworld();
    world.world_map_ctrl.as_mut().unwrap().azimuth = azimuth;
    let before = {
        let ms = &world.actors[0].move_state;
        (i32::from(ms.world_x), i32::from(ms.world_z))
    };
    world.set_pad(pad);
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::WorldMap, "a tick left the overworld");
    let ms = &world.actors[0].move_state;
    (
        (i32::from(ms.world_x) - before.0).signum(),
        (i32::from(ms.world_z) - before.1).signum(),
    )
}

/// The retail bit → world-axis assignment, taken at the mover rather than at
/// the remap: `0x1000` = Z+, `0x4000` = Z-, `0x2000` = X+, `0x8000` = X-.
///
/// This is `FUN_801D01B0`'s four step arms (see the module docs), and it is
/// the layer the "overworld Z is inverted" reading accused. It is not
/// inverted, and this pins it so the accusation cannot be re-raised without
/// evidence.
#[test]
fn direction_bits_step_the_retail_world_axes() {
    for (bits, want) in [
        (0x1000u16, (0, 1)),
        (0x4000, (0, -1)),
        (0x2000, (1, 0)),
        (0x8000, (-1, 0)),
    ] {
        let mut world = clear_overworld();
        let before = {
            let ms = &world.actors[0].move_state;
            (i32::from(ms.world_x), i32::from(ms.world_z))
        };
        world.advance_with_collision(0, bits, 8);
        let ms = &world.actors[0].move_state;
        let got = (
            (i32::from(ms.world_x) - before.0).signum(),
            (i32::from(ms.world_z) - before.1).signum(),
        );
        assert_eq!(
            got, want,
            "dir bits {bits:#06x} stepped {got:?}, want {want:?}"
        );
    }
}

/// A held d-pad direction moves the overworld player, and the direction the
/// remap chose is the direction the actor actually travels.
///
/// The leg that had no coverage: `world_map_camera_relative_bits` ->
/// `advance_with_collision` -> `move_state`. A disagreement anywhere along it
/// (a bit-to-axis swap in the mover, a mover that reads the field remap
/// instead, a dir-bit set the mover silently drops) shows up here and nowhere
/// else.
#[test]
fn every_press_walks_the_way_the_remap_chose() {
    for azimuth in [0i32, 512, 1024, 1536, 2048, 2560, 3072, 3584] {
        for button in [
            PadButton::Up,
            PadButton::Down,
            PadButton::Left,
            PadButton::Right,
        ] {
            let sx = i32::from(button == PadButton::Right) - i32::from(button == PadButton::Left);
            let sy = i32::from(button == PadButton::Up) - i32::from(button == PadButton::Down);
            let want = bits_to_axis(world_map_camera_relative_bits(azimuth, sx, sy));
            let got = walk_one_tick(azimuth, button.mask());
            assert_ne!(want, (0, 0), "az {azimuth} {button:?}: remap chose nothing");
            assert_eq!(
                got, want,
                "az {azimuth} {button:?}: remap chose {want:?}, actor walked {got:?}"
            );
        }
    }
}

/// The four presses form one screen frame, of the **handedness the camera
/// requires**, at every azimuth.
///
/// Stated as invariants of the frame rather than as a table of expected axes,
/// because the port's frame is deliberately offset from retail's (see the
/// module docs) and a table would pin the offset instead of the property:
///
/// 1. at a cardinal azimuth each press yields exactly one world axis,
/// 2. Up and Down are exact opposites, and so are Left and Right,
/// 3. `Up x Right` is `+1` at every azimuth.
///
/// (3) has to name the value, not merely require it constant. Inverting one
/// world axis turns this frame from a reflection into a rotation and leaves
/// (1), (2) and "the handedness never changes" all intact - a
/// self-consistency check cannot see a defect that is self-consistent. `+1` is
/// not a free choice either: it is what the camera geometry forces, and
/// `engine-shell`'s `world_map_camera_remap` (which projects the chosen world
/// direction through the real `world_map_camera_mvp`) fails on both of its
/// assertions if the sign moves. This test carries that verdict down to the
/// layer the camera cannot reach.
#[test]
fn the_four_presses_form_one_consistent_frame() {
    /// `Up x Right` for the port's overworld frame. See the doc comment.
    const CAMERA_HANDEDNESS: i32 = 1;

    let mut handedness = None;
    for azimuth in [0i32, 1024, 2048, 3072] {
        let up = walk_one_tick(azimuth, PadButton::Up.mask());
        let down = walk_one_tick(azimuth, PadButton::Down.mask());
        let left = walk_one_tick(azimuth, PadButton::Left.mask());
        let right = walk_one_tick(azimuth, PadButton::Right.mask());

        for (name, d) in [("up", up), ("down", down), ("left", left), ("right", right)] {
            assert_eq!(
                d.0.abs() + d.1.abs(),
                1,
                "az {azimuth}: {name} walked {d:?}, want one cardinal axis"
            );
        }
        assert_eq!(
            (-up.0, -up.1),
            down,
            "az {azimuth}: Up {up:?} and Down {down:?} are not opposite"
        );
        assert_eq!(
            (-left.0, -left.1),
            right,
            "az {azimuth}: Left {left:?} and Right {right:?} are not opposite"
        );

        // 2D cross product of Up x Right.
        let cross = up.0 * right.1 - up.1 * right.0;
        assert_eq!(
            cross, CAMERA_HANDEDNESS,
            "az {azimuth}: Up {up:?} / Right {right:?} give handedness {cross}, \
             want {CAMERA_HANDEDNESS} - one world axis is sign-inverted"
        );
        match handedness {
            None => handedness = Some(cross),
            Some(h) => assert_eq!(cross, h, "az {azimuth}: handedness is not constant"),
        }
    }
}

/// A quarter turn of the camera turns every press's world direction by a
/// quarter turn, in the same sense.
///
/// The rotation-equivariance half of the frame check: it catches a remap that
/// is right at one azimuth and wrong at another (a quadrant fold, a `sin`/`cos`
/// swap on one branch) even though every individual azimuth passes the frame
/// test above.
#[test]
fn a_quarter_turn_of_the_camera_turns_every_press_a_quarter_turn() {
    for button in [
        PadButton::Up,
        PadButton::Down,
        PadButton::Left,
        PadButton::Right,
    ] {
        let steps: Vec<(i32, i32)> = [0i32, 1024, 2048, 3072]
            .into_iter()
            .map(|az| walk_one_tick(az, button.mask()))
            .collect();
        for w in steps.windows(2) {
            let (from, to) = (w[0], w[1]);
            // One quarter turn about +Y in this XZ frame: the sense is read
            // off the first pair and then required to hold for the rest.
            let rotated = (from.1, -from.0);
            let rotated_other = (-from.1, from.0);
            assert!(
                to == rotated || to == rotated_other,
                "{button:?}: {from:?} -> {to:?} is not a quarter turn"
            );
        }
        let sense: Vec<bool> = steps
            .windows(2)
            .map(|w| w[1] == (w[0].1, -w[0].0))
            .collect();
        assert!(
            sense.iter().all(|&s| s == sense[0]),
            "{button:?}: the quarter turns do not all go the same way ({steps:?})"
        );
    }
}
