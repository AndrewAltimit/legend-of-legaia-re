//! A party night's podium in the world
//! ([`legaia_engine_expansion::podium_stage`]): the three blocks as shaded
//! boxes standing on the floor, the stand-in figures of rivals with no
//! body NPC on their tops, and the confetti and sparkles as camera-facing
//! flecks. Every corner carries its scene depth, so the field's own walls
//! hide what stands behind them.

use legaia_engine_expansion::mode_stage::StageFrame;
use legaia_engine_expansion::podium_stage::{PodiumBlock, PodiumFrame};
use legaia_render_kernels::screen_prim::ScreenPrim;

use super::{Projector, Pt, body, quad};

fn shade(c: [u8; 4], k: f32) -> [u8; 4] {
    let s = |v: u8| (f32::from(v) * k).clamp(0.0, 255.0) as u8;
    [s(c[0]), s(c[1]), s(c[2]), c[3]]
}

/// One block: four sides and the top, lit from the camera's upper left.
fn block(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, b: &PodiumBlock) {
    if b.height < 1.0 {
        return;
    }
    // The floor sampler stands on the block's top: its base is `height`
    // below what the centre samples (Y down).
    let top_y = pr.on_floor(b.centre[0], b.centre[1], 0.0)[1];
    let base_y = top_y + b.height;
    let a = f32::from(b.facing) / 4096.0 * std::f32::consts::TAU;
    let (fx, fz) = (a.cos(), a.sin());
    let (rx, rz) = (-fz, fx);
    let h = b.half;
    let corner = |s: f32, t: f32, y: f32| {
        [
            b.centre[0] + rx * h * s + fx * h * t,
            y,
            b.centre[1] + rz * h * s + fz * h * t,
        ]
    };
    let pts = |c: [[f32; 3]; 4]| -> Option<[Pt; 4]> {
        Some([pr.at(c[0])?, pr.at(c[1])?, pr.at(c[2])?, pr.at(c[3])?])
    };
    let mid = [b.centre[0], (top_y + base_y) * 0.5, b.centre[1]];
    let ot = pr.ot(mid, 1);
    // (s0, t0, s1, t1, light): each side's two ground corners.
    let sides = [
        (-1.0, 1.0, 1.0, 1.0, 1.0),    // front, toward the camera
        (1.0, 1.0, 1.0, -1.0, 0.72),   // right
        (-1.0, -1.0, -1.0, 1.0, 0.86), // left
        (1.0, -1.0, -1.0, -1.0, 0.6),  // back
    ];
    for (s0, t0, s1, t1, k) in sides {
        let c = [
            corner(s0, t0, top_y),
            corner(s1, t1, top_y),
            corner(s0, t0, base_y),
            corner(s1, t1, base_y),
        ];
        if let Some(p) = pts(c) {
            let hi = shade(b.fill, k);
            let lo = shade(b.fill, k * 0.62);
            out.push(quad(p, [hi, hi, lo, lo], false, ot));
        }
    }
    // The top, its rim colour, and a darker inset so it reads as a step.
    let c = [
        corner(-1.0, -1.0, top_y),
        corner(1.0, -1.0, top_y),
        corner(-1.0, 1.0, top_y),
        corner(1.0, 1.0, top_y),
    ];
    if let Some(p) = pts(c) {
        out.push(quad(p, [b.rim; 4], false, ot.saturating_sub(1)));
    }
    let k = 0.78;
    let c = [
        corner(-k, -k, top_y - 0.5),
        corner(k, -k, top_y - 0.5),
        corner(-k, k, top_y - 0.5),
        corner(k, k, top_y - 0.5),
    ];
    if let Some(p) = pts(c) {
        out.push(quad(
            p,
            [shade(b.fill, 1.08); 4],
            false,
            ot.saturating_sub(2),
        ));
    }
    // A stripe of the rim round the front face's top.
    let c = [
        corner(-1.0, 1.0, top_y),
        corner(1.0, 1.0, top_y),
        corner(-1.0, 1.0, top_y + 10.0),
        corner(1.0, 1.0, top_y + 10.0),
    ];
    if let Some(p) = pts(c) {
        out.push(quad(p, [b.rim; 4], false, ot.saturating_sub(1)));
    }
}

/// A fleck at world `p` (Y down), `size` world units, facing the camera;
/// `squash` narrows it as it turns.
fn fleck(
    out: &mut Vec<ScreenPrim>,
    pr: &Projector<'_>,
    p: [f32; 3],
    size: f32,
    squash: f32,
    color: [u8; 4],
    blend: bool,
) {
    let (Some(c), Some(t)) = (pr.at(p), pr.at([p[0], p[1] - size, p[2]])) else {
        return;
    };
    let r = ((t.x - c.x).powi(2) + (t.y - c.y).powi(2)).sqrt().max(1.0) * 0.5;
    let w = r * squash.max(0.15);
    let q = |dx: f32, dy: f32| Pt {
        x: c.x + dx,
        y: c.y + dy,
        depth: c.depth,
    };
    out.push(quad(
        [q(-w, -r), q(w, -r), q(-w, r), q(w, r)],
        [color; 4],
        blend,
        pr.ot(p, 0),
    ));
}

pub(super) fn podium_pieces(
    pr: &Projector<'_>,
    stage: &StageFrame,
    pod: &PodiumFrame,
) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    for b in &pod.blocks {
        block(&mut out, pr, b);
    }
    for b in stage.bodies.iter().filter(|b| b.actor.is_none()) {
        body(&mut out, pr, b.pos, b.seat, false);
    }
    for c in &pod.confetti {
        let floor = pr.on_floor(c.pos[0], c.pos[2], 0.0)[1];
        let squash = (c.turn * std::f32::consts::TAU).cos().abs();
        fleck(
            &mut out,
            pr,
            [c.pos[0], floor - c.pos[1], c.pos[2]],
            c.size,
            squash,
            c.color,
            false,
        );
    }
    for s in &pod.sparkles {
        let floor = pr.on_floor(s[0], s[2], 0.0)[1];
        let p = [s[0], floor - s[1] + pod_lift(pod, s), s[2]];
        fleck(&mut out, pr, p, 9.0, 1.0, [255, 246, 200, 200], true);
    }
    out
}

/// The sparkles' heights are over the block's top already (the floor the
/// sampler answers there): take the block back out.
fn pod_lift(pod: &PodiumFrame, s: &[f32; 3]) -> f32 {
    pod.figures
        .iter()
        .find(|f| f.cheer)
        .filter(|f| (f.pos[0] - s[0]).abs() < 80.0 && (f.pos[1] - s[2]).abs() < 80.0)
        .map_or(0.0, |f| f.lift)
}
