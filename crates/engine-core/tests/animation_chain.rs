//! Animation chain integration tests.
//!
//! Verifies that `World::tick_actors` advances active `AnimPlayer` instances
//! and stores the resulting `PoseFrame` on the actor. Uses synthetic ANM
//! records that don't require disc data.

use legaia_anm::{AnimPlayer, RECORD_HEADER_SIZE};
use legaia_engine_core::world::{SceneMode, World};

/// Build a minimal valid ANM record for `bone_count` bones. The
/// `src_pos` is all-zero; `dst_pos` is `(10, 20, 30)` for bone 0 to give
/// a non-trivial interpolation target.
fn synth_anm_record(bone_count: usize) -> Vec<u8> {
    let total = RECORD_HEADER_SIZE + 8 * bone_count + 24 * bone_count;
    let mut buf = vec![0u8; total];
    buf[4] = 0x0C;
    buf[5] = 0x08; // marker_1 = 0x080C
    if bone_count > 0 {
        let kf_off = RECORD_HEADER_SIZE + 8 * bone_count;
        let write_i16 = |buf: &mut [u8], off: usize, v: i16| {
            buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        // bone 0: src_pos=(0,0,0), dst_pos=(10,20,30)
        write_i16(&mut buf, kf_off + 6, 10); // dst_pos[0]
        write_i16(&mut buf, kf_off + 8, 20); // dst_pos[1]
        write_i16(&mut buf, kf_off + 10, 30); // dst_pos[2]
    }
    buf
}

#[test]
fn tick_produces_pose_frame_for_active_actor() {
    let mut world = World::new();

    // Activate actor 0 and give it a 2-bone animation.
    world.actors[0].activate();
    let record = synth_anm_record(2);
    let player = AnimPlayer::new(record, 2).expect("synthetic record should be valid");
    world.set_actor_animation(0, player);

    // Before the first tick, no pose frame exists.
    assert!(world.actors[0].pose_frame.is_none());

    world.tick();

    // After one tick, pose_frame should be populated.
    let frame = world.actors[0]
        .pose_frame
        .as_ref()
        .expect("pose_frame should be Some after tick");
    assert_eq!(frame.bone_outputs.len(), 2);
}

#[test]
fn inactive_actor_with_animation_is_not_ticked() {
    let mut world = World::new();

    // Actor 0 is NOT activated (active = false).
    let record = synth_anm_record(1);
    let player = AnimPlayer::new(record, 1).unwrap();
    world.actors[0].active_animation = Some(player);

    world.tick();

    // pose_frame should remain None — inactive actors are skipped.
    assert!(world.actors[0].pose_frame.is_none());
}

#[test]
fn actor_without_animation_remains_pose_none() {
    let mut world = World::new();
    world.actors[0].activate();
    world.mode = SceneMode::Field;

    world.tick();

    assert!(world.actors[0].pose_frame.is_none());
}

#[test]
fn pose_frame_advances_each_tick() {
    let mut world = World::new();
    world.actors[0].activate();
    let record = synth_anm_record(1);
    let mut player = AnimPlayer::new(record, 1).unwrap();
    player.frame_delta = 0x40; // large delta so we see factor movement
    world.set_actor_animation(0, player);

    world.tick();
    let f0 = world.actors[0].pose_frame.clone().unwrap();

    world.tick();
    let f1 = world.actors[0].pose_frame.clone().unwrap();

    // Each tick should advance the factor and produce different bone outputs.
    assert_ne!(f0.factor, f1.factor, "factor should advance between ticks");
}

#[test]
fn set_actor_animation_resets_pose_frame() {
    let mut world = World::new();
    world.actors[0].activate();
    let record = synth_anm_record(1);
    let player = AnimPlayer::new(record.clone(), 1).unwrap();
    world.set_actor_animation(0, player);
    world.tick();
    assert!(world.actors[0].pose_frame.is_some());

    // Setting a new animation should clear the stale pose_frame.
    let player2 = AnimPlayer::new(record, 1).unwrap();
    world.set_actor_animation(0, player2);
    assert!(
        world.actors[0].pose_frame.is_none(),
        "pose_frame should be cleared when a new animation is assigned"
    );
}

#[test]
fn tmd_binding_roundtrip() {
    let mut world = World::new();
    world.actors[3].activate();
    world.set_actor_tmd_binding(3, 7);
    assert_eq!(world.actors[3].tmd_binding, Some(7));

    // Out-of-range slot is silently ignored.
    world.set_actor_tmd_binding(999, 0);
}

#[test]
fn bone_output_is_nonzero_at_midpoint_factor() {
    let mut world = World::new();
    world.actors[0].activate();

    // dst_pos[0] = 10 for bone 0; at factor 0x80 (~50%) we expect ~5.
    let record = synth_anm_record(1);
    let mut player = AnimPlayer::new(record, 1).unwrap();
    player.frame_delta = 0x80;
    world.set_actor_animation(0, player);

    world.tick(); // factor moves from 0 to 0x80
    let frame = world.actors[0].pose_frame.as_ref().unwrap();
    let (pos, _rot) = frame.bone_outputs[0];
    // factor=0x80 → lerp(0, 10, 0x80/0x100) ≈ 5
    assert!(
        pos[0] > 0,
        "interpolated pos[0] should be non-zero at midpoint factor"
    );
}
