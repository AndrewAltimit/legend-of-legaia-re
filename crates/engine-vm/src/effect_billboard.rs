//! Where an effect sprite's quad half-extents live, and why a world-space
//! billboard builder has to divide them by the camera's own scale.
//!
//! REF: FUN_800195a8 - the retail sprite-quad projector. The projector itself
//! is ported as `legaia_engine_render::billboard::project_billboard`; this
//! module carries only the one step a *world-space* billboard builder gets
//! wrong, so both hosts can share it (`engine-render` links wgpu, so the
//! browser play page cannot depend on it).
//!
//! ## The half-extent is a view-space quantity
//!
//! Read `FUN_800195a8`'s instruction stream in order (`ghidra/scripts/funcs/
//! 800195a8.txt`):
//!
//! 1. `jal 0x8003d344` transforms the sprite **centre** - one GTE `MVMVA`
//!    (`cop2 0x480012`: rotation matrix x V0 + TR) - and the caller reads
//!    MAC1..MAC3 back as halfwords at `0x48/0x4c/0x50(sp)`. Whatever scale the
//!    GTE rotation matrix carries is applied *here*, to the centre.
//! 2. `subu a2,v1,s0` / `addu v1,v1,s0` (and the `s1` pair) form the four
//!    corners by adding the half-extents to that **already-transformed**
//!    view-space centre, sharing its view Z.
//! 3. `jal 0x8003d178` then resets the GTE rotation matrix to identity and
//!    zeroes TRX/TRY/TRZ, so the `RTPT` that follows is a pure perspective
//!    divide of those view-space corners.
//!
//! So the camera matrix multiplies the centre and never touches the
//! half-extents. In battle that matrix is the base matrix at `0x8007BF10`,
//! `16384 * I` in every catalogued battle savestate = a **4x uniform scale**
//! composed under the camera rotation (`BATTLE_WORLD_SCALE`). A port that
//! offsets the corners in *world* space and then draws the quad under the
//! same scaled MVP puts the half-extents through that 4x as well, so every
//! effect sprite in battle comes out exactly `BATTLE_WORLD_SCALE` too large -
//! a 32-texel puff spanning 1280 view units instead of 320, i.e. most of an
//! actor's height instead of a fifth of it.
//!
//! [`world_half_extents`] is the correction: the size a host must use when it
//! builds the quad in world space under a camera that composes `view_scale`.

/// The world-space half-extents of one effect billboard, given the pass-2
/// sprite size (`atlas w/h * sprite_scale >> 8`, already in retail view units)
/// and the uniform scale the drawing camera composes ahead of the projection.
///
/// `view_scale = 1.0` outside battle (the field cameras compose no scale), and
/// `BATTLE_WORLD_SCALE` on the battle stage. A non-finite or non-positive
/// scale is treated as `1.0` so a degenerate camera cannot blow the quad up.
///
/// REF: FUN_800195a8
pub fn world_half_extents(size: [f32; 2], view_scale: f32) -> (f32, f32) {
    let s = if view_scale.is_finite() && view_scale > 0.0 {
        view_scale
    } else {
        1.0
    };
    (size[0] * 0.5 / s, size[1] * 0.5 / s)
}

#[cfg(test)]
mod tests {
    use super::world_half_extents;

    /// The field case is the identity: no camera scale, so the pass-2 size is
    /// already the world size.
    #[test]
    fn unit_scale_is_half_the_sprite_size() {
        assert_eq!(world_half_extents([320.0, 240.0], 1.0), (160.0, 120.0));
    }

    /// The battle case: retail's 4x base matrix scales the centre only, so a
    /// world-space quad has to shrink by the same factor to project at the
    /// retail size. A 32-texel puff is 320 view units, i.e. +/-40 world units
    /// under the 4x stage camera - about a fifth of a 425-unit actor, not
    /// three quarters of one.
    #[test]
    fn battle_scale_divides_the_half_extent() {
        assert_eq!(world_half_extents([320.0, 320.0], 4.0), (40.0, 40.0));
    }

    /// A degenerate camera scale must not produce an infinite quad.
    #[test]
    fn non_positive_scale_falls_back_to_unit() {
        assert_eq!(world_half_extents([320.0, 320.0], 0.0), (160.0, 160.0));
        assert_eq!(world_half_extents([320.0, 320.0], f32::NAN), (160.0, 160.0));
        assert_eq!(world_half_extents([320.0, 320.0], -4.0), (160.0, 160.0));
    }
}
