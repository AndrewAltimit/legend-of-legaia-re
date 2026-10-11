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
//! Retail creates a placed object's actor from **one of two sweeps**, and every
//! placed record belongs to exactly one of them. Both walk the object-index grid
//! and take the cells whose record carries the *placed* flag (`+0x12` bit `0x4`);
//! they differ in what else they demand, and the anchor tile's
//! [`CELL_BIND_OWNED`] (`0x400`) bit is the switch:
//!
//! 1. **`FUN_8003A55C`** (SCUS, scene init, whole grid; see
//!    `ghidra/scripts/funcs/8003a55c.txt`) resolves the record's **object bind**
//!    at its footprint-anchor tile ([`Placement::anchor_col`] /
//!    [`Placement::anchor_row`]) via `func_0x801d5630(1, anchor_col, anchor_row)`
//!    and **skips the record when there is none** (`s1 == 0` -> next tile). The
//!    bind supplies the object's interaction script *and* its animation id, so
//!    this sweep owns the scripted / animated props. Every record it spawns has
//!    `0x400` set on its anchor tile.
//! 2. **`FUN_801D7B50`** (field overlay, sub-area window rebuild) frees the actor
//!    list and re-populates it from the cells inside the current window. It does
//!    **no** bind lookup; its only extra gate is the anchor tile's `0x400`
//!    (`801d7ccc: andi v0,v0,0x400` -> `bne` skips the tile). So it spawns exactly
//!    the placed records the init sweep left behind - unbound, unscripted,
//!    unposed. Rim Elm's cave shell (record `168` at cell `(32, 93)`) is one.
//!
//! Either way the actor is a static-object actor (tick fn `0x8003BC08`) at a
//! world position derived from the tile `(col, row)` and the record's signed
//! `X/Y/Z` offsets, drawing its mesh from the scene's `scene_asset_table` TMD
//! pack through the actor's `+0x44` mesh chain.
//!
//! The bind's own record - a MAN partition-0 record - carries the actor's
//! **animation id**, and a multi-object mesh is drawn *posed* by that clip
//! rather than with its raw object-local vertices; the resolver lives in
//! `legaia_engine_scene::field_env` (it needs the MAN, which this module does
//! not see). See `docs/subsystems/field-locomotion.md` ("The object bind").
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
/// Object-record `+0x12` flag bit that every mesh-drawing record family
/// carries (placed landmarks `0x16`/`0x17`, decorations `0x12`/`0x13`, plus
/// their `0x8xx` collider-correction variants); at spawn it selects the
/// placed actor's render type 5 (mesh-chain draw) vs 0. The `0x0011` family
/// (record 408 in every kingdom walk `.MAP`: same index, same `+0x10 = 4`,
/// same flags across Drake / Sebucus / Karisto - the riverbank/system cells)
/// lacks it, and stamping that family draws a wall/gate mesh down every river
/// (visually falsified against retail). **The decoration sweep does not test
/// this bit**, though: what keeps the riverbank family out of the draw is
/// that its cells carry only the walk bit [`CELL_WALK_VISIBLE`], never the
/// draw gate [`CELL_VISIBLE`] - see [`parse_walk_decorations`].
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
/// `0x2000` is **not** exclusive to overview mode, though: the ordinary-field
/// static-object / decoration pass (`FUN_801F7088`, PROT 0900) reuses the same
/// bit as its per-object draw gate over a normal scene `.MAP` (real `town01`:
/// 1946 cells set `0x1000`, 208 set `0x2000`, 126 both). So in a field scene
/// `0x2000` marks the decoration/object cells that pass draws, alongside the
/// `0x1000` walk-visible ground cells - two consumers of the same grid word.
///
/// **The free-roam *walk* view (game mode `0x03`) keeps this gate for its
/// pack-mesh decorations and uses [`CELL_WALK_VISIBLE`] (`0x1000`) only for
/// the heightfield ground.** It reads the per-scene walk `.MAP` (e.g. `map01`
/// walk = extraction entry `0083`, the block's first entry), whose `+0x10`
/// values are small (`0..39`) because the walk pool is 5 party + the 40-mesh
/// slot-1 landmark pack. The biggest landmarks - the enterable mountains -
/// sit on cells that carry `0x2000` **without** `0x1000` (the mesh is the
/// ground there), which is why a `0x1000`-gated decoration sweep loses
/// exactly them. The per-object mesh resolution is the same
/// [`pack_mesh_index`] (`+0x10`) **plus the pack prefix**: the retail path
/// (`FUN_80020f88` -> `actor+0x64 = record[+0x10] + DAT_8007b6f8`, prefix `= 5`;
/// `FUN_80024d78` then builds the actor's mesh chain from
/// `DAT_8007C018[actor+0x64]`) was pinned 14/14 against a live walk capture, so
/// the walk continent pool index is `FIELD_ACTOR_PACK_BIAS + pack_mesh_index`.
pub const CELL_VISIBLE: u16 = 0x2000;
/// Object-index-grid cell bit marking a **walk-view** (game mode `0x03`)
/// heightfield **ground** tile - the cell gate of the continent surface
/// ([`build_walk_heightfield`]; the bilinear floor sampler `FUN_80019278`
/// tests the same bit). The Drake walk `.MAP` grid sets this on ~16k cells
/// (vs ~300 with `0x2000`). It is **not** the pack-mesh draw gate:
/// decorations and the big landmarks are [`CELL_VISIBLE`]-gated, and a cell
/// can carry either bit without the other (a river cell is walk-visible
/// ground with no mesh; an enterable-mountain cell is a mesh with no ground).
/// See the `CELL_VISIBLE` docs for the shared `+0x10`-plus-prefix mesh
/// resolution.
pub const CELL_WALK_VISIBLE: u16 = 0x1000;
/// Object-index-grid cell bit `0x8000`: the field ground pass's **depth-sort**
/// flag. PROT 0900's ground emitter (`FUN_801F6D48`, and its depth-cued twin
/// `FUN_801F69EC`) links a cell carrying it into the ordering-table bucket its
/// own minimum projected `Z` picks; a cell without it goes into the fixed far
/// bucket `(0x3FF6 >> ot_shift) * 4` (`andi s0,s5,0x8000` / `beqz` at
/// `0x801F6F94`, the far-bucket pointer `s6` formed at `0x801F6D68`). The far
/// bucket is the last the GPU walks first, so such a cell draws **under
/// everything** - see [`WalkHeightfield::far_bucket`].
// REF: FUN_801F6D48
pub const CELL_GROUND_DEPTH_SORTED: u16 = 0x8000;
/// Object-index-grid cell bit `0x0400`, read on a placed object's
/// **footprint-anchor tile**: the marker that says *"this object is the
/// init sweep's - do not re-create it"*.
///
/// Retail spawns placed objects from **two** sweeps, and this bit is what keeps
/// them disjoint:
///
/// - `FUN_8003A55C` (SCUS, scene init, whole grid) resolves each placed record's
///   **object bind** at its anchor tile and skips the record when there is none.
///   The bound objects are the scripted / animated ones.
/// - `FUN_801D7B50` (field overlay, sub-area window rebuild) frees the actor list
///   and re-populates it from the cells in the current window. It does **no** bind
///   lookup; its only extra gate is this bit on the anchor tile
///   (`801d7ccc: andi v0,v0,0x400` / `bne -> next tile`), so it spawns exactly the
///   placed records the init sweep did *not*.
///
/// The two sets are complementary on the disc: across `town01` / `town0c` /
/// `koin3` / `map01` every placement whose anchor tile carries a kind-1 trigger
/// also carries this bit (37 / 58 / 6 of them), and every placement without a
/// trigger has the bit clear (9 / 5 / 0). **The union is every placed record** -
/// so a static whole-map renderer draws them all, the bound ones posed by their
/// bind's clip and the rest at their raw object-local vertices.
///
/// (Rim Elm's cave is the case that makes it visible: the cavern shell is
/// `town01` record `168` at cell `(32, 93)`, placed with no bind, so only the
/// window sweep ever creates it.)
// REF: FUN_801D7B50, FUN_8003A55C
pub const CELL_BIND_OWNED: u16 = 0x0400;
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
    /// `+0x1E` **cull radius**, in units of `0x40` (half a tile). Copied to the
    /// spawned actor's `+0x58` (`FUN_80020f88`), which the screen-space
    /// bounding-box cull `FUN_8001b73c` reads as the object's half-extent when
    /// it projects the four corners of a `(r+1) * 0x40` box and rejects the
    /// draw when every corner falls off screen. `FUN_8003a55c` additionally
    /// ORs the actor's render-flag word `+0x74` with `0x40000000` when the
    /// byte is nonzero. It is **not** an animation / state selector - see
    /// the object-bind record for that.
    pub cull_radius: u8,
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
            cull_radius: r[0x1E],
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
    /// Grid cell column (`0..128`) whose object-index-grid `u16` selected this
    /// record - the tile the mesh is positioned from.
    pub col: u8,
    /// Grid cell row (`0..128`); pairs with [`Self::col`].
    pub row: u8,
    /// Footprint-anchor tile column, `col + col_delta` (`FUN_8003A55C`'s
    /// bounds-gated `(col + record[+0x06], row + record[+0x07])`). This - NOT
    /// [`Self::col`] - is the tile the object's **bind lookup** keys on: retail
    /// calls `FUN_801D5630(1, anchor_col, anchor_row)` to find the object's
    /// kind-1 tile-trigger entry and, through it, the MAN partition-0 record
    /// that carries the object's interaction script + animation id.
    pub anchor_col: u8,
    /// Footprint-anchor tile row, `row + row_delta`; pairs with
    /// [`Self::anchor_col`].
    pub anchor_row: u8,
    /// The object-index-grid `u16` **at the anchor tile**
    /// ([`Self::anchor_col`] / [`Self::anchor_row`]). Only one bit of it is
    /// consumed: [`CELL_BIND_OWNED`] (`0x400`), which decides *which* of retail's
    /// two placed-object spawners owns this record - set, the init sweep
    /// (`FUN_8003A55C`) creates it from its bind; clear, the sub-area window
    /// sweep (`FUN_801D7B50`) creates it, unbound and unposed. Every placed
    /// record is spawned by exactly one of the two.
    pub anchor_cell: u16,
    /// World X (additive offset; see module docs).
    pub world_x: i32,
    /// World Z (subtractive offset; see module docs).
    pub world_z: i32,
    /// Record Y offset (added to the floor height by the consumer).
    pub y_off: i16,
    /// Low nibble of the collision/floor grid byte at the placement tile
    /// (`(col, row)` - the tile whose object-index-grid cell selected this
    /// record; `FUN_8003A55C` reads exactly that byte), or `None` when the
    /// grid region is absent. The floor-height tier the **placed-object**
    /// sweep resolves world Y through (`-lut[nibble] + y_off`).
    pub floor_nibble: Option<u8>,
    /// The floor nibbles of the cell's **2 x 2 corner-tile block**
    /// (`(col, row)`, `(col+1, row)`, `(col, row+1)`, `(col+1, row+1)` - the
    /// flat `+0x4000` grid bytes at `+0`, `+1`, `+0x80`, `+0x81`), carried by
    /// the **terrain / decoration** sweeps only (`None` on the placed-object
    /// sweep). Retail's per-cell terrain emitters (`FUN_801F69D8` in PROT 0901
    /// and its field sibling `FUN_801F7088` in PROT 0900, same body) resolve a
    /// visible cell's world Y from the *average* of these four corners'
    /// LUT heights, not from the single cell nibble - the consumer-side math
    /// lives in `legaia_engine_scene::field_env`. A cell on a floor-tier edge
    /// therefore lands mid-slope, matching the ramp its mesh bakes; sampling
    /// only `(col, row)` snaps every edge cell a full tier up or down, which
    /// breaks terraced towns (Vidna) into stair-stepped plateaus.
    pub floor_corner_nibbles: Option<[u8; 4]>,
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
    /// The record's `+0x1E` byte ([`ObjectRecord::cull_radius`]). The field
    /// decoration pass `FUN_801F7088` widens its per-cell window test by it
    /// (`0x801F7594..0x801F75D8`), so a large cell mesh is drawn from further
    /// outside the camera's visible-tile window than a small one.
    pub cull_radius: u8,
}

impl Placement {
    /// World Y of the placement's anchor in the retail (pre-flip) frame, from
    /// the scene's 16-entry floor-height LUT (`man[+0x02..+0x22]`; the runtime
    /// stores it negated, so every term is `-lut[nibble]`):
    ///
    /// - a **terrain / decoration** cell ([`Self::floor_corner_nibbles`] is
    ///   `Some`) takes the *average* of its 2 x 2 corner-tile heights, the
    ///   sum divided by 4 rounding toward zero (`if (sum < 0) sum += 3;`
    ///   then `sum >>= 2` - body `0x801F6FC8..0x801F7040` of the per-cell
    ///   sweep), so a cell on a floor-tier edge lands mid-slope where its
    ///   mesh's baked ramp expects it;
    /// - a **placed** object (`FUN_8003A55C`) reads the single placement-tile
    ///   nibble ([`Self::floor_nibble`]);
    /// - a placement with neither (no floor grid) sits at `0`.
    ///
    /// Then `+ y_off` in every case. One kernel for every consumer - the
    /// native draw resolver, the web viewer's walk-frame stamps and the
    /// parity tests - so the hosts cannot drift on tier-edge cells.
    pub fn world_y(&self, lut: &[i16; 16]) -> i32 {
        let h = |nibble: u8| -(lut[(nibble & 0x0F) as usize] as i32);
        let floor = match (self.floor_corner_nibbles, self.floor_nibble) {
            (Some(corners), _) => {
                let sum: i32 = corners.iter().map(|&n| h(n)).sum();
                if sum < 0 { (sum + 3) >> 2 } else { sum >> 2 }
            }
            (None, Some(nib)) => h(nib),
            (None, None) => return 0,
        };
        floor + self.y_off as i32
    }
}

/// The object-index-grid `u16` at tile `(col, row)`. `grid` is the field map
/// sliced from [`OBJECT_GRID_OFFSET`]. Off-grid / truncated reads give `0`.
fn grid_cell(grid: &[u8], col: u8, row: u8) -> u16 {
    let off = (row as usize * GRID_DIM + col as usize) * 2;
    grid.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .unwrap_or(0)
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
                anchor_col: acol as u8,
                anchor_row: arow as u8,
                anchor_cell: grid_cell(grid, acol as u8, arow as u8),
                world_x: world_x(col as u8, rec.x_off),
                world_z: world_z(row as u8, rec.z_off),
                y_off: rec.y_off,
                floor_nibble,
                // The placed-object sweep (`FUN_8003A55C`) resolves Y from the
                // single placement-tile nibble; the corner block is the
                // terrain sweeps' input.
                floor_corner_nibbles: None,
                pack_index: pack_mesh_index(obj_idx, &rec),
                flags: rec.flags,
                rot_x: rec.rot_x,
                rot_y: rec.rot_y,
                rot_z: rec.rot_z,
                collider_x: world_x(col as u8, rec.x_off) + collision_footprint_offset(&rec).0,
                collider_z: world_z(row as u8, rec.z_off) + collision_footprint_offset(&rec).1,
                cull_radius: rec.cull_radius,
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
/// `record[+0x10]` for **every** id (`true`) versus routing a **placed**
/// record through [`pack_mesh_index`], which returns `None` for the
/// protagonist / NPC ids `1..=3` (`false`). An unplaced record - a
/// decoration cell - takes `+0x10` either way: scenes do put scenery on
/// records `1..=3` (`vozz`'s ground fern is record 3, pack mesh 10, on
/// dozens of cells; `vell`, `rugi`, `retona`, `deene`, `opdeene` and
/// `rikuroa` each dress fifteen or more cells through those records).
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
            // The 2x2 corner-tile block retail's terrain emitters average the
            // cell's Y from: the flat `+0x4000` grid bytes at `+0`, `+1`,
            // `+0x80`, `+0x81` (`FUN_801F69D8` / `FUN_801F7088`, both read the
            // raw neighbour bytes with no bounds clamp - the flat-offset read
            // is the retail behaviour at the grid edge too).
            let corner = |d: usize| field_map.get(0x4000 + row * GRID_DIM + col + d).copied();
            let floor_corner_nibbles = match (corner(0), corner(1), corner(0x80), corner(0x81)) {
                (Some(a), Some(b), Some(c), Some(d)) => {
                    Some([a & 0x0F, b & 0x0F, c & 0x0F, d & 0x0F])
                }
                _ => None,
            };
            let acol = (col as i32 + rec.col_delta as i32).clamp(0, GRID_DIM as i32 - 1) as u8;
            let arow = (row as i32 + rec.row_delta as i32).clamp(0, GRID_DIM as i32 - 1) as u8;
            out.push(Placement {
                obj_idx,
                col: col as u8,
                row: row as u8,
                anchor_col: acol,
                anchor_row: arow,
                anchor_cell: grid_cell(grid, acol, arow),
                world_x: world_x(col as u8, rec.x_off),
                world_z: world_z(row as u8, rec.z_off),
                y_off: rec.y_off,
                floor_nibble,
                floor_corner_nibbles,
                // A decoration cell - a record without the placed flag - is
                // drawn by the per-cell pass itself, which indexes the pack
                // with `record[+0x10]` whatever the record's index
                // (`FUN_801F7088`: `lhu v0,0x10(s0)` ... `lw v0,-0x3fe8(at)`
                // at `0x801F77E4..0x801F7804`, behind only the cell's
                // `0x2000` and the record's placed-flag test). The id
                // carve-out is the placed-object spawner's.
                pack_index: if walk_mesh || rec.flags & FLAG_PLACED == 0 {
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
                cull_radius: rec.cull_radius,
            });
        }
    }
    out
}

/// The walk view's **decoration layer**: every draw-gated ([`CELL_VISIBLE`],
/// `0x2000`) cell whose object record is **not** placed - the record's
/// `+0x10` pack mesh stamped at the cell. This is the per-cell pack-mesh
/// sweep of the slot-B render library resident on the world map (the `0x2000`
/// gate + placed-flag skip + `+0x10` mesh kernel - `FUN_801F7088` in PROT
/// 0900 for towns, the byte-shared kernel of the kingdom sibling PROT 0901 on
/// the overworld), layered over the heightfield ground: the crossed-quad
/// billboard trees (forest clusters reference one tree mesh from dozens of
/// cells each), the mountain groups, small props - **and the big enterable
/// mountains**, whose cells carry `0x2000` without [`CELL_WALK_VISIBLE`]
/// because the mountain mesh replaces the walkable ground there (Drake record
/// 412, mesh 23, at cell `(39, 80)`; Karisto's cull-radius-8..10 records
/// 444 / 445 / 447 / 462 / 463 / 473). Gating on `0x1000` instead drops
/// exactly those.
///
/// The placed (`FLAG_PLACED`) interactive set is [`parse_placements`]'s;
/// excluding it here keeps the two layers disjoint so a consumer drawing both
/// never double-stamps a landmark. The bulk ground cells (97% of the walk
/// grid) carry only `0x1000` and are the heightfield surface
/// ([`build_walk_heightfield`]), and so is the `0x0011` riverbank/system
/// family (record 408: nonzero `+0x10`, no `0x2000` cell anywhere) - neither
/// needs a flag test to stay out; see [`FLAG_MESH_DRAWN`]. A `0x2000` cell
/// whose record's `+0x10` is `0` stamps pack slot 0 like any other value
/// (one such cell on Drake and one on Sebucus, both `0x0813` prop records).
/// Mesh resolution is the walk rule (`record[+0x10]` directly, per
/// `FUN_80020f88`), the same one [`parse_walk_terrain_tiles`] uses; world Y
/// is the corner-block average ([`Placement::world_y`]).
pub fn parse_walk_decorations(field_map: &[u8]) -> Vec<Placement> {
    parse_terrain_tiles_gated(field_map, CELL_VISIBLE, true)
        .into_iter()
        .filter(|p| p.pack_index.is_some() && p.flags & FLAG_PLACED == 0)
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
/// ground - the port-side analogue of the retail terrain renderer, whose
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
    /// Per-vertex floor-elevation tier (`0..16`, the corner tile's `+0x4000`
    /// low nibble) the vertex's Y was resolved through. The ladder those
    /// tiers index is script-animated (field-VM op `0x4C` nibble 9), and
    /// retail's ground emitter re-reads it every frame, so a drawn ground
    /// re-resolves its Y from these against the live ladder.
    pub corner_tiers: Vec<u8>,
    /// Per-vertex: the cell links into the ground pass's fixed **far bucket**
    /// (its object-grid word lacks [`CELL_GROUND_DEPTH_SORTED`]), so retail
    /// draws it before - and therefore under - every other primitive in the
    /// frame, whatever its depth. Only the field ground pass (PROT 0900,
    /// [`GroundCellGate::WalkVisible`]) has the bit; the overworld emitter keys
    /// every cell on its own corners (`legaia_engine_field::overworld_draw_order`)
    /// and leaves this `false`. Empty on a heightfield built without it.
    pub far_bucket: Vec<bool>,
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
/// a single pass. A port-side per-entity bilinear sampler (what
/// `FUN_80019278` literally does) is not currently needed - entities walking
/// on the heightfield get implicit interpolation from the rasteriser.
pub fn build_walk_heightfield(field_map: &[u8], lut: &[i16; 16]) -> WalkHeightfield {
    build_ground_heightfield(field_map, lut, GroundCellGate::WalkVisible)
}

/// Which cells of the object grid a ground builder draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundCellGate {
    /// Cells carrying the `0x1000` visible bit - the ordinary field's ground
    /// pair in PROT 0900, and [`build_walk_heightfield`]'s gate.
    WalkVisible,
    /// The kingdom overworld's ground emitter `FUN_801F89B8` (PROT 0901),
    /// which tests no cell bit: the cell word is only masked `& 0x1FF`
    /// (`0x801F8BA0`) to reach the record whose `+0x14..+0x18` run textures
    /// the quad. Every cell draws - including a cell without the `0x1000`
    /// bit whose record carries a real terrain page (one or two per kingdom,
    /// holes under the field gate). A record with no terrain page (tpage
    /// `0`) would texture from VRAM page `(0, 0)`, the display area, which
    /// the port does not hold; those cells are skipped rather than given the
    /// grass fallback. On the three kingdoms that is `map01`'s zero-word
    /// border row plus a handful of decoration cells.
    TerrainRecord,
}

/// [`build_walk_heightfield`] with the cell gate chosen by the caller: the
/// kingdom overworld builds with [`GroundCellGate::TerrainRecord`].
///
/// REF: FUN_801F89B8
pub fn build_ground_heightfield(
    field_map: &[u8],
    lut: &[i16; 16],
    gate: GroundCellGate,
) -> WalkHeightfield {
    let mut hf = WalkHeightfield::default();
    let Some(obj_grid) = field_map.get(OBJECT_GRID_OFFSET..) else {
        return hf;
    };
    // Corner floor tier of tile (c, r), clamped to the grid edge so border
    // cells stay watertight.
    let corner_tier = |c: usize, r: usize| -> u8 {
        let cc = c.min(GRID_DIM - 1);
        let rr = r.min(GRID_DIM - 1);
        field_map
            .get(COLLISION_GRID_OFFSET + rr * GRID_DIM + cc)
            .map(|b| b & 0x0F)
            .unwrap_or(0)
    };
    // Corner height (pre-Y-flip) through the LUT.
    let corner_y = |c: usize, r: usize| -> f32 { -(lut[corner_tier(c, r) as usize] as f32) };
    for row in 0..GRID_DIM {
        for col in 0..GRID_DIM {
            let cell_off = (row * GRID_DIM + col) * 2;
            let Some(cell_bytes) = obj_grid.get(cell_off..cell_off + 2) else {
                continue;
            };
            let cell = u16::from_le_bytes([cell_bytes[0], cell_bytes[1]]);
            if gate == GroundCellGate::WalkVisible && cell & CELL_WALK_VISIBLE == 0 {
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
                _ if gate == GroundCellGate::TerrainRecord => continue,
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
            hf.corner_tiers.extend([
                corner_tier(col, row),
                corner_tier(col + 1, row),
                corner_tier(col, row + 1),
                corner_tier(col + 1, row + 1),
            ]);
            let far = gate == GroundCellGate::WalkVisible && cell & CELL_GROUND_DEPTH_SORTED == 0;
            hf.far_bucket.extend([far; 4]);
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
mod tests;
