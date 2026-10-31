//! The in-world stage of a golf round or a race played on a pack map: the
//! session's state mapped onto the field scene the map builds.
//!
//! The session stays the only authority. [`WorldStage::step`] reads its view
//! once a frame and returns a [`StageFrame`] - where the party leader
//! stands and what clip it plays, the balls, the aim arrow, the other seats'
//! bodies, the boost trails, the camera shot and the banner - and nothing it
//! keeps (camera ease, the leader's walk, banner timers) ever reaches the
//! session, so a session ticks the same with the stage running or not, and
//! a lockstep peer that draws nothing agrees with one that draws everything.
//!
//! Positions are field world `(X, Z)` through the session's
//! [`MapFrame`]; heights are the world's to fill (the floor sampler), so
//! this module needs no scene.
//!
//! The leader is the session's local seat ([`ActiveMode::local_seat`],
//! seat 0 offline): a networked peer stands its own golfer or racer.
//!
//! - **Golf.** The leader stands at the player's ball, beside it and facing
//!   the aim, plays the swing clip for [`SWING_FRAMES`] after a strike, and
//!   walks to the ball's new lie once it rests. While a ball rolls the
//!   camera follows it along the shot's line; between shots it returns
//!   behind the next ball to play, looking down its aim.
//! - **Racing.** The leader is the player's racer, facing its heading, its
//!   run clip at speed. The camera chases from behind on the heading.
//! - **Grid Duel.** A duel at a table ([`crate::duel_stage`]): the leader
//!   stands at the player's side, the cards and NPCs are the table's.

use legaia_engine_modes::active::{ActiveMode, ModeView, seat_name_for};
use legaia_engine_modes::cards::duel::DuelView;
use legaia_engine_modes::cards::table::DuelTableFrame;
use legaia_engine_modes::fixed::Vec2;
use legaia_engine_modes::fixed::{ANGLE_TURN, FX_ONE};
use legaia_engine_modes::golf::course::{Course, Surface, TILE};
use legaia_engine_modes::golf::{GolfPhase, GolfView};
use legaia_engine_modes::map_frame::MapFrame;
use legaia_engine_modes::poker::PokerView;
use legaia_engine_modes::poker::table::PokerTableFrame;
use legaia_engine_modes::racing::{BOOST_SPEED, MAX_SPEED, RacePhase, RaceView};

use crate::duel_stage::{DuelNpcPose, DuelStage, DuelStageFrame};
use crate::poker_stage::{PokerStage, PokerStageFrame};

/// Frames the leader holds the swing clip after a strike.
pub const SWING_FRAMES: u32 = 24;
/// How far beside the ball the golfer stands, world units.
pub const ADDRESS_OFFSET: f32 = 40.0;
/// The leader's walk to the ball, world units a frame (the field run).
pub const WALK_SPEED: f32 = 12.0;
/// Frames a banner stays up.
pub const BANNER_FRAMES: u32 = 120;
/// Boost trail points kept per racer.
pub const TRAIL_POINTS: usize = 14;
/// Camera focus ease per frame (fraction of the remaining distance).
pub const FOCUS_EASE: f32 = 0.18;
/// Camera yaw ease per frame.
pub const YAW_EASE: f32 = 0.12;

/// The clip the leader plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderClip {
    Idle,
    Walk,
    Run,
    /// The golf swing. The party bank carries no swing, so the host plays
    /// the landing clip, whose crouch-and-rise reads as a putt.
    Swing,
}

/// The party leader's pose on the stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StagePose {
    /// World `(X, Z)`.
    pub pos: [f32; 2],
    /// Facing as a mode angle (`0` = world `+X`, a quarter turn = `+Z`).
    pub facing: u16,
    pub clip: LeaderClip,
}

/// Another seat's body: an AI golfer at its ball, an AI racer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageBody {
    pub seat: usize,
    pub pos: [f32; 2],
    pub facing: u16,
    pub boosting: bool,
    /// Under way (a racer with speed): the body plays its walk clip.
    pub moving: bool,
    /// The field NPC slot that draws this body, when the instance names one
    /// (`bodies`; the world fills it in). `None` draws a stand-in figure.
    pub actor: Option<u8>,
}

/// A golf ball.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageBall {
    pub seat: usize,
    pub pos: [f32; 2],
    pub rolling: bool,
}

/// The aim arrow on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageAim {
    pub from: [f32; 2],
    /// Mode angle.
    pub angle: u16,
    /// Arrow length, world units.
    pub length: f32,
    /// The power meter while it runs (`0..=100`).
    pub power: Option<u32>,
}

/// A racer's boost trail, newest point first.
#[derive(Debug, Clone, PartialEq)]
pub struct StageTrail {
    pub seat: usize,
    pub points: Vec<[f32; 2]>,
}

/// A race's track laid on the ground, for the hosts to mark out: the
/// session's centerline (a closed loop), its half-width and its boost
/// segments, in world units. Drawn only; the session keeps racers on it.
#[derive(Debug, Clone, PartialEq)]
pub struct StageTrack {
    /// World `(X, Z)` of each waypoint, in lap order; segment `i` runs from
    /// point `i` to point `i + 1` (the last back to the first).
    pub points: Vec<[f32; 2]>,
    /// Half the track's width, world units.
    pub half_width: f32,
    /// Segments that boost a racer running along them.
    pub boost_segments: Vec<usize>,
}

/// One playable tile of a golf hole, for the hosts to tint on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageTile {
    /// World `(X, Z)` of the tile's low corner; the tile runs
    /// [`StageCourse::tile`] world units along each axis from it.
    pub at: [f32; 2],
    pub surface: Surface,
    /// Downhill direction, each `-1..=1` (`(0, 0)` flat).
    pub slope: (i8, i8),
}

/// The golf hole in play laid on the ground: its playable tiles (walls and
/// out of bounds left out) and its tee. Drawn only; the session rolls the
/// ball on its own tiles.
#[derive(Debug, Clone, PartialEq)]
pub struct StageCourse {
    pub tiles: Vec<StageTile>,
    /// A tile's side, world units.
    pub tile: f32,
    /// World `(X, Z)` of the tee.
    pub tee: [f32; 2],
}

/// The camera's shot: look at `focus` from behind along `yaw`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageShot {
    /// World `(X, Z)` the camera looks at.
    pub focus: [f32; 2],
    /// The direction the camera looks, as a mode angle in 12-bit units
    /// (fractional while it eases).
    pub yaw: f32,
    /// How far the camera looks down, 12-bit units (the field follow
    /// camera's is `450`).
    pub pitch: f32,
    /// Distance scale over the field follow camera's (`1` = the same).
    pub distance: f32,
    /// The focus's height above the floor, world units (a duel table's
    /// top; `0` on the ground).
    pub lift: f32,
}

impl StageShot {
    /// One step of an ease toward `target`: position, pitch, distance and
    /// lift by `k` of the remaining distance, the yaw the short way round.
    pub fn ease_to(&self, target: &Self, k: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * k;
        Self {
            focus: ease(self.focus, target.focus, k),
            yaw: ease_angle(self.yaw, target.yaw, k),
            pitch: l(self.pitch, target.pitch),
            distance: l(self.distance, target.distance),
            lift: l(self.lift, target.lift),
        }
    }
}

/// The shots' pitches, 12-bit units: aiming looks down on the ball and its
/// line, a rolling ball is watched from higher, the race chases from high
/// enough that a map's walls rarely stand between the camera and the racer.
pub const AIM_PITCH: f32 = 600.0;
pub const ROLL_PITCH: f32 = 680.0;
pub const CUP_PITCH: f32 = 720.0;
pub const CHASE_PITCH: f32 = 540.0;

/// One frame of the stage.
#[derive(Debug, Clone, PartialEq)]
pub struct StageFrame {
    pub frame: MapFrame,
    pub leader: Option<StagePose>,
    pub bodies: Vec<StageBody>,
    pub balls: Vec<StageBall>,
    pub cup: Option<[f32; 2]>,
    pub aim: Option<StageAim>,
    pub trails: Vec<StageTrail>,
    /// A race's track, to mark out on the ground.
    pub track: Option<StageTrack>,
    /// A golf hole's tiles, to tint on the ground.
    pub course: Option<StageCourse>,
    pub shot: StageShot,
    pub banner: Option<String>,
    /// A Grid Duel's table: cards, bursts, NPCs and cues.
    pub duel: Option<DuelStageFrame>,
    /// A poker table: cards, chips, NPCs and cues.
    pub poker: Option<PokerStageFrame>,
    /// A party night's podium: blocks, figures, confetti.
    pub podium: Option<crate::podium_stage::PodiumFrame>,
}

impl StageFrame {
    /// The NPCs a table drives this frame and the sound cues it plays.
    pub fn table_npcs_and_cues(
        &self,
    ) -> Option<(&[DuelNpcPose], &[legaia_expansion_format::audio::SoundRef])> {
        if let Some(d) = &self.duel {
            return Some((&d.npcs, &d.cues));
        }
        self.poker.as_ref().map(|p| (&p.npcs[..], &p.cues[..]))
    }
}

/// The hole in play's playable tiles in world units.
fn stage_course(c: &Course, frame: &MapFrame) -> StageCourse {
    let mut tiles = Vec::new();
    for ty in 0..c.height as i32 {
        for tx in 0..c.width as i32 {
            let low = Vec2::new(tx * TILE * FX_ONE, ty * TILE * FX_ONE);
            let centre = Vec2::new(
                (tx * TILE + TILE / 2) * FX_ONE,
                (ty * TILE + TILE / 2) * FX_ONE,
            );
            let t = c.tile_at(centre);
            if matches!(t.surface, Surface::Wall | Surface::OutOfBounds) {
                continue;
            }
            tiles.push(StageTile {
                at: frame.to_world_f(low),
                surface: t.surface,
                slope: t.slope,
            });
        }
    }
    StageCourse {
        tiles,
        tile: frame.len_to_world(TILE * FX_ONE),
        tee: frame.to_world_f(c.tee),
    }
}

/// What the stage remembers between frames. Presentation only.
#[derive(Debug, Clone, Default)]
pub struct WorldStage {
    shot: Option<StageShot>,
    leader: Option<[f32; 2]>,
    swing: u32,
    /// Golf: each seat's stroke count and holed flag last frame.
    last_strokes: Vec<(u8, bool)>,
    /// Racing: seat 0's lap last frame, and whether it had finished.
    last_lap: Option<(u8, bool)>,
    banner: Option<(String, u32)>,
    trails: Vec<Vec<[f32; 2]>>,
    /// A duel table's memory.
    pub duel: DuelStage,
    /// A poker table's memory.
    pub poker: PokerStage,
}

fn angle_rad(a: f32) -> f32 {
    a / f32::from(ANGLE_TURN) * std::f32::consts::TAU
}

fn ahead(p: [f32; 2], angle: f32, len: f32) -> [f32; 2] {
    let r = angle_rad(angle);
    [p[0] + r.cos() * len, p[1] + r.sin() * len]
}

/// Ease angle `from` toward `to` (12-bit units) the short way round.
fn ease_angle(from: f32, to: f32, k: f32) -> f32 {
    let turn = f32::from(ANGLE_TURN);
    let d = (to - from + turn * 1.5).rem_euclid(turn) - turn * 0.5;
    (from + d * k).rem_euclid(turn)
}

fn ease(from: [f32; 2], to: [f32; 2], k: f32) -> [f32; 2] {
    [
        from[0] + (to[0] - from[0]) * k,
        from[1] + (to[1] - from[1]) * k,
    ]
}

/// A golf result as a player names it, against `par`.
pub fn score_name(strokes: u8, par: u8) -> String {
    if strokes == 1 {
        return "Hole in one!".to_string();
    }
    match i32::from(strokes) - i32::from(par) {
        -3 => "Albatross!".to_string(),
        -2 => "Eagle!".to_string(),
        -1 => "Birdie!".to_string(),
        0 => "Par".to_string(),
        1 => "Bogey".to_string(),
        2 => "Double bogey".to_string(),
        n => format!("{n:+} over par"),
    }
}

impl WorldStage {
    /// The stage of `mode` this frame, or `None` when the session is not
    /// played on a map (the host draws its top-down screen instead).
    pub fn step(&mut self, mode: &ActiveMode) -> Option<StageFrame> {
        let frame = mode.map_frame()?.clone();
        let mut out = match mode.view() {
            ModeView::Golf(v) => {
                let mut f = self.golf(&frame, &v, mode.local_seat);
                f.course = mode.golf_course().map(|c| stage_course(c, &frame));
                f
            }
            ModeView::Race(v) => {
                let mut f = self.race(&frame, &v, mode.local_seat);
                f.track = mode.race_track().map(|t| StageTrack {
                    points: t.points.iter().map(|&p| frame.to_world_f(p)).collect(),
                    half_width: frame.len_to_world(t.half_width),
                    boost_segments: t.boost_segments.clone(),
                });
                f
            }
            ModeView::Duel(v) => {
                let table = mode.duel_table()?;
                self.duel.watching = mode.watching;
                self.duel.seat_names = [mode.seat_label(0, "Rival"), mode.seat_label(1, "Rival")];
                return Some(self.duel_frame(table, &v, mode.local_seat));
            }
            ModeView::Poker(v) => {
                let table = mode.poker_table()?;
                self.poker.watching = mode.watching;
                return Some(self.poker_frame(table, &v, mode.local_seat));
            }
            _ => return None,
        };
        if let Some((_, t)) = &mut self.banner {
            *t = t.saturating_sub(1);
        }
        if self.banner.as_ref().is_some_and(|(_, t)| *t == 0) {
            self.banner = None;
        }
        if out.banner.is_none() {
            out.banner = self.banner.as_ref().map(|(s, _)| s.clone());
        }
        Some(out)
    }

    /// A duel table's frame off `view`.
    fn duel_frame(&mut self, table: &DuelTableFrame, view: &DuelView, local: usize) -> StageFrame {
        let duel = self.duel.step(table, view, local);
        StageFrame {
            frame: table.frame.clone(),
            leader: Some(self.duel.leader(table)),
            bodies: Vec::new(),
            balls: Vec::new(),
            cup: None,
            aim: None,
            trails: Vec::new(),
            track: None,
            course: None,
            shot: duel.shot,
            banner: duel.banner.clone(),
            duel: Some(duel),
            poker: None,
            podium: None,
        }
    }

    /// A poker table's frame off `view`.
    fn poker_frame(
        &mut self,
        table: &PokerTableFrame,
        view: &PokerView,
        local: usize,
    ) -> StageFrame {
        let poker = self.poker.step(table, view, local);
        StageFrame {
            frame: table.frame.clone(),
            leader: Some(self.poker.leader(&poker.table)),
            bodies: Vec::new(),
            balls: Vec::new(),
            cup: None,
            aim: None,
            trails: Vec::new(),
            track: None,
            course: None,
            shot: poker.shot,
            banner: poker.banner.clone(),
            duel: None,
            poker: Some(poker),
            podium: None,
        }
    }

    /// The stage after the session has left (the seat's results stage): a
    /// duel table keeps playing its last moves and the reaction shot off
    /// the last view. `None` for every other mode.
    pub fn step_after(&mut self) -> Option<StageFrame> {
        if let (Some(t), Some(v)) = (self.poker.table().cloned(), self.poker.last_view().cloned()) {
            let local = self.poker.local();
            return Some(self.poker_frame(&t, &v, local));
        }
        let table = self.duel.table()?.clone();
        let view = self.duel.last_view()?.clone();
        let local = self.duel.local();
        Some(self.duel_frame(&table, &view, local))
    }

    /// Note `mode`'s last view as the session leaves play, so the results
    /// stage plays out the move that ended it.
    pub fn note_final(&mut self, mode: &ActiveMode) {
        match mode.view() {
            ModeView::Duel(v) => self.duel.note_final(v),
            ModeView::Poker(v) => self.poker.note_final(v),
            _ => {}
        }
    }

    /// Whether the results card waits for the stage (a duel's reaction
    /// shot).
    pub fn holds_results(&self) -> bool {
        self.duel.holds_results()
    }

    fn raise(&mut self, text: String) {
        self.banner = Some((text, BANNER_FRAMES));
    }

    /// Ease the camera toward `target`; the first frame cuts.
    fn shoot(&mut self, target: StageShot) -> StageShot {
        let s = match self.shot {
            None => target,
            Some(s) => StageShot {
                yaw: ease_angle(s.yaw, target.yaw, YAW_EASE),
                ..s.ease_to(&target, FOCUS_EASE)
            },
        };
        self.shot = Some(s);
        s
    }

    fn golf(&mut self, frame: &MapFrame, v: &GolfView, local: usize) -> StageFrame {
        let local = local.min(v.balls.len().saturating_sub(1));
        let w = |p| frame.to_world_f(p);
        let rolling_seat = match v.phase {
            GolfPhase::Rolling { seat } => Some(seat),
            _ => None,
        };
        // Banners off the scorecard's movement: a ball dropping names the
        // score, a new hole its number.
        let now: Vec<(u8, bool)> = v.balls.iter().map(|b| (b.strokes, b.done)).collect();
        if now.len() == self.last_strokes.len() {
            let dropped: Vec<usize> = (0..now.len())
                .filter(|&s| now[s].1 && !self.last_strokes[s].1 && now[s].0 > 0)
                .collect();
            for seat in dropped {
                let what = score_name(now[seat].0, v.par);
                self.raise(if seat == local {
                    what
                } else {
                    format!("{}: {what}", seat_name_for(seat, local))
                });
            }
            if self.last_strokes.iter().all(|s| s.1) && now.iter().all(|s| s.0 == 0) {
                self.raise(format!("Hole {} - par {}", v.hole + 1, v.par));
            }
        } else {
            self.raise(format!("Hole {} - par {}", v.hole + 1, v.par));
        }
        if let Some(seat) = rolling_seat
            && let Some(last) = self.last_strokes.get(seat)
            && v.balls[seat].strokes > last.0
            && seat == local
        {
            self.swing = SWING_FRAMES;
        }
        self.last_strokes = now;

        let ball_pos = |seat: usize| match (rolling_seat, v.rolling) {
            (Some(s), Some(r)) if s == seat => r.pos,
            _ => v.balls[seat].pos,
        };
        let balls = v
            .balls
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.done)
            .map(|(seat, _)| StageBall {
                seat,
                pos: w(ball_pos(seat)),
                rolling: rolling_seat == Some(seat),
            })
            .collect();
        let cup = w(v.cup);
        let to_play = match v.phase {
            GolfPhase::Aim { seat } | GolfPhase::Power { seat, .. } => Some(seat),
            _ => None,
        };
        let aim = to_play.map(|seat| {
            let b = &v.balls[seat];
            let power = match v.phase {
                GolfPhase::Power { meter, .. } => Some(meter),
                _ => None,
            };
            StageAim {
                from: w(b.pos),
                angle: b.aim,
                length: frame.len_to_world(3 * 32 * FX_ONE)
                    * (0.35 + 0.65 * power.unwrap_or(100) as f32 / 100.0),
                power,
            }
        });

        // The leader: at the player's resting ball (where it was struck
        // from while it rolls), beside it, facing the aim; at the cup once
        // holed.
        let me = &v.balls[local];
        let (stand, facing) = if me.done {
            (
                ahead(cup, f32::from(me.aim) + 1024.0, ADDRESS_OFFSET * 2.0),
                me.aim,
            )
        } else {
            let at = w(me.pos);
            (
                ahead(at, f32::from(me.aim) + 1024.0, ADDRESS_OFFSET),
                me.aim,
            )
        };
        let from = self.leader.unwrap_or(stand);
        let d = [stand[0] - from[0], stand[1] - from[1]];
        let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
        let (pos, walking) = if dist <= WALK_SPEED {
            (stand, false)
        } else {
            let k = WALK_SPEED / dist;
            ([from[0] + d[0] * k, from[1] + d[1] * k], true)
        };
        self.leader = Some(pos);
        let walk_facing = (d[1].atan2(d[0]) / std::f32::consts::TAU * f32::from(ANGLE_TURN))
            .rem_euclid(f32::from(ANGLE_TURN)) as u16;
        let clip = if self.swing > 0 {
            self.swing -= 1;
            LeaderClip::Swing
        } else if walking {
            LeaderClip::Run
        } else {
            LeaderClip::Idle
        };
        let leader = StagePose {
            pos,
            facing: if walking { walk_facing } else { facing },
            clip,
        };
        let bodies = v
            .balls
            .iter()
            .enumerate()
            .filter(|&(seat, _)| seat != local)
            .map(|(seat, b)| {
                // The other side of the ball from the leader, so a shared
                // tee shows both.
                let at = if b.done { cup } else { w(b.pos) };
                StageBody {
                    seat,
                    pos: ahead(at, f32::from(b.aim) - 1024.0, ADDRESS_OFFSET),
                    facing: b.aim,
                    boosting: false,
                    moving: false,
                    actor: None,
                }
            })
            .collect();

        // The camera: behind the ball to play down its aim; following the
        // rolling ball along the shot's line; on the cup between holes.
        let target = match (v.phase, v.rolling) {
            (GolfPhase::Rolling { seat }, Some(r)) => StageShot {
                focus: w(r.pos),
                yaw: f32::from(v.balls[seat].aim),
                pitch: ROLL_PITCH,
                distance: 0.75,
                lift: 0.0,
            },
            (GolfPhase::Aim { seat } | GolfPhase::Power { seat, .. }, _) => StageShot {
                focus: ahead(w(v.balls[seat].pos), f32::from(v.balls[seat].aim), 160.0),
                yaw: f32::from(v.balls[seat].aim),
                pitch: AIM_PITCH,
                distance: 0.55,
                lift: 0.0,
            },
            _ => StageShot {
                focus: cup,
                yaw: self.shot.map_or(0.0, |s| s.yaw),
                pitch: CUP_PITCH,
                distance: 0.8,
                lift: 0.0,
            },
        };
        let shot = self.shoot(target);
        StageFrame {
            frame: frame.clone(),
            leader: Some(leader),
            bodies,
            balls,
            cup: Some(cup),
            aim,
            trails: Vec::new(),
            track: None,
            course: None,
            shot,
            banner: None,
            duel: None,
            poker: None,
            podium: None,
        }
    }

    fn race(&mut self, frame: &MapFrame, v: &RaceView, local: usize) -> StageFrame {
        let w = |p| frame.to_world_f(p);
        self.trails.resize(v.racers.len(), Vec::new());
        for (seat, r) in v.racers.iter().enumerate() {
            let t = &mut self.trails[seat];
            if r.boost_frames > 0 {
                t.insert(0, w(r.pos));
                t.truncate(TRAIL_POINTS);
            } else if !t.is_empty() {
                t.pop();
            }
        }
        let banner = match v.phase {
            RacePhase::Countdown { frames } => Some(format!("{}", frames / 60 + 1)),
            RacePhase::Racing if v.frame < 60 => Some("Go!".to_string()),
            _ => None,
        };
        if let Some(me) = v.racers.get(local) {
            let now = (me.lap, me.finished.is_some());
            if let Some(last) = self.last_lap {
                if now.1 && !last.1 {
                    let place = v
                        .standings
                        .iter()
                        .position(|&s| s == local)
                        .map_or(0, |p| p + 1);
                    self.raise(format!("Finished - place {place}"));
                } else if now.0 > last.0 && !now.1 {
                    self.raise(format!("Lap {}", now.0 + 1));
                }
            }
            self.last_lap = Some(now);
        }
        let pose = |r: &legaia_engine_modes::racing::Racer| (w(r.pos), r.heading);
        let leader = v.racers.get(local).map(|r| {
            let (pos, facing) = pose(r);
            StagePose {
                pos,
                facing,
                clip: if r.speed > MAX_SPEED / 2 {
                    LeaderClip::Run
                } else if r.speed > 0 {
                    LeaderClip::Walk
                } else {
                    LeaderClip::Idle
                },
            }
        });
        let bodies = v
            .racers
            .iter()
            .enumerate()
            .filter(|&(seat, _)| seat != local)
            .map(|(seat, r)| {
                let (pos, facing) = pose(r);
                StageBody {
                    seat,
                    pos,
                    facing,
                    boosting: r.boost_frames > 0,
                    moving: r.speed > 0,
                    actor: None,
                }
            })
            .collect();
        let trails = self
            .trails
            .iter()
            .enumerate()
            .filter(|(_, t)| t.len() > 1)
            .map(|(seat, t)| StageTrail {
                seat,
                points: t.clone(),
            })
            .collect();
        let target = match v.racers.get(local) {
            Some(r) => {
                // A little ahead of the racer, pulled back as it speeds up.
                let pull = 1.0 + 0.25 * (r.speed.max(0) as f32 / BOOST_SPEED as f32);
                StageShot {
                    focus: ahead(w(r.pos), f32::from(r.heading), 96.0),
                    yaw: f32::from(r.heading),
                    pitch: CHASE_PITCH,
                    distance: 0.5 * pull,
                    lift: 0.0,
                }
            }
            None => StageShot {
                focus: [frame.origin[0] as f32, frame.origin[1] as f32],
                yaw: 0.0,
                pitch: CHASE_PITCH,
                distance: 1.0,
                lift: 0.0,
            },
        };
        let shot = self.shoot(target);
        StageFrame {
            frame: frame.clone(),
            leader,
            bodies,
            balls: Vec::new(),
            cup: None,
            aim: None,
            trails,
            track: None,
            course: None,
            shot,
            banner,
            duel: None,
            poker: None,
            podium: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_names_follow_par() {
        assert_eq!(score_name(1, 3), "Hole in one!");
        assert_eq!(score_name(2, 3), "Birdie!");
        assert_eq!(score_name(3, 3), "Par");
        assert_eq!(score_name(6, 3), "+3 over par");
    }

    #[test]
    fn angles_ease_the_short_way_round() {
        let a = ease_angle(4000.0, 100.0, 0.5);
        assert!(a > 4000.0 || a < 100.0, "{a}");
        assert!((ease_angle(0.0, 1024.0, 1.0) - 1024.0).abs() < 1e-3);
    }
}
