//! The bank's **ground coverage field**: which XZ cells of a field scene have
//! a floor under them, at what height, and how far each cell lies from the
//! edge where the floor ends.
//!
//! The walk grid alone cannot answer this. Its floor tiers cover the whole
//! `0x80 x 0x80` tile map, so a tower top ringed by open sky (`teien`, the
//! Sky Gardens) reads as one continuous floor out to the map's edge and the
//! bank used to lie flat across the empty air. The field is rasterised from
//! the geometry the hosts actually draw instead: every up-facing triangle of
//! the assembled static scene and of the walk-ground heightfield, sampled at
//! each cell centre ([`FogGroundBuilder`]). It is built once per scene load,
//! on the CPU, and both hosts upload the same bytes ([`FogGround::texels`]).
//!
//! Three things come out of it:
//!
//! - **Height**: the sheet mesh lies on the drawn floor where there is one,
//!   so the bank follows terraces and steps the walk grid does not author.
//! - **Coverage**: a signed distance to the floor's edge ([`FogGround::dist`],
//!   positive on the floor), which the shaders turn into a soft fade so the
//!   bank thins out over a rim instead of hovering over nothing.
//! - **Spill**: just past a rim the sheet mesh keeps going but **falls**,
//!   almost straight down the wall face ([`spill_drop`], [`SPILL_REACH`]),
//!   and the shaders break those fragments into slow tendrils pouring off
//!   the edge, thinning and parting as they fall.
//! - **Haze**: further out the mesh lies as a faint, thick, flat layer far
//!   below the rim ([`HAZE_DEPTH`], [`haze_weight`]) - the misty void a
//!   tower top looks out over, so the fall does not end on nothing. It lies
//!   only where nothing is drawn above it, never on a floor.
//!
//! Nothing here is retail; see the module above.

/// World units per field cell (half a 128-unit walk tile).
pub const GROUND_CELL: f32 = 64.0;
/// Largest side of the field, in cells. A scene wider than
/// `GROUND_MAX_DIM * GROUND_CELL` takes coarser cells.
pub const GROUND_MAX_DIM: usize = 256;
/// Uncovered cells added on every side of the drawn geometry's extent, so
/// the texture's clamped border always reads as "no floor".
const BORDER_CELLS: usize = 4;
/// Steepest triangle that still counts as floor: `|n.y| / |n|` at or above
/// this (about 50 degrees from level).
pub const FLOOR_MIN_UP: f32 = 0.64;
/// Steepest see-through surface that still counts as floor
/// ([`FogGroundBuilder::add_see_through`]): about 14 degrees from level.
pub const LEVEL_MIN_UP: f32 = 0.97;
/// A surface more than this far **above** the walk floor at its cell is a
/// roof, canopy or overhang, not floor (retail Y-down, so "above" is a
/// smaller Y).
pub const ROOF_CLEARANCE: f32 = 96.0;
/// How far past a rim the spill reaches, world units. Short against
/// [`SPILL_DEPTH`]: the curtain falls nearly straight down the wall face,
/// with only a slight outward bow over the lip.
pub const SPILL_REACH: f32 = 256.0;
/// How far the spill has fallen at [`SPILL_REACH`], world units.
pub const SPILL_DEPTH: f32 = 1400.0;
/// How far below its rim the distant haze layer lies, world units.
pub const HAZE_DEPTH: f32 = 1900.0;
/// Distance past the rim (world units) where the haze is at its thickest.
pub const HAZE_PEAK: f32 = 1000.0;
/// Distance past the rim (world units) beyond which there is no haze.
pub const HAZE_REACH: f32 = 3200.0;
/// The haze's share of the bank at its thickest: a faint layer, not a
/// second bank.
pub const HAZE_WEIGHT: f32 = 0.15;
/// The spill's share of the bank at the lip: the curtain pouring off a rim
/// carries a third of the bank on the floor behind it, so the edge reads as
/// a wisp going over rather than a second bank hung down the wall.
pub const SPILL_SHARE: f32 = 1.0 / 3.0;
/// How far above a surface below the rim the spill sheet stays when it
/// drapes over it, world units.
pub const SPILL_CLEARANCE: f32 = 24.0;
/// Over this distance either side of an edge the coverage fades (world
/// units): the soft rim.
pub const EDGE_SOFT: f32 = 96.0;

/// How far below its rim the spill sheet lies at `out` world units past the
/// edge: a fall that starts level (the bank first slides over the lip) and
/// steepens - `SPILL_DEPTH * (out / SPILL_REACH)^1.5`. With the reach short
/// against the depth, past the first sheet-mesh quad it is a near-vertical
/// curtain.
pub fn spill_drop(out: f32) -> f32 {
    let t = (out / SPILL_REACH).clamp(0.0, 1.0);
    SPILL_DEPTH * t * t.sqrt()
}

/// The spill's share of the bank at `out` world units past the rim: full at
/// the lip, holding through the first of the fall and thinning to nothing at
/// [`SPILL_REACH`] (`1 - t^1.5`).
pub fn spill_weight(out: f32) -> f32 {
    let t = (out / SPILL_REACH).clamp(0.0, 1.0);
    1.0 - t * t.sqrt()
}

/// The distant haze's share of the bank at `out` world units past the rim
/// (`0..=HAZE_WEIGHT`): nothing within the spill's reach, rising to its
/// thickest at [`HAZE_PEAK`] and fading out by [`HAZE_REACH`].
pub fn haze_weight(out: f32) -> f32 {
    let rise = smooth01((out - SPILL_REACH) / (HAZE_PEAK - SPILL_REACH));
    let fall = smooth01((out - HAZE_PEAK) / (HAZE_REACH - HAZE_PEAK));
    HAZE_WEIGHT * rise * (1.0 - fall)
}

/// Collects floor candidates per cell from triangles, then resolves them into
/// a [`FogGround`].
#[derive(Debug, Clone)]
pub struct FogGroundBuilder {
    origin: [f32; 2],
    cell: f32,
    w: usize,
    h: usize,
    /// Every up-facing surface height seen at each cell centre.
    cands: Vec<Vec<f32>>,
    /// The highest point of any drawn surface over each cell - walls and
    /// slopes included - so the spill drapes over a tower's flank instead of
    /// falling through it.
    top: Vec<f32>,
}

impl FogGroundBuilder {
    /// A builder over the XZ rectangle `lo..hi` (world units), padded by a
    /// border of empty cells.
    pub fn new(lo: [f32; 2], hi: [f32; 2]) -> Self {
        let span = [(hi[0] - lo[0]).max(0.0), (hi[1] - lo[1]).max(0.0)];
        let inner = (GROUND_MAX_DIM - 2 * BORDER_CELLS) as f32;
        let cell = GROUND_CELL.max(span[0] / inner).max(span[1] / inner);
        let w = (span[0] / cell).ceil() as usize + 2 * BORDER_CELLS;
        let h = (span[1] / cell).ceil() as usize + 2 * BORDER_CELLS;
        let origin = [
            lo[0] - BORDER_CELLS as f32 * cell,
            lo[1] - BORDER_CELLS as f32 * cell,
        ];
        Self {
            origin,
            cell,
            w,
            h,
            cands: vec![Vec::new(); w * h],
            top: vec![f32::INFINITY; w * h],
        }
    }

    /// A builder sized to the XZ extent of `tris` (retail Y-down world
    /// triangles).
    pub fn for_triangles<'a>(tris: impl IntoIterator<Item = &'a [[f32; 3]; 3]>) -> Self {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for t in tris {
            for v in t {
                lo[0] = lo[0].min(v[0]);
                lo[1] = lo[1].min(v[2]);
                hi[0] = hi[0].max(v[0]);
                hi[1] = hi[1].max(v[2]);
            }
        }
        if !lo[0].is_finite() {
            lo = [0.0; 2];
            hi = [0.0; 2];
        }
        Self::new(lo, hi)
    }

    /// Rasterise one triangle (retail Y-down world space): every cell centre
    /// inside its XZ footprint gets the triangle's height there as a floor
    /// candidate, if the triangle faces up or down steeply enough to be floor
    /// (either winding - the disc meshes and the heightfield wind opposite).
    pub fn add_triangle(&mut self, t: &[[f32; 3]; 3]) {
        self.add_surface(t, FLOOR_MIN_UP, true);
    }

    /// A see-through surface (a blended pond or a mist veil): a floor
    /// candidate only when it lies level ([`LEVEL_MIN_UP`] - a water sheet,
    /// not a veil hung across a corridor), and never something the spill
    /// drapes over. The bank lies on what reads as ground on screen, and a
    /// translucent sheet the eye looks straight through is not a wall.
    pub fn add_see_through(&mut self, t: &[[f32; 3]; 3]) {
        self.add_surface(t, LEVEL_MIN_UP, false);
    }

    fn add_surface(&mut self, t: &[[f32; 3]; 3], min_up: f32, solid: bool) {
        let [a, b, c] = *t;
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len <= 1.0e-6 {
            return;
        }
        if solid {
            self.mark_top(t);
        }
        if n[1].abs() < min_up * len {
            return;
        }
        // XZ barycentrics.
        let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if det.abs() <= 1.0e-6 {
            return;
        }
        let minx = a[0].min(b[0]).min(c[0]);
        let maxx = a[0].max(b[0]).max(c[0]);
        let minz = a[2].min(b[2]).min(c[2]);
        let maxz = a[2].max(b[2]).max(c[2]);
        let cx0 = (((minx - self.origin[0]) / self.cell) - 0.5)
            .ceil()
            .max(0.0) as usize;
        let cz0 = (((minz - self.origin[1]) / self.cell) - 0.5)
            .ceil()
            .max(0.0) as usize;
        let cx1 = (((maxx - self.origin[0]) / self.cell) - 0.5).floor();
        let cz1 = (((maxz - self.origin[1]) / self.cell) - 0.5).floor();
        if cx1 < 0.0 || cz1 < 0.0 {
            return;
        }
        let cx1 = (cx1 as usize).min(self.w - 1);
        let cz1 = (cz1 as usize).min(self.h - 1);
        // A hair of slack so cells on a shared edge land in one of the two
        // triangles either side of it.
        let eps = -1.0e-4;
        for cz in cz0..=cz1 {
            let pz = self.origin[1] + (cz as f32 + 0.5) * self.cell;
            for cx in cx0..=cx1 {
                let px = self.origin[0] + (cx as f32 + 0.5) * self.cell;
                let l1 = ((b[2] - c[2]) * (px - c[0]) + (c[0] - b[0]) * (pz - c[2])) / det;
                let l2 = ((c[2] - a[2]) * (px - c[0]) + (a[0] - c[0]) * (pz - c[2])) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 < eps || l2 < eps || l3 < eps {
                    continue;
                }
                let y = l1 * a[1] + l2 * b[1] + l3 * c[1];
                self.cands[cz * self.w + cx].push(y);
            }
        }
    }

    /// Sample the whole triangle (any orientation) at under half a cell's
    /// spacing and raise each touched cell's [`Self::top`].
    fn mark_top(&mut self, t: &[[f32; 3]; 3]) {
        let [a, b, c] = *t;
        let edge = |p: [f32; 3], q: [f32; 3]| {
            ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
        };
        let longest = edge(a, b).max(edge(b, c)).max(edge(c, a));
        let steps = ((longest / (self.cell * 0.45)).ceil() as usize).clamp(1, 512);
        for i in 0..=steps {
            for j in 0..=(steps - i) {
                let u = i as f32 / steps as f32;
                let v = j as f32 / steps as f32;
                let w = 1.0 - u - v;
                let p = [
                    a[0] * w + b[0] * u + c[0] * v,
                    a[1] * w + b[1] * u + c[1] * v,
                    a[2] * w + b[2] * u + c[2] * v,
                ];
                let cx = ((p[0] - self.origin[0]) / self.cell).floor();
                let cz = ((p[2] - self.origin[1]) / self.cell).floor();
                if cx < 0.0 || cz < 0.0 || cx >= self.w as f32 || cz >= self.h as f32 {
                    continue;
                }
                let k = cz as usize * self.w + cx as usize;
                if p[1] < self.top[k] {
                    self.top[k] = p[1];
                }
            }
        }
    }

    /// World X / Z of cell `(cx, cz)`'s centre.
    pub fn cell_centre(&self, cx: usize, cz: usize) -> [f32; 2] {
        [
            self.origin[0] + (cx as f32 + 0.5) * self.cell,
            self.origin[1] + (cz as f32 + 0.5) * self.cell,
        ]
    }

    /// Pick each cell's floor and derive the edge distances.
    ///
    /// `walk_floor` is the walk grid's floor height at a world X / Z (retail
    /// Y-down), `None` where the scene has no walk grid. A candidate more
    /// than [`ROOF_CLEARANCE`] above it is a roof, hedge top or canopy and is
    /// not floor; of the rest the **highest** wins (the surface the bank
    /// would settle on - a floor tile drawn over the heightfield, not the
    /// ground under it). With no walk grid the highest candidate wins
    /// outright. A cell whose only surfaces are roofs is still **solid**
    /// ([`FogGround::from_cells`]): something stands there, so it is not the
    /// open air a rim spills into.
    pub fn finish(self, walk_floor: impl Fn(f32, f32) -> Option<f32>) -> FogGround {
        let (w, h) = (self.w, self.h);
        let mut height = vec![f32::NAN; w * h];
        let mut solid = vec![false; w * h];
        for cz in 0..h {
            for cx in 0..w {
                let cands = &self.cands[cz * w + cx];
                if cands.is_empty() {
                    continue;
                }
                solid[cz * w + cx] = true;
                let [px, pz] = self.cell_centre(cx, cz);
                let floor = walk_floor(px, pz);
                let mut best = f32::NAN;
                for &y in cands {
                    if floor.is_some_and(|f| y < f - ROOF_CLEARANCE) {
                        continue;
                    }
                    if best.is_nan() || y < best {
                        best = y;
                    }
                }
                height[cz * w + cx] = best;
            }
        }
        let mut g = FogGround::from_cells(self.origin, self.cell, w, h, height, solid);
        g.top = self.top;
        g
    }
}

/// One scene's resolved ground field - see the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct FogGround {
    /// World X / Z of cell `(0, 0)`'s corner.
    pub origin: [f32; 2],
    /// World units per cell.
    pub cell: f32,
    pub w: usize,
    pub h: usize,
    /// Floor height per cell (retail Y-down), `NaN` where no floor.
    pub height: Vec<f32>,
    /// Signed distance from each cell centre to the floor's edge, world
    /// units: positive on the floor (to the nearest floorless cell), negative
    /// off it (to the nearest floor cell).
    pub dist: Vec<f32>,
    /// The nearest floor cell's index - for a floor cell itself, for an open
    /// one the rim the spill falls from. `u32::MAX` when the field has no
    /// floor at all.
    pub rim: Vec<u32>,
    /// The highest point of any drawn surface over each cell (retail Y-down,
    /// `INFINITY` where nothing is drawn) - what the spill drapes over.
    pub top: Vec<f32>,
}

impl FogGround {
    /// [`Self::from_cells`] with every cell that has a height solid.
    pub fn from_heights(origin: [f32; 2], cell: f32, w: usize, h: usize, height: Vec<f32>) -> Self {
        let solid = height.iter().map(|y| !y.is_nan()).collect();
        Self::from_cells(origin, cell, w, h, height, solid)
    }

    /// Resolve per-cell floor heights (`NaN` = no floor surface) and
    /// solidity (any drawn surface at all) into the field.
    ///
    /// **Open air** is the empty space that reaches the field's border: the
    /// solid cells are first closed by one cell (so a seam, or a wall one
    /// cell thick between two meshes, is not a crack to the sky), then the
    /// remaining empty cells are flood-filled from the border. Everything
    /// else is **covered** - floor, and the enclosed pockets a floor
    /// surrounds (a hedge maze's tops, a roof, a pit), which take the height
    /// of the nearest floor cell, so the bank runs on through them as it
    /// always did and the scene's own depth hides it there.
    pub fn from_cells(
        origin: [f32; 2],
        cell: f32,
        w: usize,
        h: usize,
        mut height: Vec<f32>,
        solid: Vec<bool>,
    ) -> Self {
        assert_eq!(height.len(), w * h, "height grid size");
        assert_eq!(solid.len(), w * h, "solid grid size");
        let closed = erode(w, h, &dilate(w, h, &solid));
        let solid: Vec<bool> = solid.iter().zip(&closed).map(|(a, b)| *a || *b).collect();
        let open = flood_from_border(w, h, |i| !solid[i]);
        // Enclosed pockets take the nearest floor's height.
        let to_surface = nearest_site(w, h, |i| !height[i].is_nan());
        for i in 0..w * h {
            if !open[i] && height[i].is_nan() {
                height[i] = to_surface[i].map_or(f32::NAN, |(sx, sz)| height[sz * w + sx]);
            }
        }
        let covered: Vec<bool> = (0..w * h)
            .map(|i| !open[i] && !height[i].is_nan())
            .collect();
        for (y, c) in height.iter_mut().zip(&covered) {
            if !c {
                *y = f32::NAN;
            }
        }
        // Nearest floor cell to every cell, and nearest open cell to every
        // floor cell.
        let to_floor = nearest_site(w, h, |i| covered[i]);
        let to_open = nearest_site(w, h, |i| !covered[i]);
        let mut dist = vec![0.0f32; w * h];
        let mut rim = vec![u32::MAX; w * h];
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                if covered[i] {
                    rim[i] = i as u32;
                    // Edge sits half a cell short of the floorless centre.
                    dist[i] = match to_open[i] {
                        Some((sx, sz)) => (site_dist(x, z, sx, sz) - 0.5) * cell,
                        None => f32::INFINITY,
                    };
                } else {
                    match to_floor[i] {
                        Some((sx, sz)) => {
                            dist[i] = -(site_dist(x, z, sx, sz) - 0.5) * cell;
                            rim[i] = (sz * w + sx) as u32;
                        }
                        None => dist[i] = f32::NEG_INFINITY,
                    }
                }
            }
        }
        smooth_dist(w, h, cell, &mut dist);
        Self {
            origin,
            cell,
            w,
            h,
            height,
            dist,
            rim,
            top: vec![f32::INFINITY; w * h],
        }
    }

    /// The index of the cell under world `(x, z)`, `None` outside the field.
    pub fn cell_index(&self, x: f32, z: f32) -> Option<usize> {
        let cx = ((x - self.origin[0]) / self.cell).floor();
        let cz = ((z - self.origin[1]) / self.cell).floor();
        if cx < 0.0 || cz < 0.0 || cx >= self.w as f32 || cz >= self.h as f32 {
            return None;
        }
        Some(cz as usize * self.w + cx as usize)
    }

    /// Whether the cell under world `(x, z)` has floor.
    pub fn covered_at(&self, x: f32, z: f32) -> bool {
        self.cell_index(x, z)
            .is_some_and(|i| !self.height[i].is_nan())
    }

    /// Signed edge distance at world `(x, z)`, bilinear over cell centres
    /// (outside the field: as far off the floor as the border).
    pub fn dist_at(&self, x: f32, z: f32) -> f32 {
        let gx = (x - self.origin[0]) / self.cell - 0.5;
        let gz = (z - self.origin[1]) / self.cell - 0.5;
        let x0 = gx.floor();
        let z0 = gz.floor();
        let (fx, fz) = (gx - x0, gz - z0);
        let cap = (self.w.max(self.h) as f32) * self.cell;
        let at = |x: i32, z: i32| -> f32 {
            let x = x.clamp(0, self.w as i32 - 1) as usize;
            let z = z.clamp(0, self.h as i32 - 1) as usize;
            self.dist[z * self.w + x].clamp(-cap, cap)
        };
        let (x0, z0) = (x0 as i32, z0 as i32);
        let a = at(x0, z0) + (at(x0 + 1, z0) - at(x0, z0)) * fx;
        let b = at(x0, z0 + 1) + (at(x0 + 1, z0 + 1) - at(x0, z0 + 1)) * fx;
        a + (b - a) * fz
    }

    /// World X / Z of cell `i`'s centre.
    pub fn centre_of(&self, i: usize) -> [f32; 2] {
        [
            self.origin[0] + ((i % self.w) as f32 + 0.5) * self.cell,
            self.origin[1] + ((i / self.w) as f32 + 0.5) * self.cell,
        ]
    }

    /// The rim cell under world `(x, z)`: for a point on the floor, its own
    /// cell; off it, the nearest floor cell. `None` outside the field or
    /// when the field has no floor.
    pub fn rim_of(&self, x: f32, z: f32) -> Option<usize> {
        let i = self.cell_index(x, z)?;
        let r = self.rim[i];
        (r != u32::MAX).then_some(r as usize)
    }

    /// Where the sheet mesh lies at world `(x, z)`, how much bank it may
    /// carry there, and whether it is spill:
    ///
    /// - on the floor (any of the four cells around the point covered), the
    ///   mean height of the covered ones at full weight;
    /// - within [`SPILL_REACH`] past a rim, the rim's height lowered by
    ///   [`spill_drop`] at [`spill_weight`] times [`SPILL_SHARE`];
    /// - further out, the distant haze: [`HAZE_DEPTH`] below the rim at
    ///   [`haze_weight`], weight `0` where something drawn stands above
    ///   that plane (the haze never lies on a lower roof or hillside);
    /// - with no floor at all, `None` (no bank). Past the field's edge,
    ///   where nothing is drawn, the haze continues off the nearest border
    ///   cell's rim.
    pub fn sheet_at(&self, x: f32, z: f32) -> Option<FogSheetPoint> {
        let gx = ((x - self.origin[0]) / self.cell - 0.5).floor() as i32;
        let gz = ((z - self.origin[1]) / self.cell - 0.5).floor() as i32;
        let (mut sum, mut n) = (0.0f32, 0u32);
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (cx, cz) = (gx + dx, gz + dz);
            if cx < 0 || cz < 0 || cx >= self.w as i32 || cz >= self.h as i32 {
                continue;
            }
            let y = self.height[cz as usize * self.w + cx as usize];
            if !y.is_nan() {
                sum += y;
                n += 1;
            }
        }
        if n > 0 {
            return Some(FogSheetPoint {
                y: sum / n as f32,
                weight: 1.0,
                spill: false,
                haze: false,
            });
        }
        let (r, out) = match self.cell_index(x, z) {
            Some(_) => (self.rim_of(x, z)?, -self.dist_at(x, z)),
            // Beyond the field nothing is drawn: the nearest border cell's
            // rim, that far further out (the haze reaches past the field).
            None => {
                let cx = ((x - self.origin[0]) / self.cell).floor();
                let cz = ((z - self.origin[1]) / self.cell).floor();
                let cx = cx.clamp(0.0, self.w as f32 - 1.0) as usize;
                let cz = cz.clamp(0.0, self.h as f32 - 1.0) as usize;
                let i = cz * self.w + cx;
                let r = self.rim[i];
                if r == u32::MAX {
                    return None;
                }
                let c = self.centre_of(i);
                let extra = ((x - c[0]).powi(2) + (z - c[1]).powi(2)).sqrt();
                (r as usize, (-self.dist[i]).max(0.0) + extra)
            }
        };
        let rim_y = self.height[r];
        // The highest drawn surface in the four cells around the point.
        let mut top = f32::INFINITY;
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (cx, cz) = (gx + dx, gz + dz);
            if cx < 0 || cz < 0 || cx >= self.w as i32 || cz >= self.h as i32 {
                continue;
            }
            top = top.min(self.top[cz as usize * self.w + cx as usize]);
        }
        if out >= SPILL_REACH {
            let y = rim_y + HAZE_DEPTH;
            return Some(FogSheetPoint {
                y,
                weight: if top < y { 0.0 } else { haze_weight(out) },
                spill: true,
                haze: true,
            });
        }
        // Drape over whatever stands below the rim (a tower's flared flank,
        // a ledge): never deeper than just above the highest drawn surface
        // in the four cells around the point, never above the rim.
        let y = (rim_y + spill_drop(out)).min(top - SPILL_CLEARANCE);
        Some(FogSheetPoint {
            y: y.max(rim_y),
            weight: spill_weight(out) * SPILL_SHARE,
            spill: true,
            haze: false,
        })
    }

    /// The field as `w * h` RGBA8 texels, row-major `[z][x]`, what both hosts
    /// upload (once per [`FogGroundFrame::ground_gen`]):
    ///
    /// ```text
    /// R  coverage: 255 on the floor, fading to 0 across EDGE_SOFT either
    ///    side of its edge
    /// G  spill: spill_weight of the distance past the rim, 0 on the floor
    /// B  distance past the rim / SPILL_REACH (0 on the floor, 255 at reach
    ///    and beyond) - the spill streaks' flow coordinate
    /// A  255 (unused)
    /// ```
    pub fn texels(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for &d in &self.dist {
            let cover = smooth01((d + EDGE_SOFT) / (2.0 * EDGE_SOFT));
            let past = (-d).max(0.0);
            let spill = if d < 0.0 && d.is_finite() {
                spill_weight(past)
            } else {
                0.0
            };
            let flow = (past / SPILL_REACH).min(1.0);
            out.extend_from_slice(&[unorm8(cover), unorm8(spill), unorm8(flow), 255]);
        }
        out
    }

    /// Floor cells, for diagnostics and tests.
    pub fn covered_count(&self) -> usize {
        self.height.iter().filter(|y| !y.is_nan()).count()
    }
}

/// One [`FogGround::sheet_at`] sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogSheetPoint {
    /// Sheet height, retail Y-down.
    pub y: f32,
    /// Bank the sheet may carry here, `0..=1`.
    pub weight: f32,
    /// Past a rim: the sheet is falling (or is the haze far below), so it is
    /// exempt from the floor-step test that keeps sheets off cliff faces.
    pub spill: bool,
    /// The distant haze layer rather than the bank or its spill: the hosts
    /// draw it as a faint, thick, slow layer ([`FogVolume::ground_weight`]
    /// carries it as a negative weight).
    pub haze: bool,
}

/// The per-frame view of a [`FogGround`] the hosts upload: the texture rect
/// and its texels, re-uploaded only when [`Self::ground_gen`] changes.
#[derive(Debug, Clone, Copy)]
pub struct FogGroundFrame<'a> {
    /// World X / Z of the texture's corner.
    pub origin: [f32; 2],
    /// World X / Z span of the whole texture.
    pub extent: [f32; 2],
    pub w: u32,
    pub h: u32,
    /// `w * h` RGBA8 texels ([`FogGround::texels`]).
    pub texels: &'a [u8],
    pub ground_gen: u32,
}

/// Box-blur passes over the signed edge distance ([`smooth_dist`]).
const DIST_SMOOTH_PASSES: usize = 3;

/// Smooth the cell-quantised signed distance so a rim that runs diagonal to
/// the cell grid reads as one straight edge rather than a staircase. The
/// distance comes from nearest cell centres, so a diagonal rim carries a
/// one-cell sawtooth; the spill's outward direction is that field's
/// gradient (the shaders' `spill_dir`) and its drop is a steep curve of it,
/// so unsmoothed the curtain pinched into a row of V's (one per cell step)
/// and fell unevenly along the edge. Three 3x3 box passes (about 1.4
/// cells of blur) cancel most of the sawtooth; more start rounding the
/// corners into the edges beside them. Infinite entries (a field with no
/// floor or no open air) leave the field as it is; finite ones are clamped
/// to a few haze reaches first so a far cell cannot drag a rim.
fn smooth_dist(w: usize, h: usize, cell: f32, dist: &mut [f32]) {
    if dist.iter().any(|d| !d.is_finite()) {
        return;
    }
    let cap = (4.0 * HAZE_REACH).max(4.0 * cell);
    let mut cur: Vec<f32> = dist.iter().map(|d| d.clamp(-cap, cap)).collect();
    let mut next = cur.clone();
    for _ in 0..DIST_SMOOTH_PASSES {
        for z in 0..h {
            for x in 0..w {
                let (mut sum, mut n) = (0.0f32, 0.0f32);
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                        if nx < 0 || nz < 0 || nx >= w as i32 || nz >= h as i32 {
                            continue;
                        }
                        sum += cur[nz as usize * w + nx as usize];
                        n += 1.0;
                    }
                }
                next[z * w + x] = sum / n;
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    // Only the band about the rim the coverage fade and the spill read
    // takes the smoothed value: deep on the floor the blur would only
    // lower the ridge down its middle, and far out it would shift the
    // haze's reach.
    for (d, s) in dist.iter_mut().zip(&cur) {
        if *d > -3.0 * SPILL_REACH && *d < 2.0 * EDGE_SOFT {
            *d = *s;
        }
    }
}

fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn unorm8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn site_dist(x: usize, z: usize, sx: usize, sz: usize) -> f32 {
    let dx = x as f32 - sx as f32;
    let dz = z as f32 - sz as f32;
    (dx * dx + dz * dz).sqrt()
}

/// For every cell, the nearest cell (in cells, near-Euclidean) for which
/// `site` holds: two raster passes propagating the nearest site through the
/// 8-neighbourhood (an 8SSEDT-style sweep). `None` where no site exists.
fn nearest_site(w: usize, h: usize, site: impl Fn(usize) -> bool) -> Vec<Option<(usize, usize)>> {
    let mut near: Vec<Option<(usize, usize)>> = (0..w * h)
        .map(|i| site(i).then_some((i % w, i / w)))
        .collect();
    let relax = |near: &mut Vec<Option<(usize, usize)>>, x: usize, z: usize, nx: i32, nz: i32| {
        if nx < 0 || nz < 0 || nx >= w as i32 || nz >= h as i32 {
            return;
        }
        let Some(s) = near[nz as usize * w + nx as usize] else {
            return;
        };
        let i = z * w + x;
        let d = site_dist(x, z, s.0, s.1);
        if near[i].is_none_or(|c| d < site_dist(x, z, c.0, c.1)) {
            near[i] = Some(s);
        }
    };
    for z in 0..h {
        for x in 0..w {
            let (xi, zi) = (x as i32, z as i32);
            relax(&mut near, x, z, xi - 1, zi);
            relax(&mut near, x, z, xi, zi - 1);
            relax(&mut near, x, z, xi - 1, zi - 1);
            relax(&mut near, x, z, xi + 1, zi - 1);
        }
        for x in (0..w).rev() {
            relax(&mut near, x, z, x as i32 + 1, z as i32);
        }
    }
    for z in (0..h).rev() {
        for x in (0..w).rev() {
            let (xi, zi) = (x as i32, z as i32);
            relax(&mut near, x, z, xi + 1, zi);
            relax(&mut near, x, z, xi, zi + 1);
            relax(&mut near, x, z, xi + 1, zi + 1);
            relax(&mut near, x, z, xi - 1, zi + 1);
        }
        for x in 0..w {
            relax(&mut near, x, z, x as i32 - 1, z as i32);
        }
    }
    near
}

fn in_set(w: usize, h: usize, set: &[bool], x: i32, z: i32) -> bool {
    x >= 0 && z >= 0 && x < w as i32 && z < h as i32 && set[z as usize * w + x as usize]
}

/// One-cell 8-neighbour dilation.
fn dilate(w: usize, h: usize, set: &[bool]) -> Vec<bool> {
    let mut out = set.to_vec();
    for z in 0..h {
        for x in 0..w {
            let (xi, zi) = (x as i32, z as i32);
            if !set[z * w + x]
                && (-1..=1).any(|dz| (-1..=1).any(|dx| in_set(w, h, set, xi + dx, zi + dz)))
            {
                out[z * w + x] = true;
            }
        }
    }
    out
}

/// One-cell 8-neighbour erosion (outside the field counts as empty).
fn erode(w: usize, h: usize, set: &[bool]) -> Vec<bool> {
    let mut out = set.to_vec();
    for z in 0..h {
        for x in 0..w {
            let (xi, zi) = (x as i32, z as i32);
            if set[z * w + x]
                && (-1..=1).any(|dz| (-1..=1).any(|dx| !in_set(w, h, set, xi + dx, zi + dz)))
            {
                out[z * w + x] = false;
            }
        }
    }
    out
}

/// The cells for which `pass` holds that a 4-neighbour walk through such
/// cells reaches from the field's border.
fn flood_from_border(w: usize, h: usize, pass: impl Fn(usize) -> bool) -> Vec<bool> {
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
    for z in 0..h {
        for x in 0..w {
            if (x == 0 || z == 0 || x + 1 == w || z + 1 == h) && pass(z * w + x) {
                seen[z * w + x] = true;
                stack.push((x, z));
            }
        }
    }
    while let Some((x, z)) = stack.pop() {
        let mut next = [None; 4];
        if x > 0 {
            next[0] = Some((x - 1, z));
        }
        if x + 1 < w {
            next[1] = Some((x + 1, z));
        }
        if z > 0 {
            next[2] = Some((x, z - 1));
        }
        if z + 1 < h {
            next[3] = Some((x, z + 1));
        }
        for (nx, nz) in next.into_iter().flatten() {
            let i = nz * w + nx;
            if !seen[i] && pass(i) {
                seen[i] = true;
                stack.push((nx, nz));
            }
        }
    }
    seen
}
