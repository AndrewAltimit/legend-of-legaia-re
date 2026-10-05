//! Retail's per-primitive near reject: the `OTZ` cut every TMD prim handler
//! runs before it links a packet.
//!
//! Each handler behind the per-prim dispatcher `FUN_80043390` projects its
//! corners (`RTPT` / `RTPS`), runs `AVSZ3` / `AVSZ4`, reads `OTZ` back and
//! drops the primitive when it is below the scratch halfword `0x1F80037E`
//! (kind 13, `FUN_80043768`: `mfc2 s2,$7` / `sub s1,s2,t4` / `bltz s1` at
//! `0x80043868..0x80043874`; `t4` is `lhu 0x6A(0x1F800314)` loaded per group
//! at `0x8004359C`). The dispatcher stages `ZSF3 = 0x555 >> s` and
//! `ZSF4 = 0x400 >> s` with `s` the ordering-table shift byte `0x1F8003A4`
//! (`0x80043568..0x8004357C`), so `OTZ` is the mean corner `SZ` shifted right
//! by `s`, and the cut drops a primitive whose mean depth is under
//! `0x10 << s`.
//!
//! The scene init (`FUN_8001D424` at `0x8001D4E8`, `FUN_8001DCF8` at
//! `0x8001DD5C`) writes `0x10` to `0x1F80037E`; the shift reads `3` in every
//! catalogued field state and `2` in every battle state. There is no
//! near-plane clip anywhere on this path, so a primitive whose mean depth sits
//! near or behind the eye is simply not drawn - where a port that projects it
//! and clips per pixel paints a huge stretched surface across the frame (a
//! battle body standing next to the camera, a wall the cutscene camera grazes).
//!
//! `SZ` is the GTE's saturated screen depth: a corner behind the eye
//! contributes `0`, not a negative depth, which is why the mean is taken over
//! clamped corners rather than over the primitive's centroid.
//!
//! Both hosts read this through their mesh vertex stage: every vertex carries
//! its primitive's corners ([`prim_corner_refs`]), and the stage computes the
//! same integer `OTZ` from the frame's matrix. [`prim_near_rejected`] is the
//! CPU statement of that test, kept in lockstep with the shaders by tests.
//!
//! REF: FUN_80043390, FUN_80043768

/// The `OTZ` floor the scene init stores in scratch `0x1F80037E`.
pub const NEAR_OTZ: i32 = 0x10;

/// Floats per vertex in a [`prim_corner_refs`] record: `[c0.xyz, n, c1.xyz,
/// c2.xyz, c3.xyz]`.
pub const PRIM_REF_FLOATS: usize = 13;

/// One vertex's primitive-corner record. `n` (lane 3) is the corner count -
/// `3` or `4` - or `0` when the vertex has no single owning primitive, which
/// the shaders read as "never reject".
pub type PrimRef = [f32; PRIM_REF_FLOATS];

/// The GTE's `SZ` for a view depth: floored to an integer and saturated to
/// `0..=0xFFFF`.
pub fn screen_z(view_depth: f32) -> i32 {
    if view_depth.is_nan() {
        return 0;
    }
    (view_depth.floor() as i64).clamp(0, 0xFFFF) as i32
}

/// `AVSZ3` / `AVSZ4` over the corners' `SZ`, with the dispatcher's
/// `ZSF3 = 0x555 >> shift` / `ZSF4 = 0x400 >> shift`.
pub fn otz(sz: &[i32], ot_shift: u32) -> i32 {
    let sum: i64 = sz.iter().map(|&z| i64::from(z.clamp(0, 0xFFFF))).sum();
    let zsf: i64 = if sz.len() == 4 {
        0x400 >> ot_shift
    } else {
        0x555 >> ot_shift
    };
    ((zsf * sum) >> 12) as i32
}

/// Whether retail drops a primitive whose corners sit at `view_depths`.
pub fn prim_near_rejected(view_depths: &[f32], ot_shift: u32) -> bool {
    let sz: Vec<i32> = view_depths.iter().map(|&w| screen_z(w)).collect();
    otz(&sz, ot_shift) < NEAR_OTZ
}

/// The per-frame parameters both hosts' vertex stages read: `[enable,
/// sz_per_w, ot_shift, near_otz]`. `sz_per_w` converts the frame's clip `w`
/// into GTE `SZ` units (`1.0` for the field and battle cameras, whose `w` is
/// the eye-space depth). `None` is off.
pub fn shader_params(cut: Option<(f32, u32)>) -> [f32; 4] {
    match cut {
        Some((sz_per_w, shift)) => [1.0, sz_per_w, shift as f32, NEAR_OTZ as f32],
        None => [0.0; 4],
    }
}

/// Per-vertex primitive-corner records for a triangle list.
///
/// A primitive is a triangle, or two consecutive triangles that share an
/// edge (the mesh builders split a quad `[0,1,2, 1,3,2]`). Every vertex of a
/// primitive gets the primitive's corners, so the vertex stage reaches the
/// same verdict on each and the whole primitive drops together. A vertex
/// that two primitives share (a welded heightfield) has no single owner;
/// every primitive touching such a vertex is written with `n = 0` and is never
/// rejected, which keeps a shared edge from tearing.
pub fn prim_corner_refs(positions: &[[f32; 3]], indices: &[u32]) -> Vec<PrimRef> {
    let mut refs = vec![[0.0f32; PRIM_REF_FLOATS]; positions.len()];
    let tris = indices.as_chunks::<3>().0;
    // Group the triangles into primitives.
    let mut groups: Vec<Vec<u32>> = Vec::with_capacity(tris.len());
    let mut i = 0;
    while i < tris.len() {
        let a = tris[i];
        if let Some(b) = tris.get(i + 1) {
            let shared = b.iter().filter(|v| a.contains(v)).count();
            let extra: Vec<u32> = b.iter().copied().filter(|v| !a.contains(v)).collect();
            if shared == 2 && extra.len() == 1 && a.iter().all(|v| *v != extra[0]) {
                // Retail corner order for a quad: v0, v1, v2, v3.
                groups.push(vec![a[0], a[1], a[2], extra[0]]);
                i += 2;
                continue;
            }
        }
        groups.push(a.to_vec());
        i += 1;
    }
    // A vertex owned by more than one primitive disqualifies all of them.
    let mut owner = vec![usize::MAX; positions.len()];
    let mut shared = vec![false; groups.len()];
    for (g, verts) in groups.iter().enumerate() {
        for &v in verts {
            let Some(o) = owner.get_mut(v as usize) else {
                continue;
            };
            if *o == usize::MAX || *o == g {
                *o = g;
            } else {
                shared[g] = true;
                shared[*o] = true;
            }
        }
    }
    for (g, verts) in groups.iter().enumerate() {
        if shared[g] || verts.iter().any(|&v| v as usize >= positions.len()) {
            continue;
        }
        let c = |k: usize| positions[verts[k.min(verts.len() - 1)] as usize];
        let (c0, c1, c2, c3) = (c(0), c(1), c(2), c(3));
        let rec: PrimRef = [
            c0[0],
            c0[1],
            c0[2],
            verts.len() as f32,
            c1[0],
            c1[1],
            c1[2],
            c2[0],
            c2[1],
            c2[2],
            c3[0],
            c3[1],
            c3[2],
        ];
        for &v in verts {
            refs[v as usize] = rec;
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_cut_drops_a_quad_whose_mean_depth_is_under_128() {
        assert!(prim_near_rejected(&[127.0; 4], 3));
        assert!(!prim_near_rejected(&[128.0; 4], 3));
        // Mean, not minimum: one near corner does not drop a far quad.
        assert!(!prim_near_rejected(&[0.0, 200.0, 200.0, 200.0], 3));
    }

    #[test]
    fn the_battle_cut_drops_under_64() {
        assert!(prim_near_rejected(&[63.0; 4], 2));
        assert!(!prim_near_rejected(&[64.0; 4], 2));
        assert!(prim_near_rejected(&[63.0; 3], 2));
    }

    #[test]
    fn a_corner_behind_the_eye_counts_as_zero_not_negative() {
        // Centroid depth 100 (rejected on the field), but the behind-the-eye
        // corner saturates to 0, lifting the mean to 175.
        assert!(!prim_near_rejected(&[-500.0, 300.0, 300.0, 300.0], 3));
    }

    #[test]
    fn triangles_use_zsf3() {
        // (0xAA * sum) >> 12 < 16  <=>  sum <= 385.
        assert!(prim_near_rejected(&[128.0, 128.0, 129.0], 3));
        assert!(!prim_near_rejected(&[128.0, 129.0, 129.0], 3));
    }

    #[test]
    fn a_split_quad_shares_one_record_and_a_welded_vertex_disables() {
        let pos = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        let refs = prim_corner_refs(&pos, &[0, 1, 2, 1, 3, 2]);
        assert!(refs.iter().all(|r| r[3] == 4.0));
        assert_eq!(&refs[0][10..13], &[1.0, 1.0, 0.0]);
        // Two triangles sharing only one vertex: separate prims, shared vertex.
        let pos = vec![[0.0; 3]; 5];
        let refs = prim_corner_refs(&pos, &[0, 1, 2, 2, 3, 4]);
        assert!(refs.iter().all(|r| r[3] == 0.0));
        // Two independent triangles.
        let pos = vec![[0.0; 3]; 6];
        let refs = prim_corner_refs(&pos, &[0, 1, 2, 3, 4, 5]);
        assert!(refs.iter().all(|r| r[3] == 3.0));
    }
}
