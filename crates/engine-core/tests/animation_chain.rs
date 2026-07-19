//! Animation chain integration tests.
//!
//! Verifies that `World::tick_actors` advances active `AnimPlayer` instances
//! and stores the resulting `PoseFrame` on the actor. Uses synthetic ANM
//! records that don't require disc data.

use legaia_anm::{AnimPlayer, RECORD_HEADER_SIZE};
use legaia_engine_core::world::{SceneMode, World};
use legaia_tmd::mesh::tmd_to_vram_mesh_posed;

/// Advance the world until the **actor pool's** next pass has run.
///
/// The anim player is stepped by `World::tick_actors`, which is part of the
/// per-actor pool - and retail runs that pool once per *game tick*, a span of
/// `frame_step` vsyncs (`DAT_1F800393`, resolved by `FUN_80016B6C`; the field
/// scene loader `FUN_801D6704` installs a floor of 2). So one `World::tick`
/// is not one anim advance, and these chain tests step to the pass instead of
/// assuming it. The pass is detected by the pose frame's own advance.
fn tick_to_actor_pass(world: &mut World, slot: usize) {
    let before = world.actors[slot].pose_frame.as_ref().map(|f| f.factor);
    for _ in 0..32 {
        world.tick();
        if world.actors[slot].pose_frame.as_ref().map(|f| f.factor) != before {
            return;
        }
    }
    panic!("actor pool did not run within 32 sim ticks");
}

/// Minimal valid Legaia TMD with one object, 5 vertices, 4 FT3 triangles.
fn synth_pyramid_tmd() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    let prim_top: u32 = 28;
    let prim_size: u32 = 8 + (4 + 1) * 20 + 4;
    let vert_top: u32 = prim_top + prim_size;
    buf.extend_from_slice(&vert_top.to_le_bytes());
    buf.extend_from_slice(&5u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&prim_top.to_le_bytes());
    buf.extend_from_slice(&4u32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes());
    buf.extend_from_slice(&0x0020u16.to_le_bytes());
    buf.extend_from_slice(&[7, 5, 1, 0x27]);
    for (a, b, c) in [(4u16, 0u16, 1u16), (4, 1, 2), (4, 2, 3), (4, 3, 0)] {
        let mut prim = vec![0u8; 20];
        prim[14..16].copy_from_slice(&(a * 8).to_le_bytes());
        prim[16..18].copy_from_slice(&(b * 8).to_le_bytes());
        prim[18..20].copy_from_slice(&(c * 8).to_le_bytes());
        buf.extend_from_slice(&prim);
    }
    buf.extend_from_slice(&[0u8; 20]);
    buf.extend_from_slice(&0u32.to_le_bytes());
    for (x, y, z) in [
        (64i16, 85i16, 0i16),
        (0, 85, -64),
        (-64, 85, 0),
        (0, 85, 64),
        (0, -170, 0),
    ] {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&z.to_le_bytes());
        buf.extend_from_slice(&0i16.to_le_bytes());
    }
    buf
}

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

    tick_to_actor_pass(&mut world, 0);

    // After one actor pass, pose_frame should be populated.
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

    // pose_frame should remain None - inactive actors are skipped.
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

    tick_to_actor_pass(&mut world, 0);
    let f0 = world.actors[0].pose_frame.clone().unwrap();

    tick_to_actor_pass(&mut world, 0);
    let f1 = world.actors[0].pose_frame.clone().unwrap();

    // Each actor pass should advance the factor and produce different bone
    // outputs.
    assert_ne!(f0.factor, f1.factor, "factor should advance between passes");
}

#[test]
fn set_actor_animation_resets_pose_frame() {
    let mut world = World::new();
    world.actors[0].activate();
    let record = synth_anm_record(1);
    let player = AnimPlayer::new(record.clone(), 1).unwrap();
    world.set_actor_animation(0, player);
    tick_to_actor_pass(&mut world, 0);
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

    tick_to_actor_pass(&mut world, 0); // factor moves from 0 to 0x80
    let frame = world.actors[0].pose_frame.as_ref().unwrap();
    let (pos, _rot) = frame.bone_outputs[0];
    // factor=0x80 → lerp(0, 10, 0x80/0x100) ≈ 5
    assert!(
        pos[0] > 0,
        "interpolated pos[0] should be non-zero at midpoint factor"
    );
}

#[test]
fn posed_mesh_differs_across_bone_offsets() {
    let buf = synth_pyramid_tmd();
    let tmd = legaia_tmd::parse(&buf).unwrap();

    let mesh_zero = tmd_to_vram_mesh_posed(&tmd, &buf, &[([0i16; 3], [0i16; 3])]);
    let mesh_moved = tmd_to_vram_mesh_posed(&tmd, &buf, &[([50, 100, 150], [0i16; 3])]);

    assert_eq!(
        mesh_zero.positions.len(),
        mesh_moved.positions.len(),
        "vertex count must be stable across bone offsets"
    );
    assert!(!mesh_zero.positions.is_empty());
    assert!(
        mesh_zero
            .positions
            .iter()
            .zip(mesh_moved.positions.iter())
            .any(|(p0, p1)| p0 != p1),
        "vertex positions must differ when bone offset changes"
    );
}

#[test]
fn animation_player_pose_frame_drives_posed_mesh() {
    let buf = synth_pyramid_tmd();
    let tmd = legaia_tmd::parse(&buf).unwrap();

    // dst_pos[0]=10 for bone 0; at full factor the mesh shifts along x.
    let record = synth_anm_record(1);
    let mut world = World::new();
    world.actors[0].activate();
    world.set_actor_tmd_binding(0, 0);
    world.set_actor_animation(0, AnimPlayer::new(record, 1).unwrap());

    tick_to_actor_pass(&mut world, 0);
    let pose = world.actors[0].pose_frame.as_ref().unwrap();

    let mesh = tmd_to_vram_mesh_posed(&tmd, &buf, &pose.bone_outputs);
    assert!(!mesh.positions.is_empty(), "posed mesh must have vertices");
    assert_eq!(world.actors[0].tmd_binding, Some(0));
}
