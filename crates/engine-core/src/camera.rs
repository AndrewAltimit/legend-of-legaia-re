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
    /// establishes them - the routine is six stores and none of them is the
    /// focus trio or `_DAT_8007B6F4`, so [`Camera::reset_globals_for_scene_entry`]
    /// rewrites only those six axes and this constant's `H` is the field value
    /// the register otherwise holds (`512`), not `0`. A glide beat that names
    /// slot `9` starts from that value; seeding `0` made the first frames of
    /// town01's entry glide project through an `H` no retail frame ever had.
    ///
    /// PORT: FUN_80025C24
    pub const FIELD_RESET: Self = Self([0x1B8, 0x64, 0, 0, -256, 16420, 0, 0, 0, 512]);

    /// The axes `FUN_80025C24` writes: pitch, yaw, roll and the eye trio.
    pub const FIELD_RESET_AXES: [usize; 6] = [0, 1, 2, 3, 4, 5];

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
    /// Roll in radians - op-`0x45` slot `2` (`_DAT_8007B794`, the
    /// `RotMatrixZ` angle `FUN_8001CF50` composes third, after pitch and
    /// yaw).
    ///
    /// Retail authors this. An executing census of every MAN record on the
    /// disc (`crates/engine-core/tests/thread_camera_roll_execution.rs`)
    /// finds control-flow-reachable Configure beats staging a non-zero roll
    /// in eight scenes, from a `10`-unit (0.9 deg) tilt up to `-660`
    /// (-58 deg) - all in-range 12-bit angles, each held across the beats of
    /// one shot the way an authored Dutch angle is. A camera that composes
    /// pitch and yaw only frames those shots wrong.
    ///
    /// REF: FUN_8001CF50
    pub roll: f32,
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
    /// The zone-driven follow camera: the retail camera parameter block,
    /// the target it composes and the ease that walks the globals toward
    /// it. See [`crate::camera_zone`] and [`Self::zone_follow_tick`].
    pub zone: ZoneFollow,
}

/// The state of the **zone-driven field follow camera** - the engine side
/// of retail's camera parameter block (`0x8007B606..`), staging descriptor
/// (`0x801F3580`) and the per-frame ease that runs between them.
///
/// Lives on [`Camera`] so both hosts get one camera: the native window and
/// the browser play page each read the composed globals through
/// [`crate::camera_view::field_follow_view`].
#[derive(Debug, Clone)]
pub struct ZoneFollow {
    /// The camera parameter block (`0x8007B607..0x8007B627`).
    pub config: crate::camera_zone::CameraZoneConfig,
    /// The last composed target (the staging descriptor's camera fields).
    pub target: crate::camera_zone::CameraTarget,
    /// The camera-region record the block was last loaded from; `None`
    /// when it holds the zone-miss defaults (or the boot zeros).
    pub loaded_record: Option<[u8; crate::field_regions::ZONE_RECORD_STRIDE]>,
    /// The walk-region attribute box the last query latched
    /// (`0x1F800384..87`), which the composer's sweeps span.
    pub attrs: crate::field_regions::RegionAttributes,
    /// `true` once this scene's follow camera has composed from field
    /// terrain - the gate [`crate::camera_view::field_follow_view`] reads
    /// to prefer the composed globals over its pinned fallback.
    pub active: bool,
    /// The player tile the block was last queried at.
    tile: Option<(i32, i32)>,
    /// The player `(X, footing, Z)` of the previous tick - retail eases
    /// only on a frame the player moved.
    prev_player: Option<[i32; 3]>,
    /// The next tick copies the target straight into the globals (the
    /// arrival actor's `FUN_801DB8EC`), instead of easing.
    snap_pending: bool,
    /// The next tick re-runs the zone query even on the same tile.
    reload_pending: bool,
    /// The op-`0x43` ramp register values already folded into the block,
    /// so a ramp write is recognised as a change rather than re-applied.
    ramp_seen: [i32; 4],
    /// Whether a script owned the camera on the previous tick - the
    /// hand-back edge snaps (see [`Camera::tick_globals`]).
    prev_scripted: bool,
    /// Camera-zone arms the field VM ran, moved off the world by
    /// [`Camera::route_camera_events`] and applied by the next
    /// [`Camera::zone_follow_tick`]. See
    /// [`crate::world::camera_hooks`].
    pending: Vec<crate::world::CameraZoneRequest>,
    /// The camera's **visible tile window** (`0x1F8003E8..EB`, signed
    /// tiles) as the focus edge clamp reads it. Seeded to the field default
    /// and overwritten by a camera-region record's mask-kind side-write -
    /// the four bytes [`crate::camera_zone::CameraZoneConfig::load_record`]
    /// returns.
    pub view_window: [i8; 4],
}

impl Default for ZoneFollow {
    fn default() -> Self {
        Self {
            config: crate::camera_zone::CameraZoneConfig::BOOT,
            target: crate::camera_zone::CameraTarget::default(),
            loaded_record: None,
            attrs: crate::field_regions::RegionAttributes::DEFAULT_FILL,
            active: false,
            tile: None,
            prev_player: None,
            snap_pending: false,
            reload_pending: true,
            ramp_seen: crate::register_ramp::CameraRegisterFile::DEFAULTS,
            prev_scripted: false,
            pending: Vec::new(),
            view_window: {
                let (a, b, c, d) = crate::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
                [a, b, c, d]
            },
        }
    }
}

impl ZoneFollow {
    /// Load one 18-byte camera-region record into the block (the op-`0x45`
    /// LOAD arm, `FUN_801DBC20(operand + 1)`).
    pub fn load_record(&mut self, rec: &[u8; crate::field_regions::ZONE_RECORD_STRIDE]) {
        if let Some(w) = self.config.load_record(rec) {
            self.view_window = w.map(|b| b as i8);
        }
        self.loaded_record = Some(*rec);
        self.ramp_seen = crate::register_ramp::CameraRegisterFile::DEFAULTS;
    }

    /// Arm a scene-entry / arrival snap: re-query the tile and copy the
    /// composed target into the globals on the next tick.
    pub fn arm_arrival(&mut self) {
        self.snap_pending = true;
        self.reload_pending = true;
        self.tile = None;
        self.prev_player = None;
        self.active = false;
    }
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
            roll: 0.0,
            manual_orbit: 0.0,
            render_yaw_bias: 0.0,
            distance: CameraDistance::Retail,
            motion_state: MotionState::default(),
            motion_target: MotionTarget::default(),
            globals: RetailCamGlobals::default(),
            mover: None,
            last_field_frame: 0,
            script_owns_focus: false,
            zone: ZoneFollow::default(),
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
        // The camera-zone arms of op `0x4C` (nibble-3 sub-8/9/D/E and
        // nibble-C sub-4) queue on the world because the field VM's host is
        // `World` while these globals live here. Both hosts call this right
        // before `tick`, so this is the one drain point.
        self.zone.pending.extend(world.take_camera_zone_requests());
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
                    // Slot 2 = roll (`_DAT_8007B794`, the `RotMatrixZ` angle).
                    // Retail authors it: see [`Self::roll`] for the executing
                    // census that found the eight scenes staging one.
                    if let Some(v) = slot(2) {
                        self.roll = ang(v);
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
                    // Retail's op-`0x45` arms compose the zone camera into
                    // the same staging struct the beat's slots overwrite
                    // (`FUN_801DAB90(player, 0x801C6EA8)` at `0x801DF228`),
                    // so an unmasked slot holds the scene's own shot, and a
                    // glide starts from it. Snap the zone pose in first if
                    // this scene has not had its arrival snap yet.
                    self.prime_zone_before_script(world);
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
                    // Op-`0x45` LOAD: the 18 bytes after the op byte are one
                    // camera-region record, and retail hands them straight to
                    // the camera-config loader (`FUN_801DBC20(operand + 1)`
                    // at `0x801DF28C`). It is not an eye / look-at pair and
                    // it does not seize the camera: the follow camera keeps
                    // running and eases toward the shot the new block
                    // composes.
                    if let Ok(rec) = <[u8; crate::field_regions::ZONE_RECORD_STRIDE]>::try_from(
                        payload.as_slice(),
                    ) {
                        self.zone.load_record(&rec);
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
    /// `apply` a duration in 60 Hz frames. `World::clock.display_frames` is the
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
        let now = world.clock.display_frames;
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

        // The zone-driven follow camera: retail's per-scene / per-tile camera
        // parameters composed into the same ten globals, then eased. It runs
        // only in a field scene with terrain loaded (the zone table and the
        // walk-region table are what it queries) and only while nothing
        // scripted owns the shot.
        let zone_scene = world.mode == crate::world::SceneMode::Field && has_field_terrain(world);
        if zone_scene {
            // A scripted shot handing the camera back snaps. Retail's
            // scripts do this themselves through the `[4C 39]` / `[4C 3E]`
            // arms (`FUN_801DB8EC`), which is how every walkable
            // post-opening state in the library holds a settled follow pose
            // (live == staging) rather than an ease in flight from the
            // cinematic shot. Those arms are wired now
            // ([`crate::world::CameraZoneRequest`]); the hand-back edge
            // stays as the port's backstop for a shot the script drops
            // without one, and it snaps from the resident block rather than
            // re-querying (retail's hand-back does not re-query either).
            if self.zone.prev_scripted && !scripted {
                self.zone.snap_pending = true;
                self.zone.prev_player = None;
            }
            self.zone.prev_scripted = scripted;
            if !scripted && self.mode == CameraMode::Follow {
                self.zone_follow_tick(world, dt);
            } else if gliding && self.mode == CameraMode::Follow {
                // A scripted glide in a free-roam scene drives these same
                // globals from the zone pose it was primed with; the follow
                // view keeps reading them rather than its fallback.
                self.zone.active = !self.zone.snap_pending;
            }
            return;
        }

        // A world without field terrain (unit worlds, the ramp oracles):
        // camera-register zone ramps (field-VM op `0x43` sub-3..6) own four
        // of these ten axes while the player stands in an authored zone. The
        // field-overlay camera composer reads all four straight into the same
        // camera descriptor these globals mirror - see
        // [`crate::register_ramp::RampSlot::camera_axis`] for the per-register
        // store sites. (In a terrain-bearing scene the same four registers
        // are folded into the parameter block instead, which is where retail
        // keeps them - see [`Self::zone_follow_tick`].)
        //
        // Gated on a register having actually been written: with no ramp in
        // the scene the file still holds `CAMERA_ZONE_DEFAULTS`, whose
        // `0x4000` eye-back is *not* the `16420` this camera resets to, so an
        // ungated feed would re-frame every ramp-free scene. A scripted glide
        // still wins - it is the shot the script staged.
        // REF: FUN_801DABA4 (the field-overlay camera composer)
        if !gliding && world.camera.registers.written() {
            for slot in crate::register_ramp::RampSlot::ALL {
                let axis = slot.camera_axis();
                let v = world.camera.registers.get(slot);
                // Retail's eye-back store is a halfword whose sign the
                // composer folds into the yaw (it picks which side of the
                // player the orbit sits on), so the depth axis takes the
                // magnitude; the port's follow camera has no side flip.
                self.globals.0[axis] = if axis == 5 { v.abs() } else { v };
            }
        }
    }

    /// One frame of the **zone-driven follow camera** - the engine side of
    /// retail's `FUN_801DE3E0` (tile query + load), `FUN_801DAB90`
    /// (compose), `FUN_801DB510` (ease) and `FUN_801DB8EC` (snap), all in
    /// [`crate::camera_zone`].
    ///
    /// Retail's query is **script-driven**, and so is the port's: the four
    /// `0x4C` arms ([`crate::world::CameraZoneRequest`], queued by the field
    /// VM and drained in [`Self::route_camera_events`]), the op-`0x45` LOAD
    /// record, and the player-seat path - which retail runs in code at
    /// `0x801D1FE8..0x801D2014` as exactly the `[4C 39]` sequence, and which
    /// the port arms as [`ZoneFollow::arm_arrival`]. On top of those there
    /// is retail's **per-frame** re-query at `0x801D17FC..0x801D1830`, gated
    /// on scratchpad flag bit `22` ([`crate::world::ZONE_REQUERY_FLAG`]);
    /// with the bit clear - its state on every mode entry, and in 109 of the
    /// disc's 124 CDNAME scenes - the block simply stays put while the
    /// player walks. The four op-`0x43` ramp registers are folded into the
    /// block as they change, exactly the cells retail's ramps write.
    ///
    /// The composer's floor sample reads the **static** elevation LUT
    /// ([`World::sample_field_floor_height_static`]): `FUN_801DAB90`
    /// swaps the MAN's own ladder (`*(_DAT_8007B898) + 2`, 16 negated
    /// `short`s) into scratchpad `0x1F80035C` around its `FUN_80019278`
    /// call and restores the live rungs after, so a scripted floor-tier bob
    /// never shakes the camera.
    ///
    /// PORT: FUN_801DE3E0
    /// REF: FUN_801DAB90, FUN_801DB510, FUN_801DB8EC, FUN_801DBE9C, FUN_801D1344
    fn zone_follow_tick(&mut self, world: &World, dt: i32) {
        use crate::camera_zone::{ComposeInputs, compose, ease_step, ease_step_i16, snap};
        use crate::field_regions::{RegionTable, refresh_region_attributes, zone_query};
        use crate::register_ramp::{CameraRegisterFile, RampSlot};

        let Some(a) = world
            .player_actor_slot
            .and_then(|s| world.actors.get(s as usize))
        else {
            return;
        };
        let (x, y, z) = (
            i32::from(a.move_state.world_x),
            i32::from(a.move_state.world_y),
            i32::from(a.move_state.world_z),
        );
        // The player's tile in the two conventions retail uses: the seat
        // path and every `0x4C` arm take `(coord - 0x40) >> 7` (the exact
        // inverse of the tile-centre seat `tile * 0x80 + 0x40`), while the
        // per-frame re-query at `0x801D1804..0x801D181C` takes
        // `(coord + 0x40) >> 7` - one tile further on. Both are reproduced
        // rather than unified, because they select different records along a
        // region edge.
        let tile = ((x - 0x40) >> 7, (z - 0x40) >> 7);
        let frame_tile = ((x + 0x40) >> 7, (z + 0x40) >> 7);
        let requery_per_frame = world.camera_zone_requery_per_frame();
        let pending = std::mem::take(&mut self.zone.pending);
        let zone = &mut self.zone;

        // 1. The attribute box. Retail latches it in `FUN_800180EC`, which
        //    runs from the sub-area rebuild sweep `FUN_80017DD4` (and from
        //    the `[4C 3D]` arm); the port refreshes it whenever the player
        //    changes tile, which is the same box on every library state.
        let table = RegionTable::parse(&world.terrain.map_region_block);
        if zone.tile != Some(tile) || zone.reload_pending {
            let (_, attrs) = refresh_region_attributes(table.as_ref(), tile.0, tile.1, false);
            zone.attrs = attrs;
            zone.tile = Some(tile);
        }

        // 2. The zone query + block load (`FUN_801DE3E0`). Never on a bare
        //    tile crossing: only on the seat / arrival, on a queued script
        //    arm, or while the per-frame re-query flag is raised.
        let load_at = |zone: &mut ZoneFollow, tx: i32, tz: i32| {
            let attrs = zone.attrs;
            let hit = zone_query(&world.terrain.zone_table, table.as_ref(), &attrs, tx, tz)
                .and_then(|r| r.record)
                .and_then(|r| <[u8; crate::field_regions::ZONE_RECORD_STRIDE]>::try_from(r).ok());
            match hit {
                Some(rec) => zone.load_record(&rec),
                None => {
                    zone.config.load_zone_miss();
                    zone.loaded_record = None;
                    zone.ramp_seen = CameraRegisterFile::DEFAULTS;
                }
            }
        };
        if zone.reload_pending {
            load_at(zone, tile.0, tile.1);
            zone.reload_pending = false;
        } else if requery_per_frame {
            load_at(zone, frame_tile.0, frame_tile.1);
        }
        for req in pending {
            use crate::world::CameraZoneRequest as R;
            match req {
                R::QueryAtPlayer => load_at(zone, tile.0, tile.1),
                R::QueryAtTile { x: tx, z: tz } => load_at(zone, i32::from(tx), i32::from(tz)),
                R::QueryConformAndSnap => {
                    load_at(zone, tile.0, tile.1);
                    zone.snap_pending = true;
                }
                R::SnapAndClamp => zone.snap_pending = true,
                R::RefreshAttributes => {
                    let (_, a) = refresh_region_attributes(table.as_ref(), tile.0, tile.1, false);
                    zone.attrs = a;
                }
            }
        }

        // 3. Ramp registers (`FUN_80037018` stores through `+0x94`) land in
        //    the block's own cells.
        if world.camera.registers.written() {
            for slot in RampSlot::ALL {
                let v = world.camera.registers.get(slot);
                if v != zone.ramp_seen[slot.index()] {
                    zone.ramp_seen[slot.index()] = v;
                    match slot {
                        RampSlot::Dat8007B60C => zone.config.pitch = v,
                        RampSlot::Dat8007B610 => zone.config.yaw = v,
                        RampSlot::Dat8007B614 => zone.config.depth = v,
                        RampSlot::Dat8007B618 => zone.config.h = v,
                    }
                }
            }
        }

        // 4. Compose (`FUN_801DAB90`).
        let inputs = ComposeInputs {
            player: [x, y, z],
            // The static ladder, not the live one: the composer swaps the
            // MAN's own rungs in around its floor sample.
            floor_y: world.sample_field_floor_height_static(x, z),
            attr_box: zone.attrs.box_bytes,
            live_pitch: self.globals.0[0],
            live_yaw: self.globals.0[1],
            half_eye_y: world.party.scene_save_allowed,
        };
        let composed = compose(&zone.config, &inputs);
        if let Some(ly) = composed.live_yaw {
            self.globals.0[1] = i32::from(ly);
        }
        zone.target = composed.target;

        // 5. Snap on arrival (`FUN_801DB8EC`), else ease on a frame the
        //    player moved (`FUN_801DB510`'s settle test).
        let moved = zone.prev_player != Some([x, y, z]);
        zone.prev_player = Some([x, y, z]);
        let mode5 = zone.config.mode_nibble() == 5;
        let focus_anchor = [
            -(zone.config.anchor_x << 7) - 0x40,
            -(zone.config.anchor_z << 7) - 0x40,
        ];
        let t = zone.target;
        let g = &mut self.globals.0;
        if zone.snap_pending {
            let (p, yw, eye, h) = snap(&t);
            g[0] = p;
            g[1] = yw;
            g[3..6].copy_from_slice(&eye);
            g[9] = h;
            if mode5 {
                g[6] = focus_anchor[0];
                g[8] = focus_anchor[1];
            }
            zone.snap_pending = false;
        } else if moved || !zone.active {
            let code = zone.config.ease_shift();
            for _ in 0..dt.clamp(1, 8) {
                g[0] = i32::from(ease_step_i16(g[0] as i16, t.pitch, code));
                g[1] = i32::from(ease_step_i16(g[1] as i16, t.yaw, code));
                g[9] = i32::from(ease_step_i16(g[9] as i16, t.h, code));
                for (axis, e) in t.eye.iter().enumerate() {
                    g[3 + axis] = ease_step(g[3 + axis], i32::from(*e), code);
                }
                if mode5 {
                    g[6] = ease_step(g[6], focus_anchor[0], code);
                    g[8] = ease_step(g[8], focus_anchor[1], code);
                }
            }
        }

        // 6. The focus edge clamp (`FUN_801DAA50`), which every retail
        //    caller of the ease and the snap runs immediately after them.
        //    Keeps the focus inside the latched walk region widened by the
        //    camera's visible-tile window, so the lens never pans past a
        //    room's edge. The script focus override (`_DAT_8007B628` /
        //    `_DAT_8007B62A`) has no port-side writer yet, so it is passed
        //    as "unset".
        let clamped = crate::camera_zone::clamp_focus(
            [g[6], g[8]],
            zone.config.mode_nibble(),
            zone.attrs.kind != 0,
            zone.attrs.box_bytes,
            zone.view_window,
            world.party.scene_save_allowed,
            [0, 0],
        );
        g[6] = clamped[0];
        g[8] = clamped[1];
        zone.active = true;
    }

    /// Run the arrival snap now if it is still pending in a terrain-bearing
    /// free-roam scene, so a scripted beat that is about to capture the live
    /// globals sees the zone camera's pose rather than the field reset.
    fn prime_zone_before_script(&mut self, world: &World) {
        if self.zone.snap_pending
            && self.mode == CameraMode::Follow
            && world.mode == crate::world::SceneMode::Field
            && has_field_terrain(world)
        {
            self.zone_follow_tick(world, 1);
        }
    }

    /// The composed follow yaw, PSX 12-bit units (sign-extended from the
    /// live halfword), or `None` while the zone camera is not driving the
    /// frame.
    pub fn zone_follow_yaw_units(&self) -> Option<i32> {
        self.zone
            .active
            .then_some(i32::from(self.globals.0[1] as i16))
    }

    /// Reset the retail camera globals to their field-entry values and drop
    /// any glide in flight - the engine side of `FUN_80025C24`, called when a
    /// scene is entered so a previous scene's shot can't leak into the next.
    ///
    /// PORT: FUN_80025C24
    pub fn reset_globals_for_scene_entry(&mut self) {
        // Six stores in retail: the three angles and the eye trio. The focus
        // trio is re-pinned to the player by the follow camera on the next
        // frame and `H` keeps the register's live value, so a scene whose
        // entry beat glides slot `9` starts that glide from the `H` the
        // player was just looking through, as retail does.
        for axis in RetailCamGlobals::FIELD_RESET_AXES {
            self.globals.0[axis] = RetailCamGlobals::FIELD_RESET.0[axis];
        }
        if self.globals.0[9] == 0 {
            self.globals.0[9] = RetailCamGlobals::FIELD_RESET.0[9];
        }
        self.mover = None;
        self.script_owns_focus = false;
        // The arrival actor (`FUN_801DBE9C`) re-pins the focus and snaps the
        // composed shot in on the first frame of the new scene.
        self.zone.arm_arrival();
    }

    /// The camera azimuth to feed
    /// [`crate::world::FieldLocomotion::camera_azimuth`](crate::world::FieldLocomotion::camera_azimuth)
    /// this frame, in PSX 12-bit units (`4096` = full turn): scripted yaw +
    /// the user's manual orbit + the host renderer's fixed framing bias.
    /// This is what keeps the d-pad -> world-direction remap ("screen up
    /// walks away from the camera") tracking the yaw the player actually
    /// sees, including after a drag-orbit. All three terms default to `0`,
    /// so headless hosts keep the historical `yaw`-only feed bit-identical.
    pub fn compass_azimuth_units(&self) -> u16 {
        // A host that renders the retail follow view declares it through a
        // non-zero `render_yaw_bias`; while the zone camera composes that
        // view, the bias it actually sees is the live follow yaw's negation
        // (compass sense = `-psi`), not the pinned anchor value.
        let bias = match self.zone_follow_yaw_units() {
            Some(yaw) if self.render_yaw_bias != 0.0 => {
                -(yaw as f32) / 4096.0 * std::f32::consts::TAU
            }
            _ => self.render_yaw_bias,
        };
        let az = (self.yaw + self.manual_orbit + bias) / std::f32::consts::TAU * 4096.0;
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
    /// [`crate::world::FieldLocomotion::camera_azimuth`](crate::world::FieldLocomotion::camera_azimuth)
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
            self.roll = 0.0;
        }
    }
}

/// Whether a world carries the per-scene field terrain the zone camera
/// queries - the walk-region table, the MAN section-3 zone table, or the
/// collision grid the floor sampler reads.
fn has_field_terrain(world: &World) -> bool {
    !world.terrain.zone_table.is_empty()
        || !world.terrain.map_region_block.is_empty()
        || !world.terrain.collision_grid.is_empty()
}

/// The default camera-zone parameter set the tile re-query `FUN_801DE3E0`
/// installs when no camera-region record covers the player's tile (the
/// same nine stores sit in `FUN_801DBE9C`'s dev-only query leg). Raw values
/// and the `0x8007B607..` globals they land in; the typed form is
/// [`crate::camera_zone::CameraZoneConfig::ZONE_MISS`].
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
/// query arm (the `_DAT_8007B868 != 0` leg). `_DAT_8007B868` is the
/// dev/dual-mode gate and retail boots with it `0`, so **retail never runs
/// this arm**: its `== 0` leg counts the same `+0x54` countdown down, then
/// re-pins the focus to the player and snaps the composed shot in through
/// `FUN_801DB8EC` without touching the parameter block. The retail-side
/// zone query is the script-driven `FUN_801DE3E0` ([`crate::camera_zone`]).
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
/// NOT WIRED: this is the dev-only leg. The engine's arrival snap is
/// [`ZoneFollow::arm_arrival`] (armed by
/// [`Camera::reset_globals_for_scene_entry`], consumed by the zone follow
/// tick), which is the retail leg's job; the tile query itself runs from
/// the same tick through [`crate::field_regions::zone_query`]. There is no
/// per-actor `+0x54` countdown and no `_DAT_8007B868` dev word in the
/// engine, so nothing selects this leg.
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

    /// Slot 2 is the roll angle, and retail authors it - `juui2`'s opening
    /// beat stages `-660` units (-58 deg) alongside pitch, yaw, the eye trio,
    /// focus X/Z and H. The controller surfaces it as [`Camera::roll`]
    /// alongside pitch and yaw (the render hosts decode the staged slot
    /// themselves, the same way they do for those two), and a camera that
    /// drops the term frames that shot upright.
    #[test]
    fn camera_configure_slot_two_sets_the_roll() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World::default();
        let p = |slot: u8, value: i16| CameraParam {
            slot,
            value: value as u16,
        };
        // The `juui2` P2[0] beat, verbatim (entry 597, pc 0x000A).
        w.pending_field_events = vec![FieldEvent::CameraConfigure {
            params: vec![
                p(0, -643),
                p(1, -1480),
                p(2, -660),
                p(3, -19),
                p(4, 521),
                p(5, 4537),
                p(6, -3522),
                p(8, -13904),
                p(9, 280),
            ],
            apply_trigger: 0,
            mode: 0,
        }];
        let mut c = Camera::default();
        c.route_camera_events(&mut w);
        assert_eq!(c.globals.angles(), [-643, -1480, -660], "all three angles");
        let want = -660.0 * std::f32::consts::TAU / 4096.0;
        assert!(
            (c.roll - want).abs() < 1e-4,
            "slot 2 -> Camera::roll: {} vs {want}",
            c.roll
        );
        // Free-roam clears it with the rest of the scripted pose, so a rolled
        // cutscene cannot leave the field camera tilted.
        let field = World {
            mode: crate::world::SceneMode::Field,
            ..World::default()
        };
        c.reset_for_free_roam(&field);
        assert_eq!(c.roll, 0.0);
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
        w.clock.display_frames = 50;
        c.tick(&w);
        let mid = c.globals.tr_eye()[2];
        assert!(
            (16420..17420).contains(&mid),
            "midpoint {mid} interpolates between start and target"
        );

        // Run out the duration: exact arrival, and the one-shot mover retires.
        w.clock.display_frames = 100;
        c.tick(&w);
        assert_eq!(c.globals.tr_eye()[2], 17420, "glide arrives exactly");
        assert!(c.mover.is_none(), "the mover is one-shot");
    }

    /// Scene entry restores the `FUN_80025C24` field defaults so a departing
    /// scene's shot cannot leak into the next one - the six axes retail
    /// writes. Focus and `H` are the scene's to establish, so they survive.
    #[test]
    fn scene_entry_resets_globals_to_field_defaults() {
        let mut c = Camera {
            globals: RetailCamGlobals([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            ..Default::default()
        };
        c.reset_globals_for_scene_entry();
        assert_eq!(c.globals.angles(), [0x1B8, 0x64, 0]);
        assert_eq!(c.globals.tr_eye(), [0, -256, 16420]);
        assert_eq!(c.globals.0[6..=8], [7, 8, 9], "focus is not a reset axis");
        assert_eq!(c.globals.h(), 10, "H keeps the register's live value");
    }

    /// A camera that never carried an `H` (the boot default, or a headless
    /// host) gets the field register value, never `0`: a glide beat naming
    /// slot `9` must start from the `H` retail was projecting through.
    #[test]
    fn scene_entry_seeds_a_missing_h_with_the_field_value() {
        let mut c = Camera {
            globals: RetailCamGlobals([0; AXIS_COUNT]),
            ..Default::default()
        };
        c.reset_globals_for_scene_entry();
        assert_eq!(c.globals.h(), 512);
        assert_eq!(
            Camera::new().globals.h(),
            512,
            "the boot default carries it too"
        );
    }

    /// Op-`0x45` LOAD carries one 18-byte camera-region record, and the
    /// router hands it to the camera-config loader (`FUN_801DBC20`): the
    /// parameter block takes the record's split, the follow camera keeps
    /// the frame (retail's arm loads and returns - no mode change).
    #[test]
    fn route_camera_load_splits_the_record_into_the_parameter_block() {
        let mut w = World::default();
        let mut payload = vec![0u8; crate::field_regions::ZONE_RECORD_STRIDE];
        payload[5] = 0x1A; // mode 1, strength 0xA
        payload[6] = 0x21;
        payload[10..12].copy_from_slice(&(-160i16).to_le_bytes());
        payload[12..14].copy_from_slice(&0x1B8i16.to_le_bytes());
        payload[16..18].copy_from_slice(&0x200i16.to_le_bytes());
        w.pending_field_events = vec![FieldEvent::CameraLoad {
            payload: payload.clone(),
        }];
        let mut c = Camera::default();
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 1);
        assert_eq!(c.zone.config.mode, 0x1A);
        assert_eq!(c.zone.config.b608, 0x21);
        assert_eq!(c.zone.config.yaw, -160);
        assert_eq!(c.zone.config.pitch, 0x1B8);
        assert_eq!(c.zone.config.h, 0x200);
        assert_eq!(
            c.zone.loaded_record.as_ref().map(|r| r.to_vec()),
            Some(payload)
        );
        assert_eq!(c.mode, CameraMode::Follow, "LOAD does not seize the camera");
        // A short payload is ignored rather than mis-split.
        w.pending_field_events = vec![FieldEvent::CameraLoad {
            payload: vec![0u8; 12],
        }];
        c.route_camera_events(&mut w);
        assert_eq!(c.zone.config.mode, 0x1A);
    }

    /// A terrain-bearing field world drives the zone camera: the globals
    /// leave the field reset for the composed shot, snap in on arrival, and
    /// the compass reports the live yaw's negation on a host that renders
    /// the follow view.
    #[test]
    fn zone_camera_composes_from_terrain_and_snaps_on_arrival() {
        use crate::field_regions::ZONE_RECORD_STRIDE;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.spawn_actor(0);
        w.player_actor_slot = Some(0);
        w.actors[0].move_state.world_x = 0x1040;
        w.actors[0].move_state.world_z = 0x2040;
        // One kind-1 record covering the whole map: mode 1 strength 0,
        // yaw -160, pitch 450, depth 0x3000, H 512.
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 1;
        rec[1..5].copy_from_slice(&[0, 0, 0x7F, 0x7F]);
        rec[5] = 0x10;
        rec[6] = 0x10;
        rec[7] = 0x30;
        rec[8] = 0x00;
        rec[9] = 0x20;
        rec[10..12].copy_from_slice(&(-160i16).to_le_bytes());
        rec[12..14].copy_from_slice(&450i16.to_le_bytes());
        rec[14..16].copy_from_slice(&0x3000i16.to_le_bytes());
        rec[16..18].copy_from_slice(&512i16.to_le_bytes());
        let mut zone_table = vec![1u8];
        zone_table.extend_from_slice(&rec);
        w.load_field_region_tables(&[], &zone_table);

        let mut c = Camera {
            render_yaw_bias: crate::camera_view::retail_field_render_yaw_bias(),
            ..Default::default()
        };
        c.reset_globals_for_scene_entry();
        w.tick();
        c.tick(&w);
        assert!(c.zone.active);
        assert_eq!(c.zone.loaded_record, Some(rec));
        assert_eq!(c.globals.angles()[0], 450, "snapped pitch");
        assert_eq!(c.globals.angles()[1] as i16, -160, "snapped yaw");
        assert_eq!(c.globals.h(), 512);
        assert_eq!(c.globals.tr_eye()[2], 0x3000);
        assert_eq!(c.zone_follow_yaw_units(), Some(-160));
        // Compass = -yaw = +160 units.
        assert_eq!(c.compass_azimuth_units(), 160);

        // Walk one step: the ease keeps the settled pose exactly.
        w.actors[0].move_state.world_x += 2;
        w.tick();
        c.tick(&w);
        assert_eq!(c.globals.angles()[0], 450);
        assert_eq!(c.globals.angles()[1] as i16, -160);

        // A scene with NO covering record composes the miss defaults.
        let mut w2 = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w2.spawn_actor(0);
        w2.player_actor_slot = Some(0);
        w2.load_field_region_tables(&[], &[0u8]);
        let mut c2 = Camera::default();
        c2.reset_globals_for_scene_entry();
        w2.tick();
        c2.tick(&w2);
        assert_eq!(c2.zone.loaded_record, None);
        assert_eq!(
            c2.zone.config,
            crate::camera_zone::CameraZoneConfig::ZONE_MISS
        );
        assert_eq!(c2.globals.angles()[0], 0x1B8);
        assert_eq!(c2.globals.angles()[1], 0);
        assert_eq!(c2.globals.h(), 0x300);
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
