//! A golf hole laid on the ground
//! ([`legaia_engine_expansion::mode_stage::StageCourse`]): every playable
//! tile tinted by its surface (fairway mown in a checker, darker
//! rough, sand, water), a white line round the hole's edge where play
//! ends, chevrons down each sloped tile, the tee's mat and markers, and a
//! lighter putting green round the cup. The tints blend over the map's own
//! floor, so the hole reads as a course laid on the scene rather than a
//! board pasted over it.

use std::collections::HashSet;

use legaia_engine_expansion::mode_stage::StageCourse;
use legaia_engine_modes::golf::course::Surface;
use legaia_render_kernels::screen_prim::ScreenPrim;

use super::track::ground_quad;
use super::{GROUND_BAND, Projector, ground_disc_ot};

/// The edge line's width, world units.
const EDGE_W: f32 = 5.0;
const EDGE: [u8; 4] = [236, 236, 228, 255];

/// A surface's tint, and the lighter stripe of a mown fairway.
fn tint(s: Surface, stripe: bool) -> [u8; 4] {
    match s {
        Surface::Fairway if stripe => [96, 214, 96, 255],
        Surface::Fairway => [70, 186, 76, 255],
        Surface::Rough => [36, 112, 46, 255],
        Surface::Sand => [226, 200, 134, 255],
        Surface::Water => [40, 116, 224, 255],
        Surface::Wall | Surface::OutOfBounds => [0, 0, 0, 255],
    }
}

/// The hole's pieces through `pr`.
pub(super) fn course_pieces(
    out: &mut Vec<ScreenPrim>,
    pr: &Projector<'_>,
    c: &StageCourse,
    cup: Option<[f32; 2]>,
) {
    let s = c.tile;
    if s <= 0.0 {
        return;
    }
    let key = |at: [f32; 2]| ((at[0] / s).round() as i32, (at[1] / s).round() as i32);
    let open: HashSet<(i32, i32)> = c.tiles.iter().map(|t| key(t.at)).collect();
    for t in &c.tiles {
        let (kx, kz) = key(t.at);
        let [x0, z0] = t.at;
        let (x1, z1) = (x0 + s, z0 + s);
        // The fairway is mown in a checker, a tile a square.
        let k = tint(t.surface, (kx + kz).rem_euclid(2) == 0);
        ground_quad(
            out,
            pr,
            [[x0, z0], [x1, z0], [x0, z1], [x1, z1]],
            [k; 4],
            true,
            6,
        );
        // The edge line where the neighbour is out of play.
        let w = EDGE_W;
        for (dx, dz, rect) in [
            (-1, 0, [[x0, z0], [x0 + w, z0], [x0, z1], [x0 + w, z1]]),
            (1, 0, [[x1 - w, z0], [x1, z0], [x1 - w, z1], [x1, z1]]),
            (0, -1, [[x0, z0], [x1, z0], [x0, z0 + w], [x1, z0 + w]]),
            (0, 1, [[x0, z1 - w], [x1, z1 - w], [x0, z1], [x1, z1]]),
        ] {
            if !open.contains(&(kx + dx, kz + dz)) {
                ground_quad(out, pr, rect, [EDGE; 4], false, 5);
            }
        }
        // A sloped tile: a chevron pointing downhill.
        if t.slope != (0, 0) {
            let (sx, sz) = (f32::from(t.slope.0), f32::from(t.slope.1));
            let l = (sx * sx + sz * sz).sqrt();
            let (dx, dz) = (sx / l, sz / l);
            let (nx, nz) = (-dz, dx);
            let (cx, cz) = (x0 + s * 0.5, z0 + s * 0.5);
            let at = |along: f32, across: f32| {
                [
                    cx + dx * along * s + nx * across * s,
                    cz + dz * along * s + nz * across * s,
                ]
            };
            let k = [255, 255, 255, 255];
            for side in [1.0f32, -1.0] {
                ground_quad(
                    out,
                    pr,
                    [
                        at(-0.18, side * 0.26),
                        at(-0.08, side * 0.26),
                        at(0.16, 0.0),
                        at(0.26, 0.0),
                    ],
                    [k; 4],
                    true,
                    4,
                );
            }
        }
    }
    // The putting green round the cup.
    if let Some(cup) = cup {
        ground_disc_ot(
            out,
            pr,
            cup,
            s * 0.9,
            [150, 236, 140, 255],
            true,
            GROUND_BAND + 5,
        );
    }
    // The tee: a darker mat and two red markers either side.
    let [tx, tz] = c.tee;
    let m = s * 0.3;
    ground_quad(
        out,
        pr,
        [
            [tx - m, tz - m],
            [tx + m, tz - m],
            [tx - m, tz + m],
            [tx + m, tz + m],
        ],
        [[40, 120, 52, 255]; 4],
        false,
        5,
    );
    for d in [-1.0f32, 1.0] {
        ground_disc_ot(
            out,
            pr,
            [tx + d * s * 0.36, tz],
            5.0,
            [220, 50, 44, 255],
            false,
            GROUND_BAND + 3,
        );
    }
}
