//! [`PhaseGate`]: the point in an in-flight action a capture was taken at,
//! as a predicate the engine replay can be stopped on, with its environment
//! hand-off.

use legaia_engine_mode::mode::SceneMode;

/// The caster's invoke clip through the summon band (`0x32` stages
/// `actor[+0x1DA] = 9`).
pub const SUMMON_INVOKE_CLIP: u8 = 9;
/// The summon band's head, `FUN_801E295C` state `0x32`.
pub const SUMMON_INVOKE_STATE: u8 = 0x32;
/// PROT 0903 (Gimard) and its walk-in arm, the one module arm that hands
/// the camera to case 6 (`FUN_801D5854(7, 6)`).
pub const GIMARD_MODULE: u32 = 903;
pub const GIMARD_WALK_ARM: u8 = 11;

/// Where in the summon band a capture sits, as an engine frame has to be
/// gated to match it: the action-SM state, and - while a flash is up - the
/// same flash the same number of vsyncs in. `play-window` reads it as
/// `LEGAIA_CAPTURE_GATE` and captures the first frame it holds; the headless
/// seed samples on the same predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseGate {
    pub action_state: u8,
    pub fade: Option<RetailFade>,
    /// The summon module's phase `ctx[+0x279]`, gated on only in `0x35` /
    /// `0x36` and only for a module whose arms the engine paces on retail's
    /// own countdown (`cast_module_camera::module_director`): there the band's
    /// length is the module's, so the state alone does not place the frame.
    pub module_phase: Option<u8>,
    /// For a `0x33` capture with no flash up yet: retail's close-up
    /// accumulator `ctx[+0x87C]`, which the invoke clip's commit zeroed and
    /// which then gains `8` a vsync. The state alone does not place such a
    /// frame - `0x33` runs until the clip's first effect record fires - so
    /// the engine frame is the first one where its caster has committed the
    /// same clip and the engine's own `ctx[+0x87C]` has reached the value.
    pub cam_accum: Option<u32>,
    /// For a capture inside PROT 0903's walk arm (`11`): retail's yaw base
    /// `ctx[+0x6DA]`, which arm 10 seats at `0x200` and the walk swings by
    /// `6 * scalar * delta` a pass. The module phase is the arm's entry; this
    /// is how far into the walk the frame is.
    pub walk_yaw: Option<u16>,
    /// For a capture in the Done band's hold (`0x51` / `0x52`): retail's
    /// countdown `ctx[+0x6D8]` ([`crate::battle_drive::SpanGate::DoneHold`]). The band holds
    /// `0x3C` frames, or `0x96` behind a magic level-up banner, so the state
    /// alone places the frame at the hold's first vsync -
    /// `shiny_refactor_gimard_levelup` was taken 46 vsyncs in.
    pub done_hold: Option<i16>,
    /// For a capture inside one of PROT 0903's countdown-paced arms
    /// (`1..=10`): retail's module countdown
    /// ([`legaia_engine_cast::cast_module_camera::GIMARD_COUNTDOWN_VA`]). The
    /// arm alone places the frame at the arm's first pass - the
    /// `shiny_refactor_gimard_plus35` frame was taken at arm 9's entry, 16
    /// vsyncs before retail's (countdown `928` against the arm's opening
    /// value) - so the engine frame is the first one whose own countdown
    /// has drained as far.
    pub module_countdown: Option<i32>,
    /// For a capture in the summon close-up `0x33` / `0x34`: retail's
    /// close-up accumulator `ctx[+0x87C]` at the gated instant. Not a
    /// predicate - the flash's age or [`Self::cam_accum`] places the frame -
    /// but the value the engine's accumulator is aligned to once the gate
    /// holds ([`Self::align_camera`]).
    pub accum_align: Option<u32>,
    /// Whether the frame is the state after a whole pass of its camera: set
    /// for a `0x32` capture's RAM phase ([`Self::steer`], [`Self::met`]).
    /// The displayed frame is two game frames older than the RAM, which on a
    /// `0x32` capture is the pass that entered the state, so the display
    /// gate leaves it clear.
    pub camera_pass: bool,
    /// The capture sits on a module arm that polls the creature stream's CD
    /// read (`FUN_8003F2B8(1)`,
    /// `legaia_engine_cast::cast_module_camera::stream_poll_arm`). Retail
    /// holds the arm for as long as the drive takes - four vsyncs in one
    /// PCSX-Redux cast, twenty in `nova_summon_mid_cast`'s Mednafen state -
    /// while the engine's stream is resident and the arm passes on its first
    /// tick. The clocks that ran on through the wait (the flash's age) then
    /// cannot place the frame: the engine's frame is its tick on the arm.
    pub stream_wait: bool,
    /// The capture sits on a module arm that waits on the side-band applier
    /// stage `ctx[+0x276]`
    /// (`legaia_engine_cast::cast_module_camera::sideband_wait_arm`). Unlike
    /// a stream poll, the clocks that run through this wait (the flash's
    /// age) still place the frame: the replay holds the engine's arm busy
    /// ([`Self::steer`]) until they do, as retail's read held its own.
    pub sideband_wait: bool,
}

/// How far a walk-arm yaw base has swung from its seat (`0x200`), in
/// 12-bit units.
pub fn walk_swing(yaw: i32) -> i32 {
    (yaw - legaia_engine_cast::cast_module_camera::GIMARD_WALK_YAW_BASE) & 0xFFF
}

impl PhaseGate {
    /// `state[,white|black,age]`.
    pub fn to_env(&self) -> String {
        let mut s = self.env_state_and_fade();
        if let Some(p) = self.module_phase {
            s.push_str(&format!(",m{p}"));
        }
        if self.stream_wait {
            s.push_str(",s");
        }
        if self.sideband_wait {
            s.push_str(",h");
        }
        if let Some(a) = self.cam_accum {
            s.push_str(&format!(",a{a}"));
        }
        if let Some(y) = self.walk_yaw {
            s.push_str(&format!(",y{y}"));
        }
        if let Some(d) = self.done_hold {
            s.push_str(&format!(",d{d}"));
        }
        if let Some(c) = self.module_countdown {
            s.push_str(&format!(",c{c}"));
        }
        if let Some(k) = self.accum_align {
            s.push_str(&format!(",k{k}"));
        }
        if self.camera_pass {
            s.push_str(",p");
        }
        s
    }

    pub(crate) fn env_state_and_fade(&self) -> String {
        match self.fade {
            None => format!("{}", self.action_state),
            Some(f) => format!(
                "{},{},{}",
                self.action_state,
                if f.to_white { "white" } else { "black" },
                f.age
            ),
        }
    }

    pub fn from_env(s: &str) -> Option<Self> {
        let mut parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let camera_pass = parts.last() == Some(&"p");
        if camera_pass {
            parts.pop();
        }
        let accum_align = match parts.last() {
            Some(t) if t.starts_with('k') => {
                let k = t[1..].parse().ok()?;
                parts.pop();
                Some(k)
            }
            _ => None,
        };
        let module_countdown = match parts.last() {
            Some(t) if t.starts_with('c') => {
                let c = t[1..].parse().ok()?;
                parts.pop();
                Some(c)
            }
            _ => None,
        };
        let done_hold = match parts.last() {
            Some(t) if t.starts_with('d') => {
                let d = t[1..].parse().ok()?;
                parts.pop();
                Some(d)
            }
            _ => None,
        };
        let walk_yaw = match parts.last() {
            Some(t) if t.starts_with('y') => {
                let y = t[1..].parse().ok()?;
                parts.pop();
                Some(y)
            }
            _ => None,
        };
        let cam_accum = match parts.last() {
            Some(t) if t.starts_with('a') => {
                let a = t[1..].parse().ok()?;
                parts.pop();
                Some(a)
            }
            _ => None,
        };
        let sideband_wait = parts.last() == Some(&"h");
        if sideband_wait {
            parts.pop();
        }
        let stream_wait = parts.last() == Some(&"s");
        if stream_wait {
            parts.pop();
        }
        let module_phase = match parts.last() {
            Some(t) if t.starts_with('m') => {
                let p = t[1..].parse().ok()?;
                parts.pop();
                Some(p)
            }
            _ => None,
        };
        let mut it = parts.into_iter();
        let action_state = it.next()?.parse().ok()?;
        let fade = match (it.next(), it.next()) {
            (Some(dir), Some(age)) => Some(RetailFade {
                to_white: dir == "white",
                age: age.parse().ok()?,
            }),
            _ => None,
        };
        Some(Self {
            action_state,
            fade,
            module_phase,
            cam_accum,
            walk_yaw,
            done_hold,
            module_countdown,
            accum_align,
            camera_pass,
            stream_wait,
            sideband_wait,
        })
    }

    /// Once the gate holds, phase-align the summon close-up's accumulator
    /// to the capture's ([`Self::accum_align`],
    /// `BattleCamera::align_close_up_accum`); returns whether it moved. The
    /// headless seed calls it before it samples `camera`, the image child on
    /// the frame it captures.
    pub fn align_camera(&self, world: &mut legaia_engine_core::world::World) -> bool {
        let fade = self.align_fade(world);
        let accum = match (self.accum_align, world.battle.camera.as_mut()) {
            (Some(accum), Some(cam)) => cam.align_close_up_accum(accum),
            _ => false,
        };
        fade || accum
    }

    /// Once the gate holds, phase-align the summon flash to the capture's
    /// age ([`Self::fade`]); returns whether it moved.
    ///
    /// On retail's cadence the flash steps with the battle pass, a frame
    /// step of vsyncs at a time (`FUN_80020C14` drains `DAT_1F800393` a
    /// tick), so the ages a replay can stop on are the spawn pass's plus
    /// whole frames. Retail's own frames are not all one step - the step is
    /// measured per frame from the frame's cost (`FUN_80016B6C`), and a
    /// summon's heavy frames run longer - so the age a capture holds can
    /// fall between two of the replay's passes (`mule_summon_mid_cast`
    /// shows age `29` where the replay's step-`2` passes hold `28` and
    /// `30`). The flash ramps `0xFF / 20` a vsync, so a vsync's difference
    /// lifts the whole frame by a dozen levels. The flash is then re-run to
    /// the capture's age from its spawn: a clock aligned once the gate
    /// holds, like the close-up accumulator, not a predicate. A flash more
    /// than one frame step from the capture's is a different frame, and is
    /// left alone.
    pub fn align_fade(&self, world: &mut legaia_engine_core::world::World) -> bool {
        use legaia_engine_battle_vm::battle_action::{
            SUMMON_FADE_ID, SUMMON_FLASH_IN, SUMMON_FLASH_OUT,
        };
        let Some(want) = self.fade else {
            return false;
        };
        let max_drift = u16::from(world.battle_frame_step().max(1));
        let Some(have) = world.presentation.fade.as_ref().and_then(|f| {
            (f.kind == 1 && (f.target_rgb()[0] == 0xFF) == want.to_white).then(|| f.age_vsyncs())
        }) else {
            return false;
        };
        if have == want.age || have.abs_diff(want.age) > max_drift {
            return false;
        }
        let t = if want.to_white {
            SUMMON_FLASH_IN
        } else {
            SUMMON_FLASH_OUT
        };
        legaia_engine_system::fade::spawn_fade(
            &mut world.presentation.fade,
            &legaia_engine_system::fade::FadeTemplate {
                kind: t.kind,
                duration: t.duration,
                start_rgb: t.start_rgb,
                end_rgb: t.end_rgb,
                mode: [t.delay, t.hold, 0],
            },
            SUMMON_FADE_ID,
        );
        if let Some(f) = world.presentation.fade.as_mut() {
            for _ in 0..want.age {
                f.step_vsyncs(1);
            }
        }
        true
    }

    /// Per-tick world steering for a `0x32` capture, run before each tick of
    /// the replay. `0x32` waits on the CD-ready poll (`FUN_8003DE7C(1)` at
    /// `0x801E498C`) for the summon archive read `0x29`'s route arm queued
    /// (`jal 0x8003EC70` at `0x801E463C`), so retail spends at least one pass
    /// in it - the read cannot land on the pass that queued it - while the
    /// engine's poll is always ready and leaves on the entry tick. Hold the
    /// engine's poll busy until its camera has run a pass in the state
    /// ([`Self::met`]); every other gate leaves the poll alone.
    ///
    /// A capture on a side-band wait arm ([`Self::sideband_wait`]) holds
    /// that arm busy instead, for as long as the engine stands on it in the
    /// capture's state: retail's arm waited out the turn's read while the
    /// band's other clocks ran on, and the engine's read is resident.
    pub fn steer(&self, world: &mut legaia_engine_core::world::World) {
        if self.sideband_wait {
            world.casting.module_cam.sideband_busy = world.mode == SceneMode::Battle
                && world.battle_ctx.action_state == self.action_state
                && self.module_phase == Some(world.casting.module_phase);
        }
        if !self.camera_pass || self.action_state != SUMMON_INVOKE_STATE {
            return;
        }
        let holding = world.mode == SceneMode::Battle
            && world.battle_ctx.action_state == SUMMON_INVOKE_STATE
            && world
                .battle
                .camera
                .as_ref()
                .is_some_and(|c| c.steps_in_state() == 0);
        world.audio.sound_bank_ready = !holding;
    }

    /// Whether `world` is at this phase: the action SM on the same state
    /// and, when the capture had a flash up, the same flash at least as far
    /// in.
    pub fn met(&self, world: &legaia_engine_core::world::World) -> bool {
        if world.mode != SceneMode::Battle || world.battle_ctx.action_state != self.action_state {
            return false;
        }
        // A retail capture's RAM is the state after a whole pass, and the
        // pass that enters `0x32` has already framed it: `0x29`'s summon-route arm
        // stores the `0x600` depth (`0x801E4604`) and falls through to the
        // state's case-6 call (`0x801E4748`), so the step table a `0x32`
        // capture holds targets the summon close-up. The engine enters the
        // state on a tick its camera may not step on, and its pose is then
        // still the `0x29` chase's.
        if self.camera_pass
            && world
                .battle
                .camera
                .as_ref()
                .is_some_and(|c| c.steps_in_state() == 0)
        {
            return false;
        }
        if let Some(p) = self.module_phase
            && world.casting.module_phase < p
        {
            return false;
        }
        // On the stream poll's own arm the engine's one tick there is the
        // frame (`Self::stream_wait`).
        let on_stream_arm =
            self.stream_wait && self.module_phase == Some(world.casting.module_phase);
        // A walk that arrives sooner than retail's leaves the arm before its
        // yaw gets as far; the arm's exit is then the nearest frame.
        if let Some(y) = self.walk_yaw
            && world.casting.module_phase <= GIMARD_WALK_ARM
            && walk_swing(world.casting.module_cam.yaw_base) < walk_swing(i32::from(y))
        {
            return false;
        }
        // Same arm, and the engine's countdown drained at least as far; a
        // later arm has passed the frame and takes it at once.
        if let Some(c) = self.module_countdown
            && self.module_phase == Some(world.casting.module_phase)
            && world.casting.module_cam.countdown.0 > c
        {
            return false;
        }
        if let Some(acc) = self.cam_accum {
            let caster = world
                .actors
                .get(usize::from(world.battle_ctx.active_actor))
                .map(|a| &a.battle);
            let committed = caster.is_some_and(|b| {
                b.current_anim == SUMMON_INVOKE_CLIP && b.queued_anim == SUMMON_INVOKE_CLIP
            });
            // The engine's own `ctx[+0x87C]`: the commit zeroes it and the
            // framing prologue adds `8` on the commit frame itself, so it
            // reads `8 * (frames since the commit + 1)` - counting frames
            // from the commit instead is one vsync late, and on a capture
            // taken on the band's last `0x33` frame the gate was never met.
            let accum = world.battle.camera.as_ref().map_or_else(
                || {
                    let since = world
                        .clock
                        .display_frames
                        .saturating_sub(world.battle_ctx.active_clip_commit_frame);
                    since.saturating_add(1).saturating_mul(8)
                },
                |c| u64::from(c.close_up_accum()),
            );
            if !committed || accum < u64::from(acc) {
                return false;
            }
        }
        if let Some(t) = self.done_hold
            && world.battle_ctx.frame_timer > t
        {
            return false;
        }
        let Some(want) = self.fade else {
            return true;
        };
        if on_stream_arm {
            return true;
        }
        world.presentation.fade.as_ref().is_some_and(|f| {
            f.kind == 1
                && (f.target_rgb()[0] == 0xFF) == want.to_white
                && f.age_vsyncs() >= want.age
        })
    }
}

/// The summon band's live full-screen flash in a capture: which of the two
/// templates it is and how many vsyncs it has run. Read off the SCUS fade
/// actor's `+0x7C` block ([`legaia_engine_system::fade_ramp`]), whose
/// countdowns give the age back exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailFade {
    /// `true` for the `0x33` flash-in (black to white), `false` for the
    /// `0x34` flash-out.
    pub to_white: bool,
    /// Vsyncs since the spawn, the start delay included.
    pub age: u16,
}

#[cfg(test)]
mod tests;
