//! The cutscene sequencer: a pack cutscene (`cutscenes.toml`,
//! [`legaia_expansion_format::cutscene`]) run one frame at a time.
//!
//! The sequencer is a deterministic interpreter over plain state. Each
//! [`CutsceneRun::step`] advances its clocks by the frames the world ran,
//! resolves the waits that have cleared, runs every track forward to its next
//! wait and returns the [`Command`]s the steps issued. It reads the world only
//! through [`CutsceneEnv`] (is this actor still walking, is the dialogue box
//! closed, which option was picked, does this gate hold) and writes it only
//! through those commands, so the same run over the same answers produces the
//! same commands, frame for frame - which is what its tests pin and what makes
//! a cutscene replay bit-identically.
//!
//! The camera, the screen fades and the letterbox are the sequencer's own
//! state ([`CutsceneCamera`], [`ScreenLayer`]): a camera step glides between
//! poses here and the hosts read the pose ([`CutsceneRun::camera`]) over their
//! follow camera, so nothing of the shot is host-side.
//!
//! `engine-core` holds one [`CutsceneRuntime`] on `World::expansion` and runs
//! the commands (`World::tick_expansion_cutscene`).

use std::collections::BTreeMap;
use std::sync::Arc;

use legaia_expansion_format::cutscene::{
    CameraFocus, CutsceneDef, FadeTo, STEP_BUDGET, Step, TintMode, labels,
};
use legaia_expansion_format::quest::Prerequisites;
use legaia_expansion_format::refs::DiscRef;

/// What the sequencer asks of the world.
pub trait CutsceneEnv {
    /// Whether `actor` is still walking a leg a `move` started.
    fn actor_busy(&self, actor: &str) -> bool;
    /// Whether a dialogue box (or a queued pack line) still owns the screen.
    fn dialog_busy(&self) -> bool;
    /// The option picked on the last line the cutscene said, once it closed.
    fn choice(&self) -> Option<usize>;
    /// Whether pack line `line` ends on a menu.
    fn line_has_choices(&self, line: &str) -> bool;
    /// Whether a gate holds.
    fn holds(&self, requires: &Prerequisites) -> bool;
    /// A pack counter's value.
    fn counter(&self, counter: &str) -> i64;
    /// An actor's world position `(x, z)`.
    fn actor_pos(&self, actor: &str) -> Option<(i32, i32)>;
    /// The world centre `(x, z)` of a tile in the cutscene's coordinates.
    fn tile_world(&self, tile: [u16; 2]) -> (i32, i32);
}

/// What a step does to the world.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Show pack line `line`.
    Say(String),
    /// Close the dialogue box on screen (a skip past a line).
    CloseDialog,
    /// Walk `actor` through `waypoints` (tiles, in order).
    Walk {
        actor: String,
        waypoints: Vec<[u16; 2]>,
        speed: Option<u16>,
        run: bool,
        clip: Option<u8>,
    },
    /// Put a walking actor on the end of its leg at once (a skip).
    FinishWalk {
        actor: String,
    },
    /// Turn `actor` to sector `facing`, or toward `toward`.
    Face {
        actor: String,
        facing: Option<u8>,
        toward: Option<String>,
    },
    /// Play `clip` on `actor`.
    Clip {
        actor: String,
        clip: u8,
    },
    /// Put `actor`'s idle clip back.
    IdleClip {
        actor: String,
    },
    /// Put `actor` on `tile`.
    Teleport {
        actor: String,
        tile: [u16; 2],
        facing: Option<u8>,
    },
    /// Bring a pack NPC on (at `tile` when given).
    Show {
        npc: String,
        tile: Option<[u16; 2]>,
    },
    /// Take a pack NPC off.
    Hide {
        npc: String,
    },
    SetFlag(String),
    ClearFlag(String),
    Counter {
        counter: String,
        add: i64,
    },
    GiveItem {
        item: u8,
        count: u32,
    },
    TakeItem {
        item: u8,
        count: u32,
    },
    Gold(i64),
    GiveCard(String),
    StartQuest(String),
    Sfx(u16),
    Bgm(DiscRef),
    /// A retail cue or pack sound (`World::expansion_play_sound`).
    Sound(legaia_expansion_format::audio::SoundRef),
    /// Raise a title banner of a pack line (`banner`).
    Banner(String),
    /// A held music pick and its fade (`World::expansion_set_music`).
    Music(legaia_expansion_format::audio::MusicRef, u16),
    /// Release the held music pick.
    MusicRelease,
    /// Lock or unlock the player.
    Lock(bool),
    /// The cutscene ends on this fight; the world starts it once the
    /// cutscene is gone.
    Fight(Fight),
}

/// The fight a cutscene ends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fight {
    Formation(String),
    Boss(String),
}

/// What a track is waiting on.
#[derive(Debug, Clone, PartialEq)]
enum Wait {
    None,
    Frames(u32),
    Actor(String),
    Dialog {
        line: String,
        on_choice: Vec<String>,
    },
    Camera,
    Fade,
    Track(String),
    /// A `clip` with `frames`: the idle clip goes back when they run out.
    ClipHold {
        actor: String,
        frames: u32,
    },
}

#[derive(Debug, Clone)]
struct Track {
    /// `None` for the main track.
    name: Option<String>,
    pc: usize,
    wait: Wait,
    done: bool,
}

/// A camera pose, measured from the follow camera's own shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    /// World `(x, z)` the shot looks at.
    pub focus: [f32; 2],
    /// How much of [`Self::focus`] replaces the follow camera's own focus:
    /// `0` is the follow focus, `1` the pose's.
    pub focus_weight: f32,
    /// Degrees added to the follow camera's yaw.
    pub yaw: f32,
    /// Degrees added to the follow camera's pitch (positive looks further
    /// down).
    pub pitch: f32,
    /// Factor on the follow camera's eye distance.
    pub zoom: f32,
}

impl CameraPose {
    /// The follow camera itself.
    pub const FOLLOW: Self = Self {
        focus: [0.0, 0.0],
        focus_weight: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        zoom: 1.0,
    };

    /// Whether the pose is the follow camera's.
    pub fn is_follow(&self) -> bool {
        self.focus_weight == 0.0 && self.yaw == 0.0 && self.pitch == 0.0 && self.zoom == 1.0
    }

    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        // A pose easing in from the follow focus takes the target's point
        // from the start; only the weight moves.
        let from_focus = if a.focus_weight == 0.0 {
            b.focus
        } else {
            a.focus
        };
        let to_focus = if b.focus_weight == 0.0 {
            a.focus
        } else {
            b.focus
        };
        Self {
            focus: [l(from_focus[0], to_focus[0]), l(from_focus[1], to_focus[1])],
            focus_weight: l(a.focus_weight, b.focus_weight),
            yaw: l(a.yaw, b.yaw),
            pitch: l(a.pitch, b.pitch),
            zoom: l(a.zoom, b.zoom),
        }
    }
}

/// Smoothstep easing: still at both ends of a glide.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The sequencer's camera: a glide between two poses, an actor it follows,
/// and a shake.
#[derive(Debug, Clone, PartialEq)]
pub struct CutsceneCamera {
    from: CameraPose,
    to: CameraPose,
    frames: u32,
    t: u32,
    /// The actor whose position is the focus target while set.
    follow: Option<String>,
    shake_amplitude: f32,
    shake_frames: u32,
    shake_t: u32,
    /// A frame counter the shake jitter keys on.
    clock: u32,
}

impl Default for CutsceneCamera {
    fn default() -> Self {
        Self {
            from: CameraPose::FOLLOW,
            to: CameraPose::FOLLOW,
            frames: 0,
            t: 0,
            follow: None,
            shake_amplitude: 0.0,
            shake_frames: 0,
            shake_t: 0,
            clock: 0,
        }
    }
}

impl CutsceneCamera {
    /// The pose this frame.
    pub fn pose(&self) -> CameraPose {
        if self.frames == 0 || self.t >= self.frames {
            return self.to;
        }
        CameraPose::lerp(
            &self.from,
            &self.to,
            ease(self.t as f32 / self.frames as f32),
        )
    }

    /// Whether a glide is still in flight.
    pub fn gliding(&self) -> bool {
        self.t < self.frames
    }

    /// This frame's shake offset `(x, z)`, world units.
    pub fn shake(&self) -> [f32; 2] {
        if self.shake_t >= self.shake_frames || self.shake_frames == 0 {
            return [0.0, 0.0];
        }
        let left = 1.0 - self.shake_t as f32 / self.shake_frames as f32;
        let a = self.shake_amplitude * left;
        // A fixed jitter sequence (an integer hash of the frame), so a shake
        // replays exactly.
        let h = |k: u32| {
            let mut x = self.clock.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B);
            x ^= x >> 15;
            x = x.wrapping_mul(0x2C1B_3C6D);
            x ^= x >> 12;
            (x & 0xFFFF) as f32 / 32767.5 - 1.0
        };
        [a * h(1), a * h(2)]
    }

    /// Whether the camera is anything but the follow camera this frame.
    pub fn active(&self) -> bool {
        !self.pose().is_follow() || self.gliding() || self.shake() != [0.0, 0.0]
    }

    fn advance(&mut self, frames: u32, env: &dyn CutsceneEnv) {
        self.clock = self.clock.wrapping_add(frames);
        self.t = self.t.saturating_add(frames).min(self.frames);
        self.shake_t = self.shake_t.saturating_add(frames).min(self.shake_frames);
        if let Some(a) = &self.follow
            && let Some((x, z)) = env.actor_pos(a)
        {
            self.to.focus = [x as f32, z as f32];
            if !self.gliding() {
                self.from.focus = self.to.focus;
            }
        }
    }

    fn glide_to(&mut self, to: CameraPose, frames: u32, follow: Option<String>) {
        self.from = self.pose();
        self.to = to;
        self.frames = frames;
        self.t = 0;
        self.follow = follow;
    }
}

/// A screen-wide colour layer: the fade, a flash and a held tint.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScreenLayer {
    /// Fade level `0..=1` at the start and end of the current ramp.
    fade_from: f32,
    fade_to: f32,
    fade_frames: u32,
    fade_t: u32,
    /// Whether the fade is to white (else black).
    fade_white: bool,
    flash: [u8; 3],
    flash_frames: u32,
    flash_t: u32,
    tint_from: [u8; 3],
    tint_to: [u8; 3],
    tint_frames: u32,
    tint_t: u32,
    tint_mode: TintMode,
    /// Letterbox level `0..=1`.
    letterbox: f32,
    letterbox_on: bool,
}

/// One full-screen quad: `(rgb 0xRRGGBB, abr)` in the shape
/// `legaia_render_kernels::screen_prim::fade_prim` takes - ABR `1` adds the
/// colour, `2` subtracts it.
pub type ScreenQuad = (u32, u8);

/// Frames the letterbox takes to slide in or out.
pub const LETTERBOX_FRAMES: u32 = 20;
/// The letterbox bar height at full extent, in display scanlines of the
/// 240-line frame.
pub const LETTERBOX_LINES: i16 = 24;

fn grey(level: f32) -> u32 {
    let v = (level.clamp(0.0, 1.0) * 255.0).round() as u32;
    (v << 16) | (v << 8) | v
}

fn pack_rgb(c: [u8; 3]) -> u32 {
    (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
}

impl ScreenLayer {
    /// The fade level this frame.
    pub fn fade_level(&self) -> f32 {
        if self.fade_frames == 0 || self.fade_t >= self.fade_frames {
            return self.fade_to;
        }
        let t = self.fade_t as f32 / self.fade_frames as f32;
        self.fade_from + (self.fade_to - self.fade_from) * t
    }

    fn fading(&self) -> bool {
        self.fade_t < self.fade_frames
    }

    fn tint(&self) -> [u8; 3] {
        if self.tint_frames == 0 || self.tint_t >= self.tint_frames {
            return self.tint_to;
        }
        let t = self.tint_t as f32 / self.tint_frames as f32;
        let mut out = [0u8; 3];
        for (i, o) in out.iter_mut().enumerate() {
            let (a, b) = (f32::from(self.tint_from[i]), f32::from(self.tint_to[i]));
            *o = (a + (b - a) * t).round() as u8;
        }
        out
    }

    /// Flash `color`, fading out over `frames`.
    pub fn start_flash(&mut self, color: [u8; 3], frames: u32) {
        self.flash = color;
        self.flash_frames = frames;
        self.flash_t = 0;
    }

    /// Ramp the held tint to `color` over `frames` (`[0, 0, 0]` clears it).
    /// A change of mode ramps from clear, not from the other mode's colour.
    pub fn start_tint(&mut self, color: [u8; 3], frames: u32, mode: TintMode) {
        self.tint_from = if mode == self.tint_mode {
            self.tint()
        } else {
            [0, 0, 0]
        };
        self.tint_to = color;
        self.tint_frames = frames;
        self.tint_t = 0;
        self.tint_mode = mode;
    }

    /// Advance the layer's ramps by `frames`.
    pub fn tick(&mut self, frames: u32) {
        self.advance(frames);
    }

    /// The quads this frame, in draw order: the tint, the flash, the fade.
    pub fn quads(&self) -> Vec<ScreenQuad> {
        let mut out = Vec::new();
        let tint = self.tint();
        if tint != [0, 0, 0] {
            let abr = match self.tint_mode {
                TintMode::Add => 1,
                TintMode::Subtract => 2,
            };
            out.push((pack_rgb(tint), abr));
        }
        if self.flash_t < self.flash_frames && self.flash != [0, 0, 0] {
            let k = 1.0 - self.flash_t as f32 / self.flash_frames as f32;
            let c = self.flash.map(|v| (f32::from(v) * k).round() as u8);
            out.push((pack_rgb(c), 1));
        }
        let level = self.fade_level();
        if level > 0.0 {
            out.push((grey(level), if self.fade_white { 1 } else { 2 }));
        }
        out
    }

    /// The letterbox bar height this frame, display scanlines.
    pub fn letterbox_lines(&self) -> i16 {
        (ease(self.letterbox) * f32::from(LETTERBOX_LINES)).round() as i16
    }

    fn advance(&mut self, frames: u32) {
        self.fade_t = self.fade_t.saturating_add(frames).min(self.fade_frames);
        self.flash_t = self.flash_t.saturating_add(frames).min(self.flash_frames);
        self.tint_t = self.tint_t.saturating_add(frames).min(self.tint_frames);
        let d = frames as f32 / LETTERBOX_FRAMES as f32;
        self.letterbox = if self.letterbox_on {
            (self.letterbox + d).min(1.0)
        } else {
            (self.letterbox - d).max(0.0)
        };
    }

    fn fade(&mut self, to: FadeTo, frames: u32) {
        let level = self.fade_level();
        let (target, white) = match to {
            FadeTo::Black => (1.0, false),
            FadeTo::White => (1.0, true),
            FadeTo::Clear => (0.0, self.fade_white),
        };
        // Changing colour through a held fade starts the new one from clear.
        let from = if target > 0.0 && white != self.fade_white {
            0.0
        } else {
            level
        };
        self.fade_white = white;
        self.fade_from = from;
        self.fade_to = target;
        self.fade_frames = frames;
        self.fade_t = 0;
    }
}

/// One cutscene in play.
#[derive(Debug, Clone)]
pub struct CutsceneRun {
    /// The cutscene id.
    pub id: String,
    def: Arc<CutsceneDef>,
    tracks: Vec<Track>,
    /// The camera.
    pub camera: CutsceneCamera,
    /// The fades, flash, tint and letterbox.
    pub screen: ScreenLayer,
    /// Whether the player is locked now.
    pub locked: bool,
    /// Whether Start skips the run (it has been seen before).
    pub skippable: bool,
    /// Frames the run has played.
    pub frame: u32,
    /// The run has ended: the world drops it (once Start is up, after a
    /// skip).
    pub finished: bool,
    /// The run was skipped.
    pub skipped: bool,
    /// When set, every step the run executes is recorded here as
    /// `(frame, track, step index)` (the main track's name is `""`): the
    /// workbench's timeline preview ([`preview`]).
    pub trace: Option<Vec<(u32, String, usize)>>,
}

impl CutsceneRun {
    /// A run of `def` (id `id`). `seen` is whether the player has seen it
    /// before, which makes it skippable when the cutscene allows.
    pub fn new(id: &str, def: Arc<CutsceneDef>, seen: bool) -> Self {
        let screen = ScreenLayer {
            letterbox_on: def.letterbox,
            ..Default::default()
        };
        Self {
            id: id.to_string(),
            locked: def.lock_player,
            skippable: def.skippable && seen,
            tracks: vec![Track {
                name: None,
                pc: 0,
                wait: Wait::None,
                done: false,
            }],
            def,
            camera: CutsceneCamera::default(),
            screen,
            frame: 0,
            finished: false,
            skipped: false,
            trace: None,
        }
    }

    /// The cutscene being run.
    pub fn def(&self) -> &CutsceneDef {
        &self.def
    }

    fn steps(&self, track: usize) -> &[Step] {
        match &self.tracks[track].name {
            None => &self.def.steps,
            Some(n) => self.def.tracks.get(n).map_or(&[], |t| &t.steps),
        }
    }

    /// Where each live track stands: `(track name, step index)`, the main
    /// track as `""`. For the workbench preview and the tests.
    pub fn cursors(&self) -> Vec<(String, usize)> {
        self.tracks
            .iter()
            .filter(|t| !t.done)
            .map(|t| (t.name.clone().unwrap_or_default(), t.pc))
            .collect()
    }

    /// One frame: advance the clocks `frames` display frames, clear the
    /// waits that are over, and run each track to its next wait. Returns
    /// the commands the steps issued, in order.
    pub fn step(&mut self, env: &dyn CutsceneEnv, frames: u32) -> Vec<Command> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        self.frame = self.frame.saturating_add(frames);
        self.camera.advance(frames, env);
        self.screen.advance(frames);
        let mut k = 0;
        while k < self.tracks.len() {
            if !self.tracks[k].done {
                self.clear_wait(k, env, frames, &mut out);
                self.run_track(k, env, false, &mut out);
            }
            if self.finished {
                break;
            }
            k += 1;
        }
        // A join on a track that ended this frame (a later track in the
        // list) clears this frame too.
        for k in 0..self.tracks.len() {
            if self.finished {
                break;
            }
            if let Wait::Track(name) = &self.tracks[k].wait
                && !self
                    .tracks
                    .iter()
                    .any(|t| t.name.as_deref() == Some(name.as_str()) && !t.done)
            {
                self.tracks[k].wait = Wait::None;
                self.run_track(k, env, false, &mut out);
            }
        }
        out
    }

    /// Skip: run every track to its end at once, the waits all cleared - a
    /// walk lands, a glide arrives, a line not yet shown is not shown. A line
    /// ending on a menu stops the skip there (its choice is the player's to
    /// make), and the run goes on from it as usual. `false`, doing nothing,
    /// when the run is not skippable or a menu is up.
    pub fn skip(&mut self, env: &dyn CutsceneEnv) -> Option<Vec<Command>> {
        if !self.skippable || self.finished {
            return None;
        }
        let on_menu = self.tracks.iter().any(|t| {
            !t.done && matches!(&t.wait, Wait::Dialog { line, .. } if env.line_has_choices(line))
        });
        if on_menu {
            return None;
        }
        let mut out = Vec::new();
        self.skipped = true;
        // Settle what is in flight.
        for t in &mut self.tracks {
            match std::mem::replace(&mut t.wait, Wait::None) {
                Wait::Actor(a) => out.push(Command::FinishWalk { actor: a }),
                Wait::Dialog { .. } => out.push(Command::CloseDialog),
                Wait::ClipHold { actor, .. } => out.push(Command::IdleClip { actor }),
                _ => {}
            }
        }
        self.camera.t = self.camera.frames;
        self.screen.fade_t = self.screen.fade_frames;
        self.screen.flash_t = self.screen.flash_frames;
        self.screen.tint_t = self.screen.tint_frames;
        let mut k = 0;
        while k < self.tracks.len() && !self.finished {
            if !self.tracks[k].done {
                self.run_track(k, env, true, &mut out);
                // A menu stopped it: the rest plays.
                if matches!(self.tracks[k].wait, Wait::Dialog { .. }) {
                    return Some(out);
                }
            }
            k += 1;
        }
        if !self.finished {
            // Every track ran out: the main one is done, so the run is.
            self.finished = true;
        }
        Some(out)
    }

    fn clear_wait(&mut self, k: usize, env: &dyn CutsceneEnv, frames: u32, out: &mut Vec<Command>) {
        let wait = std::mem::replace(&mut self.tracks[k].wait, Wait::None);
        let still = match wait {
            Wait::None => None,
            Wait::Frames(n) => {
                let left = n.saturating_sub(frames);
                (left > 0).then_some(Wait::Frames(left))
            }
            Wait::Actor(a) => env.actor_busy(&a).then_some(Wait::Actor(a)),
            Wait::Dialog { line, on_choice } => {
                if env.dialog_busy() {
                    Some(Wait::Dialog { line, on_choice })
                } else {
                    if let Some(label) = env
                        .choice()
                        .and_then(|c| on_choice.get(c))
                        .filter(|l| !l.is_empty())
                    {
                        self.jump(k, label);
                    }
                    None
                }
            }
            Wait::Camera => self.camera.gliding().then_some(Wait::Camera),
            Wait::Fade => self.screen.fading().then_some(Wait::Fade),
            Wait::Track(name) => self
                .tracks
                .iter()
                .any(|t| t.name.as_deref() == Some(name.as_str()) && !t.done)
                .then_some(Wait::Track(name)),
            Wait::ClipHold { actor, frames: n } => {
                let left = n.saturating_sub(frames);
                if left > 0 {
                    Some(Wait::ClipHold {
                        actor,
                        frames: left,
                    })
                } else {
                    out.push(Command::IdleClip { actor });
                    None
                }
            }
        };
        if let Some(w) = still {
            self.tracks[k].wait = w;
        }
    }

    fn jump(&mut self, k: usize, label: &str) {
        let target = labels(self.steps(k)).get(label).copied();
        if let Some(i) = target {
            self.tracks[k].pc = i;
        }
    }

    fn run_track(
        &mut self,
        k: usize,
        env: &dyn CutsceneEnv,
        skipping: bool,
        out: &mut Vec<Command>,
    ) {
        let mut budget = STEP_BUDGET;
        while self.tracks[k].wait == Wait::None && !self.tracks[k].done && !self.finished {
            if budget == 0 {
                // A loop that never waits (the validator refuses one): the
                // run ends rather than hang the frame.
                self.end();
                return;
            }
            budget -= 1;
            let pc = self.tracks[k].pc;
            let Some(step) = self.steps(k).get(pc).cloned() else {
                self.finish_track(k);
                return;
            };
            self.tracks[k].pc += 1;
            if let Some(t) = self.trace.as_mut() {
                let name = self.tracks[k].name.clone().unwrap_or_default();
                t.push((self.frame, name, pc));
            }
            self.exec(k, &step, env, skipping, out);
        }
    }

    fn finish_track(&mut self, k: usize) {
        self.tracks[k].done = true;
        if k == 0 {
            self.end();
        }
    }

    fn end(&mut self) {
        self.finished = true;
        for t in &mut self.tracks {
            t.done = true;
        }
    }

    fn set_wait(&mut self, k: usize, wait: Wait) {
        self.tracks[k].wait = wait;
    }

    fn exec(
        &mut self,
        k: usize,
        step: &Step,
        env: &dyn CutsceneEnv,
        skipping: bool,
        out: &mut Vec<Command>,
    ) {
        match step {
            Step::Wait { frames } => {
                if !skipping && *frames > 0 {
                    self.set_wait(k, Wait::Frames(*frames));
                }
            }
            Step::Say { line, on_choice } => {
                let menu = env.line_has_choices(line);
                if skipping && !menu {
                    return;
                }
                out.push(Command::Say(line.clone()));
                self.set_wait(
                    k,
                    Wait::Dialog {
                        line: line.clone(),
                        on_choice: on_choice.clone(),
                    },
                );
            }
            Step::Move {
                actor,
                to,
                path,
                speed,
                run,
                clip,
                wait,
            } => {
                let waypoints: Vec<[u16; 2]> = path.iter().chain(to.iter()).copied().collect();
                let Some(&last) = waypoints.last() else {
                    return;
                };
                if skipping {
                    out.push(Command::Teleport {
                        actor: actor.clone(),
                        tile: last,
                        facing: None,
                    });
                    return;
                }
                out.push(Command::Walk {
                    actor: actor.clone(),
                    waypoints,
                    speed: *speed,
                    run: *run,
                    clip: *clip,
                });
                if *wait {
                    self.set_wait(k, Wait::Actor(actor.clone()));
                }
            }
            Step::Face {
                actor,
                facing,
                toward,
            } => out.push(Command::Face {
                actor: actor.clone(),
                facing: *facing,
                toward: toward.clone(),
            }),
            Step::Clip {
                actor,
                clip,
                frames,
            } => {
                if skipping {
                    if frames.is_some() {
                        out.push(Command::IdleClip {
                            actor: actor.clone(),
                        });
                    } else {
                        out.push(Command::Clip {
                            actor: actor.clone(),
                            clip: *clip,
                        });
                    }
                    return;
                }
                out.push(Command::Clip {
                    actor: actor.clone(),
                    clip: *clip,
                });
                if let Some(f) = frames {
                    self.set_wait(
                        k,
                        Wait::ClipHold {
                            actor: actor.clone(),
                            frames: *f,
                        },
                    );
                }
            }
            Step::Teleport {
                actor,
                tile,
                facing,
            } => out.push(Command::Teleport {
                actor: actor.clone(),
                tile: *tile,
                facing: *facing,
            }),
            Step::Show { npc, tile } => out.push(Command::Show {
                npc: npc.clone(),
                tile: *tile,
            }),
            Step::Hide { npc } => out.push(Command::Hide { npc: npc.clone() }),
            Step::Camera {
                focus,
                yaw,
                pitch,
                zoom,
                frames,
                wait,
            } => {
                let mut to = self.camera.to;
                let mut follow = self.camera.follow.clone();
                match focus {
                    Some(CameraFocus::Tile { tile }) => {
                        let (x, z) = env.tile_world(*tile);
                        to.focus = [x as f32, z as f32];
                        to.focus_weight = 1.0;
                        follow = None;
                    }
                    Some(CameraFocus::Actor { actor }) => {
                        if let Some((x, z)) = env.actor_pos(actor) {
                            to.focus = [x as f32, z as f32];
                        }
                        to.focus_weight = 1.0;
                        follow = Some(actor.clone());
                    }
                    None => {}
                }
                if let Some(v) = yaw {
                    to.yaw = *v;
                }
                if let Some(v) = pitch {
                    to.pitch = *v;
                }
                if let Some(v) = zoom {
                    to.zoom = *v;
                }
                let frames = if skipping { 0 } else { *frames };
                self.camera.glide_to(to, frames, follow);
                if *wait && frames > 0 {
                    self.set_wait(k, Wait::Camera);
                }
            }
            Step::CameraReset { frames, wait } => {
                let frames = if skipping { 0 } else { *frames };
                let mut to = CameraPose::FOLLOW;
                to.focus = self.camera.pose().focus;
                self.camera.glide_to(to, frames, None);
                if *wait && frames > 0 {
                    self.set_wait(k, Wait::Camera);
                }
            }
            Step::Shake { amplitude, frames } => {
                if !skipping {
                    self.camera.shake_amplitude = f32::from(*amplitude);
                    self.camera.shake_frames = *frames;
                    self.camera.shake_t = 0;
                }
            }
            Step::Fade { to, frames, wait } => {
                let frames = if skipping { 0 } else { *frames };
                self.screen.fade(*to, frames);
                if *wait && frames > 0 {
                    self.set_wait(k, Wait::Fade);
                }
            }
            Step::Banner { line } => {
                if !skipping {
                    out.push(Command::Banner(line.clone()));
                }
            }
            Step::Flash { color, frames } => {
                if !skipping {
                    self.screen.flash = *color;
                    self.screen.flash_frames = *frames;
                    self.screen.flash_t = 0;
                }
            }
            Step::Tint {
                color,
                frames,
                mode,
            } => {
                self.screen.tint_from = self.screen.tint();
                self.screen.tint_to = *color;
                self.screen.tint_frames = if skipping { 0 } else { *frames };
                self.screen.tint_t = 0;
                self.screen.tint_mode = *mode;
            }
            Step::SetFlag { flag } => out.push(Command::SetFlag(flag.clone())),
            Step::ClearFlag { flag } => out.push(Command::ClearFlag(flag.clone())),
            Step::Counter { counter, add } => out.push(Command::Counter {
                counter: counter.clone(),
                add: *add,
            }),
            Step::Label { .. } => {}
            Step::Goto { label } => self.jump(k, label),
            Step::Branch {
                requires,
                counter,
                at_least,
                goto,
                otherwise,
            } => {
                let holds = env.holds(requires)
                    && counter
                        .as_deref()
                        .is_none_or(|c| env.counter(c) >= *at_least);
                if holds {
                    self.jump(k, goto);
                } else if let Some(o) = otherwise {
                    self.jump(k, o);
                }
            }
            Step::StartTrack { track } => {
                let running = self
                    .tracks
                    .iter()
                    .any(|t| t.name.as_deref() == Some(track.as_str()) && !t.done);
                if !running && self.def.tracks.contains_key(track) {
                    self.tracks.push(Track {
                        name: Some(track.clone()),
                        pc: 0,
                        wait: Wait::None,
                        done: false,
                    });
                }
            }
            Step::Join { track } => {
                if skipping {
                    // A skip runs the joined track out first.
                    if let Some(j) = self
                        .tracks
                        .iter()
                        .position(|t| t.name.as_deref() == Some(track.as_str()) && !t.done)
                    {
                        self.run_track(j, env, true, out);
                    }
                    return;
                }
                self.set_wait(k, Wait::Track(track.clone()));
            }
            Step::Lock => {
                self.locked = true;
                out.push(Command::Lock(true));
            }
            Step::Unlock => {
                self.locked = false;
                out.push(Command::Lock(false));
            }
            Step::Battle { formation } => {
                out.push(Command::Fight(Fight::Formation(formation.clone())));
                self.end();
            }
            Step::Boss { boss } => {
                out.push(Command::Fight(Fight::Boss(boss.clone())));
                self.end();
            }
            Step::GiveItem { item, count } => out.push(Command::GiveItem {
                item: *item,
                count: *count,
            }),
            Step::TakeItem { item, count } => out.push(Command::TakeItem {
                item: *item,
                count: *count,
            }),
            Step::Gold { amount } => out.push(Command::Gold(*amount)),
            Step::GiveCard { card } => out.push(Command::GiveCard(card.clone())),
            Step::StartQuest { quest } => out.push(Command::StartQuest(quest.clone())),
            Step::Sfx { id } => {
                if !skipping {
                    out.push(Command::Sfx(*id));
                }
            }
            Step::Bgm { bgm } => out.push(Command::Bgm(bgm.clone())),
            Step::Sound { sound } => {
                if !skipping {
                    out.push(Command::Sound(sound.clone()));
                }
            }
            Step::Music { music, fade } => out.push(Command::Music(music.clone(), *fade)),
            Step::MusicRelease => out.push(Command::MusicRelease),
            Step::End => self.end(),
        }
    }
}

/// The `World`'s cutscene seat (`World::expansion.cutscene`).
#[derive(Debug, Clone, Default)]
pub struct CutsceneRuntime {
    /// The cutscene in play.
    pub run: Option<CutsceneRun>,
    /// Cutscenes waiting for the field to be free, oldest first (a quest's
    /// completion cutscene raised in battle).
    pub queued: Vec<String>,
    /// The option picked on the last line the run said, once its box
    /// closed; cleared when the run says another.
    pub last_choice: Option<usize>,
    /// The line the run is waiting on.
    pub said: Option<String>,
    /// A fight the run ended on, started once the run is gone.
    pub fight: Option<Fight>,
    /// The run was skipped and is waiting for Start to be let go before it
    /// ends, so the press that skipped it does not open the menu.
    pub awaiting_release: bool,
    /// Pack NPCs hidden this visit (`hide`, or an unmet `present`), by
    /// placement slot, with where they stood.
    pub hidden: BTreeMap<u8, (i16, i16)>,
    /// The walks in flight, by actor id.
    pub walks: BTreeMap<String, Walk>,
    /// The run moved the player (a walk or a teleport): the follow camera
    /// re-seats on it when the run ends.
    pub player_moved: bool,
}

/// One actor's walk along the waypoints a `move` gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// World `(x, z)` stops still to reach, in order.
    pub stops: Vec<(i16, i16)>,
    /// World units a frame (`None`: the actor's own pace).
    pub speed: Option<u16>,
    /// Whether the player runs.
    pub run: bool,
    /// The clip the walk plays.
    pub clip: Option<u8>,
    /// The clip to put back on arrival.
    pub idle: Option<u8>,
    /// Whether the current stop's leg has been started.
    pub leg_started: bool,
    /// The NPC glide speed a `speed` replaced for the walk (`Some(None)`:
    /// it had none of its own), put back when the walk ends.
    pub prior_speed: Option<Option<u16>>,
}

impl CutsceneRuntime {
    /// Whether a cutscene is in play.
    pub fn active(&self) -> bool {
        self.run.is_some()
    }

    /// Whether a cutscene holds the player now.
    pub fn locks_player(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.locked || self.awaiting_release)
    }

    /// Drop every run and queue (a scene change, a load).
    pub fn clear(&mut self) {
        self.run = None;
        self.queued.clear();
        self.last_choice = None;
        self.said = None;
        self.fight = None;
        self.awaiting_release = false;
        self.walks.clear();
        self.player_moved = false;
    }
}

pub mod preview;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A scripted world: actors walk for a fixed number of frames, lines
    /// close after a fixed number, and the picked option is preset.
    #[derive(Default)]
    struct Env {
        walking: RefCell<BTreeMap<String, u32>>,
        dialog_left: RefCell<u32>,
        choice: Option<usize>,
        menus: Vec<String>,
        flags: Vec<String>,
        counter: i64,
    }

    impl Env {
        /// Apply this frame's commands the way the world does.
        fn apply(&self, cmds: &[Command]) {
            for c in cmds {
                match c {
                    Command::Walk {
                        actor, waypoints, ..
                    } => {
                        self.walking
                            .borrow_mut()
                            .insert(actor.clone(), waypoints.len() as u32 * 10);
                    }
                    Command::FinishWalk { actor } => {
                        self.walking.borrow_mut().remove(actor);
                    }
                    Command::Say(_) => *self.dialog_left.borrow_mut() = 30,
                    Command::CloseDialog => *self.dialog_left.borrow_mut() = 0,
                    _ => {}
                }
            }
        }

        fn tick(&self) {
            for v in self.walking.borrow_mut().values_mut() {
                *v = v.saturating_sub(1);
            }
            self.walking.borrow_mut().retain(|_, v| *v > 0);
            let mut d = self.dialog_left.borrow_mut();
            *d = d.saturating_sub(1);
        }
    }

    impl CutsceneEnv for Env {
        fn actor_busy(&self, actor: &str) -> bool {
            self.walking.borrow().contains_key(actor)
        }
        fn dialog_busy(&self) -> bool {
            *self.dialog_left.borrow() > 0
        }
        fn choice(&self) -> Option<usize> {
            self.choice
        }
        fn line_has_choices(&self, line: &str) -> bool {
            self.menus.iter().any(|m| m == line)
        }
        fn holds(&self, r: &Prerequisites) -> bool {
            r.flags.iter().all(|f| self.flags.contains(f))
        }
        fn counter(&self, _: &str) -> i64 {
            self.counter
        }
        fn actor_pos(&self, _: &str) -> Option<(i32, i32)> {
            Some((1000, 2000))
        }
        fn tile_world(&self, t: [u16; 2]) -> (i32, i32) {
            (i32::from(t[0]) * 128 + 64, i32::from(t[1]) * 128 + 64)
        }
    }

    fn def(text: &str) -> Arc<CutsceneDef> {
        let s: legaia_expansion_format::cutscene::CutsceneSection =
            toml::from_str(text).expect("parses");
        Arc::new(s.cutscenes.into_values().next().expect("one cutscene"))
    }

    /// Run to the end, at most `cap` frames: every command with its frame.
    fn run(run: &mut CutsceneRun, env: &Env, cap: u32) -> Vec<(u32, Command)> {
        let mut log = Vec::new();
        for f in 0..cap {
            let cmds = run.step(env, 1);
            env.apply(&cmds);
            log.extend(cmds.into_iter().map(|c| (f, c)));
            if run.finished {
                break;
            }
            env.tick();
        }
        log
    }

    const SCENE: &str = r#"
[cutscenes.c]
steps = [
  { do = "move", actor = "guide", path = [[1, 1], [2, 1]] },
  { do = "wait", frames = 5 },
  { do = "say", line = "offer", on_choice = ["yes", "no"] },
  { do = "label", name = "yes" },
  { do = "set_flag", flag = "agreed" },
  { do = "boss", boss = "warden" },
  { do = "label", name = "no" },
  { do = "set_flag", flag = "refused" },
]
"#;

    #[test]
    fn steps_wait_for_the_walk_the_frames_and_the_line() {
        let env = Env {
            choice: Some(0),
            menus: vec!["offer".into()],
            ..Default::default()
        };
        let mut r = CutsceneRun::new("c", def(SCENE), false);
        let log = run(&mut r, &env, 1000);
        let at = |c: &Command| log.iter().find(|(_, x)| x == c).map(|(f, _)| *f);
        assert_eq!(
            at(&Command::Walk {
                actor: "guide".into(),
                waypoints: vec![[1, 1], [2, 1]],
                speed: None,
                run: false,
                clip: None,
            }),
            Some(0)
        );
        // The walk takes 20 frames; the wait 5 more.
        let say = at(&Command::Say("offer".into())).unwrap();
        assert_eq!(say, 25);
        // The line closes 30 frames on, and option 0 jumps to `yes`.
        assert_eq!(at(&Command::SetFlag("agreed".into())), Some(say + 30));
        assert_eq!(
            at(&Command::Fight(Fight::Boss("warden".into()))),
            Some(say + 30)
        );
        assert!(at(&Command::SetFlag("refused".into())).is_none());
        assert!(r.finished);
    }

    #[test]
    fn the_other_option_takes_the_other_branch() {
        let env = Env {
            choice: Some(1),
            menus: vec!["offer".into()],
            ..Default::default()
        };
        let mut r = CutsceneRun::new("c", def(SCENE), false);
        let log = run(&mut r, &env, 1000);
        assert!(
            log.iter()
                .any(|(_, c)| *c == Command::SetFlag("refused".into()))
        );
        assert!(!log.iter().any(|(_, c)| matches!(c, Command::Fight(_))));
        assert!(r.finished);
    }

    #[test]
    fn the_same_answers_give_the_same_commands() {
        let make = || {
            let env = Env {
                choice: Some(0),
                menus: vec!["offer".into()],
                ..Default::default()
            };
            let mut r = CutsceneRun::new("c", def(SCENE), false);
            run(&mut r, &env, 1000)
        };
        assert_eq!(make(), make());
    }

    const TRACKS: &str = r#"
[cutscenes.c]
steps = [
  { do = "start_track", track = "pan" },
  { do = "move", actor = "player", to = [3, 3] },
  { do = "set_flag", flag = "walked" },
  { do = "join", track = "pan" },
  { do = "set_flag", flag = "joined" },
]
[cutscenes.c.tracks.pan]
steps = [
  { do = "camera", focus = { tile = [4, 4] }, zoom = 2.0, frames = 40 },
  { do = "set_flag", flag = "panned" },
]
"#;

    #[test]
    fn a_track_runs_beside_the_main_one_and_a_join_waits_for_it() {
        let env = Env::default();
        let mut r = CutsceneRun::new("c", def(TRACKS), false);
        let mut log = Vec::new();
        let mut poses = Vec::new();
        for f in 0..200 {
            let cmds = r.step(&env, 1);
            env.apply(&cmds);
            log.extend(cmds.into_iter().map(|c| (f, c)));
            poses.push(r.camera.pose());
            if r.finished {
                break;
            }
            env.tick();
        }
        let at = |flag: &str| {
            log.iter()
                .find(|(_, c)| *c == Command::SetFlag(flag.into()))
                .map(|(f, _)| *f)
        };
        // The walk (10 frames) ends before the pan (40), and the main track
        // waits at the join for the pan.
        assert_eq!(at("walked"), Some(10));
        assert_eq!(at("panned"), Some(40));
        assert_eq!(at("joined"), Some(40));
        // The pan eases: it starts and ends still and lands on the tile.
        assert_eq!(poses[0].zoom, 1.0);
        let end = poses[40];
        assert_eq!(end.focus, [4.0 * 128.0 + 64.0, 4.0 * 128.0 + 64.0]);
        assert_eq!((end.zoom, end.focus_weight), (2.0, 1.0));
        let d0 = poses[1].zoom - poses[0].zoom;
        let dm = poses[20].zoom - poses[19].zoom;
        assert!(d0 < dm, "eased in: {d0} < {dm}");
    }

    #[test]
    fn a_skip_lands_the_walks_and_runs_to_the_end_but_stops_at_a_menu() {
        let env = Env {
            menus: vec!["offer".into()],
            ..Default::default()
        };
        // Not seen: not skippable.
        let mut r = CutsceneRun::new("c", def(SCENE), false);
        let cmds = r.step(&env, 1);
        env.apply(&cmds);
        assert!(r.skip(&env).is_none());
        // Seen: the skip lands the walk and runs up to the menu line.
        let mut r = CutsceneRun::new("c", def(SCENE), true);
        let cmds = r.step(&env, 1);
        env.apply(&cmds);
        let cmds = r.skip(&env).expect("skippable");
        assert_eq!(
            cmds,
            vec![
                Command::FinishWalk {
                    actor: "guide".into()
                },
                Command::Say("offer".into()),
            ]
        );
        assert!(!r.finished, "the menu is the player's");
        env.apply(&cmds);
        assert!(r.skip(&env).is_none(), "no skip over a menu");
    }

    #[test]
    fn a_skip_without_a_menu_finishes_and_keeps_the_state_changes() {
        let env = Env::default();
        let mut r = CutsceneRun::new("c", def(TRACKS), true);
        let cmds = r.step(&env, 1);
        env.apply(&cmds);
        let cmds = r.skip(&env).unwrap();
        assert!(r.finished && r.skipped);
        for f in ["walked", "panned", "joined"] {
            assert!(cmds.contains(&Command::SetFlag(f.into())), "{f}: {cmds:?}");
        }
        assert_eq!(r.camera.pose().zoom, 2.0, "the glide landed");
    }

    #[test]
    fn a_branch_reads_flags_and_counters() {
        let text = r#"
[cutscenes.c]
steps = [
  { do = "branch", requires = { flags = ["a"] }, counter = "n", at_least = 2, goto = "hit", otherwise = "miss" },
  { do = "label", name = "hit" },
  { do = "set_flag", flag = "hit" },
  { do = "end" },
  { do = "label", name = "miss" },
  { do = "set_flag", flag = "miss" },
]
"#;
        let go = |flags: Vec<String>, counter| {
            let env = Env {
                flags,
                counter,
                ..Default::default()
            };
            let mut r = CutsceneRun::new("c", def(text), false);
            r.step(&env, 1)
        };
        assert_eq!(
            go(vec!["a".into()], 2),
            vec![Command::SetFlag("hit".into())]
        );
        assert_eq!(
            go(vec!["a".into()], 1),
            vec![Command::SetFlag("miss".into())]
        );
        assert_eq!(go(vec![], 5), vec![Command::SetFlag("miss".into())]);
    }

    #[test]
    fn a_loop_that_never_waits_is_ended() {
        let text = r#"
[cutscenes.c]
steps = [
  { do = "label", name = "top" },
  { do = "counter", counter = "n" },
  { do = "goto", label = "top" },
]
"#;
        let env = Env::default();
        let mut r = CutsceneRun::new("c", def(text), false);
        let cmds = r.step(&env, 1);
        assert!(r.finished);
        assert!(cmds.len() < STEP_BUDGET);
    }

    #[test]
    fn fades_ramp_and_the_letterbox_slides() {
        let text = r#"
[cutscenes.c]
letterbox = true
steps = [
  { do = "fade", to = "black", frames = 10 },
  { do = "fade", to = "clear", frames = 10 },
  { do = "flash", color = [255, 255, 255], frames = 4 },
  { do = "wait", frames = 30 },
]
"#;
        let env = Env::default();
        let mut r = CutsceneRun::new("c", def(text), false);
        let mut levels = Vec::new();
        for _ in 0..25 {
            r.step(&env, 1);
            levels.push(r.screen.fade_level());
        }
        assert_eq!(levels[0], 0.0);
        assert_eq!(levels[10], 1.0);
        assert_eq!(levels[20], 0.0);
        assert!(levels[5] > 0.4 && levels[5] < 0.6);
        assert_eq!(r.screen.letterbox_lines(), LETTERBOX_LINES);
        // Mid-fade the quad subtracts a grey.
        let mut r = CutsceneRun::new("c", def(text), false);
        for _ in 0..6 {
            r.step(&env, 1);
        }
        let q = r.screen.quads();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].1, 2);
    }

    #[test]
    fn a_camera_on_an_actor_follows_it() {
        let text = r#"
[cutscenes.c]
steps = [
  { do = "camera", focus = { actor = "guide" }, frames = 0 },
  { do = "wait", frames = 3 },
]
"#;
        let env = Env::default();
        let mut r = CutsceneRun::new("c", def(text), false);
        r.step(&env, 1);
        r.step(&env, 1);
        assert_eq!(r.camera.pose().focus, [1000.0, 2000.0]);
        assert!(r.camera.active());
    }
}
