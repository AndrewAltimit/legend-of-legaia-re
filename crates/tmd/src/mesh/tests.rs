use super::*;
use crate::parse;

/// Same synthetic pyramid as the parser test: 4-vert base + apex,
/// 4 triangles + 1 quad. Build the bytes inline so the test doesn't
/// reach into the parser test module.
fn synth_pyramid_tmd() -> Vec<u8> {
    let mut buf = Vec::new();
    // Header
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    // Object table: prims start at offset 28 (right after table), then
    // verts. Synthetic prim section has 1 group of 4 FT3 prims (count=4
    // flags=0x20 olen=7 ilen=5) for 8 + 4*20 = 88 bytes, plus a 20-byte
    // footer slot, plus a 4-byte terminator = 112 bytes total.
    let prim_top: u32 = 28;
    let prim_size: u32 = 8 + (4 + 1) * 20 + 4; // 112
    let vert_top: u32 = prim_top + prim_size; // 140
    buf.extend_from_slice(&vert_top.to_le_bytes()); // vert_top
    buf.extend_from_slice(&5u32.to_le_bytes()); // n_vert
    buf.extend_from_slice(&0u32.to_le_bytes()); // normal_top
    buf.extend_from_slice(&0u32.to_le_bytes()); // n_normal
    buf.extend_from_slice(&prim_top.to_le_bytes()); // prim_top
    buf.extend_from_slice(&4u32.to_le_bytes()); // n_primitive
    buf.extend_from_slice(&0i32.to_le_bytes()); // scale
    // Group header: count=4 flags=0x20 olen=7 ilen=5 flag=1 mode=0x27
    buf.extend_from_slice(&4u16.to_le_bytes());
    buf.extend_from_slice(&0x0020u16.to_le_bytes());
    buf.extend_from_slice(&[7, 5, 1, 0x27]);
    // 4 prims of 20 bytes each, vertex indices at byte offset 14 (raw
    // byte-offset = array_idx * 8). Apex-fan: (4,0,1) (4,1,2) (4,2,3) (4,3,0)
    let fan: [(u16, u16, u16); 4] = [(4, 0, 1), (4, 1, 2), (4, 2, 3), (4, 3, 0)];
    for (a, b, c) in fan {
        let mut prim = vec![0u8; 20];
        prim[14..16].copy_from_slice(&(a * 8).to_le_bytes());
        prim[16..18].copy_from_slice(&(b * 8).to_le_bytes());
        prim[18..20].copy_from_slice(&(c * 8).to_le_bytes());
        buf.extend_from_slice(&prim);
    }
    // Footer slot (one extra prim-stride of zeros).
    buf.extend_from_slice(&[0u8; 20]);
    // Terminator u32.
    buf.extend_from_slice(&0u32.to_le_bytes());
    // Vertices: 4 base @ y=85, apex at (0, -170, 0).
    let verts = [
        (64i16, 85i16, 0i16),
        (0, 85, -64),
        (-64, 85, 0),
        (0, 85, 64),
        (0, -170, 0),
    ];
    for (x, y, z) in verts {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&z.to_le_bytes());
        buf.extend_from_slice(&0i16.to_le_bytes());
    }
    buf
}

/// One untextured gouraud triangle (`G3`, flags 0x1D): 3 colour words at
/// the prim start, then 3 vertex indices. Exercises the colour-mesh path.
fn synth_untextured_g3_tmd() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // magic
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes()); // 1 object
    let prim_top: u32 = 28;
    let prim_size: u32 = 8 + (1 + 1) * 20 + 4; // 1 prim + footer + terminator
    let vert_top: u32 = prim_top + prim_size;
    buf.extend_from_slice(&vert_top.to_le_bytes()); // vert_top
    buf.extend_from_slice(&3u32.to_le_bytes()); // n_vert
    buf.extend_from_slice(&0u32.to_le_bytes()); // normal_top
    buf.extend_from_slice(&0u32.to_le_bytes()); // n_normal
    buf.extend_from_slice(&prim_top.to_le_bytes()); // prim_top
    buf.extend_from_slice(&1u32.to_le_bytes()); // n_primitive
    buf.extend_from_slice(&0i32.to_le_bytes()); // scale
    // Group: count=1 flags=0x1D (G3) olen=5 ilen=5 flag=0 mode=0x31.
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0x001Du16.to_le_bytes());
    buf.extend_from_slice(&[5, 5, 0, 0x31]);
    // Prim: [0..4) red, [4..8) green, [8..12) blue, [12..18) verts 0,1,2.
    let mut prim = vec![0u8; 20];
    prim[0..4].copy_from_slice(&[0xFF, 0x00, 0x00, 0x34]);
    prim[4..8].copy_from_slice(&[0x00, 0xFF, 0x00, 0x34]);
    prim[8..12].copy_from_slice(&[0x00, 0x00, 0xFF, 0x34]);
    for (i, &raw) in [0u16, 8, 16].iter().enumerate() {
        prim[12 + i * 2..14 + i * 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim);
    buf.extend_from_slice(&[0u8; 20]); // footer slot
    buf.extend_from_slice(&0u32.to_le_bytes()); // terminator
    for (x, y, z) in [(0i16, 0i16, 0i16), (64, 0, 0), (0, 64, 0)] {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&z.to_le_bytes());
        buf.extend_from_slice(&0i16.to_le_bytes());
    }
    buf
}

#[test]
fn color_mesh_from_untextured_prim() {
    let buf = synth_untextured_g3_tmd();
    let tmd = parse(&buf).unwrap();
    let cm = tmd_to_color_mesh(&tmd, &buf);
    assert!(!cm.is_empty());
    assert_eq!(cm.positions.len(), 3);
    assert_eq!(cm.indices, vec![0, 1, 2]);
    // Per-vertex gouraud colours (RGB, code byte dropped).
    assert_eq!(cm.colors, vec![[0xFF, 0, 0], [0, 0xFF, 0], [0, 0, 0xFF]]);
    // The textured VRAM builder drops this prim (no UVs) -> empty mesh,
    // which is exactly why the colour path exists.
    let vm = tmd_to_vram_mesh(&tmd, &buf);
    assert!(vm.indices.is_empty());
}

#[test]
fn color_mesh_skips_textured_prims() {
    // The FT3 pyramid is all textured -> the colour mesh is empty.
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    assert!(tmd_to_color_mesh(&tmd, &buf).is_empty());
}

#[test]
fn pyramid_to_mesh() {
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    let mesh = tmd_to_mesh(&tmd, &buf);
    assert_eq!(mesh.vertex_count(), 5);
    assert_eq!(mesh.triangle_count(), 4); // 4 FT3 fan tris
    assert_eq!(mesh.indices.len(), 12);
    // Apex (vertex 4) is in every triangle.
    for tri in mesh.indices.chunks_exact(3) {
        assert!(tri.contains(&4u32), "expected apex (4) in tri {:?}", tri);
    }
}

#[test]
fn aabb_pyramid() {
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    let mesh = tmd_to_mesh(&tmd, &buf);
    let (lo, hi) = mesh.aabb();
    assert_eq!(lo, [-64.0, -170.0, -64.0]);
    assert_eq!(hi, [64.0, 85.0, 64.0]);
}

#[test]
fn vram_mesh_pyramid_has_per_corner_verts() {
    // Synth pyramid prims are FT3 (flags=0x20) - the parser decodes a
    // texture block with all-zero UVs/CBA/TSB. tmd_to_vram_mesh emits
    // 4 prims × 3 corners = 12 verts, one per (prim, corner) pair.
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    let vmesh = tmd_to_vram_mesh(&tmd, &buf);
    assert_eq!(vmesh.positions.len(), 12);
    assert_eq!(vmesh.uvs.len(), 12);
    assert_eq!(vmesh.cba_tsb.len(), 12);
    assert_eq!(vmesh.indices.len(), 12);
    assert_eq!(vmesh.triangle_count(), 4);
    // The fixture group's mode byte is 0x27 (ABE set), so the builder
    // packs the semi-transparency enable into TSB bit 15 on top of the
    // all-zero on-disc CBA/TSB.
    for ct in &vmesh.cba_tsb {
        assert_eq!(*ct, [0, TSB_SEMI_TRANSPARENT_BIT]);
    }
}

#[test]
fn pack_tsb_semi_packs_bit15_only() {
    // Sets the bit when ABE is on, leaves decode fields alone.
    assert_eq!(pack_tsb_semi(0x001A, true), 0x801A);
    assert_eq!(pack_tsb_semi(0x001A, false), 0x001A);
    // Clears on-disc garbage in bit 15 when ABE is off.
    assert_eq!(pack_tsb_semi(0x801A, false), 0x001A);
    // All used TSB fields (bits 0..=8) survive the round trip.
    assert_eq!(pack_tsb_semi(0x01FF, true) & 0x01FF, 0x01FF);
}

#[test]
fn empty_object_produces_empty_mesh() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    let tmd = parse(&buf).unwrap();
    let mesh = tmd_to_mesh(&tmd, &buf);
    assert_eq!(mesh.vertex_count(), 0);
    assert_eq!(mesh.triangle_count(), 0);
}

#[test]
fn posed_mesh_applies_translation_per_bone() {
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();

    // Shift object 0 by (+100, +200, +300).
    let offset: [i16; 3] = [100, 200, 300];
    let no_rot: [i16; 3] = [0, 0, 0];
    let bone_offsets = [(offset, no_rot)];

    let unposed = tmd_to_vram_mesh(&tmd, &buf);
    let posed = tmd_to_vram_mesh_posed(&tmd, &buf, &bone_offsets);

    assert_eq!(
        unposed.positions.len(),
        posed.positions.len(),
        "vertex count should match between posed and unposed"
    );

    for (u, p) in unposed.positions.iter().zip(posed.positions.iter()) {
        assert!(
            (p[0] - u[0] - 100.0).abs() < 0.5,
            "x should shift by 100: unposed={}, posed={}",
            u[0],
            p[0]
        );
        assert!(
            (p[1] - u[1] - 200.0).abs() < 0.5,
            "y should shift by 200: unposed={}, posed={}",
            u[1],
            p[1]
        );
        assert!(
            (p[2] - u[2] - 300.0).abs() < 0.5,
            "z should shift by 300: unposed={}, posed={}",
            u[2],
            p[2]
        );
    }
}

#[test]
fn posed_mesh_zero_offset_matches_unposed() {
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    let bone_offsets = [([0i16; 3], [0i16; 3])];

    let unposed = tmd_to_vram_mesh(&tmd, &buf);
    let posed = tmd_to_vram_mesh_posed(&tmd, &buf, &bone_offsets);

    for (u, p) in unposed.positions.iter().zip(posed.positions.iter()) {
        assert_eq!(u, p, "zero offset should not move vertices");
    }
}

#[test]
fn rot_zyx_identity_and_axis_quarter_turns() {
    let id = rot_zyx([3.0, 5.0, 7.0], 1.0, 0.0, 1.0, 0.0, 1.0, 0.0);
    assert_eq!(id, [3.0, 5.0, 7.0], "all-zero angles = identity");

    // 90 deg about Z (cz=0, sz=1): (x,y,z) -> (-y, x, z).
    let rz = rot_zyx([1.0, 0.0, 0.0], 1.0, 0.0, 1.0, 0.0, 0.0, 1.0);
    assert!((rz[0] - 0.0).abs() < 1e-5 && (rz[1] - 1.0).abs() < 1e-5 && rz[2].abs() < 1e-5);

    // 90 deg about X (cx=0, sx=1): (x,y,z) -> (x, -z, y).
    let rx = rot_zyx([0.0, 1.0, 0.0], 0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    assert!(rx[0].abs() < 1e-5 && rx[1].abs() < 1e-5 && (rx[2] - 1.0).abs() < 1e-5);
}

#[test]
fn posed_rot_zero_rotation_matches_translation_posed() {
    // With zero rotation the rigid-transform builder must reduce exactly to
    // the translation-only builder (so battle posing and field posing agree
    // at the rest orientation).
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    let bone = [([100i16, 200, 300], [0i16; 3])];

    let t_only = tmd_to_vram_mesh_posed(&tmd, &buf, &bone);
    let rigid = tmd_to_vram_mesh_posed_rot(&tmd, &buf, &bone);

    assert_eq!(t_only.positions.len(), rigid.positions.len());
    for (a, b) in t_only.positions.iter().zip(rigid.positions.iter()) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1e-3, "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn posed_rot_quarter_turn_z_rotates_the_aabb() {
    // A 90-deg Z turn (rz = 4096/4 = 1024) maps (x,y,z) -> (-y, x, z), so
    // the pyramid's tall -Y apex swings onto +X. Check the posed AABB.
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();
    let bone = [([0i16; 3], [0i16, 0, 1024])];
    let posed = tmd_to_vram_mesh_posed_rot(&tmd, &buf, &bone);
    let (lo, hi) = posed.aabb();
    // verts rotate to x in [-85,170], y in [-64,64], z in [-64,64].
    assert!((lo[0] - -85.0).abs() < 0.5, "lo.x {}", lo[0]);
    assert!((hi[0] - 170.0).abs() < 0.5, "hi.x {}", hi[0]);
    assert!(
        (lo[1] - -64.0).abs() < 0.5 && (hi[1] - 64.0).abs() < 0.5,
        "y {lo:?} {hi:?}"
    );
}

#[test]
fn posed_mesh_empty_offsets_matches_unposed() {
    let buf = synth_pyramid_tmd();
    let tmd = parse(&buf).unwrap();

    let unposed = tmd_to_vram_mesh(&tmd, &buf);
    let posed = tmd_to_vram_mesh_posed(&tmd, &buf, &[]);

    for (u, p) in unposed.positions.iter().zip(posed.positions.iter()) {
        assert_eq!(u, p, "empty offsets should not move vertices");
    }
}
