//! Coplanar-primitive resolution post-passes for scene meshes.
//!
//! Retail's GPU has **no depth buffer**: primitives are painter-ordered
//! through the ordering table, so coplanar surfaces resolve by draw order
//! (within one OT bucket the earliest-inserted prim paints last, i.e. wins)
//! and mean-Z bucket separation (a small decal prim's centroid usually lands
//! in a nearer bucket than the large base surface it sits on). The port's
//! depth-tested renderers turn every such coplanar pair into per-pixel
//! z-fighting instead. These opt-in post-passes restore a stable, retail-like
//! winner at the geometry level:
//!
//! * [`mark_double_sided_pairs`] finds **double-sided prims** - two triangles
//!   over the same vertex triple with opposite winding (the same wall authored
//!   once per visible side). Retail's NCLIP backface test only ever rasterises
//!   the camera-facing copy; the port draws both (`cull_mode: None`, because
//!   winding is not globally consistent across the corpus) and the two copies
//!   fight wherever their per-side textures differ. The pass flags both
//!   copies via [`CBA_DOUBLE_SIDED_BIT`]; the fragment shaders then discard
//!   the away-facing copy of *flagged* prims only - a per-prim NCLIP without
//!   touching the (unsafe) global cull.
//! * [`separate_coplanar_prims`] finds distinct coplanar overlapping prims
//!   inside one mesh (decal quads baked onto walls/floors) and nudges the
//!   smaller/later prim `0.5` world units toward its visible side. Retail
//!   art itself uses ~1-unit offsets for the decals it wants unambiguous, so
//!   the nudge stays inside authored practice and is far below visibility
//!   (a field tile is 128 units).
//!
//! Both passes are **opt-in** (the scene-assembly consumers call them after
//! building a mesh); the plain builders stay byte-faithful for the
//! preservation/export paths.

use super::{ColorMesh, VramMesh};

/// Bit 15 of the per-vertex **CBA** attribute marks the prim as one copy of a
/// detected double-sided pair. A PSX CBA word only uses bits 0..=14 (CLUT x
/// in bits 0..=5, CLUT y in bits 6..=14), so bit 15 is free for this
/// engine-side packing - the shaders' CLUT decode masks it out. Fragment
/// shaders discard the away-facing copy of flagged prims (see
/// `engine-render`'s VRAM-mesh shader and `site/js/webgl-shaders.js`).
pub const CBA_DOUBLE_SIDED_BIT: u16 = 0x8000;

/// The colour-half twin of [`CBA_DOUBLE_SIDED_BIT`]: bit 14 of a
/// [`ColorMesh`] per-vertex blend word marks one copy of a double-sided pair
/// detected by [`resolve_hybrid`]. The blend word only uses bit 15 (ABE) and
/// bits 5..=6 (ABR), so bit 14 is free; `psx_blend` masks it out. The native
/// colour-mesh fragment shader discards the away-facing copy of flagged
/// prims, and the web merge translates the bit back onto the merged CBA
/// attribute.
pub const BLEND_DOUBLE_SIDED_BIT: u16 = 0x4000;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct VKey(i64, i64, i64);

fn vkey(p: [f32; 3]) -> VKey {
    // Env-mesh vertices are integer PSX coordinates; round defensively so a
    // posed/scaled variant still keys consistently.
    VKey(
        (p[0] * 16.0).round() as i64,
        (p[1] * 16.0).round() as i64,
        (p[2] * 16.0).round() as i64,
    )
}

/// Mark both copies of every exact-duplicate opposite-winding triangle pair
/// with [`CBA_DOUBLE_SIDED_BIT`]. Returns the number of pairs marked.
///
/// Pairing is per vertex-triple: two triangles whose three positions match
/// exactly but whose cyclic order differs are one double-sided surface.
/// Same-winding exact duplicates (true double emission) are left alone -
/// they interpolate to identical depths, so the depth test resolves them
/// deterministically (first drawn wins) without shimmer.
pub fn mark_double_sided_pairs(mesh: &mut VramMesh) -> usize {
    use std::collections::HashMap;
    let tris: Vec<[u32; 3]> = mesh
        .indices
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let mut by_triple: HashMap<[VKey; 3], Vec<usize>> = HashMap::new();
    for (t_idx, t) in tris.iter().enumerate() {
        let mut k = [
            vkey(mesh.positions[t[0] as usize]),
            vkey(mesh.positions[t[1] as usize]),
            vkey(mesh.positions[t[2] as usize]),
        ];
        // Degenerate (repeated-vertex) triangles never pair.
        if k[0] == k[1] || k[1] == k[2] || k[0] == k[2] {
            continue;
        }
        k.sort_by_key(|v| (v.0, v.1, v.2));
        by_triple.entry(k).or_default().push(t_idx);
    }
    let same_cyclic = |a: [VKey; 3], b: [VKey; 3]| -> bool {
        (0..3).any(|r| (0..3).all(|i| a[i] == b[(i + r) % 3]))
    };
    let keys_of = |t: [u32; 3]| -> [VKey; 3] {
        [
            vkey(mesh.positions[t[0] as usize]),
            vkey(mesh.positions[t[1] as usize]),
            vkey(mesh.positions[t[2] as usize]),
        ]
    };
    let mut pairs = 0usize;
    let mut flagged: Vec<u32> = Vec::new();
    for group in by_triple.values() {
        if group.len() < 2 {
            continue;
        }
        // Greedy pair-up: each triangle joins at most one pair, and only with
        // an opposite-winding partner.
        let mut used = vec![false; group.len()];
        for i in 0..group.len() {
            if used[i] {
                continue;
            }
            for j in (i + 1)..group.len() {
                if used[j] {
                    continue;
                }
                let (a, b) = (tris[group[i]], tris[group[j]]);
                if same_cyclic(keys_of(a), keys_of(b)) {
                    continue; // same winding: depth test already stable
                }
                used[i] = true;
                used[j] = true;
                pairs += 1;
                flagged.extend_from_slice(&a);
                flagged.extend_from_slice(&b);
                break;
            }
        }
    }
    for v in flagged {
        mesh.cba_tsb[v as usize][0] |= CBA_DOUBLE_SIDED_BIT;
    }
    pairs
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// How far [`separate_coplanar_prims`] nudges each successive coplanar layer,
/// in PSX world units. Half a unit: dominant over depth-buffer interpolation
/// noise at every sane camera distance, invisible against 128-unit tiles, and
/// half of the smallest offset retail's own art uses for decals it separates
/// explicitly (1 unit).
pub const COPLANAR_NUDGE: f32 = 0.5;

/// The visible-side direction of an emitted triangle.
///
/// The mesh builders emit corners in the PSX SDK order, and the visible side
/// (the one retail's NCLIP keeps) is the **negative** cross-product side of
/// that order: a floor authored to be seen from above (`-Y` side in the
/// retail Y-down frame) emits with `cross(e1, e2) = +Y`. Derived from the
/// dance-stage NCLIP parity (the one consumer that reproduces retail's cull:
/// front face = CCW under the viewer's single-reflection projection).
fn visible_side_normal(v0: [f32; 3], v1: [f32; 3], v2: [f32; 3]) -> Option<[f32; 3]> {
    let n = cross(sub(v1, v0), sub(v2, v0));
    let len = dot(n, n).sqrt();
    if len < 1e-6 {
        return None;
    }
    Some([-n[0] / len, -n[1] / len, -n[2] / len])
}

struct TriRec {
    tri: usize,
    n: [f32; 3], // canonical unit normal (dominant component positive)
    vis: [f32; 3],
    d: f32,
    area: f32,
    p2: [[f32; 2]; 3],
}

fn dominant_axis(n: [f32; 3]) -> usize {
    let a = [n[0].abs(), n[1].abs(), n[2].abs()];
    if a[0] >= a[1] && a[0] >= a[2] {
        0
    } else if a[1] >= a[2] {
        1
    } else {
        2
    }
}

fn project2(v: [f32; 3], ax: usize) -> [f32; 2] {
    match ax {
        0 => [v[1], v[2]],
        1 => [v[0], v[2]],
        _ => [v[0], v[1]],
    }
}

fn poly_area2(p: &[[f32; 2]]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let j = (i + 1) % p.len();
        a += p[i][0] * p[j][1] - p[j][0] * p[i][1];
    }
    a.abs() * 0.5
}

/// Sutherland-Hodgman clip of `subject` by the convex triangle `clip`.
fn clip_tri_tri(mut subject: Vec<[f32; 2]>, clip: &[[f32; 2]; 3]) -> Vec<[f32; 2]> {
    let mut c = *clip;
    let signed =
        (c[1][0] - c[0][0]) * (c[2][1] - c[0][1]) - (c[2][0] - c[0][0]) * (c[1][1] - c[0][1]);
    if signed < 0.0 {
        c.swap(1, 2);
    }
    for e in 0..3 {
        let a = c[e];
        let b = c[(e + 1) % 3];
        if subject.is_empty() {
            return subject;
        }
        let inside =
            |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= 0.0;
        let isect = |p: [f32; 2], q: [f32; 2]| -> [f32; 2] {
            let r = [q[0] - p[0], q[1] - p[1]];
            let s = [b[0] - a[0], b[1] - a[1]];
            let denom = r[0] * s[1] - r[1] * s[0];
            if denom.abs() < 1e-9 {
                return p;
            }
            let t = ((a[0] - p[0]) * s[1] - (a[1] - p[1]) * s[0]) / denom;
            [p[0] + t * r[0], p[1] + t * r[1]]
        };
        let mut out: Vec<[f32; 2]> = Vec::new();
        for i in 0..subject.len() {
            let cur = subject[i];
            let prev = subject[(i + subject.len() - 1) % subject.len()];
            let (ci, pi) = (inside(cur), inside(prev));
            if ci {
                if !pi {
                    out.push(isect(prev, cur));
                }
                out.push(cur);
            } else if pi {
                out.push(isect(prev, cur));
            }
        }
        subject = out;
    }
    subject
}

/// Overlap area of two coplanar triangles, projected on the plane's dominant
/// axis. `0.0` when they merely share an edge.
fn tri_overlap_area(a: &TriRec, b: &TriRec) -> f32 {
    let clipped = clip_tri_tri(a.p2.to_vec(), &b.p2);
    if clipped.len() < 3 {
        return 0.0;
    }
    poly_area2(&clipped)
}

/// Nudge distinct coplanar overlapping prims apart so the depth buffer
/// resolves them deterministically: within each coplanar overlap cluster the
/// largest surface stays put and each successive overlapping layer moves
/// [`COPLANAR_NUDGE`] world units toward its visible side. Skips triangles
/// flagged by [`mark_double_sided_pairs`] (their resolution is the facing
/// discard, not a nudge - run that pass FIRST). Returns the number of
/// triangles moved.
pub fn separate_coplanar_prims(mesh: &mut VramMesh) -> usize {
    use std::collections::HashMap;
    let tris: Vec<[u32; 3]> = mesh
        .indices
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let mut recs: Vec<TriRec> = Vec::new();
    for (t_idx, t) in tris.iter().enumerate() {
        if mesh.cba_tsb[t[0] as usize][0] & CBA_DOUBLE_SIDED_BIT != 0 {
            continue;
        }
        let v = [
            mesh.positions[t[0] as usize],
            mesh.positions[t[1] as usize],
            mesh.positions[t[2] as usize],
        ];
        let Some(vis) = visible_side_normal(v[0], v[1], v[2]) else {
            continue;
        };
        // Canonical plane orientation for clustering (sign-insensitive).
        let mut n = [-vis[0], -vis[1], -vis[2]];
        let ax = dominant_axis(n);
        if n[ax] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let d = dot(n, v[0]);
        let e1 = sub(v[1], v[0]);
        let e2 = sub(v[2], v[0]);
        let c = cross(e1, e2);
        let area = dot(c, c).sqrt() * 0.5;
        if area < 1.0 {
            continue;
        }
        recs.push(TriRec {
            tri: t_idx,
            n,
            vis,
            d,
            area,
            p2: [project2(v[0], ax), project2(v[1], ax), project2(v[2], ax)],
        });
    }
    // Bucket by quantized plane; d straddles land in adjacent buckets.
    let mut buckets: HashMap<(i32, i32, i32, i64), Vec<usize>> = HashMap::new();
    for (i, r) in recs.iter().enumerate() {
        let base = (
            (r.n[0] * 512.0).round() as i32,
            (r.n[1] * 512.0).round() as i32,
            (r.n[2] * 512.0).round() as i32,
        );
        for dd in -1..=1i64 {
            buckets
                .entry((base.0, base.1, base.2, (r.d / 2.0).floor() as i64 + dd))
                .or_default()
                .push(i);
        }
    }
    // Overlap edges between coplanar rec pairs.
    let mut edges: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut seen: std::collections::HashSet<(usize, usize)> = Default::default();
    for members in buckets.values() {
        for xi in 0..members.len() {
            for yi in (xi + 1)..members.len() {
                let (i, j) = (members[xi].min(members[yi]), members[xi].max(members[yi]));
                if i == j || seen.contains(&(i, j)) {
                    continue;
                }
                let (a, b) = (&recs[i], &recs[j]);
                if dot(a.n, b.n) < 0.9999 || (a.d - b.d).abs() > 0.05 {
                    continue;
                }
                let ov = tri_overlap_area(a, b);
                if ov < 4.0 || ov < 0.02 * a.area.min(b.area) {
                    continue;
                }
                seen.insert((i, j));
                edges.entry(i).or_default().push(j);
                edges.entry(j).or_default().push(i);
            }
        }
    }
    if edges.is_empty() {
        return 0;
    }
    // Greedy colouring in area-descending order: the largest surface takes
    // rank 0 (stays), each overlapping layer takes the smallest rank its
    // already-ranked neighbours don't hold. Rank grows only inside a mutual
    // overlap clique, so long tile chains stay at rank <= 1.
    let mut order: Vec<usize> = edges.keys().copied().collect();
    order.sort_by(|&a, &b| {
        recs[b]
            .area
            .partial_cmp(&recs[a].area)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(recs[a].tri.cmp(&recs[b].tri))
    });
    let mut rank: HashMap<usize, u32> = HashMap::new();
    for &i in &order {
        let mut used = [false; 8];
        for &j in &edges[&i] {
            if let Some(&r) = rank.get(&j)
                && (r as usize) < used.len()
            {
                used[r as usize] = true;
            }
        }
        let r = used.iter().position(|&u| !u).unwrap_or(used.len() - 1) as u32;
        rank.insert(i, r.min(4));
    }
    // Apply: shift each ranked triangle's vertices toward its visible side.
    // Vertices are per-prim (the builders never share them across prims), so
    // moving a triangle's three vertices moves only that triangle.
    let mut moved = 0usize;
    for (&i, &r) in &rank {
        if r == 0 {
            continue;
        }
        let rec = &recs[i];
        let t = tris[rec.tri];
        let shift = COPLANAR_NUDGE * r as f32;
        for &vi in &t {
            let p = &mut mesh.positions[vi as usize];
            p[0] += rec.vis[0] * shift;
            p[1] += rec.vis[1] * shift;
            p[2] += rec.vis[2] * shift;
        }
        moved += 1;
    }
    moved
}

/// Run both coplanar passes over the **hybrid** of one TMD's textured and
/// untextured halves as a single primitive stream, then split the result
/// back. A scene TMD interleaves textured prims with untextured `F*`/`G*`
/// colour prims, and its baked decals (floor shadows, wall paintings) are
/// routinely colour prims lying on textured bases - pairs neither
/// single-mesh pass can see. Consumers that draw the two halves on separate
/// pipelines (the native play-window) and consumers that merge them (the
/// web viewers) both call this so the two hosts share one model.
///
/// Position nudges land back in whichever half owns the vertex; a colour
/// vertex of a detected double-sided pair gets [`BLEND_DOUBLE_SIDED_BIT`]
/// set in its blend word (the textured half keeps using
/// [`CBA_DOUBLE_SIDED_BIT`]). Returns `(double_sided_pairs, nudged_tris)`.
pub fn resolve_hybrid(vmesh: &mut VramMesh, cmesh: &mut ColorMesh) -> (usize, usize) {
    let n_verts = vmesh.positions.len();
    let n_idx = vmesh.indices.len();
    if cmesh.blend.len() < cmesh.positions.len() {
        // Builders always fill blend per-vertex; pad defensively so the
        // flag write-back below can't index out of range.
        cmesh.blend.resize(cmesh.positions.len(), 0);
    }
    // Append the colour half so both passes see one stream (matching the
    // web viewers' merged draw order: textured half first).
    for (p, blend) in cmesh.positions.iter().zip(cmesh.blend.iter()) {
        vmesh.positions.push(*p);
        vmesh.uvs.push([0, 0]);
        vmesh.cba_tsb.push([0, *blend]);
        vmesh.normals.push([0.0; 3]);
        vmesh.colors.push([0x80; 3]);
    }
    vmesh
        .indices
        .extend(cmesh.indices.iter().map(|i| i + n_verts as u32));
    let ds = mark_double_sided_pairs(vmesh);
    let nudged = separate_coplanar_prims(vmesh);
    // Split back: nudged positions return to their owning half, and the
    // colour half's pair flag moves onto its blend word.
    for i in 0..cmesh.positions.len() {
        cmesh.positions[i] = vmesh.positions[n_verts + i];
        if vmesh.cba_tsb[n_verts + i][0] & CBA_DOUBLE_SIDED_BIT != 0 {
            cmesh.blend[i] |= BLEND_DOUBLE_SIDED_BIT;
        }
    }
    vmesh.positions.truncate(n_verts);
    vmesh.uvs.truncate(n_verts);
    vmesh.cba_tsb.truncate(n_verts);
    vmesh.normals.truncate(n_verts);
    vmesh.colors.truncate(n_verts);
    vmesh.indices.truncate(n_idx);
    (ds, nudged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_from_tris(tris: &[[[f32; 3]; 3]]) -> VramMesh {
        let mut m = VramMesh {
            positions: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
        };
        for t in tris {
            let base = m.positions.len() as u32;
            for v in t {
                m.positions.push(*v);
                m.uvs.push([0, 0]);
                m.cba_tsb.push([0x1234, 0x000A]);
                m.normals.push([0.0, 0.0, 0.0]);
                m.colors.push([0x80; 3]);
            }
            m.indices.extend([base, base + 1, base + 2]);
        }
        m
    }

    #[test]
    fn double_sided_pair_is_flagged_both_copies() {
        let a = [[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [0.0, 0.0, 64.0]];
        let b = [[0.0, 0.0, 0.0], [0.0, 0.0, 64.0], [64.0, 0.0, 0.0]]; // opposite winding
        let mut m = mesh_from_tris(&[a, b]);
        assert_eq!(mark_double_sided_pairs(&mut m), 1);
        for ct in &m.cba_tsb {
            assert_ne!(ct[0] & CBA_DOUBLE_SIDED_BIT, 0);
            assert_eq!(ct[0] & 0x7FFF, 0x1234); // CLUT bits untouched
        }
    }

    #[test]
    fn same_winding_duplicate_not_flagged() {
        let a = [[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [0.0, 0.0, 64.0]];
        let mut m = mesh_from_tris(&[a, a]);
        assert_eq!(mark_double_sided_pairs(&mut m), 0);
        for ct in &m.cba_tsb {
            assert_eq!(ct[0] & CBA_DOUBLE_SIDED_BIT, 0);
        }
    }

    #[test]
    fn disjoint_coplanar_tris_are_not_nudged() {
        let a = [[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [0.0, 0.0, 64.0]];
        let b = [
            [100.0, 0.0, 100.0],
            [164.0, 0.0, 100.0],
            [100.0, 0.0, 164.0],
        ];
        let mut m = mesh_from_tris(&[a, b]);
        let orig = m.positions.clone();
        assert_eq!(separate_coplanar_prims(&mut m), 0);
        assert_eq!(m.positions, orig);
    }

    #[test]
    fn coplanar_decal_moves_toward_visible_side_and_base_stays() {
        // Base: large floor tri visible from above in the retail Y-down frame
        // (visible side = -Y). Winding chosen so cross(e1, e2) = +Y.
        let base = [[0.0, 0.0, 0.0], [0.0, 0.0, 128.0], [128.0, 0.0, 0.0]];
        // Decal: smaller coplanar tri fully inside the base, same winding.
        let decal = [[16.0, 0.0, 16.0], [16.0, 0.0, 48.0], [48.0, 0.0, 16.0]];
        let mut m = mesh_from_tris(&[base, decal]);
        assert_eq!(separate_coplanar_prims(&mut m), 1);
        // Base untouched.
        for v in 0..3 {
            assert_eq!(m.positions[v][1], 0.0);
        }
        // Decal moved 0.5 toward -Y (up, the visible side).
        for v in 3..6 {
            assert!((m.positions[v][1] + COPLANAR_NUDGE).abs() < 1e-4);
        }
    }

    fn color_mesh_from_tris(tris: &[[[f32; 3]; 3]]) -> ColorMesh {
        let mut m = ColorMesh {
            positions: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
            blend: Vec::new(),
        };
        for t in tris {
            let base = m.positions.len() as u32;
            for v in t {
                m.positions.push(*v);
                m.colors.push([0x20; 3]);
                m.blend.push(0);
            }
            m.indices.extend([base, base + 1, base + 2]);
        }
        m
    }

    #[test]
    fn hybrid_colour_decal_over_textured_base_is_nudged() {
        // The koin4 shadow class: an untextured colour decal (baked shadow)
        // lying on a textured floor. Neither single-mesh pass sees the pair;
        // the hybrid kernel must nudge the decal and leave the base.
        let base = [[0.0, 0.0, 0.0], [0.0, 0.0, 128.0], [128.0, 0.0, 0.0]];
        let decal = [[16.0, 0.0, 16.0], [16.0, 0.0, 48.0], [48.0, 0.0, 16.0]];
        let mut vm = mesh_from_tris(&[base]);
        let mut cm = color_mesh_from_tris(&[decal]);
        let (ds, nudged) = resolve_hybrid(&mut vm, &mut cm);
        assert_eq!(ds, 0);
        assert_eq!(nudged, 1);
        // Textured base untouched, and the merged tail fully removed.
        assert_eq!(vm.positions.len(), 3);
        assert_eq!(vm.indices.len(), 3);
        for v in &vm.positions {
            assert_eq!(v[1], 0.0);
        }
        // Colour decal moved 0.5 toward -Y (up, its visible side).
        for v in &cm.positions {
            assert!((v[1] + COPLANAR_NUDGE).abs() < 1e-4);
        }
    }

    #[test]
    fn hybrid_double_sided_cross_family_pair_flags_both_halves() {
        // One textured copy + one opposite-winding colour copy of the same
        // surface: the flag must land on CBA bit 15 for the textured half
        // and on blend bit 14 for the colour half.
        let a = [[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [0.0, 0.0, 64.0]];
        let b = [[0.0, 0.0, 0.0], [0.0, 0.0, 64.0], [64.0, 0.0, 0.0]];
        let mut vm = mesh_from_tris(&[a]);
        let mut cm = color_mesh_from_tris(&[b]);
        let (ds, _) = resolve_hybrid(&mut vm, &mut cm);
        assert_eq!(ds, 1);
        for ct in &vm.cba_tsb {
            assert_ne!(ct[0] & CBA_DOUBLE_SIDED_BIT, 0);
            assert_eq!(ct[0] & 0x7FFF, 0x1234); // CLUT bits untouched
        }
        for w in &cm.blend {
            assert_ne!(w & BLEND_DOUBLE_SIDED_BIT, 0);
            assert_eq!(w & !BLEND_DOUBLE_SIDED_BIT, 0); // ABE/ABR untouched
        }
    }

    #[test]
    fn tile_chain_ranks_alternate_instead_of_growing() {
        // Three coplanar strips: A overlaps B, B overlaps C, A and C are
        // disjoint. Greedy colouring must keep every rank <= 1 (nudge <= 0.5).
        let a = [[0.0, 0.0, 0.0], [0.0, 0.0, 64.0], [100.0, 0.0, 0.0]];
        let b = [[60.0, 0.0, 0.0], [60.0, 0.0, 64.0], [160.0, 0.0, 0.0]];
        let c = [[120.0, 0.0, 0.0], [120.0, 0.0, 64.0], [220.0, 0.0, 0.0]];
        let mut m = mesh_from_tris(&[a, b, c]);
        let moved = separate_coplanar_prims(&mut m);
        assert!(moved >= 1);
        for p in &m.positions {
            assert!(
                p[1].abs() <= COPLANAR_NUDGE + 1e-4,
                "rank grew past 1: {p:?}"
            );
        }
    }
}
