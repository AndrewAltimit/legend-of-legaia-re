use super::*;

fn fresh() -> ActorPhysics {
    let mut p = ActorPhysics::default();
    p.set_dispatch(0x06);
    p
}

#[test]
fn tick_scalars_product_is_byte_multiply() {
    assert_eq!(
        TickScalars {
            frame_delta: 4,
            speed: 3
        }
        .product(),
        12
    );
    assert_eq!(TickScalars::idle().product(), 1);
}

#[test]
fn common_pre_update_drains_timer_and_advances_rotation() {
    let mut p = fresh();
    p.timer = 100;
    p.frame_pace = 50;
    p.rotation_accum = 0;
    common_pre_update(
        &mut p,
        TickScalars {
            frame_delta: 2,
            speed: 3,
        },
    );
    assert_eq!(p.timer, 94);
    assert_eq!(p.rotation_accum, 100);
}

#[test]
fn common_pre_update_saturates_negative_timer() {
    let mut p = fresh();
    p.timer = i16::MIN + 1;
    common_pre_update(
        &mut p,
        TickScalars {
            frame_delta: 100,
            speed: 100,
        },
    );
    assert_eq!(p.timer, i16::MIN);
}

#[test]
fn keyframe_accel_shifts_by_six_into_shake_lanes() {
    let mut p = fresh();
    p.kf_accel = [128, 64, 0, 0, 0];
    p.kf_shake = [10, 5, 0, 0];
    p.spline_z = 0;
    keyframe_accel_update(
        &mut p,
        TickScalars {
            frame_delta: 4,
            speed: 2,
        },
    );
    assert_eq!(p.kf_shake[0], 10 + ((128i64 * 8) >> 6) as i16);
    assert_eq!(p.kf_shake[1], 5 + ((64i64 * 8) >> 6) as i16);
}

#[test]
fn keyframe_accel_clamps_spline_z_non_negative() {
    let mut p = fresh();
    p.kf_accel = [0, 0, 0, 0, -1000];
    p.spline_z = 0;
    keyframe_accel_update(
        &mut p,
        TickScalars {
            frame_delta: 64,
            speed: 1,
        },
    );
    assert_eq!(p.spline_z, 0);
}

#[test]
fn path_alt_inactive_emitter_integrates_velocity() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.path_active = 0;
    p.path_vel = [0, 64, 32];
    p.path_pos = [10, 20, 0];
    let listener = ListenerState::unicast(0, 0, 0);
    let mut out = TickResult::default();
    path_alt_update(
        &mut p,
        TickScalars {
            frame_delta: 4,
            speed: 2,
        },
        &listener,
        &mut out,
    );
    assert_eq!(p.path_pos[0], 10 + ((64i64 * 8) >> 6) as i16);
    assert_eq!(p.path_pos[1], 20 + ((32i64 * 8) >> 6) as i16);
}

#[test]
fn path_alt_active_ramp_snaps_when_counter_drains() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.set_path_active(1);
    p.ramp_counter = 1;
    p.path_vel = [3, 30, 40];
    p.path_pos = [5, 10, 15];
    let listener = ListenerState::unicast(0, 0, 0);
    let mut out = TickResult::default();
    path_alt_update(
        &mut p,
        TickScalars {
            frame_delta: 4,
            speed: 1,
        },
        &listener,
        &mut out,
    );
    // Snap formula:
    //   path_pos[0] = path_pos[2] + path_vel[1] = 15 + 30 = 45
    //   path_pos[1] = path_vel[0]  + path_vel[2] = 3 + 40 = 43
    // (Both then clamped to 0..=0x7F, leaving them unchanged.)
    assert_eq!(p.path_pos[0], 45);
    assert_eq!(p.path_pos[1], 43);
    assert_eq!(p.path_active, 0);
    assert_eq!(p.path_vel[1], 0);
    assert_eq!(p.path_vel[2], 0);
}

#[test]
fn path_alt_clamps_to_midi_range() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.path_pos = [0x100, -10, 0];
    let listener = ListenerState::unicast(0, 0, 0);
    let mut out = TickResult::default();
    path_alt_update(&mut p, TickScalars::idle(), &listener, &mut out);
    assert_eq!(p.path_pos[0], 0x7F);
    assert_eq!(p.path_pos[1], 0);
}

#[test]
fn path_alt_releases_channel_on_out_of_range_when_active() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.release_pending = 1; // emitter previously keyed on
    p.path_state = 1;
    p.path_pos = [0x40, 0x40, 0];
    p.world_x = 1000;
    p.range_x_high = 100;
    p.range_z_high = 100;
    p.sfx_bank_index = 42;
    p.sfx_channel = 7;
    let listener = ListenerState::unicast(0, 0, 0x80);
    let mut out = TickResult::default();
    path_alt_update(&mut p, TickScalars::idle(), &listener, &mut out);
    assert!(matches!(
        out.events.first(),
        Some(TickEvent::SfxRelease {
            bank_index: 42,
            channel: 7
        })
    ));
    assert_eq!(p.release_pending, 0);
}

#[test]
fn path_alt_emits_key_on_when_inactive_and_in_range() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.release_pending = 0;
    p.path_pos = [0x40, 0x40, 0];
    p.range_x_high = 100;
    p.range_z_high = 100;
    p.sfx_bank_index = 1;
    p.sfx_channel = 0;
    p.sfx_bank_row = 3;
    let listener = ListenerState::unicast(0, 0, 0x100);
    let mut out = TickResult::default();
    path_alt_update(&mut p, TickScalars::idle(), &listener, &mut out);
    let key_ons = out
        .events
        .iter()
        .filter(|e| matches!(e, TickEvent::SfxUpdate { key_on: true, .. }))
        .count();
    assert_eq!(key_ons, 3);
    assert_eq!(p.release_pending, 1);
}

#[test]
fn path_alt_emits_volume_only_when_already_keyed_on() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.release_pending = 1;
    p.path_pos = [0x40, 0x40, 0];
    p.range_x_high = 100;
    p.range_z_high = 100;
    p.sfx_bank_row = 2;
    let listener = ListenerState::unicast(0, 0, 0x100);
    let mut out = TickResult::default();
    path_alt_update(&mut p, TickScalars::idle(), &listener, &mut out);
    let vol_only = out
        .events
        .iter()
        .filter(|e| matches!(e, TickEvent::SfxUpdate { key_on: false, .. }))
        .count();
    assert_eq!(vol_only, 2);
}

#[test]
fn path_alt_mute_and_release_path_takes_precedence() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.path_pos = [0x40, 0x40, 0];
    p.range_x_high = 100;
    p.range_z_high = 100;
    p.sfx_bank_index = 7;
    p.sfx_channel = 1;
    let mut listener = ListenerState::unicast(0, 0, 0x100);
    listener.mute_and_release = true;
    let mut out = TickResult::default();
    path_alt_update(&mut p, TickScalars::idle(), &listener, &mut out);
    assert_eq!(out.events.len(), 1);
    assert!(matches!(
        out.events[0],
        TickEvent::SfxRelease {
            bank_index: 7,
            channel: 1
        }
    ));
    assert_eq!(p.status_flags & 0x8, 0x8);
}

#[test]
fn path_alt_force_mono_averages_volumes() {
    let mut p = fresh();
    p.set_dispatch(0x05);
    p.path_pos = [0x40, 0x10, 0];
    p.range_x_high = 100;
    p.range_z_high = 100;
    p.sfx_bank_row = 1;
    let mut listener = ListenerState::unicast(0, 0, 0x100);
    listener.force_mono = true;
    let mut out = TickResult::default();
    path_alt_update(&mut p, TickScalars::idle(), &listener, &mut out);
    // Single key-on emitted; volume_left == volume_right.
    if let Some(TickEvent::SfxUpdate {
        volume_left,
        volume_right,
        ..
    }) = out
        .events
        .iter()
        .find(|e| matches!(e, TickEvent::SfxUpdate { .. }))
    {
        assert_eq!(volume_left, volume_right);
    } else {
        panic!("expected SfxUpdate event");
    }
}

#[test]
fn path_arm_integrates_three_axis_velocity() {
    let mut p = fresh();
    p.set_dispatch(0x03);
    p.path_vel = [10, 20, 30];
    p.zoom = 0;
    p.zoom_rate = 64;
    let cont = path_update(
        &mut p,
        TickScalars {
            frame_delta: 4,
            speed: 2,
        },
    );
    assert_eq!(p.path_pos[0], ((10i64 * 8) >> 6) as i16);
    assert_eq!(p.path_pos[1], ((20i64 * 8) >> 6) as i16);
    assert_eq!(p.path_pos[2], ((30i64 * 8) >> 6) as i16);
    assert_eq!(p.zoom, 8);
    assert!(!cont);
}

#[test]
fn path_arm_clamps_zoom_at_0x100() {
    let mut p = fresh();
    p.set_dispatch(0x03);
    p.zoom = 0xFF;
    p.zoom_rate = 64;
    let _ = path_update(
        &mut p,
        TickScalars {
            frame_delta: 16,
            speed: 16,
        },
    );
    assert_eq!(p.zoom, 0x100);
}

#[test]
fn path_arm_advances_state_machine_and_signals_continuation() {
    let mut p = fresh();
    p.set_dispatch(0x03);
    p.path_state = 5;
    let cont = path_update(&mut p, TickScalars::idle());
    assert!(cont);
    assert_eq!(p.path_state, 6);
}

#[test]
fn path_arm_caps_state_at_one_thousand() {
    let mut p = fresh();
    p.set_dispatch(0x03);
    p.path_state = 999;
    let _ = path_update(&mut p, TickScalars::idle());
    assert_eq!(p.path_state, 1000);
    let _ = path_update(&mut p, TickScalars::idle());
    assert_eq!(p.path_state, 1000);
}

#[test]
fn default_movement_clears_unlink_bit_when_visibility_flag_set() {
    let mut p = fresh();
    p.visibility_flags = 0x2000;
    p.status_flags = 0x2 | 0x10;
    default_movement_update(&mut p, TickScalars::idle());
    assert_eq!(p.status_flags, 0x10);
}

#[test]
fn default_movement_advances_motion_via_accel() {
    let mut p = fresh();
    p.accel = [128, 64, 32];
    default_movement_update(
        &mut p,
        TickScalars {
            frame_delta: 4,
            speed: 2,
        },
    );
    assert_eq!(p.motion_x, ((128i64 * 8) >> 6) as i16);
    assert_eq!(p.motion_y, ((64i64 * 8) >> 6) as i16);
    assert_eq!(p.motion_z, ((32i64 * 8) >> 6) as i16);
}

#[test]
fn apply_world_rotation_uses_supplied_lut() {
    let mut p = fresh();
    p.path_vel = [0x10, 100, 0];
    let sin = |_: u16| 4096_i32;
    let cos = |_: u16| -4096_i32;
    let prev_x = p.world_x;
    let prev_z = p.world_z;
    apply_world_rotation(&mut p, TickScalars::idle(), &sin, &cos);
    assert!(p.world_x.checked_sub(prev_x).is_some());
    assert!(p.world_z.checked_sub(prev_z).is_some());
    // dx and dz have opposite signs because sin and cos do.
    assert!(p.world_x >= 0);
    assert!(p.world_z <= 0);
}

#[test]
fn late_update_clamps_focal_envelope_at_0x1000() {
    let mut p = fresh();
    p.focal_envelope = 0x2000;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Keyframe), &mut out);
    assert_eq!(p.focal_envelope, 0x1000);
}

#[test]
fn late_update_resets_focal_above_16000() {
    let mut p = fresh();
    p.focal_envelope = 16500;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Keyframe), &mut out);
    assert_eq!(p.focal_envelope, 0);
}

#[test]
fn late_update_clamps_shake_at_15000() {
    let mut p = fresh();
    p.shake_envelope = 15500;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Keyframe), &mut out);
    assert_eq!(p.shake_envelope, 15000);
}

#[test]
fn late_update_emits_spline_draw_for_dispatch_seven() {
    let mut p = fresh();
    p.set_dispatch(0x07);
    p.world_x = 10;
    p.world_y = 20;
    p.world_z = 30;
    p.spline_halfwidth = 64;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Spline), &mut out);
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, TickEvent::SplineDraw { halfwidth: 64, .. }))
    );
}

#[test]
fn late_update_emits_damp_draw_for_dispatch_four() {
    let mut p = fresh();
    p.damp_ramp = 5;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Damp), &mut out);
    assert_eq!(p.damp_ramp, 4);
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, TickEvent::DampDraw { .. }))
    );
}

#[test]
fn late_update_resets_damp_ramp_when_negative() {
    let mut p = fresh();
    p.damp_ramp = 0;
    p.spline_halfwidth = 100;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Damp), &mut out);
    assert_eq!(p.damp_ramp, 100);
}

#[test]
fn late_update_emits_move_vm_kick_when_kick_set() {
    let mut p = fresh();
    p.move_vm_kick = 1;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Snap), &mut out);
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, TickEvent::MoveVmKick))
    );
}

#[test]
fn late_update_emits_unlink_request_when_visibility_bit_set() {
    let mut p = fresh();
    p.visibility_flags = 0x2000;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Snap), &mut out);
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, TickEvent::UnlinkRequest))
    );
}

#[test]
fn late_update_emits_keyframe_pose_when_record_ptr_present() {
    let mut p = fresh();
    p.set_record_ptr(0x80100000);
    p.set_bone_count(20);
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Keyframe), &mut out);
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, TickEvent::KeyframePoseWritten { bone_count: 20 }))
    );
}

#[test]
fn late_update_skips_keyframe_pose_without_record_ptr() {
    let mut p = fresh();
    p.set_bone_count(20);
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Keyframe), &mut out);
    assert!(
        !out.events
            .iter()
            .any(|e| matches!(e, TickEvent::KeyframePoseWritten { .. }))
    );
}

#[test]
fn late_update_signals_kill_on_negative_timer_with_kill_bit() {
    let mut p = fresh();
    p.timer = -1;
    p.status_flags = 0x8;
    let mut out = TickResult::default();
    common_late_update(&mut p, Some(DispatchByte::Keyframe), &mut out);
    assert!(out.kill_requested);
}

#[test]
fn tick_actor_runs_full_pipeline_for_keyframe() {
    let mut p = ActorPhysics::default();
    p.set_dispatch(0x06);
    p.timer = 10;
    p.kf_accel = [64, 0, 0, 0, 0];
    p.set_record_ptr(0x80100000);
    p.set_bone_count(8);
    let listener = ListenerState::unicast(0, 0, 0);
    let res = tick_actor(
        &mut p,
        TickScalars {
            frame_delta: 4,
            speed: 1,
        },
        &listener,
    );
    assert_eq!(p.timer, 6);
    assert_eq!(p.kf_shake[0], 4); // 64*4>>6 = 4
    assert!(
        res.events
            .iter()
            .any(|e| matches!(e, TickEvent::KeyframePoseWritten { bone_count: 8 }))
    );
}

#[test]
fn tick_actor_for_path_alt_skips_default_movement() {
    let mut p = ActorPhysics::default();
    p.set_dispatch(0x05);
    p.accel = [128, 0, 0];
    let listener = ListenerState::unicast(0, 0, 0);
    let _ = tick_actor(&mut p, TickScalars::idle(), &listener);
    assert_eq!(p.motion_x, 0);
}

#[test]
fn tick_actor_for_unknown_dispatch_runs_pre_late_and_default() {
    let mut p = ActorPhysics::default();
    p.set_dispatch(0xFE);
    p.timer = 100;
    p.frame_pace = 10;
    p.accel = [200, 0, 0];
    let listener = ListenerState::unicast(0, 0, 0);
    let _ = tick_actor(&mut p, TickScalars::idle(), &listener);
    assert_eq!(p.timer, 99);
    assert_eq!(p.rotation_accum, 10);
    assert_ne!(p.motion_x, 0);
}

#[test]
fn tick_actor_kill_request_propagates_through_status_flag() {
    let mut p = ActorPhysics::default();
    p.set_dispatch(0x06);
    p.timer = -1;
    p.status_flags = 0x8;
    let listener = ListenerState::unicast(0, 0, 0);
    let res = tick_actor(&mut p, TickScalars::idle(), &listener);
    assert!(res.kill_requested);
}

#[test]
fn set_path_active_keeps_kf_shake_lane2_in_sync() {
    let mut p = ActorPhysics::default();
    p.set_path_active(0x1234);
    assert_eq!(p.path_active, 0x1234);
    assert_eq!(p.kf_shake[2], 0x1234);
}

#[test]
fn set_kf_shake_lane2_keeps_path_active_in_sync() {
    let mut p = ActorPhysics::default();
    p.set_kf_shake_lane2(-5);
    assert_eq!(p.kf_shake[2], -5);
    assert_eq!(p.path_active, -5);
}

#[test]
fn at_origin_initialises_world_position_only() {
    let p = ActorPhysics::at_origin(100, 200);
    assert_eq!(p.world_x, 100);
    assert_eq!(p.world_z, 200);
    assert_eq!(p.world_y, 0);
    assert_eq!(p.timer, 0);
}
