use super::*;

/// Build a section with one FT3 group: count=2, ilen=5, vertex_offset=14.
/// Vertex indices are stored as raw byte-offsets (array_idx * 8).
fn ft3_section() -> Vec<u8> {
    let mut buf = Vec::new();
    // Group header: count=2, flags=0x20, olen=7, ilen=5, flag=1, mode=0x27
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&0x0020u16.to_le_bytes());
    buf.extend_from_slice(&[7, 5, 1, 0x27]);
    // Prim 0: 20 bytes; vertex byte-offsets at bytes 14-19 for verts 0,1,2
    let mut prim0 = vec![0xAAu8; 20];
    for (vi, &raw) in [0u16, 8, 16].iter().enumerate() {
        let off = 14 + vi * 2;
        prim0[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim0);
    // Prim 1: vertex byte-offsets for verts 3,4,5
    let mut prim1 = vec![0xBBu8; 20];
    for (vi, &raw) in [24u16, 32, 40].iter().enumerate() {
        let off = 14 + vi * 2;
        prim1[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim1);
    // Footer slot (one extra ilen*4 = 20 bytes of padding)
    buf.extend_from_slice(&[0; 20]);
    // Terminator u32
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf
}

#[test]
fn iterates_one_group() {
    let buf = ft3_section();
    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g.header.count, 2);
    assert_eq!(g.header.flags, 0x20);
    assert_eq!(g.header.ilen, 5);
    assert_eq!(g.header.n_vertices(), 3);
    assert_eq!(g.prims.len(), 2);
    assert_eq!(g.prims[0].vertex_indices(), vec![0, 1, 2]);
    assert_eq!(g.prims[1].vertex_indices(), vec![3, 4, 5]);
}

#[test]
fn vertex_offset_lookup() {
    // entry 4 triangle (flags 0x20): byte4=0x07 -> 14 bytes
    assert_eq!(vertex_offset_bytes(0x20), Some(14));
    // entry 4 QUAD (flags 0x22): byte3==1 forces iVar2=8 -> 16 bytes
    assert_eq!(vertex_offset_bytes(0x22), Some(16));
    // entry 5 triangle (flags 0x24): byte4=0x0B -> 22 bytes
    assert_eq!(vertex_offset_bytes(0x24), Some(22));
    // entry 5 QUAD (flags 0x26): byte3==3 forces iVar2=0xE -> 28 bytes
    assert_eq!(vertex_offset_bytes(0x26), Some(28));
    // entry 2 triangle (flags 0x18): byte4=0x02 -> 4 bytes
    assert_eq!(vertex_offset_bytes(0x18), Some(4));
    // entry 2 QUAD (flags 0x1A): byte3==0 -> iVar2 = byte4 -> 4 bytes
    assert_eq!(vertex_offset_bytes(0x1A), Some(4));
    // Out-of-range flags
    assert_eq!(vertex_offset_bytes(0x00), None);
    assert_eq!(vertex_offset_bytes(0x100), None);
}

#[test]
fn quad_vs_triangle_classification() {
    // Triangle (flags >> 1) & 1 == 0
    let h = GroupHeader {
        count: 1,
        flags: 0x20,
        olen: 7,
        ilen: 5,
        flag: 0,
        mode: 0,
    };
    assert_eq!(h.n_vertices(), 3);
    // Quad
    let h2 = GroupHeader {
        count: 1,
        flags: 0x22,
        olen: 9,
        ilen: 6,
        flag: 0,
        mode: 0,
    };
    assert_eq!(h2.n_vertices(), 4);
}

#[test]
fn abe_is_mode_bit_1() {
    let mut h = GroupHeader {
        count: 1,
        flags: 0x20,
        olen: 7,
        ilen: 5,
        flag: 0,
        mode: 0x27, // textured tri with ABE (the common Legaia mode)
    };
    assert!(h.abe());
    h.mode = 0x25; // same shape, ABE clear
    assert!(!h.abe());
    h.mode = 0x02; // only the ABE bit
    assert!(h.abe());
}

#[test]
fn terminator_stops_iteration() {
    let mut buf = Vec::new();
    // Just a terminator
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    assert!(groups.is_empty());
}

#[test]
fn rejects_section_overrun() {
    // Truncated buffer: header says count=10 but only 8 bytes of buffer
    let buf = vec![10, 0, 0x20, 0, 7, 5, 1, 0x27];
    assert!(iter_groups(&buf, 0, buf.len()).is_err());
}

#[test]
fn lenient_returns_partial_walk_on_error() {
    // Two groups: a valid FT3 group followed by a malformed one with
    // ilen=0. Strict iter_groups bails on the second; lenient should
    // return the first.
    let mut buf = ft3_section();
    // Strip the trailing terminator and the footer slot before
    // appending the malformed group, so the malformed header lands
    // where the next group would start.
    buf.truncate(buf.len() - 4); // drop terminator
    // Append a malformed header with ilen == 0.
    buf.extend_from_slice(&1u16.to_le_bytes()); // count=1
    buf.extend_from_slice(&0x0020u16.to_le_bytes()); // flags
    buf.extend_from_slice(&[7, 0, 1, 0x27]); // ilen=0 (malformed)
    // Pad enough that the boundary check evaluates against the bad ilen.
    buf.extend_from_slice(&[0u8; 32]);

    // Strict variant rejects the whole walk:
    assert!(iter_groups(&buf, 0, buf.len()).is_err());
    // Lenient variant keeps the first group:
    let groups = iter_groups_lenient(&buf, 0, buf.len());
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].header.count, 2);
    assert_eq!(groups[0].prims[0].vertex_indices(), vec![0, 1, 2]);
}

#[test]
fn lenient_handles_empty_section() {
    // Empty section -> empty walk, no panic.
    assert!(iter_groups_lenient(&[], 0, 0).is_empty());
    // Section_end past buffer -> empty walk (defensive).
    assert!(iter_groups_lenient(&[0u8; 4], 0, 100).is_empty());
}

/// Untextured Gouraud-quad group (`flags=0x1F` -> row 3 quad,
/// `PacketShape::G4`). The bytes immediately before the vertex
/// indices are per-vertex colours, not a texture block; the
/// descriptor-gated walker must leave `uvs` empty and `cba/tsb`
/// zero. Earlier walks read those colour words as `(uv, cba, tsb)`,
/// producing bogus CLUT addresses and the spurious "VRAM is missing
/// CLUT data for row 0" warnings.
#[test]
fn untextured_prim_does_not_yield_uvs() {
    let mut buf = Vec::new();
    // Group header: count=1, flags=0x1F, olen=8, ilen=6, flag=1, mode=0x39.
    // Flags 0x1F => f_shifted=0x0F => row 3 quad => G4 (byte3=0x02), vert_off=0x06*2+2=14? No, byte3=0 -> vert_off=byte4=12? Recompute:
    // table_idx = (0x0F - 8) >> 1 = 3. (byte3, byte4) = (0x02, 0x06). is_quad=1. byte3!=0/1/3 -> vert_off = (byte4+2)*2 = 16 bytes.
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0x001Fu16.to_le_bytes());
    buf.extend_from_slice(&[8, 6, 1, 0x39]);
    // Prim body: 24 bytes. Bytes 0..16 are colours (would be misread
    // as texture descriptor); bytes 16..24 hold 4 u16 vertex indices.
    let mut prim = vec![0u8; 24];
    // Put recognisable colour bytes in the first 16 so we'd notice if
    // they leaked into uvs/cba/tsb.
    for (i, byte) in prim.iter_mut().enumerate().take(16) {
        *byte = 0xC0 + (i as u8);
    }
    for (vi, &raw) in [0u16, 8, 16, 24].iter().enumerate() {
        let off = 16 + vi * 2;
        prim[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim);
    // Footer slot (one prim stride) + terminator.
    buf.extend_from_slice(&[0u8; 24]);
    buf.extend_from_slice(&0u32.to_le_bytes());

    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    assert_eq!(groups.len(), 1);
    let p = &groups[0].prims[0];
    assert!(
        p.uvs.is_empty(),
        "untextured G4 must not produce uvs; got {:?}",
        p.uvs
    );
    assert_eq!(p.cba, 0, "untextured G4 must not produce a CBA");
    assert_eq!(p.tsb, 0, "untextured G4 must not produce a TSB");
    // Vertex indices must still decode normally.
    assert_eq!(p.vertex_indices(), vec![0, 1, 2, 3]);
    // Gouraud quad: one [R,G,B] per corner at a 4-byte stride from the prim
    // start, in stored order (colors[i] pairs with vertex_indices_raw[i]).
    assert_eq!(
        p.colors,
        vec![
            [0xC0, 0xC1, 0xC2],
            [0xC4, 0xC5, 0xC6],
            [0xC8, 0xC9, 0xCA],
            [0xCC, 0xCD, 0xCE],
        ]
    );
}

/// Flat quad (`F4`, flags `0x1B` -> row 2, byte3 0x00): one colour word at
/// the prim start shared by all four vertices; the vertex indices follow at
/// `vert_off = byte4 = 4` (the byte3==0 path cancels the quad `+2`).
#[test]
fn flat_quad_replicates_single_colour() {
    let mut buf = Vec::new();
    // Group header: count=1, flags=0x1B, olen=4, ilen=3, flag=0, mode=0x29.
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0x001Bu16.to_le_bytes());
    buf.extend_from_slice(&[4, 3, 0, 0x29]);
    // Prim body: 12 bytes. [0..4) colour word R G B code, [4..12) 4 u16 verts.
    let mut prim = vec![0u8; 12];
    prim[0..4].copy_from_slice(&[0x11, 0x22, 0x33, 0x28]);
    for (vi, &raw) in [0u16, 8, 24, 16].iter().enumerate() {
        let off = 4 + vi * 2;
        prim[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim);
    buf.extend_from_slice(&[0u8; 12]); // footer slot
    buf.extend_from_slice(&0u32.to_le_bytes()); // terminator

    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    let p = &groups[0].prims[0];
    assert!(p.uvs.is_empty());
    assert_eq!(p.vertex_indices(), vec![0, 1, 3, 2]);
    // The single colour word, dropping the code byte, replicated to all 4.
    assert_eq!(
        p.colors,
        vec![
            [0x11, 0x22, 0x33],
            [0x11, 0x22, 0x33],
            [0x11, 0x22, 0x33],
            [0x11, 0x22, 0x33]
        ]
    );
}

/// **Textured** prims on the baked-colour rows carry a colour block too -
/// it is exactly what pushes their texture block off offset 0. Dropping it
/// (as the walker used to, returning `colors` empty for anything textured)
/// throws away the field's entire lighting signal: the GPU modulates the
/// texel by this word as `texel * colour / 128`.
///
/// Flat textured triangle (`FT3`, flags `0x21` -> row 4, byte1 = 3): one
/// colour word at offset 0, the 12-byte texture block at 4, vertices at
/// `byte4 = 7` u16 = byte 14. Bytes mirror a real town0c prim.
#[test]
fn flat_textured_triangle_keeps_its_baked_colour() {
    let mut buf = Vec::new();
    // count=1, flags=0x21, olen=7, ilen=5 (20-byte prim), flag=0, mode=0x24.
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0x0021u16.to_le_bytes());
    buf.extend_from_slice(&[7, 5, 0, 0x24]);
    let mut prim = vec![0u8; 20];
    // [0..4) colour word: RGB + the GP0 code byte for POLY_FT3 (0x24).
    prim[0..4].copy_from_slice(&[0x80, 0x80, 0x80, 0x24]);
    // [4..16) texture block: u0 v0 cba | u1 v1 tsb | u2 v2.
    prim[4..16].copy_from_slice(&[
        0x3D, 0x32, 0x00, 0x7D, 0x21, 0x45, 0x0C, 0x00, 0x21, 0x20, 0, 0,
    ]);
    // Vertices at byte 14 (they overlap the block's trailing pad halfword).
    for (vi, &raw) in [0x150u16, 0x108, 0x168].iter().enumerate() {
        let off = 14 + vi * 2;
        prim[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim);
    buf.extend_from_slice(&[0u8; 20]);
    buf.extend_from_slice(&0u32.to_le_bytes());

    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    let p = &groups[0].prims[0];
    // The texture block still decodes from offset 4.
    assert_eq!(p.cba, 0x7D00);
    assert_eq!(p.tsb, 0x000C);
    assert!(!p.uvs.is_empty());
    // ...and the colour word is now kept, replicated to all three corners.
    assert_eq!(p.colors, vec![[0x80, 0x80, 0x80]; 3]);
}

/// Gouraud textured triangle (`GT3`, flags `0x25` -> row 5): one colour word
/// per corner at offset 0, texture block at 12. The per-corner ramp is the
/// baked shading - here `0x80 / 0x60 / 0x50`, i.e. the same texel drawn at
/// 1.0x, 0.75x and 0.625x across the face.
#[test]
fn gouraud_textured_triangle_keeps_its_per_corner_ramp() {
    let mut buf = Vec::new();
    // count=1, flags=0x25, olen=9, ilen=7 (28-byte prim), flag=0, mode=0x34.
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0x0025u16.to_le_bytes());
    buf.extend_from_slice(&[9, 7, 0, 0x34]);
    let mut prim = vec![0u8; 28];
    prim[0..4].copy_from_slice(&[0x80, 0x80, 0x80, 0x34]);
    prim[4..8].copy_from_slice(&[0x60, 0x60, 0x60, 0x34]);
    prim[8..12].copy_from_slice(&[0x50, 0x50, 0x50, 0x34]);
    prim[12..22].copy_from_slice(&[0x4F, 0x9C, 0x80, 0x7A, 0x6F, 0x8D, 0x1A, 0x00, 0x6F, 0xAD]);
    for (vi, &raw) in [0x128u16, 0x118, 0x120].iter().enumerate() {
        let off = 22 + vi * 2;
        prim[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim);
    buf.extend_from_slice(&[0u8; 28]);
    buf.extend_from_slice(&0u32.to_le_bytes());

    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    let p = &groups[0].prims[0];
    assert_eq!(p.cba, 0x7A80);
    assert_eq!(p.tsb, 0x001A);
    assert_eq!(
        p.colors,
        vec![[0x80, 0x80, 0x80], [0x60, 0x60, 0x60], [0x50, 0x50, 0x50]]
    );
}

/// The **light-source-lit** rows (0/1) carry NO colour block - their texture
/// block starts at offset 0 and normal indices trail the vertices. They must
/// come back as [`MODULATION_NEUTRAL`], not as the texture block reinterpreted
/// as a colour (which would paint them with whatever `[u0, v0, cba_lo]`
/// happens to be) and not as black.
///
/// Gouraud textured lit quad (`GT4`, flags `0x17` -> row 1): 12-byte texture
/// block at 0, four vertex indices at 12, four normal indices at 20.
#[test]
fn lit_row_prims_are_neutral_not_texture_bytes() {
    let mut buf = Vec::new();
    // count=1, flags=0x17, olen=9, ilen=7 (28-byte prim), flag=0, mode=0x3C.
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&0x0017u16.to_le_bytes());
    buf.extend_from_slice(&[9, 7, 0, 0x3C]);
    let mut prim = vec![0u8; 28];
    // Texture block at offset 0 - the bytes an offset-0 colour read would
    // wrongly pick up as an RGB (0x11, 0xE5, 0x89).
    prim[0..12].copy_from_slice(&[
        0x11, 0xE5, 0x89, 0x7A, 0x11, 0xEF, 0x1A, 0x00, 0x16, 0xE5, 0x16, 0xEF,
    ]);
    for (vi, &raw) in [0u16, 8, 0x90, 0x98].iter().enumerate() {
        let off = 12 + vi * 2;
        prim[off..off + 2].copy_from_slice(&raw.to_le_bytes());
    }
    buf.extend_from_slice(&prim);
    buf.extend_from_slice(&[0u8; 28]);
    buf.extend_from_slice(&0u32.to_le_bytes());

    let groups = iter_groups(&buf, 0, buf.len()).unwrap();
    let p = &groups[0].prims[0];
    assert_eq!(p.cba, 0x7A89, "lit rows read their texture block at 0");
    assert_eq!(p.tsb, 0x001A);
    assert_eq!(p.vertex_indices(), vec![0, 1, 18, 19]);
    assert_eq!(
        p.colors,
        vec![[MODULATION_NEUTRAL; 3]; 4],
        "a lit prim has no colour word; it must draw at the raw texel"
    );
}
