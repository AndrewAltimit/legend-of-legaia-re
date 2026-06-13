//! Random-encounter randomization: reassign the monster ids in each scene's
//! formation table.
//!
//! The formations live in the per-scene MAN asset (type `0x03`, descriptor
//! index 2 of a scene bundle), inside an LZS stream. Each formation record is
//! `[3 reserved][u8 count 0..4][u8 ids...]` (see
//! [`legaia_asset::man_section`] and `docs/formats/encounter.md`). An edit is
//! therefore: locate the MAN inside the scene's PROT entry, decompress, rewrite
//! the id bytes (same length — count is preserved), recompress, and write the
//! stream back over the original (the LZS decoder stops at the descriptor's
//! decompressed size, so a shorter or equal re-pack is safe).
//!
//! **Sane pool.** Reassignment draws only from the ids the scene *already*
//! uses, so every swapped-in monster is one the scene loads — no missing model
//! / crash. `Shuffle` redistributes the existing ids (same monsters, new
//! formations — difficulty preserved); `Random` draws each id uniformly from
//! the scene's distinct-id set.
//!
//! **Bosses are protected.** A scene's formation array holds both random
//! encounters *and* scripted/boss fights (Tetsu, Cort, Songi, …) that the field
//! VM engages by explicit index. Only the random ones — the formations reached
//! by a region whose `rate_increment > 0` (a region that can actually trigger an
//! encounter) — are randomized; scripted formations are left exactly as
//! authored, so a randomized run never replaces a boss (see
//! `random_formation_mask` / [`SceneEncounters::is_random_formation`]).

use legaia_asset::man_section::{self, EncounterSection};
use legaia_asset::scene_asset_table;

use crate::drops::DropMode;
use crate::rng::SplitMix64;

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

/// Which formation indices a scene's **random-encounter** roll can produce.
///
/// The encounter section holds a single formation array, but only some of those
/// formations are random encounters. Each region record (a per-area AABB) names
/// a contiguous `[formation_range_base, +formation_range_count)` slice it rolls
/// into **and** a `rate_increment`: the per-step amount it adds to the encounter
/// counter while the player stands in the AABB. A region with
/// `rate_increment == 0` never advances the counter, so it never triggers a
/// random encounter — it can reference formations without ever rolling them
/// (the retail position-aware roll `FUN_801D9E1C`; mirrored in
/// `engine_core::region_encounter`).
///
/// So a formation is a random encounter iff some region with **`rate_increment >
/// 0`** reaches it. Formations reached only by rate-0 regions (or by no region)
/// are *scripted* fights the field VM engages by explicit index — boss battles
/// (Tetsu, Cort, Songi, …) and story encounters. Randomizing those would replace
/// a boss, so the randomizer must leave them alone.
///
/// Returns a `bool` per formation index. Verified against the corpus: town01
/// (Rim Elm) has rate-0 regions covering formations 2..=4 but its only rate>0
/// regions reach 0..=2, so the Tetsu fight at index 4 is correctly scripted;
/// cave01's rate>0 regions reach 0..=9, leaving the scripted ids 19/20 at
/// indices 10/11 untouched.
fn random_formation_mask(body: &[u8], sec: &EncounterSection) -> Vec<bool> {
    let mut mask = vec![false; sec.formation_count as usize];
    for r in man_section::region_records(body, sec).flatten() {
        // A zero-rate region never triggers an encounter, so the formations it
        // references are not (by themselves) random — only rate>0 regions do.
        if r.rate_increment == 0 {
            continue;
        }
        let base = r.formation_range_base as usize;
        let count = r.formation_range_count as usize;
        for i in base..base.saturating_add(count) {
            if let Some(slot) = mask.get_mut(i) {
                *slot = true;
            }
        }
    }
    mask
}

/// A scene's encounter data, located inside one PROT scene-bundle entry and
/// decompressed so its formation ids can be rewritten.
pub struct SceneEncounters {
    /// PROT entry index this scene bundle lives in.
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within (the original compressed
    /// length; the data after it belongs to the next asset).
    pub compressed_budget: usize,
    /// Decompressed MAN buffer (mutated in place by [`Self::randomize`]).
    pub decoded: Vec<u8>,
    /// Absolute offset of the formation array within `decoded`.
    formation_array_off: usize,
    formation_stride: usize,
    formation_count: usize,
    /// Per-formation flag: `true` when the formation is reachable by some region
    /// (a **random** encounter), `false` when it's a scripted/boss fight the
    /// field VM engages by index. Only random formations are randomized — see
    /// [`random_formation_mask`].
    random_mask: Vec<bool>,
}

impl SceneEncounters {
    /// Try to locate a scene bundle's encounter data in a PROT entry's bytes.
    /// Returns `None` when the entry isn't a scene asset table, carries no MAN,
    /// or the MAN doesn't decode / parse — i.e. "nothing to randomize here".
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        let table = scene_asset_table::detect(entry)?;
        let man = table
            .used()
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()?;
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        // Plain scene bundles place the descriptor table at offset 0, so a
        // descriptor offset is entry-relative.
        let man_offset = man.data_offset as usize;
        let body = entry.get(man_offset..)?;
        let (decoded, _consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        let compressed_budget = crate::man_compressed_budget(&table, man_offset, entry.len());
        let manfile = man_section::parse(&decoded).ok()?;
        let sec_body = manfile.encounter_section_body(&decoded)?;
        let sec: EncounterSection = man_section::parse_encounter_section(sec_body).ok()?;
        let formation_array_off = manfile.encounter_section().body_offset() + sec.formation_range.0;
        // Bounds sanity: the whole formation array must sit inside `decoded`.
        let arr_end =
            formation_array_off + sec.formation_count as usize * sec.formation_stride as usize;
        if arr_end > decoded.len() {
            return None;
        }
        let random_mask = random_formation_mask(sec_body, &sec);
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget,
            decoded,
            formation_array_off,
            formation_stride: sec.formation_stride as usize,
            formation_count: sec.formation_count as usize,
            random_mask,
        })
    }

    /// Whether formation `i` is a **random** encounter (reachable by a region),
    /// as opposed to a scripted/boss fight the field VM engages by index. Only
    /// random formations are touched by [`Self::randomize`] /
    /// [`Self::randomize_with_extra`], so a boss is never replaced. Out-of-range
    /// `i` (and any formation no region references) is `false`.
    pub fn is_random_formation(&self, i: usize) -> bool {
        self.random_mask.get(i).copied().unwrap_or(false)
    }

    /// Count of formations that are random encounters (the population the
    /// randomizer actually touches).
    pub fn random_formation_count(&self) -> usize {
        self.random_mask.iter().filter(|&&b| b).count()
    }

    /// Monster count of formation `i` (`0..4`), clamped defensively.
    fn count(&self, i: usize) -> usize {
        let rec = self.formation_array_off + i * self.formation_stride;
        (self.decoded[rec + 3] as usize).min(4)
    }

    /// `(absolute offset, length)` of formation `i`'s id bytes within `decoded`.
    fn id_span(&self, i: usize) -> (usize, usize) {
        let rec = self.formation_array_off + i * self.formation_stride;
        (rec + 4, self.count(i))
    }

    /// Number of formation rows this scene declares. Each row index is also the
    /// `formation_id` the engine registers it under (the MAN row index is the
    /// encounter table's formation index, per `docs/formats/encounter.md`), so a
    /// caller can patch row `i` here and force the same row at runtime.
    pub fn formation_count(&self) -> usize {
        self.formation_count
    }

    /// The monster ids of formation row `i` (its `count` ids, `0..4`), read live
    /// from [`Self::decoded`]. Empty when `i` is out of range. Useful for a
    /// surgical inspection of one row without re-parsing the MAN.
    pub fn formation_ids(&self, i: usize) -> Vec<u8> {
        if i >= self.formation_count {
            return Vec::new();
        }
        let (off, len) = self.id_span(i);
        self.decoded[off..off + len].to_vec()
    }

    /// Absolute offset within [`Self::decoded`] of formation row `i`'s monster-id
    /// slot `slot`, so a caller can rewrite a single id in place (the counterpart
    /// to [`Self::randomize`]'s whole-array rewrite). `None` when `i` or `slot`
    /// is out of range.
    pub fn formation_id_offset(&self, i: usize, slot: usize) -> Option<usize> {
        if i >= self.formation_count {
            return None;
        }
        let (off, len) = self.id_span(i);
        (slot < len).then_some(off + slot)
    }

    /// The distinct monster ids this scene uses across its **random** formations
    /// — the safe pool to draw from (every id is already scene-loaded, and
    /// scripted/boss ids are excluded so a `Random` roll never drops a boss into
    /// an ordinary encounter). Scripted formations are skipped (see
    /// [`Self::is_random_formation`]).
    pub fn monster_pool(&self) -> Vec<u8> {
        let mut pool = Vec::new();
        for i in 0..self.formation_count {
            if !self.is_random_formation(i) {
                continue;
            }
            let (off, len) = self.id_span(i);
            for &id in &self.decoded[off..off + len] {
                if !pool.contains(&id) {
                    pool.push(id);
                }
            }
        }
        pool.sort_unstable();
        pool
    }

    /// Total monster-id slots across all formations (the count of bytes a
    /// shuffle permutes / a random pass rewrites).
    pub fn id_slot_count(&self) -> usize {
        (0..self.formation_count).map(|i| self.count(i)).sum()
    }

    /// Count formation id slots whose current id is in `set` (live from
    /// [`Self::decoded`]). Used to report how many unused enemies a run actually
    /// placed.
    pub fn count_ids_in(&self, set: &[u8]) -> usize {
        let mut n = 0;
        for i in 0..self.formation_count {
            let (off, len) = self.id_span(i);
            for &id in &self.decoded[off..off + len] {
                if set.contains(&id) {
                    n += 1;
                }
            }
        }
        n
    }

    /// Rewrite the formation ids in place from `seed`. Returns the number of id
    /// bytes that actually changed. The per-scene RNG is derived from
    /// `(seed, entry_idx)` so the result is independent of iteration order and
    /// reproducible.
    pub fn randomize(&mut self, seed: u64, mode: DropMode) -> usize {
        self.randomize_with_extra(seed, mode, &[])
    }

    /// Like [`Self::randomize`], but for [`DropMode::Random`] the candidate pool
    /// is the scene's own monster ids **plus** `extra` (deduped). This is how the
    /// `--unused-enemies` toggle re-introduces monsters no formation references:
    /// the battle loader streams a monster's archive slot on demand by id, so an
    /// id outside the scene's own set still loads and renders. `extra` has no
    /// effect under [`DropMode::Shuffle`] — a multiset-preserving permutation
    /// can't introduce a new id, by construction — so passing it there is a
    /// no-op (a `Shuffle` run never spawns an unused enemy; document that at the
    /// CLI). The base RNG sequence is unchanged when `extra` is empty, so the
    /// existing (no-unused) results stay byte-identical.
    pub fn randomize_with_extra(&mut self, seed: u64, mode: DropMode, extra: &[u8]) -> usize {
        let mut pool = self.monster_pool();
        if mode == DropMode::Random {
            for &id in extra {
                if !pool.contains(&id) {
                    pool.push(id);
                }
            }
        }
        if pool.is_empty() {
            return 0;
        }
        let mut rng =
            SplitMix64::new(seed ^ (self.entry_idx as u64).wrapping_mul(0x9E3779B97F4A7C15));

        // Every **random** formation id slot, in a stable order. Scripted/boss
        // formations (no region references them) are excluded, so a randomized
        // run never replaces a Tetsu / Cort / Songi fight — only the ordinary
        // random encounters are shuffled/redrawn.
        let slots = self.random_slot_offsets();
        let originals: Vec<u8> = slots.iter().map(|&o| self.decoded[o]).collect();

        let new_vals: Vec<u8> = match mode {
            DropMode::Shuffle => {
                let mut vals = originals.clone();
                rng.shuffle(&mut vals);
                vals
            }
            DropMode::Random => slots.iter().map(|_| pool[rng.below(pool.len())]).collect(),
        };

        let mut changed = 0;
        for (&off, &v) in slots.iter().zip(&new_vals) {
            if self.decoded[off] != v {
                self.decoded[off] = v;
                changed += 1;
            }
        }
        changed
    }

    /// Absolute `decoded` offsets of every **random**-formation monster-id slot,
    /// in a stable order (ascending formation index, then slot). Scripted/boss
    /// formations are excluded — this is exactly the population every randomize
    /// path rewrites. Shared by the per-scene [`Self::randomize_with_extra`] and
    /// the cross-scene scoped passes ([`Self::random_slot_ids`] /
    /// [`Self::fill_random_slots_from_pool`] / [`Self::apply_random_slots`]).
    fn random_slot_offsets(&self) -> Vec<usize> {
        let mut slots = Vec::new();
        for i in 0..self.formation_count {
            if !self.is_random_formation(i) {
                continue;
            }
            let (off, len) = self.id_span(i);
            for s in 0..len {
                slots.push(off + s);
            }
        }
        slots
    }

    /// How many random-encounter monster-id slots this scene exposes to a
    /// scoped (kingdom / world) pass — the count of ids it contributes to a
    /// cross-scene shuffle and consumes back from it.
    pub fn random_slot_count(&self) -> usize {
        self.random_slot_offsets().len()
    }

    /// The current monster ids in this scene's random-formation slots, in the
    /// stable [`Self::random_slot_offsets`] order. This is the scene's
    /// contribution to a cross-scene **shuffle** multiset (and the read side of
    /// [`Self::apply_random_slots`]).
    pub fn random_slot_ids(&self) -> Vec<u8> {
        self.random_slot_offsets()
            .iter()
            .map(|&o| self.decoded[o])
            .collect()
    }

    /// Overwrite this scene's random-formation slots with `ids` (taken in the
    /// stable [`Self::random_slot_offsets`] order), the write side of a
    /// cross-scene shuffle. Only the first `min(ids.len(), slot_count)` slots
    /// are written, so a short slice is a partial-but-safe write rather than a
    /// panic. Returns the number of id bytes that actually changed.
    pub fn apply_random_slots(&mut self, ids: &[u8]) -> usize {
        let mut changed = 0;
        for (&off, &v) in self.random_slot_offsets().iter().zip(ids) {
            if self.decoded[off] != v {
                self.decoded[off] = v;
                changed += 1;
            }
        }
        changed
    }

    /// Fill this scene's random-formation slots by drawing each id uniformly
    /// from an **externally supplied** `pool` (the kingdom-wide or world-wide
    /// monster set), as opposed to [`Self::randomize`]'s scene-local pool. The
    /// per-scene RNG is derived from `(seed, entry_idx)` so the result is
    /// reproducible and independent of iteration order. Returns the number of id
    /// bytes that changed; a no-op (0) when `pool` is empty.
    pub fn fill_random_slots_from_pool(&mut self, seed: u64, pool: &[u8]) -> usize {
        if pool.is_empty() {
            return 0;
        }
        let mut rng =
            SplitMix64::new(seed ^ (self.entry_idx as u64).wrapping_mul(0x9E3779B97F4A7C15));
        let slots = self.random_slot_offsets();
        let new_vals: Vec<u8> = (0..slots.len())
            .map(|_| pool[rng.below(pool.len())])
            .collect();
        let mut changed = 0;
        for (&off, &v) in slots.iter().zip(&new_vals) {
            if self.decoded[off] != v {
                self.decoded[off] = v;
                changed += 1;
            }
        }
        changed
    }

    /// Recompress the (mutated) MAN. Returns the stream if it fits the original
    /// compressed footprint, or `None` if it would overflow (the rare case our
    /// re-packer is a byte or two looser than the retail packer).
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_preserves_id_multiset_and_counts() {
        // Build a minimal MAN-less SceneEncounters by hand to exercise the
        // id-rewrite logic directly (locate() is covered by the disc-gated test).
        // decoded layout: formation array at offset 0, stride 8, 3 formations.
        let mut decoded = vec![0u8; 24];
        // formation 0: count 2, ids [10, 20]
        decoded[3] = 2;
        decoded[4] = 10;
        decoded[5] = 20;
        // formation 1: count 1, id [30]
        decoded[8 + 3] = 1;
        decoded[8 + 4] = 30;
        // formation 2: count 0
        let mut se = SceneEncounters {
            entry_idx: 7,
            man_offset: 0,
            compressed_budget: 9999,
            decoded,
            formation_array_off: 0,
            formation_stride: 8,
            formation_count: 3,
            random_mask: vec![true; 3],
        };
        let before: Vec<u8> = (0..3)
            .flat_map(|i| {
                let (o, l) = se.id_span(i);
                se.decoded[o..o + l].to_vec()
            })
            .collect();
        se.randomize(0x1234, DropMode::Shuffle);
        // Counts unchanged.
        assert_eq!(se.count(0), 2);
        assert_eq!(se.count(1), 1);
        assert_eq!(se.count(2), 0);
        // Multiset of ids preserved.
        let mut after: Vec<u8> = (0..3)
            .flat_map(|i| {
                let (o, l) = se.id_span(i);
                se.decoded[o..o + l].to_vec()
            })
            .collect();
        let mut b = before.clone();
        b.sort_unstable();
        after.sort_unstable();
        assert_eq!(b, after, "shuffle keeps the same multiset of monster ids");
    }

    #[test]
    fn formation_accessors_read_and_locate_single_ids() {
        let mut decoded = vec![0u8; 24];
        // formation 0: count 2, ids [10, 20]
        decoded[3] = 2;
        decoded[4] = 10;
        decoded[5] = 20;
        // formation 1: count 1, id [30]
        decoded[8 + 3] = 1;
        decoded[8 + 4] = 30;
        // formation 2: count 0
        let se = SceneEncounters {
            entry_idx: 7,
            man_offset: 0,
            compressed_budget: 9999,
            decoded,
            formation_array_off: 0,
            formation_stride: 8,
            formation_count: 3,
            random_mask: vec![true; 3],
        };
        assert_eq!(se.formation_count(), 3);
        assert_eq!(se.formation_ids(0), vec![10, 20]);
        assert_eq!(se.formation_ids(1), vec![30]);
        assert!(se.formation_ids(2).is_empty(), "zero-count row has no ids");
        assert!(se.formation_ids(3).is_empty(), "out-of-range row is empty");
        // The slot offset points at exactly the id byte.
        let off = se.formation_id_offset(0, 1).expect("row 0 slot 1");
        assert_eq!(se.decoded[off], 20);
        assert_eq!(se.formation_id_offset(1, 0), Some(8 + 4));
        assert!(se.formation_id_offset(1, 1).is_none(), "row 1 has one slot");
        assert!(se.formation_id_offset(2, 0).is_none(), "row 2 is empty");
    }

    #[test]
    fn random_draws_only_from_scene_pool_and_is_deterministic() {
        let mut decoded = vec![0u8; 16];
        decoded[3] = 2;
        decoded[4] = 5;
        decoded[5] = 9;
        decoded[8 + 3] = 2;
        decoded[8 + 4] = 9;
        decoded[8 + 5] = 5;
        let make = || SceneEncounters {
            entry_idx: 3,
            man_offset: 0,
            compressed_budget: 9999,
            decoded: decoded.clone(),
            formation_array_off: 0,
            formation_stride: 8,
            formation_count: 2,
            random_mask: vec![true; 2],
        };
        let pool = make().monster_pool();
        assert_eq!(pool, vec![5, 9]);
        let mut a = make();
        a.randomize(42, DropMode::Random);
        let mut b = make();
        b.randomize(42, DropMode::Random);
        assert_eq!(a.decoded, b.decoded, "same seed reproduces the rewrite");
        for i in 0..2 {
            let (o, l) = a.id_span(i);
            for &id in &a.decoded[o..o + l] {
                assert!(pool.contains(&id), "id {id} not in scene pool");
            }
        }
    }

    #[test]
    fn scripted_formations_are_never_randomized() {
        // 3 formations; the middle one (index 1) is a scripted boss no region
        // references, so it must be left untouched while 0 and 2 are randomized.
        let mut decoded = vec![0u8; 24];
        decoded[3] = 1;
        decoded[4] = 10; // formation 0: id 10 (random)
        decoded[8 + 3] = 1;
        decoded[8 + 4] = 0x4F; // formation 1: id 0x4F "Tetsu" (scripted boss)
        decoded[16 + 3] = 1;
        decoded[16 + 4] = 20; // formation 2: id 20 (random)
        let make = || SceneEncounters {
            entry_idx: 1,
            man_offset: 0,
            compressed_budget: 9999,
            decoded: decoded.clone(),
            formation_array_off: 0,
            formation_stride: 8,
            formation_count: 3,
            random_mask: vec![true, false, true],
        };
        // The boss id is excluded from the candidate pool entirely.
        assert_eq!(
            make().monster_pool(),
            vec![10, 20],
            "boss id 0x4F not in pool"
        );
        assert_eq!(make().random_formation_count(), 2);

        for mode in [DropMode::Shuffle, DropMode::Random] {
            let mut se = make();
            se.randomize(7, mode);
            let (off, _) = se.id_span(1);
            assert_eq!(
                se.decoded[off], 0x4F,
                "scripted boss formation (index 1) must be untouched ({mode:?})"
            );
            // The boss id never leaked into a random formation either.
            for i in [0usize, 2] {
                let (o, l) = se.id_span(i);
                for &id in &se.decoded[o..o + l] {
                    assert_ne!(id, 0x4F, "boss id must not appear in a random formation");
                }
            }
        }
    }

    /// Hand-build a scene whose formation array is a list of `(ids, is_random)`
    /// rows at stride 8 (count byte at +3, ids from +4). Mirrors the layout the
    /// other unit tests assemble by hand, factored so the cross-scene tests can
    /// build several scenes cheaply.
    fn make_scene(entry_idx: usize, rows: &[(&[u8], bool)]) -> SceneEncounters {
        let stride = 8;
        let mut decoded = vec![0u8; rows.len() * stride];
        let mut mask = Vec::with_capacity(rows.len());
        for (i, (ids, is_random)) in rows.iter().enumerate() {
            let rec = i * stride;
            decoded[rec + 3] = ids.len() as u8;
            decoded[rec + 4..rec + 4 + ids.len()].copy_from_slice(ids);
            mask.push(*is_random);
        }
        SceneEncounters {
            entry_idx,
            man_offset: 0,
            compressed_budget: 9999,
            decoded,
            formation_array_off: 0,
            formation_stride: stride,
            formation_count: rows.len(),
            random_mask: mask,
        }
    }

    #[test]
    fn random_slot_ids_and_apply_roundtrip_skip_scripted() {
        // Rows: random [10,20], scripted [0x4F], random [30].
        let mut se = make_scene(2, &[(&[10, 20], true), (&[0x4F], false), (&[30], true)]);
        // Only the random rows' ids are exposed, in order.
        assert_eq!(se.random_slot_count(), 3);
        assert_eq!(se.random_slot_ids(), vec![10, 20, 30]);
        // Apply a permutation; the scripted boss row stays put.
        let changed = se.apply_random_slots(&[30, 10, 20]);
        assert_eq!(changed, 3);
        assert_eq!(se.random_slot_ids(), vec![30, 10, 20]);
        assert_eq!(se.formation_ids(1), vec![0x4F], "scripted row untouched");
        // A short slice is a partial, panic-free write.
        let mut se2 = make_scene(2, &[(&[1, 2, 3], true)]);
        se2.apply_random_slots(&[9]);
        assert_eq!(se2.random_slot_ids(), vec![9, 2, 3]);
    }

    #[test]
    fn fill_from_pool_only_uses_pool_and_skips_scripted() {
        let pool = [40u8, 41, 42, 43];
        let mut se = make_scene(5, &[(&[10, 20], true), (&[0x4F], false)]);
        let changed = se.fill_random_slots_from_pool(99, &pool);
        assert!(changed > 0);
        for &id in &se.random_slot_ids() {
            assert!(
                pool.contains(&id),
                "filled id {id} came from outside the pool"
            );
        }
        assert_eq!(se.formation_ids(1), vec![0x4F], "scripted row untouched");
        // Same seed reproduces the fill; empty pool is a no-op.
        let mut a = make_scene(5, &[(&[10, 20], true)]);
        let mut b = make_scene(5, &[(&[10, 20], true)]);
        a.fill_random_slots_from_pool(99, &pool);
        b.fill_random_slots_from_pool(99, &pool);
        assert_eq!(a.decoded, b.decoded);
        let mut c = make_scene(5, &[(&[10, 20], true)]);
        assert_eq!(c.fill_random_slots_from_pool(99, &[]), 0);
    }

    #[test]
    fn cross_scene_shuffle_preserves_group_multiset() {
        // Two scenes in one group; simulate the orchestration's gather -> shuffle
        // -> redistribute over their random slots and assert the group-wide
        // multiset is conserved while scripted ids never move.
        let mut scenes = vec![
            make_scene(0, &[(&[1, 2], true), (&[0x90], false)]),
            make_scene(1, &[(&[3], true), (&[4, 5], true)]),
        ];
        let before: Vec<u8> = {
            let mut v: Vec<u8> = scenes.iter().flat_map(|s| s.random_slot_ids()).collect();
            v.sort_unstable();
            v
        };
        let mut all_ids: Vec<u8> = scenes.iter().flat_map(|s| s.random_slot_ids()).collect();
        let mut rng = SplitMix64::new(0xABCD);
        rng.shuffle(&mut all_ids);
        let mut cursor = 0;
        for s in &mut scenes {
            let n = s.random_slot_count();
            s.apply_random_slots(&all_ids[cursor..cursor + n]);
            cursor += n;
        }
        let after: Vec<u8> = {
            let mut v: Vec<u8> = scenes.iter().flat_map(|s| s.random_slot_ids()).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(before, after, "cross-scene shuffle conserves the multiset");
        assert_eq!(scenes[0].formation_ids(1), vec![0x90], "scripted id stays");
    }
}
