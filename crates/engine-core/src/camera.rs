//! Per-scene camera controller.
//!
//! Consumes the field-VM op-`0x45` event stream (Configure / Save / Load /
//! Apply - see [`crate::field_events::FieldEvent`]) and projects a target
//! actor's world position into a screen-space view. Engines plug the result
//! into [`legaia_engine_render`] each frame.
//!
//! Two layers:
//!
//! - [`CameraState`] (in [`crate::world`]) - the raw scratch the field VM
//!   reads / writes. Holds the most recent op-`0x45` payloads.
//! - [`Camera`] (here) - the *runtime* camera. Reads `CameraState`, layers
//!   in a follow target, and exposes a `(eye, look_at)` pair plus a yaw /
//!   pitch the renderer can use to build a view matrix.
//!
//! The retail engine does the per-frame math via the third motion VM
//! ([`legaia_engine_vm::motion_vm`]) and the move-VM ext sub-ops 0x06 / 0x36
//! / 0x39. This module assembles those primitives into a single Camera
//! that's easy to drive from [`crate::scene::SceneHost`].

use crate::field_events::FieldEvent;
use crate::world::World;
use legaia_engine_vm::motion_vm::{MotionState, MotionTarget, StepResult, step};

/// Camera mode - controls how the camera derives its `eye` from the
/// world / scene state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// Follow a specific actor slot (default - slot 0 = player).
    #[default]
    Follow,
    /// Held at the last `Apply` payload - engine expects the field VM to
    /// keep ticking it via op `0x45`. Useful for cutscenes that pre-bake
    /// camera paths.
    Cinematic,
    /// Static - no per-frame motion. Useful for menus, title screen.
    Static,
}

/// Runtime camera. Composed from the field-VM's CameraState plus a follow
/// target. Engines call [`Camera::tick`] each frame after the world ticks
/// to update the view; the resulting `eye` / `look_at` pair feeds the
/// renderer's `view` matrix.
#[derive(Debug, Clone)]
pub struct Camera {
    pub mode: CameraMode,
    /// Actor slot to follow when `mode == Follow`. Defaults to 0.
    pub follow_slot: u8,
    /// Distance from target along the -Z axis when following. This is an
    /// engine framing choice - op-0x45 carries no eye-distance param (retail
    /// places the eye at the GTE translation and projects through `H`), so it
    /// is not driven by Camera Configure.
    pub follow_distance: f32,
    /// Y offset added to `look_at`. Engine framing default (comfortable
    /// shoulder height); like `follow_distance`, not an op-0x45 param.
    pub follow_height: f32,
    /// Computed eye position in world coordinates.
    pub eye: [f32; 3],
    /// Computed look-at point.
    pub look_at: [f32; 3],
    /// Yaw in radians (wrapped). Renderers can read this directly when they
    /// want a free-camera mode.
    pub yaw: f32,
    /// Pitch in radians.
    pub pitch: f32,
    /// Internal motion-VM state used for cinematic / scripted paths. Driven
    /// by [`Camera::tick_script`].
    pub motion_state: MotionState,
    /// Latest cinematic target - set when an op `0x45` apply event fires.
    pub motion_target: MotionTarget,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            mode: CameraMode::Follow,
            follow_slot: 0,
            follow_distance: 200.0,
            follow_height: 80.0,
            eye: [0.0, 80.0, 200.0],
            look_at: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            motion_state: MotionState::default(),
            motion_target: MotionTarget::default(),
        }
    }
}

impl Camera {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain the world's pending field-VM events of camera variants and
    /// fold them into this camera. Non-camera events are restored to the
    /// world queue so engine layers that also consume them aren't shorted.
    /// Returns the number of camera events applied this frame.
    ///
    /// The op-`0x45` Configure slot→camera mapping mirrors the retail apply
    /// handler; the GTE rotation build it feeds is `FUN_8001CF50`.
    ///
    /// REF: FUN_801DE084
    pub fn route_camera_events(&mut self, world: &mut World) -> usize {
        let mut applied = 0usize;
        let mut leftover = Vec::new();
        for ev in world.drain_field_events() {
            match ev {
                FieldEvent::CameraConfigure {
                    params,
                    apply_trigger,
                    mode,
                } => {
                    // Op-0x45 slot layout, pinned from the Camera Configure
                    // apply handler `FUN_801DE084` (writes the camera globals)
                    // + the GTE rotation build `FUN_8001CF50` (RotMatrixX/Y/Z
                    // at 0x800461A4/629C/638C). The 10 slots are three Euler
                    // angles, an offset trio, a focus trio, and H:
                    //   0 = pitch  (`_DAT_8007B790`, RotX)   1 = yaw (RotY)
                    //   2 = roll   (`_DAT_8007B794`, RotZ)   3,4,5 = offset
                    //   6,7,8 = focus (negated translation)  9 = GTE H
                    // Angles are 12-bit (4096 = 360 deg). See
                    // docs/subsystems/cutscene.md.
                    let ang = |v: u16| (v as i16) as f32 * std::f32::consts::TAU / 4096.0;
                    let slot = |s: u8| params.iter().find(|p| p.slot == s).map(|p| p.value);
                    if let Some(v) = slot(0) {
                        self.pitch = ang(v);
                    }
                    if let Some(v) = slot(1) {
                        self.yaw = ang(v);
                    }
                    // A full focus trio (slots 6/7/8) re-targets the cinematic
                    // look-at. The focus globals are the negated GTE
                    // translation, so X/Z are negated back to a world point
                    // (matching the shell's `cutscene_view`).
                    if let (Some(fx), Some(fy), Some(fz)) = (slot(6), slot(7), slot(8)) {
                        self.look_at = [
                            -((fx as i16) as f32),
                            (fy as i16) as f32,
                            -((fz as i16) as f32),
                        ];
                    }
                    let _ = (apply_trigger, mode);
                    if std::env::var_os("LEGAIA_DIAG_CAMERA").is_some() {
                        eprintln!(
                            "DIAG camera configure: params={params:?} -> pitch={:.3} yaw={:.3} look_at={:?}",
                            self.pitch, self.yaw, self.look_at
                        );
                    }
                    applied += 1;
                }
                FieldEvent::CameraSave => {
                    // Engine snapshots the current eye/look-at into world.camera_state
                    // already; we just record we saw the event.
                    applied += 1;
                }
                FieldEvent::CameraLoad { payload } => {
                    if payload.len() >= 12 {
                        let read_i16 = |off: usize| -> i16 {
                            i16::from_le_bytes([payload[off], payload[off + 1]])
                        };
                        self.eye = [read_i16(0) as f32, read_i16(2) as f32, read_i16(4) as f32];
                        self.look_at =
                            [read_i16(6) as f32, read_i16(8) as f32, read_i16(10) as f32];
                        self.mode = CameraMode::Cinematic;
                    }
                    applied += 1;
                }
                FieldEvent::CameraApply => {
                    // Apply commits whatever the configure pass staged; engine
                    // can re-derive eye/look-at on the next tick.
                    self.mode = CameraMode::Cinematic;
                    applied += 1;
                }
                other => leftover.push(other),
            }
        }
        world.pending_field_events.extend(leftover);
        applied
    }

    /// Per-frame tick. Reads the world to update `eye` / `look_at` based on
    /// `mode`. Pure function over the world - engines call after
    /// [`World::tick`] each frame.
    ///
    /// [`World::tick`]: crate::world::World::tick
    pub fn tick(&mut self, world: &World) {
        match self.mode {
            CameraMode::Follow => {
                let actor = world
                    .actors
                    .get(self.follow_slot as usize)
                    .filter(|a| a.active);
                if let Some(a) = actor {
                    let tx = a.move_state.world_x as f32;
                    let ty = a.move_state.world_y as f32;
                    let tz = a.move_state.world_z as f32;
                    self.look_at = [tx, ty + self.follow_height, tz];
                    let yaw_sin = self.yaw.sin();
                    let yaw_cos = self.yaw.cos();
                    self.eye = [
                        tx - self.follow_distance * yaw_sin,
                        ty + self.follow_height,
                        tz - self.follow_distance * yaw_cos,
                    ];
                }
            }
            CameraMode::Static | CameraMode::Cinematic => {
                // No per-frame motion - keep eye/look_at at whatever the last
                // event configured.
            }
        }
    }

    /// Drive the cinematic motion script for one tick. Optional layer above
    /// [`Camera::tick`] - engines that want to pre-bake camera paths upload
    /// motion-VM bytecode and call this each frame.
    pub fn tick_script(&mut self, bytecode: &[u8]) -> StepResult {
        step(&mut self.motion_state, self.motion_target, bytecode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SceneMode;
    use legaia_engine_vm::Position as ActorVmPosition;

    fn world_with_actor_at(slot: u8, x: i16, z: i16) -> World {
        let mut w = World::default();
        let actor = w.spawn_actor(slot as usize);
        actor.default_pos = ActorVmPosition::new(x, 0);
        actor.move_state.world_x = x;
        actor.move_state.world_y = 0;
        actor.move_state.world_z = z;
        w
    }

    #[test]
    fn follow_mode_tracks_actor_xz() {
        let w = world_with_actor_at(0, 100, 200);
        let mut c = Camera::default();
        c.tick(&w);
        // look_at = (100, height, 200).
        assert_eq!(c.look_at, [100.0, 80.0, 200.0]);
        // eye = (100, height, 200 - distance) when yaw == 0.
        assert_eq!(c.eye, [100.0, 80.0, 200.0 - 200.0]);
    }

    #[test]
    fn follow_mode_tracks_player_after_locomotion() {
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.install_field_player(0);
        w.actors[0].move_state.world_x = 100;
        w.actors[0].move_state.world_z = 100;
        let mut c = Camera {
            follow_slot: 0,
            ..Default::default()
        };
        // Walk +Z one frame (speed 8) then advance the camera.
        w.set_pad(crate::input::PadButton::Up.mask());
        let _ = w.tick();
        c.tick(&w);
        assert_eq!(w.actors[0].move_state.world_z, 108);
        // Camera look-at Z tracks the moved player.
        assert_eq!(c.look_at[2], 108.0);
    }

    #[test]
    fn follow_mode_yaw_offsets_eye() {
        let w = world_with_actor_at(0, 0, 0);
        let mut c = Camera {
            yaw: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        c.tick(&w);
        // yaw=π/2 -> sin=1, cos=0 -> eye_x = -distance, eye_z = 0.
        assert!((c.eye[0] + 200.0).abs() < 1e-3, "eye_x={}", c.eye[0]);
        assert!(c.eye[2].abs() < 1e-3, "eye_z={}", c.eye[2]);
    }

    #[test]
    fn static_mode_does_not_move_eye() {
        let mut c = Camera {
            mode: CameraMode::Static,
            eye: [1.0, 2.0, 3.0],
            look_at: [4.0, 5.0, 6.0],
            ..Default::default()
        };
        let w = world_with_actor_at(0, 99, 99);
        c.tick(&w);
        assert_eq!(c.eye, [1.0, 2.0, 3.0]);
        assert_eq!(c.look_at, [4.0, 5.0, 6.0]);
    }

    #[test]
    fn route_camera_events_consumes_camera_variants() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        // Decoded op-0x45 slots: 0 = pitch, 1 = yaw, 6/7/8 = focus.
        // 1024 (12-bit) = quarter turn = TAU/4.
        w.pending_field_events = vec![
            FieldEvent::CameraConfigure {
                params: vec![
                    CameraParam {
                        slot: 0,
                        value: 512,
                    }, // pitch 1/8 turn
                    CameraParam {
                        slot: 1,
                        value: 1024,
                    }, // yaw 1/4 turn
                    CameraParam {
                        slot: 6,
                        value: (-100i16) as u16,
                    },
                    CameraParam { slot: 7, value: 40 },
                    CameraParam {
                        slot: 8,
                        value: (-200i16) as u16,
                    },
                ],
                apply_trigger: 0,
                mode: 0,
            },
            FieldEvent::CameraApply,
            FieldEvent::Bgm {
                text_id: 1,
                sub_op: 1,
            },
        ];
        let mut c = Camera::default();
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 2);
        use std::f32::consts::TAU;
        assert!((c.pitch - TAU / 8.0).abs() < 1e-3, "slot 0 -> pitch");
        assert!((c.yaw - TAU / 4.0).abs() < 1e-3, "slot 1 -> yaw");
        // Focus (6/7/8) -> look_at with X/Z negated back to world space.
        assert_eq!(c.look_at, [100.0, 40.0, 200.0]);
        // Non-camera event preserved.
        assert_eq!(w.pending_field_events.len(), 1);
        match &w.pending_field_events[0] {
            FieldEvent::Bgm { sub_op, .. } => assert_eq!(*sub_op, 1),
            other => panic!("expected Bgm, got {other:?}"),
        }
    }

    #[test]
    fn route_camera_load_writes_eye_and_lookat() {
        let mut w = World::default();
        let mut payload = vec![0u8; 12];
        payload[0..2].copy_from_slice(&10i16.to_le_bytes());
        payload[2..4].copy_from_slice(&20i16.to_le_bytes());
        payload[4..6].copy_from_slice(&30i16.to_le_bytes());
        payload[6..8].copy_from_slice(&40i16.to_le_bytes());
        payload[8..10].copy_from_slice(&50i16.to_le_bytes());
        payload[10..12].copy_from_slice(&60i16.to_le_bytes());
        w.pending_field_events = vec![FieldEvent::CameraLoad { payload }];
        let mut c = Camera::default();
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 1);
        assert_eq!(c.eye, [10.0, 20.0, 30.0]);
        assert_eq!(c.look_at, [40.0, 50.0, 60.0]);
        assert_eq!(c.mode, CameraMode::Cinematic);
    }

    #[test]
    fn tick_script_advances_motion_state() {
        let mut c = Camera::default();
        c.motion_state.speed = 2;
        c.motion_target = MotionTarget {
            x: 4,
            y: 0,
            z: 0,
            id: 0,
        };
        let bc = [0x41]; // TranslateX without target byte
        let r1 = c.tick_script(&bc);
        assert_eq!(r1, StepResult::Yield);
        assert_eq!(c.motion_state.world_x, 2);
    }
}
