//! A race's track marked out on the ground
//! ([`legaia_engine_expansion::mode_stage::StageTrack`]): red-and-white
//! kerbs along both edges, a chequered start line across the first
//! segment, and cyan chevrons down every boost segment pointing the way
//! round. The track is the session's centerline and half-width, so the
//! marks show exactly where a racer is held, whatever the map draws under
//! them.

use legaia_engine_expansion::mode_stage::StageTrack;
use legaia_render_kernels::screen_prim::ScreenPrim;

use super::{GROUND_BAND, Projector, quad};

/// A kerb's width across the edge, and one dash's length, world units.
const KERB_W: f32 = 14.0;
const KERB_DASH: f32 = 28.0;
/// The start line's depth along the track, and its squares across it.
const START_DEPTH: f32 = 22.0;
const START_SQUARES: usize = 8;
/// A boost chevron's length along the track, and the gap between two.
const CHEVRON_LEN: f32 = 26.0;
const CHEVRON_GAP: f32 = 44.0;
/// How far a mark floats over the floor, so it never fights the ground.
const LIFT: f32 = 1.5;

const KERB_RED: [u8; 4] = [206, 46, 40, 255];
const KERB_WHITE: [u8; 4] = [236, 232, 224, 255];
const CHECK_DARK: [u8; 4] = [24, 24, 28, 255];
const CHECK_LIGHT: [u8; 4] = [240, 240, 236, 255];
const BOOST: [u8; 4] = [70, 190, 255, 255];

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn unit(v: [f32; 2]) -> [f32; 2] {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-3);
    [v[0] / l, v[1] / l]
}

/// The left-hand normal of a direction on the ground.
fn normal(d: [f32; 2]) -> [f32; 2] {
    [-d[1], d[0]]
}

/// A quad on the floor from four ground points, `[tl, tr, bl, br]`, in the
/// ground band at `layer` (a higher layer draws first).
pub(super) fn ground_quad(
    out: &mut Vec<ScreenPrim>,
    pr: &Projector<'_>,
    c: [[f32; 2]; 4],
    colors: [[u8; 4]; 4],
    blend: bool,
    layer: u32,
) {
    let p = c.map(|q| pr.on_floor(q[0], q[1], LIFT));
    let (Some(a), Some(b), Some(cc), Some(d)) =
        (pr.at(p[0]), pr.at(p[1]), pr.at(p[2]), pr.at(p[3]))
    else {
        return;
    };
    out.push(quad([a, b, cc, d], colors, blend, GROUND_BAND + layer));
}

/// Where the edge `side` (`+1` left, `-1` right) of the track meets at
/// waypoint `i`: the mitre of the two segments' offsets, held to twice the
/// half-width at a sharp corner.
fn edge_at(points: &[[f32; 2]], i: usize, off: f32) -> [f32; 2] {
    let n = points.len();
    let p = points[i];
    let d_in = unit(sub(p, points[(i + n - 1) % n]));
    let d_out = unit(sub(points[(i + 1) % n], p));
    let (n0, n1) = (normal(d_in), normal(d_out));
    let m = unit([n0[0] + n1[0], n0[1] + n1[1]]);
    let cos = (m[0] * n1[0] + m[1] * n1[1]).max(0.5);
    let k = off / cos;
    [p[0] + m[0] * k, p[1] + m[1] * k]
}

/// The kerbs: dashes along both edges of every segment, alternating red
/// and white, the white a touch raised in light.
fn kerbs(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    let n = t.points.len();
    let hw = t.half_width;
    for side in [1.0f32, -1.0] {
        let mut phase = 0usize;
        for i in 0..n {
            let j = (i + 1) % n;
            let outer = (
                edge_at(&t.points, i, side * (hw + KERB_W * 0.5)),
                edge_at(&t.points, j, side * (hw + KERB_W * 0.5)),
            );
            let inner = (
                edge_at(&t.points, i, side * (hw - KERB_W * 0.5)),
                edge_at(&t.points, j, side * (hw - KERB_W * 0.5)),
            );
            let len = {
                let d = sub(t.points[j], t.points[i]);
                (d[0] * d[0] + d[1] * d[1]).sqrt()
            };
            let dashes = ((len / KERB_DASH).round() as usize).max(1);
            let lerp = |a: [f32; 2], b: [f32; 2], k: f32| {
                [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]
            };
            for k in 0..dashes {
                let (k0, k1) = (k as f32 / dashes as f32, (k + 1) as f32 / dashes as f32);
                let c = [
                    lerp(outer.0, outer.1, k0),
                    lerp(outer.0, outer.1, k1),
                    lerp(inner.0, inner.1, k0),
                    lerp(inner.0, inner.1, k1),
                ];
                let col = if phase.is_multiple_of(2) {
                    KERB_RED
                } else {
                    KERB_WHITE
                };
                let lo = [col[0] / 4 * 3, col[1] / 4 * 3, col[2] / 4 * 3, 255];
                // Outer edge darker, so the kerb reads as a lip.
                ground_quad(out, pr, c, [lo, lo, col, col], false, 3);
                phase += 1;
            }
        }
    }
}

/// The chequered start line across the start of segment `0`.
fn start_line(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    let n = t.points.len();
    if n < 2 {
        return;
    }
    let p = t.points[0];
    let d = unit(sub(t.points[1], p));
    let nrm = normal(d);
    let hw = t.half_width;
    let at = |across: f32, along: f32| {
        [
            p[0] + nrm[0] * across + d[0] * along,
            p[1] + nrm[1] * across + d[1] * along,
        ]
    };
    let sq = hw * 2.0 / START_SQUARES as f32;
    for row in 0..2 {
        let (a0, a1) = (
            -START_DEPTH * 0.5 + row as f32 * START_DEPTH * 0.5,
            row as f32 * START_DEPTH * 0.5,
        );
        for col in 0..START_SQUARES {
            let (c0, c1) = (-hw + col as f32 * sq, -hw + (col + 1) as f32 * sq);
            let k = if (row + col) % 2 == 0 {
                CHECK_LIGHT
            } else {
                CHECK_DARK
            };
            ground_quad(
                out,
                pr,
                [at(c1, a1), at(c0, a1), at(c1, a0), at(c0, a0)],
                [k; 4],
                false,
                2,
            );
        }
    }
}

/// Chevrons down each boost segment, pointing along it.
fn chevrons(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    let n = t.points.len();
    let hw = t.half_width * 0.45;
    for &seg in &t.boost_segments {
        if seg >= n {
            continue;
        }
        let (a, b) = (t.points[seg], t.points[(seg + 1) % n]);
        let dv = sub(b, a);
        let len = (dv[0] * dv[0] + dv[1] * dv[1]).sqrt();
        let d = unit(dv);
        let nrm = normal(d);
        let count = ((len - CHEVRON_LEN) / CHEVRON_GAP).floor().max(0.0) as usize;
        for k in 0..count {
            let s = CHEVRON_GAP * 0.5 + k as f32 * CHEVRON_GAP;
            let col = BOOST;
            let at = |across: f32, along: f32| {
                [
                    a[0] + nrm[0] * across + d[0] * along,
                    a[1] + nrm[1] * across + d[1] * along,
                ]
            };
            let w = 7.0;
            // Each arm: from the side back to the tip, `w` thick.
            for side in [1.0f32, -1.0] {
                let c = [
                    at(side * hw, s),
                    at(side * hw, s + w),
                    at(0.0, s + CHEVRON_LEN),
                    at(0.0, s + CHEVRON_LEN + w),
                ];
                ground_quad(out, pr, c, [col; 4], true, 2);
            }
        }
    }
}

/// The track's marks through `pr`.
pub(super) fn track_pieces(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    if t.points.len() < 2 {
        return;
    }
    kerbs(out, pr, t);
    start_line(out, pr, t);
    chevrons(out, pr, t);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edge_mitres_round_a_corner() {
        // A square loop run anticlockwise (+X, then +Z, ...): at each
        // corner the left edge sits on the corner's diagonal, the track's
        // half-width from both segments.
        let pts = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let e = edge_at(&pts, 1, 10.0);
        assert!(
            (e[0] - 90.0).abs() < 1e-3 && (e[1] - 10.0).abs() < 1e-3,
            "{e:?}"
        );
        let e = edge_at(&pts, 1, -10.0);
        assert!(
            (e[0] - 110.0).abs() < 1e-3 && (e[1] + 10.0).abs() < 1e-3,
            "{e:?}"
        );
        // A straight run offsets square to it.
        let line = [[0.0, 0.0], [50.0, 0.0], [100.0, 0.0], [50.0, 40.0]];
        let e = edge_at(&line, 1, 10.0);
        assert!(
            (e[0] - 50.0).abs() < 1e-3 && (e[1] - 10.0).abs() < 1e-3,
            "{e:?}"
        );
    }
}
