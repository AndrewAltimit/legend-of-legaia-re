//! Field collision grid, region tables, floor sampling, wall/interact probes, NPC/actor motion, direction decoding, and free-movement locomotion.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

/// Result of one direction's prop-collision probe
/// ([`World::field_prop_dir_probe`]): whether a solid prop box blocks the
/// step, and - for a static-class (auto-touch) hit - which prop-bank entry
/// the contact posts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PropDirProbe {
    /// A solid prop box overlaps a probe point: the 2-unit step is refused
    /// (retail result bits `1`/`4` both gate the commit).
    pub blocked: bool,
    /// Static-class hit with a bank entry: the anchor whose record the
    /// contact auto-posts (`None` for interact-class or unbound props).
    pub touch: Option<(u8, u8)>,
}

impl World {
    // --- field collision grid + free-movement locomotion ----------------

    /// The `FIELD_ACTOR_PROBES` row indices of the directions held in a
    /// post-remap `dir_bits` word (`0x1000` = Z+ -> row 2, `0x4000` = Z- ->
    /// row 0, `0x2000` = X+ -> row 3, `0x8000` = X- -> row 1).
    fn dirs_of_bits(dir_bits: u16) -> impl Iterator<Item = usize> {
        [(0x1000u16, 2usize), (0x4000, 0), (0x2000, 3), (0x8000, 1)]
            .into_iter()
            .filter_map(move |(bit, dir)| (dir_bits & bit != 0).then_some(dir))
    }

    /// Reset the per-scene field collision grid to "all walkable" (every
    /// byte zero). Called at field entry; the scene prescript repaints the
    /// wall bits via the field-VM `0x4C` outer-nibble-7 op. Mirrors the
    /// retail wholesale clear of `*(_DAT_1F8003EC) + 0x4000` at scene boot
    /// (the exact retail clear site is unpinned; zeroing here is the
    /// engine-side equivalent - see `docs/subsystems/field-locomotion.md`).
    pub fn reset_field_collision_grid(&mut self) {
        self.field_collision_grid.clear();
        self.field_collision_grid.resize(FIELD_GRID_LEN, 0);
    }

    /// Load the per-scene base collision/floor grid from the field map file's
    /// `+0x4000` region (the `DATA\FIELD\<scene>.MAP` slice exposed by
    /// [`crate::scene::Scene::field_collision_grid`]). `grid` is the raw
    /// `0x80 x 0x80` byte grid: high nibble = sub-cell wall bits, low nibble =
    /// floor-elevation tier - the same byte format the runtime grid uses, so
    /// it copies verbatim. The field-VM `0x4C` nibble-7 ops then layer
    /// story-conditional deltas on top as the prescript runs.
    ///
    /// PORT: the `+0x4000` sub-region streamed by `FUN_8001f7c0` into the
    /// field buffer at `*(_DAT_1f8003ec)`. Byte-exact vs live RAM (town01).
    pub fn load_field_collision_grid(&mut self, grid: &[u8]) {
        let n = grid.len().min(FIELD_GRID_LEN);
        self.field_collision_grid.clear();
        self.field_collision_grid.resize(FIELD_GRID_LEN, 0);
        self.field_collision_grid[..n].copy_from_slice(&grid[..n]);
    }

    /// Load the per-scene `.MAP` **object-grid** cell words (`+0x8000`, one
    /// little-endian `u16` per tile). The floor sampler tests each tile's
    /// [`CELL_ELEVATION_OVERRIDE`] bit; the other bits (`0x1FF` object index,
    /// `0x1000` walk-visible, `0x2000` visible) belong to the scene's
    /// placement / ground layers. A short slice loads what it has and leaves
    /// the rest zero (a plain bilinear tile).
    pub fn load_field_object_cells(&mut self, cells: &[u8]) {
        let n = (cells.len() / 2).min(FIELD_GRID_LEN);
        self.field_object_cells.clear();
        self.field_object_cells.resize(FIELD_GRID_LEN, 0);
        for (i, slot) in self.field_object_cells[..n].iter_mut().enumerate() {
            *slot = u16::from_le_bytes([cells[i * 2], cells[i * 2 + 1]]);
        }
    }

    /// Install the scene's kind-2 **elevation-override** records from the
    /// `.MAP` trigger blocks: `primary` is the `+0x10000` block, `fallback` the
    /// `+0x12000` one (the sibling sectors the retail loader reads
    /// contiguously). The two parse into one first-match-wins list, mirroring
    /// `FUN_801D5630`'s primary-then-fallback scan.
    ///
    /// PORT: FUN_801D5630 (kind 2) / FUN_801D5AE0
    pub fn load_field_elevation_overrides(&mut self, primary: &[u8], fallback: &[u8]) {
        self.field_elevation_overrides =
            crate::world::field_elevation::parse_elevation_overrides(primary)
                .into_iter()
                .chain(crate::world::field_elevation::parse_elevation_overrides(
                    fallback,
                ))
                .collect();
    }

    /// Install the per-scene region / zone tables (the `.MAP` `+0x10000`
    /// block + the MAN section-3 camera-region table) and run the initial
    /// per-tile refresh. Pass empty slices for scenes without the data -
    /// the refresh then clears [`Self::extra_flags`] and resets the
    /// attribute block to the default fill, so stale tables never leak
    /// across a transition.
    pub fn load_field_region_tables(&mut self, map_region_block: &[u8], zone_table: &[u8]) {
        self.field_map_region_block = map_region_block.to_vec();
        self.field_zone_table = zone_table.to_vec();
        self.refresh_field_regions();
    }

    /// Per-tile region refresh - drives the [`crate::field_regions`] ports
    /// (`FUN_800180EC` + `FUN_801DBA20`) against the player's current tile.
    ///
    /// Quantises `tile = (world - 0x40) >> 7` (the retail locomotion-cluster
    /// convention for `FUN_801DBA20`'s arguments), rebuilds
    /// [`Self::extra_flags`] (the `_DAT_8007B8F4` region-type mask the
    /// field-VM op `0x42` mode 0 tests), latches the scratch attribute
    /// block, and re-selects the current camera-zone record. Called on
    /// scene entry and on every player tile crossing
    /// (`Self::live_field_tick`).
    ///
    /// REF: FUN_800180EC, FUN_801DBA20 (ports in [`crate::field_regions`])
    pub fn refresh_field_regions(&mut self) {
        if self.field_map_region_block.is_empty() && self.field_zone_table.is_empty() {
            // No per-scene tables installed - leave `extra_flags` to the
            // host (e.g. tests that drive op 0x42 directly).
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (wx, wz) = match self.actors.get(slot as usize) {
            Some(a) => (a.move_state.world_x, a.move_state.world_z),
            None => return,
        };
        let tx = (wx as i32 - 0x40) >> 7;
        let tz = (wz as i32 - 0x40) >> 7;
        let table = crate::field_regions::RegionTable::parse(&self.field_map_region_block);
        let world_map_mode = self.mode == SceneMode::WorldMap;
        let (mask, attrs) =
            crate::field_regions::refresh_region_attributes(table.as_ref(), tx, tz, world_map_mode);
        self.extra_flags = mask;
        self.field_region_attributes = attrs;
        if let Some(result) =
            crate::field_regions::zone_query(&self.field_zone_table, table.as_ref(), &attrs, tx, tz)
        {
            // Retail rewrites `_DAT_8007B8F4` from the zone query's own
            // rebuild too (identical recomputation).
            self.extra_flags = result.region_mask;
            self.field_zone_record = result.record.map(|r| {
                let mut rec = [0u8; crate::field_regions::ZONE_RECORD_STRIDE];
                rec.copy_from_slice(r);
                rec
            });
        } else {
            self.field_zone_record = None;
        }
    }

    /// Apply one field-VM `0x4C` outer-nibble-7 rectangular wall paint to
    /// the collision grid. `x_range` / `z_range` are the half-open tile
    /// spans the VM dispatcher already computed from the op operands; `sub`
    /// selects the per-byte high-nibble mutation:
    ///
    /// | sub | op |
    /// |---|---|
    /// | 0 | `byte &= 0x0F` (clear walls - make walkable) |
    /// | 1 | `byte |= 0xF0` (block all four sub-cells) |
    /// | 2 | `byte &= ~(mask << 4)` (clear selected wall bits) |
    /// | 3 | `byte |= (mask << 4)` (set selected wall bits) |
    ///
    /// Out-of-range tiles are skipped. The low nibble (floor-elevation
    /// tier) is preserved.
    /// Sample the field floor height at a world `(x, z)`, the port of
    /// `FUN_80019278`'s height branch (`ghidra/scripts/funcs/80019278.txt`).
    ///
    /// Retail keeps **two** floor models and picks between them per tile on the
    /// object-grid cell's [`CELL_ELEVATION_OVERRIDE`] (`0x800`) bit
    /// ([`Self::field_object_cells`]):
    ///
    /// - **Plain tiles** (bit clear) take the collision grid's low-nibble
    ///   elevation tier through [`Self::field_floor_height_lut`] and
    ///   **bilinearly interpolate** it across the `2x2` corner-tile block. The
    ///   tile is `(x >> 7, z >> 7)` (128-unit tiles); the sub-tile weights are
    ///   `x & 0x7F` / `z & 0x7F` (0..=127). When all four corner tiers match,
    ///   the LUT value returns directly (the retail fast path); otherwise the
    ///   four corner heights are weighted `top*(0x80-wz) + bottom*wz` (each edge
    ///   interpolated by `wx`) and divided by `0x4000` (`>> 14`, with the retail
    ///   `+0x3FFF` round-toward-zero on a negative accumulator).
    ///
    /// - **Ramp / stair tiles** (bit set) do **not** interpolate at all. Their
    ///   height is the *flat mean* of the four corner tiers (`sum >> 2`) plus
    ///   the tile's kind-2 [`ElevationOverride`] delta - a whole-tile step plus
    ///   a per-64-unit-sub-cell step, i.e. an authored staircase. A tile with
    ///   the bit but no record keeps just the mean. This is the model Rim Elm's
    ///   shore ramps are built from: their collision nibbles are sea-level `0`,
    ///   so interpolating them drops the player through the drawn stairs.
    ///
    /// The retail function's other `+0x8000` use - the world-map continent
    /// `0x1000` on-grid flag side effect on the entity's flag word - is not
    /// reproduced here. Returns `0` when the grid / LUT isn't loaded or the
    /// tile is out of range.
    ///
    /// PORT: FUN_80019278
    pub fn sample_field_floor_height(&self, world_x: i32, world_z: i32) -> i32 {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            return 0;
        }
        let tile_x = world_x >> 7;
        let tile_z = world_z >> 7;
        // The 2x2 block needs (tile_x+1, tile_z+1) in range.
        if tile_x < 0
            || tile_z < 0
            || tile_x as usize + 1 >= FIELD_GRID_STRIDE
            || tile_z as usize + 1 >= FIELD_GRID_STRIDE
        {
            return 0;
        }
        let base = tile_z as usize * FIELD_GRID_STRIDE + tile_x as usize;
        let g = &self.field_collision_grid;
        let lut = &self.field_floor_height_lut;
        // Low nibble = elevation tier; LUT-index it for each of the 4 corners.
        let c00 = (g[base] & 0x0F) as usize;
        let c01 = (g[base + 1] & 0x0F) as usize;
        let c10 = (g[base + FIELD_GRID_STRIDE] & 0x0F) as usize;
        let c11 = (g[base + FIELD_GRID_STRIDE + 1] & 0x0F) as usize;
        let (l00, l01, l10, l11) = (
            lut[c00] as i32,
            lut[c01] as i32,
            lut[c10] as i32,
            lut[c11] as i32,
        );
        // Ramp / stair tile: flat tile mean + the authored elevation override.
        if self.field_tile_has_elevation_override(tile_x, tile_z) {
            let mean = (l00 + l01 + l10 + l11) >> 2;
            let delta = crate::world::field_elevation::lookup_elevation_override(
                &self.field_elevation_overrides,
                tile_x as u8,
                tile_z as u8,
            )
            .map_or(0, |r| r.delta_at(world_x, world_z));
            return mean + delta;
        }
        if c00 == c01 && c00 == c10 && c00 == c11 {
            return l00;
        }
        let wx = world_x & 0x7F;
        let wz = world_z & 0x7F;
        let acc =
            (l01 * wx + l00 * (0x80 - wx)) * (0x80 - wz) + l10 * (0x80 - wx) * wz + l11 * wx * wz;
        if acc < 0 {
            (acc + 0x3FFF) >> 14
        } else {
            acc >> 14
        }
    }

    /// Does tile `(tile_x, tile_z)` carry the object-grid
    /// [`CELL_ELEVATION_OVERRIDE`] bit - i.e. is its floor an authored ramp /
    /// staircase rather than the bilinear nibble surface? `false` for scenes
    /// with no object grid loaded (every tile then reads as a plain tile).
    ///
    /// REF: FUN_80019278
    pub fn field_tile_has_elevation_override(&self, tile_x: i32, tile_z: i32) -> bool {
        if !(0..FIELD_GRID_STRIDE as i32).contains(&tile_x)
            || !(0..FIELD_GRID_STRIDE as i32).contains(&tile_z)
        {
            return false;
        }
        let idx = tile_z as usize * FIELD_GRID_STRIDE + tile_x as usize;
        self.field_object_cells
            .get(idx)
            .is_some_and(|c| c & CELL_ELEVATION_OVERRIDE != 0)
    }

    pub(crate) fn paint_field_collision(
        &mut self,
        sub: u8,
        x_range: (u8, u8),
        z_range: (u8, u8),
        mask: u8,
    ) {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            self.reset_field_collision_grid();
        }
        let hi = mask << 4;
        for row in z_range.0..z_range.1 {
            let row_base = (row as usize) * FIELD_GRID_STRIDE;
            for col in x_range.0..x_range.1 {
                let idx = row_base + col as usize;
                let Some(byte) = self.field_collision_grid.get_mut(idx) else {
                    continue;
                };
                match sub {
                    0 => *byte &= 0x0F,
                    1 => *byte |= 0xF0,
                    2 => *byte &= !hi,
                    3 => *byte |= hi,
                    _ => {}
                }
            }
        }
    }

    /// Sample the collision grid at world coords `(x, z)` and return `true`
    /// if the covering sub-cell is a wall.
    ///
    /// PORT: FUN_801cfe4c
    ///
    /// Single candidate-centre wall test against the `+0x4000` grid, using
    /// retail's exact sub-cell derivation: `zc = (z>>6)+2`,
    /// `xc = ((x+0x3f)>>6)-1`, tile column/row = `sub_cell >> 1` (rows of
    /// `0x80` bytes), wall bit = `byte >> 4 & quadrant_mask` with quadrant
    /// `(zc & 1) * 2 + (xc & 1)`.
    ///
    /// The `+2` Z bias and `ceil-1` X rounding are NOT optional look-ahead:
    /// the wall bits are authored with the bias baked in. This is proven by
    /// the `rimelm_wall_press_down` capture: the live player rests pressed
    /// against a wall at a position whose plain floor-indexed cell is an
    /// all-quads wall byte (the player could never legally stand there under
    /// floor indexing) while the biased read places that wall band one tile
    /// north, exactly where the on-screen wall blocks. The floor sampler
    /// ([`Self::sample_field_floor_height`], `FUN_80019278`) reads the SAME
    /// grid bytes with plain floor indexing - the low (elevation) and high
    /// (wall) nibbles of one byte are addressed under two different
    /// world-to-cell mappings by their two retail consumers. See
    /// `docs/subsystems/field-locomotion.md` ("Collision") and the
    /// disc-gated `engine-shell/tests/field_collision_discriminator.rs`.
    ///
    /// Retail tests **three leading-edge footprint probes** through this
    /// sampler (47-48 units ahead, ±16 lateral; per-direction table
    /// `DAT_801f2214` = `FIELD_WALL_PROBES`) - see
    /// [`World::field_dir_blocked`], wired into pad locomotion behind
    /// [`World::leading_edge_wall_probes`]. With the flag off, locomotion
    /// tests one candidate-centre point - a standoff/feel difference, not an
    /// indexing one.
    pub fn field_tile_is_wall(&self, x: i16, z: i16) -> bool {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            return false;
        }
        if x < 0 || z < 0 {
            return true; // off the grid origin reads as a wall (clamp inside)
        }
        let zc = ((z as i32) >> 6) + 2;
        let xc = (((x as i32) + 0x3F) >> 6) - 1;
        let col = (xc / 2) & 0x7F;
        let row = (zc - (zc >> 31)) >> 1;
        let idx = (col + row * FIELD_GRID_STRIDE as i32) as usize;
        let Some(&byte) = self.field_collision_grid.get(idx) else {
            return false;
        };
        let quad = ((zc & 1) << 1 | (xc & 1)) as u32;
        (byte >> 4) & (1u8 << quad) != 0
    }

    /// Is world `(x, z)` on the scene's authored **walkable floor** - i.e. does
    /// its plain (unbiased) `.MAP` object-grid cell carry the
    /// [`legaia_asset::field_objects::CELL_WALK_VISIBLE`] (`0x1000`) bit? This
    /// is the retail "player may stand on this tile" flag the field walk loader
    /// gates on ([`Self::field_object_cells`], plain `world >> 7` indexing -
    /// the same convention [`Self::sample_field_floor_height`] samples the floor
    /// under). `false` for out-of-range coords and for scenes with no object
    /// grid loaded (every tile then reads as off-floor void).
    pub fn field_tile_is_walk_visible(&self, x: i16, z: i16) -> bool {
        if x < 0 || z < 0 {
            return false;
        }
        let tx = (x as usize) >> 7;
        let tz = (z as usize) >> 7;
        if tx >= FIELD_GRID_STRIDE || tz >= FIELD_GRID_STRIDE {
            return false;
        }
        self.field_object_cells
            .get(tz * FIELD_GRID_STRIDE + tx)
            .is_some_and(|c| c & legaia_asset::field_objects::CELL_WALK_VISIBLE != 0)
    }

    /// Is world `(x, z)` a valid cold-entry standing spot - on the authored
    /// walkable floor ([`Self::field_tile_is_walk_visible`]) **and** clear of
    /// the collision-grid wall bits ([`Self::field_tile_is_wall`])?
    fn field_spawn_is_valid(&self, x: i16, z: i16) -> bool {
        self.field_tile_is_walk_visible(x, z) && !self.field_tile_is_wall(x, z)
    }

    /// Mean tile of every walk-visible
    /// ([`legaia_asset::field_objects::CELL_WALK_VISIBLE`]) floor cell, returned
    /// as its world-space centre. The seed for the cold-spawn nearest-floor
    /// search - it lands in the middle of the scene's playable region rather
    /// than at the fixed retail town01 seat. `None` for a scene with no
    /// walk-visible floor loaded.
    fn walk_visible_centroid(&self) -> Option<(i16, i16)> {
        let (mut sum_c, mut sum_r, mut n) = (0i64, 0i64, 0i64);
        for r in 0..FIELD_GRID_STRIDE {
            for c in 0..FIELD_GRID_STRIDE {
                if self
                    .field_object_cells
                    .get(r * FIELD_GRID_STRIDE + c)
                    .is_some_and(|w| w & legaia_asset::field_objects::CELL_WALK_VISIBLE != 0)
                {
                    sum_c += c as i64;
                    sum_r += r as i64;
                    n += 1;
                }
            }
        }
        if n == 0 {
            return None;
        }
        Some((
            ((sum_c / n) * 128 + 64) as i16,
            ((sum_r / n) * 128 + 64) as i16,
        ))
    }

    /// Expanding (Chebyshev-ring) search outward from the tile covering
    /// `(seed_x, seed_z)` for the nearest tile whose centre is a valid cold
    /// spawn ([`Self::field_spawn_is_valid`]). Returns that tile's world-space
    /// centre, or `None` if the whole grid has no valid tile.
    fn nearest_valid_field_spawn(&self, seed_x: i16, seed_z: i16) -> Option<(i16, i16)> {
        let stride = FIELD_GRID_STRIDE as i32;
        let stx = ((seed_x as i32) >> 7).clamp(0, stride - 1);
        let stz = ((seed_z as i32) >> 7).clamp(0, stride - 1);
        for radius in 0..stride {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    // Only the current ring's perimeter (nearest-first order).
                    if dx.abs().max(dz.abs()) != radius {
                        continue;
                    }
                    let (tx, tz) = (stx + dx, stz + dz);
                    if tx < 0 || tz < 0 || tx >= stride || tz >= stride {
                        continue;
                    }
                    let (wx, wz) = ((tx * 128 + 64) as i16, (tz * 128 + 64) as i16);
                    if self.field_spawn_is_valid(wx, wz) {
                        return Some((wx, wz));
                    }
                }
            }
        }
        None
    }

    /// Resolve a cold field-entry player spawn to an in-bounds, standable
    /// world `(x, z)`.
    ///
    /// Retail seats a cold (non-warp) field entry at the camera-window centre
    /// [`FIELD_COLD_SPAWN_XZ`] (`0xA40`); a cold entry only ever happens for the
    /// New Game opening (town01, Vahn's authored Rim Elm spawn), where that
    /// coordinate is a real standable tile. The engine's scene picker enters
    /// arbitrary scenes cold, and for most of them the fixed seat lands off the
    /// authored walkable floor (in a wall or in the surrounding void).
    ///
    /// This keeps the retail seat whenever it is valid - town01 and any other
    /// scene whose `0xA40` centre is genuinely on the walkable floor stay
    /// byte-identical - and otherwise relocates to the walkable tile nearest the
    /// scene's own playable-floor centroid ([`Self::walk_visible_centroid`]),
    /// so the player never spawns inside a wall or off-grid. Falls back to the
    /// retail seat when a scene supplies no walk-visible floor (nothing better
    /// to resolve against).
    ///
    /// Requires the collision grid ([`Self::load_field_collision_grid`]) and the
    /// object-grid cells ([`Self::load_field_object_cells`]) to be loaded first.
    pub fn resolve_cold_field_spawn(&self) -> (i16, i16) {
        let default = (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ);
        if self.field_spawn_is_valid(default.0, default.1) {
            return default;
        }
        let Some((cx, cz)) = self.walk_visible_centroid() else {
            return default;
        };
        self.nearest_valid_field_spawn(cx, cz).unwrap_or(default)
    }

    /// Retail's static-wall direction test: from the CURRENT position
    /// `(x, z)`, probe the three leading-edge points of `FIELD_WALL_PROBES`
    /// row `dir` (`0` = Z-, `1` = X-, `2` = Z+, `3` = X+) through
    /// [`Self::field_tile_is_wall`]; the direction is blocked when any probe
    /// lands on a wall sub-cell.
    ///
    /// PORT: FUN_801cfe4c
    /// REF: FUN_801cfc40
    ///
    /// This is the static-wall arm of `FUN_801cfe4c` (result bit `2`): the
    /// probes are taken at the player's pre-step position, so a step commits
    /// while the edge is still clear and the next step from the deeper
    /// position blocks - the player rests 47-48 units off the wall plane,
    /// step-exact (pinned by the `rimelm_wall_press_left`/`_down` captures).
    /// The actor-collision arm (result bits `1`/`4`) is
    /// [`Self::field_actor_dir_blocked`].
    pub fn field_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        FIELD_WALL_PROBES[dir & 3]
            .iter()
            .any(|&(dx, dz)| self.field_tile_is_wall(x.saturating_add(dx), z.saturating_sub(dz)))
    }

    /// Retail's actor-collision direction test: from the CURRENT position
    /// `(x, z)`, take the three probe points of `FIELD_ACTOR_PROBES` row
    /// `dir` (same `(x + dx, z - dz)` convention as the wall probes) and
    /// box-test each against every field NPC's position
    /// ([`Self::field_npc_positions`]); the direction is blocked when any
    /// probe lands within `FIELD_NPC_BOX_HALF` (40 units) of an NPC on
    /// both axes (strict).
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801cfe4c
    ///
    /// Covers both entity classes of `FUN_801cfc40`:
    ///
    /// - the **moving-actor arm** (result bit `1`) - the class village NPCs
    ///   belong to, capture-pinned by `rimelm_npc_press_tetsu` (the sparring
    ///   partner's `flags+0x10 = 0x08020884` carries the `0x20000` class
    ///   bit, and the mutual `+0x98` collision link is live in-frame). The
    ///   positions are LIVE: `Self::tick_field_npc_motions` walks routed /
    ///   scripted NPCs through the motion VM and writes back into
    ///   [`Self::field_npc_positions`], so a moving NPC's
    ///   ±`FIELD_NPC_BOX_HALF` (40) box follows it, exactly as retail
    ///   probes the live `+0x14`/`+0x18`.
    /// - the **static-entity arm** (result bit `4`) - placed `.MAP` props,
    ///   box ±`FIELD_PROP_BOX_HALF` (80) around the record-derived
    ///   footprint centre ([`Self::field_prop_colliders`]).
    ///
    /// The locomotion-path touch dispatches are modelled alongside: the
    /// button-press interact (facing probe + event + face-the-NPC,
    /// `Self::tick_field_interaction_probe`) and the no-button prop
    /// walk-touch event post (`Self::check_field_walk_touch`, the
    /// `FUN_801d5b5c` analogue for the decoded script classes). Not
    /// modelled: the mutual `+0x98` partner-link bookkeeping itself and the
    /// `_DAT_8007b6b8 == 0x20` full-table delegation to `FUN_801cf9f4`.
    /// Faithful quirk kept: the probe has no near-side clamp, so a position
    /// already deep inside a box (past the probe reach) reads clear -
    /// exactly as retail's forward-only probe behaves.
    pub fn field_actor_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        self.field_npc_dir_blocked(x, z, dir) || self.field_prop_dir_probe(x, z, dir).blocked
    }

    /// The **moving-NPC arm** of the actor-collision direction test (retail
    /// result bit `1` for the village-NPC class): the three probe points
    /// against every live NPC position at ±[`FIELD_NPC_BOX_HALF`].
    ///
    /// PORT: FUN_801cfc40
    pub(crate) fn field_npc_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        if self.field_npc_positions.is_empty() {
            return false;
        }
        FIELD_ACTOR_PROBES[dir & 3].iter().any(|&(dx, dz)| {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            self.field_npc_positions.values().any(|&(ax, az)| {
                (px - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (pz - az as i32).abs() < FIELD_NPC_BOX_HALF
            })
        })
    }

    /// The **placed-prop arms** of the actor-collision direction test: the
    /// three `FIELD_ACTOR_PROBES` points of `dir` box-tested against every
    /// solid prop collider. A static-class hit (`+0x10` clear of the
    /// `0x40020000` interact bits - retail contact result bit `4`) also
    /// surfaces the touched prop's bank anchor, which the locomotion
    /// auto-posts (`FUN_801D01B0` `0x801d0800` -> `FUN_801D5B5C`); an
    /// interact-class hit (bit `1`) blocks silently - only the button-gated
    /// facing probe fires it.
    ///
    /// Box classes per collider (see [`FieldPropCollider`]): static = ±80
    /// around the footprint centre; moving-box = ±40 around the live
    /// position. A non-solid collider (script ran `31 00`) is skipped
    /// entirely, exactly as `FUN_801CF754`'s `flags & 3` filter drops the
    /// opened door from the candidate list.
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801CF754, FUN_801D5B5C
    pub(crate) fn field_prop_dir_probe(&self, x: i16, z: i16, dir: usize) -> PropDirProbe {
        let mut out = PropDirProbe::default();
        if self.field_prop_colliders.is_empty() {
            return out;
        }
        for &(dx, dz) in &FIELD_ACTOR_PROBES[dir & 3] {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            for c in &self.field_prop_colliders {
                if !c.solid {
                    continue;
                }
                let ((cx, cz), half) = if c.moving_box {
                    (c.live, FIELD_NPC_BOX_HALF)
                } else {
                    (c.center, FIELD_PROP_BOX_HALF)
                };
                if (px - cx).abs() < half && (pz - cz).abs() < half {
                    out.blocked = true;
                    if !c.interact && out.touch.is_none() {
                        out.touch = c.anchor;
                    }
                }
            }
        }
        out
    }

    /// Retail's interact probe: from the player's position, take the single
    /// [`FIELD_FACING_PROBES`] compass point 64 units ahead along the
    /// current facing and return the NPC whose ±[`FIELD_INTERACT_BOX_HALF`]
    /// (72-unit) box contains it, if any.
    ///
    /// PORT: FUN_801cf9f4
    /// REF: FUN_801d01b0
    ///
    /// The engine's field heading ([`decode_field_direction`]
    /// (Self::decode_field_direction)) stores `0` = Z+ while the retail
    /// facing byte stores `0` = Z- (a Z+ walk writes `0x800` to `+0x26`), so
    /// the sector index adds the half-turn before quantising. On overlapping
    /// NPC boxes retail keeps the *last* actor-list hit (the `+0x98` link is
    /// overwritten per match); the engine's NPC set is a hash map with no
    /// list order, so it picks the hit nearest the probe point instead
    /// (tie-break: lowest slot) - identical whenever NPCs stand more than
    /// 144 units apart, which every authored placement does.
    pub(crate) fn field_interact_probe_slot(&self) -> Option<u8> {
        let slot = self.player_actor_slot? as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return None;
        }
        let ms = &self.actors[slot].move_state;
        let (x, z) = (ms.world_x, ms.world_z);
        let sector = (((ms.render_26 as i32 + 0x800) & 0xfff) >> 9) as usize;
        let (dx, dz) = FIELD_FACING_PROBES[sector];
        let px = x.saturating_add(dx) as i32;
        let pz = z.saturating_sub(dz) as i32;
        let mut best: Option<(i32, u8)> = None;
        for (&npc_slot, &(ax, az)) in &self.field_npc_positions {
            let (ex, ez) = ((px - ax as i32).abs(), (pz - az as i32).abs());
            if ex < FIELD_INTERACT_BOX_HALF && ez < FIELD_INTERACT_BOX_HALF {
                let d = ex * ex + ez * ez;
                if best.is_none_or(|(bd, bs)| d < bd || (d == bd && npc_slot < bs)) {
                    best = Some((d, npc_slot));
                }
            }
        }
        best.map(|(_, s)| s)
    }

    /// Turn the player toward field NPC `npc_slot` (retail's face-the-NPC
    /// step after a successful interact probe: `func_0x80019b28` computes
    /// the 12-bit angle from the touched actor to the player and stores it
    /// in the player's `+0x26`). The engine computes the same angle with
    /// float `atan2` in its own heading convention (`0` = Z+) rather than
    /// retail's arctan LUT at `0x8006f4c8`, so it is shape-faithful, not
    /// bit-exact - the value only feeds the heading marker and the next
    /// probe's 45° sector quantisation.
    ///
    /// REF: FUN_80019b28
    pub(crate) fn face_field_npc(&mut self, npc_slot: u8) {
        let Some(&(nx, nz)) = self.field_npc_positions.get(&npc_slot) else {
            return;
        };
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() {
            return;
        }
        let ms = &mut self.actors[slot].move_state;
        let (dx, dz) = (
            (nx as i32 - ms.world_x as i32) as f32,
            (nz as i32 - ms.world_z as i32) as f32,
        );
        if dx == 0.0 && dz == 0.0 {
            return;
        }
        ms.render_26 =
            ((dx.atan2(dz) / std::f32::consts::TAU * 4096.0).round() as i32 & 0x0FFF) as i16;
    }

    /// Seed each placed field NPC's **initial facing** from its MAN spawn
    /// prologue, so a never-walked NPC stands with its retail heading instead
    /// of the unrotated default.
    ///
    /// Retail applies these at scene load: the placement installer
    /// `FUN_8003A1E4` pre-runs the record's `0x24`/`0x25`-marked prologue
    /// through the field VM, and the prologue's `0x4C 0x51` / `0x38`
    /// (simple-path) ops write the actor's `+0x26` heading from the
    /// 8-direction LUT at SCUS `0x80073F04`
    /// ([`crate::man_field_scripts::placement_initial_facing`]). The engine
    /// derives the same LUT index statically per placement and stores the
    /// converted 12-bit engine heading (`0` = Z+;
    /// [`crate::man_field_scripts::facing_index_to_engine_heading`]) in
    /// [`Self::field_npc_headings`] - the map every NPC draw reads. A later
    /// walk overwrites the slot exactly as retail's per-step facing writes
    /// overwrite `+0x26`, and an already-present heading (a scripted channel
    /// move that ran first) is kept.
    ///
    /// Call after [`Self::install_field_carriers_from_man`] (whose inner
    /// install clears `field_npc_headings`).
    // PORT: FUN_8003A1E4 (spawn prologue pre-run -> +0x26 facing writes)
    // REF: FUN_801DE840, FUN_801d01b0 (heading-space convention)
    pub fn seed_field_npc_facings(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        for p in man_file.actor_placements(man) {
            let Ok(slot) = u8::try_from(p.index) else {
                continue;
            };
            let Some(idx) = crate::man_field_scripts::placement_initial_facing(man_file, man, &p)
            else {
                continue;
            };
            let Some(heading) = crate::man_field_scripts::facing_index_to_engine_heading(idx)
            else {
                continue;
            };
            self.field_npc_headings.entry(slot).or_insert(heading);
        }
    }

    /// Start a field NPC walking to world `(tx, tz)` through the motion VM -
    /// the engine's start-motion kernel for the MAN-placed actor set. Mirrors
    /// the retail start shape: write the walk target onto the actor and reset
    /// the glide state so the per-frame motion stepper picks it up (retail's
    /// `FUN_800358c0` writes the target into the actor `+0xA`/`+0xC` + subobj
    /// mirrors and clears the `+0x20` glide cursor; the per-frame consumer is
    /// the motion VM `FUN_8003774C`, ported in
    /// [`legaia_engine_vm::motion_vm`]). Returns `false` (and does nothing)
    /// when `slot` is not an installed field NPC - the retail actor-list
    /// search miss, which returns 0.
    ///
    /// A leg started here is *scripted* (`route_cursor = None`): it runs even
    /// while [`Self::animate_field_npcs`] is off and even during a dialogue
    /// (the interaction partner executing its own prologue walk), and ends
    /// where it lands.
    ///
    /// REF: FUN_800358c0, FUN_8003774C
    pub fn start_field_npc_motion(&mut self, slot: u8, tx: i16, tz: i16) -> bool {
        let Some(&(cx, cz)) = self.field_npc_positions.get(&slot) else {
            return false;
        };
        // Faithful glide speed: the placement's own `0x4C 0x51` motion-op
        // base step (retail `FUN_8003774C` `4 << bits`), derived at scene load
        // into `field_npc_glide_speeds`; the stand-in `FIELD_NPC_MOTION_SPEED`
        // is the fallback for a placement with no decodable motion leg.
        let speed = self
            .field_npc_glide_speeds
            .get(&slot)
            .copied()
            .unwrap_or(FIELD_NPC_MOTION_SPEED);
        self.field_npc_motions.insert(
            slot,
            FieldNpcMotion {
                state: vm::motion_vm::MotionState {
                    world_x: cx,
                    world_y: 0,
                    world_z: cz,
                    speed,
                    yaw: 0,
                    op_accum: 0,
                    pc: 0,
                },
                target: (tx, tz),
                route_cursor: None,
            },
        );
        true
    }

    /// Step every in-flight field-NPC walk leg one frame through the ported
    /// motion VM and kick autonomous route legs, writing each NPC's new
    /// position back into [`Self::field_npc_positions`] - so the moving NPC's
    /// ±40-unit collision box ([`Self::field_actor_dir_blocked`]) and its
    /// interact box ([`Self::field_interact_probe_slot`]) follow the live
    /// position, exactly as retail probes the live `+0x14`/`+0x18` rather
    /// than the spawn anchor.
    ///
    /// Autonomous legs (started from [`Self::field_npc_routes`], gated by
    /// [`Self::animate_field_npcs`]) loop their waypoints - a patrol - and
    /// pause while a dialogue is up (retail's interaction motion-pause kick:
    /// the touch event post reloads every moving-class actor's pause timer,
    /// `FUN_8003c9ac`). Scripted legs (interaction-prologue `0x4C 0x51`,
    /// actor-VM `start_motion`) keep stepping through a dialogue - they ARE
    /// the interaction's choreography.
    ///
    /// REF: FUN_8003774C, FUN_8003c9ac
    pub(crate) fn tick_field_npc_motions(&mut self) {
        // A running cutscene timeline owns the stage: its per-actor channels
        // ([`Self::step_field_channels`]) drive NPC moves, so the engine's
        // autonomous waypoint substitute stands down (it would overwrite the
        // scripted positions each frame).
        if self.cutscene_timeline_active() {
            return;
        }
        let dialogue_up = self.current_dialog.is_some() || self.inline_dialogue.is_some();
        // Kick autonomous legs for routed NPCs with no in-flight motion.
        if self.animate_field_npcs && !dialogue_up {
            let kicks: Vec<(u8, (i16, i16))> = self
                .field_npc_routes
                .iter()
                .filter(|(slot, _)| !self.field_npc_motions.contains_key(slot))
                .filter_map(|(&slot, route)| {
                    let first = *route.first()?;
                    // A one-waypoint route that has arrived stays put (no
                    // restart churn); multi-waypoint routes always loop.
                    if route.len() == 1 && self.field_npc_positions.get(&slot) == Some(&first) {
                        return None;
                    }
                    Some((slot, first))
                })
                .collect();
            for (slot, (tx, tz)) in kicks {
                if self.start_field_npc_motion(slot, tx, tz)
                    && let Some(m) = self.field_npc_motions.get_mut(&slot)
                {
                    m.route_cursor = Some(0);
                }
            }
        }
        // Step each leg; collect per-slot outcomes, then apply.
        let slots: Vec<u8> = self.field_npc_motions.keys().copied().collect();
        for slot in slots {
            let Some(motion) = self.field_npc_motions.get_mut(&slot) else {
                continue;
            };
            if dialogue_up && motion.route_cursor.is_some() {
                continue; // autonomous legs pause during an interaction
            }
            let target = vm::motion_vm::MotionTarget {
                x: motion.target.0,
                y: 0,
                z: motion.target.1,
                id: 0,
            };
            let result = vm::motion_vm::step(&mut motion.state, target, &FIELD_NPC_MOTION_PROGRAM);
            let pos = (motion.state.world_x, motion.state.world_z);
            let cursor = motion.route_cursor;
            // Track the walker's heading from the step direction (12-bit,
            // `0` = Z+ - the same convention the player's `render_26`
            // carries); an unmoved step keeps the previous facing.
            if let Some(&(px, pz)) = self.field_npc_positions.get(&slot) {
                let (dx, dz) = ((pos.0 - px) as f32, (pos.1 - pz) as f32);
                if dx != 0.0 || dz != 0.0 {
                    let heading = ((dx.atan2(dz) / std::f32::consts::TAU * 4096.0).round() as i32
                        & 0x0FFF) as i16;
                    self.field_npc_headings.insert(slot, heading);
                }
            }
            self.field_npc_positions.insert(slot, pos);
            if result == vm::motion_vm::StepResult::Done {
                match cursor {
                    // Patrol loop: start the next route leg (wrapping).
                    Some(i) => {
                        let next = self
                            .field_npc_routes
                            .get(&slot)
                            .filter(|route| route.len() > 1)
                            .map(|route| ((i + 1) % route.len(), route[(i + 1) % route.len()]));
                        match next {
                            Some((ni, (tx, tz))) => {
                                if self.start_field_npc_motion(slot, tx, tz)
                                    && let Some(m) = self.field_npc_motions.get_mut(&slot)
                                {
                                    m.route_cursor = Some(ni);
                                }
                            }
                            None => {
                                self.field_npc_motions.remove(&slot);
                            }
                        }
                    }
                    // Scripted leg: ends where it lands.
                    None => {
                        self.field_npc_motions.remove(&slot);
                    }
                }
            }
        }
    }

    /// The locomotion's per-step **walk-touch dispatch**: when the player's
    /// body stands inside a walk-touch placement's static contact box
    /// (±[`FIELD_PROP_BOX_HALF`]), post that placement's event - no button
    /// press, the same dispatch path the button-gated interact uses
    /// ([`Self::trigger_field_interact`]) plus the decoded script effect:
    ///
    /// - [`WalkTouchEvent::Warp`] → queue the door-warp scene transition
    ///   (the effect of the record's `0x3E` op through the host's
    ///   `scene_transition` path);
    /// - [`WalkTouchEvent::PlayerMoveTo`] → snap the player to the decoded
    ///   world coords (the record's cross-context `0x23` into the player
    ///   channel) and surface a [`FieldEvent::MoveTo`].
    ///
    /// Retail posts the touch event (`FUN_801d5b5c`) on every contact step,
    /// gated by the player's `+0x10 & 0x80000` engaged flag until the dialog
    /// SM teardown clears it; the engine latches one post per contact
    /// ([`Self::active_walk_touch`]) instead. The full post kernel (engaged
    /// flag, facing save/restore, touch counters) is not modelled.
    ///
    /// REF: FUN_801d5b5c, FUN_801cfc40
    fn check_field_walk_touch(&mut self) {
        if self.field_walk_touch.is_empty() {
            self.active_walk_touch = None;
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        let (px, pz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        // Contact fires from the SAME probe points that block movement
        // (retail: `FUN_801cfe4c`'s three `FUN_801cfc40` calls both refuse
        // the step and link/post the touched actor) - so a **solid** door
        // object still fires its walk-touch while the player stands pressed
        // against its box, 64+ units short of the centre. The stand-inside
        // test is kept as well (a landing seated inside a box, nav drivers).
        let mut points: Vec<(i32, i32)> = vec![(px as i32, pz as i32)];
        for dir in Self::dirs_of_bits(self.last_move_dir_bits) {
            for &(dx, dz) in &FIELD_ACTOR_PROBES[dir] {
                points.push((px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32));
            }
        }
        let hit = self
            .field_walk_touch
            .iter()
            .find(|(_, ((wx, wz), _))| {
                points.iter().any(|&(qx, qz)| {
                    (qx - *wx as i32).abs() < FIELD_PROP_BOX_HALF
                        && (qz - *wz as i32).abs() < FIELD_PROP_BOX_HALF
                })
            })
            .map(|(&s, &(_, event))| (s, event));
        let Some((touch_slot, event)) = hit else {
            self.active_walk_touch = None;
            return;
        };
        if self.active_walk_touch == Some(touch_slot) {
            return; // still inside the same contact - already posted
        }
        self.active_walk_touch = Some(touch_slot);
        // A door record is a field-VM script, not a constant: its opening
        // `SysFlag.Test` chain picks which arm runs (teleport into the
        // interior vs. spawn the story beat). Retail resumes the record on
        // contact, so the arm is chosen against the *live* flags - re-resolve
        // here rather than reusing the load-time structural decode. Falls back
        // to that decode when the record can't be re-walked.
        // REF: FUN_801d5b5c (contact resumes the object's script)
        let event = self
            .field_walk_touch_records
            .get(&touch_slot)
            .copied()
            .and_then(|record| {
                let man = self.field_channels_man.clone()?;
                let man_file = legaia_asset::man_section::parse(&man).ok()?;
                let flags = self.system_flags.clone();
                let test = |idx: u16| -> bool {
                    let byte = usize::from(idx >> 3);
                    byte < flags.len() && flags[byte] & (0x80u8 >> (idx & 7)) != 0
                };
                crate::man_field_scripts::resolve_walk_touch_event(&man_file, &man, record, &test)
            })
            .unwrap_or(event);
        // Post through the same dispatch path the button-gated interact uses.
        self.trigger_field_interact(0, touch_slot);
        match event {
            WalkTouchEvent::Warp { target_map } => {
                self.pending_scene_transition = Some(target_map);
            }
            WalkTouchEvent::PlayerMoveTo {
                world_x,
                world_z,
                facing,
            } => {
                // The retail op-0x23 player arm rewrites X/Z (`+0x14`/`+0x18`)
                // and re-seats the actor on the floor; the paired op-0x38
                // cross-context CAM_CFG writes the arrival heading (`+0x26`).
                // A door's interior is a *sub-area of the same collision grid*
                // at its own elevation, so the floor must be resampled at the
                // landing - otherwise the player keeps the doorstep's outdoor
                // height until the next locomotion frame nudges it.
                let y = self.sample_field_floor_height(world_x as i32, world_z as i32) as i16;
                if let Some(p) = self.player_actor_slot
                    && let Some(actor) = self.actors.get_mut(p as usize)
                {
                    actor.move_state.world_x = world_x;
                    actor.move_state.world_z = world_z;
                    actor.move_state.world_y = y;
                    if let Some(heading) = facing {
                        actor.move_state.render_26 = heading;
                    }
                }
                self.pending_field_events.push(FieldEvent::MoveTo {
                    world_x: world_x as u16,
                    world_z: world_z as u16,
                    is_player: true,
                });
            }
            // Boss-stager contact: the `trigger_field_interact` call above
            // already ran the placement's record ([`crate::world::World::
            // run_boss_stager_record`]); the event carries no extra effect.
            WalkTouchEvent::StagerBeat => {}
            // The record's taken arm is an op-`0x44` SPAWN_RECORD: queue the
            // referenced record so `SceneHost::tick` installs it as a spawned
            // field-VM context (the same drain the in-script op-`0x44` uses).
            // This is the arm a story-gated door takes once its flag is set -
            // the in-house beat, not a bare reposition.
            WalkTouchEvent::SpawnRecord { flat_index } => {
                if let Ok(idx) = u8::try_from(flat_index) {
                    self.pending_record_spawns.push(idx);
                }
            }
        }
    }

    /// Actor-VM glide start (`MotionAt` / `EffectMotion` → `start_motion`,
    /// retail `FUN_800358c0`): record the target on the actor and install a
    /// motion-VM leg gliding the actor's sprite position
    /// (`move_state.world_x` / `world_y`) toward it, stepped once per tick by
    /// [`Self::tick_actor_motions`]. The retail kernel writes the target into
    /// the actor `+0xA`/`+0xC` and its subobj mirrors and clears the `+0x20`
    /// glide cursor; the per-frame glide is the motion-VM pursue step.
    ///
    /// REF: FUN_800358c0
    pub(crate) fn start_actor_motion(&mut self, actor_id: u8, target: ActorVmPosition) {
        let Some(actor) = self.actors.get(actor_id as usize) else {
            return;
        };
        if !actor.active {
            return;
        }
        let (cx, cy) = (actor.move_state.world_x, actor.move_state.world_y);
        self.actors[actor_id as usize].motion_target = Some(target);
        self.actor_motions.insert(
            actor_id,
            FieldNpcMotion {
                state: vm::motion_vm::MotionState {
                    world_x: cx,
                    world_y: 0,
                    // The sprite-actor glide runs in the actor VM's packed
                    // (x, y) plane; the motion VM's XZ pursue step maps
                    // y → z here.
                    world_z: cy,
                    speed: FIELD_NPC_MOTION_SPEED,
                    yaw: 0,
                    op_accum: 0,
                    pc: 0,
                },
                target: (target.x, target.y),
                route_cursor: None,
            },
        );
    }

    /// Step every actor-VM glide ([`Self::start_actor_motion`]) one frame
    /// through the motion VM, writing back into the actor's `move_state`.
    /// Finished or stale (despawned-actor) glides are dropped.
    ///
    /// REF: FUN_8003774C
    pub(crate) fn tick_actor_motions(&mut self) {
        if self.actor_motions.is_empty() {
            return;
        }
        let slots: Vec<u8> = self.actor_motions.keys().copied().collect();
        for slot in slots {
            let alive = self
                .actors
                .get(slot as usize)
                .is_some_and(|actor| actor.active);
            if !alive {
                self.actor_motions.remove(&slot);
                continue;
            }
            let Some(motion) = self.actor_motions.get_mut(&slot) else {
                continue;
            };
            let target = vm::motion_vm::MotionTarget {
                x: motion.target.0,
                y: 0,
                z: motion.target.1,
                id: 0,
            };
            let result = vm::motion_vm::step(&mut motion.state, target, &FIELD_NPC_MOTION_PROGRAM);
            let (nx, ny) = (motion.state.world_x, motion.state.world_z);
            let actor = &mut self.actors[slot as usize];
            actor.move_state.world_x = nx;
            actor.move_state.world_y = ny;
            if result == vm::motion_vm::StepResult::Done {
                self.actor_motions.remove(&slot);
            }
        }
    }

    /// Decode this frame's held d-pad into a camera-relative movement
    /// direction and an 8-direction heading angle. Returns
    /// `(dir_bits, heading)` where `dir_bits` uses the retail post-remap
    /// convention (`0x1000` = Z+, `0x4000` = Z-, `0x2000` = X+, `0x8000` =
    /// X-) and `heading` is a PSX 12-bit angle (`4096` = full turn).
    /// `dir_bits == 0` means no direction is held.
    ///
    /// The raw screen direction (up / down / left / right) is remapped by
    /// [`World::field_camera_azimuth`] quantised to the nearest 90° so
    /// "screen up" always walks away from the camera, the same job
    /// `func_0x800467e8` does in retail.
    fn decode_field_direction(&self) -> (u16, i16) {
        let up = self.input.pressed(input::PadButton::Up);
        let down = self.input.pressed(input::PadButton::Down);
        let left = self.input.pressed(input::PadButton::Left);
        let right = self.input.pressed(input::PadButton::Right);

        // Screen-space delta: +Y forward (away from camera), +X right.
        let mut sx: i32 = 0;
        let mut sy: i32 = 0;
        if up {
            sy += 1;
        }
        if down {
            sy -= 1;
        }
        if right {
            sx += 1;
        }
        if left {
            sx -= 1;
        }
        if sx == 0 && sy == 0 {
            return (0, 0);
        }

        // Quantise the camera azimuth to one of four cardinal rotations and
        // rotate the screen delta into world space. quadrant 0 = identity
        // (screen-up -> +Z, screen-right -> +X).
        let quadrant = (((self.field_camera_azimuth as u32) + 512) / 1024) & 3;
        let (mut wx, mut wz) = match quadrant {
            0 => (sx, sy),
            1 => (sy, -sx),
            2 => (-sx, -sy),
            _ => (-sy, sx),
        };
        wx = wx.clamp(-1, 1);
        wz = wz.clamp(-1, 1);

        let mut bits = 0u16;
        if wz > 0 {
            bits |= 0x1000; // Z+
        } else if wz < 0 {
            bits |= 0x4000; // Z-
        }
        if wx > 0 {
            bits |= 0x2000; // X+
        } else if wx < 0 {
            bits |= 0x8000; // X-
        }

        // Heading: atan2(wx, wz) in 12-bit units. Z+ = 0, X+ = quarter turn.
        let heading = (((wx as f32).atan2(wz as f32) / std::f32::consts::TAU * 4096.0).round()
            as i32
            & 0x0FFF) as i16;
        (bits, heading)
    }

    /// Free-movement locomotion step - the engine-side port of
    /// `FUN_801d01b0` (field overlay `overlay_0897`).
    ///
    /// PORT: FUN_801d01b0
    ///
    /// Reads this frame's
    /// pad, turns it into a camera-relative direction + facing, and
    /// advances the player actor in 2-unit increments with per-axis
    /// collision against [`World::field_collision_grid`].
    ///
    /// No-ops when there is no player actor, while a dialog box is up (the
    /// field VM owns the frame), while the tile-board minigame is installed
    /// (that mode runs its own digital stepper), or while the player's
    /// movement-disabled flag (`+0x10 & 0x80000`) is set (encounter queued
    /// / cutscene owns the player). Reads only pad bits + grid + actor
    /// state, so it is deterministic across identical pad streams.
    pub fn step_field_locomotion(&mut self) {
        if self.current_dialog.is_some() || self.tile_board.is_some() {
            return;
        }
        // Lock pad-driven locomotion while an opening-cutscene timeline owns
        // the scene (the establishing camera sweep + name-entry). During the
        // sweep the script drives the lead actor through its own MoveTo ops;
        // the pad must not also walk the player out from under the cinematic
        // camera. Releases the frame the timeline drops (matches retail, where
        // free-roam control returns only after the opening choreography ends).
        if self.cutscene_timeline_active() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        if self.actors[slot].move_state.flags & 0x0008_0000 != 0 {
            return;
        }

        let (dir_bits, heading) = self.decode_field_direction();
        self.last_move_dir_bits = dir_bits;
        if dir_bits == 0 {
            return;
        }
        self.actors[slot].move_state.render_26 = heading;

        // speed = ((base_step * player[+0x72]) >> 12) * DAT_1f800393.
        let mult = self.actors[slot].move_state.field_72 as i32;
        let ratio = self.move_ramp_ratio.max(1) as i32;
        let mut speed = ((FIELD_BASE_STEP * mult) >> 12) * ratio;
        // Diagonal normalise (camera mode 4, both axes pressed): x0.75.
        let z_pressed = dir_bits & 0x5000 != 0;
        let x_pressed = dir_bits & 0xA000 != 0;
        if z_pressed && x_pressed {
            speed -= speed >> 2;
        }
        if speed <= 0 {
            return;
        }

        // A held direction is a movement frame for the locomotion animation
        // even when the step is wall-blocked (retail walks in place).
        if let Some(anim) = &mut self.field_player_anim {
            anim.moved_this_frame = true;
        }

        self.advance_with_collision(slot, dir_bits, speed);

        // Walk-touch dispatch (retail: the per-sub-step touch check inside
        // `FUN_801d01b0`, posting `FUN_801d5b5c` on a static-entity contact
        // with no button press): post a touched placement's walk-touch event.
        self.check_field_walk_touch();

        // Terrain follow (gated): after the X/Z step commits, snap the
        // player's Y to the per-scene floor elevation at the new tile. Done
        // here rather than inside the shared `advance_with_collision` so the
        // world-map walk path (which collides through the same routine but
        // derives height from the continent grid) is unaffected. No-op height
        // 0 until a scene supplies a floor LUT.
        if self.follow_terrain_height {
            let (x, z) = {
                let ms = &self.actors[slot].move_state;
                (ms.world_x as i32, ms.world_z as i32)
            };
            let y = self.sample_field_floor_height(x, z);
            self.actors[slot].move_state.world_y = y as i16;
        }
    }

    /// Advance actor `slot` by `speed` world units in the direction encoded by
    /// `dir_bits` (post-remap convention: `0x1000`=Z+, `0x4000`=Z-,
    /// `0x2000`=X+, `0x8000`=X-), stepping `FIELD_STEP_UNIT` at a time and
    /// committing only the axes that stay off a wall in
    /// [`World::field_collision_grid`]. X collision uses the just-committed Z
    /// so a diagonal move can't tunnel through a wall corner.
    ///
    /// Shared by [`Self::step_field_locomotion`] and
    /// `Self::step_world_map_locomotion`: retail `FUN_801d01b0` is the same
    /// routine in both the field and world-map-walk overlays, and both collide
    /// against the same `_DAT_1f8003ec + 0x4000` walkability grid.
    ///
    /// With [`Self::leading_edge_wall_probes`] set, each axis instead blocks
    /// on retail's three-probe leading-edge footprint taken at the CURRENT
    /// position ([`Self::field_dir_blocked`]) - the retail standoff - and
    /// commits the step whenever the edge is clear. The default candidate-
    /// centre test is kept (off-flag) for the locomotion oracles and the
    /// BFS nav drivers. With [`Self::solid_field_npcs`] set, each axis
    /// additionally blocks when the direction's actor-collision probes land
    /// inside a field NPC's body box ([`Self::field_npc_dir_blocked`]) -
    /// retail gates a step on the actor bits and the wall bit together
    /// (`FUN_801cfe4c` returning any of `1`/`2`/`4` refuses the 2-unit step).
    ///
    /// **Placed props block unconditionally** ([`Self::field_prop_dir_probe`]):
    /// retail's placed-object actors always sit in the collision candidate
    /// list (`FUN_801CF754`), so a closed door is solid until its touch pass
    /// runs `31 00`. A static-class prop hit also records the touched prop
    /// into [`Self::pending_prop_touch`] - the same probe both refuses the
    /// step and posts the touch (`FUN_801D01B0`'s bit-`4` auto-post of
    /// `FUN_801D5B5C`).
    pub fn advance_with_collision(&mut self, slot: usize, dir_bits: u16, speed: i32) {
        let edge = self.leading_edge_wall_probes;
        let solid_npcs = self.solid_field_npcs;
        let mut remaining = speed;
        while remaining > 0 {
            let ms = &self.actors[slot].move_state;
            let (cx, cz) = (ms.world_x, ms.world_z);
            // Z axis.
            if dir_bits & 0x1000 != 0 {
                let nz = cz.saturating_add(FIELD_STEP_UNIT as i16);
                let prop = self.probe_props_for_step(cx, cz, 2);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz, 2)
                } else {
                    self.field_tile_is_wall(cx, nz)
                } || (solid_npcs && self.field_npc_dir_blocked(cx, cz, 2))
                    || prop;
                if !blocked {
                    self.actors[slot].move_state.world_z = nz;
                }
            } else if dir_bits & 0x4000 != 0 {
                let nz = cz.saturating_sub(FIELD_STEP_UNIT as i16);
                let prop = self.probe_props_for_step(cx, cz, 0);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz, 0)
                } else {
                    self.field_tile_is_wall(cx, nz)
                } || (solid_npcs && self.field_npc_dir_blocked(cx, cz, 0))
                    || prop;
                if !blocked {
                    self.actors[slot].move_state.world_z = nz;
                }
            }
            // X axis (re-read X in case Z committed; X collision uses the
            // committed Z so footprints don't tunnel diagonally).
            let cz2 = self.actors[slot].move_state.world_z;
            if dir_bits & 0x2000 != 0 {
                let nx = cx.saturating_add(FIELD_STEP_UNIT as i16);
                let prop = self.probe_props_for_step(cx, cz2, 3);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz2, 3)
                } else {
                    self.field_tile_is_wall(nx, cz2)
                } || (solid_npcs && self.field_npc_dir_blocked(cx, cz2, 3))
                    || prop;
                if !blocked {
                    self.actors[slot].move_state.world_x = nx;
                }
            } else if dir_bits & 0x8000 != 0 {
                let nx = cx.saturating_sub(FIELD_STEP_UNIT as i16);
                let prop = self.probe_props_for_step(cx, cz2, 1);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz2, 1)
                } else {
                    self.field_tile_is_wall(nx, cz2)
                } || (solid_npcs && self.field_npc_dir_blocked(cx, cz2, 1))
                    || prop;
                if !blocked {
                    self.actors[slot].move_state.world_x = nx;
                }
            }
            remaining -= FIELD_STEP_UNIT;
        }
    }

    /// One movement sub-step's prop probe: blocks on any solid prop box hit
    /// and latches a static-class touch into [`Self::pending_prop_touch`]
    /// (drained by [`Self::tick_prop_interactions`]). Returns whether the
    /// step is prop-blocked.
    fn probe_props_for_step(&mut self, x: i16, z: i16, dir: usize) -> bool {
        let probe = self.field_prop_dir_probe(x, z, dir);
        if let Some(anchor) = probe.touch
            && self.pending_prop_touch.is_none()
        {
            self.pending_prop_touch = Some(anchor);
        }
        probe.blocked
    }

    /// Step the player one navigation frame toward world position `(tx, tz)`,
    /// using the same per-axis field collision as pad locomotion
    /// ([`Self::advance_with_collision`]) but a world-space direction. Returns
    /// `true` once the player is within `tol` units of the target on both axes.
    ///
    /// This is the auto-navigation primitive a driver loops (following a path of
    /// waypoints) to walk the player to a target - e.g. the v0.1 oracle walking
    /// from the cold-boot spawn to the sparring partner before talking to it.
    /// It drives the real locomotion stepping/collision, just without the pad →
    /// camera-relative remap. No-op without an active player actor.
    pub fn nav_step_toward(&mut self, tx: i16, tz: i16, tol: i16) -> bool {
        let Some(slot) = self.player_actor_slot else {
            return false;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return false;
        }
        let (cx, cz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        if (cx - tx).abs() <= tol && (cz - tz).abs() <= tol {
            return true;
        }
        let mut dir = 0u16;
        let (mut wx, mut wz) = (0i32, 0i32);
        if tz > cz {
            dir |= 0x1000; // Z+
            wz = 1;
        } else if tz < cz {
            dir |= 0x4000; // Z-
            wz = -1;
        }
        if tx > cx {
            dir |= 0x2000; // X+
            wx = 1;
        } else if tx < cx {
            dir |= 0x8000; // X-
            wx = -1;
        }
        if dir != 0 {
            self.last_move_dir_bits = dir;
            // Walking sets the heading, exactly as the pad path does (retail
            // locomotion writes the facing every moved frame) - so a nav walk
            // leaves the player facing its travel direction and the interact
            // probe ([`Self::field_interact_probe_slot`]) sees the same state
            // a pad walk would produce.
            self.actors[slot].move_state.render_26 =
                (((wx as f32).atan2(wz as f32) / std::f32::consts::TAU * 4096.0).round() as i32
                    & 0x0FFF) as i16;
            // A nav step is a movement frame for the locomotion animation,
            // same as a held pad direction.
            if let Some(anim) = &mut self.field_player_anim {
                anim.moved_this_frame = true;
            }
            self.advance_with_collision(slot, dir, FIELD_BASE_STEP);
        }
        false
    }
}
