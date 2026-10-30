//! A solid mesh's footprint on the collision grid.
//!
//! A pack map's prop (or decoration) marked `solid` blocks the ground its
//! mesh stands on, not only the tile its origin names: a cottage four tiles
//! across closes every `64 x 64` wall sub-cell under its walls. The
//! footprint is the convex hull, in the floor plane, of the mesh's vertices
//! in the lower half of its height (after its rotation and scale), so a
//! roof's eaves overhanging the walls do not widen it and a tree is closed
//! round its trunk and lower boughs rather than its whole canopy. A sub-cell
//! is closed when its centre lies inside the hull.
//!
//! The grid convention is the synthetic image's: a sub-cell's
//! wall bit sits one byte row below its floor nibble (the wall probe's `+2`
//! Z bias, [`field-locomotion.md`](../../../../docs/subsystems/field-locomotion.md#wall-probes)).

use legaia_scene_formats::field_objects::{COLLISION_GRID_OFFSET, GRID_DIM};

use super::tmd_ops::rotation_deg;

/// The share of a mesh's height, from its lowest vertex up, whose vertices
/// make its footprint.
const BASE_BAND: f32 = 0.5;

/// A placed mesh's footprint: its base hull and vertical span.
#[derive(Debug, Clone, PartialEq)]
pub struct Footprint {
    /// The floor-plane convex hull `[x, z]` (counter-clockwise, world
    /// units, relative to the mesh's origin) of the lower vertices.
    pub hull: Vec<[f32; 2]>,
    /// The lowest and highest vertex, world units up from the origin.
    pub bottom: f32,
    pub top: f32,
}

impl Footprint {
    /// Does the mesh stand in the player's way: wider or deeper than
    /// [`super::OPEN_PROP_WARN_EXTENT`], taller than a step, and reaching
    /// down to body height above the floor when raised by `lift` (a prop's
    /// `offset[1]`)? A flat ring on the floor or a wheel on a wall is not.
    pub fn is_large_obstacle(&self, lift: f32) -> bool {
        hull_extent(&self.hull)
            .iter()
            .any(|&e| e > super::OPEN_PROP_WARN_EXTENT)
            && self.top - self.bottom > 48.0
            && lift + self.bottom < 96.0
    }
}

/// `raw`'s [`Footprint`] once turned by `rotation` (degrees, `Rx * Ry *
/// Rz`) and scaled by `scale`. `None` for a mesh that does not parse or has
/// fewer than three distinct points.
pub fn footprint(raw: &[u8], rotation: [f32; 3], scale: f32) -> Option<Footprint> {
    let tmd = legaia_tmd::parse(raw).ok()?;
    let m = rotation_deg(rotation);
    let pts: Vec<[f32; 3]> = tmd
        .objects
        .iter()
        .flat_map(|o| o.vertices.iter())
        .map(|v| {
            let p = [f32::from(v.x), f32::from(v.y), f32::from(v.z)].map(|c| c * scale);
            std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2])
        })
        .collect();
    // Retail frame: Y down, so the floor is the largest Y.
    let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p[1]), hi.max(p[1]))
    });
    if pts.is_empty() {
        return None;
    }
    let cut = hi - (hi - lo) * BASE_BAND;
    let mut flat: Vec<[f32; 2]> = pts
        .iter()
        .filter(|p| p[1] >= cut)
        .map(|p| [p[0], p[2]])
        .collect();
    let hull = convex_hull(&mut flat);
    (hull.len() >= 3).then_some(Footprint {
        hull,
        bottom: -hi,
        top: -lo,
    })
}

/// The hull of [`footprint`].
pub fn base_hull(raw: &[u8], rotation: [f32; 3], scale: f32) -> Option<Vec<[f32; 2]>> {
    footprint(raw, rotation, scale).map(|f| f.hull)
}

/// Andrew's monotone chain; counter-clockwise, no collinear points.
fn convex_hull(pts: &mut [[f32; 2]]) -> Vec<[f32; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut h: Vec<[f32; 2]> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let iter: Box<dyn Iterator<Item = &[f32; 2]>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
                h.pop();
            }
            h.push(p);
        }
        h.pop();
    }
    h
}

/// Is `p` inside the counter-clockwise convex polygon `hull`?
pub fn hull_contains(hull: &[[f32; 2]], p: [f32; 2]) -> bool {
    hull.len() >= 3
        && (0..hull.len()).all(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= 0.0
        })
}

/// The hull's extent `[width, depth]` in world units.
pub fn hull_extent(hull: &[[f32; 2]]) -> [f32; 2] {
    let ext = |i: usize| {
        let (lo, hi) = hull.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[i]), hi.max(p[i]))
        });
        (hi - lo).max(0.0)
    };
    [ext(0), ext(1)]
}

/// Close every wall sub-cell of the field image `img` whose centre lies
/// inside `hull` placed at world `(x, z)`. Returns how many sub-cells it
/// closed (already closed ones included).
pub fn close_hull(img: &mut [u8], hull: &[[f32; 2]], x: f32, z: f32) -> usize {
    let (mut x0, mut z0, mut x1, mut z1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in hull {
        x0 = x0.min(x + p[0]);
        z0 = z0.min(z + p[1]);
        x1 = x1.max(x + p[0]);
        z1 = z1.max(z + p[1]);
    }
    let sub = |v: f32| (v / 64.0).floor() as i32;
    let max_sub = (GRID_DIM * 2) as i32 - 1;
    let mut n = 0;
    for sr in sub(z0).max(0)..=sub(z1).min(max_sub - 2) {
        for sc in sub(x0).max(0)..=sub(x1).min(max_sub) {
            let centre = [sc as f32 * 64.0 + 32.0 - x, sr as f32 * 64.0 + 32.0 - z];
            if !hull_contains(hull, centre) {
                continue;
            }
            let (col, row) = ((sc / 2) as usize, (sr / 2) as usize + 1);
            let bit = ((sr & 1) << 1 | (sc & 1)) as u8;
            img[COLLISION_GRID_OFFSET + row * GRID_DIM + col] |= 1 << (4 + bit);
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_hull_closes_the_sub_cells_under_it() {
        let mut img = vec![0u8; COLLISION_GRID_OFFSET + GRID_DIM * GRID_DIM];
        // A 256 x 256 square centred on tile (10, 10)'s centre (1344, 1344).
        let hull = vec![
            [-128.0, -128.0],
            [128.0, -128.0],
            [128.0, 128.0],
            [-128.0, 128.0],
        ];
        assert_eq!(close_hull(&mut img, &hull, 1344.0, 1344.0), 16);
        // Tiles 9..=11 a side partly: the centre tile is fully closed, its
        // wall bits stored one row down.
        let at = |c: usize, r: usize| img[COLLISION_GRID_OFFSET + (r + 1) * GRID_DIM + c] >> 4;
        assert_eq!(at(10, 10), 0xF);
        // Tile (9, 10)'s high-X quarters (tr, br) only.
        assert_eq!(at(9, 10), 0b1010);
        assert_eq!(at(11, 11), 0b0001);
        assert_eq!(at(12, 10), 0);
    }

    #[test]
    fn hull_drops_interior_points() {
        let mut pts = vec![[0.0, 0.0], [2.0, 0.0], [1.0, 1.0], [2.0, 2.0], [0.0, 2.0]];
        let h = convex_hull(&mut pts);
        assert_eq!(h.len(), 4);
        assert!(hull_contains(&h, [1.0, 1.5]));
        assert!(!hull_contains(&h, [3.0, 1.0]));
    }
}
