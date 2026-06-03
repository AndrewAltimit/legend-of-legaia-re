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

use legaia_asset::man_section::{self, EncounterSection};
use legaia_asset::scene_asset_table;

use crate::drops::DropMode;
use crate::rng::SplitMix64;

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

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
        let (decoded, consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
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
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget: consumed,
            decoded,
            formation_array_off,
            formation_stride: sec.formation_stride as usize,
            formation_count: sec.formation_count as usize,
        })
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

    /// The distinct monster ids this scene uses across all its formations — the
    /// safe pool to draw from (every id is already scene-loaded).
    pub fn monster_pool(&self) -> Vec<u8> {
        let mut pool = Vec::new();
        for i in 0..self.formation_count {
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

        // Collect every id slot's (offset, original value) in a stable order.
        let mut slots: Vec<usize> = Vec::new();
        for i in 0..self.formation_count {
            let (off, len) = self.id_span(i);
            for s in 0..len {
                slots.push(off + s);
            }
        }
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
}
