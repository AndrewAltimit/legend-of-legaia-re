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

// ---------------------------------------------------------------------------
// Chained streak ribbon
// ---------------------------------------------------------------------------

// The ribbon is the sibling of `build_afterimage_quad`. Both are reached from
// the same move-FX draw dispatcher - `0x801e0ca0` calls the single-quad
// afterimage, `0x801e0cd0` calls the ribbon - and both take the same
// trail-texture id from the move-power record's `+0x0b` byte, so they share
// `TEXPAGE`, `CLUT_BASE` and `MODULATION_COLOR`.
//
// Where the afterimage emits *one* billboard, the ribbon emits a *chain*: the
// projected billboard becomes the bottom segment, and each further segment
// re-uses the previous segment's top edge as its own bottom edge, so the quads
// form a continuous strip climbing up the screen until it leaves the top.
// Every segment gets a shared horizontal wobble, so the strip snakes.
//
// Retail allocates each `POLY_FT4` out of the frame packet arena
// (`ctx->[0x8c] += 0x28`) and links it with `addPrim` (`FUN_8003d2c4`) at the
// *same* OT bucket for every segment - the depth `FUN_800195a8` returned for
// the projected billboard. Arena allocation and OT linking belong to the
// retail software renderer, which engine-render replaces with its own draw
// ordering; what is ported here is the geometry, the jitter law and the packet
// field assembly.

/// Billboard half-width handed to the GTE projector by the ribbon caller
/// (constant `0x100`, unlike the afterimage's dynamic half-width).
pub const RIBBON_PROJECTION_HALF_WIDTH: i16 = 0x100;

/// Billboard half-height handed to the GTE projector by the ribbon caller
/// (constant `0x200`).
pub const RIBBON_PROJECTION_HALF_HEIGHT: i16 = 0x200;

/// The ribbon is suppressed entirely when the projected top edge spans this
/// many pixels or more (retail `slti 0x41` on `x1 - x0`, then a bare return).
/// The packet already carved out of the arena is simply never linked.
pub const RIBBON_MAX_TOP_EDGE_SPAN: i16 = 0x41;

/// Floor applied to the segment height (retail keeps the projected height
/// only when it is `>= 0x40`, otherwise substitutes `0x40`). It is a floor,
/// not a cap: a tall billboard yields tall segments.
pub const RIBBON_MIN_SEGMENT_HEIGHT: i32 = 0x40;

/// Port-side guard only - retail has no iteration limit, it just walks the
/// baseline off the top of the screen. With the `0x40` height floor and
/// 16-bit screen coordinates the retail loop cannot exceed ~1024 segments,
/// so this bound is unreachable for any projection a real frame produces.
const RIBBON_MAX_SEGMENTS: usize = 2048;

/// Project the ribbon's bottom-segment corners exactly as `FUN_801e1d98`
/// does: the move actor's point unshifted, half-width
/// [`RIBBON_PROJECTION_HALF_WIDTH`], half-height
/// [`RIBBON_PROJECTION_HALF_HEIGHT`], no in-plane spin. Unlike the afterimage
/// path there is no `+0x120` Y push - the ribbon is anchored on the actor.
#[allow(clippy::too_many_arguments)]
pub fn project_ribbon_corners(
    rot: &crate::gte::GteMat3,
    trans: crate::gte::GteVec3,
    actor_point: (i16, i16, i16),
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
            actor_point.1 as i32,
            actor_point.2 as i32,
        ),
        RIBBON_PROJECTION_HALF_WIDTH,
        RIBBON_PROJECTION_HALF_HEIGHT,
        0,
        h,
        ofx,
        ofy,
        ot_shift,
    )
}

/// UV assembly shared by every ribbon segment: `band` picks one of four
/// `0x20`-wide texture sub-columns, the quad samples `band ..= band|0x1f`
/// horizontally and `0 ..= 0x3f` vertically, in `TL, TR, BL, BR` corner order.
///
/// This is *not* the afterimage's mapping - [`build_afterimage_quad`] puts the
/// `|0x1f` edge on corners 0/1 (a horizontal mirror) and the `0x3f` row on
/// corners 1/3. Do not fold the two.
fn ribbon_uv(band: u8) -> [(u8, u8); 4] {
    [
        (band, 0x00),
        (band | 0x1f, 0x00),
        (band, 0x3f),
        (band | 0x1f, 0x3f),
    ]
}

/// PORT: FUN_801e1d98 (battle move-FX chained streak ribbon)
///
/// Build the full chain of ribbon segments from the four projected corners of
/// the bottom billboard, the move's trail-texture id, and an injected random
/// source.
///
/// Returns the segments in retail emission order (bottom first, climbing).
/// An empty vector means the ribbon was suppressed by the
/// [`RIBBON_MAX_TOP_EDGE_SPAN`] test - retail draws nothing at all in that
/// case, it does not fall back to a single quad.
///
/// `rng` is drawn **seven** times for the bottom segment (shared `x0/x1`
/// wobble, shared `x2/x3` wobble, then `y0`, `y1`, `y2`, `y3` independently,
/// then the brightness band) and **four** times per further segment (shared
/// `x0/x1` wobble, then `y0`, `y1`, then the band). Feed the BIOS-`rand`
/// sequence to reproduce a capture.
pub fn build_streak_ribbon(
    corners: [(i16, i16); 4],
    trail_id: u8,
    mut rng: impl FnMut() -> u32,
) -> Vec<AfterimageQuad> {
    // Retail reads the projected corners back out of the packet it just
    // filled, so every comparison below is on the 16-bit stored values.
    let (x0, y0) = corners[0];
    let (x1, _) = corners[1];
    let (_, y2) = corners[2];

    // Suppression test: signed 16-bit top-edge span (`lh` on both).
    if x1.wrapping_sub(x0) >= RIBBON_MAX_TOP_EDGE_SPAN {
        return Vec::new();
    }

    // Segment height: `lhu` on both, 32-bit subtract, floored at 0x40 after a
    // *sign-extended* comparison. Retail keeps the un-sign-extended 32-bit
    // difference when it passes, which is why `seg` and `seg16` are tracked
    // separately - the shift-derived jitter magnitudes come off the
    // sign-extended copy, the baseline walk off the raw one.
    let raw = (y2 as u16 as i32).wrapping_sub(y0 as u16 as i32);
    let seg: i32 = if (raw as i16 as i32) < RIBBON_MIN_SEGMENT_HEIGHT {
        RIBBON_MIN_SEGMENT_HEIGHT
    } else {
        raw
    };
    let seg16 = seg as i16 as i32;

    // `s6 = seg << 16`; retail reads the jitter magnitudes back out of it with
    // `sra 17/18/19`, i.e. arithmetic halving of the sign-extended height.
    let half = seg16 >> 1;
    let quarter = seg16 >> 2;
    let eighth = seg16 >> 3;

    let clut = CLUT_BASE.wrapping_add(trail_id as u16);
    let mut out: Vec<AfterimageQuad> = Vec::new();

    // ---- bottom segment ------------------------------------------------
    let mut xy = corners;

    // One shared wobble moves the whole top edge: [-quarter, +quarter].
    let dx_top = (rng() as i32 % (half + 1)) - quarter;
    xy[0].0 = xy[0].0.wrapping_add(dx_top as i16);
    xy[1].0 = xy[1].0.wrapping_add(dx_top as i16);

    // A second, smaller shared wobble moves the whole bottom edge.
    let dx_bottom = (rng() as i32 % (quarter + 1)) - eighth;
    xy[2].0 = xy[2].0.wrapping_add(dx_bottom as i16);
    xy[3].0 = xy[3].0.wrapping_add(dx_bottom as i16);

    // Each of the four Y coordinates gets its own draw, in corner order.
    for corner in xy.iter_mut() {
        let dy = (rng() as i32 % (quarter + 1)) - eighth;
        corner.1 = corner.1.wrapping_add(dy as i16);
    }

    // Brightness band. Retail takes a signed `% 4` here and a `& 3` in the
    // loop below; the BIOS `rand` returns 0..0x7fff so the two agree.
    let band = ((rng() % 4) << 5) as u8;
    out.push(AfterimageQuad {
        xy,
        uv: ribbon_uv(band),
        clut,
        tpage: TEXPAGE,
        color: MODULATION_COLOR,
        semi_transparent: true,
    });

    // ---- chain ---------------------------------------------------------
    // Running state carried across segments: the post-jitter top edge (which
    // becomes the next segment's bottom edge) and the un-jittered baseline Y,
    // which steps up by one segment height per iteration.
    let mut top_left = xy[0];
    let mut top_right_x = xy[1].0;
    let mut top_right_y = xy[1].1;
    let mut baseline: i32 = (y0 as u16 as i32).wrapping_sub(seg);

    let span = 2 * seg16 + 1;
    let bound = -seg16;

    while (baseline as i16 as i32) > bound && out.len() < RIBBON_MAX_SEGMENTS {
        // The previous top edge is this segment's bottom edge, verbatim.
        let mut seg_xy = [(0i16, 0i16); 4];
        seg_xy[2] = top_left;
        seg_xy[3] = (top_right_x, top_right_y);

        // Shared horizontal wobble over the full segment height: [-seg, +seg].
        let dx = (rng() as i32 % span) - seg;
        let nx0 = top_left.0.wrapping_add(dx as i16);
        let nx1 = top_right_x.wrapping_add(dx as i16);

        // Both new top corners hang off the stepped baseline independently.
        let ny0 = (baseline + (rng() as i32 % (half + 1)) - quarter) as i16;
        let ny1 = (baseline + (rng() as i32 % (half + 1)) - quarter) as i16;

        seg_xy[0] = (nx0, ny0);
        seg_xy[1] = (nx1, ny1);

        let band = ((rng() & 3) << 5) as u8;
        out.push(AfterimageQuad {
            xy: seg_xy,
            uv: ribbon_uv(band),
            clut,
            tpage: TEXPAGE,
            color: MODULATION_COLOR,
            semi_transparent: true,
        });

        top_left = (nx0, ny0);
        top_right_x = nx1;
        top_right_y = ny1;
        baseline = baseline.wrapping_sub(seg);
    }

    out
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

    // ---- chained streak ribbon (FUN_801e1d98) -------------------------

    /// Corners in the projector's order (TL, TR, BL, BR) for a 32x64
    /// billboard whose bottom edge sits at y = 200.
    fn ribbon_corners() -> [(i16, i16); 4] {
        [(100, 136), (132, 136), (100, 200), (132, 200)]
    }

    #[test]
    fn ribbon_suppressed_when_top_edge_spans_0x41_or_more() {
        // x1 - x0 == 0x41 -> retail returns without linking anything.
        let wide = [(0, 0), (0x41, 0), (0, 0x40), (0x41, 0x40)];
        assert!(build_streak_ribbon(wide, 0, || 0).is_empty());

        // 0x40 is still inside the window.
        let ok = [(0, 0), (0x40, 0), (0, 0x40), (0x40, 0x40)];
        assert!(!build_streak_ribbon(ok, 0, || 0).is_empty());
    }

    #[test]
    fn ribbon_segment_height_is_floored_not_capped() {
        // A 16px-tall billboard still steps by the 0x40 floor, so the chain
        // from y=16 reaches the top of the screen in one extra segment.
        let short = [(0, 0), (8, 0), (0, 16), (8, 16)];
        let quads = build_streak_ribbon(short, 0, || 0);
        // baseline starts at 0 - 0x40 = -0x40, which is not > -0x40, so the
        // chain is the bottom segment alone.
        assert_eq!(quads.len(), 1);

        // A 0x100-tall billboard keeps its own height rather than being cut
        // back to 0x40 - the step is four times as long, so a chain anchored
        // at the same y is four times shorter.
        let tall = [(0, 0x100), (8, 0x100), (0, 0x200), (8, 0x200)];
        assert_eq!(build_streak_ribbon(tall, 0, || 0).len(), 2);
        let floored = [(0, 0x100), (8, 0x100), (0, 0x140), (8, 0x140)];
        assert_eq!(build_streak_ribbon(floored, 0, || 0).len(), 5);
    }

    #[test]
    fn ribbon_segments_share_edges_with_their_predecessor() {
        let quads = build_streak_ribbon(ribbon_corners(), 0, || 0);
        assert!(quads.len() >= 2);
        for pair in quads.windows(2) {
            // Segment n+1's bottom edge (corners 2, 3) is segment n's top
            // edge (corners 0, 1), verbatim - that is what makes it a ribbon.
            assert_eq!(pair[1].xy[2], pair[0].xy[0]);
            assert_eq!(pair[1].xy[3], pair[0].xy[1]);
        }
    }

    #[test]
    fn ribbon_draw_counts_are_seven_then_four_per_segment() {
        let mut count = 0usize;
        let quads = build_streak_ribbon(ribbon_corners(), 0, || {
            count += 1;
            0
        });
        assert_eq!(count, 7 + 4 * (quads.len() - 1));
    }

    #[test]
    fn ribbon_bottom_segment_wobbles_the_two_edges_independently() {
        // seg = 200 - 136 = 0x40, so half = 0x20, quarter = 0x10,
        // eighth = 8. Top wobble = draw % 0x21 - 0x10, bottom = % 0x11 - 8.
        let draws = [0x20, 0x10, 0, 0, 0, 0, 0];
        let mut i = 0;
        let quads = build_streak_ribbon(ribbon_corners(), 0, || {
            let v = draws.get(i).copied().unwrap_or(0);
            i += 1;
            v
        });
        let b = quads[0];
        // Top edge: both corners shift by +0x10, together.
        assert_eq!(b.xy[0].0, 100 + 0x10);
        assert_eq!(b.xy[1].0, 132 + 0x10);
        // Bottom edge: both corners shift by 0x10 % 0x11 - 8 = +8, together.
        assert_eq!(b.xy[2].0, 100 + 8);
        assert_eq!(b.xy[3].0, 132 + 8);
        // Y draws are all zero -> every corner drops by `eighth`.
        assert_eq!(b.xy[0].1, 136 - 8);
        assert_eq!(b.xy[2].1, 200 - 8);
    }

    #[test]
    fn ribbon_uv_mapping_is_not_the_afterimage_mapping() {
        // band draw 3 -> 0x60 on both paths, but the corner assignment is
        // mirrored between them; folding the two would flip the texture.
        let mut draws = [0u32; 7];
        draws[6] = 3;
        let mut i = 0;
        let quads = build_streak_ribbon(ribbon_corners(), 0, || {
            let v = draws.get(i).copied().unwrap_or(0);
            i += 1;
            v
        });
        assert_eq!(
            quads[0].uv,
            [(0x60, 0x00), (0x7f, 0x00), (0x60, 0x3f), (0x7f, 0x3f)]
        );

        let mut a_draws = [0u32; 9];
        a_draws[8] = 3;
        let a = build_afterimage_quad([(0, 0); 4], 0, seq(&a_draws));
        assert_ne!(a.uv, quads[0].uv);
    }

    #[test]
    fn ribbon_shares_the_afterimage_packet_constants() {
        let quads = build_streak_ribbon(ribbon_corners(), 0x0b, || 0);
        for q in &quads {
            assert_eq!(q.clut, 0x770b);
            assert_eq!(q.tpage, TEXPAGE);
            assert_eq!(q.color, MODULATION_COLOR);
            assert!(q.semi_transparent);
        }
    }

    #[test]
    fn ribbon_chain_climbs_and_terminates() {
        // A bottom edge low on a 240-line screen produces one segment per
        // 0x40 of height plus the bottom one, and always terminates.
        let quads = build_streak_ribbon([(100, 176), (132, 176), (100, 240), (132, 240)], 0, || 0);
        // baseline: 112, 48, -16 all pass `> -0x40`; -80 stops the walk.
        assert_eq!(quads.len(), 1 + 3);
        assert!(quads.last().unwrap().xy[0].1 < quads[0].xy[0].1);
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
