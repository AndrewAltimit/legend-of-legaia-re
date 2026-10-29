//! Checkpoint racing for 1-8 racers on a closed track.
//!
//! The track is a closed centerline polyline with a half-width
//! ([`Track`]); the drivable surface is every point within the half-width
//! of some segment. Each centerline point is a checkpoint. A racer passes
//! a checkpoint when the segment nearest to it becomes the one leaving the
//! checkpoint, and only the next checkpoint in order counts, so a short
//! cut that skips one gains nothing. Passing checkpoint `0` closes a lap.
//!
//! Driving is arcade: a heading and a scalar speed. Cross accelerates,
//! Square brakes, Left / Right steer, R1 spends a full boost meter. A boost
//! pad segment gives a short boost for free. Running into the track edge
//! pushes the racer back inside and slides it along the edge, losing the
//! speed it carried into the wall and an eighth of the rest; two racers that
//! overlap are pushed apart and the one behind loses a quarter of its
//! speed. A racer that has finished coasts to a stop and no longer
//! collides. Everything is 20.12 fixed point.
//!
//! The race starts after a countdown and ends when every racer has
//! finished, [`RaceConfig::finish_grace`] frames after the first
//! finisher, or at [`RaceConfig::time_limit`]; racers still running are
//! placed behind the finishers by progress.

use legaia_engine_system::pad::PadButton;
use serde::{Deserialize, Serialize};

use crate::fixed::{FX_ONE, Fx, Vec2, angle_delta, cos12, fx_mul, isqrt, sin12};
use crate::session::{
    AiTier, ModeResults, ModeRng, ModeSession, PadEdges, PadWord, Placement, SeatKind, StateHasher,
};

/// Top speed, world units per frame.
pub const MAX_SPEED: Fx = 6 * FX_ONE;
/// Top speed while boosting.
pub const BOOST_SPEED: Fx = 9 * FX_ONE;
/// Throttle acceleration per frame.
pub const ACCEL: Fx = FX_ONE / 10;
/// Brake deceleration per frame.
pub const BRAKE: Fx = FX_ONE / 4;
/// Coasting drag per frame.
pub const COAST_DRAG: Fx = FX_ONE / 40;
/// Heading change per frame of steering.
pub const TURN_RATE: u16 = 40;
/// Boost meter capacity; it fills one unit per frame.
pub const BOOST_METER: u32 = 300;
/// Frames a boost lasts (meter or pad).
pub const BOOST_FRAMES: u32 = 45;
/// Racer collision radius, world units.
pub const RACER_RADIUS: i32 = 10;
/// Countdown before the start.
pub const COUNTDOWN_FRAMES: u32 = 180;

/// A closed track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub points: Vec<Vec2>,
    pub half_width: Fx,
    pub boost_segments: Vec<usize>,
    pub laps: u8,
}

/// Why a track was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackError {
    TooFewPoints,
    DegenerateSegment(usize),
    NarrowTrack,
    /// The track names a map the pack does not carry.
    NoMap(String),
    /// A waypoint falls off the map.
    OffMap(usize),
}

fn closest_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let ap = p - a;
    let den = ab.len_sq();
    if den == 0 {
        return a;
    }
    let num = (ap.x as i64 * ab.x as i64 + ap.y as i64 * ab.y as i64).clamp(0, den);
    // `ab * num` reaches ~2^70 on a large track: widen.
    let (num, den) = (num as i128, den as i128);
    Vec2::new(
        a.x + (ab.x as i128 * num / den) as Fx,
        a.y + (ab.y as i128 * num / den) as Fx,
    )
}

impl Track {
    pub fn new(
        points: Vec<Vec2>,
        half_width: Fx,
        boost_segments: Vec<usize>,
        laps: u8,
    ) -> Result<Self, TrackError> {
        if points.len() < 3 {
            return Err(TrackError::TooFewPoints);
        }
        for i in 0..points.len() {
            if points[i] == points[(i + 1) % points.len()] {
                return Err(TrackError::DegenerateSegment(i));
            }
        }
        if half_width < 2 * RACER_RADIUS * FX_ONE {
            return Err(TrackError::NarrowTrack);
        }
        Ok(Self {
            points,
            half_width,
            boost_segments,
            laps: laps.max(1),
        })
    }

    /// Build from a pack track (world units).
    pub fn from_def(t: &legaia_expansion_format::modes::RaceTrack) -> Result<Self, TrackError> {
        Self::new(
            t.points
                .iter()
                .map(|p| Vec2::new(p[0] * FX_ONE, p[1] * FX_ONE))
                .collect(),
            (t.half_width as i32).saturating_mul(FX_ONE),
            t.boost_segments.iter().map(|&s| s as usize).collect(),
            t.laps,
        )
    }

    /// Build a pack track laid on its map: the centerline is the
    /// [`RaceTrack::waypoints`](legaia_expansion_format::modes::RaceTrack::waypoints)
    /// tile centres, in mode units of the returned frame (mode `(0, 0)` at
    /// the map's tile `[0, 0]` corner, [`crate::map_frame::RACE_UNIT`] world
    /// units a mode unit).
    pub fn from_map(
        t: &legaia_expansion_format::modes::RaceTrack,
        def: &legaia_expansion_format::map::MapDef,
        map_id: &str,
    ) -> Result<(Self, crate::map_frame::MapFrame), TrackError> {
        use crate::map_frame::{MAP_TILE, MapFrame, RACE_UNIT};
        let (w, h) = def
            .tile_grid()
            .map(|g| (g.width, g.height))
            .map_err(|_| TrackError::NoMap(map_id.to_string()))?;
        let mut points = Vec::with_capacity(t.waypoints.len());
        for (i, wp) in t.waypoints.iter().enumerate() {
            if usize::from(wp[0]) >= w || usize::from(wp[1]) >= h {
                return Err(TrackError::OffMap(i));
            }
            let centre = |c: u16| (i32::from(c) * MAP_TILE + MAP_TILE / 2) / RACE_UNIT * FX_ONE;
            points.push(Vec2::new(centre(wp[0]), centre(wp[1])));
        }
        let track = Self::new(
            points,
            (t.half_width as i32).saturating_mul(FX_ONE),
            t.boost_segments.iter().map(|&s| s as usize).collect(),
            t.laps,
        )?;
        let frame = MapFrame {
            map_id: map_id.to_string(),
            spawn: t.spawn.clone(),
            origin: [
                i32::from(def.tiles.origin[0]) * MAP_TILE,
                i32::from(def.tiles.origin[1]) * MAP_TILE,
            ],
            unit: RACE_UNIT,
        };
        Ok((track, frame))
    }

    fn seg(&self, i: usize) -> (Vec2, Vec2) {
        let n = self.points.len();
        (self.points[i % n], self.points[(i + 1) % n])
    }

    /// The nearest segment to `p` and the closest point on it.
    pub fn nearest(&self, p: Vec2) -> (usize, Vec2) {
        let mut best = (0, self.points[0], i64::MAX);
        for i in 0..self.points.len() {
            let (a, b) = self.seg(i);
            let c = closest_on_segment(p, a, b);
            let d = (p - c).len_sq();
            if d < best.2 {
                best = (i, c, d);
            }
        }
        (best.0, best.1)
    }

    /// Whether `p` has reached checkpoint `cp`: the nearest segment is the
    /// one leaving it. The switch from segment `cp - 1` to `cp` happens on
    /// the corner's bisector, so a racer that cuts a corner still passes,
    /// and one that skips a segment never does.
    fn reached(&self, p: Vec2, cp: usize) -> bool {
        self.nearest(p).0 == cp
    }
}

/// A race's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaceConfig {
    pub seats: Vec<SeatKind>,
    pub track: Track,
    /// Frames the race runs on after the first finisher.
    pub finish_grace: u32,
    /// Frames after the start at which the race ends regardless.
    pub time_limit: u32,
    pub seed: u64,
}

impl RaceConfig {
    /// Build from a pack track: seat 0 the player, the rest AI.
    pub fn from_def(
        t: &legaia_expansion_format::modes::RaceTrack,
        seed: u64,
    ) -> Result<Self, TrackError> {
        Ok(Self::with_track(t, Track::from_def(t)?, seed))
    }

    /// [`Self::from_def`] for a track laid on its pack map
    /// ([`Track::from_map`]), with the frame that places it on the field.
    pub fn from_map(
        t: &legaia_expansion_format::modes::RaceTrack,
        def: &legaia_expansion_format::map::MapDef,
        map_id: &str,
        seed: u64,
    ) -> Result<(Self, crate::map_frame::MapFrame), TrackError> {
        let (track, frame) = Track::from_map(t, def, map_id)?;
        Ok((Self::with_track(t, track, seed), frame))
    }

    fn with_track(t: &legaia_expansion_format::modes::RaceTrack, track: Track, seed: u64) -> Self {
        let n = (t.racers as usize).clamp(1, crate::session::MAX_SEATS);
        let mut seats = vec![SeatKind::Human];
        seats.extend(std::iter::repeat_n(SeatKind::Ai(t.ai_tier.into()), n - 1));
        Self {
            seats,
            track,
            finish_grace: 30 * 60,
            time_limit: t.laps.max(1) as u32 * 3 * 60 * 60,
            seed,
        }
    }
}

/// One racer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Racer {
    pub pos: Vec2,
    pub heading: u16,
    pub speed: Fx,
    pub boost_meter: u32,
    pub boost_frames: u32,
    pub lap: u8,
    pub next_cp: usize,
    /// Frame the racer finished on.
    pub finished: Option<u32>,
    /// The AI's lateral line offset in world units.
    pub line_offset: i32,
}

/// The race phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RacePhase {
    Countdown { frames: u32 },
    Racing,
    Finished,
}

/// What a tick reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaceEvent {
    Go,
    Checkpoint { seat: usize, cp: usize },
    Lap { seat: usize, lap: u8 },
    Finished { seat: usize, place: u8, frame: u32 },
    WallHit { seat: usize },
    Bump { a: usize, b: usize },
    Boost { seat: usize },
    RaceOver,
}

/// The race snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaceView {
    pub phase: RacePhase,
    pub frame: u32,
    pub racers: Vec<Racer>,
    /// Seats in current running order.
    pub standings: Vec<usize>,
}

/// A running race.
#[derive(Debug, Clone)]
pub struct RaceSession {
    cfg: RaceConfig,
    racers: Vec<Racer>,
    phase: RacePhase,
    frame: u32,
    finishers: Vec<usize>,
    first_finish: Option<u32>,
    rng: ModeRng,
    pads: PadEdges,
}

impl RaceSession {
    /// Hand `seat` to `kind` from the next tick on: a networked table
    /// seats its players as humans and gives a dropped player's seat to the
    /// AI. Out-of-range seats are ignored.
    pub fn set_seat_kind(&mut self, seat: usize, kind: SeatKind) {
        if let Some(k) = self.cfg.seats.get_mut(seat) {
            *k = kind;
        }
    }

    pub fn new(cfg: RaceConfig) -> Self {
        assert!(!cfg.seats.is_empty() && cfg.seats.len() <= crate::session::MAX_SEATS);
        let mut rng = ModeRng::new(cfg.seed);
        let t = &cfg.track;
        let (a, b) = t.seg(0);
        let dir = b - a;
        let heading = dir.angle();
        // A grid of two columns just past the start line.
        let lateral = (heading as i32 + 1024).rem_euclid(4096) as u16;
        let racers = (0..cfg.seats.len())
            .map(|i| {
                let row = (i / 2) as i32;
                let col = if i % 2 == 0 { -1 } else { 1 };
                let fwd = Vec2::from_angle(heading, 8 * FX_ONE - row * 3 * RACER_RADIUS * FX_ONE);
                let side = Vec2::from_angle(lateral, col * (t.half_width / 2));
                let max_off = (t.half_width / FX_ONE) / 2;
                Racer {
                    pos: a + fwd + side,
                    heading,
                    speed: 0,
                    boost_meter: 0,
                    boost_frames: 0,
                    lap: 0,
                    next_cp: 1,
                    finished: None,
                    line_offset: rng.range(-max_off, max_off),
                }
            })
            .collect();
        Self {
            cfg,
            racers,
            phase: RacePhase::Countdown {
                frames: COUNTDOWN_FRAMES,
            },
            frame: 0,
            finishers: Vec::new(),
            first_finish: None,
            rng,
            pads: PadEdges::default(),
        }
    }

    pub fn phase(&self) -> RacePhase {
        self.phase
    }

    pub fn racers(&self) -> &[Racer] {
        &self.racers
    }

    /// The track being raced.
    pub fn track(&self) -> &Track {
        &self.cfg.track
    }

    /// Progress key: laps, checkpoints, then closeness to the next one.
    fn progress(&self, seat: usize) -> (u8, usize, i64) {
        let r = &self.racers[seat];
        let n = self.cfg.track.points.len();
        let cps = if r.next_cp == 0 { n } else { r.next_cp };
        let to_next = (r.pos - self.cfg.track.points[r.next_cp]).len_sq();
        (r.lap, cps, -to_next)
    }

    /// Seats in running order.
    pub fn standings(&self) -> Vec<usize> {
        let mut s: Vec<usize> = (0..self.racers.len()).collect();
        s.sort_by(|&a, &b| {
            let fa = self.finishers.iter().position(|&x| x == a);
            let fb = self.finishers.iter().position(|&x| x == b);
            match (fa, fb) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => self.progress(b).cmp(&self.progress(a)).then(a.cmp(&b)),
            }
        });
        s
    }

    /// The AI's controls: (steer -1/0/1, throttle, brake, boost).
    fn ai_controls(&mut self, seat: usize, tier: AiTier) -> (i32, bool, bool, bool) {
        let t = &self.cfg.track;
        let r = self.racers[seat];
        let n = t.points.len();
        // Pure pursuit: a point a fixed distance ahead along the centerline
        // from the racer's projection onto it, shifted by the racer's line.
        let (mut seg, mut at) = t.nearest(r.pos);
        let mut left = t.half_width as i64 * 2;
        loop {
            let end = t.points[(seg + 1) % n];
            let rest = (end - at).length() as i64;
            if rest >= left {
                at = at + (end - at).scale(left as i32, rest.max(1) as i32);
                break;
            }
            left -= rest;
            at = end;
            seg = (seg + 1) % n;
        }
        let (a, b) = t.seg(seg);
        let lateral = ((b - a).angle() as i32 + 1024).rem_euclid(4096) as u16;
        let target = at + Vec2::from_angle(lateral, r.line_offset * FX_ONE);
        let want = (target - r.pos).angle();
        let d = angle_delta(r.heading, want);
        let steer = if d > TURN_RATE as i32 / 2 {
            1
        } else if d < -(TURN_RATE as i32) / 2 {
            -1
        } else {
            0
        };
        let (cap_pct, brake_turn, use_boost) = match tier {
            AiTier::Novice => (80, 900, false),
            AiTier::Regular => (92, 700, true),
            AiTier::Expert => (100, 600, true),
        };
        let cap = MAX_SPEED * cap_pct / 100;
        let sharp = d.abs() > brake_turn;
        // Even in a sharp turn, crawl rather than stall.
        let throttle = (!sharp && r.speed < cap) || r.speed < MAX_SPEED / 4;
        let brake = sharp && r.speed > MAX_SPEED / 2;
        let boost = use_boost && d.abs() < 120 && r.boost_meter >= BOOST_METER;
        // A little jitter so AI racers don't drive identical lines.
        let steer = if self.rng.chance(3) { 0 } else { steer };
        (steer, throttle, brake, boost)
    }

    fn drive(&mut self, seat: usize, ev: &mut Vec<RaceEvent>) {
        let (steer, throttle, brake, boost) = match self.cfg.seats[seat] {
            SeatKind::Ai(tier) => self.ai_controls(seat, tier),
            SeatKind::Human => {
                let p = &self.pads;
                let steer =
                    p.held(seat, PadButton::Right) as i32 - p.held(seat, PadButton::Left) as i32;
                (
                    steer,
                    p.held(seat, PadButton::Cross),
                    p.held(seat, PadButton::Square),
                    p.pressed(seat, PadButton::R1),
                )
            }
        };
        let t = &self.cfg.track;
        let r = &mut self.racers[seat];
        if r.finished.is_some() {
            // Coast to a stop past the line.
            r.speed = (r.speed - BRAKE).max(0);
        } else {
            r.heading = (r.heading as i32 + steer * TURN_RATE as i32).rem_euclid(4096) as u16;
            if boost && r.boost_meter >= BOOST_METER {
                r.boost_meter = 0;
                r.boost_frames = BOOST_FRAMES;
                ev.push(RaceEvent::Boost { seat });
            }
            let top = if r.boost_frames > 0 {
                BOOST_SPEED
            } else {
                MAX_SPEED
            };
            if r.boost_frames > 0 {
                r.speed = (r.speed + ACCEL * 2).min(top);
                r.boost_frames -= 1;
            } else if brake {
                r.speed = (r.speed - BRAKE).max(0);
            } else if throttle {
                r.speed = (r.speed + ACCEL).min(top);
            } else {
                r.speed = (r.speed - COAST_DRAG).max(0);
            }
            if r.speed > top {
                r.speed = (r.speed - ACCEL).max(top);
            }
            r.boost_meter = (r.boost_meter + 1).min(BOOST_METER);
        }
        r.pos = r.pos
            + Vec2::new(
                fx_mul(cos12(r.heading), r.speed),
                fx_mul(sin12(r.heading), r.speed),
            );
        // Keep inside the track.
        let (seg, c) = t.nearest(r.pos);
        let off = r.pos - c;
        let limit = t.half_width - RACER_RADIUS * FX_ONE;
        let dist = off.length();
        if dist > limit {
            r.pos = c + off.scale(limit, dist.max(1));
            // Slide: drop the velocity's outward component, turn the
            // heading along what is left and lose a little speed.
            let n = off.scale(FX_ONE, dist.max(1));
            let v = Vec2::from_angle(r.heading, r.speed);
            let out = fx_mul(v.x, n.x) + fx_mul(v.y, n.y);
            if out > 0 {
                let slid = v - Vec2::new(fx_mul(n.x, out), fx_mul(n.y, out));
                if slid.length() > 0 {
                    r.heading = slid.angle();
                }
                r.speed = slid.length() * 7 / 8;
            }
            ev.push(RaceEvent::WallHit { seat });
        }
        if t.boost_segments.contains(&seg) && r.boost_frames == 0 && r.finished.is_none() {
            r.boost_frames = BOOST_FRAMES / 2;
            ev.push(RaceEvent::Boost { seat });
        }
    }

    fn checkpoints(&mut self, seat: usize, ev: &mut Vec<RaceEvent>) {
        let n = self.cfg.track.points.len();
        let r = self.racers[seat];
        if r.finished.is_some() || !self.cfg.track.reached(r.pos, r.next_cp) {
            return;
        }
        let cp = r.next_cp;
        let r = &mut self.racers[seat];
        r.next_cp = (cp + 1) % n;
        ev.push(RaceEvent::Checkpoint { seat, cp });
        if cp == 0 {
            r.lap += 1;
            ev.push(RaceEvent::Lap { seat, lap: r.lap });
            if r.lap >= self.cfg.track.laps {
                r.finished = Some(self.frame);
                self.finishers.push(seat);
                self.first_finish.get_or_insert(self.frame);
                ev.push(RaceEvent::Finished {
                    seat,
                    place: self.finishers.len() as u8,
                    frame: self.frame,
                });
            }
        }
    }

    fn collide(&mut self, ev: &mut Vec<RaceEvent>) {
        let d = 2 * RACER_RADIUS * FX_ONE;
        for a in 0..self.racers.len() {
            for b in a + 1..self.racers.len() {
                // Finished racers park past the line as ghosts.
                if self.racers[a].finished.is_some() || self.racers[b].finished.is_some() {
                    continue;
                }
                let delta = self.racers[b].pos - self.racers[a].pos;
                let dist_sq = delta.len_sq();
                if dist_sq >= d as i64 * d as i64 {
                    continue;
                }
                let dist = (isqrt(dist_sq as u64) as Fx).max(1);
                let push = (d - dist) / 2 + 1;
                let n = if dist_sq == 0 {
                    Vec2::new(FX_ONE, 0)
                } else {
                    delta.scale(FX_ONE, dist)
                };
                let shove = Vec2::new(fx_mul(n.x, push), fx_mul(n.y, push));
                self.racers[a].pos = self.racers[a].pos - shove;
                self.racers[b].pos = self.racers[b].pos + shove;
                // The one behind (lower progress) loses speed.
                let behind = if self.progress(a) < self.progress(b) {
                    a
                } else {
                    b
                };
                self.racers[behind].speed = self.racers[behind].speed * 3 / 4;
                ev.push(RaceEvent::Bump { a, b });
            }
        }
    }
}

impl ModeSession for RaceSession {
    type Event = RaceEvent;
    type View = RaceView;

    fn seats(&self) -> usize {
        self.cfg.seats.len()
    }

    fn seat_kind(&self, seat: usize) -> SeatKind {
        self.cfg.seats[seat]
    }

    fn tick(&mut self, pads: &[PadWord]) -> Vec<RaceEvent> {
        self.pads.update(pads);
        let mut ev = Vec::new();
        match self.phase {
            RacePhase::Finished => return ev,
            RacePhase::Countdown { frames } => {
                if frames <= 1 {
                    self.phase = RacePhase::Racing;
                    ev.push(RaceEvent::Go);
                } else {
                    self.phase = RacePhase::Countdown { frames: frames - 1 };
                }
                return ev;
            }
            RacePhase::Racing => {}
        }
        self.frame += 1;
        for seat in 0..self.racers.len() {
            self.drive(seat, &mut ev);
        }
        self.collide(&mut ev);
        for seat in 0..self.racers.len() {
            self.checkpoints(seat, &mut ev);
        }
        let all_done = self.racers.iter().all(|r| r.finished.is_some());
        let grace_out = self
            .first_finish
            .is_some_and(|f| self.frame >= f + self.cfg.finish_grace);
        if all_done || grace_out || self.frame >= self.cfg.time_limit {
            self.phase = RacePhase::Finished;
            ev.push(RaceEvent::RaceOver);
        }
        ev
    }

    fn view(&self) -> RaceView {
        RaceView {
            phase: self.phase,
            frame: self.frame,
            racers: self.racers.clone(),
            standings: self.standings(),
        }
    }

    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.rng.hash_into(&mut h);
        self.pads.hash_into(&mut h);
        for r in &self.racers {
            h.i32(r.pos.x);
            h.i32(r.pos.y);
            h.u16(r.heading);
            h.i32(r.speed);
            h.u32(r.boost_meter);
            h.u32(r.boost_frames);
            h.u8(r.lap);
            h.usize(r.next_cp);
            h.u32(r.finished.unwrap_or(u32::MAX));
            h.i32(r.line_offset);
        }
        match self.phase {
            RacePhase::Countdown { frames } => {
                h.u8(0);
                h.u32(frames)
            }
            RacePhase::Racing => h.u8(1),
            RacePhase::Finished => h.u8(2),
        }
        h.u32(self.frame);
        for &f in &self.finishers {
            h.usize(f);
        }
        h.finish()
    }

    fn is_finished(&self) -> bool {
        self.phase == RacePhase::Finished
    }

    fn results(&self) -> Option<ModeResults> {
        if !self.is_finished() {
            return None;
        }
        let placements = self
            .standings()
            .into_iter()
            .enumerate()
            .map(|(i, seat)| Placement {
                seat,
                place: (i + 1) as u8,
                score: self.racers[seat].finished.map_or(i64::MAX, |f| f as i64),
            })
            .collect();
        Some(ModeResults {
            placements,
            rewards: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1200 x 800 rectangle, eight checkpoints.
    fn oval(laps: u8) -> Track {
        let pts = [
            (0, 0),
            (600, 0),
            (1200, 0),
            (1200, 400),
            (1200, 800),
            (600, 800),
            (0, 800),
            (0, 400),
        ];
        Track::new(
            pts.iter()
                .map(|&(x, y)| Vec2::new(x * FX_ONE, y * FX_ONE))
                .collect(),
            80 * FX_ONE,
            vec![],
            laps,
        )
        .unwrap()
    }

    fn race(seats: Vec<SeatKind>, laps: u8, seed: u64) -> RaceSession {
        RaceSession::new(RaceConfig {
            seats,
            track: oval(laps),
            finish_grace: 60 * 60,
            time_limit: 60 * 60 * 10,
            seed,
        })
    }

    #[test]
    fn track_validation() {
        let p = |v: &[(i32, i32)]| v.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>();
        assert_eq!(
            Track::new(p(&[(0, 0), (1, 1)]), 100 * FX_ONE, vec![], 1),
            Err(TrackError::TooFewPoints)
        );
        assert_eq!(
            Track::new(p(&[(0, 0), (0, 0), (5, 5)]), 100 * FX_ONE, vec![], 1),
            Err(TrackError::DegenerateSegment(0))
        );
        assert_eq!(
            Track::new(p(&[(0, 0), (9, 0), (5, 5)]), FX_ONE, vec![], 1),
            Err(TrackError::NarrowTrack)
        );
    }

    #[test]
    fn countdown_holds_racers_then_go() {
        let mut s = race(vec![SeatKind::Human], 1, 0);
        let start = s.racers()[0].pos;
        let cross = PadButton::Cross.mask();
        for _ in 0..COUNTDOWN_FRAMES - 1 {
            assert!(!s.tick(&[cross]).contains(&RaceEvent::Go));
        }
        assert_eq!(s.racers()[0].pos, start);
        assert!(s.tick(&[cross]).contains(&RaceEvent::Go));
        for _ in 0..30 {
            s.tick(&[cross]);
        }
        assert!(s.racers()[0].pos.x > start.x);
    }

    #[test]
    fn walls_keep_a_racer_on_the_track() {
        let mut s = race(vec![SeatKind::Human], 1, 0);
        // Hold throttle and steer right forever: the racer scrapes the edge
        // but never leaves the track.
        let pad = PadButton::Cross.mask() | PadButton::Right.mask();
        let mut hits = 0;
        for _ in 0..2000 {
            let ev = s.tick(&[pad]);
            hits += ev
                .iter()
                .filter(|e| matches!(e, RaceEvent::WallHit { .. }))
                .count();
            let p = s.racers()[0].pos;
            let (_, c) = s.cfg.track.nearest(p);
            assert!((p - c).length() <= s.cfg.track.half_width);
        }
        assert!(hits > 0);
    }

    #[test]
    fn checkpoints_count_in_order_only() {
        let t = oval(1);
        // Checkpoint 2 at (1200, 0) arriving from the left.
        assert!(t.reached(Vec2::new(1201 * FX_ONE, 5 * FX_ONE), 2));
        assert!(!t.reached(Vec2::new(1100 * FX_ONE, 0), 2));
        let mut s = race(vec![SeatKind::Human], 1, 0);
        // Teleporting onto checkpoint 3 does not count while 1 is due.
        s.racers[0].pos = Vec2::new(1200 * FX_ONE, 401 * FX_ONE);
        let mut ev = Vec::new();
        s.checkpoints(0, &mut ev);
        assert!(ev.is_empty());
        assert_eq!(s.racers[0].next_cp, 1);
    }

    #[test]
    fn ai_field_finishes_with_placements() {
        let seats = vec![
            SeatKind::Ai(AiTier::Expert),
            SeatKind::Ai(AiTier::Novice),
            SeatKind::Ai(AiTier::Regular),
            SeatKind::Ai(AiTier::Regular),
        ];
        let mut expert_ahead = 0;
        for seed in 0..6 {
            let mut s = race(seats.clone(), 2, seed);
            let mut laps = 0;
            let mut frames = 0;
            while !s.is_finished() {
                let ev = s.tick(&[]);
                laps += ev
                    .iter()
                    .filter(|e| matches!(e, RaceEvent::Lap { .. }))
                    .count();
                frames += 1;
                assert!(frames < 60 * 60 * 10, "race never ended");
            }
            for r in s.racers() {
                assert!(r.finished.is_some(), "seed {seed} {r:?} frame {}", s.frame);
            }
            assert_eq!(laps, 8);
            let r = s.results().unwrap();
            let places: Vec<u8> = r.placements.iter().map(|p| p.place).collect();
            assert_eq!(places, vec![1, 2, 3, 4]);
            let pos = |seat| r.placements.iter().position(|p| p.seat == seat).unwrap();
            if pos(0) < pos(1) {
                expert_ahead += 1;
            }
        }
        assert!(expert_ahead >= 4, "expert ahead in {expert_ahead}/6");
    }

    #[test]
    fn boost_spends_the_meter() {
        let mut s = race(vec![SeatKind::Human], 1, 0);
        for _ in 0..COUNTDOWN_FRAMES {
            s.tick(&[]);
        }
        s.racers[0].boost_meter = BOOST_METER;
        let ev = s.tick(&[PadButton::Cross.mask() | PadButton::R1.mask()]);
        assert!(ev.contains(&RaceEvent::Boost { seat: 0 }));
        assert_eq!(s.racers()[0].boost_meter, 1);
        for _ in 0..30 {
            s.tick(&[PadButton::Cross.mask()]);
        }
        assert!(s.racers()[0].speed > MAX_SPEED);
    }
}
