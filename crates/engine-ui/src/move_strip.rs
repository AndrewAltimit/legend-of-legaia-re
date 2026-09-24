//! The draw half of the move-VM extension's scanline strip emitter: project
//! each captured sub-op `0x2C` request through the frame's field camera, run
//! the ported row walk (`legaia_engine_vm::move_ext_strip::emit_strip`) and
//! hand back screen primitives.
//!
//! Retail makes both projections inside `FUN_801D31B0` itself - a
//! `RotTransPers` (`FUN_8005BA38`) of the actor position for the screen
//! centre and depth, then the billboard projector (`FUN_800195A8`) of a box
//! about the same point. Both run under the field camera the frame is drawn
//! through, so the port runs them here, on the host's render pass, with the
//! pose the host just resolved - one kernel both play hosts call, the way
//! the fog sheets are drawn.
//!
//! The eye-space vector is [`FieldCameraView::eye_space`] (`R * (p - focus) +
//! tr`, the vector the GTE divides by); the screen point is
//! `OFX + H * x / z`, `OFY + H * y / z` with the field's `OFX = 160` and the
//! engine's shared `OFY` ([`GTE_OFY`]); the depth is `SZ3 >> 2`. The box's
//! corners fan out from the view-space centre by the slab's half-extent and
//! divide by the same `z`, which is the projector's own recipe
//! ([`crate::billboard`]).

use legaia_engine_vm::battle_cam_script::GTE_OFY;
use legaia_engine_vm::move_ext_strip::{
    StripClip, StripEmit, StripProjection, StripQuad, StripRequest, emit_strip,
};
use legaia_engine_vm::psx_camera::FieldCameraView;

use crate::gte::{psx_cos, psx_sin};
use crate::screen_prim::{ScreenPrim, ScreenQuad};

/// The field's GTE `OFX` - half the 320-pixel display.
const FIELD_OFX: f32 = 160.0;

/// The scratchpad OT-resolution byte `DAT_1F8003A4` the field runs with.
///
/// The engine keeps no ordering table of its own depth resolution; the
/// strip's slot only has to sort it among the other screen primitives, so
/// the unshifted `SZ3 >> 2` stands.
pub const FIELD_OT_SHIFT: u8 = 0;

/// Project one request the way `FUN_801D31B0`'s two projector calls do.
/// `None` when the actor sits at or behind the eye.
pub fn project_request(req: &StripRequest, view: &FieldCameraView) -> Option<StripProjection> {
    let p = [
        f32::from(req.pos[0]),
        f32::from(req.pos[1]),
        f32::from(req.pos[2]),
    ];
    let e = view.eye_space(p);
    if e[2] <= 0.5 {
        return None;
    }
    let scale = view.h / e[2];
    let screen = |x: f32, y: f32| -> (i16, i16) {
        let sx = (FIELD_OFX + x * scale).floor().clamp(-1024.0, 1023.0);
        let sy = (GTE_OFY + y * scale).floor().clamp(-1024.0, 1023.0);
        (sx as i16, sy as i16)
    };
    let (hw, hh) = (f32::from(req.slab.half_w), f32::from(req.slab.half_h));
    let corners = [
        screen(e[0] - hw, e[1] - hh),
        screen(e[0] + hw, e[1] - hh),
        screen(e[0] - hw, e[1] + hh),
        screen(e[0] + hw, e[1] + hh),
    ];
    let sz3 = e[2].clamp(0.0, f32::from(u16::MAX)) as u32;
    Some(StripProjection {
        centre: screen(e[0], e[1]),
        otz: sz3 >> 2,
        corners,
    })
}

/// Run the strip emitter for one request under `view`.
pub fn strip_emit(req: &StripRequest, view: &FieldCameraView) -> Option<StripEmit> {
    let proj = project_request(req, view)?;
    emit_strip(
        &req.slab,
        &req.scroll,
        &proj,
        &StripClip::default(),
        FIELD_OT_SHIFT,
        |a| (psx_sin(a) as i16, psx_cos(a) as i16),
    )
}

/// One emitted span as a screen primitive: an opaque `POLY_FT4` at
/// modulation `0x808080` (the retail packet's `0x2C808080` word).
pub fn strip_quad_prim(q: &StripQuad, ot_index: u32) -> ScreenPrim {
    ScreenPrim::Textured(ScreenQuad {
        xy: q.xy(),
        uv: q.uv(),
        clut: q.clut,
        tpage: q.tpage,
        color: 0x0080_8080,
        gouraud: None,
        semi_transparent: false,
        ot_index,
        depth: None,
    })
}

/// Every captured request's spans as screen primitives, under `view`.
///
/// Both play hosts call this from their draw path with the requests the
/// world captured since the last draw
/// (`legaia_engine_core::world::World::take_move_strip_requests`).
pub fn move_strip_prims(requests: &[StripRequest], view: &FieldCameraView) -> Vec<ScreenPrim> {
    let mut out = Vec::new();
    for req in requests {
        if let Some(e) = strip_emit(req, view) {
            out.extend(e.quads.iter().map(|q| strip_quad_prim(q, e.ot_index)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::move_ext_strip::{StripScroll, StripSlab};

    fn view() -> FieldCameraView {
        FieldCameraView {
            focus: [0.0, 0.0, 0.0],
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
            h: 256.0,
            tr_eye: [0.0, 0.0, 1024.0],
        }
    }

    fn req() -> StripRequest {
        StripRequest {
            slab: StripSlab {
                u0: 0,
                v0: 0,
                u1: 63,
                v1: 31,
                tpage: 0x0017,
                clut: 0x7F00,
                half_w: 128,
                half_h: 64,
                wobble_amp: 0,
                wobble_freq: 0,
            },
            scroll: StripScroll::default(),
            pos: [0, 0, 0],
        }
    }

    #[test]
    fn a_point_on_the_axis_projects_to_the_screen_centre() {
        let p = project_request(&req(), &view()).unwrap();
        assert_eq!(p.centre, (160, GTE_OFY as i16));
        assert_eq!(p.otz, 1024 >> 2);
        // Half-extents scale by H / z = 1/4.
        assert_eq!(p.corners[0], (160 - 32, GTE_OFY as i16 - 16));
        assert_eq!(p.corners[3], (160 + 32, GTE_OFY as i16 + 16));
    }

    #[test]
    fn behind_the_eye_draws_nothing() {
        let mut r = req();
        r.pos = [0, 0, -2048];
        assert!(project_request(&r, &view()).is_none());
        assert!(move_strip_prims(&[r], &view()).is_empty());
    }

    #[test]
    fn a_request_draws_opaque_one_row_spans() {
        let prims = move_strip_prims(&[req()], &view());
        assert!(!prims.is_empty());
        for p in &prims {
            let ScreenPrim::Textured(q) = p else {
                panic!("strip spans are textured");
            };
            assert!(!q.semi_transparent);
            assert_eq!(q.color, 0x0080_8080);
            assert_eq!(q.xy[2].1 - q.xy[0].1, 1, "one pixel tall");
            assert_eq!(q.uv[0].1, q.uv[2].1, "one texel row");
        }
    }
}
