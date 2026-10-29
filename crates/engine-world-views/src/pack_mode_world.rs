//! The in-world stage of a golf round or a race
//! ([`legaia_engine_expansion::mode_stage`]) as the hosts draw it: the
//! camera shot it frames through, and its world pieces - balls, the cup and
//! flag, the aim arrow, the other seats' bodies, the boost trails - as
//! screen primitives projected through that frame.
//!
//! None of these are retail objects, and none carry disc bytes: each is a
//! small shape built here (a shaded disc for a ball, a ground strip for the
//! arrow, a capsule billboard for a body). Every corner carries its scene
//! depth ([`legaia_render_kernels::screen_prim::CornerDepth`]), so a host
//! depth-tests them against the field it already drew - a ball rolling
//! behind a wall is hidden by it - the same way both hosts draw the
//! overworld markers. The party leader is not here: it is the real player
//! actor, moved by `World::stage_pack_mode` and drawn with the field's
//! actors.

use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_core::world::World;
use legaia_engine_expansion::mode_stage::{StageFrame, StageShot};
use legaia_engine_field::camera::Camera;
use legaia_engine_field::camera_frame::{self, FIELD_CAM_DEPTH, FIELD_H, FieldCameraFrame, to_rad};
use legaia_engine_field::world_map_markers::{DISPLAY_H, DISPLAY_W, depth_toward};
use legaia_render_kernels::screen_prim::{CornerDepth, FlatQuad, ScreenPrim};

/// The ball's radius, world units.
pub const BALL_RADIUS: f32 = 11.0;
/// The cup's radius on the ground, world units.
pub const CUP_RADIUS: f32 = 22.0;
/// The flagpole's height, world units.
pub const FLAG_HEIGHT: f32 = 110.0;
/// An AI body's height and half-width, world units.
pub const BODY_HEIGHT: f32 = 100.0;
pub const BODY_HALF_WIDTH: f32 = 16.0;
/// The aim arrow's half-width, world units.
pub const ARROW_HALF_WIDTH: f32 = 6.0;
/// Segments of a disc.
const DISC_SEGMENTS: usize = 10;
/// The ordering-table bucket the nearest stage piece links at; a piece
/// links `depth`-many buckets past it, so farther pieces draw first.
const STAGE_OT_BASE: u32 = 16;

/// The colour each seat's ball and body take (the top-down screen's).
pub fn seat_color(seat: usize) -> [u8; 4] {
    const C: [[u8; 4]; 8] = [
        [240, 220, 80, 255],
        [90, 170, 250, 255],
        [240, 110, 100, 255],
        [140, 230, 130, 255],
        [210, 130, 240, 255],
        [250, 170, 70, 255],
        [120, 230, 230, 255],
        [230, 230, 230, 255],
    ];
    C[seat % C.len()]
}

/// A stage shot's yaw (the mode angle the camera looks along) as the PSX
/// camera's render yaw: the field camera's eye sits at
/// `focus + (sin yaw, -cos yaw) * depth` on the ground plane and looks back
/// at the focus, so it looks along mode angle `yaw + 1024`.
pub fn shot_yaw_rad(shot_yaw: f32) -> f32 {
    to_rad(shot_yaw - 1024.0)
}

/// The frame a stage shot resolves to: the field camera's `H` and eye
/// depth (scaled by the camera's distance preset and the shot's own pull),
/// looking down at the shot's pitch on the shot's focus on the floor, along
/// the shot's yaw.
pub fn stage_view(world: &World, cam: &Camera, shot: &StageShot) -> FieldCameraView {
    let [fx, fz] = shot.focus;
    let floor = world.sample_field_floor_height(fx as i32, fz as i32) as f32;
    FieldCameraView {
        focus: [fx, floor, fz],
        pitch: to_rad(shot.pitch),
        yaw: shot_yaw_rad(shot.yaw),
        roll: 0.0,
        h: FIELD_H,
        tr_eye: [
            0.0,
            0.0,
            FIELD_CAM_DEPTH * cam.distance.scale() * shot.distance,
        ],
    }
}

/// The stage's camera frame, while the in-world stage owns the frame.
pub fn stage_frame(world: &World, cam: &Camera) -> Option<FieldCameraFrame> {
    let stage = world.pack_mode_stage()?;
    Some(FieldCameraFrame::Venue(stage_view(world, cam, &stage.shot)))
}

/// A projected point: display pixels and scene depth.
#[derive(Clone, Copy)]
struct Pt {
    x: f32,
    y: f32,
    depth: f32,
}

/// The frame's projection, and the world whose floor the pieces stand on.
struct Projector<'a> {
    vp: [f32; 16],
    eye: [f32; 3],
    world: &'a World,
}

impl<'a> Projector<'a> {
    fn new(world: &'a World, frame: &FieldCameraFrame, aabb: ([f32; 3], [f32; 3])) -> Option<Self> {
        Some(Self {
            vp: camera_frame::frame_vp(frame, aabb, DISPLAY_W / DISPLAY_H)?,
            eye: camera_frame::frame_eye(frame)?,
            world,
        })
    }

    /// A world point at height `lift` above the floor under `(x, z)`.
    fn on_floor(&self, x: f32, z: f32, lift: f32) -> [f32; 3] {
        [
            x,
            self.world.sample_field_floor_height(x as i32, z as i32) as f32 - lift,
            z,
        ]
    }

    /// A raw Y-down world point on the display, `None` behind the lens.
    fn at(&self, p: [f32; 3]) -> Option<Pt> {
        let v = [p[0], -p[1], p[2], 1.0];
        let row = |r: usize| (0..4).map(|c| self.vp[c * 4 + r] * v[c]).sum::<f32>();
        let w = row(3);
        if w < 1.0 {
            return None;
        }
        let x = (row(0) / w + 1.0) * 0.5 * DISPLAY_W;
        let y = (1.0 - row(1) / w) * 0.5 * DISPLAY_H;
        if x.abs() > 4096.0 || y.abs() > 4096.0 {
            return None;
        }
        Some(Pt {
            x,
            y,
            depth: depth_toward(&self.vp, p, self.eye)?,
        })
    }

    /// The bucket a piece at `p` links at: one per eight world units of
    /// distance, plus `layer` so a ground piece draws before what stands on
    /// it.
    fn ot(&self, p: [f32; 3], layer: u32) -> u32 {
        let d = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        STAGE_OT_BASE + (dist / 8.0) as u32 * 4 + layer
    }

    /// The direction on screen that is "up" in the world at `p`, unit.
    fn screen_up(&self, p: [f32; 3]) -> Option<(f32, f32)> {
        let a = self.at(p)?;
        let b = self.at([p[0], p[1] - 16.0, p[2]])?;
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        (len > 1e-3).then(|| (dx / len, dy / len))
    }
}

fn quad(c: [Pt; 4], colors: [[u8; 4]; 4], blend: bool, ot: u32) -> ScreenPrim {
    let p = |q: Pt| (q.x.round() as i16, q.y.round() as i16);
    ScreenPrim::Flat(FlatQuad {
        xy: c.map(p),
        color: colors[0],
        gouraud: (colors.iter().any(|&k| k != colors[0])).then_some(colors),
        semi_transparent: blend,
        abr_mode: 0,
        ot_index: ot,
        depth: Some(CornerDepth::new(c.map(|q| q.depth))),
    })
}

fn tri(a: Pt, b: Pt, c: Pt, colors: [[u8; 4]; 3], blend: bool, ot: u32) -> ScreenPrim {
    quad(
        [a, b, c, c],
        [colors[0], colors[1], colors[2], colors[2]],
        blend,
        ot,
    )
}

/// A flat disc on the ground, as a fan of triangles.
fn ground_disc(
    out: &mut Vec<ScreenPrim>,
    pr: &Projector<'_>,
    centre: [f32; 2],
    r: f32,
    color: [u8; 4],
    blend: bool,
    layer: u32,
) {
    let c = pr.on_floor(centre[0], centre[1], 0.5);
    let Some(cp) = pr.at(c) else {
        return;
    };
    let ot = pr.ot(c, layer);
    let rim: Vec<Option<Pt>> = (0..DISC_SEGMENTS)
        .map(|i| {
            let a = i as f32 / DISC_SEGMENTS as f32 * std::f32::consts::TAU;
            pr.at([centre[0] + a.cos() * r, c[1], centre[1] + a.sin() * r])
        })
        .collect();
    for i in 0..DISC_SEGMENTS {
        if let (Some(a), Some(b)) = (rim[i], rim[(i + 1) % DISC_SEGMENTS]) {
            out.push(tri(cp, a, b, [color; 3], blend, ot));
        }
    }
}

/// A ball: a disc facing the camera, bright at the top and shaded at the
/// rim, over its own blob shadow.
fn ball(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, pos: [f32; 2], tint: [u8; 4]) {
    ground_disc(out, pr, pos, BALL_RADIUS * 1.2, [0, 0, 0, 160], true, 3);
    let c = pr.on_floor(pos[0], pos[1], BALL_RADIUS);
    let (Some(cp), Some(top)) = (pr.at(c), pr.at([c[0], c[1] - BALL_RADIUS, c[2]])) else {
        return;
    };
    let r = ((top.x - cp.x).powi(2) + (top.y - cp.y).powi(2))
        .sqrt()
        .max(2.5);
    let ot = pr.ot(c, 0);
    let rim = [tint[0] / 2 + 60, tint[1] / 2 + 60, tint[2] / 2 + 60, 255];
    let hi = [255, 255, 255, 255];
    let centre = Pt {
        x: cp.x - r * 0.25,
        y: cp.y - r * 0.3,
        depth: cp.depth,
    };
    for i in 0..DISC_SEGMENTS {
        let a0 = i as f32 / DISC_SEGMENTS as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / DISC_SEGMENTS as f32 * std::f32::consts::TAU;
        let p = |a: f32| Pt {
            x: cp.x + a.cos() * r,
            y: cp.y + a.sin() * r,
            depth: cp.depth,
        };
        out.push(tri(centre, p(a0), p(a1), [hi, rim, rim], false, ot));
    }
    // The seat's colour as a band, so balls tell apart.
    out.push(quad(
        [
            Pt {
                x: cp.x - r * 0.7,
                y: cp.y + r * 0.15,
                depth: cp.depth,
            },
            Pt {
                x: cp.x + r * 0.7,
                y: cp.y + r * 0.15,
                depth: cp.depth,
            },
            Pt {
                x: cp.x - r * 0.6,
                y: cp.y + r * 0.45,
                depth: cp.depth,
            },
            Pt {
                x: cp.x + r * 0.6,
                y: cp.y + r * 0.45,
                depth: cp.depth,
            },
        ],
        [tint; 4],
        false,
        ot.saturating_sub(1).max(STAGE_OT_BASE),
    ));
}

/// The cup on the ground and its flag.
fn cup(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, pos: [f32; 2]) {
    ground_disc(out, pr, pos, CUP_RADIUS, [8, 8, 8, 255], false, 3);
    ground_disc(
        out,
        pr,
        pos,
        CUP_RADIUS + 4.0,
        [235, 235, 235, 255],
        false,
        4,
    );
    let base = pr.on_floor(pos[0], pos[1], 0.0);
    let top = [base[0], base[1] - FLAG_HEIGHT, base[2]];
    let Some(up) = pr.screen_up(base) else {
        return;
    };
    let (Some(b), Some(t)) = (pr.at(base), pr.at(top)) else {
        return;
    };
    let side = (-up.1, up.0);
    let ot = pr.ot(base, 1);
    let w = 1.0;
    let off = |p: Pt, s: f32| Pt {
        x: p.x + side.0 * s,
        y: p.y + side.1 * s,
        depth: p.depth,
    };
    out.push(quad(
        [off(t, -w), off(t, w), off(b, -w), off(b, w)],
        [[235, 235, 235, 255]; 4],
        false,
        ot,
    ));
    let flag_low = pr.at([top[0], top[1] + FLAG_HEIGHT * 0.25, top[2]]);
    let flag_tip = pr.at([top[0] + 36.0, top[1] + FLAG_HEIGHT * 0.12, top[2]]);
    if let (Some(lo), Some(tip)) = (flag_low, flag_tip) {
        out.push(tri(t, tip, lo, [[220, 40, 40, 255]; 3], false, ot));
    }
}

/// The aim arrow: a strip along the ground from the ball, and its head.
fn arrow(
    out: &mut Vec<ScreenPrim>,
    pr: &Projector<'_>,
    from: [f32; 2],
    angle: u16,
    length: f32,
    power: Option<u32>,
) {
    let a = f32::from(angle) / 4096.0 * std::f32::consts::TAU;
    let (dx, dz) = (a.cos(), a.sin());
    let (nx, nz) = (-dz, dx);
    let steps = 8;
    let head = 18.0;
    let body = (length - head).max(8.0);
    let start = BALL_RADIUS * 2.0;
    let color = |t: f32| match power {
        Some(p) => {
            let k = p.min(100) as f32 / 100.0;
            [255, (230.0 - 170.0 * k * t) as u8, 60, 220]
        }
        None => [255, 255, 255, 200],
    };
    let point = |d: f32, side: f32| {
        let x = from[0] + dx * d + nx * side;
        let z = from[1] + dz * d + nz * side;
        pr.at(pr.on_floor(x, z, 1.5))
    };
    for i in 0..steps {
        let (t0, t1) = (i as f32 / steps as f32, (i + 1) as f32 / steps as f32);
        let (d0, d1) = (start + body * t0, start + body * t1);
        let c = [
            point(d0, -ARROW_HALF_WIDTH),
            point(d0, ARROW_HALF_WIDTH),
            point(d1, -ARROW_HALF_WIDTH),
            point(d1, ARROW_HALF_WIDTH),
        ];
        if let [Some(a), Some(b), Some(c2), Some(d)] = c {
            let mid = pr.on_floor(from[0] + dx * d0, from[1] + dz * d0, 1.5);
            let (k0, k1) = (color(t0), color(t1));
            out.push(quad([a, b, c2, d], [k0, k0, k1, k1], true, pr.ot(mid, 2)));
        }
    }
    let base = start + body;
    if let (Some(l), Some(r), Some(tip)) = (
        point(base, -ARROW_HALF_WIDTH * 2.5),
        point(base, ARROW_HALF_WIDTH * 2.5),
        point(base + head, 0.0),
    ) {
        let mid = pr.on_floor(from[0] + dx * base, from[1] + dz * base, 1.5);
        let k = color(1.0);
        out.push(tri(l, r, tip, [k; 3], true, pr.ot(mid, 2)));
    }
}

/// Another seat's body: a capsule standing on its shadow, in its seat's
/// colour, with a head.
fn body(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, pos: [f32; 2], seat: usize, boosting: bool) {
    ground_disc(out, pr, pos, BODY_HALF_WIDTH * 1.4, [0, 0, 0, 150], true, 3);
    let feet = pr.on_floor(pos[0], pos[1], 0.0);
    let Some(up) = pr.screen_up(feet) else {
        return;
    };
    let (Some(f), Some(n), Some(h)) = (
        pr.at(feet),
        pr.at([feet[0], feet[1] - BODY_HEIGHT * 0.72, feet[2]]),
        pr.at([feet[0], feet[1] - BODY_HEIGHT * 0.86, feet[2]]),
    ) else {
        return;
    };
    let scale = ((n.x - f.x).powi(2) + (n.y - f.y).powi(2)).sqrt() / (BODY_HEIGHT * 0.72);
    let half = BODY_HALF_WIDTH * scale;
    let side = (-up.1, up.0);
    let off = |p: Pt, s: f32| Pt {
        x: p.x + side.0 * s,
        y: p.y + side.1 * s,
        depth: p.depth,
    };
    let ot = pr.ot(feet, 1);
    let tint = seat_color(seat);
    let shade = [tint[0] / 2, tint[1] / 2, tint[2] / 2, 255];
    out.push(quad(
        [
            off(n, -half),
            off(n, half),
            off(f, -half * 0.8),
            off(f, half * 0.8),
        ],
        [tint, tint, shade, shade],
        false,
        ot,
    ));
    // The head: a disc above the shoulders.
    let r = half * 0.8;
    let skin = [236, 200, 160, 255];
    for i in 0..DISC_SEGMENTS {
        let a0 = i as f32 / DISC_SEGMENTS as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / DISC_SEGMENTS as f32 * std::f32::consts::TAU;
        let p = |a: f32| Pt {
            x: h.x + a.cos() * r,
            y: h.y + a.sin() * r,
            depth: h.depth,
        };
        out.push(tri(h, p(a0), p(a1), [skin; 3], false, ot.saturating_sub(1)));
    }
    if boosting {
        ground_disc(
            out,
            pr,
            pos,
            BODY_HALF_WIDTH * 2.0,
            [80, 200, 255, 140],
            true,
            3,
        );
    }
}

/// A boost trail: a fading ribbon through the racer's recent positions.
fn trail(out: &mut Vec<ScreenPrim>, pr: &Projector<'_>, points: &[[f32; 2]]) {
    let n = points.len();
    for i in 0..n.saturating_sub(1) {
        let (a, b) = (points[i], points[i + 1]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dz * dz).sqrt().max(1e-3);
        let (nx, nz) = (-dz / len, dx / len);
        let w0 = 12.0 * (1.0 - i as f32 / n as f32);
        let w1 = 12.0 * (1.0 - (i + 1) as f32 / n as f32);
        let p = |q: [f32; 2], s: f32| pr.at(pr.on_floor(q[0] + nx * s, q[1] + nz * s, 18.0));
        if let (Some(p0), Some(p1), Some(p2), Some(p3)) = (p(a, -w0), p(a, w0), p(b, -w1), p(b, w1))
        {
            let alpha = |t: f32| (200.0 * (1.0 - t)) as u8;
            let k0 = [90, 210, 255, alpha(i as f32 / n as f32)];
            let k1 = [90, 210, 255, alpha((i + 1) as f32 / n as f32)];
            let mid = pr.on_floor(a[0], a[1], 18.0);
            out.push(quad(
                [p0, p1, p2, p3],
                [k0, k0, k1, k1],
                true,
                pr.ot(mid, 2),
            ));
        }
    }
}

/// The stage's world pieces through `frame`. Empty unless the in-world
/// stage owns the frame.
pub fn stage_prims(
    world: &World,
    frame: &FieldCameraFrame,
    aabb: ([f32; 3], [f32; 3]),
) -> Vec<ScreenPrim> {
    let Some(stage) = world.pack_mode_stage() else {
        return Vec::new();
    };
    let Some(pr) = Projector::new(world, frame, aabb) else {
        return Vec::new();
    };
    stage_pieces(&pr, stage)
}

fn stage_pieces(pr: &Projector<'_>, stage: &StageFrame) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    for t in &stage.trails {
        trail(&mut out, pr, &t.points);
    }
    if let Some(c) = stage.cup {
        cup(&mut out, pr, c);
    }
    if let Some(a) = stage.aim {
        arrow(&mut out, pr, a.from, a.angle, a.length, a.power);
    }
    // A body a map NPC draws is the field's actor; only the rest are
    // stand-in figures.
    for b in stage.bodies.iter().filter(|b| b.actor.is_none()) {
        body(&mut out, pr, b.pos, b.seat, b.boosting);
    }
    // The player's ball last, so a shared tee shows it on top.
    let mut balls: Vec<_> = stage.balls.iter().collect();
    balls.sort_by_key(|b| std::cmp::Reverse(b.seat));
    for b in balls {
        let tint = seat_color(b.seat);
        ball(&mut out, pr, b.pos, tint);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_camera::psx_camera::psx_camera_eye;

    #[test]
    fn a_shot_looks_down_its_yaw() {
        // For each mode angle the eye sits behind the focus, opposite the
        // look direction.
        for m in [0.0f32, 512.0, 1024.0, 2048.0, 3000.0] {
            let eye = psx_camera_eye(
                to_rad(legaia_engine_field::camera_frame::FIELD_PITCH_UNITS),
                shot_yaw_rad(m),
                0.0,
                [0.0, 0.0, 1000.0],
                [0.0, 0.0, 0.0],
            );
            let a = m / 4096.0 * std::f32::consts::TAU;
            let (lx, lz) = (a.cos(), a.sin());
            // Eye -> focus points along the look direction.
            let (dx, dz) = (-eye[0], -eye[2]);
            let len = (dx * dx + dz * dz).sqrt();
            let dot = (dx * lx + dz * lz) / len;
            assert!(dot > 0.999, "angle {m}: dot {dot}");
            assert!(eye[1] < 0.0, "the eye is above the focus (Y down)");
        }
    }
}
