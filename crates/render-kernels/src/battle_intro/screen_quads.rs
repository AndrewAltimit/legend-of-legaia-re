//! Intro quads and fades as screen prims, and the placement rotation.

use crate::battle_intro_styles::IntroFade;
use crate::battle_intro_transition::IntroQuad;
use crate::screen_prim::{ScreenPrim, ScreenQuad, fade_prim};

/// Convert one built transition quad into a screen-space primitive.
///
/// The mapping is direct because `FUN_801CF1B0` already emits a screen-space
/// `POLY_GT4`: its corners are pixels, its `tpage` / `clut` are the GP0 words,
/// and its per-vertex colours are the gradient. The vertex order is shared
/// (`v0` top-left, `v1` top-right, `v2` bottom-left, `v3` bottom-right), so no
/// re-ordering happens here either.
///
/// The `abr` bit is carried in the primitive's code byte
/// (`(abr << 1) | POLY_GT4`), and the ABR *mode* in TSB bits 5..=6 of the same
/// tpage word the quad already carries - which is why `semi_transparent` reads
/// the code byte while [`ScreenQuad::abr_mode`] reads the tpage.
pub fn intro_quad_to_screen(q: &IntroQuad) -> ScreenQuad {
    let rgb = |c: [u8; 3]| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]);
    ScreenQuad {
        xy: std::array::from_fn(|i| (q.verts[i].x, q.verts[i].y)),
        uv: std::array::from_fn(|i| (q.verts[i].u, q.verts[i].v)),
        clut: q.clut,
        tpage: q.tpage,
        color: rgb(q.verts[0].rgb),
        gouraud: Some(std::array::from_fn(|i| rgb(q.verts[i].rgb))),
        semi_transparent: q.code & 0x02 != 0,
        ot_index: q.ot_depth,
        depth: None,
    }
}

/// The full-screen quad `FUN_80024EE4` pushes for a resolved fade.
///
/// Both halves are retail's now. The **ramp** is
/// [`crate::battle_intro_styles::intro_fade`]; the **quad** is the
/// emitter's own six-word packet, corners taken from the scratchpad
/// display-rect words (`_DAT_1F800378` / `_DAT_1F80037A`), which is the whole
/// PSX display - so the port draws the whole display rect. Command byte
/// `0x2B` makes it semi-transparent, and its ABR mode is the emitter's second
/// argument, folded into the `SetDrawMode` packet that precedes it.
///
/// That second argument used to be read here as an OT depth, which put every
/// style's fade on ABR `0` (`0.5B + 0.5F`). It halved both tails: the
/// additive styles ([`IntroStyle::SpinUpParticles`],
/// [`IntroStyle::Swirl`]) topped out at a washed grey instead of a full
/// white-out, and the subtractive ones never reached black. The OT layer is
/// `a0` - [`crate::battle_intro_styles::INTRO_FADE_LAYER`], the
/// same `2` for all five styles - so that is what the bucket carries.
/// The quad itself is [`crate::screen_prim::fade_prim`], which is
/// also what the browser play page emits: the fade is the one part of this
/// transition both hosts draw, so the two must not each build the packet.
pub fn fade_quad(f: &IntroFade) -> ScreenPrim {
    fade_prim(f.rgb, f.abr, u32::from(f.layer))
}

/// Compose the object record's three authored angles into a model rotation,
/// in retail's order.
///
/// `FUN_80026988` builds `Rx * Ry * Rz` from the halfwords `FUN_8003A55C`
/// copies out of the placement record (`+0x08`/`+0x0A`/`+0x0C`). glam's
/// per-axis constructors carry the same handedness as retail's table-driven
/// sin/cos in that frame, so no sign flips are needed - which is not
/// self-evident and is pinned by `glam_euler_matches_retail_composition`.
///
/// Angles are 12-bit (`4096` = one revolution).
///
/// REF: FUN_80026988, FUN_8003A55C, FUN_8001ADA4
pub fn placement_rotation(rot_x: u16, rot_y: u16, rot_z: u16) -> glam::Mat4 {
    let a = |v: u16| f32::from(v & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
    glam::Mat4::from_rotation_x(a(rot_x))
        * glam::Mat4::from_rotation_y(a(rot_y))
        * glam::Mat4::from_rotation_z(a(rot_z))
}

#[cfg(test)]
mod placement_rotation_tests {
    use super::*;
    use crate::battle_intro::euler_rot_psx;
    use crate::gte::GteMat3;
    use glam::{Mat4, Vec3};

    fn gte_to_mat4(m: &GteMat3) -> Mat4 {
        let f = |v: i16| f32::from(v) / 4096.0;
        Mat4::from_cols(
            [f(m.m[0][0]), f(m.m[1][0]), f(m.m[2][0]), 0.0].into(),
            [f(m.m[0][1]), f(m.m[1][1]), f(m.m[2][1]), 0.0].into(),
            [f(m.m[0][2]), f(m.m[1][2]), f(m.m[2][2]), 0.0].into(),
            [0.0, 0.0, 0.0, 1.0].into(),
        )
    }

    /// [`placement_rotation`] reproduces [`euler_rot_psx`] - retail's own
    /// matrix builder - for tilted placements, not just pure yaw.
    ///
    /// Worth pinning because three plausible alternatives (reversed order,
    /// negated angles, both) all agree with retail on the pure-yaw and
    /// single-axis cases that dominate the disc, and only diverge once two
    /// axes are non-zero at the same time. A port that guessed wrong would
    /// look right on almost every placement and wrong on the handful that
    /// actually carry a tilt.
    #[test]
    fn glam_euler_matches_retail_composition() {
        // jagaroom record 82 (yaw + roll), then single axes, then a triple.
        for (x, y, z) in [
            (0u16, 0x0EA0u16, 0x0120u16),
            (0x0400, 0, 0),
            (0, 0, 0x0800),
            (0x0200, 0x0300, 0x0100),
            (0x0FBF, 0x0100, 0x0200),
        ] {
            let retail = gte_to_mat4(&euler_rot_psx((x as i16, y as i16, z as i16)));
            let ours = placement_rotation(x, y, z);
            for v in [
                Vec3::new(100.0, 200.0, 300.0),
                Vec3::new(-676.0, 0.0, 0.0),
                Vec3::new(0.0, -676.0, 0.0),
            ] {
                let (a, b) = (retail.transform_vector3(v), ours.transform_vector3(v));
                // Retail truncates to q3.12 per term, so a few tenths of a
                // world unit over a 700-unit lever arm is the expected floor.
                assert!(
                    (a - b).length() < 1.0,
                    "x={x:#x} y={y:#x} z={z:#x} v={v:?}: retail {a:?} vs ours {b:?}"
                );
            }
        }
    }
}
