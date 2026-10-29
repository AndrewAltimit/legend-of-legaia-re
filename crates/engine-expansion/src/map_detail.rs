//! The presentation kernels of a pack map's detail layer
//! ([`legaia_expansion_format::map_detail`]): an animated prop's pose, a
//! flickering light's strength and a palette cycle's words, each a pure
//! function of the world's frame counter.
//!
//! Nothing here touches simulation state: the world reads these to tell the
//! hosts how to draw a prop or light a frame, and to rewrite VRAM words, so
//! two runs of the same inputs draw the same frames and a save or a replay
//! never sees them.

use legaia_expansion_format::map_detail::{Animator, Axis};

/// A 3x3 matrix, row-major.
pub type Mat3 = [[f32; 3]; 3];

const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn rotation(axis: Axis, angle: f32) -> Mat3 {
    let (s, c) = angle.sin_cos();
    match axis {
        Axis::X => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        Axis::Y => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        Axis::Z => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// The fraction of its cycle an animator of `period` frames and `phase` is
/// at on `frame`, in `0..1`.
fn cycle(frame: u64, period: i32, phase: f32) -> f32 {
    if period == 0 {
        return 0.0;
    }
    let p = i64::from(period.unsigned_abs());
    let t = (frame as i64).rem_euclid(p) as f32 / p as f32 + phase;
    let t = t.rem_euclid(1.0);
    if period < 0 { 1.0 - t } else { t }
}

/// A prop's draw transform on one frame, in the retail field frame (`Y`
/// down): turn about the prop's own origin by `linear`, then move it by
/// `offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub linear: Mat3,
    pub offset: [f32; 3],
}

impl Pose {
    pub const REST: Self = Self {
        linear: IDENTITY,
        offset: [0.0; 3],
    };

    /// Whether the pose moves the draw at all.
    pub fn is_rest(&self) -> bool {
        *self == Self::REST
    }

    /// [`Self::linear`] as a column-major 4x4, the form a host's placed-draw
    /// turn takes (`legaia_engine_scene::field_env::turn_placed_model`).
    pub fn turn_matrix(&self) -> [f32; 16] {
        let mut out = [0.0f32; 16];
        for c in 0..3 {
            for r in 0..3 {
                out[c * 4 + r] = self.linear[r][c];
            }
        }
        out[15] = 1.0;
        out
    }
}

/// The pose `anims` hold on `frame`: each animator's turn composed in list
/// order, their moves summed.
pub fn pose(anims: &[Animator], frame: u64) -> Pose {
    let tau = std::f32::consts::TAU;
    let mut p = Pose::REST;
    for a in anims {
        match *a {
            Animator::Spin {
                axis,
                period,
                phase,
            } => {
                p.linear = mul(
                    &p.linear,
                    &rotation(axis, cycle(frame, period, phase) * tau),
                );
            }
            Animator::Sway {
                axis,
                degrees,
                period,
                phase,
            } => {
                let a = degrees.to_radians() * (cycle(frame, period, phase) * tau).sin();
                p.linear = mul(&p.linear, &rotation(axis, a));
            }
            Animator::Bob {
                amplitude,
                period,
                phase,
            } => {
                // Up positive in the file; the field frame is Y down.
                p.offset[1] -= f32::from(amplitude) * (cycle(frame, period, phase) * tau).sin();
            }
            Animator::Orbit {
                radius,
                period,
                phase,
            } => {
                let (s, c) = (cycle(frame, period, phase) * tau).sin_cos();
                p.offset[0] += f32::from(radius) * c;
                p.offset[2] += f32::from(radius) * s;
            }
            Animator::Pulse {
                amount,
                period,
                phase,
            } => {
                let k = 1.0 + amount * (cycle(frame, period, phase) * tau).sin();
                p.linear = mul(&p.linear, &[[k, 0.0, 0.0], [0.0, k, 0.0], [0.0, 0.0, k]]);
            }
        }
    }
    p
}

/// A flickering light's strength on `frame`: `1` for a steady light, and
/// for one of depth `flicker` a value in `1 - flicker ..= 1` that wavers
/// every `period` frames. `seed` (the light's index) keeps two lamps out of
/// step. The waver is two incommensurate sines and a hashed step, which
/// reads as flame rather than a pulse.
pub fn flicker_gain(flicker: f32, period: i32, seed: u32, frame: u64) -> f32 {
    if flicker <= 0.0 || period < 1 {
        return 1.0;
    }
    let p = period as f32;
    let t = frame as f32 / p + seed as f32 * 0.618;
    let step = (frame / period as u64) as u32 ^ seed.wrapping_mul(0x9E37_79B9);
    let hash = step
        .wrapping_mul(0x85EB_CA6B)
        .rotate_left(13)
        .wrapping_mul(0xC2B2_AE35);
    let noise = (hash >> 8) as f32 / (1u32 << 24) as f32;
    let wave = 0.5
        + 0.25 * (t * std::f32::consts::TAU).sin()
        + 0.15 * (t * 2.71 * std::f32::consts::TAU).sin()
        + 0.1 * (noise * 2.0 - 1.0);
    1.0 - flicker * wave.clamp(0.0, 1.0)
}

/// Palette entries `start .. start + count` of `src` rotated `steps` places
/// (toward higher entries; `reverse` the other way). Entries outside the
/// range keep their words.
pub fn cycle_palette(
    src: &[u16],
    start: usize,
    count: usize,
    steps: u64,
    reverse: bool,
) -> Vec<u16> {
    let mut out = src.to_vec();
    let end = (start + count).min(src.len());
    if start >= end {
        return out;
    }
    let n = end - start;
    let k = (steps % n as u64) as usize;
    for i in 0..n {
        let from = if reverse {
            (i + k) % n
        } else {
            (i + n - k) % n
        };
        out[start + i] = src[start + from];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn a_spin_turns_a_full_circle_a_period() {
        let s = [Animator::Spin {
            axis: Axis::Y,
            period: 120,
            phase: 0.0,
        }];
        assert!(pose(&s, 0).is_rest() || close(pose(&s, 0).linear[0][0], 1.0));
        let q = pose(&s, 30).linear;
        // A quarter turn about Y.
        assert!(close(q[0][0], 0.0) && close(q[0][2], 1.0));
        assert_eq!(pose(&s, 7), pose(&s, 127), "periodic");
        let back = [Animator::Spin {
            axis: Axis::Y,
            period: -120,
            phase: 0.0,
        }];
        assert!(close(pose(&back, 30).linear[0][2], -1.0));
    }

    #[test]
    fn bob_and_orbit_move_the_draw() {
        let b = [Animator::Bob {
            amplitude: 10,
            period: 40,
            phase: 0.0,
        }];
        // A quarter period in: at the top, which is up (negative Y).
        assert!(close(pose(&b, 10).offset[1], -10.0));
        let o = [Animator::Orbit {
            radius: 20,
            period: 40,
            phase: 0.25,
        }];
        let p = pose(&o, 0).offset;
        assert!(close(p[0], 0.0) && close(p[2], 20.0));
    }

    #[test]
    fn a_phase_offsets_the_cycle() {
        let a = |phase| {
            [Animator::Sway {
                axis: Axis::Z,
                degrees: 10.0,
                period: 60,
                phase,
            }]
        };
        assert_eq!(pose(&a(0.5), 0), pose(&a(0.0), 30));
    }

    #[test]
    fn a_pulse_scales() {
        let a = [Animator::Pulse {
            amount: 0.2,
            period: 20,
            phase: 0.0,
        }];
        assert!(close(pose(&a, 5).linear[1][1], 1.2));
        assert!(close(pose(&a, 15).linear[1][1], 0.8));
    }

    #[test]
    fn the_turn_matrix_is_column_major() {
        let p = Pose {
            linear: [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]],
            offset: [0.0; 3],
        };
        let m = p.turn_matrix();
        assert_eq!(&m[0..4], &[1.0, 4.0, 7.0, 0.0]);
        assert_eq!(m[15], 1.0);
    }

    #[test]
    fn flicker_stays_in_its_band_and_lamps_differ() {
        for f in 0..500 {
            let g = flicker_gain(0.4, 12, 3, f);
            assert!((0.6..=1.0).contains(&g), "{g}");
        }
        assert_eq!(flicker_gain(0.0, 12, 0, 77), 1.0);
        let a: Vec<f32> = (0..50).map(|f| flicker_gain(0.4, 12, 0, f)).collect();
        let b: Vec<f32> = (0..50).map(|f| flicker_gain(0.4, 12, 1, f)).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn a_palette_cycle_rotates_only_its_range() {
        let src: Vec<u16> = (0..8).collect();
        assert_eq!(
            cycle_palette(&src, 2, 4, 1, false),
            vec![0, 1, 5, 2, 3, 4, 6, 7]
        );
        assert_eq!(
            cycle_palette(&src, 2, 4, 1, true),
            vec![0, 1, 3, 4, 5, 2, 6, 7]
        );
        assert_eq!(cycle_palette(&src, 2, 4, 4, false), src);
    }
}
