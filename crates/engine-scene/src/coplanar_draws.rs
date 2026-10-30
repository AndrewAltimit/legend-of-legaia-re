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

/// World-units the **walk-ground heightfield** sinks below its authored
/// height at every render site (positive = down in the retail Y-down frame
/// the heightfield is stored in). The heightfield is the walkable base grid;
/// indoors and in towns the env pack lays authored floor art on the SAME
/// plane (koin6: all 972 ground tris at y=0, exactly the env floor slabs'
/// plane), and the two tessellations differ - so every such floor z-fights
/// as view-angle-dependent wedge streaks along the ground grid's cell
/// diagonals. Retail resolves the tie painter-style through the ordering
/// table; the port sinks the heightfield so the authored art wins
/// deterministically, and the heightfield still draws wherever nothing
/// covers it. `0.4` clears depth rounding at any sane distance while staying
/// off the occupied below-floor lattice (authored under-layers sit at 1+
/// units; the intra/cross nudges only ever move geometry UP).
///
/// Render-site-only by design: the heightfield struct itself stays at its
/// authored heights (the `.glb` exporter and the fishing shore-anchor height
/// queries must not shift).
pub const GROUND_SINK: f32 = 0.4;

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
    for t in indices.as_chunks::<3>().0 {
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
    // In key order, never the map's own: a plane's index is what the offset
    // pass ranks, pairs and repairs by, so a per-process iteration order made
    // a multi-family draw's summed lift differ run to run (`jouine`'s stacked
    // flesh shells moved a unit between two launches of one binary).
    let mut acc: Vec<_> = acc.into_iter().collect();
    acc.sort_unstable_by_key(|&(key, _)| key);
    MeshPlanes {
        planes: acc
            .into_iter()
            .map(|(_, a)| a)
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

fn instance_plane(p: &Plane, d: &EnvDraw) -> WorldPlane {
    // The draw's own rotation (`Rx * Ry * Rz`, `EnvDraw::rotate` - the
    // composition every host draws the placement with), so a tilted
    // placement's plane is the plane it is drawn on.
    let t = [d.world_x as f32, d.world_y as f32, d.world_z as f32];
    let mut n = d.rotate(p.n);
    let vis = d.rotate(p.vis);
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
                let w = d.rotate([cx, cy, cz]);
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
    let wp = d.rotate(p.point);
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

/// How far a manufactured in-plane slide can reach: the rank cap (16) times
/// [`DRAW_NUDGE`]. A coplanar pair whose boxes are further apart than this
/// can never be slid into each other by the lifts, so it is not worth
/// tracking as an abutment.
const SLIDE_REACH: f32 = DRAW_NUDGE * 16.0;

/// Do two coplanar planes' boxes come within [`SLIDE_REACH`] of overlapping,
/// measured in the plane only (the dominant-normal axis is what
/// `MIN_SEP` judges, not this)?
fn planes_within_slide_reach(a: &WorldPlane, b: &WorldPlane) -> bool {
    let skip = dominant_axis(a.n);
    (0..3)
        .filter(|&ax| ax != skip)
        .all(|ax| a.lo[ax] - SLIDE_REACH < b.hi[ax] && b.lo[ax] - SLIDE_REACH < a.hi[ax])
}

/// In-plane box overlap under per-draw offsets, with a hair-width margin.
///
/// [`aabb_overlaps`]'s 1.5-unit shrink exists to keep edge-adjacent tiles out
/// of the *input* conflict graph. A slide the lift pass manufactures itself is
/// one [`DRAW_NUDGE`] - smaller than that shrink - so the repair pass cannot
/// reuse that test to see the overlap it just created, and needs this one.
/// The dominant-normal axis is excluded: for a coplanar pair that axis is the
/// plane's own thickness, which the repair judges with `MIN_SEP` instead.
fn planes_overlap_shifted(a: &WorldPlane, oa: [f32; 3], b: &WorldPlane, ob: [f32; 3]) -> bool {
    const EPS: f32 = 0.05;
    let skip = dominant_axis(a.n);
    (0..3).filter(|&ax| ax != skip).all(|ax| {
        let (alo, ahi) = (a.lo[ax] + oa[ax], a.hi[ax] + oa[ax]);
        let (blo, bhi) = (b.lo[ax] + ob[ax], b.hi[ax] + ob[ax]);
        alo + EPS < bhi && blo + EPS < ahi
    })
}

/// The direction a draw is lifted along for a plane whose visible side is
/// `vis`: the visible side itself, except that a lift never points **down**
/// (`+Y` in the retail Y-down frame). A plane seen from below - the
/// underside of a fallen log, a prop's base - is separated toward its back
/// side instead. Lifting it down, toward its visible side, sank the whole
/// draw past [`GROUND_SINK`], so the walk-ground heightfield the sink exists
/// to keep underneath drew over its floor (`vell`'s mound, env slot 5, whose
/// end face showed the ground through it). Either side along the normal
/// separates the coplanar pair; only the up side keeps every lifted draw
/// above the sunk ground. The magnitude is unchanged, so the lift stays off
/// the decal lattice ([`DRAW_NUDGE`]).
fn lift_dir(vis: [f32; 3]) -> [f32; 3] {
    if vis[1] > 0.0 {
        [-vis[0], -vis[1], -vis[2]]
    } else {
        vis
    }
}

/// Detect cross-draw coplanar overlap clusters and return the world-space
/// offset each affected draw should add to its translation. Draws without a
/// conflict are absent from the map.
///
/// `planes_of` maps a draw's `res_tmd` to its [`MeshPlanes`] (see
/// [`mesh_planes`]); draws whose mesh has no entry are skipped.
///
/// Within a cluster the draw presenting the **largest** plane stays put and
/// each overlapping smaller draw lifts `DRAW_NUDGE` units per rank toward
/// the surface's visible side - the same "small decal wins" outcome retail's
/// mean-Z ordering-table bucketing produces. Between draws of equal area the
/// **earlier** one lifts: equal quads share a bucket, and a bucket draws its
/// packets last-linked first, so the earlier-emitted draw is the one on top
/// (hand a field scene's layers in through [`emission_order`]). Ranks are
/// assigned by greedy graph colouring, so chains of adjacent tiles alternate 0/1 instead
/// of accumulating.
pub fn coplanar_draw_offsets(
    draws: &[EnvDraw],
    planes_of: &HashMap<usize, MeshPlanes>,
) -> HashMap<EnvDraw, [f32; 3]> {
    offsets_walk(draws, planes_of, PairWalk::Grouped)
}

/// A bucket key - a quantized normal `k` (each component within `+-129`)
/// and a distance quantum `d` - packed into one word whose order is the
/// tuple's: 9 bits a normal component, offset by 256, and 34 for `d`,
/// offset by `2^33`.
fn bucket_key(k: (i32, i32, i32), d: i64) -> u64 {
    let c = |v: i32| (v + 256).clamp(0, 511) as u64;
    let d = (d + (1 << 33)).clamp(0, (1 << 34) - 1) as u64;
    (c(k.0) << 52) | (c(k.1) << 43) | (c(k.2) << 34) | d
}

/// The in-bucket pairs [`coplanar_draw_offsets`] walks, as positions into
/// `members`, in the nested-loop order `(xi, yi)`, `xi < yi`.
///
/// With `prune`, a bucket whose members share an axis that is in-plane for
/// every one of them (not the dominant normal axis of any) is swept along
/// that axis, and a pair whose boxes are further apart on it than
/// [`SLIDE_REACH`] is left out. Such a pair can do nothing in the walk: a
/// conflict needs the boxes to meet on every axis ([`aabb_overlaps`]) and an
/// abutment needs them within [`SLIDE_REACH`] on every in-plane axis
/// ([`planes_within_slide_reach`]), and the walk skips any other pair
/// without recording it. So the pairs kept, walked in the same order, give
/// the same offsets as every pair. A floor map's one ground plane is the
/// case it is for: thousands of tile draws in one bucket, nearly all of
/// them far apart (an expansion map of 120 x 120 tiles took seconds of the
/// scene build in the quadratic walk).
#[cfg(test)]
fn bucket_pairs(members: &[usize], world: &[WorldPlane], prune: bool) -> Vec<(usize, usize)> {
    let n = members.len();
    let sweep = prune
        .then(|| {
            let mut dominant = [false; 3];
            for &m in members {
                dominant[dominant_axis(world[m].n)] = true;
            }
            (0..3).find(|&ax| !dominant[ax])
        })
        .flatten();
    let Some(ax) = sweep else {
        return (0..n)
            .flat_map(|xi| ((xi + 1)..n).map(move |yi| (xi, yi)))
            .collect();
    };
    let mut ord: Vec<usize> = (0..n).collect();
    ord.sort_by(|&p, &q| {
        world[members[p]].lo[ax]
            .total_cmp(&world[members[q]].lo[ax])
            .then(p.cmp(&q))
    });
    let mut out = Vec::new();
    for (k, &p) in ord.iter().enumerate() {
        let hi = world[members[p]].hi[ax];
        for &q in &ord[k + 1..] {
            // `planes_within_slide_reach`'s own comparison, so the cut is
            // exact in floats; `lo - SLIDE_REACH` only grows along `ord`.
            let near = (world[members[q]].lo[ax] - SLIDE_REACH).partial_cmp(&hi);
            if near != Some(std::cmp::Ordering::Less) {
                break;
            }
            out.push((p.min(q), p.max(q)));
        }
    }
    out.sort_unstable();
    out
}

/// How [`offsets_walk`] finds the plane pairs it records. All three record
/// the same pairs in the same order, which the tests pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairWalk {
    /// The definition: every bucket, every pair in it ([`bucket_pairs`]
    /// unpruned). Quadratic in a bucket's size; the tests' reference.
    #[cfg(test)]
    Full,
    /// Every bucket, its pairs swept along a free axis ([`bucket_pairs`]).
    #[cfg(test)]
    Buckets,
    /// Each normal key once ([`grouped_pair_hits`]): the pass the scene
    /// build runs.
    Grouped,
}

/// One recorded plane pair: where the bucket walk meets it first (the
/// bucket's key, then the pair's planes - a bucket walks its pairs in plane
/// order), and whether it is an abutment rather than a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PairHit {
    key: u64,
    i: u32,
    j: u32,
    abutment: bool,
}

/// The walk's tests on one plane pair `i < j`, all of them pure: `None` to
/// skip it, `Some(false)` a conflict, `Some(true)` an abutment.
fn classify_pair(world: &[WorldPlane], draws: &[EnvDraw], i: usize, j: usize) -> Option<bool> {
    let (a, b) = (&world[i], &world[j]);
    if i == j || a.draw == b.draw {
        return None; // intra-mesh handled by separate_coplanar_prims
    }
    // Plane separation measured point-through-plane (never `a.d - b.d`:
    // rep-normal noise scales into spurious tens of units of d at world
    // coordinates).
    let sep = dot(a.n, sub(b.point, a.point)).abs();
    if dot(a.n, b.n) < 0.9995 || sep > 0.75 {
        return None;
    }
    if draws[a.draw] == draws[b.draw] {
        // Exact-duplicate placements rasterise to bit-identical depths
        // (stable overdraw, not shimmer), and no per-draw offset can
        // separate two draws sharing one map key.
        return None;
    }
    if !aabb_overlaps(a, b) {
        // Coplanar but not overlapping: an **abutment**, two strips of one
        // surface meeting edge to edge. They are not a conflict as authored
        // - but a lift this pass hands one of them is a slide *inside* their
        // shared plane, and a slide turns the abutment into a coplanar
        // overlap that was never in the art. Keep the pair so the repair can
        // see one it manufactured.
        // Only a pair that is disjoint **in the plane** is an abutment. A
        // pair that overlaps in-plane and failed the test on the normal axis
        // instead is simply already separated in depth - recording that as
        // an abutment sets the repair fighting itself, because the very lift
        // that separates it reads as a manufactured overlap to the in-plane
        // test (tunnela's res38 tunnel segments: the abutment arm pushed a
        // draw back onto the neighbour the conflict arm had just lifted it
        // off, every round).
        return (planes_within_slide_reach(a, b)
            && !planes_overlap_shifted(a, [0.0; 3], b, [0.0; 3]))
        .then_some(true);
    }
    Some(false)
}

/// The quantized normals a plane is bucketed under: its own rounded normal
/// and the 26 around it (each component within `+-129`), each with the
/// plane's distance quantum along that key's unit normal. A key whose
/// normal is too short to carry a direction is left out.
fn plane_keys(w: &WorldPlane, mut each: impl FnMut((i32, i32, i32), i64)) {
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
                each(k, (dq / 2.0).floor() as i64);
            }
        }
    }
}

/// The bucket walk itself: every plane joins the buckets `(k, d - 1)`,
/// `(k, d)` and `(k, d + 1)` of each of its keys ([`plane_keys`]), and the
/// buckets are walked in key order, each one's pairs in plane order. A pair
/// is recorded at the first bucket that holds both planes.
///
/// One `(key, plane)` entry per bucket a plane joins, sorted: a bucket is a
/// run of one key, its members in plane order - the walk a map of per-key
/// lists would give, without a list per key (a decorated map instances
/// ~10^5 planes into ~10^7 mostly single-member buckets).
#[cfg(test)]
fn bucket_pair_hits(world: &[WorldPlane], draws: &[EnvDraw], prune: bool) -> Vec<PairHit> {
    let mut buckets: Vec<(u64, u32)> = Vec::new();
    for (i, w) in world.iter().enumerate() {
        plane_keys(w, |k, d| {
            for dd in -1..=1i64 {
                buckets.push((bucket_key(k, d + dd), i as u32));
            }
        });
    }
    buckets.sort_unstable();
    let mut seen: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut hits = Vec::new();
    let mut members: Vec<usize> = Vec::new();
    let mut start = 0;
    while start < buckets.len() {
        let key = buckets[start].0;
        let end = start + buckets[start..].partition_point(|e| e.0 == key);
        members.clear();
        members.extend(buckets[start..end].iter().map(|e| e.1 as usize));
        start = end;
        if members.len() < 2 {
            continue;
        }
        for (xi, yi) in bucket_pairs(&members, world, prune) {
            let (i, j) = (members[xi].min(members[yi]), members[xi].max(members[yi]));
            let Some(abutment) = classify_pair(world, draws, i, j) else {
                continue;
            };
            if seen.insert((i, j)) {
                hits.push(PairHit {
                    key,
                    i: i as u32,
                    j: j as u32,
                    abutment,
                });
            }
        }
    }
    hits
}

/// `bucket_pair_hits` without the buckets: each normal key's planes are
/// gathered once, and a pair is met once per key rather than once per
/// bucket of it.
///
/// Two planes share a bucket of key `k` exactly when their distance quanta
/// along `k` are at most 2 apart, and the first such bucket is
/// `(k, max(d) - 1)`; the keys are walked in order, so a pair's first key with a shared
/// bucket gives the bucket the walk records it at. Within one key the pairs
/// are found by a sweep along an axis no member's normal is dominant on, and
/// cut on the second such axis when there is one: a pair that records
/// anything has boxes within [`SLIDE_REACH`] on every axis but the lower
/// plane's dominant one (see `bucket_pairs`). Sorting the hits by bucket
/// and planes then gives the bucket walk's order. A floor map's one ground
/// plane is the case it is for: the bucket walk sorted ~8 million bucket
/// entries for a 120 x 120 map, and met its ground's tiles in every bucket of
/// the straddle.
fn grouped_pair_hits(world: &[WorldPlane], draws: &[EnvDraw]) -> Vec<PairHit> {
    // `(key, plane)` per normal key a plane is bucketed under, the key's
    // distance quantum in the low bits: sorted, a run of one normal key is
    // that key's planes by distance, then plane.
    let mut entries: Vec<(u64, u32)> = Vec::with_capacity(world.len() * 27);
    for (i, w) in world.iter().enumerate() {
        plane_keys(w, |k, d| entries.push((bucket_key(k, d), i as u32)));
    }
    entries.sort_unstable();
    const D_MASK: u64 = (1 << 34) - 1;
    let d_of = |key: u64| (key & D_MASK) as i64;
    let mut seen: std::collections::HashSet<(u32, u32)> = Default::default();
    let mut hits = Vec::new();
    // One window's members: (lo, hi on the sweep axis, plane, distance).
    let mut win: Vec<(f32, f32, u32, i64)> = Vec::new();
    let mut start = 0;
    while start < entries.len() {
        let normal = entries[start].0 & !D_MASK;
        let mut end = start + 1;
        while end < entries.len() && entries[end].0 & !D_MASK == normal {
            end += 1;
        }
        let group = &entries[start..end];
        start = end;
        if group.len() < 2 {
            continue;
        }
        let mut dominant = [false; 3];
        for &(_, p) in group {
            dominant[dominant_axis(world[p as usize].n)] = true;
        }
        let mut free = (0..3).filter(|&ax| !dominant[ax]);
        let (sweep, cut) = (free.next(), free.next());
        // Runs of one distance quantum.
        let mut runs: Vec<(usize, usize)> = Vec::new();
        let mut r0 = 0;
        for k in 1..=group.len() {
            if k == group.len() || d_of(group[k].0) != d_of(group[r0].0) {
                runs.push((r0, k));
                r0 = k;
            }
        }
        for (ri, &(s, _)) in runs.iter().enumerate() {
            let d = d_of(group[s].0);
            // The runs a pair with a plane of this run as its lower quantum
            // can reach: distance d, d + 1, d + 2.
            let last = runs[ri..]
                .iter()
                .take_while(|&&(rs, _)| d_of(group[rs].0) <= d + 2)
                .last()
                .map_or(s, |&(_, re)| re);
            win.clear();
            for &(key, p) in &group[s..last] {
                let w = &world[p as usize];
                let (lo, hi) = match sweep {
                    Some(ax) => (w.lo[ax], w.hi[ax]),
                    None => (0.0, 0.0),
                };
                win.push((lo, hi, p, d_of(key)));
            }
            if sweep.is_some() {
                win.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.2.cmp(&y.2)));
            }
            for (x, &(_, hi, p, dp)) in win.iter().enumerate() {
                for &(qlo, _, q, dq) in &win[x + 1..] {
                    // `planes_within_slide_reach`'s own comparison, so the
                    // cut is exact in floats; `lo - SLIDE_REACH` only grows
                    // along the window.
                    if sweep.is_some()
                        && (qlo - SLIDE_REACH).partial_cmp(&hi) != Some(std::cmp::Ordering::Less)
                    {
                        break;
                    }
                    if dp.min(dq) != d {
                        continue; // met in the window of the lower quantum
                    }
                    let (i, j) = (p.min(q), p.max(q));
                    if let Some(ax) = cut {
                        let (a, b) = (&world[i as usize], &world[j as usize]);
                        if !(a.lo[ax] - SLIDE_REACH < b.hi[ax] && b.lo[ax] - SLIDE_REACH < a.hi[ax])
                        {
                            continue;
                        }
                    }
                    if seen.contains(&(i, j)) {
                        continue;
                    }
                    let Some(abutment) = classify_pair(world, draws, i as usize, j as usize) else {
                        continue;
                    };
                    seen.insert((i, j));
                    hits.push(PairHit {
                        key: normal | (dp.max(dq) - 1).clamp(0, D_MASK as i64) as u64,
                        i,
                        j,
                        abutment,
                    });
                }
            }
        }
    }
    hits.sort_unstable();
    hits
}

/// [`coplanar_draw_offsets`], its plane pairs found by `walk`; every walk
/// gives the same offsets, which the tests pin.
fn offsets_walk(
    draws: &[EnvDraw],
    planes_of: &HashMap<usize, MeshPlanes>,
    walk: PairWalk,
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
    // distance quanta ([`plane_keys`]). Two matching sloped clusters differ
    // in their rep normals by float noise, so a single-key normal hash
    // separates them; and the distance scalar must be computed against the
    // KEY's normal (identical for everything in the bucket), because each
    // cluster's own `d` amplifies that normal noise by the world coordinate
    // magnitude (`|dn| * |v|` = tens of units at map scale). The walk over
    // the buckets is sorted: greedy family clustering is discovery-order
    // sensitive, and HashMap iteration order would make the offsets differ
    // run to run (the determinism-replay contract forbids that).
    let hits = match walk {
        #[cfg(test)]
        PairWalk::Full => bucket_pair_hits(&world, draws, false),
        #[cfg(test)]
        PairWalk::Buckets => bucket_pair_hits(&world, draws, true),
        PairWalk::Grouped => grouped_pair_hits(&world, draws),
    };
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
    // Every accepted conflict pair, kept for the post-lift repair pass.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    // Coplanar pairs that do NOT overlap as authored but sit close enough
    // that a lift could slide them into each other (`classify_pair`).
    let mut abutments: Vec<(usize, usize)> = Vec::new();
    for h in &hits {
        let (i, j) = (h.i as usize, h.j as usize);
        if h.abutment {
            abutments.push((i, j));
            continue;
        }
        let (a, b) = (&world[i], &world[j]);
        pairs.push((i, j));
        let fi = family_of(a.n, &mut family_reps);
        if families.len() <= fi {
            families.resize_with(fi + 1, Edges::default);
        }
        let edges = &mut families[fi];
        edges.entry(a.draw).or_default().push((b.draw, i));
        edges.entry(b.draw).or_default().push((a.draw, j));
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
        // Equal areas: the **later** draw is the base and the earlier one
        // lifts. Two draws laying the same quads on one plane - a sign over
        // the wall panel behind it - put their packets in the same ordering
        // table bucket (equal mean Z), and a bucket is a LIFO list: the
        // packet linked last is drawn first, so the draw the field pass
        // emitted first paints over the one it emitted after. So `draws`
        // must arrive in retail's emission order, which across the two
        // static layers is placed objects first ([`emission_order`]):
        // `koin1`'s prize-counter banner, a placed sign on its terrain
        // cell's brick panel, is the case that ranked the wrong way while
        // the hosts concatenated terrain first.
        order.sort_by(|&a, &b| {
            max_area[&b]
                .partial_cmp(&max_area[&a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.cmp(&a))
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
            let vis = lift_dir(world[pi].vis);
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
    //
    // The same loop also closes the pass's own manufactured coincidences.
    // Each family lifts along its own normal, and a lift along one normal
    // lies *inside* every plane perpendicular to it - so a draw lifted for
    // its floor tie and again for a Z-facing wall slides its X-facing wall
    // strip sideways, and an abutting neighbour's strip it merely touched
    // becomes a `DRAW_NUDGE`-wide coplanar overlap. That is a z-fight the art
    // does not contain and the detector never saw, because at input the pair
    // did not overlap (koin4's res12/res34 wall sliver: a `[0, -0.75, -0.75]`
    // sum with no component along the shared plane's `(1, 0, 0)` normal). The
    // abutment pairs are therefore repaired too, but only once the offsets
    // have actually pushed them into overlap.
    const MIN_SEP: f32 = 0.25;
    // An abutment pair gets **one** lift for the whole loop. Without that cap
    // an abutment and a conflict pair that share a mover and a normal family
    // pull it in opposite directions once per round, forever: the conflict arm
    // separates the pair, the abutment arm puts it back, and four rounds later
    // the pair is coincident again (tunnela's res38 tunnel segments). Capped,
    // the abutment fires once and the conflict arm - which runs first every
    // round - has the last word.
    let mut abutment_fired: Vec<bool> = vec![false; abutments.len()];
    for _ in 0..4 {
        let mut changed = false;
        for (idx, (&(i, j), manufactured)) in pairs
            .iter()
            .map(|p| (p, false))
            .chain(abutments.iter().map(|p| (p, true)))
            .enumerate()
        {
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
            // An abutment is only owed a lift once the accumulated offsets
            // have slid the two strips into each other; an untouched one
            // still just touches, and lifting it would be the pass inventing
            // a step in a flat surface.
            if manufactured && !planes_overlap_shifted(a, oa, b, ob) {
                continue;
            }
            if manufactured {
                let slot = idx - pairs.len();
                if abutment_fired[slot] {
                    continue;
                }
                abutment_fired[slot] = true;
            }
            let (mover, vis) = if a.area <= b.area {
                (a.draw, lift_dir(a.vis))
            } else {
                (b.draw, lift_dir(b.vis))
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

/// A field scene's two static layers in the order retail **emits** them, the
/// order [`coplanar_draw_offsets`] ranks equal-area ties by: the placed
/// objects first, then the terrain / decoration cells.
///
/// The field frame body `FUN_80016444` runs the actor render passes
/// (`FUN_8001D140` over the actor lists, `0x800166A4..0x80016738`) before
/// the decoration pass (`FUN_8001D058` at `0x80016784`, which reaches
/// `FUN_801F7088` / `FUN_801F69D8`), so every placed object's packets are
/// built ahead of every cell's. Two such packets that land in one
/// ordering-table bucket draw last-linked first, so on a shared plane the
/// placed object paints over the cell. `koin1`'s prize-counter banner is
/// that case: a placed additive sign (CLUT `(80, 504)`) on the same plane as
/// the terrain cell's brick panel, whose packets retail's display list holds
/// at lower addresses than the panel's and draws after them. Ranked
/// terrain-first, the panel lifted over the banner and hid it.
// REF: FUN_80016444, FUN_8001D140, FUN_8001D058
pub fn emission_order(terrain: &[EnvDraw], placed: &[EnvDraw]) -> Vec<EnvDraw> {
    let mut out = Vec::with_capacity(placed.len() + terrain.len());
    out.extend_from_slice(placed);
    out.extend_from_slice(terrain);
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
            floor: Default::default(),
            cell: (0, 0),
            cull_radius: 0,
            view_skip: 0,
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

    /// The pruned pair walk gives exactly the offsets of the full one, over
    /// a field that has every shape the walk acts on: a floor grid of
    /// overlapping and abutting tiles at two heights, slabs stacked on it,
    /// walls facing X and Z, and a row of tilted ramps.
    #[test]
    fn pruned_walk_matches_the_full_walk() {
        let mut planes_of = HashMap::new();
        let (p, i) = floor_quad(140.0);
        planes_of.insert(0, mesh_planes(&p, &i));
        let (p, i) = floor_quad(60.0);
        planes_of.insert(1, mesh_planes(&p, &i));
        // A wall facing X, one facing Z.
        let wall_x = vec![
            [0.0, 0.0, 0.0],
            [0.0, -100.0, 0.0],
            [0.0, 0.0, 120.0],
            [0.0, -100.0, 120.0],
        ];
        planes_of.insert(2, mesh_planes(&wall_x, &[0, 1, 2, 2, 1, 3]));
        let wall_z = vec![
            [0.0, 0.0, 0.0],
            [120.0, 0.0, 0.0],
            [0.0, -100.0, 0.0],
            [120.0, -100.0, 0.0],
        ];
        planes_of.insert(3, mesh_planes(&wall_z, &[0, 1, 2, 2, 1, 3]));
        // A ramp rising along X.
        let ramp = vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 128.0],
            [128.0, -64.0, 0.0],
            [128.0, -64.0, 128.0],
        ];
        planes_of.insert(4, mesh_planes(&ramp, &[0, 1, 2, 2, 1, 3]));
        let mut draws = Vec::new();
        for r in 0..24 {
            for c in 0..24 {
                // 140-wide tiles every 128 overlap their neighbours; every
                // third row sits 64 lower.
                let y = if r % 3 == 0 { -64 } else { 0 };
                draws.push(draw(0, c * 128, y, r * 128));
                if (r + c) % 5 == 0 {
                    draws.push(draw(1, c * 128 + 30, y, r * 128 + 30));
                }
                if (r * 7 + c) % 11 == 0 {
                    draws.push(draw(2, c * 128, 0, r * 128));
                    draws.push(draw(2, c * 128 + 1, 0, r * 128 + 60));
                }
                if (r + 3 * c) % 13 == 0 {
                    draws.push(draw(3, c * 128, 0, r * 128));
                    draws.push(draw(3, c * 128 + 128, 0, r * 128));
                }
                if r == 7 {
                    draws.push(draw(4, c * 128, 0, r * 128));
                    draws.push(draw(4, c * 128 + 64, -32, r * 128));
                }
            }
        }
        let full = offsets_walk(&draws, &planes_of, PairWalk::Full);
        let pruned = offsets_walk(&draws, &planes_of, PairWalk::Buckets);
        let grouped = offsets_walk(&draws, &planes_of, PairWalk::Grouped);
        assert!(!full.is_empty(), "the field has conflicts to resolve");
        assert_eq!(pruned, full);
        assert_eq!(grouped, full);
    }

    /// The grouped walk records what the bucket walk records, in its order,
    /// over tilted placements: every draw turned on all three axes by a
    /// pseudo-random angle, many near the same plane (shared normal keys,
    /// distance quanta one and two apart), so pairs meet first under tilted
    /// keys and in the windows of the lower quantum.
    #[test]
    fn grouped_walk_matches_the_bucket_walk_under_rotation() {
        let mut planes_of = HashMap::new();
        let (p, i) = floor_quad(96.0);
        planes_of.insert(0, mesh_planes(&p, &i));
        let (p, i) = floor_quad(40.0);
        planes_of.insert(1, mesh_planes(&p, &i));
        let box_ = vec![
            [0.0, 0.0, 0.0],
            [50.0, 0.0, 0.0],
            [0.0, -50.0, 0.0],
            [50.0, -50.0, 0.0],
            [0.0, 0.0, 50.0],
            [50.0, 0.0, 50.0],
            [0.0, -50.0, 50.0],
            [50.0, -50.0, 50.0],
        ];
        let faces = [
            0, 1, 2, 2, 1, 3, 4, 6, 5, 5, 6, 7, 0, 4, 1, 1, 4, 5, 2, 3, 6, 6, 3, 7, 0, 2, 4, 4, 2,
            6, 1, 5, 3, 3, 5, 7,
        ];
        planes_of.insert(2, mesh_planes(&box_, &faces));
        let mut seed = 0x2545_f491_u32;
        let mut rnd = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        let mut draws = Vec::new();
        for k in 0..900 {
            let mut d = draw(
                (k % 3) as usize,
                (rnd(40) * 37) as i32,
                rnd(4) as i32 - 2,
                (rnd(40) * 37) as i32,
            );
            // Mostly a few shared angles (coplanar families), some free.
            let angle = |r: u32| {
                if r < 6 {
                    (r * 64) as u16
                } else {
                    (r * 97 % 4096) as u16
                }
            };
            d.rot_y = angle(rnd(12));
            d.rot_x = if rnd(3) == 0 { angle(rnd(12)) } else { 0 };
            d.rot_z = if rnd(4) == 0 { angle(rnd(12)) } else { 0 };
            draws.push(d);
        }
        let world: Vec<WorldPlane> = draws
            .iter()
            .enumerate()
            .flat_map(|(di, d)| {
                planes_of[&d.res_tmd].planes.iter().map(move |p| {
                    let mut w = instance_plane(p, d);
                    w.draw = di;
                    w
                })
            })
            .collect();
        let full = bucket_pair_hits(&world, &draws, false);
        assert!(full.len() > 50, "the field records pairs: {}", full.len());
        assert!(full.iter().any(|h| h.abutment) && full.iter().any(|h| !h.abutment));
        assert_eq!(bucket_pair_hits(&world, &draws, true), full);
        assert_eq!(grouped_pair_hits(&world, &draws), full);
        assert_eq!(
            offsets_walk(&draws, &planes_of, PairWalk::Grouped),
            offsets_walk(&draws, &planes_of, PairWalk::Full)
        );
    }

    /// Disc-gated (an `extracted/` tree): on every field scene the grouped
    /// walk records the bucket walk's pairs, in its order.
    #[test]
    fn grouped_walk_matches_the_bucket_walk_on_every_field_scene() {
        use crate::scene::{ProtIndex, Scene};
        use crate::scene_resources::{BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind};
        if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        }
        let Some(root) = ["extracted", "../extracted", "../../extracted"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
        else {
            eprintln!("[skip] no extracted/ tree");
            return;
        };
        let index = ProtIndex::open_extracted(&root).expect("index");
        let cdname = legaia_prot::cdname::parse(&root.join("CDNAME.TXT")).expect("cdname");
        let mut names: Vec<String> = cdname.values().cloned().collect();
        names.sort();
        names.dedup();
        let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
            .iter()
            .filter_map(|n| Scene::load(&index, n).ok())
            .collect();
        let shared: Vec<&Scene> = shared.iter().collect();
        let (mut scenes, mut recorded) = (0, 0);
        for name in &names {
            if crate::scene::is_world_map_scene(name) {
                continue;
            }
            let Ok(scene) = Scene::load(&index, name) else {
                continue;
            };
            let Ok((res, _)) = crate::scene_resources::SceneResources::build_targeted_with_options(
                &scene,
                &shared,
                BuildOptions {
                    kind: SceneLoadKind::Field,
                    upload_all_tims: true,
                    system_ui: None,
                },
            ) else {
                continue;
            };
            let env_tmds = crate::field_env::env_pack_tmd_indices(&scene, &res);
            let lut = scene.field_floor_height_lut(&index).ok().flatten();
            let placed = scene
                .field_object_placements(&index)
                .ok()
                .flatten()
                .unwrap_or_default();
            let tiles: Vec<_> = scene
                .field_terrain_tiles(&index)
                .ok()
                .flatten()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.flags & legaia_scene_formats::field_objects::FLAG_PLACED == 0)
                .collect();
            let (placements, _) = crate::field_env::resolve_env_draws(&env_tmds, &placed, lut);
            let (terrain, _) = crate::field_env::resolve_env_draws(&env_tmds, &tiles, lut);
            let draws = emission_order(&terrain, &placements);
            let planes_of = draw_plane_summaries(&draws, &res);
            let world: Vec<WorldPlane> = draws
                .iter()
                .enumerate()
                .flat_map(|(di, d)| {
                    planes_of.get(&d.res_tmd).into_iter().flat_map(move |mp| {
                        mp.planes.iter().map(move |p| {
                            let mut w = instance_plane(p, d);
                            w.draw = di;
                            w
                        })
                    })
                })
                .collect();
            let reference = bucket_pair_hits(&world, &draws, true);
            assert_eq!(grouped_pair_hits(&world, &draws), reference, "{name}");
            scenes += 1;
            recorded += reference.len();
        }
        eprintln!("{scenes} field scenes, {recorded} pairs recorded alike");
        assert!(scenes > 50, "only {scenes} scenes built");
    }

    /// A mesh's planes come out in one order however the accumulator hashes:
    /// the offset pass ranks and repairs by plane index.
    #[test]
    fn mesh_planes_order_is_stable_across_builds() {
        let mut p = Vec::new();
        let mut i = Vec::new();
        for k in 0..48u32 {
            let y = k as f32 * 4.0;
            let base = p.len() as u32;
            p.extend([[0.0, y, 0.0], [0.0, y, 32.0], [32.0, y, 0.0]]);
            i.extend([base, base + 1, base + 2]);
        }
        let first: Vec<[f32; 3]> = mesh_planes(&p, &i).planes.iter().map(|q| q.point).collect();
        assert_eq!(first.len(), 48);
        for _ in 0..8 {
            let again: Vec<[f32; 3]> = mesh_planes(&p, &i).planes.iter().map(|q| q.point).collect();
            assert_eq!(first, again);
        }
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

    /// An underside plane (visible from below, `+Y` in the retail frame)
    /// still separates, but upward: a downward lift would sink the draw
    /// past the ground heightfield's `GROUND_SINK` (vell's log floor).
    #[test]
    fn underside_planes_lift_up_never_down() {
        let (p, mut i) = floor_quad(128.0);
        for tri in i.chunks_mut(3) {
            tri.swap(1, 2);
        }
        let mut planes = HashMap::new();
        planes.insert(7usize, mesh_planes(&p, &i));
        let draws = vec![draw(7, 0, 0, 0), draw(7, 64, 0, 0)];
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert_eq!(offs.len(), 1, "exactly one draw lifts: {offs:?}");
        let (_, off) = offs.iter().next().unwrap();
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

    /// Two different meshes laying the same quad on one plane at one
    /// position (koin1's banner over its brick panel): the draw emitted
    /// first is the one retail paints last, so it is the one that lifts.
    #[test]
    fn of_two_equal_planes_the_earlier_draw_lifts() {
        let (p, i) = floor_quad(128.0);
        let mut planes = HashMap::new();
        planes.insert(64usize, mesh_planes(&p, &i));
        planes.insert(63usize, mesh_planes(&p, &i));
        let draws = vec![draw(64, 0, 0, 0), draw(63, 0, 0, 0)];
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert_eq!(offs.len(), 1, "exactly one draw lifts: {offs:?}");
        let off = offs.get(&draws[0]).expect("the earlier draw lifts");
        assert!((off[1] + DRAW_NUDGE).abs() < 1e-4, "off={off:?}");
        // Whichever mesh comes first.
        let swapped = vec![draw(63, 0, 0, 0), draw(64, 0, 0, 0)];
        let offs = coplanar_draw_offsets(&swapped, &planes);
        assert!(offs.contains_key(&swapped[0]), "{offs:?}");
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
            floor: Default::default(),
            cell: (0, 0),
            cull_radius: 0,
            view_skip: 0,
        };
        let draws = vec![mk(45), mk(39)];
        let offs = coplanar_draw_offsets(&draws, &planes);
        assert_eq!(offs.len(), 1, "one of the pair lifts: {offs:?}");
        let (_, off) = offs.iter().next().unwrap();
        assert!(off[2].abs() > 0.5, "lift must separate in z: {off:?}");
    }
}
