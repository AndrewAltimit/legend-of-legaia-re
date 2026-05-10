//! Per-actor physics tick - clean-room port of `FUN_80021DF4`.
//!
//! `FUN_8002519C` walks the per-frame actor list and calls this function on
//! every active record. The dispatcher is **not** an "animation interpreter"
//! the way [`legaia_anm::AnimPlayer`] is - it advances per-actor position /
//! velocity / acceleration state, emits a positional sound cue when the
//! dispatch byte selects the SFX-emitter variant, and (only for the keyframe
//! dispatch byte) writes the interpolated pose into the renderer's output
//! buffer at `actor[+0x4C]`.
//!
//! ## Dispatch ladder
//!
//! The dispatch byte at `actor[+0x5A]` (see [`anim_vm::DispatchByte`]) selects
//! a layered set of side-effects:
//!
//! 1. **Common pre-update** - runs unconditionally. Drains the per-frame
//!    timer at `+0x54` and the rotation accumulator at `+0x22`.
//! 2. **Keyframe accel** (dispatch `2` / `6`). Adds `+0xC0..+0xCA` * scalars
//!    >> 6 into the shake envelopes at `+0xB4..+0xC8`.
//! 3. **Positional SFX emitter** (dispatch `5`). Either ramps a fade between
//!    `(+0x90, +0x92)` and `(+0x94 + +0x98, +0x96 + +0x9A)` over `+0xBC`
//!    frames, or simply integrates `+0x98 / +0x9A` into `+0x90 / +0x92`.
//!    Audio cues surface as [`TickEvent`] entries.
//! 4. **Path interpolation** (dispatch `3`). Adds `+0x96 / +0x98 / +0x9A`
//!    velocities into `+0x90 / +0x92 / +0x94`, advances the zoom envelope at
//!    `+0x68` (clamped at `0x100`).
//! 5. **Default movement** (every dispatch byte except `5`). Adds
//!    `+0x80..+0x84` into `+0x24..+0x28`, runs the trig-LUT-driven
//!    world-position update via [`apply_world_rotation`], and accumulates
//!    the camera-shake envelopes at `+0x72 / +0x78 / +0x7A`.
//! 6. **Common late-update** - caps the envelopes, kicks the move VM,
//!    fires the per-arm render event (line draws for `4`, scene-graph
//!    triangle for `7`), and for dispatch `6` writes the keyframe pose.
//!
//! ## What this port covers vs what it doesn't
//!
//! - **Covered.** The dispatch ladder, the per-arm position / velocity /
//!   acceleration math (with field offsets matching retail), and host
//!   callbacks for cross-cutting effects (audio cues, render submissions,
//!   move-VM kicks). Tests verify the arithmetic shape against the
//!   decompiled C reference for each arm.
//! - **Out of scope.** Bit-exact MIPS-cycle behaviour (the retail dispatcher
//!   leans on the `1F800380`-region scratchpad register file for nearly
//!   every multiply-add - we use straight i64 multiplication and a single
//!   round-down shift, which matches the source's `>> 6` / `>> 18` when
//!   neither operand is `i32::MIN`). The trig-LUT contents are supplied by
//!   the caller via [`apply_world_rotation`].
//!
//! The dispatcher reads many of these fields at different widths from
//! different arms (e.g. `+0xB8` is read as `i32` by the SFX-emitter arm and
//! as `i16` by the keyframe arm). Both views are kept in sync via the
//! `path_active` (i32) and `kf_shake[2]` (i16) fields - touching either
//! field via the public API keeps the other in lockstep.

use crate::anim_vm::DispatchByte;

/// Global tick scalars.
///
/// The retail dispatcher reads `DAT_1F800393` (per-frame delta multiplier)
/// and `DAT_1F80037D` (game-speed multiplier). Both are unsigned bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickScalars {
    pub frame_delta: u8,
    pub speed: u8,
}

impl TickScalars {
    /// Idle-frame scalars (`frame_delta = 1`, `speed = 1`). Most of the
    /// dispatcher arithmetic devolves to a straight add at this rate.
    pub const fn idle() -> Self {
        Self {
            frame_delta: 1,
            speed: 1,
        }
    }

    /// `frame_delta * speed` - the multiplier the dispatcher applies in
    /// nearly every arm.
    pub fn product(self) -> u32 {
        u32::from(self.frame_delta) * u32::from(self.speed)
    }
}

/// World-space listener used by the positional SFX emitter (dispatch `5`).
///
/// Mirrors `_DAT_80089118` / `_DAT_80089120` (listener position),
/// `_DAT_8007BABC` / `_DAT_8007BAA0` (channel authority), `_DAT_8007BF40`
/// (master volume), `_DAT_800846BC` (mono fold), `_DAT_8007B83C` (SFX state),
/// and `_DAT_8007B9EC` (mute-and-release flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListenerState {
    pub x: i32,
    pub z: i32,
    pub current_channel: u32,
    pub assigned_channel: u32,
    pub master_volume: u32,
    pub force_mono: bool,
    pub sfx_state: u8,
    pub mute_and_release: bool,
}

impl ListenerState {
    /// Build a listener that always passes the channel-authority check.
    pub const fn unicast(x: i32, z: i32, master_volume: u32) -> Self {
        Self {
            x,
            z,
            current_channel: 1,
            assigned_channel: 1,
            master_volume,
            force_mono: false,
            sfx_state: 3,
            mute_and_release: false,
        }
    }
}

/// Per-actor record fields read / written by the tick. Field offsets and
/// access widths match the retail layout (annotated inline). The struct is
/// `Default + Copy` so tests can build instances directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorPhysics {
    /// `+0x10` - actor status flags. Bits the dispatcher tests:
    /// - `0x00008` - kill-on-next-tick.
    /// - `0x00002` - needs un-link from sprite list.
    /// - `0x10000` - emitter "stop" request (PathAlt arm clears it).
    pub status_flags: u32,
    /// `+0x14 / +0x16 / +0x18` - world-space position.
    pub world_x: i16,
    pub world_y: i16,
    pub world_z: i16,
    /// `+0x22` - rotation accumulator.
    pub rotation_accum: u16,
    /// `+0x24..+0x28` - motion accumulators.
    pub motion_x: i16,
    pub motion_y: i16,
    pub motion_z: i16,
    /// `+0x2A` - secondary spin accumulator.
    pub spin_a: i16,
    /// `+0x3C / +0x3E / +0x40` - per-axis world-space rotation factors
    /// folded into world position via the trig LUTs.
    pub rot_factor_x: i16,
    pub rot_factor_y: i16,
    pub rot_factor_z: i16,
    /// `+0x4C` - keyframe-record output pointer. Non-zero = the keyframe
    /// arm should write a pose. Used as a presence gate, not dereferenced.
    pub record_ptr: usize,
    /// `+0x52` - render flags (bit `0x400` enables the spline arm's extra
    /// render call).
    pub render_flags: u16,
    /// `+0x54` - countdown timer. Common pre-update drains it.
    pub timer: i16,
    /// `+0x56` - non-zero kicks the move VM (`FUN_800204F8`).
    pub move_vm_kick: i16,
    /// `+0x5A` - dispatch byte the tick reads at the start.
    pub dispatch_byte: u16,
    /// `+0x68 / +0x6A` - zoom envelope (clamped at `0x100`).
    pub zoom: i16,
    pub zoom_rate: i16,
    /// `+0x72` - camera-shake envelope (clamped to `0..=15000`).
    pub shake_envelope: i16,
    /// `+0x78 / +0x7A` - secondary shake / focal envelopes.
    pub focal_envelope: i16,
    pub anim_z_bias: i16,
    /// `+0x80..+0x84` - per-axis acceleration vector.
    pub accel: [i16; 3],
    /// `+0x86` - visibility flags. Bit `0x2000` triggers the un-link helper.
    pub visibility_flags: u16,
    /// `+0x90 / +0x92 / +0x94` - emitter / path target tuple.
    pub path_pos: [i16; 3],
    /// `+0x96 / +0x98 / +0x9A` - emitter / path velocity tuple.
    pub path_vel: [i16; 3],
    /// `+0x9C` - path step counter / state machine register.
    pub path_state: i32,
    /// `+0xA0 / +0xA4 / +0xA8` - listener-distance bound checks.
    pub range_z_low: i32,
    pub range_x_high: i32,
    pub range_z_high: i32,
    /// `+0xAC` - SFX bank index.
    pub sfx_bank_index: i32,
    /// `+0xB0 / +0xB2` - SsAPI channel + bank-row index.
    pub sfx_channel: i16,
    pub sfx_bank_row: i16,
    /// `+0xB4..+0xBA` - keyframe shake envelopes (4 lanes of i16). Only
    /// the keyframe arms (dispatch `2` / `6`) read these; the SFX emitter
    /// arm aliases the same bytes via `release_pending` (`+0xB4` as i32)
    /// and `path_active` (`+0xB8` as i32).
    pub kf_shake: [i16; 4],
    /// `+0xB4` (i32 view) - SFX emitter "key-on done, release pending"
    /// flag. Set to `1` when the emitter has issued a key-on through the
    /// SsAPI; cleared to `0` when the channel is released. Aliases
    /// `kf_shake[0..2]` in the retail layout.
    pub release_pending: i32,
    /// `+0xB8` (i32 view) - SFX emitter ramp-active flag (PathAlt arm
    /// only). Aliases `kf_shake[2..4]`. Use [`set_path_active`] /
    /// [`set_kf_shake_lane2`] to keep both views in sync.
    pub path_active: i32,
    /// `+0xBC` - PathAlt ramp counter.
    pub ramp_counter: i32,
    /// `+0xC0..+0xCA` - keyframe accelerator vector (5 lanes of i16).
    pub kf_accel: [i16; 5],
    /// `+0xC0` (i32 view) - same bytes as `kf_accel[0..2]`. The PathAlt
    /// arm reads it as the ramp duration.
    pub ramp_duration: i32,
    /// `+0xC4..+0xCC` - spline-arm draw bbox.
    pub spline_halfwidth: i16,
    pub spline_step1: i16,
    pub spline_step2: i16,
    pub spline_z: i16,
    pub spline_step_x: i16,
    pub spline_step_y: i16,
    pub spline_step_z: i16,
    pub spline_step_w: i16,
    /// `+0xC6` - damp arm ramp counter.
    pub damp_ramp: i16,
    /// `+0xD0` - frame-pace accelerator (common pre-update reads this).
    pub frame_pace: i16,
    /// Bone count for the keyframe pose write (retail reads
    /// `**actor[+0x44]`; engines populate this directly).
    pub bone_count: u16,
}

impl ActorPhysics {
    /// New actor at world `(x, _, z)` with no pending motion / shake.
    pub fn at_origin(x: i16, z: i16) -> Self {
        Self {
            world_x: x,
            world_z: z,
            ..Self::default()
        }
    }

    /// Set the dispatch byte the tick reads on next entry.
    pub fn set_dispatch(&mut self, b: u16) {
        self.dispatch_byte = b;
    }

    /// Read the active dispatch byte.
    pub fn dispatch(&self) -> u16 {
        self.dispatch_byte
    }

    /// Update the i32 view of the path-active flag (`+0xB8` as int) and the
    /// i16 view (`kf_shake[2]`) atomically. Use this whenever code needs
    /// both views in sync.
    pub fn set_path_active(&mut self, active: i32) {
        self.path_active = active;
        // Mirror the low half into the kf_shake view.
        self.kf_shake[2] = (active & 0xFFFF) as i16;
    }

    /// Update the keyframe-shake i16 view of `+0xB8` (`kf_shake[2]`) and
    /// keep the path-active i32 view in sync.
    pub fn set_kf_shake_lane2(&mut self, v: i16) {
        self.kf_shake[2] = v;
        // Sign-extend into the i32 view.
        self.path_active = i32::from(v);
    }

    /// Set the ramp duration `+0xC0` and keep the i16 view (`kf_accel[0]`)
    /// in sync with the low half.
    pub fn set_ramp_duration(&mut self, dur: i32) {
        self.ramp_duration = dur;
        self.kf_accel[0] = (dur & 0xFFFF) as i16;
    }

    /// Engine-side hook: opt this actor into the keyframe pose write.
    pub fn set_record_ptr(&mut self, ptr: usize) {
        self.record_ptr = ptr;
    }

    /// Engine-side hook: how many bones the renderer should pull on the
    /// next keyframe pose write.
    pub fn set_bone_count(&mut self, n: u16) {
        self.bone_count = n;
    }
}

/// Cross-cutting events emitted by the tick. Engines fold them into their
/// host runtime (audio mixer, scene graph, move-VM driver).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickEvent {
    /// Move VM (`FUN_800204F8`) should be invoked for this actor.
    MoveVmKick,
    /// Visibility-flag bit `0x2000` is set; the un-link helper at
    /// `FUN_801D79E8` should fire.
    UnlinkRequest,
    /// SFX emitter wants its volume/pan pair sent to the audio mixer.
    SfxUpdate {
        bank_index: i32,
        channel_base: i16,
        slot: u8,
        volume_left: u32,
        volume_right: u32,
        /// `true` for "key on" (per-slot `FUN_80065034`); `false` for
        /// "volume update only" (`FUN_800657D0`).
        key_on: bool,
    },
    /// SFX emitter wants its channel released (`FUN_800250D4`).
    SfxRelease { bank_index: i32, channel: i16 },
    /// Spline arm wants its scene-graph triangle drawn.
    SplineDraw {
        center: [i16; 3],
        halfwidth: i16,
        step: [i16; 4],
        zoom_shift: u8,
    },
    /// Damp arm wants its dampened bounding-box drawn.
    DampDraw {
        ramp_counter: i16,
        bbox_origin: [i16; 4],
    },
    /// Keyframe arm produced a fresh pose (writeback to `actor[+0x4C]`).
    KeyframePoseWritten { bone_count: u16 },
}

/// Outcome of one tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickResult {
    pub events: Vec<TickEvent>,
    /// `true` when the dispatcher saw `actor[+0x10] & 8` during the tick.
    pub kill_requested: bool,
}

impl TickResult {
    fn push(&mut self, e: TickEvent) {
        self.events.push(e);
    }
}

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

    let path_continued = if matches!(dispatch, Some(DispatchByte::Path)) {
        path_update(physics, scalars)
    } else {
        false
    };

    if !matches!(dispatch, Some(DispatchByte::PathAlt)) && !path_continued {
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

/// Default-movement arm - dispatch byte ≠ `5`. Folds `accel` * scalar into
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

#[cfg(test)]
mod tests {
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
}
