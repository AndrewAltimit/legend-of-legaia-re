//! Legaia TMD *encoder* - the write-side complement of [`crate::parse`] +
//! [`crate::legaia_prims`].
//!
//! Builds a byte-exact Legaia TMD (`id 0x80000002`, `FLIST_BIT` relative
//! offsets) from a typed model. The layout mirrors the retail battle_data
//! corpus exactly:
//!
//! ```text
//! [12-byte header][nobj x 28-byte object table]
//! then per object, consecutively:
//!   [primitive groups][4-byte zero terminator][vertices]
//! ```
//!
//! Section ordering per object is `prim_top < vert_top` with **zero** gap
//! between the terminator and the vertex array, and `normal_top` points at
//! the end of the vertex array with `n_normal == 0` (no monster / character
//! mesh in the corpus carries normals - they are all baked-colour rows).
//! Each group is `8-byte header + count x stride + one zero footer slot`
//! (the renderer always advances one extra slot; retail footers are all
//! zeros - verified across the Lu Delilas mesh, 30/30 groups).
//!
//! Only the **baked-colour** rows (4/5 of `DAT_8007326C`, plus the
//! untextured rows 2/3) are supported - the lit rows (0/1) need normals and
//! never occur in battle meshes. Per-shape header tuples come from the
//! descriptor table; they were also empirically confirmed against a
//! full-corpus census (8519 TMDs), including the conventions the parser
//! drops: the GP0 code byte `mode & 0xFE` rides on **every** colour word of
//! a textured prim but only on the **leading** word of an untextured
//! gouraud prim (archive-wide census across all 194 monster meshes, zero
//! exceptions), and semi-transparent (ABE) groups clear the `flags` LSB
//! while setting `mode` bit 1.

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::descriptor::PacketShape;
use crate::legaia_prims::{self, GROUP_HEADER_SIZE};
use crate::{OBJECT_SIZE, Tmd, VECTOR_SIZE};

/// The `scale` word every Legaia object carries (not a log2 scale).
pub const LEGAIA_OBJECT_SCALE: i32 = 0x0080_8080;

/// One primitive of a typed model.
///
/// Quads store their vertices in the PSX **Z-order** the file uses
/// (`v0, v1, v2, v3` with `v3` opposite `v0` - triangles `(v0,v1,v2)` +
/// `(v1,v3,v2)`), i.e. exactly what
/// [`Prim::vertex_indices`](crate::legaia_prims::Prim::vertex_indices)
/// returns. A perimeter-ordered quad `a,b,c,d` converts to `a,b,d,c`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelPrim {
    /// Vertex array indices into the owning object's vertex list
    /// (3 for triangles, 4 for quads).
    pub vertices: Vec<u16>,
    /// Per-vertex `(u, v)` texels - required (same length as `vertices`)
    /// for the `Ft*`/`Gt*` shapes, must be empty for `F*`/`G*`.
    pub uvs: Vec<(u8, u8)>,
    /// Raw CLUT base address (textured shapes only; 0 otherwise).
    pub cba: u16,
    /// Raw texture page / TSB word (textured shapes only; 0 otherwise).
    pub tsb: u16,
    /// Baked colours: exactly 1 entry for flat shapes (`F*`/`Ft*`),
    /// `n_vertices` entries for gouraud shapes (`G*`/`Gt*`).
    pub colors: Vec<[u8; 3]>,
}

/// One primitive group: a run of same-shape prims sharing a group header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelGroup {
    pub shape: PacketShape,
    /// Semi-transparency (ABE): clears the `flags` LSB and sets `mode`
    /// bit 1. Only valid on textured shapes (the untextured ABE variants
    /// never occur on the disc and the renderer path is unverified).
    pub semi_transparent: bool,
    pub prims: Vec<ModelPrim>,
}

/// One TMD object: a rigid body part posed by the animation stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelObject {
    /// `SVECTOR` positions (the stored i16 GTE space).
    pub vertices: Vec<[i16; 3]>,
    pub groups: Vec<ModelGroup>,
    /// Object scale word; retail is always [`LEGAIA_OBJECT_SCALE`].
    pub scale: i32,
}

/// Per-shape encoding row: everything the group header + prim layout needs.
/// `(flags, olen, ilen, mode)` for the non-ABE form; ABE derives as
/// `flags - 1` / `mode | 2`.
fn shape_row(shape: PacketShape) -> (u16, u8, u8, u8) {
    match shape {
        PacketShape::F3 => (0x19, 4, 3, 0x21),
        PacketShape::F4 => (0x1B, 5, 3, 0x29),
        PacketShape::G3 => (0x1D, 6, 5, 0x31),
        PacketShape::G4 => (0x1F, 8, 6, 0x39),
        PacketShape::Ft3 => (0x21, 7, 5, 0x25),
        PacketShape::Ft4 => (0x23, 9, 6, 0x2D),
        PacketShape::Gt3 => (0x25, 9, 7, 0x35),
        PacketShape::Gt4 => (0x27, 12, 9, 0x3D),
    }
}

/// Resolve a group's `flags` value back to `(shape, semi_transparent)`.
/// Returns `None` for the lit rows (0/1) and anything out of range.
fn shape_for_flags(flags: u16) -> Option<(PacketShape, bool)> {
    for shape in [
        PacketShape::F3,
        PacketShape::F4,
        PacketShape::G3,
        PacketShape::G4,
        PacketShape::Ft3,
        PacketShape::Ft4,
        PacketShape::Gt3,
        PacketShape::Gt4,
    ] {
        let (base_flags, _, _, _) = shape_row(shape);
        if flags == base_flags {
            return Some((shape, false));
        }
        if flags == base_flags - 1 && shape.is_textured() {
            return Some((shape, true));
        }
    }
    None
}

/// Number of leading colour words a shape stores.
fn color_words(shape: PacketShape) -> usize {
    if shape.is_gouraud() {
        shape.n_vertices()
    } else {
        1
    }
}

/// Encode a typed model into Legaia TMD bytes (`id 0x80000002`, `flags 0`).
pub fn encode(objects: &[ModelObject]) -> Result<Vec<u8>> {
    if objects.is_empty() {
        bail!("model has no objects");
    }
    if objects.len() > 1024 {
        bail!("implausible object count {}", objects.len());
    }

    // Validate + measure each object's sections first.
    let mut prim_section_sizes = Vec::with_capacity(objects.len());
    for (oi, o) in objects.iter().enumerate() {
        if o.vertices.len() > u16::MAX as usize / VECTOR_SIZE {
            bail!(
                "object {} has {} vertices; the u16 byte-offset index space caps at {}",
                oi,
                o.vertices.len(),
                u16::MAX as usize / VECTOR_SIZE
            );
        }
        let mut size = 4usize; // zero terminator
        for (gi, g) in o.groups.iter().enumerate() {
            let ctx = || format!("object {oi} group {gi} ({:?})", g.shape);
            if g.prims.is_empty() {
                bail!("{} is empty", ctx());
            }
            if g.prims.len() > u16::MAX as usize {
                bail!("{} has {} prims (u16 count overflow)", ctx(), g.prims.len());
            }
            if g.semi_transparent && !g.shape.is_textured() {
                bail!(
                    "{}: semi-transparent untextured groups never occur on the disc; unsupported",
                    ctx()
                );
            }
            let n_verts = g.shape.n_vertices();
            let n_colors = color_words(g.shape);
            for (pi, p) in g.prims.iter().enumerate() {
                if p.vertices.len() != n_verts {
                    bail!(
                        "{} prim {}: {} vertices, shape needs {}",
                        ctx(),
                        pi,
                        p.vertices.len(),
                        n_verts
                    );
                }
                if let Some(&bad) = p.vertices.iter().find(|&&v| v as usize >= o.vertices.len()) {
                    bail!(
                        "{} prim {}: vertex index {} out of range ({} vertices)",
                        ctx(),
                        pi,
                        bad,
                        o.vertices.len()
                    );
                }
                let want_uvs = if g.shape.is_textured() { n_verts } else { 0 };
                if p.uvs.len() != want_uvs {
                    bail!(
                        "{} prim {}: {} uvs, shape needs {}",
                        ctx(),
                        pi,
                        p.uvs.len(),
                        want_uvs
                    );
                }
                if p.colors.len() != n_colors {
                    bail!(
                        "{} prim {}: {} colour words, shape needs {}",
                        ctx(),
                        pi,
                        p.colors.len(),
                        n_colors
                    );
                }
            }
            let (_, _, ilen, _) = shape_row(g.shape);
            size += GROUP_HEADER_SIZE + (g.prims.len() + 1) * ilen as usize * 4;
        }
        prim_section_sizes.push(size);
    }

    let mut buf = Vec::new();
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(objects.len() as u32).to_le_bytes());

    // Object table - offsets are relative to the end of the 12-byte header.
    let mut cursor = (objects.len() * OBJECT_SIZE) as u32;
    for (o, &prim_size) in objects.iter().zip(&prim_section_sizes) {
        let prim_top = cursor;
        let vert_top = prim_top + prim_size as u32;
        let normal_top = vert_top + (o.vertices.len() * VECTOR_SIZE) as u32;
        let n_primitive: u32 = o.groups.iter().map(|g| g.prims.len() as u32).sum();
        buf.extend_from_slice(&vert_top.to_le_bytes());
        buf.extend_from_slice(&(o.vertices.len() as u32).to_le_bytes());
        buf.extend_from_slice(&normal_top.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&prim_top.to_le_bytes());
        buf.extend_from_slice(&n_primitive.to_le_bytes());
        buf.extend_from_slice(&o.scale.to_le_bytes());
        cursor = normal_top;
    }

    // Per-object data: groups + terminator, then vertices.
    for o in objects {
        for g in &o.groups {
            let (mut flags, olen, ilen, mut mode) = shape_row(g.shape);
            if g.semi_transparent {
                flags -= 1;
                mode |= 2;
            }
            let stride = ilen as usize * 4;
            buf.extend_from_slice(&(g.prims.len() as u16).to_le_bytes());
            buf.extend_from_slice(&flags.to_le_bytes());
            buf.extend_from_slice(&[olen, ilen, 0x01, mode]);
            let code = mode & 0xFE;
            for p in &g.prims {
                let start = buf.len();
                // Colour word(s). GP0 code-byte convention (archive-wide
                // census, no exceptions): textured shapes carry the code on
                // EVERY word; untextured gouraud only on the leading word
                // (trailing words zero); flat shapes on their single word.
                for (wi, c) in p.colors.iter().enumerate() {
                    let wcode = if wi == 0 || g.shape.is_textured() {
                        code
                    } else {
                        0
                    };
                    buf.extend_from_slice(&[c[0], c[1], c[2], wcode]);
                }
                // Texture block: [u0 v0][cba][u1 v1][tsb][u2 v2]([u3 v3]).
                if g.shape.is_textured() {
                    buf.extend_from_slice(&[p.uvs[0].0, p.uvs[0].1]);
                    buf.extend_from_slice(&p.cba.to_le_bytes());
                    buf.extend_from_slice(&[p.uvs[1].0, p.uvs[1].1]);
                    buf.extend_from_slice(&p.tsb.to_le_bytes());
                    buf.extend_from_slice(&[p.uvs[2].0, p.uvs[2].1]);
                    if let Some(uv3) = p.uvs.get(3) {
                        buf.extend_from_slice(&[uv3.0, uv3.1]);
                    }
                }
                // Vertex indices as byte offsets (index * 8).
                for &v in &p.vertices {
                    buf.extend_from_slice(&(v * VECTOR_SIZE as u16).to_le_bytes());
                }
                // Pad to the stride (F3 / G3 have a 2-byte tail).
                buf.resize(start + stride, 0);
            }
            // Footer slot: one zero-filled prim stride.
            buf.resize(buf.len() + stride, 0);
        }
        // Section terminator.
        buf.extend_from_slice(&0u32.to_le_bytes());
        for v in &o.vertices {
            buf.extend_from_slice(&v[0].to_le_bytes());
            buf.extend_from_slice(&v[1].to_le_bytes());
            buf.extend_from_slice(&v[2].to_le_bytes());
            buf.extend_from_slice(&0i16.to_le_bytes());
        }
    }

    Ok(buf)
}

/// Decode a parsed TMD back into the typed model [`encode`] consumes.
///
/// Inverse of [`encode`] for the baked-colour corpus: `decode_model` then
/// `encode` reproduces the original bytes exactly (retail footers are zero
/// and pad bytes are zero). Bails on lit rows (0/1), which carry normals
/// and never occur in battle meshes.
pub fn decode_model(tmd: &Tmd, buf: &[u8]) -> Result<Vec<ModelObject>> {
    let mut objects = Vec::with_capacity(tmd.objects.len());
    for (oi, o) in tmd.objects.iter().enumerate() {
        if o.header.n_normal != 0 {
            bail!(
                "object {} carries {} normals (lit rows); only baked-colour meshes are supported",
                oi,
                o.header.n_normal
            );
        }
        let groups =
            legaia_prims::iter_groups(buf, o.primitives_byte_offset, o.primitives_byte_size)
                .with_context(|| format!("object {oi} primitive walk"))?;
        let mut out_groups = Vec::with_capacity(groups.len());
        for g in &groups {
            let Some((shape, semi_transparent)) = shape_for_flags(g.header.flags) else {
                bail!(
                    "object {} group at +{:#x}: flags {:#06x} is not a baked-colour row",
                    oi,
                    g.header_offset,
                    g.header.flags
                );
            };
            // Cross-check the stored header tuple against the canonical row.
            let (mut want_flags, want_olen, want_ilen, mut want_mode) = shape_row(shape);
            if semi_transparent {
                want_flags -= 1;
                want_mode |= 2;
            }
            if (g.header.olen, g.header.ilen, g.header.mode) != (want_olen, want_ilen, want_mode) {
                bail!(
                    "object {} group at +{:#x}: header (olen {}, ilen {}, mode {:#04x}) \
                     diverges from the canonical {:?} row ({}, {}, {:#04x})",
                    oi,
                    g.header_offset,
                    g.header.olen,
                    g.header.ilen,
                    g.header.mode,
                    shape,
                    want_olen,
                    want_ilen,
                    want_mode
                );
            }
            debug_assert_eq!(g.header.flags, want_flags);
            let n_colors = color_words(shape);
            let mut prims = Vec::with_capacity(g.prims.len());
            for p in &g.prims {
                prims.push(ModelPrim {
                    vertices: p.vertex_indices(),
                    uvs: p.uvs.clone(),
                    cba: p.cba,
                    tsb: p.tsb,
                    // The parser replicates a flat prim's single colour word
                    // to every vertex; keep only the stored words.
                    colors: p.colors.iter().take(n_colors).copied().collect(),
                });
            }
            out_groups.push(ModelGroup {
                shape,
                semi_transparent,
                prims,
            });
        }
        objects.push(ModelObject {
            vertices: o.vertices.iter().map(|v| [v.x, v.y, v.z]).collect(),
            groups: out_groups,
            scale: o.header.scale,
        });
    }
    Ok(objects)
}

/// Convenience: total prim count of a typed model.
pub fn prim_count(objects: &[ModelObject]) -> usize {
    objects
        .iter()
        .flat_map(|o| &o.groups)
        .map(|g| g.prims.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HEADER_SIZE, parse};

    fn gt3_prim(a: u16, b: u16, c: u16) -> ModelPrim {
        ModelPrim {
            vertices: vec![a, b, c],
            uvs: vec![(0, 0), (15, 0), (0, 15)],
            cba: 0x7900,
            tsb: 0x000C,
            colors: vec![[0x80; 3], [0x60; 3], [0x50; 3]],
        }
    }

    fn sample_model() -> Vec<ModelObject> {
        vec![
            ModelObject {
                vertices: vec![[0, 0, 0], [100, 0, 0], [0, 100, 0], [100, 100, 0]],
                groups: vec![
                    ModelGroup {
                        shape: PacketShape::Gt3,
                        semi_transparent: false,
                        prims: vec![gt3_prim(0, 1, 2), gt3_prim(1, 3, 2)],
                    },
                    ModelGroup {
                        shape: PacketShape::Gt4,
                        semi_transparent: true,
                        prims: vec![ModelPrim {
                            vertices: vec![0, 1, 2, 3],
                            uvs: vec![(0, 0), (31, 0), (0, 31), (31, 31)],
                            cba: 0x7940,
                            tsb: 0x010C,
                            colors: vec![[0x80; 3]; 4],
                        }],
                    },
                ],
                scale: LEGAIA_OBJECT_SCALE,
            },
            ModelObject {
                vertices: vec![[0, 0, 0], [50, 0, 0], [0, 50, 0]],
                groups: vec![
                    ModelGroup {
                        shape: PacketShape::F3,
                        semi_transparent: false,
                        prims: vec![ModelPrim {
                            vertices: vec![0, 1, 2],
                            uvs: vec![],
                            cba: 0,
                            tsb: 0,
                            colors: vec![[0xC0, 0x40, 0x40]],
                        }],
                    },
                    ModelGroup {
                        shape: PacketShape::G3,
                        semi_transparent: false,
                        prims: vec![ModelPrim {
                            vertices: vec![2, 1, 0],
                            uvs: vec![],
                            cba: 0,
                            tsb: 0,
                            colors: vec![[0x10; 3], [0x20; 3], [0x30; 3]],
                        }],
                    },
                ],
                scale: LEGAIA_OBJECT_SCALE,
            },
        ]
    }

    #[test]
    fn encode_parse_decode_round_trips() {
        let model = sample_model();
        let bytes = encode(&model).unwrap();
        let tmd = parse(&bytes).unwrap();
        assert_eq!(tmd.header.id, 0x8000_0002);
        assert_eq!(tmd.header.flags, 0);
        assert_eq!(tmd.objects.len(), 2);
        let back = decode_model(&tmd, &bytes).unwrap();
        assert_eq!(back, model);
    }

    #[test]
    fn encode_is_deterministic_and_reencodes_byte_exact() {
        let model = sample_model();
        let bytes = encode(&model).unwrap();
        let tmd = parse(&bytes).unwrap();
        let back = decode_model(&tmd, &bytes).unwrap();
        let bytes2 = encode(&back).unwrap();
        assert_eq!(bytes, bytes2);
    }

    #[test]
    fn layout_matches_retail_conventions() {
        let model = sample_model();
        let bytes = encode(&model).unwrap();
        let tmd = parse(&bytes).unwrap();
        // prim_top < vert_top, zero gap between terminator and verts,
        // normal_top == vert_top + n_vert*8, objects consecutive.
        let mut expected_prim_top = (tmd.objects.len() * OBJECT_SIZE) as u32;
        for o in &tmd.objects {
            assert_eq!(o.header.prim_top, expected_prim_top);
            assert!(o.header.prim_top < o.header.vert_top);
            assert_eq!(
                o.header.normal_top,
                o.header.vert_top + (o.header.n_vert * VECTOR_SIZE as u32)
            );
            assert_eq!(o.header.n_normal, 0);
            assert_eq!(o.header.scale, LEGAIA_OBJECT_SCALE);
            expected_prim_top = o.header.normal_top;
        }
        // Trailing byte count: file ends exactly at the last object's normal_top.
        assert_eq!(
            bytes.len(),
            HEADER_SIZE + tmd.objects.last().unwrap().header.normal_top as usize
        );
    }

    #[test]
    fn every_colour_word_carries_the_gp0_code() {
        let model = sample_model();
        let bytes = encode(&model).unwrap();
        let tmd = parse(&bytes).unwrap();
        // Object 0 group 0 is GT3 (mode 0x35): every colour word's 4th byte
        // must be 0x34 (mode & 0xFE), for all three words.
        let o = &tmd.objects[0];
        let g0 =
            legaia_prims::iter_groups(&bytes, o.primitives_byte_offset, o.primitives_byte_size)
                .unwrap();
        let prim0 = g0[0].prims[0].bytes_offset;
        for w in 0..3 {
            assert_eq!(bytes[prim0 + w * 4 + 3], 0x34, "colour word {w}");
        }
        // Object 0 group 1 is GT4 + ABE: flags 0x26, mode 0x3F, code 0x3E.
        assert_eq!(g0[1].header.flags, 0x26);
        assert_eq!(g0[1].header.mode, 0x3F);
        let prim1 = g0[1].prims[0].bytes_offset;
        for w in 0..4 {
            assert_eq!(bytes[prim1 + w * 4 + 3], 0x3E, "colour word {w}");
        }
        // Object 1 group 1 is untextured G3 (mode 0x31): the code byte 0x30
        // rides only on the leading word; trailing words carry zero.
        let o1 = &tmd.objects[1];
        let g1 =
            legaia_prims::iter_groups(&bytes, o1.primitives_byte_offset, o1.primitives_byte_size)
                .unwrap();
        let g3prim = g1[1].prims[0].bytes_offset;
        assert_eq!(bytes[g3prim + 3], 0x30, "G3 leading colour word");
        assert_eq!(bytes[g3prim + 7], 0x00, "G3 trailing colour word 1");
        assert_eq!(bytes[g3prim + 11], 0x00, "G3 trailing colour word 2");
    }

    #[test]
    fn rejects_semi_transparent_untextured() {
        let mut model = sample_model();
        model[1].groups[0].semi_transparent = true; // F3 + ABE
        assert!(encode(&model).is_err());
    }

    #[test]
    fn rejects_out_of_range_vertex_index() {
        let mut model = sample_model();
        model[0].groups[0].prims[0].vertices[0] = 99;
        assert!(encode(&model).is_err());
    }

    #[test]
    fn rejects_wrong_uv_count() {
        let mut model = sample_model();
        model[0].groups[0].prims[0].uvs.pop();
        assert!(encode(&model).is_err());
    }

    #[test]
    fn rejects_wrong_colour_count() {
        let mut model = sample_model();
        model[0].groups[0].prims[0].colors.pop();
        assert!(encode(&model).is_err());
    }

    #[test]
    fn rejects_empty_model_and_empty_group() {
        assert!(encode(&[]).is_err());
        let mut model = sample_model();
        model[0].groups[0].prims.clear();
        assert!(encode(&model).is_err());
    }
}
