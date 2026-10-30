//! A walk path over the collision grid's open sub-cells: an A* search on
//! the `64`-unit lattice [`World::field_subcell_open`] defines, the result
//! pulled taut into the fewest straight legs that keep to open ground.
//!
//! No retail routine walks a path - a retail scripted walk is a straight
//! glide the script author kept clear of walls. This is what an expansion
//! cutscene's `move` step walks when its straight leg would cross a wall
//! (`World::run_cutscene_command`), so a pack author can name a tile across
//! the room and the actor goes round the table. It reads the grid and
//! writes nothing.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::*;

/// Sub-cells a search may expand before it gives up (a quarter of the
/// lattice: a path that needs more is not one a cutscene means).
const MAX_EXPANSIONS: usize = 0x4000;
/// World units between samples on a leg's line-of-sight test (a quarter of
/// a sub-cell, so no wall corner is stepped over).
const SIGHT_STEP: i32 = 16;

impl World {
    /// Whether the straight leg from world `a` to `b` keeps to open
    /// sub-cells all the way.
    pub(in crate::world) fn field_leg_clear(&self, a: (i32, i32), b: (i32, i32)) -> bool {
        let (dx, dz) = (b.0 - a.0, b.1 - a.1);
        let n = (dx.abs().max(dz.abs()) / SIGHT_STEP).max(1);
        (0..=n).all(|i| {
            let x = a.0 + dx * i / n;
            let z = a.1 + dz * i / n;
            self.field_subcell_open(x.div_euclid(64), z.div_euclid(64))
        })
    }

    /// The world points a walk from `from` to `to` passes through: the
    /// turns of the shortest open path, then `to` itself, `from` left out.
    /// `None` when there is no grid, either end stands closed, or no path
    /// joins them within the search budget - the caller walks straight.
    pub(in crate::world) fn field_walk_path(
        &self,
        from: (i32, i32),
        to: (i32, i32),
    ) -> Option<Vec<(i32, i32)>> {
        if self.terrain.collision_grid.is_empty() {
            return None;
        }
        let stride = (FIELD_GRID_STRIDE * 2) as i32;
        let cell = |p: (i32, i32)| (p.0.div_euclid(64), p.1.div_euclid(64));
        let (start, goal) = (cell(from), cell(to));
        if !self.field_subcell_open(start.0, start.1) || !self.field_subcell_open(goal.0, goal.1) {
            return None;
        }
        if start == goal {
            return Some(vec![to]);
        }
        let idx = |c: (i32, i32)| (c.1 * stride + c.0) as usize;
        let h = |c: (i32, i32)| {
            let (ax, az) = ((c.0 - goal.0).abs(), (c.1 - goal.1).abs());
            10 * ax.max(az) + 4 * ax.min(az)
        };
        let n = (stride * stride) as usize;
        let mut cost = vec![i32::MAX; n];
        let mut came = vec![u32::MAX; n];
        let mut open = BinaryHeap::new();
        cost[idx(start)] = 0;
        open.push(Reverse((h(start), 0i32, start.0, start.1)));
        let mut expanded = 0usize;
        let mut found = false;
        while let Some(Reverse((_, g, cx, cz))) = open.pop() {
            let c = (cx, cz);
            if g > cost[idx(c)] {
                continue;
            }
            if c == goal {
                found = true;
                break;
            }
            expanded += 1;
            if expanded > MAX_EXPANSIONS {
                return None;
            }
            for (dx, dz) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let nc = (cx + dx, cz + dz);
                if !self.field_subcell_open(nc.0, nc.1) {
                    continue;
                }
                // A diagonal never cuts a wall's corner.
                if dx != 0
                    && dz != 0
                    && (!self.field_subcell_open(cx + dx, cz)
                        || !self.field_subcell_open(cx, cz + dz))
                {
                    continue;
                }
                let step = if dx != 0 && dz != 0 { 14 } else { 10 };
                let ng = g + step;
                if ng < cost[idx(nc)] {
                    cost[idx(nc)] = ng;
                    came[idx(nc)] = idx(c) as u32;
                    open.push(Reverse((ng + h(nc), ng, nc.0, nc.1)));
                }
            }
        }
        if !found {
            return None;
        }
        // Back from the goal to the start, as sub-cell centres.
        let mut cells = vec![goal];
        let mut at = idx(goal);
        while at != idx(start) {
            at = came[at] as usize;
            let s = stride as usize;
            cells.push(((at % s) as i32, (at / s) as i32));
        }
        cells.reverse();
        let centre = |c: (i32, i32)| (c.0 * 64 + 32, c.1 * 64 + 32);
        let mut points: Vec<(i32, i32)> = cells.iter().map(|&c| centre(c)).collect();
        // The ends are the walk's own points, not their cells' centres.
        points[0] = from;
        if let Some(last) = points.last_mut() {
            *last = to;
        }
        // Pull the path taut: from each kept point, jump to the farthest
        // point a clear straight leg reaches.
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < points.len() {
            let mut j = points.len() - 1;
            while j > i + 1 && !self.field_leg_clear(points[i], points[j]) {
                j -= 1;
            }
            out.push(points[j]);
            i = j;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field world: floor tiles `1..12` square, a wall band across tile
    /// row 6 from tile column 2 to 8 (open round its ends).
    fn walled() -> World {
        let mut w = World::new();
        let mut grid = vec![0xF0u8; FIELD_GRID_LEN];
        let mut cells = vec![0u8; FIELD_GRID_LEN * 2];
        for z in 1..12 {
            for x in 1..12 {
                grid[z * FIELD_GRID_STRIDE + x] = 0;
                let c = legaia_scene_formats::field_objects::CELL_WALK_VISIBLE.to_le_bytes();
                cells[(z * FIELD_GRID_STRIDE + x) * 2..][..2].copy_from_slice(&c);
            }
        }
        for x in 2..9 {
            grid[6 * FIELD_GRID_STRIDE + x] = 0xF0;
        }
        w.load_field_collision_grid(&grid);
        w.load_field_object_cells(&cells);
        w
    }

    /// The centre of the first open sub-cell in sub-cell row `sz`, from
    /// column `sx` on.
    fn open_at(w: &World, sx: i32, sz: i32) -> (i32, i32) {
        let sx = (sx..64)
            .find(|&x| w.field_subcell_open(x, sz))
            .expect("an open sub-cell");
        (sx * 64 + 32, sz * 64 + 32)
    }

    #[test]
    fn a_walk_round_a_wall_keeps_to_open_ground() {
        let w = walled();
        // Two open spots straight across the band from each other.
        let a = open_at(&w, 9, 6);
        let b = (a.0, open_at(&w, a.0 / 64, 17).1);
        assert!(w.field_subcell_open(b.0 / 64, b.1 / 64));
        assert!(
            !w.field_leg_clear(a, b),
            "the straight leg crosses the band"
        );
        let path = w.field_walk_path(a, b).expect("a way round");
        assert_eq!(path.last(), Some(&b));
        assert!(path.len() >= 2, "{path:?}");
        let mut prev = a;
        for &p in &path {
            assert!(w.field_leg_clear(prev, p), "{prev:?} -> {p:?} in {path:?}");
            prev = p;
        }
    }

    #[test]
    fn a_clear_walk_is_one_leg_and_no_grid_is_no_path() {
        let w = walled();
        let a = open_at(&w, 4, 4);
        let b = open_at(&w, a.0 / 64 + 8, 4);
        assert!(w.field_leg_clear(a, b));
        assert_eq!(w.field_walk_path(a, b), Some(vec![b]));
        assert_eq!(World::new().field_walk_path(a, b), None);
    }
}
