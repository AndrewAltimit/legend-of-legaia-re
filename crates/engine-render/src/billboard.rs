//! Screen-space billboard / sprite-quad corner projector.
//!
//! PORT: FUN_800195a8
//!
//! **Wiring status.** Split, so read it per item. [`psx_sin`] / [`psx_cos`]
//! (the clean-room trig LUT) are live: the disc-gated oracle
//! `crates/engine-shell/tests/gte_sin_lut_real.rs` pins them entry-for-entry
//! against the retail table, and other GTE matrix builders reuse them.
//!
//! NOT WIRED: [`project_billboard`] and its `rot_z_psx` / `apply_rot_z`
//! helpers. Its non-test callers are all ports of retail riders that are
//! themselves inert - [`crate::afterimage::project_streak_corners`] and
//! [`crate::afterimage::project_ribbon_corners`] (the streak has no
//! per-frame emitter; see that module's NOT WIRED note for the two gaps)
//! and [`crate::battle_on_screen::battle_actor_on_screen`] (no battle actor
//! pool in this crate). The remaining retail riders of `FUN_800195a8`
//! (cutscene and world-map sprite emitters such as `FUN_800485BC`) are not
//! ported, and engine-shell draws its live effect billboards as 3D meshes
//! (`effect_billboard_mesh`) rather than through a projected screen quad,
//! so nothing else reaches for it. Exercised by unit tests; do not delete.
//!
//! The retail SCUS helper `FUN_800195a8` projects a sprite quad about a 3D
//! center point and hands the four screen-space corners back through caller
//! out-pointers. Every quad emitter that draws a camera-facing rectangle
//! (battle move-FX afterimage streaks via `FUN_801e1ab0`, cutscene and
//! world-map sprite emitters such as `FUN_800485BC`) rides it. The exact
//! retail instruction stream, mirrored here step for step:
//!
//! 1. **Center transform** - `FUN_8003d344` runs one GTE `MVMVA`
//!    (`cop2 0x480012`: rotation matrix × V0 + TR, sf=1) on the center
//!    vector and reads back MAC1..MAC3: the **view-space** position under
//!    the ambient camera matrix. The caller then reads each MAC word's low
//!    halfword (`lhu`), so the view position wraps to i16.
//! 2. **Corner fan-out** - four corners are formed by 16-bit adds/subs in
//!    view space, all sharing the view Z:
//!    `c0 = (x-hw, y-hh)`, `c1 = (x+hw, y-hh)`, `c2 = (x-hw, y+hh)`,
//!    `c3 = (x+hw, y+hh)`.
//! 3. **Matrix reset** - `FUN_8003d178` writes identity into the GTE
//!    rotation control words **and zeroes TRX/TRY/TRZ**, so the projection
//!    pass is a pure perspective divide of the view-space corners.
//! 4. **Optional in-plane spin** - when `angle & 0xFFF != 0`,
//!    `FUN_8004638c` composes the (now identity) rotation with
//!    `Rz(angle)` from the in-image sin/cos LUT pair (sin at `0x80070A2C`,
//!    cos at `0x8007122C` = the same table read 0x400 entries ahead),
//!    preserving TR. The corner vectors include the view-space center, so a
//!    non-zero angle spins the quad about the **camera axis**, not the quad
//!    center - retail callers pass `0` unless they want exactly that.
//! 5. **Projection** - `FUN_8005bac8` runs `RTPT` (`cop2 0x280030`) on
//!    corners 0..2 and one `RTPS` (`cop2 0x180001`) on corner 3, storing the
//!    four SXY words through the out-pointers (`swc2` of data registers
//!    12/13/14 - the SXY FIFO, **not** MAC0) and returning `SZ3 >> 2` (the
//!    classic OT-bucket depth, from the last corner's view Z). Both ops
//!    carry `sf = 1`, so the perspective divide is the GTE's own UNR
//!    reciprocal ([`crate::gte::gte_divide`]) and each stored corner is
//!    saturated to the SXY FIFO's signed 11 bits, not to `i16` - see
//!    [Projection is the hardware path](#projection-is-the-hardware-path).
//! 6. **Depth shift** - the return value is shifted right by the scratchpad
//!    OT-resolution byte `DAT_1F8003A4` (passed in here as `ot_shift`), and
//!    `FUN_8003d1a4` restores the GTE control words saved at `DAT_1F8003C8`
//!    (state save/restore is implicit here - the camera arguments are
//!    borrowed, never mutated).
//!
//! The corner order matters: `FUN_801e1ab0` wires the four out-pointers
//! straight into the `POLY_FT4` packet's `xy0..xy3` slots, so [`xy`]
//! (`BillboardCorners::xy`) is in exactly that retail vertex order.
//!
//! REF: FUN_801e1ab0 - the afterimage streak, the rider whose out-pointer
//! order fixes [`BillboardCorners::xy`]; ported as [`crate::afterimage`].
//! REF: FUN_800485bc - a cutscene / world-map sprite emitter riding the same
//! projector, documented but not ported.
//!
//! ## Projection is the hardware path
//!
//! The corner projection runs the same two kernels the rest of the crate's
//! GTE model uses, because retail reaches it through real `RTPT`/`RTPS`:
//!
//! * the quotient is [`crate::gte::gte_divide`], the UNR reciprocal, not an
//!   exact `h * x / z`. The two agree at many depths and differ in the low
//!   bit at many others, and `Camera::transform` is pinned bit-exact against
//!   a real-COP2 RTPT ring capture, so the exact divide was a divergence
//!   from the model this crate already validates.
//! * each stored corner is saturated by [`crate::gte::ScreenXY::saturate_sxy`]
//!   to `[-0x400, 0x3FF]`. That is the SXY-FIFO bound (`swc2` reads the FIFO
//!   entry, which the GTE has already clamped); an `i16` bound let corners
//!   past +/-1024 through as coordinates the hardware cannot represent.
//! * a behind-camera corner is **not** a special case. `SZ3` clamps to `0`,
//!   the divide saturates its quotient at `0x1FFFF` (~2.0 in q16), and the
//!   corner lands at roughly `OFX + 2 * IR1` - the classic PSX
//!   vertex-behind-the-lens smear. Consumers drop the quad on
//!   [`BillboardCorners::behind`] rather than on a sentinel coordinate.
//!
//! ## Clean-room boundary: the trig LUT
//!
//! Retail reads sine values from an in-image LUT (Sony bytes, never
//! committed). [`psx_sin`] / [`psx_cos`] compute the same q3.12 values
//! trigonometrically; the disc-gated oracle
//! `crates/engine-shell/tests/gte_sin_lut_real.rs` compares all 4096
//! entries of both tables against the user's own `SCUS_942.54` to pin the
//! reproduction.

use crate::gte::{
    GteMat3, GteVec3, ROT_ONE, SXY_MAX, SXY_MIN, ScreenXY, gte_divide, gte_persp_term, rot_trans,
};

/// One full turn in the PSX 12-bit angle space (`0x1000` = 360°).
pub const PSX_ANGLE_TURN: u16 = 0x1000;

/// q3.12 sine of a PSX 12-bit angle (`4096` units per turn).
///
/// Mirrors the retail sin LUT at `0x80070A2C` (indexed `base + 2*angle`
/// by `FUN_8004638c` and the other `RotMatrix*` builders). The retail table
/// is `4096 * sin(2*pi*angle/4096)` **truncated toward zero** (a C `(int)`
/// cast - so `cos(tiny) = 4095` and both lobes bias one step toward zero,
/// not round-to-nearest); the disc-gated LUT oracle asserts the
/// reproduction entry-for-entry against the real table.
pub fn psx_sin(angle: u16) -> i32 {
    let a = (angle & (PSX_ANGLE_TURN - 1)) as f64;
    let radians = a * std::f64::consts::TAU / PSX_ANGLE_TURN as f64;
    (radians.sin() * ROT_ONE as f64).trunc() as i32
}

/// q3.12 cosine of a PSX 12-bit angle.
///
/// Retail has no separate cosine table: the "cos" pointer (`0x8007122C`)
/// is the sine table read `0x400` entries (90°) ahead, which is why the
/// combined LUT spans 5120 entries (1.25 turns).
pub fn psx_cos(angle: u16) -> i32 {
    psx_sin(angle.wrapping_add(PSX_ANGLE_TURN / 4))
}

/// Build `Rz(angle)` for a PSX 12-bit angle in q3.12.
///
/// The 12-bit-angle sibling of [`GteMat3::rot_z`] (which takes radians);
/// this is the exact matrix `FUN_8004638c` assembles when the current GTE
/// rotation is identity - the state [`project_billboard`] runs it in.
pub fn rot_z_psx(angle: u16) -> GteMat3 {
    let c = psx_cos(angle).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let s = psx_sin(angle).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    GteMat3 {
        m: [[c, -s, 0], [s, c, 0], [0, 0, ROT_ONE as i16]],
    }
}

/// Compose an existing rotation with an in-plane `Rz(angle)` spin.
///
// REF: FUN_8004638c - the general retail form: saves TR, multiplies the
// current GTE rotation by Rz(angle) column-by-column via three MVMVA
// passes over the sin/cos LUT vectors, restores TR. `R * Rz` with q3.12
// renormalisation per element, which `GteMat3::mul` reproduces.
pub fn apply_rot_z(rot: &GteMat3, angle: u16) -> GteMat3 {
    rot.mul(&rot_z_psx(angle))
}

/// The four projected corners of a billboard quad plus its OT depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillboardCorners {
    /// Screen-space corners in the retail out-pointer order
    /// (`(-w,-h)`, `(+w,-h)`, `(-w,+h)`, `(+w,+h)`) - the order
    /// `FUN_801e1ab0` writes them into `POLY_FT4.xy0..xy3`. Each component
    /// is an SXY-FIFO entry, so it is already saturated to signed 11 bits
    /// (`[-0x400, 0x3FF]`).
    pub xy: [(i16, i16); 4],
    /// View-space Z of the billboard plane (shared by all four corners;
    /// the low 16 bits the retail caller reads back with `lhu`).
    pub view_z: i16,
    /// The function's return value: `(SZ3 >> 2) >> ot_shift` - the OT
    /// bucket the caller links the packet at (via `addPrim`,
    /// `FUN_8003d2c4`).
    pub depth: i32,
    /// True when the billboard plane sits behind the camera
    /// (`view_z <= 0`). The corner SXYs are still whatever the GTE latches -
    /// `SZ3` clamps to `0`, so the divide returns its saturated `0x1FFFF`
    /// quotient and the corners smear outward from `(OFX, OFY)` rather than
    /// taking a sentinel value. Consumers that want the quad gone must test
    /// this flag.
    pub behind: bool,
}

/// Project a billboard quad about `center` under the camera `(rot, trans)`.
///
/// PORT: FUN_800195a8 - see the module docs for the per-step mapping to the
/// retail instruction stream. `h`/`ofx`/`ofy` are the ambient GTE projection
/// registers (battle uses `H = 256` with the orbit camera); `ot_shift` is
/// the scratchpad OT-resolution byte `DAT_1F8003A4` (`0` collapses the shift).
#[allow(clippy::too_many_arguments)]
pub fn project_billboard(
    rot: &GteMat3,
    trans: GteVec3,
    center: GteVec3,
    half_w: i16,
    half_h: i16,
    angle: u16,
    h: i32,
    ofx: i32,
    ofy: i32,
    ot_shift: u32,
) -> BillboardCorners {
    // Step 1: MVMVA center transform, low-halfword (`lhu`) readback.
    let view = rot_trans(rot, center, trans);
    let vx = view.x as i16;
    let vy = view.y as i16;
    let vz = view.z as i16;

    // Step 2: 16-bit corner fan-out (retail subu/addu then sh - wrapping).
    let corners = [
        (vx.wrapping_sub(half_w), vy.wrapping_sub(half_h)),
        (vx.wrapping_add(half_w), vy.wrapping_sub(half_h)),
        (vx.wrapping_sub(half_w), vy.wrapping_add(half_h)),
        (vx.wrapping_add(half_w), vy.wrapping_add(half_h)),
    ];

    // Steps 3+4: rotation = identity (TR zeroed), optionally composed with
    // Rz. The Rz row 3 is (0, 0, 4096), so the view Z is exact through the
    // spin and every corner keeps `vz`.
    let spin = (angle & (PSX_ANGLE_TURN - 1) != 0).then(|| rot_z_psx(angle));

    // Step 5: perspective divide per corner (RTPT ×3 + RTPS ×1 - identical
    // arithmetic per slot). Every corner shares the plane's view Z, and the
    // Rz row 3 is `(0, 0, 4096)`, so `SZ3` and therefore the UNR quotient are
    // computed once for the whole quad exactly as the hardware would per op.
    let sz3 = i32::from(vz).clamp(0, u16::MAX as i32) as u16;
    let (recip, _overflow) = gte_divide(h.clamp(0, u16::MAX as i32) as u16, sz3);

    let mut xy = [(0i16, 0i16); 4];
    let behind = vz <= 0;
    for (slot, &(cx, cy)) in xy.iter_mut().zip(&corners) {
        let (px, py) = match &spin {
            Some(rz) => {
                let r = rz.mul_vec(GteVec3::new(cx.into(), cy.into(), vz.into()));
                (r.x, r.y)
            }
            None => (cx.into(), cy.into()),
        };
        // IR1 / IR2: the i16-saturated numerators the reciprocal multiplies.
        let ir1 = px.clamp(SXY_MIN, SXY_MAX);
        let ir2 = py.clamp(SXY_MIN, SXY_MAX);
        // MAC0 = OFX + IR * (H / SZ3). OFX/OFY are q16.16 control words, so
        // adding the integer offset after the `>> 16` is the same value.
        let sx = i64::from(ofx) + gte_persp_term(ir1, recip);
        let sy = i64::from(ofy) + gte_persp_term(ir2, recip);
        let saturated = ScreenXY::new(
            sx.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            sy.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        )
        .saturate_sxy();
        *slot = (saturated.x as i16, saturated.y as i16);
    }

    // Step 6: SZ3 (u16-clamped view Z of the final RTPS) >> 2, then the
    // scratchpad OT shift.
    let depth = (i32::from(sz3) >> 2) >> (ot_shift & 0x1F);

    BillboardCorners {
        xy,
        view_z: vz,
        depth,
        behind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gte::{SX_MAX, SX_MIN};

    #[test]
    fn psx_sin_cardinal_points() {
        assert_eq!(psx_sin(0), 0);
        assert_eq!(psx_sin(0x400), 4096);
        assert_eq!(psx_sin(0x800), 0);
        assert_eq!(psx_sin(0xC00), -4096);
        assert_eq!(psx_cos(0), 4096);
        assert_eq!(psx_cos(0x400), 0);
        // 12-bit wrap: angle 0x1000 aliases 0.
        assert_eq!(psx_sin(0x1000), psx_sin(0));
    }

    #[test]
    fn psx_sin_symmetry() {
        for a in 0..0x400u16 {
            assert_eq!(psx_sin(a), -psx_sin(a + 0x800), "antisymmetry at {a}");
            assert_eq!(psx_sin(a), psx_sin(0x800 - a), "mirror at {a}");
        }
    }

    /// One axis of the hardware projection, spelled out so a test's expected
    /// pixel can be read rather than guessed:
    /// `SX = OFX + ((IR * (H / SZ3)) >> 16)`, with the quotient from the UNR
    /// reciprocal and the shift arithmetic (so it floors, it does not
    /// truncate toward zero).
    fn expect_sx(ir: i32, h: u16, sz3: u16, of: i32) -> i16 {
        let (recip, _) = crate::gte::gte_divide(h, sz3);
        (of as i64 + crate::gte::gte_persp_term(ir, recip)).clamp(-0x400, 0x3FF) as i16
    }

    #[test]
    fn axis_aligned_projection_matches_the_unr_divide() {
        // Identity camera, center 1000 units ahead, 16-unit half-extents,
        // H=320 focal, 160/120 screen center.
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            GteVec3::new(0, 0, 1000),
            16,
            16,
            0,
            320,
            160,
            120,
            0,
        );
        let (left, right) = (
            expect_sx(-16, 320, 1000, 160),
            expect_sx(16, 320, 1000, 160),
        );
        let (top, bottom) = (
            expect_sx(-16, 320, 1000, 120),
            expect_sx(16, 320, 1000, 120),
        );
        assert_eq!(b.xy[0], (left, top));
        assert_eq!(b.xy[1], (right, top));
        assert_eq!(b.xy[2], (left, bottom));
        assert_eq!(b.xy[3], (right, bottom));
        assert_eq!(b.view_z, 1000);
        assert!(!b.behind);
        // depth = (1000 >> 2) >> 0
        assert_eq!(b.depth, 250);

        // The arithmetic shift floors, so a symmetric +/-16 box is NOT
        // symmetric about the screen centre: 320*16/1000 = 5.12 gives +5 on
        // the right and -6 on the left. An exact `h*x/z` divide - what this
        // port used to do - would put both edges 5 pixels out.
        assert_eq!((left, right), (154, 165));
        assert_eq!(160 - left, 6);
        assert_eq!(right - 160, 5);
    }

    #[test]
    fn camera_translation_offsets_center() {
        // TR pushes the plane deeper; corners scale accordingly.
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::new(100, 0, 1000),
            GteVec3::new(0, 0, 1000),
            0,
            0,
            0,
            320,
            0,
            0,
            0,
        );
        // view = (100, 0, 2000): sx = 320*100/2000 = 16.
        assert_eq!(b.xy[0], (expect_sx(100, 320, 2000, 0), 0));
        assert_eq!(b.xy[0], (16, 0));
        assert_eq!(b.view_z, 2000);
    }

    #[test]
    fn quarter_turn_spin_rotates_about_view_origin() {
        // angle 0x400 = 90°: (x, y) -> (-y, x) about the camera axis. The
        // center offset rotates too - the documented retail semantics.
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            GteVec3::new(100, 0, 1000),
            10,
            20,
            0x400,
            320,
            0,
            0,
            0,
        );
        // corner0 pre-spin = (90, -20); post-spin = (20, 90).
        assert_eq!(
            b.xy[0],
            (expect_sx(20, 320, 1000, 0), expect_sx(90, 320, 1000, 0))
        );
        assert_eq!(b.xy[0], (6, 28));
        // Z untouched by the spin.
        assert_eq!(b.view_z, 1000);
        assert_eq!(b.depth, 250);
    }

    #[test]
    fn ot_shift_scales_depth() {
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            GteVec3::new(0, 0, 4000),
            1,
            1,
            0,
            320,
            0,
            0,
            2,
        );
        // (4000 >> 2) >> 2 = 250.
        assert_eq!(b.depth, 250);
    }

    #[test]
    fn behind_camera_flags_but_does_not_take_a_sentinel_coordinate() {
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            GteVec3::new(50, -30, -10),
            5,
            5,
            0,
            320,
            0,
            0,
            0,
        );
        assert!(b.behind);
        // Negative view Z clamps SZ3 to 0 -> depth 0.
        assert_eq!(b.depth, 0);
        // SZ3 = 0 saturates the UNR quotient at 0x1FFFF (~2.0 in q16), so the
        // corners land at ~2x the view offset rather than at an i16 extreme -
        // the PSX's behind-the-lens smear. Consumers drop the quad on
        // `behind`; the coordinates are not a sentinel.
        assert_eq!(
            b.xy[0],
            (expect_sx(45, 320, 0, 0), expect_sx(-35, 320, 0, 0))
        );
        assert_eq!(b.xy[0], (89, -70));
    }

    #[test]
    fn view_position_wraps_to_low_halfword() {
        // A view coordinate past i16 wraps (retail `lhu` readback), it does
        // not saturate: 0x10064 -> 0x0064 = 100. Without the wrap the
        // numerator would saturate IR1 and drive the corner to SX_MAX.
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            GteVec3::new(0x1_0064, 0, 1000),
            0,
            0,
            0,
            320,
            0,
            0,
            0,
        );
        assert_eq!(b.xy[0].0, expect_sx(100, 320, 1000, 0));
        assert_eq!(b.xy[0].0, 32);
        assert_ne!(b.xy[0].0, SX_MAX as i16);
    }

    #[test]
    fn corners_saturate_to_the_sxy_fifo_not_to_i16() {
        // A corner far off the right of the screen: 320*30000/1000 = 9600,
        // well past the SXY FIFO's signed-11-bit range. The GTE latches
        // SX_MAX; an i16 bound would have stored 9600.
        let b = project_billboard(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            GteVec3::new(30_000, -30_000, 1000),
            0,
            0,
            0,
            320,
            0,
            0,
            0,
        );
        assert_eq!(b.xy[0], (SX_MAX as i16, SX_MIN as i16));
    }

    #[test]
    fn rot_z_psx_matches_radian_builder_at_cardinals() {
        for (a, rad) in [
            (0u16, 0.0f32),
            (0x400, std::f32::consts::FRAC_PI_2),
            (0x800, std::f32::consts::PI),
        ] {
            assert_eq!(rot_z_psx(a).m, GteMat3::rot_z(rad).m, "angle {a:#x}");
        }
    }
}
