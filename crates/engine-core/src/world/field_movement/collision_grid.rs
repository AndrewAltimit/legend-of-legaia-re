//! The field collision grid: load and reset, object cells, elevation overrides,
//! region tables, floor-height sampling and the per-tile wall / walk-visible
//! queries.

use super::*;
use legaia_engine_field::field_elevation::CELL_ELEVATION_OVERRIDE;
use legaia_engine_mode::mode::SceneMode;

impl World {
    // --- field collision grid + free-movement locomotion ----------------

    /// The `FIELD_ACTOR_PROBES` row indices of the directions held in a
    /// post-remap `dir_bits` word (`0x1000` = Z+ -> row 2, `0x4000` = Z- ->
    /// row 0, `0x2000` = X+ -> row 3, `0x8000` = X- -> row 1).
    pub(crate) fn dirs_of_bits(dir_bits: u16) -> impl Iterator<Item = usize> {
        [(0x1000u16, 2usize), (0x4000, 0), (0x2000, 3), (0x8000, 1)]
            .into_iter()
            .filter_map(move |(bit, dir)| (dir_bits & bit != 0).then_some(dir))
    }

    /// Reset the per-scene field collision grid to "all walkable" (every
    /// byte zero). Called at field entry; the scene prescript repaints the
    /// wall bits via the field-VM `0x4C` outer-nibble-7 op.
    ///
    /// This is a **pre-load scrub, not a model of retail**. Retail does not
    /// author the grid from the script: the base wall + floor data is an
    /// on-disc blob (the `+0x4000..+0x8000` region of `DATA\FIELD\<scene>.MAP`,
    /// streamed in by `FUN_8001F7C0`), and the live grid byte-matches PROT
    /// 0109 with zero diffs. The nibble-7 paints are story-conditional
    /// *deltas* layered on that base. So the "retail wholesale clear" this
    /// comment used to claim is not a real retail step, and the long-standing
    /// hunt for its clear site was a non-question. Zeroing here only
    /// guarantees a clean buffer before [`Self::load_field_collision_grid`]
    /// overwrites it with the disc base - see
    /// `docs/subsystems/field-locomotion.md`.
    pub fn reset_field_collision_grid(&mut self) {
        self.terrain.collision_grid.clear();
        self.terrain.collision_grid.resize(FIELD_GRID_LEN, 0);
    }

    /// Load the per-scene base collision/floor grid from the field map file's
    /// `+0x4000` region (the `DATA\FIELD\<scene>.MAP` slice exposed by
    /// [`legaia_engine_scene::scene::Scene::field_collision_grid`]). `grid` is the raw
    /// `0x80 x 0x80` byte grid: high nibble = sub-cell wall bits, low nibble =
    /// floor-elevation tier - the same byte format the runtime grid uses, so
    /// it copies verbatim. The field-VM `0x4C` nibble-7 ops then layer
    /// story-conditional deltas on top as the prescript runs.
    ///
    /// PORT: the `+0x4000` sub-region streamed by `FUN_8001f7c0` into the
    /// field buffer at `*(_DAT_1f8003ec)`. Byte-exact vs live RAM (town01).
    pub fn load_field_collision_grid(&mut self, grid: &[u8]) {
        let n = grid.len().min(FIELD_GRID_LEN);
        self.terrain.collision_grid.clear();
        self.terrain.collision_grid.resize(FIELD_GRID_LEN, 0);
        self.terrain.collision_grid[..n].copy_from_slice(&grid[..n]);
    }

    /// Load the per-scene `.MAP` **object-grid** cell words (`+0x8000`, one
    /// little-endian `u16` per tile). The floor sampler tests each tile's
    /// [`CELL_ELEVATION_OVERRIDE`] bit; the other bits (`0x1FF` object index,
    /// `0x1000` walk-visible, `0x2000` visible) belong to the scene's
    /// placement / ground layers. A short slice loads what it has and leaves
    /// the rest zero (a plain bilinear tile).
    pub fn load_field_object_cells(&mut self, cells: &[u8]) {
        let n = (cells.len() / 2).min(FIELD_GRID_LEN);
        self.terrain.object_cells.clear();
        self.terrain.object_cells.resize(FIELD_GRID_LEN, 0);
        for (i, slot) in self.terrain.object_cells[..n].iter_mut().enumerate() {
            *slot = u16::from_le_bytes([cells[i * 2], cells[i * 2 + 1]]);
        }
        // Which bit this scene records its authored floor with. See
        // [`crate::world::FieldTerrain::floor_cell_bit`]: eighteen field scenes
        // author `CELL_VISIBLE` and never `CELL_WALK_VISIBLE`, and reading
        // those as floorless makes the cold-spawn resolver inert in exactly
        // the scenes whose retail seat is a wall.
        self.terrain.floor_cell_bit = if self
            .terrain
            .object_cells
            .iter()
            .any(|c| c & legaia_scene_formats::field_objects::CELL_WALK_VISIBLE != 0)
        {
            legaia_scene_formats::field_objects::CELL_WALK_VISIBLE
        } else {
            legaia_scene_formats::field_objects::CELL_VISIBLE
        };
    }

    /// Install the scene's kind-2 **elevation-override** records from the
    /// `.MAP` trigger blocks: `primary` is the `+0x10000` block, `fallback` the
    /// `+0x12000` one (the sibling sectors the retail loader reads
    /// contiguously). The two parse into one first-match-wins list, mirroring
    /// `FUN_801D5630`'s primary-then-fallback scan.
    ///
    /// PORT: FUN_801D5630 (kind 2) / FUN_801D5AE0
    pub fn load_field_elevation_overrides(&mut self, primary: &[u8], fallback: &[u8]) {
        self.terrain.elevation_overrides =
            legaia_engine_field::field_elevation::parse_elevation_overrides(primary)
                .into_iter()
                .chain(legaia_engine_field::field_elevation::parse_elevation_overrides(fallback))
                .collect();
    }

    /// Field-VM op `4C 83` - rewrite the **elevation override** of every
    /// tile in the inclusive rectangle `[col_start..=col_end] x
    /// [row_start..=row_end]` that has one. The arm (`0x801E20A8..0x801E2130`)
    /// walks X inner, Z outer, resolves each tile through `FUN_801D5630(2, x,
    /// z)` - the kind-2 lookup, primary block then fallback - and on a hit
    /// stores `rec[2] = value` (the whole-tile step) and `rec[3] = 0` (every
    /// sub-cell step); a tile without a record is left alone. `vozz` and
    /// `rikuroa` pair it with a `4C 7x` wall paint, raising or dropping a
    /// span of floor and re-walling it in one beat.
    ///
    /// PORT: FUN_801DE840 (`4C 83`)
    pub fn fill_elevation_rect(
        &mut self,
        col_start: u8,
        row_start: u8,
        col_end: u8,
        row_end: u8,
        value: u8,
    ) {
        for z in row_start..=row_end {
            for x in col_start..=col_end {
                if let Some(rec) = self
                    .terrain
                    .elevation_overrides
                    .iter_mut()
                    .find(|r| r.tile_x == x && r.tile_z == z)
                {
                    rec.coarse = value as i8;
                    rec.quads = 0;
                }
            }
        }
    }

    /// Install the per-scene region / zone tables (the `.MAP` `+0x10000`
    /// block + the MAN section-3 camera-region table) and run the initial
    /// per-tile refresh. Pass empty slices for scenes without the data -
    /// the refresh then clears [`legaia_engine_system::story_flag_state::StoryFlagState::extra_flags`] and resets the
    /// attribute block to the default fill, so stale tables never leak
    /// across a transition.
    pub fn load_field_region_tables(&mut self, map_region_block: &[u8], zone_table: &[u8]) {
        self.terrain.map_region_block = map_region_block.to_vec();
        self.terrain.zone_table = zone_table.to_vec();
        self.refresh_field_regions();
    }

    /// Per-tile region refresh - drives the [`legaia_engine_field::field_regions`] ports
    /// (`FUN_800180EC` + `FUN_801DBA20`) against the player's current tile.
    ///
    /// Quantises `tile = (world - 0x40) >> 7` (the retail locomotion-cluster
    /// convention for `FUN_801DBA20`'s arguments), rebuilds
    /// [`legaia_engine_system::story_flag_state::StoryFlagState::extra_flags`] (the `_DAT_8007B8F4` region-type mask the
    /// field-VM op `0x42` mode 0 tests), latches the scratch attribute
    /// block, and re-selects the current camera-zone record. Called on
    /// scene entry and on every player tile crossing
    /// (`Self::live_field_tick`).
    ///
    /// REF: FUN_800180EC, FUN_801DBA20 (ports in [`legaia_engine_field::field_regions`])
    pub fn refresh_field_regions(&mut self) {
        if self.terrain.map_region_block.is_empty() && self.terrain.zone_table.is_empty() {
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
        let table =
            legaia_engine_field::field_regions::RegionTable::parse(&self.terrain.map_region_block);
        let world_map_mode = self.mode == SceneMode::WorldMap;
        let (mask, attrs) = legaia_engine_field::field_regions::refresh_region_attributes(
            table.as_ref(),
            tx,
            tz,
            world_map_mode,
        );
        self.flags.extra_flags = mask;
        self.terrain.region_attributes = attrs;
        if let Some(result) = legaia_engine_field::field_regions::zone_query(
            &self.terrain.zone_table,
            table.as_ref(),
            &attrs,
            tx,
            tz,
        ) {
            // Retail rewrites `_DAT_8007B8F4` from the zone query's own
            // rebuild too (identical recomputation).
            self.flags.extra_flags = result.region_mask;
            self.terrain.zone_record = result.record.map(|r| {
                let mut rec = [0u8; legaia_engine_field::field_regions::ZONE_RECORD_STRIDE];
                rec.copy_from_slice(r);
                rec
            });
        } else {
            self.terrain.zone_record = None;
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
    /// ([`crate::world::FieldTerrain::object_cells`]):
    ///
    /// - **Plain tiles** (bit clear) take the collision grid's low-nibble
    ///   elevation tier through [`crate::world::FieldTerrain::floor_height_lut`] and
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
    ///   the tile's kind-2 [`ElevationOverride`](legaia_engine_field::field_elevation::ElevationOverride) delta - a whole-tile step plus
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
        self.sample_field_floor_height_with(world_x, world_z, &self.terrain.floor_height_lut)
    }

    /// The same sample taken through the scene's **MAN-header** ladder
    /// ([`crate::world::FieldTerrain::floor_height_lut_static`]) instead of
    /// the live one.
    ///
    /// This is the reading the **camera composer** takes.
    /// `FUN_801DAB90` saves the sixteen live rungs at scratchpad
    /// `0x1F80035C`, writes the MAN header's own ladder over them
    /// (`*(_DAT_8007B898) + 2`, sixteen negated `short`s - the same source
    /// `FUN_8003AEB0` installs at scene entry), calls `FUN_80019278`, and
    /// restores the live rungs (`0x801DAC40..0x801DACA0`). So a floor-tier
    /// oscillator raised by op `0x4C` nibble-9 sub-`0..2` moves the terrain
    /// and the actors standing on it but never the camera. A whole ladder
    /// installed by sub-`E` is different: that arm writes the MAN copy as
    /// well, so the camera follows it.
    ///
    /// REF: FUN_801DAB90
    pub fn sample_field_floor_height_static(&self, world_x: i32, world_z: i32) -> i32 {
        self.sample_field_floor_height_with(world_x, world_z, &self.terrain.floor_height_lut_static)
    }

    fn sample_field_floor_height_with(&self, world_x: i32, world_z: i32, lut: &[i16; 16]) -> i32 {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN {
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
        let g = &self.terrain.collision_grid;
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
            let delta = legaia_engine_field::field_elevation::lookup_elevation_override(
                &self.terrain.elevation_overrides,
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
        self.terrain
            .object_cells
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
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN {
            self.reset_field_collision_grid();
        }
        let hi = mask << 4;
        for row in z_range.0..z_range.1 {
            let row_base = (row as usize) * FIELD_GRID_STRIDE;
            for col in x_range.0..x_range.1 {
                let idx = row_base + col as usize;
                let Some(byte) = self.terrain.collision_grid.get_mut(idx) else {
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
    /// REF: FUN_801cfe4c (the per-direction composite that inlines this
    /// sampler three times over `DAT_801F2214` - ported as
    /// [`Self::field_dir_blocked`]; this item is the single-point probe
    /// `FUN_801d56c4` tagged below)
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
    /// disc-gated `parity/tests/field_collision_discriminator.rs`.
    ///
    /// Retail tests **three leading-edge footprint probes** through this
    /// sampler (47-48 units ahead, ±16 lateral; per-direction table
    /// `DAT_801f2214` = `FIELD_WALL_PROBES`) - see
    /// [`World::field_dir_blocked`], wired into pad locomotion behind
    /// [`crate::world::FieldLocomotion::leading_edge_wall_probes`]. With the flag off, locomotion
    /// tests one candidate-centre point - a standoff/feel difference, not an
    /// indexing one.
    ///
    /// **Both** cell axes are masked to 7 bits, so the byte index can never
    /// leave the `0x4000`-byte grid: retail's row term is
    /// `((z_cell + sign) << 6) & 0x3f80` (`0x801D5710..0x801D571C`), which is
    /// `((z_cell / 2) & 0x7F) * 0x80` - the same `& 0x7F` the column gets at
    /// `0x801D570C`. A Z past the grid's last row therefore **wraps** in
    /// retail rather than reading as open floor, and the port must wrap with
    /// it: an unmasked row index runs off the end of the buffer from
    /// `z >= 0x3F80` up, and reading that as "no wall" hands the player a
    /// free corridor along the far edge of every scene.
    ///
    /// PORT: FUN_801d56c4 (field-overlay walkability probe, 47 instructions
    /// `0x801D56C4..0x801D577C`: the two signed `/64` cell derivations, the
    /// `(col + row*0x80)` byte index into `*(0x1F8003EC) + 0x4000`, the
    /// four-way quadrant-bit select out of the high nibble, and the
    /// `sltu zero, masked` "non-zero means blocked" return)
    pub fn field_tile_is_wall(&self, x: i16, z: i16) -> bool {
        if self.terrain.collision_grid.len() < FIELD_GRID_LEN {
            return false;
        }
        if x < 0 || z < 0 {
            return true; // off the grid origin reads as a wall (clamp inside)
        }
        let zc = ((z as i32) >> 6) + 2;
        let xc = (((x as i32) + 0x3F) >> 6) - 1;
        let col = (xc / 2) & 0x7F;
        let row = ((zc - (zc >> 31)) >> 1) & 0x7F;
        let idx = (col + row * FIELD_GRID_STRIDE as i32) as usize;
        let Some(&byte) = self.terrain.collision_grid.get(idx) else {
            return false;
        };
        let quad = ((zc & 1) << 1 | (xc & 1)) as u32;
        (byte >> 4) & (1u8 << quad) != 0
    }

    /// Is world `(x, z)` on the scene's authored **walkable floor** - i.e. does
    /// its plain (unbiased) `.MAP` object-grid cell carry this scene's floor
    /// bit ([`crate::world::FieldTerrain::floor_cell_bit`]: `CELL_WALK_VISIBLE` `0x1000` where
    /// the scene authors it, `CELL_VISIBLE` `0x2000` in the eighteen scenes
    /// that never do)? Plain `world >> 7` indexing - the same convention
    /// [`Self::sample_field_floor_height`] samples the floor under. `false`
    /// for out-of-range coords and for scenes with no object grid loaded
    /// (every tile then reads as off-floor void).
    ///
    /// This is the port's "inside the authored area" filter for seating, not
    /// retail's standing rule: retail decides where the player may stand from
    /// the collision grid's wall bits ([`Self::field_tile_is_wall`]) and never
    /// reads this grid for it.
    pub fn field_tile_is_walk_visible(&self, x: i16, z: i16) -> bool {
        if x < 0 || z < 0 {
            return false;
        }
        let tx = (x as usize) >> 7;
        let tz = (z as usize) >> 7;
        if tx >= FIELD_GRID_STRIDE || tz >= FIELD_GRID_STRIDE {
            return false;
        }
        self.terrain
            .object_cells
            .get(tz * FIELD_GRID_STRIDE + tx)
            .is_some_and(|c| c & self.terrain.floor_cell_bit != 0)
    }

    /// Is world `(x, z)` a valid cold-entry standing spot - on the authored
    /// walkable floor ([`Self::field_tile_is_walk_visible`]) **and** clear of
    /// the collision-grid wall bits ([`Self::field_tile_is_wall`])?
    pub(super) fn field_spawn_is_valid(&self, x: i16, z: i16) -> bool {
        self.field_tile_is_walk_visible(x, z) && !self.field_tile_is_wall(x, z)
    }

    /// Is the 64-unit **sub-cell** `(sx, sz)` (the wall-bit granularity of the
    /// collision grid - four per 128-unit tile) an open standing spot? Tests
    /// the sub-cell's world-space centre through the two spawn-validity
    /// samplers: on the authored walk-visible floor and clear of the biased
    /// wall read. Out-of-range sub-cells read closed.
    pub(super) fn field_subcell_open(&self, sx: i32, sz: i32) -> bool {
        let stride = (FIELD_GRID_STRIDE * 2) as i32;
        if !(0..stride).contains(&sx) || !(0..stride).contains(&sz) {
            return false;
        }
        let (x, z) = ((sx * 64 + 32) as i16, (sz * 64 + 32) as i16);
        self.field_spawn_is_valid(x, z)
    }
}
