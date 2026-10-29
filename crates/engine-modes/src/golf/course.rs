//! A golf hole as a tile grid, and the ball physics over it.
//!
//! Positions are [`Fx`] world units; a tile is [`TILE`] units square, tile
//! `(tx, ty)` spanning `[tx*TILE, (tx+1)*TILE)` on each axis. The ball is
//! a point integrated in [`SUBSTEPS`] sub-steps per frame:
//!
//! 1. slope: an arrow tile pushes the ball downhill by [`SLOPE_ACCEL`];
//!    the height map pushes it along the negative height gradient;
//! 2. rolling friction per surface ([`Surface::friction`]), a per-mille
//!    of speed lost each frame, plus a small constant drag so a ball on
//!    the flat always stops;
//! 3. each axis moves separately; an axis step into a wall tile is undone
//!    and that velocity component reflects at [`WALL_RESTITUTION`];
//! 4. a ball inside the cup radius below [`CUP_MAX_SPEED`] drops.
//!
//! A ball that ends a sub-step on water or out of bounds is a hazard: the
//! shot is replayed from where it was struck with a penalty stroke.

use legaia_expansion_format::modes::GolfHole;
use serde::{Deserialize, Serialize};

use crate::fixed::{FX_ONE, Fx, Vec2, isqrt};

/// Tile edge in world units.
pub const TILE: i32 = 32;
/// Sub-steps per frame.
pub const SUBSTEPS: i32 = 4;
/// Fastest shot, world units per frame at full power.
pub const MAX_SHOT_SPEED: Fx = 10 * FX_ONE;
/// Arrow-tile acceleration, [`Fx`] per frame.
pub const SLOPE_ACCEL: Fx = FX_ONE / 12;
/// Height-gradient acceleration per height step, [`Fx`] per frame.
pub const HEIGHT_ACCEL: Fx = FX_ONE / 16;
/// Constant drag, [`Fx`] per frame.
pub const DRAG: Fx = FX_ONE / 64;
/// Fraction of the normal velocity a wall returns, per-mille.
pub const WALL_RESTITUTION: i32 = 600;
/// Cup capture radius, world units.
pub const CUP_RADIUS: Fx = 6 * FX_ONE;
/// A ball faster than this skips over the cup.
pub const CUP_MAX_SPEED: Fx = 5 * FX_ONE;
/// Below this speed the ball counts as settling.
pub const REST_SPEED: Fx = FX_ONE / 8;
/// Settling frames before the ball is called at rest.
pub const REST_FRAMES: u32 = 20;
/// A roll is stopped after this many frames whatever its speed.
pub const MAX_ROLL_FRAMES: u32 = 60 * 20;

/// What a tile is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    Fairway,
    Rough,
    Sand,
    Water,
    Wall,
    OutOfBounds,
}

impl Surface {
    /// Speed lost per frame, per-mille.
    pub fn friction(self) -> i32 {
        match self {
            Self::Fairway => 15,
            Self::Rough => 45,
            Self::Sand => 130,
            _ => 0,
        }
    }

    pub fn is_hazard(self) -> bool {
        matches!(self, Self::Water | Self::OutOfBounds)
    }
}

/// One tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tile {
    pub surface: Surface,
    /// Downhill direction of an arrow tile, each `-1..=1`.
    pub slope: (i8, i8),
    pub height: u8,
}

/// Why a hole's tile data was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CourseError {
    Empty,
    BadTile { row: usize, col: usize, ch: char },
    NeedOneTee(usize),
    NeedOneCup(usize),
    HeightShape,
}

/// A parsed hole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Course {
    pub width: usize,
    pub height: usize,
    tiles: Vec<Tile>,
    pub tee: Vec2,
    pub cup: Vec2,
    pub par: u8,
    pub max_strokes: u8,
}

/// How a simulated or played roll ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RollEnd {
    Rest,
    Holed,
    Hazard,
}

/// A ball in motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ball {
    pub pos: Vec2,
    pub vel: Vec2,
    pub slow_frames: u32,
    pub frames: u32,
}

fn tile_centre(tx: usize, ty: usize) -> Vec2 {
    Vec2::new(
        (tx as i32 * TILE + TILE / 2) * FX_ONE,
        (ty as i32 * TILE + TILE / 2) * FX_ONE,
    )
}

impl Course {
    /// Parse a pack hole (legend in `legaia_expansion_format::modes`).
    pub fn from_hole(h: &GolfHole) -> Result<Self, CourseError> {
        let height = h.rows.len();
        let width = h.rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        if width == 0 {
            return Err(CourseError::Empty);
        }
        if !h.heights.is_empty() && h.heights.len() != height {
            return Err(CourseError::HeightShape);
        }
        let mut tiles = Vec::with_capacity(width * height);
        let (mut tees, mut cups) = (Vec::new(), Vec::new());
        for (ty, row) in h.rows.iter().enumerate() {
            let hrow: Vec<char> = h
                .heights
                .get(ty)
                .map(|r| r.chars().collect())
                .unwrap_or_default();
            let chars: Vec<char> = row.chars().collect();
            for tx in 0..width {
                let ch = chars.get(tx).copied().unwrap_or(' ');
                let (surface, slope) = match ch {
                    '.' => (Surface::Fairway, (0, 0)),
                    ',' => (Surface::Rough, (0, 0)),
                    's' => (Surface::Sand, (0, 0)),
                    'w' => (Surface::Water, (0, 0)),
                    '#' => (Surface::Wall, (0, 0)),
                    ' ' => (Surface::OutOfBounds, (0, 0)),
                    'T' => {
                        tees.push((tx, ty));
                        (Surface::Fairway, (0, 0))
                    }
                    'O' => {
                        cups.push((tx, ty));
                        (Surface::Fairway, (0, 0))
                    }
                    '^' => (Surface::Fairway, (0, -1)),
                    'v' => (Surface::Fairway, (0, 1)),
                    '<' => (Surface::Fairway, (-1, 0)),
                    '>' => (Surface::Fairway, (1, 0)),
                    _ => {
                        return Err(CourseError::BadTile {
                            row: ty,
                            col: tx,
                            ch,
                        });
                    }
                };
                let height = match hrow.get(tx) {
                    Some(c) if c.is_ascii_digit() => *c as u8 - b'0',
                    Some(' ') | None => 0,
                    Some(&c) => {
                        return Err(CourseError::BadTile {
                            row: ty,
                            col: tx,
                            ch: c,
                        });
                    }
                };
                tiles.push(Tile {
                    surface,
                    slope,
                    height,
                });
            }
        }
        if tees.len() != 1 {
            return Err(CourseError::NeedOneTee(tees.len()));
        }
        if cups.len() != 1 {
            return Err(CourseError::NeedOneCup(cups.len()));
        }
        let par = h.par.max(1);
        let max_strokes = if h.max_strokes == 0 {
            par.saturating_add(5)
        } else {
            h.max_strokes
        };
        Ok(Self {
            width,
            height,
            tiles,
            tee: tile_centre(tees[0].0, tees[0].1),
            cup: tile_centre(cups[0].0, cups[0].1),
            par,
            max_strokes,
        })
    }

    /// The tile under a world position (out of bounds off the grid).
    pub fn tile_at(&self, p: Vec2) -> Tile {
        let tx = p.x.div_euclid(TILE * FX_ONE);
        let ty = p.y.div_euclid(TILE * FX_ONE);
        if tx < 0 || ty < 0 || tx as usize >= self.width || ty as usize >= self.height {
            return Tile {
                surface: Surface::OutOfBounds,
                slope: (0, 0),
                height: 0,
            };
        }
        self.tiles[ty as usize * self.width + tx as usize]
    }

    fn height_at(&self, tx: i32, ty: i32) -> Option<i32> {
        if tx < 0 || ty < 0 || tx as usize >= self.width || ty as usize >= self.height {
            return None;
        }
        let t = self.tiles[ty as usize * self.width + tx as usize];
        (t.surface != Surface::Wall).then_some(t.height as i32)
    }

    /// Downhill acceleration at `p`, [`Fx`] per frame.
    fn slope_accel(&self, p: Vec2) -> Vec2 {
        let t = self.tile_at(p);
        let mut a = Vec2::new(t.slope.0 as Fx * SLOPE_ACCEL, t.slope.1 as Fx * SLOPE_ACCEL);
        let tx = p.x.div_euclid(TILE * FX_ONE);
        let ty = p.y.div_euclid(TILE * FX_ONE);
        let h = t.height as i32;
        // Central difference, falling back to one side at an edge or wall.
        let gx = match (self.height_at(tx - 1, ty), self.height_at(tx + 1, ty)) {
            (Some(l), Some(r)) => (r - l) / 2,
            (Some(l), None) => h - l,
            (None, Some(r)) => r - h,
            _ => 0,
        };
        let gy = match (self.height_at(tx, ty - 1), self.height_at(tx, ty + 1)) {
            (Some(u), Some(d)) => (d - u) / 2,
            (Some(u), None) => h - u,
            (None, Some(d)) => d - h,
            _ => 0,
        };
        a.x -= gx * HEIGHT_ACCEL;
        a.y -= gy * HEIGHT_ACCEL;
        a
    }

    /// Strike a ball from `pos`.
    pub fn strike(pos: Vec2, angle: u16, power: u32) -> Ball {
        let speed = (MAX_SHOT_SPEED as i64 * power.min(100) as i64 / 100) as Fx;
        Ball {
            pos,
            vel: Vec2::from_angle(angle, speed),
            slow_frames: 0,
            frames: 0,
        }
    }

    /// Advance a rolling ball one frame; `Some` once the roll has ended.
    pub fn step(&self, b: &mut Ball) -> Option<RollEnd> {
        b.frames += 1;
        let accel = self.slope_accel(b.pos);
        b.vel = b.vel + accel;
        let surface = self.tile_at(b.pos).surface;
        let keep = 1000 - surface.friction();
        b.vel = b.vel.scale(keep, 1000);
        // Constant drag against the direction of travel.
        let speed = b.vel.length();
        if speed <= DRAG {
            b.vel = Vec2::ZERO;
        } else {
            b.vel = b.vel.scale(speed - DRAG, speed);
        }
        for _ in 0..SUBSTEPS {
            let dx = b.vel.x / SUBSTEPS;
            let dy = b.vel.y / SUBSTEPS;
            let nx = Vec2::new(b.pos.x + dx, b.pos.y);
            if self.tile_at(nx).surface == Surface::Wall {
                b.vel.x = -b.vel.x * WALL_RESTITUTION / 1000;
            } else {
                b.pos = nx;
            }
            let ny = Vec2::new(b.pos.x, b.pos.y + dy);
            if self.tile_at(ny).surface == Surface::Wall {
                b.vel.y = -b.vel.y * WALL_RESTITUTION / 1000;
            } else {
                b.pos = ny;
            }
            if self.tile_at(b.pos).surface.is_hazard() {
                return Some(RollEnd::Hazard);
            }
            let to_cup = (b.pos - self.cup).len_sq();
            let r = CUP_RADIUS as i64;
            if to_cup <= r * r && b.vel.length() <= CUP_MAX_SPEED {
                b.pos = self.cup;
                b.vel = Vec2::ZERO;
                return Some(RollEnd::Holed);
            }
        }
        if b.vel.length() < REST_SPEED {
            b.slow_frames += 1;
        } else {
            b.slow_frames = 0;
        }
        if b.slow_frames >= REST_FRAMES || b.frames >= MAX_ROLL_FRAMES {
            b.vel = Vec2::ZERO;
            return Some(RollEnd::Rest);
        }
        None
    }

    /// Play a whole shot out without a session (the AI's look-ahead).
    pub fn simulate(&self, from: Vec2, angle: u16, power: u32) -> (RollEnd, Vec2) {
        let mut b = Self::strike(from, angle, power);
        loop {
            if let Some(end) = self.step(&mut b) {
                return (end, b.pos);
            }
        }
    }

    /// Straight-line distance to the cup in world units.
    pub fn distance_to_cup(&self, p: Vec2) -> i64 {
        isqrt((p - self.cup).len_sq() as u64) as i64 / FX_ONE as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::ANGLE_QUARTER;

    fn hole(rows: &[&str]) -> Course {
        Course::from_hole(&GolfHole {
            par: 2,
            rows: rows.iter().map(|r| r.to_string()).collect(),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn parse_validates_tee_and_cup() {
        let err = |rows: &[&str]| {
            Course::from_hole(&GolfHole {
                rows: rows.iter().map(|r| r.to_string()).collect(),
                ..Default::default()
            })
            .unwrap_err()
        };
        assert_eq!(err(&["...O"]), CourseError::NeedOneTee(0));
        assert_eq!(err(&["T.TO"]), CourseError::NeedOneTee(2));
        assert_eq!(err(&["T..."]), CourseError::NeedOneCup(0));
        assert!(matches!(
            err(&["T.xO"]),
            CourseError::BadTile { ch: 'x', .. }
        ));
        let c = hole(&["T.O"]);
        assert_eq!(c.max_strokes, 7);
        assert_eq!(c.tee, Vec2::new(16 * FX_ONE, 16 * FX_ONE));
    }

    #[test]
    fn flat_roll_stops_and_friction_orders_surfaces() {
        let fair = hole(&["T..........................................O"]);
        let rough = hole(&["T,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,O"]);
        let sand = hole(&["Tssssssssssssssssssssssssssssssssssssssssss.O"]);
        let roll = |c: &Course| {
            let (end, p) = c.simulate(c.tee, 0, 50);
            assert_eq!(end, RollEnd::Rest);
            p.x - c.tee.x
        };
        let (f, r, s) = (roll(&fair), roll(&rough), roll(&sand));
        assert!(f > r && r > s && s > 0, "{f} {r} {s}");
    }

    #[test]
    fn walls_bounce_the_ball_back() {
        let c = hole(&["#####", "#T.O#", "#####"]);
        // Fire straight up into the wall: the ball comes back down and
        // stays on the course.
        let (end, p) = c.simulate(c.tee, 3 * ANGLE_QUARTER, 100);
        assert_ne!(end, RollEnd::Hazard);
        assert_eq!(c.tile_at(p).surface, Surface::Fairway);
    }

    #[test]
    fn water_is_a_hazard_and_a_soft_putt_drops() {
        let c = hole(&["Tw.O"]);
        assert_eq!(c.simulate(c.tee, 0, 60).0, RollEnd::Hazard);
        let c = hole(&["T.O"]);
        // A gentle putt along the row finds the cup.
        let holed = (5..40).any(|p| c.simulate(c.tee, 0, p).0 == RollEnd::Holed);
        assert!(holed);
        // Full power skips it.
        assert_ne!(c.simulate(c.tee, 0, 100).0, RollEnd::Holed);
    }

    #[test]
    fn slopes_pull_the_ball_downhill() {
        let c = hole(&["  ...  ", "  .T.  ", "  >>>  ", "  >>>  ", "  .O.  "]);
        let mut b = Ball {
            pos: Vec2::new(
                3 * TILE * FX_ONE + TILE * FX_ONE / 2,
                2 * TILE * FX_ONE + 4 * FX_ONE,
            ),
            vel: Vec2::ZERO,
            slow_frames: 0,
            frames: 0,
        };
        let x0 = b.pos.x;
        for _ in 0..10 {
            c.step(&mut b);
        }
        assert!(b.pos.x > x0);
        // A height map does the same along its gradient.
        let mut h = GolfHole {
            rows: vec!["T....O".into()],
            heights: vec!["543210".into()],
            ..Default::default()
        };
        let c = Course::from_hole(&h).unwrap();
        let mut b = Course::strike(c.tee, 0, 0);
        for _ in 0..30 {
            c.step(&mut b);
        }
        assert!(b.pos.x > c.tee.x);
        h.heights = vec!["543210".into(), "000000".into()];
        assert!(Course::from_hole(&h).is_err());
    }
}
