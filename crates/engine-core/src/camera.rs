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
use legaia_engine_vm::camera_mover::{AXIS_COUNT, CameraMover};
use legaia_engine_vm::motion_vm::{MotionState, MotionTarget, StepResult, step};
use serde::{Deserialize, Serialize};

/// The ten live retail camera globals, in the order the op-`0x45` param mask
/// and the camera mover both use them.
///
/// This is the state the retail engine actually renders from, and the state a
/// state trace samples - not a world-space `(eye, look_at)` pair. Keeping it
/// verbatim is what makes the engine comparable to a recomp capture channel
/// for channel:
///
/// | axis | global | role |
/// |---|---|---|
/// | 0 / 1 / 2 | `_DAT_8007B790/92/94` | pitch / yaw / roll (12-bit, `4096` = full turn) |
/// | 3 / 4 / 5 | `_DAT_800840B8/BC/C0` | eye-space translation trio `tr_eye`; axis 5 is the eye-back depth |
/// | 6 / 7 / 8 | `_DAT_80089118/1C/20` | camera focus, stored **negated** in X and Z |
/// | 9 | `_DAT_8007B6F4` | GTE `H` projection register |
///
/// The focus storage convention is the one that catches people out: the
/// globals hold `(-X, +Y, -Z)` of the world focus point (`FUN_801DAB90`), so
/// a retail capture of a shot focused on world `(8640, 0, 10304)` reads
/// `(-8640, 0, -10304)`. See
/// [`cutscene.md`](../../../docs/subsystems/cutscene.md).
///
/// REF: FUN_801DE084
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailCamGlobals(pub [i32; AXIS_COUNT]);

impl RetailCamGlobals {
    /// The field-entry reset values written by `FUN_80025C24` (caller
    /// `FUN_801D6704`, field init): angles `(0x1B8, 0x64, 0)` and
    /// `tr_eye = (0, -256, 16420)`. Focus and `H` are left as the scene
    /// establishes them.
    ///
    /// PORT: FUN_80025C24
    pub const FIELD_RESET: Self = Self([0x1B8, 0x64, 0, 0, -256, 16420, 0, 0, 0, 0]);

    /// Pitch / yaw / roll, 12-bit units.
    pub fn angles(&self) -> [i32; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// The eye-space translation trio (`_DAT_800840B8`).
    pub fn tr_eye(&self) -> [i32; 3] {
        [self.0[3], self.0[4], self.0[5]]
    }

    /// The focus trio exactly as retail stores it - X and Z **negated**.
    pub fn focus_stored(&self) -> [i32; 3] {
        [self.0[6], self.0[7], self.0[8]]
    }

    /// The focus as a world-space point: `(-axis6, axis7, -axis8)`.
    pub fn focus_world(&self) -> [i32; 3] {
        [-self.0[6], self.0[7], -self.0[8]]
    }

    /// GTE `H`.
    pub fn h(&self) -> i32 {
        self.0[9]
    }

    /// The same ten axes in the shape the camera-relative effect-actor
    /// normalizer wants (`legaia_engine_vm::camera_rel_actor`). The
    /// normalizer compares each of a spawn record's ten reference
    /// halfwords against exactly these globals, so the conversion is a
    /// re-labelling, not a transform - note in particular that the focus
    /// goes across **stored** (X and Z negated), because that is the form
    /// `FUN_80021248` compares against.
    pub fn camera_snapshot(&self) -> legaia_engine_vm::camera_rel_actor::CameraSnapshot {
        legaia_engine_vm::camera_rel_actor::CameraSnapshot {
            angles: [self.0[0] as u16, self.0[1] as u16, self.0[2] as u16],
            offsets: self.tr_eye(),
            focus: self.focus_stored(),
            gte_h: self.0[9] as i16,
        }
    }
}

impl Default for RetailCamGlobals {
    fn default() -> Self {
        Self::FIELD_RESET
    }
}

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
    /// The live retail camera globals - the pose retail actually renders and
    /// a state trace samples. Driven by op-`0x45` Configure beats through
    /// [`Self::globals`] / [`Self::mover`], and by the follow camera in
    /// [`Self::tick`].
    pub globals: RetailCamGlobals,
    /// The single in-flight camera-mover glide, when a beat staged one with
    /// `apply != 0`. `None` once it has arrived (the retail actor marks
    /// itself dead and frees its pair block).
    pub mover: Option<CameraMover>,
    /// Display-frame counter the mover was last advanced to, so a glide
    /// advances in retail display frames rather than sim ticks (retail's
    /// `DAT_1F800393` credit - see `camera_mover`'s module docs).
    last_field_frame: u64,
    /// Latched once this scene has executed an op-`0x45` Configure: from then
    /// on the script owns the focus globals and the follow camera stops
    /// writing them. A scripted scene seizes the camera in retail, and the
    /// focus it stages is meant to survive the settled gaps between beats -
    /// retail holds two distinct focus values across the whole of `opdeene`.
    /// Cleared by [`Self::reset_globals_for_scene_entry`].
    script_owns_focus: bool,
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
            globals: RetailCamGlobals::default(),
            mover: None,
            last_field_frame: 0,
            script_owns_focus: false,
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
                    // The ten retail globals. Every masked slot writes its
                    // axis; an absent slot holds its prior value, which is
                    // what `FUN_801DE084` does by writing only the slots the
                    // mask selects. `apply_trigger` then chooses between the
                    // two commit behaviours:
                    //
                    // - `apply == 0` - SNAP. Write straight through and mark
                    //   every live mover dead, cancelling a glide in flight.
                    // - `apply != 0` - GLIDE. Hand the one mover actor ten
                    //   `(start, end)` pairs, `start` from the LIVE globals,
                    //   and let it interpolate over `apply` display frames
                    //   with `mode` as the shared ease curve.
                    //
                    // This is the half the camera was missing entirely: the
                    // eye-space translation trio (slots 3/4/5) had no engine
                    // representation at all, so every scripted shot rendered
                    // and traced from the follow orbit's fixed height.
                    self.script_owns_focus = true;
                    let mut target = self.globals;
                    for p in &params {
                        if (p.slot as usize) < AXIS_COUNT {
                            target.0[p.slot as usize] = (p.value as i16) as i32;
                        }
                    }
                    if apply_trigger == 0 {
                        self.globals = target;
                        self.mover = None;
                    } else {
                        let mut mv = self.mover.take().unwrap_or_default();
                        mv.arm(self.globals.0, target.0, apply_trigger, mode);
                        self.mover = Some(mv);
                    }
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
        self.tick_globals(world);
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

    /// Advance the retail camera globals one frame: run any in-flight mover
    /// glide, then let the follow camera write the focus it owns.
    ///
    /// The mover is clocked in **display frames**, not sim ticks - retail
    /// credits `DAT_1F800393` (the adaptive frame-skip factor) per logic tick,
    /// which banks exactly one unit per display frame, making every authored
    /// `apply` a duration in 60 Hz frames. `World::field_frames` is the
    /// engine's display-frame counter, so diffing it is the faithful clock
    /// (the same thing the renderer's glide does).
    ///
    /// In [`CameraMode::Follow`] with no glide in flight, the follow camera
    /// owns the focus globals: `FUN_801DBE9C` stores the **negated** anchor
    /// position (`_DAT_80089118 = -(anchor+0x14)`,
    /// `_DAT_80089120 = -(anchor+0x18)`). Writing it here is what makes a
    /// free-roam field frame comparable against a retail capture, which
    /// samples those same globals whether a cutscene is running or not.
    ///
    /// PORT: FUN_801DC0BC
    /// REF: FUN_801DBE9C
    fn tick_globals(&mut self, world: &World) {
        let now = world.field_frames;
        let dt = now.saturating_sub(self.last_field_frame) as i32;
        self.last_field_frame = now;

        let gliding = if let Some(mv) = self.mover.as_mut() {
            let arrived = mv.tick(dt);
            self.globals.0 = mv.values();
            if arrived {
                self.mover = None;
            }
            true
        } else {
            false
        };

        // The follow camera only owns the focus in free-roam. A cutscene's
        // staged focus must survive the gaps BETWEEN its beats, not just the
        // frames a glide happens to be in flight: retail's `opdeene` holds two
        // distinct focus values across the whole scene, so a writeback gated
        // only on `!gliding` re-pins the focus to the player on every settled
        // frame and turns those two values into ~1000.
        let scripted = gliding || self.script_owns_focus || world.cutscene_timeline_active();
        if !scripted
            && self.mode == CameraMode::Follow
            && let Some(a) = world
                .actors
                .get(self.follow_slot as usize)
                .filter(|a| a.active)
        {
            self.globals.0[6] = -(a.move_state.world_x as i32);
            self.globals.0[8] = -(a.move_state.world_z as i32);
        }
    }

    /// Reset the retail camera globals to their field-entry values and drop
    /// any glide in flight - the engine side of `FUN_80025C24`, called when a
    /// scene is entered so a previous scene's shot can't leak into the next.
    ///
    /// PORT: FUN_80025C24
    pub fn reset_globals_for_scene_entry(&mut self) {
        self.globals = RetailCamGlobals::FIELD_RESET;
        self.mover = None;
        self.script_owns_focus = false;
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
        let az =
            (self.yaw + self.manual_orbit + self.render_yaw_bias) / std::f32::consts::TAU * 4096.0;
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

/// The default camera-zone parameter set `FUN_801DBE9C` installs when the
/// per-tile zone query misses (no camera-region record covers the player's
/// tile). Raw values + the `0x8007B607..` globals they land in.
///
/// | field | global | value |
/// |---|---|---|
/// | `mode` | `DAT_8007B607` | `0x10` |
/// | `param_b608` | `DAT_8007B608` | `0x10` |
/// | `param_b609` | `DAT_8007B609` | `0x30` |
/// | `param_b60a` | `DAT_8007B60A` | `0x51` |
/// | `param_b60b` | `DAT_8007B60B` | `0x20` |
/// | `angle` | `DAT_8007B60C` | `0x1B8` |
/// | `b610` | `DAT_8007B610` | `0` |
/// | `b614` | `DAT_8007B614` | `0x4000` |
/// | `b618` | `DAT_8007B618` | `0x300` |
///
/// (`0x1B8` is the same default pitch `FUN_80025C24` seeds at scene entry;
/// see [`Camera::reset_globals_for_scene_entry`].)
// REF: FUN_801DBE9C (miss-path stores at 0x801dbf18..0x801dbf74)
pub const CAMERA_ZONE_DEFAULTS: [(u32, u32); 9] = [
    (0x8007B607, 0x10),
    (0x8007B608, 0x10),
    (0x8007B609, 0x30),
    (0x8007B60A, 0x51),
    (0x8007B60B, 0x20),
    (0x8007B60C, 0x1B8),
    (0x8007B610, 0),
    (0x8007B614, 0x4000),
    (0x8007B618, 0x300),
];

/// What one [`camera_zone_arrival_tick`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraZoneArrival {
    /// Countdown still running - nothing else happened, actor not flagged.
    Waiting,
    /// Countdown expired: the player tile was queried and a camera-region
    /// record hit - load it (`FUN_801DBC20`), then run the follow update.
    LoadZoneConfig,
    /// Countdown expired with no covering record: install
    /// [`CAMERA_ZONE_DEFAULTS`], then run the follow update.
    LoadDefaults,
}

/// The camera-zone **arrival tick** - one frame of `FUN_801DBE9C`'s
/// query arm (the `_DAT_8007B868 != 0` leg; the `== 0` leg skips the
/// query and only runs the follow update).
///
/// PORT: FUN_801dbe9c
///
/// Decrements the actor's `+0x54` countdown; while it has not reached
/// `-1` the tick returns [`CameraZoneArrival::Waiting`] (and retail
/// neither flags the actor nor touches the camera). On expiry the player
/// tile quantises as `(pos - 0x40) >> 7` (the region-refresh form, NOT
/// the walk-on dispatch's raw `>> 7`) and the zone-record query
/// (`FUN_801DBA20` = [`crate::field_regions::zone_query`]) picks between
/// the record load and [`CAMERA_ZONE_DEFAULTS`]. Either way the follow
/// update (`FUN_801DB8EC` + the negated-focus store, see
/// [`Camera::tick`]) runs and the actor's `+0x10` flags gain bit `8`.
///
/// Provenance: `overlay_0897_locomotion_cluster.txt` at `0x801dbe9c..
/// 0x801dc0b8` (the committed `FUN_801DBEC4` name is a mid-function
/// label of this body, not its entry).
///
/// NOT WIRED: the engine has no arrival countdown to tick. Its camera picks
/// a zone by querying the region table directly at the player's tile through
/// [`crate::field_regions::zone_query`] (`FUN_801DBA20`, which *is* live off
/// the field movement path), so there is no per-actor `+0x54` counter for a
/// scene entry or a door arrival to arm, and no `_DAT_8007B868` dev/retail
/// word selecting between the query leg and the follow-only leg. Wiring this
/// needs that countdown owned and armed somewhere - i.e. the retail camera
/// actor, not the engine's per-scene camera state.
pub fn camera_zone_arrival_tick(
    countdown: &mut i16,
    player_pos: (i16, i16),
    zone_hit: impl FnOnce(i32, i32) -> bool,
) -> CameraZoneArrival {
    *countdown = countdown.wrapping_sub(1);
    if *countdown != -1 {
        return CameraZoneArrival::Waiting;
    }
    let tile_x = i32::from(player_pos.0 - 0x40) >> 7;
    let tile_z = i32::from(player_pos.1 - 0x40) >> 7;
    if zone_hit(tile_x, tile_z) {
        CameraZoneArrival::LoadZoneConfig
    } else {
        CameraZoneArrival::LoadDefaults
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_arrival_counts_down_then_queries_at_region_tile() {
        let mut cd: i16 = 2;
        // Two waiting frames (2 -> 1, 1 -> 0), then expiry at -1.
        assert_eq!(
            camera_zone_arrival_tick(&mut cd, (0, 0), |_, _| true),
            CameraZoneArrival::Waiting
        );
        assert_eq!(
            camera_zone_arrival_tick(&mut cd, (0, 0), |_, _| true),
            CameraZoneArrival::Waiting
        );
        let mut seen = None;
        let r = camera_zone_arrival_tick(&mut cd, (1838, 2526), |x, z| {
            seen = Some((x, z));
            true
        });
        assert_eq!(r, CameraZoneArrival::LoadZoneConfig);
        // Region-refresh quantisation: (pos - 0x40) >> 7.
        assert_eq!(seen, Some(((1838 - 0x40) >> 7, (2526 - 0x40) >> 7)));
    }

    #[test]
    fn zone_arrival_miss_installs_defaults() {
        let mut cd: i16 = 0;
        assert_eq!(
            camera_zone_arrival_tick(&mut cd, (0x40, 0x40), |_, _| false),
            CameraZoneArrival::LoadDefaults
        );
        // The default set includes the scene-entry pitch and GTE far plane.
        assert!(CAMERA_ZONE_DEFAULTS.contains(&(0x8007B60C, 0x1B8)));
        assert_eq!(CAMERA_ZONE_DEFAULTS.len(), 9);
    }
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

    /// A snap beat (`apply == 0`) writes every masked slot straight into the
    /// retail globals, and an absent slot holds. The focus lands in retail's
    /// STORED convention (negated X/Z) - the trace channel compares against
    /// that word, not against a world-space point.
    #[test]
    fn snap_beat_writes_all_ten_globals_in_retail_convention() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World::default();
        let p = |slot: u8, value: i16| CameraParam {
            slot,
            value: value as u16,
        };
        w.pending_field_events = vec![FieldEvent::CameraConfigure {
            // Pitch/yaw, the full eye-space trio, focus X/Z (no slot 7), H.
            params: vec![
                p(0, 240),
                p(1, -455),
                p(3, 280),
                p(4, 5462),
                p(5, 832),
                p(6, -8568),
                p(8, -8944),
                p(9, 776),
            ],
            apply_trigger: 0,
            mode: 0,
        }];
        let mut c = Camera::default();
        c.route_camera_events(&mut w);
        assert_eq!(
            c.globals.angles(),
            [240, -455, 0],
            "pitch/yaw set, roll held"
        );
        assert_eq!(c.globals.tr_eye(), [280, 5462, 832], "eye-space trio");
        assert_eq!(
            c.globals.focus_stored(),
            [-8568, 0, -8944],
            "focus stored negated in X/Z; absent slot 7 holds its prior 0"
        );
        assert_eq!(c.globals.focus_world(), [8568, 0, 8944], "world focus");
        assert_eq!(c.globals.h(), 776);
        assert!(c.mover.is_none(), "a snap cancels any glide in flight");
    }

    /// A glide beat (`apply != 0`) arms the mover instead of snapping, and the
    /// globals interpolate toward the target over the beat's duration in
    /// display frames, arriving exactly.
    #[test]
    fn glide_beat_arms_the_mover_and_arrives_exactly() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World {
            // Slot 5 (eye-back depth) from the field reset 16420 -> 17420.
            pending_field_events: vec![FieldEvent::CameraConfigure {
                params: vec![CameraParam {
                    slot: 5,
                    value: 17420,
                }],
                apply_trigger: 100,
                mode: 1, // linear
            }],
            ..World::default()
        };
        let mut c = Camera::default();
        c.route_camera_events(&mut w);
        assert!(c.mover.is_some(), "apply != 0 arms a glide, does not snap");
        assert_eq!(
            c.globals.tr_eye()[2],
            16420,
            "arming alone does not move the global"
        );

        // Advance 50 of the 100 display frames - halfway on a linear curve.
        w.field_frames = 50;
        c.tick(&w);
        let mid = c.globals.tr_eye()[2];
        assert!(
            (16420..17420).contains(&mid),
            "midpoint {mid} interpolates between start and target"
        );

        // Run out the duration: exact arrival, and the one-shot mover retires.
        w.field_frames = 100;
        c.tick(&w);
        assert_eq!(c.globals.tr_eye()[2], 17420, "glide arrives exactly");
        assert!(c.mover.is_none(), "the mover is one-shot");
    }

    /// Scene entry restores the `FUN_80025C24` field defaults so a departing
    /// scene's shot cannot leak into the next one.
    #[test]
    fn scene_entry_resets_globals_to_field_defaults() {
        let mut c = Camera {
            globals: RetailCamGlobals([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            ..Default::default()
        };
        c.reset_globals_for_scene_entry();
        assert_eq!(c.globals, RetailCamGlobals::FIELD_RESET);
        assert_eq!(c.globals.angles(), [0x1B8, 0x64, 0]);
        assert_eq!(c.globals.tr_eye(), [0, -256, 16420]);
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
