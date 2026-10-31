//! A race's track marked out on the ground
//! ([`legaia_engine_expansion::mode_stage::StageTrack`]): red-and-white
//! kerbs along both edges, a chequered start line across the first
//! segment (and a finish line across a sprint's last point), cyan chevrons
//! down every boost segment pointing the way, a gate standing over every
//! checkpoint - the next one the leader must pass in gold, the ones a
//! sprint has left behind dimmed - and the power-ups floating over their
//! spots. The track is the session's centerline and half-width, so the
//! marks show exactly where a racer is held, whatever the ground under
//! them; every mark follows the floor (a kingdom overworld's heightfield
//! included).

use legaia_engine_expansion::mode_stage::{StageHop, StagePickup, StageTrack};
use legaia_engine_modes::racing::PowerKind;
use legaia_render_kernels::screen_prim::ScreenPrim;

use super::{GROUND_BAND, Projector, Pt, ground_disc, quad, tri};

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
/// A gate's post height and its banner's depth, world units.
const GATE_HEIGHT: f32 = 150.0;
const GATE_BANNER: f32 = 26.0;
/// A post's half-width on screen, pixels.
const POST_HALF: f32 = 1.5;
const GATE_NEXT: [u8; 4] = [255, 206, 60, 255];
const GATE_AHEAD: [u8; 4] = [226, 232, 244, 255];
const GATE_PASSED: [u8; 4] = [96, 100, 112, 255];
/// A power-up's float height over its spot, and its gem's half-size, world
/// units.
const PICKUP_FLOAT: f32 = 64.0;
const PICKUP_GEM: f32 = 30.0;

/// A power-up's colour.
pub fn power_color(kind: PowerKind) -> [u8; 4] {
    match kind {
        PowerKind::Boost => [80, 200, 255, 255],
        PowerKind::Hop => [120, 236, 110, 255],
        PowerKind::Shield => [255, 210, 70, 255],
        PowerKind::Slow => [200, 110, 255, 255],
    }
}

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
/// half-width at a sharp corner. An open course's ends take their one
/// segment's offset.
fn edge_at(points: &[[f32; 2]], closed: bool, i: usize, off: f32) -> [f32; 2] {
    let n = points.len();
    let p = points[i];
    let d_out = if !closed && i + 1 == n {
        unit(sub(p, points[i - 1]))
    } else {
        unit(sub(points[(i + 1) % n], p))
    };
    let d_in = if !closed && i == 0 {
        d_out
    } else {
        unit(sub(p, points[(i + n - 1) % n]))
    };
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
        for i in 0..segments(t) {
            let j = (i + 1) % n;
            let outer = (
                edge_at(&t.points, t.closed, i, side * (hw + KERB_W * 0.5)),
                edge_at(&t.points, t.closed, j, side * (hw + KERB_W * 0.5)),
            );
            let inner = (
                edge_at(&t.points, t.closed, i, side * (hw - KERB_W * 0.5)),
                edge_at(&t.points, t.closed, j, side * (hw - KERB_W * 0.5)),
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

/// The segments a track has: one per point on a circuit, one fewer on a
/// sprint.
fn segments(t: &StageTrack) -> usize {
    if t.closed {
        t.points.len()
    } else {
        t.points.len().saturating_sub(1)
    }
}

/// A chequered line across the track at point `at`, square to `toward`.
fn chequer_line(
    out: &mut Vec<ScreenPrim>,
    pr: &Projector<'_>,
    t: &StageTrack,
    at_point: usize,
    toward: usize,
) {
    let p = t.points[at_point];
    let d = unit(sub(t.points[toward], p));
    let d = if toward < at_point { [-d[0], -d[1]] } else { d };
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

/// The chequered start line across the start of segment `0`, and a
/// sprint's finish line across its last point.
fn start_line(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    let n = t.points.len();
    if n < 2 {
        return;
    }
    chequer_line(out, pr, t, 0, 1);
    if !t.closed {
        chequer_line(out, pr, t, n - 1, n - 2);
    }
}

/// A vertical post from `base` up `height`, `half` pixels each side.
fn post(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, base: [f32; 3], height: f32, col: [u8; 4]) {
    let top = [base[0], base[1] - height, base[2]];
    let (Some(up), Some(b), Some(tp)) = (pr.screen_up(base), pr.at(base), pr.at(top)) else {
        return;
    };
    let side = (-up.1, up.0);
    let off = |p: Pt, s: f32| Pt {
        x: p.x + side.0 * s,
        y: p.y + side.1 * s,
        depth: p.depth,
    };
    let lo = [col[0] / 3 * 2, col[1] / 3 * 2, col[2] / 3 * 2, 255];
    out.push(quad(
        [
            off(tp, -POST_HALF),
            off(tp, POST_HALF),
            off(b, -POST_HALF),
            off(b, POST_HALF),
        ],
        [col, col, lo, lo],
        false,
        pr.ot(base, 1),
    ));
}

/// The gates: a pair of posts either side of the track over each
/// checkpoint and a banner between their tops. The start (and a sprint's
/// finish) carry the chequered lines instead of a ground stripe.
fn gates(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    let n = t.points.len();
    let off = t.half_width + KERB_W;
    for i in 0..n {
        let col = match t.next_gate {
            Some(g) if g == i => GATE_NEXT,
            Some(g) if !t.closed && i < g => GATE_PASSED,
            None if !t.closed => GATE_PASSED,
            _ => GATE_AHEAD,
        };
        let l = edge_at(&t.points, t.closed, i, off);
        let r = edge_at(&t.points, t.closed, i, -off);
        let (lb, rb) = (pr.on_floor(l[0], l[1], 0.0), pr.on_floor(r[0], r[1], 0.0));
        post(out, pr, lb, GATE_HEIGHT, col);
        post(out, pr, rb, GATE_HEIGHT, col);
        // The banner: a strip between the post tops, its lower edge a
        // banner's depth down. Both posts' tops stand on their own floor.
        let top = |b: [f32; 3], k: f32| [b[0], b[1] - GATE_HEIGHT + k, b[2]];
        let c = [
            pr.at(top(lb, 0.0)),
            pr.at(top(rb, 0.0)),
            pr.at(top(lb, GATE_BANNER)),
            pr.at(top(rb, GATE_BANNER)),
        ];
        if let [Some(a), Some(b), Some(cc), Some(d)] = c {
            let lo = [col[0] / 4 * 3, col[1] / 4 * 3, col[2] / 4 * 3, 255];
            let mid = [
                (lb[0] + rb[0]) * 0.5,
                (lb[1] + rb[1]) * 0.5 - GATE_HEIGHT,
                (lb[2] + rb[2]) * 0.5,
            ];
            out.push(quad(
                [a, b, cc, d],
                [col, col, lo, lo],
                false,
                pr.ot(mid, 0),
            ));
        }
        // The next gate also marks its line on the ground.
        let is_line = i == 0 || (!t.closed && i + 1 == n);
        if t.next_gate == Some(i) && !is_line {
            let li = edge_at(&t.points, t.closed, i, t.half_width);
            let ri = edge_at(&t.points, t.closed, i, -t.half_width);
            let d = unit(sub(ri, li));
            let a = [-d[1] * 6.0, d[0] * 6.0];
            let glow = [col[0], col[1], col[2], 170];
            ground_quad(
                out,
                pr,
                [
                    [li[0] - a[0], li[1] - a[1]],
                    [ri[0] - a[0], ri[1] - a[1]],
                    [li[0] + a[0], li[1] + a[1]],
                    [ri[0] + a[0], ri[1] + a[1]],
                ],
                [glow; 4],
                true,
                2,
            );
        }
    }
}

/// One power-up: a coloured ring on the ground and a gem floating and
/// turning over it, shaped by kind - a diamond for a boost, an arrowhead
/// for a hop, a square for a shield, an hourglass for a slow. A spot
/// waiting to come back shows its ring faintly and no gem.
fn pickup(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, p: &StagePickup, clock: u32, k: usize) {
    let col = power_color(p.kind);
    if !p.ready {
        ground_disc(
            out,
            pr,
            p.pos,
            PICKUP_GEM * 1.4,
            [col[0], col[1], col[2], 60],
            true,
            3,
        );
        return;
    }
    ground_disc(
        out,
        pr,
        p.pos,
        PICKUP_GEM * 1.6,
        [col[0], col[1], col[2], 150],
        true,
        3,
    );
    let phase = clock as f32 * 0.07 + k as f32 * 1.7;
    let bob = phase.sin() * 6.0;
    let c = pr.on_floor(p.pos[0], p.pos[1], PICKUP_FLOAT + bob);
    let (Some(cp), Some(top)) = (pr.at(c), pr.at([c[0], c[1] - PICKUP_GEM, c[2]])) else {
        return;
    };
    let r = ((top.x - cp.x).powi(2) + (top.y - cp.y).powi(2))
        .sqrt()
        .max(3.0);
    // The turn: the gem's width breathes with the cosine of its spin.
    let w = r * (0.35 + 0.65 * phase.cos().abs());
    let at = |x: f32, y: f32| Pt {
        x: cp.x + x,
        y: cp.y + y,
        depth: cp.depth,
    };
    let hi = [
        (col[0] as u16 / 2 + 128).min(255) as u8,
        (col[1] as u16 / 2 + 128).min(255) as u8,
        (col[2] as u16 / 2 + 128).min(255) as u8,
        255,
    ];
    let lo = [col[0] / 2, col[1] / 2, col[2] / 2, 255];
    let ot = pr.ot(c, 0);
    match p.kind {
        PowerKind::Boost => {
            out.push(tri(
                at(0.0, -r),
                at(-w, 0.0),
                at(w, 0.0),
                [hi, col, col],
                false,
                ot,
            ));
            out.push(tri(
                at(-w, 0.0),
                at(w, 0.0),
                at(0.0, r),
                [col, col, lo],
                false,
                ot,
            ));
        }
        PowerKind::Hop => {
            out.push(tri(
                at(0.0, -r),
                at(-w, r * 0.6),
                at(0.0, r * 0.15),
                [hi, col, lo],
                false,
                ot,
            ));
            out.push(tri(
                at(0.0, -r),
                at(w, r * 0.6),
                at(0.0, r * 0.15),
                [hi, col, lo],
                false,
                ot,
            ));
        }
        PowerKind::Shield => {
            out.push(quad(
                [
                    at(-w, -r * 0.8),
                    at(w, -r * 0.8),
                    at(-w * 0.8, r),
                    at(w * 0.8, r),
                ],
                [hi, hi, lo, lo],
                false,
                ot,
            ));
        }
        PowerKind::Slow => {
            out.push(tri(
                at(-w, -r),
                at(w, -r),
                at(0.0, 0.0),
                [hi, hi, col],
                false,
                ot,
            ));
            out.push(tri(
                at(0.0, 0.0),
                at(-w, r),
                at(w, r),
                [col, lo, lo],
                false,
                ot,
            ));
        }
    }
}

/// Chevrons down each boost segment, pointing along it.
fn chevrons(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, t: &StageTrack) {
    let n = t.points.len();
    let hw = t.half_width * 0.45;
    for &seg in &t.boost_segments {
        if seg >= segments(t) {
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
    gates(out, pr, t);
    for (k, p) in t.pickups.iter().enumerate() {
        pickup(out, pr, p, t.clock, k);
    }
    for h in &t.hops {
        hop_arc(out, pr, h);
    }
}

/// A hop's leap: a dotted arc from takeoff to landing over its ground
/// path - the dots behind the racer dimmed - and a ring where it lands.
fn hop_arc(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, h: &StageHop) {
    let col = power_color(PowerKind::Hop);
    let n = h.arc.len().max(2) as f32 - 1.0;
    for (i, p) in h.arc.iter().enumerate() {
        let behind = (i as f32 / n) < h.done;
        let a = if behind { 90 } else { 230 };
        let c = pr.on_floor(p[0], p[1], p[2]);
        let Some(cp) = pr.at(c) else {
            continue;
        };
        let r = 2.5;
        let at = |x: f32, y: f32| Pt {
            x: cp.x + x,
            y: cp.y + y,
            depth: cp.depth,
        };
        out.push(quad(
            [at(-r, -r), at(r, -r), at(-r, r), at(r, r)],
            [[col[0], col[1], col[2], a]; 4],
            true,
            pr.ot(c, 0),
        ));
    }
    if let Some(end) = h.arc.last() {
        ground_disc(
            out,
            pr,
            [end[0], end[1]],
            26.0,
            [col[0], col[1], col[2], 110],
            true,
            3,
        );
    }
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
        let e = edge_at(&pts, true, 1, 10.0);
        assert!(
            (e[0] - 90.0).abs() < 1e-3 && (e[1] - 10.0).abs() < 1e-3,
            "{e:?}"
        );
        let e = edge_at(&pts, true, 1, -10.0);
        assert!(
            (e[0] - 110.0).abs() < 1e-3 && (e[1] + 10.0).abs() < 1e-3,
            "{e:?}"
        );
        // A straight run offsets square to it.
        let line = [[0.0, 0.0], [50.0, 0.0], [100.0, 0.0], [50.0, 40.0]];
        let e = edge_at(&line, true, 1, 10.0);
        assert!(
            (e[0] - 50.0).abs() < 1e-3 && (e[1] - 10.0).abs() < 1e-3,
            "{e:?}"
        );
    }

    #[test]
    fn an_open_course_ends_square_to_its_one_segment() {
        let line = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]];
        let e = edge_at(&line, false, 0, 10.0);
        assert!((e[0]).abs() < 1e-3 && (e[1] - 10.0).abs() < 1e-3, "{e:?}");
        let e = edge_at(&line, false, 2, 10.0);
        assert!(
            (e[0] - 90.0).abs() < 1e-3 && (e[1] - 100.0).abs() < 1e-3,
            "{e:?}"
        );
    }
}
