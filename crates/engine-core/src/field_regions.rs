//! Per-tile field **region** queries: the scene `.MAP` region-table scan,
//! the region-attribute refresh, and the MAN zone-record query.
//!
//! PORT: FUN_80017FBC, FUN_800180EC, FUN_801DBA20
//!
//! Three cooperating retail primitives (the first two `SCUS_942.54`-resident,
//! the third in the field overlay 0897):
//!
//! - [`RegionTable::scan`] - `FUN_80017FBC`, the shared **resumable
//!   point-in-AABB scan** over the per-scene region table at
//!   `*(_DAT_1F8003EC) + 0x10000` (the `.MAP` file's `+0x10000` block:
//!   record-body offset `s16` at `+0x1000E`, record count `s16` at
//!   `+0x10010`). Each record is `[x0, z0, x1, z1, type, pad×3]`; the scan
//!   normalises each axis to min/max, widens degenerate (min == max) boxes
//!   by 2, and matches half-open containment `min <= t < max`. The cursor
//!   (`gp+0x608`) makes it an iterator: callers resume to collect every
//!   matching region.
//! - [`refresh_region_attributes`] - `FUN_800180EC`, the per-tile refresh:
//!   rebuilds the region-type bitmask (`_DAT_8007B8F4` - the bank the
//!   field-VM op `0x42` mode 0 tests via
//!   [`legaia_engine_vm::field::FieldHost::extra_flags`]) by ORing
//!   `1 << type` for every region containing the tile, and latches the last
//!   type-0/1 region's raw box bytes into the scratchpad attribute block
//!   (`0x1F800384..87` + type at `0x1F80037C`). Falls back to the full-map
//!   default box when no type-0/1 region matches or the game mode is the
//!   world map (`_DAT_8007B83C` = `0xE`/`0xF`).
//! - [`zone_query`] - `FUN_801DBA20`, the **zone-record query** over the MAN
//!   section-3 table the boot walk installs at the control block
//!   `_DAT_801C6EA4 + 0x4` (count-prefixed 18-byte records). Rebuilds the
//!   same region-type bitmask, then returns the first record whose arm
//!   matches: kind `0` = record anchor point and player tile both inside the
//!   scratch attribute box, kind `1` = inclusive bbox containment, kind
//!   `>= 2` = region-type-bit test against the rebuilt mask. The field
//!   camera arrival handler (`FUN_801DBEC4`) feeds the hit to the camera
//!   config loader (`FUN_801DBC20`) - the 18-byte payload is a camera-region
//!   record.
//!
//! Tile units are 128 world units; the retail callers quantise
//! `tile = (world - 0x40) >> 7` (see the locomotion-cluster callsite of
//! `FUN_801DBA20`).
//!
//! Provenance: `ghidra/scripts/funcs/80017fbc.txt`,
//! `ghidra/scripts/funcs/800180ec.txt`, `ghidra/scripts/funcs/801dba20.txt`
//! (field-overlay copy re-confirmed in
//! `overlay_0897_locomotion_cluster.txt`).
//!
//! REF: FUN_801DBEC4, FUN_801DBC20

/// Byte offset of the region-table block inside the `.MAP` file.
pub const MAP_REGION_BLOCK_OFFSET: usize = 0x10000;

/// Byte offset of the **fallback** trigger-table block: the retail loader
/// reads `0x28` sectors contiguously from the `.MAP` LBA (`FUN_8001F7C0`),
/// so `+0x12000..` holds the first sectors of the *next* PROT entry (the
/// dev-build `DATA_FIELD<scene>` sibling). Its header has the same shape as
/// the `+0x10000` block; the per-tile trigger lookup (`FUN_801d5630` /
/// `FUN_801d5ae0`) scans the primary table first and falls back here.
pub const MAP_TRIGGER_FALLBACK_OFFSET: usize = 0x12000;

/// One kind-1 tile-trigger record: `[tile_x, tile_z, record_index, gate]`.
///
/// The `.MAP` `+0x10000` (and `+0x12000` fallback) block's kind-1 sub-table.
/// When the player enters tile `(x, z)`, the per-frame tile trigger
/// (`FUN_801D1EC4`) resolves the record and calls
/// `FUN_8003BDE0(x, z, record, gate)`: `gate == 1` spawns MAN partition-2
/// record `record_index` as a new field-VM context (door / cutscene records);
/// `gate == 0` records are object-bind entries consumed at scene init
/// (`FUN_8003A55C`) and never spawn.
// REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileTrigger {
    /// Trigger tile X (exact-match against the player tile).
    pub tile_x: u8,
    /// Trigger tile Z.
    pub tile_z: u8,
    /// Partition-2 record index to spawn (gate 1) or partition-0 object
    /// script to bind (gate 0).
    pub record: u8,
    /// Dispatch gate: `1` = spawn the P2 record on walk-on; `0` = init-time
    /// object bind.
    pub gate: u8,
}

/// Parse the kind-1 tile-trigger sub-table out of a `.MAP` trigger block
/// (either the `+0x10000` primary or the `+0x12000` fallback). Header shape
/// (shared across kinds `k`): sub-table offset `s16` at `+4k+2`, count `s16`
/// at `+4k+4`; kind-1 records are 4 bytes. Returns an empty vec on a short /
/// negative header.
// REF: FUN_801D5AE0
pub fn parse_tile_triggers(block: &[u8]) -> Vec<TileTrigger> {
    let read_s16 = |off: usize| -> Option<i16> {
        Some(i16::from_le_bytes([*block.get(off)?, *block.get(off + 1)?]))
    };
    let (Some(off), Some(count)) = (read_s16(6), read_s16(8)) else {
        return Vec::new();
    };
    if off < 0 || count <= 0 {
        return Vec::new();
    }
    let (off, count) = (off as usize, count as usize);
    (0..count)
        .map_while(|i| {
            let r = block.get(off + i * 4..off + i * 4 + 4)?;
            Some(TileTrigger {
                tile_x: r[0],
                tile_z: r[1],
                record: r[2],
                gate: r[3],
            })
        })
        .collect()
}

/// Exact-match lookup of a kind-1 trigger at `(tile_x, tile_z)`: primary
/// table first, then the fallback - first hit wins, mirroring
/// `FUN_801d5630`'s scan order.
// REF: FUN_801D5630
pub fn lookup_tile_trigger(
    primary: &[TileTrigger],
    fallback: &[TileTrigger],
    tile_x: u8,
    tile_z: u8,
) -> Option<TileTrigger> {
    primary
        .iter()
        .chain(fallback.iter())
        .copied()
        .find(|t| t.tile_x == tile_x && t.tile_z == tile_z)
}

/// One **kind-0** tile-trigger record: `[tile_x, tile_z, dest_x, dest_z]`.
///
/// The `.MAP` trigger block's kind-0 sub-table is the **intra-scene teleport**
/// table - the second door class, alongside the kind-1 gate-0 object binds
/// whose MAN script carries a player-channel move op. A kind-0 record has no
/// script at all: stepping onto `(tile_x, tile_z)` repositions the player
/// outright.
///
/// `dest_x` / `dest_z` are **half-tile** units. Retail's landing arithmetic
/// (`FUN_801D1EC4` body `0x801d1f88..0x801d1fb0`) is exact:
///
/// ```text
/// player.world_x = dest_x * 64 + 64          // sh -> player+0x14
/// player.world_z = (dest_z + 1) * 64         // sh -> player+0x18
/// landing tile   = (dest_x >> 1, dest_z >> 1)
/// ```
///
/// Retail then re-samples the floor height into `player+0x16`, resets the
/// camera, and looks a **kind-1** trigger up at the landing tile so the
/// arrival's own record (ambience switch / story beat) spawns.
// REF: FUN_801D1EC4, FUN_801D5630
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntraSceneTeleport {
    /// Trigger tile X (exact-match against the player tile).
    pub tile_x: u8,
    /// Trigger tile Z.
    pub tile_z: u8,
    /// Destination X in half-tiles.
    pub dest_x: u8,
    /// Destination Z in half-tiles.
    pub dest_z: u8,
}

impl IntraSceneTeleport {
    /// The landing position in world units, exactly as retail writes it into
    /// the player's `+0x14` / `+0x18`.
    // PORT: FUN_801D1EC4 (0x801d1f88..0x801d1fb0)
    pub fn dest_world(&self) -> (i16, i16) {
        (
            i16::from(self.dest_x) * 64 + 64,
            (i16::from(self.dest_z) + 1) * 64,
        )
    }

    /// The landing **tile**, the key retail re-queries the kind-1 table with
    /// so the arrival's own record spawns (`dest >> 1` - the destination is in
    /// half-tiles).
    pub fn dest_tile(&self) -> (u8, u8) {
        (self.dest_x >> 1, self.dest_z >> 1)
    }
}

/// Parse the kind-0 intra-scene-teleport sub-table out of a `.MAP` trigger
/// block. Same header shape as the kind-1 table (sub-table offset `s16` at
/// `+4k+2`, count `s16` at `+4k+4`, `k = 0`), 4-byte records.
// REF: FUN_801D5AE0
pub fn parse_intra_scene_teleports(block: &[u8]) -> Vec<IntraSceneTeleport> {
    let read_s16 = |off: usize| -> Option<i16> {
        Some(i16::from_le_bytes([*block.get(off)?, *block.get(off + 1)?]))
    };
    let (Some(off), Some(count)) = (read_s16(2), read_s16(4)) else {
        return Vec::new();
    };
    if off < 0 || count <= 0 {
        return Vec::new();
    }
    let (off, count) = (off as usize, count as usize);
    (0..count)
        .map_while(|i| {
            let r = block.get(off + i * 4..off + i * 4 + 4)?;
            Some(IntraSceneTeleport {
                tile_x: r[0],
                tile_z: r[1],
                dest_x: r[2],
                dest_z: r[3],
            })
        })
        .collect()
}

/// Exact-match lookup of a kind-0 intra-scene teleport at `(tile_x, tile_z)`:
/// primary table first, then the fallback - the same scan order
/// [`lookup_tile_trigger`] uses.
// REF: FUN_801D5630
pub fn lookup_intra_scene_teleport(
    primary: &[IntraSceneTeleport],
    fallback: &[IntraSceneTeleport],
    tile_x: u8,
    tile_z: u8,
) -> Option<IntraSceneTeleport> {
    primary
        .iter()
        .chain(fallback.iter())
        .copied()
        .find(|t| t.tile_x == tile_x && t.tile_z == tile_z)
}

/// Byte offset of the `.MAP` **per-tile object-index map**: `0x80 x 0x80`
/// `u16`s, each `& 0x1FF` an index into the object-descriptor table at the
/// file's start. Retail addresses it as `*(_DAT_1F8003EC) + 0x8000`.
pub const MAP_OBJECT_INDEX_OFFSET: usize = 0x8000;

/// Object-cell bits that mark a tile as **trigger-bearing**. Retail's per-frame
/// tile dispatch (`FUN_801D1EC4` at `0x801d2140`) reads the object-index word
/// at the crossed tile and only consults the trigger tables when
/// `cell & 0x600 != 0` - the fast gate in front of both the kind-1 record spawn
/// and the kind-0 teleport. Every kind-0 trigger tile on the disc carries it.
// REF: FUN_801D1EC4
pub const MAP_OBJECT_TRIGGER_BITS: u16 = 0x0600;

/// Stride of an object descriptor (the `.MAP` file's `+0x0000..+0x4000`
/// table, `_DAT_1F8003EC + index * 0x20`).
pub const MAP_OBJECT_DESCRIPTOR_STRIDE: usize = 0x20;

/// Descriptor `+0x12` bit that marks an object as **spawnable** - the gate
/// `FUN_8003A55C` tests (`psVar7[9] & 4`) before allocating an actor for it.
pub const MAP_OBJECT_SPAWN_BIT: i16 = 4;

/// One spawnable `.MAP` object, decoded the way retail's scene-init object
/// spawner does.
///
/// Retail (`FUN_8003A55C`) walks the `0x80 x 0x80` object-index map; for each
/// tile whose descriptor has [`MAP_OBJECT_SPAWN_BIT`] it allocates an actor at
/// the descriptor-derived world position and resolves the tile trigger at the
/// descriptor's **key tile** (`tile + (i8 dx, i8 dz)` from descriptor bytes
/// `+6`/`+7`). The trigger's `record` byte is the MAN record the actor's script
/// runs - so the trigger tile is a *lookup key*, not a place the player stands.
///
/// The player's contact box against the object (`FUN_801CFC40`) is centred on
/// [`Self::contact`] - the object's world position plus the same coarse
/// `dx * 0x80` / `dz * 0x80` offsets **plus** the fine `(i8) desc[+0xE] * 0x10`
/// / `desc[+0xF] * 0x10` offsets - with a half-extent of
/// [`MAP_OBJECT_CONTACT_HALF`] on each axis.
// PORT: FUN_8003A55C (the object walk + world-position + key-tile math)
// REF: FUN_801CFC40 (the contact-box centre + half-extent)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapObject {
    /// Index into the descriptor table (`raw & 0x1FF`; retail stores it on the
    /// actor at `+0x60`).
    pub descriptor: u16,
    /// Object-map tile the object sits on.
    pub tile: (u8, u8),
    /// The object's world position (`tile * 128 + 0x40 + desc.i16[+0]` on X,
    /// `tile * 128 + 0x40 - desc.i16[+4]` on Z).
    pub world: (i16, i16),
    /// Centre of the player-contact box (`FUN_801CFC40`).
    pub contact: (i16, i16),
    /// The trigger **key tile** the record lookup uses.
    pub key_tile: (u8, u8),
}

/// Half-extent of the static-object contact box on each axis
/// (`FUN_801CFC40`: `0x40` base plus the `0x10` static-object pad).
pub const MAP_OBJECT_CONTACT_HALF: i32 = 0x50;

/// Decode every spawnable object of a scene `.MAP` (the full file, from
/// offset 0) - PORT: FUN_8003A55C.
///
/// Returns one [`MapObject`] per (tile, spawnable object) pair, in row-major
/// tile order (the order retail's double loop spawns them in). Objects whose
/// descriptor lacks [`MAP_OBJECT_SPAWN_BIT`], or whose key tile falls outside
/// the `0..0x80` grid (retail's two range guards), are skipped.
///
/// Callers join `key_tile` against the scene's kind-1 trigger tables
/// ([`lookup_tile_trigger`]) to find the MAN record the object's script is;
/// an object whose key tile has no trigger spawns no script (retail's
/// `iVar14 == 0` bail).
pub fn parse_map_objects(map: &[u8]) -> Vec<MapObject> {
    let mut out = Vec::new();
    for tz in 0..0x80usize {
        for tx in 0..0x80usize {
            let o = MAP_OBJECT_INDEX_OFFSET + (tz * 0x80 + tx) * 2;
            let (Some(&lo), Some(&hi)) = (map.get(o), map.get(o + 1)) else {
                continue;
            };
            let descriptor = u16::from_le_bytes([lo, hi]) & 0x1FF;
            let base = usize::from(descriptor) * MAP_OBJECT_DESCRIPTOR_STRIDE;
            let Some(d) = map.get(base..base + MAP_OBJECT_DESCRIPTOR_STRIDE) else {
                continue;
            };
            let flags = i16::from_le_bytes([d[0x12], d[0x13]]);
            if flags & MAP_OBJECT_SPAWN_BIT == 0 {
                continue;
            }
            let (dx, dz) = (i32::from(d[6] as i8), i32::from(d[7] as i8));
            let (kx, kz) = (tx as i32 + dx, tz as i32 + dz);
            if !(0..0x80).contains(&kx) || !(0..0x80).contains(&kz) {
                continue; // retail's `< 0x80` / `-1 <` guards
            }
            let wx = tx as i32 * 0x80 + 0x40 + i32::from(i16::from_le_bytes([d[0], d[1]]));
            let wz = tz as i32 * 0x80 + 0x40 - i32::from(i16::from_le_bytes([d[4], d[5]]));
            // Contact-box centre: the coarse key-tile offset plus the fine
            // `* 0x10` offsets the touch test adds (`FUN_801CFC40`).
            let cx = wx + dx * 0x80 + i32::from(d[0x0E] as i8) * 0x10;
            let cz = wz + dz * 0x80 + i32::from(d[0x0F] as i8) * 0x10;
            let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            out.push(MapObject {
                descriptor,
                tile: (tx as u8, tz as u8),
                world: (clamp(wx), clamp(wz)),
                contact: (clamp(cx), clamp(cz)),
                key_tile: (kx as u8, kz as u8),
            });
        }
    }
    out
}

/// Region-record stride. Retail reads it from the resident byte
/// `DAT_8007B31B`; in the disc corpus the table body is 8-byte records
/// (`[x0, z0, x1, z1, type, 0, 0, 0]` - see the disc-gated structural test).
pub const REGION_RECORD_STRIDE: usize = 8;

/// Zone-record stride: the MAN section-3 table is count-prefixed 18-byte
/// records (`FUN_801DBA20` advances `pbVar7 += 0x12`).
pub const ZONE_RECORD_STRIDE: usize = 0x12;

/// One match from the region-table scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionMatch {
    /// Record index in the table (the cursor value when matched).
    pub index: usize,
    /// Raw record bytes `[x0, z0, x1, z1, type]` (the prefix every consumer
    /// reads; the remaining stride bytes are zero in the corpus).
    pub raw: [u8; 5],
}

impl RegionMatch {
    /// The region-type byte (`record[+4]`) - the bit index ORed into the
    /// region-type mask.
    pub fn kind(&self) -> u8 {
        self.raw[4]
    }
}

/// The per-scene `.MAP` region table (the `+0x10000` block).
///
/// Parse with [`RegionTable::parse`] over the `.MAP` bytes from `+0x10000`
/// (e.g. [`crate::scene::Scene::field_map_region_block`]).
#[derive(Debug, Clone, Copy)]
pub struct RegionTable<'a> {
    /// The `.MAP` `+0x10000..` block.
    block: &'a [u8],
    /// Record-body offset relative to the block start (`s16` at `+0xE`).
    body: usize,
    /// Record count (`s16` at `+0x10`).
    count: usize,
}

impl<'a> RegionTable<'a> {
    /// Parse the region-table header out of the `.MAP` `+0x10000..` block.
    ///
    /// Mirrors `FUN_80017FBC`'s address math: body =
    /// `block + s16_at(block+0xE)`, count = `s16_at(block+0x10)`. Returns
    /// `None` when the block is too short or declares a negative count.
    pub fn parse(block: &'a [u8]) -> Option<Self> {
        let body = i16::from_le_bytes([*block.get(0xE)?, *block.get(0xF)?]);
        let count = i16::from_le_bytes([*block.get(0x10)?, *block.get(0x11)?]);
        if body < 0 || count < 0 {
            return None;
        }
        Some(Self {
            block,
            body: body as usize,
            count: count as usize,
        })
    }

    /// Number of records the header declares.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Resumable point-in-AABB scan - PORT: FUN_80017FBC.
    ///
    /// `cursor` mirrors the retail resumable iterator state (`gp+0x608`):
    /// pass `&mut 0` to reset, then call again with the same cursor to find
    /// the *next* matching region. Containment per the disassembly:
    ///
    /// - per-axis normalise: `x_min = min(x0, x1)`, `x_max = max(x0, x1)`
    ///   (same for z with bytes 1/3);
    /// - degenerate widening: `x_min == x_max` → `x_max += 2`;
    ///   `z_min == z_max` → `z_min -= 2`;
    /// - half-open match: `x_min <= tx < x_max && z_min <= tz < z_max`
    ///   (byte bounds widened to `i32`, so negative tiles never match).
    pub fn scan(&self, cursor: &mut usize, tile_x: i32, tile_z: i32) -> Option<RegionMatch> {
        while *cursor < self.count {
            let index = *cursor;
            *cursor += 1;
            let off = self.body + index * REGION_RECORD_STRIDE;
            let rec = self.block.get(off..off + 5)?;
            let (x0, z0, x1, z1) = (rec[0], rec[1], rec[2], rec[3]);
            let (x_min, mut x_max) = (x0.min(x1) as i32, x0.max(x1) as i32);
            let (mut z_min, z_max) = (z0.min(z1) as i32, z0.max(z1) as i32);
            if x_min == x_max {
                x_max += 2;
            }
            if z_min == z_max {
                z_min -= 2;
            }
            if x_min <= tile_x && tile_x < x_max && z_min <= tile_z && tile_z < z_max {
                return Some(RegionMatch {
                    index,
                    raw: [rec[0], rec[1], rec[2], rec[3], rec[4]],
                });
            }
        }
        None
    }
}

/// The scratchpad region-attribute block (`0x1F800384..87` box bytes +
/// `0x1F80037C` type) that `FUN_800180EC` latches and `FUN_801DBA20`'s
/// kind-0 arm reads.
///
/// `box_bytes` keeps the retail store order: `[rec[0], rec[3], rec[2],
/// rec[1]]` - i.e. `[x0, z1, x1, z0]` of the raw (unnormalised) record, so
/// index 0/2 are the x bounds and index 1/3 the z bounds of the latched
/// region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionAttributes {
    pub box_bytes: [u8; 4],
    pub kind: u8,
}

impl RegionAttributes {
    /// The full-map default fill (`0x384/0x385 = 0`, `0x386/0x387 = 0x7F`,
    /// type `1`) `FUN_800180EC` writes when no type-0/1 region matches or
    /// the game mode is the world map.
    pub const DEFAULT_FILL: Self = Self {
        box_bytes: [0, 0, 0x7F, 0x7F],
        kind: 1,
    };
}

impl Default for RegionAttributes {
    fn default() -> Self {
        Self::DEFAULT_FILL
    }
}

/// Per-tile region-attribute refresh - PORT: FUN_800180EC.
///
/// Walks every region containing `(tile_x, tile_z)` via the resumable scan,
/// ORing `1 << type` into the returned mask (the `_DAT_8007B8F4` mirror the
/// field-VM op `0x42` mode 0 tests). For each type-0/1 match it latches the
/// record's raw box bytes (store order `[+0, +3, +2, +1]`) and type; after
/// the walk, when no type-0/1 region matched (`mask & 3 == 0`) **or**
/// `world_map_mode` is set (retail game mode `_DAT_8007B83C` = `0xE`/`0xF`),
/// the attributes fall back to [`RegionAttributes::DEFAULT_FILL`] (the mask
/// keeps whatever the walk accumulated).
pub fn refresh_region_attributes(
    table: Option<&RegionTable<'_>>,
    tile_x: i32,
    tile_z: i32,
    world_map_mode: bool,
) -> (u32, RegionAttributes) {
    let mut mask = 0u32;
    let mut attrs = RegionAttributes::DEFAULT_FILL;
    if let Some(table) = table {
        let mut cursor = 0usize;
        while let Some(m) = table.scan(&mut cursor, tile_x, tile_z) {
            mask |= 1u32 << (m.kind() & 0x1F);
            if m.kind() < 2 {
                attrs = RegionAttributes {
                    box_bytes: [m.raw[0], m.raw[3], m.raw[2], m.raw[1]],
                    kind: m.kind(),
                };
            }
        }
    }
    if mask & 3 == 0 || world_map_mode {
        attrs = RegionAttributes::DEFAULT_FILL;
    }
    (mask, attrs)
}

/// Result of a [`zone_query`] walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneQueryResult<'a> {
    /// The rebuilt region-type mask (identical recomputation to
    /// [`refresh_region_attributes`]'s - retail rewrites `_DAT_8007B8F4`
    /// from both paths).
    pub region_mask: u32,
    /// The first matching 18-byte zone record, if any.
    pub record: Option<&'a [u8]>,
}

/// Zone-record query - PORT: FUN_801DBA20.
///
/// `zone_table` is the MAN section-3 body (the pointer the boot walk
/// installs at the control block `_DAT_801C6EA4 + 0x4`): a count byte
/// followed by `count` 18-byte records. Returns `None` when the table is
/// empty (retail bails before touching the mask). Otherwise rebuilds the
/// region-type mask from the `.MAP` table scan, then walks the zone records
/// dispatching on `record[0]`:
///
/// - kind `0`: matches when the record's anchor point `(record[1],
///   record[2])` **and** the player tile both sit inside the latched
///   scratch attribute box (`attrs.box_bytes`, inclusive bounds);
/// - kind `1`: matches when the player tile is inside the record's own
///   inclusive bbox `record[1..=4]` (`[x_min, z_min, x_max, z_max]`);
/// - kind `>= 2`: matches when bit `kind` of the rebuilt mask is set.
///
/// The first match wins; its 18-byte record is the camera-region payload
/// the arrival handler hands to the camera-config loader.
pub fn zone_query<'a>(
    zone_table: &'a [u8],
    table: Option<&RegionTable<'_>>,
    attrs: &RegionAttributes,
    tile_x: i32,
    tile_z: i32,
) -> Option<ZoneQueryResult<'a>> {
    let count = *zone_table.first()? as usize;
    if count == 0 {
        return None;
    }
    let mut region_mask = 0u32;
    if let Some(table) = table {
        let mut cursor = 0usize;
        while let Some(m) = table.scan(&mut cursor, tile_x, tile_z) {
            region_mask |= 1u32 << (m.kind() & 0x1F);
        }
    }
    let [ax0, az0, ax1, az1] = attrs.box_bytes.map(|b| b as i32);
    for i in 0..count {
        let off = 1 + i * ZONE_RECORD_STRIDE;
        let Some(rec) = zone_table.get(off..off + ZONE_RECORD_STRIDE) else {
            break;
        };
        let matched = match rec[0] {
            0 => {
                let (px, pz) = (rec[1] as i32, rec[2] as i32);
                ax0 <= px
                    && az0 <= pz
                    && px <= ax1
                    && pz <= az1
                    && ax0 <= tile_x
                    && az0 <= tile_z
                    && tile_x <= ax1
                    && tile_z <= az1
            }
            1 => {
                rec[1] as i32 <= tile_x
                    && rec[2] as i32 <= tile_z
                    && tile_x <= rec[3] as i32
                    && tile_z <= rec[4] as i32
            }
            kind => region_mask & (1u32 << (kind & 0x1F)) != 0,
        };
        if matched {
            return Some(ZoneQueryResult {
                region_mask,
                record: Some(rec),
            });
        }
    }
    Some(ZoneQueryResult {
        region_mask,
        record: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `.MAP` `+0x10000` block: header up to `+0x12`, records at
    /// `body_off`.
    fn block(records: &[[u8; 5]]) -> Vec<u8> {
        let body_off = 0x20u16;
        let mut b = vec![0u8; 0x20 + records.len() * REGION_RECORD_STRIDE];
        b[0xE..0x10].copy_from_slice(&body_off.to_le_bytes());
        b[0x10..0x12].copy_from_slice(&(records.len() as u16).to_le_bytes());
        for (i, r) in records.iter().enumerate() {
            let off = body_off as usize + i * REGION_RECORD_STRIDE;
            b[off..off + 5].copy_from_slice(r);
        }
        b
    }

    #[test]
    fn scan_matches_half_open_box() {
        // Record box x [4, 10), z [2, 8) (raw order x0,z0,x1,z1).
        let b = block(&[[4, 2, 10, 8, 3]]);
        let t = RegionTable::parse(&b).unwrap();
        // `min <= t < max` per the disassembly's slt chain:
        // x_min <= tx && tx < x_max && z_min <= tz && tz < z_max.
        let mut c = 0;
        assert!(t.scan(&mut c, 4, 2).is_some());
        let mut c = 0;
        assert!(t.scan(&mut c, 9, 7).is_some());
        let mut c = 0;
        assert!(t.scan(&mut c, 10, 7).is_none(), "x max is exclusive");
        let mut c = 0;
        assert!(t.scan(&mut c, 9, 8).is_none(), "z max is exclusive");
        let mut c = 0;
        assert!(t.scan(&mut c, 3, 7).is_none(), "x min is inclusive bound");
    }

    #[test]
    fn scan_normalises_swapped_corners() {
        // Same box with swapped corner order: (10,8)-(4,2). The scan
        // normalises per-axis min/max (the lbu/sltu min-max pairs at
        // 0x80018030..0x80018088).
        let b = block(&[[10, 8, 4, 2, 3]]);
        let t = RegionTable::parse(&b).unwrap();
        let mut c = 0;
        assert!(t.scan(&mut c, 5, 5).is_some());
    }

    #[test]
    fn scan_widens_degenerate_boxes() {
        // x0 == x1 == 6: x_max += 2 → x in [6, 8). z0 == z1 == 4:
        // z_min -= 2 → z in [2, 4).
        let b = block(&[[6, 4, 6, 4, 0]]);
        let t = RegionTable::parse(&b).unwrap();
        let mut c = 0;
        assert!(t.scan(&mut c, 6, 3).is_some());
        let mut c = 0;
        assert!(t.scan(&mut c, 7, 2).is_some());
        let mut c = 0;
        assert!(t.scan(&mut c, 8, 3).is_none());
        let mut c = 0;
        assert!(t.scan(&mut c, 6, 4).is_none(), "z max stays exclusive");
    }

    #[test]
    fn scan_is_resumable() {
        // Two overlapping regions; the cursor resumes past the first match
        // exactly like the retail gp+0x608 iterator.
        let b = block(&[[0, 0, 0x20, 0x20, 2], [0, 0, 0x10, 0x10, 5]]);
        let t = RegionTable::parse(&b).unwrap();
        let mut c = 0;
        let first = t.scan(&mut c, 5, 5).unwrap();
        assert_eq!(first.index, 0);
        assert_eq!(first.kind(), 2);
        let second = t.scan(&mut c, 5, 5).unwrap();
        assert_eq!(second.index, 1);
        assert_eq!(second.kind(), 5);
        assert!(t.scan(&mut c, 5, 5).is_none());
    }

    #[test]
    fn refresh_builds_mask_and_latches_type01_box() {
        // A type-0 region and a type-4 region both containing the tile:
        // mask = (1<<0) | (1<<4); attrs latch the type-0 record's raw bytes
        // in store order [+0, +3, +2, +1] (sb chain at 0x8001816c..0x80018190).
        let b = block(&[[4, 2, 10, 8, 0], [0, 0, 0x20, 0x20, 4]]);
        let t = RegionTable::parse(&b).unwrap();
        let (mask, attrs) = refresh_region_attributes(Some(&t), 5, 5, false);
        assert_eq!(mask, (1 << 0) | (1 << 4));
        assert_eq!(attrs.box_bytes, [4, 8, 10, 2]);
        assert_eq!(attrs.kind, 0);
    }

    #[test]
    fn refresh_defaults_when_no_type01_match() {
        // Only a type-4 region: mask bit 4 set, but mask & 3 == 0 → the
        // attribute block falls back to the full-map default fill
        // (0,0,0x7F,0x7F, type 1 - the sb chain at 0x800181b8..0x800181d0).
        let b = block(&[[0, 0, 0x20, 0x20, 4]]);
        let t = RegionTable::parse(&b).unwrap();
        let (mask, attrs) = refresh_region_attributes(Some(&t), 5, 5, false);
        assert_eq!(mask, 1 << 4);
        assert_eq!(attrs, RegionAttributes::DEFAULT_FILL);
    }

    #[test]
    fn refresh_world_map_mode_forces_default() {
        // World-map game modes (0xE/0xF) force the default fill even when a
        // type-1 region matched (0x800181d8..0x80018210).
        let b = block(&[[4, 2, 10, 8, 1]]);
        let t = RegionTable::parse(&b).unwrap();
        let (mask, attrs) = refresh_region_attributes(Some(&t), 5, 5, true);
        assert_eq!(mask, 1 << 1);
        assert_eq!(attrs, RegionAttributes::DEFAULT_FILL);
    }

    fn zone_table(records: &[[u8; ZONE_RECORD_STRIDE]]) -> Vec<u8> {
        let mut t = vec![records.len() as u8];
        for r in records {
            t.extend_from_slice(r);
        }
        t
    }

    #[test]
    fn zone_query_empty_table_is_none() {
        assert!(zone_query(&[0u8], None, &RegionAttributes::DEFAULT_FILL, 5, 5).is_none());
        assert!(zone_query(&[], None, &RegionAttributes::DEFAULT_FILL, 5, 5).is_none());
    }

    #[test]
    fn zone_query_kind1_inclusive_bbox() {
        // kind-1 arm: rec[1] <= tx && rec[2] <= tz && tx <= rec[3] &&
        // tz <= rec[4] (all inclusive - the slt chain at 0x801dbb84..).
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 1;
        rec[1..5].copy_from_slice(&[4, 2, 10, 8]);
        let t = zone_table(&[rec]);
        let attrs = RegionAttributes::DEFAULT_FILL;
        let hit = zone_query(&t, None, &attrs, 10, 8).unwrap();
        assert!(hit.record.is_some(), "max corner is inclusive");
        let miss = zone_query(&t, None, &attrs, 11, 8).unwrap();
        assert!(miss.record.is_none());
    }

    #[test]
    fn zone_query_kind0_uses_scratch_box() {
        // kind-0 arm: the record anchor (rec[1], rec[2]) AND the player tile
        // must both sit inside the latched attribute box.
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 0;
        rec[1] = 6; // anchor x
        rec[2] = 6; // anchor z
        let t = zone_table(&[rec]);
        let inside = RegionAttributes {
            box_bytes: [4, 2, 10, 8],
            kind: 0,
        };
        assert!(
            zone_query(&t, None, &inside, 5, 5)
                .unwrap()
                .record
                .is_some()
        );
        // Tile outside the box → no match even though the anchor is inside.
        assert!(
            zone_query(&t, None, &inside, 11, 5)
                .unwrap()
                .record
                .is_none()
        );
        // Anchor outside the box → no match even though the tile is inside.
        let narrow = RegionAttributes {
            box_bytes: [4, 2, 5, 5],
            kind: 0,
        };
        assert!(
            zone_query(&t, None, &narrow, 5, 5)
                .unwrap()
                .record
                .is_none()
        );
    }

    #[test]
    fn zone_query_kind_ge2_tests_region_mask() {
        // kind >= 2 arm: match iff bit `kind` of the rebuilt .MAP region
        // mask is set (`_DAT_8007B8F4 & 1 << (kind & 0x1f)`).
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 4;
        let zt = zone_table(&[rec]);
        let attrs = RegionAttributes::DEFAULT_FILL;
        // .MAP table with a type-4 region containing the tile.
        let b = block(&[[0, 0, 0x20, 0x20, 4]]);
        let mt = RegionTable::parse(&b).unwrap();
        let hit = zone_query(&zt, Some(&mt), &attrs, 5, 5).unwrap();
        assert_eq!(hit.region_mask, 1 << 4);
        assert!(hit.record.is_some());
        // Tile outside the type-4 region → bit clear → no match.
        let miss = zone_query(&zt, Some(&mt), &attrs, 0x30, 5).unwrap();
        assert_eq!(miss.region_mask, 0);
        assert!(miss.record.is_none());
    }

    #[test]
    fn zone_query_first_match_wins() {
        let mut a = [0u8; ZONE_RECORD_STRIDE];
        a[0] = 1;
        a[1..5].copy_from_slice(&[0, 0, 0x7F, 0x7F]);
        a[5] = 0xAA; // payload marker
        let mut b = a;
        b[5] = 0xBB;
        let t = zone_table(&[a, b]);
        let hit = zone_query(&t, None, &RegionAttributes::DEFAULT_FILL, 5, 5).unwrap();
        assert_eq!(hit.record.unwrap()[5], 0xAA);
    }
}
