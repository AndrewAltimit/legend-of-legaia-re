//! Per-scene camera controller.
//!
//! Consumes the field-VM op-`0x45` event stream (Configure / Save / Load /
//! Apply — see [`crate::field_events::FieldEvent`]) and projects a target
//! actor's world position into a screen-space view. Engines plug the result
//! into [`legaia_engine_render`] each frame.
//!
//! Two layers:
//!
//! - [`CameraState`] (in [`crate::world`]) — the raw scratch the field VM
//!   reads / writes. Holds the most recent op-`0x45` payloads.
//! - [`Camera`] (here) — the *runtime* camera. Reads `CameraState`, layers
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

/// Camera mode — controls how the camera derives its `eye` from the
/// world / scene state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// Follow a specific actor slot (default — slot 0 = player).
    #[default]
    Follow,
    /// Held at the last `Apply` payload — engine expects the field VM to
    /// keep ticking it via op `0x45`. Useful for cutscenes that pre-bake
    /// camera paths.
    Cinematic,
    /// Static — no per-frame motion. Useful for menus, title screen.
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
    /// Distance from target along the -Z axis when following. Engines bump
    /// this from the camera-configure payload's `slot 1` (zoom param).
    pub follow_distance: f32,
    /// Y offset added to `look_at`. Engines bump this from configure payload
    /// `slot 2` (height param). Defaults to a comfortable shoulder height.
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
    /// Latest cinematic target — set when an op `0x45` apply event fires.
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
                    // Slot conventions inferred from CameraConfigure usage in
                    // the field-VM: slot 1 = follow distance, slot 2 = height,
                    // slot 3 = yaw delta. Other slots feed cinematic state.
                    for p in &params {
                        match p.slot {
                            1 => self.follow_distance = (p.value as i16) as f32,
                            2 => self.follow_height = (p.value as i16) as f32,
                            3 => {
                                self.yaw = (p.value as i16) as f32 * std::f32::consts::TAU / 4096.0
                            }
                            _ => {}
                        }
                    }
                    let _ = (apply_trigger, mode);
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
    /// `mode`. Pure function over the world — engines call after
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
                // No per-frame motion — keep eye/look_at at whatever the last
                // event configured.
            }
        }
    }

    /// Drive the cinematic motion script for one tick. Optional layer above
    /// [`Camera::tick`] — engines that want to pre-bake camera paths upload
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
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.pending_field_events = vec![
            FieldEvent::CameraConfigure {
                params: vec![
                    legaia_engine_vm::field::CameraParam {
                        slot: 1,
                        value: 300,
                    },
                    legaia_engine_vm::field::CameraParam {
                        slot: 2,
                        value: 100,
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
        assert_eq!(c.follow_distance, 300.0);
        assert_eq!(c.follow_height, 100.0);
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
