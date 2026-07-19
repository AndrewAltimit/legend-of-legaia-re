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
//!   pitch `32`, TR `(0, 1280, 7680)`, idle orbit `-4` yaw units per
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
//! `TR.y` is the only genuine table: `0x801F4D2C + char_id * 2`, keyed on
//! **character identity** (`DAT_8007BD10[slot] - 1`, the party-record
//! selector), not on seat - a per-model height offset. See
//! [`SUBMENU_HEIGHT`] for what is pinned.
//!
//! REF: FUN_801D5854 (the mode-0 submenu framing), FUN_801D829C (the
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
}

/// Tutorial-dialogue close-up (trace frames 1..45: 240+ frames static).
const DIALOGUE_POSE: BattleCamPose = BattleCamPose {
    pitch: 0.0,
    yaw: 0.0,
    tr: [0.0, 1280.0, 1638.0],
};
/// Far Begin/Run framing (pitch / TR; yaw free-orbits).
const MENU_PITCH: f32 = 32.0;
const MENU_TR: [f32; 3] = [0.0, 1280.0, 7680.0];
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

/// Per-**character** camera height (retail `0x801F4D2C + char_id * 2`),
/// indexed by character id (`0` = Vahn, `1` = Noa, `2` = Gala, `3` = Terra
/// - i.e. `DAT_8007BD10[slot] - 1`).
///
/// Only Vahn's entry is pinned from the trace (`0x480` = 1152). The other
/// three are read out of the battle-action overlay at boot when available;
/// this table is the fallback, and defaults them to Vahn's height so an
/// unpinned character frames like the measured case instead of jumping.
/// Reading `0x801F4D2C` out of the overlay closes the gap.
pub(crate) const SUBMENU_HEIGHT: [f32; 4] = [1152.0, 1152.0, 1152.0, 1152.0];

/// The acting battle actor the submenu framing is built around.
///
/// `facing` is retail `actor[+0x46]` (12-bit angle), `char_id` the party
/// record selector (`DAT_8007BD10[slot] - 1`), `world` the actor position
/// at `actor[+0x34/+0x36/+0x38]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BattleCamActor {
    pub facing: i32,
    pub char_id: u8,
    pub world: [f32; 3],
}

impl Default for BattleCamActor {
    /// The measured solo-Vahn case: character `0`, facing `0`, seated at
    /// the traced `(0, -800)`. Reproduces the originally pinned framing
    /// exactly, so an un-wired host keeps the measured behaviour.
    fn default() -> Self {
        BattleCamActor {
            facing: 0,
            char_id: 0,
            world: [0.0, 0.0, -800.0],
        }
    }
}

impl BattleCamActor {
    /// Retail's mode-0 submenu framing for this actor (`FUN_801D5854`).
    pub(crate) fn submenu_pose(self) -> BattleCamPose {
        BattleCamPose {
            pitch: SUBMENU_PITCH,
            yaw: (SUBMENU_YAW_BASE - self.facing).rem_euclid(4096) as f32,
            tr: [
                SUBMENU_TR_X,
                SUBMENU_HEIGHT[(self.char_id as usize).min(SUBMENU_HEIGHT.len() - 1)],
                prescale_tr_z(SUBMENU_TR_Z_RAW),
            ],
        }
    }

    /// Retail's focus trio (`0x80089118/1C/20`) - the negated actor
    /// position. This is where the per-seat variation actually lives.
    ///
    /// Not yet consumed by the window's battle MVP, which composes only the
    /// rotation + translation trios; wiring the focus trio through
    /// `battle_camera_mvp` is what makes a non-solo party frame on the
    /// acting member instead of the formation centre.
    #[allow(dead_code)]
    pub(crate) fn focus(self) -> [f32; 3] {
        [-self.world[0], -self.world[1], -self.world[2]]
    }
}
/// Over-the-shoulder swing pose the submenu exit passes through
/// (trace frames 163..173; yaw target is `4096` = `0` unwrapped upward
/// from `2288`).
const SWING_POSE: BattleCamPose = BattleCamPose {
    pitch: 256.0,
    yaw: 4096.0,
    tr: [0.0, 1536.0, 3276.0],
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
    /// Per-step absolute rates: `[pitch, yaw, tr.x, tr.y, tr.z]`.
    rate: [f32; 5],
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
            BattleCamPhase::Menu => BattleCamPose {
                pitch: MENU_PITCH,
                yaw: 0.0,
                tr: MENU_TR,
            },
        };
        BattleCamera {
            phase,
            pose,
            glides: std::collections::VecDeque::new(),
            last_frames: frames_now,
            frame_accum: 0,
            actor,
        }
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
                        target: BattleCamPose {
                            pitch: MENU_PITCH,
                            yaw: 0.0,
                            tr: MENU_TR,
                        },
                        rate: [
                            DIALOGUE_EXIT_PITCH_RATE,
                            0.0,
                            f32::INFINITY,
                            f32::INFINITY,
                            DIALOGUE_EXIT_Z_RATE,
                        ],
                        yaw_glides: false,
                        steps_left: None,
                    });
                } else {
                    // Submenu exit: swing up over the shoulder, then ease
                    // back down to the menu framing (orbit resumes for the
                    // return segment - retail re-enters at yaw 0).
                    let swing = Glide::linear(&from, SWING_POSE, SUBMENU_SWING_STEPS, true);
                    let back = Glide::linear(
                        &swing.target,
                        BattleCamPose {
                            pitch: MENU_PITCH,
                            yaw: 0.0,
                            tr: MENU_TR,
                        },
                        SWING_RETURN_STEPS,
                        false,
                    );
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
                if g.yaw_glides {
                    self.pose.yaw = g.target.yaw;
                }
                true
            }
            Some(n) => {
                if let Some(front) = self.glides.front_mut() {
                    front.steps_left = Some(n - 1);
                }
                self.pose.pitch = step_toward(self.pose.pitch, g.target.pitch, g.rate[0]);
                for k in 0..3 {
                    self.pose.tr[k] = step_toward(self.pose.tr[k], g.target.tr[k], g.rate[2 + k]);
                }
                if g.yaw_glides {
                    self.pose.yaw = step_toward(self.pose.yaw, g.target.yaw, g.rate[1]);
                }
                false
            }
            // Rate-clamped glide: each component clamps independently.
            None => {
                self.pose.pitch = step_toward(self.pose.pitch, g.target.pitch, g.rate[0]);
                for k in 0..3 {
                    self.pose.tr[k] = step_toward(self.pose.tr[k], g.target.tr[k], g.rate[2 + k]);
                }
                if g.yaw_glides {
                    self.pose.yaw = step_toward(self.pose.yaw, g.target.yaw, g.rate[1]);
                }
                self.pose.pitch == g.target.pitch
                    && self.pose.tr == g.target.tr
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

    /// TR.x / TR.z / pitch are seat- and character-invariant constants.
    #[test]
    fn submenu_constants_do_not_vary_by_actor() {
        for facing in [0, 700, 2048, 4095] {
            for char_id in 0..4u8 {
                let p = BattleCamActor {
                    facing,
                    char_id,
                    world: [1.0, 2.0, 3.0],
                }
                .submenu_pose();
                assert_eq!(p.pitch, 32.0);
                assert_eq!(p.tr[0], -512.0);
                assert_eq!(p.tr[2], 2457.0);
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

    /// The per-seat half of the framing: focus is the negated actor
    /// position, so the camera orbits about whoever is acting.
    #[test]
    fn focus_is_the_negated_actor_position() {
        let a = BattleCamActor {
            facing: 0,
            char_id: 1,
            world: [640.0, -128.0, -800.0],
        };
        assert_eq!(a.focus(), [-640.0, 128.0, 800.0]);
        // Two seats at different positions frame differently even though
        // their pose trios agree - which is why a solo trace saw one pose.
        let b = BattleCamActor {
            world: [-640.0, -128.0, -800.0],
            ..a
        };
        assert_eq!(a.submenu_pose().tr, b.submenu_pose().tr);
        assert_ne!(a.focus(), b.focus());
    }

    /// Retargeting the camera at a different seat moves the glide target.
    #[test]
    fn set_actor_retargets_the_submenu_glide() {
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
        cam.set_actor(BattleCamActor {
            facing: 1024,
            char_id: 2,
            world: [640.0, 0.0, -800.0],
        });
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, 2 * SUBMENU_ENTER_STEPS as u64);
        assert_eq!(cam.pose().yaw.rem_euclid(4096.0), 1264.0);
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
        let mut cam = BattleCamera::new(BattleCamPhase::Dialogue, 0);
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
        assert_eq!(cam.pose().tr, MENU_TR);
        assert_eq!(cam.pose().yaw, 4064.0);
    }

    /// Menu idle orbit: -4 yaw units per step, framing held.
    #[test]
    fn menu_idle_orbit_rate() {
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
        steps(&mut cam, 10);
        assert_eq!(cam.pose().yaw, (0.0f32 - 40.0).rem_euclid(4096.0));
        assert_eq!(cam.pose().pitch, MENU_PITCH);
        assert_eq!(cam.pose().tr, MENU_TR);
    }

    /// Submenu open glides every component to the measured close-up in 6
    /// steps (shortest-arc yaw) and then holds it with the orbit paused.
    #[test]
    fn submenu_glide_arrives_in_six_steps_and_holds() {
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
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
        let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
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
        assert_eq!(back.tr, MENU_TR);
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
