//! The per-actor physics record, the listener state and the tick's events and result.

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
    /// - `0x00002` - outside the camera's visible tile window (set / cleared
    ///   by the field cull `FUN_801D79E8`; not an un-link request).
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
    /// `+0x56` - non-zero calls the clip selector / cursor clock
    /// `FUN_800204F8` (`jal` at `0x80022EF4`; ported as
    /// `move_buffer::cursor_advance`). That is not the move-table VM
    /// `FUN_80023070`, which the tick reaches on the `+0x54 < 0` path.
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
    /// `+0x56 != 0`: the clip selector / cursor clock `FUN_800204F8` should
    /// run for this actor (`0x80022EF4`). The name predates that reading -
    /// this is not the move-table VM `FUN_80023070`.
    MoveVmKick,
    /// Visibility-flag bit `0x2000` is set; the field cull `FUN_801D79E8`
    /// should run. That routine is not an un-link helper: it compares the
    /// actor's tile against the camera's visible window (`0x1F800384..87`
    /// and `0x1F8003E8..EB`, widened by `+0x58`), sets `+0x10 |= 2` when
    /// outside (`0x801D7B10`), and otherwise clears the bit and, when
    /// `+0x52 & 0x40`, snaps `+0x16` to the floor through `FUN_80019278`.
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
    pub(super) fn push(&mut self, e: TickEvent) {
        self.events.push(e);
    }
}
