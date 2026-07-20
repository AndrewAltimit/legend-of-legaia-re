//! Phase-scripted retail battle camera (game mode `0x15`).
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
//! REF: FUN_801D5854 (the framing cases), FUN_801D829C (the
//! battle camera angle-tween builder these glides ride in retail; the
//! fixed-point kernel port lives at `legaia_engine_vm::battle_camera`).

/// Battle-camera framing phase, derived from the live battle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BattleCamPhase {
    /// An in-battle dialogue box is up (the tutorial text).
    Dialogue,
    /// No menu owns the pad: the far framing with the idle orbit.
    Menu,
    /// A command / arts / spell / item submenu is open.
    Submenu,
}

/// One camera pose: 12-bit angle units (`4096` = full turn) + the eye-space
/// translation trio, exactly the retail globals' value space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BattleCamPose {
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

/// `FUN_801D829C`'s TR.z prescale: world distance -> GTE projection units.
/// `0xA0` = 160 = PSX screen half-width; `<< 8` = GTE `H = 256`. The divide
/// truncates, which is why the traced `0x600` lands on `2457`, not `2457.6`.
const fn prescale_tr_z(raw: i32) -> f32 {
    ((raw << 8) / 0xA0) as f32
}

/// Camera height used when the host has no disc table to resolve
/// `0x801F4D2C` from. Vahn's entry, the one value the solo-Vahn camera trace
/// observes, so an unpinned character frames like the measured case instead
/// of jumping. Real per-character heights come from
/// `legaia_asset::battle_camera_table` via [`BattleCamActor::height`].
pub(crate) const SUBMENU_HEIGHT_FALLBACK: f32 = 1152.0; // 0x480

/// The acting battle actor the submenu framing is built around.
///
/// `facing` is retail `actor[+0x46]` (12-bit angle), `world` the actor
/// position at `actor[+0x34/+0x36/+0x38]`, and `height` the `TR.y` the host
/// resolved out of the disc table `0x801F4D2C` for this actor's character id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BattleCamActor {
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
    pub(crate) fn submenu_pose(self) -> BattleCamPose {
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
pub(crate) struct FormationBox {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

/// Retail's case-9 far framing for a formation. `None` (no present actors)
/// keeps the minimum depth and the origin focus, which is what retail's
/// un-entered min/max accumulators degenerate to.
pub(crate) fn menu_framing(bbox: Option<FormationBox>, yaw: f32) -> BattleCamPose {
    let (span, focus) = match bbox {
        Some(b) => {
            let dx = b.max[0] - b.min[0];
            let dz = b.max[1] - b.min[1];
            // Retail keeps the LARGER of the two extents, so a formation that
            // is wide but shallow still fits the frame.
            let span = dx.max(dz);
            let centre = [
                (b.min[0] + b.max[0]) * 0.5,
                0.0,
                (b.min[1] + b.max[1]) * 0.5,
            ];
            (span, centre)
        }
        None => (0.0, [0.0; 3]),
    };
    // `span * 3`, clamped up to the 0x800 floor, then through the same
    // projection prescale every TR.z takes.
    let raw = (span * MENU_SPAN_SCALE).max(MENU_TR_Z_MIN_RAW);
    BattleCamPose {
        pitch: MENU_PITCH,
        yaw,
        tr: [MENU_TR_X, MENU_TR_Y, prescale_tr_z(raw as i32)],
        focus,
    }
}
/// Over-the-shoulder swing pose the submenu exit passes through
/// (trace frames 163..173; yaw target is `4096` = `0` unwrapped upward from
/// `2288`). Retail case `1` orbits the acting actor like the close-up does,
/// so the focus is filled in per swing rather than baked here.
const SWING_POSE: BattleCamPose = BattleCamPose {
    pitch: 256.0,
    yaw: 4096.0,
    tr: [0.0, 1536.0, 3276.0],
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
    fn linear(from: &BattleCamPose, mut target: BattleCamPose, steps: u32, yaw: bool) -> Self {
        if yaw {
            // Shortest arc: unwrap the target so the signed delta is the
            // short way round (retail's 12-bit wrap-adjust).
            let delta = (target.yaw - from.yaw).rem_euclid(4096.0);
            let delta = if delta > 2048.0 {
                delta - 4096.0
            } else {
                delta
            };
            target.yaw = from.yaw + delta;
        }
        let steps_f = steps.max(1) as f32;
        let rate = [
            (target.pitch - from.pitch).abs() / steps_f,
            (target.yaw - from.yaw).abs() / steps_f,
            (target.tr[0] - from.tr[0]).abs() / steps_f,
            (target.tr[1] - from.tr[1]).abs() / steps_f,
            (target.tr[2] - from.tr[2]).abs() / steps_f,
            (target.focus[0] - from.focus[0]).abs() / steps_f,
            (target.focus[1] - from.focus[1]).abs() / steps_f,
            (target.focus[2] - from.focus[2]).abs() / steps_f,
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
pub(crate) struct BattleCamera {
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

impl BattleCamera {
    /// New camera snapped to the entry phase's framing (a battle that opens
    /// on tutorial dialogue starts in the held close-up; any other battle
    /// starts at the far menu framing).
    pub(crate) fn new(phase: BattleCamPhase, frames_now: u64) -> Self {
        let actor = BattleCamActor::default();
        let pose = match phase {
            BattleCamPhase::Dialogue => DIALOGUE_POSE,
            BattleCamPhase::Submenu => actor.submenu_pose(),
            BattleCamPhase::Menu => menu_framing(None, 0.0),
        };
        BattleCamera {
            phase,
            pose,
            glides: std::collections::VecDeque::new(),
            last_frames: frames_now,
            frame_accum: 0,
            actor,
            formation: None,
        }
    }

    /// Install the formation the far menu framing sizes itself to (retail's
    /// per-frame `min`/`max` walk over the present actors). Hosts call this
    /// as the battle formation changes; an already-armed glide is left alone.
    pub(crate) fn set_formation(&mut self, formation: Option<FormationBox>) {
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
    pub(crate) fn set_actor(&mut self, actor: BattleCamActor) {
        self.actor = actor;
    }

    /// Current camera pose (12-bit angle units + eye-space TR).
    pub(crate) fn pose(&self) -> BattleCamPose {
        self.pose
    }

    /// Observe the live battle phase; a change arms the measured glide.
    pub(crate) fn set_phase(&mut self, phase: BattleCamPhase) {
        if phase == self.phase {
            return;
        }
        let from = self.pose;
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
                        &from,
                        BattleCamPose {
                            focus: self.actor.world,
                            ..SWING_POSE
                        },
                        SUBMENU_SWING_STEPS,
                        true,
                    );
                    let back =
                        Glide::linear(&swing.target, self.menu_pose(), SWING_RETURN_STEPS, false);
                    self.glides.push_back(swing);
                    self.glides.push_back(back);
                }
            }
            BattleCamPhase::Submenu => {
                self.glides.push_back(Glide::linear(
                    &from,
                    self.actor.submenu_pose(),
                    SUBMENU_ENTER_STEPS,
                    true,
                ));
            }
            BattleCamPhase::Dialogue => {
                // Retail never re-enters the dialogue close-up mid-battle;
                // snap defensively.
                self.pose = DIALOGUE_POSE;
            }
        }
        self.phase = phase;
    }

    /// Advance to the world's retail-frame counter, stepping the camera once
    /// per 2 display frames (the measured cadence: every trace entry is an
    /// even frame apart).
    pub(crate) fn advance_to(&mut self, frames_now: u64) {
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

    /// Build the case-9 formation box from retail seat rows.
    fn seats_box(
        party: &[legaia_engine_core::battle_seats::Seat],
        monsters: &[legaia_engine_core::battle_seats::Seat],
    ) -> FormationBox {
        let mut b: Option<FormationBox> = None;
        for s in party.iter().chain(monsters) {
            let (x, z) = (s.x as f32, s.z as f32);
            match &mut b {
                None => {
                    b = Some(FormationBox {
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
        b.expect("non-empty formation")
    }

    /// **The independent check on the case-9 law.** The traced far framing
    /// (`TR.z = 7680`) was measured on the solo-Vahn tutorial fight. Feeding
    /// that fight's *retail seat table* rows - party count 1, monster count 1
    /// (`FUN_800513F0`'s `0x800775C8` / `0x80077608`) - through the formation
    /// law reproduces `7680` exactly, with nothing fitted to the trace: the
    /// seats give a 1600-unit Z span, `1600 * 3 = 0x12C0`, and the prescale
    /// lands on `7680`. A law that merely happened to pass through the
    /// measured point would not also land on the seat geometry that produced
    /// it.
    #[test]
    fn traced_menu_depth_falls_out_of_the_retail_seat_tables() {
        use legaia_engine_core::battle_seats::{MONSTER_SEATS, PARTY_SEATS};
        let solo = seats_box(&PARTY_SEATS[0][..1], &MONSTER_SEATS[0][..1]);
        assert_eq!(solo.max[1] - solo.min[1], 1600.0, "Vahn -800 vs Tetsu +800");
        let p = menu_framing(Some(solo), 0.0);
        assert_eq!(p.tr[2], 7680.0, "the traced far-framing depth");
        assert_eq!(p.focus, [0.0, 0.0, 0.0], "the fight is centred on origin");
        // And that box IS what the traced formation stands in for.
        assert_eq!(p.tr, traced_menu_tr());
    }

    /// A real three-member party pulls the camera further back than the solo
    /// fight the trace captured - the framing difference a solo-Vahn trace
    /// structurally cannot observe.
    #[test]
    fn multi_member_formations_frame_wider_than_the_traced_solo_fight() {
        use legaia_engine_core::battle_seats::{MONSTER_SEATS, PARTY_SEATS};
        let solo = menu_framing(
            Some(seats_box(&PARTY_SEATS[0][..1], &MONSTER_SEATS[0][..1])),
            0.0,
        );
        let trio = menu_framing(
            Some(seats_box(&PARTY_SEATS[2], &MONSTER_SEATS[0][..1])),
            0.0,
        );
        // 3 party + 1 monster spans -825..800 in Z = 1625 > the solo 1600.
        assert!(
            trio.tr[2] > solo.tr[2],
            "trio {:?} vs solo {:?}",
            trio.tr,
            solo.tr
        );
        assert_eq!(trio.tr[2], prescale_tr_z(1625 * 3));
        // A four-monster row widens it in X instead, and X now wins.
        let crowd = menu_framing(Some(seats_box(&PARTY_SEATS[2], &MONSTER_SEATS[3])), 0.0);
        assert_eq!(
            crowd.tr[2],
            prescale_tr_z(1800 * 3),
            "X span 1800 dominates"
        );
        assert!(crowd.tr[2] > trio.tr[2]);
    }

    /// Disc-gated: the per-character heights the submenu close-up reads come
    /// off the real battle-action overlay, and each of the three playable
    /// members frames at its own height. Skips and passes without a disc.
    #[test]
    fn real_disc_heights_give_each_member_its_own_framing() {
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
            return;
        }
        let mut prot = None;
        for base in ["extracted", "../../extracted"] {
            let p = std::path::PathBuf::from(base).join("PROT.DAT");
            if p.is_file() {
                prot = Some(p);
                break;
            }
        }
        let Some(prot) = prot else {
            eprintln!("[skip] extracted/PROT.DAT missing");
            return;
        };
        let mut archive = legaia_prot::archive::Archive::open(&prot).expect("open PROT.DAT");
        let entry = archive
            .entries
            .get(legaia_asset::battle_camera_table::BATTLE_ACTION_OVERLAY_PROT_INDEX)
            .cloned()
            .expect("PROT 0898");
        let mut bytes = Vec::new();
        archive
            .read_entry(&entry, &mut bytes)
            .expect("read PROT 0898");
        let table = legaia_asset::battle_camera_table::parse(&bytes).expect("height table");

        // Vahn's entry is the one the solo trace pinned, so it anchors the
        // table to the measurement.
        assert_eq!(
            table.height_for_char_id(1).map(|h| h as f32),
            Some(SUBMENU_HEIGHT_FALLBACK),
            "Vahn's disc height is the traced fallback"
        );
        // The three battle-party members each frame at their own height.
        let poses: Vec<BattleCamPose> = (1..=3u8)
            .map(|id| {
                BattleCamActor {
                    facing: 0,
                    height: table.height_for_char_id(id).map(|h| h as f32),
                    world: [0.0, 0.0, -800.0],
                }
                .submenu_pose()
            })
            .collect();
        for (i, a) in poses.iter().enumerate() {
            for b in &poses[i + 1..] {
                assert_ne!(a.tr[1], b.tr[1], "distinct per-character heights");
                assert_eq!(a.tr[0], b.tr[0]);
                assert_eq!(a.tr[2], b.tr[2]);
            }
        }
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
}
