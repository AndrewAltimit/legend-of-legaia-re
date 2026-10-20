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
//!   [`legaia_engine_vm::field::FieldFlagsHost::extra_flags`]) by ORing
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
//! `ghidra/scripts/funcs/800180ec.txt`, and the field-overlay copy in
//! `overlay_0897_locomotion_cluster.txt`.
//!
//! NB: `ghidra/scripts/funcs/801dba20.txt` was previously cited here and is
//! **not** a dump of `FUN_801DBA20` - its own header reads
//! `entry=801db7f4`, a different function. Do not use it as provenance for
//! this port. The tile quantisation above was re-confirmed against the
//! instruction stream at `0x801DBEEC..0x801DBEFC` (signed `lh`, `-0x40`,
//! then `sra` by 7).
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
    /// The descriptor's authored rotation, `+0x08` / `+0x0A` / `+0x0C` (PSX
    /// angle units) - what `FUN_8003A55C` copies into the actor's
    /// `+0x24` / `+0x26` / `+0x28`, the angles its draw composes. A script
    /// can turn the actor afterwards (op `0x38`, op `4C 48`), so this is the
    /// bind-time seed, not the live heading.
    pub rot: [u16; 3],
    /// The descriptor's `+0x02` height offset, the `y_off` the bind seats the
    /// actor's `+0x16` with on top of the floor term.
    pub y_off: i16,
    /// The floor-tier nibble of the object's **own** tile (`.MAP` `+0x4000`,
    /// `& 0xF`) - the ladder rung `FUN_8003A55C` reads for the seat
    /// (`0x8003A620..0x8003A648`).
    pub floor_nibble: u8,
}

impl MapObject {
    /// The Y `FUN_8003A55C` seats the object's actor at:
    /// `ladder[floor_nibble] + y_off` (`0x8003A62C..0x8003A64C`), with
    /// `ladder` the **runtime** (negated) floor ladder at `0x1F80035C` -
    /// `World::terrain.floor_height_lut`'s frame. Retail Y-down.
    // REF: FUN_8003A55C
    pub fn seat_y(&self, runtime_ladder: &[i16; 16]) -> i16 {
        runtime_ladder[usize::from(self.floor_nibble & 0x0F)].wrapping_add(self.y_off)
    }
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
                rot: [0x08, 0x0A, 0x0C].map(|o| u16::from_le_bytes([d[o], d[o + 1]])),
                y_off: i16::from_le_bytes([d[2], d[3]]),
                floor_nibble: map
                    .get(MAP_WALK_GRID_OFFSET + tz * 0x80 + tx)
                    .map_or(0, |b| b & 0x0F),
            });
        }
    }
    out
}

/// Rebuild the derived marker bits of the object-index grid - the writer of
/// the bits the tile dispatch and the floor sampler read.
///
/// PORT: FUN_80017bec (SCUS_942.54; the Ghidra dump carries no instruction
/// stream - ported from the static-recomp rendering of `func_80017BEC`,
/// 428 bytes / 18 blocks, and re-read against a capstone decode of the SCUS
/// bytes `0x80017BEC..0x80017D94`, which the three passes below match).
///
/// Three passes over the scene buffer at `_DAT_1F8003EC` (the `.MAP` image):
///
/// 1. **Descriptor flag decay**: for each of the 512 object descriptors
///    (`+0x0000`, 0x20 stride), when the `u16` countdown at `+0x16` is zero,
///    bit 0 of the flags word `+0x12` is cleared.
/// 2. **Cell mirror rebuild**: every object-index cell (`+0x8000`, u16 per
///    tile, `0x80 x 0x80`) gets bit `0x1000` set/cleared from its owning
///    descriptor's flags bit 0 and bit `0x2000` from flags bit 1 (descriptor =
///    `cell & 0x1FF`).
/// 3. **Trigger-presence stamp**: for trigger kinds 0..2 (header `s16` pairs
///    at `+0x10002`/`+0x10004` + 4k, body offset relative to `+0x10000`,
///    4-byte records `[tile_x][tile_z]..`), the cell at each record's tile
///    gets bit `0x200 << kind` ORed in - producing the `0x600` fast-gate mask
///    ([`MAP_OBJECT_TRIGGER_BITS`]) the walk-on dispatch tests and the `0x800`
///    elevation-override marker `FUN_80019278` routes on.
///
/// Pass 3 is set-only (never cleared).
///
/// Live: `SceneHost`'s field entry (`scene/host/scene_entry.rs`) runs it
/// over the scene's `.MAP` bytes before `World::load_field_object_cells`
/// decodes the cell words, which is where retail's `MAIN_INIT` calls it
/// (`jal 0x80017BEC` at `0x801D6BF8`, unconditional, after the bundle walk).
/// Pass 1 sees the disc's descriptor countdowns there, exactly as retail's
/// load-time call does; nothing in the engine decrements `+0x16` later, and
/// retail's other two callers are the fishing and dance overlays' own inits.
///
/// The pass is not a no-op over the disc. On every block-start `.MAP` it
/// changes cells in three scenes only - one cell of `retona` (`+0x3000`),
/// one of `juui1` (`+0x1000`) and two of `noaru` (the `0x800`
/// elevation-override stamp, which moves the floor sampler) - and the
/// `retona_field_card_boot` capture has the stamped value at `retona`'s tile
/// `(0x1D, 0x18)`, which the disc bytes lack. So the live cells are the
/// refreshed ones, not the disc's.
pub fn refresh_object_grid_marks(map: &mut [u8]) {
    let rd16 = |m: &[u8], o: usize| u16::from_le_bytes([m[o], m[o + 1]]);
    let wr16 = |m: &mut [u8], o: usize, v: u16| m[o..o + 2].copy_from_slice(&v.to_le_bytes());
    if map.len() < MAP_REGION_BLOCK_OFFSET + 0x12 {
        return;
    }
    // Pass 1: countdown-expired descriptors lose flags bit 0.
    for i in 0..0x200usize {
        let base = i * MAP_OBJECT_DESCRIPTOR_STRIDE;
        if rd16(map, base + 0x16) == 0 {
            let f = rd16(map, base + 0x12);
            wr16(map, base + 0x12, f & 0xFFFE);
        }
    }
    // Pass 2: mirror descriptor flag bits 0/1 into cell bits 0x1000/0x2000.
    // Retail walks columns outer, rows inner (cell = +0x8000 + col*2 + row*0x100).
    for col in 0..0x80usize {
        for row in 0..0x80usize {
            let o = MAP_OBJECT_INDEX_OFFSET + col * 2 + row * 0x100;
            let cell = rd16(map, o);
            let flags = rd16(
                map,
                usize::from(cell & 0x1FF) * MAP_OBJECT_DESCRIPTOR_STRIDE + 0x12,
            );
            let cell = if flags & 1 != 0 {
                cell | 0x1000
            } else {
                cell & !0x1000
            };
            // Retail re-reads the cell between the two updates; the value is
            // the one just stored, so a sequential update is equivalent.
            let cell = if flags & 2 != 0 {
                cell | 0x2000
            } else {
                cell & !0x2000
            };
            wr16(map, o, cell);
        }
    }
    // Pass 3: stamp trigger-presence bits 0x200 << kind for kinds 0..2.
    for kind in 0..3usize {
        let hdr = MAP_REGION_BLOCK_OFFSET + kind * 4;
        let body = i16::from_le_bytes([map[hdr + 2], map[hdr + 3]]);
        let count = i16::from_le_bytes([map[hdr + 4], map[hdr + 5]]);
        let Some(base) = MAP_REGION_BLOCK_OFFSET.checked_add_signed(body as isize) else {
            continue;
        };
        for j in 0..count.max(0) as usize {
            let rec = base + j * 4;
            let (Some(&tx), Some(&tz)) = (map.get(rec), map.get(rec + 1)) else {
                break;
            };
            let o = MAP_OBJECT_INDEX_OFFSET + usize::from(tx) * 2 + usize::from(tz) * 0x100;
            if o + 1 >= map.len() {
                continue; // malformed record; retail would write blind
            }
            let cell = rd16(map, o);
            wr16(map, o, cell | (0x200 << kind));
        }
    }
}

/// Object-cell bit that marks a tile as **owned by the scene-init bind sweep**
/// (`legaia_scene_formats::field_objects::CELL_BIND_OWNED`, stamped by `FUN_8003AEB0`
/// from the gate-0 bind triggers). The window sweep skips any placement whose
/// footprint-anchor tile carries it, which is what keeps the two sweeps
/// disjoint.
pub const CELL_BIND_OWNED: u16 = 0x400;

/// Offset of the walkability / floor-nibble grid inside the `.MAP` file - the
/// byte the window sweep reads to index the scene's elevation LUT.
pub const MAP_WALK_GRID_OFFSET: usize = 0x4000;

/// One actor the sub-area window sweep creates.
///
/// Field names are the actor fields retail seeds, so the struct doubles as the
/// spawn descriptor a host applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSpawn {
    /// Object descriptor index (`cell & 0x1FF`; retail writes it to the actor's
    /// `+0x60`).
    pub descriptor: u16,
    /// Object-map tile the placement sits on.
    pub tile: (u8, u8),
    /// The placement's **footprint-anchor tile** (`tile + (i8 desc[+6], i8
    /// desc[+7])`) - the tile whose [`CELL_BIND_OWNED`] bit the sweep tests,
    /// and the same anchor a `legaia_engine_scene::field_env::EnvDraw` carries, which is
    /// how a host matches a drawn placement to this actor.
    pub anchor: (u8, u8),
    /// The spawn vector retail hands `FUN_80024C88`: `x = tile_x * 0x80 +
    /// 0x40 + desc[+0]`, `y = elevation_lut[nibble] + desc[+2]`,
    /// `z = tile_z * 0x80 - (desc[+4] - 0x40)`.
    pub world: (i16, i16, i16),
    /// Rotation triple from the descriptor's `+0x8` / `+0xA` / `+0xC`, written
    /// to the actor's `+0x24` / `+0x26` / `+0x28`.
    pub rotation: (u16, u16, u16),
    /// Sprite-template selector written to the template's `+0x14`: `5` when the
    /// descriptor's flags carry bit `0x2`, else `0`. `FUN_80020DE0` copies it
    /// into the actor's draw kind `+0x56` - see [`Self::drawn`].
    pub template_kind: u16,
    /// Descriptor `+0x1E` is non-zero, so the actor takes `+0x74 |=
    /// 0x40000000`.
    pub flag_40000000: bool,
    /// Descriptor flags bit `0x800`, so the actor takes `+0x74 |= 0x10000000`.
    pub flag_10000000: bool,
    /// Descriptor flags bit `0x1000`, so the actor takes `+0x10 |= 4`.
    pub flag_state_4: bool,
}

impl WindowSpawn {
    /// Whether the actor ever reaches a draw.
    ///
    /// The per-actor render dispatcher `FUN_8001ADA4` switches on the draw kind
    /// `+0x56` through an 11-entry table indexed by `kind - 1`
    /// (`addiu v1,v0,-0x1` / `sltiu v0,v1,0xb` at `0x8001AE68..0x8001AE70`),
    /// so draw kind `0` wraps to an out-of-range index and falls to the
    /// no-draw exit `0x8001B704`. A window actor keeps the template's kind:
    /// its `+0x5C` anim id is `0`, and the anim tick `FUN_800204F8` returns at
    /// `blez` (`0x8002052C`) before the one store that would flip it to kind
    /// `1`. So `template_kind == 0` is an actor that exists and never draws;
    /// `5` is the single-transform mesh arm (`0x8001B1A8`).
    ///
    /// The other three descriptor bits do not change *whether* it draws:
    ///
    /// - `flag_state_4` (`+0x10 & 4`) is read in the kind-5 arm after the
    ///   on-screen probe `FUN_8001B73C` (`0x8001B1E0..0x8001B1FC`): a probe
    ///   miss with the bit set skips the draw. It culls an object that is off
    ///   screen, which the port's whole-scene draw cannot show a difference
    ///   for.
    /// - `flag_40000000` / `flag_10000000` make the colour word `+0x74`'s high
    ///   byte non-zero, which the prim dispatcher `FUN_80043390` reads as "a
    ///   blend argument is present" (`srl s7,a1,0x18` / `ori a2,a2,1` at
    ///   `0x800433C0..0x800433CC`): far colour from the word's low 24 bits,
    ///   `IR0` cleared (`mtc2 t8,IR0` at `0x800434D4`), the depth-cue handler
    ///   bank, and `ABR = (word >> 24) & 3 = 0`, semi-transparency bit 31
    ///   clear. With `IR0 = 0` the depth cue is the identity, so the packet
    ///   colour is the TMD's baked colour either way - the same retail
    ///   shading every placed object already draws with.
    pub fn drawn(&self) -> bool {
        self.template_kind != 0
    }
}

/// Plan the sub-area **window rebuild** sweep over a whole `.MAP` image.
///
/// The `.MAP`-shaped entry point of [`window_rebuild_spawns_resident`]: the
/// object-descriptor region, the walk grid and the object-index grid are read
/// out of one buffer at their file offsets (`+0x0000`, `+0x4000`, `+0x8000`).
pub fn window_rebuild_spawns(
    map: &[u8],
    window: (u8, u8, u8, u8),
    elevation_lut: [i16; 16],
) -> Vec<WindowSpawn> {
    let descriptors = map.get(..MAP_WALK_GRID_OFFSET).unwrap_or(map);
    let walk_grid = map
        .get(MAP_WALK_GRID_OFFSET..MAP_OBJECT_INDEX_OFFSET)
        .unwrap_or(&[]);
    let cells: Vec<u16> = map
        .get(MAP_OBJECT_INDEX_OFFSET..)
        .unwrap_or(&[])
        .as_chunks::<2>()
        .0
        .iter()
        .take(0x80 * 0x80)
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    window_rebuild_spawns_resident(descriptors, walk_grid, &cells, window, elevation_lut)
}

/// Plan the sub-area **window rebuild** sweep over the scene's resident field
/// state.
///
/// PORT: FUN_801D7B50.
///
/// The complement of the scene-init object sweep [`parse_map_objects`]
/// (`FUN_8003A55C`). Retail runs this one from the two camera re-centre
/// routines `FUN_80017DD4` / `FUN_80017EC8` (`jal 0x801D7B50` at
/// `0x80017E14` / `0x80017F08`), each of which first re-latches the region box
/// through `FUN_800180EC` at the re-centre tile. It frees its own static-object
/// actor list (the scene control block's `+0x24` head, `0x8007C36C`; the init
/// sweep's actors live on `+0x0C` and are untouched) and re-populates it from
/// the placements inside the box. Differences from the init sweep, all of them
/// load-bearing:
///
/// - **Bounded, not whole-grid.** The walk covers `window = [x0, z0, x1, z1)`,
///   the scratchpad region box `0x1F800384..0x1F800387` that
///   [`refresh_region_attributes`] latches - not the full `0x80 x 0x80` grid.
///   Both loops are half-open (`while t < limit`), so an empty or inverted box
///   spawns nothing.
/// - **No bind lookup.** It never resolves a trigger, so its actors carry no
///   script and no animation clip (`+0x5C == 0`).
/// - **The `0x400` gate instead.** Its only extra test is
///   [`CELL_BIND_OWNED`] on the *footprint-anchor* tile
///   (`0x801d7ccc`): set means the init sweep already owns this placement, so
///   the window sweep skips it. Across the disc corpus the bit and the bind are
///   complementary, so the union of the two sweeps is every placed record.
/// - **Elevation from the walk grid.** Y is not the descriptor's raw `+0x2`: the
///   sweep reads the walk-grid byte at `tile_x + tile_z * 0x80`, takes its low
///   nibble, and indexes the scene's 16-entry elevation LUT in the scratchpad
///   (`0x1F80035C`) before adding `+0x2`.
///
/// `descriptors` is the `.MAP` `+0x0000..0x4000` object-descriptor region,
/// `walk_grid` the live walkability grid (`+0x4000`), `cells` the live
/// object-index grid words (`+0x8000`, row-major `tz * 0x80 + tx`), `window` is
/// `(x0, z0, x1, z1)` in the scratchpad's byte order and `elevation_lut` the
/// scratchpad LUT. The returned list is in the sweep's own order (X outer,
/// Z inner).
///
/// Live: `legaia_engine_core::world::World::recentre_field_window` runs it on every
/// camera re-centre the engine models and keeps the result as the scene's
/// windowed static-object list
/// (`legaia_engine_core::world::StaticObjectWindow`); both play hosts consume that list
/// through `legaia_engine_world_views::scene_staging::placed_draw_live`.
// REF: FUN_8003A55C (the complementary init sweep), FUN_80024C88 (the spawn),
// FUN_80020DE0 (template +0x14 -> actor +0x56)
pub fn window_rebuild_spawns_resident(
    descriptors: &[u8],
    walk_grid: &[u8],
    cells: &[u16],
    window: (u8, u8, u8, u8),
    elevation_lut: [i16; 16],
) -> Vec<WindowSpawn> {
    let (x0, z0, x1, z1) = window;
    let mut out = Vec::new();
    let cell_at = |tx: usize, tz: usize| -> Option<u16> { cells.get(tx + tz * 0x80).copied() };
    let mut tx = i32::from(x0);
    while tx < i32::from(x1) {
        let mut tz = i32::from(z0);
        while tz < i32::from(z1) {
            let this = tz;
            tz += 1;
            let Some(cell) = cell_at(tx as usize, this as usize) else {
                continue;
            };
            let descriptor = cell & 0x1FF;
            let base = usize::from(descriptor) * MAP_OBJECT_DESCRIPTOR_STRIDE;
            let Some(d) = descriptors.get(base..base + MAP_OBJECT_DESCRIPTOR_STRIDE) else {
                continue;
            };
            let flags = u16::from_le_bytes([d[0x12], d[0x13]]);
            if flags & MAP_OBJECT_SPAWN_BIT as u16 == 0 {
                continue;
            }
            // Footprint-anchor range guards, then the ownership gate.
            let (ax, az) = (tx + i32::from(d[6] as i8), this + i32::from(d[7] as i8));
            if !(0..0x80).contains(&ax) || !(0..0x80).contains(&az) {
                continue;
            }
            let Some(anchor_cell) = cell_at(ax as usize, az as usize) else {
                continue;
            };
            if anchor_cell & CELL_BIND_OWNED != 0 {
                continue; // the init sweep's placement
            }
            let wx = tx * 0x80 + i32::from(u16::from_le_bytes([d[0], d[1]])) + 0x40;
            let nibble = walk_grid
                .get(tx as usize + this as usize * 0x80)
                .map(|b| usize::from(b & 0xF))
                .unwrap_or(0);
            let wy = i32::from(elevation_lut[nibble])
                + i32::from(u16::from_le_bytes([d[2], d[3]]) as i16);
            let wz = this * 0x80 - (i32::from(u16::from_le_bytes([d[4], d[5]])) - 0x40);
            out.push(WindowSpawn {
                descriptor,
                tile: (tx as u8, this as u8),
                anchor: (ax as u8, az as u8),
                world: (wx as i16, wy as i16, wz as i16),
                rotation: (
                    u16::from_le_bytes([d[8], d[9]]),
                    u16::from_le_bytes([d[0x0A], d[0x0B]]),
                    u16::from_le_bytes([d[0x0C], d[0x0D]]),
                ),
                template_kind: if flags & 2 != 0 { 5 } else { 0 },
                flag_40000000: d[0x1E] != 0,
                flag_10000000: flags & 0x800 != 0,
                flag_state_4: flags & 0x1000 != 0,
            });
        }
        tx += 1;
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
/// (e.g. `legaia_engine_scene::scene::Scene::field_map_region_block`).
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

/// The region-type mask alone: `1 << (type & 0x1F)` ORed over every region
/// containing the tile, with no attribute latch.
///
/// This is the field overlay's `FUN_801DE234(tile_x, tile_z)`: it zeroes
/// `_DAT_8007B8F4`, then loops the resumable scan `FUN_80017FBC` (first call
/// `a0 = 0`, every later one `a0 = 1`) and ORs `1 << record[+4]` into the
/// word until the scan returns null (`0x801DE234..0x801DE2AC`). `FUN_801DBA20`
/// opens on the same loop inline (`0x801DBA64..0x801DBAA0`), which is why
/// [`zone_query`] calls this. Other slot-A images hold unrelated code at the
/// same VA, hence the stem.
///
/// PORT: overlay_field_0897_801de234
pub fn region_type_mask(table: Option<&RegionTable<'_>>, tile_x: i32, tile_z: i32) -> u32 {
    let mut mask = 0u32;
    if let Some(table) = table {
        let mut cursor = 0usize;
        while let Some(m) = table.scan(&mut cursor, tile_x, tile_z) {
            mask |= 1u32 << (m.kind() & 0x1F);
        }
    }
    mask
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
    let region_mask = region_type_mask(table, tile_x, tile_z);
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
mod tests;
