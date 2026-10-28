//! The projection half of the move-VM framebuffer grab (render mode `7`,
//! `legaia_engine_vm::vram_grab`): put the part's point through the frame's
//! field camera the way the arm's one `RTPS` does, and turn the result into
//! the `FUN_800468A4` call the arm makes.
//!
//! Retail projects inside the part tick, under the camera `FUN_800172C0`
//! has just loaded; the port projects here, with the view the host drew the
//! frame through, the way the move-VM strip emitter does
//! ([`crate::move_strip`]). The GTE's eye vector is the base matrix times
//! [`FieldCameraView::eye_space`] (`R * (p - focus) + tr`, which the view
//! carries over the `6x` base scale), or the base matrix times the point
//! alone on the camera-locked arm; the screen point is `OFX + H * x / z`,
//! `OFY + H * y / z` with the field's `OFX = 160` and the engine's `OFY`
//! ([`GTE_OFY`]); `IR3` is the eye-space depth.

use legaia_engine_camera::projection::GTE_OFY;
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_vm::vram_grab::GrabRequest;
use legaia_engine_vm::vram_rect_copy::RectCopyCall;

use crate::gte::FIELD_BASE_SCALE;
use crate::move_strip::FIELD_OT_SHIFT;

/// The field's GTE `OFX` - half the 320-pixel display.
const FIELD_OFX: f32 = 160.0;

/// One `RTPS` of `pos` under `view`, or under the base matrix alone when
/// `camera_locked`: the screen point (`SXY2`) and `IR3`. `None` when the
/// point sits at or behind the eye, where the GTE's divide saturates and the
/// rect it would place is off any frame.
///
/// PORT: FUN_8003D368
pub fn rtps_point(
    pos: [i16; 3],
    camera_locked: bool,
    view: &FieldCameraView,
) -> Option<((i16, i16), i32)> {
    let p = pos.map(f32::from);
    let e = if camera_locked {
        p.map(|c| c * FIELD_BASE_SCALE)
    } else {
        view.eye_space(p).map(|c| c * FIELD_BASE_SCALE)
    };
    if e[2] <= 0.5 {
        return None;
    }
    let scale = view.h / e[2];
    let sx = (FIELD_OFX + e[0] * scale).floor().clamp(-1024.0, 1023.0) as i16;
    let sy = (GTE_OFY + e[1] * scale).floor().clamp(-1024.0, 1023.0) as i16;
    // `IR3` with `sf = 1, lm = 0`: the eye depth, saturated to `s16`.
    let ir3 = e[2].clamp(-32768.0, 32767.0) as i32;
    Some(((sx, sy), ir3))
}

/// The `FUN_800468A4` call one grab makes under `view`
/// (`0x80022C68..0x80022CB4`): slot `(IR3 + bias) >> (shift + 2)`, source
/// the projected point plus the offset, extent and destination as seated.
pub fn grab_copy(req: &GrabRequest, view: &FieldCameraView) -> Option<RectCopyCall> {
    let ((sx, sy), ir3) = rtps_point(req.pos, req.camera_locked, view)?;
    let ot_slot = (ir3 + i32::from(req.depth_bias)) >> (u32::from(FIELD_OT_SHIFT) + 2);
    Some(RectCopyCall {
        ot_slot,
        src_x: sx.wrapping_add(req.offset[0]),
        src_y: sy.wrapping_add(req.offset[1]),
        w: req.size[0],
        h: req.size[1],
        dst_x: req.dest[0],
        dst_y: req.dest[1],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> FieldCameraView {
        FieldCameraView {
            focus: [1000.0, 0.0, 1000.0],
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
            h: 256.0,
            tr_eye: [0.0, 0.0, 800.0],
        }
    }

    fn req(pos: [i16; 3]) -> GrabRequest {
        GrabRequest {
            pos,
            camera_locked: false,
            depth_bias: 0x300,
            offset: [-32, -32],
            size: [64, 64],
            dest: [832, 0],
        }
    }

    #[test]
    fn a_point_at_the_focus_grabs_the_rect_round_the_screen_centre() {
        let c = grab_copy(&req([1000, 0, 1000]), &view()).expect("in front");
        assert_eq!((c.src_x, c.src_y), (160 - 32, GTE_OFY as i16 - 32));
        assert_eq!((c.w, c.h, c.dst_x, c.dst_y), (64, 64, 832, 0));
        // `IR3` is the 6x eye depth: `(4800 + 0x300) >> 2`.
        assert_eq!(c.ot_slot, (4800 + 0x300) >> 2);
    }

    #[test]
    fn the_source_follows_the_point_across_the_screen() {
        let a = grab_copy(&req([1000, 0, 1000]), &view()).unwrap();
        let b = grab_copy(&req([1100, 0, 1000]), &view()).unwrap();
        assert!(
            b.src_x > a.src_x,
            "a point to the right grabs further right"
        );
        assert_eq!(a.src_y, b.src_y);
    }

    #[test]
    fn the_locked_arm_ignores_the_camera() {
        let mut r = req([0, 0, 400]);
        r.camera_locked = true;
        let c = grab_copy(&r, &view()).unwrap();
        assert_eq!((c.src_x, c.src_y), (160 - 32, GTE_OFY as i16 - 32));
    }

    #[test]
    fn a_point_behind_the_eye_grabs_nothing() {
        assert_eq!(grab_copy(&req([1000, 0, -5000]), &view()), None);
    }
}
