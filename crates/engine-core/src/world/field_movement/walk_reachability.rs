//! Walkable-component analysis over the collision grid: connected components,
//! reachability, the nearest standable seat and the cold-spawn resolver.

use super::*;

impl World {
    /// Label the 4-connected components of the open sub-cell lattice
    /// ([`Self::field_subcell_open`], `0x100 x 0x100` sub-cells). Returns
    /// `(labels, sizes)`: `labels[sz * 0x100 + sx]` is `0` for a closed
    /// sub-cell or the 1-based component id; `sizes[id - 1]` is that
    /// component's sub-cell count. Deterministic: components are numbered in
    /// row-major scan order.
    pub(crate) fn field_walk_components(&self) -> (Vec<u16>, Vec<u32>) {
        let (labels, sizes, _) = self.field_walk_components_edged();
        (labels, sizes)
    }

    /// [`Self::field_walk_components`] plus, per component, which of the
    /// map's four outer edges its sub-cells reach
    /// (`[x_min, x_max, z_min, z_max]`).
    ///
    /// The edge record is what separates a scene's playable ground from the
    /// **open space around it**. A kingdom overworld's collision grid leaves
    /// the sea open - retail can afford that, because the coastline is a
    /// closed wall ring and a party that only ever arrives through a door
    /// warp can never be on the water side of it. The sea is nevertheless
    /// the grid's *largest* open region by a factor of ~4, so a size-only
    /// pick lands the cold-entry player offshore with the whole continent
    /// unreachable. A region that reaches three or more map edges is that
    /// surrounding space rather than an enclosed area of the map, which is
    /// the discriminator [`Self::resolve_cold_field_spawn`] uses.
    fn field_walk_components_edged(&self) -> (Vec<u16>, Vec<u32>, Vec<[bool; 4]>) {
        self.subcell_components_edged(&|sx, sz| self.field_subcell_open(sx, sz))
    }

    /// The component labelling behind [`Self::field_walk_components_edged`]
    /// over any per-sub-cell `open` predicate.
    fn subcell_components_edged(
        &self,
        open: &dyn Fn(i32, i32) -> bool,
    ) -> (Vec<u16>, Vec<u32>, Vec<[bool; 4]>) {
        let stride = FIELD_GRID_STRIDE * 2;
        let last = stride as i32 - 1;
        let mut labels = vec![0u16; stride * stride];
        let mut sizes: Vec<u32> = Vec::new();
        let mut edges: Vec<[bool; 4]> = Vec::new();
        let mut queue: std::collections::VecDeque<(i32, i32)> = std::collections::VecDeque::new();
        for sz in 0..stride as i32 {
            for sx in 0..stride as i32 {
                let idx = sz as usize * stride + sx as usize;
                if labels[idx] != 0 || !open(sx, sz) {
                    continue;
                }
                let label = (sizes.len() + 1) as u16;
                let mut count = 0u32;
                let mut touched = [false; 4];
                labels[idx] = label;
                queue.push_back((sx, sz));
                while let Some((cx, cz)) = queue.pop_front() {
                    count += 1;
                    touched[0] |= cx == 0;
                    touched[1] |= cx == last;
                    touched[2] |= cz == 0;
                    touched[3] |= cz == last;
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (cx + dx, cz + dz);
                        if !(0..stride as i32).contains(&nx) || !(0..stride as i32).contains(&nz) {
                            continue;
                        }
                        let nidx = nz as usize * stride + nx as usize;
                        if labels[nidx] == 0 && open(nx, nz) {
                            labels[nidx] = label;
                            queue.push_back((nx, nz));
                        }
                    }
                }
                sizes.push(count);
                edges.push(touched);
            }
        }
        (labels, sizes, edges)
    }

    /// Whether the collision grid alone lets a walker get from `(x, z)` onto
    /// the authored walk-visible floor within `budget` 64-unit sub-cells - the
    /// wall-bit flood the locomotion collision (`FUN_801CFE4C`) actually
    /// enforces, ending on a sub-cell [`Self::field_walk_component_size`]
    /// counts. `false` when the point itself sits in a wall bit.
    ///
    /// The two disagree on floor a placed object provides: `chitei2`'s
    /// escape platform (the env mesh on partition-0 record 31) carries no
    /// floor-cell bit, yet it is open collision whose stairs lead down onto
    /// the corridor floor.
    pub fn field_collision_reaches_floor(&self, x: i16, z: i16, budget: usize) -> bool {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN || x < 0 || z < 0 {
            return false;
        }
        let stride = (FIELD_GRID_STRIDE * 2) as i32;
        let open = |sx: i32, sz: i32| {
            (0..stride).contains(&sx)
                && (0..stride).contains(&sz)
                && !self.field_tile_is_wall((sx * 64 + 32) as i16, (sz * 64 + 32) as i16)
        };
        let start = ((x as i32) >> 6, (z as i32) >> 6);
        if !open(start.0, start.1) {
            return false;
        }
        let mut seen = std::collections::HashSet::from([start]);
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some((cx, cz)) = queue.pop_front() {
            if self.field_subcell_open(cx, cz) {
                return true;
            }
            if seen.len() >= budget {
                break;
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = (cx + dx, cz + dz);
                if open(n.0, n.1) && seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        false
    }

    /// Whether the collision grid boxes a walker at `(x, z)` into a pocket
    /// with no way onto the authored walk-visible floor: the point sits in a
    /// wall bit, or the wall-bit flood from it closes off inside `budget`
    /// 64-unit sub-cells without reaching a sub-cell
    /// [`Self::field_walk_component_size`] counts.
    ///
    /// A flood that runs past the budget is open ground, floor bit or not,
    /// and the walker is not boxed in. Retail's locomotion reads only the
    /// wall bits (`FUN_801CFE4C`), so open collision with no floor-cell bit is
    /// ground the player walks off: `taiku`'s post-boss cutscene (P2[29])
    /// ends with the party on the castle-collapse escape route, a stretch
    /// of open collision the object grid marks no floor on.
    pub fn field_collision_boxed_in(&self, x: i16, z: i16, budget: usize) -> bool {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN || x < 0 || z < 0 {
            return false;
        }
        let stride = (FIELD_GRID_STRIDE * 2) as i32;
        let open = |sx: i32, sz: i32| {
            (0..stride).contains(&sx)
                && (0..stride).contains(&sz)
                && !self.field_tile_is_wall((sx * 64 + 32) as i16, (sz * 64 + 32) as i16)
        };
        let start = ((x as i32) >> 6, (z as i32) >> 6);
        if !open(start.0, start.1) {
            return true;
        }
        let mut seen = std::collections::HashSet::from([start]);
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some((cx, cz)) = queue.pop_front() {
            if self.field_subcell_open(cx, cz) {
                return false;
            }
            if seen.len() >= budget {
                return false;
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = (cx + dx, cz + dz);
                if open(n.0, n.1) && seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        true
    }

    /// Size (in 64-unit sub-cells) of the connected open-floor region the
    /// world point `(x, z)` stands in - `0` when the covering sub-cell is
    /// closed (off the walk-visible floor or inside a wall). The reachability
    /// measure the cold-spawn resolver and the spawn-sweep tests share: a
    /// position inside a walled-off pocket reads as a tiny component even
    /// though the point itself is "valid".
    pub fn field_walk_component_size(&self, x: i16, z: i16) -> usize {
        if x < 0 || z < 0 {
            return 0;
        }
        let (sx, sz) = ((x as i32) >> 6, (z as i32) >> 6);
        if !self.field_subcell_open(sx, sz) {
            return 0;
        }
        let stride = FIELD_GRID_STRIDE * 2;
        let (labels, sizes) = self.field_walk_components();
        let label = labels[sz as usize * stride + sx as usize];
        if label == 0 {
            0
        } else {
            sizes[label as usize - 1] as usize
        }
    }

    /// The nearest standable world position to `(x, z)`, or `(x, z)` itself
    /// when it already is one.
    ///
    /// "Standable" is the spawn test [`Self::field_spawn_is_valid`]: on the
    /// authored walk-visible floor and clear of the collision grid's wall
    /// bits. The search walks outward in 64-unit sub-cell rings and stops at
    /// [`SEAT_RESCUE_RADIUS_SUBCELLS`]; within a ring, sub-cells are visited
    /// in a fixed row-major order, so the answer is deterministic. A point
    /// with nothing standable inside the radius is returned unchanged - a
    /// bounded nudge is a correction, and hauling the player across the map
    /// would be a different decision that the caller has not asked for.
    ///
    /// Scenes with no collision grid loaded (`field_spawn_is_valid` reads
    /// every tile as off-floor there) pass straight through.
    pub fn nearest_standable_seat(&self, x: i16, z: i16) -> (i16, i16) {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN
            || self.terrain.object_cells.len() < FIELD_GRID_LEN
            || self.field_spawn_is_valid(x, z)
        {
            return (x, z);
        }
        let (sx0, sz0) = ((x as i32) >> 6, (z as i32) >> 6);
        for r in 1..=SEAT_RESCUE_RADIUS_SUBCELLS {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let (sx, sz) = (sx0 + dx, sz0 + dz);
                    if self.field_subcell_open(sx, sz) {
                        return ((sx * 64 + 32) as i16, (sz * 64 + 32) as i16);
                    }
                }
            }
        }
        (x, z)
    }

    /// Size (in 64-unit sub-cells) of the scene's largest connected open-floor
    /// region, or `0` when no sub-cell is open.
    pub fn field_largest_walk_component_size(&self) -> usize {
        self.field_walk_components()
            .1
            .into_iter()
            .max()
            .unwrap_or(0) as usize
    }

    /// The first of `arrivals` (world `(x, z)` door-arrival spots, best
    /// first) that makes a **scene-picker seat**: `(index, seat)`, or `None`
    /// when none qualifies.
    ///
    /// Every seat must be clear of the wall bits, off every `avoid_tiles`
    /// tile (the host passes the kind-0 teleport pads and the gate-1 walk-on
    /// trigger tiles: the first tick's stale-tile compare would fire either,
    /// warping the party out or starting the arrival's cutscene beat) and one
    /// the player can walk off (a leading-edge probe clear), and must stand
    /// in a **room**: an open region that
    /// reaches fewer than three map edges (never the open space around a
    /// map, where the cutscene arrivals parked at tile `(0x7F, 0x7F)` stand)
    /// and spans at least [`PICKER_ARRIVAL_MIN_REGION_SUBCELLS`] sub-cells
    /// (never a sealed pocket). A door tile is routinely a wall cell (a door
    /// is a gap in a wall) or a trigger tile, so an arrival that fails is
    /// replaced by the nearest sub-cell centre that passes, within
    /// [`SEAT_RESCUE_RADIUS_SUBCELLS`]. An arrival is tried two ways:
    ///
    /// 1. By retail's own standing rule, the collision grid's wall bits
    ///    ([`Self::field_tile_is_wall`], regions per
    ///    [`Self::field_wall_region`]). This is what seats arrivals the
    ///    port's walk-visible floor filter leaves off its authored area
    ///    (Rim Elm's gate among them).
    /// 2. By the port's authored-floor rule, the one
    ///    [`Self::resolve_cold_field_spawn`] uses
    ///    ([`Self::field_spawn_is_valid`]). This covers the towns whose wall
    ///    grid leaves the land beyond their gates open to the map edges, so
    ///    no wall-bit region there is a room.
    ///
    /// Unlike the cold resolver it does not demand the scene's *largest*
    /// region: a dungeon whose floors are disconnected rooms is entered
    /// through a door into one of them.
    ///
    /// Requires the collision grid (and, for the second rule, the object
    /// cells) to be loaded.
    pub fn picker_arrival_seat(
        &self,
        arrivals: &[(i16, i16)],
        avoid_tiles: &[(u8, u8)],
    ) -> Option<(usize, (i16, i16))> {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN {
            return None;
        }
        let stride = FIELD_GRID_STRIDE * 2;
        let wall_free = |sx: i32, sz: i32| self.field_subcell_wall_free(sx, sz);
        let wall_regions = self.subcell_components_edged(&wall_free);
        let floor_regions = self.field_walk_components_edged();
        // The tile quantisation the trigger dispatchers use (`world >> 7`).
        let on_avoided_tile = |x: i16, z: i16| -> bool {
            let (tx, tz) = ((x as u16 >> 7) as u8, (z as u16 >> 7) as u8);
            avoid_tiles.iter().any(|&(kx, kz)| (kx, kz) == (tx, tz))
        };
        // Inside an enclosed region of `regions` big enough to be a room.
        let in_room = |(labels, sizes, edges): &(Vec<u16>, Vec<u32>, Vec<[bool; 4]>),
                       x: i16,
                       z: i16|
         -> bool {
            let (sx, sz) = ((x as usize) >> 6, (z as usize) >> 6);
            let Some(i) = labels
                .get(sz * stride + sx)
                .and_then(|&l| usize::from(l).checked_sub(1))
            else {
                return false;
            };
            edges[i].iter().filter(|t| **t).count() < 3
                && sizes[i] as usize >= PICKER_ARRIVAL_MIN_REGION_SUBCELLS
        };
        let usable = |x: i16, z: i16| -> bool {
            x >= 0
                && z >= 0
                && !self.field_tile_is_wall(x, z)
                && !on_avoided_tile(x, z)
                && (0..4).any(|d| !self.field_dir_blocked(x, z, d))
        };
        // Retail's rule (wall bits) first; then the port's authored-floor
        // rule, for the towns whose wall grid leaves the land beyond their
        // gates open to the map edges.
        let good_by_walls = |x: i16, z: i16| usable(x, z) && in_room(&wall_regions, x, z);
        let good_by_floor = |x: i16, z: i16| {
            usable(x, z) && self.field_spawn_is_valid(x, z) && in_room(&floor_regions, x, z)
        };
        // The arrival itself when it qualifies, else the nearest sub-cell
        // centre that does, ring by ring (row-major within a ring) out to the
        // seat-rescue radius.
        let nearest = |x: i16, z: i16, good: &dyn Fn(i16, i16) -> bool| -> Option<(i16, i16)> {
            if good(x, z) {
                return Some((x, z));
            }
            let (sx0, sz0) = ((x as i32) >> 6, (z as i32) >> 6);
            for r in 1..=SEAT_RESCUE_RADIUS_SUBCELLS {
                for dz in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dz.abs()) != r {
                            continue;
                        }
                        let (sx, sz) = (sx0 + dx, sz0 + dz);
                        if !(0..stride as i32).contains(&sx) || !(0..stride as i32).contains(&sz) {
                            continue;
                        }
                        let c = ((sx * 64 + 32) as i16, (sz * 64 + 32) as i16);
                        if good(c.0, c.1) {
                            return Some(c);
                        }
                    }
                }
            }
            None
        };
        arrivals.iter().enumerate().find_map(|(i, &(x, z))| {
            nearest(x, z, &good_by_walls)
                .or_else(|| nearest(x, z, &good_by_floor))
                .map(|seat| (i, seat))
        })
    }

    /// The wall-bounded region world `(x, z)` stands in, by the collision
    /// grid's wall bits alone (retail's standing rule): `(sub-cells, map
    /// edges reached)`, or `None` when the point sits on a wall bit or no
    /// grid is loaded. The measure [`Self::picker_arrival_seat`] judges a
    /// door arrival by.
    pub fn field_wall_region(&self, x: i16, z: i16) -> Option<(usize, usize)> {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN || x < 0 || z < 0 {
            return None;
        }
        let (sx, sz) = ((x as i32) >> 6, (z as i32) >> 6);
        if !self.field_subcell_wall_free(sx, sz) {
            return None;
        }
        let (labels, sizes, edges) =
            self.subcell_components_edged(&|a, b| self.field_subcell_wall_free(a, b));
        let label = labels[sz as usize * FIELD_GRID_STRIDE * 2 + sx as usize];
        let i = usize::from(label).checked_sub(1)?;
        Some((sizes[i] as usize, edges[i].iter().filter(|t| **t).count()))
    }

    /// Is the 64-unit sub-cell `(sx, sz)` clear of the collision grid's wall
    /// bits at its centre? Out-of-range sub-cells read closed.
    fn field_subcell_wall_free(&self, sx: i32, sz: i32) -> bool {
        let stride = (FIELD_GRID_STRIDE * 2) as i32;
        (0..stride).contains(&sx)
            && (0..stride).contains(&sz)
            && !self.field_tile_is_wall((sx * 64 + 32) as i16, (sz * 64 + 32) as i16)
    }

    /// Resolve a cold field-entry player spawn to an in-bounds, standable,
    /// **reachable** world `(x, z)`.
    ///
    /// Retail seats a cold (non-warp) field entry at the camera-window centre
    /// [`FIELD_COLD_SPAWN_XZ`] (`0xA40`); a cold entry only ever happens for the
    /// New Game opening (town01, Vahn's authored Rim Elm spawn), where that
    /// coordinate is a real standable tile. The engine's scene picker enters
    /// arbitrary scenes cold, and for many of them the fixed seat lands off the
    /// authored walkable floor, inside a walled-off pocket (a "valid" point
    /// whose connected region is tiny), in a secondary region cut off from the
    /// scene's main playable area, on a kind-0 intra-scene teleport tile
    /// (a door pad whose first tile-crossing dispatch warps the player), or -
    /// on the three kingdom overworlds - **offshore**.
    ///
    /// The **main region** is the largest connected open-floor component that
    /// does not reach three or more of the map's outer edges
    /// ([`Self::field_walk_components_edged`]). Dropping the outer ones is
    /// what keeps a kingdom overworld's cold entry on its continent: `map01` /
    /// `map02` / `map03` leave the sea open in the collision grid (retail's
    /// coastline ring is what keeps the party out of it, and retail never
    /// cold-enters an overworld at all), and that sea is ~4x the size of the
    /// continent, so a plain size pick seats the player on open water with the
    /// whole landmass walled off behind the coast. No other scene on the disc
    /// has an outer component large enough to be picked, so this narrows to
    /// the three overworlds and leaves every town / dungeon spawn where it
    /// was.
    ///
    /// Selection rule (deterministic per scene):
    ///
    /// 1. Keep the retail seat when it is standable, inside the scene's
    ///    **main region**, not on a kind-0 teleport tile (`teleport_tiles`),
    ///    and a seat the player can walk off (at least one of the four
    ///    leading-edge wall probes clear) - town01's New Game opening stays
    ///    byte-identical.
    /// 2. Otherwise take the first kind-0 teleport **destination** (`anchors`,
    ///    in disc table order) that passes the same checks - a retail-authored
    ///    door-arrival spot.
    /// 3. Otherwise spawn at the main region's own sub-cell nearest its
    ///    centroid (skipping teleport tiles and sub-cells the player cannot
    ///    walk off), i.e. the middle of the scene's biggest enclosed playable
    ///    region.
    /// 4. A scene with no open floor at all keeps the retail seat (nothing
    ///    better to resolve against).
    ///
    /// Requires the collision grid ([`Self::load_field_collision_grid`]) and the
    /// object-grid cells ([`Self::load_field_object_cells`]) to be loaded first.
    /// `teleport_tiles` / `anchors` are the scene's `.MAP` kind-0 trigger tiles
    /// and landing positions ([`legaia_engine_field::field_regions::IntraSceneTeleport`]).
    pub fn resolve_cold_field_spawn(
        &self,
        teleport_tiles: &[(u8, u8)],
        anchors: &[(i16, i16)],
    ) -> (i16, i16) {
        let default = (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ);
        let stride = FIELD_GRID_STRIDE * 2;
        let (labels, sizes, edges) = self.field_walk_components_edged();
        // Largest component, ignoring any that reaches three or more of the
        // map's outer edges (see `field_walk_components_edged`: that is the
        // open space *around* the authored area - the kingdom overworlds'
        // sea, which outweighs their continent ~4:1). Ties keep the first
        // (lowest label) for determinism; a scene whose every component is
        // an outer one keeps the plain largest.
        let enclosed = |i: usize| edges[i].iter().filter(|t| **t).count() < 3;
        let pick = |filtered: bool| -> Option<u16> {
            sizes
                .iter()
                .enumerate()
                .filter(|(i, _)| !filtered || enclosed(*i))
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))
                .map(|(i, _)| (i + 1) as u16)
        };
        let Some(largest_label) = pick(true).or_else(|| pick(false)) else {
            return default;
        };
        let on_teleport_tile = |x: i16, z: i16| -> bool {
            let (tx, tz) = ((x as u16 >> 7) as u8, (z as u16 >> 7) as u8);
            teleport_tiles.iter().any(|&(kx, kz)| (kx, kz) == (tx, tz))
        };
        // A seat the player cannot walk off is not a seat. The sub-cell
        // lattice is open-floor granularity; the wall bits the locomotion
        // controller actually probes are read with retail's `+2` Z bias and
        // `ceil-1` X rounding ([`Self::field_dir_blocked`]), so a sub-cell can
        // be "open" while all four leading-edge probes from its centre land on
        // wall. `kor5`'s retail seat is exactly that, and so is the centroid
        // `korb3` resolves to without this test.
        let can_move =
            |x: i16, z: i16| -> bool { (0..4).any(|d| !self.field_dir_blocked(x, z, d)) };
        let good = |x: i16, z: i16| -> bool {
            if x < 0 || z < 0 || !self.field_spawn_is_valid(x, z) || on_teleport_tile(x, z) {
                return false;
            }
            let (sx, sz) = ((x as usize) >> 6, (z as usize) >> 6);
            labels
                .get(sz * stride + sx)
                .is_some_and(|&l| l == largest_label)
                && can_move(x, z)
        };
        // 1. The retail seat, when it is genuinely standable and reachable.
        if good(default.0, default.1) {
            return default;
        }
        // 2. A retail-authored door-arrival anchor inside the main region.
        for &(ax, az) in anchors {
            if good(ax, az) {
                return (ax, az);
            }
        }
        // 3. The largest component's sub-cell nearest its centroid.
        let (mut sum_x, mut sum_z, mut n) = (0i64, 0i64, 0i64);
        for sz in 0..stride {
            for sx in 0..stride {
                if labels[sz * stride + sx] == largest_label {
                    sum_x += sx as i64;
                    sum_z += sz as i64;
                    n += 1;
                }
            }
        }
        if n == 0 {
            return default;
        }
        let (cx, cz) = (sum_x / n, sum_z / n);
        let mut best: Option<(i64, (i16, i16))> = None;
        let mut best_any: Option<(i64, (i16, i16))> = None;
        for sz in 0..stride {
            for sx in 0..stride {
                if labels[sz * stride + sx] != largest_label {
                    continue;
                }
                let d = (sx as i64 - cx).pow(2) + (sz as i64 - cz).pow(2);
                let world = ((sx * 64 + 32) as i16, (sz * 64 + 32) as i16);
                if best_any.is_none_or(|(bd, _)| d < bd) {
                    best_any = Some((d, world));
                }
                if !on_teleport_tile(world.0, world.1)
                    && best.is_none_or(|(bd, _)| d < bd)
                    && can_move(world.0, world.1)
                {
                    best = Some((d, world));
                }
            }
        }
        best.or(best_any).map(|(_, w)| w).unwrap_or(default)
    }
}
