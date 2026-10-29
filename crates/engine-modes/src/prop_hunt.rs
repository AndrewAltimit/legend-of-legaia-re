//! Prop hunt: hiders disguise as props, hunters find them.
//!
//! A round runs in three phases:
//!
//! 1. **Choose** - each hider picks a prop from the arena's allowed list
//!    (Left / Right, Cross confirms; the cursor's prop is taken when the
//!    timer runs out).
//! 2. **Hide** - hiders move into position; hunters are held at their
//!    spawn until the hide timer ends.
//! 3. **Hunt** - hunters move and inspect (Cross): the nearest prop within
//!    reach is checked. A disguised hider is found (out for the round, the
//!    hunter scores [`FIND_POINTS`]); a decoy costs the hunter
//!    `miss_penalty` health, and a hunter at zero health is out. Nothing in
//!    reach is a free whiff. Hiders may taunt (Triangle) for
//!    [`TAUNT_POINTS`], which tells every host where they are.
//!
//! The round ends when every hider is found (hunters win), every hunter
//! is out, or the timer expires (hiders win). Surviving hiders score a
//! point per second of hunt plus [`SURVIVE_POINTS`]; winning hunters score
//! [`HUNTER_WIN_POINTS`]. Roles rotate so every seat hunts in turn; after
//! the last round the highest total wins.
//!
//! The kernel is geometry-agnostic. It moves avatars in a 2D [`Arena`] of
//! axis-aligned blockers, which is enough to play and test the rules; a
//! host with real scene collision instead feeds each avatar's position
//! through [`PropHuntSession::tick_with_positions`], which makes the
//! positions part of the lockstep input. Prop ids are the pack's strings;
//! a host resolves them to disc meshes.

use legaia_engine_system::pad::PadButton;
use serde::{Deserialize, Serialize};

use crate::fixed::{FX_ONE, Fx, Vec2};
use crate::session::{
    AiTier, ModeResults, ModeRng, ModeSession, PadEdges, PadWord, SeatKind, StateHasher,
};

/// Hider walk speed, world units per frame.
pub const HIDER_SPEED: Fx = 2 * FX_ONE;
/// Hunter walk speed.
pub const HUNTER_SPEED: Fx = 5 * FX_ONE / 2;
/// Inspect reach beyond the prop's own radius, world units.
pub const INSPECT_REACH: i32 = 24;
/// Frames between inspects.
pub const INSPECT_COOLDOWN: u32 = 20;
/// Frames between taunts.
pub const TAUNT_COOLDOWN: u32 = 5 * 60;
/// Frames the choose phase lasts.
pub const CHOOSE_FRAMES: u32 = 10 * 60;
/// Frames the end-of-round banner stays up.
pub const ROUND_OVER_FRAMES: u32 = 3 * 60;
pub const FIND_POINTS: i64 = 100;
pub const TAUNT_POINTS: i64 = 5;
pub const SURVIVE_POINTS: i64 = 50;
pub const HUNTER_WIN_POINTS: i64 = 50;
/// A hider counts as fidgeting this many frames after it last moved.
pub const MOVED_RECENTLY: u32 = 2 * 60;

/// An axis-aligned rectangle `[x0, y0, x1, y1]` in [`Fx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x0: Fx,
    pub y0: Fx,
    pub x1: Fx,
    pub y1: Fx,
}

impl Rect {
    pub fn from_units(r: [i32; 4]) -> Self {
        Self {
            x0: r[0].min(r[2]) * FX_ONE,
            y0: r[1].min(r[3]) * FX_ONE,
            x1: r[0].max(r[2]) * FX_ONE,
            y1: r[1].max(r[3]) * FX_ONE,
        }
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1
    }
}

/// The minimal arena: bounds and solid blockers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arena {
    pub bounds: Rect,
    pub blockers: Vec<Rect>,
}

impl Arena {
    /// Whether a point is walkable.
    pub fn open(&self, p: Vec2) -> bool {
        self.bounds.contains(p) && !self.blockers.iter().any(|b| b.contains(p))
    }

    /// Move `from` by `d`, one axis at a time, refusing steps into walls.
    pub fn slide(&self, from: Vec2, d: Vec2) -> Vec2 {
        let mut p = from;
        let nx = Vec2::new(p.x + d.x, p.y);
        if self.open(nx) {
            p = nx;
        }
        let ny = Vec2::new(p.x, p.y + d.y);
        if self.open(ny) {
            p = ny;
        }
        p
    }

    /// A random walkable point (rejection sampled; the centre if the arena
    /// is nearly solid).
    pub fn random_point(&self, rng: &mut ModeRng) -> Vec2 {
        let b = self.bounds;
        for _ in 0..64 {
            let p = Vec2::new(rng.range(b.x0, b.x1 - 1), rng.range(b.y0, b.y1 - 1));
            if self.open(p) {
                return p;
            }
        }
        self.centre()
    }

    pub fn centre(&self) -> Vec2 {
        Vec2::new(
            self.bounds.x0 / 2 + self.bounds.x1 / 2,
            self.bounds.y0 / 2 + self.bounds.y1 / 2,
        )
    }
}

/// A prop kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropKind {
    pub id: String,
    pub radius: i32,
}

/// A decoy in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Decoy {
    pub prop: usize,
    pub pos: Vec2,
}

/// A match's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropHuntConfig {
    pub seats: Vec<SeatKind>,
    pub hunters: usize,
    pub arena: Arena,
    pub props: Vec<PropKind>,
    pub decoys: Vec<Decoy>,
    pub rounds: u8,
    pub hide_frames: u32,
    pub round_frames: u32,
    pub hunter_health: u32,
    pub miss_penalty: u32,
    pub seed: u64,
}

/// Why a pack arena was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    NoProps,
    UnknownDecoyProp(String),
    BadSeatCount,
}

impl PropHuntConfig {
    /// Build from a pack arena: seat 0 the player, the rest AI.
    pub fn from_arena(
        a: &legaia_expansion_format::modes::PropHuntArena,
        seed: u64,
    ) -> Result<Self, ArenaError> {
        if a.props.is_empty() {
            return Err(ArenaError::NoProps);
        }
        let players = a.players as usize;
        let hunters = a.hunters as usize;
        if !(2..=crate::session::MAX_SEATS).contains(&players) || hunters == 0 || hunters >= players
        {
            return Err(ArenaError::BadSeatCount);
        }
        let props: Vec<PropKind> = a
            .props
            .iter()
            .map(|p| PropKind {
                id: p.id.clone(),
                radius: p.radius.max(1) as i32,
            })
            .collect();
        let decoys = a
            .decoys
            .iter()
            .map(|d| {
                let prop = props
                    .iter()
                    .position(|p| p.id == d.prop)
                    .ok_or_else(|| ArenaError::UnknownDecoyProp(d.prop.clone()))?;
                Ok(Decoy {
                    prop,
                    pos: Vec2::new(d.x * FX_ONE, d.y * FX_ONE),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut seats = vec![SeatKind::Human];
        seats.extend(std::iter::repeat_n(
            SeatKind::Ai(a.ai_tier.into()),
            players - 1,
        ));
        Ok(Self {
            seats,
            hunters,
            arena: Arena {
                bounds: Rect::from_units(a.bounds),
                blockers: a.blockers.iter().map(|&b| Rect::from_units(b)).collect(),
            },
            props,
            decoys,
            rounds: a.rounds.max(1),
            hide_frames: a.hide_frames,
            round_frames: a.round_frames.max(1),
            hunter_health: a.hunter_health.max(1),
            miss_penalty: a.miss_penalty,
            seed,
        })
    }
}

/// A seat's role this round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Hider,
    Hunter,
}

/// One seat's avatar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Avatar {
    pub role: Role,
    pub pos: Vec2,
    /// The hider's chosen prop (or the cursor during Choose).
    pub prop: usize,
    pub confirmed: bool,
    /// A found hider or a hunter out of health.
    pub out: bool,
    pub health: u32,
    pub cooldown: u32,
    pub taunt_cooldown: u32,
    /// Frame of the hunt the hider last moved.
    pub last_moved: Option<u32>,
    pub score: i64,
    /// AI waypoint.
    goal: Option<Vec2>,
}

/// The round phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HuntPhase {
    Choose { timer: u32 },
    Hide { timer: u32 },
    Hunt { timer: u32 },
    RoundOver { timer: u32, hunters_won: bool },
    Finished,
}

/// What an inspect touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Decoy(usize),
    Hider(usize),
}

/// What a tick reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HuntEvent {
    RoundStarted {
        round: u8,
        hunters: Vec<usize>,
    },
    PropChosen {
        seat: usize,
        prop: String,
    },
    HuntersReleased,
    Found {
        hunter: usize,
        hider: usize,
    },
    Missed {
        hunter: usize,
        decoy: usize,
        health: u32,
    },
    HunterOut {
        seat: usize,
    },
    Taunt {
        seat: usize,
        pos: Vec2,
    },
    RoundOver {
        hunters_won: bool,
    },
    Finished,
}

/// The match snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HuntView {
    pub phase: HuntPhase,
    pub round: u8,
    pub avatars: Vec<Avatar>,
    pub decoys: Vec<Decoy>,
    pub prop_ids: Vec<String>,
}

/// A running match.
#[derive(Debug, Clone)]
pub struct PropHuntSession {
    cfg: PropHuntConfig,
    avatars: Vec<Avatar>,
    phase: HuntPhase,
    round: u8,
    hunt_frame: u32,
    /// Per hunter: targets already inspected this round (AI memory).
    checked: Vec<Vec<Target>>,
    rng: ModeRng,
    pads: PadEdges,
}

impl PropHuntSession {
    pub fn new(cfg: PropHuntConfig) -> Self {
        let n = cfg.seats.len();
        assert!((2..=crate::session::MAX_SEATS).contains(&n));
        assert!(cfg.hunters >= 1 && cfg.hunters < n && !cfg.props.is_empty());
        let mut s = Self {
            avatars: Vec::new(),
            phase: HuntPhase::Finished,
            round: 0,
            hunt_frame: 0,
            checked: vec![Vec::new(); n],
            rng: ModeRng::new(cfg.seed),
            pads: PadEdges::default(),
            cfg,
        };
        let mut ev = Vec::new();
        s.start_round(&mut ev);
        s
    }

    pub fn phase(&self) -> HuntPhase {
        self.phase
    }

    pub fn avatars(&self) -> &[Avatar] {
        &self.avatars
    }

    fn hunters_for(&self, round: u8) -> Vec<usize> {
        let n = self.cfg.seats.len();
        (0..self.cfg.hunters)
            .map(|k| (round as usize * self.cfg.hunters + k) % n)
            .collect()
    }

    fn start_round(&mut self, ev: &mut Vec<HuntEvent>) {
        let hunters = self.hunters_for(self.round);
        let centre = self.cfg.arena.centre();
        let scores: Vec<i64> = self.avatars.iter().map(|a| a.score).collect();
        let n = self.cfg.seats.len();
        self.avatars = (0..n)
            .map(|seat| {
                let hunter = hunters.contains(&seat);
                let pos = if hunter {
                    centre
                } else {
                    self.cfg.arena.random_point(&mut self.rng)
                };
                Avatar {
                    role: if hunter { Role::Hunter } else { Role::Hider },
                    pos,
                    prop: 0,
                    confirmed: hunter,
                    out: false,
                    health: self.cfg.hunter_health,
                    cooldown: 0,
                    taunt_cooldown: 0,
                    last_moved: None,
                    score: scores.get(seat).copied().unwrap_or(0),
                    goal: None,
                }
            })
            .collect();
        self.checked = vec![Vec::new(); n];
        self.hunt_frame = 0;
        self.phase = HuntPhase::Choose {
            timer: CHOOSE_FRAMES,
        };
        ev.push(HuntEvent::RoundStarted {
            round: self.round,
            hunters,
        });
    }

    fn radius_of(&self, t: Target) -> i32 {
        let prop = match t {
            Target::Decoy(i) => self.cfg.decoys[i].prop,
            Target::Hider(s) => self.avatars[s].prop,
        };
        self.cfg.props[prop].radius
    }

    fn pos_of(&self, t: Target) -> Vec2 {
        match t {
            Target::Decoy(i) => self.cfg.decoys[i].pos,
            Target::Hider(s) => self.avatars[s].pos,
        }
    }

    /// Every prop a hunter could inspect: decoys and disguised hiders.
    fn targets(&self) -> Vec<Target> {
        let mut out: Vec<Target> = (0..self.cfg.decoys.len()).map(Target::Decoy).collect();
        out.extend(
            (0..self.avatars.len())
                .filter(|&s| self.avatars[s].role == Role::Hider && !self.avatars[s].out)
                .map(Target::Hider),
        );
        out
    }

    fn in_reach(&self, hunter: usize, t: Target) -> Option<i64> {
        let d = (self.pos_of(t) - self.avatars[hunter].pos).len_sq();
        let r = ((self.radius_of(t) + INSPECT_REACH) * FX_ONE) as i64;
        (d <= r * r).then_some(d)
    }

    /// Inspect for `hunter` (tests and alternate hosts; Cross ends here).
    pub fn inspect(&mut self, hunter: usize) -> Vec<HuntEvent> {
        let a = self.avatars[hunter];
        if a.role != Role::Hunter || a.out || a.cooldown > 0 {
            return Vec::new();
        }
        if !matches!(self.phase, HuntPhase::Hunt { .. }) {
            return Vec::new();
        }
        let Some(t) = self
            .targets()
            .into_iter()
            .filter_map(|t| self.in_reach(hunter, t).map(|d| (d, t)))
            .min_by_key(|&(d, t)| (d, matches!(t, Target::Decoy(_))))
            .map(|(_, t)| t)
        else {
            return Vec::new();
        };
        self.avatars[hunter].cooldown = INSPECT_COOLDOWN;
        self.checked[hunter].push(t);
        let mut ev = Vec::new();
        match t {
            Target::Hider(h) => {
                self.avatars[h].out = true;
                self.avatars[hunter].score += FIND_POINTS;
                ev.push(HuntEvent::Found { hunter, hider: h });
            }
            Target::Decoy(d) => {
                let a = &mut self.avatars[hunter];
                a.health = a.health.saturating_sub(self.cfg.miss_penalty);
                ev.push(HuntEvent::Missed {
                    hunter,
                    decoy: d,
                    health: a.health,
                });
                if a.health == 0 {
                    a.out = true;
                    ev.push(HuntEvent::HunterOut { seat: hunter });
                }
            }
        }
        ev
    }

    fn walk(&mut self, seat: usize, dir: (i32, i32)) {
        if dir == (0, 0) {
            return;
        }
        let a = self.avatars[seat];
        let speed = match a.role {
            Role::Hider => HIDER_SPEED,
            Role::Hunter => HUNTER_SPEED,
        };
        // Diagonals at ~0.7 so 8-way speed is even.
        let s = if dir.0 != 0 && dir.1 != 0 {
            speed * 181 / 256
        } else {
            speed
        };
        let to = self.cfg.arena.slide(a.pos, Vec2::new(dir.0 * s, dir.1 * s));
        if to != a.pos {
            self.avatars[seat].pos = to;
            if a.role == Role::Hider && matches!(self.phase, HuntPhase::Hunt { .. }) {
                self.avatars[seat].last_moved = Some(self.hunt_frame);
            }
        }
    }

    fn pad_dir(&self, seat: usize) -> (i32, i32) {
        let p = &self.pads;
        (
            p.held(seat, PadButton::Right) as i32 - p.held(seat, PadButton::Left) as i32,
            p.held(seat, PadButton::Down) as i32 - p.held(seat, PadButton::Up) as i32,
        )
    }

    fn toward(&self, seat: usize, goal: Vec2) -> (i32, i32) {
        let d = goal - self.avatars[seat].pos;
        let step = FX_ONE * 2;
        (
            if d.x > step {
                1
            } else if d.x < -step {
                -1
            } else {
                0
            },
            if d.y > step {
                1
            } else if d.y < -step {
                -1
            } else {
                0
            },
        )
    }

    /// One AI seat's frame: (walk direction, inspect, taunt).
    fn ai(&mut self, seat: usize, tier: AiTier) -> ((i32, i32), bool, bool) {
        let a = self.avatars[seat];
        match (a.role, self.phase) {
            (Role::Hider, HuntPhase::Hide { .. }) => {
                let goal = match a.goal {
                    Some(g) => g,
                    None => {
                        // Stand beside a decoy of the chosen prop when there is
                        // one, anywhere open otherwise.
                        let same: Vec<Vec2> = self
                            .cfg
                            .decoys
                            .iter()
                            .filter(|d| d.prop == a.prop)
                            .map(|d| d.pos)
                            .collect();
                        let g = if same.is_empty() || tier == AiTier::Novice {
                            self.cfg.arena.random_point(&mut self.rng)
                        } else {
                            let at = same[self.rng.below(same.len() as u32) as usize];
                            let r = self.cfg.props[a.prop].radius * 3 * FX_ONE;
                            let off = Vec2::new(self.rng.range(-r, r), self.rng.range(-r, r));
                            if self.cfg.arena.open(at + off) {
                                at + off
                            } else {
                                at
                            }
                        };
                        self.avatars[seat].goal = Some(g);
                        g
                    }
                };
                (self.toward(seat, goal), false, false)
            }
            (Role::Hider, HuntPhase::Hunt { .. }) => {
                // Novices fidget and taunt; experts freeze.
                let fidget = match tier {
                    AiTier::Novice => 2,
                    AiTier::Regular => 0,
                    AiTier::Expert => 0,
                };
                let dir = if self.rng.chance(fidget) {
                    (self.rng.range(-1, 1), self.rng.range(-1, 1))
                } else {
                    (0, 0)
                };
                let taunt = tier != AiTier::Expert && a.taunt_cooldown == 0 && self.rng.chance(1);
                (dir, false, taunt)
            }
            (Role::Hunter, HuntPhase::Hunt { .. }) if !a.out => {
                let unchecked: Vec<Target> = self
                    .targets()
                    .into_iter()
                    .filter(|t| !self.checked[seat].contains(t))
                    .collect();
                if unchecked.is_empty() {
                    return ((0, 0), false, false);
                }
                // Prefer what is near; a regular or expert hunter also
                // prefers props it saw move.
                let frame = self.hunt_frame;
                let pick = unchecked
                    .iter()
                    .copied()
                    .min_by_key(|&t| {
                        let d = (self.pos_of(t) - a.pos).len_sq();
                        let moved = match t {
                            Target::Hider(h) => self.avatars[h]
                                .last_moved
                                .is_some_and(|m| frame.saturating_sub(m) < MOVED_RECENTLY),
                            Target::Decoy(_) => false,
                        };
                        let bias = if moved && tier != AiTier::Novice {
                            4
                        } else {
                            1
                        };
                        d / bias
                    })
                    .unwrap();
                let inspect = self.in_reach(seat, pick).is_some() && a.cooldown == 0;
                let dir = if inspect {
                    (0, 0)
                } else {
                    self.toward(seat, self.pos_of(pick))
                };
                (dir, inspect, false)
            }
            _ => ((0, 0), false, false),
        }
    }

    /// [`ModeSession::tick`] with host-authored positions: `positions[i]`
    /// replaces seat `i`'s avatar position (after its pad moved it) when
    /// `Some`. A host with real collision drives movement itself and
    /// feeds the result here; in lockstep the positions travel with the
    /// pad words.
    pub fn tick_with_positions(
        &mut self,
        pads: &[PadWord],
        positions: &[Option<Vec2>],
    ) -> Vec<HuntEvent> {
        self.step(pads, positions)
    }

    fn step(&mut self, pads: &[PadWord], positions: &[Option<Vec2>]) -> Vec<HuntEvent> {
        self.pads.update(pads);
        let mut ev = Vec::new();
        let n = self.avatars.len();
        match self.phase {
            HuntPhase::Finished => return ev,
            HuntPhase::RoundOver { timer, hunters_won } => {
                if timer > 0 {
                    self.phase = HuntPhase::RoundOver {
                        timer: timer - 1,
                        hunters_won,
                    };
                } else if self.round + 1 < self.cfg.rounds {
                    self.round += 1;
                    self.start_round(&mut ev);
                } else {
                    self.phase = HuntPhase::Finished;
                    ev.push(HuntEvent::Finished);
                }
                return ev;
            }
            HuntPhase::Choose { timer } => {
                for seat in 0..n {
                    let a = self.avatars[seat];
                    if a.role != Role::Hider || a.confirmed {
                        continue;
                    }
                    let count = self.cfg.props.len();
                    let confirm = match self.cfg.seats[seat] {
                        SeatKind::Ai(tier) => {
                            // Experts pick the prop with the most decoys.
                            let p = if tier == AiTier::Expert {
                                (0..count)
                                    .max_by_key(|&p| {
                                        self.cfg.decoys.iter().filter(|d| d.prop == p).count()
                                    })
                                    .unwrap()
                            } else {
                                self.rng.below(count as u32) as usize
                            };
                            self.avatars[seat].prop = p;
                            true
                        }
                        SeatKind::Human => {
                            let av = &mut self.avatars[seat];
                            if self.pads.pressed(seat, PadButton::Left) {
                                av.prop = (av.prop + count - 1) % count;
                            }
                            if self.pads.pressed(seat, PadButton::Right) {
                                av.prop = (av.prop + 1) % count;
                            }
                            self.pads.pressed(seat, PadButton::Cross) || timer <= 1
                        }
                    };
                    if confirm {
                        self.avatars[seat].confirmed = true;
                        ev.push(HuntEvent::PropChosen {
                            seat,
                            prop: self.cfg.props[self.avatars[seat].prop].id.clone(),
                        });
                    }
                }
                let all = self.avatars.iter().all(|a| a.confirmed);
                self.phase = if all || timer <= 1 {
                    HuntPhase::Hide {
                        timer: self.cfg.hide_frames,
                    }
                } else {
                    HuntPhase::Choose { timer: timer - 1 }
                };
                return ev;
            }
            HuntPhase::Hide { .. } | HuntPhase::Hunt { .. } => {}
        }
        let hunting = matches!(self.phase, HuntPhase::Hunt { .. });
        for seat in 0..n {
            let a = self.avatars[seat];
            if a.out || (a.role == Role::Hunter && !hunting) {
                continue;
            }
            let (dir, inspect, taunt) = match self.cfg.seats[seat] {
                SeatKind::Ai(tier) => self.ai(seat, tier),
                SeatKind::Human => (
                    self.pad_dir(seat),
                    self.pads.pressed(seat, PadButton::Cross),
                    self.pads.pressed(seat, PadButton::Triangle),
                ),
            };
            self.walk(seat, dir);
            if let Some(Some(p)) = positions.get(seat) {
                if *p != self.avatars[seat].pos && a.role == Role::Hider && hunting {
                    self.avatars[seat].last_moved = Some(self.hunt_frame);
                }
                self.avatars[seat].pos = *p;
            }
            let av = &mut self.avatars[seat];
            av.cooldown = av.cooldown.saturating_sub(1);
            av.taunt_cooldown = av.taunt_cooldown.saturating_sub(1);
            if inspect {
                ev.extend(self.inspect(seat));
            }
            let av = &mut self.avatars[seat];
            if taunt && hunting && av.role == Role::Hider && av.taunt_cooldown == 0 {
                av.taunt_cooldown = TAUNT_COOLDOWN;
                av.score += TAUNT_POINTS;
                ev.push(HuntEvent::Taunt { seat, pos: av.pos });
            }
        }
        match self.phase {
            HuntPhase::Hide { timer } => {
                self.phase = if timer <= 1 {
                    ev.push(HuntEvent::HuntersReleased);
                    HuntPhase::Hunt {
                        timer: self.cfg.round_frames,
                    }
                } else {
                    HuntPhase::Hide { timer: timer - 1 }
                };
            }
            HuntPhase::Hunt { timer } => {
                self.hunt_frame += 1;
                if self.hunt_frame.is_multiple_of(60) {
                    for a in &mut self.avatars {
                        if a.role == Role::Hider && !a.out {
                            a.score += 1;
                        }
                    }
                }
                let hiders_left = self.avatars.iter().any(|a| a.role == Role::Hider && !a.out);
                let hunters_left = self
                    .avatars
                    .iter()
                    .any(|a| a.role == Role::Hunter && !a.out);
                let over = !hiders_left || !hunters_left || timer <= 1;
                if over {
                    let hunters_won = !hiders_left;
                    for a in &mut self.avatars {
                        match a.role {
                            Role::Hider if !a.out => a.score += SURVIVE_POINTS,
                            Role::Hunter if hunters_won && !a.out => a.score += HUNTER_WIN_POINTS,
                            _ => {}
                        }
                    }
                    ev.push(HuntEvent::RoundOver { hunters_won });
                    self.phase = HuntPhase::RoundOver {
                        timer: ROUND_OVER_FRAMES,
                        hunters_won,
                    };
                } else {
                    self.phase = HuntPhase::Hunt { timer: timer - 1 };
                }
            }
            _ => {}
        }
        ev
    }
}

impl ModeSession for PropHuntSession {
    type Event = HuntEvent;
    type View = HuntView;

    fn seats(&self) -> usize {
        self.cfg.seats.len()
    }

    fn seat_kind(&self, seat: usize) -> SeatKind {
        self.cfg.seats[seat]
    }

    fn tick(&mut self, pads: &[PadWord]) -> Vec<HuntEvent> {
        self.step(pads, &[])
    }

    fn view(&self) -> HuntView {
        HuntView {
            phase: self.phase,
            round: self.round,
            avatars: self.avatars.clone(),
            decoys: self.cfg.decoys.clone(),
            prop_ids: self.cfg.props.iter().map(|p| p.id.clone()).collect(),
        }
    }

    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.rng.hash_into(&mut h);
        self.pads.hash_into(&mut h);
        for a in &self.avatars {
            h.u8(a.role as u8);
            h.i32(a.pos.x);
            h.i32(a.pos.y);
            h.usize(a.prop);
            h.bool(a.confirmed);
            h.bool(a.out);
            h.u32(a.health);
            h.u32(a.cooldown);
            h.u32(a.taunt_cooldown);
            h.u32(a.last_moved.unwrap_or(u32::MAX));
            h.i64(a.score);
            if let Some(g) = a.goal {
                h.i32(g.x);
                h.i32(g.y);
            }
        }
        let (tag, t, w) = match self.phase {
            HuntPhase::Choose { timer } => (0, timer, false),
            HuntPhase::Hide { timer } => (1, timer, false),
            HuntPhase::Hunt { timer } => (2, timer, false),
            HuntPhase::RoundOver { timer, hunters_won } => (3, timer, hunters_won),
            HuntPhase::Finished => (4, 0, false),
        };
        h.u8(tag);
        h.u32(t);
        h.bool(w);
        h.u8(self.round);
        h.u32(self.hunt_frame);
        for c in &self.checked {
            h.usize(c.len());
            for t in c {
                match t {
                    Target::Decoy(i) => h.usize(*i),
                    Target::Hider(s) => h.usize(1000 + *s),
                }
            }
        }
        h.finish()
    }

    fn is_finished(&self) -> bool {
        self.phase == HuntPhase::Finished
    }

    fn results(&self) -> Option<ModeResults> {
        self.is_finished().then(|| {
            let scores: Vec<i64> = self.avatars.iter().map(|a| a.score).collect();
            ModeResults {
                placements: ModeResults::rank(&scores, false),
                rewards: Vec::new(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(seats: Vec<SeatKind>) -> PropHuntConfig {
        PropHuntConfig {
            seats,
            hunters: 1,
            arena: Arena {
                bounds: Rect::from_units([0, 0, 400, 400]),
                blockers: vec![Rect::from_units([180, 180, 220, 220])],
            },
            props: vec![
                PropKind {
                    id: "barrel".into(),
                    radius: 12,
                },
                PropKind {
                    id: "crate".into(),
                    radius: 16,
                },
            ],
            decoys: vec![
                Decoy {
                    prop: 0,
                    pos: Vec2::new(50 * FX_ONE, 50 * FX_ONE),
                },
                Decoy {
                    prop: 1,
                    pos: Vec2::new(350 * FX_ONE, 50 * FX_ONE),
                },
                Decoy {
                    prop: 0,
                    pos: Vec2::new(50 * FX_ONE, 350 * FX_ONE),
                },
            ],
            rounds: 2,
            hide_frames: 120,
            round_frames: 60 * 60,
            hunter_health: 30,
            miss_penalty: 10,
            seed: 3,
        }
    }

    fn to_hunt(s: &mut PropHuntSession) {
        while !matches!(s.phase(), HuntPhase::Hunt { .. }) {
            s.tick(&[PadButton::Cross.mask(); 8]);
        }
    }

    #[test]
    fn arena_slides_along_blockers() {
        let a = cfg(vec![SeatKind::Human; 2]).arena;
        let p = Vec2::new(170 * FX_ONE, 200 * FX_ONE);
        // Pushing right into the blocker while moving down: only y moves.
        let q = a.slide(p, Vec2::new(20 * FX_ONE, 5 * FX_ONE));
        assert_eq!(q, Vec2::new(170 * FX_ONE, 205 * FX_ONE));
        assert!(!a.open(Vec2::new(-1, 0)));
    }

    #[test]
    fn roles_rotate_and_hunters_wait_out_the_hide() {
        let mut s = PropHuntSession::new(cfg(vec![SeatKind::Human; 3]));
        assert_eq!(s.avatars()[0].role, Role::Hunter);
        let start = s.avatars()[0].pos;
        // Everyone confirms; then holding Right does not move the hunter
        // during the hide.
        s.tick(&[PadButton::Cross.mask(); 3]);
        assert!(matches!(s.phase(), HuntPhase::Hide { .. }));
        for _ in 0..50 {
            s.tick(&[PadButton::Right.mask(); 3]);
        }
        assert_eq!(s.avatars()[0].pos, start);
        assert!(s.avatars()[1].pos.x > 0);
        assert_eq!(s.hunters_for(1), vec![1]);
    }

    #[test]
    fn inspecting_a_hider_finds_it_and_decoys_cost_health() {
        let mut s = PropHuntSession::new(cfg(vec![SeatKind::Human; 3]));
        to_hunt(&mut s);
        // Put the hunter beside decoy 0 and inspect: a miss.
        s.avatars[0].pos = Vec2::new(60 * FX_ONE, 50 * FX_ONE);
        s.avatars[1].pos = Vec2::new(300 * FX_ONE, 300 * FX_ONE);
        s.avatars[2].pos = Vec2::new(300 * FX_ONE, 350 * FX_ONE);
        let ev = s.inspect(0);
        assert_eq!(
            ev,
            vec![HuntEvent::Missed {
                hunter: 0,
                decoy: 0,
                health: 20
            }]
        );
        // Cooldown blocks an immediate retry.
        assert!(s.inspect(0).is_empty());
        for _ in 0..INSPECT_COOLDOWN {
            s.tick(&[]);
        }
        s.avatars[0].pos = Vec2::new(300 * FX_ONE, 310 * FX_ONE);
        let ev = s.inspect(0);
        assert_eq!(
            ev,
            vec![HuntEvent::Found {
                hunter: 0,
                hider: 1
            }]
        );
        assert!(s.avatars()[1].out);
        assert_eq!(s.avatars()[0].score, FIND_POINTS);
        // Nothing in reach is a free whiff.
        for _ in 0..INSPECT_COOLDOWN {
            s.tick(&[]);
        }
        s.avatars[0].pos = Vec2::new(120 * FX_ONE, 120 * FX_ONE);
        assert!(s.inspect(0).is_empty());
        assert_eq!(s.avatars()[0].health, 20);
    }

    #[test]
    fn misses_knock_a_hunter_out_and_hiders_win() {
        let mut s = PropHuntSession::new(cfg(vec![SeatKind::Human; 2]));
        to_hunt(&mut s);
        s.avatars[1].pos = Vec2::new(300 * FX_ONE, 300 * FX_ONE);
        s.avatars[0].pos = Vec2::new(55 * FX_ONE, 50 * FX_ONE);
        let mut ev = Vec::new();
        for _ in 0..3 {
            ev.extend(s.inspect(0));
            for _ in 0..INSPECT_COOLDOWN {
                ev.extend(s.tick(&[]));
            }
        }
        assert!(ev.contains(&HuntEvent::HunterOut { seat: 0 }));
        assert!(ev.contains(&HuntEvent::RoundOver { hunters_won: false }));
        assert!(s.avatars()[1].score >= SURVIVE_POINTS);
    }

    #[test]
    fn taunts_score_and_broadcast() {
        let mut s = PropHuntSession::new(cfg(vec![SeatKind::Human; 2]));
        to_hunt(&mut s);
        let before = s.avatars()[1].score;
        let ev = s.tick(&[0, PadButton::Triangle.mask()]);
        assert!(
            ev.iter()
                .any(|e| matches!(e, HuntEvent::Taunt { seat: 1, .. }))
        );
        assert_eq!(s.avatars()[1].score, before + TAUNT_POINTS);
        s.tick(&[]);
        // Cooldown.
        let ev = s.tick(&[0, PadButton::Triangle.mask()]);
        assert!(!ev.iter().any(|e| matches!(e, HuntEvent::Taunt { .. })));
    }

    #[test]
    fn host_positions_override_and_count_as_movement() {
        let mut s = PropHuntSession::new(cfg(vec![SeatKind::Human; 2]));
        to_hunt(&mut s);
        let p = Vec2::new(123 * FX_ONE, 77 * FX_ONE);
        s.tick_with_positions(&[], &[None, Some(p)]);
        assert_eq!(s.avatars()[1].pos, p);
        assert!(s.avatars()[1].last_moved.is_some());
    }

    #[test]
    fn ai_match_runs_every_round_to_a_result() {
        let mut c = cfg(vec![
            SeatKind::Ai(AiTier::Expert),
            SeatKind::Ai(AiTier::Novice),
            SeatKind::Ai(AiTier::Regular),
        ]);
        c.round_frames = 20 * 60;
        let mut s = PropHuntSession::new(c);
        let mut rounds = 0;
        let mut frames = 0;
        while !s.is_finished() {
            let ev = s.tick(&[]);
            rounds += ev
                .iter()
                .filter(|e| matches!(e, HuntEvent::RoundOver { .. }))
                .count();
            frames += 1;
            assert!(frames < 100_000);
        }
        assert_eq!(rounds, 2);
        let r = s.results().unwrap();
        assert_eq!(r.placements.len(), 3);
    }

    #[test]
    fn from_arena_validates() {
        use legaia_expansion_format::modes::{DecoyPlacement, PropDef, PropHuntArena};
        let mut a = PropHuntArena {
            props: vec![PropDef {
                id: "barrel".into(),
                mesh: None,
                radius: 10,
            }],
            ..Default::default()
        };
        assert!(PropHuntConfig::from_arena(&a, 0).is_ok());
        a.decoys.push(DecoyPlacement {
            prop: "vase".into(),
            x: 0,
            y: 0,
        });
        assert_eq!(
            PropHuntConfig::from_arena(&a, 0),
            Err(ArenaError::UnknownDecoyProp("vase".into()))
        );
        a.decoys.clear();
        a.hunters = 4;
        assert_eq!(
            PropHuntConfig::from_arena(&a, 0),
            Err(ArenaError::BadSeatCount)
        );
        a.props.clear();
        assert_eq!(PropHuntConfig::from_arena(&a, 0), Err(ArenaError::NoProps));
    }
}
