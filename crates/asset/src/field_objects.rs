//! Field static-object placement: the per-scene table that positions a
//! field/town scene's static environment objects (terrain segments,
//! buildings, props) in world space.
//!
//! ## Source
//!
//! The data lives in the per-scene **field map file** (retail
//! `DATA\FIELD\<scene>.MAP`, the `0x12000`-byte block `FUN_8001f7c0` streams
//! into the field-buffer base `_DAT_1f8003ec`). Three regions of that buffer
//! cooperate:
//!
//! | Offset | Region | Use |
//! |---|---|---|
//! | `+0x0000` | object-record table | `0x20`-byte records, indexed by object id |
//! | `+0x4000` | collision/floor grid | `0x80 x 0x80` bytes (1/tile); low nibble = floor tier |
//! | `+0x8000` | object-index grid | `0x80 x 0x80` `u16` (1/tile); `cell & 0x1FF` = object id |
//!
//! ## Consumer (provenance)
//!
//! `FUN_8003A55C` (see `ghidra/scripts/funcs/8003a55c.txt`) sweeps the
//! `128 x 128` tile grid; for each tile whose object-index-grid `u16` selects
//! an object record with the *placed* flag (`+0x12` bit `0x4`) set, it
//! allocates a static-object actor (tick fn `0x8003BC08`) at a world position
//! derived from the tile `(col, row)` and the record's signed `X/Y/Z` offsets,
//! then links the object's interaction script via `func_0x801d5630`. Each
//! placed actor draws its mesh from the scene's `scene_asset_table` TMD pack
//! through the actor's `+0x44` mesh chain.
//!
//! ## Coordinate convention (validated against a live `town01` save state)
//!
//! ```text
//! world_x = col * 0x80 + record.x_off + 0x40
//! world_z = row * 0x80 - (record.z_off - 0x40)
//! world_y = floor_height(grid_nibble) + record.y_off
//! ```
//!
//! - `col`, `row` are `0..128`; tile size = `0x80` (128) world units;
//!   `0x40` (64) is the tile centre.
//! - X is **additive** in the offset; Z is **subtractive** of `(z_off - 0x40)`.
//! - `world_y` needs the per-scene floor-height LUT, which is not in the map
//!   file; this parser reports `y_off` and the tile's floor nibble so a
//!   consumer can resolve height separately.
//!
//! Worked example (`town01`, Vahn's house): object id `137`, anchor tile
//! `(col 38, row 25)`, record `x_off=-64, y_off=0, z_off=56` ->
//! `world_x = 38*128 - 64 + 64 = 4864`, `world_z = 25*128 - (56-64) = 3208`,
//! matching the live actor at `(4864, _, 3208)`.

/// Tiles per grid edge (the grid is `GRID_DIM x GRID_DIM`).
pub const GRID_DIM: usize = 0x80;
/// World units per tile.
pub const TILE: i32 = 0x80;
/// Half a tile (tile centre offset).
pub const TILE_CENTER: i32 = 0x40;
/// Stride of one object record in the `+0x0000` table.
pub const OBJECT_RECORD_STRIDE: usize = 0x20;
/// Byte offset of the object-index grid within the field map file.
pub const OBJECT_GRID_OFFSET: usize = 0x8000;
/// Mask selecting the object-record index out of an object-index-grid cell.
pub const OBJECT_INDEX_MASK: u16 = 0x1FF;
/// Object-record `+0x12` flag bit marking the tile as a placed/visible object.
pub const FLAG_PLACED: u16 = 0x4;
/// Object-record `+0x12` flag bit marking the record's `+0x10` pack mesh as
/// **drawn**. Every record family that renders a mesh carries it (placed
/// landmarks `0x16`/`0x17`, decorations `0x13`, plus their `0x8xx` collider-
/// correction variants); the `0x0011` family does NOT - those are the
/// riverbank/system cells (record 408 in every kingdom walk `.MAP`: same
/// index, same `+0x10 = 4`, same flags across Drake / Sebucus / Karisto)
/// whose `+0x10` is not a decoration mesh reference. Stamping them draws a
/// wall/gate mesh down every river - visually falsified against retail.
pub const FLAG_MESH_DRAWN: u16 = 0x2;
/// Object-record `+0x12` flag bit selecting the collision-footprint offset
/// **correction** (`-x_off`, `+z_off`) in the static-entity collision arm.
/// At spawn the bit is mirrored into the actor's `+0x52`, which
/// `FUN_801cf9f4` tests before applying the correction (live-verified:
/// town01 record `137`, flags `0x101E`, is the only catalogued static
/// collision actor with `+0x52 & 8` set).
pub const FLAG_COLLIDER_CORRECTION: u16 = 0x8;
/// Object-index-grid cell bit marking the tile as a **visible** terrain cell.
/// The overhead continent sweep (`FUN_801F69D8`) renders every cell with this
/// bit set (ground / trees / mountains) - the bulk continent, distinct from
/// the placed-flag interactive objects [`parse_placements`] returns.
///
/// This `0x2000` gate is the **top-down overview** path (game mode `0x0D`,
/// `FUN_801F69D8`), reading `opmap01.MAP` whose pool is the *larger* overview
/// pack - so `+0x10` reaches well past `0x3F` there.
///
/// **The free-roam *walk* view (game mode `0x03`) uses the same record layout
/// but a different cell gate, [`CELL_WALK_VISIBLE`] (`0x1000`).** It reads the
/// per-scene walk `.MAP` (e.g. `map01` walk = PROT entry `0085`), whose `+0x10`
/// values are small (`0..39`) because the walk pool is 5 party + the 40-mesh
/// slot-1 landmark pack. The per-object mesh resolution is the same
/// [`pack_mesh_index`] (`+0x10`) **plus the pack prefix**: the retail path
/// (`FUN_80020f88` -> `actor+0x64 = record[+0x10] + DAT_8007b6f8`, prefix `= 5`;
/// `FUN_80024d78` then builds the actor's mesh chain from
/// `DAT_8007C018[actor+0x64]`) was pinned 14/14 against a live walk capture, so
/// the walk continent pool index is `FIELD_ACTOR_PACK_BIAS + pack_mesh_index`.
pub const CELL_VISIBLE: u16 = 0x2000;
/// Object-index-grid cell bit marking a **walk-view** (game mode `0x03`) visible
/// continent tile - the free-roam analogue of [`CELL_VISIBLE`]. The Drake walk
/// `.MAP` grid sets this on ~15k cells (vs ~300 with `0x2000`); see the
/// `CELL_VISIBLE` docs for the shared `+0x10`-plus-prefix mesh resolution.
pub const CELL_WALK_VISIBLE: u16 = 0x1000;
/// The global-TMD-pool prefix retail adds to a record's `+0x10` mesh id
/// (`FUN_80020f88`: `actor+0x64 = record[+0x10] + DAT_8007b6f8`, prefix `= 5` -
/// the five party/NPC meshes that occupy the head of `DAT_8007C018`). Pool
/// index = `FIELD_ACTOR_PACK_BIAS + pack_mesh_index`; a consumer indexing the
/// scene's own mesh pack (which starts where the shared prefix ends) uses
/// [`pack_mesh_index`] unbiased.
pub const FIELD_ACTOR_PACK_BIAS: u16 = 5;

/// One `0x20`-byte object record (only the fields `FUN_8003A55C` consumes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRecord {
    /// `+0x00` signed world-X offset from the tile centre.
    pub x_off: i16,
    /// `+0x02` signed Y offset added to the tile's floor height.
    pub y_off: i16,
    /// `+0x04` signed world-Z offset (subtractive; see module docs).
    pub z_off: i16,
    /// `+0x06` signed column delta to the footprint anchor.
    pub col_delta: i8,
    /// `+0x07` signed row delta to the footprint anchor.
    pub row_delta: i8,
    /// `+0x08` rotation about world X in PSX angle units (`4096` = full
    /// revolution). `FUN_8003A55C` copies the `+0x08/+0x0A/+0x0C` triple
    /// verbatim into the spawned actor's `+0x24/+0x26/+0x28`, where the
    /// per-actor render dispatcher (`FUN_8001ADA4`) feeds it to the
    /// angle-triple -> GTE-matrix builder `FUN_80026988` (each component
    /// masked `& 0xFFF`). Zero on every retail walk-`.MAP` record; some
    /// town maps tilt props with it (e.g. `koin2`).
    pub rot_x: u16,
    /// `+0x0A` rotation about world Y (yaw), PSX angle units. The field the
    /// Sebucus island bridges use for their quarter-turn orientations
    /// (`0x400` = 90 degrees); the walk-view decoration layer also varies it
    /// per tree/prop. For a pure-Y angle `FUN_80026988` produces the
    /// row-major `[c 0 s; 0 1 0; -s 0 c]` - local `+Z` maps to
    /// `(sin, 0, cos)`, matching the locomotion forward vector.
    pub rot_y: u16,
    /// `+0x0C` rotation about world Z, PSX angle units. Zero on every retail
    /// walk-`.MAP` record; used by a few town maps.
    pub rot_z: u16,
    /// `+0x0E` signed sub-tile X delta (16-unit steps) to the collision
    /// footprint anchor - only read by the static-entity collision arm
    /// (`FUN_801cf9f4`; see [`collision_footprint_offset`]).
    pub sub_anchor_x: i8,
    /// `+0x0F` signed sub-tile Z delta (16-unit steps); pairs with
    /// [`Self::sub_anchor_x`].
    pub sub_anchor_z: i8,
    /// `+0x10` `u16`: scene_asset_table TMD pack index for the object's mesh
    /// (the geometry id) - the mesh selector for every object id.
    pub pack_index_field: u16,
    /// `+0x12` flags; bit [`FLAG_PLACED`] gates spawning.
    pub flags: u16,
    /// `+0x14` ground-tile atlas index (`0..63`) into the `8 x 8` grid of
    /// `32 x 32`-texel tiles on the cell's terrain page (`u = (id % 8) * 32`,
    /// `v = (id / 8) * 32`). The per-cell texture selector for the walk-view
    /// continent ground - see [`WalkHeightfield`].
    pub terrain_tile: u8,
    /// `+0x15` ground-tile PSX `tpage` word (4bpp; e.g. `0x1A` -> fb `(640, 256)`
    /// grass, `0x0C` -> mountain, `0x1B`/`0x1C` -> water, `0x0B` -> forest). The
    /// per-cell terrain-type / VRAM-page selector.
    pub terrain_tpage: u16,
    /// `+0x16..+0x18` ground-tile PSX `clut` (CBA) word (little-endian:
    /// `r[0x16] | r[0x17] << 8`). Selects the tile's palette row.
    pub terrain_clut: u16,
}

impl ObjectRecord {
    /// Decode a record from the `0x20`-byte window at `table[idx*0x20..]`.
    /// Returns `None` if the window does not fit.
    pub fn parse(table: &[u8], idx: usize) -> Option<Self> {
        let base = idx.checked_mul(OBJECT_RECORD_STRIDE)?;
        let r = table.get(base..base + OBJECT_RECORD_STRIDE)?;
        Some(ObjectRecord {
            x_off: i16::from_le_bytes([r[0x00], r[0x01]]),
            y_off: i16::from_le_bytes([r[0x02], r[0x03]]),
            z_off: i16::from_le_bytes([r[0x04], r[0x05]]),
            col_delta: r[0x06] as i8,
            row_delta: r[0x07] as i8,
            rot_x: u16::from_le_bytes([r[0x08], r[0x09]]),
            rot_y: u16::from_le_bytes([r[0x0A], r[0x0B]]),
            rot_z: u16::from_le_bytes([r[0x0C], r[0x0D]]),
            sub_anchor_x: r[0x0E] as i8,
            sub_anchor_z: r[0x0F] as i8,
            pack_index_field: u16::from_le_bytes([r[0x10], r[0x11]]),
            flags: u16::from_le_bytes([r[0x12], r[0x13]]),
            terrain_tile: r[0x14],
            terrain_tpage: r[0x15] as u16,
            terrain_clut: u16::from_le_bytes([r[0x16], r[0x17]]),
        })
    }

    /// `true` when the record's placed flag is set.
    pub fn is_placed(&self) -> bool {
        self.flags & FLAG_PLACED != 0
    }
}

/// The collision-footprint offset of a placed static object, relative to its
/// spawn (placement) world position - what retail's static-entity collision
/// arm adds to the spawned actor's live position to centre the ±80-unit
/// blocking box (`FUN_801cf9f4`, result bit `4`):
///
/// ```text
/// off_x = col_delta * 0x80 + sub_anchor_x * 0x10
/// off_z = row_delta * 0x80 + sub_anchor_z * 0x10
/// if flags & FLAG_COLLIDER_CORRECTION { off_x -= x_off; off_z += z_off }
/// ```
///
/// Live-verified against the spawned static collision actors of catalogued
/// captures: town01 records `315` (offset `(80, -48)`) and `137`
/// (correction arm, `(-32, -40)`), town0c `331`/`332`, koin3 `116`.
pub fn collision_footprint_offset(rec: &ObjectRecord) -> (i32, i32) {
    let mut off_x = rec.col_delta as i32 * TILE + rec.sub_anchor_x as i32 * 0x10;
    let mut off_z = rec.row_delta as i32 * TILE + rec.sub_anchor_z as i32 * 0x10;
    if rec.flags & FLAG_COLLIDER_CORRECTION != 0 {
        off_x -= rec.x_off as i32;
        off_z += rec.z_off as i32;
    }
    (off_x, off_z)
}

/// The scene_asset_table TMD pack index this object draws, or `None` for
/// objects whose mesh is NOT in the scene pack (the protagonist / NPC ids
/// `1/2/3`, whose geometry lives in the shared player/NPC pack).
///
/// The mesh is the record's `+0x10` field ([`ObjectRecord::pack_index_field`])
/// **uniformly**, for every object id - the retail rule (`FUN_80020f88`:
/// `actor+0x64 = record[+0x10] + DAT_8007b6f8`; `FUN_801F69D8` takes the same
/// `+0x10` for its per-cell terrain meshes). The object id selects the *record*,
/// never the mesh.
///
/// (An earlier reading gave object ids `93..=118` a positional "field-actor
/// band" rule - `pack_index = obj_idx - 5` - which is **falsified**: town0c cell
/// `(30, 17)` carries object id `99` whose record `+0x10 = 2`, and the retail
/// prim pool draws that cell's surface from env-pack mesh **2** (the quad's
/// `cba=0x7D00 / tsb=0x000C` + UV set matches mesh 2's prim byte-for-byte), not
/// from mesh `94`. The band rule silently swapped ten town meshes per Rim Elm
/// map - among them the terrain slab south-east of the spawn, whose absence left
/// a clear-colour hole in the ground.)
///
/// `anim_id` (resolved separately via the MAN script) only drives animation;
/// it does not pick geometry.
pub fn pack_mesh_index(obj_idx: u16, rec: &ObjectRecord) -> Option<u16> {
    match obj_idx {
        // Protagonist / NPC meshes: not in the scene pack.
        1..=3 => None,
        _ => Some(rec.pack_index_field),
    }
}

/// One placed static object: its grid anchor, source object id, and world
/// position. `world_y` is left to the consumer (needs the floor-height LUT);
/// [`Self::y_off`] + [`Self::floor_nibble`] carry what the map file knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Object-record index (the grid cell's `& 0x1FF`).
    pub obj_idx: u16,
    /// Anchor tile column (`0..128`).
    pub col: u8,
    /// Anchor tile row (`0..128`).
    pub row: u8,
    /// World X (additive offset; see module docs).
    pub world_x: i32,
    /// World Z (subtractive offset; see module docs).
    pub world_z: i32,
    /// Record Y offset (added to the floor height by the consumer).
    pub y_off: i16,
    /// Low nibble of the collision/floor grid byte at the anchor tile (the
    /// floor-height tier), or `None` when the grid region is absent.
    pub floor_nibble: Option<u8>,
    /// scene_asset_table TMD pack index for this object's mesh (see
    /// [`pack_mesh_index`]), or `None` for protagonist / NPC ids whose mesh is
    /// not in the scene pack.
    pub pack_index: Option<u16>,
    /// The record's flags (placed bit already confirmed set).
    pub flags: u16,
    /// Record rotation about world X (`+0x08`), PSX angle units (`4096` =
    /// full revolution); see [`ObjectRecord::rot_x`]. Zero on retail walk
    /// maps.
    pub rot_x: u16,
    /// Record rotation about world Y (`+0x0A`), PSX angle units - the
    /// authored yaw of the placed mesh (bridge orientations, tree variety);
    /// see [`ObjectRecord::rot_y`].
    pub rot_y: u16,
    /// Record rotation about world Z (`+0x0C`), PSX angle units; see
    /// [`ObjectRecord::rot_z`]. Zero on retail walk maps.
    pub rot_z: u16,
    /// World X of the static collision-box centre
    /// (`world_x + collision_footprint_offset`); see
    /// [`collision_footprint_offset`].
    pub collider_x: i32,
    /// World Z of the static collision-box centre; pairs with
    /// [`Self::collider_x`].
    pub collider_z: i32,
}

/// World X for a tile column + record X offset (`col*0x80 + x_off + 0x40`).
pub fn world_x(col: u8, x_off: i16) -> i32 {
    col as i32 * TILE + x_off as i32 + TILE_CENTER
}

/// World Z for a tile row + record Z offset (`row*0x80 - (z_off - 0x40)`).
pub fn world_z(row: u8, z_off: i16) -> i32 {
    row as i32 * TILE - (z_off as i32 - TILE_CENTER)
}

/// Walk the `128 x 128` object-index grid of a field map file and return one
/// [`Placement`] per placed tile, mirroring `FUN_8003A55C`'s sweep.
///
/// PORT: FUN_8003A55C (the grid sweep + placed-flag gate + world-XZ formula;
/// the actor allocation the retail sweep also does is the engine host's job)
///
/// `field_map` is the **extended** field map file footprint (the object-index
/// grid at `+0x8000` lives past the TOC-indexed `0x4000`-byte payload). Tiles
/// whose object record is absent, unplaced, or whose footprint anchor lands
/// off-grid are skipped, matching the retail bounds gate.
pub fn parse_placements(field_map: &[u8]) -> Vec<Placement> {
    let mut out = Vec::new();
    let Some(grid) = field_map.get(OBJECT_GRID_OFFSET..) else {
        return out;
    };
    for row in 0..GRID_DIM {
        for col in 0..GRID_DIM {
            let cell_off = (row * GRID_DIM + col) * 2;
            let Some(cell_bytes) = grid.get(cell_off..cell_off + 2) else {
                continue;
            };
            let cell = u16::from_le_bytes([cell_bytes[0], cell_bytes[1]]);
            let obj_idx = cell & OBJECT_INDEX_MASK;
            let Some(rec) = ObjectRecord::parse(field_map, obj_idx as usize) else {
                continue;
            };
            if !rec.is_placed() {
                continue;
            }
            // Footprint-anchor bounds gate (matches FUN_8003A55C): the
            // anchor tile (col+col_delta, row+row_delta) must stay on grid.
            let acol = col as i32 + rec.col_delta as i32;
            let arow = row as i32 + rec.row_delta as i32;
            if !(0..GRID_DIM as i32).contains(&acol) || !(0..GRID_DIM as i32).contains(&arow) {
                continue;
            }
            let floor_nibble = field_map
                .get(0x4000 + row * GRID_DIM + col)
                .map(|b| b & 0x0F);
            out.push(Placement {
                obj_idx,
                col: col as u8,
                row: row as u8,
                world_x: world_x(col as u8, rec.x_off),
                world_z: world_z(row as u8, rec.z_off),
                y_off: rec.y_off,
                floor_nibble,
                pack_index: pack_mesh_index(obj_idx, &rec),
                flags: rec.flags,
                rot_x: rec.rot_x,
                rot_y: rec.rot_y,
                rot_z: rec.rot_z,
                collider_x: world_x(col as u8, rec.x_off) + collision_footprint_offset(&rec).0,
                collider_z: world_z(row as u8, rec.z_off) + collision_footprint_offset(&rec).1,
            });
        }
    }
    out
}

/// Walk the `128 x 128` object-index grid and return one [`Placement`] per
/// **visible** tile (cell bit [`CELL_VISIBLE`]), mirroring the overhead
/// continent sweep `FUN_801F69D8`'s `(cell & 0x2000) != 0` gate.
///
/// This is the **bulk continent terrain** - the ground, trees, and mountain
/// meshes that tile the kingdom - as opposed to [`parse_placements`], which
/// returns only the placed-flag (`0x4`) interactive / collision objects. A
/// scene has far more visible terrain tiles than placed objects (a kingdom
/// overworld tiles most of the walkable continent), so the world-map render
/// needs this sweep to draw a populated continent rather than a handful of
/// landmarks.
///
/// World position + mesh resolution use the same formulas as
/// [`parse_placements`]; the only difference is the gate (visible bit instead
/// of the placed flag) and that the footprint-anchor bounds check is relaxed
/// to a plain on-grid test (terrain tiles have no interaction footprint).
/// `obj_idx == 0` cells are skipped (record 0 is the empty/sentinel slot).
pub fn parse_terrain_tiles(field_map: &[u8]) -> Vec<Placement> {
    parse_terrain_tiles_gated(field_map, CELL_VISIBLE, false)
}

/// The free-roam **walk** view's bulk continent: [`parse_terrain_tiles`] gated
/// on [`CELL_WALK_VISIBLE`] (`0x1000`) instead of the overhead-overview's
/// `0x2000`. A real Drake `map01` walk `.MAP` sets `0x1000` on ~16k cells
/// (vs ~300 with `0x2000`).
///
/// The mesh is **`record[+0x10]` uniformly** (retail `FUN_80020f88`:
/// `actor+0x64 = record[+0x10] + prefix`) - the same rule [`pack_mesh_index`]
/// applies; `walk_mesh` only skips the protagonist/NPC id carve-out, which no
/// continent tile hits.
pub fn parse_walk_terrain_tiles(field_map: &[u8]) -> Vec<Placement> {
    parse_terrain_tiles_gated(field_map, CELL_WALK_VISIBLE, true)
}

/// Shared object-grid sweep for [`parse_terrain_tiles`] (overview, `0x2000`)
/// and [`parse_walk_terrain_tiles`] (walk, `0x1000`). `gate` selects the
/// object-index-grid cell bit that marks a drawn tile; `walk_mesh` takes
/// `record[+0x10]` for **every** id (`true`) versus routing through
/// [`pack_mesh_index`], which returns `None` for the protagonist / NPC ids
/// `1..=3` (`false`). Both resolve the same `+0x10` mesh otherwise.
pub fn parse_terrain_tiles_gated(field_map: &[u8], gate: u16, walk_mesh: bool) -> Vec<Placement> {
    let mut out = Vec::new();
    let Some(grid) = field_map.get(OBJECT_GRID_OFFSET..) else {
        return out;
    };
    for row in 0..GRID_DIM {
        for col in 0..GRID_DIM {
            let cell_off = (row * GRID_DIM + col) * 2;
            let Some(cell_bytes) = grid.get(cell_off..cell_off + 2) else {
                continue;
            };
            let cell = u16::from_le_bytes([cell_bytes[0], cell_bytes[1]]);
            if cell & gate == 0 {
                continue;
            }
            let obj_idx = cell & OBJECT_INDEX_MASK;
            if obj_idx == 0 {
                continue;
            }
            let Some(rec) = ObjectRecord::parse(field_map, obj_idx as usize) else {
                continue;
            };
            let floor_nibble = field_map
                .get(0x4000 + row * GRID_DIM + col)
                .map(|b| b & 0x0F);
            out.push(Placement {
                obj_idx,
                col: col as u8,
                row: row as u8,
                world_x: world_x(col as u8, rec.x_off),
                world_z: world_z(row as u8, rec.z_off),
                y_off: rec.y_off,
                floor_nibble,
                pack_index: if walk_mesh {
                    Some(rec.pack_index_field)
                } else {
                    pack_mesh_index(obj_idx, &rec)
                },
                flags: rec.flags,
                rot_x: rec.rot_x,
                rot_y: rec.rot_y,
                rot_z: rec.rot_z,
                collider_x: world_x(col as u8, rec.x_off) + collision_footprint_offset(&rec).0,
                collider_z: world_z(row as u8, rec.z_off) + collision_footprint_offset(&rec).1,
            });
        }
    }
    out
}

/// The walk view's **decoration layer**: every walk-visible
/// ([`CELL_WALK_VISIBLE`]) cell whose object record carries a nonzero `+0x10`
/// pack mesh with the [`FLAG_MESH_DRAWN`] bit but **not** the placed flag -
/// the crossed-quad billboard trees, mountain groups, and small props stamped
/// across the continent (Drake ~300 cells over 35 records; forest clusters
/// reference one tree mesh from dozens of cells each). Flag families
/// `0x0013`/`0x0813`, vs the placed (`FLAG_PLACED`) interactive set
/// [`parse_placements`] returns; excluding placed records here keeps the two
/// layers disjoint so a consumer drawing both never double-stamps a landmark.
/// The `0x0011` family (nonzero `+0x10` but no `FLAG_MESH_DRAWN`) is
/// excluded - those are the riverbank/system cells (see [`FLAG_MESH_DRAWN`]).
///
/// The bulk ground cells (97% of the walk grid) have `+0x10 == 0` and are the
/// heightfield surface ([`build_walk_heightfield`]), not part of this layer.
/// Mesh resolution is the walk rule (`record[+0x10]` directly, per
/// `FUN_80020f88`), the same one [`parse_walk_terrain_tiles`] uses.
pub fn parse_walk_decorations(field_map: &[u8]) -> Vec<Placement> {
    parse_terrain_tiles_gated(field_map, CELL_WALK_VISIBLE, true)
        .into_iter()
        .filter(|p| {
            p.pack_index.is_some_and(|m| m != 0)
                && p.flags & FLAG_MESH_DRAWN != 0
                && p.flags & FLAG_PLACED == 0
        })
        .collect()
}

/// Byte offset of the collision / floor-height grid within the field map file
/// (`0x80 x 0x80` bytes, 1/tile; low nibble = floor-elevation tier).
pub const COLLISION_GRID_OFFSET: usize = 0x4000;

/// The walk-view continent ground is textured per cell from a **terrain-type-
/// keyed multi-page atlas**: every visible cell's object record carries its own
/// tile + page + palette, so grass, mountain, water, and forest cells each
/// sample a different VRAM page. The selector is the record's `+0x14..+0x18`
/// run, byte-verified against the retail prim pool (the ground quads emit in a
/// row-major world-cell sweep; aligning a quad-run's UV sequence to the map's
/// `+0x14` grid matches exactly, and the page/CLUT each map 1:1 to `+0x15` /
/// `+0x16..+0x18`):
/// - `+0x14` -> the `8 x 8` atlas tile index (`u = (id % 8) * 32`,
///   `v = (id / 8) * 32`; tiles are `32 x 32` texels),
/// - `+0x15` -> the PSX `tpage` word (4bpp page; `0x1A` grass at fb `(640, 256)`,
///   `0x0C` mountain, `0x1B`/`0x1C` water, `0x0B` forest, ...),
/// - `+0x16..+0x18` -> the PSX `clut` (CBA) word.
///
/// (The earlier "single 3x3 grass page, positional `(col % 3, row % 3)`,
/// `+0x14` unused" reading was a misread: grass cells happen to use page `0x1A`
/// with `+0x14` landing in the top-left `3 x 3` block, so the mod-3 cross-row
/// sequence was coincidental. `+0x14` is the tile selector, not metadata.)
pub const GROUND_ATLAS_TILE_PX: u8 = 32;
/// Tiles per atlas axis (the atlas is `GROUND_ATLAS_AXIS x GROUND_ATLAS_AXIS`
/// `32 x 32`-texel tiles filling one `256 x 256` VRAM page).
pub const GROUND_ATLAS_AXIS: usize = 8;
/// PSX `tpage` word for the **grass** ground-tile page (4bpp at fb `(640, 256)`).
/// Used as the fallback page when a cell's record carries no terrain bytes;
/// per-cell pages come from [`ObjectRecord::terrain_tpage`].
pub const GROUND_ATLAS_TPAGE: u16 = 0x001A;
/// PSX `clut` (CBA) word for the grass-tile CLUT (fb `(0, 497)`); fallback only.
pub const GROUND_ATLAS_CLUT: u16 = 0x7C40;

/// A triangulated heightfield surface for the world-map walk-view continent
/// ground - the clean-room analogue of the retail terrain renderer, whose
/// elevation comes from the `+0x4000` floor-nibble grid (the height math is
/// pinned by `FUN_80019278`, the SCUS bilinear ground-height sampler: a tile's
/// low nibble indexes the 16-entry floor LUT, and the surface interpolates
/// between adjacent tile heights).
///
/// This is **not** the per-cell pack-mesh instancing the old
/// [`parse_walk_terrain_tiles`] modelled (that floods the map with pool-5 mesh
/// because the bulk-terrain records carry `+0x10 == 0`). The continent ground
/// is a heightfield surface; the slot-1 pack meshes are only the sparse placed
/// landmarks ([`parse_placements`]).
///
/// Positions are in the same pre-Y-flip world frame the placement draws use
/// (`world_y = -lut[nibble]`), so the engine applies the same `(1, -1, 1)`
/// model flip.
///
/// [`Self::uvs`] + [`Self::cba_tsb`] texture each cell from the retail terrain-
/// type-keyed multi-page atlas (see the `GROUND_ATLAS_*` constants): the cell's
/// object record gives the tile (`+0x14` -> `8 x 8` atlas UV), the VRAM page
/// (`+0x15` -> `tpage`), and the palette (`+0x16..+0x18` -> `clut`). `tile_id`
/// keeps the raw `+0x14` byte for non-texturing terrain-class uses (encounter /
/// footstep / walk-speed). See `docs/subsystems/world-map.md` "Ground texturing".
#[derive(Debug, Clone, Default)]
pub struct WalkHeightfield {
    /// Per-vertex world position (pre-Y-flip): `(col*128, -lut[nibble], row*128)`.
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex source-tile `+0x14` id (`0..63`) - the atlas tile index this
    /// cell draws, also usable as terrain-class metadata (see the struct docs).
    pub tile_ids: Vec<u8>,
    /// Per-vertex page-local texture coordinates into the cell's terrain page.
    /// Each cell's four corners cover the `32 x 32` atlas tile selected by the
    /// record's `+0x14` byte (`u = (id % 8) * 32`, `v = (id / 8) * 32`).
    pub uvs: Vec<[u8; 2]>,
    /// Per-vertex `[clut, tpage]` (PSX CBA + tpage words) selecting the cell's
    /// terrain page + palette from the record's `+0x15` / `+0x16..+0x18` bytes.
    /// Distinct per cell so grass / mountain / water / forest cells sample their
    /// own VRAM page in a single mesh.
    pub cba_tsb: Vec<[u16; 2]>,
    /// Per-vertex GP0 modulation colour - the `[R][G][B]` word a PSX primitive
    /// carries and the GPU blends as `out = texel * colour / 128`
    /// ([`GROUND_PRIM_COLOR`]). The ground layer is **unshaded**: retail emits
    /// its ground as *flat* `POLY_FT4` (one colour word per quad, not gouraud)
    /// and every one on a town terrain page reads neutral `0x80,0x80,0x80` (two
    /// Rim Elm field captures, 550 + 1056 ground quads, zero exceptions). All the
    /// ground's contrast comes from the atlas texels; the shading a town frame
    /// does carry lives on the *env meshes* over it, whose prims carry their own
    /// baked colour words. Kept per vertex so a renderer that modulates by a
    /// vertex colour attribute can feed the ground through the same path.
    pub colors: Vec<[u8; 3]>,
    /// Triangle indices (two triangles per visible cell quad).
    pub indices: Vec<u32>,
}

/// The GP0 modulation colour every retail ground primitive carries: neutral
/// (`0x80` = 128 = `texel * 128 / 128`). See [`WalkHeightfield::colors`].
pub const GROUND_PRIM_COLOR: [u8; 3] = [0x80, 0x80, 0x80];

impl WalkHeightfield {
    /// Number of visible cells (quads) emitted.
    pub fn quad_count(&self) -> usize {
        self.indices.len() / 6
    }
}

/// Build the walk-view continent ground as a heightfield surface from a field
/// map file's floor grid (`+0x4000`) gated on the object-grid `0x1000` visible
/// bit. `lut` is the 16-entry floor-height LUT (from the MAN header). Each
/// visible cell `(c, r)` emits a quad whose four corners take their Y from the
/// floor nibble of the corner tile (`-lut[nibble]`), giving a continuous
/// heightfield (adjacent cells share corner heights). Empty if the map has no
/// grid.
///
/// REF: FUN_80019278
///
/// `FUN_80019278` (SCUS `0x80019278`, always-resident, no overlay aliasing)
/// is the **bilinear ground-height sampler** retail uses to read an entity's
/// height at runtime: it reads the entity's XZ, gates on the object grid's
/// `0x1000` cell bit, and bilinearly interpolates the floor height from the
/// `2 x 2` block of `+0x4000` nibbles (`grid[0], [1], [0x80], [0x81]`, each
/// `& 0xF` -> `DAT_1F80035C[nibble]` LUT, weighted by the sub-tile XZ
/// position, `>> 0xE`). This builder shares the nibble-LUT decode but only at
/// integer cell corners - pre-baking the heightfield mesh once so the
/// renderer's GPU vertex interpolation supplies the same bilinear surface in
/// a single pass. A clean-room per-entity bilinear sampler (what
/// `FUN_80019278` literally does) is not currently needed - entities walking
/// on the heightfield get implicit interpolation from the rasteriser.
pub fn build_walk_heightfield(field_map: &[u8], lut: &[i16; 16]) -> WalkHeightfield {
    let mut hf = WalkHeightfield::default();
    let Some(obj_grid) = field_map.get(OBJECT_GRID_OFFSET..) else {
        return hf;
    };
    // Corner height (pre-Y-flip) from the floor nibble of tile (c, r), clamped
    // to the grid edge so border cells stay watertight.
    let corner_y = |c: usize, r: usize| -> f32 {
        let cc = c.min(GRID_DIM - 1);
        let rr = r.min(GRID_DIM - 1);
        let nib = field_map
            .get(COLLISION_GRID_OFFSET + rr * GRID_DIM + cc)
            .map(|b| (b & 0x0F) as usize)
            .unwrap_or(0);
        -(lut[nib] as f32)
    };
    for row in 0..GRID_DIM {
        for col in 0..GRID_DIM {
            let cell_off = (row * GRID_DIM + col) * 2;
            let Some(cell_bytes) = obj_grid.get(cell_off..cell_off + 2) else {
                continue;
            };
            let cell = u16::from_le_bytes([cell_bytes[0], cell_bytes[1]]);
            if cell & CELL_WALK_VISIBLE == 0 {
                continue;
            }
            // This cell's terrain texture comes from its object record's
            // +0x14..+0x18 run: +0x14 = atlas tile, +0x15 = tpage (terrain
            // page), +0x16..+0x18 = clut. Fall back to the grass page when the
            // record is absent / carries no terrain bytes (tpage 0 = fb (0,0),
            // never a real terrain page).
            let obj_idx = (cell & OBJECT_INDEX_MASK) as usize;
            let rec = ObjectRecord::parse(field_map, obj_idx);
            let tile_id = rec.map(|r| r.terrain_tile).unwrap_or(0);
            let (tpage, clut) = match rec {
                Some(r) if r.terrain_tpage != 0 => (r.terrain_tpage, r.terrain_clut),
                _ => (GROUND_ATLAS_TPAGE, GROUND_ATLAS_CLUT),
            };
            let x0 = (col as i32 * TILE) as f32;
            let x1 = ((col as i32 + 1) * TILE) as f32;
            let z0 = (row as i32 * TILE) as f32;
            let z1 = ((row as i32 + 1) * TILE) as f32;
            let base = hf.positions.len() as u32;
            // 4 corners: (c,r) (c+1,r) (c,r+1) (c+1,r+1).
            hf.positions.push([x0, corner_y(col, row), z0]);
            hf.positions.push([x1, corner_y(col + 1, row), z0]);
            hf.positions.push([x0, corner_y(col, row + 1), z1]);
            hf.positions.push([x1, corner_y(col + 1, row + 1), z1]);
            // Per-cell atlas tile from +0x14: the 8x8 atlas places tile `id` at
            // `(u, v) = ((id % 8) * 32, (id / 8) * 32)`. Compute in a wide type
            // and clamp to the u8 page extent: the bottom-right tile origin is
            // `(224, 224)` and `+31` reaches the page edge `(255, 255)`, but the
            // intermediate `224 + 32` overflows a u8 - so widen, then cast.
            let px = GROUND_ATLAS_TILE_PX as usize;
            let u_lo = ((tile_id as usize % GROUND_ATLAS_AXIS) * px).min(255) as u8;
            let v_lo = ((tile_id as usize / GROUND_ATLAS_AXIS) * px).min(255) as u8;
            let u_hi = (u_lo as usize + px - 1).min(255) as u8;
            let v_hi = (v_lo as usize + px - 1).min(255) as u8;
            // Corner→texel mapping. U runs along +X/col (left tile edge = u_lo).
            // V is FLIPPED relative to +Z/row: the retail terrain emitter maps the
            // low-Z (row) corner to the tile's *bottom* texel row and the high-Z
            // corner to the *top* row. Measured camera-independently from the
            // retail prim pool: recovering each ground POLY_FT4's world (col,row)
            // and reading its per-corner UVs gives, for ~96–100% of cells across
            // the mountain + coast captures and every terrain page,
            // `(c,r)→(u_lo,v_hi)`, `(c,r+1)→(u_lo,v_lo)` - i.e. V decreases as the
            // row index increases. Baking V the other way mirrors every tile
            // vertically in place, which leaves uniform tiles (grass) looking fine
            // but makes directional transition tiles (coastline sand, ridges) face
            // the wrong way and break continuity with their row-neighbours.
            hf.uvs.push([u_lo, v_hi]); // (col,   row)   low-Z
            hf.uvs.push([u_hi, v_hi]); // (col+1, row)
            hf.uvs.push([u_lo, v_lo]); // (col,   row+1) high-Z
            hf.uvs.push([u_hi, v_lo]); // (col+1, row+1)
            for _ in 0..4 {
                hf.tile_ids.push(tile_id);
                hf.cba_tsb.push([clut, tpage]);
                hf.colors.push(GROUND_PRIM_COLOR);
            }
            // Two triangles, standard PSX quad winding (v0,v1,v2)+(v1,v3,v2).
            hf.indices
                .extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }
    hf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vahns_house_position_formula() {
        // town01 record 137: x_off=-64, y_off=0, z_off=56, anchor (col 38, row 25).
        assert_eq!(world_x(38, -64), 4864);
        assert_eq!(world_z(25, 56), 3208);
    }

    #[test]
    fn record_parse_reads_offsets_and_flag() {
        let mut table = vec![0u8; OBJECT_RECORD_STRIDE * 2];
        // record 1: x=-64, y=0, z=56, col_delta=-1, row_delta=-1, flags=0x101E.
        let r = &mut table[OBJECT_RECORD_STRIDE..];
        r[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
        r[0x04..0x06].copy_from_slice(&56i16.to_le_bytes());
        r[0x06] = (-1i8) as u8;
        r[0x07] = (-1i8) as u8;
        r[0x12..0x14].copy_from_slice(&0x101Eu16.to_le_bytes());
        let rec = ObjectRecord::parse(&table, 1).unwrap();
        assert_eq!(rec.x_off, -64);
        assert_eq!(rec.z_off, 56);
        assert_eq!(rec.col_delta, -1);
        assert!(rec.is_placed());
    }

    #[test]
    fn parse_placements_emits_placed_tiles_only() {
        // Build a synthetic field map: one placed record (idx 137, the Vahn's-
        // house id: >=120 so its mesh comes from the +0x10 field), grid cell
        // (row 25, col 38) -> idx 137, every other cell -> idx 0 (unplaced,
        // since record 0 has no placed flag).
        let mut map = vec![0u8; 0x12000];
        let r = &mut map[OBJECT_RECORD_STRIDE * 137..OBJECT_RECORD_STRIDE * 138];
        r[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
        r[0x04..0x06].copy_from_slice(&56i16.to_le_bytes());
        r[0x10..0x12].copy_from_slice(&36u16.to_le_bytes()); // pack mesh index
        r[0x12..0x14].copy_from_slice(&0x0004u16.to_le_bytes()); // placed
        let cell = OBJECT_GRID_OFFSET + (25 * GRID_DIM + 38) * 2;
        map[cell..cell + 2].copy_from_slice(&137u16.to_le_bytes());
        let p = parse_placements(&map);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].obj_idx, 137);
        assert_eq!((p[0].col, p[0].row), (38, 25));
        assert_eq!((p[0].world_x, p[0].world_z), (4864, 3208));
        assert_eq!(p[0].pack_index, Some(36));
    }

    #[test]
    fn parse_terrain_tiles_emits_visible_cells_regardless_of_placed_flag() {
        // Record 5: a terrain tile mesh, NOT placed-flagged (flags = 0).
        let mut map = vec![0u8; 0x12000];
        let r = &mut map[OBJECT_RECORD_STRIDE * 5..OBJECT_RECORD_STRIDE * 6];
        r[0x10..0x12].copy_from_slice(&12u16.to_le_bytes()); // pack mesh index
        // flags stays 0 -> NOT placed (parse_placements would skip it).
        // Cell (row 10, col 20): visible bit set + record index 5.
        let cell = OBJECT_GRID_OFFSET + (10 * GRID_DIM + 20) * 2;
        map[cell..cell + 2].copy_from_slice(&(CELL_VISIBLE | 5).to_le_bytes());
        // A second cell pointing at record 5 but WITHOUT the visible bit: skipped.
        let cell2 = OBJECT_GRID_OFFSET + (11 * GRID_DIM + 20) * 2;
        map[cell2..cell2 + 2].copy_from_slice(&5u16.to_le_bytes());

        // parse_placements drops the unplaced record entirely.
        assert!(parse_placements(&map).is_empty());
        // parse_terrain_tiles emits the visible cell (and only it).
        let t = parse_terrain_tiles(&map);
        assert_eq!(t.len(), 1, "only the CELL_VISIBLE cell is emitted");
        assert_eq!(t[0].obj_idx, 5);
        assert_eq!((t[0].col, t[0].row), (20, 10));
        assert_eq!(t[0].pack_index, Some(12));
    }

    #[test]
    fn parse_walk_decorations_skips_ground_and_placed_records() {
        let mut map = vec![0u8; 0x12000];
        // Record 475: a tree mesh (nonzero +0x10, decoration flags 0x0013).
        let r = &mut map[OBJECT_RECORD_STRIDE * 475..OBJECT_RECORD_STRIDE * 476];
        r[0x10..0x12].copy_from_slice(&11u16.to_le_bytes());
        r[0x12..0x14].copy_from_slice(&0x0013u16.to_le_bytes());
        // Record 349: a placed landmark (nonzero +0x10 AND the placed flag).
        let r = &mut map[OBJECT_RECORD_STRIDE * 349..OBJECT_RECORD_STRIDE * 350];
        r[0x10..0x12].copy_from_slice(&6u16.to_le_bytes());
        r[0x12..0x14].copy_from_slice(&0x0017u16.to_le_bytes());
        // Record 200: bulk ground (+0x10 == 0, flags 0x0011).
        let r = &mut map[OBJECT_RECORD_STRIDE * 200..OBJECT_RECORD_STRIDE * 201];
        r[0x12..0x14].copy_from_slice(&0x0011u16.to_le_bytes());
        // Record 408: the riverbank/system family - nonzero +0x10 but flags
        // 0x0011 (no FLAG_MESH_DRAWN). Stamping it draws a wall mesh down
        // every river, so it must be excluded.
        let r = &mut map[OBJECT_RECORD_STRIDE * 408..OBJECT_RECORD_STRIDE * 409];
        r[0x10..0x12].copy_from_slice(&4u16.to_le_bytes());
        r[0x12..0x14].copy_from_slice(&0x0011u16.to_le_bytes());
        // Walk-visible cells, one per record; the tree stamps twice.
        for (row, col, idx) in [
            (4, 10, 475u16),
            (4, 11, 475),
            (5, 10, 349),
            (5, 11, 200),
            (6, 10, 408),
        ] {
            let cell = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
            map[cell..cell + 2].copy_from_slice(&(CELL_WALK_VISIBLE | idx).to_le_bytes());
        }
        let d = parse_walk_decorations(&map);
        // Only the two tree cells: the placed landmark belongs to
        // parse_placements, the zero-mesh cell to the heightfield, and the
        // riverbank record lacks FLAG_MESH_DRAWN.
        assert_eq!(d.len(), 2);
        assert!(
            d.iter()
                .all(|p| p.obj_idx == 475 && p.pack_index == Some(11))
        );
        assert_eq!(
            d.iter().map(|p| (p.col, p.row)).collect::<Vec<_>>(),
            vec![(10, 4), (11, 4)]
        );
    }

    #[test]
    fn pack_mesh_index_rule() {
        let mut rec = ObjectRecord::parse(&[0u8; OBJECT_RECORD_STRIDE], 0).unwrap();
        rec.pack_index_field = 15;
        // >= 120 and == 83 use the +0x10 field.
        assert_eq!(pack_mesh_index(230, &rec), Some(15));
        assert_eq!(pack_mesh_index(83, &rec), Some(15));
        // Ids 93..=118 are NOT positional: they take `+0x10` like every other
        // id (the old `obj_idx - 5` "field-actor band" rule is falsified - see
        // the town0c cell (30,17) / env-mesh-2 prim match in the fn docs).
        assert_eq!(pack_mesh_index(96, &rec), Some(15));
        assert_eq!(pack_mesh_index(118, &rec), Some(15));
        // Protagonist / NPC ids draw from a different pool.
        assert_eq!(pack_mesh_index(1, &rec), None);
        assert_eq!(pack_mesh_index(3, &rec), None);
    }

    #[test]
    fn heightfield_emits_quad_per_visible_cell_with_floor_heights() {
        let mut map = vec![0u8; 0x12000];
        // Floor LUT: nibble 2 -> height 80, nibble 5 -> 200 (negated in mesh).
        let lut = {
            let mut l = [0i16; 16];
            l[2] = 80;
            l[5] = 200;
            l
        };
        // Tile (col 10, row 4): floor nibble 2; mark its object cell walk-visible
        // with obj_idx 7, and give record 7 a terrain run (+0x14..+0x18): tile
        // 0x2A on the mountain page 0x0C with CLUT 0x7EC0.
        map[COLLISION_GRID_OFFSET + 4 * GRID_DIM + 10] = 0x02;
        let cell = OBJECT_GRID_OFFSET + (4 * GRID_DIM + 10) * 2;
        map[cell..cell + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 7).to_le_bytes());
        map[7 * OBJECT_RECORD_STRIDE + 0x14] = 0x2A; // tile index
        map[7 * OBJECT_RECORD_STRIDE + 0x15] = 0x0C; // tpage (mountain page)
        map[7 * OBJECT_RECORD_STRIDE + 0x16] = 0xC0; // clut low
        map[7 * OBJECT_RECORD_STRIDE + 0x17] = 0x7E; // clut high

        let hf = build_walk_heightfield(&map, &lut);
        assert_eq!(hf.quad_count(), 1, "one visible cell -> one quad");
        assert_eq!(hf.positions.len(), 4);
        assert_eq!(hf.indices.len(), 6);
        // Corner (10,4) sits at world (10*128, -lut[2], 4*128).
        assert_eq!(hf.positions[0], [1280.0, -80.0, 512.0]);
        // The far corner (11,5) reads nibble 0 (height 0) since those tiles
        // weren't painted.
        assert_eq!(hf.positions[3][1], 0.0);
        // Every vertex carries the cell's +0x14 tile id.
        assert!(hf.tile_ids.iter().all(|&t| t == 0x2A));
        // Atlas tile 0x2A = 42 -> (42 % 8, 42 / 8) = (col 2, row 5) -> UV origin
        // (64, 160), spanning a 32x32 rect. V is flipped vs the row axis (retail
        // emitter): the low-Z (row) corner takes the tile's bottom texel row.
        assert_eq!(hf.uvs.len(), 4);
        assert_eq!(hf.uvs[0], [64, 191]); // (col,   row)   low-Z -> bottom row
        assert_eq!(hf.uvs[1], [95, 191]); // (col+1, row)
        assert_eq!(hf.uvs[2], [64, 160]); // (col,   row+1) high-Z -> top row
        assert_eq!(hf.uvs[3], [95, 160]); // (col+1, row+1)
        // Every vertex carries the cell's [clut, tpage] from +0x15 / +0x16..+0x18.
        assert!(hf.cba_tsb.iter().all(|&ct| ct == [0x7EC0, 0x000C]));
    }

    #[test]
    fn ground_page_and_clut_are_per_cell_from_record() {
        // Two adjacent walk-visible cells with different terrain records: a
        // grass cell (page 0x1A) and a water cell (page 0x1B) coexist in one
        // heightfield, each sampling its own page - the multi-page terrain atlas.
        let lut = [0i16; 16];
        let mut map = vec![0u8; 0x12000];
        // record 1: grass tile 9 on page 0x1A / CLUT 0x7C40.
        let g = OBJECT_RECORD_STRIDE;
        map[g + 0x14] = 9;
        map[g + 0x15] = 0x1A;
        map[g + 0x16] = 0x40;
        map[g + 0x17] = 0x7C;
        // record 2: water tile 17 on page 0x1B / CLUT 0x7F41.
        let w = 2 * OBJECT_RECORD_STRIDE;
        map[w + 0x14] = 17;
        map[w + 0x15] = 0x1B;
        map[w + 0x16] = 0x41;
        map[w + 0x17] = 0x7F;
        let cg = OBJECT_GRID_OFFSET; // (col 0, row 0)
        map[cg..cg + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 1).to_le_bytes());
        let cw = OBJECT_GRID_OFFSET + 2; // (col 1, row 0)
        map[cw..cw + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 2).to_le_bytes());

        let hf = build_walk_heightfield(&map, &lut);
        assert_eq!(hf.quad_count(), 2);
        // First cell (grass): tile 9 -> origin (32, 32); first (low-Z) corner takes
        // the bottom texel row (V flipped vs row), page 0x1A / CLUT 0x7C40.
        assert_eq!(hf.uvs[0], [32, 63]);
        assert_eq!(hf.cba_tsb[0], [0x7C40, 0x001A]);
        // Second cell (water): tile 17 -> (17 % 8, 17 / 8) = (1, 2) -> origin
        // (32, 64); first corner -> bottom row (32, 95), page 0x1B / CLUT 0x7F41.
        assert_eq!(hf.uvs[4], [32, 95]);
        assert_eq!(hf.cba_tsb[4], [0x7F41, 0x001B]);
    }

    #[test]
    fn collision_footprint_offset_matches_live_verified_records() {
        // town01 record 315 (live static actor, `v0_1_pre_battle_tetsu`):
        // col_delta 1, sub (-3, -3), no correction flag -> (80, -48).
        let mut rec = [0u8; OBJECT_RECORD_STRIDE];
        rec[0x06] = 1;
        rec[0x0E] = 0xFD;
        rec[0x0F] = 0xFD;
        rec[0x12] = 0x16;
        let r = ObjectRecord::parse(&rec, 0).unwrap();
        assert_eq!(collision_footprint_offset(&r), (80, -48));

        // town01 record 137 (the correction arm: flag bit 0x8 -> the spawn
        // mirrors it into actor +0x52, and the offset subtracts x_off /
        // adds z_off): deltas (-1, -1), sub (2, 2), x_off -64, z_off 56
        // -> (-96 + 64, -96 + 56) = (-32, -40).
        let mut rec = [0u8; OBJECT_RECORD_STRIDE];
        rec[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
        rec[0x04..0x06].copy_from_slice(&56i16.to_le_bytes());
        rec[0x06] = 0xFF;
        rec[0x07] = 0xFF;
        rec[0x0E] = 2;
        rec[0x0F] = 2;
        rec[0x12..0x14].copy_from_slice(&0x101Eu16.to_le_bytes());
        let r = ObjectRecord::parse(&rec, 0).unwrap();
        assert_eq!(collision_footprint_offset(&r), (-32, -40));
    }

    #[test]
    fn placements_carry_collider_centres() {
        // A placed record with a footprint offset: the placement's collider
        // centre = its world position + the offset.
        let mut map = vec![0u8; 0x12000];
        let rec = OBJECT_RECORD_STRIDE; // record 1
        map[rec + 0x06] = 1; // col_delta 1 -> +128
        map[rec + 0x0E] = 0xFD; // sub X -3 -> -48
        map[rec + 0x0F] = 0xFD; // sub Z -3 -> -48
        map[rec + 0x12] = FLAG_PLACED as u8;
        let cell = OBJECT_GRID_OFFSET + (3 * GRID_DIM + 5) * 2; // (col 5, row 3)
        map[cell..cell + 2].copy_from_slice(&1u16.to_le_bytes());
        let ps = parse_placements(&map);
        assert_eq!(ps.len(), 1);
        let p = &ps[0];
        assert_eq!((p.world_x, p.world_z), (5 * 128 + 64, 3 * 128 + 64));
        assert_eq!(p.collider_x, p.world_x + 128 - 48);
        assert_eq!(p.collider_z, p.world_z - 48);
    }

    #[test]
    fn ground_cell_without_terrain_bytes_falls_back_to_grass_page() {
        // A visible cell whose record carries no terrain run (tpage 0) uses the
        // grass fallback page, never tpage 0 (= fb (0,0), the framebuffer).
        let lut = [0i16; 16];
        let mut map = vec![0u8; 0x12000];
        let cell = OBJECT_GRID_OFFSET;
        map[cell..cell + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 1).to_le_bytes());
        let hf = build_walk_heightfield(&map, &lut);
        assert_eq!(hf.quad_count(), 1);
        assert_eq!(hf.cba_tsb[0], [GROUND_ATLAS_CLUT, GROUND_ATLAS_TPAGE]);
    }
}
