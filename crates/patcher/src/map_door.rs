//! `.MAP` **kind-0 intra-scene-teleport** ("map door") randomization.
//!
//! Alongside the MAN-script door classes ([`crate::door`] - the `0x3F`
//! scene-transition op - and [`crate::house_door`] - the cross-context player
//! `MOVE_TO` in named partition-0 records), a field scene carries a **third**
//! door class that lives entirely in map data: the per-scene `.MAP` file's
//! trigger block (`+0x10000`) holds a **kind-0 sub-table** of
//! `[tile_x][tile_z][dest_x][dest_z]` records. Crossing onto `(tile_x,
//! tile_z)` repositions the player outright - no object, no script, no record
//! name (retail arm `FUN_801D1EC4` at `0x801d21c0` → `FUN_801D5630`; the
//! engine port is `legaia_engine_core::field_regions::IntraSceneTeleport`).
//! **Most house EXITS are this class**: the way into Vahn's house in Rim Elm
//! is a script door, the way back out is a kind-0 record at the tile just
//! inside the doorway.
//!
//! `dest` is in **half-tiles**: the landing is `world = (dest_x*64 + 64,
//! (dest_z+1)*64)`, landing tile `(dest_x >> 1, dest_z >> 1)`.
//!
//! ## Softlock policy: reachability-verified per-scene shuffle
//!
//! Kind-0 records carry no ＩＮ/ＯＵＴ name to classify by, so the sanity
//! policy is derived from the `.MAP` itself. The scene's walkable surface
//! (the authored walk-visible floor from the object grid at `+0x8000`, minus
//! the collision-grid wall bits at `+0x4000` - the same two samplers the
//! engine's spawn resolver flood-fills) partitions into 4-connected
//! components: the town proper / each dungeon room is a component, house
//! interiors and ledges are pockets. Each record is an edge in the scene's
//! component graph: from the component(s) its trigger tile touches to the
//! component its destination lands in.
//!
//! The shuffle permutes the destinations among the scene's **located**
//! records (both endpoints attributable to a component), then **verifies**
//! the resulting component graph before accepting it, retrying the
//! (seed-deterministic) permutation a bounded number of times:
//!
//! - **retail reachability is preserved**: every `component → component`
//!   reachability the original teleport graph has (walking free within a
//!   component, teleports as directed edges) still holds - a dungeon's far
//!   room stays exactly as reachable as it was;
//! - **no new one-way trap**: no component becomes reachable from the main
//!   (largest) component without a way back, unless it was already like that
//!   on the retail disc (a few authored one-way drops exist).
//!
//! The permutation also preserves the scene's destination multiset outright,
//! so every landing stays a retail landing spot, every component that
//! received a teleport landing still receives one, and no landing can appear
//! on top of a trigger tile that retail didn't already pair it with. A scene
//! with no verifiable permutation keeps its vanilla doors.
//!
//! Records stay vanilla ("static") when an endpoint can't be attributed - a
//! destination whose sub-cells are closed in the base grid (story-gated
//! collision paints open some areas at runtime) or a trigger tile with no
//! open sub-cell - and when the record sits past the `.MAP`'s own `0x12000`
//! footprint (the `+0x12000` fallback window is the next PROT entry's
//! sectors - not safely writable).
//!
//! The edit is a same-size 2-byte in-place write per record (the `.MAP` is
//! raw, not LZS) - no relocation, and the trigger tiles never move.

use crate::rng::SplitMix64;

/// On-disc footprint of a per-scene `.MAP` field-map file (PROT entry sized
/// exactly this; the trigger block occupies the last `0x2000` bytes). Mirrors
/// `legaia_engine_core::scene::FIELD_MAP_LEN`.
pub const FIELD_MAP_LEN: usize = 0x12000;
/// Byte offset of the `.MAP` trigger block (retail `*(_DAT_1F8003EC) + 0x10000`).
pub const TRIGGER_BLOCK_OFFSET: usize = 0x10000;
/// Byte offset of the collision/floor grid (`0x80 x 0x80` bytes, high nibble =
/// 4 sub-cell wall bits).
const COLLISION_OFFSET: usize = 0x4000;
/// Byte offset of the per-tile object-index grid (`0x80 x 0x80` LE `u16`s).
const OBJECT_GRID_OFFSET: usize = 0x8000;
/// Tiles per grid side.
const GRID_STRIDE: usize = 0x80;
/// Sub-cells (64-unit wall-bit granularity) per side.
const SUB_STRIDE: usize = 0x100;

/// One planned destination rewrite: `(table_index, old_dest, new_dest)`.
pub type MapDoorEdit = (usize, (u8, u8), (u8, u8));

/// Which shuffle class a kind-0 record belongs to (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapDoorClass {
    /// Destination lands in the scene's largest walk component ("exit").
    MainBound,
    /// Destination lands in another attributed component ("entry" - a house
    /// interior, a dungeon room, a ledge).
    PocketBound,
    /// An endpoint can't be attributed to a walk component; kept vanilla.
    Static,
}

/// One kind-0 intra-scene-teleport record, located and classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapDoorSite {
    /// Index into the kind-0 record table.
    pub table_index: usize,
    /// Trigger tile `(tile_x, tile_z)` (exact-match against the player tile).
    pub tile: (u8, u8),
    /// Destination `(dest_x, dest_z)` in half-tiles.
    pub dest: (u8, u8),
    /// Shuffle class.
    pub class: MapDoorClass,
    /// Walk-component label of the original destination (`0` = unattributed).
    pub(crate) dest_comp: u16,
    /// Walk-component labels the trigger tile's four sub-cells touch
    /// (deduplicated, zero-padded).
    pub(crate) trig_comps: [u16; 4],
}

impl MapDoorSite {
    /// Absolute byte offset (within the `.MAP` entry) of this record's
    /// `dest_x` byte, given the record table's absolute offset.
    pub fn dest_off(&self, table_off: usize) -> usize {
        table_off + self.table_index * 4 + 2
    }

    /// The landing tile (`dest >> 1` - the destination is in half-tiles).
    pub fn dest_tile(&self) -> (u8, u8) {
        (self.dest.0 >> 1, self.dest.1 >> 1)
    }
}

/// A scene's `.MAP` with its classified kind-0 teleport sites.
pub struct SceneMapDoors {
    /// PROT entry index of the `.MAP` file.
    pub entry_idx: usize,
    /// Absolute byte offset of the kind-0 record table within the entry.
    pub table_off: usize,
    /// All kind-0 records in the primary trigger block, in table order.
    pub sites: Vec<MapDoorSite>,
    /// Kind-0 records whose table slot lies past the `.MAP`'s own footprint
    /// (in the `+0x12000` fallback window - the next PROT entry's sectors).
    /// Not represented in [`Self::sites`]; never touched.
    pub beyond_footprint: usize,
    /// Label of the largest walk component (`0` when nothing is open).
    pub(crate) main_label: u16,
}

impl SceneMapDoors {
    /// Locate + classify a `.MAP` entry's kind-0 teleport records. `entry`
    /// must be the file's full [`FIELD_MAP_LEN`] footprint (the trigger block
    /// lives past the TOC-indexed payload). Returns `None` when the entry
    /// isn't that size or carries no in-footprint kind-0 record.
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        if entry.len() != FIELD_MAP_LEN {
            return None;
        }
        // Kind-0 sub-table header, shared shape across kinds `k`: sub-table
        // offset `s16` at block `+4k+2`, count `s16` at `+4k+4`; `k = 0`,
        // 4-byte records. REF: FUN_801D5AE0.
        let block = &entry[TRIGGER_BLOCK_OFFSET..];
        let read_s16 =
            |off: usize| -> Option<i16> { Some(i16::from_le_bytes([block[off], block[off + 1]])) };
        let (off, count) = (read_s16(2)?, read_s16(4)?);
        if off < 0 || count <= 0 {
            return None;
        }
        let table_off = TRIGGER_BLOCK_OFFSET + off as usize;
        let count = count as usize;
        // Only records fully inside the `.MAP`'s own footprint are writable.
        let in_footprint = if table_off >= FIELD_MAP_LEN {
            0
        } else {
            ((FIELD_MAP_LEN - table_off) / 4).min(count)
        };
        let beyond_footprint = count - in_footprint;
        if in_footprint == 0 {
            return None;
        }

        let (labels, main_label) = walk_components(entry);

        // Destination component. The landing world point
        // `world = (dest_x*64 + 64, (dest_z+1)*64)` sits exactly on a
        // sub-cell **corner** (both coordinates are multiples of 64), so it
        // touches the four sub-cells around `(dest_x + 1, dest_z + 1)`; a
        // doorway-threshold landing routinely has wall bits on some of them.
        // The landing's component is the component of its open corners -
        // unambiguous when they all agree (`0` when none is open or when the
        // corners straddle two different components, i.e. a landing on top of
        // a wall seam - kept static).
        let dest_label = |dest: (u8, u8)| -> u16 {
            let (cx, cz) = (dest.0 as usize + 1, dest.1 as usize + 1);
            let mut label = 0u16;
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sz) = (cx.wrapping_sub(dx), cz.wrapping_sub(dz));
                if sx >= SUB_STRIDE || sz >= SUB_STRIDE {
                    continue;
                }
                let l = labels[sz * SUB_STRIDE + sx];
                if l != 0 {
                    if label != 0 && label != l {
                        return 0; // straddles a wall seam - ambiguous
                    }
                    label = l;
                }
            }
            label
        };

        // Components the trigger tile's four sub-cells touch: the components
        // a player can physically step onto the tile from.
        let trig_labels = |tile: (u8, u8)| -> [u16; 4] {
            let (tx, tz) = (tile.0 as usize, tile.1 as usize);
            let mut out = [0u16; 4];
            let mut n = 0;
            for (a, b) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sz) = (tx * 2 + a, tz * 2 + b);
                if sx >= SUB_STRIDE || sz >= SUB_STRIDE {
                    continue;
                }
                let l = labels[sz * SUB_STRIDE + sx];
                if l != 0 && !out[..n].contains(&l) {
                    out[n] = l;
                    n += 1;
                }
            }
            out
        };

        let sites: Vec<MapDoorSite> = (0..in_footprint)
            .map(|i| {
                let r = &entry[table_off + i * 4..table_off + i * 4 + 4];
                let (tile, dest) = ((r[0], r[1]), (r[2], r[3]));
                let dest_comp = dest_label(dest);
                let trig_comps = trig_labels(tile);
                let class = if dest_comp == 0 || trig_comps[0] == 0 {
                    MapDoorClass::Static
                } else if dest_comp == main_label {
                    MapDoorClass::MainBound
                } else {
                    MapDoorClass::PocketBound
                };
                MapDoorSite {
                    table_index: i,
                    tile,
                    dest,
                    class,
                    dest_comp,
                    trig_comps,
                }
            })
            .collect();

        Some(Self {
            entry_idx,
            table_off,
            sites,
            beyond_footprint,
            main_label,
        })
    }

    /// Plan a per-scene, reachability-verified shuffle of the destinations
    /// (see the module doc): permute the located sites' destinations, verify
    /// the resulting component graph, retry (deterministically) up to
    /// [`Self::MAX_ATTEMPTS`] times, and keep the scene vanilla when no
    /// permutation verifies. Static sites never move. Deterministic from
    /// `(seed, entry_idx)`. Returns `(table_index, old_dest, new_dest)` for
    /// every site whose destination actually changes.
    pub fn plan_shuffle(&self, seed: u64) -> Vec<MapDoorEdit> {
        let mut rng = SplitMix64::new(
            seed ^ 0x4D41_5044_4F4F_5253 // "MAPDOORS" salt: own stream, distinct
                ^ (self.entry_idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        );
        let idxs: Vec<usize> = (0..self.sites.len())
            .filter(|&i| self.sites[i].class != MapDoorClass::Static)
            .collect();
        // A destination's component travels with it through the permutation.
        let orig: Vec<((u8, u8), u16)> = idxs
            .iter()
            .map(|&i| (self.sites[i].dest, self.sites[i].dest_comp))
            .collect();
        let distinct: std::collections::HashSet<(u8, u8)> = orig.iter().map(|&(d, _)| d).collect();
        if distinct.len() < 2 {
            return Vec::new();
        }
        let reach_orig = self.reachability(&idxs, &orig);
        for _ in 0..Self::MAX_ATTEMPTS {
            let mut dests = orig.clone();
            rng.shuffle(&mut dests);
            // Anti-identity: a rotation guarantees movement (same rule as the
            // house-door shuffle).
            if dests == orig {
                dests.rotate_left(1);
            }
            if self.verified(&idxs, &orig, &dests, &reach_orig) {
                return idxs
                    .iter()
                    .zip(dests)
                    .filter(|(i, (d, _))| self.sites[**i].dest != *d)
                    .map(|(&i, (d, _))| (self.sites[i].table_index, self.sites[i].dest, d))
                    .collect();
            }
        }
        Vec::new()
    }

    /// Bounded deterministic retries for the verified shuffle.
    pub const MAX_ATTEMPTS: usize = 32;

    /// Does `self`'s (possibly patched) destination assignment satisfy the
    /// shuffle's acceptance conditions against `baseline` (the vanilla
    /// scene): every retail component-reachability pair preserved, no new
    /// one-way trap from the main component. The oracle the disc-gated
    /// round-trip test re-checks off a patched image.
    pub fn preserves_reachability_of(&self, baseline: &SceneMapDoors) -> bool {
        if self.sites.len() != baseline.sites.len() {
            return false;
        }
        let idxs: Vec<usize> = (0..baseline.sites.len())
            .filter(|&i| baseline.sites[i].class != MapDoorClass::Static)
            .collect();
        let orig: Vec<((u8, u8), u16)> = idxs
            .iter()
            .map(|&i| (baseline.sites[i].dest, baseline.sites[i].dest_comp))
            .collect();
        let cand: Vec<((u8, u8), u16)> = idxs
            .iter()
            .map(|&i| (self.sites[i].dest, self.sites[i].dest_comp))
            .collect();
        let reach_orig = baseline.reachability(&idxs, &orig);
        baseline.verified(&idxs, &orig, &cand, &reach_orig)
    }

    /// Component-level reachability closure of the teleport graph: walking is
    /// free within a component, each located site is a directed edge from
    /// every component its trigger tile touches to its destination's
    /// component. Returns the set of ordered `(from, to)` component pairs
    /// with `from != to` that are reachable.
    fn reachability(
        &self,
        idxs: &[usize],
        dests: &[((u8, u8), u16)],
    ) -> std::collections::BTreeSet<(u16, u16)> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut adj: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();
        let mut nodes: BTreeSet<u16> = BTreeSet::new();
        for (&i, &(_, dcomp)) in idxs.iter().zip(dests) {
            nodes.insert(dcomp);
            for &t in &self.sites[i].trig_comps {
                if t != 0 {
                    nodes.insert(t);
                    adj.entry(t).or_default().insert(dcomp);
                }
            }
        }
        let mut reach: BTreeSet<(u16, u16)> = BTreeSet::new();
        for &start in &nodes {
            let mut seen: BTreeSet<u16> = BTreeSet::new();
            let mut stack = vec![start];
            while let Some(c) = stack.pop() {
                if let Some(next) = adj.get(&c) {
                    for &n in next {
                        if seen.insert(n) {
                            stack.push(n);
                        }
                    }
                }
            }
            for c in seen {
                if c != start {
                    reach.insert((start, c));
                }
            }
        }
        reach
    }

    /// Accept a candidate permutation iff (1) every retail reachability pair
    /// survives, and (2) it creates no **new** one-way trap from the main
    /// component (reachable from main without a way back, unless retail
    /// already authored that one-way).
    fn verified(
        &self,
        idxs: &[usize],
        orig: &[((u8, u8), u16)],
        cand: &[((u8, u8), u16)],
        reach_orig: &std::collections::BTreeSet<(u16, u16)>,
    ) -> bool {
        let reach_new = self.reachability(idxs, cand);
        if !reach_orig.is_subset(&reach_new) {
            return false;
        }
        let main = self.main_label;
        if main == 0 {
            return true;
        }
        let comps: std::collections::BTreeSet<u16> = orig
            .iter()
            .chain(cand)
            .map(|&(_, c)| c)
            .chain(
                idxs.iter()
                    .flat_map(|&i| self.sites[i].trig_comps.into_iter().filter(|&t| t != 0)),
            )
            .collect();
        for &c in &comps {
            if c == main {
                continue;
            }
            let new_oneway = reach_new.contains(&(main, c)) && !reach_new.contains(&(c, main));
            let retail_oneway = reach_orig.contains(&(main, c)) && !reach_orig.contains(&(c, main));
            if new_oneway && !retail_oneway {
                return false;
            }
        }
        true
    }
}

/// Flood-fill the 4-connected components of the scene's open sub-cell lattice
/// (`0x100 x 0x100`): `labels[sz * 0x100 + sx]` is `0` for a closed sub-cell
/// or the 1-based component id; also returns the largest component's label
/// (`0` when nothing is open). Deterministic (row-major numbering; largest =
/// first maximum). Port of the engine's spawn-resolver flood fill
/// (`legaia_engine_core` `field_walk_components`).
fn walk_components(entry: &[u8]) -> (Vec<u16>, u16) {
    let mut labels = vec![0u16; SUB_STRIDE * SUB_STRIDE];
    let mut sizes: Vec<u32> = Vec::new();
    let mut queue: std::collections::VecDeque<(i32, i32)> = std::collections::VecDeque::new();
    for sz in 0..SUB_STRIDE as i32 {
        for sx in 0..SUB_STRIDE as i32 {
            let idx = sz as usize * SUB_STRIDE + sx as usize;
            if labels[idx] != 0 || !subcell_open(entry, sx, sz) {
                continue;
            }
            let label = (sizes.len() + 1) as u16;
            let mut count = 0u32;
            labels[idx] = label;
            queue.push_back((sx, sz));
            while let Some((cx, cz)) = queue.pop_front() {
                count += 1;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (cx + dx, cz + dz);
                    if !(0..SUB_STRIDE as i32).contains(&nx)
                        || !(0..SUB_STRIDE as i32).contains(&nz)
                    {
                        continue;
                    }
                    let nidx = nz as usize * SUB_STRIDE + nx as usize;
                    if labels[nidx] == 0 && subcell_open(entry, nx, nz) {
                        labels[nidx] = label;
                        queue.push_back((nx, nz));
                    }
                }
            }
            sizes.push(count);
        }
    }
    let main = sizes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0))) // first maximum wins
        .map(|(i, _)| (i + 1) as u16)
        .unwrap_or(0);
    (labels, main)
}

/// Is the 64-unit sub-cell `(sx, sz)` an open standing spot - on the authored
/// walk-visible floor (object-grid cell bit
/// [`legaia_asset::field_objects::CELL_WALK_VISIBLE`]) **and** clear of the
/// collision grid's wall bits? Ports the engine's two spawn-validity samplers
/// verbatim, including the wall read's retail index bias.
fn subcell_open(entry: &[u8], sx: i32, sz: i32) -> bool {
    if !(0..SUB_STRIDE as i32).contains(&sx) || !(0..SUB_STRIDE as i32).contains(&sz) {
        return false;
    }
    // Sub-cell centre in world units.
    let (x, z) = (sx * 64 + 32, sz * 64 + 32);
    // Walk-visible floor: plain `world >> 7` tile indexing into the object
    // grid.
    let (tx, tz) = ((x >> 7) as usize, (z >> 7) as usize);
    let cell_off = OBJECT_GRID_OFFSET + (tz * GRID_STRIDE + tx) * 2;
    let cell = u16::from_le_bytes([entry[cell_off], entry[cell_off + 1]]);
    if cell & legaia_asset::field_objects::CELL_WALK_VISIBLE == 0 {
        return false;
    }
    // Collision wall bit: the engine's biased read (out-of-grid reads open).
    let zc = (z >> 6) + 2;
    let xc = ((x + 0x3F) >> 6) - 1;
    let col = ((xc / 2) & 0x7F) as usize;
    let row = ((zc - (zc >> 31)) >> 1) as usize;
    let quad = ((zc & 1) << 1 | (xc & 1)) as u8;
    let grid = &entry[COLLISION_OFFSET..COLLISION_OFFSET + GRID_STRIDE * GRID_STRIDE];
    match grid.get(row * GRID_STRIDE + col) {
        Some(&byte) => (byte >> 4) & (1u8 << quad) == 0,
        None => true,
    }
}

/// Map-door randomization only supports `Shuffle` (the multiset-preserving
/// permutation keeps every destination one the scene's kind-0 system already
/// uses); `Random` would place the player off-map.
pub fn supported_mode(mode: crate::drops::DropMode) -> bool {
    matches!(mode, crate::drops::DropMode::Shuffle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthesize a minimal `.MAP` entry: a walk-visible main area plus
    /// pockets, walled apart in the collision grid, and a kind-0 table.
    ///
    /// Layout (tile coords): main = x 0..=15, z 0..=15; pocket A = x 20..=23,
    /// z 0..=3; pocket B = x 30..=33, z 0..=3. Everything else is off-floor.
    fn synth_map(records: &[[u8; 4]]) -> Vec<u8> {
        let mut e = vec![0u8; FIELD_MAP_LEN];
        let mut visible = |x0: usize, x1: usize, z0: usize, z1: usize| {
            for tz in z0..=z1 {
                for tx in x0..=x1 {
                    let off = OBJECT_GRID_OFFSET + (tz * GRID_STRIDE + tx) * 2;
                    e[off..off + 2].copy_from_slice(
                        &legaia_asset::field_objects::CELL_WALK_VISIBLE.to_le_bytes(),
                    );
                }
            }
        };
        visible(0, 15, 0, 15); // main
        visible(20, 23, 0, 3); // pocket A
        visible(30, 33, 0, 3); // pocket B
        // Kind-0 table at block offset 8.
        let table_off = TRIGGER_BLOCK_OFFSET + 8;
        e[TRIGGER_BLOCK_OFFSET + 2..TRIGGER_BLOCK_OFFSET + 4].copy_from_slice(&8i16.to_le_bytes());
        e[TRIGGER_BLOCK_OFFSET + 4..TRIGGER_BLOCK_OFFSET + 6]
            .copy_from_slice(&(records.len() as i16).to_le_bytes());
        for (i, r) in records.iter().enumerate() {
            e[table_off + i * 4..table_off + i * 4 + 4].copy_from_slice(r);
        }
        e
    }

    /// dest half-tiles for a landing at the centre of tile `(tx, tz)`:
    /// `world = (tx*128 + 64, tz*128 + 64)` ⇒ `dest = (2*tx, 2*tz)`.
    fn dest_for_tile(tx: u8, tz: u8) -> (u8, u8) {
        (tx * 2, tz * 2)
    }

    #[test]
    fn classification_finds_exits_entries_and_static() {
        let (ax, az) = dest_for_tile(21, 1); // pocket A landing
        let (bx, bz) = dest_for_tile(31, 1); // pocket B landing
        let (m1x, m1z) = dest_for_tile(5, 5); // main landings
        let (m2x, m2z) = dest_for_tile(10, 10);
        let (offx, offz) = dest_for_tile(60, 60); // off-floor (closed)
        let records = [
            // entries (trigger in main, dest in pockets A / B)
            [2, 2, ax, az],
            [3, 3, bx, bz],
            // exits (triggers inside pockets A / B, dest in main)
            [21, 2, m1x, m1z],
            [31, 2, m2x, m2z],
            // dest off-floor → static
            [4, 4, offx, offz],
        ];
        let e = synth_map(&records);
        let sd = SceneMapDoors::locate(&e, 7).expect("locate");
        assert_eq!(sd.sites.len(), 5);
        assert_eq!(sd.beyond_footprint, 0);
        let classes: Vec<MapDoorClass> = sd.sites.iter().map(|s| s.class).collect();
        assert_eq!(
            classes,
            vec![
                MapDoorClass::PocketBound,
                MapDoorClass::PocketBound,
                MapDoorClass::MainBound,
                MapDoorClass::MainBound,
                MapDoorClass::Static,
            ]
        );
    }

    #[test]
    fn verification_never_strands_an_unescapable_pocket() {
        // Pocket B gets an entry but NO exit record of its own; pocket A has
        // one. Retail reachability: main→A, main→B, A→main (+ A→B). The only
        // record leaving A must keep a destination that reaches main - i.e.
        // the main landing - or the whole scene stays vanilla.
        let (ax, az) = dest_for_tile(21, 1);
        let (bx, bz) = dest_for_tile(31, 1);
        let (mx, mz) = dest_for_tile(5, 5);
        let records = [
            [2, 2, ax, az],  // entry to A
            [3, 3, bx, bz],  // entry to B
            [21, 2, mx, mz], // exit from A (A's only escape)
        ];
        let e = synth_map(&records);
        let sd = SceneMapDoors::locate(&e, 7).expect("locate");
        assert_eq!(sd.sites[1].class, MapDoorClass::PocketBound);
        for seed in 0u64..64 {
            let plan = sd.plan_shuffle(seed);
            let exit_dest = plan
                .iter()
                .find(|&&(i, _, _)| i == 2)
                .map(|&(_, _, d)| d)
                .unwrap_or((mx, mz));
            assert_eq!(
                exit_dest,
                (mx, mz),
                "seed {seed}: A's only escape must keep reaching main"
            );
        }
    }

    #[test]
    fn shuffle_preserves_the_dest_multiset_and_is_deterministic() {
        let (ax, az) = dest_for_tile(21, 1);
        let (bx, bz) = dest_for_tile(31, 1);
        let (m1x, m1z) = dest_for_tile(5, 5);
        let (m2x, m2z) = dest_for_tile(10, 10);
        let records = [
            [2, 2, ax, az],
            [3, 3, bx, bz],
            [21, 2, m1x, m1z],
            [31, 2, m2x, m2z],
        ];
        let e = synth_map(&records);
        let sd = SceneMapDoors::locate(&e, 7).expect("locate");
        let mut moved_any = false;
        for seed in 0u64..32 {
            let plan = sd.plan_shuffle(seed);
            moved_any |= !plan.is_empty();
            // Multiset preserved: apply the plan and compare sorted dests.
            let mut dests: Vec<(u8, u8)> = sd.sites.iter().map(|s| s.dest).collect();
            for &(i, from, to) in &plan {
                assert_eq!(dests[i], from, "seed {seed}: plan's `from` is current");
                dests[i] = to;
            }
            dests.sort_unstable();
            let mut orig: Vec<(u8, u8)> = sd.sites.iter().map(|s| s.dest).collect();
            orig.sort_unstable();
            assert_eq!(dests, orig, "seed {seed}: destination multiset preserved");
            // A pocket's own exit never loops back into its pocket (that
            // would strand it - rejected by the reachability check).
            for &(i, _, to) in &plan {
                if i == 2 {
                    assert_ne!(to, (ax, az), "seed {seed}: exit-from-A must leave A");
                }
                if i == 3 {
                    assert_ne!(to, (bx, bz), "seed {seed}: exit-from-B must leave B");
                }
            }
            // Deterministic.
            assert_eq!(plan, sd.plan_shuffle(seed), "seed {seed}");
        }
        assert!(moved_any, "some seed must produce a verified shuffle");
    }

    #[test]
    fn wrong_footprint_or_empty_table_is_none() {
        assert!(SceneMapDoors::locate(&[0u8; 0x4000], 0).is_none());
        let e = synth_map(&[]);
        assert!(SceneMapDoors::locate(&e, 0).is_none());
    }

    #[test]
    fn records_past_the_footprint_are_counted_not_touched() {
        // Table parked 8 bytes before the footprint end: room for 2 records
        // in-footprint, 3 more spill into the fallback window.
        let mut e = synth_map(&[]);
        let off = (FIELD_MAP_LEN - TRIGGER_BLOCK_OFFSET - 8) as i16;
        e[TRIGGER_BLOCK_OFFSET + 2..TRIGGER_BLOCK_OFFSET + 4].copy_from_slice(&off.to_le_bytes());
        e[TRIGGER_BLOCK_OFFSET + 4..TRIGGER_BLOCK_OFFSET + 6].copy_from_slice(&5i16.to_le_bytes());
        let table_off = TRIGGER_BLOCK_OFFSET + off as usize;
        let (mx, mz) = dest_for_tile(5, 5);
        e[table_off..table_off + 4].copy_from_slice(&[2, 2, mx, mz]);
        e[table_off + 4..table_off + 8].copy_from_slice(&[3, 3, mx, mz]);
        let sd = SceneMapDoors::locate(&e, 0).expect("locate");
        assert_eq!(sd.sites.len(), 2);
        assert_eq!(sd.beyond_footprint, 3);
    }
}
