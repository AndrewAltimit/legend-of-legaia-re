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
use serde::{Deserialize, Serialize};

/// Discrete camera-distance preset for the field follow camera. `Retail`
/// is the faithful framing; `Far` / `Farther` are engine enhancements that
/// pull the eye back so more of the scene is on screen. A pure framing
/// knob: it scales the eye-back distance only, so it never feeds the
/// world simulation (locomotion, encounters, replays are unaffected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraDistance {
    /// The savestate-pinned retail framing (scale 1.0).
    #[default]
    Retail,
    /// A bit further out than retail - the interactive play-window default.
    Far,
    /// Wide vantage for eyeballing scene layout.
    Farther,
}

impl CameraDistance {
    /// Multiplier applied to the follow camera's eye-back distance.
    pub fn scale(self) -> f32 {
        match self {
            Self::Retail => 1.0,
            Self::Far => 1.35,
            Self::Farther => 1.8,
        }
    }

    /// Next preset in the cycle Retail -> Far -> Farther -> Retail.
    pub fn cycle(self) -> Self {
        match self {
            Self::Retail => Self::Far,
            Self::Far => Self::Farther,
            Self::Farther => Self::Retail,
        }
    }

    /// Human-readable label for HUD / logs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::Far => "far",
            Self::Farther => "farther",
        }
    }

    /// Parse a CLI/HUD label (`retail` / `far` / `farther`),
    /// case-insensitive. `None` for unknown strings.
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "retail" => Some(Self::Retail),
            "far" => Some(Self::Far),
            "farther" => Some(Self::Farther),
            _ => None,
        }
    }
}

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
    /// User-controlled orbit around the follow target (radians), in the
    /// **compass sense**: positive swings "screen up" from world `+Z`
    /// toward `+X`. Composed on top of the scripted [`Self::yaw`] by both
    /// the follow-eye computation and [`Self::compass_azimuth_units`], so
    /// dragging the camera around the player keeps the movement compass
    /// aligned with the view. Preserved across
    /// [`Self::reset_for_free_roam`] (it is player intent, not leaked
    /// cutscene state). Default `0.0`.
    pub manual_orbit: f32,
    /// Fixed yaw the HOST's renderer frames the follow view with, in the
    /// compass sense (radians). A renderer that draws the field at a
    /// non-zero base yaw (e.g. the play-window's savestate-pinned
    /// `-160`-unit follow yaw = `+160` units compass) sets this once so
    /// [`Self::compass_azimuth_units`] reports the yaw the player actually
    /// sees. Default `0.0` (headless hosts / the plain follow eye).
    pub render_yaw_bias: f32,
    /// Discrete eye-back distance preset. Scales [`Self::follow_distance`]
    /// in the follow-eye computation; render hosts multiply their own
    /// follow-camera depth by [`CameraDistance::scale`]. Default
    /// [`CameraDistance::Retail`] keeps every headless / oracle path
    /// bit-identical; interactive hosts may default further out.
    pub distance: CameraDistance,
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
            manual_orbit: 0.0,
            render_yaw_bias: 0.0,
            distance: CameraDistance::Retail,
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
                    // Focus slots 6/7/8 re-target the cinematic look-at, each
                    // applied INDEPENDENTLY on its own presence - the retail
                    // apply handler `FUN_801DE084` writes each camera focus
                    // global only when its slot bit is set, leaving the others
                    // at their prior value. A beat that supplies only focus X/Z
                    // (opdeene's opening beats omit slot 7 entirely) must still
                    // pan the look-at horizontally rather than freeze it; the
                    // all-or-nothing gate used before never retargeted such
                    // beats, pinning the shot on one angle. The focus globals
                    // are the negated GTE translation, so X/Z are negated back
                    // to a world point (matching the shell's `cutscene_view`).
                    if let Some(fx) = slot(6) {
                        self.look_at[0] = -((fx as i16) as f32);
                    }
                    if let Some(fy) = slot(7) {
                        self.look_at[1] = (fy as i16) as f32;
                    }
                    if let Some(fz) = slot(8) {
                        self.look_at[2] = -((fz as i16) as f32);
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
                    // Effective yaw = scripted yaw + the user's manual orbit
                    // (compass sense: forward = (sin, cos)). Distance preset
                    // scales the eye-back distance only. Defaults (orbit 0,
                    // Retail) keep this arithmetic bit-identical to the
                    // historical `yaw`/`follow_distance` form.
                    let yaw = self.yaw + self.manual_orbit;
                    let dist = self.follow_distance * self.distance.scale();
                    self.eye = [
                        tx - dist * yaw.sin(),
                        ty + self.follow_height,
                        tz - dist * yaw.cos(),
                    ];
                }
            }
            CameraMode::Static | CameraMode::Cinematic => {
                // No per-frame motion - keep eye/look_at at whatever the last
                // event configured.
            }
        }
    }

    /// The camera azimuth to feed
    /// [`World::field_camera_azimuth`](crate::world::World::field_camera_azimuth)
    /// this frame, in PSX 12-bit units (`4096` = full turn): scripted yaw +
    /// the user's manual orbit + the host renderer's fixed framing bias.
    /// This is what keeps the d-pad -> world-direction remap ("screen up
    /// walks away from the camera") tracking the yaw the player actually
    /// sees, including after a drag-orbit. All three terms default to `0`,
    /// so headless hosts keep the historical `yaw`-only feed bit-identical.
    pub fn compass_azimuth_units(&self) -> u16 {
        let az = (self.yaw + self.manual_orbit + self.render_yaw_bias) / std::f32::consts::TAU
            * 4096.0;
        az.rem_euclid(4096.0) as u16
    }

    /// Drive the cinematic motion script for one tick. Optional layer above
    /// [`Camera::tick`] - engines that want to pre-bake camera paths upload
    /// motion-VM bytecode and call this each frame.
    pub fn tick_script(&mut self, bytecode: &[u8]) -> StepResult {
        step(&mut self.motion_state, self.motion_target, bytecode)
    }

    /// Snap the camera back to the follow default when the field is in
    /// free-roam (a plain [`SceneMode::Field`] with no cutscene timeline owning
    /// the scene).
    ///
    /// An opening / scripted cutscene folds op-`0x45` Camera Configure yaw into
    /// [`Self::yaw`] and flips [`Self::mode`] to [`CameraMode::Cinematic`] (see
    /// [`Self::route_camera_events`]). That stale cinematic yaw must not leak
    /// into free-roam: a renderer frames free-roam field with a fixed follow
    /// camera, and hosts feed [`Self::yaw`] into
    /// [`World::field_camera_azimuth`](crate::world::World::field_camera_azimuth)
    /// to remap the d-pad camera-relative - so a non-zero leaked yaw rotates
    /// the controls off the on-screen camera (the New Game prologue → Rim Elm
    /// hand-off left the d-pad ~180deg inverted). Retail returns control on the
    /// follow camera; this restores it.
    ///
    /// Gated on `!cutscene_timeline_active()` (the same gate that unlocks
    /// [`World::step_field_locomotion`](crate::world::World::step_field_locomotion)),
    /// so an active cutscene's own beats keep their configured yaw. No-op
    /// outside free-roam field (world map / battle / menu / cutscene).
    pub fn reset_for_free_roam(&mut self, world: &World) {
        if matches!(world.mode, crate::world::SceneMode::Field) && !world.cutscene_timeline_active()
        {
            self.mode = CameraMode::Follow;
            self.yaw = 0.0;
            self.pitch = 0.0;
        }
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
    fn camera_configure_focus_slots_apply_per_axis() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        // A beat that supplies focus X (slot 6) and Z (slot 8) but NOT Y
        // (slot 7) - opdeene's opening beats omit slot 7 entirely. The look-at
        // must pan X/Z while keeping the prior Y, not stay frozen.
        w.pending_field_events = vec![FieldEvent::CameraConfigure {
            params: vec![
                CameraParam {
                    slot: 6,
                    value: (-100i16) as u16,
                },
                CameraParam {
                    slot: 8,
                    value: (-200i16) as u16,
                },
            ],
            apply_trigger: 0,
            mode: 0,
        }];
        // Prior look-at (e.g. the scene-centre Y a shell falls back to).
        let mut c = Camera {
            look_at: [1.0, 55.0, 2.0],
            ..Default::default()
        };
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 1);
        assert_eq!(
            c.look_at,
            [100.0, 55.0, 200.0],
            "X/Z retarget from slots 6/8; Y kept from the prior look-at (slot 7 absent)"
        );
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
    fn reset_for_free_roam_clears_leaked_cinematic_yaw() {
        // A cutscene left the camera Cinematic at a ~180deg yaw (the state that
        // inverts the field d-pad remap). Free-roam field (no active timeline)
        // must snap it back to the follow default so `field_camera_azimuth`
        // quantises to quadrant 0.
        let w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        assert!(!w.cutscene_timeline_active(), "no timeline installed");
        let mut c = Camera {
            mode: CameraMode::Cinematic,
            yaw: std::f32::consts::PI,
            pitch: 0.5,
            ..Default::default()
        };
        c.reset_for_free_roam(&w);
        assert_eq!(c.mode, CameraMode::Follow);
        assert_eq!(c.yaw, 0.0);
        assert_eq!(c.pitch, 0.0);
    }

    #[test]
    fn reset_for_free_roam_noop_outside_field() {
        // Only free-roam field resets; other modes keep whatever the scene
        // configured (e.g. a menu / battle / world-map camera).
        for mode in [SceneMode::Menu, SceneMode::Battle, SceneMode::WorldMap] {
            let w = World {
                mode,
                ..World::default()
            };
            let mut c = Camera {
                mode: CameraMode::Cinematic,
                yaw: std::f32::consts::PI,
                ..Default::default()
            };
            c.reset_for_free_roam(&w);
            assert_eq!(c.mode, CameraMode::Cinematic, "mode {mode:?} untouched");
            assert_eq!(c.yaw, std::f32::consts::PI, "mode {mode:?} yaw kept");
        }
    }

    #[test]
    fn distance_preset_scales_follow_eye_only() {
        let w = world_with_actor_at(0, 0, 0);
        let mut c = Camera {
            distance: CameraDistance::Far,
            ..Default::default()
        };
        c.tick(&w);
        // Eye pulled back by the preset scale; look-at unchanged.
        assert!((c.eye[2] + 200.0 * CameraDistance::Far.scale()).abs() < 1e-3);
        assert_eq!(c.look_at, [0.0, 80.0, 0.0]);
        // Retail preset is the identity (the historical framing).
        let mut r = Camera::default();
        r.tick(&w);
        assert_eq!(r.eye, [0.0, 80.0, -200.0]);
    }

    #[test]
    fn distance_cycle_and_labels_round_trip() {
        let mut d = CameraDistance::Retail;
        for _ in 0..3 {
            assert_eq!(CameraDistance::from_label(d.label()), Some(d));
            d = d.cycle();
        }
        assert_eq!(d, CameraDistance::Retail, "cycle is a 3-cycle");
        assert!(CameraDistance::Retail.scale() == 1.0);
        assert!(CameraDistance::Far.scale() > 1.0);
        assert!(CameraDistance::Farther.scale() > CameraDistance::Far.scale());
    }

    #[test]
    fn manual_orbit_rotates_follow_eye_and_compass_together() {
        let w = world_with_actor_at(0, 0, 0);
        let mut c = Camera {
            manual_orbit: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        c.tick(&w);
        // Quarter-turn orbit: eye swings to -X (same as a scripted
        // yaw = pi/2 - see `follow_mode_yaw_offsets_eye`).
        assert!((c.eye[0] + 200.0).abs() < 1e-3, "eye_x={}", c.eye[0]);
        assert!(c.eye[2].abs() < 1e-3, "eye_z={}", c.eye[2]);
        // And the compass azimuth follows: pi/2 = 1024 units, so the
        // d-pad remap quantises to quadrant 1 (screen-up walks +X).
        assert_eq!(c.compass_azimuth_units(), 1024);
    }

    #[test]
    fn compass_azimuth_defaults_to_zero_and_sums_bias() {
        let c = Camera::default();
        assert_eq!(c.compass_azimuth_units(), 0, "defaults keep the old feed");
        let c = Camera {
            yaw: std::f32::consts::PI,
            manual_orbit: std::f32::consts::FRAC_PI_2,
            render_yaw_bias: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        // pi + pi/2 + pi/2 = full turn -> wraps to 0.
        assert_eq!(c.compass_azimuth_units(), 0);
    }

    #[test]
    fn reset_for_free_roam_preserves_manual_orbit_and_distance() {
        let w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        let mut c = Camera {
            mode: CameraMode::Cinematic,
            yaw: std::f32::consts::PI,
            manual_orbit: 0.5,
            distance: CameraDistance::Farther,
            ..Default::default()
        };
        c.reset_for_free_roam(&w);
        assert_eq!(c.yaw, 0.0, "scripted yaw resets");
        assert_eq!(c.manual_orbit, 0.5, "player orbit intent is kept");
        assert_eq!(c.distance, CameraDistance::Farther, "preset is kept");
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
