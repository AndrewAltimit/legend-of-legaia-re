//! Repairing the **grip** of an item-alone cut - inferring the shaft the
//! closed fist hid.
//!
//! [`equip_isolate`](super::equip_isolate) hands back the held item with the
//! hand removed, and for a welded weapon (every one of Vahn's) that leaves a
//! hole where the fist was: the part of the haft inside the closed hand was
//! never modelled, so the great axe comes out in two pieces, an axe head on
//! a stub of shaft above the grip and the pommel end below it, each ending
//! in an open ring of vertices where it met the fingers. Retail never drew
//! that stretch of shaft - the fist covered it - so no cut recovers it. But
//! the two rings say what was there: two coaxial polygon loops of the same
//! size, facing each other across a gap the width of a hand, are the two
//! ends of one straight prism. This module finds such pairs and lofts a tube
//! between them, textured from one rim, so the item downloads as one piece.
//!
//! What it deliberately does **not** do:
//!
//! * It never bridges loops that face *away* from each other. A Ra-Seru
//!   armband cut from the forearm is open at both ends, but its two rims
//!   look outward, and connecting them would run a tube back through the
//!   cuff. The same test leaves a helmet's neck and face openings alone.
//! * It never caps a lone loop. A blade whose whole hilt was inside the
//!   fist has one open ring and no partner; a flat lid there would be a
//!   guess about the item's silhouette, where a bridge is a guess only about
//!   the stretch of shaft between two ends that both exist. Bridging is
//!   inference; capping would be invention.
//! * It only runs where the caller asks - the item-alone export and its
//!   preview - never on the record-keeping palette cut, which stays exact.
//!
//! The mesh is worked in **object-local** space (the same space the glTF
//! nodes are posed from), and a bridge is only ever built between two loops
//! of the **same** object: a tube spanning two bones would tear the moment
//! either moved.

use std::collections::{BTreeMap, HashMap};

use legaia_tmd::mesh::VramMesh;

/// A loop pair may be bridged only when the gap between the loop centroids
/// is at most this many mean loop radii - roughly the width of a closed
/// hand against the shaft it grips, with margin. Two loops further apart are
/// two ends of different things.
const MAX_GAP_RADII: f32 = 6.0;
/// The two loops must be roughly the same size (ratio of mean radii).
const MAX_RADIUS_RATIO: f32 = 2.0;
/// Cosine bound for "the loop's open side faces the other loop": the line
/// joining the two centroids must lie within ~40 degrees of each loop's
/// opening direction.
const MIN_FACING_COS: f32 = 0.75;
/// The lateral offset between the two loops (their centroid separation
/// projected onto the loop plane) may be at most this many mean radii - the
/// two ends of one straight shaft are coaxial.
const MAX_LATERAL_RADII: f32 = 0.9;
/// Two loop vertices in the same object closer than this are one point (the
/// TMD stores integer coordinates, so this only absorbs float noise).
const WELD_EPS: f32 = 0.01;

/// One tube the repair added.
#[derive(Debug, Clone, PartialEq)]
pub struct Bridge {
    /// Object both loops belong to (per-vertex object id of the mesh).
    pub object: u32,
    /// Vertex counts of the two rims.
    pub loop_a: usize,
    pub loop_b: usize,
    /// Distance between the two rim centroids, in model units.
    pub gap: f32,
    /// Triangles added.
    pub triangles: usize,
}

/// One closed boundary loop of the mesh, in weld space.
struct Loop {
    object: u32,
    /// Weld ids in walk order.
    ring: Vec<usize>,
    centroid: [f32; 3],
    /// Unit direction the opening faces: away from the surface behind the
    /// rim, through the hole - for a prism stub, along its axis.
    open: [f32; 3],
    /// Mean distance of a rim vertex from the centroid.
    radius: f32,
}

/// Find the open rims of `mesh` (per-vertex object ids `object_ids`), pair the
/// ones that are two ends of one shaft, and append a lofted tube between
/// each pair. Returns what was built; the mesh and its object ids grow in
/// place (new vertices are copies of rim vertices, so every attribute stream
/// stays parallel).
///
/// Only rims on the **same** object are ever paired. Measured over every
/// held-item record on the disc, a cross-object pairing (compared in
/// rest-pose world space) found no real grip - Vahn's swords, which put the
/// blade on the hand object and the pommel block on the forearm, have both
/// pieces closed at the fist, so there is no rim to bridge - and it did
/// pair the elbow-facing rims of two Ra-Seru arm plates, which is a tube
/// through a joint. So the same-bone rule is not a simplification; it is
/// what the data supports.
pub fn bridge_open_loops(mesh: &mut VramMesh, object_ids: &mut Vec<u32>) -> Vec<Bridge> {
    let n = mesh.positions.len();
    if n < 3 || mesh.indices.len() < 3 || object_ids.len() != n {
        return Vec::new();
    }
    // ---- Weld by (object, position) so per-prim duplicated corners share
    // an id and the boundary walk sees real adjacency. ----
    let mut weld_of = vec![usize::MAX; n];
    let mut weld_rep: Vec<usize> = Vec::new(); // representative vertex per weld id
    {
        let mut table: HashMap<(u32, i64, i64, i64), usize> = HashMap::new();
        let q = |x: f32| (x / WELD_EPS).round() as i64;
        for v in 0..n {
            let p = mesh.positions[v];
            let key = (object_ids[v], q(p[0]), q(p[1]), q(p[2]));
            let id = *table.entry(key).or_insert_with(|| {
                weld_rep.push(v);
                weld_rep.len() - 1
            });
            weld_of[v] = id;
        }
    }
    // ---- Undirected edge -> (use count, one adjacent triangle, the mesh
    // vertex indices at each end from that triangle). ----
    struct EdgeUse {
        count: u32,
        tri: usize,
        /// Mesh vertex index at the lower / higher weld id, taken from the
        /// first triangle seen (carries the UV / material of that rim).
        lo_v: u32,
        hi_v: u32,
    }
    let mut edges: HashMap<(usize, usize), EdgeUse> = HashMap::new();
    let tri_count = mesh.indices.len() / 3;
    for t in 0..tri_count {
        let idx = [
            mesh.indices[t * 3],
            mesh.indices[t * 3 + 1],
            mesh.indices[t * 3 + 2],
        ];
        for k in 0..3 {
            let a = idx[k];
            let b = idx[(k + 1) % 3];
            let (wa, wb) = (weld_of[a as usize], weld_of[b as usize]);
            if wa == wb {
                continue; // degenerate edge
            }
            let (lo, hi, lo_v, hi_v) = if wa < wb {
                (wa, wb, a, b)
            } else {
                (wb, wa, b, a)
            };
            edges
                .entry((lo, hi))
                .and_modify(|e| e.count += 1)
                .or_insert(EdgeUse {
                    count: 1,
                    tri: t,
                    lo_v,
                    hi_v,
                });
        }
    }
    // ---- Boundary adjacency: weld id -> its boundary neighbours. ----
    let mut adj: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut boundary: HashMap<(usize, usize), &EdgeUse> = HashMap::new();
    for (k, e) in &edges {
        if e.count == 1 {
            adj.entry(k.0).or_default().push(k.1);
            adj.entry(k.1).or_default().push(k.0);
            boundary.insert(*k, e);
        }
    }
    // ---- Walk closed loops. A weld vertex with boundary degree != 2 is a
    // junction the walk refuses to cross (its loops are ambiguous). ----
    let mut visited: HashMap<usize, bool> = HashMap::new();
    let mut loops: Vec<Loop> = Vec::new();
    let pos_of = |w: usize| mesh.positions[weld_rep[w]];
    for (&start, nb) in &adj {
        if nb.len() != 2 || visited.get(&start).copied().unwrap_or(false) {
            continue;
        }
        let mut ring = vec![start];
        let mut prev = start;
        let mut cur = nb[0];
        let mut closed = false;
        while ring.len() <= adj.len() {
            let Some(cn) = adj.get(&cur) else { break };
            if cn.len() != 2 {
                break;
            }
            if cur == start {
                closed = true;
                break;
            }
            ring.push(cur);
            let next = if cn[0] == prev { cn[1] } else { cn[0] };
            prev = cur;
            cur = next;
        }
        for &w in &ring {
            visited.insert(w, true);
        }
        if !closed || ring.len() < 3 {
            continue;
        }
        let object = object_ids[weld_rep[start]];
        // Centroid, radius.
        let mut c = [0.0f32; 3];
        for &w in &ring {
            let p = pos_of(w);
            for k in 0..3 {
                c[k] += p[k];
            }
        }
        for v in &mut c {
            *v /= ring.len() as f32;
        }
        let radius = ring.iter().map(|&w| dist(pos_of(w), c)).sum::<f32>() / ring.len() as f32;
        if radius <= 0.0 {
            continue;
        }
        // Open direction: from the adjacent triangles' far corners towards
        // the rim, then snapped onto the rim plane's normal so it is exactly
        // "through the hole".
        let mut open = [0.0f32; 3];
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            let key = if a < b { (a, b) } else { (b, a) };
            let Some(e) = boundary.get(&key) else {
                continue;
            };
            let t = e.tri;
            let tri = [
                mesh.indices[t * 3] as usize,
                mesh.indices[t * 3 + 1] as usize,
                mesh.indices[t * 3 + 2] as usize,
            ];
            let mut tc = [0.0f32; 3];
            for &vi in &tri {
                for (acc, x) in tc.iter_mut().zip(mesh.positions[vi]) {
                    *acc += x / 3.0;
                }
            }
            let mid = mid(pos_of(a), pos_of(b));
            for k in 0..3 {
                open[k] += mid[k] - tc[k];
            }
        }
        // The rim need not be perpendicular to the shaft - a fist grips
        // diagonally, and the fingers' edge cuts the haft obliquely - so the
        // opening direction is taken from the geometry *behind* the rim
        // (which for a prism runs along its axis), not from the rim's plane.
        let Some(open) = normalize(open) else {
            continue;
        };
        loops.push(Loop {
            object,
            ring,
            centroid: c,
            open,
            radius,
        });
    }
    if loops.len() < 2 {
        return Vec::new();
    }
    // ---- Pair loops: same object, facing each other, coaxial, similar
    // size, close. Greedy by gap. ----
    let mut candidates: Vec<(f32, usize, usize)> = Vec::new();
    for i in 0..loops.len() {
        for j in (i + 1)..loops.len() {
            let (a, b) = (&loops[i], &loops[j]);
            if a.object != b.object {
                continue;
            }
            let v = sub(b.centroid, a.centroid);
            let gap = len(v);
            if gap <= 0.0 {
                continue;
            }
            let dir = [v[0] / gap, v[1] / gap, v[2] / gap];
            if dot(a.open, dir) < MIN_FACING_COS || dot(b.open, dir) > -MIN_FACING_COS {
                continue;
            }
            let rmax = a.radius.max(b.radius);
            let rmin = a.radius.min(b.radius);
            if rmax / rmin > MAX_RADIUS_RATIO {
                continue;
            }
            let rmean = (a.radius + b.radius) * 0.5;
            if gap > MAX_GAP_RADII * rmean {
                continue;
            }
            // Lateral offset: the separation not along the first loop's axis.
            let along = dot(v, a.open);
            let lateral = len(sub(
                v,
                [a.open[0] * along, a.open[1] * along, a.open[2] * along],
            ));
            if lateral > MAX_LATERAL_RADII * rmean {
                continue;
            }
            candidates.push((gap, i, j));
        }
    }
    candidates.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut used = vec![false; loops.len()];
    let mut out = Vec::new();
    for (gap, i, j) in candidates {
        if used[i] || used[j] {
            continue;
        }
        used[i] = true;
        used[j] = true;
        // Rim vertex (mesh index) per weld id, from the boundary triangles:
        // the UV / material / colour a bridge vertex copies.
        let rim_vertex = |lp: &Loop| -> Vec<u32> {
            let mut out = Vec::with_capacity(lp.ring.len());
            for k in 0..lp.ring.len() {
                let w = lp.ring[k];
                let nb = lp.ring[(k + 1) % lp.ring.len()];
                let key = if w < nb { (w, nb) } else { (nb, w) };
                let v = boundary
                    .get(&key)
                    .map(|e| if key.0 == w { e.lo_v } else { e.hi_v })
                    .unwrap_or(weld_rep[w] as u32);
                out.push(v);
            }
            out
        };
        // The rim with more vertices hosts the tube and donates its material.
        let (i, j) = if loops[i].ring.len() >= loops[j].ring.len() {
            (i, j)
        } else {
            (j, i)
        };
        let (a, b) = (&loops[i], &loops[j]);
        let ra = rim_vertex(a);
        let rb = rim_vertex(b);
        let rb_pos: Vec<[f32; 3]> = rb.iter().map(|&v| mesh.positions[v as usize]).collect();
        let b_centroid = b.centroid;
        let axis = normalize(sub(b_centroid, a.centroid)).unwrap_or(a.open);
        let tris = loft(mesh, object_ids, a, &ra, b_centroid, &rb_pos, axis);
        if tris == 0 {
            continue;
        }
        out.push(Bridge {
            object: a.object,
            loop_a: a.ring.len(),
            loop_b: b.ring.len(),
            gap,
            triangles: tris,
        });
    }
    out
}

/// Loft a tube between rims `a` and `b` around `axis` (unit, from `a` to
/// `b`). New vertices are copies of the rim vertices; the **`a` rim is the
/// material donor** - every bridge vertex takes its UV / CBA-TSB / colour
/// from the `a` rim vertex nearest in angle, so the tube reads as the shaft
/// continuing rather than as a smear between two unrelated texture spots.
/// Returns the triangle count added.
fn loft(
    mesh: &mut VramMesh,
    object_ids: &mut Vec<u32>,
    a: &Loop,
    ra: &[u32],
    b_centroid: [f32; 3],
    rb_pos: &[[f32; 3]],
    axis: [f32; 3],
) -> usize {
    // Basis in the plane perpendicular to the axis.
    let helper = if axis[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let Some(e1) = normalize(cross(axis, helper)) else {
        return 0;
    };
    let e2 = cross(axis, e1);
    let angle = |p: [f32; 3], c: [f32; 3]| -> f32 {
        let d = sub(p, c);
        dot(d, e2).atan2(dot(d, e1))
    };
    // Rim vertices sorted by angle, as (angle, rim slot). Positions are in
    // `a`'s object space for both rims (`rb_pos` for the `b` rim).
    let mut sa: Vec<(f32, usize)> = ra
        .iter()
        .enumerate()
        .map(|(k, &vi)| (angle(mesh.positions[vi as usize], a.centroid), k))
        .collect();
    sa.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut sb: Vec<(f32, usize)> = rb_pos
        .iter()
        .enumerate()
        .map(|(k, p)| (angle(*p, b_centroid), k))
        .collect();
    sb.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    if sa.len() < 3 || sb.len() < 3 {
        return 0;
    }
    // Donor lookup: nearest `a` rim vertex (mesh index) by angle.
    let donor_for = |ang: f32| -> u32 {
        let mut best = ra[sa[0].1];
        let mut bd = f32::INFINITY;
        for &(t, v) in &sa {
            let mut d = (t - ang).abs();
            if d > std::f32::consts::PI {
                d = std::f32::consts::TAU - d;
            }
            if d < bd {
                bd = d;
                best = ra[v];
            }
        }
        best
    };
    // Emit new vertices: position from the rim vertex, everything else from
    // the donor.
    let ra_pos: Vec<[f32; 3]> = ra.iter().map(|&v| mesh.positions[v as usize]).collect();
    let object = a.object;
    let mut push = |pos: [f32; 3], donor: u32| -> u32 {
        let id = mesh.positions.len() as u32;
        mesh.positions.push(pos);
        mesh.uvs.push(mesh.uvs[donor as usize]);
        mesh.cba_tsb.push(mesh.cba_tsb[donor as usize]);
        mesh.normals.push([0.0; 3]);
        mesh.colors.push(
            mesh.colors
                .get(donor as usize)
                .copied()
                .unwrap_or([0x80; 3]),
        );
        object_ids.push(object);
        id
    };
    let na: Vec<u32> = sa.iter().map(|&(_, k)| push(ra_pos[k], ra[k])).collect();
    // Rotate the `b` rim so it starts at the vertex nearest `a`'s first in
    // angle, and unroll its angles into one monotone run starting within
    // half a turn before `a`'s start - the merge below then only ever
    // compares like with like.
    let (la, lb) = (sa.len(), sb.len());
    let ang_dist = |x: f32, y: f32| {
        let d = (x - y).abs();
        if d > std::f32::consts::PI {
            std::f32::consts::TAU - d
        } else {
            d
        }
    };
    let j0 = (0..lb)
        .min_by(|&x, &y| {
            ang_dist(sb[x].0, sa[0].0)
                .partial_cmp(&ang_dist(sb[y].0, sa[0].0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    let mut ang_b: Vec<f32> = (0..lb)
        .map(|k| {
            let idx = (j0 + k) % lb;
            sb[idx].0
                + if j0 + k >= lb {
                    std::f32::consts::TAU
                } else {
                    0.0
                }
        })
        .collect();
    if ang_b[0] - sa[0].0 > std::f32::consts::PI {
        for t in &mut ang_b {
            *t -= std::f32::consts::TAU;
        }
    }
    let nb: Vec<u32> = (0..lb)
        .map(|k| {
            let (t, slot) = sb[(j0 + k) % lb];
            push(rb_pos[slot], donor_for(t))
        })
        .collect();
    let ang_a: Vec<f32> = sa.iter().map(|&(t, _)| t).collect();
    let next_a = |i: usize| {
        if i + 1 < la {
            ang_a[i + 1]
        } else {
            ang_a[0] + std::f32::consts::TAU
        }
    };
    let next_b = |k: usize| {
        if k + 1 < lb {
            ang_b[k + 1]
        } else {
            ang_b[0] + std::f32::consts::TAU
        }
    };
    // Zip the two rings: at each step advance whichever ring's next vertex
    // comes first in angle, emitting one triangle per advance. `la + lb`
    // advances close the tube by themselves (the indices wrap).
    let (mut i, mut k) = (0usize, 0usize);
    let mut new_indices: Vec<u32> = Vec::with_capacity((la + lb) * 3);
    while i < la || k < lb {
        let ai = na[i % la];
        let bk = nb[k % lb];
        let adv_a = k >= lb || (i < la && next_a(i) <= next_b(k));
        if adv_a {
            new_indices.extend_from_slice(&[ai, bk, na[(i + 1) % la]]);
            i += 1;
        } else {
            new_indices.extend_from_slice(&[ai, bk, nb[(k + 1) % lb]]);
            k += 1;
        }
    }
    // Orient every triangle outward (away from the axis line), so a
    // back-face-culling viewer sees the tube from outside.
    let mid_axis = mid(a.centroid, b_centroid);
    for t in new_indices.chunks_exact_mut(3) {
        let p0 = mesh.positions[t[0] as usize];
        let p1 = mesh.positions[t[1] as usize];
        let p2 = mesh.positions[t[2] as usize];
        let n = cross(sub(p1, p0), sub(p2, p0));
        let c = [
            (p0[0] + p1[0] + p2[0]) / 3.0,
            (p0[1] + p1[1] + p2[1]) / 3.0,
            (p0[2] + p1[2] + p2[2]) / 3.0,
        ];
        // Radial direction from the axis line to the triangle centre.
        let d = sub(c, mid_axis);
        let along = dot(d, axis);
        let radial = sub(d, [axis[0] * along, axis[1] * along, axis[2] * along]);
        if dot(n, radial) < 0.0 {
            t.swap(1, 2);
        }
    }
    // Drop degenerate triangles (two corners at one weld position).
    let mut kept = 0usize;
    for t in new_indices.chunks_exact(3) {
        let p0 = mesh.positions[t[0] as usize];
        let p1 = mesh.positions[t[1] as usize];
        let p2 = mesh.positions[t[2] as usize];
        if len(cross(sub(p1, p0), sub(p2, p0))) < 1e-6 {
            continue;
        }
        mesh.indices.extend_from_slice(t);
        kept += 1;
    }
    kept
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mid(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0]) * 0.5,
        (a[1] + b[1]) * 0.5,
        (a[2] + b[2]) * 0.5,
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn len(v: [f32; 3]) -> f32 {
    dot(v, v).sqrt()
}
fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    len(sub(a, b))
}
fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let l = len(v);
    if l < 1e-6 {
        None
    } else {
        Some([v[0] / l, v[1] / l, v[2] / l])
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A square tube of `n` rings along +Y between `y0` and `y1`, open at
    /// both ends, radius `r`. Every quad is two triangles with their own
    /// vertices (as the TMD mesh builder emits them).
    fn open_tube(mesh: &mut VramMesh, ids: &mut Vec<u32>, object: u32, y0: f32, y1: f32, r: f32) {
        let corners = [[r, 0.0, r], [-r, 0.0, r], [-r, 0.0, -r], [r, 0.0, -r]];
        let mut quad = |p: [[f32; 3]; 4]| {
            let base = mesh.positions.len() as u32;
            for c in p {
                mesh.positions.push(c);
                mesh.uvs.push([7, 9]);
                mesh.cba_tsb.push([0x1234, 0x0056]);
                mesh.normals.push([0.0; 3]);
                mesh.colors.push([0x80; 3]);
                ids.push(object);
            }
            mesh.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        };
        for k in 0..4 {
            let a = corners[k];
            let b = corners[(k + 1) % 4];
            quad([
                [a[0], y0, a[2]],
                [b[0], y0, b[2]],
                [b[0], y1, b[2]],
                [a[0], y1, a[2]],
            ]);
        }
    }

    fn empty() -> VramMesh {
        VramMesh {
            positions: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
        }
    }

    #[test]
    fn two_coaxial_stubs_facing_each_other_get_bridged() {
        let mut mesh = empty();
        let mut ids = Vec::new();
        // Upper stub y in [10, 30], lower stub y in [-30, -10]: a 20-unit
        // gap (the fist) between two 4-unit-radius shaft ends.
        open_tube(&mut mesh, &mut ids, 3, 10.0, 30.0, 4.0);
        open_tube(&mut mesh, &mut ids, 3, -30.0, -10.0, 4.0);
        let before_tris = mesh.indices.len() / 3;
        let bridges = bridge_open_loops(&mut mesh, &mut ids);
        assert_eq!(bridges.len(), 1, "{bridges:?}");
        let b = &bridges[0];
        assert_eq!(b.object, 3);
        assert_eq!((b.loop_a, b.loop_b), (4, 4));
        assert!((b.gap - 20.0).abs() < 1e-3, "gap {}", b.gap);
        // A 4-to-4 loft is 8 triangles (4 quads).
        assert_eq!(b.triangles, 8);
        assert_eq!(mesh.indices.len() / 3, before_tris + 8);
        assert_eq!(mesh.positions.len(), ids.len());
        assert_eq!(mesh.uvs.len(), mesh.positions.len());
        // Every bridge vertex sits on one of the two rims (y = 10 or -10).
        for p in &mesh.positions[mesh.positions.len() - 8..] {
            assert!((p[1].abs() - 10.0).abs() < 1e-3, "{p:?}");
        }
        // The far ends (y = 30 / -30) remain open - only the gap was closed:
        // re-running finds two loops that face away from each other.
        assert!(bridge_open_loops(&mut mesh, &mut ids).is_empty());
    }

    #[test]
    fn a_cuff_open_at_both_ends_is_left_alone() {
        // One tube: its two rims face away from each other (a Ra-Seru
        // armband, a helmet's neck + crown holes).
        let mut mesh = empty();
        let mut ids = Vec::new();
        open_tube(&mut mesh, &mut ids, 0, -10.0, 10.0, 6.0);
        assert!(bridge_open_loops(&mut mesh, &mut ids).is_empty());
    }

    #[test]
    fn stubs_on_different_objects_or_off_axis_are_not_bridged() {
        let mut mesh = empty();
        let mut ids = Vec::new();
        open_tube(&mut mesh, &mut ids, 1, 10.0, 30.0, 4.0);
        open_tube(&mut mesh, &mut ids, 2, -30.0, -10.0, 4.0);
        assert!(bridge_open_loops(&mut mesh, &mut ids).is_empty(), "objects");

        let mut mesh = empty();
        let mut ids = Vec::new();
        open_tube(&mut mesh, &mut ids, 1, 10.0, 30.0, 4.0);
        // Lower stub shifted 12 units sideways: not coaxial.
        let n0 = mesh.positions.len();
        open_tube(&mut mesh, &mut ids, 1, -30.0, -10.0, 4.0);
        for p in &mut mesh.positions[n0..] {
            p[0] += 12.0;
        }
        assert!(bridge_open_loops(&mut mesh, &mut ids).is_empty(), "lateral");

        let mut mesh = empty();
        let mut ids = Vec::new();
        open_tube(&mut mesh, &mut ids, 1, 10.0, 30.0, 4.0);
        // 200 units apart: not one shaft.
        open_tube(&mut mesh, &mut ids, 1, -230.0, -210.0, 4.0);
        assert!(bridge_open_loops(&mut mesh, &mut ids).is_empty(), "gap");
    }

    #[test]
    fn unequal_rims_zip_to_a_closed_tube() {
        // A 4-gon rim against a 6-gon rim: the zip must still close.
        let mut mesh = empty();
        let mut ids = Vec::new();
        open_tube(&mut mesh, &mut ids, 0, 10.0, 30.0, 4.0);
        // Hexagonal lower stub built by hand.
        let r = 4.0f32;
        let hex: Vec<[f32; 2]> = (0..6)
            .map(|k| {
                let a = k as f32 * std::f32::consts::TAU / 6.0;
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        for k in 0..6 {
            let a = hex[k];
            let b = hex[(k + 1) % 6];
            let base = mesh.positions.len() as u32;
            for c in [
                [a[0], -30.0, a[1]],
                [b[0], -30.0, b[1]],
                [b[0], -10.0, b[1]],
                [a[0], -10.0, a[1]],
            ] {
                mesh.positions.push(c);
                mesh.uvs.push([1, 1]);
                mesh.cba_tsb.push([1, 1]);
                mesh.normals.push([0.0; 3]);
                mesh.colors.push([0x80; 3]);
                ids.push(0);
            }
            mesh.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let bridges = bridge_open_loops(&mut mesh, &mut ids);
        assert_eq!(bridges.len(), 1, "{bridges:?}");
        assert_eq!(bridges[0].loop_a + bridges[0].loop_b, 10);
        // 4 + 6 rim vertices -> 10 triangles.
        assert_eq!(bridges[0].triangles, 10);
        // After the bridge the gap rims are closed: the only open loops left
        // are the two far ends, which face away from each other.
        assert!(bridge_open_loops(&mut mesh, &mut ids).is_empty());
        // Every bridge vertex borrowed the material of the rim with more
        // vertices (the hexagon - the donor).
        for k in mesh.cba_tsb.len() - 10..mesh.cba_tsb.len() {
            assert_eq!(mesh.cba_tsb[k], [1, 1]);
        }
    }
}
