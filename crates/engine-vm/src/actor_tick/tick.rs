//! `tick_actor` and the update stages it runs: pre-update, keyframe acceleration, the two path updates, default movement, world rotation and the late update.

use super::*;

/// Run one frame of the per-actor physics tick.
pub fn tick_actor(
    physics: &mut ActorPhysics,
    scalars: TickScalars,
    listener: &ListenerState,
) -> TickResult {
    let mut out = TickResult::default();
    let dispatch = DispatchByte::from_byte(physics.dispatch_byte);

    common_pre_update(physics, scalars);

    if matches!(
        dispatch,
        Some(DispatchByte::KeyframeAlt) | Some(DispatchByte::Keyframe)
    ) {
        keyframe_accel_update(physics, scalars);
    }

    if matches!(dispatch, Some(DispatchByte::PathAlt)) {
        path_alt_update(physics, scalars, listener, &mut out);
    }

    if matches!(dispatch, Some(DispatchByte::Path)) {
        path_update(physics, scalars);
    }

    // Retail branches past the default motion block for dispatch 3 and 5
    // alike (`beq` to `0x80022B80` at `0x800228A8` / `0x800228B0`); the
    // `+0x9C` counter `path_update` advances gates nothing here.
    if !matches!(
        dispatch,
        Some(DispatchByte::Path) | Some(DispatchByte::PathAlt)
    ) {
        default_movement_update(physics, scalars);
    }

    common_late_update(physics, dispatch, &mut out);

    out
}

/// Common pre-update - drains the per-frame timer at `+0x54` and the
/// rotation accumulator at `+0x22`. Runs unconditionally.
pub fn common_pre_update(p: &mut ActorPhysics, s: TickScalars) {
    let dec = (i32::from(s.frame_delta) * i32::from(s.speed)) as i16;
    p.timer = p.timer.saturating_sub(dec);
    let bump = (i32::from(p.frame_pace) * i32::from(s.frame_delta)) as i16;
    p.rotation_accum = p.rotation_accum.wrapping_add(bump as u16);
}

/// Keyframe-acceleration update - dispatch `2` / `6`.
pub fn keyframe_accel_update(p: &mut ActorPhysics, s: TickScalars) {
    let prod = i64::from(s.product());
    for lane in 0..4 {
        let term = ((i64::from(p.kf_accel[lane])) * prod) >> 6;
        p.kf_shake[lane] = p.kf_shake[lane].saturating_add(term as i16);
    }
    let term4 = ((i64::from(p.kf_accel[4])) * prod) >> 6;
    p.spline_z = p.spline_z.saturating_add(term4 as i16);
    if p.spline_z < 0 {
        p.spline_z = 0;
    }
}

/// Positional SFX emitter - dispatch `5`.
pub fn path_alt_update(
    p: &mut ActorPhysics,
    s: TickScalars,
    listener: &ListenerState,
    out: &mut TickResult,
) {
    let prod = i64::from(s.product());

    if p.path_active == 0 {
        // Inactive emitter: integrate velocity into pan/volume registers.
        let dx = ((i64::from(p.path_vel[1])) * prod) >> 6;
        let dz = ((i64::from(p.path_vel[2])) * prod) >> 6;
        p.path_pos[0] = p.path_pos[0].saturating_add(dx as i16);
        p.path_pos[1] = p.path_pos[1].saturating_add(dz as i16);
    } else {
        p.ramp_counter -= i32::from(s.frame_delta);
        if p.ramp_counter < 0 {
            // Snap-to-final. Retail formula:
            //   path_pos[0] (+0x90) = path_pos[2] (+0x94) + path_vel[1] (+0x98)
            //   path_pos[1] (+0x92) = path_vel[0]  (+0x96) + path_vel[2] (+0x9A)
            let v98 = p.path_vel[1];
            let v9a = p.path_vel[2];
            let v94 = p.path_pos[2];
            let v96 = p.path_vel[0];
            p.ramp_counter = 0;
            p.set_path_active(0);
            p.path_vel[1] = 0;
            p.path_vel[2] = 0;
            p.path_pos[0] = v94.saturating_add(v98);
            p.path_pos[1] = v96.saturating_add(v9a);
        } else if p.ramp_duration > 0 {
            let ratio = ((p.ramp_duration - p.ramp_counter) * 0x100) / p.ramp_duration;
            let mut iv = i32::from(p.path_vel[1]) * ratio;
            if iv < 0 {
                iv += 0xFF;
            }
            let mut iz = i32::from(p.path_vel[2]) * ratio;
            if iz < 0 {
                iz += 0xFF;
            }
            // Same target field offsets as the snap branch.
            p.path_pos[0] = p.path_pos[2].saturating_add((iv as u32 >> 8) as i16);
            p.path_pos[1] = p.path_vel[0].saturating_add((iz as u32 >> 8) as i16);
        }
    }

    // Clamp pan / volume to MIDI 0..=0x7F.
    if p.path_pos[1] < 0 {
        p.path_pos[1] = 0;
    }
    if p.path_pos[0] < 0 {
        p.path_pos[0] = 0;
    }
    if p.path_pos[1] > 0x7F {
        p.path_pos[1] = 0x7F;
    }
    if p.path_pos[0] > 0x7F {
        p.path_pos[0] = 0x7F;
    }

    let dx = i32::from(p.world_x) - listener.x;
    let dz = i32::from(p.world_z) - listener.z;
    let in_range = (-p.range_x_high) < dx
        && dx < p.range_x_high
        && (-p.range_z_high) < dz
        && dz < p.range_z_high;
    let zero_state = p.path_state == 0;
    let same_authority = listener.current_channel == listener.assigned_channel;

    if !in_range && !zero_state {
        if p.release_pending != 0 {
            p.release_pending = 0;
            out.push(TickEvent::SfxRelease {
                bank_index: p.sfx_bank_index,
                channel: p.sfx_channel,
            });
        }
        return;
    }

    if !same_authority || (zero_state && !in_range) {
        return;
    }

    let falloff_x = if p.range_x_high == 0 {
        0x80
    } else {
        let abs_high = p.range_x_high.abs();
        let abs_d = dx.abs();
        let delta = (abs_high - abs_d).abs();
        (delta << 7) / abs_high
    };
    let falloff_z = if p.range_z_high == 0 {
        0x80
    } else {
        let abs_high = p.range_z_high.abs();
        let abs_d = dz.abs();
        let delta = (abs_high - abs_d).abs();
        (delta << 7) / abs_high
    };
    let mut combined = (falloff_x * falloff_z) >> 7;
    combined = combined.clamp(0, 0x100);

    let master = (i64::from(listener.master_volume) >> 1).max(0);
    let pan_l = i64::from(p.path_pos[0]);
    let pan_r = i64::from(p.path_pos[1]);
    let mut vol_l = ((master * pan_l * i64::from(combined)) >> 7) >> 7;
    let mut vol_r = ((master * pan_r * i64::from(combined)) >> 7) >> 7;
    if vol_l > 0x3F80 {
        vol_l = 0x3F80;
    }
    if vol_r > 0x3F80 {
        vol_r = 0x3F80;
    }
    if listener.force_mono {
        let mid = (vol_l + vol_r) >> 1;
        vol_l = mid;
        vol_r = mid;
    }

    if listener.mute_and_release {
        if p.release_pending != 0 {
            p.release_pending = 0;
        }
        out.push(TickEvent::SfxRelease {
            bank_index: p.sfx_bank_index,
            channel: p.sfx_channel,
        });
        p.status_flags |= 0x8;
        return;
    }

    if listener.sfx_state == 3 {
        let slot_count = (p.sfx_bank_row as u32) & 0x1F;
        if p.release_pending == 0 && p.sfx_channel >= 0 {
            p.release_pending = 1;
            for slot in 0..slot_count {
                out.push(TickEvent::SfxUpdate {
                    bank_index: p.sfx_bank_index,
                    channel_base: p.sfx_channel,
                    slot: slot as u8,
                    volume_left: vol_l as u32,
                    volume_right: vol_r as u32,
                    key_on: true,
                });
            }
        } else if p.release_pending != 0 {
            for slot in 0..slot_count {
                out.push(TickEvent::SfxUpdate {
                    bank_index: p.sfx_bank_index,
                    channel_base: p.sfx_channel,
                    slot: slot as u8,
                    volume_left: vol_l as u32,
                    volume_right: vol_r as u32,
                    key_on: false,
                });
            }
        }
    }
}

/// Path arm - dispatch `3`. Adds three-axis velocity into `+0x90..+0x94`,
/// advances `+0x68` (zoom) with clamp at `0x100`, and increments the path
/// state machine at `+0x9C`.
///
/// Returns `true` when the inner sub-state took the b80 shortcut (the
/// caller should then skip the default-movement arm).
pub fn path_update(p: &mut ActorPhysics, s: TickScalars) -> bool {
    let prod = i64::from(s.product());
    let dx = ((i64::from(p.path_vel[0])) * prod) >> 6;
    let dy = ((i64::from(p.path_vel[1])) * prod) >> 6;
    let dz = ((i64::from(p.path_vel[2])) * prod) >> 6;
    p.path_pos[0] = p.path_pos[0].saturating_add(dx as i16);
    p.path_pos[1] = p.path_pos[1].saturating_add(dy as i16);
    p.path_pos[2] = p.path_pos[2].saturating_add(dz as i16);

    let zoom_step = ((i64::from(p.zoom_rate)) * prod) >> 6;
    let new_zoom = i32::from(p.zoom).saturating_add(zoom_step as i32);
    p.zoom = new_zoom.clamp(i32::from(i16::MIN), 0x100) as i16;

    if p.path_state != 0 {
        let next = (p.path_state + 1).min(1000);
        p.path_state = next;
        true
    } else {
        false
    }
}

/// Default-movement arm - dispatch byte other than `3` / `5`. Folds `accel` * scalar into
/// `motion_x..motion_z`, runs the rotation step, and accumulates the
/// shake / focal envelopes.
pub fn default_movement_update(p: &mut ActorPhysics, s: TickScalars) {
    let prod = i64::from(s.product());

    let r = ((i64::from(p.path_vel[2])) * prod) >> 6;
    p.path_vel[0] = p.path_vel[0].saturating_add(r as i16);

    if (p.visibility_flags & 0x2000) != 0 {
        p.status_flags &= !0x2u32;
    }

    p.motion_x = p
        .motion_x
        .saturating_add((((i64::from(p.accel[0])) * prod) >> 6) as i16);
    p.motion_y = p
        .motion_y
        .saturating_add((((i64::from(p.accel[1])) * prod) >> 6) as i16);
    p.motion_z = p
        .motion_z
        .saturating_add((((i64::from(p.accel[2])) * prod) >> 6) as i16);

    p.rot_factor_y = p
        .rot_factor_y
        .saturating_add((((i64::from(p.rot_factor_z)) * prod) >> 6) as i16);

    p.shake_envelope = p
        .shake_envelope
        .saturating_add((((i64::from(p.path_pos[1])) * prod) >> 6) as i16);
    p.anim_z_bias = p
        .anim_z_bias
        .saturating_add((((i64::from(p.path_pos[2])) * prod) >> 6) as i16);
    p.focal_envelope = p
        .focal_envelope
        .saturating_add((((i64::from(p.path_pos[0])) * prod) >> 6) as i16);
}

/// Apply the world-rotation step using engine-supplied trig LUTs. The retail
/// formula:
///
/// ```text
/// world_x += (sin_lut[(rot_y & 0xFFF)] * path_vel[1]
///             + rot_factor_x * 0x1000) * frame_delta * speed >> 18
/// world_z += (cos_lut[(rot_y & 0xFFF)] * path_vel[1]
///             + rot_factor_z * 0x1000) * frame_delta * speed >> 18
/// ```
///
/// where `rot_y` is `+0x96`. Engines call this after
/// [`default_movement_update`] once they've stood up their LUTs.
pub fn apply_world_rotation(
    p: &mut ActorPhysics,
    s: TickScalars,
    sin_lut: &dyn Fn(u16) -> i32,
    cos_lut: &dyn Fn(u16) -> i32,
) {
    let idx = (p.path_vel[0] as u16) & 0x0FFF;
    let prod = i64::from(s.product());
    let sin_v = i64::from(sin_lut(idx));
    let cos_v = i64::from(cos_lut(idx));
    let pv = i64::from(p.path_vel[1]);
    let dx = ((sin_v * pv + i64::from(p.rot_factor_x) * 0x1000) * prod) >> 18;
    let dz = ((cos_v * pv + i64::from(p.rot_factor_z) * 0x1000) * prod) >> 18;
    p.world_x = p.world_x.saturating_add(dx as i16);
    p.world_z = p.world_z.saturating_add(dz as i16);
}

/// Common late-update - every dispatch byte. Caps the envelopes, emits the
/// per-arm render event, optionally fires the move-VM kick, and (only for
/// dispatch `6` with a present record pointer) emits the keyframe pose.
pub fn common_late_update(
    p: &mut ActorPhysics,
    dispatch: Option<DispatchByte>,
    out: &mut TickResult,
) {
    if p.anim_z_bias < 0 {
        p.anim_z_bias = 0;
    }

    // Retail (`0x80022B94..0x80022BB8`): `+0x54 < 0` first steps the
    // move-table VM `FUN_80023070` on this actor (`jal` at `0x80022BA4`),
    // and only then leaves early when `+0x10 & 8`. The engine runs that step
    // as its own pass rather than from inside this function: the move-VM
    // half of `+0x54` is `ActorState::wait_timer`, decremented and gated by
    // `crate::move_vm::decrement_wait_timer` + `crate::move_vm::actor_tick`
    // (the same `bgez` gate and post-call `& 8` test), which
    // `World::tick_move_vms_with_delta` in engine-core runs for every live
    // actor with bytecode, and which the summon-part and ambient-effect
    // drivers call directly. This function only reports the post-VM flag test
    // as `kill_requested`.
    // REF: FUN_80023070
    if p.timer < 0 {
        out.kill_requested = true;
        if (p.status_flags & 0x8) != 0 {
            return;
        }
    }

    let focal_u = p.focal_envelope as u16;
    if focal_u > 16000 {
        p.focal_envelope = 0;
    } else if focal_u > 0x1000 {
        p.focal_envelope = 0x1000;
    }

    let shake_u = p.shake_envelope as u16;
    if shake_u > 16000 {
        p.shake_envelope = 0;
    } else if shake_u > 15000 {
        p.shake_envelope = 15000;
    }

    if matches!(dispatch, Some(DispatchByte::Spline)) {
        out.push(TickEvent::SplineDraw {
            center: [p.world_x, p.world_y, p.world_z],
            halfwidth: p.spline_halfwidth,
            step: [
                p.spline_step_x,
                p.spline_step_y,
                p.spline_step_z,
                p.spline_step_w,
            ],
            zoom_shift: 2,
        });
    }

    if matches!(dispatch, Some(DispatchByte::Damp)) {
        p.damp_ramp = p.damp_ramp.saturating_sub(1);
        if p.damp_ramp < 0 {
            p.damp_ramp = p.spline_halfwidth;
        }
        out.push(TickEvent::DampDraw {
            ramp_counter: p.damp_ramp,
            bbox_origin: [
                p.spline_step1,
                p.spline_step2,
                p.spline_step_x,
                p.spline_step_y,
            ],
        });
    }

    if p.move_vm_kick != 0 {
        out.push(TickEvent::MoveVmKick);
    }

    if (p.visibility_flags & 0x2000) != 0 {
        out.push(TickEvent::UnlinkRequest);
    }

    if matches!(dispatch, Some(DispatchByte::Keyframe)) && p.record_ptr != 0 {
        out.push(TickEvent::KeyframePoseWritten {
            bone_count: p.bone_count,
        });
    }
}
