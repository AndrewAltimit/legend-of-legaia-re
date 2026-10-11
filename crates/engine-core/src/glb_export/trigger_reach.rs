//! Trigger reach analysis: a standing capsule tested against the scene's own baked triangles.

use super::*;

// --- Trigger reach analysis: capsule vs the scene's own baked mesh. ---
//
// Retail dispatches doorway teleports on a height-blind 2D check for a
// POINT player, and retail's collision is the walkability grid - but a
// glTF consumer collides a fat capsule against the rendered mesh. Rim Elm
// alone has a door whose retail-walkable approach channel is narrower
// than a capsule (Vahn's), one on an unclimbable porch, and one whose
// contact samples a floor layer the approach never walks. So after the
// world glb is baked, each trigger box is checked against the exact
// triangles that ship: a standing-capsule grid is built over the
// retail-standable cells around the contact, flood-filled from an outer
// ring (standability prunes the inside of exterior house shells, which
// pure mesh analysis mistakes for open rooms), and a box no reachable
// cell can touch is extended toward the nearest spot a player can
// actually occupy.

/// Player-capsule model for trigger reach analysis (export-frame meters
/// at the default 1/64 scale; all scale-relative).
const REACH_RADIUS: f32 = 0.30;
const REACH_GRID_STEP: f32 = 0.15;
const REACH_HALF_GRID: f32 = 4.5;
const REACH_SEED_RING: f32 = 3.5;
const REACH_STEP_UP: f32 = 0.35;
const REACH_TORSO_LO: f32 = 0.55;
const REACH_TORSO_HI: f32 = 1.40;

/// World-space triangle soup parsed back out of the scene's own baked glb
/// bytes - the exact geometry a Unity / glTF consumer builds colliders
/// from, so the analysis and the shipped world cannot drift apart.
pub struct GlbTriangles {
    tris: Vec<[[f32; 3]; 3]>,
}

impl GlbTriangles {
    pub fn from_glb(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
            return None;
        }
        let jlen = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
        let json: Value = serde_json::from_slice(bytes.get(20..20 + jlen)?).ok()?;
        let boff = 20 + jlen;
        let blen = u32::from_le_bytes(bytes.get(boff..boff + 4)?.try_into().ok()?) as usize;
        let bin = bytes.get(boff + 8..boff + 8 + blen)?;

        let accessor = |ai: usize| -> Option<(&Value, usize)> {
            let a = json["accessors"].get(ai)?;
            let bv = json["bufferViews"].get(a["bufferView"].as_u64()? as usize)?;
            let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize
                + a["byteOffset"].as_u64().unwrap_or(0) as usize;
            Some((a, off))
        };
        let read_vec3 = |ai: usize| -> Option<Vec<[f32; 3]>> {
            let (a, off) = accessor(ai)?;
            let count = a["count"].as_u64()? as usize;
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let p = off + i * 12;
                let b = bin.get(p..p + 12)?;
                out.push([
                    f32::from_le_bytes(b[0..4].try_into().ok()?),
                    f32::from_le_bytes(b[4..8].try_into().ok()?),
                    f32::from_le_bytes(b[8..12].try_into().ok()?),
                ]);
            }
            Some(out)
        };
        let read_indices = |ai: usize| -> Option<Vec<u32>> {
            let (a, off) = accessor(ai)?;
            let count = a["count"].as_u64()? as usize;
            let ct = a["componentType"].as_u64()?;
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                out.push(match ct {
                    5123 => u32::from(u16::from_le_bytes(
                        bin.get(off + i * 2..off + i * 2 + 2)?.try_into().ok()?,
                    )),
                    5125 => {
                        u32::from_le_bytes(bin.get(off + i * 4..off + i * 4 + 4)?.try_into().ok()?)
                    }
                    _ => return None,
                });
            }
            Some(out)
        };

        let mut tris = Vec::new();
        for node in json["nodes"].as_array()?.iter() {
            let Some(mi) = node["mesh"].as_u64() else {
                continue;
            };
            let g3 = |v: &Value, d: f32| -> [f32; 3] {
                match v.as_array() {
                    Some(a) => [
                        a[0].as_f64().unwrap_or(d as f64) as f32,
                        a[1].as_f64().unwrap_or(d as f64) as f32,
                        a[2].as_f64().unwrap_or(d as f64) as f32,
                    ],
                    None => [d, d, d],
                }
            };
            let t = g3(&node["translation"], 0.0);
            let sc = g3(&node["scale"], 1.0);
            // rotation quaternion -> matrix
            let m = match node["rotation"].as_array() {
                Some(q) => {
                    let (x, y, z, w) = (
                        q[0].as_f64().unwrap_or(0.0) as f32,
                        q[1].as_f64().unwrap_or(0.0) as f32,
                        q[2].as_f64().unwrap_or(0.0) as f32,
                        q[3].as_f64().unwrap_or(1.0) as f32,
                    );
                    [
                        [
                            1.0 - 2.0 * (y * y + z * z),
                            2.0 * (x * y - z * w),
                            2.0 * (x * z + y * w),
                        ],
                        [
                            2.0 * (x * y + z * w),
                            1.0 - 2.0 * (x * x + z * z),
                            2.0 * (y * z - x * w),
                        ],
                        [
                            2.0 * (x * z - y * w),
                            2.0 * (y * z + x * w),
                            1.0 - 2.0 * (x * x + y * y),
                        ],
                    ]
                }
                None => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            };
            let mesh = json["meshes"].get(mi as usize)?;
            for prim in mesh["primitives"].as_array()?.iter() {
                let Some(pi) = prim["attributes"]["POSITION"].as_u64() else {
                    continue;
                };
                let Some(ii) = prim["indices"].as_u64() else {
                    continue;
                };
                let (Some(pos), Some(idx)) = (read_vec3(pi as usize), read_indices(ii as usize))
                else {
                    continue;
                };
                let world: Vec<[f32; 3]> = pos
                    .iter()
                    .map(|p| {
                        let (px, py, pz) = (p[0] * sc[0], p[1] * sc[1], p[2] * sc[2]);
                        [
                            m[0][0] * px + m[0][1] * py + m[0][2] * pz + t[0],
                            m[1][0] * px + m[1][1] * py + m[1][2] * pz + t[1],
                            m[2][0] * px + m[2][1] * py + m[2][2] * pz + t[2],
                        ]
                    })
                    .collect();
                for tri in idx.chunks(3) {
                    if tri.len() == 3 {
                        tris.push([
                            world[tri[0] as usize],
                            world[tri[1] as usize],
                            world[tri[2] as usize],
                        ]);
                    }
                }
            }
        }
        Some(Self { tris })
    }
}

fn closest_pt_on_tri(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let sub = |u: [f32; 3], v: [f32; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
    let dot = |u: [f32; 3], v: [f32; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v];
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [
            b[0] + (c[0] - b[0]) * w,
            b[1] + (c[1] - b[1]) * w,
            b[2] + (c[2] - b[2]) * w,
        ];
    }
    let den = 1.0 / (va + vb + vc);
    let (v, w) = (vb * den, vc * den);
    [
        a[0] + ab[0] * v + ac[0] * w,
        a[1] + ab[1] * v + ac[1] * w,
        a[2] + ab[2] * v + ac[2] * w,
    ]
}

/// Extend one trigger box (bottom-center `pos` + `half`) so a player
/// capsule standing on the shipped mesh can touch it. The analysis is
/// confined to the door's own floor level (`door_y` +- a step-and-a-half):
/// without the band, rooftops and overpasses read as standable ground and
/// a flood across them "reaches" the box from a level the door does not
/// serve. Unchanged when a reachable standing cell already meets the box
/// (the common case), when the glb is absent, or when nothing on the
/// door's level is reachable (the cross-layer cases the vertical envelope
/// already covers).
pub(super) fn reach_extend_box(
    soup: &GlbTriangles,
    floor: &FloorSampler,
    scale: f32,
    door_y: f32,
    pos: &mut [f32; 3],
    half: &mut [f32; 3],
) {
    const LEVEL_BAND: f32 = 1.75;
    let (cx, cz) = (pos[0], pos[2]);
    let ceil_y = door_y + 2.0;
    // Local triangles, bucketed on a coarse XZ hash for the fit test.
    let lim = REACH_HALF_GRID + 1.0;
    let local: Vec<&[[f32; 3]; 3]> = soup
        .tris
        .iter()
        .filter(|t| {
            t.iter().any(|v| {
                (v[0] - cx).abs() < lim + 2.0
                    && (v[2] - cz).abs() < lim + 2.0
                    && v[1] < ceil_y + 4.0
                    && v[1] > pos[1] - 8.0
            })
        })
        .collect();
    const BUCKET: f32 = 0.75;
    let nb = (2.0 * (REACH_HALF_GRID + 1.0) / BUCKET).ceil() as i32 + 1;
    let bucket_of = |x: f32, z: f32| -> Option<usize> {
        let bi = ((x - (cx - lim)) / BUCKET) as i32;
        let bj = ((z - (cz - lim)) / BUCKET) as i32;
        if bi < 0 || bj < 0 || bi >= nb || bj >= nb {
            return None;
        }
        Some((bj * nb + bi) as usize)
    };
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); (nb * nb) as usize];
    for (ti, t) in local.iter().enumerate() {
        let (mut x0, mut x1) = (f32::MAX, f32::MIN);
        let (mut z0, mut z1) = (f32::MAX, f32::MIN);
        for v in t.iter() {
            x0 = x0.min(v[0]);
            x1 = x1.max(v[0]);
            z0 = z0.min(v[2]);
            z1 = z1.max(v[2]);
        }
        let mut x = x0 - BUCKET;
        while x <= x1 + BUCKET {
            let mut z = z0 - BUCKET;
            while z <= z1 + BUCKET {
                if let Some(b) = bucket_of(x, z)
                    && buckets[b].last() != Some(&ti)
                {
                    buckets[b].push(ti);
                }
                z += BUCKET;
            }
            x += BUCKET;
        }
    }

    let n = (2.0 * REACH_HALF_GRID / REACH_GRID_STEP) as i32 + 1;
    let cell_xz = |i: i32, j: i32| -> (f32, f32) {
        (
            cx - REACH_HALF_GRID + i as f32 * REACH_GRID_STEP,
            cz - REACH_HALF_GRID + j as f32 * REACH_GRID_STEP,
        )
    };
    // Ground per cell: the highest surface at/below the ceiling.
    let ground_at = |x: f32, z: f32| -> Option<f32> {
        let b = bucket_of(x, z)?;
        let mut best: Option<f32> = None;
        for &ti in &buckets[b] {
            let t = local[ti];
            let (x1, z1, x2, z2, x3, z3) = (t[0][0], t[0][2], t[1][0], t[1][2], t[2][0], t[2][2]);
            let den = (z2 - z3) * (x1 - x3) + (x3 - x2) * (z1 - z3);
            if den.abs() < 1e-12 {
                continue;
            }
            let l1 = ((z2 - z3) * (x - x3) + (x3 - x2) * (z - z3)) / den;
            let l2 = ((z3 - z1) * (x - x3) + (x1 - x3) * (z - z3)) / den;
            let l3 = 1.0 - l1 - l2;
            if l1 < -0.001 || l2 < -0.001 || l3 < -0.001 {
                continue;
            }
            let y = l1 * t[0][1] + l2 * t[1][1] + l3 * t[2][1];
            if y <= ceil_y && best.is_none_or(|b| y > b) {
                best = Some(y);
            }
        }
        best
    };
    let capsule_fits = |x: f32, z: f32, g: f32| -> bool {
        let Some(b) = bucket_of(x, z) else {
            return false;
        };
        for &ti in &buckets[b] {
            let t = local[ti];
            for k in 0..4 {
                let y = g + REACH_TORSO_LO + (REACH_TORSO_HI - REACH_TORSO_LO) * (k as f32) / 3.0;
                let q = closest_pt_on_tri([x, y, z], t[0], t[1], t[2]);
                let d2 =
                    (q[0] - x) * (q[0] - x) + (q[1] - y) * (q[1] - y) + (q[2] - z) * (q[2] - z);
                if d2 < REACH_RADIUS * REACH_RADIUS {
                    return false;
                }
            }
        }
        true
    };

    // Standing domain: retail-standable + ground on the door's level +
    // capsule fits.
    let mut ground = vec![None::<f32>; (n * n) as usize];
    let mut open = vec![false; (n * n) as usize];
    for j in 0..n {
        for i in 0..n {
            let (x, z) = cell_xz(i, j);
            let (px, pz) = (x / scale, z / scale);
            if px < 0.0 || pz < 0.0 || px > i16::MAX as f32 || pz > i16::MAX as f32 {
                continue;
            }
            if !floor.standable(px as i16, pz as i16) {
                continue;
            }
            let Some(g) = ground_at(x, z) else { continue };
            if (g - door_y).abs() > LEVEL_BAND {
                continue;
            }
            ground[(j * n + i) as usize] = Some(g);
            open[(j * n + i) as usize] = capsule_fits(x, z, g);
        }
    }
    // Flood from the outer ring inward.
    let mut reach = vec![false; (n * n) as usize];
    let mut queue: Vec<(i32, i32)> = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (x, z) = cell_xz(i, j);
            let d = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
            if d >= REACH_SEED_RING && open[(j * n + i) as usize] {
                reach[(j * n + i) as usize] = true;
                queue.push((i, j));
            }
        }
    }
    while let Some((i, j)) = queue.pop() {
        let g0 = ground[(j * n + i) as usize].unwrap_or(0.0);
        for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (ni, nj) = (i + di, j + dj);
            if ni < 0 || nj < 0 || ni >= n || nj >= n {
                continue;
            }
            let k = (nj * n + ni) as usize;
            if reach[k] || !open[k] {
                continue;
            }
            let Some(g1) = ground[k] else { continue };
            if (g1 - g0).abs() > REACH_STEP_UP {
                continue;
            }
            reach[k] = true;
            queue.push((ni, nj));
        }
    }
    // Does any reachable capsule already touch the box?
    let (bx0, bx1) = (pos[0] - half[0], pos[0] + half[0]);
    let (bz0, bz1) = (pos[2] - half[2], pos[2] + half[2]);
    let mut nearest: Option<(f32, f32, f32, f32)> = None; // (dist, x, z, ground)
    for j in 0..n {
        for i in 0..n {
            let k = (j * n + i) as usize;
            if !reach[k] {
                continue;
            }
            let (x, z) = cell_xz(i, j);
            if x >= bx0 - REACH_RADIUS
                && x <= bx1 + REACH_RADIUS
                && z >= bz0 - REACH_RADIUS
                && z <= bz1 + REACH_RADIUS
            {
                // Vertical check too: the capsule must overlap the span.
                let g = ground[k].unwrap_or(0.0);
                let (top, bottom) = (pos[1] + 2.0 * half[1], pos[1]);
                if g + REACH_TORSO_HI > bottom && g + 0.1 < top {
                    return; // reachable as-is - keep the tight box
                }
            }
            let d = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
            if nearest.is_none_or(|(bd, ..)| d < bd) {
                nearest = Some((d, x, z, ground[k].unwrap_or(0.0)));
            }
        }
    }
    let Some((_, rx, rz, rg)) = nearest else {
        return; // nothing reachable anywhere - leave the retail box alone
    };
    // Extend the AABB to cover the nearest reachable standing spot.
    let pad = REACH_RADIUS + 0.15;
    let nx0 = bx0.min(rx - pad);
    let nx1 = bx1.max(rx + pad);
    let nz0 = bz0.min(rz - pad);
    let nz1 = bz1.max(rz + pad);
    let ny0 = pos[1].min(rg - 0.5);
    let ny1 = (pos[1] + 2.0 * half[1]).max(rg + 2.2);
    pos[0] = (nx0 + nx1) / 2.0;
    pos[2] = (nz0 + nz1) / 2.0;
    pos[1] = ny0;
    half[0] = (nx1 - nx0) / 2.0;
    half[2] = (nz1 - nz0) / 2.0;
    half[1] = (ny1 - ny0) / 2.0;
}
