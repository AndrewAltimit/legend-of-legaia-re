//! The tile shatter's projection: the intro GTE frame, the corner table and the per-tile quad emit.

use crate::gte::{
    GteMat3, GteVec3, ScreenXY, avsz4_with_scale, gte_divide, gte_persp_term, nclip, psx_cos,
    psx_sin,
};
use crate::screen_prim::{ScreenPrim, ScreenQuad};

// ---------------------------------------------------------------------------
// The tile shatter's projection (the dispatcher work around `FUN_801D0E54`)
// ---------------------------------------------------------------------------

/// GTE screen-centre offsets during the transition, in whole pixels.
/// `OFY = 114` is **not** `240 / 2` - pinned by the GTE control file of nine
/// save states (`crates/mednafen/tests/gte_projection_real.rs`).
pub const INTRO_OFX: i32 = 160;
/// See [`INTRO_OFX`].
pub const INTRO_OFY: i32 = 114;

/// GTE focal length the tile tick loads (`li a0,0x80` at `0x801D0D30` into
/// `FUN_8003D254`).
pub const INTRO_H: u16 = 0x80;

/// The FT4 handler's near cutoff: a primitive whose `AVSZ4` OT depth lands
/// below this is dropped (`sub v1,s2,t4; bltz` at `0x80043AF0`). The value
/// is the scratch halfword `0x1F80037E`, read as `0x10` on every frame of a
/// live shatter capture.
pub const INTRO_NEAR_OTZ: i32 = 0x10;

/// `ZSF4` during the transition: the dispatcher writes
/// `0x400 >> (_DAT_1F8003A4 & 0x1F)` and the shift is `0` in the live
/// capture, so `AVSZ4` yields `(sz0+sz1+sz2+sz3) >> 2` - the plain average.
pub const INTRO_ZSF4: i32 = 0x400;

/// Overlay VA of the tile seeder's corner-offset table (`0x801CE8BC`, four
/// words) inside PROT 0979. `parse_tile_corner_table` reads it; the values
/// decode to `[0, 1, 17, 18]` - one step right / one row down in the
/// `17 x 17` vertex grid.
pub const TILE_CORNER_TABLE_VA: u32 = 0x801C_E8BC;

/// The intro overlay's PROT entry (the transition styles' data tables).
pub const INTRO_OVERLAY_PROT: u32 = 979;

/// The tile seeder's corner offsets when PROT 0979 is not to hand - the
/// values [`TILE_CORNER_TABLE_VA`] decodes to, so a disc-free host seeds the
/// same grid.
pub const DEFAULT_TILE_CORNERS: [i32; 4] = [0, 1, 0x11, 0x12];

/// A raw PROT 0979 entry relocated to its load base, with that base - the
/// form the two in-overlay table readers take. `None` when the static
/// overlay map has no base for the entry or the image does not relocate.
pub fn intro_overlay_as_loaded(raw: &[u8]) -> Option<(Vec<u8>, u32)> {
    let rec = legaia_asset::static_overlay::overlay_map().by_prot_index(INTRO_OVERLAY_PROT)?;
    let as_loaded = legaia_asset::static_overlay::as_loaded(raw, rec).ok()?;
    Some((as_loaded, rec.base_va))
}

/// Read the tile corner-offset table out of a PROT 0979 image relocated to
/// its load base. `None` when the image is too short.
pub fn parse_tile_corner_table(as_loaded: &[u8], base_va: u32) -> Option<[i32; 4]> {
    let off = TILE_CORNER_TABLE_VA.checked_sub(base_va)? as usize;
    let bytes = as_loaded.get(off..off + 16)?;
    Some(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

/// The SCUS Euler-triple → GTE rotation kernel `FUN_80026988`, exactly as
/// the disassembly composes it: `Rx(x) * Ry(y) * Rz(z)` with every product
/// truncated to q3.12 **per term** (each `mult` is followed by its own
/// `sra 12`; the mixed second-row/third-row terms are two-step products, so
/// they truncate twice). Angles are 12-bit (`& 0xFFF`), sin/cos from the
/// same table [`psx_sin`] reproduces.
///
/// PORT: FUN_80026988
pub fn euler_rot_psx(angles: (i16, i16, i16)) -> GteMat3 {
    let ang = |a: i16| (a as u16) & 0xFFF;
    let (cx, sx) = (psx_cos(ang(angles.0)), psx_sin(ang(angles.0)));
    let (cy, sy) = (psx_cos(ang(angles.1)), psx_sin(ang(angles.1)));
    let (cz, sz) = (psx_cos(ang(angles.2)), psx_sin(ang(angles.2)));
    let q = |v: i32| v >> 12;
    // The two shared two-step products (`iVar3` / `iVar2` in the decomp).
    let a = q(cz * -sy);
    let b = q(sz * -sy);
    let e = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    GteMat3 {
        m: [
            [e(q(cz * cy)), e(q(-(sz * cy))), e(sy)],
            [
                e(q(sz * cx) - q(a * sx)),
                e(q(cz * cx) + q(b * sx)),
                e(q(-(cy * sx))),
            ],
            [
                e(q(sz * sx) + q(a * cx)),
                e(q(cz * sx) - q(b * cx)),
                e(q(cy * cx)),
            ],
        ],
    }
}

/// One projected corner: the saturated SXY plus the SZ depth bucket.
#[derive(Debug, Clone, Copy)]
pub(super) struct ProjCorner {
    pub(super) xy: ScreenXY,
    pub(super) sz: i32,
}

/// `RTPS` for one intro-packet corner: rotate by the packet's own matrix,
/// translate by the record position (the per-record MVMVA result - the
/// transition's view matrix is identity rotation with zero translation from
/// the second frame on, pinned live), perspective-divide through the UNR
/// reciprocal at [`INTRO_H`], and offset by the transition's screen centre.
/// Shared by the tile shatter, the two particle fields, the swirl and the
/// spin-up ring - all five run on the same transition GTE file.
pub(super) fn project_intro_corner(rot: &GteMat3, tr: GteVec3, c: (i16, i16, i16)) -> ProjCorner {
    let v = rot.mul_vec(GteVec3::new(i32::from(c.0), i32::from(c.1), i32::from(c.2)));
    let (x, y, z) = (v.x + tr.x, v.y + tr.y, v.z + tr.z);
    // SZ3 saturates to 0..0xFFFF; the divide then saturates at the
    // behind-camera bound exactly as hardware does.
    let sz = z.clamp(0, 0xFFFF);
    let (recip, _overflow) = gte_divide(INTRO_H, sz as u16);
    let ir = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
    let sx = INTRO_OFX + gte_persp_term(ir(x), recip) as i32;
    let sy = INTRO_OFY + gte_persp_term(ir(y), recip) as i32;
    ProjCorner {
        xy: ScreenXY::new(sx, sy).saturate_sxy(),
        sz,
    }
}

/// Project one tile's ten-face packet and append the survivors to `prims`.
///
/// The accept chain is the FT4 handler's (`FUN_800439E4`, the slot-17 leaf
/// `FUN_80043390` dispatches this packet to): `NCLIP` over corners 0-2, a
/// second `NCLIP` over 1-3 after the fourth `RTPS`, **accept unless
/// `nclip1 <= 0 && nclip2 >= 0`**. A planar quad's two strip triangles wind
/// oppositely, so the straight orientation (`n1 > 0`) passes and the reversed
/// one (`n1 < 0, n2 > 0`) rejects - the faces are single-sided, which is why
/// the packet's back face reverses its corner order relative to the front:
/// each culls exactly when it faces away. Then `AVSZ4` for the OT depth
/// and the [`INTRO_NEAR_OTZ`] cutoff. Faces are appended in packet order;
/// [`crate::screen_prim::order_primitives`]'s tie-break (later-submitted
/// draws first) then reproduces `AddPrim`'s prepend, which is what lands the
/// shade set on top of its opaque siblings within a bucket.
pub fn emit_tile(
    rec: &legaia_engine_vm::battle_intro_tiles::TileRecord,
    prims: &mut Vec<ScreenPrim>,
) {
    use legaia_engine_vm::battle_intro_tiles::tile_face_quads;
    let Some(quads) = tile_face_quads(rec) else {
        return;
    };
    let rot = euler_rot_psx(rec.angles);
    let tr = GteVec3::new(
        i32::from(rec.pos.0),
        i32::from(rec.pos.1),
        i32::from(rec.pos.2),
    );
    for q in &quads {
        let p: [ProjCorner; 4] =
            std::array::from_fn(|i| project_intro_corner(&rot, tr, q.corners[i]));
        let grey = u32::from(q.grey);
        push_ft4_quad(
            &p,
            q.uv,
            q.clut,
            q.tpage,
            grey << 16 | grey << 8 | grey,
            q.semi_transparent,
            CullMode::SingleSided,
            prims,
        );
    }
}

/// Backface handling of one dispatched packet - the dispatcher's NCLIP mask.
///
/// `FUN_80043390` decodes bit 27 of its second argument into the mask the
/// kind handlers AND over each NCLIP result (`0x80043520..0x80043540`):
/// clear = `0xFFFFFFFF` (the plain single-sided accept), set = `0x7FFFFFFF`
/// (the sign bit is stripped, so `n <= 0 && n >= 0` can only reject a
/// degenerate quad - effectively double-sided). The tile shatter passes `0`
/// there and culls; the swirl passes `0x18808080` and does not, which is what
/// lets its x-mirrored half - whose winding is reversed - draw at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CullMode {
    SingleSided,
    DoubleSided,
}

/// The FT4 handler's accept chain (`FUN_800439E4` and its fog-bank sibling
/// share it): `NCLIP` over corners 0-2, a second `NCLIP` over 1-3 after the
/// fourth `RTPS`, reject on `n1 <= 0 && n2 >= 0` (through the [`CullMode`]
/// mask), then `AVSZ4` for the OT depth and the [`INTRO_NEAR_OTZ`] cutoff.
/// Returns whether the quad was pushed.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_ft4_quad(
    p: &[ProjCorner; 4],
    uv: [(u8, u8); 4],
    clut: u16,
    tpage: u16,
    color: u32,
    semi_transparent: bool,
    cull: CullMode,
    prims: &mut Vec<ScreenPrim>,
) -> bool {
    if cull == CullMode::SingleSided {
        let n1 = nclip(p[0].xy, p[1].xy, p[2].xy);
        let n2 = nclip(p[1].xy, p[2].xy, p[3].xy);
        if n1 <= 0 && n2 >= 0 {
            return false;
        }
    }
    let otz = avsz4_with_scale(p[0].sz, p[1].sz, p[2].sz, p[3].sz, INTRO_ZSF4);
    if otz < INTRO_NEAR_OTZ {
        return false;
    }
    prims.push(ScreenPrim::Textured(ScreenQuad {
        xy: std::array::from_fn(|i| (p[i].xy.x as i16, p[i].xy.y as i16)),
        uv,
        clut,
        tpage,
        color,
        gouraud: None,
        semi_transparent,
        ot_index: otz as u32,
        depth: None,
    }));
    true
}
