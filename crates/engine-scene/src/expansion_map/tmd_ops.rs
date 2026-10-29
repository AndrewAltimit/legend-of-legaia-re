//! Raw Legaia-TMD operations a pack map's detail layer needs: rewriting a
//! mesh's texture words in place, and baking several placed meshes into one.
//!
//! Both work on the file bytes rather than on a decoded model, so every
//! primitive row survives - the light-source rows (`0x10..=0x17`, with
//! normals) that [`legaia_tmd::encode::decode_model`] refuses included. A
//! primitive's vertex and normal indices are byte offsets into its own
//! object's arrays, so an object's primitive section copies verbatim into a
//! new file as long as the object keeps its own arrays.

use anyhow::{Context, Result, bail};
use legaia_tmd::descriptor::Descriptor;
use legaia_tmd::legaia_prims;

/// Bytes of a TMD header (`id`, `flags`, `nobj`).
const HEADER: usize = 12;
/// Bytes of one object-table entry.
const OBJECT: usize = 28;
/// Bytes of one `SVECTOR`.
const VECTOR: usize = 8;

/// One textured primitive's texture words, as [`retexture`] hands them out.
pub struct TexWords<'a> {
    pub cba: u16,
    pub tsb: u16,
    /// `(u, v)` per vertex.
    pub uvs: &'a mut [(u8, u8)],
}

/// Rewrite every textured primitive's CLUT, page and UV words in `raw`
/// through `f`, which returns the new `(cba, tsb)` and may edit the UVs.
pub fn retexture(raw: &mut [u8], mut f: impl FnMut(TexWords<'_>) -> (u16, u16)) -> Result<()> {
    let tmd = legaia_tmd::parse(raw).context("parse")?;
    for o in &tmd.objects {
        let groups =
            legaia_prims::iter_groups(raw, o.primitives_byte_offset, o.primitives_byte_size)
                .context("primitive walk")?;
        for g in &groups {
            let Some(d) = Descriptor::for_flags(g.header.flags) else {
                continue;
            };
            let (true, Some(block)) = (d.packet_shape.is_textured(), d.texture_block_offset) else {
                continue;
            };
            let n = g.header.n_vertices();
            for p in &g.prims {
                let b = p.bytes_offset + block;
                if b + 4 + n * 2 > raw.len() {
                    bail!("texture block at {b:#x} runs past the file");
                }
                // [u0 v0][cba][u1 v1][tsb][u2 v2]([u3 v3])
                let uv_at = [b, b + 4, b + 8, b + 10];
                let mut uvs: Vec<(u8, u8)> =
                    uv_at[..n].iter().map(|&o| (raw[o], raw[o + 1])).collect();
                let cba = u16::from_le_bytes([raw[b + 2], raw[b + 3]]);
                let tsb = u16::from_le_bytes([raw[b + 6], raw[b + 7]]);
                let (cba, tsb) = f(TexWords {
                    cba,
                    tsb,
                    uvs: &mut uvs,
                });
                raw[b + 2..b + 4].copy_from_slice(&cba.to_le_bytes());
                raw[b + 6..b + 8].copy_from_slice(&tsb.to_le_bytes());
                for (&o, uv) in uv_at.iter().zip(&uvs) {
                    raw[o] = uv.0;
                    raw[o + 1] = uv.1;
                }
            }
        }
    }
    Ok(())
}

/// A 3x3 rotation, row-major.
pub type Mat3 = [[f32; 3]; 3];

/// Retail's object rotation `Rx * Ry * Rz` from degrees - the composition a
/// placed object's record angles are drawn with
/// (`legaia_render_kernels::battle_intro::placement_rotation`).
pub fn rotation_deg(rot: [f32; 3]) -> Mat3 {
    let [(sx, cx), (sy, cy), (sz, cz)] = rot.map(|d| d.to_radians().sin_cos());
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mul(&mul(&rx, &ry), &rz)
}

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: &Mat3, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

/// One mesh placed into a [`bake`]: its file and where it stands in the
/// baked mesh's frame (retail field frame, Y down).
pub struct Placed<'a> {
    pub raw: &'a [u8],
    /// Translation, after rotation and scale.
    pub offset: [f32; 3],
    /// Rotation, degrees (`Rx * Ry * Rz`).
    pub rotation: [f32; 3],
    pub scale: f32,
}

/// Bake `parts` into one TMD: every object of every part, its vertices
/// carried into the shared frame (`offset + R * (scale * v)`) and its
/// normals turned with it. The objects keep their primitive sections byte
/// for byte, so the baked mesh draws exactly as its parts would side by
/// side.
///
/// Fails on a part that does not parse, a vertex the move carries out of the
/// `i16` range, or more than 1024 objects.
pub fn bake(parts: &[Placed<'_>]) -> Result<Vec<u8>> {
    struct Obj {
        prims: Vec<u8>,
        n_prim: u32,
        verts: Vec<[i16; 3]>,
        normals: Vec<[i16; 3]>,
        scale: i32,
    }
    let mut objs: Vec<Obj> = Vec::new();
    for (pi, part) in parts.iter().enumerate() {
        let tmd = legaia_tmd::parse(part.raw).with_context(|| format!("part {pi}"))?;
        let rot = rotation_deg(part.rotation);
        for (oi, o) in tmd.objects.iter().enumerate() {
            // The primitive section up to and including its terminator.
            let groups = legaia_prims::iter_groups(
                part.raw,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            )
            .with_context(|| format!("part {pi} object {oi}"))?;
            let len: usize = groups.iter().map(|g| g.header.total_bytes()).sum::<usize>() + 4;
            let start = o.primitives_byte_offset;
            let Some(prims) = part.raw.get(start..start + len) else {
                bail!("part {pi} object {oi}: primitive section runs past the file");
            };
            let mut prims = prims.to_vec();
            // A section that ran to the end of its buffer may lack the zero
            // terminator's bytes; make sure it ends in one.
            let tail = prims.len() - 4;
            prims[tail..].fill(0);
            let verts = o
                .vertices
                .iter()
                .map(|v| {
                    let p = apply(
                        &rot,
                        [f32::from(v.x), f32::from(v.y), f32::from(v.z)].map(|c| c * part.scale),
                    );
                    let w = [0, 1, 2].map(|i| (p[i] + part.offset[i]).round());
                    if w.iter().any(|c| !(-32768.0..=32767.0).contains(c)) {
                        bail!("part {pi} object {oi}: a vertex lands outside the i16 range");
                    }
                    Ok(w.map(|c| c as i16))
                })
                .collect::<Result<Vec<_>>>()?;
            let normals = o
                .normals
                .iter()
                .map(|n| {
                    let p = apply(&rot, [f32::from(n.x), f32::from(n.y), f32::from(n.z)]);
                    p.map(|c| c.round().clamp(-32768.0, 32767.0) as i16)
                })
                .collect();
            let n_prim = groups.iter().map(|g| u32::from(g.header.count)).sum();
            objs.push(Obj {
                prims,
                n_prim,
                verts,
                normals,
                scale: o.header.scale,
            });
        }
    }
    if objs.is_empty() {
        bail!("nothing to bake");
    }
    if objs.len() > 1024 {
        bail!("{} objects; a TMD holds at most 1024", objs.len());
    }
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(objs.len() as u32).to_le_bytes());
    // Offsets count from the end of the header (`FLIST_BIT`).
    let mut cursor = (objs.len() * OBJECT) as u32;
    for o in &objs {
        let prim_top = cursor;
        let vert_top = prim_top + o.prims.len() as u32;
        let normal_top = vert_top + (o.verts.len() * VECTOR) as u32;
        for w in [
            vert_top,
            o.verts.len() as u32,
            normal_top,
            o.normals.len() as u32,
            prim_top,
            o.n_prim,
        ] {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        buf.extend_from_slice(&o.scale.to_le_bytes());
        cursor = normal_top + (o.normals.len() * VECTOR) as u32;
    }
    for o in &objs {
        buf.extend_from_slice(&o.prims);
        for v in o.verts.iter().chain(&o.normals) {
            for c in v {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            buf.extend_from_slice(&0i16.to_le_bytes());
        }
    }
    debug_assert_eq!(buf.len(), HEADER + cursor as usize);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_tmd::descriptor::PacketShape;
    use legaia_tmd::encode::{LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, encode};

    fn quad(cba: u16, tsb: u16) -> Vec<u8> {
        encode(&[ModelObject {
            vertices: vec![[-10, 0, -10], [10, 0, -10], [-10, 0, 10], [10, 0, 10]],
            groups: vec![ModelGroup {
                shape: PacketShape::Ft4,
                semi_transparent: false,
                prims: vec![ModelPrim {
                    vertices: vec![0, 1, 2, 3],
                    uvs: vec![(4, 8), (20, 8), (4, 24), (20, 24)],
                    cba,
                    tsb,
                    colors: vec![[128, 128, 128]],
                }],
            }],
            scale: LEGAIA_OBJECT_SCALE,
        }])
        .unwrap()
    }

    #[test]
    fn retexture_rewrites_every_word() {
        let mut raw = quad(0x7F00, 0x001C);
        retexture(&mut raw, |t| {
            assert_eq!((t.cba, t.tsb), (0x7F00, 0x001C));
            for uv in t.uvs.iter_mut() {
                uv.0 += 100;
            }
            (0x1234, 0x0019)
        })
        .unwrap();
        let tmd = legaia_tmd::parse(&raw).unwrap();
        let o = &tmd.objects[0];
        let g = legaia_prims::iter_groups(&raw, o.primitives_byte_offset, o.primitives_byte_size)
            .unwrap();
        let p = &g[0].prims[0];
        assert_eq!((p.cba, p.tsb), (0x1234, 0x0019));
        assert_eq!(p.uvs[0], (104, 8));
        assert_eq!(p.uvs[3], (120, 24));
    }

    #[test]
    fn bake_moves_parts_and_keeps_their_prims() {
        let a = quad(0x7F00, 0x001C);
        let b = quad(0x7F10, 0x001D);
        let baked = bake(&[
            Placed {
                raw: &a,
                offset: [0.0; 3],
                rotation: [0.0; 3],
                scale: 1.0,
            },
            Placed {
                raw: &b,
                offset: [40.0, -8.0, 0.0],
                rotation: [0.0, 90.0, 0.0],
                scale: 2.0,
            },
        ])
        .unwrap();
        let tmd = legaia_tmd::parse(&baked).unwrap();
        assert_eq!(tmd.objects.len(), 2);
        let v = &tmd.objects[1].vertices[0];
        // (-10, 0, -10) * 2, turned a quarter about Y: x' = c x + s z = -20,
        // z' = -s x + c z = 20; then moved by the offset.
        assert_eq!((v.x, v.y, v.z), (20, -8, 20));
        let o = &tmd.objects[1];
        let g = legaia_prims::iter_groups(&baked, o.primitives_byte_offset, o.primitives_byte_size)
            .unwrap();
        assert_eq!(g[0].prims[0].cba, 0x7F10);
        assert_eq!(g[0].prims[0].vertex_indices(), vec![0, 1, 2, 3]);
        // The baked mesh re-bakes (its own sections are well formed).
        bake(&[Placed {
            raw: &baked,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            scale: 1.0,
        }])
        .unwrap();
    }

    #[test]
    fn bake_refuses_a_vertex_out_of_range() {
        let a = quad(0, 0x1C);
        assert!(
            bake(&[Placed {
                raw: &a,
                offset: [32760.0, 0.0, 0.0],
                rotation: [0.0; 3],
                scale: 1.0,
            }])
            .is_err()
        );
    }
}
