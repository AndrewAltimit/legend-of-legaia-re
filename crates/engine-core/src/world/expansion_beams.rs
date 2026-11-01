//! A pack map's light beams (`[[beams]]`,
//! [`legaia_expansion_format::map_beams`]) as screen primitives.
//!
//! An enhancement: the hosts ask for these only with enhanced lighting on
//! (`HostView::beam_prims`), so the faithful mode draws nothing of them. A
//! beam is a camera-facing ribbon round the shaft's axis, a grid of
//! additive (ABR 1) Gouraud quads whose corners carry the shaft's soft
//! falloff - dark at the edges, the profile along its length - and the
//! scene depth of the axis point they stand on, so the scene in front of a
//! beam hides it. A `fall` beam lays a soft pool on the floor at its foot;
//! its dust motes are the particle sheet's soft dot, additive. No state:
//! the shaft, the breath and every mote are pure functions of the frame.

use legaia_engine_field::fog_particles::FogView;
use legaia_expansion_format::map_beams::{BeamDef, BeamProfile};
use legaia_expansion_format::map_detail::{ParticleSheet, SPRITE_DOT};
use legaia_render_kernels::screen_prim::{CornerDepth, FlatQuad, ScreenPrim, fog_puff_prim};

use super::World;

/// Rings along the shaft, top (`0`) to foot (`1`).
const RINGS: [f32; 6] = [0.0, 0.12, 0.3, 0.55, 0.8, 1.0];
/// Columns across the shaft and each one's share of the centre's light: a
/// soft-edged column, dark at both sides.
const COLUMNS: [(f32, f32); 5] = [
    (-0.5, 0.0),
    (-0.22, 0.55),
    (0.0, 1.0),
    (0.22, 0.55),
    (0.5, 0.0),
];

/// A projected axis point: screen position, view depth, scene depth and
/// the screen pixels one world unit spans there.
#[derive(Clone, Copy)]
struct Ring {
    xy: (f32, f32),
    sz: i32,
    depth: f32,
    px_per_unit: f32,
}

fn project_ring(view: &FogView, p: [i32; 3]) -> Option<Ring> {
    let (sx, sy, sz) = view.project(p)?;
    let depth = view.ndc_depth(p)?;
    // A world offset of 64 along X and along Z, projected: the larger is
    // the ribbon's scale there whichever way the camera faces.
    let span = [[64, 0, 0], [0, 0, 64]]
        .iter()
        .filter_map(|d| view.project([p[0] + d[0], p[1], p[2] + d[2]]))
        .map(|(ax, ay, _)| {
            let (dx, dy) = ((ax - sx) as f32, (ay - sy) as f32);
            (dx * dx + dy * dy).sqrt()
        })
        .fold(0.0f32, f32::max);
    Some(Ring {
        xy: (sx as f32, sy as f32),
        sz,
        depth,
        px_per_unit: span / 64.0,
    })
}

/// A ribbon corner: screen position, colour, scene depth and view depth.
type Corner = ((i16, i16), [u8; 4], f32, i32);
/// A pool point: screen position, scene depth and view depth.
type PoolPoint = ((i16, i16), f32, i32);

fn px(v: f32) -> i16 {
    (v.round() as i32).clamp(-1024, 1023) as i16
}

fn scaled(color: [u8; 3], k: f32) -> [u8; 4] {
    let c = |v: u8| (f32::from(v) * k).round().clamp(0.0, 255.0) as u8;
    [c(color[0]), c(color[1]), c(color[2]), 255]
}

fn additive(xy: [(i16, i16); 4], colors: [[u8; 4]; 4], ot: u32, depth: [f32; 4]) -> ScreenPrim {
    ScreenPrim::Flat(FlatQuad {
        xy,
        color: colors[0],
        gouraud: Some(colors),
        semi_transparent: true,
        abr_mode: 1,
        ot_index: ot,
        depth: Some(CornerDepth::new(depth)),
    })
}

fn on_screen(xy: &[(i16, i16)]) -> bool {
    let (x0, x1) = xy
        .iter()
        .fold((i16::MAX, i16::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (y0, y1) = xy
        .iter()
        .fold((i16::MAX, i16::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    x1 >= -16 && x0 <= 336 && y1 >= -16 && y0 <= 256
}

/// One beam's primitives through `view`; `floor` is the Y-down floor under
/// its tile.
pub fn beam_prims(
    b: &BeamDef,
    frame: u64,
    centre: (i32, i32),
    floor: i32,
    view: &FogView,
    sheet: Option<ParticleSheet>,
) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    if b.intensity == 0 || b.color == [0, 0, 0] {
        return out;
    }
    let s = b.shaft(frame, centre);
    let peak = f32::from(b.intensity) / 255.0 * s.gain;
    // The axis point at `t` (`0` top, `1` foot), Y-down world.
    let axis = |t: f32| -> [i32; 3] {
        let up = 1.0 - t;
        [
            (s.foot[0] + s.axis[0] * up).round() as i32,
            floor - (s.foot[1] + s.axis[1] * up).round() as i32,
            (s.foot[2] + s.axis[2] * up).round() as i32,
        ]
    };
    let rings: Vec<Option<Ring>> = RINGS.iter().map(|&t| project_ring(view, axis(t))).collect();
    let (Some(top), Some(foot)) = (rings[0], rings[RINGS.len() - 1]) else {
        return out;
    };
    // Across the ribbon on screen: perpendicular to the projected axis.
    let (dx, dy) = (top.xy.0 - foot.xy.0, top.xy.1 - foot.xy.1);
    let len = (dx * dx + dy * dy).sqrt();
    let across = if len > 1.0 {
        (-dy / len, dx / len)
    } else {
        (1.0, 0.0)
    };
    let width_at = |t: f32| s.width[0] + (s.width[1] - s.width[0]) * t;
    let corner = |ri: usize, cj: usize| -> Option<Corner> {
        let r = rings[ri]?;
        let t = RINGS[ri];
        let (off, share) = COLUMNS[cj];
        let half = width_at(t) * r.px_per_unit;
        let xy = (
            px(r.xy.0 + across.0 * off * half),
            px(r.xy.1 + across.1 * off * half),
        );
        Some((
            xy,
            scaled(b.color, peak * share * b.profile_at(t)),
            r.depth,
            r.sz,
        ))
    };
    for ri in 0..RINGS.len() - 1 {
        for cj in 0..COLUMNS.len() - 1 {
            let (Some(a), Some(bb), Some(c), Some(d)) = (
                corner(ri, cj),
                corner(ri, cj + 1),
                corner(ri + 1, cj),
                corner(ri + 1, cj + 1),
            ) else {
                continue;
            };
            let xy = [a.0, bb.0, c.0, d.0];
            if !on_screen(&xy) || [a.1, bb.1, c.1, d.1].iter().all(|k| k[..3] == [0, 0, 0]) {
                continue;
            }
            let sz = a.3.max(c.3).max(0) as u32;
            out.push(additive(
                xy,
                [a.1, bb.1, c.1, d.1],
                sz >> 5,
                [a.2, bb.2, c.2, d.2],
            ));
        }
    }
    // The pool: a soft diamond of light on the floor round the foot.
    if b.pool && b.profile == BeamProfile::Fall {
        let r = (s.width[1] * 0.9) as i32;
        let y = floor - s.foot[1].round() as i32 - 2;
        let (fx, fz) = (s.foot[0].round() as i32, s.foot[2].round() as i32);
        let pts: Vec<Option<PoolPoint>> = [
            [0, 0],
            [r, 0],
            [0, r],
            [-r, 0],
            [0, -r],
            [r * 7 / 10, r * 7 / 10],
            [-r * 7 / 10, r * 7 / 10],
            [-r * 7 / 10, -r * 7 / 10],
            [r * 7 / 10, -r * 7 / 10],
        ]
        .iter()
        .map(|o| {
            let p = [fx + o[0], y, fz + o[1]];
            let (sx, sy, sz) = view.project(p)?;
            Some(((sx as i16, sy as i16), view.ndc_depth(p)?, sz))
        })
        .collect();
        let bright = scaled(b.color, peak * 0.55 * b.profile_at(1.0));
        let dark = [0, 0, 0, 255];
        // Four quads: centre, a rim point, the diagonal, the next rim point.
        for (rim, diag, next) in [(1, 5, 2), (2, 6, 3), (3, 7, 4), (4, 8, 1)] {
            let (Some(c), Some(a), Some(d), Some(n)) = (pts[0], pts[rim], pts[diag], pts[next])
            else {
                continue;
            };
            let xy = [c.0, a.0, n.0, d.0];
            if !on_screen(&xy) {
                continue;
            }
            let sz = c.2.max(0) as u32;
            out.push(additive(
                xy,
                [bright, dark, dark, dark],
                sz >> 5,
                [c.1, a.1, n.1, d.1],
            ));
        }
    }
    // The dust: soft dots drifting in the shaft.
    if let Some(sheet) = sheet {
        let su = sheet.u0 + SPRITE_DOT * 16;
        for i in 0..b.motes {
            let Some(m) = b.mote(i, frame) else {
                break;
            };
            let p = axis(m.along);
            let Some(r) = project_ring(view, p) else {
                continue;
            };
            let half = width_at(m.along) * r.px_per_unit;
            let (cx, cy) = (
                r.xy.0 + across.0 * m.across * half,
                r.xy.1 + across.1 * m.across * half,
            );
            let k = (m.light * (0.5 + peak)).min(1.0);
            let rgb = scaled(b.color, k);
            if rgb[..3] == [0, 0, 0] {
                continue;
            }
            let rad = (m.size * 0.5 * r.px_per_unit).clamp(1.0, 8.0);
            let xy = [
                (px(cx - rad), px(cy - rad)),
                (px(cx + rad), px(cy - rad)),
                (px(cx - rad), px(cy + rad)),
                (px(cx + rad), px(cy + rad)),
            ];
            if !on_screen(&xy) {
                continue;
            }
            out.push(fog_puff_prim(
                xy,
                [
                    (su, sheet.v0),
                    (su + 15, sheet.v0),
                    (su, sheet.v0 + 15),
                    (su + 15, sheet.v0 + 15),
                ],
                sheet.clut_glow,
                sheet.tpage | (1 << 5),
                [rgb[0], rgb[1], rgb[2]],
                (r.sz.max(0) as u32) >> 5,
                Some(r.depth),
            ));
        }
    }
    out
}

impl World {
    /// This frame's pack-map light beams through `view`. Empty off a map,
    /// outside the field, or on a map with no `[[beams]]`. The hosts call
    /// it only with enhanced lighting on.
    pub fn expansion_beam_prims(&self, view: &FogView) -> Vec<ScreenPrim> {
        let Some(map) = self.expansion.map.as_ref() else {
            return Vec::new();
        };
        if map.def.beams.is_empty() || self.mode != legaia_engine_mode::mode::SceneMode::Field {
            return Vec::new();
        }
        let mut out = Vec::new();
        for b in &map.def.beams {
            let centre = map.def.tile_centre(b.tile[0], b.tile[1]);
            let floor = self.sample_field_floor_height(centre.0, centre.1);
            out.extend(beam_prims(
                b,
                self.frame,
                centre,
                floor,
                view,
                map.particle_sheet,
            ));
        }
        out
    }
}
