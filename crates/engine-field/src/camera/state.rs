//! `Camera` and `ZoneFollow`: the state structs, the distance / mode enums, defaults and construction.

use super::*;

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
    /// Roll in radians - op-`0x45` slot `2` (`_DAT_8007B794`, the Z angle
    /// the camera's Euler kernel `FUN_80026988` composes third, after pitch
    /// and yaw, when `FUN_800172C0` builds the view).
    ///
    /// Retail authors this. An executing census of every MAN record on the
    /// disc (`crates/engine-core/tests/thread_camera_roll_execution.rs`)
    /// finds control-flow-reachable Configure beats staging a non-zero roll
    /// in eight scenes, from a `10`-unit (0.9 deg) tilt up to `-660`
    /// (-58 deg) - all in-range 12-bit angles, each held across the beats of
    /// one shot the way an authored Dutch angle is. A camera that composes
    /// pitch and yaw only frames those shots wrong.
    ///
    /// REF: FUN_800172C0, FUN_80026988
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
    /// User-controlled **tilt** on the follow camera (radians), added to the
    /// pitch the scene composes: positive tips the lens further down toward
    /// a top-down survey, negative brings it toward the horizon. The
    /// composed pitch clamps into [`follow_knobs::PITCH_MIN`] ..
    /// [`follow_knobs::PITCH_MAX`] (widened to include the scene's own pitch
    /// so an untouched tilt never moves a retail shot). Pure framing: the
    /// compass reads yaw only, so a tilt never remaps the d-pad. Preserved
    /// across [`Self::reset_for_free_roam`] like the orbit. Default `0.0`.
    pub manual_tilt: f32,
    /// User-controlled **continuous zoom** on the follow camera: a
    /// multiplier on the eye-back depth, composed under the coarse
    /// [`Self::distance`] preset. `> 1` pulls the eye back, `< 1` brings
    /// it in, clamped to [`follow_knobs::ZOOM_MIN`] ..
    /// [`follow_knobs::ZOOM_MAX`]. Pure framing, never a simulation
    /// input. Preserved across [`Self::reset_for_free_roam`]. Default `1.0`.
    pub manual_zoom: f32,
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
    pub(super) last_field_frame: u64,
    /// Latched once this scene has executed an op-`0x45` Configure: from then
    /// on the script owns the focus globals and the follow camera stops
    /// writing them. A scripted scene seizes the camera in retail, and the
    /// focus it stages is meant to survive the settled gaps between beats -
    /// retail holds two distinct focus values across the whole of `opdeene`.
    /// Cleared by [`Self::reset_globals_for_scene_entry`].
    pub(super) script_owns_focus: bool,
    /// The zone-driven follow camera: the retail camera parameter block,
    /// the target it composes and the ease that walks the globals toward
    /// it. See [`crate::camera_zone`] and [`Self::zone_follow_tick`].
    pub zone: ZoneFollow,
    /// This frame's `apply_trigger == 0` Configure beats, as packed-component
    /// snaps for the host's [`legaia_engine_camera::psx_camera::CutsceneCameraInterp`].
    ///
    /// Retail's mover snaps the live camera globals to an `apply == 0` beat
    /// immediately. The field VM runs until yield, so a snap beat followed by
    /// a glide beat in the same tick (`map01`'s fly-in: the aerial snap, then
    /// the descent with no yield between) commits both before any host looks,
    /// and the merged [`Self::globals`] only carries the glide beat's
    /// targets, so a host that reads the merged state alone glides from the
    /// wrong pose.
    ///
    /// Filled by [`Self::route_camera_events`] and drained by
    /// [`Self::take_camera_snap_beats`]. It lives here because this is where
    /// the beats are: `route_camera_events` consumes every
    /// `FieldEvent::CameraConfigure` off the world queue and does not restore
    /// it, so a host looking for them in its own later drain of
    /// `pending_field_events` finds none.
    pub camera_snap_beats: Vec<Vec<(u8, u16)>>,
}

/// The state of the **zone-driven field follow camera** - the engine side
/// of retail's camera parameter block (`0x8007B606..`), staging descriptor
/// (`0x801F3580`) and the per-frame ease that runs between them.
///
/// Lives on [`Camera`] so both hosts get one camera: the native window and
/// the browser play page each read the composed globals through
/// `legaia_engine_world_views::camera_view::field_follow_view`.
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
    /// terrain - the gate `legaia_engine_world_views::camera_view::field_follow_view` reads
    /// to prefer the composed globals over its pinned fallback.
    pub active: bool,
    /// The player tile the block was last queried at.
    pub(super) tile: Option<(i32, i32)>,
    /// The player `(X, footing, Z)` of the previous tick - retail eases
    /// only on a frame the player moved.
    pub(super) prev_player: Option<[i32; 3]>,
    /// The next tick copies the target straight into the globals (the
    /// arrival actor's `FUN_801DB8EC`), instead of easing. Public only so
    /// engine-core's `World`-driven camera tests can stage an arrival.
    #[doc(hidden)]
    pub snap_pending: bool,
    /// The next tick re-runs the zone query even on the same tile.
    pub(super) reload_pending: bool,
    /// The op-`0x43` ramp register values already folded into the block,
    /// so a ramp write is recognised as a change rather than re-applied.
    pub(super) ramp_seen: [i32; 4],
    /// Whether a script owned the camera on the previous tick - the
    /// hand-back edge snaps (see [`Camera::tick_globals`]).
    pub(super) prev_scripted: bool,
    /// Camera-zone arms the field VM ran, moved off the world by
    /// [`Camera::route_camera_events`] and applied by the next
    /// [`Camera::zone_follow_tick`]. See
    /// `legaia_engine_core::world::camera_hooks`.
    pub(super) pending: Vec<CameraZoneRequest>,
    /// The camera's **visible tile window** (`0x1F8003E8..EB`, signed
    /// tiles) as the focus edge clamp reads it. Seeded to the field default
    /// and overwritten by a camera-region record's mask-kind side-write -
    /// the four bytes [`crate::camera_zone::CameraZoneConfig::load_record`]
    /// returns. Seeded per scene entry from
    /// [`legaia_engine_system::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW`], replaced by a
    /// camera-region record's mask-kind side-write
    /// ([`Self::load_record`]) and by field-VM op `0x46`
    /// ([`Camera::route_camera_events`]) - retail's order is seed, then
    /// whichever of those the scene's script runs.
    pub view_window: [i8; 4],
    /// The jitter pair the follow ease last added into the eye X / Y
    /// globals (retail's scene control block `+0x18` / `+0x1C`,
    /// `*(0x801C6EA4)`), subtracted back out at the top of the next ease.
    pub shake_offset: [i32; 2],
    /// The `rand()` state the follow ease's shake draws from. Retail draws
    /// from the one process-wide BIOS seed, so [`Camera::tick_on_stream`]
    /// lends `World::rng_state` in here and takes it back after the tick;
    /// only a bare [`Camera::tick`] (a test, a preview) draws on this copy
    /// alone. A rest shake (`amplitude == 0`) draws nothing.
    pub(super) shake_seed: u32,
    /// The focus pair (`_DAT_80089118` / `_DAT_80089120`) this follow
    /// camera left at the end of its previous tick. A mode-5 fixed shot
    /// eases its focus from here - retail's `FUN_801DB510` pins the focus
    /// onto the player (`0x801DB820`) only for the other modes, and a
    /// stationary mode-5 frame leaves it where it was.
    pub(super) focus_held: Option<[i32; 2]>,
    /// The follow switch `_DAT_8007B606` (byte): off, `FUN_801DB510` only
    /// pins the focus onto the player and neither composes nor eases
    /// (`0x801DB550`). New-game init stores `1` (SCUS `0x80034B60`, the
    /// `_DAT_8007B868 == 0` test, which is true on retail); its only other
    /// writers are the dev menu's CAMERA row (`0x801EAD38`, `0x801EA1C8`),
    /// which the port's dev menu carries as
    /// `legaia_engine_menus::dev_menu_host::DevMenuRow::Camera`.
    pub follow_enabled: bool,
    /// A focus pair (stored form) the next snap lands instead of the
    /// player's - [`Self::seat_focus_after_snap`].
    pub(super) seat_focus: Option<[i32; 2]>,
    /// An eye translation the next snap lands instead of the composed one
    /// - [`Self::seat_eye_after_snap`].
    pub(super) seat_eye: Option<[i32; 3]>,
    /// A pitch / yaw / `H` the next snap lands instead of the composed
    /// ones - [`Self::seat_angles_after_snap`].
    pub(super) seat_angles: Option<[i32; 3]>,
    /// What the seat's snap landed ([`SeatHold`]), kept while the player
    /// stands on the seat.
    pub(super) seat_hold: Option<SeatHold>,
}

/// The seat's landed focus / eye, and the pose its snap composed there.
///
/// A seat lands a retail state's focus and eye over the snap because both
/// are follow-ease history the seat cannot replay. A later snap composing
/// the same target at the same spot re-lands them rather than undoing the
/// seat: the entry script's first pass runs on the ticks after the seat, and
/// a region selector arm that ends on `4C 39` (`town01` `P1[0]` `+0x1A4`)
/// snaps there, where retail ran it on the player's arrival, before the walk
/// that left the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SeatHold {
    pub(super) player: [i32; 3],
    pub(super) target: crate::camera_zone::CameraTarget,
    pub(super) focus: Option<[i32; 2]>,
    pub(super) eye: Option<[i32; 3]>,
    pub(super) angles: Option<[i32; 3]>,
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
                let (a, b, c, d) = legaia_engine_system::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
                [a, b, c, d]
            },
            shake_offset: [0, 0],
            shake_seed: 1,
            focus_held: None,
            follow_enabled: true,
            seat_focus: None,
            seat_eye: None,
            seat_angles: None,
            seat_hold: None,
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

    /// Copy the composed target straight into the globals on the next tick
    /// instead of easing - `FUN_801DB8EC`'s snap, which the dev menu's
    /// `CAMERA` row runs when it switches the follow camera on
    /// (`0x801EA1E4`). Unlike [`Self::arm_arrival`] it keeps the tile and the
    /// latched region, so it re-frames without re-querying the zone.
    pub fn request_snap(&mut self) {
        self.snap_pending = true;
    }

    /// [`Self::arm_arrival`] over a given parameter block instead of the
    /// tile re-query: the snap composes from `config`. For a seat that
    /// carries a retail state's own block (`0x8007B607..0x8007B627`): the
    /// block holds whichever camera-region record a script or a walk-on
    /// loader last ran (`kor5` P2[0] / P2[1], the band at tile X 26 / 28),
    /// which is walk history a seat cannot replay.
    pub fn arm_arrival_over(&mut self, config: crate::camera_zone::CameraZoneConfig) {
        self.arm_arrival();
        self.config = config;
        self.loaded_record = None;
        self.ramp_seen = crate::register_ramp::CameraRegisterFile::DEFAULTS;
        self.reload_pending = false;
    }

    /// Land `focus` (stored form, X / Z negated) as the focus of the next
    /// snap, after its clamp. A debug-seat aid for pairing a retail save
    /// state whose focus is not on the player: retail's follow ease writes
    /// the focus only on a frame it runs and the player moved, so a state
    /// whose player was poked - or carried by a script while
    /// movement-locked - holds a focus the seat's snap would otherwise
    /// replace. The ease then leaves it until the player moves.
    pub fn seat_focus_after_snap(&mut self, focus: [i32; 2]) {
        self.seat_focus = Some(focus);
    }

    /// Land `eye` (the eye translation `0x800840B8/BC/C0`) as the eye of the
    /// next snap. A debug-seat aid like [`Self::seat_focus_after_snap`]: the
    /// follow ease steps the eye an eighth of its gap only on a frame the
    /// player moved (`FUN_801DB510`'s stationary test) and not at all while
    /// the player is movement-locked, so a retail state whose player stopped,
    /// or was taken by a talk, before the ease closed holds an eye short of
    /// the pose the seat's snap composes. The ease then carries on from it
    /// as retail's would, the next frame the player moves.
    pub fn seat_eye_after_snap(&mut self, eye: [i32; 3]) {
        self.seat_eye = Some(eye);
    }

    /// Land `[pitch, yaw, h]` (`_DAT_8007B790` / `_DAT_8007B792` /
    /// `_DAT_8007B6F4`) as the angles of the next snap. A debug-seat aid like
    /// [`Self::seat_eye_after_snap`]: the follow ease steps the three on the
    /// same moved-frame legs as the eye (`FUN_801DB510`'s descriptor list at
    /// `0x801F2798`), so a retail state whose player stopped mid-glide - a
    /// mode-1 / 2 yaw sweep the walk left short of its target - holds angles
    /// the snap would otherwise replace.
    pub fn seat_angles_after_snap(&mut self, angles: [i32; 3]) {
        self.seat_angles = Some(angles);
    }

    /// Arm a scene-entry / arrival snap: re-query the tile and copy the
    /// composed target into the globals on the next tick.
    pub fn arm_arrival(&mut self) {
        self.snap_pending = true;
        self.reload_pending = true;
        self.tile = None;
        self.prev_player = None;
        self.active = false;
        self.focus_held = None;
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
            manual_tilt: 0.0,
            manual_zoom: 1.0,
            render_yaw_bias: 0.0,
            distance: CameraDistance::Retail,
            motion_state: MotionState::default(),
            motion_target: MotionTarget::default(),
            globals: RetailCamGlobals::default(),
            mover: None,
            last_field_frame: 0,
            script_owns_focus: false,
            zone: ZoneFollow::default(),
            camera_snap_beats: Vec::new(),
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
    /// handler; the view build it feeds is `FUN_800172C0` (Euler kernel
    /// `FUN_80026988`).
    ///
    /// Put the mover in flight exactly `left` display frames short of its
    /// duration and re-derive the globals from it - a capture-alignment aid
    /// for the retail-compare image channel, not a game path. Retail's
    /// mover credits the adaptive frame-skip factor per logic tick
    /// (`t = min(t + DAT_1F800393, d)`), so the progress a glide has made at
    /// a given script phase is frame-skip history no replay reproduces; a
    /// retail state's own mover (`+0x9E - +0x9C`) says how much it had left.
    /// No-op with no glide in flight.
    ///
    /// REF: FUN_801DC0BC
    pub fn align_glide_frames_left(&mut self, left: i32) {
        if let Some(mv) = self.mover.as_mut() {
            mv.t = (mv.d - left).clamp(0, mv.d);
            self.globals.0 = mv.values();
        }
    }

    /// Take this frame's `apply_trigger == 0` Configure beats as packed
    /// component snaps, ready for
    /// [`legaia_engine_camera::psx_camera::CutsceneCameraInterp::snap_components`].
    /// Both hosts call it once per frame, right before arming the glide;
    /// empty on every frame no snap beat executed. See
    /// [`Self::camera_snap_beats`].
    pub fn take_camera_snap_beats(&mut self) -> Vec<Vec<(usize, f32)>> {
        std::mem::take(&mut self.camera_snap_beats)
            .iter()
            .map(|b| legaia_engine_camera::psx_camera::CutsceneCameraInterp::snap_components_for(b))
            .collect()
    }

    /// Drop any banked snap beats - the host's cutscene interp is not live
    /// this frame, so replaying them would snap a pose nothing reads.
    pub fn clear_camera_snap_beats(&mut self) {
        self.camera_snap_beats.clear();
    }
}
