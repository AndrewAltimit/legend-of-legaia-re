//! Weapon-trail band emitter - the projected `POLY_G4` half of the swept
//! weapon trail. The trigger + sweep/band schedule (the simulation half)
//! is `legaia_engine_vm::battle_trail`.
//!
//! PORT: FUN_800485BC (per-band gouraud quad-strip emitter)
//! REF: FUN_80048310 - the sweep driver whose band order
//! `legaia_engine_vm::battle_trail::band_schedule` reproduces
//!
//! # What the retail emitter does
//!
//! `FUN_800485BC(actor, points, seg, colA, colB)` transforms the captured
//! control points of sweep steps `seg` and `seg + 1` (yaw-rotate the local
//! offsets by `actor[+0x26]` against the sin/cos LUTs, add the battle
//! slot's world base `ctx[+0x34]/[+0x38]`), projects each through the GTE
//! (`FUN_800195A8`), and emits one semi-transparent gouraud quad
//! (`0x3B808080` command word - `POLY_G4` with the ABE bit) per
//! consecutive control-point pair: `v0 = A[i-1]`, `v1 = B[i-1]`,
//! `v2 = A[i]`, `v3 = B[i]`, colours `colA` on the leading (`A`, step
//! `seg`) edge and `colB` on the trailing edge, OT-linked at the average
//! of the four projected depths.
//!
//! # Engine mapping, disclosed
//!
//! * **Placement/projection are the host camera's.** The hosts transform
//!   the ring-sampled local control points with the same model matrix
//!   their live battle body draws with (which subsumes retail's
//!   yaw + slot-base arithmetic under the engine's battle framing) and
//!   project with their battle MVP into the 320x240 stage -
//!   [`project_stage_point`], the same mapping the move-FX streak pass
//!   uses. The **packet** (vertex order, edge colours, blend) is retail.
//! * **Blend**: the `POLY_G4` packet carries no texpage, so its ABR comes
//!   from the GPU state the OT walk left active; the trail draws inside
//!   the battle FX group whose draw wrapper arms ABR 1
//!   (`FUN_80043390`'s mode byte `0x85` - the same additive mode the
//!   after-image ghosts use), and the double-emission design of the band
//!   ladder (white lead + tint restack on segment 0) only composes
//!   sensibly under `B + F`. The port pins ABR mode 1.
//! * **Depth**: retail interleaves the bands with the scene through the
//!   software OT; the screen-prim overlay draws over the scene instead
//!   (the same disclosed simplification as the move-FX streak).

use crate::screen_prim::{FlatQuad, ScreenPrim};
use glam::{Mat4, Vec4};
use legaia_engine_vm::battle_trail::{TRAIL_POINTS, band_schedule};

/// ABR blend mode of every trail band (additive `B + F` - see the module
/// docs for why).
pub const TRAIL_ABR_MODE: u8 = 1;

/// Overlay OT bucket every trail band links at. Retail interleaves the
/// bands with the scene through the software OT at the averaged projected
/// depth; the screen-prim overlay draws over the scene, so within the
/// overlay only the relative order matters and one bucket keeps the bands
/// in submission (band-ladder) order. Shared by both hosts so the ladder
/// composes identically.
pub const WEAPON_TRAIL_OT: u32 = 0x20;

/// Project a world/placement-space point into the retail 320x240 stage
/// under an arbitrary MVP. `None` behind the near plane - the sweep
/// truncates there rather than smearing a wrapped vertex across the
/// screen.
///
/// The centre mapping is the same one the move-FX streak pass uses
/// ([`crate::streak_pass`]): NDC to `STAGE_W x STAGE_H` with stage Y
/// growing downward.
pub fn project_stage_point(mvp: &Mat4, p: [f32; 3]) -> Option<(i16, i16)> {
    project_stage_point_cols(&mvp.to_cols_array(), p)
}

/// [`project_stage_point`] over a bare column-major `[f32; 16]`
/// (`m[col * 4 + row]`) - the shape hosts that carry no `glam` (the
/// browser play page's `battle_vp`) hand around.
pub fn project_stage_point_cols(vp: &[f32; 16], p: [f32; 3]) -> Option<(i16, i16)> {
    let mvp = Mat4::from_cols_array(vp);
    let clip = mvp * Vec4::new(p[0], p[1], p[2], 1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    let cx = (ndc_x * 0.5 + 0.5) * crate::streak_pass::STAGE_W;
    let cy = (0.5 - ndc_y * 0.5) * crate::streak_pass::STAGE_H;
    let clamp = |v: f32| v.clamp(-4096.0, 4096.0) as i16;
    Some((clamp(cx), clamp(cy)))
}

/// Emit the weapon-trail screen primitives for one actor's projected sweep.
///
/// `steps[k][i]` = the stage-space position of control point `i` at sweep
/// step `k` (step 0 = the current pose, each further step 2 frames older -
/// the retail rewind). `rgb` is the trigger tint, `ot_index` the overlay
/// OT bucket the whole trail links at.
///
/// Returns the bands in the retail emission order (white lead band, grey
/// second band, then the linear tint fade -
/// `legaia_engine_vm::battle_trail::band_schedule`), each band one gouraud
/// quad per consecutive control-point pair.
pub fn weapon_trail_prims(
    steps: &[[(i16, i16); TRAIL_POINTS]],
    rgb: [u8; 3],
    ot_index: u32,
) -> Vec<ScreenPrim> {
    let bands = band_schedule(steps.len(), rgb);
    let mut out = Vec::with_capacity(bands.len() * (TRAIL_POINTS - 1));
    for band in bands {
        let (Some(a), Some(b)) = (steps.get(band.seg), steps.get(band.seg + 1)) else {
            continue;
        };
        let lead = [band.lead[0], band.lead[1], band.lead[2], 0xFF];
        let tail = [band.tail[0], band.tail[1], band.tail[2], 0xFF];
        for i in 1..TRAIL_POINTS {
            out.push(ScreenPrim::Flat(FlatQuad {
                // Retail `POLY_G4` slotting (`0x800488x` stores):
                // xy0 = A[i-1], xy1 = B[i-1], xy2 = A[i], xy3 = B[i].
                xy: [a[i - 1], b[i - 1], a[i], b[i]],
                color: lead,
                // colA on the leading (step `seg`) edge, colB trailing.
                gouraud: Some([lead, tail, lead, tail]),
                semi_transparent: true,
                abr_mode: TRAIL_ABR_MODE,
                ot_index,
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_prim::BlendClass;

    fn steps(n: usize) -> Vec<[(i16, i16); TRAIL_POINTS]> {
        // A sweep marching left across the stage, blade points stacked
        // vertically.
        (0..n)
            .map(|k| {
                let x = 160 - 10 * k as i16;
                [(x, 100), (x, 120), (x, 140)]
            })
            .collect()
    }

    #[test]
    fn short_sweeps_draw_nothing() {
        assert!(weapon_trail_prims(&steps(0), [0x80, 0x20, 0x40], 8).is_empty());
        assert!(weapon_trail_prims(&steps(1), [0x80, 0x20, 0x40], 8).is_empty());
    }

    #[test]
    fn each_band_is_one_gouraud_quad_per_point_pair() {
        let prims = weapon_trail_prims(&steps(5), [0x80, 0x20, 0x40], 8);
        // 2 lead bands + 4 tint bands, times (TRAIL_POINTS - 1) quads.
        assert_eq!(prims.len(), 6 * (TRAIL_POINTS - 1));
        for p in &prims {
            let ScreenPrim::Flat(q) = p else {
                panic!("trail bands are untextured POLY_G4s");
            };
            assert!(q.semi_transparent);
            assert_eq!(q.abr_mode, TRAIL_ABR_MODE);
            assert_eq!(p.blend_class(), BlendClass::Semi(1));
            let g = q.gouraud.expect("gouraud edges");
            // v0/v2 share the leading colour, v1/v3 the trailing one.
            assert_eq!(g[0], g[2]);
            assert_eq!(g[1], g[3]);
        }
        // The first quad is the white leading band between steps 0 and 1.
        let ScreenPrim::Flat(q) = &prims[0] else {
            unreachable!()
        };
        let g = q.gouraud.unwrap();
        assert_eq!(g[0], [0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(g[1], [0x80, 0x80, 0x80, 0xFF]);
        // Its corners span steps 0 and 1 in the retail vertex order.
        let s = steps(5);
        assert_eq!(q.xy, [s[0][0], s[1][0], s[0][1], s[1][1]]);
    }

    #[test]
    fn the_oldest_tint_band_fades_to_black() {
        let prims = weapon_trail_prims(&steps(4), [0x40, 0x80, 0x20], 8);
        let ScreenPrim::Flat(last) = prims.last().unwrap() else {
            unreachable!()
        };
        let g = last.gouraud.unwrap();
        assert_eq!(g[1], [0, 0, 0, 0xFF], "trailing edge vanishes");
    }

    #[test]
    fn stage_projection_maps_the_origin_to_screen_centre() {
        // A bare pinhole looking down -Z from +Z: x/w, y/w with w = -z + 10.
        let mvp = Mat4::from_cols_array_2d(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, -1.0],
            [0.0, 0.0, 0.0, 10.0],
        ]);
        let c = project_stage_point(&mvp, [0.0, 0.0, 0.0]).unwrap();
        assert_eq!(c, (160, 120));
        // +Y in NDC is stage-up, so a +y point lands above centre.
        let up = project_stage_point(&mvp, [0.0, 1.0, 0.0]).unwrap();
        assert!(up.1 < 120);
        // Behind the near plane: no projection.
        assert!(project_stage_point(&mvp, [0.0, 0.0, 100.0]).is_none());
    }
}
