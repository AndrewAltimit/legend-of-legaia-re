//! Field collision grid, region tables, floor sampling, wall/interact probes, NPC/actor motion, direction decoding, and free-movement locomotion.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

mod locomotion;
mod npc_motion;
use legaia_engine_vm::field_ledge_hop_arc as hop_arc;

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

/// The walk half of the ambient motion VM's collision service: retail's
/// `FUN_801cf8ac` box test, whose only subject is the **player actor**.
///
/// Both call sites reduce to "is the player standing where I am about to
/// step". The directional steps `0x03`/`0x19`/`0x20` probe the single
/// `DAT_801F2254` compass point ([`FIELD_FACING_PROBES`]) for their
/// heading-LUT index; the `0x18` wander probes the three-point fan of
/// `DAT_801F21B4` ([`FIELD_ACTOR_PROBES`]) for its cardinal. Both apply the
/// shared `(x + dx, z - dz)` convention around the *walking actor* and
/// accept a hit inside [`FIELD_NPC_BOX_HALF`] on both axes.
///
/// Neither probe reads the walkability grid, so a wandering villager is
/// bounded by its op's authored AABB rather than by walls.
///
/// What this covers of `FUN_801cf8ac` (`0x801CF8AC..0x801CF9F0`) is the
/// **class arm** - `+0x10 & 0x01020000` set, box `0x40 - 0x18` = ±40 about
/// the walker's live position. That is the only arm an ambient walker can
/// take: every placement the MAN spawner `FUN_8003A1E4` seats gets
/// `+0x10 |= 0x20000` (`lui v1, 2; or` at `0x8003A3A8..0x8003A3B4`, plus the
/// party-bank bit `0x01000000` for a `>= 0xF0` special model), and the
/// ambient channels are exactly those placements. The same bit makes the
/// hit's result class `1` (`+0x10 & 0x40020000`), never `4`.
///
/// The routine's **collision-exempt early-out** is ported too
/// ([`Self::exempt`]): `+0x10 & 3` non-zero returns `0` before any box test
/// (`andi v0, v0, 3; bnez` at `0x801CF8B8`), and the directional caller
/// skips the call outright under `+0x10 & 1` (`0x80038484..0x80038490`). A
/// script sets those bits with op `0x31` (`31 00` / `31 01`), and the
/// placement channel's live flag word
/// ([`World::field_channel_flags`]) is where the port keeps them - so a walker
/// a script has made collision-exempt walks through the player, as in retail.
///
/// The rest of the routine changes nothing an ambient walker can observe:
///
/// - the **no-class arm** (`0x801CF8D4..0x801CF930`: box `0x40 + 0x10` = ±80
///   plus the model-bbox offset of the 32-byte record
///   `*(0x1F8003EC) + actor[+0x60] * 32`) is unreachable for a placement,
///   which leaves the class arm only if bit 17 is cleared and no disc script
///   clears it; and its result is `4`, which every caller drops - all three
///   sites (`0x800384C0`, `0x80038A78`, `0x80038F3C`) test `result & 1`;
/// - the hit's mutual contact link (`0x801CF9BC..0x801CF9C8`,
///   `player[+0x98] = actor`, `actor[+0x98] = player`) is overwritten before
///   it is read: the player-side readers (`0x801D0750`, `0x801D0868`) run
///   only on a hit of the player's own probe, which rewrites `+0x98` first.
///
/// PORT: FUN_801cf8ac
/// REF: FUN_801d5a68
struct AmbientPlayerProbe {
    player: Option<(i16, i16)>,
    /// The walker's `+0x10 & 3` collision-exempt bits are up: every probe
    /// misses.
    exempt: bool,
}

/// The 12-bit engine heading that points along `(dx, dz)` - the engine's
/// stand-in for retail's arctangent resolver `FUN_80019B28` (LUT at
/// `DAT_8006F4C8`), in the engine's own convention where `0` faces Z+ rather
/// than retail's `+0x26` space where `0` faces Z-.
///
/// Float `atan2` rather than the LUT, so the result is shape-faithful and not
/// bit-exact; every consumer either draws it or quantises it into a 45°
/// compass sector.
///
/// REF: FUN_80019B28
fn engine_bearing(dx: f32, dz: f32) -> i16 {
    ((dx.atan2(dz) / std::f32::consts::TAU * 4096.0).round() as i32 & 0x0FFF) as i16
}

/// Does this motion stream carry a walk op - a directional step
/// (`0x03`/`0x19`/`0x20`) or the `0x18` AABB wander?
fn stream_has_walk_op(code: &[u8]) -> bool {
    let mut pc = 0usize;
    while pc < code.len() {
        let Some(w) = legaia_asset::man_motion::op_width(code[pc]) else {
            return false;
        };
        if matches!(code[pc], 0x03 | 0x19 | 0x20 | 0x18) {
            return true;
        }
        pc += w;
    }
    false
}

impl AmbientPlayerProbe {
    fn hit(&self, x: i16, z: i16, dx: i16, dz: i16) -> bool {
        if self.exempt {
            return false;
        }
        let Some((px, pz)) = self.player else {
            return false;
        };
        let qx = x.saturating_add(dx) as i32;
        let qz = z.saturating_sub(dz) as i32;
        (qx - px as i32).abs() < FIELD_NPC_BOX_HALF && (qz - pz as i32).abs() < FIELD_NPC_BOX_HALF
    }
}

impl vm::ambient_motion::AmbientBlocking for AmbientPlayerProbe {
    fn step_blocked(&self, x: i16, z: i16, lut_index: u8) -> bool {
        let (dx, dz) = FIELD_FACING_PROBES[usize::from(lut_index & 7)];
        self.hit(x, z, dx, dz)
    }

    /// The wander fan, which retail hoists into its own routine.
    ///
    /// `FUN_801d5a68(actor, dir)` indexes `DAT_801F21B4` at `dir * 0x10`,
    /// reads three `(i16 dx, i16 dz)` pairs from `+0x00 / +0x04 / +0x08` of
    /// that row, calls `FUN_801cf8ac(actor, dx, dz)` on each, and returns the
    /// bitwise **or** of the three results - so any one of the fan's three
    /// points hitting the player refuses the step. The row's fourth pair
    /// (`+0x0C`) is never read.
    ///
    /// PORT: FUN_801d5a68
    fn wander_blocked(&self, x: i16, z: i16, dir4: u8) -> bool {
        FIELD_ACTOR_PROBES[usize::from(dir4) & 3]
            .iter()
            .any(|&(dx, dz)| self.hit(x, z, dx, dz))
    }
}

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
            .any(|c| c & legaia_asset::field_objects::CELL_WALK_VISIBLE != 0)
        {
            legaia_asset::field_objects::CELL_WALK_VISIBLE
        } else {
            legaia_asset::field_objects::CELL_VISIBLE
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
    /// the refresh then clears [`crate::world::StoryFlagState::extra_flags`] and resets the
    /// attribute block to the default fill, so stale tables never leak
    /// across a transition.
    pub fn load_field_region_tables(&mut self, map_region_block: &[u8], zone_table: &[u8]) {
        self.terrain.map_region_block = map_region_block.to_vec();
        self.terrain.zone_table = zone_table.to_vec();
        self.refresh_field_regions();
    }

    /// Per-tile region refresh - drives the [`crate::field_regions`] ports
    /// (`FUN_800180EC` + `FUN_801DBA20`) against the player's current tile.
    ///
    /// Quantises `tile = (world - 0x40) >> 7` (the retail locomotion-cluster
    /// convention for `FUN_801DBA20`'s arguments), rebuilds
    /// [`crate::world::StoryFlagState::extra_flags`] (the `_DAT_8007B8F4` region-type mask the
    /// field-VM op `0x42` mode 0 tests), latches the scratch attribute
    /// block, and re-selects the current camera-zone record. Called on
    /// scene entry and on every player tile crossing
    /// (`Self::live_field_tick`).
    ///
    /// REF: FUN_800180EC, FUN_801DBA20 (ports in [`crate::field_regions`])
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
        let table = crate::field_regions::RegionTable::parse(&self.terrain.map_region_block);
        let world_map_mode = self.mode == SceneMode::WorldMap;
        let (mask, attrs) =
            crate::field_regions::refresh_region_attributes(table.as_ref(), tx, tz, world_map_mode);
        self.flags.extra_flags = mask;
        self.terrain.region_attributes = attrs;
        if let Some(result) = crate::field_regions::zone_query(
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
                let mut rec = [0u8; crate::field_regions::ZONE_RECORD_STRIDE];
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
            let delta = crate::world::field_elevation::lookup_elevation_override(
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
    /// disc-gated `engine-shell/tests/field_collision_discriminator.rs`.
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
    fn field_spawn_is_valid(&self, x: i16, z: i16) -> bool {
        self.field_tile_is_walk_visible(x, z) && !self.field_tile_is_wall(x, z)
    }

    /// Is the 64-unit **sub-cell** `(sx, sz)` (the wall-bit granularity of the
    /// collision grid - four per 128-unit tile) an open standing spot? Tests
    /// the sub-cell's world-space centre through the two spawn-validity
    /// samplers: on the authored walk-visible floor and clear of the biased
    /// wall read. Out-of-range sub-cells read closed.
    fn field_subcell_open(&self, sx: i32, sz: i32) -> bool {
        let stride = (FIELD_GRID_STRIDE * 2) as i32;
        if !(0..stride).contains(&sx) || !(0..stride).contains(&sz) {
            return false;
        }
        let (x, z) = ((sx * 64 + 32) as i16, (sz * 64 + 32) as i16);
        self.field_spawn_is_valid(x, z)
    }

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
        let stride = FIELD_GRID_STRIDE * 2;
        let last = stride as i32 - 1;
        let mut labels = vec![0u16; stride * stride];
        let mut sizes: Vec<u32> = Vec::new();
        let mut edges: Vec<[bool; 4]> = Vec::new();
        let mut queue: std::collections::VecDeque<(i32, i32)> = std::collections::VecDeque::new();
        for sz in 0..stride as i32 {
            for sx in 0..stride as i32 {
                let idx = sz as usize * stride + sx as usize;
                if labels[idx] != 0 || !self.field_subcell_open(sx, sz) {
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
                        if labels[nidx] == 0 && self.field_subcell_open(nx, nz) {
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
    /// and landing positions ([`crate::field_regions::IntraSceneTeleport`]).
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

    /// Camera-relative pad-direction remap: rotate a held direction mask by
    /// the field camera's rotation index `rot` (0..7 eighth-turns) around the
    /// eight-direction ring [`FIELD_DIR_RING`], so "screen up" always walks
    /// away from the camera regardless of azimuth. Returns the input mask
    /// unchanged when `rot == 0` (identity camera) or no direction is held.
    ///
    /// PORT: FUN_800467e8
    ///
    /// Faithful to the retail path (`func_0x800467e8(&_DAT_8007b850)`, called
    /// at the top of the free-movement controller `FUN_801d01b0` before the
    /// wall-slide resolve): retail finds the held direction's index in the
    /// ring, adds the camera step (`gp+0x2d8`), wraps `& 7`, and writes the
    /// rotated mask back over the direction nibble (`held & 0xffff0fff | new`).
    /// This is a **45°** remap - it rotates diagonals as first-class ring
    /// entries, unlike [`World::decode_field_direction`]'s 90°-quantised
    /// screen-vector rotation, which the two agree on for even `rot` (the
    /// axis-aligned cameras every retail field scene actually uses).
    ///
    /// The live pad path calls this every frame through
    /// [`Self::decode_field_direction`], with the rotation index
    /// [`Self::field_pad_ring_rotation`] derives from the port's own camera
    /// azimuth. `precise_movement` keeps its continuous decode, which no ring
    /// index can express.
    ///
    /// **Where retail's `gp+0x2D8` comes from.** Not from the camera. Its
    /// disc-wide writers are a field-VM arm and the tile-board walker: the
    /// `0x4C` outer-nibble-`2` arm at `0x801E0EB8` stores `sub_op & 7` there
    /// (and, when `0x8007B6B0` reads `-1000`, turns the player with it -
    /// `actor[+0x26] += (new - old) * 0x200`), and the tile-board walker
    /// derives it from the terrain type. So the index is **authored**, per
    /// scene, to match the camera the same script installed; there is no
    /// azimuth-to-octant arithmetic anywhere in retail. A port whose camera
    /// free-orbits has no authored index to read, so it derives one from the
    /// azimuth instead - which is a port decision, not a retail one, and is
    /// why `field_pad_ring_rotation` is separate from this routine.
    pub fn remap_pad_direction(held: u16, rot: u32) -> u16 {
        if rot == 0 {
            return held;
        }
        let dir = held & 0xF000;
        if dir == 0 {
            return held;
        }
        // Locate the held direction in the ring (retail's linear scan; a valid
        // direction mask is always one of the eight entries).
        let idx = FIELD_DIR_RING.iter().position(|&m| m == dir).unwrap_or(8);
        let rotated = FIELD_DIR_RING[(idx as u32).wrapping_add(rot) as usize & 7];
        // Retail rewrites the 0xf000 direction nibble in place (32-bit
        // `held & 0xffff0fff`); on the 16-bit mask that clears the top nibble.
        (held & 0x0FFF) | rotated
    }

    /// The eighth-turn index [`Self::decode_field_direction`] rotates the held
    /// d-pad mask by: the camera azimuth
    /// ([`crate::world::FieldLocomotion::camera_azimuth`], 12-bit, `0` = the
    /// follow default) rounded to the nearest of eight compass steps.
    ///
    /// This stands in for retail's `gp+0x2D8`, which is authored per scene by
    /// a field-VM arm rather than computed (see
    /// [`Self::remap_pad_direction`]). The port's camera free-orbits, so the
    /// authored index would be stale the moment the player drags the view;
    /// deriving it from the live azimuth keeps "screen up walks away from the
    /// camera" true at every orbit, which is the law the hosts' compass
    /// oracles measure.
    ///
    /// It is a strict refinement of the 90-degree quantisation it replaced:
    /// `rot` is even exactly where the old `quadrant` was defined, and
    /// `rot == 2 * quadrant` there, so every axis-aligned camera - which is
    /// every camera a retail field scene installs - decodes bit-identically.
    /// The odd steps are the ones that used to snap up to 45 degrees away.
    pub(crate) fn field_pad_ring_rotation(&self) -> u32 {
        ((self.locomotion.camera_azimuth as u32).wrapping_add(0x100) >> 9) & 7
    }

    /// Retail's wall-slide direction resolver: given the post-remap held mask
    /// and the player's current position, return the mask the per-axis step
    /// loop actually walks on - the held direction, plus a perpendicular
    /// **slide** bit when the held direction is blocked but there is open
    /// space to one side, so the player skids along a wall instead of
    /// sticking to it.
    ///
    /// PORT: FUN_80046494
    /// REF: FUN_801d56c4
    ///
    /// Two short-circuits return the raw mask untouched, exactly as retail:
    /// the no-clip pad bit (`held & 0x2`), and any of the four pure diagonals
    /// (`0x9000`/`0xc000`/`0x3000`/`0x6000`) - a diagonal already offers two
    /// axes for the collision step to resolve independently. Otherwise, for
    /// each held cardinal ([`FIELD_SLIDE_DIRS`]):
    ///
    /// 1. **Three-point block test** at the ±62-unit candidate point: the
    ///    point offset `±FIELD_SLIDE_LATERAL` perpendicular to travel, plus
    ///    dead centre, sampled through [`World::field_tile_is_wall`] (retail's
    ///    walkability probe `func_0x801d56c4`). Clear on all three -> just OR
    ///    the direction bit and move on.
    /// 2. **Slide search**: sweep the perpendicular axis over
    ///    [`FIELD_SLIDE_SWEEP`], summing the offsets that come back walkable.
    /// 3. **Sign picks the slide**: a negative sum ORs the row's negative
    ///    slide bit, a positive sum the positive one ([`FIELD_SLIDE_BITS`]); a
    ///    sum of exactly zero (symmetric dead end) adds nothing. The original
    ///    direction bit is ORed in regardless.
    ///
    /// Live on the pad path: [`Self::step_field_locomotion`] resolves the
    /// camera-remapped mask through this function and feeds the result to
    /// [`Self::advance_with_collision`], the same order retail uses
    /// (`jal 0x80046494` at `0x801D03EC` inside `FUN_801D01B0`, result kept
    /// in `s0` at `0x801D0404`). Only the step reads the resolved mask - the
    /// heading and the diagonal speed cut stay on the held one. The precise
    /// free-angle path keeps its own vector step and does not resolve.
    ///
    /// The "wiring it moves every pinned wall rest" worry was measured and
    /// is false for the oracle that carried it: on the live grid of both
    /// wall-press captures the resolver hands back exactly the held cardinal
    /// at the captured rest position, so the pinned legs of
    /// `engine-shell/tests/field_collision_discriminator.rs` are
    /// slide-neutral. The same grid carries thousands of sliding positions,
    /// and `wall_slide_wire_skids_where_the_bare_stepper_sticks` drives one
    /// of them through this path against the bare stepper.
    pub fn resolve_field_slide(&self, held: u16, x: i16, z: i16) -> u16 {
        // No-clip pad bit: pass the raw mask through untouched.
        if held & 0x2 != 0 {
            return held;
        }
        // Pure diagonals are never slide-resolved.
        if matches!(held & 0xF000, 0x9000 | 0xC000 | 0x3000 | 0x6000) {
            return held;
        }
        let px = x as i32;
        let pz = z as i32;
        // The walkability probe: retail `func_0x801d56c4` returns "blocked"
        // for a wall sub-cell, which is exactly `field_tile_is_wall` (same
        // `+2` Z / `ceil-1` X biased sub-cell derivation). Coords stay well
        // inside i16 for field-sized worlds; clamp defensively.
        let wall = |wx: i32, wz: i32| -> bool {
            self.field_tile_is_wall(
                wx.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                wz.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            )
        };
        let mut out = 0u16;
        for (i, &(mask, dx, dy)) in FIELD_SLIDE_DIRS.iter().enumerate() {
            if held & mask == 0 {
                continue;
            }
            // Lateral offset perpendicular to the travel axis.
            let (latx, latz) = if dx != 0 {
                (0, FIELD_SLIDE_LATERAL)
            } else {
                (FIELD_SLIDE_LATERAL, 0)
            };
            let (cx, cz) = (px + dx, pz + dy);
            let blocked = wall(cx + latx, cz + latz) || wall(cx - latx, cz - latz) || wall(cx, cz);
            if !blocked {
                out |= mask;
                continue;
            }
            // Sweep the perpendicular axis; sum the walkable offsets.
            let mut total = 0i32;
            for &off in FIELD_SLIDE_SWEEP.iter() {
                if dx == 0 {
                    // Z-travel -> sweep in X.
                    if !wall(cx + off, cz) {
                        total += off;
                    }
                }
                if dy == 0 {
                    // X-travel -> sweep in Z.
                    if !wall(cx, cz + off) {
                        total += off;
                    }
                }
            }
            let (neg_bit, pos_bit) = FIELD_SLIDE_BITS[i];
            if total < 0 {
                out |= neg_bit;
            }
            if total > 0 {
                out |= pos_bit;
            }
            out |= mask;
        }
        out
    }

    /// Retail's actor-collision direction test: from the CURRENT position
    /// `(x, z)`, take the three probe points of `FIELD_ACTOR_PROBES` row
    /// `dir` (same `(x + dx, z - dz)` convention as the wall probes) and
    /// box-test each against every field NPC's position
    /// ([`crate::world::FieldNpcState::positions`]); the direction is blocked when any
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
    ///   positions are LIVE: `Self::tick_field_npc_motions` walks
    ///   scripted NPCs through the motion VM and writes back into
    ///   [`crate::world::FieldNpcState::positions`], so a moving NPC's
    ///   ±`FIELD_NPC_BOX_HALF` (40) box follows it, exactly as retail
    ///   probes the live `+0x14`/`+0x18`.
    /// - the **static-entity arm** (result bit `4`) - placed `.MAP` props,
    ///   box ±`FIELD_PROP_BOX_HALF` (80) around the record-derived
    ///   footprint centre ([`crate::world::FieldPropState::colliders`]).
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

    /// The actor / prop sweep at **one exact world point** - retail
    /// `FUN_801cfc40` invoked with a single offset pair, which is how the
    /// ledge classifier calls it.
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801d1878
    ///
    /// `FUN_801cfc40` takes no footprint. It stages **one** probe point into
    /// scratchpad `0x1F800020` - `+0 = actor.x + a2`, `+2 = actor.y`,
    /// `+4 = actor.z - a3` (`0x801CFCA0..0x801CFCCC`; the `a3` subtraction is
    /// why callers pass the Z offset negated) - and box-tests every candidate
    /// against that single point. The half-extent is `0x40` widened by the
    /// caller's two stack words (`0x801CFD6C..0x801CFDD0`), and the ledge
    /// classifier passes `0` for both at each of its two call sites
    /// (`sw zero, 0x10(sp)` / `sw zero, 0x14(sp)` at
    /// `0x801D1A88`+`0x801D1A90` and `0x801D1AAC`+`0x801D1AB4`, the second of
    /// each pair in the `jal`'s delay slot) - the same zero-extent call the
    /// walk controller makes, so the boxes are the established
    /// [`FIELD_NPC_BOX_HALF`] / [`FIELD_PROP_BOX_HALF`].
    ///
    /// [`Self::field_actor_dir_blocked`] is the *walk controller's* shape of
    /// the same routine - three points off a compass table, taken at the
    /// actor's own position - and the two are not interchangeable.
    pub(crate) fn field_actor_point_blocked(&self, px: i32, pz: i32) -> bool {
        if self.npcs.solid
            && self.npcs.positions.values().any(|&(ax, az)| {
                (px - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (pz - az as i32).abs() < FIELD_NPC_BOX_HALF
            })
        {
            return true;
        }
        self.props.colliders.iter().any(|c| {
            if !c.solid || self.collider_arrival_exempt(c) {
                return false;
            }
            let ((cx, cz), half) = if c.moving_box {
                (c.live, FIELD_NPC_BOX_HALF)
            } else {
                (c.center, FIELD_PROP_BOX_HALF)
            };
            (px - cx).abs() < half && (pz - cz).abs() < half
        })
    }

    /// The **moving-NPC arm** of the actor-collision direction test (retail
    /// result bit `1` for the village-NPC class): the three probe points
    /// against every live NPC position at ±[`FIELD_NPC_BOX_HALF`].
    ///
    /// PORT: FUN_801cfc40
    pub(crate) fn field_npc_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        if self.npcs.positions.is_empty() {
            return false;
        }
        FIELD_ACTOR_PROBES[dir & 3].iter().any(|&(dx, dz)| {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            self.npcs.positions.values().any(|&(ax, az)| {
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
        if self.props.colliders.is_empty() {
            return out;
        }
        for &(dx, dz) in &FIELD_ACTOR_PROBES[dir & 3] {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            for c in &self.props.colliders {
                if !c.solid || self.collider_arrival_exempt(c) {
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
    pub fn field_interact_probe_slot(&self) -> Option<u8> {
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
        let consider = |slot: u8, ax: i16, az: i16, best: &mut Option<(i32, u8)>| {
            let (ex, ez) = ((px - ax as i32).abs(), (pz - az as i32).abs());
            if ex < FIELD_INTERACT_BOX_HALF && ez < FIELD_INTERACT_BOX_HALF {
                let d = ex * ex + ez * ez;
                if best.is_none_or(|(bd, bs)| d < bd || (d == bd && slot < bs)) {
                    *best = Some((d, slot));
                }
            }
        };
        for (&npc_slot, &(ax, az)) in &self.npcs.positions {
            consider(npc_slot, ax, az, &mut best);
        }
        // Retail's probe walks the **actor list** and box-tests every placed
        // actor, with no "is this one a talk NPC" filter - the touched actor's
        // own record is what the dialog SM then runs. The engine's
        // `field_npc_positions` is seeded only for text-bearing NPC placements,
        // so a **minigame door** (a casino cabinet, the dance-hall desk) sat
        // outside the probe entirely and pressing the action button at one did
        // nothing at all. Its placement anchor is already carried by
        // `field_walk_touch`; admit exactly the slots that also carry an
        // interaction record and are not already NPC anchors, which is the door
        // set and nothing else.
        // REF: FUN_801cf9f4 (the actor-list walk), FUN_80039B7C
        for (&slot, &((ax, az), _)) in &self.props.walk_touch {
            if self.npcs.positions.contains_key(&slot)
                || !self.npcs.dialog_prologue.contains_key(&slot)
            {
                continue;
            }
            consider(slot, ax, az, &mut best);
        }
        // A talk proxy is box-tested at its own spawn position and answers
        // with the actor whose conversation its touch runs: `concnow`
        // P1[26], one tile in front of the gate, for guard P1[13], who
        // stands in the wall line beyond the probe's reach.
        // REF: FUN_801cf9f4, FUN_801D5B5C
        for &(target, (ax, az)) in self.npcs.talk_proxies.values() {
            if self.npcs.positions.contains_key(&target) {
                consider(target, ax, az, &mut best);
            }
        }
        best.map(|(_, s)| s)
    }
}

#[cfg(test)]
mod face_target_tests {
    use super::*;

    /// Talking to a field NPC turns it to face the player: the interaction
    /// dispatch ([`World::trigger_field_interact`]) drives the ported `0x4C`
    /// `FaceTarget` motion-VM leg through [`World::face_field_npc_toward`] and
    /// settles the NPC's [`crate::world::FieldNpcState::headings`] entry onto the player
    /// bearing, converging from whatever stale facing it held.
    #[test]
    fn interaction_start_turns_npc_to_face_player() {
        let mut w = World::new();
        // Player in slot 0, standing at (+100, 0) - due +X of the NPC.
        w.player_actor_slot = Some(0);
        w.actors[0].active = true;
        w.actors[0].move_state.world_x = 100;
        w.actors[0].move_state.world_z = 0;
        // NPC placement slot 3 at the origin, facing the *opposite* way (0x800)
        // so the face leg has a full half-turn to converge.
        w.npcs.positions.insert(3, (0, 0));
        w.npcs.headings.insert(3, 0x800);

        w.trigger_field_interact(0x05, 3);

        // atan2(dx=100, dz=0) = +pi/2 -> 12-bit yaw 0x400 (X+); the one-shot
        // FaceTarget leg snaps straight onto it.
        assert_eq!(w.npcs.headings.get(&3), Some(&0x0400));
    }

    /// An NPC nothing has turned stands at the spawn default, retail
    /// `+0x26 = 0` - engine `0x800`, the pose the hosts draw for an absent
    /// heading. The talk snap saves that pose and the teardown puts it back,
    /// rather than the opposite compass point an `unwrap_or(0)` read gave.
    #[test]
    fn talk_restore_of_an_unturned_npc_returns_the_spawn_heading() {
        let mut w = World::new();
        w.player_actor_slot = Some(0);
        w.actors[0].active = true;
        w.actors[0].move_state.world_x = 100;
        w.actors[0].move_state.world_z = 0;
        w.npcs.positions.insert(3, (0, 0));
        assert_eq!(w.npcs.heading(3), crate::world::SPAWN_HEADING);
        w.face_field_npc_at_player(3);
        assert_eq!(w.npcs.headings.get(&3), Some(&0x0400));
        w.release_talk_facing();
        assert_eq!(w.npcs.heading(3), crate::world::SPAWN_HEADING);
    }

    /// The face driver rotates toward the bearing from an arbitrary start and
    /// is a no-op for a slot with no surfaced position (the retail actor-list
    /// miss never poses an actor).
    #[test]
    fn face_field_npc_toward_converges_and_skips_unplaced() {
        let mut w = World::new();
        // NPC at the origin, facing +X (0x400). Player is due -Z (0, -100):
        // atan2(dx=0, dz=-100) = pi -> yaw 0x800.
        w.npcs.positions.insert(2, (0, 0));
        w.npcs.headings.insert(2, 0x400);
        w.face_field_npc_toward(2, 0, -100);
        assert_eq!(w.npcs.headings.get(&2), Some(&0x0800));

        // A slot with no position is left untouched - no heading is invented.
        w.face_field_npc_toward(9, 100, 100);
        assert!(!w.npcs.headings.contains_key(&9));
    }
}

#[cfg(test)]
mod seat_tests {
    use super::*;

    /// A synthetic scene: every tile walk-visible, wall bytes only at the
    /// named **grid** cells (`(col, row)` of the `+0x4000` grid).
    fn scene(walls: &[(usize, usize)]) -> World {
        let mut w = World::new();
        let mut grid = vec![0u8; FIELD_GRID_LEN];
        for &(col, row) in walls {
            grid[row * FIELD_GRID_STRIDE + col] = 0xF0;
        }
        w.load_field_collision_grid(&grid);
        // Object grid: `CELL_WALK_VISIBLE` (0x1000) on every tile, LE u16.
        w.load_field_object_cells(&[0x00u8, 0x10].repeat(FIELD_GRID_LEN));
        w.player_actor_slot = Some(0);
        w.actors[0].active = true;
        w
    }

    /// World centre of collision-grid cell `(col, row)` under the biased
    /// derivation the wall probe uses (`x in [128c, 128c+128)`,
    /// `z in [128r-128, 128r)`).
    fn cell_centre(col: i16, row: i16) -> (i16, i16) {
        (col * 128 + 64, row * 128 - 64)
    }

    /// Retail's byte index masks **both** cell axes to 7 bits
    /// (`0x801D570C` column, `0x801D571C` row), so a Z past the grid's last
    /// row wraps onto a real row instead of running off the buffer. The
    /// unmasked port read fell out of `Vec::get` and answered "no wall",
    /// which handed the player an open corridor along the far edge of every
    /// scene.
    #[test]
    fn wall_probe_wraps_the_row_like_retail() {
        // Grid row 0 is solid, row 1 is open.
        let w = scene(&(0..FIELD_GRID_STRIDE).map(|c| (c, 0)).collect::<Vec<_>>());
        let open = cell_centre(20, 1);
        assert!(!w.field_tile_is_wall(open.0, open.1), "grid row 1 is open");
        // `z = 16300` derives z_cell 256 -> row 128, which retail masks back
        // to row 0 (`andi 0x3f80`) and must read that row's wall bits. The
        // unmasked port index ran off the buffer and answered "open".
        assert!(
            w.field_tile_is_wall(open.0, 16300),
            "a Z past the last grid row wraps onto row 0, not into open floor"
        );
    }

    /// The seat rescue is a **no-op on a standable tile** - an authored door
    /// arrival lands exactly where the op-`0x3F` operand says.
    #[test]
    fn authored_seat_is_untouched() {
        let mut w = scene(&[]);
        w.seat_player_at_tile(20, 20);
        let ms = &w.actors[0].move_state;
        assert_eq!((ms.world_x, ms.world_z), (20 * 128 + 64, 20 * 128 + 64));
        // ...and the half-tile bit still selects the far half.
        w.seat_player_at_tile(20 | 0x80, 20);
        let ms = &w.actors[0].move_state;
        assert_eq!((ms.world_x, ms.world_z), (20 * 128 + 128, 20 * 128 + 64));
    }

    /// A seat the walkability grid does not cover is nudged onto the nearest
    /// open sub-cell rather than parking the player inside a wall, where
    /// every locomotion direction is blocked.
    #[test]
    fn seat_inside_a_wall_is_rescued_to_open_floor() {
        // A 3x3 grid-cell block of solid wall over the seat tile (20, 20),
        // whose biased cell is (col 20, row 21).
        let walls: Vec<(usize, usize)> = (19..=21)
            .flat_map(|c| (20..=22).map(move |r| (c, r)))
            .collect();
        let mut w = scene(&walls);
        let asked = (20i16 * 128 + 64, 20i16 * 128 + 64);
        assert!(
            w.field_tile_is_wall(asked.0, asked.1),
            "the tile the caller names is a wall"
        );
        w.seat_player_at_tile_rescued(20, 20);
        let seated = {
            let ms = &w.actors[0].move_state;
            (ms.world_x, ms.world_z)
        };
        assert_ne!(seated, asked, "the seat moved off the wall");
        assert!(
            !w.field_tile_is_wall(seated.0, seated.1),
            "the rescued seat is standable"
        );
        // The nudge stays local - within the documented sub-cell radius.
        let d = ((seated.0 - asked.0) as i32)
            .abs()
            .max(((seated.1 - asked.1) as i32).abs());
        assert!(
            d <= SEAT_RESCUE_RADIUS_SUBCELLS * 64 + 64,
            "the nudge is bounded, got {d} units"
        );
    }

    /// The *authored* seat keeps the wall tile. A door is a gap in a wall, so
    /// a real op-`0x3F` arrival tile routinely reads as closed - `jou`'s
    /// castle door `(94, 97)` and two of `map01`'s four `suimon` portals do.
    /// Rescuing those lands the player off the destination's own walk-on band
    /// and the door goes dead, which is why the nudge is opt-in.
    #[test]
    fn an_authored_door_seat_keeps_its_wall_tile() {
        let walls: Vec<(usize, usize)> = (19..=21)
            .flat_map(|c| (20..=22).map(move |r| (c, r)))
            .collect();
        let mut w = scene(&walls);
        let asked = (20i16 * 128 + 64, 20i16 * 128 + 64);
        assert!(
            w.field_tile_is_wall(asked.0, asked.1),
            "the door tile is a wall"
        );
        w.seat_player_at_tile(20, 20);
        let ms = &w.actors[0].move_state;
        assert_eq!(
            (ms.world_x, ms.world_z),
            asked,
            "an op-0x3F arrival lands byte-exactly on its operand even when \
             the walkability grid marks that tile closed"
        );
    }

    /// Nothing standable within the radius: the caller's coordinate is
    /// returned unchanged rather than the player being hauled across the map.
    #[test]
    fn a_seat_with_no_open_floor_nearby_is_left_alone() {
        let walls: Vec<(usize, usize)> = (0..FIELD_GRID_STRIDE)
            .flat_map(|c| (0..FIELD_GRID_STRIDE).map(move |r| (c, r)))
            .collect();
        let w = scene(&walls);
        assert_eq!(w.nearest_standable_seat(2624, 2624), (2624, 2624));
    }

    /// The cold-spawn resolver's main region skips an open area that reaches
    /// three or more map edges - the shape a kingdom overworld's sea has, and
    /// the reason a size-only pick seated the player offshore. Here the
    /// border ring is the bigger region and the enclosed pocket the smaller
    /// one; the pocket must win.
    #[test]
    fn cold_spawn_prefers_an_enclosed_region_over_the_map_border() {
        // Wall ring around grid cells 8..=40 seals an interior pocket;
        // everything outside it is one big open region touching all four map
        // edges - the shape of a kingdom overworld's sea.
        let mut walls: Vec<(usize, usize)> = Vec::new();
        for i in 8..=40 {
            walls.push((i, 8));
            walls.push((i, 40));
            walls.push((8, i));
            walls.push((40, i));
        }
        let w = scene(&walls);
        let inside = (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ);
        let pocket = w.field_walk_component_size(inside.0, inside.1);
        assert!(pocket > 0, "the pocket is open floor");
        assert!(
            w.field_largest_walk_component_size() > pocket,
            "the border region is the larger of the two - a size-only pick \
             would take it"
        );
        // The retail cold seat (0xA40) is inside the pocket, so rule 1 keeps
        // it: that is the assertion that the pocket, not the border region,
        // is the main region.
        assert_eq!(
            w.resolve_cold_field_spawn(&[], &[]),
            (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ)
        );
    }
}
