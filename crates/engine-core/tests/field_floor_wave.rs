//! The scene floor-height ladder is **animated**, and the drawn ground has to
//! follow it.
//!
//! Field-VM op `0x4C` outer-nibble 9 owns the sixteen-rung elevation ladder at
//! `0x1F80035C`: sub-`0xE` rewrites all sixteen rungs and sub-`0..2` sets one
//! rung oscillating every frame (`FUN_801DDE34` allocates `0x801F27EC`, tick
//! `FUN_801DA930` stores `pos >> 16` into `0x1F800314 + 0x48 + slot*2`). Retail's
//! per-cell terrain emitters re-read that array every frame, so the floor of
//! `jou`'s organic Seru interior undulates; `4C 90` occurs 180 times across 19
//! scenes.
//!
//! The port resolves its environment draws once per scene, against the scene's
//! MAN-header ladder, so the wave was invisible: only the walk heightfield
//! (`World::sample_field_floor_height`, which reads the live array) moved. These
//! tests pin the kernel that closes that gap -
//! [`legaia_engine_core::field_env::FloorWave`] - end to end through the real
//! field VM.
//!
//! The **sign** is the trap. `Scene::field_floor_height_lut` returns the MAN
//! header's sixteen shorts; `FUN_8003AEB0` installs their **negation** into the
//! scratchpad, which is what `World::field_floor_height_lut` holds and what
//! `FUN_801DA930` writes. `Placement::world_y` consumes the MAN frame (its every
//! term is `-lut[nibble]`). Handing the world's copy straight to a draw resolver
//! therefore inverts the wave. `FloorWave::from_scene_and_world` is where that
//! conversion lives so no host has to pick a sign.

use legaia_asset::field_objects::Placement;
use legaia_engine_core::field_env::{self, EnvDraw, FloorWave};
use legaia_engine_core::world::{SceneMode, World};

/// A placement standing on floor tier `nibble`, at pack slot 0.
fn placed(nibble: u8) -> Placement {
    Placement {
        obj_idx: 1,
        col: 4,
        row: 4,
        anchor_col: 4,
        anchor_row: 4,
        anchor_cell: 0,
        world_x: 0,
        world_z: 0,
        y_off: 0,
        floor_nibble: Some(nibble),
        floor_corner_nibbles: None,
        pack_index: Some(0),
        flags: 0,
        rot_x: 0,
        rot_y: 0,
        rot_z: 0,
        collider_x: 0,
        collider_z: 0,
    }
}

/// A terrain / decoration cell whose 2x2 corner block sits on `corners`.
fn terrain(corners: [u8; 4]) -> Placement {
    Placement {
        floor_nibble: None,
        floor_corner_nibbles: Some(corners),
        ..placed(0)
    }
}

/// `4C 90 <rung> 50 00 16 00 00 80` - the operand triple `jou`'s entry script
/// carries at `P1[0] +0x0190`: period `0x50`, amplitude `0x16`, burst-arm word
/// `0x8000`.
fn arm_rung(rung: u8) -> Vec<u8> {
    vec![0x4C, 0x90, rung, 0x50, 0x00, 0x16, 0x00, 0x00, 0x80]
}

/// Seed a field world at a scene's MAN ladder, exactly as
/// `SceneHost::enter_field_scene` does (the runtime copy is NEGATED).
fn world_on_ladder(man_lut: [i16; 16], script: Vec<u8>) -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.field_frame_step = 1;
    world.field_floor_height_lut = man_lut.map(i16::wrapping_neg);
    world.load_field_script(script);
    world
}

fn draws(placements: &[Placement], man_lut: [i16; 16]) -> Vec<EnvDraw> {
    let (draws, _) = field_env::resolve_env_draws(&[0usize], placements, Some(man_lut));
    assert_eq!(
        draws.len(),
        placements.len(),
        "every placement must resolve"
    );
    draws
}

/// A ladder nobody has moved yields no wave at all - the short-circuit both
/// hosts skip the whole pass on.
#[test]
fn a_still_ladder_is_no_wave() {
    let man = [0x40i16; 16];
    let scratch = man.map(i16::wrapping_neg);
    assert!(FloorWave::from_scene_and_world(Some(man), &scratch).is_none());
    // ... and the inverse pairing IS a wave, which is what makes the assertion
    // above a statement about the sign convention and not about equality.
    assert!(FloorWave::from_scene_and_world(Some(man), &man).is_some());
}

/// End to end through the field VM: `4C 90` on one rung moves every draw
/// standing on that rung, in both the terrain (corner-average) and the placed
/// (single-nibble) flavours, and leaves every other rung's draws alone.
#[test]
fn op_4c_90_moves_the_drawn_ground_under_the_rung_it_arms() {
    let mut man = [0i16; 16];
    man[4] = 0x40;
    man[5] = 0x80;
    let list = [terrain([4, 4, 4, 4]), placed(4), terrain([5, 5, 5, 5])];
    let d = draws(&list, man);
    let baked: Vec<i32> = d.iter().map(|d| d.world_y).collect();
    assert_eq!(baked[0], -0x40, "corner-average of one tier is that tier");
    assert_eq!(baked[1], -0x40);
    assert_eq!(baked[2], -0x80);

    let mut world = world_on_ladder(man, arm_rung(4));
    let mut span = [(i32::MAX, i32::MIN); 3];
    let mut untouched_moved = false;
    for _ in 0..128 {
        let _ = world.tick();
        let Some(wave) = FloorWave::from_scene_and_world(Some(man), &world.field_floor_height_lut)
        else {
            continue;
        };
        for (i, draw) in d.iter().enumerate() {
            let off = wave.offset(&draw.floor);
            span[i].0 = span[i].0.min(off);
            span[i].1 = span[i].1.max(off);
            if i == 2 && off != 0 {
                untouched_moved = true;
            }
        }
    }
    assert_eq!(world.floor_tier_bobs.len(), 1, "the rung must be armed");
    assert!(
        span[0].1 > span[0].0,
        "the terrain cell on rung 4 must swing: {:?}",
        span[0]
    );
    assert!(
        span[1].1 > span[1].0,
        "so must the placed object on rung 4: {:?}",
        span[1]
    );
    assert_eq!(
        span[0], span[1],
        "the corner-average of a uniform block is the single-nibble height, \
         so the two flavours must move identically"
    );
    assert!(
        !untouched_moved,
        "a draw on rung 5 must not move - the wave is per-rung, not global"
    );
}

/// The sign the brief warns about, asserted rather than assumed: routing the
/// world's copy in **unconverted** inverts the wave.
#[test]
fn the_worlds_copy_is_the_scratchpad_frame_not_the_man_frame() {
    let mut man = [0i16; 16];
    man[3] = 0x100;
    let d = draws(&[placed(3)], man);
    let mut world = world_on_ladder(man, arm_rung(3));
    for _ in 0..40 {
        let _ = world.tick();
    }
    let live_scratch = world.field_floor_height_lut;
    let right = FloorWave::from_scene_and_world(Some(man), &live_scratch)
        .expect("the rung has moved")
        .offset(&d[0].floor);
    // The un-converted pairing - the mistake the conversion exists to prevent.
    let wrong = FloorWave::between(man, live_scratch)
        .expect("still a difference")
        .offset(&d[0].floor);
    assert_ne!(right, 0);
    // `height(lut) = -lut[n]`, so the converted form is
    // `live_scratch[3] + man[3]` and the unconverted one is
    // `-live_scratch[3] + man[3]`: they differ by twice the live rung, i.e. the
    // unconverted pairing reflects the wave about the baked height instead of
    // following it.
    assert_eq!(wrong - right, -2 * i32::from(live_scratch[3]));
    assert!(
        (wrong - right).abs() > 0,
        "the rung is off zero, so the two readings must actually differ"
    );
}

/// `4C 9E` re-installs all sixteen rungs mid-scene. That is a floor move too,
/// and the draw list must follow it rather than staying on the MAN ladder.
#[test]
fn op_4c_9e_reinstalls_the_ladder_and_the_draws_follow() {
    let man = [0i16; 16];
    let d = draws(&[terrain([2, 2, 2, 2])], man);
    assert_eq!(d[0].world_y, 0);
    let mut script = vec![0x4C, 0x9E];
    for i in 0..16u16 {
        script.extend_from_slice(&(i * 0x20).to_le_bytes());
    }
    let mut world = world_on_ladder(man, script);
    let _ = world.tick();
    let wave = FloorWave::from_scene_and_world(Some(man), &world.field_floor_height_lut)
        .expect("the install moved the ladder");
    // The install writes `-words[i]` into the scratchpad, i.e. `words[i]` in
    // the MAN frame, so rung 2's draw height goes from `0` to `-(2 * 0x20)`.
    assert_eq!(wave.offset(&d[0].floor), -(2 * 0x20));
}
