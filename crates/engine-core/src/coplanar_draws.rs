//! Cross-draw coplanar-surface separation for assembled field/world scenes.
//!
//! The `.MAP` terrain-tile and placed-object sweeps routinely put two draws'
//! geometry on the **same world plane** with large overlap: adjacent ground
//! tiles whose meshes span past their cell, a placed slab over a terrain
//! tile, two instances of the same tile mesh half a cell apart. Retail's
//! GPU resolves these painter-style through the ordering table (no depth
//! buffer, a stable per-bucket winner); the port's depth-tested renderers
//! turn each pair into per-pixel z-fighting because the two draws reach the
//! same plane through *different* model transforms, so their interpolated
//! depths differ only by float rounding.
//!
//! [`coplanar_draw_offsets`] restores a deterministic winner at the geometry
//! level: it detects the coplanar overlap clusters across a scene's resolved
//! [`EnvDraw`] list and hands back a small world-space offset (multiples of
//! [`DRAW_NUDGE`] along the surface's visible side) for every draw that needs
//! to lift off a larger/earlier base. Both renderers (the native play-window
//! and the web viewer's assembled scene) apply the offset to the draw's
//! translation, so the two stay pixel-consistent.
//!
//! This is the cross-draw sibling of the intra-mesh pass
//! `legaia_tmd::mesh::separate_coplanar_prims`; see that module for why the
//! nudge magnitude is safe (retail art authors ~1-unit decal offsets itself).

use crate::field_env::EnvDraw;
use std::collections::HashMap;

/// World-units offset per coplanar rank. Larger than the intra-mesh prim
/// nudge (`0.5`) so a lifted *draw* also clears its own mesh's already-nudged
/// decal layers - and deliberately NOT commensurate with either authored
/// practice or the intra nudge. Retail art lays parallel decal layers on a
/// ~1-unit lattice, so a whole-draw lift of exactly `1.0` maps a mesh's
/// authored offset layer straight onto the neighbouring draw's base plane:
/// the pass then *creates* the coincidence it exists to remove (koin4's
/// res14/res15 floor pair fought only after the lift). `0.75` keeps every
/// lifted plane at least `0.25` world units away from the `{0, 0.5, 1.0,
/// 1.5, 2.0}` lattice of neighbouring bases, intra-nudged decals and
/// authored layers - far above depth-buffer resolution at scene scale.
pub const DRAW_NUDGE: f32 = 0.75;

/// Planes smaller than this total triangle area within one mesh are ignored -
/// they cannot host the overlaps that read as shimmer, and skipping them
/// keeps the cluster graph small. `64` (an 8x8-unit patch, 1/256 of a field
/// tile) is deliberately low: the survivor population at `512` was dominated
/// by sub-512 wall / kerb strips along tile seams, each a visible flicker
/// band at oblique angles.
const MIN_PLANE_AREA: f32 = 64.0;

/// One significant plane of a mesh, in object-local space.
#[derive(Debug, Clone, Copy)]
struct Plane {
    /// Unit normal, canonical orientation (dominant component positive).
    n: [f32; 3],
    /// Visible-side unit direction (`-cross(e1, e2)` of the emitted winding;
    /// see `legaia_tmd::mesh` for the derivation).
    vis: [f32; 3],
    /// A vertex on the plane (the cluster's first triangle's `v0`). Plane
    /// *distance* comparisons go through this point, never through `d - d`:
    /// two matching sloped clusters carry representative normals that differ
    /// by quantization noise, and at world-scale coordinates that noise
    /// scales into `|dn| * |v|` (tens of units) of spurious `d` difference.
    point: [f32; 3],
    /// Total triangle area on this plane.
    area: f32,
    /// Object-local AABB of the plane's triangles.
    lo: [f32; 3],
    hi: [f32; 3],
}

/// The significant planes of one mesh, ready to instance under a draw's
/// world transform. Build once per distinct `res_tmd` via [`mesh_planes`].
#[derive(Debug, Clone, Default)]
pub struct MeshPlanes {
    planes: Vec<Plane>,
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

/// Extract the significant planes of a triangle mesh (positions + triangle
/// index list - the same buffers the renderers upload).
pub fn mesh_planes(positions: &[[f32; 3]], indices: &[u32]) -> MeshPlanes {
    #[derive(Default)]
    struct Acc {
        n: [f32; 3],
        vis: [f32; 3],
        point: [f32; 3],
        area: f32,
        lo: [f32; 3],
        hi: [f32; 3],
        init: bool,
    }
    let mut acc: HashMap<(i32, i32, i32, i64), Acc> = HashMap::new();
    for t in indices.chunks_exact(3) {
        let (Some(&v0), Some(&v1), Some(&v2)) = (
            positions.get(t[0] as usize),
            positions.get(t[1] as usize),
            positions.get(t[2] as usize),
        ) else {
            continue;
        };
        let c = cross(sub(v1, v0), sub(v2, v0));
        let len = dot(c, c).sqrt();
        if len < 1e-6 {
            continue;
        }
        let area = len * 0.5;
        let vis = [-c[0] / len, -c[1] / len, -c[2] / len];
        let mut n = [-vis[0], -vis[1], -vis[2]];
        let ax = dominant_axis(n);
        if n[ax] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let d = dot(n, v0);
        let key = (
            (n[0] * 256.0).round() as i32,
            (n[1] * 256.0).round() as i32,
            (n[2] * 256.0).round() as i32,
            (d / 0.5).round() as i64,
        );
        let a = acc.entry(key).or_default();
        if !a.init {
            a.n = n;
            a.vis = vis;
            a.point = v0;
            a.lo = v0;
            a.hi = v0;
            a.init = true;
        }
        a.area += area;
        for v in [v0, v1, v2] {
            for (ax, &val) in v.iter().enumerate() {
                if val < a.lo[ax] {
                    a.lo[ax] = val;
                }
                if val > a.hi[ax] {
                    a.hi[ax] = val;
                }
            }
        }
    }
    MeshPlanes {
        planes: acc
            .into_values()
            .filter(|a| a.area >= MIN_PLANE_AREA)
            .map(|a| Plane {
                n: a.n,
                vis: a.vis,
                point: a.point,
                area: a.area,
                lo: a.lo,
                hi: a.hi,
            })
            .collect(),
    }
}

/// A plane instanced into world space under one draw's transform.
#[derive(Debug, Clone, Copy)]
struct WorldPlane {
    draw: usize,
    n: [f32; 3],
    vis: [f32; 3],
    /// World-space on-plane vertex (see [`Plane::point`]).
    point: [f32; 3],
    area: f32,
    lo: [f32; 3],
    hi: [f32; 3],
}

fn rot_y(v: [f32; 3], sin: f32, cos: f32) -> [f32; 3] {
    // Retail pure-Y rotation (FUN_80026988): local +Z -> (sin, 0, cos).
    [cos * v[0] + sin * v[2], v[1], -sin * v[0] + cos * v[2]]
}

fn instance_plane(p: &Plane, d: &EnvDraw) -> WorldPlane {
    // Known gap: this rotates by yaw only, while the draw itself now composes
    // all three authored angles (`battle_intro::placement_rotation`). The
    // handful of disc placements carrying a real X/Z tilt therefore get a
    // plane whose normal is off by that tilt. Harmless for the pure-yaw
    // majority; fixing it means sharing the same composition here.
    let ang = f32::from(d.rot_y & 0x0FFF) * (std::f32::consts::TAU / 4096.0);
    let (s, c) = ang.sin_cos();
    let t = [d.world_x as f32, d.world_y as f32, d.world_z as f32];
    let mut n = rot_y(p.n, s, c);
    let vis = rot_y(p.vis, s, c);
    // Re-canonicalise after rotation so matching planes hash together.
    let ax = dominant_axis(n);
    if n[ax] < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    // Rotate the 8 AABB corners; take the world AABB.
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for cx in [p.lo[0], p.hi[0]] {
        for cy in [p.lo[1], p.hi[1]] {
            for cz in [p.lo[2], p.hi[2]] {
                let w = rot_y([cx, cy, cz], s, c);
                for ax in 0..3 {
                    let v = w[ax] + t[ax];
                    if v < lo[ax] {
                        lo[ax] = v;
                    }
                    if v > hi[ax] {
                        hi[ax] = v;
                    }
                }
            }
        }
    }
    let wp = rot_y(p.point, s, c);
    WorldPlane {
        draw: 0,
        n,
        vis,
        point: [wp[0] + t[0], wp[1] + t[1], wp[2] + t[2]],
        area: p.area,
        lo,
        hi,
    }
}

/// AABB overlap with a shrink margin so edge-adjacent tiles don't count.
fn aabb_overlaps(a: &WorldPlane, b: &WorldPlane) -> bool {
    const MARGIN: f32 = 1.5;
    (0..3).all(|ax| {
        a.lo[ax] + MARGIN < b.hi[ax] && b.lo[ax] + MARGIN < a.hi[ax] || {
            // A perfectly flat axis (plane thickness 0) still overlaps.
            (a.hi[ax] - a.lo[ax]) < MARGIN * 2.0 || (b.hi[ax] - b.lo[ax]) < MARGIN * 2.0
        } && a.lo[ax] <= b.hi[ax]
            && b.lo[ax] <= a.hi[ax]
    })
}

/// Detect cross-draw coplanar overlap clusters and return the world-space
/// offset each affected draw should add to its translation. Draws without a
/// conflict are absent from the map.
///
/// `planes_of` maps a draw's `res_tmd` to its [`MeshPlanes`] (see
/// [`mesh_planes`]); draws whose mesh has no entry are skipped.
///
/// Within a cluster the draw presenting the **largest** plane stays put and
/// each overlapping smaller/later draw lifts `DRAW_NUDGE` units per rank
/// toward the surface's visible side - the same "small decal wins" outcome
/// retail's mean-Z ordering-table bucketing produces. Ranks are assigned by
/// greedy graph colouring, so chains of adjacent tiles alternate 0/1 instead
/// of accumulating.
pub fn coplanar_draw_offsets(
    draws: &[EnvDraw],
    planes_of: &HashMap<usize, MeshPlanes>,
) -> HashMap<EnvDraw, [f32; 3]> {
    // Instance every significant plane into world space.
    let mut world: Vec<WorldPlane> = Vec::new();
    for (di, d) in draws.iter().enumerate() {
        let Some(mp) = planes_of.get(&d.res_tmd) else {
            continue;
        };
        for p in &mp.planes {
            let mut wp = instance_plane(p, d);
            wp.draw = di;
            world.push(wp);
        }
    }
    // Bucket by quantized plane, straddling BOTH the normal quanta and the
    // distance quanta. Two matching sloped clusters differ in their rep
    // normals by float noise, so a single-key normal hash separates them;
    // and the distance scalar must be computed against the KEY's normal
    // (identical for everything in the bucket), because each cluster's own
    // `d` amplifies that normal noise by the world coordinate magnitude
    // (`|dn| * |v|` = tens of units at map scale).
    let mut buckets: HashMap<(i32, i32, i32, i64), Vec<usize>> = HashMap::new();
    for (i, w) in world.iter().enumerate() {
        let base = (
            (w.n[0] * 128.0).round() as i32,
            (w.n[1] * 128.0).round() as i32,
            (w.n[2] * 128.0).round() as i32,
        );
        for dx in -1..=1i32 {
            for dy in -1..=1i32 {
                for dz in -1..=1i32 {
                    let k = (base.0 + dx, base.1 + dy, base.2 + dz);
                    let nq = [k.0 as f32 / 128.0, k.1 as f32 / 128.0, k.2 as f32 / 128.0];
                    let len = dot(nq, nq).sqrt();
                    if len < 0.5 {
                        continue;
                    }
                    let nq = [nq[0] / len, nq[1] / len, nq[2] / len];
                    let dq = dot(nq, w.point);
                    for dd in -1..=1i64 {
                        buckets
                            .entry((k.0, k.1, k.2, (dq / 2.0).floor() as i64 + dd))
                            .or_default()
                            .push(i);
                    }
                }
            }
        }
    }
    // Conflict edges between draws, grouped by **normal family** (nearly
    // parallel world normals of the conflicting plane pairs). A draw can
    // conflict on several non-parallel families at once - its floor apron
    // against one neighbour AND its wall strip against another - and a lift
    // along one family's normal is orthogonal to the other family's plane
    // offset, so resolving "the largest conflict" alone leaves every other
    // family coincident (koin4: a draw lifted along Z for its floor tie kept
    // z-fighting a wall on X). Each family therefore gets its own colouring
    // and its own lift component; the components sum.
    //
    // Families are clustered by angle against a representative normal, NOT by
    // quantizing the normal into a key: a wall shell's plane clusters differ
    // by float noise (one rounds to `(0, -1, 256)`, its twin to `(0, 0,
    // 256)`), and two quantization-split families over the same physical
    // plane rank the same draw pair independently - their per-family lifts
    // then cancel (opposite `vis`) or coincide (same rank order), and the
    // pair the pass detected z-fights anyway (chitei2's stacked cave-wall
    // shells).
    type Edges = HashMap<usize, Vec<(usize, usize)>>; // draw -> (other draw, own plane idx)
    let mut family_reps: Vec<[f32; 3]> = Vec::new();
    let mut families: Vec<Edges> = Vec::new();
    let family_of = |n: [f32; 3], reps: &mut Vec<[f32; 3]>| -> usize {
        if let Some(fi) = reps.iter().position(|r| dot(*r, n) >= 0.999) {
            return fi;
        }
        reps.push(n);
        reps.len() - 1
    };
    let mut seen: std::collections::HashSet<(usize, usize)> = Default::default();
    // Every accepted conflict pair, kept for the post-lift repair pass.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    // Sorted bucket walk: greedy family clustering is discovery-order
    // sensitive, and HashMap iteration order would make the offsets differ
    // run to run (the determinism-replay contract forbids that).
    let mut bucket_keys: Vec<_> = buckets.keys().copied().collect();
    bucket_keys.sort();
    for key in bucket_keys {
        let members = &buckets[&key];
        for xi in 0..members.len() {
            for yi in (xi + 1)..members.len() {
                let (i, j) = (members[xi].min(members[yi]), members[xi].max(members[yi]));
                if i == j || seen.contains(&(i, j)) {
                    continue;
                }
                let (a, b) = (&world[i], &world[j]);
                if a.draw == b.draw {
                    continue; // intra-mesh handled by separate_coplanar_prims
                }
                if draws[a.draw] == draws[b.draw] {
                    // Exact-duplicate placements rasterise to bit-identical
                    // depths (stable overdraw, not shimmer), and no per-draw
                    // offset can separate two draws sharing one map key.
                    continue;
                }
                // Plane separation measured point-through-plane (never
                // `a.d - b.d`: rep-normal noise scales into spurious tens of
                // units of d at world coordinates).
                let sep = dot(a.n, sub(b.point, a.point)).abs();
                if dot(a.n, b.n) < 0.9995 || sep > 0.75 {
                    continue;
                }
                if !aabb_overlaps(a, b) {
                    continue;
                }
                seen.insert((i, j));
                pairs.push((i, j));
                let fi = family_of(a.n, &mut family_reps);
                if families.len() <= fi {
                    families.resize_with(fi + 1, Edges::default);
                }
                let edges = &mut families[fi];
                edges.entry(a.draw).or_default().push((b.draw, i));
                edges.entry(b.draw).or_default().push((a.draw, j));
            }
        }
    }
    if families.is_empty() {
        return HashMap::new();
    }
    // Per family: rank draws by greedy colouring in plane-area-descending
    // order - the biggest surface stays put, overlapping draws take the
    // smallest free rank among their already-ranked conflict partners - then
    // shift each ranked draw toward the visible side of its largest
    // conflicting plane in that family. The family list is in first-seen
    // order over the sorted-bucket walk, and per-draw contributions are
    // independent across families, so the summed offsets are deterministic.
    //
    // Accumulate per draw INDEX (never per `EnvDraw` key here): two draw
    // indices can carry equal `EnvDraw` values, and map-keyed accumulation
    // doubles their contribution into the one shared entry.
    let mut off_by_draw: Vec<[f32; 3]> = vec![[0.0; 3]; draws.len()];
    for edges in &families {
        if edges.is_empty() {
            continue;
        }
        let mut max_area: HashMap<usize, f32> = HashMap::new();
        for (&d, partners) in edges {
            let m = partners
                .iter()
                .map(|&(_, pi)| world[pi].area)
                .fold(0.0f32, f32::max);
            max_area.insert(d, m);
        }
        let mut order: Vec<usize> = edges.keys().copied().collect();
        order.sort_by(|&a, &b| {
            max_area[&b]
                .partial_cmp(&max_area[&a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });
        // Rank capacity must exceed the largest mutual-overlap clique or the
        // clamp hands the overflow draws one shared rank - taiku's plaza
        // stacks six-plus instances of one slab mesh on a single plane, and
        // a cap of 3 parked three of them on the same lift, recreating the
        // very coincidence the pass removes. 16 ranks x 0.75 units tops out
        // at ~11 world units, still far below visibility against 128-unit
        // tiles.
        let mut rank: HashMap<usize, u32> = HashMap::new();
        for &d in &order {
            let mut used = [false; 16];
            for &(other, _) in &edges[&d] {
                if let Some(&r) = rank.get(&other)
                    && (r as usize) < used.len()
                {
                    used[r as usize] = true;
                }
            }
            let r = used.iter().position(|&u| !u).unwrap_or(used.len() - 1) as u32;
            rank.insert(d, r.min(15));
        }
        for (&d, &r) in &rank {
            if r == 0 {
                continue;
            }
            let Some(&(_, pi)) = edges[&d].iter().max_by(|a, b| {
                world[a.1]
                    .area
                    .partial_cmp(&world[b.1].area)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) else {
                continue;
            };
            let vis = world[pi].vis;
            let shift = DRAW_NUDGE * r as f32;
            let e = &mut off_by_draw[d];
            e[0] += vis[0] * shift;
            e[1] += vis[1] * shift;
            e[2] += vis[2] * shift;
        }
    }
    // Repair pass: the per-family lifts compose by summing, and a draw pair
    // that conflicts in MANY families at once (two shells stacked at one
    // position touch on every family) can sum back into coincidence - each
    // family ranks independently, so one family lifts A past B while another
    // lifts B past A. Re-measure every detected pair under the summed
    // offsets and bump the smaller-plane draw of any still-coincident pair,
    // iterating until every pair separates (bounded; each round moves only
    // draws that still collide).
    const MIN_SEP: f32 = 0.25;
    for _ in 0..4 {
        let mut changed = false;
        for &(i, j) in &pairs {
            let (a, b) = (&world[i], &world[j]);
            let (oa, ob) = (off_by_draw[a.draw], off_by_draw[b.draw]);
            let rel = [
                (b.point[0] + ob[0]) - (a.point[0] + oa[0]),
                (b.point[1] + ob[1]) - (a.point[1] + oa[1]),
                (b.point[2] + ob[2]) - (a.point[2] + oa[2]),
            ];
            if dot(a.n, rel).abs() >= MIN_SEP {
                continue;
            }
            let (mover, vis) = if a.area <= b.area {
                (a.draw, a.vis)
            } else {
                (b.draw, b.vis)
            };
            let e = &mut off_by_draw[mover];
            e[0] += vis[0] * DRAW_NUDGE;
            e[1] += vis[1] * DRAW_NUDGE;
            e[2] += vis[2] * DRAW_NUDGE;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    let mut out: HashMap<EnvDraw, [f32; 3]> = HashMap::new();
    for (di, off) in off_by_draw.iter().enumerate() {
        if off == &[0.0; 3] {
            continue;
        }
        // First index wins for duplicate `EnvDraw` values (their geometry is
        // identical, so either offset is equally valid - but only one can
        // apply).
        out.entry(draws[di]).or_insert(*off);
    }
    out
}

/// Convenience: build [`MeshPlanes`] for every distinct `res_tmd` referenced
/// by `draws`, from the scene's parsed TMDs (unfiltered triangle walk - plane
/// geometry does not depend on VRAM state).
pub fn draw_plane_summaries(
    draws: &[EnvDraw],
    res: &crate::scene_resources::SceneResources,
) -> HashMap<usize, MeshPlanes> {
    let mut out: HashMap<usize, MeshPlanes> = HashMap::new();
    for d in draws {
        if out.contains_key(&d.res_tmd) {
            continue;
        }
        let Some(rt) = res.tmds.get(d.res_tmd) else {
            continue;
        };
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&rt.tmd, &rt.raw);
        let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rt.tmd, &rt.raw);
        let mut positions = mesh.positions;
        let mut indices = mesh.indices;
        let base = positions.len() as u32;
        positions.extend_from_slice(&cmesh.positions);
        indices.extend(cmesh.indices.iter().map(|i| i + base));
        out.insert(d.res_tmd, mesh_planes(&positions, &indices));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(res_tmd: usize, x: i32, y: i32, z: i32) -> EnvDraw {
        EnvDraw {
            env_slot: res_tmd,
            res_tmd,
            world_x: x,
            world_y: y,
            world_z: z,
            rot_y: 0,
            rot_x: 0,
            rot_z: 0,
            anim_id: 0,
            anchor: (0, 0),
        }
    }

    /// A flat floor quad (two tris) visible from above in the retail Y-down
    /// frame: winding gives cross(e1, e2) = +Y, visible side -Y.
    fn floor_quad(size: f32) -> (Vec<[f32; 3]>, Vec<u32>) {
        let p = vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, size],
            [size, 0.0, 0.0],
            [size, 0.0, size],
        ];
        (p, vec![0, 1, 2, 2, 1, 3])
    }

    #[test]
    fn overlapping_same_plane_draws_get_one_lift() {
        let (p, i) = floor_quad(128.0);
        let mut planes = HashMap::new();
        planes.insert(7usize, mesh_planes(&p, &i));
        // Two instances of the same tile mesh, half a tile apart: their
        // floor planes coincide (same y) and overlap by half.
        let draws = vec![draw(7, 0, 0, 0), draw(7, 64, 0, 0)];
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert_eq!(offs.len(), 1, "exactly one draw lifts: {offs:?}");
        let (_, off) = offs.iter().next().unwrap();
        // Lift toward -Y (up in the retail frame), one nudge.
        assert!(off[0].abs() < 1e-4 && off[2].abs() < 1e-4);
        assert!((off[1] + DRAW_NUDGE).abs() < 1e-4, "off={off:?}");
    }

    #[test]
    fn disjoint_draws_are_untouched() {
        let (p, i) = floor_quad(128.0);
        let mut planes = HashMap::new();
        planes.insert(7usize, mesh_planes(&p, &i));
        let draws = vec![draw(7, 0, 0, 0), draw(7, 512, 0, 0)];
        assert!(coplanar_draw_offsets(&draws, &planes).is_empty());
    }

    #[test]
    fn different_heights_are_untouched() {
        let (p, i) = floor_quad(128.0);
        let mut planes = HashMap::new();
        planes.insert(7usize, mesh_planes(&p, &i));
        // 64 units apart in Y: a real terrace step, not a coplanar tie.
        let draws = vec![draw(7, 0, 0, 0), draw(7, 64, 64, 0)];
        assert!(coplanar_draw_offsets(&draws, &planes).is_empty());
    }

    #[test]
    fn tile_chain_ranks_stay_bounded() {
        let (p, i) = floor_quad(128.0);
        let mut planes = HashMap::new();
        planes.insert(7usize, mesh_planes(&p, &i));
        // A long row of half-overlapping tiles: every draw's lift must stay
        // within one nudge of the plane (colouring, not accumulation).
        let draws: Vec<EnvDraw> = (0..10).map(|k| draw(7, k * 64, 0, 0)).collect();
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert!(!offs.is_empty());
        for off in offs.values() {
            assert!(
                off[1].abs() <= DRAW_NUDGE * 3.0 + 1e-4,
                "rank accumulated: {off:?}"
            );
        }
    }

    #[test]
    fn smaller_surface_lifts_off_larger_base() {
        let (pb, ib) = floor_quad(1024.0);
        let (ps, is_) = floor_quad(128.0);
        let mut planes = HashMap::new();
        planes.insert(1usize, mesh_planes(&pb, &ib));
        planes.insert(2usize, mesh_planes(&ps, &is_));
        // Small slab (emitted FIRST) resting on the big floor: the big floor
        // must stay put regardless of order; the slab lifts.
        let draws = vec![draw(2, 100, 0, 100), draw(1, 0, 0, 0)];
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert_eq!(offs.len(), 1);
        assert!(offs.contains_key(&draws[0]), "small slab lifts: {offs:?}");
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    /// The chitei2 shape: two DIFFERENT meshes at the SAME position, each
    /// carrying a z-facing wall on one plane (flat AABB in z). The pair must
    /// get a z-separating lift.
    #[test]
    fn same_position_wall_pair_gets_z_lift() {
        // Wall quad in the x/y extent seen in chitei2 res45/res39 (z const).
        let wall = |x0: f32, x1: f32, z: f32| -> (Vec<[f32; 3]>, Vec<u32>) {
            let p = vec![
                [x0, -576.0, z],
                [x1, -576.0, z],
                [x0, -520.0, z],
                [x1, -520.0, z],
            ];
            (p, vec![0, 1, 2, 2, 1, 3])
        };
        let (pa, ia) = wall(-128.0, 128.0, -224.0);
        let (pb, ib) = wall(-192.0, 192.0, -224.0);
        let mut planes = HashMap::new();
        planes.insert(45usize, mesh_planes(&pa, &ia));
        planes.insert(39usize, mesh_planes(&pb, &ib));
        let mk = |res: usize| EnvDraw {
            env_slot: res,
            res_tmd: res,
            world_x: 5312,
            world_y: 0,
            world_z: 15680,
            rot_y: 0,
            rot_x: 0,
            rot_z: 0,
            anim_id: 0,
            anchor: (0, 0),
        };
        let draws = vec![mk(45), mk(39)];
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert_eq!(offs.len(), 1, "one of the pair lifts: {offs:?}");
        let (_, off) = offs.iter().next().unwrap();
        assert!(off[2].abs() > 0.5, "lift must separate in z: {off:?}");
    }
}
