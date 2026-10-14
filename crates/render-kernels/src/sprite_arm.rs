//! The move-VM actor's draw words, decoded the way the render dispatcher
//! reads them: the `+0x74` colour word with its `+0x78` depth-cue level
//! ([`ColourWord`]) and the `0x4000` sprite arm's one textured quad
//! ([`sprite_arm_quad`]).
//!
//! Every surface that draws a move-VM part reads these two - the summon
//! scene graph and the effect arms in `engine-effects`, the Baka Fighter
//! impact effect in `engine-minigames` - so they sit here, beside the prim
//! dispatcher ([`crate::prim_dispatch`]) that consumes the colour word.

use legaia_engine_vm::move_vm::ActorState;

/// The colour word `+0x74` and depth-cue level `+0x78` the draw hands the
/// prim dispatcher `FUN_80043390`, decoded the way that routine decodes
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourWord {
    /// Bit 31 - the GP0 ABE bit the dispatcher ORs into every packet
    /// (`srl s6,a1,0x1f; sll s6,s6,0x19` at `0x80043510`).
    pub semi: bool,
    /// `(a1 >> 24) & 3` - the ABR mode ORed into the packets' tpage bits
    /// (`0x80043504..0x8004350C`).
    pub abr: u8,
    /// The low 24 bits - the GTE far colour the depth cue blends toward
    /// (`ctc2` into `RFC/GFC/BFC` at `0x800434C8..0x800434D0`).
    pub far: [u8; 3],
    /// The depth-cue level, `IR0` for every fog-bank prim (`lwc2 t0,
    /// -0x2DC(t2)` in the bank handlers) - `+0x78`, with bit 0 forced when
    /// the mode byte is non-zero (`ori a2,a2,1` at `0x800433CC`).
    pub ir0: u16,
    /// Mode bit `0x20` (`lui s1,0x2000` / `and` at `0x800434EC..0x800434F0`):
    /// the dispatcher selects handler bank 3 (`ori s2,s2,0xF0`), whose
    /// gouraud-textured quad leaf `FUN_80045BB4` (kind 19) draws each quad as
    /// sixteen `POLY_GT3`s instead of one `GT4` ([`subdivide_quad`]). Bank 3
    /// shares every other kind's leaf with bank 1.
    pub split_quads: bool,
}

impl ColourWord {
    /// Decode the part's `+0x74` word and `+0x78` level.
    pub fn of(st: &ActorState) -> Self {
        let w = st.field_74;
        let mode = (w >> 24) as u8;
        let mut ir0 = st.field_78;
        if mode != 0 {
            ir0 |= 1;
        }
        ColourWord {
            semi: w >> 31 != 0,
            abr: mode & 3,
            far: [w as u8, (w >> 8) as u8, (w >> 16) as u8],
            ir0: ir0.min(0x1000),
            split_quads: mode & 0x20 != 0,
        }
    }

    /// A packet colour after the depth cue: `c + (far - c) * IR0 >> 12`
    /// per channel (the GTE `DPCS` the fog banks run).
    pub fn cue(&self, c: [u8; 3]) -> [u8; 3] {
        let ir0 = i32::from(self.ir0);
        std::array::from_fn(|k| {
            let (c, f) = (i32::from(c[k]), i32::from(self.far[k]));
            (c + (((f - c) * ir0) >> 12)).clamp(0, 255) as u8
        })
    }
}

/// One vertex of a quad the bank-3 leaf splits: position, colour, texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitVertex {
    pub pos: [i16; 3],
    pub rgb: [u8; 3],
    pub uv: [u8; 2],
}

/// PORT: FUN_80045BB4 - the bank-3 `GT4` leaf's split of one quad (corners
/// in packet order `0..3`, triangles `(0, 1, 2)` / `(2, 1, 3)`) into sixteen
/// triangles, as vertex triples in emit order.
///
/// The leaf averages the four corners into a centre (`0x80045D04..
/// 0x80045DDC`: positions `sra 2` of the summed halfwords, colour and UV
/// bytes `srl 2` of their sums), then for each of the four rim edges forms
/// the triangle centre / corner / next corner (`0x80045E0C..0x80045E6C`, the
/// corner pointer wrapping back by `0x40` on the last edge) and splits it at
/// its three edge midpoints (`0x80045E7C..0x80045F44`): colours as
/// `((a & 0xFEFEFE) >> 1) + ((b & 0xFEFEFE) >> 1)`, texels as the `srl 1` of
/// the byte sum and positions as the `sra 1` of the halfword sum. The packed
/// byte table it stages at `-0x2B4(t2)` picks each sub-triangle's three of
/// the six points: `(0, 3, 5)`, `(1, 3, 4)`, `(2, 4, 5)`, `(3, 4, 5)` - three
/// corner triangles and the middle one. Each goes out as one `POLY_GT3`
/// (code `0x34`, `0x80045DE4`); the depth cue then runs per vertex (`DPCT`
/// at `0x80045F68`) exactly as on an unsplit packet, so a caller cues the
/// returned colours itself. What the split changes on screen is the affine
/// texture map: sixteen small triangles follow the perspective far more
/// closely than one large quad.
pub fn subdivide_quad(c: [SplitVertex; 4]) -> Vec<[SplitVertex; 3]> {
    let centre = SplitVertex {
        pos: std::array::from_fn(|k| {
            (c.iter().map(|v| i32::from(v.pos[k])).sum::<i32>() >> 2) as i16
        }),
        rgb: std::array::from_fn(|k| {
            (c.iter().map(|v| u32::from(v.rgb[k])).sum::<u32>() >> 2) as u8
        }),
        uv: std::array::from_fn(|k| (c.iter().map(|v| u32::from(v.uv[k])).sum::<u32>() >> 2) as u8),
    };
    let mid = |a: SplitVertex, b: SplitVertex| SplitVertex {
        pos: std::array::from_fn(|k| ((i32::from(a.pos[k]) + i32::from(b.pos[k])) >> 1) as i16),
        rgb: std::array::from_fn(|k| (a.rgb[k] >> 1) + (b.rgb[k] >> 1)),
        uv: std::array::from_fn(|k| ((u32::from(a.uv[k]) + u32::from(b.uv[k])) >> 1) as u8),
    };
    // The quad's rim in winding order: packet corners 0, 1, 3, 2.
    let rim = [c[0], c[1], c[3], c[2]];
    let mut out = Vec::with_capacity(16);
    for e in 0..4 {
        // Points 0..2 the triangle's corners, 3..5 its edge midpoints.
        let (p0, p1, p2) = (centre, rim[e], rim[(e + 1) % 4]);
        let (p3, p4, p5) = (mid(p0, p1), mid(p1, p2), mid(p0, p2));
        out.push([p0, p3, p5]);
        out.push([p1, p3, p4]);
        out.push([p2, p4, p5]);
        out.push([p3, p4, p5]);
    }
    out
}

/// One quad the `0x4000` sprite arm builds, in the part's model space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteArmQuad {
    /// The four vertices, packet order `0..3` (the index words
    /// `0x00080000` / `0x00180010`).
    pub verts: [[i16; 3]; 4],
    /// Each vertex's texel.
    pub uvs: [[u8; 2]; 4],
    /// The CLUT word (hi half of the first UV word).
    pub clut: u16,
    /// The tpage word (hi half of the second UV word).
    pub tpage: u16,
    /// The packet colour (`+0xA0`'s low 24 bits).
    pub rgb: [u8; 3],
    /// `GT4` (`0x3C`) when `(mode >> 3) & 7 == 0`, else `FT4` (`0x2C`). The
    /// four colours are one word either way.
    pub gouraud: bool,
}

/// PORT: FUN_8002A5A4 - the render dispatcher's `0x4000` **sprite arm**:
/// one textured quad out of a draw-kind-4 node's `+0x9C` block.
///
/// `mode` is `+0x9E`; its low two bits pick the plane - `0` puts the quad in
/// XY, `1` in XZ, `2` in ZY - with the vertices at `(∓w/2, ∓h/2)` in packet
/// order (`w = +0xB4`, `h = +0xB6`, each halved toward zero -
/// `srl 0x1f; addu; sra 1`). Mode `3` reaches the vertex stores through
/// three pointers no arm initialised (`0x8002A628` falls to `0x8002A67C`
/// with `a2` still the call's count argument), so it returns `None` rather
/// than inventing a plane. The UV words are built with an **unmasked**
/// `addu` chain - `(clut << 16) + (v0 << 8) + u0`, `(tpage << 16) + (v0 << 8)
/// + u1`, `((v1 << 8) + u0) + (((v1 << 8) + u1) << 16)` - so a halfword past
/// `0xFF` carries into the byte above it, and that is what the GPU reads;
/// the port packs and unpacks the same words. `u0 = +0xA8`, `v0 = +0xAA`,
/// `u1 = +0xAC`, `v1 = +0xAE`, `tpage = +0xB0`, `clut = +0xB2`.
pub fn sprite_arm_quad(mode: u16, st: &ActorState) -> Option<SpriteArmQuad> {
    let half =
        |v: i16| -> i16 { ((i32::from(v) + ((i32::from(v) as u32 >> 31) as i32)) >> 1) as i16 };
    let hw = half(st.actor_u16(0xB4) as i16);
    let hh = half(st.actor_u16(0xB6) as i16);
    let corners = [(-hw, -hh), (hw, -hh), (-hw, hh), (hw, hh)];
    let verts = match mode & 3 {
        0 => corners.map(|(a, b)| [a, b, 0]),
        1 => corners.map(|(a, b)| [a, 0, b]),
        2 => corners.map(|(a, b)| [0, b, a]),
        _ => return None,
    };
    let u0 = u32::from(st.actor_u16(0xA8));
    let v0 = u32::from(st.actor_u16(0xAA));
    let u1 = u32::from(st.actor_u16(0xAC));
    let v1 = u32::from(st.actor_u16(0xAE));
    let tpage = u32::from(st.actor_u16(0xB0));
    let clut = u32::from(st.actor_u16(0xB2));
    let w1 = (clut << 16).wrapping_add(v0 << 8).wrapping_add(u0);
    let w2 = (tpage << 16).wrapping_add(v0 << 8).wrapping_add(u1);
    let w3 = ((v1 << 8).wrapping_add(u0)).wrapping_add(((v1 << 8).wrapping_add(u1)) << 16);
    let uv = |w: u32| [w as u8, (w >> 8) as u8];
    let colour = st.actor_u32(0xA0);
    Some(SpriteArmQuad {
        verts,
        uvs: [uv(w1), uv(w2), uv(w3), uv(w3 >> 16)],
        clut: (w1 >> 16) as u16,
        tpage: (w2 >> 16) as u16,
        rgb: [colour as u8, (colour >> 8) as u8, (colour >> 16) as u8],
        gouraud: (mode >> 3) & 7 == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uv_words_carry_like_the_addu_chain() {
        let mut st = ActorState::new();
        st.set_actor_u16(0xB4, 0x20);
        st.set_actor_u16(0xB6, 0x20);
        st.set_actor_u16(0xA8, 0x100); // u0 past a byte: carries into v
        st.set_actor_u16(0xAA, 0x20);
        st.set_actor_u16(0xAC, 0x11F);
        st.set_actor_u16(0xAE, 0x3F);
        st.set_actor_u16(0xB0, 0x15);
        st.set_actor_u16(0xB2, 0x7DC0);
        let q = sprite_arm_quad(0, &st).unwrap();
        assert_eq!(q.uvs[0], [0x00, 0x21]);
        assert_eq!(q.uvs[1], [0x1F, 0x21]);
        assert_eq!(q.uvs[2], [0x00, 0x40]);
        assert_eq!(q.uvs[3], [0x1F, 0x40]);
        assert!(sprite_arm_quad(3, &st).is_none(), "mode 3 has no plane");
        assert_eq!(sprite_arm_quad(1, &st).unwrap().verts[0], [-0x10, 0, -0x10]);
        assert_eq!(sprite_arm_quad(2, &st).unwrap().verts[1], [0, -0x10, 0x10]);
    }

    #[test]
    fn the_depth_cue_lerps_toward_the_far_colour() {
        let c = ColourWord {
            semi: true,
            abr: 1,
            far: [0, 0, 0],
            ir0: 0x800,
            split_quads: false,
        };
        assert_eq!(c.cue([0x80, 0x40, 0xFF]), [0x40, 0x20, 0x7F]);
    }

    fn sv(x: i16, z: i16, g: u8, u: u8, v: u8) -> SplitVertex {
        SplitVertex {
            pos: [x, 0, z],
            rgb: [g, g, g],
            uv: [u, v],
        }
    }

    /// One quad through the leaf's arithmetic: the centre is the `>> 2` of
    /// the corner sums, each midpoint the `>> 1` of its pair (colours with
    /// each half's low bit dropped first), sixteen triangles tiling the quad.
    #[test]
    fn a_bank_3_quad_splits_into_sixteen_interpolated_triangles() {
        let q = [
            sv(0, 0, 0x00, 0, 0),
            sv(64, 0, 0x41, 63, 0),
            sv(0, 65, 0x41, 0, 63),
            sv(64, 65, 0x81, 63, 63),
        ];
        let t = subdivide_quad(q);
        assert_eq!(t.len(), 16);
        // Centre: positions and bytes `>> 2` of the corner sums.
        let c = t[0][0];
        assert_eq!(c.pos, [32, 0, 32]);
        assert_eq!(c.rgb, [((0x41u32 + 0x41 + 0x81) >> 2) as u8; 3]);
        assert_eq!(c.uv, [31, 31]);
        // Edge 0 (centre, corner 0, corner 1): point 3 = mid(centre, corner 0).
        assert_eq!(t[0][1].pos, [16, 0, 16]);
        assert_eq!(t[0][1].uv, [15, 15]);
        assert_eq!(t[0][1].rgb, [c.rgb[0] >> 1; 3]);
        // Point 4 of edge 0: the rim midpoint of corners 0 and 1.
        assert_eq!(t[1][2].pos, [32, 0, 0]);
        assert_eq!(t[1][2].uv, [31, 0]);
        assert_eq!(t[1][2].rgb, [0x41 >> 1; 3]);
        // Edge 2 runs corner 3 -> corner 2: each half drops its low bit.
        assert_eq!(t[9][2].rgb, [(0x81 >> 1) + (0x41 >> 1); 3]);
        assert_eq!(t[9][2].pos, [32, 0, 65]);
        // The sixteen triangles tile the quad (truncated centre aside).
        let area: i32 = t
            .iter()
            .map(|[a, b, c]| {
                let (ax, az) = (i32::from(a.pos[0]), i32::from(a.pos[2]));
                ((i32::from(b.pos[0]) - ax) * (i32::from(c.pos[2]) - az)
                    - (i32::from(c.pos[0]) - ax) * (i32::from(b.pos[2]) - az))
                    .abs()
            })
            .sum();
        assert!((area - 2 * 64 * 65).abs() <= 16, "{area}");
    }
}
