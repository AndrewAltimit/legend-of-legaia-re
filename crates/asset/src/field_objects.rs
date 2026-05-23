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
/// Object ids `93..=118` are the "field-actor" band: their mesh is selected
/// positionally (`pack_index = obj_idx - FIELD_ACTOR_PACK_BIAS`) rather than
/// from the record's `+0x10` field. These map to the last meshes of the pack.
pub const FIELD_ACTOR_BAND: std::ops::RangeInclusive<u16> = 93..=118;
/// Subtracted from an object id in [`FIELD_ACTOR_BAND`] to get its pack index.
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
    /// `+0x10` `u16`: scene_asset_table TMD pack index for the object's mesh
    /// (the geometry id), for objects outside [`FIELD_ACTOR_BAND`].
    pub pack_index_field: u16,
    /// `+0x12` flags; bit [`FLAG_PLACED`] gates spawning.
    pub flags: u16,
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
            pack_index_field: u16::from_le_bytes([r[0x10], r[0x11]]),
            flags: u16::from_le_bytes([r[0x12], r[0x13]]),
        })
    }

    /// `true` when the record's placed flag is set.
    pub fn is_placed(&self) -> bool {
        self.flags & FLAG_PLACED != 0
    }
}

/// The scene_asset_table TMD pack index this object draws, or `None` for
/// objects whose mesh is NOT in the scene pack (the protagonist / NPC ids
/// `1/2/3`, whose geometry lives in the shared player/NPC pack).
///
/// Two cases, byte-verified against a live `town01` save:
/// - object ids in [`FIELD_ACTOR_BAND`] (`93..=118`) select positionally:
///   `pack_index = obj_idx - FIELD_ACTOR_PACK_BIAS` (the last pack meshes);
/// - every other id uses the record's `+0x10` field ([`ObjectRecord::pack_index_field`]).
///
/// `anim_id` (resolved separately via the MAN script) only drives animation;
/// it does not pick geometry.
pub fn pack_mesh_index(obj_idx: u16, rec: &ObjectRecord) -> Option<u16> {
    match obj_idx {
        // Protagonist / NPC meshes: not in the scene pack.
        1..=3 => None,
        id if FIELD_ACTOR_BAND.contains(&id) => Some(id - FIELD_ACTOR_PACK_BIAS),
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
            });
        }
    }
    out
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
    fn pack_mesh_index_rule() {
        let mut rec = ObjectRecord::parse(&[0u8; OBJECT_RECORD_STRIDE], 0).unwrap();
        rec.pack_index_field = 15;
        // >= 120 and == 83 use the +0x10 field.
        assert_eq!(pack_mesh_index(230, &rec), Some(15));
        assert_eq!(pack_mesh_index(83, &rec), Some(15));
        // Field-actor band 93..=118 is positional (obj_idx - 5).
        assert_eq!(pack_mesh_index(96, &rec), Some(91));
        assert_eq!(pack_mesh_index(118, &rec), Some(113));
        // Protagonist / NPC ids draw from a different pool.
        assert_eq!(pack_mesh_index(1, &rec), None);
        assert_eq!(pack_mesh_index(3, &rec), None);
    }
}
