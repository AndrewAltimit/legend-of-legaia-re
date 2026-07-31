//! Phase-scripted retail battle camera (game mode `0x15`) - the ONE model
//! both hosts run: the native play-window (`engine-shell`'s `window/battle_cam`
//! adapter) and the browser play page (`web-viewer::play_battle_render`) drive
//! this same state machine and project through [`battle_vp`], so the browser
//! battle frames exactly like the native window instead of approximating it.
//!
//! Retail's battle camera is NOT a fixed orbit: it glides between three
//! scripted framings keyed on the battle phase, holding static in the
//! close-ups and idling in a slow orbit only in the far "menu" framing.
//! Pinned per-frame from the PCSX-Redux camera trace on the
//! `s5_tetsu_battle` anchor (rotation trio `0x8007B790`, translation trio
//! `0x800840B8`, GTE `H = 256`), cross-checked against the four catalogued
//! mednafen Tetsu battle states:
//!
//! - **Dialogue** (tutorial / stage-overlay text up): held close-up,
//!   pitch `0`, yaw `0`, TR `(0, 1280, 1638)` - static, no orbit.
//! - **Menu** (top Begin/Run framing, and any time no menu owns the pad):
//!   pitch `32`, TR `(0, 1280, z)` with `z` sized to the live formation (the
//!   traced solo fight lands on `7680`), idle orbit `-4` yaw units per
//!   camera step.
//! - **Submenu** (per-character command menu open): glide to the
//!   active-character close-up - yaw `2288`, TR `(-512, 1152, 2457)` -
//!   then held static while the submenu is open.
//!
//! One camera step spans **2 vsyncs** (every trace entry lands on an even
//! frame delta); the glide laws are the measured per-step increments:
//!
//! - Dialogue dismiss: pitch `+6`/step clamped at `32`, TR.z `+864`/step
//!   clamped at `7680`, while the idle orbit resumes immediately (yaw runs
//!   `-4`/step from `0` during the glide).
//! - Submenu open: all components arrive together over **6** steps
//!   (linear per-component increments, shortest-arc yaw).
//! - Submenu exit: a scripted swing back out - 6 steps up to the
//!   over-the-shoulder pose (pitch `256`, yaw eased to `0` mod 4096,
//!   TR `(0, 1536, 3276)`), then 7 steps back down to the menu framing
//!   with the idle orbit already running. (Retail holds the swing pose
//!   while the strike animation plays; the engine chains the two segments
//!   back-to-back.)
//!
//! ## Submenu framing is a formula, not a per-seat table
//!
//! The submenu close-up comes from `FUN_801D5854` case `0` (mode `0`,
//! called with the active battle-actor slot). Every component is either a
//! constant or a function of the acting actor - there is no seat table and
//! no `base + seat * delta` angle law:
//!
//! ```text
//! pitch = 0x20                                  // constant
//! yaw   = 0x8F0 - actor[+0x46]                  // facing-relative
//! TR    = (-0x200, HEIGHT[char_id], 0x600)      // x, z constant
//! focus = -actor[+0x34/+0x36/+0x38]             // negated world position
//! ```
//!
//! Two things follow. First, the measured `yaw 2288` is not a seat magic
//! number - it is `0x8F0` with Vahn's battle facing of `0` subtracted, so
//! the framing is a fixed over-the-shoulder offset that generalizes to any
//! seat once the actor's facing is tracked. Second, the per-seat variation
//! lives entirely in the **focus** trio (`0x80089118/1C/20`), which is the
//! negated position of whichever actor is acting: the camera orbits about
//! the active character. A solo-Vahn trace cannot distinguish that from a
//! constant, which is why the original measurement read as one fixed pose.
//!
//! `TR.z` is the one prescaled slot. `FUN_801D829C` rewrites its argument
//! as `(z << 8) / 0xA0` - a world distance into GTE projection units
//! (`0xA0` = 160 = screen half-width, `<< 8` = `H = 256`). The measured
//! `2457` is `floor(0x600 * 256 / 160)`; the truncation is why the traced
//! values are not exact divides.
//!
//! `TR.y` is the only genuine table: `0x801F4D2C + (char_id - 1) * 2`, keyed
//! on **character identity** (`DAT_8007BD10[slot]`, the 1-based party-record
//! selector), not on seat - a per-model height offset. It is disc data, read
//! off the battle-action overlay by `legaia_asset::battle_camera_table` and
//! handed to [`BattleCamActor::height`] by the host rather than transcribed
//! here; [`SUBMENU_HEIGHT_FALLBACK`] covers a disc-free host.
//!
//! ## The far "menu" framing is also computed, not a constant
//!
//! `FUN_801D5854` case `9` builds the Begin/Run framing from the **live
//! formation**, which is why its depth is not a magic number either:
//!
//! ```text
//! pitch = 0x20                       // constant
//! yaw   = _DAT_8007B792              // unchanged - the idle orbit owns it
//! TR    = (0, 0x500, span * 3)       // span clamped up to 0x800
//! focus = -(bbox centre of the framed actors)
//! ```
//!
//! The bbox spans the actor slots selected by the framing argument (whole
//! field / enemies only / party only), over actors whose `+0x14c` presence
//! halfword is non-zero, taking `min`/`max` of `actor[+0x34]` (X) and
//! `actor[+0x38]` (Z). `span = max(dx, dz)`, and `TR.z = max(span * 3,
//! 0x800)`. The traced `7680` is `prescale(0x12C0)`, i.e. a span of `1600` in
//! that particular fight - a measurement of one formation, not a constant.
//! See [`menu_framing`].
//!
//! ## Focus trio
//!
//! Every case passes a focus trio alongside the rotation and translation, and
//! `FUN_801D829C` tweens all nine components together over one duration. The
//! focus is the negated world point the camera orbits: the acting actor for
//! the close-ups, the formation centre for the menu framing. It is the only
//! place per-seat variation lives, so a host that drops it frames every seat
//! on the formation centre - see [`BattleCamPose::focus`].
//!
//! ## The glides step on retail's own increments
//!
//! `FUN_801D829C` is the tween builder retail's framing cases arm, and its
//! port is [`crate::battle_camera::build_camera_angle_tween`]. `Glide::linear`
//! calls it: the arrive-together glides take their per-component rates, their
//! 12-bit shortest-arc yaw and their TR.z projection prescale straight out of
//! it, so there is one implementation of the stepping arithmetic rather than
//! two, and the increments are retail's `ceil(|delta| / duration)` integers
//! rather than an exact float divide.
//!
//! The dialogue-dismiss glide keeps its own per-component rates: the trace has
//! pitch settling on step 6 and TR.z on step 7, so that transition is not one
//! arrive-together tween and no single duration reproduces it.
//!
//! REF: FUN_801D5854 (the framing cases), FUN_801D829C (the angle-tween
//! builder).

/// Battle-camera framing phase, derived from the live battle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleCamPhase {
    /// An in-battle dialogue box is up (the tutorial text).
    Dialogue,
    /// No menu owns the pad: the far framing with the idle orbit.
    Menu,
    /// A command / arts / spell / item submenu is open.
    Submenu,
}

/// The retail phase for one frame of battle state. Both hosts feed the same
/// two booleans: `dialogue_up` = an in-battle dialogue / inline-dialogue box
/// owns the screen, `submenu_open` = a per-character command / arts / spell /
/// item session owns the pad.
pub fn phase_for(dialogue_up: bool, submenu_open: bool) -> BattleCamPhase {
    if dialogue_up {
        BattleCamPhase::Dialogue
    } else if submenu_open {
        BattleCamPhase::Submenu
    } else {
        BattleCamPhase::Menu
    }
}

/// One camera pose: 12-bit angle units (`4096` = full turn) + the eye-space
/// translation trio, exactly the retail globals' value space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleCamPose {
    /// Pitch, 12-bit units (`0x8007B790`).
    pub pitch: f32,
    /// Yaw, 12-bit units (`0x8007B792`). May run outside `[0, 4096)`
    /// mid-glide (shortest-arc unwrap); normalized when the orbit owns it.
    pub yaw: f32,
    /// Eye-space translation `(x, y, z)` (`0x800840B8/BC/C0`).
    pub tr: [f32; 3],
    /// The world point the camera orbits (`0x80089118/1C/20`, stored negated
    /// in retail; held un-negated here). `FUN_801D829C` tweens it on the same
    /// clock as the rotation and translation trios.
    pub focus: [f32; 3],
}

/// Tutorial-dialogue close-up (trace frames 1..45: 240+ frames static).
const DIALOGUE_POSE: BattleCamPose = BattleCamPose {
    pitch: 0.0,
    yaw: 0.0,
    tr: [0.0, 1280.0, 1638.0],
    focus: [0.0; 3],
};
/// Far Begin/Run framing, `FUN_801D5854` case `9`. Pitch and TR.x / TR.y are
/// the case's constants; yaw free-orbits and TR.z is formation-sized.
const MENU_PITCH: f32 = 32.0; // 0x20
const MENU_TR_X: f32 = 0.0;
const MENU_TR_Y: f32 = 1280.0; // 0x500
/// Formation span -> raw eye-space depth: `max(dx, dz) * 3`, floored.
const MENU_SPAN_SCALE: f32 = 3.0;
const MENU_TR_Z_MIN_RAW: f32 = 2048.0; // 0x800
/// Submenu close-up constants, from `FUN_801D5854` case `0`.
const SUBMENU_PITCH: f32 = 32.0; // 0x20
/// Yaw base: retail computes `0x8F0 - actor_facing`.
const SUBMENU_YAW_BASE: i32 = 0x8F0; // 2288
/// Eye-space X, constant across every seat and character.
const SUBMENU_TR_X: f32 = -512.0; // -0x200
/// Raw eye-space Z before `FUN_801D829C`'s projection prescale.
const SUBMENU_TR_Z_RAW: i32 = 0x600; // 1536

/// The pose a host renders before the first camera tick arms the state: the
/// far framing at its minimum depth (`prescale(0x800)`), on the origin.
pub const BOOT_POSE: BattleCamPose = BattleCamPose {
    pitch: MENU_PITCH,
    yaw: 0.0,
    tr: [MENU_TR_X, MENU_TR_Y, prescale_tr_z(0x800)],
    focus: [0.0; 3],
};

/// `FUN_801D829C`'s TR.z prescale: world distance -> GTE projection units.
/// `0xA0` = 160 = PSX screen half-width; `<< 8` = GTE `H = 256`. The divide
/// truncates, which is why the traced `0x600` lands on `2457`, not `2457.6`.
pub const fn prescale_tr_z(raw: i32) -> f32 {
    ((raw << 8) / 0xA0) as f32
}

/// Camera height used when the host has no disc table to resolve
/// `0x801F4D2C` from. Vahn's entry, the one value the solo-Vahn camera trace
/// observes, so an unpinned character frames like the measured case instead
/// of jumping. Real per-character heights come from
/// `legaia_asset::battle_camera_table` via [`BattleCamActor::height`].
pub const SUBMENU_HEIGHT_FALLBACK: f32 = 1152.0; // 0x480

/// The acting battle actor the submenu framing is built around.
///
/// `facing` is retail `actor[+0x46]` (12-bit angle), `world` the actor
/// position at `actor[+0x34/+0x36/+0x38]`, and `height` the `TR.y` the host
/// resolved out of the disc table `0x801F4D2C` for this actor's character id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleCamActor {
    pub facing: i32,
    pub world: [f32; 3],
    /// Per-character `TR.y` from `0x801F4D2C`; `None` falls back to
    /// [`SUBMENU_HEIGHT_FALLBACK`].
    pub height: Option<f32>,
}

impl Default for BattleCamActor {
    /// The measured solo-Vahn case: facing `0`, seated at the traced
    /// `(0, 0, -800)`, on the fallback height. Reproduces the originally
    /// pinned framing exactly, so an un-wired host keeps the measured
    /// behaviour.
    fn default() -> Self {
        BattleCamActor {
            facing: 0,
            world: [0.0, 0.0, -800.0],
            height: None,
        }
    }
}

impl BattleCamActor {
    /// Retail's case-0 submenu framing for this actor (`FUN_801D5854`): a
    /// fixed over-the-shoulder offset, facing-relative yaw, per-character
    /// height, orbiting the actor's own position.
    pub fn submenu_pose(self) -> BattleCamPose {
        BattleCamPose {
            pitch: SUBMENU_PITCH,
            yaw: (SUBMENU_YAW_BASE - self.facing).rem_euclid(4096) as f32,
            tr: [
                SUBMENU_TR_X,
                self.height.unwrap_or(SUBMENU_HEIGHT_FALLBACK),
                prescale_tr_z(SUBMENU_TR_Z_RAW),
            ],
            focus: self.world,
        }
    }
}

/// The world-space X/Z extent of the actors the far framing encloses -
/// retail's `min`/`max` walk over `actor[+0x34]` / `actor[+0x38]` for every
/// present actor in the selected slot range (`FUN_801D5854` case `9`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormationBox {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl FormationBox {
    /// Fold one present actor's world X/Z into the accumulator - retail's
    /// per-actor `min`/`max` step, shared so both hosts build the box with
    /// the same arithmetic (only the host-side presence predicate differs).
    pub fn extend(bbox: &mut Option<FormationBox>, x: f32, z: f32) {
        match bbox {
            None => {
                *bbox = Some(FormationBox {
                    min: [x, z],
                    max: [x, z],
                })
            }
            Some(b) => {
                b.min[0] = b.min[0].min(x);
                b.min[1] = b.min[1].min(z);
                b.max[0] = b.max[0].max(x);
                b.max[1] = b.max[1].max(z);
            }
        }
    }
}

/// The far framing's eye-space Z in **raw world units**, before the
/// projection prescale: `max(span * 3, 0x800)`, where `span` is the larger of
/// the formation's two extents. This is the value retail's case 9 hands the
/// tween builder, and [`prescale_tr_z`] is what the builder then does to it.
pub fn menu_raw_z(bbox: Option<FormationBox>) -> i32 {
    let span = match bbox {
        // Retail keeps the LARGER of the two extents, so a formation that is
        // wide but shallow still fits the frame.
        Some(b) => (b.max[0] - b.min[0]).max(b.max[1] - b.min[1]),
        None => 0.0,
    };
    (span * MENU_SPAN_SCALE).max(MENU_TR_Z_MIN_RAW) as i32
}

/// Retail's case-9 far framing for a formation. `None` (no present actors)
/// keeps the minimum depth and the origin focus, which is what retail's
/// un-entered min/max accumulators degenerate to.
pub fn menu_framing(bbox: Option<FormationBox>, yaw: f32) -> BattleCamPose {
    let focus = match bbox {
        Some(b) => [
            (b.min[0] + b.max[0]) * 0.5,
            0.0,
            (b.min[1] + b.max[1]) * 0.5,
        ],
        None => [0.0; 3],
    };
    BattleCamPose {
        pitch: MENU_PITCH,
        yaw,
        tr: [MENU_TR_X, MENU_TR_Y, prescale_tr_z(menu_raw_z(bbox))],
        focus,
    }
}
/// Raw eye-space Z of the swing pose, before the projection prescale.
const SWING_TR_Z_RAW: i32 = 0x800;

/// Over-the-shoulder swing pose the submenu exit passes through
/// (trace frames 163..173; yaw target is `4096` = `0` unwrapped upward from
/// `2288`). Retail case `1` orbits the acting actor like the close-up does,
/// so the focus is filled in per swing rather than baked here.
const SWING_POSE: BattleCamPose = BattleCamPose {
    pitch: 256.0,
    yaw: 4096.0,
    tr: [0.0, 1536.0, prescale_tr_z(SWING_TR_Z_RAW)],
    focus: [0.0; 3],
};

/// Idle-orbit yaw decrement per camera step (`-4` units per 2 vsyncs
/// = -120 units/s; the mednafen menu state's yaw 3372 is an orbit sample).
const ORBIT_STEP: f32 = 4.0;
/// Dialogue-dismiss glide rates (per step, clamped per component).
const DIALOGUE_EXIT_PITCH_RATE: f32 = 6.0;
const DIALOGUE_EXIT_Z_RATE: f32 = 864.0;
/// Step counts for the linear (arrive-together) glides.
const SUBMENU_ENTER_STEPS: u32 = 6;
const SUBMENU_SWING_STEPS: u32 = 6;
const SWING_RETURN_STEPS: u32 = 7;

/// One glide segment: per-component per-step rates toward `target`, each
/// component clamping independently. `yaw_glides` routes yaw through the
/// glide (shortest-arc, pre-unwrapped into `target.yaw`); when `false` the
/// idle orbit keeps owning yaw during the glide (the dialogue-dismiss law).
#[derive(Debug, Clone, Copy)]
struct Glide {
    target: BattleCamPose,
    /// Per-step absolute rates, in the order `FUN_801D829C` walks its nine
    /// components: `[pitch, yaw, tr.x, tr.y, tr.z, focus.x, focus.y, focus.z]`
    /// (roll is never driven, so it is dropped).
    rate: [f32; 8],
    yaw_glides: bool,
    /// `Some(n)`: an arrive-together glide over exactly `n` steps (the final
    /// step lands every component ON the target, so float rounding in the
    /// per-step rates can't leave a residue). `None`: a rate-clamped glide
    /// (each component clamps independently; done when all are at target).
    steps_left: Option<u32>,
}

impl Glide {
    /// Linear glide: every component arrives together after `steps` steps.
    ///
    /// The rate table is **retail's**. `build_camera_angle_tween` is
    /// `FUN_801D829C`, and its fourth argument is the glide's duration: it
    /// emits one integer per-frame increment per component
    /// (`ceil(|current - target| / duration)`) plus the 12-bit shortest-arc
    /// adjustment on the rotation pair, which is precisely the arrive-together
    /// law this walker wants. Calling it here means the engine holds one
    /// implementation of the stepping arithmetic instead of two, and steps on
    /// retail's rounding rather than on an exact float divide.
    ///
    /// `target_tr_z_raw` is the target's eye-space Z in **world** units - the
    /// space retail's framing cases pass, and the one input the builder
    /// converts (`(z << 8) / 0xA0`). The converted value is what the glide
    /// converges on, so the caller's `target.tr[2]` is overwritten with it.
    ///
    /// `from` is `&mut` for the same reason retail's `current` side is: the
    /// shortest-arc unwrap can move the *current* angle a full turn rather
    /// than the target, and the pose the walker steps has to see that.
    ///
    /// REF: FUN_801D829C
    fn linear(
        from: &mut BattleCamPose,
        mut target: BattleCamPose,
        target_tr_z_raw: i32,
        steps: u32,
        yaw: bool,
    ) -> Self {
        use crate::battle_camera::{CameraAngles, build_camera_angle_tween};

        let trio = |v: [f32; 3]| [v[0] as i16, v[1] as i16, v[2] as i16];
        let mut cur = CameraAngles {
            rotation: [from.pitch as i16, from.yaw as i16, 0],
            shake: trio(from.tr),
            focus: trio(from.focus),
        };
        let mut tgt = CameraAngles {
            rotation: [target.pitch as i16, target.yaw as i16, 0],
            shake: [
                target.tr[0] as i16,
                target.tr[1] as i16,
                target_tr_z_raw as i16,
            ],
            focus: trio(target.focus),
        };
        let table = build_camera_angle_tween(&mut cur, &mut tgt, steps.max(1) as u16);
        // Step 1 of the builder converted TR.z into projection units in place.
        target.tr[2] = tgt.shake[2] as f32;
        if yaw {
            // Both ends come back from the builder's wrap-adjust; the segment
            // that leaves yaw to the idle orbit keeps its own value instead.
            from.yaw = cur.rotation[1] as f32;
            target.yaw = tgt.rotation[1] as f32;
        }
        // The builder's slot order is rotation, translation, focus; roll
        // (slot 2) is never driven here.
        let rate = [
            table[0].step as f32,
            table[1].step as f32,
            table[3].step as f32,
            table[4].step as f32,
            table[5].step as f32,
            table[6].step as f32,
            table[7].step as f32,
            table[8].step as f32,
        ];
        Glide {
            target,
            rate,
            yaw_glides: yaw,
            steps_left: Some(steps.max(1)),
        }
    }
}

/// Step `v` toward `target` by at most `rate`, clamping at the target.
fn step_toward(v: f32, target: f32, rate: f32) -> f32 {
    let d = target - v;
    if d.abs() <= rate {
        target
    } else {
        v + rate.copysign(d)
    }
}

/// The phase-scripted battle camera state. Created on battle entry, stepped
/// once per 2 retail display frames (`World::field_frames`), dropped on exit.
#[derive(Debug)]
pub struct BattleCamera {
    phase: BattleCamPhase,
    pose: BattleCamPose,
    /// Chained glide segments (front = active).
    glides: std::collections::VecDeque<Glide>,
    /// `field_frames` value already consumed, for the 2-vsync step cadence.
    last_frames: u64,
    /// Sub-step vsync accumulator (steps fire every 2 frames).
    frame_accum: u64,
    /// The acting actor the submenu close-up frames. Defaults to the
    /// measured solo-Vahn case; hosts that track the live battle actor call
    /// [`BattleCamera::set_actor`] so non-Vahn seats frame correctly.
    actor: BattleCamActor,
    /// The formation the far menu framing encloses. `None` (an un-wired host)
    /// falls back to retail's degenerate case: minimum depth, origin focus.
    formation: Option<FormationBox>,
}

/// Drive one host's battle camera for a frame - the single shared entry both
/// hosts call so the create / retarget / phase-change / step ordering cannot
/// drift between them. `slot` is the host's per-battle camera state
/// (dropped whenever `active` is false so the next battle re-snaps);
/// `active` is "a stage-dome battle owns the 3D frame"; `frames` is the
/// world's retail display-frame counter (`World::field_frames`, one camera
/// step per 2 frames).
///
/// A battle that opens on dialogue snaps to the held close-up; any other
/// battle snaps to the far menu framing (retail's loading pose resolves
/// there) and glides out to whichever phase is already live. The entry snap
/// takes the LIVE formation: retail's case 9 always runs against the live
/// actor table, so a battle that opens on the far framing sizes its depth
/// and centres its focus immediately rather than sitting at the degenerate
/// minimum until the first phase transition re-derives it.
pub fn drive(
    slot: &mut Option<BattleCamera>,
    active: bool,
    phase: BattleCamPhase,
    acting: Option<BattleCamActor>,
    formation: Option<FormationBox>,
    frames: u64,
) {
    if !active {
        *slot = None;
        return;
    }
    let entry = if phase == BattleCamPhase::Dialogue {
        BattleCamPhase::Dialogue
    } else {
        BattleCamPhase::Menu
    };
    let cam =
        slot.get_or_insert_with(|| BattleCamera::new_with_formation(entry, formation, frames));
    if let Some(actor) = acting {
        cam.set_actor(actor);
    }
    cam.set_formation(formation);
    cam.set_phase(phase);
    cam.advance_to(frames);
}

impl BattleCamera {
    /// New camera snapped to the entry phase's framing (a battle that opens
    /// on tutorial dialogue starts in the held close-up; any other battle
    /// starts at the far menu framing sized to no formation - see
    /// [`BattleCamera::new_with_formation`] for the live-formation entry).
    pub fn new(phase: BattleCamPhase, frames_now: u64) -> Self {
        Self::new_with_formation(phase, None, frames_now)
    }

    /// [`BattleCamera::new`] with the live formation available at entry, so
    /// a battle opening on the far menu framing snaps to the case-9
    /// formation-sized depth + centre instead of the degenerate minimum
    /// (retail's case 9 runs against the live actor table).
    pub fn new_with_formation(
        phase: BattleCamPhase,
        formation: Option<FormationBox>,
        frames_now: u64,
    ) -> Self {
        let actor = BattleCamActor::default();
        let pose = match phase {
            BattleCamPhase::Dialogue => DIALOGUE_POSE,
            BattleCamPhase::Submenu => actor.submenu_pose(),
            BattleCamPhase::Menu => menu_framing(formation, 0.0),
        };
        BattleCamera {
            phase,
            pose,
            glides: std::collections::VecDeque::new(),
            last_frames: frames_now,
            frame_accum: 0,
            actor,
            formation,
        }
    }

    /// Install the formation the far menu framing sizes itself to (retail's
    /// per-frame `min`/`max` walk over the present actors). Hosts call this
    /// as the battle formation changes; an already-armed glide is left alone.
    pub fn set_formation(&mut self, formation: Option<FormationBox>) {
        self.formation = formation;
    }

    /// The far menu framing for the live formation, at the current yaw (the
    /// idle orbit owns yaw across this transition - retail passes
    /// `_DAT_8007B792` straight through).
    fn menu_pose(&self) -> BattleCamPose {
        menu_framing(self.formation, self.pose.yaw)
    }

    /// Point the submenu close-up at the acting battle actor. Retail
    /// rebuilds the framing from the actor record on every submenu open
    /// (`FUN_801D5854` case `0`), so hosts should call this as the active
    /// seat changes; an already-armed glide is left alone.
    pub fn set_actor(&mut self, actor: BattleCamActor) {
        self.actor = actor;
    }

    /// Current camera pose (12-bit angle units + eye-space TR).
    pub fn pose(&self) -> BattleCamPose {
        self.pose
    }

    /// Observe the live battle phase; a change arms the measured glide.
    pub fn set_phase(&mut self, phase: BattleCamPhase) {
        if phase == self.phase {
            return;
        }
        // `Glide::linear` may unwrap the CURRENT yaw a full turn (retail's
        // wrap-adjust moves whichever side is behind), so the live pose has to
        // see the adjustment.
        let mut from = self.pose;
        self.glides.clear();
        match phase {
            BattleCamPhase::Menu => {
                if self.phase == BattleCamPhase::Dialogue {
                    // Dialogue dismiss: rate-clamped pitch/TR glide while
                    // the idle orbit resumes immediately (yaw not glided).
                    self.glides.push_back(Glide {
                        target: self.menu_pose(),
                        rate: [
                            DIALOGUE_EXIT_PITCH_RATE,
                            0.0,
                            f32::INFINITY,
                            f32::INFINITY,
                            DIALOGUE_EXIT_Z_RATE,
                            f32::INFINITY,
                            f32::INFINITY,
                            f32::INFINITY,
                        ],
                        yaw_glides: false,
                        steps_left: None,
                    });
                } else {
                    // Submenu exit: swing up over the shoulder, then ease
                    // back down to the menu framing (orbit resumes for the
                    // return segment - retail re-enters at yaw 0). The swing
                    // stays on the acting actor (retail case 1); only the
                    // return pulls the focus out to the formation centre.
                    let swing = Glide::linear(
                        &mut from,
                        BattleCamPose {
                            focus: self.actor.world,
                            ..SWING_POSE
                        },
                        SWING_TR_Z_RAW,
                        SUBMENU_SWING_STEPS,
                        true,
                    );
                    let mut swing_end = swing.target;
                    let back = Glide::linear(
                        &mut swing_end,
                        self.menu_pose(),
                        menu_raw_z(self.formation),
                        SWING_RETURN_STEPS,
                        false,
                    );
                    self.glides.push_back(swing);
                    self.glides.push_back(back);
                }
            }
            BattleCamPhase::Submenu => {
                self.glides.push_back(Glide::linear(
                    &mut from,
                    self.actor.submenu_pose(),
                    SUBMENU_TR_Z_RAW,
                    SUBMENU_ENTER_STEPS,
                    true,
                ));
            }
            BattleCamPhase::Dialogue => {
                // Retail never re-enters the dialogue close-up mid-battle;
                // snap defensively.
                self.pose = DIALOGUE_POSE;
                from = DIALOGUE_POSE;
            }
        }
        self.pose = from;
        self.phase = phase;
    }

    /// Advance to the world's retail-frame counter, stepping the camera once
    /// per 2 display frames (the measured cadence: every trace entry is an
    /// even frame apart).
    pub fn advance_to(&mut self, frames_now: u64) {
        let elapsed = frames_now.saturating_sub(self.last_frames);
        self.last_frames = frames_now;
        self.frame_accum += elapsed;
        while self.frame_accum >= 2 {
            self.frame_accum -= 2;
            self.step_once();
        }
    }

    /// One rate-limited step of every driven component (all but yaw, which
    /// only moves when the segment owns it - otherwise the idle orbit does).
    fn step_components(&mut self, g: &Glide) {
        self.pose.pitch = step_toward(self.pose.pitch, g.target.pitch, g.rate[0]);
        for k in 0..3 {
            self.pose.tr[k] = step_toward(self.pose.tr[k], g.target.tr[k], g.rate[2 + k]);
            self.pose.focus[k] = step_toward(self.pose.focus[k], g.target.focus[k], g.rate[5 + k]);
        }
        if g.yaw_glides {
            self.pose.yaw = step_toward(self.pose.yaw, g.target.yaw, g.rate[1]);
        }
    }

    fn step_once(&mut self) {
        // Yaw: the idle orbit owns it in the Menu phase unless the active
        // glide segment glides it (submenu enter / the exit swing).
        let yaw_gliding = self.glides.front().is_some_and(|g| g.yaw_glides);
        if !yaw_gliding && self.phase == BattleCamPhase::Menu {
            self.pose.yaw = (self.pose.yaw - ORBIT_STEP).rem_euclid(4096.0);
        }
        let Some(g) = self.glides.front().copied() else {
            return;
        };
        let done = match g.steps_left {
            // Arrive-together glide: the final step lands ON the target
            // (no float residue from the per-step rate division).
            Some(1) => {
                self.pose.pitch = g.target.pitch;
                self.pose.tr = g.target.tr;
                self.pose.focus = g.target.focus;
                if g.yaw_glides {
                    self.pose.yaw = g.target.yaw;
                }
                true
            }
            Some(n) => {
                if let Some(front) = self.glides.front_mut() {
                    front.steps_left = Some(n - 1);
                }
                self.step_components(&g);
                false
            }
            // Rate-clamped glide: each component clamps independently.
            None => {
                self.step_components(&g);
                self.pose.pitch == g.target.pitch
                    && self.pose.tr == g.target.tr
                    && self.pose.focus == g.target.focus
                    && (!g.yaw_glides || self.pose.yaw == g.target.yaw)
            }
        };
        if done {
            self.glides.pop_front();
            if g.yaw_glides {
                // Re-enter the wrapped orbit domain (the exit swing lands
                // on 4096 = 0, where the idle orbit resumes).
                self.pose.yaw = self.pose.yaw.rem_euclid(4096.0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Projection: the retail GTE battle camera as one view-projection matrix.
// ---------------------------------------------------------------------------

/// GTE projection focal length the battle camera runs on (`_DAT_8007B6F4`).
pub const GTE_H: f32 = 256.0;

/// Near plane both hosts project with (governs depth precision only - the
/// retail GTE has no near plane; 4 units is the engine's shared choice).
pub const PSX_NEAR: f32 = 4.0;

/// Far plane both hosts project with. Paired with the native renderer's
/// `legaia_engine_render::window::SCENE_FAR` (a hard wgpu link this crate
/// cannot take); the pairing is pinned by `scene_far_paired_with_renderer`
/// in the engine-shell camera tests.
pub const SCENE_FAR: f32 = 1_000_000.0;

/// Column-major 4x4 multiply: `out = a * b` (same layout as WebGL `mat4`
/// and `glam::Mat4::to_cols_array`).
fn mat_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            out[c * 4 + r] = s;
        }
    }
    out
}

/// The full battle view-projection for one [`BattleCamPose`], column-major -
/// the shared rendition of the native window's `psx_camera_mvp` composition
/// specialised to the battle camera (`battle_dome_camera_mvp`): retail GTE
/// `screen = H * (R*(v - focus) + TR) / Ze` with `R = Rx(pitch)*Ry(yaw)`
/// (12-bit angles, roll never staged by the battle phase script), `H = 256`,
/// PSX screen `+Y` down over the 320x240 frame, X corrected so the 4:3
/// retail framing holds at any viewport aspect.
///
/// `world_scale` is the retail 4x battle world scale (base matrix
/// `0x8007BF10 = 16384*I`) the host composes onto the ACTOR draws; the focus
/// trio targets those scaled actors, so it is pre-scaled here exactly as the
/// native camera pre-scales it. The trailing `scale(1,-1,1)` factor cancels
/// the per-model Y-flip every draw's model matrix carries, recovering the
/// raw PSX Y-down vertex the retail transform expects.
///
/// Depth maps `[near, far]` to `[0, 1]` (the native wgpu convention; well
/// inside WebGL's `[-1, 1]` clip range, so the same matrix serves both
/// hosts). Pinned against the native glam composition by
/// `battle_vp_matches_the_native_glam_composition` in the engine-shell
/// camera tests.
///
/// REF: FUN_8001CF50 (retail camera-rotation build), FUN_80026988 /
/// FUN_80026f50 (the projection), FUN_80048A08 (per-actor world-scale
/// composition).
pub fn battle_vp(pose: &BattleCamPose, world_scale: f32, aspect: f32) -> [f32; 16] {
    let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
    let (pitch, yaw) = (to_rad(pose.pitch), to_rad(pose.yaw));
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    // R = Rx(pitch) * Ry(yaw), the same right-handed factors glam's
    // from_rotation_x/y build (and the q3.12 GTE port's).
    let rx: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, cp, sp, 0.0, //
        0.0, -sp, cp, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let ry: [f32; 16] = [
        cy, 0.0, -sy, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        sy, 0.0, cy, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let r = mat_mul(&rx, &ry);
    let t: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        pose.tr[0], pose.tr[1], pose.tr[2], 1.0,
    ];
    // The focus targets the world-scaled actor stage (see the doc above).
    let f = [
        pose.focus[0] * world_scale,
        pose.focus[1] * world_scale,
        pose.focus[2] * world_scale,
    ];
    let neg_focus: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        -f[0], -f[1], -f[2], 1.0,
    ];
    let flip: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, -1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    // PSX perspective onto a 320x240 frame: ndc.x = H*Ex/(160*Ez),
    // ndc.y = -H*Ey/(120*Ez) (PSX +Y down -> NDC up), clip.w = Ez.
    let (near, far) = (PSX_NEAR, SCENE_FAR);
    let a = far / (far - near);
    let b = -near * far / (far - near);
    let aspect_fix = (4.0 / 3.0) / aspect.max(0.01);
    let proj: [f32; 16] = [
        GTE_H / 160.0 * aspect_fix,
        0.0,
        0.0,
        0.0, //
        0.0,
        -GTE_H / 120.0,
        0.0,
        0.0, //
        0.0,
        0.0,
        a,
        1.0, //
        0.0,
        0.0,
        b,
        0.0,
    ];
    mat_mul(
        &proj,
        &mat_mul(&t, &mat_mul(&r, &mat_mul(&neg_focus, &flip))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formation behind the traced Tetsu fight. The trace pins the far
    /// framing's TR.z at `7680` = `prescale(0x12C0)`, and case 9 builds that
    /// raw `0x12C0` as `span * 3`, so the traced formation spanned `1600`
    /// world units. Every trace-pinned menu assertion below is stated
    /// against this formation - which is the point: the law reproduces the
    /// measurement instead of hardcoding it.
    fn traced_formation() -> FormationBox {
        FormationBox {
            min: [-800.0, -800.0],
            max: [800.0, 800.0],
        }
    }

    /// The traced far framing, reproduced by the case-9 law.
    fn traced_menu_tr() -> [f32; 3] {
        menu_framing(Some(traced_formation()), 0.0).tr
    }

    /// A camera armed on the traced formation.
    fn traced_cam(phase: BattleCamPhase) -> BattleCamera {
        let mut cam = BattleCamera::new(phase, 0);
        cam.set_formation(Some(traced_formation()));
        // Re-snap: `new` built the entry pose before the formation landed.
        if phase == BattleCamPhase::Menu {
            cam.pose = cam.menu_pose();
        }
        cam
    }

    /// The originally measured solo-Vahn framing must fall out of the
    /// formula, not be hardcoded: `yaw 2288 / TR (-512, 1152, 2457)`.
    #[test]
    fn default_actor_reproduces_the_measured_vahn_framing() {
        let p = BattleCamActor::default().submenu_pose();
        assert_eq!(p.pitch, 32.0);
        assert_eq!(p.yaw, 2288.0, "0x8F0 - facing 0");
        assert_eq!(p.tr, [-512.0, 1152.0, 2457.0]);
    }

    /// `2288` is `0x8F0` minus the actor's facing - a fixed
    /// over-the-shoulder offset, so any seat's framing follows its facing.
    #[test]
    fn submenu_yaw_tracks_actor_facing() {
        let at = |facing| {
            BattleCamActor {
                facing,
                ..Default::default()
            }
            .submenu_pose()
            .yaw
        };
        assert_eq!(at(0), 2288.0);
        assert_eq!(at(1024), 1264.0, "quarter turn right");
        assert_eq!(at(2288), 0.0, "actor facing the base angle");
        // Wraps into [0, 4096) rather than going negative.
        assert_eq!(at(3000), (0x8F0 - 3000 + 4096) as f32);
        assert_eq!(at(4096), 2288.0, "full turn is identity");
    }

    /// TR.x / TR.z / pitch are seat- and character-invariant constants: only
    /// TR.y (the disc table) and the yaw/focus vary.
    #[test]
    fn submenu_constants_do_not_vary_by_actor() {
        let heights = [1152.0f32, 960.0, 1408.0, 512.0];
        for facing in [0, 700, 2048, 4095] {
            for height in heights {
                let p = BattleCamActor {
                    facing,
                    height: Some(height),
                    world: [1.0, 2.0, 3.0],
                }
                .submenu_pose();
                assert_eq!(p.pitch, 32.0);
                assert_eq!(p.tr[0], -512.0);
                assert_eq!(p.tr[2], 2457.0);
                assert_eq!(p.tr[1], height, "TR.y is the per-character table");
            }
        }
    }

    /// The prescale truncates - `0x600` lands on 2457, not 2458.
    #[test]
    fn tr_z_prescale_truncates() {
        assert_eq!(prescale_tr_z(0x600), 2457.0);
        // The other traced framings fall out of the same divide.
        assert_eq!(prescale_tr_z(0x400), 1638.0);
        assert_eq!(prescale_tr_z(0x800), 3276.0);
    }

    /// The per-seat half of the framing: the focus is the acting actor's own
    /// position, so the camera orbits about whoever is acting.
    #[test]
    fn focus_is_the_acting_actor_position() {
        let a = BattleCamActor {
            facing: 0,
            height: None,
            world: [640.0, -128.0, -800.0],
        };
        assert_eq!(a.submenu_pose().focus, [640.0, -128.0, -800.0]);
        // Two seats at different positions frame differently even though
        // their rotation + translation trios agree - which is exactly why a
        // solo trace could not tell the focus from a constant.
        let b = BattleCamActor {
            world: [-640.0, -128.0, -800.0],
            ..a
        };
        assert_eq!(a.submenu_pose().tr, b.submenu_pose().tr);
        assert_eq!(a.submenu_pose().yaw, b.submenu_pose().yaw);
        assert_ne!(a.submenu_pose().focus, b.submenu_pose().focus);
    }

    /// Retargeting the camera at a different seat moves the glide target.
    #[test]
    fn set_actor_retargets_the_submenu_glide() {
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
        cam.set_actor(BattleCamActor {
            facing: 1024,
            height: Some(960.0),
            world: [640.0, 0.0, -800.0],
        });
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 2 * SUBMENU_ENTER_STEPS as u64);
        assert_eq!(cam.pose().yaw.rem_euclid(4096.0), 1264.0);
        // Height comes from the disc table, and the focus followed the seat.
        assert_eq!(cam.pose().tr[1], 960.0);
        assert_eq!(cam.pose().focus, [640.0, 0.0, -800.0]);
    }

    /// The traced far framing falls out of the case-9 formation law rather
    /// than being a constant: a 1600-unit span reproduces `TR.z = 7680`
    /// (`prescale(0x12C0)`), and the focus lands on the formation centre.
    #[test]
    fn menu_framing_reproduces_the_traced_depth_from_the_formation() {
        let p = menu_framing(Some(traced_formation()), 0.0);
        assert_eq!(p.pitch, MENU_PITCH);
        assert_eq!(p.tr, [0.0, 1280.0, 7680.0]);
        assert_eq!(p.focus, [0.0; 3], "symmetric formation centres on origin");
    }

    /// A wider formation pushes the camera back; an off-centre one drags the
    /// focus with it. Both are invisible to a solo trace.
    #[test]
    fn menu_framing_tracks_the_formation() {
        // Twice the span -> twice the raw depth.
        let wide = menu_framing(
            Some(FormationBox {
                min: [-1600.0, -1600.0],
                max: [1600.0, 1600.0],
            }),
            0.0,
        );
        assert_eq!(wide.tr[2], prescale_tr_z(3200 * 3));
        // The LARGER of the two extents wins, so a wide-but-shallow line
        // frames on its width.
        let shallow = menu_framing(
            Some(FormationBox {
                min: [-800.0, -10.0],
                max: [800.0, 10.0],
            }),
            0.0,
        );
        assert_eq!(shallow.tr[2], traced_menu_tr()[2]);
        // Off-centre formation -> off-centre focus.
        let off = menu_framing(
            Some(FormationBox {
                min: [200.0, -800.0],
                max: [1800.0, 800.0],
            }),
            0.0,
        );
        assert_eq!(off.focus, [1000.0, 0.0, 0.0]);
        assert_eq!(off.tr[2], traced_menu_tr()[2], "same span, same depth");
    }

    /// Below the 0x800 floor the depth clamps - a solo actor (a degenerate
    /// box) does not collapse the camera onto its own head.
    #[test]
    fn menu_depth_clamps_at_the_retail_floor() {
        let solo = menu_framing(
            Some(FormationBox {
                min: [640.0, -800.0],
                max: [640.0, -800.0],
            }),
            0.0,
        );
        assert_eq!(solo.tr[2], prescale_tr_z(0x800));
        assert_eq!(solo.focus, [640.0, 0.0, -800.0], "still centres on it");
        // No actors at all degenerates the same way, on the origin.
        assert_eq!(menu_framing(None, 0.0).tr[2], prescale_tr_z(0x800));
        assert_eq!(menu_framing(None, 0.0).focus, [0.0; 3]);
    }

    /// The idle orbit owns yaw across the menu framing, so the case-9 pose
    /// passes the live yaw straight through (retail's `_DAT_8007B792`).
    #[test]
    fn menu_framing_passes_yaw_through() {
        for yaw in [0.0, 1234.0, 4064.0] {
            assert_eq!(menu_framing(Some(traced_formation()), yaw).yaw, yaw);
        }
    }

    /// The focus trio glides on the same clock as the rotation and
    /// translation trios (`FUN_801D829C` tweens all nine together), so a
    /// submenu open pans onto the acting seat instead of cutting.
    #[test]
    fn focus_tweens_with_the_rest_of_the_pose() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        cam.set_actor(BattleCamActor {
            facing: 0,
            height: Some(1408.0),
            world: [1200.0, 0.0, -800.0],
        });
        cam.set_phase(BattleCamPhase::Submenu);
        // Mid-glide the focus is partway between the formation centre and
        // the seat - not snapped to either end.
        steps(&mut cam, 3);
        let mid = cam.pose().focus;
        assert!(mid[0] > 0.0 && mid[0] < 1200.0, "focus mid-pan: {mid:?}");
        // And it arrives with everything else on step 6.
        steps(&mut cam, 3);
        assert_eq!(cam.pose().focus, [1200.0, 0.0, -800.0]);
        assert_eq!(cam.pose().tr[1], 1408.0);
    }

    /// Two different seats produce genuinely different framings - the check
    /// a solo-Vahn trace structurally cannot make. Same TR trio, different
    /// yaw and different focus.
    #[test]
    fn non_vahn_seats_frame_differently() {
        let seat = |facing, height, world| {
            let mut cam = traced_cam(BattleCamPhase::Menu);
            cam.set_actor(BattleCamActor {
                facing,
                height: Some(height),
                world,
            });
            cam.set_phase(BattleCamPhase::Submenu);
            steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
            cam.pose()
        };
        // Vahn centre-seat, Noa left-seat, Gala right-seat: retail heights
        // 0x480 / 0x3C0 / 0x580 and three different facings.
        let vahn = seat(0, 1152.0, [0.0, 0.0, -800.0]);
        let noa = seat(512, 960.0, [-700.0, 0.0, -900.0]);
        let gala = seat(3584, 1408.0, [700.0, 0.0, -900.0]);
        for (a, b) in [(&vahn, &noa), (&noa, &gala), (&vahn, &gala)] {
            assert_ne!(a.yaw, b.yaw, "facing-relative yaw must differ");
            assert_ne!(a.focus, b.focus, "focus must follow the seat");
            assert_ne!(a.tr[1], b.tr[1], "per-character height must differ");
            assert_eq!(a.tr[0], b.tr[0], "TR.x is seat-invariant");
            assert_eq!(a.tr[2], b.tr[2], "TR.z is seat-invariant");
        }
        // Each yaw is its own `0x8F0 - facing`.
        assert_eq!(vahn.yaw, 2288.0);
        assert_eq!(noa.yaw, (0x8F0 - 512) as f32);
        assert_eq!(gala.yaw.rem_euclid(4096.0), (0x8F0 - 3584 + 4096) as f32);
    }

    /// The submenu-exit swing stays on the acting actor (retail case 1) and
    /// only the return segment pulls the focus back to the formation centre.
    #[test]
    fn exit_swing_holds_the_seat_then_releases_it() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        cam.set_actor(BattleCamActor {
            facing: 0,
            height: Some(960.0),
            world: [-700.0, 0.0, -900.0],
        });
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
        cam.set_phase(BattleCamPhase::Menu);
        steps(&mut cam, SUBMENU_SWING_STEPS as u64);
        assert_eq!(cam.pose().focus, [-700.0, 0.0, -900.0], "swing holds it");
        steps(&mut cam, SWING_RETURN_STEPS as u64);
        assert_eq!(cam.pose().focus, [0.0; 3], "return re-centres");
        assert_eq!(cam.pose().tr, traced_menu_tr());
    }

    fn steps(cam: &mut BattleCamera, n: u64) {
        for _ in 0..n {
            cam.advance_to(cam.last_frames + 2);
        }
    }

    /// Battle entry on tutorial dialogue: the measured held close-up, static
    /// over any number of frames.
    #[test]
    fn dialogue_close_up_holds_static() {
        let mut cam = BattleCamera::new(BattleCamPhase::Dialogue, 0);
        steps(&mut cam, 120);
        assert_eq!(cam.pose(), DIALOGUE_POSE);
    }

    /// Dialogue dismiss reproduces the traced glide: pitch +6/step to 32,
    /// TR.z +864/step to 7680, yaw resuming the -4/step orbit from 0
    /// (trace frames 45..57).
    #[test]
    fn dialogue_dismiss_glide_matches_trace() {
        let mut cam = traced_cam(BattleCamPhase::Dialogue);
        cam.set_phase(BattleCamPhase::Menu);
        // Traced (pitch, yaw, z) per step; yaw 0 on the first step (the
        // orbit decrement lands from the second entry on).
        let want = [
            (6.0, 4092.0, 2502.0),
            (12.0, 4088.0, 3366.0),
            (18.0, 4084.0, 4230.0),
            (24.0, 4080.0, 5094.0),
            (30.0, 4076.0, 5958.0),
            (32.0, 4072.0, 6822.0),
            (32.0, 4068.0, 7680.0),
        ];
        for (i, (p, y, z)) in want.into_iter().enumerate() {
            steps(&mut cam, 1);
            let pose = cam.pose();
            assert_eq!((pose.pitch, pose.tr[2]), (p, z), "step {i}");
            assert_eq!(pose.yaw, y, "yaw step {i}");
        }
        // Settled: pure idle orbit thereafter.
        steps(&mut cam, 1);
        assert_eq!(cam.pose().tr, traced_menu_tr());
        assert_eq!(cam.pose().yaw, 4064.0);
    }

    /// Menu idle orbit: -4 yaw units per step, framing held.
    #[test]
    fn menu_idle_orbit_rate() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        steps(&mut cam, 10);
        assert_eq!(cam.pose().yaw, (0.0f32 - 40.0).rem_euclid(4096.0));
        assert_eq!(cam.pose().pitch, MENU_PITCH);
        assert_eq!(cam.pose().tr, traced_menu_tr());
    }

    /// Submenu open glides every component to the measured close-up in 6
    /// steps (shortest-arc yaw) and then holds it with the orbit paused.
    #[test]
    fn submenu_glide_arrives_in_six_steps_and_holds() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        // Orbit a while first (trace picks up the glide from yaw ~4024).
        steps(&mut cam, 18);
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 5);
        assert_ne!(
            cam.pose().tr,
            BattleCamActor::default().submenu_pose().tr,
            "still mid-glide"
        );
        steps(&mut cam, 1);
        let pose = cam.pose();
        assert_eq!(pose.pitch, BattleCamActor::default().submenu_pose().pitch);
        assert_eq!(pose.tr, BattleCamActor::default().submenu_pose().tr);
        assert_eq!(
            pose.yaw.rem_euclid(4096.0),
            BattleCamActor::default().submenu_pose().yaw
        );
        // Held static while the submenu stays open.
        steps(&mut cam, 30);
        assert_eq!(cam.pose(), pose);
    }

    /// Submenu exit passes through the measured swing pose (6 steps), then
    /// returns to the menu framing (7 steps) with the orbit running again.
    #[test]
    fn submenu_exit_swings_out_then_returns() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 6);
        cam.set_phase(BattleCamPhase::Menu);
        steps(&mut cam, 6);
        let swing = cam.pose();
        assert_eq!(swing.pitch, SWING_POSE.pitch);
        assert_eq!(swing.tr, SWING_POSE.tr);
        assert_eq!(swing.yaw, 0.0, "swing lands on yaw 4096 = 0");
        steps(&mut cam, 7);
        let back = cam.pose();
        assert_eq!(back.pitch, MENU_PITCH);
        assert_eq!(back.tr, traced_menu_tr());
        // Orbit ran through the 7 return steps: yaw 0 -> -28 (mod 4096).
        assert_eq!(back.yaw, 4096.0 - 28.0);
        // And keeps orbiting.
        steps(&mut cam, 1);
        assert_eq!(cam.pose().yaw, 4096.0 - 32.0);
    }

    /// The glide steps on `FUN_801D829C`'s **integer** per-frame increments,
    /// not on an exact float divide. Retail's `ceil(|delta| / duration)`
    /// overshoots slightly and clamps at the endpoint; a float rate would land
    /// a fraction short on the same step. Pick a height delta the step count
    /// does not divide and the two laws separate on the very first step.
    #[test]
    fn glide_rates_are_the_retail_ceiling_increments() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        // TR.y runs 1280 -> 1401: delta 121 over 6 steps. ceil = 21/step; an
        // exact divide would be 20.1667.
        cam.set_actor(BattleCamActor {
            facing: 0,
            height: Some(1401.0),
            world: [0.0, 0.0, -800.0],
        });
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 1);
        assert_eq!(cam.pose().tr[1], 1280.0 + 21.0);
        steps(&mut cam, 4);
        assert_eq!(cam.pose().tr[1], 1280.0 + 105.0);
        // The last step clamps rather than overshooting to 1406.
        steps(&mut cam, 1);
        assert_eq!(cam.pose().tr[1], 1401.0);
    }

    /// The builder owns the TR.z projection prescale, so a glide handed the
    /// raw world Z converges on exactly the value the framing cases publish.
    #[test]
    fn glide_endpoints_agree_with_the_framing_cases() {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
        assert_eq!(
            cam.pose().tr[2],
            BattleCamActor::default().submenu_pose().tr[2]
        );
        cam.set_phase(BattleCamPhase::Menu);
        steps(&mut cam, (SUBMENU_SWING_STEPS + SWING_RETURN_STEPS) as u64);
        assert_eq!(cam.pose().tr[2], traced_menu_tr()[2]);
        // And the raw-Z helper is what `menu_framing` prescales.
        assert_eq!(
            prescale_tr_z(menu_raw_z(Some(traced_formation()))),
            traced_menu_tr()[2]
        );
    }

    /// The shortest-arc unwrap goes the short way in both directions.
    #[test]
    fn submenu_yaw_takes_shortest_arc() {
        // From yaw 800 the short way to 2288 is +1488 (forward).
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
        cam.pose.yaw = 800.0;
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 1);
        assert!(cam.pose().yaw > 800.0);
        // From yaw 3500 the short way to 2288 is -1212 (backward).
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
        cam.pose.yaw = 3500.0;
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 1);
        assert!(cam.pose().yaw < 3500.0);
        steps(&mut cam, 5);
        assert_eq!(cam.pose().yaw, BattleCamActor::default().submenu_pose().yaw);
    }

    /// `phase_for` is the shared boolean mapping: dialogue outranks the
    /// submenu (retail's tutorial text draws over an open menu).
    #[test]
    fn phase_for_maps_the_battle_state() {
        assert_eq!(phase_for(true, false), BattleCamPhase::Dialogue);
        assert_eq!(phase_for(true, true), BattleCamPhase::Dialogue);
        assert_eq!(phase_for(false, true), BattleCamPhase::Submenu);
        assert_eq!(phase_for(false, false), BattleCamPhase::Menu);
    }

    /// `drive` owns the whole per-frame ordering: entry snap, retarget,
    /// phase change, step - and drops the state when the battle ends.
    #[test]
    fn drive_creates_steps_and_drops() {
        let mut slot: Option<BattleCamera> = None;
        // Inactive: stays empty.
        drive(&mut slot, false, BattleCamPhase::Menu, None, None, 0);
        assert!(slot.is_none());
        // First active frame in the Menu phase: entry snap to the far
        // framing (BOOT depth - no formation installed yet on frame 0).
        drive(&mut slot, true, BattleCamPhase::Menu, None, None, 0);
        let p0 = slot.as_ref().unwrap().pose();
        assert_eq!((p0.pitch, p0.tr), (BOOT_POSE.pitch, BOOT_POSE.tr));
        // Formation + 2 frames: the framing resizes and the orbit runs.
        let formation = Some(FormationBox {
            min: [-800.0, -800.0],
            max: [800.0, 800.0],
        });
        drive(&mut slot, true, BattleCamPhase::Menu, None, formation, 2);
        let p1 = slot.as_ref().unwrap().pose();
        assert_eq!(p1.yaw, 4092.0, "one orbit step");
        // Submenu opens on the traced default seat: 6 steps arrive on the
        // close-up.
        for f in 2..8 {
            drive(
                &mut slot,
                true,
                BattleCamPhase::Submenu,
                None,
                formation,
                f * 2,
            );
        }
        let p2 = slot.as_ref().unwrap().pose();
        assert_eq!(p2.tr, BattleCamActor::default().submenu_pose().tr);
        // Battle ends: the state drops so the next battle re-snaps.
        drive(&mut slot, false, BattleCamPhase::Menu, None, None, 16);
        assert!(slot.is_none());
    }

    /// A Menu-phase entry with the formation already live snaps straight to
    /// the case-9 formation-sized framing - no degenerate minimum-depth
    /// frame while waiting for the first phase transition.
    #[test]
    fn drive_entry_sizes_to_the_live_formation() {
        let formation = Some(FormationBox {
            min: [-800.0, -800.0],
            max: [800.0, 800.0],
        });
        let mut slot: Option<BattleCamera> = None;
        drive(&mut slot, true, BattleCamPhase::Menu, None, formation, 0);
        let p = slot.as_ref().unwrap().pose();
        assert_eq!(p.tr, [0.0, 1280.0, 7680.0], "the traced far framing");
        assert_eq!(p.focus, [0.0; 3]);
    }

    /// A battle that opens on dialogue snaps to the held close-up; any other
    /// entry snaps to the far framing.
    #[test]
    fn drive_entry_snap_follows_the_opening_phase() {
        let mut slot: Option<BattleCamera> = None;
        drive(&mut slot, true, BattleCamPhase::Dialogue, None, None, 0);
        assert_eq!(slot.as_ref().unwrap().pose(), DIALOGUE_POSE);
        let mut slot: Option<BattleCamera> = None;
        // Opening straight into a submenu still enters at the menu framing
        // and glides in (retail's loading pose resolves at the far framing).
        drive(&mut slot, true, BattleCamPhase::Submenu, None, None, 0);
        let p = slot.as_ref().unwrap().pose();
        assert_ne!(p.tr, BattleCamActor::default().submenu_pose().tr);
    }

    /// Project a point through [`battle_vp`] composed with the same
    /// per-draw model factors the hosts use, to PSX 320x240 screen pixels.
    fn project(vp: &[f32; 16], model: &[f32; 16], v: [f32; 3]) -> Option<(f32, f32)> {
        let m = mat_mul(vp, model);
        let x = m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12];
        let y = m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13];
        let w = m[3] * v[0] + m[7] * v[1] + m[11] * v[2] + m[15];
        if w <= 1.0 {
            return None;
        }
        Some((160.0 + x / w * 160.0, 120.0 - y / w * 120.0))
    }

    /// The Y-flip model factor every host draw carries.
    const FLIP: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, -1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];

    /// [`battle_vp`] reproduces the exact retail projection
    /// `screen = H*(Rx(p)*Ry(y)*(v*S - focus*S) + TR)/Ez` against a
    /// hand-rolled reference, dome (raw units) and actor (4x world scale)
    /// draw classes both.
    #[test]
    fn battle_vp_matches_the_handrolled_retail_projection() {
        let pose = BattleCamPose {
            pitch: 32.0,
            yaw: 224.0,
            tr: [0.0, 1280.0, 7680.0],
            focus: [100.0, 0.0, -50.0],
        };
        let handrolled = |v: [f32; 3], s: f32| -> Option<(f32, f32)> {
            let to_rad = |u: f32| u / 4096.0 * std::f32::consts::TAU;
            let (sy, cy) = to_rad(pose.yaw).sin_cos();
            let (sp, cp) = to_rad(pose.pitch).sin_cos();
            // World-scale the vertex and subtract the scaled focus.
            let p = [
                v[0] * s - pose.focus[0] * 4.0,
                v[1] * s - pose.focus[1] * 4.0,
                v[2] * s - pose.focus[2] * 4.0,
            ];
            let ry = [cy * p[0] + sy * p[2], p[1], -sy * p[0] + cy * p[2]];
            let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
            let ez = e[2] + pose.tr[2];
            if ez <= 1.0 {
                return None;
            }
            Some((
                256.0 * (e[0] + pose.tr[0]) / ez + 160.0,
                256.0 * (e[1] + pose.tr[1]) / ez + 120.0,
            ))
        };
        let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
        // Dome class: raw PSX vertices, model = FLIP (the vp's trailing flip
        // cancels it, so the retail chain sees the raw Y-down vertex).
        for v in [[1000.0f32, -500.0, 3000.0], [-2000.0, 0.0, 6000.0]] {
            let got = project(&vp, &FLIP, v).unwrap();
            // The dome draws unscaled but the camera focus is scaled - the
            // handrolled reference scales the vertex by 1 and focus by 4.
            let want = handrolled(v, 1.0).unwrap();
            let d = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
            assert!(d < 0.05, "dome {v:?}: {d}px ({got:?} vs {want:?})");
        }
        // Actor class: model composes scale(4)*FLIP under the camera.
        let scale4: [f32; 16] = [
            4.0, 0.0, 0.0, 0.0, //
            0.0, 4.0, 0.0, 0.0, //
            0.0, 0.0, 4.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let model = mat_mul(&scale4, &FLIP);
        for v in [[100.0f32, -130.0, -800.0], [0.0, 0.0, 800.0]] {
            let got = project(&vp, &model, v).unwrap();
            let want = handrolled(v, 4.0).unwrap();
            let d = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
            assert!(d < 0.05, "actor {v:?}: {d}px ({got:?} vs {want:?})");
        }
    }

    /// The far menu framing keeps the whole formation inside the 320x240
    /// frame - the property the browser's old orbit-projection approximation
    /// broke (party out of shot at some angles).
    #[test]
    fn menu_framing_keeps_the_formation_on_screen() {
        let formation = FormationBox {
            min: [-800.0, -900.0],
            max: [800.0, 800.0],
        };
        let scale4: [f32; 16] = [
            4.0, 0.0, 0.0, 0.0, //
            0.0, 4.0, 0.0, 0.0, //
            0.0, 0.0, 4.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        let model = mat_mul(&scale4, &FLIP);
        for yaw in [0.0f32, 512.0, 1024.0, 2048.0, 3000.0, 3900.0] {
            let pose = menu_framing(Some(formation), yaw);
            let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
            for (x, z) in [
                (formation.min[0], formation.min[1]),
                (formation.min[0], formation.max[1]),
                (formation.max[0], formation.min[1]),
                (formation.max[0], formation.max[1]),
            ] {
                // Seat position + a standing character's head (~130 raw
                // units up = negative retail Y; the model/vp flips cancel,
                // so the raw Y-down coordinate goes straight in).
                for y in [0.0f32, -130.0] {
                    let (sx, sy) = project(&vp, &model, [x, y, z])
                        .expect("formation corner in front of the camera");
                    assert!(
                        (-40.0..360.0).contains(&sx) && (-40.0..280.0).contains(&sy),
                        "yaw {yaw}: corner ({x},{z}) h {y} off-frame at ({sx},{sy})"
                    );
                }
            }
        }
    }
}
