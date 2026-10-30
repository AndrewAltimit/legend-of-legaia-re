//! A party night's podium **in the world**: three blocks on a pack map,
//! the top three's party leaders standing on them, confetti falling, and
//! the camera's shot.
//!
//! The night's standings are the only input; everything here is a pure
//! function of them, the podium's placement and a frame clock, so the
//! podium draws the same on both hosts and never feeds the night.
//!
//! Layout, seen from the camera (which stands in front of the podium and
//! looks back along its facing): 2nd on the left, 1st in the middle and
//! highest, 3rd on the right. Each block rises in over
//! [`RISE_FRAMES`]; the figures stand on its top (the world stands the
//! blocks up as standable tops, so the floor sampler lifts whoever stands
//! there); confetti falls over the whole podium on fixed per-piece paths.

use legaia_engine_modes::cards::table::sector_to_mode_angle;
use legaia_engine_modes::party::{PlayerKey, Standing};

use crate::mode_stage::StageShot;

/// Frames the blocks take to rise.
pub const RISE_FRAMES: u32 = 50;
/// Each place's block height, world units (1st, 2nd, 3rd).
pub const BLOCK_HEIGHTS: [f32; 3] = [112.0, 76.0, 48.0];
/// A block's half-width, world units.
pub const BLOCK_HALF: f32 = 58.0;
/// Centre-to-centre spacing of the blocks, world units.
pub const BLOCK_SPACING: f32 = 128.0;
/// Confetti pieces falling at once.
pub const CONFETTI: usize = 72;
/// The height confetti falls from, world units over the floor.
const CONFETTI_TOP: f32 = 520.0;

/// One block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PodiumBlock {
    /// 1, 2 or 3.
    pub place: u8,
    /// World `(X, Z)` of the block's centre.
    pub centre: [f32; 2],
    /// Half-width, world units.
    pub half: f32,
    /// Height this frame (rising), world units.
    pub height: f32,
    /// Height once risen.
    pub full_height: f32,
    /// The podium's facing, a mode angle: the block's front face.
    pub facing: u16,
    pub fill: [u8; 4],
    pub rim: [u8; 4],
}

/// Who stands on a block.
#[derive(Debug, Clone, PartialEq)]
pub struct PodiumFigure {
    pub key: PlayerKey,
    pub name: String,
    pub place: u8,
    /// World `(X, Z)`: the block's centre.
    pub pos: [f32; 2],
    /// Mode angle: toward the camera.
    pub facing: u16,
    /// The block's height under it this frame.
    pub lift: f32,
    /// The winner, celebrating.
    pub cheer: bool,
}

/// One piece of confetti: world `(X, height over the floor, Z)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Confetti {
    pub pos: [f32; 3],
    pub color: [u8; 4],
    /// Edge length, world units.
    pub size: f32,
    /// Its spin, `0..1` of a turn: the piece's width narrows as it turns.
    pub turn: f32,
}

/// The podium this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PodiumFrame {
    pub blocks: Vec<PodiumBlock>,
    pub figures: Vec<PodiumFigure>,
    pub confetti: Vec<Confetti>,
    /// Twinkles round the winner's head: world `(X, height, Z)`.
    pub sparkles: Vec<[f32; 3]>,
    pub shot: StageShot,
}

/// Where the podium stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PodiumPlace {
    /// World `(X, Z)` of the first-place block.
    pub centre: [f32; 2],
    /// Compass sector the podium faces (`0..8`).
    pub facing: u8,
}

const CONFETTI_COLORS: [[u8; 4]; 6] = [
    [240, 86, 120, 255],
    [250, 210, 80, 255],
    [100, 200, 250, 255],
    [140, 240, 140, 255],
    [210, 130, 250, 255],
    [255, 255, 255, 255],
];

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A mode angle's unit vector on the ground (`0` = `+X`, a quarter = `+Z`).
fn dir(angle: u16) -> [f32; 2] {
    let a = f32::from(angle) / 4096.0 * std::f32::consts::TAU;
    [a.cos(), a.sin()]
}

/// The podium of `standings` (place order, as [`PartyNight::standings`]
/// gives them) at `place`, `frame` frames after it came up.
///
/// [`PartyNight::standings`]: legaia_engine_modes::party::PartyNight::standings
pub fn podium_frame(place: PodiumPlace, standings: &[Standing], frame: u32) -> PodiumFrame {
    let facing = sector_to_mode_angle(place.facing);
    let f = dir(facing);
    // The camera stands at `centre + f * d` looking back along `-f`; its
    // right hand on the ground is `f` turned a quarter back.
    let right = [-f[1], f[0]];
    let [cx, cz] = place.centre;
    let rise = smooth(frame as f32 / RISE_FRAMES as f32);
    // Block slot k: 0 = 1st (middle), 1 = 2nd (left), 2 = 3rd (right).
    let offsets = [0.0, -BLOCK_SPACING, BLOCK_SPACING];
    let fills = [
        ([210, 168, 56, 255], [255, 236, 150, 255]),
        ([168, 174, 188, 255], [236, 240, 250, 255]),
        ([172, 110, 64, 255], [240, 190, 140, 255]),
    ];
    let mut blocks = Vec::new();
    let mut figures = Vec::new();
    for (k, s) in standings.iter().filter(|s| !s.left).take(3).enumerate() {
        // Tied places share a block height but keep their own block.
        let tier = usize::from(s.place.clamp(1, 3) - 1);
        let centre = [cx + right[0] * offsets[k], cz + right[1] * offsets[k]];
        let full = BLOCK_HEIGHTS[tier];
        // The lower blocks rise a beat behind the winner's.
        let lag = (k as f32) * 0.12;
        let h = full * smooth((rise - lag) / (1.0 - lag));
        blocks.push(PodiumBlock {
            place: s.place,
            centre,
            half: BLOCK_HALF,
            height: h,
            full_height: full,
            facing,
            fill: fills[tier].0,
            rim: fills[tier].1,
        });
        figures.push(PodiumFigure {
            key: s.key,
            name: s.name.clone(),
            place: s.place,
            pos: centre,
            facing,
            lift: h,
            cheer: s.place == 1,
        });
    }
    // Confetti over the podium, once the blocks are up.
    let mut confetti = Vec::new();
    if frame >= RISE_FRAMES / 2 {
        let t = (frame - RISE_FRAMES / 2) as f32;
        for i in 0..CONFETTI as u32 {
            let seed = i.wrapping_mul(2_654_435_761).rotate_left(7) ^ 0x9E37;
            let u = |shift: u32| ((seed >> shift) & 0xFF) as f32 / 255.0;
            let speed = 1.4 + u(0) * 1.6;
            let span = CONFETTI_TOP + 40.0;
            let y = CONFETTI_TOP - (t * speed + u(8) * span) % span;
            let along = (u(16) - 0.5) * 2.0 * (BLOCK_SPACING * 1.9);
            let depth = (u(24) - 0.5) * 2.0 * 150.0;
            let sway = ((t * 0.05 + u(4) * 6.3).sin()) * 18.0;
            confetti.push(Confetti {
                pos: [
                    cx + right[0] * (along + sway) + f[0] * depth,
                    y.max(1.0),
                    cz + right[1] * (along + sway) + f[1] * depth,
                ],
                color: CONFETTI_COLORS[i as usize % CONFETTI_COLORS.len()],
                size: 7.0 + u(12) * 5.0,
                turn: (t * (0.01 + u(20) * 0.03) + u(2)) % 1.0,
            });
        }
    }
    // Sparkles circling the winner's head.
    let mut sparkles = Vec::new();
    if let Some(w) = figures.iter().find(|f| f.cheer) {
        for i in 0..8u32 {
            if (frame / 4 + i).is_multiple_of(3) {
                continue;
            }
            let a = (i as f32 / 8.0 + frame as f32 / 180.0) * std::f32::consts::TAU;
            sparkles.push([
                w.pos[0] + a.cos() * 46.0,
                w.lift + 120.0 + (a * 2.0).sin() * 14.0,
                w.pos[1] + a.sin() * 46.0,
            ]);
        }
    }
    // The shot: from far out and low, easing in over the blocks' rise,
    // then a slow sway either side of straight on.
    let ease = smooth(frame as f32 / 150.0);
    let sway = (frame as f32 / 300.0 * std::f32::consts::TAU).sin() * 140.0 * ease;
    let shot = StageShot {
        focus: [cx, cz],
        yaw: (f32::from(facing) + 2048.0 + sway).rem_euclid(4096.0),
        pitch: 300.0 + 60.0 * ease,
        distance: 1.5 - 0.8 * ease,
        lift: 90.0,
    };
    PodiumFrame {
        blocks,
        figures,
        confetti,
        sparkles,
        shot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standings() -> Vec<Standing> {
        ["You", "Mirela", "Tobin", "Odalric"]
            .iter()
            .enumerate()
            .map(|(i, n)| Standing {
                key: i as PlayerKey,
                name: n.to_string(),
                score: 40 - i as i64 * 10,
                place: i as u8 + 1,
                left: false,
            })
            .collect()
    }

    #[test]
    fn three_blocks_rise_with_the_winner_highest_in_the_middle() {
        let place = PodiumPlace {
            centre: [1000.0, 2000.0],
            facing: 4,
        };
        let f0 = podium_frame(place, &standings(), 0);
        assert_eq!(f0.blocks.len(), 3);
        assert!(f0.blocks.iter().all(|b| b.height < 1.0));
        assert!(f0.confetti.is_empty());
        let f = podium_frame(place, &standings(), RISE_FRAMES * 2);
        let h: Vec<f32> = f.blocks.iter().map(|b| b.height).collect();
        assert_eq!(h, BLOCK_HEIGHTS.to_vec());
        assert_eq!(f.blocks[0].centre, [1000.0, 2000.0]);
        // 2nd and 3rd stand either side, the same distance off.
        let d = |b: &PodiumBlock| {
            ((b.centre[0] - 1000.0).powi(2) + (b.centre[1] - 2000.0).powi(2)).sqrt()
        };
        assert!((d(&f.blocks[1]) - BLOCK_SPACING).abs() < 0.01);
        assert!((d(&f.blocks[2]) - BLOCK_SPACING).abs() < 0.01);
        assert_eq!(f.figures.len(), 3);
        assert!(f.figures[0].cheer && !f.figures[1].cheer);
        assert_eq!(f.confetti.len(), CONFETTI);
        assert!(!f.sparkles.is_empty());
        // Fourth place has no block.
        assert!(f.figures.iter().all(|x| x.key != 3));
    }

    #[test]
    fn the_shot_looks_back_at_the_podium_and_settles() {
        let place = PodiumPlace {
            centre: [0.0, 0.0],
            facing: 0,
        };
        let a = podium_frame(place, &standings(), 0);
        let b = podium_frame(place, &standings(), 150);
        assert!(a.shot.distance > b.shot.distance);
        let facing = f32::from(sector_to_mode_angle(0));
        assert!(((a.shot.yaw - facing).rem_euclid(4096.0) - 2048.0).abs() < 1.0);
        assert_eq!(a, podium_frame(place, &standings(), 0), "deterministic");
    }

    #[test]
    fn confetti_stays_over_the_podium() {
        let place = PodiumPlace {
            centre: [500.0, 500.0],
            facing: 2,
        };
        for frame in [60, 333, 2000] {
            for c in podium_frame(place, &standings(), frame).confetti {
                assert!(c.pos[1] > 0.0 && c.pos[1] <= CONFETTI_TOP);
                assert!((c.pos[0] - 500.0).abs() < 400.0 && (c.pos[2] - 500.0).abs() < 400.0);
            }
        }
    }
}
