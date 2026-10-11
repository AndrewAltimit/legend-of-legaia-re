//! The arena geometry: backdrop hybrid and ring filters, the second copy, ground grid and checker floor.

use legaia_scene_formats::scene_tmd_stream;
use legaia_tmd::mesh::VramMesh;

/// The arena shell as a hybrid VRAM mesh plus its packet-colour stream,
/// with TMD object 1 (the wall-base dust decal) dropped: its texels are
/// genuinely bright, and the retail match capture shows a mist-free
/// interior, so retail's backdrop path does not draw it as static geometry.
pub fn arena_hybrid(buf: &[u8]) -> Option<(VramMesh, Vec<u8>)> {
    let stream = scene_tmd_stream::detect(buf)?;
    let tmd_bytes = buf.get(stream.tmd_range())?;
    let tmd = legaia_tmd::parse(tmd_bytes).ok()?;
    let (mesh, oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
    let flat = legaia_engine_field::packet_color::hybrid(&mesh, &shading);
    if !oids.iter().any(|&o| o != 0) {
        return Some((mesh, flat));
    }
    let keep: Vec<bool> = oids.iter().map(|&o| o == 0).collect();
    Some(filter_mesh(&mesh, &flat, &keep))
}

/// The arena as retail draws it: [`arena_hybrid`]'s half-shell twice.
/// `FUN_800513F0` registers the backdrop TMD once and spawns two backdrop
/// actors from it - copy A at raw coordinates, copy B under
/// [`ARENA_SECOND_COPY`] - which closes the half-stage into the full ring.
/// One copy alone leaves the whole `-X` half of the arena open. Every dome
/// host draws this mesh.
pub fn arena_ring(buf: &[u8]) -> Option<(VramMesh, Vec<u8>)> {
    let (half, flat) = arena_hybrid(buf)?;
    let mut ring = half.clone();
    let mut ring_flat = flat.clone();
    append(
        &mut ring,
        &mut ring_flat,
        &second_copy(&half, ARENA_SECOND_COPY),
        &flat,
    );
    Some((ring, ring_flat))
}

fn filter_mesh(mesh: &VramMesh, flat: &[u8], keep: &[bool]) -> (VramMesh, Vec<u8>) {
    let mut out = empty_mesh();
    let mut remap = vec![u32::MAX; keep.len()];
    let mut flat2 = Vec::new();
    for (i, &k) in keep.iter().enumerate() {
        if !k {
            continue;
        }
        remap[i] = out.positions.len() as u32;
        out.positions.push(mesh.positions[i]);
        out.uvs.push(mesh.uvs[i]);
        out.cba_tsb.push(mesh.cba_tsb[i]);
        out.normals.push(mesh.normals[i]);
        out.colors.push(mesh.colors[i]);
        flat2.extend_from_slice(&flat[i * 4..i * 4 + 4]);
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| remap[i as usize]);
        if a != u32::MAX && b != u32::MAX && c != u32::MAX {
            out.indices.extend_from_slice(&[a, b, c]);
        }
    }
    (out, flat2)
}

/// The transform the arena's second backdrop copy takes.
///
/// Retail picks it from the `SCUS_942.54` mirror list at `DAT_80078B50`,
/// keyed by `word[0x80084540] + byte[0x8007BD60]`. The dome contest leaves
/// both at `3` (the retail `minigame_muscle_dome` state), and backdrop id `6`
/// is not on the list, so the copy takes the default half turn. The shell
/// is open toward `-X` and only about a third of it is symmetric in `z`, so
/// the two transforms put the furniture in visibly different places.
pub const ARENA_SECOND_COPY: legaia_scene_formats::battle_backdrop::SecondCopy =
    legaia_scene_formats::battle_backdrop::SecondCopy::HalfTurn;

/// `mesh` under a backdrop second-copy transform, its winding restored when
/// the transform reflects.
fn second_copy(
    mesh: &VramMesh,
    copy: legaia_scene_formats::battle_backdrop::SecondCopy,
) -> VramMesh {
    let k = copy.scale();
    let mut out = mesh.clone();
    for p in &mut out.positions {
        *p = [p[0] * k[0], p[1] * k[1], p[2] * k[2]];
    }
    for n in &mut out.normals {
        *n = [n[0] * k[0], n[1] * k[1], n[2] * k[2]];
    }
    if copy.flips_winding() {
        for t in out.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
    }
    out
}

/// Append `src` (with its `flat` stream) onto `dst`.
pub(super) fn append(dst: &mut VramMesh, dst_flat: &mut Vec<u8>, src: &VramMesh, flat: &[u8]) {
    let base = dst.positions.len() as u32;
    dst.positions.extend_from_slice(&src.positions);
    dst.uvs.extend_from_slice(&src.uvs);
    dst.cba_tsb.extend_from_slice(&src.cba_tsb);
    dst.normals.extend_from_slice(&src.normals);
    dst.colors.extend_from_slice(&src.colors);
    dst.indices.extend(src.indices.iter().map(|i| i + base));
    dst_flat.extend_from_slice(flat);
}

pub(super) fn empty_mesh() -> VramMesh {
    VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    }
}

/// Quads onto a mesh: `[x0, z0, x1, z1]` at floor height 0, one UV rect and
/// one `[cba, tsb]` pair, `[r, g, b, textured]` fill.
fn push_floor_quad(
    m: &mut VramMesh,
    flat: &mut Vec<u8>,
    rect: [f32; 4],
    uv: [u8; 4],
    ct: [u16; 2],
    rgba: [u8; 4],
) {
    let base = m.positions.len() as u32;
    let [x0, z0, x1, z1] = rect;
    let [u0, v0, u1, v1] = uv;
    m.positions
        .extend_from_slice(&[[x0, 0.0, z0], [x1, 0.0, z0], [x1, 0.0, z1], [x0, 0.0, z1]]);
    m.uvs
        .extend_from_slice(&[[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
    for _ in 0..4 {
        m.cba_tsb.push(ct);
        m.normals.push([0.0, -1.0, 0.0]);
        m.colors.push([rgba[0], rgba[1], rgba[2]]);
        flat.extend_from_slice(&rgba);
    }
    m.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The retail battle ground grid: 28x28 cells of `0x200`, each four `0x100`
/// sub-quads over the 32x32 texel windows at `(192, 192)` of the `(832, 0)`
/// page, CLUT `(0, 479)`.
pub(super) fn ground_grid(m: &mut VramMesh, flat: &mut Vec<u8>) {
    const CELL: f32 = 512.0;
    const SUB: f32 = 256.0;
    const N: i32 = 14;
    for cz in -N..N {
        for cx in -N..N {
            for sr in 0..2u8 {
                for sc in 0..2u8 {
                    let x0 = cx as f32 * CELL + f32::from(sc) * SUB;
                    let z0 = cz as f32 * CELL + f32::from(sr) * SUB;
                    let u0 = 192 + sc * 32;
                    let v0 = 192 + sr * 32;
                    push_floor_quad(
                        m,
                        flat,
                        [x0, z0, x0 + SUB, z0 + SUB],
                        [u0, v0, u0 + 31, v0 + 31],
                        [0x77C0, 0x000D],
                        [128, 128, 128, 255],
                    );
                }
            }
        }
    }
}

/// The fallback floor without an arena: a dark checkerboard sized off the
/// fighters' spacing.
pub(super) fn checker_floor(m: &mut VramMesh, flat: &mut Vec<u8>, extent: f32) {
    let t = (extent / 4.0).round().max(160.0);
    const N: i32 = 12;
    for iz in -N..N {
        for ix in -N..N {
            let c = if (ix + iz) & 1 == 0 {
                [34, 36, 44, 0]
            } else {
                [48, 52, 62, 0]
            };
            let x0 = ix as f32 * t;
            let z0 = iz as f32 * t;
            push_floor_quad(m, flat, [x0, z0, x0 + t, z0 + t], [0; 4], [0, 0], c);
        }
    }
}
