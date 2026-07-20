//! 2D afterimage / motion-streak primitive builder.
//!
//! PORT: FUN_801e1ab0 (battle move-FX afterimage streak draw)
//!
//! NOT WIRED: no host builds a streak quad. The *destination* exists - a quad
//! converts through [`crate::screen_overlay::afterimage_screen_quad`] and
//! would draw via [`crate::RenderTarget::ScreenOverlay`] - but that converter
//! has no non-test caller either, so the pass is a road with nothing on it.
//! Two concrete gaps sit between here and a visible trail:
//!
//! * **The half-width has no source.** [`streak_half_width`] reads the retail
//!   move-FX state halfword at `+0x6c6`, and `legaia_engine_core`'s move-FX
//!   scene-graph does not model that word - there is no value to pass.
//! * **The host consumes the trail id as a log line.** engine-shell reads
//!   [`legaia_engine_core::World::active_move_fx_trail_texpage`] and prints
//!   it; nothing turns it into a per-frame streak pass over the move-FX part
//!   positions, which is what emits one quad per call the way retail does.
//!
//! The arithmetic below is unit-tested against the retail draw order and is
//! the part that would be reused unchanged once a caller exists; do not
//! delete it.
//!
//! Each call to the retail `FUN_801e1ab0` emits **one** semi-transparent
//! textured quad (a PSX `POLY_FT4`) into the depth-sorted primitive buffer;
//! the per-frame streak is built by calling it repeatedly. The quad is a
//! billboard sprite positioned at the move actor's screen point (`+0x120`
//! pixels down, `0x100`-unit half-size) and jittered per corner so the
//! after-images shimmer.
//!
//! Two of the three things the retail function touches are out of clean-room
//! scope and stay on the caller's side:
//!
//! * **`FUN_80056798`** is the BIOS `rand()` thunk (`li t1,0x2f; jr 0xa0`);
//!   this kernel takes an injected `rng` so the *consumption* of the random
//!   draws is what is ported (and unit-testable), not the BIOS LCG itself.
//! * **`FUN_800195a8`** is the GTE billboard projection (view-space quad
//!   corners ±half-size, optional Z-rotation, then a perspective divide). It
//!   is ported as [`crate::billboard::project_billboard`];
//!   [`project_streak_corners`] reproduces this caller's exact invocation of
//!   it. `build_afterimage_quad` still accepts pre-projected corners so a
//!   capture replay can feed recorded SXYs directly.
//!
//! What this module ports is the genuinely move-FX-specific arithmetic: the
//! per-corner positional jitter, the random brightness band that selects a
//! texture sub-column, and the exact `POLY_FT4` field assembly (UVs, CLUT,
//! texpage, modulation colour, semi-transparency). Linking the finished packet
//! into the ordering table (`FUN_8003d2c4` at depth `FUN_800195a8`'s return)
//! is the retail software OT, which engine-render replaces with its own draw
//! ordering.
//!
//! **What the draw path would be.** The screen-space `POLY_FT4` /
//! ordering-table primitive path the retail streak needs exists as
//! [`crate::screen_overlay`]: an [`AfterimageQuad`] converts to a
//! [`crate::screen_overlay::ScreenQuad`] via
//! [`crate::screen_overlay::afterimage_screen_quad`], would be ordered
//! back-to-front with the rest of the frame's screen primitives, and drawn
//! through [`crate::RenderTarget::ScreenOverlay`] against the shared PSX VRAM.
//! That machinery is built and tested; what is missing is the per-frame
//! emitter described in the NOT WIRED note above. Live effect billboards
//! currently draw as 3D meshes via engine-shell's `effect_billboard_mesh`
//! instead, which is why nothing has needed this 2D path yet.

/// Units the source point is pushed down (+Y) before projection
/// (retail adds `0x120` to the Y of `*(_DAT_8007bd24 + 0x1144)`). Applied
/// projection-side; [`project_streak_corners`] folds it in.
pub const SCREEN_Y_OFFSET: i16 = 0x120;

/// Billboard half-HEIGHT handed to the GTE projection (`0x100`, the constant
/// third argument of the `FUN_800195a8` call). The half-WIDTH is dynamic:
/// the move-FX state halfword at `+0x6c6` minus `0x200` - see
/// [`streak_half_width`].
pub const PROJECTION_HALF_SIZE: i16 = 0x100;

/// Derive the streak's billboard half-width from the move-FX state halfword
/// at `+0x6c6` (retail passes `*(short*)(state + 0x6c6) - 0x200` as the
/// projector's half-width argument, 16-bit wrapping).
pub fn streak_half_width(state_word: i16) -> i16 {
    state_word.wrapping_sub(0x200)
}

/// Project the four streak-quad corners exactly as `FUN_801e1ab0` does:
/// center = the move actor's point with [`SCREEN_Y_OFFSET`] added to Y,
/// half-height [`PROJECTION_HALF_SIZE`], no in-plane spin. The returned
/// corner order feeds [`build_afterimage_quad`] (and the retail packet's
/// `xy0..xy3`) directly; the returned `depth` is the OT bucket the retail
/// caller links the packet at.
#[allow(clippy::too_many_arguments)]
pub fn project_streak_corners(
    rot: &crate::gte::GteMat3,
    trans: crate::gte::GteVec3,
    actor_point: (i16, i16, i16),
    half_w: i16,
    h: i32,
    ofx: i32,
    ofy: i32,
    ot_shift: u32,
) -> crate::billboard::BillboardCorners {
    crate::billboard::project_billboard(
        rot,
        trans,
        crate::gte::GteVec3::new(
            actor_point.0 as i32,
            actor_point.1.wrapping_add(SCREEN_Y_OFFSET) as i32,
            actor_point.2 as i32,
        ),
        half_w,
        PROJECTION_HALF_SIZE,
        0,
        h,
        ofx,
        ofy,
        ot_shift,
    )
}

/// 24-bit modulation colour baked into the command word `0x2e808080` - a
/// neutral 50%-grey so the texel passes through the semi-transparency blend
/// unmodulated.
pub const MODULATION_COLOR: u32 = 0x0080_8080;

/// GP0 texpage attribute written to the vertex-1 slot (`0x27`): texpage X base
/// 7 (`×64`), Y base 0, semi-transparency mode 1, 4-bpp CLUT lookup.
pub const TEXPAGE: u16 = 0x0027;

/// Base of the CLUT framebuffer word; the trail-texture id is added to select
/// the column. `0x7700` decodes to framebuffer CLUT (X = id, Y = 476) - the
/// row-476..479 trail/NPC CLUT band.
pub const CLUT_BASE: u16 = 0x7700;

/// One semi-transparent textured quad (`POLY_FT4`) emitted by the afterimage
/// streak pass, with every GPU-packet field resolved. The retail packet order
/// is `[tag][color][xy0][uv0|clut][xy1][uv1|tpage][xy2][uv2][xy3][uv3]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterimageQuad {
    /// Four screen-space corners `(x, y)` after per-corner jitter, in the
    /// retail vertex order (0,1,2,3).
    pub xy: [(i16, i16); 4],
    /// Four `(u, v)` texel coordinates. The `u` columns are picked by the
    /// random brightness band; the quad samples a `0x1f`-wide × `0x3f`-tall
    /// region.
    pub uv: [(u8, u8); 4],
    /// GP0 CLUT field (vertex-0 slot): [`CLUT_BASE`] `+ trail_id`.
    pub clut: u16,
    /// GP0 texpage field (vertex-1 slot): the constant [`TEXPAGE`].
    pub tpage: u16,
    /// 24-bit modulation colour ([`MODULATION_COLOR`]).
    pub color: u32,
    /// Always `true` - command `0x2e` is a semi-transparent textured quad.
    pub semi_transparent: bool,
}

/// Build one afterimage quad from four projected screen corners, the move's
/// trail-texture id (`move-power +0x0b`), and an injected random source.
///
/// `rng` is called exactly **nine** times, in the retail draw order: four X
/// jitters (corners 0,1,2,3), four Y jitters (corners 0,1,2,3), then the
/// brightness band. Only the low bits documented per draw are consumed, so any
/// uniform source works; feed the BIOS-`rand` sequence to reproduce a capture
/// byte-for-byte.
pub fn build_afterimage_quad(
    corners: [(i16, i16); 4],
    trail_id: u8,
    mut rng: impl FnMut() -> u32,
) -> AfterimageQuad {
    let mut xy = corners;

    // X jitter: every corner shares `-2 + rand % 5`, i.e. a [-2, +2] wobble.
    for corner in xy.iter_mut() {
        let dx = -2i16 + (rng() % 5) as i16;
        corner.0 = corner.0.wrapping_add(dx);
    }

    // Y jitter: top corners (0, 2) use `-8 + rand % 9` ([-8, 0]); bottom
    // corners (1, 3) use `-4 + rand % 9` ([-4, +4]). The asymmetry rakes the
    // streak upward.
    const Y_BIAS: [i16; 4] = [-8, -4, -8, -4];
    for (corner, bias) in xy.iter_mut().zip(Y_BIAS) {
        let dy = bias + (rng() % 9) as i16;
        corner.1 = corner.1.wrapping_add(dy);
    }

    // Brightness band: `(rand & 3) << 5` picks one of four 0x20-wide texture
    // sub-columns (0x00 / 0x20 / 0x40 / 0x60). Corners 0,1 sample the right
    // edge of the band (`| 0x1f`); corners 2,3 sample the left edge.
    let band = ((rng() & 3) << 5) as u8;
    let uv = [
        (band | 0x1f, 0x00),
        (band | 0x1f, 0x3f),
        (band, 0x00),
        (band, 0x3f),
    ];

    AfterimageQuad {
        xy,
        uv,
        clut: CLUT_BASE.wrapping_add(trail_id as u16),
        tpage: TEXPAGE,
        color: MODULATION_COLOR,
        semi_transparent: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic rng over a fixed sequence; panics if drained so a test
    /// that consumes the wrong number of draws fails loudly.
    fn seq(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i];
            i += 1;
            v
        }
    }

    #[test]
    fn all_zero_draws_apply_min_jitter_and_base_band() {
        let corners = [(100, 200), (110, 200), (100, 260), (110, 260)];
        let q = build_afterimage_quad(corners, 0, seq(&[0; 9]));

        // X: -2 each. Y bias only: [-8, -4, -8, -4].
        assert_eq!(q.xy[0], (98, 192));
        assert_eq!(q.xy[1], (108, 196));
        assert_eq!(q.xy[2], (98, 252));
        assert_eq!(q.xy[3], (108, 256));

        // band 0 -> uv columns 0x1f / 0x00.
        assert_eq!(
            q.uv,
            [(0x1f, 0x00), (0x1f, 0x3f), (0x00, 0x00), (0x00, 0x3f)]
        );
    }

    #[test]
    fn jitter_uses_modulo_not_raw_draw() {
        // %5 of {5,6,7,8} = {0,1,2,3}; %9 of {9,10,11,12} = {0,1,2,3}.
        let draws = [5, 6, 7, 8, 9, 10, 11, 12, 0];
        let q = build_afterimage_quad([(0, 0); 4], 0, seq(&draws));
        // X deltas: -2+0, -2+1, -2+2, -2+3 = -2,-1,0,1
        assert_eq!([q.xy[0].0, q.xy[1].0, q.xy[2].0, q.xy[3].0], [-2, -1, 0, 1]);
        // Y deltas: -8+0, -4+1, -8+2, -4+3 = -8,-3,-6,-1
        assert_eq!(
            [q.xy[0].1, q.xy[1].1, q.xy[2].1, q.xy[3].1],
            [-8, -3, -6, -1]
        );
    }

    #[test]
    fn x_jitter_reaches_full_minus2_to_plus2_range() {
        // %5 of {2,3,4,...} = {2,3,4} -> deltas {0,1,2}; with 0 and 1 -> {-2,-1}.
        let q = build_afterimage_quad([(0, 0); 4], 0, seq(&[1, 4, 0, 0, 0, 0, 0, 0, 0]));
        // -2+1=-1, -2+4=+2, -2+0=-2, -2+0=-2
        assert_eq!(
            [q.xy[0].0, q.xy[1].0, q.xy[2].0, q.xy[3].0],
            [-1, 2, -2, -2]
        );
    }

    #[test]
    fn brightness_band_selects_uv_columns() {
        // band draw = 3 -> (3 & 3) << 5 = 0x60.
        let mut draws = [0u32; 9];
        draws[8] = 3;
        let q = build_afterimage_quad([(0, 0); 4], 0, seq(&draws));
        assert_eq!(
            q.uv,
            [(0x7f, 0x00), (0x7f, 0x3f), (0x60, 0x00), (0x60, 0x3f)]
        );

        // band draw masks to low 2 bits: 6 & 3 = 2 -> 0x40.
        draws[8] = 6;
        let q = build_afterimage_quad([(0, 0); 4], 0, seq(&draws));
        assert_eq!(q.uv[2].0, 0x40);
        assert_eq!(q.uv[0].0, 0x5f);
    }

    #[test]
    fn trail_id_offsets_clut_constants_fixed() {
        let q = build_afterimage_quad([(0, 0); 4], 0x12, seq(&[0; 9]));
        assert_eq!(q.clut, 0x7712);
        assert_eq!(q.tpage, 0x27);
        assert_eq!(q.color, 0x0080_8080);
        assert!(q.semi_transparent);
    }

    #[test]
    fn consumes_exactly_nine_draws() {
        // A 9-element sequence must be fully consumed and not over-read.
        let mut count = 0;
        let q = build_afterimage_quad([(0, 0); 4], 0, || {
            count += 1;
            0
        });
        assert_eq!(count, 9);
        let _ = q;
    }
}
