//! Mini-golf for 1-8 players.
//!
//! A course is a list of tile-grid holes ([`course::Course`]). Each hole
//! every player tees off in turn order - seat order on the first hole,
//! then lowest score on the previous hole first - and after that the
//! player farthest from the cup plays next. A stroke is two presses: in
//! the aim phase Left / Right turn the aim (Square held for fine steps)
//! and Cross starts the power meter, which climbs to 100 and falls back
//! until Cross locks it (Circle returns to aiming). The ball then rolls
//! under [`course::Course::step`]; water or out of bounds costs a penalty
//! stroke and replays from where the ball was struck. A player who
//! reaches the hole's stroke cap picks up and scores the cap.
//!
//! AI seats look ahead with the same physics: they simulate candidate
//! shots and take the one that ends nearest the cup, then perturb aim and
//! power by their tier's error.

pub mod course;

use legaia_engine_system::pad::PadButton;
use serde::{Deserialize, Serialize};

use crate::fixed::{Vec2, angle_delta};
use crate::session::{
    AiTier, ModeResults, ModeRng, ModeSession, PadEdges, PadWord, SeatKind, StateHasher,
};
use course::{Ball, Course, CourseError, RollEnd};

/// Aim turn per frame held, coarse and fine.
pub const AIM_STEP: u16 = 24;
pub const AIM_STEP_FINE: u16 = 4;
/// Power-meter change per frame.
pub const METER_STEP: u32 = 2;
/// Frames the scorecard stays up between holes.
pub const HOLE_OVER_FRAMES: u32 = 90;

/// A round's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GolfConfig {
    pub seats: Vec<SeatKind>,
    pub holes: Vec<Course>,
    pub ai_think_frames: u32,
    pub seed: u64,
}

impl GolfConfig {
    /// Build from a pack course: seat 0 the player, `ai` AI seats after.
    pub fn from_course(
        c: &legaia_expansion_format::modes::GolfCourse,
        ai: &[AiTier],
        seed: u64,
    ) -> Result<Self, CourseError> {
        let holes = c
            .holes
            .iter()
            .map(Course::from_hole)
            .collect::<Result<Vec<_>, _>>()?;
        if holes.is_empty() {
            return Err(CourseError::Empty);
        }
        let mut seats = vec![SeatKind::Human];
        seats.extend(ai.iter().map(|&t| SeatKind::Ai(t)));
        Ok(Self {
            seats,
            holes,
            ai_think_frames: 30,
            seed,
        })
    }
}

/// The stroke phase of the player to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GolfPhase {
    Aim {
        seat: usize,
    },
    Power {
        seat: usize,
        meter: u32,
        rising: bool,
    },
    Rolling {
        seat: usize,
    },
    HoleOver {
        timer: u32,
    },
    Finished,
}

/// What a tick reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GolfEvent {
    HoleStarted { hole: usize },
    Struck { seat: usize, power: u32 },
    WallHit { seat: usize },
    Hazard { seat: usize },
    Holed { seat: usize, strokes: u8 },
    PickedUp { seat: usize },
    HoleFinished { hole: usize },
    Finished,
}

/// A player's ball and stroke count on the current hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerBall {
    pub pos: Vec2,
    pub aim: u16,
    pub strokes: u8,
    pub done: bool,
}

/// The round snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GolfView {
    pub phase: GolfPhase,
    pub hole: usize,
    pub par: u8,
    pub cup: Vec2,
    pub balls: Vec<PlayerBall>,
    /// The ball in flight, if any.
    pub rolling: Option<Ball>,
    /// `scorecard[seat][hole]`, `None` for unplayed holes.
    pub scorecard: Vec<Vec<Option<u8>>>,
}

/// A running round.
#[derive(Debug, Clone)]
pub struct GolfSession {
    cfg: GolfConfig,
    hole: usize,
    balls: Vec<PlayerBall>,
    rolling: Option<Ball>,
    struck_from: Vec2,
    scorecard: Vec<Vec<Option<u8>>>,
    /// Tee order for the current hole.
    order: Vec<usize>,
    teed: Vec<bool>,
    phase: GolfPhase,
    ai_wait: u32,
    ai_shot: Option<(u16, u32)>,
    rng: ModeRng,
    pads: PadEdges,
}

impl GolfSession {
    pub fn new(cfg: GolfConfig) -> Self {
        assert!(!cfg.holes.is_empty() && !cfg.seats.is_empty());
        assert!(cfg.seats.len() <= crate::session::MAX_SEATS);
        let n = cfg.seats.len();
        let mut s = Self {
            hole: 0,
            balls: Vec::new(),
            rolling: None,
            struck_from: Vec2::ZERO,
            scorecard: vec![vec![None; cfg.holes.len()]; n],
            order: (0..n).collect(),
            teed: vec![false; n],
            phase: GolfPhase::HoleOver { timer: 0 },
            ai_wait: 0,
            ai_shot: None,
            rng: ModeRng::new(cfg.seed),
            pads: PadEdges::default(),
            cfg,
        };
        s.start_hole(0, &mut Vec::new());
        s
    }

    pub fn phase(&self) -> GolfPhase {
        self.phase
    }

    pub fn course(&self) -> &Course {
        &self.cfg.holes[self.hole]
    }

    /// Total strokes per seat over the holes played.
    pub fn totals(&self) -> Vec<i64> {
        self.scorecard
            .iter()
            .map(|h| h.iter().flatten().map(|&s| s as i64).sum())
            .collect()
    }

    fn start_hole(&mut self, hole: usize, ev: &mut Vec<GolfEvent>) {
        self.hole = hole;
        let tee = self.cfg.holes[hole].tee;
        let cup = self.cfg.holes[hole].cup;
        let aim = (cup - tee).angle();
        let n = self.cfg.seats.len();
        self.balls = vec![
            PlayerBall {
                pos: tee,
                aim,
                strokes: 0,
                done: false,
            };
            n
        ];
        if hole > 0 {
            // Honour: best score on the last hole tees first.
            let prev = hole - 1;
            self.order.sort_by_key(|&s| (self.scorecard[s][prev], s));
        }
        self.teed = vec![false; n];
        ev.push(GolfEvent::HoleStarted { hole });
        self.next_player(ev);
    }

    /// Pick who plays next: tee order until everyone has teed off, then the
    /// ball farthest from the cup.
    fn next_player(&mut self, ev: &mut Vec<GolfEvent>) {
        let next = if let Some(&s) = self.order.iter().find(|&&s| !self.teed[s]) {
            Some(s)
        } else {
            let course = &self.cfg.holes[self.hole];
            (0..self.balls.len())
                .filter(|&s| !self.balls[s].done)
                .max_by_key(|&s| (course.distance_to_cup(self.balls[s].pos), usize::MAX - s))
        };
        match next {
            Some(seat) => {
                self.phase = GolfPhase::Aim { seat };
                self.ai_wait = self.cfg.ai_think_frames;
                self.ai_shot = None;
            }
            None => {
                ev.push(GolfEvent::HoleFinished { hole: self.hole });
                self.phase = GolfPhase::HoleOver {
                    timer: HOLE_OVER_FRAMES,
                };
            }
        }
    }

    /// Strike for `seat` at its current aim (tests and alternate hosts).
    pub fn strike(&mut self, seat: usize, power: u32) -> Vec<GolfEvent> {
        let b = &mut self.balls[seat];
        b.strokes += 1;
        self.teed[seat] = true;
        self.struck_from = b.pos;
        self.rolling = Some(Course::strike(b.pos, b.aim, power));
        self.phase = GolfPhase::Rolling { seat };
        vec![GolfEvent::Struck { seat, power }]
    }

    /// Set `seat`'s aim (tests and alternate hosts).
    pub fn set_aim(&mut self, seat: usize, aim: u16) {
        self.balls[seat].aim = aim & 0xFFF;
    }

    fn stroke_over(&mut self, seat: usize, ev: &mut Vec<GolfEvent>) {
        let cap = self.cfg.holes[self.hole].max_strokes;
        let b = &mut self.balls[seat];
        if !b.done && b.strokes >= cap {
            b.done = true;
            ev.push(GolfEvent::PickedUp { seat });
        }
        if b.done {
            self.scorecard[seat][self.hole] = Some(b.strokes.min(cap));
        }
        if !b.done {
            let cup = self.cfg.holes[self.hole].cup;
            b.aim = (cup - b.pos).angle();
        }
        self.next_player(ev);
    }

    fn roll(&mut self, seat: usize, ev: &mut Vec<GolfEvent>) {
        let course = &self.cfg.holes[self.hole];
        let mut ball = self.rolling.expect("rolling");
        let before = ball.vel;
        let end = course.step(&mut ball);
        if (before.x != 0 && ball.vel.x.signum() == -before.x.signum())
            || (before.y != 0 && ball.vel.y.signum() == -before.y.signum())
        {
            ev.push(GolfEvent::WallHit { seat });
        }
        self.rolling = Some(ball);
        let Some(end) = end else {
            return;
        };
        self.rolling = None;
        match end {
            RollEnd::Rest => self.balls[seat].pos = ball.pos,
            RollEnd::Holed => {
                let b = &mut self.balls[seat];
                b.pos = ball.pos;
                b.done = true;
                ev.push(GolfEvent::Holed {
                    seat,
                    strokes: b.strokes,
                });
            }
            RollEnd::Hazard => {
                let b = &mut self.balls[seat];
                b.pos = self.struck_from;
                b.strokes += 1;
                ev.push(GolfEvent::Hazard { seat });
            }
        }
        self.stroke_over(seat, ev);
    }

    /// The AI's chosen `(aim, power)` for `seat`.
    fn plan_ai_shot(&mut self, seat: usize, tier: AiTier) -> (u16, u32) {
        let course = &self.cfg.holes[self.hole];
        let from = self.balls[seat].pos;
        let direct = (course.cup - from).angle();
        let (spread, angles, powers, aim_err, power_err): (i32, i32, u32, i32, i32) = match tier {
            AiTier::Novice => (0, 1, 6, 120, 18),
            AiTier::Regular => (160, 5, 10, 40, 8),
            AiTier::Expert => (320, 9, 20, 8, 2),
        };
        let mut best = (i64::MAX, direct, 50u32);
        for ai in 0..angles {
            let off = if angles == 1 {
                0
            } else {
                -spread + 2 * spread * ai / (angles - 1)
            };
            let aim = (direct as i32 + off).rem_euclid(4096) as u16;
            for pi in 1..=powers {
                let power = pi * 100 / powers;
                let (end, rest) = course.simulate(from, aim, power);
                let score = match end {
                    RollEnd::Holed => -1,
                    RollEnd::Hazard => i64::MAX - 1,
                    RollEnd::Rest => course.distance_to_cup(rest),
                };
                if score < best.0 {
                    best = (score, aim, power);
                }
            }
        }
        let aim = (best.1 as i32 + self.rng.range(-aim_err, aim_err)).rem_euclid(4096) as u16;
        let power = (best.2 as i32 + self.rng.range(-power_err, power_err)).clamp(1, 100) as u32;
        (aim, power)
    }

    fn human(&mut self, ev: &mut Vec<GolfEvent>) {
        match self.phase {
            GolfPhase::Aim { seat } => {
                let step = if self.pads.held(seat, PadButton::Square) {
                    AIM_STEP_FINE
                } else {
                    AIM_STEP
                };
                let b = &mut self.balls[seat];
                if self.pads.held(seat, PadButton::Left) {
                    b.aim = b.aim.wrapping_sub(step) & 0xFFF;
                }
                if self.pads.held(seat, PadButton::Right) {
                    b.aim = b.aim.wrapping_add(step) & 0xFFF;
                }
                if self.pads.pressed(seat, PadButton::Cross) {
                    self.phase = GolfPhase::Power {
                        seat,
                        meter: 0,
                        rising: true,
                    };
                }
            }
            GolfPhase::Power {
                seat,
                meter,
                rising,
            } => {
                if self.pads.pressed(seat, PadButton::Circle) {
                    self.phase = GolfPhase::Aim { seat };
                } else if self.pads.pressed(seat, PadButton::Cross) {
                    ev.extend(self.strike(seat, meter));
                } else {
                    let (meter, rising) = match rising {
                        true if meter + METER_STEP >= 100 => (100, false),
                        true => (meter + METER_STEP, true),
                        false if meter <= METER_STEP => (0, true),
                        false => (meter - METER_STEP, false),
                    };
                    self.phase = GolfPhase::Power {
                        seat,
                        meter,
                        rising,
                    };
                }
            }
            _ => {}
        }
    }
}

impl ModeSession for GolfSession {
    type Event = GolfEvent;
    type View = GolfView;

    fn seats(&self) -> usize {
        self.cfg.seats.len()
    }

    fn seat_kind(&self, seat: usize) -> SeatKind {
        self.cfg.seats[seat]
    }

    fn tick(&mut self, pads: &[PadWord]) -> Vec<GolfEvent> {
        self.pads.update(pads);
        let mut ev = Vec::new();
        match self.phase {
            GolfPhase::Finished => {}
            GolfPhase::HoleOver { timer } => {
                if timer > 0 {
                    self.phase = GolfPhase::HoleOver { timer: timer - 1 };
                } else if self.hole + 1 < self.cfg.holes.len() {
                    self.start_hole(self.hole + 1, &mut ev);
                } else {
                    self.phase = GolfPhase::Finished;
                    ev.push(GolfEvent::Finished);
                }
            }
            GolfPhase::Rolling { seat } => self.roll(seat, &mut ev),
            GolfPhase::Aim { seat } | GolfPhase::Power { seat, .. } => match self.cfg.seats[seat] {
                SeatKind::Human => self.human(&mut ev),
                SeatKind::Ai(tier) => {
                    if self.ai_shot.is_none() {
                        let shot = self.plan_ai_shot(seat, tier);
                        self.ai_shot = Some(shot);
                    }
                    let (aim, power) = self.ai_shot.unwrap();
                    // Turn the aim toward the plan at the human step rate,
                    // so a host draws the AI lining up.
                    let b = &mut self.balls[seat];
                    let d = angle_delta(b.aim, aim);
                    let step = d.clamp(-(AIM_STEP as i32), AIM_STEP as i32);
                    b.aim = (b.aim as i32 + step).rem_euclid(4096) as u16;
                    if self.ai_wait > 0 {
                        self.ai_wait -= 1;
                    } else if b.aim == aim {
                        ev.extend(self.strike(seat, power));
                    }
                }
            },
        }
        ev
    }

    fn view(&self) -> GolfView {
        let c = self.course();
        GolfView {
            phase: self.phase,
            hole: self.hole,
            par: c.par,
            cup: c.cup,
            balls: self.balls.clone(),
            rolling: self.rolling,
            scorecard: self.scorecard.clone(),
        }
    }

    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.rng.hash_into(&mut h);
        self.pads.hash_into(&mut h);
        h.usize(self.hole);
        for b in &self.balls {
            h.i32(b.pos.x);
            h.i32(b.pos.y);
            h.u16(b.aim);
            h.u8(b.strokes);
            h.bool(b.done);
        }
        if let Some(r) = &self.rolling {
            h.i32(r.pos.x);
            h.i32(r.pos.y);
            h.i32(r.vel.x);
            h.i32(r.vel.y);
            h.u32(r.slow_frames);
            h.u32(r.frames);
        }
        h.i32(self.struck_from.x);
        h.i32(self.struck_from.y);
        for row in &self.scorecard {
            for s in row {
                h.u8(s.unwrap_or(0xFF));
            }
        }
        for &o in &self.order {
            h.usize(o);
        }
        for &t in &self.teed {
            h.bool(t);
        }
        match self.phase {
            GolfPhase::Aim { seat } => {
                h.u8(0);
                h.usize(seat);
            }
            GolfPhase::Power {
                seat,
                meter,
                rising,
            } => {
                h.u8(1);
                h.usize(seat);
                h.u32(meter);
                h.bool(rising);
            }
            GolfPhase::Rolling { seat } => {
                h.u8(2);
                h.usize(seat);
            }
            GolfPhase::HoleOver { timer } => {
                h.u8(3);
                h.u32(timer);
            }
            GolfPhase::Finished => h.u8(4),
        }
        h.u32(self.ai_wait);
        if let Some((a, p)) = self.ai_shot {
            h.u16(a);
            h.u32(p);
        }
        h.finish()
    }

    fn is_finished(&self) -> bool {
        self.phase == GolfPhase::Finished
    }

    fn results(&self) -> Option<ModeResults> {
        self.is_finished().then(|| ModeResults {
            placements: ModeResults::rank(&self.totals(), true),
            rewards: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::modes::{GolfCourse, GolfHole};

    fn course(holes: &[&[&str]]) -> GolfCourse {
        GolfCourse {
            id: "t".into(),
            holes: holes
                .iter()
                .map(|rows| GolfHole {
                    par: 2,
                    rows: rows.iter().map(|r| r.to_string()).collect(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    const STRAIGHT: &[&str] = &["##########", "#T......O#", "##########"];
    const DOGLEG: &[&str] = &[
        "#########",
        "#T.....,#",
        "#####.,.#",
        "#ww##...#",
        "#O.....s#",
        "#########",
    ];

    #[test]
    fn human_pad_strokes_and_scorecard() {
        let cfg = GolfConfig::from_course(&course(&[STRAIGHT]), &[], 1).unwrap();
        let mut s = GolfSession::new(cfg);
        let cross = PadButton::Cross.mask();
        let mut holed = false;
        for frame in 0..40_000 {
            // Tap Cross every 12 frames: aim -> power -> strike at ~22%.
            let ev = s.tick(&[if frame % 12 == 0 { cross } else { 0 }]);
            holed |= ev
                .iter()
                .any(|e| matches!(e, GolfEvent::Holed { .. } | GolfEvent::PickedUp { .. }));
            if s.is_finished() {
                break;
            }
        }
        assert!(s.is_finished());
        assert!(holed);
        let strokes = s.totals()[0];
        assert!((1..=7).contains(&strokes));
        assert_eq!(s.results().unwrap().placements[0].score, strokes);
    }

    #[test]
    fn hazard_costs_a_stroke_and_replays_from_the_lie() {
        let cfg = GolfConfig::from_course(&course(&[&["T.w.O"]]), &[], 1).unwrap();
        let mut s = GolfSession::new(cfg);
        let tee = s.course().tee;
        s.set_aim(0, 0);
        s.strike(0, 40);
        let mut ev = Vec::new();
        while matches!(s.phase(), GolfPhase::Rolling { .. }) {
            ev.extend(s.tick(&[]));
        }
        assert!(ev.contains(&GolfEvent::Hazard { seat: 0 }));
        assert_eq!(s.view().balls[0].strokes, 2);
        assert_eq!(s.view().balls[0].pos, tee);
    }

    #[test]
    fn farthest_ball_plays_next_after_the_tee_shots() {
        let cfg = GolfConfig {
            seats: vec![SeatKind::Human; 2],
            ..GolfConfig::from_course(&course(&[STRAIGHT]), &[], 0).unwrap()
        };
        let mut s = GolfSession::new(cfg);
        assert_eq!(s.phase(), GolfPhase::Aim { seat: 0 });
        s.strike(0, 30);
        while matches!(s.phase(), GolfPhase::Rolling { .. }) {
            s.tick(&[]);
        }
        // Seat 1 still has to tee off.
        assert_eq!(s.phase(), GolfPhase::Aim { seat: 1 });
        s.strike(1, 10);
        while matches!(s.phase(), GolfPhase::Rolling { .. }) {
            s.tick(&[]);
        }
        // Seat 1 is shorter, so farther away: it plays again.
        assert_eq!(s.phase(), GolfPhase::Aim { seat: 1 });
    }

    #[test]
    fn ai_round_finishes_and_experts_score_better() {
        let c = course(&[STRAIGHT, DOGLEG]);
        let mut expert = 0;
        let mut novice = 0;
        for seed in 0..4 {
            let mut cfg =
                GolfConfig::from_course(&c, &[AiTier::Expert, AiTier::Novice], seed).unwrap();
            cfg.seats[0] = SeatKind::Ai(AiTier::Regular);
            cfg.ai_think_frames = 0;
            let mut s = GolfSession::new(cfg);
            let mut frames = 0;
            while !s.is_finished() {
                s.tick(&[]);
                frames += 1;
                assert!(frames < 200_000);
            }
            let t = s.totals();
            expert += t[1];
            novice += t[2];
            for row in &s.view().scorecard {
                assert!(row.iter().all(|h| h.is_some()));
            }
        }
        assert!(expert <= novice, "expert {expert} novice {novice}");
    }
}
